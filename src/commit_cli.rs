//! Headless `lazygitrs sgc` / `lazygitrs gc`: the same planner and AI chain
//! the TUI uses, for scripts and shell aliases.

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::{Arc, atomic::AtomicBool};

use anyhow::{Result, bail};

use crate::config::AppConfig;
use crate::git::{ai_commit, smart_commit as sc};

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
}

/// Print one AI commit message for the staged changes (optionally staging
/// everything first, optionally committing it).
pub fn gc(repo: &Path, config: &AppConfig, args: GcArgs) -> Result<()> {
    let git = |argv: &[&str]| -> Result<()> {
        let status = std::process::Command::new("git")
            .args(argv)
            .current_dir(repo)
            .status()?;
        if !status.success() {
            bail!("git {} failed", argv.join(" "));
        }
        Ok(())
    };
    if args.all {
        git(&["add", "-A"])?;
    }
    let command = &config.user_config.git.commit.generate_command;
    status("Generating commit message");
    let Some(message) = ai_commit::generate_commit_message_cancellable(
        repo,
        command,
        Arc::new(AtomicBool::new(false)),
    )?
    else {
        bail!("cancelled");
    };
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
