//! History edits on the selection: squash, fixup, drop, move, amend, tag, fixup commits (via the interactive rebase planner).

use super::*;

pub(super) fn squash_above_commits_menu(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let commit = match model.commits.get(selected) {
        Some(c) => c.clone(),
        None => return Ok(()),
    };
    let commits_len = model.commits.len();
    drop(model);

    let hash_above = commit.hash.clone();

    // Find "in current branch" base: last commit before a merge-base boundary.
    // Use the last commit in the list as a simple heuristic.
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let last_hash = model
        .commits
        .last()
        .map(|c| c.hash.clone())
        .unwrap_or_default();
    drop(model);

    let last_hash_clone = last_hash.clone();

    gui.popup = PopupState::Menu {
        title: "Apply fixup commits".to_string(),
        items: vec![
            MenuItem {
                label: "Above the selected commit".to_string(),
                description: format!("Autosquash fixup! commits above {}", commit.short_hash()),
                key: Some("a".to_string()),
                action: if commits_len > 0 {
                    Some(Box::new(move |gui| {
                        gui.git.rebase_autosquash(&format!("{}^", hash_above))?;
                        gui.needs_refresh = true;
                        Ok(())
                    }))
                } else {
                    None
                },
            },
            MenuItem {
                label: "In current branch".to_string(),
                description: "Autosquash all fixup! commits in the branch".to_string(),
                key: Some("b".to_string()),
                action: if !last_hash.is_empty() {
                    Some(Box::new(move |gui| {
                        gui.git
                            .rebase_autosquash(&format!("{}^", last_hash_clone))?;
                        gui.needs_refresh = true;
                        Ok(())
                    }))
                } else {
                    None
                },
            },
        ],
        selected: 0,
        loading_index: None,
    };
    Ok(())
}

pub(super) fn tag_commit(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(commit) = model.commits.get(selected) {
        let _hash = commit.hash.clone();
        drop(model);

        gui.popup = PopupState::Input {
            title: "Tag name".to_string(),
            textarea: make_textarea(""),
            on_confirm: Box::new(|gui, name| {
                if !name.is_empty() {
                    gui.git.create_tag(name, "")?;
                    gui.needs_refresh = true;
                }
                Ok(())
            }),
            is_commit: false,
            confirm_focused: false,
        };
    }
    Ok(())
}

pub(super) fn squash_commit(gui: &mut Gui) -> Result<()> {
    apply_action_to_selection(gui, RebaseAction::Squash)
}

pub(super) fn fixup_commit(gui: &mut Gui) -> Result<()> {
    apply_action_to_selection(gui, RebaseAction::Fixup)
}

pub(super) fn drop_commit(gui: &mut Gui) -> Result<()> {
    apply_action_to_selection(gui, RebaseAction::Drop)
}

/// Open the Interactive Rebase planner with the user's current commit
/// selection pre-marked with `action`. Selection comes from range-select
/// (`v`) when active; otherwise it's just the focused commit.
///
/// The rebase base is the *real first-parent* of the oldest selected commit
/// (not `commits[idx+1]`). The commits panel is loaded with `git log --all`,
/// so adjacent rows are not necessarily related — using list index as parent
/// previously marked/stopped on the wrong commit.
pub(super) fn apply_action_to_selection(gui: &mut Gui, action: RebaseAction) -> Result<()> {
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

    if model.commits.get(lo).is_none() || model.commits.get(hi).is_none() {
        return Ok(());
    }

    // Newest-first list: higher index = older. Collect selected hashes and
    // resolve the oldest selected commit's real first-parent as the base.
    let selected_hashes: HashSet<String> = model.commits[lo..=hi]
        .iter()
        .map(|c| c.hash.clone())
        .collect();
    let oldest = &model.commits[hi];
    let base_hash = match oldest.parents.first() {
        Some(p) if !p.is_empty() => p.clone(),
        _ => {
            drop(model);
            gui.popup = PopupState::Message {
                title: "Interactive rebase".to_string(),
                message: "Cannot rebase the root commit (no parent).".to_string(),
                kind: crate::gui::popup::MessageKind::Error,
            };
            return Ok(());
        }
    };
    let branch_name = model.head_branch_name.clone();

    // Prefer the model copy of the base when present; otherwise synthesize.
    let base_from_model = model
        .commits
        .iter()
        .find(|c| {
            c.hash == base_hash || base_hash.starts_with(&c.hash) || c.hash.starts_with(&base_hash)
        })
        .cloned();
    drop(model);

    // Only include commits reachable from HEAD above the base. This excludes
    // unrelated `--all` commits that happen to sit between selected rows.
    let rebase_hashes = match gui.git.rebase_commit_range(&base_hash) {
        Ok(h) => h,
        Err(e) => {
            gui.popup = PopupState::Message {
                title: "Interactive rebase".to_string(),
                message: format!("Failed to determine rebase range: {e}"),
                kind: crate::gui::popup::MessageKind::Error,
            };
            return Ok(());
        }
    };
    if rebase_hashes.is_empty() {
        gui.popup = PopupState::Message {
            title: "Interactive rebase".to_string(),
            message: "Nothing to rebase above the selected commit.".to_string(),
            kind: crate::gui::popup::MessageKind::Error,
        };
        return Ok(());
    }

    // Resolve each hash in the HEAD range to a Commit. Prefer the already-
    // loaded model entry; fall back to git metadata for commits outside the
    // currently loaded page (pagination / deep history).
    let model_commits = {
        let model = gui
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        model.commits.clone()
    };
    let mut commits_to_rebase = Vec::with_capacity(rebase_hashes.len());
    for hash in &rebase_hashes {
        if let Some(c) = model_commits.iter().find(|c| &c.hash == hash) {
            commits_to_rebase.push(c.clone());
        } else {
            let msg = gui.git.commit_subject(hash).unwrap_or_default();
            let author = gui.git.commit_author_name(hash).unwrap_or_default();
            commits_to_rebase.push(synthetic_commit(hash, &msg, &author));
        }
    }

    if commits_to_rebase.is_empty() {
        gui.popup = PopupState::Message {
            title: "Interactive rebase".to_string(),
            message: "Selected commit is not reachable from HEAD — cannot rebase it.".to_string(),
            kind: crate::gui::popup::MessageKind::Error,
        };
        return Ok(());
    }

    // Ensure at least one selected commit is in the range (guards against
    // selecting a commit that is not an ancestor of HEAD).
    if !commits_to_rebase
        .iter()
        .any(|c| selected_hashes.contains(&c.hash))
    {
        gui.popup = PopupState::Message {
            title: "Interactive rebase".to_string(),
            message: "Selected commit is not on the current branch tip — cannot rebase it."
                .to_string(),
            kind: crate::gui::popup::MessageKind::Error,
        };
        return Ok(());
    }

    let base_commit = match base_from_model {
        Some(c) => c,
        None => {
            let msg = gui.git.commit_subject(&base_hash).unwrap_or_default();
            let author = gui.git.commit_author_name(&base_hash).unwrap_or_default();
            synthetic_commit(&base_hash, &msg, &author)
        }
    };

    gui.range_select_anchor = None;

    gui.rebase_mode
        .enter(branch_name, &base_commit, &commits_to_rebase);

    for entry in gui.rebase_mode.entries.iter_mut() {
        if selected_hashes.contains(&entry.hash) {
            entry.action = action;
        }
    }

    // Focus the first (newest-first) selected entry in the planner.
    if let Some(idx) = gui
        .rebase_mode
        .entries
        .iter()
        .position(|e| selected_hashes.contains(&e.hash))
    {
        gui.rebase_mode.selected = idx;
    }

    Ok(())
}

pub(super) fn synthetic_commit(hash: &str, message: &str, author: &str) -> crate::model::Commit {
    use crate::model::commit::{CommitStatus, Divergence};
    crate::model::Commit {
        hash: hash.to_string(),
        name: message.to_string(),
        status: CommitStatus::Pushed,
        action: String::new(),
        tags: Vec::new(),
        refs: Vec::new(),
        extra_info: String::new(),
        author_name: author.to_string(),
        author_email: String::new(),
        unix_timestamp: 0,
        parents: Vec::new(),
        divergence: Divergence::None,
    }
}

pub(super) fn move_commit_up(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    if selected == 0 {
        return Ok(());
    }
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(commit) = model.commits.get(selected) {
        let hash = commit.hash.clone();
        drop(model);
        gui.git.move_commit_up(&hash)?;
        gui.needs_refresh = true;
    }
    Ok(())
}

pub(super) fn move_commit_down(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let commits_len = model.commits.len();
    if let Some(commit) = model.commits.get(selected) {
        if selected >= commits_len - 1 {
            drop(model);
            return Ok(());
        }
        let hash = commit.hash.clone();
        drop(model);
        gui.git.move_commit_down(&hash)?;
        gui.needs_refresh = true;
    }
    Ok(())
}

pub(super) fn create_fixup_commit(gui: &mut Gui) -> Result<()> {
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
            title: "Create fixup commit".to_string(),
            message: format!("Create fixup commit for {}?", short),
            on_confirm: Box::new(move |gui| {
                gui.git.create_fixup_commit(&hash)?;
                gui.needs_refresh = true;
                Ok(())
            }),
        };
    }
    Ok(())
}

pub(super) fn amend_to_commit(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(commit) = model.commits.get(selected) {
        if selected == 0 {
            // HEAD commit — just amend
            drop(model);
            gui.popup = PopupState::Confirm {
                title: "Amend".to_string(),
                message: "Amend staged changes to HEAD commit?".to_string(),
                on_confirm: Box::new(|gui| {
                    gui.start_remote_op("Amend", "Amending commit...", |git| {
                        git.amend_commit()?;
                        Ok(())
                    });
                    Ok(())
                }),
            };
        } else {
            // Non-HEAD: create fixup commit + autosquash
            let hash = commit.hash.clone();
            let short = commit.short_hash().to_string();
            drop(model);

            gui.popup = PopupState::Confirm {
                title: "Amend to commit".to_string(),
                message: format!("Amend staged changes to commit {}?", short),
                on_confirm: Box::new(move |gui| {
                    let hash = hash.clone();
                    gui.start_remote_op("Amend", "Amending commit...", move |git| {
                        git.create_fixup_commit(&hash)?;
                        git.rebase_autosquash(&format!("{}^", hash))?;
                        Ok(())
                    });
                    Ok(())
                }),
            };
        }
    }
    Ok(())
}
