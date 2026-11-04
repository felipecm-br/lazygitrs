//! Unified (single-column) diff body rendering.

use super::*;

pub(super) fn render_unified_diff_body(
    buf: &mut Buffer,
    inner: Rect,
    state: &mut DiffViewState,
    theme: &Theme,
    visible_height: usize,
    show_revert_markers: bool,
    spinner_frame: usize,
) {
    const GUTTER_WIDTH: u16 = 5;
    const PREFIX_WIDTH: u16 = 2;

    let content_width = inner.width.saturating_sub(GUTTER_WIDTH * 2 + PREFIX_WIDTH);
    if content_width == 0 {
        return;
    }
    state.last_content_width = content_width as usize;

    // Skip `scroll_offset` visual rows from the start of the flattened stream
    // (all deletes in a change block, then all inserts) so mid-block scrolling
    // does not drop earlier paired lines.
    let mut skip = state.scroll_offset;
    // Sticky file header: pin the current file's separator to the first row
    // when scrolled past it in a multi-file diff.
    let sticky = state
        .sticky_for_unified_with_width(content_width as usize)
        .map(|(_, h)| h.to_string());
    let mut row = 0usize;
    if let Some(ref header) = sticky {
        render_file_header(buf, inner.x, inner.y, inner.width, header, theme);
        row = 1;
    }
    let mut line_idx = 0usize;
    while line_idx < state.lines.len() {
        if row >= visible_height {
            break;
        }
        let diff_line = &state.lines[line_idx];

        if let Some(ref header) = diff_line.file_header {
            if skip > 0 {
                skip -= 1;
                line_idx += 1;
                continue;
            }
            let y = inner.y + row as u16;
            render_file_header(buf, inner.x, y, inner.width, header, theme);
            row += 1;
            line_idx += 1;
            continue;
        }

        if let Some(message) = diff_line.preview_placeholder {
            if skip > 0 {
                skip -= 1;
            } else {
                // Inline images overlay these rows; skip stripes when visible.
                let has_inline = !state.image_preview_hidden
                    && state.inline_images.contains_key(&diff_line.section_index);
                if !has_inline {
                    render_preview_placeholder(
                        buf,
                        Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
                        message,
                        theme,
                    );
                }
                row += 1;
            }
            line_idx += 1;
            continue;
        }

        if diff_line.change_type == ChangeType::Equal {
            let height =
                unified_line_visual_height(diff_line, content_width as usize, state, line_idx);
            if skip >= height {
                skip -= height;
                line_idx += 1;
                continue;
            }
            let default_hl = FileHighlighter::default();
            let (_, new_highlighter) = state
                .highlighters_for_section(diff_line.section_index)
                .unwrap_or((&default_hl, &default_hl));
            let old_num = state.file_line_number(line_idx, DiffPanel::Old);
            let new_num = state.file_line_number(line_idx, DiffPanel::New);
            let line_data = diff_line
                .new_line
                .as_ref()
                .or(diff_line.old_line.as_ref())
                .map(|(n, text)| (*n, text.as_str()));
            // Consume leading wrap rows that fall before the viewport.
            let start_chunk = skip;
            skip = 0;
            render_unified_row(
                buf,
                inner,
                &mut row,
                visible_height,
                state,
                old_num,
                new_num,
                ' ',
                line_data,
                &None,
                ChangeType::Equal,
                false,
                new_highlighter,
                Color::Reset,
                Color::Reset,
                theme.diff_gutter,
                content_width,
                None,
                theme,
                start_chunk,
                true,
            );
            render_diff_annotations(
                buf,
                state,
                diff_line,
                line_idx,
                inner,
                &mut row,
                visible_height,
                content_width,
                GUTTER_WIDTH * 2,
                0,
                inner.x,
                content_width,
                theme,
                spinner_frame,
            );
            line_idx += 1;
            continue;
        }

        let block_end = next_unified_change_block_end(&state.lines, line_idx);
        let block_staged = state.is_staged_line(line_idx);
        let mut marker_hunk_idx = if show_revert_markers && state.is_hunk_start_line(line_idx) {
            state.hunk_index_for_start_line(line_idx)
        } else {
            None
        };

        if state.side_view != DiffSideView::NewOnly {
            for idx in line_idx..block_end {
                if row >= visible_height {
                    break;
                }
                let line = &state.lines[idx];
                if !matches!(line.change_type, ChangeType::Delete | ChangeType::Modified) {
                    continue;
                }
                let height = unified_line_row_count(&line.old_line, content_width as usize, state);
                if skip >= height {
                    skip -= height;
                    // Still consume the hunk marker so it only shows on the
                    // first visible row of the block.
                    marker_hunk_idx.take();
                    continue;
                }
                let start_chunk = skip;
                skip = 0;
                let default_hl = FileHighlighter::default();
                let (old_highlighter, _) = state
                    .highlighters_for_section(line.section_index)
                    .unwrap_or((&default_hl, &default_hl));
                let old_num = state.file_line_number(idx, DiffPanel::Old);
                let old_line = line.old_line.as_ref().map(|(n, text)| (*n, text.as_str()));
                let old_segments = line.old_segments.clone();
                render_unified_row(
                    buf,
                    inner,
                    &mut row,
                    visible_height,
                    state,
                    old_num,
                    None,
                    '-',
                    old_line,
                    &old_segments,
                    ChangeType::Delete,
                    true,
                    old_highlighter,
                    dim_unstaged_bg(theme.diff_remove_bg, block_staged, theme),
                    dim_unstaged_bg(theme.diff_remove_gutter_bg, block_staged, theme),
                    theme.diff_remove_gutter_fg,
                    content_width,
                    marker_hunk_idx.take(),
                    theme,
                    start_chunk,
                    block_staged,
                );
            }
        }

        if state.side_view != DiffSideView::OldOnly {
            for idx in line_idx..block_end {
                if row >= visible_height {
                    break;
                }
                let line = &state.lines[idx];
                if !matches!(line.change_type, ChangeType::Insert | ChangeType::Modified) {
                    continue;
                }
                let height = unified_line_row_count(&line.new_line, content_width as usize, state);
                if skip >= height {
                    skip -= height;
                    marker_hunk_idx.take();
                    continue;
                }
                let start_chunk = skip;
                skip = 0;
                let default_hl = FileHighlighter::default();
                let (_, new_highlighter) = state
                    .highlighters_for_section(line.section_index)
                    .unwrap_or((&default_hl, &default_hl));
                let new_num = state.file_line_number(idx, DiffPanel::New);
                let new_line = line.new_line.as_ref().map(|(n, text)| (*n, text.as_str()));
                let new_segments = line.new_segments.clone();
                render_unified_row(
                    buf,
                    inner,
                    &mut row,
                    visible_height,
                    state,
                    None,
                    new_num,
                    '+',
                    new_line,
                    &new_segments,
                    ChangeType::Insert,
                    false,
                    new_highlighter,
                    dim_unstaged_bg(theme.diff_add_bg, block_staged, theme),
                    dim_unstaged_bg(theme.diff_add_gutter_bg, block_staged, theme),
                    theme.diff_add_gutter_fg,
                    content_width,
                    marker_hunk_idx.take(),
                    theme,
                    start_chunk,
                    block_staged,
                );
            }
        }

        line_idx = block_end;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_unified_row(
    buf: &mut Buffer,
    inner: Rect,
    row: &mut usize,
    visible_height: usize,
    state: &DiffViewState,
    old_num: Option<usize>,
    new_num: Option<usize>,
    sign: char,
    line_data: Option<(usize, &str)>,
    segments: &Option<Vec<InlineSegment>>,
    change_type: ChangeType,
    is_old_side: bool,
    highlighter: &FileHighlighter,
    bg: Color,
    gutter_bg: Color,
    gutter_fg: Color,
    content_width: u16,
    marker_hunk_idx: Option<usize>,
    theme: &Theme,
    start_chunk: usize,
    staged: bool,
) {
    if *row >= visible_height {
        return;
    }

    const GUTTER_WIDTH: u16 = 5;
    const PREFIX_WIDTH: u16 = 2;

    let old_num_x = inner.x;
    let new_num_x = inner.x + GUTTER_WIDTH;
    let prefix_x = inner.x + GUTTER_WIDTH * 2;
    let content_x = prefix_x + PREFIX_WIDTH;
    let gutter_style = Style::default().fg(gutter_fg).bg(gutter_bg);
    let sign_fg = match sign {
        '-' => theme.diff_remove_gutter_fg,
        '+' => theme.diff_add_gutter_fg,
        _ => theme.diff_gutter,
    };
    let sign_style = Style::default().fg(sign_fg).bg(gutter_bg);
    let fill_style = Style::default().bg(bg);
    let spans = build_content_spans(
        line_data,
        segments,
        change_type,
        is_old_side,
        highlighter,
        bg,
        theme,
        if state.wrap {
            usize::MAX / 2
        } else {
            content_width as usize
        },
        staged,
    );
    let rows = if state.wrap {
        wrap_spans(&spans, content_width as usize)
    } else {
        vec![spans]
    };

    for (chunk_idx, chunk) in rows.iter().enumerate().skip(start_chunk) {
        if *row >= visible_height {
            break;
        }
        let y = inner.y + *row as u16;
        let old_num_text = unified_line_number_text(old_num, chunk_idx);
        let new_num_text = unified_line_number_text(new_num, chunk_idx);
        buf_write_str(buf, old_num_x, y, &old_num_text, gutter_style, GUTTER_WIDTH);
        buf_write_str(buf, new_num_x, y, &new_num_text, gutter_style, GUTTER_WIDTH);

        if chunk_idx == 0 {
            if let Some(hunk_idx) = marker_hunk_idx {
                let (glyph, fg) = hunk_marker_glyph_and_fg(state, hunk_idx, theme);
                buf_write_str(
                    buf,
                    prefix_x,
                    y,
                    glyph,
                    Style::default()
                        .fg(fg)
                        .bg(gutter_bg)
                        .add_modifier(Modifier::BOLD),
                    1,
                );
            } else {
                buf_write_str(buf, prefix_x, y, " ", sign_style, 1);
            }
            buf_write_str(buf, prefix_x + 1, y, &sign.to_string(), sign_style, 1);
        } else {
            buf_write_str(buf, prefix_x, y, " ·", sign_style, PREFIX_WIDTH);
        }

        let fill: String = std::iter::repeat_n(' ', content_width as usize).collect();
        buf_write_str(buf, content_x, y, &fill, fill_style, content_width);
        buf_write_spans(
            buf,
            content_x,
            y,
            chunk,
            content_width,
            if state.wrap {
                0
            } else {
                state.horizontal_scroll
            },
            bg,
        );
        *row += 1;
    }
}

pub(super) fn unified_line_number_text(num: Option<usize>, chunk_idx: usize) -> String {
    match (num, chunk_idx) {
        (Some(n), 0) => format!("{:>4} ", n),
        (Some(_), _) => "   · ".to_string(),
        (None, _) => "     ".to_string(),
    }
}

/// Glyph + foreground for a hunk marker: blue `󰧛` for unstaged hunks,
/// green `✓` for staged hunks. No hover color — hover only shows the
/// tooltip, selection alone drives the highlight.
pub(super) fn hunk_marker_glyph_and_fg(
    state: &DiffViewState,
    hunk_idx: usize,
    theme: &Theme,
) -> (&'static str, Color) {
    let is_selected = Some(hunk_idx) == state.selected_revert_hunk;
    let fg = if is_selected {
        theme.accent
    } else if state.is_staged_hunk(hunk_idx) {
        theme.file_staged.fg.unwrap_or(Color::Green)
    } else {
        theme.separator
    };
    let glyph = if state.is_staged_hunk(hunk_idx) {
        "✓"
    } else {
        "󰧛"
    };
    (glyph, fg)
}
