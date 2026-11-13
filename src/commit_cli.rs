//! Headless `lazygitrs sgc` / `lazygitrs gc`: the same planner and AI chain
//! the TUI uses, for scripts and shell aliases.

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::{Arc, atomic::AtomicBool};

use anyhow::{Result, bail};

use crate::config::AppConfig;
use crate::git::smart_commit as sc;

pub struct SgcArgs {
    pub yes: bool,
    pub dry_run: bool,
    pub json: bool,
    pub options: sc::SmartCommitOptions,
    pub command: Option<String>,
}

/// What `main` should do after `sgc` returns.
pub enum SgcOutcome {
    Done,
    /// Interactive review: open the TUI on the plan popup.
    Review(sc::SmartCommitOptions),
}

fn status(line: &str) {
    if std::io::stderr().is_terminal() {
        eprintln!("\x1b[2m› {line}\x1b[0m");
    }
}

pub fn sgc(repo: &Path, config: &AppConfig, args: SgcArgs) -> Result<SgcOutcome> {
    let interactive = !args.yes && !args.dry_run && !args.json;
    if interactive && std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return Ok(SgcOutcome::Review(args.options));
    }

    let commit_cfg = &config.user_config.git.commit;
    let command = args
        .command
        .clone()
        .unwrap_or_else(|| commit_cfg.smart_commit_command().to_string());
    let cancel = Arc::new(AtomicBool::new(false));
    let Some(plan) = sc::plan(repo, &command, &args.options, cancel, &status)? else {
        bail!("cancelled");
    };
    if plan.from_cache {
        status("Using cached plan (working tree unchanged; --force to re-plan)");
    }

    if args.json {
        let value = serde_json::json!({
            "fromCache": plan.from_cache,
            "warnings": plan.warnings,
            "commits": plan.groups,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        if !args.yes {
            return Ok(SgcOutcome::Done);
        }
    } else {
        print_plan(&plan);
    }
    if !args.yes {
        if interactive {
            eprintln!("Not a terminal: plan printed only. Pass --yes to commit it.");
        }
        return Ok(SgcOutcome::Done);
    }

    let mut created = 0;
    for (i, group) in plan.groups.iter().enumerate() {
        match sc::commit_group(repo, group, &plan.files, commit_cfg.sign_off) {
            Ok(hash) => {
                created += 1;
                eprintln!("✓ {hash} {}", group.title);
            }
            Err(e) => {
                let rest = &plan.groups[i..];
                let _ = sc::save_remaining(repo, &command, &args.options, rest);
                bail!(
                    "commit '{}' failed after {created} commit(s): {e:#}\n{} group(s) kept for the next run",
                    group.title,
                    rest.len()
                );
            }
        }
    }
    sc::clear_cache(repo);
    eprintln!("{created} commit(s) created");
    Ok(SgcOutcome::Done)
}

fn print_plan(plan: &sc::SmartCommitPlan) {
    let mut out = std::io::stdout().lock();
    for (i, group) in plan.groups.iter().enumerate() {
        let (added, deleted) = group.files.iter().fold((0, 0), |(a, d), f| {
            plan.files
                .get(f)
                .map_or((a, d), |m| (a + m.added, d + m.deleted))
        });
        let _ = writeln!(out, "{:>2}. {}  (+{added} -{deleted})", i + 1, group.title);
        for line in group.body.lines() {
            let _ = writeln!(out, "      {line}");
        }
        for path in &group.files {
            let kind = plan.files.get(path).map_or(' ', |m| m.kind.letter());
            let _ = writeln!(out, "      {kind} {path}");
        }
    }
    for warning in &plan.warnings {
        eprintln!("note: {warning}");
    }
}

pub struct GcArgs {
    pub all: bool,
    pub commit: bool,
    /// Alternatives to generate; >1 opens a picker on a terminal.
    pub count: usize,
    pub title_only: bool,
    pub silent_empty: bool,
    pub command: Option<String>,
    pub options: sc::SmartCommitOptions,
}

fn git(repo: &Path, argv: &[&str]) -> Result<std::process::Output> {
    let out = std::process::Command::new("git")
        .args(argv)
        .current_dir(repo)
        .stdin(std::process::Stdio::null())
        .output()?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            argv.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out)
}

fn has_staged(repo: &Path) -> bool {
    std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(repo)
        .status()
        .is_ok_and(|s| s.code() == Some(1))
}

fn ai_command(config: &AppConfig, override_cmd: &Option<String>) -> String {
    override_cmd.clone().unwrap_or_else(|| {
        config
            .user_config
            .git
            .commit
            .smart_commit_command()
            .to_string()
    })
}

/// Print (or commit) an AI commit message for the staged changes.
pub fn gc(repo: &Path, config: &AppConfig, args: GcArgs) -> Result<()> {
    if args.all {
        git(repo, &["add", "-A"])?;
    }
    if !has_staged(repo) {
        if args.silent_empty {
            return Ok(());
        }
        bail!(if args.all {
            "nothing to commit: the working tree is clean"
        } else {
            "no staged changes (use -a to stage everything)"
        });
    }

    let progress = |line: &str| {
        if args.options.debug || std::io::stderr().is_terminal() {
            eprintln!("\x1b[2m› {line}\x1b[0m");
        }
    };
    let Some(candidates) = sc::generate_messages(
        repo,
        &ai_command(config, &args.command),
        &args.options,
        args.count,
        Arc::new(AtomicBool::new(false)),
        &progress,
    )?
    else {
        bail!("cancelled");
    };
    let chosen = if candidates.len() > 1 {
        match pick(&candidates)? {
            Some(index) => &candidates[index],
            None => {
                // No terminal to ask on: print every candidate, blank-line separated.
                let all: Vec<String> = candidates
                    .iter()
                    .map(|c| render(c, args.title_only))
                    .collect();
                println!("{}", all.join("\n\n"));
                return Ok(());
            }
        }
    } else {
        &candidates[0]
    };
    let message = render(chosen, args.title_only);

    if args.commit {
        let mut child = std::process::Command::new("git")
            .args(["commit", "-F", "-"])
            .current_dir(repo)
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(message.as_bytes())?;
        }
        if !child.wait()?.success() {
            bail!("git commit failed");
        }
    } else {
        println!("{message}");
    }
    Ok(())
}

fn render(group: &sc::SmartCommitGroup, title_only: bool) -> String {
    if title_only {
        group.title.clone()
    } else {
        group.message()
    }
}

/// Numbered picker on the controlling terminal; `Ok(None)` without one.
fn pick(candidates: &[sc::SmartCommitGroup]) -> Result<Option<usize>> {
    use std::io::BufRead;
    if !std::io::stderr().is_terminal() {
        return Ok(None);
    }
    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        return Ok(None);
    };
    let mut err = std::io::stderr().lock();
    for (i, c) in candidates.iter().enumerate() {
        writeln!(err, "\x1b[1m{}.\x1b[0m {}", i + 1, c.title)?;
        for line in c.body.lines() {
            writeln!(err, "     \x1b[2m{line}\x1b[0m")?;
        }
    }
    let mut reader = std::io::BufReader::new(tty);
    loop {
        write!(err, "Pick 1-{} [1], q to abort: ", candidates.len())?;
        err.flush()?;
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            bail!("aborted");
        }
        let line = line.trim();
        if line.eq_ignore_ascii_case("q") {
            bail!("aborted");
        }
        if line.is_empty() {
            return Ok(Some(0));
        }
        if let Ok(n) = line.parse::<usize>()
            && (1..=candidates.len()).contains(&n)
        {
            return Ok(Some(n - 1));
        }
    }
}

// ── prepare-commit-msg hook ────────────────────────────────────────────────

const HOOK_BEGIN: &str = "# >>> lazygitrs gc hook (managed) >>>";
const HOOK_END: &str = "# <<< lazygitrs gc hook (managed) <<<";
/// Marker of the hook the old zsh `gc hook install` wrote.
const LEGACY_MARKER: &str = "# gc-managed: prepare-commit-msg hook";

fn hook_block() -> String {
    format!(
        "{HOOK_BEGIN}\n\
         # Fills an empty commit message with `lazygitrs gc`. Remove: lazygitrs gc hook uninstall\n\
         if command -v lazygitrs >/dev/null 2>&1; then\n  \
           lazygitrs gc hook run \"$@\" </dev/null || true\n\
         fi\n\
         {HOOK_END}\n"
    )
}

fn hook_path(repo: &Path) -> Result<std::path::PathBuf> {
    let out = git(
        repo,
        &["rev-parse", "--git-path", "hooks/prepare-commit-msg"],
    )?;
    let rel = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(repo.join(rel))
}

pub fn hook_install(repo: &Path) -> Result<()> {
    let path = hook_path(repo)?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.contains(HOOK_BEGIN) {
        println!("gc hook: already installed → {}", path.display());
        return Ok(());
    }
    let mut text = remove_hook_blocks(&existing);
    if text.trim().is_empty() {
        text = "#!/bin/sh\n".to_string();
    } else if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&hook_block());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    if existing.contains(LEGACY_MARKER) {
        println!("gc hook: replaced the old zsh gc hook");
    }
    println!("gc hook: installed → {}", path.display());
    Ok(())
}

pub fn hook_uninstall(repo: &Path) -> Result<()> {
    let path = hook_path(repo)?;
    let Ok(existing) = std::fs::read_to_string(&path) else {
        println!("gc hook: no hook file found");
        return Ok(());
    };
    if !existing.contains(HOOK_BEGIN) && !existing.contains(LEGACY_MARKER) {
        bail!(
            "{} was not installed by gc; leaving it alone",
            path.display()
        );
    }
    let kept = remove_hook_blocks(&existing);
    let only_shebang = kept
        .lines()
        .all(|l| l.trim().is_empty() || l.starts_with("#!"));
    if only_shebang {
        std::fs::remove_file(&path)?;
    } else {
        std::fs::write(&path, kept)?;
    }
    println!("gc hook: uninstalled from {}", path.display());
    Ok(())
}

pub fn hook_status(repo: &Path) -> Result<()> {
    let path = hook_path(repo)?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if text.contains(HOOK_BEGIN) {
        println!("gc hook: installed → {}", path.display());
    } else if text.contains(LEGACY_MARKER) {
        println!(
            "gc hook: old zsh gc hook installed (run: lazygitrs gc hook install to replace it)"
        );
    } else {
        println!("gc hook: not installed (run: lazygitrs gc hook install)");
    }
    Ok(())
}

/// Strip the managed block and a legacy zsh block (which ran to end of file).
fn remove_hook_blocks(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if line == HOOK_BEGIN {
            skipping = true;
            continue;
        }
        if skipping {
            if line == HOOK_END {
                skipping = false;
            }
            continue;
        }
        if line.contains(LEGACY_MARKER) {
            // The legacy block starts with its own shebang right above the marker.
            if out.last().is_some_and(|l| l.starts_with("#!")) && out.len() > 1 {
                out.pop();
            }
            break;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    if out.is_empty() {
        String::new()
    } else {
        format!("{}\n", out.join("\n"))
    }
}

/// Hook body: fill `file` with a generated message when git left it empty.
/// Never fails the commit; problems are reported on stderr.
pub fn hook_run(repo: &Path, config: &AppConfig, file: &Path, source: Option<&str>) -> Result<()> {
    // message (-m/-F), template, merge, squash, commit (amend): user/git chose a message.
    if source.is_some_and(|s| !s.is_empty()) {
        return Ok(());
    }
    let existing = std::fs::read_to_string(file).unwrap_or_default();
    if has_user_message(&existing) {
        return Ok(());
    }
    if !has_staged(repo) {
        return Ok(());
    }
    let generated = sc::generate_messages(
        repo,
        &ai_command(config, &None),
        &sc::SmartCommitOptions::default(),
        1,
        Arc::new(AtomicBool::new(false)),
        &|_| {},
    );
    match generated {
        Ok(Some(candidates)) => {
            let message = candidates[0].message();
            std::fs::write(file, format!("{message}\n\n{existing}"))?;
        }
        Ok(None) => {}
        Err(e) => eprintln!("lazygitrs gc hook: {e:#}"),
    }
    Ok(())
}

/// Does the message file hold anything besides comments? Everything below
/// the scissors line (`commit.verbose` diff) is ignored, like git does.
fn has_user_message(text: &str) -> bool {
    text.lines()
        .take_while(|l| !(l.starts_with('#') && l.contains(">8")))
        .any(|l| !l.trim().is_empty() && !l.starts_with('#'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbose_diff_below_scissors_is_not_a_message() {
        let verbose = "\n# Please enter...\n# ------------------------ >8 ------------------------\n# Do not modify\ndiff --git a/x b/x\n+y\n";
        assert!(!has_user_message(verbose));
        assert!(has_user_message("fix: x\n# comment\n"));
    }

    #[test]
    fn hook_blocks_roundtrip_and_legacy_removal() {
        let user = "#!/bin/sh\necho mine\n";
        let installed = format!("{user}{}", hook_block());
        assert_eq!(remove_hook_blocks(&installed), user);

        let legacy = "#!/bin/sh\necho mine\n\n#!/usr/bin/env bash\n# gc-managed: prepare-commit-msg hook\nmsg=x\n";
        assert_eq!(remove_hook_blocks(legacy), user);
        let legacy_only = "#!/usr/bin/env bash\n# gc-managed: prepare-commit-msg hook\nmsg=x\n";
        assert_eq!(remove_hook_blocks(legacy_only), "#!/usr/bin/env bash\n");
    }
}
