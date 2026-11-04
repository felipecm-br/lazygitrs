//! Overlays drawn on the diff: revert tooltip, placeholders, file headers, hover button, annotations.

use super::*;

pub(super) fn render_revert_tooltip(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    max_width: u16,
    theme: &Theme,
    show_key: bool,
    staged: bool,
) {
    let tip_style = Style::default().bg(theme.selected_bg).fg(theme.text_strong);
    let key_style = Style::default()
        .bg(theme.selected_bg)
        .fg(theme.accent_secondary)
        .add_modifier(Modifier::BOLD);

    let label = if staged {
        " Staged hunk "
    } else {
        " Hunk menu "
    };
    let parts: Vec<(&str, Style)> = if show_key {
        vec![(" ", tip_style), ("enter", key_style), (label, tip_style)]
    } else {
        vec![(label, tip_style)]
    };

    let buf_area = buf.area();
    if y < buf_area.y || y >= buf_area.y + buf_area.height {
        return;
    }
    let max_x = (x + max_width).min(buf_area.x + buf_area.width);

    let mut col = x;
    for (text, style) in &parts {
        for ch in text.chars() {
            if col >= max_x {
                return;
            }
            if let Some(cell) = buf.cell_mut((col, y)) {
                cell.set_char(ch);
                cell.set_style(*style);
            }
            col += 1;
        }
    }
}

/// Render a file header separator line spanning the full width.
/// The same diagonal glyph and muted foreground used for absent diff sides.
pub(super) fn render_preview_placeholder(
    buf: &mut Buffer,
    area: Rect,
    message: &str,
    theme: &Theme,
) {
    let area = area.intersection(buf.area);
    if area.is_empty() {
        return;
    }
    let hatch = "╱".repeat(area.width as usize);
    let hatch_style = Style::reset().fg(theme.diff_line_number);
    for y in area.y..area.bottom() {
        buf.set_stringn(area.x, y, &hatch, area.width as usize, hatch_style);
    }
    if message.is_empty() {
        return;
    }
    // Keep the reason on one row even in narrow terminals; never wrap it into
    // the next file's header. ASCII labels make clipping cell-width safe.
    let label = if area.width as usize >= message.len() + 4 {
        format!("  {message}  ")
    } else {
        message.chars().take(area.width as usize).collect()
    };
    let x = area.x + (area.width - label.len() as u16) / 2;
    let y = area.y + area.height / 2;
    let padding = " ".repeat(label.len());
    for padding_y in y.saturating_sub(1).max(area.y)..=(y + 1).min(area.bottom() - 1) {
        buf.set_stringn(
            x,
            padding_y,
            &padding,
            label.len(),
            Style::reset().fg(theme.text_dimmed),
        );
    }
    buf.set_stringn(
        x,
        y,
        label,
        area.width as usize,
        Style::reset().fg(theme.text_dimmed),
    );
}

pub(super) fn render_file_header(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    filename: &str,
    theme: &Theme,
) {
    let buf_area = buf.area();
    if y < buf_area.y || y >= buf_area.y + buf_area.height {
        return;
    }

    let header_style = Style::default()
        .fg(theme.diff_selection_fg)
        .bg(theme.diff_selection_bg)
        .add_modifier(Modifier::BOLD);

    // Build header text: "── filename ──────..." using theme.separator_char
    let sep = &theme.separator_char;
    let prefix = format!("{}{} ", sep, sep);
    let label = format!("{}{} ", prefix, filename);

    let prefix_width = unicode_width::UnicodeWidthStr::width(prefix.as_str());
    let filename_width = unicode_width::UnicodeWidthStr::width(filename);
    let label_width = prefix_width + filename_width + 1;

    let sep_width = unicode_width::UnicodeWidthStr::width(sep.as_str()).max(1);
    let remaining_cells = (width as usize).saturating_sub(label_width);
    let repeat_count = remaining_cells / sep_width;
    let suffix = sep.repeat(repeat_count);

    let full_line = format!("{}{}", label, suffix);

    buf_write_str(buf, x, y, &full_line, header_style, width);
}

pub(super) fn render_hover_plus_button(
    buf: &mut Buffer,
    area: Rect,
    state: &DiffViewState,
    theme: &Theme,
) {
    // Don't render  when notes are hidden.
    if !state.notes_visible {
        return;
    }
    let (line_idx, panel) = match state.hovered_line {
        Some(h) => h,
        None => return,
    };

    let diff_line = match state.lines.get(line_idx) {
        Some(dl) if dl.file_header.is_none() => dl,
        _ => return,
    };

    if let Some(ref edit) = state.inline_edit
        && edit.line_idx == line_idx
    {
        return;
    }

    let pl = DiffPanelLayout::compute(area, state);
    let unified_content_width = pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
    let panel_width = pl.old_content_end_x.saturating_sub(pl.old_content_x) as usize;
    let right_content_width = pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
    let visible_height = pl.inner_end_y.saturating_sub(pl.inner_y) as usize;

    if line_idx < state.scroll_offset {
        return;
    }

    // Compute Y of the code line (first row of this diff line).
    let code_y = if pl.is_unified || state.side_view != DiffSideView::Both {
        let mut acc = 0usize;
        for (offset, dl) in state.lines[state.scroll_offset..=line_idx]
            .iter()
            .enumerate()
        {
            let idx = state.scroll_offset + offset;
            if idx == line_idx {
                break;
            }
            if acc >= visible_height {
                return;
            }
            acc += unified_line_visual_height(dl, unified_content_width, state, idx);
        }
        pl.inner_y + acc as u16
    } else {
        let mut acc = 0usize;
        for (offset, dl) in state.lines[state.scroll_offset..=line_idx]
            .iter()
            .enumerate()
        {
            let idx = state.scroll_offset + offset;
            if idx == line_idx {
                break;
            }
            if acc >= visible_height {
                return;
            }
            acc += line_visual_height(dl, panel_width, right_content_width, state, idx);
        }
        pl.inner_y + acc as u16
    };

    //  is an individual element. When there are notes, it sits on its own
    // row after the last note. When there are no notes, it sits on the code row.
    let note_count = diff_line.comment_notes.len();
    let y = code_y + (note_count * 5) as u16;

    if y >= pl.inner_end_y {
        return;
    }

    let content_end_x = match panel {
        DiffPanel::Old => pl.old_content_end_x,
        DiffPanel::New => pl.new_content_end_x,
    };
    let x = content_end_x.saturating_sub(3);

    let style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    buf_write_str(buf, x, y, "  ", style, 3);
}

pub(super) fn render_diff_annotations(
    buf: &mut Buffer,
    state: &DiffViewState,
    diff_line: &DiffLine,
    line_idx: usize,
    inner: Rect,
    row: &mut usize,
    visible_height: usize,
    panel_width: u16,
    gutter_width: u16,
    divider_width: u16,
    div_x: u16,
    right_content_width: u16,
    theme: &Theme,
    spinner_frame: usize,
) {
    let edit_active = state
        .inline_edit
        .as_ref()
        .map(|e| e.line_idx == line_idx)
        .unwrap_or(false);

    // Render saved notes and the editor interleaved in order.
    // When editing an existing note, the editor replaces it in-place.
    // When notes are hidden, skip rendering saved note boxes (but still
    // show the editor if one is active).
    for note in &diff_line.comment_notes {
        let is_being_edited = edit_active
            && state
                .inline_edit
                .as_ref()
                .map(|e| e.editing_id == note.id)
                .unwrap_or(false);

        if is_being_edited {
            if let Some(ref edit) = state.inline_edit {
                if *row + 5 <= visible_height {
                    let y = inner.y + *row as u16;
                    let file_path = state.file_at_line(line_idx);
                    let line_num = state.file_line_number(line_idx, edit.panel).unwrap_or(0);
                    let edit_area = if edit.panel == DiffPanel::Old {
                        Rect::new(inner.x, y, panel_width + gutter_width, 5)
                    } else {
                        Rect::new(
                            div_x + divider_width,
                            y,
                            right_content_width + gutter_width,
                            5,
                        )
                    };
                    let title = format!(" Edit Note - {} - {} ", file_path, line_num);
                    let bottom_spans = vec![
                        ratatui::text::Span::styled(
                            " Ctrl+S ",
                            Style::default()
                                .fg(theme.accent)
                                .add_modifier(Modifier::BOLD),
                        ),
                        ratatui::text::Span::styled(
                            " save ",
                            Style::default().fg(theme.text_dimmed),
                        ),
                        ratatui::text::Span::styled(
                            " Esc ",
                            Style::default()
                                .fg(theme.accent)
                                .add_modifier(Modifier::BOLD),
                        ),
                        ratatui::text::Span::styled(
                            " close ",
                            Style::default().fg(theme.text_dimmed),
                        ),
                    ];
                    let block = ratatui::widgets::Block::default()
                        .borders(theme.borders_for("notes"))
                        .border_type(theme.border_type_for("notes"))
                        .border_style(Style::default().fg(theme.accent))
                        .title(title)
                        .title_bottom(
                            ratatui::text::Line::from(bottom_spans)
                                .alignment(ratatui::layout::Alignment::Right),
                        );
                    let mut ta = edit.textarea.clone();
                    ta.set_block(block);
                    ratatui::widgets::Widget::render(&ta, edit_area, buf);
                }
                *row += 5;
            }
            continue;
        }

        // Skip rendering note boxes when notes are hidden.
        if !state.notes_visible {
            continue;
        }

        if *row + 5 <= visible_height {
            let y = inner.y + *row as u16;
            let (area_x, area_w) = if note.is_old {
                (inner.x, panel_width + gutter_width)
            } else {
                (div_x + divider_width, right_content_width + gutter_width)
            };
            let is_selected = state.selected_note.as_deref() == Some(&note.id);
            let is_yank_flashing = state
                .note_yank_flash
                .as_ref()
                .map(|(id, ts)| id == &note.id && ts.elapsed().as_millis() < 500)
                .unwrap_or(false);

            let (border_fg, text_fg) = if is_yank_flashing {
                let yank_gold = Color::Rgb(250, 204, 21); // Bright Neovim Yank Gold (#facc15)
                (yank_gold, yank_gold)
            } else if is_selected {
                (theme.accent_secondary, theme.accent_secondary)
            } else {
                (theme.text_dimmed, theme.accent_secondary)
            };

            let border_mod = if is_yank_flashing || is_selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            };
            let block = {
                let title_prefix = if note.source == crate::pager::NoteSource::Agent {
                    " 󱙺 "
                } else {
                    " 󱇗 "
                };
                let status_dot = match note.status {
                    crate::pager::NoteStatus::New => "●",
                    crate::pager::NoteStatus::Sent => {
                        if theme.spinner_chars.is_empty() {
                            "◆"
                        } else {
                            &theme.spinner_chars[(spinner_frame / 8) % theme.spinner_chars.len()]
                        }
                    }
                    crate::pager::NoteStatus::Addressed => "✓",
                };
                let status_color = if is_yank_flashing {
                    text_fg
                } else {
                    match note.status {
                        crate::pager::NoteStatus::New => theme.text_dimmed,
                        crate::pager::NoteStatus::Sent => theme.accent,
                        crate::pager::NoteStatus::Addressed => theme.accent_secondary,
                    }
                };
                let title_spans = vec![
                    ratatui::text::Span::styled(
                        title_prefix,
                        Style::default().fg(text_fg).add_modifier(border_mod),
                    ),
                    ratatui::text::Span::styled(
                        status_dot,
                        Style::default()
                            .fg(status_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                ];
                let hint_fg = if is_yank_flashing {
                    text_fg
                } else {
                    theme.text_dimmed
                };
                let mut bottom_spans = vec![ratatui::text::Span::styled(
                    " [d] del ",
                    Style::default().fg(hint_fg),
                )];
                if note.source == crate::pager::NoteSource::User {
                    bottom_spans.insert(
                        0,
                        ratatui::text::Span::styled(" [e] edit ", Style::default().fg(hint_fg)),
                    );
                    if note.status == crate::pager::NoteStatus::New {
                        let send_fg = if is_yank_flashing {
                            text_fg
                        } else {
                            theme.accent
                        };
                        bottom_spans.insert(
                            0,
                            ratatui::text::Span::styled(
                                " [S] send ",
                                Style::default().fg(send_fg).add_modifier(Modifier::BOLD),
                            ),
                        );
                    } else {
                        let reset_fg = if is_yank_flashing {
                            text_fg
                        } else {
                            theme.accent
                        };
                        bottom_spans.insert(
                            0,
                            ratatui::text::Span::styled(
                                " [r] reset ",
                                Style::default().fg(reset_fg).add_modifier(Modifier::BOLD),
                            ),
                        );
                    }
                }
                ratatui::widgets::Block::default()
                    .borders(theme.borders_for("notes"))
                    .border_type(theme.border_type_for("notes"))
                    .border_style(Style::default().fg(border_fg).add_modifier(border_mod))
                    .title(ratatui::text::Line::from(title_spans))
                    .title_bottom(
                        ratatui::text::Line::from(bottom_spans)
                            .alignment(ratatui::layout::Alignment::Right),
                    )
            };
            let widget = ratatui::widgets::Paragraph::new(note.text.as_str())
                .style(Style::default().fg(text_fg).add_modifier(Modifier::BOLD));
            ratatui::widgets::Widget::render(
                widget.block(block),
                Rect::new(area_x, y, area_w, 5),
                buf,
            );
        }
        *row += 5;
    }

    // Render the inline editor for NEW notes (after all saved notes).
    if let Some(ref edit) = state.inline_edit
        && edit.line_idx == line_idx
        && edit.editing_id.is_empty()
    {
        if *row + 5 <= visible_height {
            let y = inner.y + *row as u16;
            let file_path = state.file_at_line(line_idx);
            let line_num = state.file_line_number(line_idx, edit.panel).unwrap_or(0);

            let edit_area = if edit.panel == DiffPanel::Old {
                Rect::new(inner.x, y, panel_width + gutter_width, 5)
            } else {
                Rect::new(
                    div_x + divider_width,
                    y,
                    right_content_width + gutter_width,
                    5,
                )
            };

            let title = format!(" 󱞂 - {} - {} ", file_path, line_num);
            let block = ratatui::widgets::Block::default()
                .borders(theme.borders_for("notes"))
                .border_type(theme.border_type_for("notes"))
                .border_style(Style::default().fg(theme.accent))
                .title(title)
                .title_bottom(
                    ratatui::text::Line::from(vec![
                        ratatui::text::Span::styled(
                            " Ctrl+S ",
                            Style::default()
                                .fg(theme.accent)
                                .add_modifier(Modifier::BOLD),
                        ),
                        ratatui::text::Span::styled(
                            " save ",
                            Style::default().fg(theme.text_dimmed),
                        ),
                        ratatui::text::Span::styled(
                            " Esc ",
                            Style::default()
                                .fg(theme.accent)
                                .add_modifier(Modifier::BOLD),
                        ),
                        ratatui::text::Span::styled(
                            " close ",
                            Style::default().fg(theme.text_dimmed),
                        ),
                    ])
                    .alignment(ratatui::layout::Alignment::Right),
                );

            let mut ta = edit.textarea.clone();
            ta.set_block(block);
            ratatui::widgets::Widget::render(&ta, edit_area, buf);
        }
        *row += 5;
    }
}
