//! Background job results: AI commit generation, commit pagination, auto-fetch, remote ops, async files refresh, async menu actions.

use super::*;

impl Gui {
    /// Check for completed AI commit message generation results.
    pub(super) fn receive_ai_commit_results(&mut self) {
        while let Ok(result) = self.ai_commit_rx.try_recv() {
            let active_generation = self.ai_commit_job.as_ref().map(|job| job.generation);
            if active_generation != Some(result.generation) {
                continue;
            }
            self.ai_commit_job = None;

            match result.result {
                Ok(Some(message)) => {
                    let popup_width = (self.layout.width * 60 / 100).clamp(30, 60);
                    let popup_inner = popup_width.saturating_sub(4) as usize;
                    let config_width = self.config.user_config.git.commit.auto_wrap_width;
                    let wrap = if config_width > 0 {
                        popup_inner.min(config_width)
                    } else {
                        popup_inner
                    };

                    // Split AI message into summary (first line) and body (rest).
                    // Preserve AI newlines as logical lines; BodySoftWrap soft-wraps
                    // for display only (avoids collapsing `- a\n- b` into one line).
                    let (summary, body) = match message.find('\n') {
                        Some(idx) => {
                            let s = message[..idx].to_string();
                            let raw_body = message[idx + 1..].trim_start_matches('\n').to_string();
                            (s, popup::unwrap_commit_body(&raw_body))
                        }
                        None => (message.clone(), String::new()),
                    };

                    // Helper to populate the two textareas
                    let fill_commit = |stashed: &mut PopupState| {
                        if let PopupState::CommitInput {
                            summary_textarea,
                            body_textarea,
                            body_state,
                            ..
                        } = stashed
                        {
                            popup::set_commit_summary_text(summary_textarea, &summary);
                            body_state.set_text(body.clone());
                            body_state.render_into(body_textarea, wrap);
                        }
                    };

                    // Restore the stashed commit editor, replacing its textarea content.
                    // This intentionally steals focus when generation completes.
                    if let Some(mut stashed) = self.pending_commit_popup.take() {
                        fill_commit(&mut stashed);
                        self.popup = stashed;
                    } else {
                        let mut summary_ta = popup::make_commit_summary_textarea();
                        summary_ta.insert_str(&summary);
                        let mut body_ta = popup::make_commit_body_textarea();
                        let body_state = popup::BodySoftWrap::from_text(body.clone());
                        if !body.is_empty() {
                            body_state.render_into(&mut body_ta, wrap);
                        }
                        self.popup = PopupState::CommitInput {
                            kind: popup::CommitInputKind::Commit,
                            summary_textarea: summary_ta,
                            body_textarea: body_ta,
                            body_state,
                            focus: popup::CommitInputFocus::Summary,
                            on_confirm: Box::new(|gui, msg| {
                                if !msg.is_empty() {
                                    let message = msg.to_string();
                                    gui.start_remote_op(
                                        "Commit",
                                        "Creating commit...",
                                        move |git| {
                                            git.create_commit(&message, false)?;
                                            Ok(())
                                        },
                                    );
                                }
                                Ok(())
                            }),
                        };
                    }
                    self.ai_commit_source = AiCommitSource::Staged;
                }
                Ok(None) => {
                    if let Some(stashed) = self.pending_commit_popup.take() {
                        self.saved_commit_popup = Some(stashed);
                    }
                    self.ai_commit_source = AiCommitSource::Staged;
                }
                Err(e) => {
                    if let Some(stashed) = self.pending_commit_popup.take() {
                        self.saved_commit_popup = Some(stashed);
                    }
                    self.ai_commit_source = AiCommitSource::Staged;
                    self.popup = PopupState::Message {
                        title: "AI generation failed".to_string(),
                        message: format!(
                            "{}\n\nYour commit draft was saved. Open the commit prompt again to restore it.",
                            e
                        ),
                        kind: MessageKind::Error,
                    };
                }
            }
        }
    }

    pub(super) fn receive_filter_paths(&mut self) {
        let result = {
            let Some(rx) = self.filter_paths_rx.as_ref() else {
                return;
            };
            match rx.try_recv() {
                Ok(v) => Some(v),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => None,
            }
        };
        self.filter_paths_rx = None;
        match result {
            Some(Ok(paths)) => {
                let items = paths
                    .into_iter()
                    .map(|path| crate::gui::popup::ListPickerItem {
                        value: path.clone(),
                        label: path,
                        category: String::new(),
                        description: None,
                    })
                    .collect();
                self.show_list_picker(
                    "Filter by path",
                    items,
                    "Path",
                    Box::new(|gui, path| {
                        gui.commit_filter.path =
                            crate::gui::controller::commits::nonempty_for_filter(path);
                        crate::gui::controller::commits::apply_commit_filters_and_focus(gui)
                    }),
                );
            }
            Some(Err(e)) => {
                self.popup = PopupState::Message {
                    title: "Filter by path".to_string(),
                    message: format!("Could not list paths: {e}"),
                    kind: MessageKind::Error,
                };
            }
            None => {}
        }
    }

    pub(super) fn receive_commit_page_results(&mut self) {
        while let Ok(result) = self.commit_page_rx.try_recv() {
            if result.generation != self.commit_page_generation {
                continue;
            }

            self.commit_page_loading = false;
            match result.result {
                Ok(commits) => {
                    let page_len = commits.len();
                    let mut model = self
                        .model
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if result.replace {
                        model.set_commits(commits);
                        drop(model);
                        self.context_mgr.set_selection_for(ContextId::Commits, 0);
                        self.range_select_anchor = None;
                        self.commit_history_complete = page_len < DEFAULT_COMMIT_LIMIT;
                    } else {
                        let mut seen: HashSet<String> =
                            model.commits.iter().map(|c| c.hash.clone()).collect();
                        let new_commits: Vec<_> = commits
                            .into_iter()
                            .filter(|c| seen.insert(c.hash.clone()))
                            .collect();
                        model.extend_commits(new_commits);
                        self.commit_history_complete = page_len < DEFAULT_COMMIT_LIMIT;
                        self.context_mgr.clamp_selections(&model);
                    }
                }
                Err(e) => {
                    self.commit_history_complete = true;
                    if self.popup == PopupState::None {
                        self.popup = PopupState::Message {
                            title: "Commits".to_string(),
                            message: format!("Could not load commits: {}", e),
                            kind: MessageKind::Error,
                        };
                    }
                }
            }
        }
    }

    /// Reload first page of commits for current filters (async).
    /// Used by ctrl-s so we don't block UI or refresh files/branches.
    pub(crate) fn reload_filtered_commits_async(&mut self) {
        self.reset_commit_pagination();
        if let Ok(mut model) = self.model.lock() {
            model.clear_commits();
        }
        self.context_mgr.set_selection_for(ContextId::Commits, 0);
        self.range_select_anchor = None;
        self.spawn_commit_page_load(DEFAULT_COMMIT_LIMIT, 0, true);
    }

    pub(super) fn spawn_commit_page_load(&mut self, limit: usize, skip: usize, replace: bool) {
        self.commit_page_loading = true;
        let generation = self.commit_page_generation;
        let git = Arc::clone(&self.git);
        let tx = self.commit_page_tx.clone();
        let filter = crate::git::commit::CommitFilter {
            branches: self.commit_filter.branch.clone(),
            path: self.commit_filter.path.clone(),
            authors: self.commit_filter.author.clone(),
        };

        std::thread::spawn(move || {
            let unpushed = git.unpushed_commit_hashes().unwrap_or_default();
            let result = git
                .load_filtered_commits_page(&filter, limit, skip)
                .map(|mut commits| {
                    crate::git::GitCommands::apply_unpushed_status(&mut commits, &unpushed);
                    commits
                });
            let _ = tx.send(CommitPageResult {
                generation,
                replace,
                result,
            });
        });
    }

    pub(super) fn maybe_request_more_commits(&mut self) {
        if self.context_mgr.active() != ContextId::Commits
            || self.commit_page_loading
            || self.commit_history_complete
        {
            return;
        }

        let len = {
            let model = self
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            model.commits.len()
        };
        if len == 0 {
            return;
        }
        if len < DEFAULT_COMMIT_LIMIT {
            self.commit_history_complete = true;
            return;
        }

        let selected = self.context_mgr.selected(ContextId::Commits);
        let viewport_end = self
            .context_mgr
            .scroll_offset(ContextId::Commits)
            .saturating_add(self.sidebar_visible_height());
        let near_loaded_tail = selected.saturating_add(COMMIT_PAGE_PREFETCH_THRESHOLD) >= len
            || viewport_end.saturating_add(COMMIT_PAGE_PREFETCH_THRESHOLD) >= len;
        if !near_loaded_tail {
            return;
        }

        self.spawn_commit_page_load(DEFAULT_COMMIT_LIMIT, len, false);
    }

    pub(super) fn reset_commit_pagination(&mut self) {
        self.commit_page_generation = self.commit_page_generation.wrapping_add(1);
        self.commit_page_loading = false;
        self.commit_history_complete = false;
    }

    /// Kick off a silent background `git fetch --all` if auto-fetch is enabled
    /// and the configured interval has elapsed since the last one. No popup,
    /// no status on the head branch — the user shouldn't be interrupted.
    pub(super) fn maybe_start_auto_fetch(&mut self) {
        if !self.config.user_config.git.auto_fetch {
            return;
        }
        let interval = self.config.user_config.refresher.fetch_interval;
        if interval == 0 {
            return;
        }
        if self.auto_fetch_in_flight {
            return;
        }
        // Don't race a user-initiated push/pull/fetch (even with
        // --no-write-fetch-head, concurrent network ops are wasteful and can
        // still contend on packed-refs / remote-tracking updates).
        if self.remote_op_label.is_some() {
            return;
        }
        let due = match self.last_auto_fetch_at {
            None => true, // first fetch happens immediately after startup
            Some(t) => t.elapsed().as_secs() >= interval,
        };
        if !due {
            return;
        }
        self.last_auto_fetch_at = Some(Instant::now());
        self.auto_fetch_in_flight = true;
        let git = Arc::clone(&self.git);
        let tx = self.auto_fetch_tx.clone();
        let cmd_log = self.command_log.clone();
        std::thread::spawn(move || {
            crate::os::cmd::set_thread_command_log(cmd_log);
            let result = git.fetch_all_background();
            let _ = tx.send(result);
        });
    }

    /// Collect auto-fetch completions. Success triggers a full refresh so the
    /// branches/commits panes reflect any new upstream commits. Failures
    /// (offline, auth prompt suppressed, etc.) are intentionally silent —
    /// surfacing them as popups every 60s would be worse than missing data.
    pub(super) fn receive_auto_fetch_results(&mut self) {
        while let Ok(result) = self.auto_fetch_rx.try_recv() {
            self.auto_fetch_in_flight = false;
            if matches!(result, Ok(true)) {
                self.needs_refresh = true;
            }
        }
    }

    /// Check for completed background remote operations (push, pull, fetch).
    pub(super) fn receive_remote_op_results(&mut self) {
        if let Ok(result) = self.remote_op_rx.try_recv() {
            self.remote_op_label = None;
            match result {
                Ok(head) => {
                    let was_checkout = self.pending_checkout_by_name.take().is_some();
                    if let Some(head) = head {
                        let mut model = self
                            .model
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        for branch in &mut model.branches {
                            branch.head = branch.name == head.branch_name;
                        }
                        model.head_hash = head.hash;
                        model.head_branch_name = head.branch_name;
                        self.needs_diff_refresh = true;
                    }
                    self.needs_refresh = true;
                    self.remote_op_success_at = Some(Instant::now());
                    if was_checkout {
                        // A pre-checkout stream must not overwrite confirmed HEAD
                        // or files. Its workers can finish, but drop their results.
                        self.initial_load_rx = None;
                        self.refresh_in_progress = false;
                        self.files_refresh_rx = None;
                        self.files_refresh_in_progress = false;
                        self.needs_files_refresh = false;
                        // Dispatch before the next paint/input wait, not after an
                        // idle 50/200ms poll. HEAD is already visible above.
                        self.start_background_refresh();
                    }
                }
                Err(e) => {
                    let err = format!("{}", e);
                    if let Some(name) = self
                        .pending_checkout_by_name
                        .take()
                        .filter(|_| is_checkout_ref_not_found(&err))
                    {
                        self.popup = PopupState::Confirm {
                            title: "Branch not found".to_string(),
                            message: format!(
                                "Branch not found. Create a new branch named {}?",
                                name
                            ),
                            on_confirm: Box::new(move |gui| {
                                gui.git.create_branch(&name)?;
                                gui.needs_refresh = true;
                                Ok(())
                            }),
                        };
                    } else {
                        self.pending_checkout_by_name = None;
                        self.popup = PopupState::Message {
                            title: "Error".to_string(),
                            message: err,
                            kind: MessageKind::Error,
                        };
                    }
                }
            }
        }
    }

    /// Kick off a status-only files refresh on a background thread.
    pub(super) fn start_files_refresh_async(&mut self) {
        if self.files_refresh_in_progress {
            return;
        }
        self.needs_files_refresh = false;
        self.files_refresh_in_progress = true;
        let git = Arc::clone(&self.git);
        let (tx, rx) = mpsc::channel();
        self.files_refresh_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(git.refresh_files_status_only());
        });
    }

    /// Stage/unstage paths on a background thread, then load status-only and
    /// apply via the files-refresh channel. Avoids racing a status refresh
    /// ahead of the git add/reset (which would clobber optimistic UI).
    pub(crate) fn enqueue_stage_then_refresh(&mut self, paths: Vec<String>, stage: bool) {
        if paths.is_empty() {
            return;
        }
        // Coalesce: if a light refresh is already in flight, just mark that we
        // need another after it lands; still run the git op so the index keeps
        // up with optimistic presses.
        let git = Arc::clone(&self.git);
        let should_send_files = !self.files_refresh_in_progress;
        if should_send_files {
            self.files_refresh_in_progress = true;
            let (tx, rx) = mpsc::channel();
            self.files_refresh_rx = Some(rx);
            std::thread::spawn(move || {
                if stage {
                    let _ = git.stage_files(&paths);
                } else {
                    let _ = git.unstage_files(&paths);
                }
                let _ = tx.send(git.refresh_files_status_only());
            });
        } else {
            self.needs_files_refresh = true;
            std::thread::spawn(move || {
                if stage {
                    let _ = git.stage_files(&paths);
                } else {
                    let _ = git.unstage_files(&paths);
                }
            });
        }
    }

    /// Stage/unstage everything on a background thread, then status-only refresh.
    pub(crate) fn enqueue_stage_all_then_refresh(&mut self, stage: bool) {
        let git = Arc::clone(&self.git);
        let should_send_files = !self.files_refresh_in_progress;
        if should_send_files {
            self.files_refresh_in_progress = true;
            let (tx, rx) = mpsc::channel();
            self.files_refresh_rx = Some(rx);
            std::thread::spawn(move || {
                if stage {
                    let _ = git.stage_all();
                } else {
                    let _ = git.unstage_all();
                }
                let _ = tx.send(git.refresh_files_status_only());
            });
        } else {
            self.needs_files_refresh = true;
            std::thread::spawn(move || {
                if stage {
                    let _ = git.stage_all();
                } else {
                    let _ = git.unstage_all();
                }
            });
        }
    }

    /// Apply a completed status-only files refresh.
    pub(super) fn receive_files_refresh(&mut self) {
        let Some(rx) = self.files_refresh_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(files)) => {
                self.files_refresh_rx = None;
                self.files_refresh_in_progress = false;
                {
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
                }
                // If more stage ops landed while we were refreshing, do another.
                if self.needs_files_refresh {
                    self.start_files_refresh_async();
                } else {
                    self.needs_diff_refresh = true;
                }
            }
            Ok(Err(err)) => {
                self.files_refresh_rx = None;
                self.files_refresh_in_progress = false;
                self.show_error("Refresh failed", err);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.files_refresh_rx = None;
                self.files_refresh_in_progress = false;
            }
        }
    }

    /// Execute a menu item action. If `override_idx` is Some, use that index;
    /// otherwise use the currently selected index.
    pub(super) fn execute_menu_action(&mut self, override_idx: Option<usize>) {
        let popup = std::mem::replace(&mut self.popup, PopupState::None);
        if let PopupState::Menu {
            ref items,
            selected,
            ..
        } = popup
        {
            let idx = override_idx.unwrap_or(selected);
            let has_action = items.get(idx).and_then(|i| i.action.as_ref()).is_some();
            if has_action {
                // Stash the menu so async actions can restore it via start_menu_async.
                self.pending_menu_popup = Some(popup);
                // Call the action from the stashed popup.
                let action_result = {
                    let menu = self.pending_menu_popup.as_ref().unwrap();
                    if let PopupState::Menu { items, .. } = menu {
                        let action = items[idx].action.as_ref().unwrap();
                        // SAFETY: We hold a shared ref to pending_menu_popup while calling
                        // action(self). The action may move the popup out of pending_menu_popup
                        // via start_menu_async (which calls .take()), but it won't invalidate
                        // the action pointer because the action is inside items which are moved
                        // as a whole. We use a raw pointer to avoid the borrow conflict.
                        let action_ptr = action as *const dyn Fn(&mut Gui) -> Result<()>;
                        unsafe { (*action_ptr)(self) }
                    } else {
                        Ok(())
                    }
                };
                match action_result {
                    Err(e) => {
                        self.pending_menu_popup = None;
                        self.popup = PopupState::Message {
                            title: "Error".to_string(),
                            message: format!("{}", e),
                            kind: MessageKind::Error,
                        };
                    }
                    Ok(()) => {
                        if self.pending_menu_popup.is_some() {
                            // Action didn't call start_menu_async — it was synchronous.
                            // Discard the stashed menu (popup stays None = menu closed).
                            self.pending_menu_popup = None;
                        }
                    }
                }
            }
        }
    }

    /// Handle results from background menu item operations.
    pub(super) fn receive_menu_async_results(&mut self) {
        if let Ok(result) = self.menu_async_rx.try_recv() {
            // Only process if the popup is still a menu with loading state.
            // If the user pressed Esc, the menu is already gone — discard the result.
            let is_menu_loading = matches!(
                &self.popup,
                PopupState::Menu {
                    loading_index: Some(_),
                    ..
                }
            );
            if !is_menu_loading {
                return;
            }
            match result {
                Ok(outcome) => {
                    // Close the menu
                    self.popup = PopupState::None;
                    match outcome {
                        popup::MenuAsyncResult::CopyToClipboard(url) => {
                            if let Err(e) = Platform::copy_to_clipboard(&url) {
                                self.popup = PopupState::Message {
                                    title: "Error".to_string(),
                                    message: format!("{}", e),
                                    kind: MessageKind::Error,
                                };
                            }
                        }
                        popup::MenuAsyncResult::OpenUrl(url) => {
                            if let Err(e) = Platform::open_file(&url) {
                                self.popup = PopupState::Message {
                                    title: "Error".to_string(),
                                    message: format!("{}", e),
                                    kind: MessageKind::Error,
                                };
                            }
                        }
                    }
                }
                Err(e) => {
                    self.popup = PopupState::Message {
                        title: "No PR found".to_string(),
                        message: format!("{}", e),
                        kind: MessageKind::Info,
                    };
                }
            }
        }
    }

    /// Run a remote operation (push/pull/fetch) on a background thread.
    /// Non-blocking: corner toast + branch-side label; input stays free (like AI commit).
    pub fn start_remote_op<F>(&mut self, title: &str, _message: &str, op: F)
    where
        F: FnOnce(&GitCommands) -> Result<()> + Send + 'static,
    {
        if self.remote_op_label.is_some() {
            return;
        }

        // Show operation label on the head branch in the sidebar (e.g. "Pushing", "Pulling").
        let label = match title {
            "Push" => "Pushing",
            "Pull" => "Pulling",
            "Fetch" => "Fetching",
            other => other,
        };
        self.remote_op_label = Some(label.to_string());
        self.remote_op_success_at = None;
        let git = Arc::clone(&self.git);
        let tx = self.remote_op_tx.clone();
        let is_checkout = self.pending_checkout_by_name.is_some();
        std::thread::spawn(move || {
            let result = op(&git).map(|()| {
                if is_checkout {
                    // Read on the worker, never on the UI thread. If this read
                    // fails, checkout still succeeded; the refresh is the fallback.
                    git.head_state().ok()
                } else {
                    None
                }
            });
            let _ = tx.send(result);
        });
    }

    /// Start an async operation for a menu item. Restores the menu popup with a
    /// loading spinner on the item at `index` and spawns a background thread.
    pub fn start_menu_async<F>(&mut self, index: usize, op: F)
    where
        F: FnOnce(&crate::git::GitCommands) -> Result<popup::MenuAsyncResult> + Send + 'static,
    {
        // Restore the menu popup (stashed by execute_menu_action) with loading_index set.
        if let Some(menu) = self.pending_menu_popup.take()
            && let PopupState::Menu {
                title,
                items,
                selected,
                ..
            } = menu
        {
            self.popup = PopupState::Menu {
                title,
                items,
                selected,
                loading_index: Some(index),
            };
        }
        let git = Arc::clone(&self.git);
        let tx = self.menu_async_tx.clone();
        std::thread::spawn(move || {
            let result = op(&git);
            let _ = tx.send(result);
        });
    }

    pub(crate) fn ai_commit_generation_active(&self) -> bool {
        self.ai_commit_job.is_some()
    }

    /// Start AI commit message generation on a background thread.
    pub fn start_ai_commit_generation(&mut self) {
        if self.ai_commit_generation_active() {
            return;
        }

        let git = Arc::clone(&self.git);
        let tx = self.ai_commit_tx.clone();
        let cmd = self.config.user_config.git.commit.generate_command.clone();
        let source = self.ai_commit_source.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        self.ai_commit_generation = self.ai_commit_generation.wrapping_add(1);
        let generation = self.ai_commit_generation;
        self.ai_commit_job = Some(AiCommitJob {
            generation,
            cancel,
            cancel_armed_at: None,
        });

        std::thread::spawn(move || {
            let result = match source {
                AiCommitSource::Staged => {
                    crate::git::ai_commit::generate_commit_message_cancellable(
                        git.repo_path(),
                        &cmd,
                        worker_cancel,
                    )
                }
                AiCommitSource::Commit(hash) => git.commit_diff(&hash).and_then(|diff| {
                    crate::git::ai_commit::generate_commit_message_from_diff_cancellable(
                        git.repo_path(),
                        &diff,
                        &cmd,
                        worker_cancel,
                    )
                }),
            };
            let _ = tx.send(AiCommitResult { generation, result });
        });
    }

    pub(super) fn begin_ai_commit_generation_ui(&mut self) {
        if self.ai_commit_generation_active() {
            return;
        }
        self.start_ai_commit_generation();
    }

    pub fn trigger_ai_commit_generation_from_editor(&mut self) {
        let generate_cmd = self.config.user_config.git.commit.generate_command.trim();
        if self.ai_commit_generation_active() {
            return;
        }
        if generate_cmd.is_empty() {
            self.popup = PopupState::Message {
                title: "AI generation unavailable".to_string(),
                message: "Set git.commit.generateCommand in your config first.".to_string(),
                kind: MessageKind::Error,
            };
            return;
        }

        self.ai_commit_source = match &self.popup {
            PopupState::CommitInput {
                kind: popup::CommitInputKind::Reword,
                ..
            } => {
                let selected = self.context_mgr.selected_active();
                self.model
                    .lock()
                    .unwrap()
                    .commits
                    .get(selected)
                    .map(|commit| AiCommitSource::Commit(commit.hash.clone()))
                    .unwrap_or(AiCommitSource::Staged)
            }
            _ => AiCommitSource::Staged,
        };
        let stashed = std::mem::replace(&mut self.popup, PopupState::None);
        self.pending_commit_popup = Some(stashed);
        self.begin_ai_commit_generation_ui();
    }

    pub(super) fn handle_ai_commit_cancel_key(&mut self, key: KeyEvent) -> bool {
        if key.code != KeyCode::Esc {
            return false;
        }

        let Some(job) = &mut self.ai_commit_job else {
            return false;
        };

        let now = Instant::now();
        let armed = job
            .cancel_armed_at
            .map(|armed_at| now.duration_since(armed_at) <= Duration::from_millis(900))
            .unwrap_or(false);

        if armed {
            job.cancel.store(true, Ordering::Relaxed);
            self.ai_commit_job = None;
            if let Some(stashed) = self.pending_commit_popup.take() {
                self.saved_commit_popup = Some(stashed);
            }
            true
        } else {
            job.cancel_armed_at = Some(now);
            false
        }
    }
}
