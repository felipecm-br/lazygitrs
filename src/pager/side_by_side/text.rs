//! Low-level buffer writes, wrapping and visual line-height computation.

use super::*;

/// Write a string directly to the buffer at (x, y) with the given style, clamped to max_width.
#[inline]
pub(super) fn buf_write_str(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    text: &str,
    style: Style,
    max_width: u16,
) {
    let buf_area = buf.area();
    if y < buf_area.y || y >= buf_area.y + buf_area.height {
        return;
    }
    let mut col = x;
    let end_col = x.saturating_add(max_width).min(buf_area.x + buf_area.width);
    for ch in text.chars() {
        if col >= end_col {
            break;
        }
        let width = unicode_display_width(ch);
        if width == 0 {
            continue;
        }
        if let Some(cell) = buf.cell_mut((col, y)) {
            cell.set_char(ch);
            cell.set_style(style);
        }
        col += width as u16;
    }
}

/// Write styled spans directly to the buffer at (x, y), clamped to max_width.
/// `h_scroll` skips the first N display columns of content. Trailing cells
/// up to `max_width` are filled with `fill_bg` so hunk backgrounds span the
/// full panel width (lumen-style) instead of ending at the last character.
#[inline]
pub(super) fn buf_write_spans(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    spans: &[Span<'_>],
    max_width: u16,
    h_scroll: usize,
    fill_bg: Color,
) {
    let buf_area = buf.area();
    if y < buf_area.y || y >= buf_area.y + buf_area.height {
        return;
    }
    let mut col = x;
    let end_col = x.saturating_add(max_width).min(buf_area.x + buf_area.width);
    let mut skipped: usize = 0;
    for span in spans {
        for ch in span.content.chars() {
            if col >= end_col {
                return;
            }
            let width = unicode_display_width(ch);
            if width == 0 {
                continue;
            }
            if skipped < h_scroll {
                skipped += width;
                continue;
            }
            if let Some(cell) = buf.cell_mut((col, y)) {
                cell.set_char(ch);
                cell.set_style(span.style);
            }
            col += width as u16;
        }
    }
    if col < end_col {
        let fill_style = Style::default().bg(fill_bg);
        while col < end_col {
            if let Some(cell) = buf.cell_mut((col, y)) {
                cell.set_char(' ');
                cell.set_style(fill_style);
            }
            col += 1;
        }
    }
}

/// Get the display width of a character (1 for most, 2 for CJK wide chars).
#[inline]
pub(super) fn unicode_display_width(ch: char) -> usize {
    if ch == '\t' || ch == '\n' || ch == '\r' {
        return 0;
    }
    unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1)
}

/// Split a list of styled spans into visual rows of at most `width` display columns each.
/// Used by wrap mode to soft-wrap long diff lines.
/// Count how many visual rows a single line of `text` would occupy when wrapped
/// at `width` display columns. Mirrors the row count produced by `wrap_spans`,
/// without building styled spans.
pub(super) fn wrap_row_count(text: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let widths: Vec<usize> = text
        .chars()
        .filter_map(|ch| {
            let w = unicode_display_width(ch);
            if w > 0 { Some(w) } else { None }
        })
        .collect();
    if widths.is_empty() {
        return 1;
    }
    let mut rows = 0usize;
    let mut i = 0usize;
    while i < widths.len() {
        let mut col_w = 0usize;
        let mut end = i;
        while end < widths.len() {
            let w = widths[end];
            if col_w + w > width {
                break;
            }
            col_w += w;
            end += 1;
        }
        if end == i {
            end = i + 1;
        }
        i = end;
        rows += 1;
    }
    rows
}

/// Visual height (in panel rows) of a diff line, matching the renderer's
/// `num_rows` calculation in side-by-side wrap mode.
pub fn line_visual_height(
    diff_line: &DiffLine,
    panel_width: usize,
    right_content_width: usize,
    state: &DiffViewState,
    line_idx: usize,
) -> usize {
    if diff_line.file_header.is_some() {
        return 1;
    }
    let mut extra_rows = 0;
    let edit_active = state
        .inline_edit
        .as_ref()
        .map(|e| e.line_idx == line_idx)
        .unwrap_or(false);
    // Count saved notes (excluding the one being edited in-place).
    // When notes are hidden, saved notes contribute 0 rows.
    let saved_note_count = if !state.notes_visible {
        0
    } else if edit_active {
        let editing_id = state
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
    extra_rows += saved_note_count * 5;
    if edit_active {
        extra_rows += 5;
    }
    if !state.wrap {
        return 1 + extra_rows;
    }
    let is_insert = diff_line.change_type == ChangeType::Insert;
    let is_delete = diff_line.change_type == ChangeType::Delete;
    let left_rows = if is_insert {
        0
    } else {
        diff_line
            .old_line
            .as_ref()
            .map(|(_, t)| wrap_row_count(t, panel_width))
            .unwrap_or(1)
    };
    let right_rows = if is_delete {
        0
    } else {
        diff_line
            .new_line
            .as_ref()
            .map(|(_, t)| wrap_row_count(t, right_content_width))
            .unwrap_or(1)
    };
    left_rows.max(right_rows).max(1) + extra_rows
}

pub fn unified_line_visual_height(
    diff_line: &DiffLine,
    content_width: usize,
    state: &DiffViewState,
    line_idx: usize,
) -> usize {
    if diff_line.file_header.is_some() {
        return 1;
    }
    let mut extra_rows = 0;
    let edit_active = state
        .inline_edit
        .as_ref()
        .map(|e| e.line_idx == line_idx)
        .unwrap_or(false);
    let saved_note_count = if !state.notes_visible {
        0
    } else if edit_active {
        let editing_id = state
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
    extra_rows += saved_note_count * 5;
    if edit_active {
        extra_rows += 5;
    }

    let code_rows = match diff_line.change_type {
        ChangeType::Equal => unified_line_row_count(&diff_line.new_line, content_width, state),
        ChangeType::Delete => {
            if state.side_view == DiffSideView::NewOnly {
                0
            } else {
                unified_line_row_count(&diff_line.old_line, content_width, state)
            }
        }
        ChangeType::Insert => {
            if state.side_view == DiffSideView::OldOnly {
                0
            } else {
                unified_line_row_count(&diff_line.new_line, content_width, state)
            }
        }
        ChangeType::Modified => {
            let old_rows = if state.side_view == DiffSideView::NewOnly {
                0
            } else {
                unified_line_row_count(&diff_line.old_line, content_width, state)
            };
            let new_rows = if state.side_view == DiffSideView::OldOnly {
                0
            } else {
                unified_line_row_count(&diff_line.new_line, content_width, state)
            };
            old_rows + new_rows
        }
    };
    code_rows + extra_rows
}

pub(super) fn is_unified_change_line(diff_line: &DiffLine) -> bool {
    diff_line.file_header.is_none() && !matches!(diff_line.change_type, ChangeType::Equal)
}

pub(super) fn next_unified_change_block_end(lines: &[DiffLine], start: usize) -> usize {
    lines[start..]
        .iter()
        .position(|line| !is_unified_change_line(line))
        .map(|offset| start + offset)
        .unwrap_or(lines.len())
}

pub(super) fn unified_line_row_count(
    line: &Option<(usize, String)>,
    content_width: usize,
    state: &DiffViewState,
) -> usize {
    if state.wrap {
        line.as_ref()
            .map(|(_, text)| wrap_row_count(text, content_width))
            .unwrap_or(1)
    } else {
        1
    }
}

pub(super) fn wrap_spans<'a>(spans: &[Span<'a>], width: usize) -> Vec<Vec<Span<'a>>> {
    if width == 0 {
        return vec![vec![]];
    }

    // Collect (char, style) pairs, skipping zero-width control chars
    let pairs: Vec<(char, Style)> = spans
        .iter()
        .flat_map(|sp| {
            let style = sp.style;
            sp.content.chars().filter_map(move |ch| {
                if unicode_display_width(ch) > 0 {
                    Some((ch, style))
                } else {
                    None
                }
            })
        })
        .collect();

    if pairs.is_empty() {
        return vec![vec![]];
    }

    let mut rows: Vec<Vec<Span<'a>>> = Vec::new();
    let mut start = 0;

    while start < pairs.len() {
        let mut col_w = 0usize;
        let mut end = start;

        while end < pairs.len() {
            let w = unicode_display_width(pairs[end].0);
            if col_w + w > width {
                break;
            }
            col_w += w;
            end += 1;
        }
        // Avoid infinite loop when a single char is wider than `width`
        if end == start {
            end = start + 1;
        }

        // Group consecutive chars with the same style into spans
        let mut row_spans: Vec<Span<'a>> = Vec::new();
        let mut i = start;
        while i < end {
            let style = pairs[i].1;
            let mut text = String::new();
            while i < end && pairs[i].1 == style {
                text.push(pairs[i].0);
                i += 1;
            }
            row_spans.push(Span::styled(text, style));
        }

        rows.push(row_spans);
        start = end;
    }

    rows
}
