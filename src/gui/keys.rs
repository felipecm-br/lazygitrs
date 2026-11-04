//! Keyboard input: layered key dispatch (see `keymap.rs`), inline edit, diff-focused keys, paste and diff file actions.

use super::*;

impl Gui {
    /// Repo-level keybindings that work regardless of which panel is focused
    /// (including the diff panel). Returns Ok(true) if the key was consumed.
    pub(super) fn try_handle_global_repo_keys(&mut self, key: KeyEvent) -> Result<bool> {
        let kb = self.config.user_config.keybinding.clone();
        if matches_key(key, &kb.universal.push_files) || matches_key(key, &kb.universal.pull_files)
        {
            controller::remotes::handle_key(self, key, &kb)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn is_return_to_cockpit_key(&self, key: KeyEvent) -> bool {
        let keybindings = &self.config.user_config.keybinding;

        // When launched from Cockpit, Esc at a root panel (with no active selection/clipboard) returns to cockpit
        if std::env::var("LAZYGITRS_COCKPIT").is_ok()
            && !self.diff_focused
            && !self.diff_mode.active
            && self.search_query.is_empty()
            && self.range_select_anchor.is_none()
            && self.cherry_pick_clipboard.is_empty()
            && !self.patch_building.active
            && matches!(
                self.context_mgr.active(),
                ContextId::Files
                    | ContextId::Commits
                    | ContextId::Branches
                    | ContextId::Stash
                    | ContextId::Status
            )
            && matches_key(key, &keybindings.universal.return_key)
        {
            return true;
        }

        // Optional explicit return_to_cockpit binding if configured by user (disabled by default)
        let can_use_return_key = matches!(self.context_mgr.active(), ContextId::Files)
            || self.diff_focused
            || self.diff_mode.active;

        if can_use_return_key
            && !keybindings.universal.return_to_cockpit.is_empty()
            && matches_key(key, &keybindings.universal.return_to_cockpit)
        {
            return true;
        }

        false
    }

    /// Split-resize shortcuts shared by the main UI and compare mode.
    /// Supports both `<c-w>` prefix chords (`<c-w> h`, `<c-w> l`, `<c-w> k`, `<c-w> j`, `<c-w> =`)
    /// and legacy `Alt` bindings (`<a-h>`, `<a-l>`, `<a-k>`, `<a-j>`, `<a-r>`).
    pub(super) fn try_handle_panel_resize_key(&mut self, key: KeyEvent) -> bool {
        const STEP: f64 = 0.05;
        let portrait =
            layout::should_use_portrait(self.layout.width, self.layout.height, self.screen_mode);

        if self.window_prefix_armed {
            self.window_prefix_armed = false;
            match (key.code, key.modifiers) {
                (KeyCode::Char('h'), KeyModifiers::NONE | KeyModifiers::CONTROL)
                | (KeyCode::Left, KeyModifiers::NONE) => {
                    self.layout.side_panel_ratio =
                        (self.layout.side_panel_ratio - STEP).clamp(0.0, 1.0);
                }
                (KeyCode::Char('l'), KeyModifiers::NONE | KeyModifiers::CONTROL)
                | (KeyCode::Right, KeyModifiers::NONE) => {
                    self.layout.side_panel_ratio =
                        (self.layout.side_panel_ratio + STEP).clamp(0.0, 1.0);
                }
                (KeyCode::Char('k'), KeyModifiers::NONE | KeyModifiers::CONTROL)
                | (KeyCode::Up, KeyModifiers::NONE) => {
                    self.layout.side_panel_ratio = if portrait { 0.0 } else { 1.0 };
                }
                (KeyCode::Char('j'), KeyModifiers::NONE | KeyModifiers::CONTROL)
                | (KeyCode::Down, KeyModifiers::NONE) => {
                    self.layout.side_panel_ratio = if portrait { 1.0 } else { 0.0 };
                }
                (KeyCode::Char('=' | '0' | 'r'), KeyModifiers::NONE | KeyModifiers::CONTROL) => {
                    self.layout.side_panel_ratio = self.config.user_config.gui.side_panel_width;
                }
                _ => {}
            }
            return true;
        }

        if key.modifiers == KeyModifiers::CONTROL
            && matches!(key.code, KeyCode::Char('w') | KeyCode::Char('W'))
        {
            self.window_prefix_armed = true;
            return true;
        }

        let keybindings = &self.config.user_config.keybinding;
        // Side-panel resize: orientation-aware.
        // Portrait (vertical stack): side on top, diff on bottom.
        //   Alt+h/l → shrink/expand by step
        //   Alt+k → diff pane full (ratio 0.0), Alt+j → side pane full (ratio 1.0)
        // Landscape (horizontal split): side on left, diff on right.
        //   Alt+h/l → shrink/expand by step, Alt+k → side full, Alt+j → main full
        let shrink_key = matches_key(key, &keybindings.universal.shrink_side_panel);
        let expand_key = matches_key(key, &keybindings.universal.expand_side_panel);
        if shrink_key || expand_key {
            let delta = if shrink_key { -STEP } else { STEP };
            self.layout.side_panel_ratio = (self.layout.side_panel_ratio + delta).clamp(0.0, 1.0);
            return true;
        }
        if matches_key(key, &keybindings.universal.side_panel_full) {
            // Alt+k: diff full in portrait, side full in landscape
            self.layout.side_panel_ratio = if portrait { 0.0 } else { 1.0 };
            return true;
        }
        if matches_key(key, &keybindings.universal.main_panel_full) {
            // Alt+j: side full in portrait, main full in landscape
            self.layout.side_panel_ratio = if portrait { 1.0 } else { 0.0 };
            return true;
        }
        if matches_key(key, &keybindings.universal.reset_side_panel) {
            self.layout.side_panel_ratio = self.config.user_config.gui.side_panel_width;
            return true;
        }

        false
    }

    /// Compare layout shortcuts run only after its text-input handlers.
    pub(super) fn try_handle_layout_key(&mut self, mut key: KeyEvent) -> bool {
        if self.try_handle_panel_resize_key(key) {
            return true;
        }
        // Terminals may report shifted punctuation with or without SHIFT.
        if matches!(key.code, KeyCode::Char('+') | KeyCode::Char('_')) {
            key.modifiers.remove(KeyModifiers::SHIFT);
        }
        let keybindings = &self.config.user_config.keybinding;
        if matches_key(key, &keybindings.universal.next_screen_mode) {
            self.next_screen_mode();
            return true;
        }
        if matches_key(key, &keybindings.universal.prev_screen_mode) {
            self.prev_screen_mode();
            return true;
        }
        false
    }

    /// Number-key window jump. Pressing the parent window's number from a
    /// sub-context (e.g. CommitFiles) exits the sub-context first.
    pub(super) fn jump_to_window_from_key(&mut self, window: SideWindow) {
        // If we're in a sub-context (CommitFiles), pressing the parent window's
        // number key should exit the sub-context first.
        if self.context_mgr.active() == ContextId::CommitFiles && window == SideWindow::Commits {
            self.context_mgr.set_active(ContextId::Commits);
            return;
        }
        if self.context_mgr.active() == ContextId::StashFiles && window == SideWindow::Stash {
            self.context_mgr.set_active(ContextId::Stash);
            return;
        }
        if (self.context_mgr.active() == ContextId::BranchCommits
            || self.context_mgr.active() == ContextId::BranchCommitFiles)
            && window == SideWindow::Branches
        {
            if self.context_mgr.active() == ContextId::BranchCommitFiles {
                self.context_mgr.set_active(ContextId::BranchCommits);
            } else {
                self.context_mgr.set_active(ContextId::Branches);
            }
            return;
        }
        if self.context_mgr.active() == ContextId::RemoteBranches && window == SideWindow::Branches
        {
            self.context_mgr.set_active(ContextId::Remotes);
            return;
        }
        self.context_mgr.jump_to_window(window);
    }

    pub(super) fn move_selection_by(&mut self, delta: isize) {
        let model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.context_mgr.move_selection(delta, &model);
    }

    /// Rebase options key: re-enter the rebase view while rebasing, or show
    /// the options menu while merging / cherry-picking. Falls through
    /// (`Ok(false)`) otherwise.
    pub(super) fn try_rebase_options_key(&mut self) -> Result<bool> {
        let model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let is_rebasing = model.is_rebasing;
        let is_merging = model.is_merging;
        let is_cherry_picking = model.is_cherry_picking;
        drop(model);

        if is_rebasing {
            if !self.rebase_mode.active {
                self.rebase_mode.in_progress_dismissed = false;
                self.sync_rebase_progress_view();
            }
            return Ok(true);
        }
        if is_merging || is_cherry_picking {
            self.show_rebase_options_menu(false, is_merging, is_cherry_picking)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.handle_ai_commit_cancel_key(key) {
            return Ok(());
        }

        if has_command_modifier(key.modifiers) && matches!(key.code, KeyCode::Char(_)) {
            return Ok(());
        }

        // Popup takes priority
        if self.popup != PopupState::None {
            return self.handle_popup_key(key);
        }

        // Search input mode takes priority
        if self.search_active {
            return self.handle_search_key(key);
        }

        // Terminal-level shortcuts such as Cmd+1/Cmd+2 must not fall through
        // to character-only application shortcuts if the terminal forwards
        // their enhanced-keyboard events.
        if has_command_modifier(key.modifiers) {
            return Ok(());
        }

        // Rebase mode takes priority over everything
        if self.rebase_mode.active {
            return controller::rebase_mode::handle_key(self, key);
        }

        // Diff mode takes priority over normal UI
        if self.diff_mode.active {
            return controller::diff_mode::handle_key(self, key);
        }

        if !matches_key(
            key,
            &self.config.user_config.keybinding.universal.return_key,
        ) {
            self.root_esc_armed_at = None;
        }

        if self.try_handle_panel_resize_key(key) {
            return Ok(());
        }

        // Inline edit takes priority
        if self.diff_view.inline_edit.is_some() {
            return self.handle_inline_edit_key(key);
        }

        // Precedence lives in data: see `keymap.rs`.
        if keymap::dispatch(self, key, keymap::PRE_DIFF_FOCUS)? {
            return Ok(());
        }
        if self.diff_focused {
            return self.handle_diff_focused_key(key);
        }

        if keymap::dispatch(self, key, keymap::NORMAL)? {
            return Ok(());
        }

        // Context-specific keybindings
        self.handle_context_key(key)?;

        // Custom commands (lowest priority — checked after built-in bindings)
        controller::custom_commands::try_handle_key(self, key)?;

        Ok(())
    }

    pub(super) fn handle_inline_edit_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        use crossterm::event::{KeyCode, KeyModifiers};
        let keybindings = &self.config.user_config.keybinding;

        if key.code == KeyCode::Esc {
            self.diff_view.inline_edit = None;
            self.diff_view.selected_note = None;
            return Ok(());
        }

        let is_save = matches_key(key, &keybindings.universal.return_key)
            || (key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL));

        if is_save {
            let edit = self.diff_view.inline_edit.as_ref().unwrap();
            let comment = edit.textarea.lines().join("\n");
            let file_path = self.diff_view.file_at_line(edit.line_idx).to_string();

            let is_modified = {
                let model = self
                    .model
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                model.files.iter().any(|f| f.name == file_path)
            };

            if !is_modified {
                self.show_error(
                    "Erro",
                    anyhow::anyhow!("Não é permitido salvar notas em arquivos não modificados."),
                );
                self.diff_view.inline_edit = None;
                return Ok(());
            }

            let line_num = self
                .diff_view
                .file_line_number(edit.line_idx, edit.panel)
                .unwrap_or(0);

            let panel_str = match edit.panel {
                crate::pager::side_by_side::DiffPanel::Old => "Old",
                crate::pager::side_by_side::DiffPanel::New => "New",
            };

            let mut lines_file = crate::pager::notes_store::load(self.git.repo_path());

            // Editing existing note: update by id, or remove if emptied.
            // Track the note id so we can select it after saving.
            let saved_note_id = if !edit.editing_id.is_empty() {
                if comment.trim().is_empty() {
                    lines_file.notes.retain(|c| c.id != edit.editing_id);
                    None
                } else if let Some(entry) = lines_file
                    .notes
                    .iter_mut()
                    .find(|c| c.id == edit.editing_id)
                {
                    entry.comment = comment;
                    Some(edit.editing_id.clone())
                } else {
                    None
                }
            } else if !comment.trim().is_empty() {
                // New note: generate unique id.
                let id = format!(
                    "{}-{}-{}-{}",
                    file_path,
                    line_num,
                    panel_str,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                );
                lines_file
                    .notes
                    .push(crate::pager::notes_store::LinesEntry::new_user(
                        id.clone(),
                        file_path,
                        line_num,
                        panel_str,
                        comment,
                    ));
                Some(id)
            } else {
                None
            };

            crate::pager::notes_store::save(self.git.repo_path(), lines_file);
            self.diff_view.load_notes(self.git.repo_path());
            self.diff_view.inline_edit = None;
            self.diff_view.selected_note = saved_note_id;
            return Ok(());
        }

        if let Some(ref mut edit) = self.diff_view.inline_edit {
            edit.textarea.input(key);
        }
        Ok(())
    }

    pub(super) fn handle_context_key(&mut self, key: KeyEvent) -> Result<()> {
        let keybindings = self.config.user_config.keybinding.clone();
        let active = self.context_mgr.active();

        match active {
            ContextId::Files => {
                controller::files::handle_key(self, key, &keybindings)?;
            }
            ContextId::Branches => {
                controller::branches::handle_key(self, key, &keybindings)?;
            }
            ContextId::Commits => {
                controller::commits::handle_key(self, key, &keybindings)?;
            }
            ContextId::Reflog => {
                controller::reflog::handle_key(self, key, &keybindings)?;
            }
            ContextId::Stash => {
                controller::stash::handle_key(self, key, &keybindings)?;
            }
            ContextId::Remotes => {
                controller::remotes::handle_key(self, key, &keybindings)?;
            }
            ContextId::Tags => {
                controller::tags::handle_key(self, key, &keybindings)?;
            }
            ContextId::Status => {
                controller::status::handle_key(self, key, &keybindings)?;
            }
            ContextId::Worktrees => {
                controller::worktrees::handle_key(self, key, &keybindings)?;
            }
            ContextId::Submodules => {
                controller::submodules::handle_key(self, key, &keybindings)?;
            }
            ContextId::RemoteBranches => {
                controller::remote_branches::handle_key(self, key, &keybindings)?;
            }
            ContextId::CommitFiles | ContextId::StashFiles | ContextId::BranchCommitFiles => {
                controller::commit_files::handle_key(self, key, &keybindings)?;
            }
            ContextId::BranchCommits => {
                controller::branch_commits::handle_key(self, key, &keybindings)?;
            }
            _ => {}
        }

        Ok(())
    }

    pub(super) fn handle_diff_focused_search_key(&mut self, key: KeyEvent) -> Result<()> {
        if let Some(ref mut ta) = self.diff_view.search_textarea {
            match key.code {
                KeyCode::Esc => {
                    self.diff_view.dismiss_search();
                }
                KeyCode::Enter => {
                    self.diff_view.dismiss_search();
                    if !self.diff_view.search_matches.is_empty() {
                        self.diff_view.search_match_idx = 0;
                        self.diff_view.scroll_to_current_match();
                    }
                }
                _ => {
                    textarea_input(ta, key);
                    self.diff_view.search_query = ta.lines().join("");
                    self.diff_view.update_search();
                }
            }
        }
        Ok(())
    }

    pub(super) fn handle_diff_focused_key(&mut self, key: KeyEvent) -> Result<()> {
        // Diff search input mode takes priority
        if self.diff_view.search_active {
            return self.handle_diff_focused_search_key(key);
        }

        // Note actions take priority over text selection
        if self.diff_view.selected_note.is_some() {
            match key.code {
                KeyCode::Char('y') => {
                    self.copy_selected_note()?;
                    return Ok(());
                }
                KeyCode::Enter | KeyCode::Char('o') => {
                    if let Some(ref note_id) = self.diff_view.selected_note.clone() {
                        let lines_file = crate::pager::notes_store::load(self.git.repo_path());
                        if let Some(note) = lines_file.notes.iter().find(|n| &n.id == note_id) {
                            let content = if let Some(ref rationale) = note.rationale {
                                format!("{}\n\n{}", note.comment, rationale)
                            } else {
                                note.comment.clone()
                            };
                            let author = if note.author.is_empty() {
                                "unknown".to_string()
                            } else {
                                note.author.clone()
                            };
                            let full_details = format!(
                                "File: {} (line {})\nAuthor: {}\nCreated: {}\nStatus: {:?}\n\n{}",
                                note.file, note.line, author, note.created_at, note.status, content
                            );
                            self.popup = PopupState::Message {
                                title: format!("Note Details [{}]", note.file),
                                message: full_details,
                                kind: crate::gui::popup::MessageKind::Info,
                            };
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('e') => {
                    if let Some(ref note_id) = self.diff_view.selected_note.clone() {
                        for (i, dl) in self.diff_view.lines.iter().enumerate() {
                            if let Some(note) = dl.comment_notes.iter().find(|n| &n.id == note_id) {
                                // Don't allow editing AI notes
                                if note.source == crate::pager::NoteSource::Agent {
                                    return Ok(());
                                }
                                let panel = if note.is_old {
                                    crate::pager::side_by_side::DiffPanel::Old
                                } else {
                                    crate::pager::side_by_side::DiffPanel::New
                                };
                                self.open_inline_note_editor(i, panel, Some(note.id.clone()));
                                break;
                            }
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('d') => {
                    if let Some(ref note_id) = self.diff_view.selected_note.clone()
                        && self.ensure_note_visible(note_id)
                    {
                        self.delete_note(note_id.clone());
                    }
                    return Ok(());
                }
                KeyCode::Char('S') => {
                    if let Some(ref note_id) = self.diff_view.selected_note.clone() {
                        self.notify_ai_for_note(note_id.clone());
                    }
                    return Ok(());
                }
                KeyCode::Char('A') | KeyCode::Char('R') => {
                    self.notify_ai_for_all_notes();
                    return Ok(());
                }
                KeyCode::Char('r') => {
                    if let Some(ref note_id) = self.diff_view.selected_note.clone() {
                        self.reset_note_status(note_id);
                    }
                    return Ok(());
                }
                KeyCode::Char('c') => {
                    if let Some(ref note_id) = self.diff_view.selected_note.clone() {
                        for (i, dl) in self.diff_view.lines.iter().enumerate() {
                            if dl.comment_notes.iter().any(|n| &n.id == note_id) {
                                let panel = if dl.new_line.is_some() {
                                    crate::pager::side_by_side::DiffPanel::New
                                } else {
                                    crate::pager::side_by_side::DiffPanel::Old
                                };
                                self.open_inline_note_editor(i, panel, None);
                                break;
                            }
                        }
                    }
                    return Ok(());
                }
                KeyCode::Esc => {
                    self.diff_view.selected_note = None;
                    return Ok(());
                }
                _ => {}
            }
        }

        // Handle text selection keys first (y to copy, e to edit, Esc to dismiss)
        if self.diff_view.selection.is_some() {
            let is_click = self.diff_view.selection.as_ref().unwrap().is_click;
            let can_edit = self.diff_view.file_exists_on_disk;
            match key.code {
                KeyCode::Char('e') if can_edit => {
                    let sel_ref = self.diff_view.selection.as_ref().unwrap();
                    let line = sel_ref.edit_line_number;
                    // Compute column from terminal position using the same layout as the mouse handler
                    let (top_row, top_col, _, _) = sel_ref.normalized();
                    let main_panel = self.compute_main_panel_rect();
                    let pl = DiffPanelLayout::compute(main_panel, &self.diff_view);
                    let (content_start, _) = pl.content_range(sel_ref.panel);
                    let column = if top_col >= content_start {
                        (top_col - content_start) as usize + self.diff_view.horizontal_scroll + 1
                    } else {
                        1
                    };
                    // Resolve the actual filename for multi-file diffs
                    let (line_idx, line_panel) = if top_row >= pl.inner_y {
                        self.diff_view
                            .line_chunk_panel_at_row(top_row, &pl, sel_ref.panel)
                            .map(|(line_idx, _, panel)| (line_idx, panel))
                            .unwrap_or_else(|| {
                                (
                                    self.diff_view.fallback_line_idx_for_row(top_row, &pl),
                                    sel_ref.panel,
                                )
                            })
                    } else {
                        (0, sel_ref.panel)
                    };
                    let filename = self.diff_view.file_at_line(line_idx).to_string();
                    self.diff_view.selection = None;
                    let abs_path = self.git.repo_path().join(&filename);
                    if !filename.is_empty() && abs_path.exists() {
                        let abs_path = abs_path.to_string_lossy().to_string();
                        let ln =
                            line.or_else(|| self.diff_view.file_line_number(line_idx, line_panel));
                        if let Ok(launch) =
                            self.config
                                .user_config
                                .os
                                .plan_edit(&abs_path, ln, Some(column))
                        {
                            let _ = self.launch_editor(launch);
                        }
                    }
                    return Ok(());
                }
                KeyCode::Char('y') if !is_click => {
                    let text = self.diff_view.selection.as_ref().unwrap().text.clone();
                    self.diff_view.selection = None;
                    if !text.is_empty() {
                        crate::os::platform::Platform::copy_to_clipboard(&text)?;
                    }
                    return Ok(());
                }
                KeyCode::Esc => {
                    self.diff_view.selection = None;
                    return Ok(());
                }
                _ => {
                    self.diff_view.selection = None;
                    if is_click {
                        // Don't propagate click-state dismissal as a real keypress
                        return Ok(());
                    }
                }
            }
        }

        // Push/Pull are global — they fire even when the diff panel is focused.
        if self.try_handle_global_repo_keys(key)? {
            return Ok(());
        }

        let keybindings = &self.config.user_config.keybinding;

        // Toggle between Working Tree (Files) and Latest Commit (Commits at HEAD)
        if matches_key(key, &keybindings.universal.toggle_working_tree_and_head) {
            self.toggle_working_tree_and_head();
            return Ok(());
        }

        // Quick AI Review Note
        if matches_key(key, &keybindings.universal.quick_ai_note) {
            self.open_quick_ai_note_prompt();
            return Ok(());
        }

        // e / o on the diff panel (no active selection) mirror the Files tab:
        // open the working-tree file in the editor (at the first changed hunk)
        // or in the default program.
        if matches_key(key, &keybindings.universal.edit) {
            self.open_diff_file_in_editor();
            return Ok(());
        }
        if matches_key(key, &keybindings.universal.open_file) {
            self.open_diff_file_in_default_program();
            return Ok(());
        }

        // Screen mode cycling works even when diff is focused
        if matches_key(key, &keybindings.universal.next_screen_mode) {
            self.next_screen_mode();
            return Ok(());
        }
        if matches_key(key, &keybindings.universal.prev_screen_mode) {
            self.prev_screen_mode();
            return Ok(());
        }

        // Start diff content search (/)
        if matches_key(key, &keybindings.universal.start_search) {
            self.diff_view.start_search();
            return Ok(());
        }

        // n/N to navigate diff search matches
        if !self.diff_view.search_query.is_empty() {
            if matches_key(key, &keybindings.universal.next_match) {
                self.diff_view.next_search_match();
                return Ok(());
            }
            if matches_key(key, &keybindings.universal.prev_match) {
                self.diff_view.prev_search_match();
                return Ok(());
            }
        }

        if matches_key(key, &keybindings.universal.revert_block) {
            if self.context_mgr.active() == ContextId::Files {
                let hunk_idx = self
                    .diff_view
                    .selected_revert_hunk
                    .or(self.diff_view.hovered_revert_hunk);
                if let Some(hunk_idx) = hunk_idx {
                    self.diff_view.selected_revert_hunk = Some(hunk_idx);
                    self.show_hunk_context_menu(hunk_idx);
                }
            }
            return Ok(());
        }
        if matches_key(key, &keybindings.universal.undo_revert_block) {
            if self.context_mgr.active() == ContextId::Files
                && !self.diff_view.revert_undo_stack.is_empty()
                && let Err(err) = self.undo_last_revert_block()
            {
                self.popup = PopupState::Message {
                    title: "Undo revert failed".to_string(),
                    message: format!("{}", err),
                    kind: MessageKind::Error,
                };
            }
            return Ok(());
        }

        // Toggle command log (;)
        if key.code == KeyCode::Char(';') {
            self.show_command_log = !self.show_command_log;
            self.persist_command_log_visibility();
            return Ok(());
        }

        // Help popup
        if key.code == KeyCode::Char('?') {
            self.show_diff_command_palette();
            return Ok(());
        }

        // Number keys 1-5 to jump to sidebar panels (unfocus diff)
        // Use set_window instead of jump_to_window to avoid cycling tabs,
        // since the user is "arriving" from diff focus, not pressing the same key again.
        if let KeyCode::Char(c @ '1'..='5') = key.code {
            let n = c.to_digit(10).unwrap();
            if let Some(window) = SideWindow::from_number(n) {
                self.diff_focused = false;
                self.context_mgr.set_window(window);
                return Ok(());
            }
        }

        // Configured H/L scroll keybindings
        if matches_key(key, &keybindings.universal.scroll_left) {
            self.diff_view.scroll_left(4);
            return Ok(());
        }
        if matches_key(key, &keybindings.universal.scroll_right) {
            self.diff_view.scroll_right(4);
            return Ok(());
        }

        if self.is_return_to_cockpit_key(key) {
            self.quit_reason = QuitReason::ReturnToCockpit;
            self.should_quit = true;
            return Ok(());
        }

        if matches_key(key, &keybindings.universal.quit)
            || matches_key(key, &keybindings.universal.quit_alt1)
        {
            self.quit_reason = QuitReason::Normal;
            self.should_quit = true;
            return Ok(());
        }

        if matches_key(key, &keybindings.universal.return_key) {
            self.root_esc_armed_at = None;
            if self.diff_view.selected_note.is_some() {
                self.diff_view.selected_note = None;
            } else if !self.diff_view.search_query.is_empty() {
                self.diff_view.clear_search();
            } else {
                self.diff_view.selected_revert_hunk = None;
                self.diff_focused = false;
                if self.started_in_diff_mode {
                    self.started_in_diff_mode = false;
                    self.screen_mode = ScreenMode::Normal;
                    self.layout.side_panel_ratio = 0.25;
                }
            }
            return Ok(());
        }

        if matches_key(key, &keybindings.universal.revert_block) {
            if self.context_mgr.active() == ContextId::Files {
                let hunk_idx = self
                    .diff_view
                    .selected_revert_hunk
                    .or(self.diff_view.hovered_revert_hunk);
                if let Some(hunk_idx) = hunk_idx {
                    self.diff_view.selected_revert_hunk = Some(hunk_idx);
                    self.show_hunk_context_menu(hunk_idx);
                }
            }
            return Ok(());
        }
        if matches_key(key, &keybindings.universal.undo_revert_block) {
            if self.context_mgr.active() == ContextId::Files
                && !self.diff_view.revert_undo_stack.is_empty()
                && let Err(err) = self.undo_last_revert_block()
            {
                self.popup = PopupState::Message {
                    title: "Undo revert failed".to_string(),
                    message: format!("{}", err),
                    kind: MessageKind::Error,
                };
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Char(' ') if key.modifiers.is_empty() => {
                if self.context_mgr.active() == ContextId::Files {
                    let hunk_idx = self
                        .diff_view
                        .selected_revert_hunk
                        .or(self.diff_view.hovered_revert_hunk)
                        .or({
                            if !self.diff_view.hunk_starts.is_empty() {
                                Some(0)
                            } else {
                                None
                            }
                        });

                    if let Some(hunk_idx) = hunk_idx {
                        if hunk_idx < self.diff_view.hunk_starts.len() {
                            let is_staged = self.diff_view.is_staged_hunk(hunk_idx);
                            if !is_staged {
                                // Staging moves the triage forward: advance
                                // to the next hunk (no wrap on the last one).
                                self.diff_view.selected_revert_hunk = Some(hunk_idx);
                                if self.stage_selected_file_hunk(hunk_idx).is_ok() {
                                    self.diff_view.advance_revert_hunk();
                                }
                            } else {
                                // Unstaging is a correction: keep the cursor
                                // on the hunk so the marker flip is visible.
                                let _ = self.unstage_selected_file_hunk(hunk_idx);
                                self.diff_view.selected_revert_hunk = Some(hunk_idx);
                            }
                        } else if self.diff_view.hunk_starts.is_empty() {
                            let _ = self.stage_selected_file_hunk(0);
                        }
                    } else if self.diff_view.hunk_starts.is_empty() {
                        let _ = self.stage_selected_file_hunk(0);
                    }
                }
                return Ok(());
            }
            KeyCode::Char('c') if !self.diff_view.lines.is_empty() => {
                let line_idx = if let Some((idx, _)) = self.diff_view.hovered_line {
                    idx
                } else {
                    // First visible line in diff
                    self.diff_view.scroll_offset
                };
                let panel = if let Some((_, p)) = self.diff_view.hovered_line {
                    p
                } else {
                    crate::pager::side_by_side::DiffPanel::New
                };
                self.open_inline_note_editor(line_idx, panel, None);
            }
            // n/N: cycle to next/previous note
            KeyCode::Char('n') => {
                self.cycle_note(true);
            }
            KeyCode::Char('N') => {
                self.cycle_note(false);
            }
            // S / A / R: Send all pending review notes to AI agent
            KeyCode::Char('S') | KeyCode::Char('A') | KeyCode::Char('R') => {
                self.notify_ai_for_all_notes();
                return Ok(());
            }
            KeyCode::Char(',') => {
                let active = self.context_mgr.active();
                let new_idx = match active {
                    ContextId::Files if self.show_file_tree => {
                        let selected = self.context_mgr.selected(ContextId::Files);
                        crate::model::file_tree::find_parent_idx(&self.file_tree_nodes, selected)
                            .map(|idx| (ContextId::Files, idx))
                    }
                    ContextId::CommitFiles
                    | ContextId::StashFiles
                    | ContextId::BranchCommitFiles
                        if self.show_commit_file_tree =>
                    {
                        let selected = self.context_mgr.selected(active);
                        crate::model::file_tree::find_parent_idx(
                            &self.commit_file_tree_nodes,
                            selected,
                        )
                        .map(|idx| (active, idx))
                    }
                    _ => None,
                };
                if let Some((ctx, idx)) = new_idx {
                    self.context_mgr.set_selected(ctx, idx);
                    self.needs_diff_refresh = true;
                }
                return Ok(());
            }
            KeyCode::Char('.') => {
                let active = self.context_mgr.active();
                let new_idx = match active {
                    ContextId::Files if self.show_file_tree => {
                        let selected = self.context_mgr.selected(ContextId::Files);
                        crate::model::file_tree::find_first_child_idx(
                            &self.file_tree_nodes,
                            selected,
                        )
                        .map(|idx| (ContextId::Files, idx))
                    }
                    ContextId::CommitFiles
                    | ContextId::StashFiles
                    | ContextId::BranchCommitFiles
                        if self.show_commit_file_tree =>
                    {
                        let selected = self.context_mgr.selected(active);
                        crate::model::file_tree::find_first_child_idx(
                            &self.commit_file_tree_nodes,
                            selected,
                        )
                        .map(|idx| (active, idx))
                    }
                    _ => None,
                };
                if let Some((ctx, idx)) = new_idx {
                    self.context_mgr.set_selected(ctx, idx);
                    self.needs_diff_refresh = true;
                }
                return Ok(());
            }
            KeyCode::Char('>') => {
                let active = self.context_mgr.active();
                let new_idx = match active {
                    ContextId::Files if self.show_file_tree => {
                        let selected = self.context_mgr.selected(ContextId::Files);
                        crate::model::file_tree::find_next_sibling_idx(
                            &self.file_tree_nodes,
                            selected,
                        )
                        .map(|idx| (ContextId::Files, idx))
                    }
                    ContextId::CommitFiles
                    | ContextId::StashFiles
                    | ContextId::BranchCommitFiles
                        if self.show_commit_file_tree =>
                    {
                        let selected = self.context_mgr.selected(active);
                        crate::model::file_tree::find_next_sibling_idx(
                            &self.commit_file_tree_nodes,
                            selected,
                        )
                        .map(|idx| (active, idx))
                    }
                    _ => None,
                };
                if let Some((ctx, idx)) = new_idx {
                    self.context_mgr.set_selected(ctx, idx);
                    self.needs_diff_refresh = true;
                }
                return Ok(());
            }
            KeyCode::Char('<') => {
                let active = self.context_mgr.active();
                let new_idx = match active {
                    ContextId::Files if self.show_file_tree => {
                        let selected = self.context_mgr.selected(ContextId::Files);
                        crate::model::file_tree::find_prev_sibling_idx(
                            &self.file_tree_nodes,
                            selected,
                        )
                        .map(|idx| (ContextId::Files, idx))
                    }
                    ContextId::CommitFiles
                    | ContextId::StashFiles
                    | ContextId::BranchCommitFiles
                        if self.show_commit_file_tree =>
                    {
                        let selected = self.context_mgr.selected(active);
                        crate::model::file_tree::find_prev_sibling_idx(
                            &self.commit_file_tree_nodes,
                            selected,
                        )
                        .map(|idx| (active, idx))
                    }
                    _ => None,
                };
                if let Some((ctx, idx)) = new_idx {
                    self.context_mgr.set_selected(ctx, idx);
                    self.needs_diff_refresh = true;
                }
                return Ok(());
            }
            // j/k/up/down scroll line by line
            KeyCode::Char('j') | KeyCode::Down => {
                self.diff_view.scroll_down(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.diff_view.scroll_up(1);
            }
            // h/l/left/right scroll horizontally
            KeyCode::Char('h') | KeyCode::Left => {
                self.diff_view.scroll_left(4);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.diff_view.scroll_right(4);
            }
            // { and } jump between hunks. In Files context they also select
            // the hunk as the revert target so the marker glyph turns
            // accent-coloured; the scroll motion stays the same as plain
            // hunk navigation (always jumps, even if already in viewport).
            KeyCode::Char('}') => {
                if self.context_mgr.active() == ContextId::Files {
                    self.diff_view.cycle_next_revert_hunk();
                } else {
                    self.diff_view.next_hunk();
                }
            }
            KeyCode::Char('{') => {
                if self.context_mgr.active() == ContextId::Files {
                    self.diff_view.cycle_prev_revert_hunk();
                } else {
                    self.diff_view.prev_hunk();
                }
            }
            // [ and ] toggle old-only / new-only view
            KeyCode::Char(']') => {
                use crate::pager::side_by_side::DiffSideView;
                self.diff_view.side_view = match self.diff_view.side_view {
                    DiffSideView::NewOnly => DiffSideView::Both,
                    _ => DiffSideView::NewOnly,
                };
            }
            KeyCode::Char('[') => {
                use crate::pager::side_by_side::DiffSideView;
                self.diff_view.side_view = match self.diff_view.side_view {
                    DiffSideView::OldOnly => DiffSideView::Both,
                    _ => DiffSideView::OldOnly,
                };
            }
            // z toggles line wrapping
            KeyCode::Char('z') => {
                self.diff_view.wrap = !self.diff_view.wrap;
                self.diff_view.horizontal_scroll = 0;
                self.persist_diff_line_wrap();
            }
            // t toggles note visibility
            KeyCode::Char('t') => {
                self.diff_view.notes_visible = !self.diff_view.notes_visible;
                if !self.diff_view.notes_visible {
                    self.diff_view.selected_note = None;
                }
            }
            // Page up/down for larger scrolling
            KeyCode::PageDown => {
                self.diff_view.scroll_down(20);
            }
            KeyCode::PageUp => {
                self.diff_view.scroll_up(20);
            }
            // g/G for top/bottom
            KeyCode::Char('g') => {
                self.diff_view.scroll_offset = 0;
            }
            KeyCode::Char('G') => {
                let max = self.diff_view.lines.len().saturating_sub(1);
                self.diff_view.scroll_offset = max;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn copy_selected_note(&mut self) -> Result<()> {
        if let Some(ref note_id) = self.diff_view.selected_note.clone() {
            let mut content_to_copy = None;
            for dl in &self.diff_view.lines {
                if let Some(note) = dl.comment_notes.iter().find(|n| &n.id == note_id) {
                    let c = match &note.rationale {
                        Some(r) if !note.text.contains(r.as_str()) => {
                            format!("{}\n\n{}", note.text, r)
                        }
                        _ => note.text.clone(),
                    };
                    content_to_copy = Some(c);
                    break;
                }
            }
            if content_to_copy.is_none() {
                let lines_file = crate::pager::notes_store::load(self.git.repo_path());
                if let Some(note) = lines_file.notes.iter().find(|n| &n.id == note_id) {
                    let c = match &note.rationale {
                        Some(r) if !note.comment.contains(r.as_str()) => {
                            format!("{}\n\n{}", note.comment, r)
                        }
                        _ => note.comment.clone(),
                    };
                    content_to_copy = Some(c);
                }
            }
            if let Some(content) = content_to_copy {
                let _ = Platform::copy_to_clipboard(&content);
                self.diff_view.note_yank_flash = Some((note_id.clone(), std::time::Instant::now()));
                self.diff_view.selection = None;
            }
        }
        Ok(())
    }

    pub(super) fn open_diff_file_in_editor(&mut self) {
        let rel_path = self.diff_view.filename.clone();
        if rel_path.is_empty() {
            return;
        }
        let abs_path_buf = self.git.repo_path().join(&rel_path);
        if !abs_path_buf.exists() {
            return;
        }
        let abs_path = abs_path_buf.to_string_lossy().to_string();

        // Pick the hunk currently at the top of the viewport (after `{`/`}`
        // navigation, scroll_offset sits on a hunk start). Fall back to the
        // most recent hunk before the viewport, then the first hunk.
        let active_hunk_idx = self
            .diff_view
            .hunk_starts
            .iter()
            .rev()
            .find(|&&h| h <= self.diff_view.scroll_offset)
            .copied()
            .or_else(|| self.diff_view.hunk_starts.first().copied());

        let active_hunk_line = active_hunk_idx.and_then(|idx| {
            self.diff_view
                .file_line_number(idx, DiffPanel::New)
                .or_else(|| self.diff_view.file_line_number(idx, DiffPanel::Old))
        });

        match self
            .config
            .user_config
            .os
            .plan_edit(&abs_path, active_hunk_line, Some(1))
        {
            Ok(launch) => {
                let _ = self.launch_editor(launch);
            }
            Err(_) => {
                let _ = crate::os::platform::Platform::open_file(&abs_path);
            }
        }
    }

    pub(super) fn open_diff_file_in_default_program(&mut self) {
        let rel_path = self.diff_view.filename.clone();
        if rel_path.is_empty() {
            return;
        }
        let abs_path_buf = self.git.repo_path().join(&rel_path);
        if !abs_path_buf.exists() {
            return;
        }
        let abs_path = abs_path_buf.to_string_lossy().to_string();
        if let Ok(launch) = self.config.user_config.os.plan_open(&abs_path) {
            let _ = self.launch_editor(launch);
        }
    }

    pub(super) fn handle_paste(&mut self, data: String) {
        if data.is_empty() {
            return;
        }
        let popup_width = (self.layout.width * 60 / 100)
            .clamp(30, 60)
            .min(self.layout.width);
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
                is_commit,
                confirm_focused,
                ..
            } => {
                if *confirm_focused {
                    return;
                }
                if *is_commit {
                    textarea.insert_str(&data);
                    if effective_width > 0 {
                        auto_wrap_textarea(textarea, effective_width);
                    }
                } else {
                    // Single-line input: strip newlines from pasted content.
                    let cleaned: String = data.replace('\r', "").replace('\n', " ");
                    textarea.insert_str(&cleaned);
                    if popup_inner > 0 {
                        soft_wrap_textarea(textarea, popup_inner);
                    }
                }
            }
            PopupState::CommitInput {
                focus,
                summary_textarea,
                body_textarea,
                body_state,
                ..
            } => {
                match *focus {
                    popup::CommitInputFocus::Summary => {
                        // Split on first newline: first line into summary, rest into body.
                        match data.find('\n') {
                            Some(idx) => {
                                let s = data[..idx].replace('\r', "");
                                let b = data[idx + 1..].trim_start_matches('\n').to_string();
                                summary_textarea.insert_str(&s);
                                if !b.is_empty() {
                                    body_state.insert_str(&b);
                                    if effective_width > 0 {
                                        body_state.render_into(body_textarea, effective_width);
                                    }
                                }
                            }
                            None => {
                                summary_textarea.insert_str(&data);
                            }
                        }
                    }
                    popup::CommitInputFocus::Body => {
                        body_state.insert_str(&data);
                        if effective_width > 0 {
                            body_state.render_into(body_textarea, effective_width);
                        }
                    }
                }
            }
            PopupState::CommandPalette {
                selected,
                scroll_offset,
                search_textarea,
                ..
            } => {
                let cleaned: String = data.replace('\r', "").replace('\n', " ");
                search_textarea.insert_str(&cleaned);
                *selected = 0;
                *scroll_offset = 0;
            }
            PopupState::Checklist {
                items,
                selected,
                search_textarea,
                free_entry_category,
                ..
            } => {
                let cleaned: String = data.replace('\r', "").replace('\n', " ");
                search_textarea.insert_str(&cleaned);
                let after = search_textarea.lines().join("");
                crate::gui::popup::sync_checklist_free_entry(
                    items,
                    free_entry_category.as_deref(),
                    &after,
                );
                *selected = 0;
            }
            PopupState::RefPicker { core, .. } => {
                use crate::gui::popup::{REF_FREE_ENTRY_CATEGORY, sync_list_picker_free_entry};
                let cleaned: String = data.replace('\r', "").replace('\n', " ");
                core.search_textarea.insert_str(&cleaned);
                sync_list_picker_free_entry(core, REF_FREE_ENTRY_CATEGORY);
                // Paste always resets scroll (matches previous RefPicker paste behavior).
                core.scroll_offset = 0;
            }
            PopupState::ListPicker {
                core,
                free_entry_category,
                ..
            } => {
                use crate::gui::popup::sync_list_picker_prefer_free_entry;
                let cleaned: String = data.replace('\r', "").replace('\n', " ");
                core.search_textarea.insert_str(&cleaned);
                let category = free_entry_category.clone();
                sync_list_picker_prefer_free_entry(core, &category);
                core.scroll_offset = 0;
            }
            PopupState::ThemePicker { core, .. } => {
                let cleaned: String = data.replace('\r', "").replace('\n', " ");
                core.search_textarea.insert_str(&cleaned);
                let new_search = core.search_textarea.lines().join("");
                let new_lower = new_search.to_lowercase();
                if !new_lower.is_empty()
                    && let Some(idx) = core
                        .items
                        .iter()
                        .position(|i| i.label.to_lowercase().contains(&new_lower))
                {
                    core.selected = idx;
                    self.current_theme_index = idx;
                    core.scroll_offset = idx;
                }
            }
            _ => {}
        }
    }
}
