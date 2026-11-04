//! Mouse input: clicks, scroll, drag-resize, selection, compare-mode mouse, frame layout hit-testing.

use super::*;

impl Gui {
    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

        if !self.config.user_config.gui.mouse_events {
            return;
        }

        // Sidebar divider drag (Normal mode only). Must run before text-select /
        // focus paths so the hit strip wins the gesture.
        if self.sidebar_resizing {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    self.apply_sidebar_ratio_from_mouse(mouse.column, mouse.row);
                    return;
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.sidebar_resizing = false;
                    self.sidebar_resize_row_offset = 0;
                    return;
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    self.apply_sidebar_ratio_from_mouse(mouse.column, mouse.row);
                    return;
                }
                _ => {
                    self.sidebar_resizing = false;
                    self.sidebar_resize_row_offset = 0;
                }
            }
        } else if self.popup == PopupState::None
            && self.screen_mode == ScreenMode::Normal
            && (!self.diff_mode.active || self.diff_mode.editing.is_none())
            && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && self.sidebar_divider_hit(mouse.column, mouse.row)
        {
            self.sidebar_resizing = true;
            self.sidebar_resize_row_offset = self.portrait_sidebar_resize_offset(mouse.row);
            self.diff_view.selection = None;
            self.apply_sidebar_ratio_from_mouse(mouse.column, mouse.row);
            return;
        }

        // ✦ AI-generate button on commit-message popups: handle clicks.
        if matches!(self.popup, PopupState::CommitInput { .. }) {
            let area = ratatui::layout::Rect::new(0, 0, self.layout.width, self.layout.height);
            if let Some(btn_rect) = views::commit_ai_button_geometry(&self.popup, area) {
                let over = rect_contains(btn_rect, mouse.column, mouse.row);
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) if over => {
                        let configured = !self
                            .config
                            .user_config
                            .git
                            .commit
                            .generate_command
                            .trim()
                            .is_empty();
                        if configured {
                            self.trigger_ai_commit_generation_from_editor();
                        } else {
                            let url = "https://github.com/blankeos/lazygitrs#whats-different";
                            if let Err(e) = crate::os::platform::Platform::open_file(url) {
                                self.popup = PopupState::Message {
                                    title: "Error".to_string(),
                                    message: format!("Could not open browser: {}", e),
                                    kind: MessageKind::Error,
                                };
                            }
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }

        if matches!(self.popup, PopupState::CommitInput { .. }) {
            let area = ratatui::layout::Rect::new(0, 0, self.layout.width, self.layout.height);
            if let Some(body_rect) = views::commit_description_textarea_geometry(&self.popup, area)
                && rect_contains(body_rect, mouse.column, mouse.row)
            {
                match mouse.kind {
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        let rows: i16 = if matches!(mouse.kind, MouseEventKind::ScrollDown) {
                            3
                        } else {
                            -3
                        };
                        let wrap_width = self.commit_body_wrap_width();
                        if let PopupState::CommitInput {
                            body_textarea,
                            body_state,
                            ..
                        } = &mut self.popup
                        {
                            body_textarea.scroll((rows, 0));
                            let ratatui_textarea::DataCursor(row, col) = body_textarea.cursor();
                            body_state.set_cursor_from_visual(row, col, wrap_width);
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }

        // Rebase mode: scroll and click support
        if self.rebase_mode.active {
            match mouse.kind {
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                    // Use the viewport height stored by the renderer so this
                    // matches what's actually on screen (including resizes).
                    let list_h = self.rebase_mode.visible_height;
                    // List length includes entries + the base commit row appended at the bottom.
                    let list_len = self.rebase_mode.entries.len() + 1;
                    let delta: isize = if matches!(mouse.kind, MouseEventKind::ScrollDown) {
                        3
                    } else {
                        -3
                    };
                    scroll::scroll_viewport(&mut self.rebase_mode.scroll, delta, list_len, list_h);
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    // Compute the list area to determine which entry was clicked
                    let area =
                        ratatui::layout::Rect::new(0, 0, self.layout.width, self.layout.height);
                    let outer = ratatui::layout::Layout::default()
                        .direction(ratatui::layout::Direction::Vertical)
                        .constraints([
                            ratatui::layout::Constraint::Min(1),
                            ratatui::layout::Constraint::Length(1),
                        ])
                        .split(area);
                    let block = ratatui::widgets::Block::default()
                        .borders(ratatui::widgets::Borders::ALL)
                        .border_type(ratatui::widgets::BorderType::Rounded);
                    let inner = block.inner(outer[0]);
                    let has_banner =
                        self.rebase_mode.phase == modes::rebase_mode::RebasePhase::InProgress;
                    let banner_h: u16 = if has_banner { 2 } else { 0 };
                    // List starts after: inner.y + info_line(1) + banner_h
                    let list_y = inner.y + 1 + banner_h;
                    let list_h = inner.height.saturating_sub(1 + banner_h) as usize;
                    if mouse.row >= list_y && mouse.row < list_y + list_h as u16 {
                        let row_in_list = (mouse.row - list_y) as usize;
                        let clicked_idx = self.rebase_mode.scroll + row_in_list;
                        if clicked_idx < self.rebase_mode.entries.len() {
                            self.rebase_mode.selected = clicked_idx;
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        // Diff mode has its own mouse handling
        if self.diff_mode.active {
            self.handle_diff_mode_mouse(mouse);
            return;
        }

        // Help popup intercepts mouse scroll and click
        if let PopupState::CommandPalette {
            sections,
            selected,
            scroll_offset,
            search_textarea,
        } = &mut self.popup
        {
            // Compute total display rows so we can clamp scroll
            let search_lower = search_textarea.lines().join("").to_lowercase();
            let has_search = !search_lower.is_empty();
            let total_rows: usize = sections
                .iter()
                .map(|s| {
                    let visible = if has_search {
                        s.entries
                            .iter()
                            .filter(|e| {
                                e.key.to_lowercase().contains(&search_lower)
                                    || e.description.to_lowercase().contains(&search_lower)
                            })
                            .count()
                    } else {
                        s.entries.len()
                    };
                    if visible > 0 { visible + 1 } else { 0 } // +1 for header
                })
                .sum();

            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    *scroll_offset = scroll_offset.saturating_sub(3);
                }
                MouseEventKind::ScrollDown => {
                    *scroll_offset = (*scroll_offset + 3).min(total_rows.saturating_sub(1));
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    // Click to select an entry in the help list
                    let area =
                        ratatui::layout::Rect::new(0, 0, self.layout.width, self.layout.height);
                    let popup_width = (area.width * 70 / 100).clamp(36, 72);
                    let content_height = total_rows.max(1);
                    let popup_height = (content_height as u16 + 5)
                        .min(area.height.saturating_sub(4))
                        .max(10);
                    let x = (area.width.saturating_sub(popup_width)) / 2;
                    let y = (area.height.saturating_sub(popup_height)) / 2;
                    let inner_y = y + 1; // border
                    let list_start = inner_y + 2; // search + separator
                    let inner_height = popup_height.saturating_sub(2); // borders
                    let list_height = inner_height.saturating_sub(3) as usize; // search + sep + hint

                    if mouse.row >= list_start
                        && mouse.row < list_start + list_height as u16
                        && mouse.column >= x
                        && mouse.column < x + popup_width
                    {
                        let row_in_list = (mouse.row - list_start) as usize;
                        let display_idx = *scroll_offset + row_in_list;

                        // Build flat display list to map display_idx to entry index
                        let mut di = 0usize;
                        let mut ei = 0usize;
                        let mut clicked_entry = None;
                        'sections: for section in sections.iter() {
                            let visible_entries: Vec<_> = section
                                .entries
                                .iter()
                                .filter(|e| {
                                    !has_search
                                        || e.key.to_lowercase().contains(&search_lower)
                                        || e.description.to_lowercase().contains(&search_lower)
                                })
                                .collect();
                            if !visible_entries.is_empty() {
                                if di == display_idx {
                                    // Clicked on a header — ignore
                                    break;
                                }
                                di += 1; // header
                                for _ in visible_entries {
                                    if di == display_idx {
                                        clicked_entry = Some(ei);
                                        break 'sections;
                                    }
                                    di += 1;
                                    ei += 1;
                                }
                            }
                        }
                        if let Some(entry_idx) = clicked_entry {
                            *selected = entry_idx;
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        // Free-entry list pickers (RefPicker / ListPicker) intercept mouse scroll and click
        if matches!(
            self.popup,
            PopupState::RefPicker { .. } | PopupState::ListPicker { .. }
        ) {
            let (core, w, h) = match &mut self.popup {
                PopupState::RefPicker { core, .. } | PopupState::ListPicker { core, .. } => {
                    (core, self.layout.width, self.layout.height)
                }
                _ => unreachable!(),
            };
            handle_list_picker_mouse(core, mouse, w, h);
            return;
        }

        // ThemePicker popup intercepts mouse scroll and click (respects search filter)
        if matches!(self.popup, PopupState::ThemePicker { .. }) {
            let (w, h) = (self.layout.width, self.layout.height);
            let PopupState::ThemePicker { core, .. } = &mut self.popup else {
                unreachable!();
            };
            handle_list_picker_mouse(core, mouse, w, h);
            self.current_theme_index = core.selected;
            return;
        }

        // Action menus: first click selects, second click on same item confirms.
        if matches!(self.popup, PopupState::Menu { .. }) {
            let MouseEventKind::Down(MouseButton::Left) = mouse.kind else {
                return;
            };
            let area = ratatui::layout::Rect::new(0, 0, self.layout.width, self.layout.height);
            if let Some(idx) = views::menu_item_at(&self.popup, area, mouse.column, mouse.row) {
                let already_selected = matches!(
                    &self.popup,
                    PopupState::Menu { selected, .. } if *selected == idx
                );
                if already_selected {
                    self.execute_menu_action(Some(idx));
                } else if let PopupState::Menu { selected, .. } = &mut self.popup {
                    *selected = idx;
                }
            }
            return;
        }

        // Checklists: first click selects, second click on same item toggles.
        if matches!(self.popup, PopupState::Checklist { .. }) {
            let MouseEventKind::Down(MouseButton::Left) = mouse.kind else {
                return;
            };
            let area = ratatui::layout::Rect::new(0, 0, self.layout.width, self.layout.height);
            if let Some(visible_idx) =
                views::checklist_item_at(&self.popup, area, mouse.column, mouse.row)
                && let PopupState::Checklist {
                    items,
                    selected,
                    search_textarea,
                    ..
                } = &mut self.popup
            {
                if *selected == visible_idx {
                    let search = search_textarea.lines().join("");
                    let visible_indices: Vec<usize> = items
                        .iter()
                        .enumerate()
                        .filter(|(_, it)| {
                            it.is_free_entry
                                || search.is_empty()
                                || it.label.to_lowercase().contains(&search.to_lowercase())
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if let Some(&real_idx) = visible_indices.get(visible_idx) {
                        items[real_idx].checked = !items[real_idx].checked;
                    }
                } else {
                    *selected = visible_idx;
                }
            }
            return;
        }

        let fl = self.compute_current_frame_layout();
        if let Some(grab_rect) = fl.grab_column {
            let over_grab = mouse.column == grab_rect.x
                && mouse.row >= grab_rect.y
                && mouse.row < grab_rect.y + grab_rect.height;

            if self.grab_column_dragging.is_some() {
                self.grab_column_hovered = true;
                match mouse.kind {
                    MouseEventKind::Drag(MouseButton::Left) => {
                        let total_width = self.layout.width as f64;
                        if total_width > 0.0 {
                            let new_ratio = (mouse.column as f64) / total_width;
                            self.layout.side_panel_ratio = new_ratio.clamp(0.05, 0.95);
                        }
                        return;
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        self.grab_column_dragging = None;
                        self.grab_column_hovered = over_grab;
                        return;
                    }
                    _ => {}
                }
            }

            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) if over_grab => {
                    self.grab_column_dragging = Some(mouse.column);
                    self.grab_column_hovered = true;
                    return;
                }
                MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                    if self.grab_column_hovered != over_grab {
                        self.grab_column_hovered = over_grab;
                    }
                    if over_grab {
                        return;
                    }
                }
                _ => {}
            }
        } else {
            self.grab_column_hovered = false;
            self.grab_column_dragging = None;
        }

        let main_panel = self.compute_main_panel_rect();
        let pl = DiffPanelLayout::compute(main_panel, &self.diff_view);

        // Track mouse hover over the revert-block marker (for tooltip).
        if !self.diff_mode.active {
            let new_hover = self.revert_hunk_at_position(main_panel, &pl, mouse.column, mouse.row);
            if self.diff_view.hovered_revert_hunk != new_hover {
                self.diff_view.hovered_revert_hunk = new_hover;
            }
        }

        // Track line hover for inline notes `` button
        let new_line_hover = if rect_contains(main_panel, mouse.column, mouse.row) {
            if let Some(panel) = pl.panel_at_x(mouse.column) {
                if let Some((line_idx, _)) = self.diff_view.line_chunk_at_row(mouse.row, &pl) {
                    // Check if this row is actually the  button row of the
                    // previous line (after its notes). If so, keep hovering the
                    // previous line so the  stays visible.
                    if line_idx > 0 {
                        let content_width =
                            pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
                        let panel_width =
                            pl.old_content_end_x.saturating_sub(pl.old_content_x) as usize;
                        let right_content_width =
                            pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
                        let prev_idx = line_idx - 1;
                        if let Some(prev_dl) = self.diff_view.lines.get(prev_idx) {
                            if prev_dl.file_header.is_none() && !prev_dl.comment_notes.is_empty() {
                                // Compute prev line's code_y
                                let mut acc = 0usize;
                                for (offset, dl) in self.diff_view.lines
                                    [self.diff_view.scroll_offset..=prev_idx]
                                    .iter()
                                    .enumerate()
                                {
                                    let idx = self.diff_view.scroll_offset + offset;
                                    if idx == prev_idx {
                                        break;
                                    }
                                    if self.diff_view.view_layout
                                        == crate::pager::side_by_side::DiffViewLayout::Unified
                                        || self.diff_view.side_view
                                            != crate::pager::side_by_side::DiffSideView::Both
                                    {
                                        acc +=
                                            crate::pager::side_by_side::unified_line_visual_height(
                                                dl,
                                                content_width,
                                                &self.diff_view,
                                                idx,
                                            );
                                    } else {
                                        acc += crate::pager::side_by_side::line_visual_height(
                                            dl,
                                            panel_width,
                                            right_content_width,
                                            &self.diff_view,
                                            idx,
                                        );
                                    }
                                }
                                let plus_y = pl.inner_y
                                    + acc as u16
                                    + (prev_dl.comment_notes.len() * 5) as u16;
                                if mouse.row == plus_y {
                                    Some((prev_idx, panel))
                                } else {
                                    Some((line_idx, panel))
                                }
                            } else {
                                Some((line_idx, panel))
                            }
                        } else {
                            Some((line_idx, panel))
                        }
                    } else {
                        Some((line_idx, panel))
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        if self.diff_view.hovered_line != new_line_hover {
            self.diff_view.hovered_line = new_line_hover;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let in_main = main_panel.x <= mouse.column
                    && mouse.column < main_panel.x + main_panel.width
                    && main_panel.y <= mouse.row
                    && mouse.row < main_panel.y + main_panel.height;

                // In Full screen mode, the main_panel covers everything.
                // If the sidebar is focused (not diff_focused), clicks should
                // go to the sidebar handler, not start a diff selection.
                let full_sidebar = self.screen_mode == ScreenMode::Full && !self.diff_focused;

                if in_main && !self.diff_view.is_empty() && !full_sidebar {
                    if self.try_handle_revert_block_click(main_panel, pl, mouse.column, mouse.row) {
                        self.diff_focused = true;
                        return;
                    }
                    // Clicks on the pinned sticky file header don't start a
                    // text selection — there is no content to select there.
                    if self.diff_view.is_sticky_row(mouse.row, &pl) {
                        self.diff_view.selection = None;
                        self.diff_focused = true;
                        return;
                    }

                    if let Some((line_idx, panel)) = self.diff_view.hovered_line {
                        // Compute the  button Y position: code row + note_count * 5
                        let content_width =
                            pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
                        let panel_width =
                            pl.old_content_end_x.saturating_sub(pl.old_content_x) as usize;
                        let right_content_width =
                            pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
                        let mut plus_acc = 0usize;
                        for (offset, dl) in self.diff_view.lines
                            [self.diff_view.scroll_offset..=line_idx]
                            .iter()
                            .enumerate()
                        {
                            let idx = self.diff_view.scroll_offset + offset;
                            if idx == line_idx {
                                break;
                            }
                            if self.diff_view.view_layout
                                == crate::pager::side_by_side::DiffViewLayout::Unified
                                || self.diff_view.side_view
                                    != crate::pager::side_by_side::DiffSideView::Both
                            {
                                plus_acc += crate::pager::side_by_side::unified_line_visual_height(
                                    dl,
                                    content_width,
                                    &self.diff_view,
                                    idx,
                                );
                            } else {
                                plus_acc += crate::pager::side_by_side::line_visual_height(
                                    dl,
                                    panel_width,
                                    right_content_width,
                                    &self.diff_view,
                                    idx,
                                );
                            }
                        }
                        let note_count = if self.diff_view.notes_visible {
                            self.diff_view
                                .lines
                                .get(line_idx)
                                .map(|dl| dl.comment_notes.len())
                                .unwrap_or(0)
                        } else {
                            0
                        };
                        let plus_y = pl.inner_y
                            + self.diff_view.sticky_row_height(&pl)
                            + plus_acc as u16
                            + (note_count * 5) as u16;

                        // Check if click was on `` button (hidden when notes are off)
                        let is_on_plus_x = match panel {
                            crate::pager::side_by_side::DiffPanel::Old => {
                                mouse.column >= pl.old_content_end_x.saturating_sub(3)
                                    && mouse.column < pl.old_content_end_x
                            }
                            crate::pager::side_by_side::DiffPanel::New => {
                                mouse.column >= pl.new_content_end_x.saturating_sub(3)
                                    && mouse.column < pl.new_content_end_x
                            }
                        };
                        if self.diff_view.notes_visible && is_on_plus_x && mouse.row == plus_y {
                            self.open_inline_note_editor(line_idx, panel, None);
                            return;
                        }

                        // Check if click was on a saved note box
                        if let Some(note_id) =
                            self.note_id_at_click(line_idx, panel, mouse.row, &pl)
                        {
                            // Compute which row within the note block was clicked
                            let mut acc = 0usize;
                            for (offset, dl) in self.diff_view.lines
                                [self.diff_view.scroll_offset..=line_idx]
                                .iter()
                                .enumerate()
                            {
                                let idx = self.diff_view.scroll_offset + offset;
                                if idx == line_idx {
                                    break;
                                }
                                if self.diff_view.view_layout
                                    == crate::pager::side_by_side::DiffViewLayout::Unified
                                    || self.diff_view.side_view
                                        != crate::pager::side_by_side::DiffSideView::Both
                                {
                                    acc += crate::pager::side_by_side::unified_line_visual_height(
                                        dl,
                                        content_width,
                                        &self.diff_view,
                                        idx,
                                    );
                                } else {
                                    acc += crate::pager::side_by_side::line_visual_height(
                                        dl,
                                        panel_width,
                                        right_content_width,
                                        &self.diff_view,
                                        idx,
                                    );
                                }
                            }
                            let note_start_y =
                                pl.inner_y + self.diff_view.sticky_row_height(&pl) + acc as u16;
                            let click_off = mouse.row.saturating_sub(note_start_y);
                            let block_row = click_off % 5;

                            if block_row == 4 {
                                // Bottom border — [S] send / [e] edit / [d] del by X position
                                let panel_end_x = match panel {
                                    crate::pager::side_by_side::DiffPanel::Old => {
                                        pl.old_content_end_x
                                    }
                                    crate::pager::side_by_side::DiffPanel::New => {
                                        pl.new_content_end_x
                                    }
                                };
                                let del_start = panel_end_x.saturating_sub(9);

                                // Check note source to determine which buttons are shown
                                let is_user_note = self.diff_view.lines[line_idx]
                                    .comment_notes
                                    .iter()
                                    .any(|n| {
                                        n.id == note_id
                                            && n.source == crate::pager::NoteSource::User
                                    });
                                let has_send = self.diff_view.lines[line_idx]
                                    .comment_notes
                                    .iter()
                                    .any(|n| {
                                        n.id == note_id
                                            && n.source == crate::pager::NoteSource::User
                                            && n.status == crate::pager::NoteStatus::New
                                    });
                                let has_reset = self.diff_view.lines[line_idx]
                                    .comment_notes
                                    .iter()
                                    .any(|n| {
                                        n.id == note_id
                                            && n.source == crate::pager::NoteSource::User
                                            && n.status != crate::pager::NoteStatus::New
                                    });

                                if is_user_note {
                                    let edit_start = panel_end_x.saturating_sub(19);
                                    let send_start = panel_end_x.saturating_sub(29);
                                    let reset_start = panel_end_x.saturating_sub(30);
                                    if mouse.column >= del_start && mouse.column < panel_end_x {
                                        self.delete_note(note_id);
                                    } else if mouse.column >= edit_start && mouse.column < del_start
                                    {
                                        self.open_inline_note_editor(
                                            line_idx,
                                            panel,
                                            Some(note_id),
                                        );
                                    } else if has_send
                                        && mouse.column >= send_start
                                        && mouse.column < edit_start
                                    {
                                        self.notify_ai_for_note(note_id);
                                    } else if has_reset
                                        && mouse.column >= reset_start
                                        && mouse.column < edit_start
                                    {
                                        self.reset_note_status(&note_id);
                                    } else {
                                        self.diff_view.selected_note = Some(note_id);
                                    }
                                } else {
                                    // AI notes: only [d] del is shown
                                    if mouse.column >= del_start && mouse.column < panel_end_x {
                                        self.delete_note(note_id);
                                    } else {
                                        self.diff_view.selected_note = Some(note_id);
                                    }
                                }
                            } else {
                                // Note body — just select, don't enter edit mode
                                self.diff_view.selected_note = Some(note_id);
                            }
                            self.diff_focused = true;
                            return;
                        }
                    }

                    if let Some(panel) = pl.panel_at_x(mouse.column) {
                        self.diff_view.selection = Some(TextSelection {
                            panel,
                            start_col: mouse.column,
                            start_row: mouse.row,
                            end_col: mouse.column,
                            end_row: mouse.row,
                            dragging: true,
                            is_click: false,
                            text: String::new(),
                            edit_line_number: None,
                            edit_column_number: None,
                        });
                    } else {
                        self.diff_view.selection = None;
                    }
                    self.diff_focused = true;
                } else {
                    // Click outside diff — clear selection and handle normally
                    self.diff_view.selection = None;
                    self.handle_mouse_click(mouse.column, mouse.row);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(ref mut sel) = self.diff_view.selection
                    && sel.dragging
                {
                    let (cmin, cmax) = pl.content_range(sel.panel);
                    // Allow dragging into gutter area of same panel (5 cols before content)
                    let col_min = cmin.saturating_sub(5);
                    sel.end_col = mouse.column.max(col_min).min(cmax.saturating_sub(1));
                    sel.end_row = mouse
                        .row
                        .max(pl.inner_y)
                        .min(pl.inner_end_y.saturating_sub(1));
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // Finalize the selection
                if let Some(ref mut sel) = self.diff_view.selection {
                    sel.dragging = false;
                    // If start == end (just a click, no drag)
                    if sel.start_col == sel.end_col && sel.start_row == sel.end_row {
                        if self.diff_view.file_exists_on_disk {
                            // Keep as click-state to show the edit tooltip
                            sel.is_click = true;
                        } else {
                            self.diff_view.selection = None;
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if self.is_in_commit_details_panel(mouse.column, mouse.row) {
                    self.commit_details_scroll = self.commit_details_scroll.saturating_sub(2);
                    return;
                }
                self.diff_view.selection = None;
                let in_diff = self.diff_focused
                    || (self.screen_mode != ScreenMode::Full
                        && self.is_in_main_panel(mouse.column, mouse.row));
                if mouse.modifiers.contains(KeyModifiers::SHIFT) && in_diff {
                    self.diff_view.scroll_left(4);
                } else if in_diff {
                    self.diff_view.scroll_up(3);
                } else {
                    // Viewport-only scroll: move scroll offset without changing selection
                    let active_ctx = self.context_mgr.active();
                    let model = self
                        .model
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let list_len = self.context_mgr.list_len(&model);
                    drop(model);
                    let visible_height = self.sidebar_visible_height();
                    let mut offset = self.context_mgr.scroll_offset(active_ctx);
                    scroll::scroll_viewport(&mut offset, -3, list_len, visible_height);
                    self.context_mgr.set_scroll_offset(active_ctx, offset);
                    self.context_mgr.viewport_manually_scrolled = true;
                }
            }
            MouseEventKind::ScrollDown => {
                if self.is_in_commit_details_panel(mouse.column, mouse.row) {
                    self.commit_details_scroll = self.commit_details_scroll.saturating_add(2);
                    return;
                }
                self.diff_view.selection = None;
                let in_diff = self.diff_focused
                    || (self.screen_mode != ScreenMode::Full
                        && self.is_in_main_panel(mouse.column, mouse.row));
                if mouse.modifiers.contains(KeyModifiers::SHIFT) && in_diff {
                    self.diff_view.scroll_right(4);
                } else if in_diff {
                    self.diff_view.scroll_down(3);
                } else {
                    // Viewport-only scroll: move scroll offset without changing selection
                    let active_ctx = self.context_mgr.active();
                    let model = self
                        .model
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let list_len = self.context_mgr.list_len(&model);
                    drop(model);
                    let visible_height = self.sidebar_visible_height();
                    let mut offset = self.context_mgr.scroll_offset(active_ctx);
                    scroll::scroll_viewport(&mut offset, 3, list_len, visible_height);
                    self.context_mgr.set_scroll_offset(active_ctx, offset);
                    self.context_mgr.viewport_manually_scrolled = true;
                }
            }
            MouseEventKind::ScrollLeft => {
                if self.is_in_commit_details_panel(mouse.column, mouse.row) {
                    return;
                }
                if self.diff_focused
                    || (self.screen_mode != ScreenMode::Full
                        && self.is_in_main_panel(mouse.column, mouse.row))
                {
                    self.diff_view.scroll_left(4);
                }
            }
            MouseEventKind::ScrollRight => {
                if self.is_in_commit_details_panel(mouse.column, mouse.row) {
                    return;
                }
                if self.diff_focused
                    || (self.screen_mode != ScreenMode::Full
                        && self.is_in_main_panel(mouse.column, mouse.row))
                {
                    self.diff_view.scroll_right(4);
                }
            }
            _ => {}
        }
    }

    pub(super) fn handle_diff_mode_mouse(&mut self, mouse: MouseEvent) {
        use crate::gui::modes::diff_mode::{DiffModeFocus, DiffModeSelector};
        use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
        use ratatui::layout::Rect;

        // Help popup intercepts mouse scroll
        if let PopupState::CommandPalette {
            sections,
            scroll_offset,
            search_textarea,
            ..
        } = &mut self.popup
        {
            let search_lower = search_textarea.lines().join("").to_lowercase();
            let has_search = !search_lower.is_empty();
            let total_rows: usize = sections
                .iter()
                .map(|s| {
                    let visible = if has_search {
                        s.entries
                            .iter()
                            .filter(|e| {
                                e.key.to_lowercase().contains(&search_lower)
                                    || e.description.to_lowercase().contains(&search_lower)
                            })
                            .count()
                    } else {
                        s.entries.len()
                    };
                    if visible > 0 { visible + 1 } else { 0 }
                })
                .sum();

            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    *scroll_offset = scroll_offset.saturating_sub(3);
                }
                MouseEventKind::ScrollDown => {
                    *scroll_offset = (*scroll_offset + 3).min(total_rows.saturating_sub(1));
                }
                _ => {}
            }
            return;
        }

        // Free-entry list pickers (RefPicker / ListPicker) intercept mouse scroll and click
        if matches!(
            self.popup,
            PopupState::RefPicker { .. } | PopupState::ListPicker { .. }
        ) {
            let (core, w, h) = match &mut self.popup {
                PopupState::RefPicker { core, .. } | PopupState::ListPicker { core, .. } => {
                    (core, self.layout.width, self.layout.height)
                }
                _ => unreachable!(),
            };
            handle_list_picker_mouse(core, mouse, w, h);
            return;
        }

        let area = Rect::new(0, 0, self.layout.width, self.layout.height);

        let compare_layout = self.compute_compare_layout();
        let sidebar = compare_layout.sidebar;

        let selector_a_rect = sidebar[0];
        let selector_b_rect = sidebar[1];
        let files_rect = sidebar[2];
        let commits_rect = sidebar[3];
        let diff_rect = compare_layout.diff;

        let col = mouse.column;
        let row = mouse.row;

        // Combobox dropdown mouse handling — intercepts clicks/scrolls when editing
        if self.diff_mode.editing.is_some() && !self.diff_mode.search_results.is_empty() {
            let anchor = if matches!(
                self.diff_mode.editing,
                Some(crate::gui::modes::diff_mode::DiffModeSelector::A)
            ) {
                selector_a_rect
            } else {
                selector_b_rect
            };
            let total = self.diff_mode.search_results.len();
            let max_items = 10usize.min(total);
            let dropdown_height = (max_items as u16) + 2;
            let available_height = area.height.saturating_sub(anchor.y + anchor.height);
            let dropdown_area = Rect {
                x: anchor.x,
                y: anchor.y + anchor.height,
                width: anchor.width,
                height: dropdown_height.min(available_height),
            };

            if rect_contains(dropdown_area, col, row) {
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        // Click on a dropdown item — select it and confirm
                        let inner_y = row.saturating_sub(dropdown_area.y + 1); // +1 for top border
                        let clicked_idx = self.diff_mode.dropdown_scroll + inner_y as usize;
                        if clicked_idx < total {
                            self.diff_mode.search_selected = clicked_idx;
                            self.diff_mode.confirm_selection();
                            if self.diff_mode.has_both_refs() {
                                let _ = crate::gui::controller::diff_mode::reload_diff_files(self);
                                self.diff_mode.set_focus(DiffModeFocus::CommitFiles);
                            } else if self.diff_mode.ref_a.is_empty() {
                                self.diff_mode.set_focus(DiffModeFocus::SelectorA);
                                self.diff_mode.start_editing(DiffModeSelector::A);
                                let model = self
                                    .model
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                self.diff_mode.search_refs(
                                    &model.branches,
                                    &model.tags,
                                    &model.commits,
                                    &model.remotes,
                                    &model.head_branch_name,
                                );
                            } else {
                                self.diff_mode.set_focus(DiffModeFocus::SelectorB);
                                self.diff_mode.start_editing(DiffModeSelector::B);
                                let model = self
                                    .model
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                self.diff_mode.search_refs(
                                    &model.branches,
                                    &model.tags,
                                    &model.commits,
                                    &model.remotes,
                                    &model.head_branch_name,
                                );
                            }
                            self.needs_diff_refresh = true;
                        }
                        return;
                    }
                    MouseEventKind::ScrollUp => {
                        if self.diff_mode.search_selected > 0 {
                            self.diff_mode.search_selected =
                                self.diff_mode.search_selected.saturating_sub(3);
                            self.diff_mode.ensure_dropdown_visible(10);
                        }
                        return;
                    }
                    MouseEventKind::ScrollDown => {
                        let len = self.diff_mode.search_results.len();
                        if len > 0 {
                            self.diff_mode.search_selected =
                                (self.diff_mode.search_selected + 3).min(len - 1);
                            self.diff_mode.ensure_dropdown_visible(10);
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if click is in the diff panel — start text selection
                if rect_contains(diff_rect, col, row) && !self.diff_view.is_empty() {
                    let pl = DiffPanelLayout::compute(diff_rect, &self.diff_view);
                    if self.try_handle_revert_block_click(diff_rect, pl, col, row) {
                        self.diff_mode.set_focus(DiffModeFocus::DiffExploration);
                        return;
                    }
                    if self.diff_view.is_sticky_row(row, &pl) {
                        self.diff_view.selection = None;
                        self.diff_mode.focus = DiffModeFocus::DiffExploration;
                        return;
                    }
                    if let Some(panel) = pl.panel_at_x(col) {
                        self.diff_view.selection = Some(TextSelection {
                            panel,
                            start_col: col,
                            start_row: row,
                            end_col: col,
                            end_row: row,
                            dragging: true,
                            is_click: false,
                            text: String::new(),
                            edit_line_number: None,
                            edit_column_number: None,
                        });
                    } else {
                        self.diff_view.selection = None;
                    }
                    self.diff_mode.set_focus(DiffModeFocus::DiffExploration);
                } else {
                    self.diff_view.selection = None;

                    // Click on panels to switch focus
                    if rect_contains(selector_a_rect, col, row) {
                        self.diff_mode.set_focus(DiffModeFocus::SelectorA);
                        // Start editing on click
                        self.diff_mode.start_editing(DiffModeSelector::A);
                        let model = self
                            .model
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        self.diff_mode.search_refs(
                            &model.branches,
                            &model.tags,
                            &model.commits,
                            &model.remotes,
                            &model.head_branch_name,
                        );
                    } else if rect_contains(selector_b_rect, col, row) {
                        self.diff_mode.set_focus(DiffModeFocus::SelectorB);
                        // Start editing on click
                        self.diff_mode.start_editing(DiffModeSelector::B);
                        let model = self
                            .model
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        self.diff_mode.search_refs(
                            &model.branches,
                            &model.tags,
                            &model.commits,
                            &model.remotes,
                            &model.head_branch_name,
                        );
                    } else if rect_contains(files_rect, col, row) {
                        self.diff_mode.set_focus(DiffModeFocus::CommitFiles);
                        self.needs_diff_refresh = true;
                        // Click to select a file — use stored scroll offset
                        let inner_y = row.saturating_sub(files_rect.y + 1);
                        let len = self.diff_mode.visible_files_len();
                        let clicked_idx = self.diff_mode.diff_files_scroll + inner_y as usize;
                        if clicked_idx < len {
                            self.diff_mode.diff_files_selected = clicked_idx;
                            self.diff_mode.viewport_manually_scrolled = false;
                            self.needs_diff_refresh = true;
                        }
                    } else if rect_contains(commits_rect, col, row) {
                        self.diff_mode.set_focus(DiffModeFocus::Commits);
                        let inner_y = row.saturating_sub(commits_rect.y + 1);
                        let clicked_idx = self.diff_mode.commits_scroll + inner_y as usize;
                        if row > commits_rect.y
                            && row < commits_rect.bottom().saturating_sub(1)
                            && clicked_idx < self.diff_mode.commits.len()
                        {
                            self.diff_mode.commits_selected = clicked_idx;
                            self.diff_mode.commits_viewport_manually_scrolled = false;
                        }
                        self.needs_diff_refresh = true;
                    } else if rect_contains(diff_rect, col, row) {
                        self.diff_mode.set_focus(DiffModeFocus::DiffExploration);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let pl = DiffPanelLayout::compute(diff_rect, &self.diff_view);
                if let Some(ref mut sel) = self.diff_view.selection
                    && sel.dragging
                {
                    let (cmin, cmax) = pl.content_range(sel.panel);
                    let col_min = cmin.saturating_sub(5);
                    sel.end_col = col.max(col_min).min(cmax.saturating_sub(1));
                    sel.end_row = row.max(pl.inner_y).min(pl.inner_end_y.saturating_sub(1));
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(ref mut sel) = self.diff_view.selection {
                    sel.dragging = false;
                    if sel.start_col == sel.end_col && sel.start_row == sel.end_row {
                        if self.diff_view.file_exists_on_disk {
                            sel.is_click = true;
                        } else {
                            self.diff_view.selection = None;
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if rect_contains(diff_rect, col, row) {
                    self.diff_view.selection = None;
                    if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                        self.diff_view.scroll_left(4);
                    } else {
                        self.diff_view.scroll_up(3);
                    }
                } else if rect_contains(files_rect, col, row) {
                    // Viewport-only scroll: move scroll offset without changing selection
                    let len = self.diff_mode.visible_files_len();
                    let visible_height = files_rect.height.saturating_sub(2) as usize;
                    scroll::scroll_viewport(
                        &mut self.diff_mode.diff_files_scroll,
                        -3,
                        len,
                        visible_height,
                    );
                    self.diff_mode.viewport_manually_scrolled = true;
                } else if rect_contains(commits_rect, col, row) {
                    scroll::scroll_viewport(
                        &mut self.diff_mode.commits_scroll,
                        -3,
                        self.diff_mode.commits.len(),
                        commits_rect.height.saturating_sub(2) as usize,
                    );
                    self.diff_mode.commits_viewport_manually_scrolled = true;
                }
            }
            MouseEventKind::ScrollDown => {
                if rect_contains(diff_rect, col, row) {
                    self.diff_view.selection = None;
                    if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                        self.diff_view.scroll_right(4);
                    } else {
                        self.diff_view.scroll_down(3);
                    }
                } else if rect_contains(files_rect, col, row) {
                    // Viewport-only scroll: move scroll offset without changing selection
                    let len = self.diff_mode.visible_files_len();
                    let visible_height = files_rect.height.saturating_sub(2) as usize;
                    scroll::scroll_viewport(
                        &mut self.diff_mode.diff_files_scroll,
                        3,
                        len,
                        visible_height,
                    );
                    self.diff_mode.viewport_manually_scrolled = true;
                } else if rect_contains(commits_rect, col, row) {
                    let visible_height = commits_rect.height.saturating_sub(2) as usize;
                    if self.diff_mode.commits_scroll + visible_height + 3
                        >= self.diff_mode.commits.len()
                        && let Err(error) = controller::diff_mode::load_more_commits(self)
                    {
                        self.popup = PopupState::Message {
                            title: "Compare commits".into(),
                            message: error.to_string(),
                            kind: popup::MessageKind::Error,
                        };
                    }
                    scroll::scroll_viewport(
                        &mut self.diff_mode.commits_scroll,
                        3,
                        self.diff_mode.commits.len(),
                        visible_height,
                    );
                    self.diff_mode.commits_viewport_manually_scrolled = true;
                }
            }
            MouseEventKind::ScrollLeft if rect_contains(diff_rect, col, row) => {
                self.diff_view.scroll_left(4);
            }
            MouseEventKind::ScrollRight if rect_contains(diff_rect, col, row) => {
                self.diff_view.scroll_right(4);
            }
            _ => {}
        }
    }

    pub(super) fn handle_mouse_click(&mut self, col: u16, row: u16) {
        if let PopupState::HunkCommentary { .. } = &self.popup {
            let popup_width = (self.layout.width * 60 / 100)
                .clamp(30, 60)
                .min(self.layout.width);
            let ta_height = 16u16.min(self.layout.height);
            if ta_height >= 3 && popup_width >= 3 {
                let ta_y = (self.layout.height.saturating_sub(ta_height)) / 2;
                let x = (self.layout.width.saturating_sub(popup_width)) / 2;
                if row == ta_y
                    && col >= x + popup_width.saturating_sub(5)
                    && col <= x + popup_width.saturating_sub(1)
                {
                    self.popup = PopupState::None;
                    return;
                }
            }
        }

        let fl = self.compute_current_frame_layout();

        // Commit details panel is non-focusable; swallow clicks that land there
        // so they don't leak into the diff view / sidebars.
        if let Some(details_rect) = fl.commit_details_panel
            && rect_contains(details_rect, col, row)
        {
            return;
        }

        // In Full screen mode with sidebar focused, the sidebar is rendered
        // in main_panel — treat clicks there as sidebar item selection.
        if self.screen_mode == ScreenMode::Full && !self.diff_focused {
            let panel_rect = fl.main_panel;
            if panel_rect.x <= col
                && col < panel_rect.x + panel_rect.width
                && panel_rect.y <= row
                && row < panel_rect.y + panel_rect.height
            {
                let inner_y = row.saturating_sub(panel_rect.y + 1);
                let active_ctx = self.context_mgr.active();
                let model = self
                    .model
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let list_len = self.context_mgr.list_len(&model);
                drop(model);
                let scroll_offset = self.context_mgr.scroll_offset(active_ctx);
                let clicked_idx = scroll_offset + inner_y as usize;
                if clicked_idx < list_len {
                    self.context_mgr.set_selection(clicked_idx);
                }
            }
            return;
        }

        // Check if click is in the main (diff) panel
        if fl.main_panel.x <= col
            && col < fl.main_panel.x + fl.main_panel.width
            && fl.main_panel.y <= row
            && row < fl.main_panel.y + fl.main_panel.height
        {
            if !self.diff_view.is_empty() {
                self.diff_focused = true;
            }
            return;
        }

        // Check which side panel was clicked
        for (i, &panel_rect) in fl.side_panels.iter().enumerate() {
            if panel_rect.x <= col
                && col < panel_rect.x + panel_rect.width
                && panel_rect.y <= row
                && row < panel_rect.y + panel_rect.height
            {
                self.diff_focused = false;
                if let Some(&window) = SideWindow::ALL.get(i) {
                    let is_title_bar = row == panel_rect.y;

                    if is_title_bar {
                        // Title bar click: switch to the clicked tab if identifiable.
                        let local_x = col.saturating_sub(panel_rect.x);
                        if let Some(tab_ctx) = window.tab_at_x(local_x) {
                            self.context_mgr.set_active(tab_ctx);
                        } else {
                            // Clicked title area but not on a specific tab label —
                            // just activate this window (restore last context).
                            let ctx = self.context_mgr.last_context_for_window(window);
                            self.context_mgr.set_active(ctx);
                        }
                    } else {
                        // Content area click.
                        let current_window = self.context_mgr.active_window();
                        if current_window != window {
                            // Switching to a different window — restore its last context.
                            let ctx = self.context_mgr.last_context_for_window(window);
                            self.context_mgr.set_active(ctx);
                        }
                        // Same window: don't call set_active, preserving any sub-view.

                        // Select the clicked item.
                        let inner_y = row.saturating_sub(panel_rect.y + 1); // +1 for border
                        let active_ctx = self.context_mgr.active();
                        let model = self
                            .model
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let list_len = self.context_mgr.list_len(&model);
                        drop(model);

                        let scroll_offset = self.context_mgr.scroll_offset(active_ctx);
                        let clicked_idx = scroll_offset + inner_y as usize;
                        if clicked_idx < list_len {
                            self.context_mgr.set_selection(clicked_idx);
                        }
                    }
                }
                return;
            }
        }
    }

    pub(super) fn is_in_main_panel(&self, col: u16, row: u16) -> bool {
        let mp = self.compute_main_panel_rect();
        col >= mp.x && col < mp.x + mp.width && row >= mp.y && row < mp.y + mp.height
    }

    /// True if mouse is over the (non-focusable) commit details panel.
    pub(super) fn is_in_commit_details_panel(&self, col: u16, row: u16) -> bool {
        let fl = self.compute_current_frame_layout();
        fl.commit_details_panel
            .map(|r| rect_contains(r, col, row))
            .unwrap_or(false)
    }

    /// Compute the current frame layout using the same flags as views::render.
    /// This must match views.rs so mouse coords map to the rects actually drawn.
    pub(super) fn compute_current_frame_layout(&self) -> layout::FrameLayout {
        let area = ratatui::layout::Rect::new(0, 0, self.layout.width, self.layout.height);
        let panel_count = SideWindow::ALL.len();
        let active_window = self.context_mgr.active_window();
        let active_panel_index = SideWindow::ALL
            .iter()
            .position(|w| *w == active_window)
            .unwrap_or(1);

        // Mirror views.rs: show_details when the active context is a commit
        // list (or drill-in commit files) with a valid selection.
        let show_details = self.details_panel_applies();

        layout::compute_layout_with_details(
            area,
            self.layout.side_panel_ratio,
            panel_count,
            active_panel_index,
            self.screen_mode,
            show_details,
            !self.diff_focused,
        )
    }

    pub(super) fn compute_compare_layout(&self) -> modes::diff_mode::CompareLayout {
        modes::diff_mode::CompareLayout::new(
            ratatui::layout::Rect::new(0, 0, self.layout.width, self.layout.height),
            self.layout.side_panel_ratio,
            self.screen_mode,
            &self.diff_mode,
        )
    }

    /// Content area above the status bar (side + main live here).
    pub(super) fn content_area_rect(&self) -> ratatui::layout::Rect {
        ratatui::layout::Rect::new(
            0,
            0,
            self.layout.width,
            self.layout.height.saturating_sub(1),
        )
    }

    /// Hit-test the side↔main split for drag-resize.
    /// Portrait: expanded panel bottom border and/or main (diff) top border.
    /// Landscape: ~3-col strip around the vertical split.
    pub(super) fn sidebar_divider_hit(&self, col: u16, row: u16) -> bool {
        if self.screen_mode != ScreenMode::Normal {
            return false;
        }
        let content = self.content_area_rect();
        if content.width == 0 || content.height == 0 || !rect_contains(content, col, row) {
            return false;
        }

        if self.diff_mode.active {
            return self.compute_compare_layout().divider_hit(content, col, row);
        }
        let fl = self.compute_current_frame_layout();

        if fl.portrait {
            // Either the expanded side panel's bottom border or the main/diff
            // panel's top border (collapsed panels may sit between them).
            if fl.side_panels.is_empty() {
                return row == content.y;
            }
            if fl.main_panel.height == 0 {
                let y = content.y + content.height.saturating_sub(1);
                return row == y;
            }
            // Top border of the diff box — single row only (content is y+1).
            if fl.main_panel.height > 0 && row == fl.main_panel.y {
                return true;
            }
            let active_window = self.context_mgr.active_window();
            let active_idx = SideWindow::ALL
                .iter()
                .position(|w| *w == active_window)
                .unwrap_or(1);
            // Match layout.rs: Status stays compact; Files expands instead.
            let expand_idx = if active_idx == 0 { 1 } else { active_idx };
            let Some(panel) = fl.side_panels.get(expand_idx) else {
                return false;
            };
            if panel.height == 0 {
                return false;
            }
            // Only the bottom border row — a taller strip steals clicks from the
            // last list items (content sits on bottom-1 with Borders::ALL).
            let bottom = panel.y + panel.height.saturating_sub(1);
            row == bottom
        } else if fl.side_panels.is_empty() {
            col == content.x
        } else if fl.main_panel.width == 0 {
            col == content.x + content.width.saturating_sub(1)
        } else {
            let divider_x = fl.main_panel.x;
            let lo = divider_x.saturating_sub(1);
            let hi = divider_x.saturating_add(1);
            col >= lo && col <= hi
        }
    }

    /// Rows to add when mapping a portrait grab to the side/main split.
    /// Main/diff top border: 0 (row is already the split). Expanded panel bottom:
    /// 1 + trailing collapsed panels so both grabs drive the same ratio.
    pub(super) fn portrait_sidebar_resize_offset(&self, row: u16) -> u16 {
        if self.diff_mode.active {
            return self.compute_compare_layout().resize_row_offset(row);
        }
        let fl = self.compute_current_frame_layout();
        if !fl.portrait {
            return 0;
        }
        if fl.main_panel.height > 0 && row == fl.main_panel.y {
            return 0;
        }
        let panel_count = SideWindow::ALL.len();
        let active_window = self.context_mgr.active_window();
        let active_idx = SideWindow::ALL
            .iter()
            .position(|w| *w == active_window)
            .unwrap_or(1);
        let expand_idx = if active_idx == 0 { 1 } else { active_idx };
        let collapsed: u16 = 1;
        let trailing =
            (panel_count.saturating_sub(expand_idx.saturating_add(1)) as u16) * collapsed;
        1 + trailing
    }

    pub(super) fn apply_sidebar_ratio_from_mouse(&mut self, col: u16, row: u16) {
        let content = self.content_area_rect();
        if content.width == 0 || content.height == 0 {
            return;
        }
        let portrait = if self.diff_mode.active {
            self.compute_compare_layout().portrait
        } else {
            self.compute_current_frame_layout().portrait
        };
        let ratio = if portrait {
            let side_end = row
                .saturating_sub(content.y)
                .saturating_add(self.sidebar_resize_row_offset)
                .min(content.height);
            side_end as f64 / content.height as f64
        } else {
            let pos = col.saturating_sub(content.x).min(content.width);
            pos as f64 / content.width as f64
        };
        self.layout.side_panel_ratio = ratio.clamp(0.0, 1.0);
    }

    /// True when the active context is one where commit-details makes sense
    /// (drives both the `.` toggle and layout-time `show_details`).
    pub(super) fn context_has_commit_details(&self) -> bool {
        matches!(
            self.context_mgr.active(),
            ContextId::Commits
                | ContextId::BranchCommits
                | ContextId::Reflog
                | ContextId::CommitFiles
                | ContextId::BranchCommitFiles
                | ContextId::StashFiles
        )
    }

    pub(super) fn details_panel_applies(&self) -> bool {
        if !self.show_commit_details {
            return false;
        }
        let ctx = self.context_mgr.active();
        let sel = self.context_mgr.selected(ctx);
        let model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match ctx {
            ContextId::Commits => sel < model.commits.len(),
            ContextId::BranchCommits => sel < model.sub_commits.len(),
            ContextId::Reflog => sel < model.reflog_commits.len(),
            ContextId::CommitFiles | ContextId::BranchCommitFiles | ContextId::StashFiles => {
                let hash = &self.commit_files_hash;
                !hash.is_empty()
                    && (model.commits.iter().any(|c| c.hash == *hash)
                        || model.sub_commits.iter().any(|c| c.hash == *hash)
                        || model.reflog_commits.iter().any(|c| c.hash == *hash))
            }
            _ => false,
        }
    }

    /// Compute the exact main panel Rect using the real layout engine.
    pub(super) fn compute_main_panel_rect(&self) -> ratatui::layout::Rect {
        self.compute_current_frame_layout().main_panel
    }

    pub(super) fn revert_hunk_at_position(
        &self,
        panel_rect: ratatui::layout::Rect,
        layout: &DiffPanelLayout,
        col: u16,
        row: u16,
    ) -> Option<usize> {
        if self.context_mgr.active() != ContextId::Files {
            return None;
        }
        if self.diff_view.is_empty() {
            return None;
        }
        if !rect_contains(panel_rect, col, row) {
            return None;
        }
        let divider_x = layout.divider_x()?;
        if col != divider_x && col != divider_x + 1 {
            return None;
        }
        let (line_idx, chunk_idx) = self.diff_view.line_chunk_at_row(row, layout)?;
        let diff_line = self.diff_view.lines.get(line_idx)?;
        let edit_active = self
            .diff_view
            .inline_edit
            .as_ref()
            .map(|e| e.line_idx == line_idx)
            .unwrap_or(false);
        let saved_note_count = if !self.diff_view.notes_visible {
            0
        } else if edit_active {
            let editing_id = self
                .diff_view
                .inline_edit
                .as_ref()
                .map(|e| e.editing_id.as_str())
                .unwrap_or("");
            diff_line
                .comment_notes
                .iter()
                .filter(|n| n.id != editing_id)
                .count()
        } else {
            diff_line.comment_notes.len()
        };
        let extra_rows = saved_note_count * 5 + if edit_active { 5 } else { 0 };

        if chunk_idx != extra_rows {
            return None;
        }
        self.diff_view.hunk_index_for_start_line(line_idx)
    }
}
