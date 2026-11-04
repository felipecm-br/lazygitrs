//! Model refresh, rebase-progress sync, persisted UI preferences, and view-state helpers.

use super::*;

impl Gui {
    /// Approximate visible height of the active sidebar panel (inner area minus borders).
    pub(super) fn sidebar_visible_height(&self) -> usize {
        let fl = self.compute_current_frame_layout();
        let active_window = self.context_mgr.active_window();
        let active_panel_index = SideWindow::ALL
            .iter()
            .position(|w| *w == active_window)
            .unwrap_or(1);
        // In Full screen mode with sidebar focused, the list is rendered in main_panel
        let panel_rect = if self.screen_mode == ScreenMode::Full && !self.diff_focused {
            fl.main_panel
        } else {
            fl.side_panels
                .get(active_panel_index)
                .copied()
                .unwrap_or(fl.main_panel)
        };
        // Subtract 2 for top/bottom borders
        panel_rect.height.saturating_sub(2) as usize
    }

    pub(crate) fn sync_rebase_progress_view(&mut self) -> bool {
        let was_active_in_progress =
            self.rebase_mode.active && self.rebase_mode.phase == RebasePhase::InProgress;
        let previous_current_hash = if was_active_in_progress {
            self.rebase_mode
                .entries
                .iter()
                .find(|entry| entry.status == EntryStatus::Current)
                .map(|entry| entry.hash.clone())
        } else {
            None
        };
        let previous_selected_hash = if was_active_in_progress {
            self.rebase_mode
                .entries
                .get(self.rebase_mode.selected)
                .map(|entry| entry.hash.clone())
        } else {
            None
        };
        let previous_scroll = self.rebase_mode.scroll;

        let Some(mut progress) = self.git.parse_rebase_progress() else {
            return false;
        };
        self.git.hydrate_progress(&mut progress);
        self.rebase_mode.enter_in_progress(&progress);

        let current_hash = self
            .rebase_mode
            .entries
            .iter()
            .find(|entry| entry.status == EntryStatus::Current)
            .map(|entry| entry.hash.clone());

        if was_active_in_progress
            && previous_current_hash.is_some()
            && previous_current_hash == current_hash
            && let Some(selected_hash) = previous_selected_hash
            && let Some(selected) = self
                .rebase_mode
                .entries
                .iter()
                .position(|entry| entry.hash == selected_hash)
        {
            self.rebase_mode.selected = selected;
            let list_len = self.rebase_mode.entries.len() + 1;
            let max_scroll = list_len.saturating_sub(self.rebase_mode.visible_height);
            self.rebase_mode.scroll = previous_scroll.min(max_scroll);
            self.rebase_mode
                .ensure_visible(self.rebase_mode.visible_height);
        }

        true
    }

    /// Kick off a full model reload on a background thread (same streaming
    /// path as initial load). UI stays responsive; panels fill as parts arrive.
    pub(super) fn start_background_refresh(&mut self) {
        if self.refresh_in_progress || self.initial_load_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.initial_load_rx = Some(rx);
        self.initial_load_received = 0;
        self.refresh_in_progress = true;
        self.needs_refresh = false;
        // Taken before loading: a change that lands mid-load leaves the next
        // tick with a mismatch, so it reloads again rather than missing it.
        self.refs_fingerprint = self.git.refs_fingerprint();
        self.reset_commit_pagination();
        self.diff_preview_cache.retain_immutable();
        let git = Arc::clone(&self.git);
        let commit_filter = self.commit_filter_for_load();
        std::thread::spawn(move || {
            git.load_model_streaming(&tx, commit_filter);
        });
    }

    pub(super) fn commit_filter_for_load(&self) -> Option<crate::git::commit::CommitFilter> {
        self.commit_filter
            .is_active()
            .then(|| crate::git::commit::CommitFilter {
                branches: self.commit_filter.branch.clone(),
                path: self.commit_filter.path.clone(),
                authors: self.commit_filter.author.clone(),
            })
    }

    pub(super) fn refresh(&mut self) -> Result<()> {
        let commit_limit = {
            let model = self
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            model.commits.len().max(DEFAULT_COMMIT_LIMIT)
        };
        self.reset_commit_pagination();
        self.diff_preview_cache.retain_immutable();
        let new_model = self.git.load_model(commit_limit, self.all_branches_log)?;
        {
            let mut model = self
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            model.replace_keeping_file_order(new_model);

            // If branch filters are active, reload commits for those branches only.
            if !self.commit_filter.branch.is_empty()
                && let Ok(filtered) = self
                    .git
                    .load_commits_for_branches(&self.commit_filter.branch, commit_limit)
            {
                model.commits = filtered;
            }
        }
        self.after_model_refresh()
    }

    /// Re-apply selection-dependent views after the model was reloaded
    /// (blocking `refresh` or background streaming refresh).
    pub(super) fn after_model_refresh(&mut self) -> Result<()> {
        // Commits arrive already filtered via load_model_streaming(commit_filter)
        // or reload_filtered_commits_async — don't re-fetch here.
        let mut model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.commit_history_complete = model.commits.len() < DEFAULT_COMMIT_LIMIT;

        // Rebuild file tree inline to avoid borrow issues
        if self.show_file_tree {
            self.file_tree_nodes = build_file_tree(&model.files, &self.collapsed_dirs);
            self.context_mgr.files_list_len_override = Some(self.file_tree_nodes.len());
        } else {
            self.file_tree_nodes.clear();
            self.context_mgr.files_list_len_override = None;
        }

        // If we're viewing branch commits, re-load them (refresh wipes the model)
        if (self.context_mgr.active() == ContextId::BranchCommits
            || self.context_mgr.active() == ContextId::BranchCommitFiles)
            && !self.branch_commits_name.is_empty()
            && let Ok(commits) = self
                .git
                .load_commits_for_branch(&self.branch_commits_name, 300)
        {
            model.set_sub_commits(commits);
        }

        // If we're viewing remote branches (or drilled into commits/files from them), re-load them
        if !self.remote_branches_name.is_empty()
            && (self.context_mgr.active() == ContextId::RemoteBranches
                || ((self.context_mgr.active() == ContextId::BranchCommits
                    || self.context_mgr.active() == ContextId::BranchCommitFiles)
                    && self.sub_commits_parent_context == ContextId::RemoteBranches))
            && let Some(remote) = model
                .remotes
                .iter()
                .find(|r| r.name == self.remote_branches_name)
        {
            model.sub_remote_branches = remote.branches.clone();
        }

        // If we're viewing commit/stash files, re-load them (refresh wipes the model)
        if (self.context_mgr.active() == ContextId::CommitFiles
            || self.context_mgr.active() == ContextId::StashFiles
            || self.context_mgr.active() == ContextId::BranchCommitFiles)
            && !self.commit_files_hash.is_empty()
        {
            if let Ok(cf) = self.git.commit_files(&self.commit_files_hash) {
                model.commit_files = cf;
            }
            if self.show_commit_file_tree {
                self.commit_file_tree_nodes = crate::model::file_tree::build_commit_file_tree(
                    &model.commit_files,
                    &self.commit_files_collapsed_dirs,
                );
                self.context_mgr.commit_files_list_len_override =
                    Some(self.commit_file_tree_nodes.len());
            }
        }

        let is_rebasing = model.is_rebasing;
        drop(model);

        // Auto-enter or resync rebase InProgress mode when a rebase is
        // detected on disk. If the view is already open, keep its todo status
        // in step with Git so `rebase --continue` can advance to the next
        // paused commit without leaving the old entry marked current.
        if is_rebasing {
            let should_open = !self.rebase_mode.active && !self.rebase_mode.in_progress_dismissed;
            let should_resync =
                self.rebase_mode.active && self.rebase_mode.phase == RebasePhase::InProgress;
            if should_open || should_resync {
                self.sync_rebase_progress_view();
            }
        }
        // If rebase mode was active but the rebase completed, exit and show success.
        if !is_rebasing
            && self.rebase_mode.active
            && self.rebase_mode.phase == RebasePhase::InProgress
        {
            let branch = self.rebase_mode.branch_name.clone();
            let count = self.rebase_mode.total_count;
            self.rebase_mode.exit();
            self.popup = crate::gui::popup::PopupState::Message {
                title: "Rebase complete".to_string(),
                message: format!(
                    "Successfully rebased '{}' ({} commit{}).",
                    branch,
                    count,
                    if count == 1 { "" } else { "s" },
                ),
                kind: crate::gui::popup::MessageKind::Info,
            };
        }
        // Clear the dismissal flag once no rebase is in progress, so the next
        // rebase (or new conflict) can auto-open the InProgress view again.
        if !is_rebasing && self.rebase_mode.in_progress_dismissed {
            self.rebase_mode.in_progress_dismissed = false;
        }

        Ok(())
    }

    /// Lightweight refresh that only reloads files and diff stats.
    /// Prefer the async status-only path after stage/unstage; this full
    /// variant is kept for callers that need numstat immediately.
    pub(super) fn refresh_files_only(&mut self) -> Result<()> {
        self.diff_preview_cache.retain_immutable();
        // Status-only is enough for staging correctness; skip expensive
        // numstat/hunk subprocesses on this hot path.
        let files = self.git.load_files_status_only()?;
        let mut model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        model.set_files(files);

        if self.show_file_tree {
            self.file_tree_nodes = build_file_tree(&model.files, &self.collapsed_dirs);
            self.context_mgr.files_list_len_override = Some(self.file_tree_nodes.len());
        } else {
            self.file_tree_nodes.clear();
            self.context_mgr.files_list_len_override = None;
        }

        Ok(())
    }

    /// Rebuild the file tree from the current in-memory model (no git).
    pub(crate) fn rebuild_file_tree_from_model(&mut self) {
        let model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.show_file_tree {
            self.file_tree_nodes = build_file_tree(&model.files, &self.collapsed_dirs);
            self.context_mgr.files_list_len_override = Some(self.file_tree_nodes.len());
        } else {
            self.file_tree_nodes.clear();
            self.context_mgr.files_list_len_override = None;
        }
    }

    /// Resolve the currently selected file index in the files panel.
    /// In tree view, maps the tree node selection to the actual file index.
    /// Returns None if a directory node is selected (no file to operate on).
    pub fn selected_file_index(&self) -> Option<usize> {
        let selected = self.context_mgr.selected_active();
        if self.show_file_tree {
            self.file_tree_nodes
                .get(selected)
                .and_then(|node| node.file_index)
        } else {
            Some(selected)
        }
    }

    pub(super) fn commit_history_path(config: &AppConfig) -> std::path::PathBuf {
        config.state_dir.join("commit_message_history")
    }

    pub(super) fn persist_command_log_visibility(&self) {
        if let Ok(mut state) = AppState::load(&self.config.state_path) {
            state.show_command_log = Some(self.show_command_log);
            let _ = state.save(&self.config.state_path);
        }
    }

    pub fn persist_file_tree_visibility(&self) {
        if let Ok(mut state) = AppState::load(&self.config.state_path) {
            state.show_file_tree = Some(self.show_file_tree);
            let _ = state.save(&self.config.state_path);
        }
    }

    pub fn persist_commit_details_visibility(&self) {
        if let Ok(mut state) = AppState::load(&self.config.state_path) {
            state.show_commit_details = Some(self.show_commit_details);
            let _ = state.save(&self.config.state_path);
        }
    }

    pub fn persist_diff_line_wrap(&self) {
        if let Ok(mut state) = AppState::load(&self.config.state_path) {
            state.diff_line_wrap = Some(self.diff_view.wrap);
            let _ = state.save(&self.config.state_path);
        }
    }

    pub fn persist_diff_view_layout(&self) {
        if let Ok(mut state) = AppState::load(&self.config.state_path) {
            state.diff_view = Some(self.diff_view.view_layout.as_state_value().to_string());
            let _ = state.save(&self.config.state_path);
        }
    }

    pub(super) fn load_commit_history(config: &AppConfig) -> Vec<String> {
        let path = Self::commit_history_path(config);
        match std::fs::read_to_string(&path) {
            Ok(contents) => contents
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Effective wrap width for the commit-body textarea, derived from popup
    /// geometry and the user's `git.commit.auto_wrap_width` config.
    pub(super) fn commit_body_wrap_width(&self) -> usize {
        let popup_width = (self.layout.width * 60 / 100)
            .clamp(30, 60)
            .min(self.layout.width.max(1));
        let popup_inner = popup_width.saturating_sub(4) as usize;
        let config_width = self.config.user_config.git.commit.auto_wrap_width;
        if config_width > 0 {
            popup_inner.min(config_width)
        } else {
            popup_inner
        }
        .max(1)
    }

    pub(super) fn save_commit_history(&self) {
        let path = Self::commit_history_path(&self.config);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let contents = self.commit_message_history.join("\0");
        let _ = std::fs::write(&path, contents);
    }

    pub fn update_file_tree_state(&mut self) {
        if self.show_file_tree {
            let model = self
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.file_tree_nodes = build_file_tree(&model.files, &self.collapsed_dirs);
            self.context_mgr.files_list_len_override = Some(self.file_tree_nodes.len());
        } else {
            self.file_tree_nodes.clear();
            self.context_mgr.files_list_len_override = None;
        }
    }

    /// Exit sub-contexts (like CommitFiles) back to their parent context
    /// before navigating away to another window.
    pub(super) fn exit_sub_contexts(&mut self) {
        self.range_select_anchor = None;
        if self.context_mgr.active() == ContextId::CommitFiles {
            self.context_mgr.set_active(ContextId::Commits);
        }
        if self.context_mgr.active() == ContextId::StashFiles {
            self.context_mgr.set_active(ContextId::Stash);
        }
        if self.context_mgr.active() == ContextId::BranchCommitFiles {
            self.context_mgr.set_active(ContextId::BranchCommits);
        }
        if self.context_mgr.active() == ContextId::BranchCommits {
            self.context_mgr.set_active(ContextId::Branches);
        }
        if self.context_mgr.active() == ContextId::RemoteBranches {
            self.context_mgr.set_active(ContextId::Remotes);
        }
    }

    pub fn toggle_working_tree_and_head(&mut self) {
        self.range_select_anchor = None;
        match self.context_mgr.active() {
            ContextId::Commits | ContextId::CommitFiles | ContextId::Reflog => {
                self.context_mgr.set_active(ContextId::Files);
                self.needs_diff_refresh = true;
            }
            _ => {
                self.context_mgr.set_active(ContextId::Commits);
                self.context_mgr.set_selection_for(ContextId::Commits, 0);
                self.context_mgr.set_scroll_offset(ContextId::Commits, 0);
                self.needs_diff_refresh = true;
            }
        }
    }

    pub(super) fn next_screen_mode(&mut self) {
        self.screen_mode = match self.screen_mode {
            ScreenMode::Normal => ScreenMode::Half,
            ScreenMode::Half => ScreenMode::Full,
            ScreenMode::Full => ScreenMode::Normal,
        };
    }

    pub(super) fn prev_screen_mode(&mut self) {
        self.screen_mode = match self.screen_mode {
            ScreenMode::Normal => ScreenMode::Full,
            ScreenMode::Half => ScreenMode::Normal,
            ScreenMode::Full => ScreenMode::Half,
        };
    }
}
