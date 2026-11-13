//! Popup rendering (confirm, menu, input, commit editor, list pickers) and popup hit-testing geometry.

use super::*;

pub(super) fn wrap_popup_lines(message: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    message
        .lines()
        .flat_map(|line| {
            if line.is_empty() {
                vec![String::new()]
            } else {
                textwrap::wrap(line, width)
                    .into_iter()
                    .map(|line| line.into_owned())
                    .collect()
            }
        })
        .collect()
}

pub(super) fn visible_popup_lines(wrapped: &[String], max_lines: usize) -> Vec<String> {
    if wrapped.len() <= max_lines {
        return wrapped.to_vec();
    }
    if max_lines == 0 {
        return Vec::new();
    }
    if max_lines == 1 {
        return vec![format!("... {} more lines", wrapped.len())];
    }

    let mut visible: Vec<String> = wrapped.iter().take(max_lines - 1).cloned().collect();
    let remaining = wrapped.len() - visible.len();
    visible.push(format!("... {} more lines", remaining));
    visible
}

pub(super) fn clamped_popup_height(line_count: usize, fixed_rows: u16, area_height: u16) -> u16 {
    let line_rows = u16::try_from(line_count).unwrap_or(u16::MAX);
    line_rows.saturating_add(fixed_rows).min(area_height)
}

pub(super) fn centered_popup_x_width(area: Rect) -> (u16, u16) {
    let popup_width = (area.width * 60 / 100).clamp(30, 60).min(area.width);
    let x = (area.width.saturating_sub(popup_width)) / 2;
    (x, popup_width)
}

/// Returns the menu option index under `(col, row)`, matching `render_popup` layout.
pub fn menu_item_at(popup: &PopupState, area: Rect, col: u16, row: u16) -> Option<usize> {
    let PopupState::Menu { items, .. } = popup else {
        return None;
    };
    if items.is_empty() || area.width < 4 || area.height < 4 {
        return None;
    }
    let (x, popup_width) = centered_popup_x_width(area);
    // Matches `PopupState::Menu` in `render_popup`.
    let height = (items.len() as u16 + 2).min(area.height.saturating_sub(4));
    if height < 3 {
        return None;
    }
    let y = (area.height.saturating_sub(height)) / 2;
    if col < x || col >= x + popup_width {
        return None;
    }
    let list_start = y + 1; // below top border
    let list_rows = (height - 2) as usize; // exclude borders
    if row < list_start || row >= list_start + list_rows as u16 {
        return None;
    }
    let idx = (row - list_start) as usize;
    if idx < items.len() && idx < list_rows {
        Some(idx)
    } else {
        None
    }
}

/// Returns the visible checklist option index under `(col, row)`.
pub fn checklist_item_at(popup: &PopupState, area: Rect, col: u16, row: u16) -> Option<usize> {
    let PopupState::Checklist {
        items,
        search_textarea,
        ..
    } = popup
    else {
        return None;
    };
    let search = search_textarea.lines().join("");
    if area.width < 4 || area.height < 4 {
        return None;
    }
    let (x, popup_width) = centered_popup_x_width(area);
    let visible_count = items
        .iter()
        .filter(|it| {
            it.is_free_entry
                || search.is_empty()
                || it.label.to_lowercase().contains(&search.to_lowercase())
        })
        .count();
    // Matches `PopupState::Checklist` in `render_popup`.
    let content_lines = visible_count.max(1);
    let height = (content_lines as u16 + 6)
        .min(area.height.saturating_sub(4))
        .max(8.min(area.height));
    if height < 3 {
        return None;
    }
    let y = (area.height.saturating_sub(height)) / 2;
    if col < x || col >= x + popup_width {
        return None;
    }
    // inner.y = y+1; list starts at inner.y+2 (after search + separator)
    let list_start = y + 1 + 2;
    let inner_height = height.saturating_sub(2);
    let list_height = inner_height.saturating_sub(3); // search + sep + hint
    if list_height == 0 {
        return None;
    }
    if row < list_start || row >= list_start + list_height {
        return None;
    }
    let idx = (row - list_start) as usize;
    if idx < visible_count { Some(idx) } else { None }
}

pub(super) const SPINNER_CHARS: &[char] = &['·', '✻', '✽', '✶', '✳', '✢'];

/// Geometry of the ✦ AI-generate button shown inside the commit-message popup.
/// The button sits on the "Summary" label row, right-aligned inside the popup
/// box (not on the border).
pub fn commit_ai_button_geometry(popup: &PopupState, area: Rect) -> Option<Rect> {
    if area.width < 10 || area.height < 6 {
        return None;
    }
    let popup_width = (area.width * 60 / 100).clamp(30, 60).min(area.width);
    if popup_width < 16 {
        return None;
    }
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let ta_height: u16 = match popup {
        PopupState::CommitInput { .. } => 16,
        _ => return None,
    };
    let ta_height = ta_height.min(area.height);
    if ta_height < 7 {
        return None;
    }
    let ta_y = (area.height.saturating_sub(ta_height)) / 2;

    // Button: 3 cells (" ✦ ") on the Summary label row (inner.y = ta_y + 1),
    // right-aligned within the inner area (1 col border + 1 col right padding).
    let btn_w: u16 = 3;
    let btn_x = x + popup_width.saturating_sub(btn_w + 2);
    let btn_y = ta_y + 1;
    Some(Rect::new(btn_x, btn_y, btn_w, 1))
}

/// Geometry of the Description textarea inside the two-field commit popup.
pub fn commit_description_textarea_geometry(popup: &PopupState, area: Rect) -> Option<Rect> {
    if area.width < 4 || area.height < 4 {
        return None;
    }

    let PopupState::CommitInput { .. } = popup else {
        return None;
    };

    let popup_width = (area.width * 60 / 100).clamp(30, 60).min(area.width);
    let ta_height = 16u16.min(area.height);
    let ta_y = (area.height.saturating_sub(ta_height)) / 2;
    let ta_rect = Rect::new(
        (area.width.saturating_sub(popup_width)) / 2,
        ta_y,
        popup_width,
        ta_height,
    );
    let inner = ta_rect.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });

    if inner.height <= 6 {
        return None;
    }

    let body_height = inner.height.saturating_sub(6);
    (body_height > 0).then(|| Rect::new(inner.x, inner.y + 4, inner.width, body_height))
}

/// Tooltip rect placed one row above the popup, right-aligned with the button.
pub(super) fn commit_ai_tooltip_rect(area: Rect, btn_rect: Rect, tip_w: u16) -> Rect {
    let popup_width = (area.width * 60 / 100).clamp(30, 60).min(area.width);
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let ta_height: u16 = 16u16.min(area.height);
    let ta_y = (area.height.saturating_sub(ta_height)) / 2;

    let mut tip_x = (btn_rect.x + btn_rect.width).saturating_sub(tip_w);
    // Keep tooltip within the popup's horizontal bounds when possible.
    let popup_right = x + popup_width;
    if tip_x + tip_w > popup_right {
        tip_x = popup_right.saturating_sub(tip_w);
    }
    if tip_x + tip_w > area.width {
        tip_x = area.width.saturating_sub(tip_w);
    }
    let tip_y = if ta_y >= 1 {
        ta_y - 1
    } else if ta_y + ta_height < area.height {
        ta_y + ta_height
    } else {
        btn_rect.y
    };
    Rect::new(tip_x, tip_y, tip_w, 1)
}

pub fn render_popup(
    frame: &mut Frame,
    popup: &PopupState,
    area: Rect,
    spinner_frame: usize,
    theme: &Theme,
    ai_button_hovered: bool,
    ai_configured: bool,
) {
    // Bail out early on terminals too small to host any popup — better than
    // panicking inside a render with an out-of-bounds rect.
    if area.width < 4 || area.height < 4 {
        return;
    }
    let popup_width = (area.width * 60 / 100).clamp(30, 60).min(area.width);
    let x = (area.width.saturating_sub(popup_width)) / 2;

    match popup {
        PopupState::SmartCommitPlan(state) => {
            super::render_smart_commit_plan(frame, state, area, theme);
        }
        PopupState::Confirm { title, message, .. } => {
            let inner_width = popup_width.saturating_sub(4) as usize; // borders + padding
            let wrapped = wrap_popup_lines(message, inner_width);
            let wrapped = visible_popup_lines(&wrapped, area.height.saturating_sub(5) as usize);
            let confirm_height = clamped_popup_height(wrapped.len(), 5, area.height);
            let cy = (area.height.saturating_sub(confirm_height)) / 2;
            let popup_rect = Rect::new(x, cy, popup_width, confirm_height);
            frame.render_widget(Clear, popup_rect);
            let block = Block::default()
                .title(format!(" {} ", title))
                .borders(theme.borders_for("popup"))
                .border_type(theme.border_type_for("popup"))
                .border_style(Style::default().fg(theme.accent_secondary));

            let mut text: Vec<Line> = Vec::new();
            text.push(Line::from(""));
            for line in &wrapped {
                text.push(Line::from(format!(" {}", line)));
            }
            text.push(Line::from(""));
            let key_style = Style::default().fg(theme.accent_secondary);
            let desc_style = Style::default().fg(theme.text_dimmed);
            text.push(Line::from(vec![
                Span::styled(" y", key_style),
                Span::styled(": yes  ", desc_style),
                Span::styled("n", key_style),
                Span::styled(": no", desc_style),
            ]));

            let widget = Paragraph::new(text).block(block);
            frame.render_widget(widget, popup_rect);
        }
        PopupState::Message {
            title,
            message,
            kind,
        } => {
            let is_error = *kind == crate::gui::popup::MessageKind::Error;
            let icon = if is_error { "⚠ " } else { "" };
            let inner_width = popup_width.saturating_sub(4) as usize; // borders + padding
            let wrapped = wrap_popup_lines(message, inner_width);
            let wrapped = visible_popup_lines(&wrapped, area.height.saturating_sub(4) as usize);
            let msg_height = clamped_popup_height(wrapped.len(), 4, area.height);
            let cy = (area.height.saturating_sub(msg_height)) / 2;
            let popup_rect = Rect::new(x, cy, popup_width, msg_height);
            frame.render_widget(Clear, popup_rect);
            let border_color = if is_error {
                Color::Red
            } else {
                theme.accent_secondary
            };
            let block = Block::default()
                .title(format!(" {}{} ", icon, title))
                .borders(theme.borders_for("popup"))
                .border_type(theme.border_type_for("popup"))
                .border_style(Style::default().fg(border_color));

            let mut text: Vec<Line> = Vec::new();
            text.push(Line::from(""));
            for line in &wrapped {
                text.push(Line::from(format!(" {}", line)));
            }
            text.push(Line::from(Span::styled(
                " Press any key to dismiss",
                Style::default().fg(theme.text_dimmed),
            )));

            let widget = Paragraph::new(text).block(block);
            frame.render_widget(widget, popup_rect);
        }
        PopupState::Input {
            title,
            textarea,
            is_commit,
            confirm_focused,
            ..
        } => {
            // Textarea popup: taller to allow multiline editing
            // Add extra row for commit dialogs to fit the confirm button row
            let ta_height = if *is_commit { 14u16 } else { 12u16 };
            let ta_height = ta_height.min(area.height);
            if ta_height < 3 || popup_width < 3 {
                return;
            }
            let ta_y = (area.height.saturating_sub(ta_height)) / 2;
            let ta_rect = Rect::new(x, ta_y, popup_width, ta_height);
            frame.render_widget(Clear, ta_rect);

            // Render a container block with title and hint
            let outer = Block::default()
                .title(format!(" {} ", title))
                .borders(theme.borders_for("popup"))
                .border_type(theme.border_type_for("popup"))
                .border_style(Style::default().fg(theme.popup_border));
            frame.render_widget(outer, ta_rect);

            // Inner area for textarea + hint
            let inner = ta_rect.inner(ratatui::layout::Margin {
                horizontal: 1,
                vertical: 1,
            });

            if *is_commit {
                // Reserve 2 lines: one for hint, one for confirm button row
                if inner.height > 4 {
                    let ta_area = Rect::new(inner.x, inner.y, inner.width, inner.height - 2);
                    frame.render_widget(textarea, ta_area);

                    // Hint line (opencode-style: bold key, dim description)
                    let hint_area = Rect::new(inner.x, inner.y + inner.height - 2, inner.width, 1);
                    let key_style = Style::default()
                        .fg(theme.text)
                        .add_modifier(ratatui::style::Modifier::BOLD);
                    let desc_style = Style::default().fg(theme.text_dimmed);
                    let hint_line = Line::from(vec![
                        Span::styled(" ctrl+s ", key_style),
                        Span::styled("confirm  ", desc_style),
                        Span::styled("ctrl+o ", key_style),
                        Span::styled("menu  ", desc_style),
                        Span::styled("esc ", key_style),
                        Span::styled("cancel", desc_style),
                    ]);
                    frame.render_widget(Paragraph::new(hint_line), hint_area);

                    // Confirm button row (right-aligned)
                    let btn_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
                    let (btn_style, btn_text) = if *confirm_focused {
                        (
                            Style::default()
                                .fg(Color::Black)
                                .bg(theme.accent)
                                .add_modifier(ratatui::style::Modifier::BOLD),
                            " Confirm ",
                        )
                    } else {
                        (Style::default().fg(theme.accent), " Confirm ")
                    };
                    let btn_width = (btn_text.len() as u16).min(btn_area.width);
                    if btn_width > 0 {
                        let btn_x = btn_area.x + btn_area.width.saturating_sub(btn_width);
                        let btn_rect = Rect::new(btn_x, btn_area.y, btn_width, 1);
                        frame.render_widget(
                            Paragraph::new(Line::from(Span::styled(btn_text, btn_style))),
                            btn_rect,
                        );
                    }
                } else {
                    frame.render_widget(textarea, inner);
                }
            } else {
                // Non-commit: no button, just hint
                if inner.height > 2 {
                    let ta_area = Rect::new(inner.x, inner.y, inner.width, inner.height - 1);
                    frame.render_widget(textarea, ta_area);

                    let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
                    let key_style = Style::default()
                        .fg(theme.text)
                        .add_modifier(ratatui::style::Modifier::BOLD);
                    let desc_style = Style::default().fg(theme.text_dimmed);
                    let hint_line = Line::from(vec![
                        Span::styled(" enter ", key_style),
                        Span::styled("confirm  ", desc_style),
                        Span::styled("esc ", key_style),
                        Span::styled("cancel", desc_style),
                    ]);
                    frame.render_widget(Paragraph::new(hint_line), hint_area);
                } else {
                    frame.render_widget(textarea, inner);
                }
            }
        }
        PopupState::HunkCommentary {
            title, textarea, ..
        } => {
            let ta_height = 16u16.min(area.height);
            if ta_height >= 3 && popup_width >= 3 {
                let ta_y = (area.height.saturating_sub(ta_height)) / 2;
                let ta_rect = Rect::new(x, ta_y, popup_width, ta_height);
                frame.render_widget(Clear, ta_rect);

                let main_title = ratatui::text::Line::from(format!(" {} ", title))
                    .alignment(ratatui::layout::Alignment::Left);
                let close_title =
                    ratatui::text::Line::from(" (x) ").alignment(ratatui::layout::Alignment::Right);

                let outer = Block::default()
                    .title_top(main_title)
                    .title_top(close_title)
                    .borders(theme.borders_for("popup"))
                    .border_type(theme.border_type_for("popup"))
                    .border_style(Style::default().fg(theme.popup_border));
                frame.render_widget(outer, ta_rect);

                let inner = ta_rect.inner(ratatui::layout::Margin {
                    horizontal: 1,
                    vertical: 1,
                });

                if inner.height > 2 {
                    let ta_area = Rect::new(inner.x, inner.y, inner.width, inner.height - 1);
                    frame.render_widget(textarea, ta_area);

                    let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
                    let key_style = Style::default()
                        .fg(theme.text)
                        .add_modifier(ratatui::style::Modifier::BOLD);
                    let desc_style = Style::default().fg(theme.text_dimmed);
                    let hint_line = Line::from(vec![
                        Span::styled(" ctrl+s ", key_style),
                        Span::styled("save  ", desc_style),
                        Span::styled("esc ", key_style),
                        Span::styled("cancel", desc_style),
                    ]);
                    frame.render_widget(Paragraph::new(hint_line), hint_area);
                } else {
                    frame.render_widget(textarea, inner);
                }
            }
        }
        PopupState::CommitInput {
            kind,
            summary_textarea,
            body_textarea,
            focus,
            ..
        } => {
            // Two-field commit editor: summary (1 line) + body (multi-line)
            // Layout: border, summary label, summary input, body label, body textarea, hint, border
            let ta_height = 16u16.min(area.height);
            if ta_height < 3 || popup_width < 3 {
                return;
            }
            let ta_y = (area.height.saturating_sub(ta_height)) / 2;
            let ta_rect = Rect::new(x, ta_y, popup_width, ta_height);
            frame.render_widget(Clear, ta_rect);

            let border_color = match focus {
                CommitInputFocus::Summary => theme.popup_border,
                CommitInputFocus::Body => theme.popup_border,
            };
            let outer = Block::default()
                .title(format!(" {} ", kind.title()))
                .borders(theme.borders_for("popup"))
                .border_type(theme.border_type_for("popup"))
                .border_style(Style::default().fg(border_color));
            frame.render_widget(outer, ta_rect);

            let inner = ta_rect.inner(ratatui::layout::Margin {
                horizontal: 1,
                vertical: 1,
            });

            if inner.height > 6 {
                let focused_style = Style::default()
                    .fg(theme.accent_secondary)
                    .add_modifier(Modifier::BOLD);
                let unfocused_style = Style::default().fg(theme.text_dimmed);

                // Summary label
                let summary_label_area = Rect::new(inner.x, inner.y, inner.width, 1);
                let summary_label_style = if *focus == CommitInputFocus::Summary {
                    focused_style
                } else {
                    unfocused_style
                };
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled("Summary", summary_label_style))),
                    summary_label_area,
                );

                // Summary input (1 line)
                let summary_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
                frame.render_widget(summary_textarea, summary_area);

                // Body label
                let body_label_area = Rect::new(inner.x, inner.y + 3, inner.width, 1);
                let body_label_style = if *focus == CommitInputFocus::Body {
                    focused_style
                } else {
                    unfocused_style
                };
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled("Description", body_label_style))),
                    body_label_area,
                );

                // Body textarea (remaining space minus hint line and padding)
                let body_height = inner.height.saturating_sub(6); // 1 summary label + 1 summary + 1 gap + 1 body label + 1 hint + 1 padding
                let body_area = Rect::new(inner.x, inner.y + 4, inner.width, body_height);
                frame.render_widget(body_textarea, body_area);

                // Hint line at bottom (1 line padding above)
                let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
                let key_style = Style::default().fg(theme.text).add_modifier(Modifier::BOLD);
                let desc_style = Style::default().fg(theme.text_dimmed);
                let hint_line = Line::from(vec![
                    Span::styled(" enter ", key_style),
                    Span::styled("confirm  ", desc_style),
                    Span::styled("tab ", key_style),
                    Span::styled("switch  ", desc_style),
                    Span::styled("ctrl+o ", key_style),
                    Span::styled("menu  ", desc_style),
                    Span::styled("esc ", key_style),
                    Span::styled("cancel", desc_style),
                ]);
                frame.render_widget(Paragraph::new(hint_line), hint_area);
            } else {
                // Fallback: just render summary
                frame.render_widget(summary_textarea, inner);
            }
        }
        PopupState::Menu {
            title,
            items,
            selected,
            loading_index,
        } => {
            let height = (items.len() as u16 + 2).min(area.height - 4);
            let my = (area.height.saturating_sub(height)) / 2;
            let popup_rect = Rect::new(x, my, popup_width, height);
            frame.render_widget(Clear, popup_rect);

            let block = Block::default()
                .title(format!(" {} ", title))
                .borders(theme.borders_for("popup"))
                .border_type(theme.border_type_for("popup"))
                .border_style(Style::default().fg(theme.accent));

            let list_items: Vec<ListItem> = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let disabled = item.action.is_none();
                    let is_loading = *loading_index == Some(i);

                    let label = if let Some(ref key) = item.key {
                        format!(" {} {}", key, item.label)
                    } else {
                        format!("   {}", item.label)
                    };

                    if disabled {
                        let text_style = Style::default()
                            .fg(theme.text_dimmed)
                            .add_modifier(Modifier::CROSSED_OUT);
                        ListItem::new(Line::from(Span::styled(label, text_style)))
                    } else {
                        let selected_style = if i == *selected {
                            Style::default()
                                .bg(theme.selected_bg)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        };
                        let line = if is_loading {
                            let spinner = SPINNER_CHARS[(spinner_frame / 8) % SPINNER_CHARS.len()];
                            Line::from(vec![
                                Span::styled(label, Style::default()),
                                Span::raw(" "),
                                Span::styled(
                                    format!("{}", spinner),
                                    Style::default().fg(theme.accent_secondary),
                                ),
                            ])
                        } else if !item.description.is_empty() {
                            Line::from(vec![
                                Span::styled(label, Style::default()),
                                Span::raw(" "),
                                Span::styled(
                                    &item.description,
                                    Style::default().fg(theme.accent_secondary),
                                ),
                            ])
                        } else {
                            Line::from(label)
                        };
                        ListItem::new(line).style(selected_style)
                    }
                })
                .collect();

            let list = List::new(list_items).block(block);
            frame.render_widget(list, popup_rect);

            // Show disabled item description only when the selected item is disabled
            if let Some(selected_item) = items.get(*selected)
                && selected_item.action.is_none()
                && !selected_item.description.is_empty()
            {
                let hint_y = popup_rect.y + popup_rect.height;
                if hint_y < area.height {
                    let hint_text = format!("Disabled: {}", selected_item.description);
                    let hint_rect = Rect::new(popup_rect.x, hint_y, popup_rect.width, 1);
                    frame.render_widget(Clear, hint_rect);
                    frame.render_widget(
                        Paragraph::new(Span::styled(
                            hint_text,
                            Style::default().fg(theme.text_dimmed),
                        )),
                        hint_rect,
                    );
                }
            }
        }
        PopupState::Loading { title, message } => {
            render_loading_overlay(frame, area, spinner_frame, theme, title, message, None);
        }
        PopupState::Checklist {
            title,
            items,
            selected,
            search_textarea,
            free_entry_category,
            ..
        } => {
            let search = search_textarea.lines().join("");
            // Filter items by search query. Free-entry rows always remain
            // visible because they represent the current typed value.
            let visible: Vec<(usize, &crate::gui::popup::ChecklistItem)> = items
                .iter()
                .enumerate()
                .filter(|(_, it)| {
                    it.is_free_entry
                        || search.is_empty()
                        || it.label.to_lowercase().contains(&search.to_lowercase())
                })
                .collect();

            // Height: search bar (1) + blank (1) + items + blank (1) + hint (1) + borders (2)
            let content_lines = visible.len().max(1);
            let height = (content_lines as u16 + 6).min(area.height - 4).max(8);
            let cy = (area.height.saturating_sub(height)) / 2;
            let popup_rect = Rect::new(x, cy, popup_width, height);
            frame.render_widget(Clear, popup_rect);

            let block = Block::default()
                .title(format!(" {} ", title))
                .borders(theme.borders_for("popup"))
                .border_type(theme.border_type_for("popup"))
                .border_style(Style::default().fg(theme.accent));
            frame.render_widget(block, popup_rect);

            let inner = popup_rect.inner(ratatui::layout::Margin {
                horizontal: 1,
                vertical: 1,
            });
            if inner.height < 3 {
                // Too small, skip
            } else {
                // Search bar row (TextArea for cursor + word/line edits)
                let search_area = Rect::new(inner.x, inner.y, inner.width, 1);
                frame.render_widget(search_textarea, search_area);

                // Separator line
                let sep_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
                let sep = "─".repeat(inner.width as usize);
                frame.render_widget(
                    Paragraph::new(Span::styled(sep, Style::default().fg(theme.text_dimmed))),
                    sep_area,
                );

                // Checklist items
                let list_start = inner.y + 2;
                let list_height = inner.height.saturating_sub(3); // search + sep + hint
                let list_area = Rect::new(inner.x, list_start, inner.width, list_height);

                let list_items: Vec<ListItem> = visible
                    .iter()
                    .enumerate()
                    .map(|(vi, (_, item))| {
                        let check_sym = if item.checked { "◉" } else { "○" };
                        let check_color = if item.checked {
                            theme.accent
                        } else {
                            theme.text_dimmed
                        };
                        let is_selected = vi == *selected;
                        let category = if item.is_free_entry {
                            free_entry_category
                                .as_deref()
                                .map(|c| format!("  {c}"))
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };

                        let line = Line::from(vec![
                            Span::raw("  "),
                            Span::styled(check_sym, Style::default().fg(check_color)),
                            Span::raw("  "),
                            Span::styled(
                                format!("{}{}", item.label, category),
                                if is_selected {
                                    Style::default()
                                        .fg(theme.text_strong)
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(theme.text_strong)
                                },
                            ),
                        ]);

                        if is_selected {
                            ListItem::new(line).style(Style::default().bg(theme.selected_bg))
                        } else {
                            ListItem::new(line)
                        }
                    })
                    .collect();

                let list = List::new(list_items);
                frame.render_widget(list, list_area);

                // Hint at bottom
                let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
                let any_checked = items.iter().any(|it| it.checked);
                let mut hint_spans = vec![
                    Span::styled(" space", Style::default().fg(theme.accent_secondary)),
                    Span::styled(": toggle  ", Style::default().fg(theme.text_dimmed)),
                ];
                if any_checked {
                    hint_spans.push(Span::styled(
                        "ctrl-a",
                        Style::default().fg(theme.accent_secondary),
                    ));
                    hint_spans.push(Span::styled(
                        ": clear  ",
                        Style::default().fg(theme.text_dimmed),
                    ));
                }
                hint_spans.push(Span::styled(
                    "enter",
                    Style::default().fg(theme.accent_secondary),
                ));
                hint_spans.push(Span::styled(
                    ": apply  ",
                    Style::default().fg(theme.text_dimmed),
                ));
                hint_spans.push(Span::styled(
                    "esc",
                    Style::default().fg(theme.accent_secondary),
                ));
                hint_spans.push(Span::styled(
                    ": cancel",
                    Style::default().fg(theme.text_dimmed),
                ));
                let hint = Line::from(hint_spans);
                frame.render_widget(Paragraph::new(hint), hint_area);
            }
        }
        PopupState::CommandPalette {
            sections,
            selected,
            search_textarea,
            scroll_offset,
        } => {
            // Collect all visible entries (filtered by search) as flat list with section headers
            let search = search_textarea.lines().join("");
            let tokens = crate::gui::popup::list_picker_search_tokens(&search);
            let has_search = !tokens.is_empty();

            // Build flat display list: (is_header, key, description, executable)
            let mut display: Vec<(bool, String, String, bool)> = Vec::new();
            for section in sections {
                let visible_entries: Vec<&crate::gui::popup::CommandEntry> = if has_search {
                    section
                        .entries
                        .iter()
                        .filter(|e| {
                            crate::gui::popup::command_palette_entry_matches(
                                &e.key,
                                &e.description,
                                &tokens,
                            )
                        })
                        .collect()
                } else {
                    section.entries.iter().collect()
                };

                if !visible_entries.is_empty() {
                    display.push((true, section.title.clone(), String::new(), false));
                    for entry in visible_entries {
                        let key = if entry.key.is_empty() && entry.is_executable() {
                            "▸".into()
                        } else {
                            entry.key.clone()
                        };
                        display.push((
                            false,
                            key,
                            entry.description.clone(),
                            entry.is_executable(),
                        ));
                    }
                }
            }

            // Sizing: use more of the screen for help
            let popup_width = (area.width * 70 / 100).clamp(36, 72).min(area.width);
            let content_height = display.len().max(1);
            // search bar (1) + separator (1) + content + hint (1) + borders (2)
            let popup_height = (content_height as u16 + 5)
                .min(area.height.saturating_sub(4))
                .max(10);
            let x = (area.width.saturating_sub(popup_width)) / 2;
            let y = (area.height.saturating_sub(popup_height)) / 2;
            let popup_rect = Rect::new(x, y, popup_width, popup_height);
            frame.render_widget(Clear, popup_rect);

            let block = Block::default()
                .title(" Command Palette ")
                .borders(theme.borders_for("popup"))
                .border_type(theme.border_type_for("popup"))
                .border_style(Style::default().fg(theme.accent));
            frame.render_widget(block, popup_rect);

            let inner = popup_rect.inner(ratatui::layout::Margin {
                horizontal: 1,
                vertical: 1,
            });
            if inner.height < 3 {
                return;
            }

            // Search bar row: " " prefix + textarea
            let prefix_width = 2u16; // "  "
            let prefix_rect = Rect::new(inner.x, inner.y, prefix_width, 1);
            let prefix_style = if search.is_empty() {
                Style::default().fg(theme.text_dimmed)
            } else {
                Style::default().fg(theme.accent_secondary)
            };
            frame.render_widget(
                Paragraph::new(Span::styled("  ", prefix_style)),
                prefix_rect,
            );
            let ta_width = inner.width.saturating_sub(prefix_width);
            let ta_rect = Rect::new(inner.x + prefix_width, inner.y, ta_width, 1);
            frame.render_widget(search_textarea, ta_rect);

            // Separator
            let sep_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
            let sep = "─".repeat(inner.width as usize);
            frame.render_widget(
                Paragraph::new(Span::styled(sep, Style::default().fg(theme.text_dimmed))),
                sep_area,
            );

            // Content area
            let list_start = inner.y + 2;
            let list_height = inner.height.saturating_sub(3) as usize; // search + sep + hint
            let list_area = Rect::new(inner.x, list_start, inner.width, list_height as u16);

            // Use the stored scroll_offset, clamped to valid range
            let max_scroll = display.len().saturating_sub(list_height);
            let so = *scroll_offset;
            let effective_scroll = if so > max_scroll { max_scroll } else { so };

            let visible_display: Vec<&(bool, String, String, bool)> = display
                .iter()
                .skip(effective_scroll)
                .take(list_height)
                .collect();

            // Count non-header entries before scroll offset to track selection
            let mut entry_idx = 0usize;
            for (is_header, _, _, _) in display.iter().take(effective_scroll) {
                if !is_header {
                    entry_idx += 1;
                }
            }

            let key_col_width = 14usize;

            let mut list_items: Vec<ListItem> = Vec::new();
            for (is_header, key_or_title, desc, executable) in visible_display {
                if *is_header {
                    let line = Line::from(vec![Span::styled(
                        format!(" {} ", key_or_title),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    )]);
                    list_items.push(ListItem::new(line));
                } else {
                    let is_selected = entry_idx == *selected;
                    entry_idx += 1;

                    let key_display = format!("  {:>width$}", key_or_title, width = key_col_width);
                    let desc_display = format!("  {}", desc);

                    let key_base_style = if is_selected {
                        Style::default()
                            .fg(theme.accent_secondary)
                            .add_modifier(Modifier::BOLD)
                    } else if !executable {
                        Style::default().fg(theme.text_dimmed)
                    } else {
                        Style::default().fg(theme.accent)
                    };

                    let desc_base_style = if is_selected {
                        Style::default()
                            .fg(theme.text_strong)
                            .add_modifier(Modifier::BOLD)
                    } else if !executable {
                        Style::default().fg(theme.text_dimmed)
                    } else {
                        Style::default().fg(theme.text)
                    };

                    let highlight_style = Style::default()
                        .fg(theme.accent_secondary)
                        .add_modifier(Modifier::BOLD);

                    let build_spans = |text: &str, base: Style| -> Vec<Span<'static>> {
                        if !has_search {
                            return vec![Span::styled(text.to_string(), base)];
                        }
                        // Highlight every query token (order-free), mirroring
                        // the list-picker highlight.
                        let ranges = crate::gui::popup::list_picker_highlight_ranges(text, &tokens);
                        if ranges.is_empty() {
                            return vec![Span::styled(text.to_string(), base)];
                        }
                        let mut s = Vec::new();
                        let mut cursor = 0usize;
                        for (start, end) in ranges {
                            if start > cursor
                                && let Some(chunk) = text.get(cursor..start)
                                && !chunk.is_empty()
                            {
                                s.push(Span::styled(chunk.to_string(), base));
                            }
                            if let Some(chunk) = text.get(start..end) {
                                s.push(Span::styled(chunk.to_string(), highlight_style));
                            }
                            cursor = end;
                        }
                        if let Some(rest) = text.get(cursor..)
                            && !rest.is_empty()
                        {
                            s.push(Span::styled(rest.to_string(), base));
                        }
                        s
                    };

                    let mut spans = build_spans(&key_display, key_base_style);
                    spans.extend(build_spans(&desc_display, desc_base_style));
                    let line = Line::from(spans);

                    if is_selected {
                        list_items.push(
                            ListItem::new(line).style(Style::default().bg(theme.selected_bg)),
                        );
                    } else {
                        list_items.push(ListItem::new(line));
                    }
                }
            }

            let list = List::new(list_items);
            frame.render_widget(list, list_area);

            // Hint bar at bottom
            let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
            let hint = Line::from(vec![
                Span::styled(" ↑↓", Style::default().fg(theme.accent_secondary)),
                Span::styled(": navigate  ", Style::default().fg(theme.text_dimmed)),
                Span::styled("type", Style::default().fg(theme.accent_secondary)),
                Span::styled(": search  ", Style::default().fg(theme.text_dimmed)),
                Span::styled("enter", Style::default().fg(theme.accent_secondary)),
                Span::styled(": execute  ", Style::default().fg(theme.text_dimmed)),
                Span::styled("esc", Style::default().fg(theme.accent_secondary)),
                Span::styled(": close", Style::default().fg(theme.text_dimmed)),
            ]);
            frame.render_widget(Paragraph::new(hint), hint_area);
        }
        PopupState::RefPicker { title, core, .. } => {
            render_list_picker(
                frame,
                area,
                theme,
                core,
                title,
                70,
                72,
                36,
                &[
                    ("↑↓", "navigate"),
                    ("type", "jump to"),
                    ("enter", "select"),
                    ("esc", "cancel"),
                ],
            );
        }
        PopupState::ListPicker { title, core, .. } => {
            render_list_picker(
                frame,
                area,
                theme,
                core,
                title,
                70,
                72,
                36,
                &[
                    ("↑↓", "navigate"),
                    ("type", "filter / free entry"),
                    ("enter", "select"),
                    ("esc", "cancel"),
                ],
            );
        }
        PopupState::ThemePicker { core, .. } => {
            render_list_picker(
                frame,
                area,
                theme,
                core,
                "Color Theme",
                65,
                70,
                36,
                &[
                    ("↑↓", "preview"),
                    ("type", "filter"),
                    ("enter", "apply"),
                    ("esc", "cancel"),
                ],
            );
        }
        PopupState::None => {}
    }

    // Overlay the ✦ AI-generate button (and tooltip when hovered) inside the
    // commit-message popup, on the Summary label row.
    if let Some(btn_rect) = commit_ai_button_geometry(popup, area) {
        let buf = frame.buffer_mut();
        // No bg fill — ✦ has off-center bearings in most fonts, so any colored
        // block exposes the asymmetry. Color/weight change is the hover feedback.
        let glyph_style = if ai_configured {
            if ai_button_hovered {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.accent_secondary)
                    .add_modifier(Modifier::BOLD)
            }
        } else if ai_button_hovered {
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_dimmed)
        };
        let glyph_col = btn_rect.x + 1;
        if let Some(cell) = buf.cell_mut((glyph_col, btn_rect.y)) {
            cell.set_char('\u{F0674}'); // Nerd Font: nf-md-creation (sparkle)
            cell.set_style(glyph_style);
        }

        if ai_button_hovered {
            let tip_style = Style::default().bg(theme.selected_bg).fg(theme.text_strong);
            let key_style = Style::default()
                .bg(theme.selected_bg)
                .fg(theme.accent_secondary)
                .add_modifier(Modifier::BOLD);
            let dim_style = Style::default().bg(theme.selected_bg).fg(theme.text_dimmed);

            let parts: Vec<(&str, Style)> = if ai_configured {
                vec![
                    (" ", tip_style),
                    ("c-g", key_style),
                    (" Generate w/ AI ", tip_style),
                ]
            } else {
                vec![
                    (" \u{F0674} Generate w/ AI ", tip_style),
                    ("(needs setup)", dim_style),
                    (" ", tip_style),
                ]
            };
            let tip_w: u16 = parts.iter().map(|(s, _)| s.chars().count() as u16).sum();
            let tip_rect = commit_ai_tooltip_rect(area, btn_rect, tip_w);

            let mut col = tip_rect.x;
            for (text, style) in &parts {
                for ch in text.chars() {
                    if col >= tip_rect.x + tip_rect.width {
                        break;
                    }
                    if col >= buf.area.x + buf.area.width {
                        break;
                    }
                    if let Some(cell) = buf.cell_mut((col, tip_rect.y)) {
                        cell.set_char(ch);
                        cell.set_style(*style);
                    }
                    col += 1;
                }
            }
        }
    }
}

use crate::gui::popup::ListPickerCore;

/// Shared rendering for searchable list picker popups (RefPicker, ThemePicker, etc.).
pub(super) fn render_list_picker(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    core: &ListPickerCore,
    title: &str,
    width_pct: u16,
    width_max: u16,
    width_min: u16,
    hints: &[(&str, &str)],
) {
    let search = core.search_textarea.lines().join("");
    let search_lower = search.trim().to_lowercase();
    let matching = list_picker_matching_indices(&core.items, &search);

    // Build display rows from matching items only (search filters the list).
    let has_categories = matching
        .iter()
        .any(|&i| core.items.get(i).is_some_and(|it| !it.category.is_empty()));
    // (is_header, label, item_idx, description)
    let mut display: Vec<(bool, String, Option<usize>, Option<String>)> = Vec::new();
    if has_categories {
        let mut last_cat = String::new();
        for &ei in &matching {
            let Some(item) = core.items.get(ei) else {
                continue;
            };
            if !item.category.is_empty() && item.category != last_cat {
                display.push((true, item.category.clone(), None, None));
                last_cat = item.category.clone();
            }
            display.push((
                false,
                item.label.clone(),
                Some(ei),
                item.description.clone(),
            ));
        }
    } else {
        for &ei in &matching {
            let Some(item) = core.items.get(ei) else {
                continue;
            };
            display.push((
                false,
                item.label.clone(),
                Some(ei),
                item.description.clone(),
            ));
        }
    }

    // Popup frame
    let popup_width = (area.width * width_pct / 100)
        .min(width_max)
        .max(width_min)
        .min(area.width);
    let max_popup = (area.height * 60 / 100).max(10);
    let popup_height = max_popup.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_rect = Rect::new(x, y, popup_width, popup_height);
    frame.render_widget(Clear, popup_rect);

    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(theme.borders_for("popup"))
        .border_type(theme.border_type_for("popup"))
        .border_style(Style::default().fg(theme.accent));
    frame.render_widget(block, popup_rect);

    let inner = popup_rect.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    if inner.height < 3 {
        return;
    }

    // Search bar
    let prefix_width = 2u16;
    let prefix_rect = Rect::new(inner.x, inner.y, prefix_width, 1);
    let prefix_style = if search.is_empty() {
        Style::default().fg(theme.text_dimmed)
    } else {
        Style::default().fg(theme.accent_secondary)
    };
    frame.render_widget(
        Paragraph::new(Span::styled("  ", prefix_style)),
        prefix_rect,
    );
    let ta_width = inner.width.saturating_sub(prefix_width);
    let ta_rect = Rect::new(inner.x + prefix_width, inner.y, ta_width, 1);
    frame.render_widget(&core.search_textarea, ta_rect);

    // Separator
    let sep_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
    let sep = "─".repeat(inner.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(sep, Style::default().fg(theme.text_dimmed))),
        sep_area,
    );

    // Content area
    let list_start = inner.y + 2;
    let list_height = inner.height.saturating_sub(3) as usize; // search + sep + hint
    let list_area = Rect::new(inner.x, list_start, inner.width, list_height as u16);

    let max_scroll = display.len().saturating_sub(list_height);
    let effective_scroll = core.scroll_offset.min(max_scroll);

    let visible_display: Vec<&(bool, String, Option<usize>, Option<String>)> = display
        .iter()
        .skip(effective_scroll)
        .take(list_height)
        .collect();

    let content_w = list_area.width as usize;
    let mut list_items: Vec<ListItem> = Vec::new();
    for (is_header, label, item_idx, description) in visible_display {
        if *is_header {
            let line = Line::from(vec![Span::styled(
                format!(" {} ", label),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )]);
            list_items.push(ListItem::new(line));
        } else {
            let is_selected = *item_idx == Some(core.selected);

            let base_fg = if is_selected {
                theme.text_strong
            } else {
                theme.text
            };
            let highlight_fg = theme.accent_secondary;

            // ▸ marker for selected item
            let marker = if is_selected { "▸ " } else { "  " };
            let mut spans = vec![Span::styled(
                marker,
                Style::default().fg(theme.accent_secondary),
            )];

            // Build label spans with search match highlighting.
            // Multi-word queries highlight every token in either order,
            // mirroring `list_picker_matching_indices` (token-AND).
            if !search_lower.is_empty() {
                let tokens = crate::gui::popup::list_picker_search_tokens(&search);
                let ranges = crate::gui::popup::list_picker_highlight_ranges(label, &tokens);
                if ranges.is_empty() {
                    spans.push(Span::styled(label.clone(), Style::default().fg(base_fg)));
                } else {
                    let match_style = Style::default()
                        .fg(highlight_fg)
                        .add_modifier(Modifier::BOLD);
                    let base_style = Style::default().fg(base_fg);
                    let mut cursor = 0usize;
                    for (s, e) in ranges {
                        if s > cursor
                            && let Some(chunk) = label.get(cursor..s)
                            && !chunk.is_empty()
                        {
                            spans.push(Span::styled(chunk.to_string(), base_style));
                        }
                        if let Some(chunk) = label.get(s..e) {
                            spans.push(Span::styled(chunk.to_string(), match_style));
                        }
                        cursor = e;
                    }
                    if let Some(rest) = label.get(cursor..)
                        && !rest.is_empty()
                    {
                        spans.push(Span::styled(rest.to_string(), base_style));
                    }
                }
            } else {
                let style = if is_selected {
                    Style::default().fg(base_fg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(base_fg)
                };
                spans.push(Span::styled(label.clone(), style));
            }

            if let Some(desc) = description.as_deref().filter(|d| !d.is_empty()) {
                let used: usize = spans
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum();
                let desc_w = UnicodeWidthStr::width(desc);
                let pad = content_w.saturating_sub(used).saturating_sub(desc_w);
                if pad > 0 {
                    spans.push(Span::raw(" ".repeat(pad)));
                }
                spans.push(Span::styled(
                    desc.to_string(),
                    Style::default().fg(theme.text_dimmed),
                ));
            }

            let line = Line::from(spans);

            if is_selected {
                list_items.push(ListItem::new(line).style(Style::default().bg(theme.selected_bg)));
            } else {
                list_items.push(ListItem::new(line));
            }
        }
    }

    let list = List::new(list_items);
    frame.render_widget(list, list_area);

    // Hint bar
    let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
    let mut hint_spans = Vec::new();
    for (i, (key, desc)) in hints.iter().enumerate() {
        if i == 0 {
            hint_spans.push(Span::styled(
                format!(" {}", key),
                Style::default().fg(theme.accent_secondary),
            ));
        } else {
            hint_spans.push(Span::styled(
                key.to_string(),
                Style::default().fg(theme.accent_secondary),
            ));
        }
        hint_spans.push(Span::styled(
            format!(": {}  ", desc),
            Style::default().fg(theme.text_dimmed),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(hint_spans)), hint_area);
}
