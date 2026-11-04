use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::KeyEvent;

use crate::config::KeybindingConfig;
use crate::config::keybindings::matches_key;
use crate::git::rebase::RebaseAction;
use crate::gui::Gui;
use crate::gui::popup::{
    BodySoftWrap, ChecklistItem, CommitInputFocus, CommitInputKind, MenuItem, MessageKind,
    PopupState, make_textarea,
};
use crate::model::Branch;
use crate::os::platform::Platform;

mod clipboard;
mod filters;
mod history;
#[cfg(test)]
mod tests;

pub use clipboard::*;
pub use filters::*;
use history::*;

pub fn handle_key(gui: &mut Gui, key: KeyEvent, keybindings: &KeybindingConfig) -> Result<()> {
    // Esc: cancel range select first, then clear active commit filter, then clear clipboard
    if key.code == crossterm::event::KeyCode::Esc {
        if gui.range_select_anchor.is_some() {
            gui.range_select_anchor = None;
            return Ok(());
        }

        if gui.commit_filter.is_active() {
            gui.commit_filter.clear();
            apply_commit_filters_and_focus(gui)?;
            return Ok(());
        }

        if !gui.cherry_pick_clipboard.is_empty() {
            gui.cherry_pick_clipboard.clear();
            return Ok(());
        }
    }

    // Enter: open commit files subview
    if key.code == crossterm::event::KeyCode::Enter {
        return enter_commit_files(gui);
    }

    if matches_key(key, &keybindings.commits.revert_commit) {
        return revert_commit(gui);
    }

    // Reword in the user's editor (git opens core.editor / $GIT_EDITOR).
    if matches_key(key, &keybindings.commits.rename_commit_with_editor) {
        return reword_commit_with_editor(gui);
    }

    if matches_key(key, &keybindings.commits.rename_commit) {
        return reword_commit(gui);
    }

    if matches_key(key, &keybindings.commits.view_reset_options) {
        return show_reset_menu(gui);
    }

    if matches_key(key, &keybindings.commits.cherry_pick_copy) {
        return cherry_pick_copy(gui);
    }

    if matches_key(key, &keybindings.commits.paste_commits) {
        return paste_commits(gui);
    }

    if matches_key(key, &keybindings.commits.reset_cherry_pick) {
        gui.cherry_pick_clipboard.clear();
        return Ok(());
    }

    if matches_key(key, &keybindings.commits.squash_above_commits) {
        return squash_above_commits_menu(gui);
    }

    if matches_key(key, &keybindings.commits.tag_commit) {
        return tag_commit(gui);
    }

    // Squash down
    if matches_key(key, &keybindings.commits.squash_down) {
        return squash_commit(gui);
    }

    // Fixup
    if matches_key(key, &keybindings.commits.mark_commit_as_fixup) {
        return fixup_commit(gui);
    }

    // Move commit up
    if matches_key(key, &keybindings.commits.move_up_commit) {
        return move_commit_up(gui);
    }

    // Move commit down
    if matches_key(key, &keybindings.commits.move_down_commit) {
        return move_commit_down(gui);
    }

    // Create fixup commit
    if matches_key(key, &keybindings.commits.create_fixup_commit) {
        return create_fixup_commit(gui);
    }

    // Amend to commit
    if matches_key(key, &keybindings.commits.amend_to_commit) {
        return amend_to_commit(gui);
    }

    // Bisect options
    if matches_key(key, &keybindings.commits.view_bisect_options) {
        return super::bisect::show_bisect_menu(gui);
    }

    // Checkout commit
    if matches_key(key, &keybindings.commits.checkout_commit) {
        return checkout_commit(gui);
    }

    // Drop commit(s) — opens interactive rebase planner with Drop pre-marked
    if key.code == crossterm::event::KeyCode::Char('d') {
        return drop_commit(gui);
    }

    // Edit commit(s) — opens interactive rebase planner with Edit pre-marked
    if key.code == crossterm::event::KeyCode::Char('e') {
        return apply_action_to_selection(gui, RebaseAction::Edit);
    }

    // Open commit in browser
    if key.code == crossterm::event::KeyCode::Char('o') {
        return open_commit_in_browser_menu(gui);
    }

    // Copy to clipboard menu
    if key.code == crossterm::event::KeyCode::Char('y') {
        return copy_to_clipboard_menu(gui);
    }

    // Open commit filtering menu
    if matches_key(key, &keybindings.commits.open_log_menu)
        || (key.modifiers == crossterm::event::KeyModifiers::CONTROL
            && (key.code == crossterm::event::KeyCode::Char('l')
                || key.code == crossterm::event::KeyCode::Char('L')))
    {
        return show_filtering_menu(gui);
    }

    // Interactive rebase
    if matches_key(key, &keybindings.commits.interactive_rebase) {
        return enter_interactive_rebase(gui);
    }

    // Toggle range select
    if key.code == crossterm::event::KeyCode::Char('v') {
        return toggle_range_select(gui);
    }

    // Toggle showing all branches vs HEAD only
    if matches_key(key, &keybindings.status.all_branches_log_graph) {
        gui.all_branches_log = !gui.all_branches_log;
        gui.needs_refresh = true;
        gui.context_mgr.set_selection(0);
        return Ok(());
    }

    Ok(())
}

fn revert_commit(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(commit) = model.commits.get(selected) {
        let hash = commit.hash.clone();
        let short = commit.short_hash().to_string();
        drop(model);

        gui.popup = PopupState::Confirm {
            title: "Revert commit".to_string(),
            message: format!("Revert commit {}?", short),
            on_confirm: Box::new(move |gui| {
                gui.git.revert_commit(&hash)?;
                gui.needs_refresh = true;
                Ok(())
            }),
        };
    }
    Ok(())
}

fn reword_commit(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(commit) = model.commits.get(selected) {
        let hash = commit.hash.clone();
        let old_message = gui.git.commit_message_full(&hash)?;
        let (summary, body) = super::super::split_commit_message(&old_message);
        let is_head = selected == 0;
        drop(model);

        let mut summary_textarea = crate::gui::popup::make_commit_summary_textarea();
        summary_textarea.insert_str(&summary);
        let mut body_textarea = crate::gui::popup::make_commit_body_textarea();
        let body_state = BodySoftWrap::from_text(body);
        body_state.render_into(&mut body_textarea, gui.commit_body_wrap_width());

        gui.popup = PopupState::CommitInput {
            kind: CommitInputKind::Reword,
            summary_textarea,
            body_textarea,
            body_state,
            focus: CommitInputFocus::Summary,
            on_confirm: Box::new(move |gui, message| {
                if !message.is_empty() {
                    let message = message.to_string();
                    let hash = hash.clone();
                    if is_head {
                        gui.start_remote_op("Reword", "Rewording commit...", move |git| {
                            git.reword_commit(&hash, &message)?;
                            Ok(())
                        });
                    } else {
                        gui.start_remote_op("Reword", "Rewording commit...", move |git| {
                            git.reword_commit_rebase(&hash, &message)?;
                            Ok(())
                        });
                    }
                }
                Ok(())
            }),
        };
    }
    Ok(())
}

fn reword_commit_with_editor(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(hash) = model.commits.get(selected).map(|c| c.hash.clone()) else {
        return Ok(());
    };
    drop(model);
    let args = gui.git.reword_with_editor_command(&hash)?;
    gui.editor_handoff.pending_launch = Some(crate::config::user_config::EditorLaunch {
        program: "git".into(),
        args,
        suspend: true,
    });
    gui.needs_refresh = true;
    Ok(())
}

fn show_reset_menu(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(commit) = model.commits.get(selected) {
        let hash = commit.hash.clone();
        drop(model);
        return show_reset_menu_for_ref(gui, &hash);
    }
    Ok(())
}

/// Shared soft/mixed/hard reset options for a branch, tag, or commit ref.
/// Used by contextual `g` handlers and the global `G` reset picker.
pub fn show_reset_menu_for_ref(gui: &mut Gui, ref_name: &str) -> Result<()> {
    let ref_name = ref_name.trim();
    if ref_name.is_empty() {
        return Ok(());
    }

    let display = reset_ref_display(ref_name);
    let r1 = ref_name.to_string();
    let r2 = ref_name.to_string();
    let r3 = ref_name.to_string();

    gui.popup = PopupState::Menu {
        title: format!("Reset current branch to {}", display),
        items: vec![
            MenuItem {
                label: "Soft reset".to_string(),
                description: "Keep changes staged".to_string(),
                key: Some("s".to_string()),
                action: Some(Box::new(move |gui| {
                    gui.git.reset_to_commit(&r1, "--soft")?;
                    gui.needs_refresh = true;
                    Ok(())
                })),
            },
            MenuItem {
                label: "Mixed reset".to_string(),
                description: "Keep changes unstaged".to_string(),
                key: Some("m".to_string()),
                action: Some(Box::new(move |gui| {
                    gui.git.reset_to_commit(&r2, "--mixed")?;
                    gui.needs_refresh = true;
                    Ok(())
                })),
            },
            MenuItem {
                label: "Hard reset".to_string(),
                description: "Discard all changes".to_string(),
                key: Some("h".to_string()),
                action: Some(Box::new(move |gui| {
                    gui.git.reset_to_commit(&r3, "--hard")?;
                    gui.needs_refresh = true;
                    Ok(())
                })),
            },
        ],
        selected: 0,
        loading_index: None,
    };
    Ok(())
}

fn reset_ref_display(ref_name: &str) -> String {
    // Shorten bare commit hashes for the menu title; keep branch/tag names intact.
    if ref_name.len() >= 12 && ref_name.chars().all(|c| c.is_ascii_hexdigit()) {
        ref_name.chars().take(7).collect()
    } else {
        ref_name.to_string()
    }
}

fn cherry_pick_copy(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let (lo, hi) = if let Some(anchor) = gui.range_select_anchor {
        (anchor.min(selected), anchor.max(selected))
    } else {
        (selected, selected)
    };

    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut added = 0;
    let mut removed = 0;
    for i in lo..=hi {
        if let Some(commit) = model.commits.get(i) {
            if let Some(index) = gui
                .cherry_pick_clipboard
                .iter()
                .position(|hash| hash == &commit.hash)
            {
                gui.cherry_pick_clipboard.remove(index);
                removed += 1;
            } else {
                gui.cherry_pick_clipboard.push(commit.hash.clone());
                added += 1;
            }
        }
    }
    drop(model);

    // Exit range select after copying
    gui.range_select_anchor = None;

    let n = gui.cherry_pick_clipboard.len();
    let message = match (added, removed) {
        (0, removed) => format!(
            "Uncopied {} commit{} ({} total)",
            removed,
            if removed == 1 { "" } else { "s" },
            n,
        ),
        (added, 0) => format!(
            "Copied {} commit{} ({} total)",
            added,
            if added == 1 { "" } else { "s" },
            n,
        ),
        (added, removed) => format!(
            "Copied {} and uncopied {} commit{} ({} total)",
            added,
            removed,
            if added + removed == 1 { "" } else { "s" },
            n,
        ),
    };
    gui.popup = PopupState::Message {
        title: "Cherry-pick".to_string(),
        message,
        kind: crate::gui::popup::MessageKind::Info,
    };
    Ok(())
}

fn toggle_range_select(gui: &mut Gui) -> Result<()> {
    if gui.range_select_anchor.is_some() {
        gui.range_select_anchor = None;
    } else {
        gui.range_select_anchor = Some(gui.context_mgr.selected_active());
    }
    Ok(())
}

fn paste_commits(gui: &mut Gui) -> Result<()> {
    if gui.cherry_pick_clipboard.is_empty() {
        gui.popup = PopupState::Message {
            title: "Cherry-pick".to_string(),
            message: "No commits copied. Use cherry-pick copy (C) first.".to_string(),
            kind: crate::gui::popup::MessageKind::Error,
        };
        return Ok(());
    }

    let n = gui.cherry_pick_clipboard.len();
    let mut hashes = gui.cherry_pick_clipboard.clone();
    let target_branch = {
        let model = gui
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if model.head_branch_name.is_empty() {
            "detached HEAD".to_string()
        } else {
            format!("branch '{}'", model.head_branch_name)
        }
    };
    // The clipboard stores commits newest-first (matching the visual list order).
    // git cherry-pick applies commits in argument order, so we must reverse to
    // apply oldest-first and preserve the intended history.
    hashes.reverse();

    gui.popup = PopupState::Confirm {
        title: "Cherry-pick".to_string(),
        message: format!(
            "Cherry-pick {} copied commit{} onto {}?",
            n,
            if n == 1 { "" } else { "s" },
            target_branch,
        ),
        on_confirm: Box::new(move |gui| {
            gui.git.cherry_pick(&hashes)?;
            gui.cherry_pick_clipboard.clear();
            gui.needs_refresh = true;
            Ok(())
        }),
    };
    Ok(())
}

fn checkout_commit(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(commit) = model.commits.get(selected) {
        let hash = commit.hash.clone();
        let short = commit.short_hash().to_string();
        let branch_names = branches_at_commit(&hash, &model.branches, &model.head_branch_name);
        drop(model);

        let mut items = vec![MenuItem {
            label: format!("Checkout commit {} as detached head", short),
            description: String::new(),
            key: Some("d".to_string()),
            action: Some(Box::new(move |gui| {
                gui.git.checkout_branch(&hash)?;
                gui.needs_refresh = true;
                Ok(())
            })),
        }];

        if branch_names.is_empty() {
            items.push(MenuItem {
                label: "Checkout branch".to_string(),
                description: "No branches found at selected commit.".to_string(),
                key: Some("1".to_string()),
                action: None,
            });
        } else {
            items.extend(
                branch_names
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| MenuItem {
                        label: format!("Checkout branch '{}'", name),
                        description: String::new(),
                        key: checkout_branch_key(index),
                        action: Some(Box::new(move |gui| {
                            gui.git.checkout_branch(&name)?;
                            gui.needs_refresh = true;
                            Ok(())
                        })),
                    }),
            );
        }

        gui.popup = PopupState::Menu {
            title: "Checkout branch or commit".to_string(),
            items,
            selected: 0,
            loading_index: None,
        };
    }
    Ok(())
}

fn branches_at_commit(commit_hash: &str, branches: &[Branch], current_branch: &str) -> Vec<String> {
    branches
        .iter()
        .filter(|branch| {
            branch.name != current_branch
                && !branch.hash.is_empty()
                && (branch.hash == commit_hash || commit_hash.starts_with(&branch.hash))
        })
        .map(|branch| branch.name.clone())
        .collect()
}

fn checkout_branch_key(index: usize) -> Option<String> {
    (index < 9).then(|| (index + 1).to_string())
}

fn enter_commit_files(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(commit) = model.commits.get(selected) {
        let hash = commit.hash.clone();
        let message = commit.name.clone();
        drop(model);

        // Load commit files
        let commit_files = gui.git.commit_files(&hash)?;
        {
            let mut model = gui
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            model.commit_files = commit_files;
        }
        gui.commit_files_hash = hash;
        gui.commit_files_message = message;

        // Build commit file tree
        if gui.show_commit_file_tree {
            let model = gui
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            gui.commit_file_tree_nodes = crate::model::file_tree::build_commit_file_tree(
                &model.commit_files,
                &gui.commit_files_collapsed_dirs,
            );
            gui.context_mgr.commit_files_list_len_override = Some(gui.commit_file_tree_nodes.len());
        } else {
            gui.commit_file_tree_nodes.clear();
            let model = gui
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            gui.context_mgr.commit_files_list_len_override = None;
            let _ = model.commit_files.len(); // just ensure it compiles
        }

        // Switch to CommitFiles context
        gui.context_mgr
            .set_active(crate::gui::context::ContextId::CommitFiles);
        gui.context_mgr.set_selection(0);
        gui.needs_diff_refresh = true;
    }
    Ok(())
}

fn enter_interactive_rebase(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let base_commit = match model.commits.get(selected) {
        Some(c) => c,
        None => return Ok(()),
    };

    if base_commit.hash == model.head_hash {
        gui.popup = PopupState::Message {
            title: "Interactive rebase".to_string(),
            message: "Cannot rebase HEAD onto itself. Select a different commit.".to_string(),
            kind: crate::gui::popup::MessageKind::Error,
        };
        drop(model);
        return Ok(());
    }

    let base_hash = base_commit.hash.clone();
    let branch_name = model.head_branch_name.clone();

    // Ask git which commits would be rebased (handles all cases correctly:
    // base above HEAD, base below HEAD, non-linear histories, etc.)
    let rebase_hashes = match gui.git.rebase_commit_range(&base_hash) {
        Ok(hashes) => hashes,
        Err(e) => {
            gui.popup = PopupState::Message {
                title: "Interactive rebase".to_string(),
                message: format!("Failed to determine rebase range: {}", e),
                kind: crate::gui::popup::MessageKind::Error,
            };
            drop(model);
            return Ok(());
        }
    };

    if rebase_hashes.is_empty() {
        gui.popup = PopupState::Message {
            title: "Interactive rebase".to_string(),
            message: "No commits to rebase.".to_string(),
            kind: crate::gui::popup::MessageKind::Error,
        };
        drop(model);
        return Ok(());
    }

    // Match the hashes to model commits (preserving git's order: newest-first)
    let commits_to_rebase: Vec<_> = rebase_hashes
        .iter()
        .filter_map(|hash| model.commits.iter().find(|c| c.hash == *hash))
        .cloned()
        .collect();

    if commits_to_rebase.is_empty() {
        gui.popup = PopupState::Message {
            title: "Interactive rebase".to_string(),
            message: "No commits to rebase.".to_string(),
            kind: crate::gui::popup::MessageKind::Error,
        };
        drop(model);
        return Ok(());
    }

    gui.rebase_mode
        .enter(branch_name, base_commit, &commits_to_rebase);
    drop(model);

    Ok(())
}
