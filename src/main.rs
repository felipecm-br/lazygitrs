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
        #[command(flatten)]
        ai: AiArgs,
    },
    /// Generate a Conventional Commit message for the staged changes
    ///
    /// Prints the message (or commits it with --commit). Uses the same AI
    /// command, .sgcignore, diff compression and commitlint rules as sgc.
    Gc {
        #[command(subcommand)]
        action: Option<GcAction>,
        /// Stage all changes first (git add -A)
        #[arg(short, long)]
        all: bool,
        /// Commit with the generated message instead of printing it
        #[arg(short, long)]
        commit: bool,
        /// Generate N alternatives and pick one (prints all when not a terminal)
        #[arg(short, long, value_name = "N", default_value_t = 1)]
        generate: usize,
        /// Prefix the title with a gitmoji
        #[arg(short, long)]
        emoji: bool,
        /// Message language (ISO 639-1, e.g. pt, es)
        #[arg(short, long)]
        lang: Option<String>,
        /// Extra guidance for the AI
        #[arg(short = 'H', long)]
        hint: Option<String>,
        /// Print only the title line
        #[arg(short, long)]
        title_only: bool,
        /// Exit 0 silently when nothing is staged
        #[arg(short = 'n', long)]
        silent_empty: bool,
        /// AI command override (reads the prompt on stdin)
        #[arg(long)]
        command: Option<String>,
        #[command(flatten)]
        ai: AiArgs,
    },
}

/// Provider/model hints and debugging, shared by sgc and gc.
#[derive(clap::Args, Debug, Default)]
struct AiArgs {
    /// Provider hint for the AI command (env LAZYGITRS_AI_PROVIDER), e.g. claude
    #[arg(short = 'P', long, env = "GC_PROVIDER")]
    provider: Option<String>,
    /// Model hint for the AI command (env LAZYGITRS_AI_MODEL)
    #[arg(short, long, env = "GC_MODEL")]
    model: Option<String>,
    /// Print the prompt and the raw AI answers to stderr
    #[arg(short, long)]
    debug: bool,
}

#[derive(Subcommand, Debug)]
enum GcAction {
    /// Manage the prepare-commit-msg hook that fills empty commit messages
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
}

#[derive(Subcommand, Debug)]
enum HookAction {
    /// Install into this repository's hooks directory
    Install,
    /// Remove the lazygitrs-managed block (and a legacy zsh gc block)
    Uninstall,
    /// Show whether the hook is installed
    Status,
    /// Called by the hook: <msg-file> [source] [sha]
    #[command(hide = true)]
    Run {
        file: PathBuf,
        source: Option<String>,
        sha: Option<String>,
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
            ai,
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
                    provider: ai.provider,
                    model: ai.model,
                    debug: ai.debug,
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
        Some(Commands::Gc {
            action,
            all,
            commit,
            generate,
            emoji,
            lang,
            hint,
            title_only,
            silent_empty,
            command,
            ai,
        }) => {
            let repo = cli_repo_path(&cli.path, &cli.work_tree);
            let result = match action {
                Some(GcAction::Hook { action }) => match action {
                    HookAction::Install => commit_cli::hook_install(&repo),
                    HookAction::Uninstall => commit_cli::hook_uninstall(&repo),
                    HookAction::Status => commit_cli::hook_status(&repo),
                    HookAction::Run { file, source, .. } => {
                        let config = load_config_or_exit(cli.config.clone());
                        commit_cli::hook_run(&repo, &config, &file, source.as_deref())
                    }
                },
                None => {
                    let config = load_config_or_exit(cli.config.clone());
                    commit_cli::gc(
                        &repo,
                        &config,
                        commit_cli::GcArgs {
                            all,
                            commit,
                            count: generate,
                            title_only,
                            silent_empty,
                            command,
                            options: SmartCommitOptions {
                                emoji,
                                lang,
                                hint,
                                provider: ai.provider,
                                model: ai.model,
                                debug: ai.debug,
                                ..Default::default()
                            },
                        },
                    )
                }
            };
            if let Err(e) = result {
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

    #[test]
    fn test_cli_gc_flags_and_hook() {
        let cli = Cli::parse_from([
            "lazygitrs",
            "gc",
            "-a",
            "-g",
            "3",
            "-e",
            "-P",
            "claude",
            "-d",
        ]);
        let Some(Commands::Gc {
            all,
            generate,
            emoji,
            ai,
            action: None,
            ..
        }) = cli.command
        else {
            panic!("expected gc");
        };
        assert!(all && emoji && ai.debug);
        assert_eq!(generate, 3);
        assert_eq!(ai.provider.as_deref(), Some("claude"));

        let cli = Cli::parse_from([
            "lazygitrs",
            "gc",
            "hook",
            "run",
            ".git/COMMIT_EDITMSG",
            "message",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Gc {
                action: Some(GcAction::Hook {
                    action: HookAction::Run { .. }
                }),
                ..
            })
        ));
    }
}
