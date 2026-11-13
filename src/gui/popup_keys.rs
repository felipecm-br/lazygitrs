//! Keyboard input for popups, command palette, ref/list/theme pickers.

use super::*;

impl Gui {
    pub(crate) fn handle_popup_key(&mut self, key: KeyEvent) -> Result<()> {
        let was_help = matches!(self.popup, PopupState::CommandPalette { .. });
        let was_ref_picker = matches!(self.popup, PopupState::RefPicker { .. });
        let was_list_picker = matches!(self.popup, PopupState::ListPicker { .. });
        let was_theme_picker = matches!(self.popup, PopupState::ThemePicker { .. });

        match &self.popup {
            PopupState::SmartCommitPlan(_) => {
                if let Err(e) = self.handle_smart_commit_plan_key(key) {
                    self.popup = PopupState::Message {
                        title: "Smart commit".to_string(),
                        message: format!("{e:#}"),
                        kind: MessageKind::Error,
                    };
                }
            }
            PopupState::Confirm { .. } => {
                if key.code == KeyCode::Char('y') || key.code == KeyCode::Enter {
                    let popup = std::mem::replace(&mut self.popup, PopupState::None);
                    if let PopupState::Confirm { on_confirm, .. } = popup
                        && let Err(e) = on_confirm(self)
                    {
                        self.popup = PopupState::Message {
                            title: "Error".to_string(),
                            message: format!("{}", e),
                            kind: MessageKind::Error,
                        };
                    }
                } else {
                    self.popup = PopupState::None;
                }
            }
            PopupState::Message { .. } => {
                // Any key dismisses the message
                self.popup = PopupState::None;
            }
            PopupState::Menu {
                items,
                selected: _selected,
                loading_index,
                ..
            } => {
                // Block all input while a menu item is loading (except Esc)
                if loading_index.is_some() && key.code != KeyCode::Esc {
                    return Ok(());
                }
                let _items_len = items.len();
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        if let PopupState::Menu {
                            items, selected, ..
                        } = &mut self.popup
                        {
                            // Skip disabled items
                            let mut next = *selected + 1;
                            while next < items.len() && items[next].action.is_none() {
                                next += 1;
                            }
                            if next < items.len() {
                                *selected = next;
                            }
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        if let PopupState::Menu {
                            items, selected, ..
                        } = &mut self.popup
                        {
                            // Skip disabled items
                            if *selected > 0 {
                                let mut prev = *selected - 1;
                                while prev > 0 && items[prev].action.is_none() {
                                    prev -= 1;
                                }
                                if items[prev].action.is_some() {
                                    *selected = prev;
                                }
                            }
                        }
                    }
                    KeyCode::Enter => {
                        self.execute_menu_action(None);
                    }
                    KeyCode::Esc => {
                        if let Some(stashed) = self.pending_commit_popup.take() {
                            self.popup = stashed;
                        } else {
                            self.popup = PopupState::None;
                        }
                    }
                    KeyCode::Char(c) => {
                        // Check if the typed char matches a menu item shortcut key
                        let key_str = c.to_string();
                        let matched_idx = items
                            .iter()
                            .position(|item| item.key.as_deref() == Some(key_str.as_str()));
                        if let Some(idx) = matched_idx {
                            // Check if the item has an action (not disabled)
                            let has_action = items[idx].action.is_some();
                            if has_action {
                                self.execute_menu_action(Some(idx));
                            }
                            // If disabled, do nothing (stay on menu)
                        }
                        // If no match, ignore the key (stay on menu)
                    }
                    _ => {}
                }
            }
            PopupState::Input {
                is_commit,
                confirm_focused,
                ..
            } => {
                use crossterm::event::KeyModifiers;
                let is_commit = *is_commit;
                let confirm_focused = *confirm_focused;

                // Tab toggles focus between textarea and confirm button (commit only)
                if is_commit && key.code == KeyCode::Tab {
                    if let PopupState::Input {
                        confirm_focused, ..
                    } = &mut self.popup
                    {
                        *confirm_focused = !*confirm_focused;
                    }
                }
                // Confirm: Ctrl+S for commit, Enter on confirm button, Enter for non-commit
                else if (is_commit
                    && key.code == KeyCode::Char('s')
                    && key.modifiers.contains(KeyModifiers::CONTROL))
                    || (is_commit && confirm_focused && key.code == KeyCode::Enter)
                    || (!is_commit && key.code == KeyCode::Enter)
                {
                    let popup = std::mem::replace(&mut self.popup, PopupState::None);
                    if let PopupState::Input {
                        textarea,
                        on_confirm,
                        is_commit: was_commit,
                        ..
                    } = popup
                    {
                        // Commit messages preserve hard-wrapped newlines; single-line inputs
                        // strip soft-wrap newlines to recover the user's literal text.
                        let text = if was_commit {
                            textarea.lines().join("\n")
                        } else {
                            textarea.lines().join("")
                        };
                        // Save to commit history before calling on_confirm
                        if was_commit && !text.trim().is_empty() {
                            // Remove duplicate if it exists
                            self.commit_message_history.retain(|m| m != &text);
                            self.commit_message_history.insert(0, text.clone());
                            // Keep history bounded
                            self.commit_message_history.truncate(50);
                            self.save_commit_history();
                        }
                        self.commit_history_idx = None;
                        if let Err(e) = on_confirm(self, &text) {
                            self.popup = PopupState::Message {
                                title: "Error".to_string(),
                                message: format!("{}", e),
                                kind: MessageKind::Error,
                            };
                        }
                    }
                } else if key.code == KeyCode::Esc {
                    self.popup = PopupState::None;
                    self.commit_history_idx = None;
                } else if is_commit
                    && !confirm_focused
                    && (key.code == KeyCode::Up || key.code == KeyCode::Down)
                    && !self.commit_message_history.is_empty()
                {
                    // Cycle through commit message history with Up/Down
                    if let PopupState::Input { textarea, .. } = &mut self.popup {
                        // Only cycle if on first line (Up) or last line (Down)
                        let cursor_row = textarea.cursor().0;
                        let line_count = textarea.lines().len();
                        let should_cycle = match key.code {
                            KeyCode::Up => cursor_row == 0,
                            KeyCode::Down => cursor_row >= line_count.saturating_sub(1),
                            _ => false,
                        };

                        if should_cycle {
                            let history_len = self.commit_message_history.len();
                            match key.code {
                                KeyCode::Up => {
                                    let new_idx = match self.commit_history_idx {
                                        None => {
                                            // Save current draft
                                            self.commit_history_draft = textarea.lines().join("\n");
                                            0
                                        }
                                        Some(idx) => (idx + 1).min(history_len - 1),
                                    };
                                    self.commit_history_idx = Some(new_idx);
                                    let msg = &self.commit_message_history[new_idx];
                                    let mut new_ta =
                                        popup::make_textarea("Enter commit message...");
                                    new_ta.insert_str(msg);
                                    *textarea = new_ta;
                                }
                                KeyCode::Down => {
                                    match self.commit_history_idx {
                                        Some(0) => {
                                            // Go back to draft
                                            self.commit_history_idx = None;
                                            let draft = self.commit_history_draft.clone();
                                            let mut new_ta =
                                                popup::make_textarea("Enter commit message...");
                                            new_ta.insert_str(&draft);
                                            *textarea = new_ta;
                                        }
                                        Some(idx) => {
                                            let new_idx = idx - 1;
                                            self.commit_history_idx = Some(new_idx);
                                            let msg = &self.commit_message_history[new_idx];
                                            let mut new_ta =
                                                popup::make_textarea("Enter commit message...");
                                            new_ta.insert_str(msg);
                                            *textarea = new_ta;
                                        }
                                        None => {
                                            // Already at draft, do nothing
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            // Not at boundary — forward to textarea for normal cursor movement
                            textarea_input(textarea, key);
                        }
                    }
                } else if is_commit
                    && !confirm_focused
                    && matches_key(
                        key,
                        &self
                            .config
                            .user_config
                            .keybinding
                            .commit_message
                            .commit_menu,
                    )
                {
                    // Commit message editor menu key (configurable)
                    self.show_commit_editor_menu()?;
                } else if !confirm_focused {
                    // Forward all other keys to the textarea (only when textarea is focused)
                    if let PopupState::Input {
                        textarea,
                        is_commit,
                        ..
                    } = &mut self.popup
                    {
                        textarea_input(textarea, key);
                        let popup_width = (self.layout.width * 60 / 100)
                            .clamp(30, 60)
                            .min(self.layout.width);
                        let popup_inner = popup_width.saturating_sub(4) as usize;
                        if *is_commit {
                            // Hard-wrap: line breaks become part of the committed message
                            // (matches lazygit's 72-char convention).
                            let config_width = self.config.user_config.git.commit.auto_wrap_width;
                            let effective_width = if config_width > 0 {
                                popup_inner.min(config_width)
                            } else {
                                popup_inner
                            };
                            if effective_width > 0 {
                                auto_wrap_textarea(textarea, effective_width);
                            }
                        } else if popup_inner > 0 {
                            // Soft-wrap: visual only — newlines are stripped on submit so
                            // the original text (including spaces) round-trips exactly.
                            soft_wrap_textarea(textarea, popup_inner);
                        }
                    }
                }
            }
            PopupState::HunkCommentary { .. } => {
                use crossterm::event::KeyModifiers;
                if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    let popup = std::mem::replace(&mut self.popup, PopupState::None);
                    if let PopupState::HunkCommentary {
                        textarea,
                        file_path,
                        hunk_diff,
                        on_confirm,
                        ..
                    } = popup
                    {
                        let text = textarea.lines().join("\n");
                        if let Err(e) = on_confirm(self, &text, &file_path, &hunk_diff) {
                            self.popup = PopupState::Message {
                                title: "Error".to_string(),
                                message: format!("{}", e),
                                kind: MessageKind::Error,
                            };
                        }
                    }
                } else if key.code == KeyCode::Esc {
                    self.popup = PopupState::None;
                } else {
                    if let PopupState::HunkCommentary { textarea, .. } = &mut self.popup {
                        textarea_input(textarea, key);
                        let popup_width = (self.layout.width * 60 / 100)
                            .clamp(30, 60)
                            .min(self.layout.width);
                        let popup_inner = popup_width.saturating_sub(4) as usize;
                        if popup_inner > 0 {
                            soft_wrap_textarea(textarea, popup_inner);
                        }
                    }
                }
            }
            PopupState::CommitInput { focus, .. } => {
                use crossterm::event::KeyModifiers;
                let focus = *focus;

                // Tab toggles focus between summary and body
                if key.code == KeyCode::Tab {
                    if let PopupState::CommitInput {
                        focus,
                        summary_textarea,
                        body_textarea,
                        ..
                    } = &mut self.popup
                    {
                        *focus = match *focus {
                            popup::CommitInputFocus::Summary => popup::CommitInputFocus::Body,
                            popup::CommitInputFocus::Body => popup::CommitInputFocus::Summary,
                        };
                        // Update cursor visibility based on focus
                        let visible = ratatui::style::Style::default()
                            .add_modifier(ratatui::style::Modifier::REVERSED);
                        let hidden = ratatui::style::Style::default();
                        match *focus {
                            popup::CommitInputFocus::Summary => {
                                summary_textarea.set_cursor_style(visible);
                                body_textarea.set_cursor_style(hidden);
                            }
                            popup::CommitInputFocus::Body => {
                                summary_textarea.set_cursor_style(hidden);
                                body_textarea.set_cursor_style(visible);
                            }
                        }
                    }
                }
                // Insert a newline in the body:
                //   - Enter while focused on Body (the natural keystroke for a multi-line field).
                //   - Shift+Enter from Summary jumps focus to Body and inserts a newline.
                //   - Ctrl+J (some terminals emit this for Shift+Enter) — without this branch it
                //     would hit ratatui_textarea's default `delete_line_by_head` binding.
                else if (key.code == KeyCode::Enter
                    && (focus == popup::CommitInputFocus::Body
                        || key.modifiers.contains(KeyModifiers::SHIFT)))
                    || (key.code == KeyCode::Char('j')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    let wrap_width = self.commit_body_wrap_width();
                    if let PopupState::CommitInput {
                        focus,
                        summary_textarea,
                        body_textarea,
                        body_state,
                        ..
                    } = &mut self.popup
                    {
                        if *focus == popup::CommitInputFocus::Summary {
                            *focus = popup::CommitInputFocus::Body;
                            let visible = ratatui::style::Style::default()
                                .add_modifier(ratatui::style::Modifier::REVERSED);
                            let hidden = ratatui::style::Style::default();
                            summary_textarea.set_cursor_style(hidden);
                            body_textarea.set_cursor_style(visible);
                        }
                        body_state.insert_char('\n');
                        body_state.render_into(body_textarea, wrap_width);
                    }
                }
                // Enter on summary: submit the commit
                else if focus == popup::CommitInputFocus::Summary && key.code == KeyCode::Enter {
                    let popup = std::mem::replace(&mut self.popup, PopupState::None);
                    if let PopupState::CommitInput {
                        summary_textarea,
                        body_state,
                        on_confirm,
                        ..
                    } = popup
                    {
                        let summary = summary_textarea.lines().join("");
                        let body = body_state.raw().trim().to_string();
                        let text = if body.is_empty() {
                            summary
                        } else {
                            format!("{}\n\n{}", summary, body)
                        };
                        // Save to commit history
                        if !text.trim().is_empty() {
                            self.commit_message_history.retain(|m| m != &text);
                            self.commit_message_history.insert(0, text.clone());
                            self.commit_message_history.truncate(50);
                            self.save_commit_history();
                        }
                        self.commit_history_idx = None;
                        // Successful submit: drop any stashed in-progress editor.
                        self.saved_commit_popup = None;
                        if let Err(e) = on_confirm(self, &text) {
                            self.popup = PopupState::Message {
                                title: "Error".to_string(),
                                message: format!("{}", e),
                                kind: MessageKind::Error,
                            };
                        }
                    }
                }
                // Esc: stash editor so re-opening commit prompt restores in-progress text.
                else if key.code == KeyCode::Esc {
                    let stashed = std::mem::replace(&mut self.popup, PopupState::None);
                    self.saved_commit_popup = Some(stashed);
                    self.commit_history_idx = None;
                }
                // Open commit menu key (configurable)
                else if matches_key(
                    key,
                    &self
                        .config
                        .user_config
                        .keybinding
                        .commit_message
                        .commit_menu,
                ) {
                    self.show_commit_editor_menu()?;
                }
                // AI generate key (configurable)
                else if matches_key(
                    key,
                    &self
                        .config
                        .user_config
                        .keybinding
                        .commit_message
                        .ai_generate,
                ) {
                    self.trigger_ai_commit_generation_from_editor();
                }
                // Up/Down on summary: cycle commit history
                else if focus == popup::CommitInputFocus::Summary
                    && (key.code == KeyCode::Up || key.code == KeyCode::Down)
                    && !self.commit_message_history.is_empty()
                {
                    let wrap_width = self.commit_body_wrap_width();
                    if let PopupState::CommitInput {
                        summary_textarea,
                        body_textarea,
                        body_state,
                        ..
                    } = &mut self.popup
                    {
                        let history_len = self.commit_message_history.len();
                        let load_msg =
                            |summary_textarea: &mut ratatui_textarea::TextArea<'static>,
                             body_textarea: &mut ratatui_textarea::TextArea<'static>,
                             body_state: &mut popup::BodySoftWrap,
                             msg: &str| {
                                let (summary, body) = split_commit_message(msg);
                                popup::set_commit_summary_text(summary_textarea, &summary);
                                *body_textarea = popup::make_commit_body_textarea();
                                // Preserve history newlines; soft-wrap is display-only.
                                body_state.set_text(popup::unwrap_commit_body(&body));
                                body_state.render_into(body_textarea, wrap_width);
                            };
                        match key.code {
                            KeyCode::Up => {
                                let new_idx = match self.commit_history_idx {
                                    None => {
                                        // Save current draft
                                        let s = summary_textarea.lines().join("");
                                        let b = body_state.raw().to_string();
                                        self.commit_history_draft = if b.trim().is_empty() {
                                            s
                                        } else {
                                            format!("{}\n\n{}", s, b)
                                        };
                                        0
                                    }
                                    Some(idx) => (idx + 1).min(history_len - 1),
                                };
                                self.commit_history_idx = Some(new_idx);
                                let msg = self.commit_message_history[new_idx].clone();
                                load_msg(summary_textarea, body_textarea, body_state, &msg);
                            }
                            KeyCode::Down => match self.commit_history_idx {
                                Some(0) => {
                                    self.commit_history_idx = None;
                                    let draft = self.commit_history_draft.clone();
                                    load_msg(summary_textarea, body_textarea, body_state, &draft);
                                }
                                Some(idx) => {
                                    let new_idx = idx - 1;
                                    self.commit_history_idx = Some(new_idx);
                                    let msg = self.commit_message_history[new_idx].clone();
                                    load_msg(summary_textarea, body_textarea, body_state, &msg);
                                }
                                None => {}
                            },
                            _ => {}
                        }
                    }
                }
                // All other keys: forward to the focused textarea
                else {
                    let wrap_width = self.commit_body_wrap_width();
                    if let PopupState::CommitInput {
                        summary_textarea,
                        body_textarea,
                        body_state,
                        focus,
                        ..
                    } = &mut self.popup
                    {
                        match focus {
                            popup::CommitInputFocus::Summary => {
                                textarea_input(summary_textarea, key);
                            }
                            popup::CommitInputFocus::Body => {
                                // Body is driven by body_state; body_textarea is a soft-wrapped
                                // projection. Content edits rebuild the projection; pure cursor
                                // moves only Jump so the viewport is preserved (Up only scrolls
                                // when the cursor is already on the first visible row).
                                let mut content_changed = false;
                                let mut cursor_moved = false;
                                let alt = key.modifiers.contains(KeyModifiers::ALT);
                                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                                let cmd = has_command_modifier(key.modifiers);
                                match key.code {
                                    KeyCode::Char(c) if !ctrl && !alt && !cmd => {
                                        body_state.insert_char(c);
                                        content_changed = true;
                                    }
                                    // Cmd+Backspace / Ctrl+U: delete to start of visual line.
                                    // Most macOS terminals (Zed, WezTerm, …) intercept Cmd and
                                    // never forward it to the app, so the readline shortcut is
                                    // the only one that works everywhere.
                                    KeyCode::Backspace if cmd => {
                                        body_state.delete_to_visual_line_start(wrap_width);
                                        content_changed = true;
                                    }
                                    KeyCode::Char('u') if ctrl => {
                                        body_state.delete_to_visual_line_start(wrap_width);
                                        content_changed = true;
                                    }
                                    // Opt+Backspace / Ctrl+W: delete previous word.
                                    KeyCode::Backspace if alt => {
                                        body_state.delete_word_left();
                                        content_changed = true;
                                    }
                                    KeyCode::Char('w') if ctrl => {
                                        body_state.delete_word_left();
                                        content_changed = true;
                                    }
                                    KeyCode::Backspace => {
                                        body_state.backspace();
                                        content_changed = true;
                                    }
                                    KeyCode::Delete => {
                                        body_state.delete();
                                        content_changed = true;
                                    }
                                    // Cmd+Left/Right and Ctrl+A/E: jump to start/end of visual
                                    // row. Same reason as Cmd+Backspace — Ctrl is the portable
                                    // binding.
                                    KeyCode::Left if cmd => {
                                        body_state.move_visual_line_start(wrap_width);
                                        cursor_moved = true;
                                    }
                                    KeyCode::Right if cmd => {
                                        body_state.move_visual_line_end(wrap_width);
                                        cursor_moved = true;
                                    }
                                    KeyCode::Char('a') if ctrl => {
                                        body_state.move_visual_line_start(wrap_width);
                                        cursor_moved = true;
                                    }
                                    KeyCode::Char('e') if ctrl => {
                                        body_state.move_visual_line_end(wrap_width);
                                        cursor_moved = true;
                                    }
                                    // Opt+Left/Right: jump by word (matches the new-branch input
                                    // and the rest of the readline-style world).
                                    KeyCode::Left if alt => {
                                        body_state.move_word_left();
                                        cursor_moved = true;
                                    }
                                    KeyCode::Right if alt => {
                                        body_state.move_word_right();
                                        cursor_moved = true;
                                    }
                                    KeyCode::Char('b') if alt => {
                                        body_state.move_word_left();
                                        cursor_moved = true;
                                    }
                                    KeyCode::Char('f') if alt => {
                                        body_state.move_word_right();
                                        cursor_moved = true;
                                    }
                                    KeyCode::Left => {
                                        body_state.move_left();
                                        cursor_moved = true;
                                    }
                                    KeyCode::Right => {
                                        body_state.move_right();
                                        cursor_moved = true;
                                    }
                                    KeyCode::Up => {
                                        body_state.move_visual_up(wrap_width);
                                        cursor_moved = true;
                                    }
                                    KeyCode::Down => {
                                        body_state.move_visual_down(wrap_width);
                                        cursor_moved = true;
                                    }
                                    KeyCode::Home => {
                                        body_state.move_home();
                                        cursor_moved = true;
                                    }
                                    KeyCode::End => {
                                        body_state.move_end();
                                        cursor_moved = true;
                                    }
                                    _ => {}
                                }
                                if content_changed {
                                    body_state.render_into(body_textarea, wrap_width);
                                } else if cursor_moved {
                                    body_state.apply_cursor_into(body_textarea, wrap_width);
                                }
                            }
                        }
                    }
                }
            }
            PopupState::Checklist {
                items,
                selected: _,
                search_textarea,
                ..
            } => {
                let search = search_textarea.lines().join("");
                let visible_count = items
                    .iter()
                    .filter(|it| {
                        it.is_free_entry
                            || search.is_empty()
                            || it.label.to_lowercase().contains(&search.to_lowercase())
                    })
                    .count();
                match key.code {
                    // Arrow keys only for navigation — j/k must type into the search filter
                    // (same pattern as ListPicker / CommandPalette / ThemePicker).
                    KeyCode::Down if key.modifiers.is_empty() => {
                        if let PopupState::Checklist { selected, .. } = &mut self.popup
                            && visible_count > 0
                        {
                            *selected = (*selected + 1).min(visible_count - 1);
                        }
                    }
                    KeyCode::Up if key.modifiers.is_empty() => {
                        if let PopupState::Checklist { selected, .. } = &mut self.popup {
                            *selected = selected.saturating_sub(1);
                        }
                    }
                    KeyCode::Char(' ') if key.modifiers.is_empty() => {
                        // Toggle checked state on the visible item at `selected`
                        if let PopupState::Checklist {
                            items,
                            selected,
                            search_textarea,
                            ..
                        } = &mut self.popup
                        {
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
                            if let Some(&real_idx) = visible_indices.get(*selected) {
                                items[real_idx].checked = !items[real_idx].checked;
                            }
                        }
                    }
                    KeyCode::Enter => {
                        let popup = std::mem::replace(&mut self.popup, PopupState::None);
                        if let PopupState::Checklist {
                            items,
                            selected,
                            search_textarea,
                            on_confirm,
                            ..
                        } = popup
                        {
                            let search = search_textarea.lines().join("");
                            let mut checked: Vec<String> = items
                                .iter()
                                .filter(|it| it.checked)
                                .map(|it| it.label.clone())
                                .collect();
                            if checked.is_empty() {
                                let visible_indices: Vec<usize> = items
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, it)| {
                                        search.is_empty()
                                            || it
                                                .label
                                                .to_lowercase()
                                                .contains(&search.to_lowercase())
                                    })
                                    .map(|(i, _)| i)
                                    .collect();
                                if let Some(&real_idx) = visible_indices.get(selected) {
                                    checked.push(items[real_idx].label.clone());
                                }
                            }
                            if let Err(e) = on_confirm(self, checked) {
                                self.popup = PopupState::Message {
                                    title: "Error".to_string(),
                                    message: format!("{}", e),
                                    kind: MessageKind::Error,
                                };
                            }
                        }
                    }
                    KeyCode::Esc => {
                        self.popup = PopupState::None;
                    }
                    _ => {
                        // Search input: chars, backspace, Option/Cmd word/line edits.
                        if let PopupState::Checklist {
                            items,
                            selected,
                            search_textarea,
                            free_entry_category,
                            ..
                        } = &mut self.popup
                        {
                            let before = search_textarea.lines().join("");
                            textarea_input(search_textarea, key);
                            let after = search_textarea.lines().join("");
                            if after != before {
                                crate::gui::popup::sync_checklist_free_entry(
                                    items,
                                    free_entry_category.as_deref(),
                                    &after,
                                );
                                *selected = 0;
                            }
                        }
                    }
                }
            }
            PopupState::Loading { .. } => {
                // Block all input while loading — user must wait
            }
            PopupState::CommandPalette { .. } => {}
            PopupState::RefPicker { .. } => {}
            PopupState::ListPicker { .. } => {}
            PopupState::ThemePicker { .. } => {}
            PopupState::None => {}
        }

        // These are handled separately to avoid borrow conflicts.
        // Use else-if so that a handler that transitions to another popup
        // (e.g. Help → ThemePicker on Enter) does not also fire the new
        // popup's handler with the same key event.
        if was_help && matches!(self.popup, PopupState::CommandPalette { .. }) {
            self.handle_command_palette_key(key)?;
        } else if was_ref_picker && matches!(self.popup, PopupState::RefPicker { .. }) {
            self.handle_ref_picker_key(key)?;
        } else if was_list_picker && matches!(self.popup, PopupState::ListPicker { .. }) {
            self.handle_list_picker_key(key)?;
        } else if was_theme_picker && matches!(self.popup, PopupState::ThemePicker { .. }) {
            self.handle_theme_picker_key(key);
        }

        Ok(())
    }

    pub(super) fn handle_command_palette_key(&mut self, key: KeyEvent) -> Result<()> {
        // Helper: compute display index for a given entry selection
        fn find_display_idx(sections: &[CommandSection], sel: usize, search: &str) -> usize {
            use crate::gui::popup::{command_palette_entry_matches, list_picker_search_tokens};
            let tokens = list_picker_search_tokens(search);
            let has_search = !tokens.is_empty();
            let mut ei = 0usize;
            let mut di = 0usize;
            for section in sections {
                let mut section_has_visible = false;
                for entry in &section.entries {
                    let matches = !has_search
                        || command_palette_entry_matches(&entry.key, &entry.description, &tokens);
                    if matches {
                        if !section_has_visible {
                            section_has_visible = true;
                            di += 1; // header row
                        }
                        if ei == sel {
                            return di;
                        }
                        ei += 1;
                        di += 1;
                    }
                }
            }
            di
        }

        fn count_visible(sections: &[CommandSection], search: &str) -> usize {
            use crate::gui::popup::{command_palette_entry_matches, list_picker_search_tokens};
            let tokens = list_picker_search_tokens(search);
            let has_search = !tokens.is_empty();
            sections
                .iter()
                .map(|s| {
                    if has_search {
                        s.entries
                            .iter()
                            .filter(|e| {
                                command_palette_entry_matches(&e.key, &e.description, &tokens)
                            })
                            .count()
                    } else {
                        s.entries.len()
                    }
                })
                .sum()
        }

        let mut selected_action = None;

        if let PopupState::CommandPalette {
            sections,
            selected,
            search_textarea,
            scroll_offset,
        } = &mut self.popup
        {
            use crossterm::event::KeyModifiers;
            let search = search_textarea.lines().join("");
            let search_lower = search.to_lowercase();
            // `search_lower` kept for scroll math below; filtering uses tokens.

            // Estimate list viewport height from terminal
            let popup_height = (self.layout.height as usize).saturating_sub(4).min(50);
            let list_height = popup_height.saturating_sub(5); // borders + search + sep + hint

            match key.code {
                KeyCode::Esc | KeyCode::Char('?')
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    self.popup = PopupState::None;
                    return Ok(());
                }
                KeyCode::Enter => {
                    use crate::gui::popup::{
                        command_palette_entry_matches, list_picker_search_tokens,
                    };
                    let tokens = list_picker_search_tokens(&search);
                    let has_search = !tokens.is_empty();
                    let mut ei = 0usize;
                    'outer: for section in sections.iter() {
                        for entry in &section.entries {
                            let vis = !has_search
                                || command_palette_entry_matches(
                                    &entry.key,
                                    &entry.description,
                                    &tokens,
                                );
                            if vis {
                                if ei == *selected {
                                    selected_action = Some(entry.action.clone());
                                    break 'outer;
                                }
                                ei += 1;
                            }
                        }
                    }
                }
                KeyCode::Down => {
                    let total = count_visible(sections, &search_lower);
                    if total > 0 {
                        *selected = (*selected + 1).min(total.saturating_sub(1));
                    }
                    let sdi = find_display_idx(sections, *selected, &search_lower);
                    if sdi >= *scroll_offset + list_height {
                        *scroll_offset = sdi.saturating_sub(list_height - 1);
                    }
                }
                KeyCode::Up => {
                    *selected = selected.saturating_sub(1);
                    if *selected == 0 {
                        // First item: always scroll to top so the section header is visible
                        *scroll_offset = 0;
                    } else {
                        let sdi = find_display_idx(sections, *selected, &search_lower);
                        if sdi <= *scroll_offset {
                            // Scroll up to show the section header too when possible
                            *scroll_offset = sdi.saturating_sub(1);
                        }
                    }
                }
                _ => {
                    textarea_input(search_textarea, key);
                    let new_search = search_textarea.lines().join("");
                    if new_search != search {
                        *selected = 0;
                        *scroll_offset = 0;
                    }
                }
            }
        }

        if let Some(action) = selected_action {
            match action {
                CommandAction::Dispatch(key) => {
                    self.popup = PopupState::None;
                    self.handle_key(key)?;
                }
                CommandAction::OpenThemePicker => {
                    self.popup = PopupState::None;
                    self.show_theme_picker();
                }
                CommandAction::Unavailable => {}
            }
        }

        Ok(())
    }

    pub(super) fn handle_ref_picker_key(&mut self, key: KeyEvent) -> Result<()> {
        use crate::gui::popup::{
            REF_FREE_ENTRY_CATEGORY, list_picker_confirm_value, sync_list_picker_free_entry,
        };

        if let PopupState::RefPicker { core, .. } = &mut self.popup {
            let search = core.search_textarea.lines().join("");
            let matching = list_picker_matching_indices(&core.items, &search);

            let h = self.layout.height as usize;
            let list_height = list_picker_visible_height(h);

            match key.code {
                KeyCode::Esc => {
                    self.popup = PopupState::None;
                    return Ok(());
                }
                KeyCode::Enter => {
                    let Some(value) = list_picker_confirm_value(core) else {
                        return Ok(());
                    };
                    let popup = std::mem::replace(&mut self.popup, PopupState::None);
                    if let PopupState::RefPicker { on_confirm, .. } = popup
                        && let Err(e) = on_confirm(self, &value)
                    {
                        self.popup = PopupState::Message {
                            title: "Error".to_string(),
                            message: format!("{}", e),
                            kind: MessageKind::Error,
                        };
                    }
                    return Ok(());
                }
                KeyCode::Down => {
                    if let Some(next) = list_picker_next_match(&matching, core.selected) {
                        core.selected = next;
                    }
                    list_picker_scroll_after_nav(core, &matching, list_height, true);
                }
                KeyCode::Up => {
                    if let Some(prev) = list_picker_prev_match(&matching, core.selected) {
                        core.selected = prev;
                    }
                    list_picker_scroll_after_nav(core, &matching, list_height, false);
                }
                _ => {
                    textarea_input(&mut core.search_textarea, key);
                    let new_search = core.search_textarea.lines().join("");
                    if new_search != search {
                        sync_list_picker_free_entry(core, REF_FREE_ENTRY_CATEGORY);
                        let matching = list_picker_matching_indices(&core.items, &new_search);
                        if let Some(sel) =
                            list_picker_clamp_selection_to_matches(&matching, core.selected)
                        {
                            core.selected = sel;
                        }
                        if !new_search.is_empty() {
                            let sdi = list_picker_filtered_display_idx(
                                &core.items,
                                &matching,
                                core.selected,
                            );
                            core.scroll_offset = sdi.saturating_sub(list_height / 2);
                        } else {
                            core.scroll_offset = 0;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Handle keys for the generic free-entry [`PopupState::ListPicker`].
    /// Same navigation/search semantics as RefPicker, with a configurable free-entry category.
    pub(super) fn handle_list_picker_key(&mut self, key: KeyEvent) -> Result<()> {
        use crate::gui::popup::{list_picker_confirm_value, sync_list_picker_prefer_free_entry};

        if let PopupState::ListPicker {
            core,
            free_entry_category,
            ..
        } = &mut self.popup
        {
            let search = core.search_textarea.lines().join("");
            let matching = list_picker_matching_indices(&core.items, &search);
            let free_cat = free_entry_category.clone();

            let h = self.layout.height as usize;
            let list_height = list_picker_visible_height(h);

            match key.code {
                KeyCode::Esc => {
                    self.popup = PopupState::None;
                    return Ok(());
                }
                KeyCode::Enter => {
                    // Empty free-entry category (e.g. diff-grep) confirms real
                    // matches only: zero matches = no-op instead of jumping to
                    // a stale selection.
                    if free_cat.is_empty() && !matching.contains(&core.selected) {
                        return Ok(());
                    }
                    let Some(value) = list_picker_confirm_value(core) else {
                        return Ok(());
                    };
                    let popup = std::mem::replace(&mut self.popup, PopupState::None);
                    if let PopupState::ListPicker { on_confirm, .. } = popup
                        && let Err(e) = on_confirm(self, &value)
                    {
                        self.popup = PopupState::Message {
                            title: "Error".to_string(),
                            message: format!("{}", e),
                            kind: MessageKind::Error,
                        };
                    }
                    return Ok(());
                }
                KeyCode::Down => {
                    if let Some(next) = list_picker_next_match(&matching, core.selected) {
                        core.selected = next;
                    }
                    list_picker_scroll_after_nav(core, &matching, list_height, true);
                }
                KeyCode::Up => {
                    if let Some(prev) = list_picker_prev_match(&matching, core.selected) {
                        core.selected = prev;
                    }
                    list_picker_scroll_after_nav(core, &matching, list_height, false);
                }
                _ => {
                    textarea_input(&mut core.search_textarea, key);
                    let new_search = core.search_textarea.lines().join("");
                    if new_search != search {
                        sync_list_picker_prefer_free_entry(core, &free_cat);
                        let matching = list_picker_matching_indices(&core.items, &new_search);
                        if let Some(sel) =
                            list_picker_clamp_selection_to_matches(&matching, core.selected)
                        {
                            core.selected = sel;
                        }
                        if !new_search.is_empty() {
                            let sdi = list_picker_filtered_display_idx(
                                &core.items,
                                &matching,
                                core.selected,
                            );
                            core.scroll_offset = sdi.saturating_sub(list_height / 2);
                        } else {
                            core.scroll_offset = 0;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Open a reusable free-entry list picker (path/author filters, etc.).
    ///
    /// Callers supply candidate items and a confirm callback. Typing always
    /// inserts a synthetic free-entry row under `free_entry_category` so the
    /// user can confirm arbitrary text even when it does not match a candidate.
    pub fn show_list_picker(
        &mut self,
        title: impl Into<String>,
        items: Vec<crate::gui::popup::ListPickerItem>,
        free_entry_category: impl Into<String>,
        on_confirm: crate::gui::popup::ListPickerAction,
    ) {
        use crate::gui::popup::{ListPickerCore, make_command_palette_search_textarea};

        self.popup = PopupState::ListPicker {
            title: title.into(),
            core: ListPickerCore {
                items,
                selected: 0,
                search_textarea: make_command_palette_search_textarea(),
                scroll_offset: 0,
            },
            free_entry_category: free_entry_category.into(),
            on_confirm,
        };
    }

    pub(super) fn handle_theme_picker_key(&mut self, key: KeyEvent) {
        if let PopupState::ThemePicker {
            core,
            original_theme_index,
        } = &mut self.popup
        {
            let search = core.search_textarea.lines().join("");
            let matching = list_picker_matching_indices(&core.items, &search);

            let h = self.layout.height as usize;
            let list_height = list_picker_visible_height(h);

            match key.code {
                KeyCode::Esc => {
                    self.current_theme_index = *original_theme_index;
                    self.popup = PopupState::None;
                }
                KeyCode::Enter => {
                    let idx = core.selected;
                    self.popup = PopupState::None;
                    self.current_theme_index = idx;
                    if let Some(ct) = crate::config::COLOR_THEMES.get(idx) {
                        let mut state = self.config.app_state.clone();
                        state.color_theme = Some(ct.id.to_string());
                        let _ = state.save(&self.config.state_path);
                    }
                }
                KeyCode::Down => {
                    if let Some(next) = list_picker_next_match(&matching, core.selected) {
                        core.selected = next;
                    }
                    self.current_theme_index = core.selected;
                    list_picker_scroll_after_nav(core, &matching, list_height, true);
                }
                KeyCode::Up => {
                    if let Some(prev) = list_picker_prev_match(&matching, core.selected) {
                        core.selected = prev;
                    }
                    self.current_theme_index = core.selected;
                    list_picker_scroll_after_nav(core, &matching, list_height, false);
                }
                _ => {
                    // Search/filter — keep selection within matching themes
                    textarea_input(&mut core.search_textarea, key);
                    let new_search = core.search_textarea.lines().join("");
                    if new_search != search {
                        let matching = list_picker_matching_indices(&core.items, &new_search);
                        if !new_search.trim().is_empty() {
                            if let Some(sel) =
                                list_picker_clamp_selection_to_matches(&matching, core.selected)
                            {
                                core.selected = sel;
                                self.current_theme_index = sel;
                                let sdi = list_picker_filtered_display_idx(
                                    &core.items,
                                    &matching,
                                    core.selected,
                                );
                                core.scroll_offset = sdi.saturating_sub(list_height / 2);
                            }
                        } else {
                            core.selected = *original_theme_index;
                            self.current_theme_index = *original_theme_index;
                            core.scroll_offset =
                                original_theme_index.saturating_sub(list_height / 2);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn show_theme_picker(&mut self) {
        use crate::gui::popup::{
            ListPickerCore, ListPickerItem, make_command_palette_search_textarea,
        };

        let original = self.current_theme_index;
        let items: Vec<ListPickerItem> = crate::config::COLOR_THEMES
            .iter()
            .map(|ct| ListPickerItem {
                value: ct.id.to_string(),
                label: ct.name.to_string(),
                category: String::new(),
                description: Some(ct.appearance.as_str().to_string()),
            })
            .collect();

        self.popup = PopupState::ThemePicker {
            core: ListPickerCore {
                items,
                selected: original,
                search_textarea: make_command_palette_search_textarea(),
                scroll_offset: 0,
            },
            original_theme_index: original,
        };
    }
}
