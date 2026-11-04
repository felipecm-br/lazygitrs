use anyhow::Result;
use crossterm::event::KeyEvent;

use crate::config::keybindings::parse_key;
use crate::config::user_config::CustomCommand;
use crate::gui::Gui;
use crate::gui::context::ContextId;
use crate::gui::popup::{MessageKind, PopupState};
use crate::os::cmd::CmdBuilder;

/// Try to handle a key as a custom command. Returns Ok(true) if handled.
pub fn try_handle_key(gui: &mut Gui, key: KeyEvent) -> Result<bool> {
    let active = gui.context_mgr.active();
    let context_name = context_id_to_name(active);
    let commands = gui.config.user_config.custom_commands.clone();

    for cmd in &commands {
        if cmd.key.is_empty() || cmd.command.is_empty() {
            continue;
        }

        // Match context: "global" matches everywhere, otherwise match the panel name
        let context_matches =
            cmd.context == "global" || cmd.context.is_empty() || cmd.context == context_name;

        if !context_matches {
            continue;
        }

        if let Some(expected) = parse_key(&cmd.key)
            && key.code == expected.code
            && key.modifiers == expected.modifiers
        {
            return execute_custom_command(gui, cmd).map(|_| true);
        }
    }

    Ok(false)
}

fn execute_custom_command(gui: &mut Gui, cmd: &CustomCommand) -> Result<()> {
    // Resolve template variables in the command string
    let resolved = resolve_template(gui, &cmd.command);

    if cmd.prompts.is_empty() {
        // No prompts — execute directly
        run_command(gui, &resolved, cmd.show_output)?;
    } else {
        // Has prompts — for now, show a simple input for the first prompt
        let prompt = cmd.prompts[0].clone();
        let title = prompt.title.unwrap_or_else(|| "Input".to_string());
        let show_output = cmd.show_output;
        let cmd_template = resolved;

        gui.popup = PopupState::Input {
            title,
            textarea: crate::gui::popup::make_textarea(""),
            on_confirm: Box::new(move |gui, input| {
                let final_cmd = cmd_template.replace("{{index .PromptResponses 0}}", input);
                run_command(gui, &final_cmd, show_output)?;
                Ok(())
            }),
            is_commit: false,
            confirm_focused: false,
        };
    }

    Ok(())
}

fn run_command(gui: &mut Gui, command: &str, show_output: bool) -> Result<()> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());

    // Create a temporary script file. Unique name + create_new so concurrent
    // commands don't collide and a pre-planted path in /tmp is never reused.
    let temp_dir = std::env::temp_dir();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let temp_file = temp_dir.join(format!("lazygitrs-cmd-{}-{nonce}.sh", std::process::id()));

    let functions_file = &gui.config.user_config.os.shell_functions_file;
    let script_content = build_script(&shell, functions_file, command);
    {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_file)?;
        f.write_all(script_content.as_bytes())?;
    }

    // Execute the temporary script
    let result = CmdBuilder::new(&shell)
        .arg(temp_file.to_string_lossy().as_ref())
        .stdin(String::new())
        .cwd_path(gui.git.repo_path())
        .run();

    // Clean up temporary file
    let _ = std::fs::remove_file(&temp_file);

    let result = result?;

    if let Ok(mut log) = gui.command_log.lock() {
        log.push(format!("$ {}", command));
    }

    if show_output && !result.stdout.is_empty() {
        gui.popup = PopupState::Message {
            title: "Command output".to_string(),
            message: result.stdout_trimmed().to_string(),
            kind: MessageKind::Info,
        };
    } else if !result.success {
        gui.popup = PopupState::Message {
            title: "Command failed".to_string(),
            message: result.stderr.trim().to_string(),
            kind: MessageKind::Error,
        };
    }

    gui.needs_refresh = true;
    Ok(())
}

fn resolve_template(gui: &Gui, template: &str) -> String {
    let model = gui.model.lock().unwrap();
    let selected = gui.context_mgr.selected_active();
    let active = gui.context_mgr.active();

    let mut result = template.to_string();

    // Selected branch name
    let branch_name = model
        .branches
        .iter()
        .find(|b| b.head)
        .map(|b| b.name.as_str())
        .unwrap_or("");
    result = result.replace("{{.SelectedLocalBranch.Name}}", branch_name);
    result = result.replace("{{.CheckedOutBranch.Name}}", branch_name);

    // Selected item based on context
    match active {
        ContextId::Branches => {
            if let Some(branch) = model.branches.get(selected) {
                result = result.replace("{{.SelectedLocalBranch.Name}}", &branch.name);
            }
        }
        ContextId::Commits => {
            if let Some(commit) = model.commits.get(selected) {
                result = result.replace("{{.SelectedLocalCommit.Hash}}", &commit.hash);
                result = result.replace("{{.SelectedLocalCommit.Name}}", &commit.name);
            }
        }
        ContextId::Files => {
            let file_idx = gui.selected_file_index().unwrap_or(selected);
            if let Some(file) = model.files.get(file_idx) {
                result = result.replace("{{.SelectedFile.Name}}", &file.name);
            }
        }
        ContextId::Stash => {
            if let Some(entry) = model.stash_entries.get(selected) {
                result = result.replace("{{.SelectedStashEntry.Index}}", &entry.index.to_string());
                result = result.replace("{{.SelectedStashEntry.Name}}", &entry.name);
            }
        }
        ContextId::Tags => {
            if let Some(tag) = model.tags.get(selected) {
                result = result.replace("{{.SelectedTag.Name}}", &tag.name);
            }
        }
        _ => {}
    }

    result
}

fn context_id_to_name(ctx: ContextId) -> &'static str {
    match ctx {
        ContextId::Status => "status",
        ContextId::Files => "files",
        ContextId::Branches => "localBranches",
        ContextId::Remotes => "remotes",
        ContextId::Tags => "tags",
        ContextId::Commits => "commits",
        ContextId::Reflog => "reflogCommits",
        ContextId::Stash => "stash",
        ContextId::Worktrees => "worktrees",
        ContextId::Submodules => "submodules",
        ContextId::CommitFiles => "commitFiles",
        ContextId::StashFiles => "stashFiles",
        ContextId::BranchCommits => "branchCommits",
        ContextId::BranchCommitFiles => "branchCommitFiles",
        ContextId::RemoteBranches => "remoteBranches",
        ContextId::Staging => "staging",
    }
}

pub fn open_custom_command_prompt(gui: &mut Gui) -> Result<()> {
    gui.popup = PopupState::Input {
        title: "Execute Shell Command".to_string(),
        textarea: crate::gui::popup::make_textarea(""),
        on_confirm: Box::new(move |gui, input| {
            if !input.trim().is_empty() {
                run_command(gui, input, true)?;
            }
            Ok(())
        }),
        is_commit: false,
        confirm_focused: false,
    };
    Ok(())
}

/// Shell script for `run_command`: enable aliases, source the user's rc and
/// optional `os.shellFunctionsFile`, then run `command`.
fn build_script(shell: &str, functions_file: &str, command: &str) -> String {
    let is_zsh = shell.ends_with("zsh");
    let is_bash = shell.ends_with("bash");

    // Sourced before the rc files (the order the old hardcoded path used), so
    // the user's rc can still override anything it defines.
    let functions_file = functions_file.trim();
    let source_functions = (!functions_file.is_empty()).then(|| {
        let path = crate::config::user_config::expand_tilde(functions_file);
        let quoted = format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        format!("[ -f {quoted} ] && . {quoted} >/dev/null 2>&1\n")
    });

    let mut script = String::new();
    if is_zsh {
        script.push_str("#!/bin/zsh\n");
        script.push_str("setopt aliases\n");
        script.extend(source_functions);
        script.push_str("[ -f ~/.zshrc ] && source ~/.zshrc >/dev/null 2>&1\n");
    } else if is_bash {
        script.push_str("#!/bin/bash\n");
        script.push_str("shopt -s expand_aliases\n");
        script.extend(source_functions);
        script.push_str("[ -f ~/.bash_aliases ] && source ~/.bash_aliases >/dev/null 2>&1\n");
        script.push_str("[ -f ~/.bashrc ] && source ~/.bashrc >/dev/null 2>&1\n");
    } else {
        script.push_str("#!/bin/sh\n");
        script.extend(source_functions);
    }
    script.push_str(command);
    script.push('\n');
    script
}

#[cfg(test)]
mod tests {
    use super::build_script;

    #[test]
    fn no_personal_paths_without_config() {
        let s = build_script("/bin/zsh", "", "gst");
        assert!(!s.contains(".dotfiles"), "{s}");
        assert!(s.ends_with("gst\n"));
    }

    #[test]
    fn sources_configured_functions_file_quoted() {
        let s = build_script("/bin/bash", "/opt/it's/fns.sh", "gst");
        assert!(
            s.contains(r#"[ -f '/opt/it'\''s/fns.sh' ] && . '/opt/it'\''s/fns.sh'"#),
            "{s}"
        );
        let src = s.find("fns.sh").unwrap();
        assert!(
            src < s.find(".bashrc").unwrap(),
            "functions file before rc: {s}"
        );
        assert!(src < s.find("gst").unwrap());
    }
}
