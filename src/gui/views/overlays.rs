//! Overlays drawn above the main layout: diff text selection and loading spinner.

use super::*;

/// Render mouse text selection highlight overlay and copy tooltip on the diff view.
/// `panel_rect` is the main diff panel Rect — selection is rendered only within the selected side.
pub fn render_selection_overlay(
    frame: &mut Frame,
    diff_view: &mut DiffViewState,
    panel_rect: Rect,
    theme: &Theme,
) {
    use crate::pager::ChangeType;

    let selection = match &diff_view.selection {
        Some(sel) => sel.clone(),
        None => return,
    };

    let (top_row, top_col, bot_row, bot_col) = selection.normalized();
    let is_click = selection.is_click;

    // For non-click selections, bail on empty (single point).
    if !is_click && top_row == bot_row && top_col == bot_col {
        return;
    }

    // Use the same centralized layout that render_diff and the mouse handler use.
    let pl = DiffPanelLayout::compute(panel_rect, diff_view);
    let (content_start, content_end) = pl.content_range(selection.panel);

    // Compute the actual file line number at the top of the selection/click for editAtLine.
    let edit_line_number: Option<usize> = if top_row >= pl.inner_y {
        let (line_idx, panel) = diff_view
            .line_chunk_panel_at_row(top_row, &pl, selection.panel)
            .map(|(line_idx, _, panel)| (line_idx, panel))
            .unwrap_or_else(|| {
                (
                    diff_view.fallback_line_idx_for_row(top_row, &pl),
                    selection.panel,
                )
            });
        diff_view.file_line_number(line_idx, panel)
    } else {
        None
    };
    // Compute the file column number (1-based) from the terminal click position.
    let edit_column_number: Option<usize> = if top_col >= content_start {
        Some((top_col - content_start) as usize + diff_view.horizontal_scroll + 1)
    } else {
        Some(1)
    };
    if let Some(ref mut sel) = diff_view.selection {
        sel.edit_line_number = edit_line_number;
        sel.edit_column_number = edit_column_number;
    }

    let buf = frame.buffer_mut();
    let buf_area = *buf.area();

    // --- Click state: highlight the clicked cell and show "e edit" tooltip ---
    if is_click {
        // Highlight the single clicked cell
        if top_row >= pl.inner_y
            && top_row < pl.inner_end_y
            && top_col >= content_start
            && top_col < content_end
            && top_row < buf_area.y + buf_area.height
        {
            let highlight_style = Style::default().bg(theme.popup_border).fg(Color::Black);
            if let Some(cell) = buf.cell_mut((top_col, top_row)) {
                cell.set_style(highlight_style);
            }
        }

        if diff_view.file_exists_on_disk {
            let tooltip_style = Style::default().bg(theme.selected_bg).fg(theme.text_strong);
            let key_style = Style::default()
                .bg(theme.selected_bg)
                .fg(theme.accent_secondary)
                .add_modifier(Modifier::BOLD);

            let parts: &[(&str, Style)] = &[
                (" ", tooltip_style),
                ("e", key_style),
                (" edit ", tooltip_style),
            ];
            let tooltip_width: u16 = parts.iter().map(|(s, _)| s.len() as u16).sum();
            let tooltip_x = top_col
                .saturating_sub(tooltip_width / 2)
                .max(content_start)
                .min(content_end.saturating_sub(tooltip_width));
            let tooltip_y = (top_row + 1).min(pl.inner_end_y.saturating_sub(1));

            if tooltip_y < buf_area.y + buf_area.height {
                let mut col = tooltip_x;
                for (text, style) in parts {
                    for ch in text.chars() {
                        if col >= content_end {
                            break;
                        }
                        if let Some(cell) = buf.cell_mut((col, tooltip_y)) {
                            cell.set_char(ch);
                            cell.set_style(*style);
                        }
                        col += 1;
                    }
                }
            }
        }
        return;
    }

    // --- Drag selection: highlight text and show tooltip ---
    let mut extracted_text = String::new();

    let row_start = top_row.max(pl.inner_y);
    let row_end = bot_row.min(pl.inner_end_y.saturating_sub(1));

    let highlight_style = Style::default().bg(theme.popup_border).fg(Color::Black);

    for (i, row) in (row_start..=row_end).enumerate() {
        if row >= buf_area.y + buf_area.height {
            break;
        }

        // Map terminal row to diff line index.
        let line_idx = diff_view
            .line_chunk_at_row(row, &pl)
            .map(|(line_idx, _)| line_idx)
            .unwrap_or_else(|| diff_view.fallback_line_idx_for_row(row, &pl));
        if let Some(diff_line) = diff_view.lines.get(line_idx) {
            // Skip file header separator lines.
            if diff_line.file_header.is_some() {
                continue;
            }
            // Skip slash-fill rows (the empty side for Insert/Delete lines).
            let is_slash_fill = diff_view.view_layout == DiffViewLayout::SideBySide
                && match selection.panel {
                    DiffPanel::Old => diff_line.change_type == ChangeType::Insert,
                    DiffPanel::New => diff_line.change_type == ChangeType::Delete,
                };
            if is_slash_fill {
                continue;
            }
        }

        // Column range: intersection of mouse selection cols with panel content cols.
        let sel_col_start = if row == top_row { top_col } else { 0 };
        let sel_col_end = if row == bot_row { bot_col } else { u16::MAX };
        let hl_start = sel_col_start.max(content_start);
        let hl_end = sel_col_end.min(content_end);

        if hl_start >= hl_end {
            continue;
        }

        let mut row_text = String::new();
        for col in hl_start..hl_end {
            if col >= buf_area.x + buf_area.width {
                break;
            }
            if let Some(cell) = buf.cell_mut((col, row)) {
                row_text.push_str(cell.symbol());
                cell.set_style(highlight_style);
            }
        }

        let trimmed = row_text.trim_end();
        if !trimmed.is_empty() {
            if !extracted_text.is_empty() {
                extracted_text.push('\n');
            }
            extracted_text.push_str(trimmed);
        } else if i > 0 && i < (row_end - row_start) as usize {
            // Preserve blank lines in the middle of the selection.
            extracted_text.push('\n');
        }
    }

    // Store extracted text for the copy action.
    if let Some(ref mut sel) = diff_view.selection {
        sel.text = extracted_text;
    }

    // Tooltip below the selection (only after drag finishes).
    if !selection.dragging {
        let tooltip_style = Style::default().bg(theme.selected_bg).fg(theme.text_strong);
        let key_style = Style::default()
            .bg(theme.selected_bg)
            .fg(theme.accent_secondary)
            .add_modifier(Modifier::BOLD);

        // Build parts conditionally: include "e edit" only if file is on disk.
        let mut parts: Vec<(&str, Style)> = Vec::new();
        if diff_view.file_exists_on_disk {
            parts.push((" ", tooltip_style));
            parts.push(("e", key_style));
            parts.push((" edit  ", tooltip_style));
        } else {
            parts.push((" ", tooltip_style));
        }
        parts.push(("y", key_style));
        parts.push((" copy  ", tooltip_style));
        parts.push(("esc", key_style));
        parts.push((" ", tooltip_style));

        let tooltip_width: u16 = parts.iter().map(|(s, _)| s.len() as u16).sum();
        let tooltip_x = bot_col
            .saturating_sub(tooltip_width / 2)
            .max(content_start)
            .min(content_end.saturating_sub(tooltip_width));
        let tooltip_y = (bot_row + 1).min(pl.inner_end_y.saturating_sub(1));

        if tooltip_y < buf_area.y + buf_area.height {
            let mut col = tooltip_x;
            for (text, style) in &parts {
                for ch in text.chars() {
                    if col >= content_end {
                        break;
                    }
                    if let Some(cell) = buf.cell_mut((col, tooltip_y)) {
                        cell.set_char(ch);
                        cell.set_style(*style);
                    }
                    col += 1;
                }
            }
        }
    }
}

pub fn render_loading_overlay(
    frame: &mut Frame,
    area: Rect,
    spinner_frame: usize,
    theme: &Theme,
    title: &str,
    message: &str,
    hint: Option<(&str, &str)>,
) {
    if area.width < 4 || area.height < 3 {
        return;
    }

    // Compact bottom-right toast: spinner + short label, no large modal.
    let spinner = SPINNER_CHARS[(spinner_frame / 8) % SPINNER_CHARS.len()];
    let label = if message.is_empty() {
        title.to_string()
    } else if message.len() <= 40 {
        message.to_string()
    } else {
        // Prefer title when message is long (e.g. "Pushing branch to remote...").
        title.to_string()
    };
    let content = format!(" {spinner} {label} ");
    let mut width = (content.chars().count() as u16)
        .saturating_add(2)
        .min(area.width);
    width = width.max(12).min(area.width.saturating_sub(2).max(1));
    let height = if hint.is_some() { 4u16 } else { 3u16 };
    let height = height.min(area.height);
    let x = area.width.saturating_sub(width).saturating_sub(1);
    let y = area.height.saturating_sub(height).saturating_sub(1);
    let popup_rect = Rect::new(x, y, width, height);
    frame.render_widget(Clear, popup_rect);

    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(theme.borders_for("popup"))
        .border_type(theme.border_type_for("popup"))
        .border_style(Style::default().fg(theme.accent_secondary));

    let mut text = vec![Line::from(vec![
        Span::styled(format!(" {spinner} "), Style::default().fg(theme.accent)),
        Span::styled(
            label,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    if let Some((key, desc)) = hint {
        text.push(Line::from(vec![
            Span::styled(
                format!(" {key}"),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {desc}"), Style::default().fg(theme.text_dimmed)),
        ]));
    }

    let widget = Paragraph::new(text).block(block);
    frame.render_widget(widget, popup_rect);
}
