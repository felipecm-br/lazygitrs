//! Main loop, terminal/editor handoff, top-level rendering, interval refresh, event batching, ACP results and resize.

use super::*;

impl Gui {
    /// Get the currently active theme.
    pub fn active_theme(&self) -> crate::config::Theme {
        let mut theme = crate::config::COLOR_THEMES
            .get(self.current_theme_index)
            .map(|ct| ct.to_theme())
            .unwrap_or_default();

        let global_border = match &self.config.user_config.gui.border {
            crate::config::user_config::BorderConfig::Global(s) => s.clone(),
            crate::config::user_config::BorderConfig::Granular(map) => map
                .get("default")
                .cloned()
                .unwrap_or_else(|| "rounded".to_string()),
        };

        let (b_type, b_all) = crate::config::Theme::parse_border_type(&global_border);
        theme.border_type = b_type;
        theme.borders = b_all;

        if let crate::config::user_config::BorderConfig::Granular(map) =
            &self.config.user_config.gui.border
        {
            for (k, v) in map {
                let (bt, ba) = crate::config::Theme::parse_border_type(v);
                theme.panel_border_types.insert(k.clone(), bt);
                theme.panel_borders.insert(k.clone(), ba);
            }
        }

        theme
    }

    pub fn run(&mut self) -> Result<QuitReason> {
        let (mut terminal, keyboard_enhanced) = setup_terminal()?;
        // Continuous reader thread: reassembly needs reads between frames
        // (see `input` module). One event-per-frame is what leaked ↑ as 'A'.
        let input = InputReader::spawn();

        // Sync layout dimensions with actual terminal size so mouse handling
        // uses the correct geometry from the very first frame.
        let size = terminal.size()?;
        self.sync_layout_size(size.width, size.height);

        let mut keyboard_enhanced = keyboard_enhanced;
        let result = self.main_loop(&mut terminal, &input, &mut keyboard_enhanced);

        self.diff_preview_cache.clear();
        self.diff_view.reset_keep_prefs();
        self.displayed_diff_key.clear();
        restore_terminal(&mut terminal, keyboard_enhanced)?;
        result?;
        Ok(self.quit_reason)
    }

    /// Queue an editor launch. Terminal editors (`suspend: true`) run after the
    /// current frame via [`Self::run_suspended_editor`]; GUI editors spawn now.
    pub(crate) fn launch_editor(
        &mut self,
        launch: crate::config::user_config::EditorLaunch,
    ) -> Result<()> {
        if launch.suspend {
            self.editor_handoff.pending_launch = Some(launch);
            Ok(())
        } else {
            crate::os::cmd::log_command(&launch.display_cmd());
            std::process::Command::new(&launch.program)
                .args(&launch.args)
                .spawn()?;
            Ok(())
        }
    }

    pub(super) fn run_suspended_editor(
        &mut self,
        terminal: &mut Term,
        input: &InputReader,
        keyboard_enhanced: &mut bool,
        launch: crate::config::user_config::EditorLaunch,
    ) -> Result<()> {
        use crossterm::terminal::{Clear, ClearType};
        use std::io::Write as _;
        use std::process::Stdio;

        // Pause the input thread before leaving the alt screen — crossterm's
        // event reader is process-wide and would steal hx/nvim's keystrokes.
        input.pause();
        input.drain();
        // Dropping the cached protocols ensures they are freshly transmitted
        // after returning from another alternate screen.
        self.diff_preview_cache.clear();
        self.diff_view.reset_keep_prefs();
        self.displayed_diff_key.clear();
        self.needs_diff_refresh = true;
        restore_terminal(terminal, *keyboard_enhanced)?;

        // LeaveAlternateScreen restores the previous buffer; wipe the primary
        // screen so Helix doesn't paint over leftover lazygit frames.
        // Use the same TUI output handle (/dev/tty when stdout is piped).
        {
            let mut out = crate::os::tty::open_tui_output()
                .context("Failed to open terminal output for editor handoff")?;
            execute!(
                out,
                Clear(ClearType::All),
                Clear(ClearType::Purge),
                cursor::MoveTo(0, 0),
                cursor::Show
            )?;
            out.flush()?;
        }

        crate::os::cmd::log_command(&launch.display_cmd());
        let status = std::process::Command::new(&launch.program)
            .args(&launch.args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();

        // Re-enter the TUI regardless of editor exit status.
        let (new_term, new_enhanced) = setup_terminal()?;
        *terminal = new_term;
        *keyboard_enhanced = new_enhanced;

        input.drain();
        input.resume();

        // Force a full redraw next frame.
        terminal.clear()?;
        let size = terminal.size()?;
        self.sync_layout_size(size.width, size.height);

        match status {
            Ok(st) if st.success() => Ok(()),
            Ok(st) => {
                self.show_error(
                    "Editor failed",
                    anyhow::anyhow!("exited with status {}", st.code().unwrap_or(-1)),
                );
                Ok(())
            }
            Err(err) => {
                self.show_error("Failed to launch editor", err.into());
                Ok(())
            }
        }
    }

    pub fn render_ui(&mut self, frame: &mut Frame, theme: &crate::config::Theme) {
        if self.rebase_mode.active {
            presentation::rebase_mode::render(frame, &mut self.rebase_mode, theme);
            // Render popup overlay on top of rebase mode
            if self.popup != PopupState::None {
                views::render_popup(
                    frame,
                    &self.popup,
                    frame.area(),
                    self.spinner_frame,
                    theme,
                    false,
                    !self
                        .config
                        .user_config
                        .git
                        .commit
                        .generate_command
                        .trim()
                        .is_empty(),
                );
            } else if self.ai_commit_generation_active() {
                views::render_loading_overlay(
                    frame,
                    frame.area(),
                    self.spinner_frame,
                    theme,
                    "AI Commit",
                    "Generating commit message...",
                    Some(("Esc esc", "cancel")),
                );
            } else if let Some(label) = self.remote_op_label.as_deref() {
                views::render_loading_overlay(
                    frame,
                    frame.area(),
                    self.spinner_frame,
                    theme,
                    label,
                    "",
                    None,
                );
            }
        } else if self.diff_mode.active {
            let diff_loading_show = self.diff_loading
                && self
                    .diff_loading_since
                    .map(|t| t.elapsed() >= std::time::Duration::from_millis(50))
                    .unwrap_or(false);
            presentation::diff_mode::render(
                frame,
                &mut self.diff_mode,
                &mut self.diff_view,
                theme,
                self.diff_loading,
                diff_loading_show,
                &mut self.commit_list_cache,
                self.layout.side_panel_ratio,
                self.screen_mode,
                self.spinner_frame,
            );
            // Render popup overlay on top of diff mode (for ? help, errors, etc.)
            if self.popup != PopupState::None {
                views::render_popup(
                    frame,
                    &self.popup,
                    frame.area(),
                    self.spinner_frame,
                    theme,
                    false,
                    !self
                        .config
                        .user_config
                        .git
                        .commit
                        .generate_command
                        .trim()
                        .is_empty(),
                );
            } else if self.ai_commit_generation_active() {
                views::render_loading_overlay(
                    frame,
                    frame.area(),
                    self.spinner_frame,
                    theme,
                    "AI Commit",
                    "Generating commit message...",
                    Some(("Esc esc", "cancel")),
                );
            } else if let Some(label) = self.remote_op_label.as_deref() {
                views::render_loading_overlay(
                    frame,
                    frame.area(),
                    self.spinner_frame,
                    theme,
                    label,
                    "",
                    None,
                );
            }
        } else {
            let model = self
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let search_state = if self.search_active || !self.search_query.is_empty() {
                Some((
                    self.search_query.as_str(),
                    self.search_matches.len(),
                    self.search_match_idx,
                ))
            } else {
                None
            };
            let cmd_log = self
                .command_log
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut active_commit_filters: Vec<String> = self
                .commit_filter
                .branch
                .iter()
                .map(|branch| format!("branch: {branch}"))
                .collect();
            if let Some(path) = self.commit_filter.path.as_deref() {
                active_commit_filters.push(format!("path: {path}"));
            }
            if !self.commit_filter.author.is_empty() {
                active_commit_filters
                    .push(format!("author: {}", self.commit_filter.author.join(", ")));
            }
            views::render(
                frame,
                &model,
                &mut self.context_mgr,
                &self.layout,
                &self.popup,
                &self.config,
                theme,
                &mut self.diff_view,
                &mut self.commit_list_cache,
                self.screen_mode,
                self.show_file_tree,
                &self.file_tree_nodes,
                &self.collapsed_dirs,
                self.diff_focused,
                search_state,
                self.search_textarea.as_ref(),
                &cmd_log,
                self.show_command_log,
                &active_commit_filters,
                self.show_commit_file_tree,
                &self.commit_file_tree_nodes,
                &self.commit_files_collapsed_dirs,
                &self.commit_files_hash,
                &self.commit_files_message,
                &self.branch_commits_name,
                &self.remote_branches_name,
                self.sub_commits_parent_context,
                self.spinner_frame,
                self.remote_op_label.as_deref(),
                self.remote_op_success_at
                    .map(|t| t.elapsed() < std::time::Duration::from_secs(5))
                    .unwrap_or(false),
                &self.cherry_pick_clipboard,
                self.range_select_anchor,
                self.diff_loading,
                // Only show "Loading diff..." text after a short delay to avoid jitter on fast loads
                self.diff_loading
                    && self
                        .diff_loading_since
                        .map(|t| t.elapsed() >= std::time::Duration::from_millis(50))
                        .unwrap_or(false),
                &self.commit_stats_cache,
                &self.commit_messages_cache,
                &mut self.commit_details_scroll,
                &mut self.commit_details_scroll_hash,
                self.show_commit_details,
                false,
                !self
                    .config
                    .user_config
                    .git
                    .commit
                    .generate_command
                    .trim()
                    .is_empty(),
                self.grab_column_hovered,
                self.all_branches_log,
            );
            if self.popup == PopupState::None {
                if self.ai_commit_generation_active() {
                    views::render_loading_overlay(
                        frame,
                        frame.area(),
                        self.spinner_frame,
                        theme,
                        "AI Commit",
                        "Generating commit message...",
                        Some(("Esc esc", "cancel")),
                    );
                } else if let Some(label) = self.remote_op_label.as_deref() {
                    views::render_loading_overlay(
                        frame,
                        frame.area(),
                        self.spinner_frame,
                        theme,
                        label,
                        "",
                        None,
                    );
                }
            }
        }
    }

    /// Whether the outer frame is drawn at this terminal size. Too small a
    /// terminal skips it rather than leave the app no room.
    pub(super) fn outer_frame_active(&self, width: u16, height: u16) -> bool {
        self.outer_frame && width > 2 && height > 2
    }

    /// Track the terminal size, minus the outer frame when it is drawn, so
    /// layout and mouse hit-testing work in the app's own coordinates.
    pub(super) fn sync_layout_size(&mut self, width: u16, height: u16) {
        if self.outer_frame_active(width, height) {
            self.layout.update_size(width - 2, height - 2);
        } else {
            self.layout.update_size(width, height);
        }
    }

    /// Move a mouse event into the app's coordinates when the outer frame is
    /// drawn; events on the frame itself land on the nearest inner cell.
    pub(super) fn mouse_in_frame(&self, mut mouse: MouseEvent) -> MouseEvent {
        if self.outer_frame_active(self.layout.width + 2, self.layout.height + 2) {
            mouse.column = mouse.column.saturating_sub(1).min(self.layout.width - 1);
            mouse.row = mouse.row.saturating_sub(1).min(self.layout.height - 1);
        }
        mouse
    }

    pub(super) fn main_loop(
        &mut self,
        terminal: &mut Term,
        input: &InputReader,
        keyboard_enhanced: &mut bool,
    ) -> Result<()> {
        // A session-wide toggle (e.g. exported by tmux) wins over the config;
        // `LAZYGITRS_BORDER_GRADIENT=off` disables it.
        let border_gradient = gradient::parse_stops(
            &std::env::var("LAZYGITRS_BORDER_GRADIENT")
                .unwrap_or_else(|_| self.config.user_config.gui.border_gradient.clone()),
        );
        // `Some(title)` when the app is drawn inside a frame; the off-screen
        // terminal holds the app rendered at the frame's inner size.
        let outer_frame = self
            .outer_frame
            .then(|| std::env::var("LAZYGITRS_BORDER_TITLE").ok());
        let mut offscreen = match self.outer_frame {
            true => Some(Terminal::new(TestBackend::new(1, 1))?),
            false => None,
        };
        loop {
            // Drain any model parts that have arrived from the background load.
            if let Some(rx) = &self.initial_load_rx {
                let mut got_files = false;
                let mut got_rebase_in_progress = false;
                let received_before = self.initial_load_received;
                while let Ok(part) = rx.try_recv() {
                    let mut model = self
                        .model
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    match part {
                        ModelPart::Files(v) => {
                            model.set_files(v);
                            got_files = true;
                        }
                        ModelPart::Branches(v) => model.branches = v,
                        ModelPart::Commits(v) => {
                            // Stream already applies the active filter when one is set
                            // (`load_model_streaming(commit_filter)`), so always take it.
                            self.commit_history_complete = v.len() < DEFAULT_COMMIT_LIMIT;
                            model.set_commits(v);
                        }
                        ModelPart::Stash(v) => model.stash_entries = v,
                        ModelPart::Remotes(v) => model.remotes = v,
                        ModelPart::Tags(v) => model.tags = v,
                        ModelPart::Worktrees(v) => model.worktrees = v,
                        ModelPart::Submodules(v) => model.submodules = v,
                        ModelPart::Reflog(v) => model.reflog_commits = v,
                        ModelPart::DiffStats { added, deleted } => {
                            model.total_additions = added;
                            model.total_deletions = deleted;
                        }
                        ModelPart::RepoStatus {
                            is_rebasing,
                            is_merging,
                            is_cherry_picking,
                            is_bisecting,
                            rebase_onto_hash,
                        } => {
                            model.is_rebasing = is_rebasing;
                            model.is_merging = is_merging;
                            model.is_cherry_picking = is_cherry_picking;
                            model.is_bisecting = is_bisecting;
                            model.rebase_onto_hash = rebase_onto_hash;
                            if is_rebasing {
                                got_rebase_in_progress = true;
                            }
                        }
                        ModelPart::Head { hash, branch_name } => {
                            model.head_hash = hash;
                            model.head_branch_name = branch_name;
                        }
                        ModelPart::RepoUrl(url) => model.repo_url = url,
                        ModelPart::Contributors(c) => model.contributors = c,
                    }
                    self.initial_load_received += 1;
                }
                // Enter the InProgress rebase view as soon as we know a rebase
                // is on disk — don't wait for a future `refresh()` tick (focus
                // event / auto-refresh interval), which is what made the view
                // pop in ~0.8s after the default screen appeared on startup.
                if got_rebase_in_progress
                    && !self.rebase_mode.active
                    && !self.rebase_mode.in_progress_dismissed
                {
                    self.sync_rebase_progress_view();
                }
                // Rebuild file tree if files arrived this frame.
                if got_files {
                    if self.show_file_tree {
                        let model = self
                            .model
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        self.file_tree_nodes = build_file_tree(&model.files, &self.collapsed_dirs);
                        self.context_mgr.files_list_len_override = Some(self.file_tree_nodes.len());
                    }
                    if self.start_in_diff {
                        self.start_in_diff = false;
                        self.diff_focused = true;
                        self.screen_mode = ScreenMode::Full;
                        if !self.diff_view.hunk_starts.is_empty() {
                            self.diff_view.selected_revert_hunk = Some(0);
                        }

                        if let Some(ref target_file) = self.start_file_filter {
                            let model = self
                                .model
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if self.show_file_tree {
                                if let Some(idx) = self
                                    .file_tree_nodes
                                    .iter()
                                    .position(|n| !n.is_dir && n.path == *target_file)
                                {
                                    self.context_mgr
                                        .set_selected(crate::gui::context::ContextId::Files, idx);
                                }
                            } else {
                                if let Some(idx) =
                                    model.files.iter().position(|f| f.name == *target_file)
                                {
                                    self.context_mgr.set_selected(
                                        crate::gui::context::ContextId::Files,
                                        idx + 1,
                                    );
                                }
                            }
                        }
                    }
                }
                // Trigger a diff reload when new data arrived THIS frame.
                // (A cumulative `> 0` check here re-requested the same diff
                // every frame for the whole stream, and each request's
                // generation bump invalidated the in-flight result — diffs
                // only settled once streaming finished.)
                if self.initial_load_received > received_before {
                    self.needs_diff_refresh = true;
                }
                // All parts received — done loading.
                if self.initial_load_received >= MODEL_PART_COUNT {
                    self.initial_load_rx = None;
                    let was_refresh = self.refresh_in_progress;
                    self.refresh_in_progress = false;
                    // Leave needs_refresh alone: if another mutation arrived
                    // mid-refresh it will re-queue on the next frame.
                    self.needs_files_refresh = false;
                    self.needs_diff_refresh = true;
                    self.last_refresh_at = Instant::now();
                    // Re-apply filters / selection-dependent views after stream
                    // completes (initial load and background refresh). Needed so
                    // startup `-f/--filter` takes effect once commits arrive.
                    if (was_refresh || self.commit_filter.is_active())
                        && let Err(err) = self.after_model_refresh()
                    {
                        self.show_error("Refresh failed", err);
                    }
                }
            }

            // Request diff loading on background thread if selection changed
            self.maybe_request_diff();

            // Check for completed background diff results
            self.receive_diff_results();
            self.receive_acp_results();

            // Warm the preview cache with neighbor diffs while idle
            self.maybe_prefetch_diffs();

            // Queue details for only the commit where navigation has settled.
            self.maybe_request_commit_details();

            // Check for AI commit message generation results
            self.receive_ai_commit_results();

            // Check for completed incremental commit page loads
            self.receive_commit_page_results();
            self.receive_filter_paths();
            self.maybe_request_more_commits();

            // Check for completed background remote operations
            self.receive_remote_op_results();

            // Check for completed auto-fetch and kick off a new one if due
            self.receive_auto_fetch_results();
            self.maybe_start_auto_fetch();

            // Check for completed background menu item operations
            self.receive_menu_async_results();

            // Spinner phase from wall-clock time, so its speed no longer depends
            // on how often the loop wakes (16ms busy … 1s idle). One step per
            // 16ms keeps the old busy-mode glyph period (`frame / 8` = 128ms).
            self.spinner_frame = (self.started_at.elapsed().as_millis() / 16) as usize;

            // Render
            let theme = self.active_theme();
            self.diff_view.image_preview_hidden = self.popup != PopupState::None
                || (self.show_command_log
                    && self
                        .diff_view
                        .image_preview
                        .as_ref()
                        .is_some_and(|p| !p.uses_placeholders()))
                || self.ai_commit_generation_active()
                || self.remote_op_label.is_some()
                || self.rebase_mode.active
                || self.patch_building.active;
            terminal.draw(|frame| {
                let area = frame.area();
                let framed = outer_frame
                    .as_ref()
                    .zip(offscreen.as_mut())
                    .filter(|_| self.outer_frame_active(area.width, area.height));
                if let Some((title, offscreen)) = framed {
                    // Draw the app one cell in from every edge: render it
                    // off-screen at the inner size, then copy it into place.
                    let inner = Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2);
                    offscreen.backend_mut().resize(inner.width, inner.height);
                    let Ok(done) = offscreen.draw(|f| self.render_ui(f, &theme));
                    let buf = frame.buffer_mut();
                    for y in 0..inner.height {
                        for x in 0..inner.width {
                            buf[(inner.x + x, inner.y + y)] = done.buffer[(x, y)].clone();
                        }
                    }
                    let backend = offscreen.backend();
                    if backend.cursor_visible() {
                        let pos = backend.cursor_position();
                        frame.set_cursor_position((inner.x + pos.x, inner.y + pos.y));
                    }
                    let mut block = Block::bordered()
                        .border_type(BorderType::Rounded)
                        .border_style(theme.inactive_border);
                    if let Some(title) = title {
                        block = block.title(format!(" {title} "));
                    }
                    frame.render_widget(block, area);
                    if let Some(stops) = &border_gradient {
                        gradient::paint_frame(frame.buffer_mut(), area, stops);
                    }
                } else {
                    self.render_ui(frame, &theme);
                }

                if let (Some(stops), Some(fg)) = (&border_gradient, theme.active_border.fg) {
                    gradient::paint_matching(frame.buffer_mut(), fg, stops);
                }
            })?;

            // One batch per frame. Reassembly lives on the reader thread so a
            // split ESC [ A cannot leak as Char('A') → amend between frames.
            // Keep the frame budget tight while anything animated/async is up.
            let events = input.wait_batch(self.input_wait_timeout());
            self.handle_event_batch(events);

            // Terminal editors: leave alt screen, run hx/nvim, restore TUI.
            if let Some(launch) = self.editor_handoff.pending_launch.take() {
                self.run_suspended_editor(terminal, input, keyboard_enhanced, launch)?;
            }

            if self.should_quit {
                break;
            }

            self.arm_interval_refresh();

            // Kick off a non-blocking full refresh (same streaming path as
            // initial load). Avoids freezing the UI for ~1s on commit/reword.
            if self.needs_refresh && !self.refresh_in_progress && self.initial_load_rx.is_none() {
                self.start_background_refresh();
            } else if self.needs_files_refresh
                && !self.refresh_in_progress
                && !self.files_refresh_in_progress
            {
                // Status-only async refresh — Space spam stays responsive.
                self.start_files_refresh_async();
            }

            // Apply completed light files refresh without blocking input.
            self.receive_files_refresh();
            self.receive_interval_files_refresh();

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    /// Background auto-refresh on `refresher.refreshInterval` (0 = disabled).
    ///
    /// `last_refresh_at` is stamped when a refresh *completes*, so while one is
    /// in flight the interval still reads as elapsed. Re-arming then queued a
    /// second, identical full reload the moment the first landed — every tick
    /// ran the whole model load twice. User mutations still set
    /// `needs_refresh` directly and re-queue as before.
    pub(super) fn arm_interval_refresh(&mut self) {
        let refresh_interval = self.config.user_config.refresher.refresh_interval;
        if !(self.config.user_config.git.auto_refresh
            && refresh_interval > 0
            && !self.refresh_in_progress
            && self.initial_load_rx.is_none()
            && self.interval_files_rx.is_none()
            && self.last_refresh_at.elapsed().as_secs() >= refresh_interval)
        {
            return;
        }
        // Refs/HEAD/stash/rebase state unchanged: only the working tree can
        // have moved, so reload the Files scope (4 git calls, not ~26).
        let fingerprint = self.git.refs_fingerprint();
        if fingerprint.is_some() && fingerprint == self.refs_fingerprint {
            self.start_interval_files_refresh();
        } else {
            self.needs_refresh = true;
        }
    }

    /// Files scope of the interval refresh: status with per-file diff stats and
    /// the repo-wide totals, on a background thread.
    pub(super) fn start_interval_files_refresh(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.interval_files_rx = Some(rx);
        // Restart the interval now so the tick doesn't re-fire while in flight.
        self.last_refresh_at = Instant::now();
        let git = Arc::clone(&self.git);
        std::thread::spawn(move || {
            let result = git
                .load_files()
                .and_then(|files| Ok((files, git.diff_shortstat()?)));
            let _ = tx.send(result);
        });
    }

    pub(super) fn receive_interval_files_refresh(&mut self) {
        let Some(rx) = self.interval_files_rx.as_ref() else {
            return;
        };
        let (files, (added, deleted)) = match rx.try_recv() {
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.interval_files_rx = None;
                return;
            }
            Ok(Err(_)) => {
                // Background tick: stay quiet, like the full refresh does for
                // transient failures; the next tick retries.
                self.interval_files_rx = None;
                return;
            }
            Ok(Ok(v)) => v,
        };
        self.interval_files_rx = None;
        // A stage/unstage refresh is in flight with fresher, optimistic state;
        // don't clobber it with a status taken before that git op.
        if self.files_refresh_in_progress || self.needs_files_refresh {
            return;
        }
        {
            let mut model = self
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            model.set_files(files);
            model.total_additions = added;
            model.total_deletions = deleted;
            if self.show_file_tree {
                self.file_tree_nodes = build_file_tree(&model.files, &self.collapsed_dirs);
                self.context_mgr.files_list_len_override = Some(self.file_tree_nodes.len());
            } else {
                self.file_tree_nodes.clear();
                self.context_mgr.files_list_len_override = None;
            }
        }
        // Same as the full refresh: working-tree diffs may have changed.
        self.diff_preview_cache.retain_immutable();
        self.needs_diff_refresh = true;
    }

    pub(super) fn input_wait_timeout(&self) -> Duration {
        /// Poll cadence while background work the user is waiting on is in flight.
        const BUSY: Duration = Duration::from_millis(16);
        /// Cadence for a short while after input, so debounced and async results
        /// of that input (commit details, prefetched diffs, pages) paint promptly.
        const ACTIVE: Duration = Duration::from_millis(50);
        const ACTIVE_WINDOW: Duration = Duration::from_secs(2);
        /// Upper bound when idle. Each wake repaints, so this caps idle redraws
        /// at 1/s (previously 20/s with autoRefresh on, only to watch a timer
        /// that fires every 10s). Unprompted results (auto-fetch, AI notes)
        /// still appear within this bound.
        const IDLE: Duration = Duration::from_secs(1);

        if self.ai_commit_generation_active()
            || self.remote_op_label.is_some()
            || self.needs_refresh
            || self.needs_files_refresh
            || self.diff_loading
            || self.initial_load_rx.is_some()
            || self.refresh_in_progress
            || self.files_refresh_in_progress
            || self.interval_files_rx.is_some()
        {
            return BUSY;
        }
        if self.last_input_at.elapsed() < ACTIVE_WINDOW || self.has_idle_animation() {
            return ACTIVE;
        }
        // Wake in time for the next interval refresh, not on a fixed tick.
        let refresh_interval = self.config.user_config.refresher.refresh_interval;
        if self.config.user_config.git.auto_refresh && refresh_interval > 0 {
            let due = Duration::from_secs(refresh_interval)
                .saturating_sub(self.last_refresh_at.elapsed());
            return due.clamp(BUSY, IDLE);
        }
        IDLE
    }

    /// A spinner is on screen that isn't covered by a busy flag above: a menu
    /// item running async work, or a review note awaiting the AI's reply.
    pub(super) fn has_idle_animation(&self) -> bool {
        if matches!(
            self.popup,
            PopupState::Menu {
                loading_index: Some(_),
                ..
            }
        ) {
            return true;
        }
        self.diff_view.notes_visible
            && self.diff_view.lines.iter().any(|line| {
                line.comment_notes
                    .iter()
                    .any(|n| n.status == crate::pager::NoteStatus::Sent)
            })
    }

    /// Apply one batch of terminal events before the next paint.
    pub(super) fn handle_event_batch(&mut self, events: Vec<Event>) {
        if !events.is_empty() {
            self.last_input_at = Instant::now();
        }
        for event in events {
            match event {
                Event::Key(key) if key.kind == crossterm::event::KeyEventKind::Press => {
                    if let Err(err) = self.handle_key(key) {
                        self.show_error("Command failed", err);
                    }
                }
                Event::Mouse(mouse) => self.handle_mouse(self.mouse_in_frame(mouse)),
                Event::Resize(w, h) => self.handle_resize(w, h),
                Event::FocusGained if self.config.user_config.git.auto_refresh => {
                    self.needs_refresh = true;
                }
                Event::Paste(data) => self.handle_paste(data),
                _ => {}
            }
            if self.should_quit {
                break;
            }
        }
    }

    pub(super) fn receive_acp_results(&mut self) {
        while let Ok(event) = self.acp.rx.try_recv() {
            match event {
                crate::acp::AcpEvent::ApplyNotes(ctx) => {
                    self.apply_acp_notes(ctx);
                }
                crate::acp::AcpEvent::RefreshNotes => {
                    self.diff_view.load_notes(self.git.repo_path());
                }
                crate::acp::AcpEvent::Navigate(nav) => {
                    // NOTE: Implemented the navigation endpoint (inspired by hunk).
                    let mut found_idx = None;

                    if nav.combined_view == Some(true) {
                        found_idx = Some(0);
                    } else if self.show_file_tree {
                        found_idx = self
                            .file_tree_nodes
                            .iter()
                            .position(|n| !n.is_dir && n.path == nav.file_path);
                    } else {
                        let model = self
                            .model
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if let Some(idx) = model.files.iter().position(|f| f.name == nav.file_path)
                        {
                            found_idx = Some(idx + 1);
                        }
                    }

                    if let Some(idx) = found_idx {
                        self.context_mgr
                            .set_selected(crate::gui::context::ContextId::Files, idx);
                        self.needs_diff_refresh = true;
                        self.diff_focused = true;
                        self.pending_navigation = Some(nav.clone());
                    }
                    self.command_log
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(format!(
                            "Navigating to {} (hunk: {:?}, line: {:?}, combined: {:?})",
                            nav.file_path, nav.hunk_number, nav.line, nav.combined_view
                        ));
                }
            }
        }
    }

    pub(super) fn apply_acp_notes(&mut self, ctx: crate::acp::AgentContext) {
        let mut lines_file = crate::pager::notes_store::load(self.git.repo_path());
        let mut last_added_note_id = None;

        for file_ctx in ctx.files {
            for ann in file_ctx.annotations {
                let note_text = if let Some(r) = &ann.rationale {
                    format!("{}\n{}", ann.summary, r)
                } else {
                    ann.summary.clone()
                };

                let id = ann.id.clone().unwrap_or_else(|| {
                    let line = ann
                        .new_range
                        .map(|(s, _)| s)
                        .or_else(|| ann.old_range.map(|(s, _)| s))
                        .unwrap_or(0);
                    format!("{}-{}-agent", file_ctx.path, line)
                });

                let (line, panel) = if let Some((start_line, _)) = ann.new_range {
                    (start_line, "New")
                } else if let Some((start_line, _)) = ann.old_range {
                    (start_line, "Old")
                } else {
                    continue;
                };

                // Mark any existing user notes on the same line as addressed.
                for entry in lines_file.notes.iter_mut() {
                    if entry.file == file_ctx.path
                        && entry.line == line
                        && entry.source == crate::pager::NoteSource::User
                        && entry.status != crate::pager::NoteStatus::Addressed
                    {
                        entry.status = crate::pager::NoteStatus::Addressed;
                    }
                }

                // Remove old agent note with same id, then push the new one.
                lines_file.notes.retain(|c| c.id != id);
                lines_file
                    .notes
                    .push(crate::pager::notes_store::LinesEntry {
                        id: id.clone(),
                        file: file_ctx.path.clone(),
                        line,
                        panel: panel.to_string(),
                        comment: note_text,
                        rationale: ann.rationale.clone(),
                        source: crate::pager::NoteSource::Agent,
                        author: ann.author.unwrap_or_else(|| "agent".to_string()),
                        created_at: ann.created_at.unwrap_or_else(crate::pager::now_iso8601),
                        status: crate::pager::NoteStatus::Addressed,
                        tags: ann.tags.unwrap_or_default(),
                        confidence: ann.confidence,
                    });

                last_added_note_id = Some(id);
            }
        }

        crate::pager::notes_store::save(self.git.repo_path(), lines_file);
        self.diff_view.load_notes(self.git.repo_path());

        if let Some(ref note_id) = last_added_note_id {
            self.diff_view.selected_note = Some(note_id.clone());
            self.diff_focused = true;
            self.ensure_note_visible(note_id);
        }
    }

    pub(super) fn handle_resize(&mut self, w: u16, h: u16) {
        self.sync_layout_size(w, h);
        let w = self.layout.width;
        // Re-flow any active commit-message textarea to the new width so
        // wrapping stays consistent with what the user sees.
        let popup_width = (w * 60 / 100).clamp(30, 60).min(w);
        let popup_inner = popup_width.saturating_sub(4) as usize;
        let config_width = self.config.user_config.git.commit.auto_wrap_width;
        let effective_width = if config_width > 0 {
            popup_inner.min(config_width)
        } else {
            popup_inner
        };
        match &mut self.popup {
            PopupState::Input {
                textarea,
                is_commit: true,
                ..
            } if effective_width > 0 => {
                auto_wrap_textarea(textarea, effective_width);
            }
            PopupState::Input {
                textarea,
                is_commit: false,
                ..
            } => {
                let raw: String = textarea.lines().join("");
                if popup_inner > 0 && !raw.is_empty() {
                    let mut new_ta = popup::make_textarea("");
                    new_ta.insert_str(&raw);
                    soft_wrap_textarea(&mut new_ta, popup_inner);
                    *textarea = new_ta;
                }
            }
            PopupState::CommitInput {
                body_textarea,
                body_state,
                ..
            } if effective_width > 0 => {
                body_state.render_into(body_textarea, effective_width);
            }
            _ => {}
        }
    }
}
