use lazygitrs::git::smart_commit::{Granularity, SmartCommitOptions};
use lazygitrs::{app, commit_cli, config, os, upgrade};

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// The ASCII logo for lazygitrs
const LOGO: &str = include_str!("../logo.txt");

#[derive(Parser)]
#[command(name = "lazygitrs", version, about = "A fast and ergonomic terminal UI for git", before_help = LOGO)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to the git repository
    #[arg(short, long)]
    path: Option<PathBuf>,

    /// Git work tree path
    #[arg(short = 'w', long = "work-tree")]
    work_tree: Option<PathBuf>,

    /// Git dir path
    #[arg(short = 'g', long = "git-dir")]
    git_dir: Option<PathBuf>,

    /// Enable debug logging
    #[arg(long)]
    debug: bool,

    /// Open directly in expanded diff view (patch mode)
    #[arg(short = 'd', long, alias = "patch")]
    diff: bool,

    /// Specific file to focus in diff view (implies --diff)
    #[arg(long)]
    file: Option<String>,

    /// Filter commits by path (file or directory), like lazygit -f
    #[arg(short = 'f', long = "filter", value_name = "PATH")]
    filter_path: Option<PathBuf>,

    /// Print the default configuration YAML to stdout and exit
    #[arg(long)]
    print_default_config: bool,

    /// Print keybindings markdown reference to stdout and exit
    #[arg(long)]
    print_keybindings_md: bool,

    /// Print the JSON Schema for config.yml to stdout and exit
    #[arg(long)]
    print_config_schema: bool,

    /// Configuration file or preset name to use (e.g. 'popup' or ~/.config/lazygitrs/config.yml)
    #[arg(short = 'c', long = "config")]
    config: Option<String>,

    /// Clear the active AI session for this repository and globally
    #[arg(long)]
    clear_session: bool,

    /// Launch directly in the commits panel
    #[arg(long)]
    commits: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Upgrade lazygitrs to the latest (or a specific) version
    Upgrade {
        /// Target version (e.g. `0.0.32`) or `latest`
        target: Option<String>,
    },
    /// Smart commit: split every change into atomic AI-planned commits
    ///
    /// Opens the review popup in a terminal; with --yes commits the whole
    /// plan, with --dry-run / --json only prints it.
    Sgc {
        /// Commit the whole plan without review
        #[arg(short, long)]
        yes: bool,
        /// Print the plan and exit without committing
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Print the plan as JSON (with --yes, also commit it)
        #[arg(long)]
        json: bool,
        /// Grouping: coarse, medium or fine [default: git.commit.smartCommitGranularity]
        #[arg(short, long, value_parser = parse_granularity)]
        granularity: Option<Granularity>,
        /// Ignore the cached plan and ask the AI again
        #[arg(short, long)]
        force: bool,
        /// Prefix titles with a gitmoji
        #[arg(short, long)]
        emoji: bool,
        /// Message language (ISO 639-1, e.g. pt, es)
        #[arg(short, long)]
        lang: Option<String>,
        /// Extra guidance for the AI (e.g. "scope everything under tui")
        #[arg(short = 'H', long)]
        hint: Option<String>,
        /// AI command override (reads the prompt on stdin)
        #[arg(long)]
        command: Option<String>,
        /// Prompt byte budget for diff context
        #[arg(long, default_value_t = 0, hide_default_value = true)]
        budget: usize,
    },
    /// Print an AI commit message for the staged changes (git.commit.generateCommand)
    Gc {
        /// Stage all changes first
        #[arg(short, long)]
        all: bool,
        /// Commit with the generated message instead of printing it
        #[arg(short, long)]
        commit: bool,
    },
}

fn parse_granularity(value: &str) -> Result<Granularity, String> {
    Granularity::parse(value).ok_or_else(|| format!("'{value}' is not coarse, medium or fine"))
}

/// Restore the terminal on panic so the user isn't left in raw mode + mouse
/// capture (which makes the shell unusable — every mouse move spews escape
/// sequences into the prompt).
fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Prefer /dev/tty when stdout is redirected (Helix `:insert-output`).
        let mut out =
            crate::os::tty::open_tui_output().unwrap_or_else(|_| Box::new(std::io::stdout()));
        if crate::os::tty::nested_tty_launch() {
            // Same contract as restore_terminal: Helix still owns alt-screen /
            // raw / mouse. Only undo our kitty push and hand the tty back.
            let _ = crossterm::execute!(out, crossterm::cursor::Show);
            let _ = crossterm::execute!(
                out,
                crossterm::event::PopKeyboardEnhancementFlags,
                crossterm::event::PushKeyboardEnhancementFlags(
                    crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | crossterm::event::KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                ),
            );
            crate::os::tty::restore_foreground_tty();
        } else {
            crate::os::tty::restore_foreground_tty();
            let _ = crossterm::execute!(
                out,
                crossterm::event::DisableMouseCapture,
                crossterm::event::DisableFocusChange,
                crossterm::cursor::Show,
                crossterm::terminal::LeaveAlternateScreen,
            );
            let _ = crossterm::terminal::disable_raw_mode();
        }
        prev(info);
    }));
}

fn main() {
    // Helix `:insert-output` sets stdin=/dev/null + stdout=pipe while keeping its
    // EventStream on /dev/tty. Detect that, claim the tty foreground so Helix
    // can't steal keys, and draw on a separate /dev/tty handle (no stdout dup2).
    os::tty::reclaim_controlling_tty();
    install_panic_hook();
    let cli = Cli::parse();

    let mut start_smart_commit: Option<SmartCommitOptions> = None;
    match cli.command {
        Some(Commands::Upgrade { target }) => {
            if let Err(e) = upgrade::upgrade(target.as_deref()) {
                eprintln!("Error: {e:#}");
                std::process::exit(1);
            }
            return;
        }
        Some(Commands::Sgc {
            yes,
            dry_run,
            json,
            granularity,
            force,
            emoji,
            lang,
            hint,
            command,
            budget,
        }) => {
            let repo = cli_repo_path(&cli.path, &cli.work_tree);
            let config = load_config_or_exit(cli.config.clone());
            let granularity = granularity.unwrap_or_else(|| {
                Granularity::parse(&config.user_config.git.commit.smart_commit_granularity)
                    .unwrap_or_default()
            });
            let args = commit_cli::SgcArgs {
                yes,
                dry_run,
                json,
                command,
                options: SmartCommitOptions {
                    granularity,
                    emoji,
                    lang,
                    force,
                    hint,
                    context_budget: budget,
                },
            };
            match commit_cli::sgc(&repo, &config, args) {
                Ok(commit_cli::SgcOutcome::Done) => return,
                Ok(commit_cli::SgcOutcome::Review(options)) => start_smart_commit = Some(options),
                Err(e) => {
                    eprintln!("sgc: {e:#}");
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Gc { all, commit }) => {
            let repo = cli_repo_path(&cli.path, &cli.work_tree);
            let config = load_config_or_exit(cli.config.clone());
            if let Err(e) = commit_cli::gc(&repo, &config, commit_cli::GcArgs { all, commit }) {
                eprintln!("gc: {e:#}");
                std::process::exit(1);
            }
            return;
        }
        None => {}
    }

    if cli.print_default_config {
        let config = config::user_config::UserConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        println!("{}", yaml);
        std::process::exit(0);
    }

    if cli.print_config_schema {
        let schema = config::schema::config_json_schema();
        println!(
            "{}",
            serde_json::to_string_pretty(&schema).unwrap_or_default()
        );
        std::process::exit(0);
    }

    if cli.print_keybindings_md {
        let config = config::user_config::UserConfig::default();
        print!(
            "{}",
            config::keybindings::generate_keybindings_md(&config.keybinding)
        );
        std::process::exit(0);
    }

    if cli.clear_session {
        // 1. Delete global fallback file
        if let Ok(home) = std::env::var("HOME") {
            let global_path = std::path::PathBuf::from(home).join(".lazygitrs_active_session.json");
            let _ = std::fs::remove_file(global_path);
        }

        // 2. Try to notify running instance for the current directory via API
        if let Some(port) = std::env::current_dir()
            .ok()
            .and_then(|cwd| lazygitrs::acp::read_published_port(&cwd))
        {
            let url = format!("http://127.0.0.1:{}/session-api", port);
            let _ = std::process::Command::new("curl")
                .args([
                    "-s",
                    "-X",
                    "POST",
                    &url,
                    "-H",
                    "content-type: application/json",
                    "--data",
                    r#"{"action":"unregister"}"#,
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }

        println!(
            "AI sessions cleared (global fallback removed, and local instance unregistered if running)."
        );
        std::process::exit(0);
    }

    // Set up logging if debug mode
    if cli.debug {
        tracing_subscriber::fmt()
            .with_env_filter("lazygitrs=debug")
            .with_writer(std::io::stderr)
            .init();
    }

    let repo_path = cli_repo_path(&cli.path, &cli.work_tree);

    let start_in_diff = cli.diff || cli.file.is_some();

    match app::App::new(
        repo_path,
        cli.debug,
        start_in_diff,
        cli.file,
        cli.config,
        cli.filter_path,
        cli.commits,
    ) {
        Ok(mut app) => {
            app.start_smart_commit = start_smart_commit;
            match app.run() {
                Ok(quit_reason) => {
                    std::process::exit(quit_reason.exit_code());
                }
                Err(e) => {
                    eprintln!("Error: {:#}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {:#}", e);
            std::process::exit(1);
        }
    }
}

fn cli_repo_path(path: &Option<PathBuf>, work_tree: &Option<PathBuf>) -> PathBuf {
    path.clone()
        .or_else(|| work_tree.clone())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn load_config_or_exit(config_override: Option<String>) -> config::AppConfig {
    config::AppConfig::load(false, config_override).unwrap_or_else(|e| {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_commits_flag() {
        let cli = Cli::parse_from(["lazygitrs", "--commits"]);
        assert!(cli.commits);

        let cli_default = Cli::parse_from(["lazygitrs"]);
        assert!(!cli_default.commits);
    }

    #[test]
    fn test_cli_sgc_flags() {
        let cli = Cli::parse_from(["lazygitrs", "sgc", "-y", "-g", "fine", "-e", "-l", "pt"]);
        let Some(Commands::Sgc {
            yes,
            granularity,
            emoji,
            lang,
            ..
        }) = cli.command
        else {
            panic!("expected sgc");
        };
        assert!(yes && emoji);
        assert_eq!(granularity, Some(Granularity::Fine));
        assert_eq!(lang.as_deref(), Some("pt"));
        assert!(Cli::try_parse_from(["lazygitrs", "sgc", "-g", "huge"]).is_err());
    }
}
