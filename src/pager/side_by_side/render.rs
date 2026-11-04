//! Top-level side-by-side diff rendering and inline image regions.

use super::*;

/// Keep the rightmost `budget` columns of `text`, prefixed with `…` when
/// truncated. Tail-preserving so `src/gui/mod.rs` still reads as `…i/mod.rs`.
pub(super) fn truncate_front_ellipsis(text: &str, budget: usize) -> String {
    if Span::raw(text.to_string()).width() <= budget {
        return text.to_string();
    }
    if budget == 0 {
        return String::new();
    }
    let mut kept = String::new();
    let mut width = 1; // the `…` prefix
    for ch in text.chars().rev() {
        let cw = Span::raw(ch.to_string()).width().max(1);
        if width + cw > budget {
            break;
        }
        kept.insert(0, ch);
        width += cw;
    }
    format!("…{kept}")
}

/// Render a side-by-side diff view into the given area.
/// Uses direct buffer writes instead of per-cell Paragraph widgets for performance.
pub fn render_diff(
    frame: &mut Frame,
    area: Rect,
    state: &mut DiffViewState,
    theme: &Theme,
    focused: bool,
    diff_loading: bool,
    show_revert_markers: bool,
    spinner_frame: usize,
) {
    let border_style = if focused {
        theme.active_border
    } else {
        theme.inactive_border
    };

    if state.is_empty() {
        let msg = if diff_loading {
            " Loading diff..."
        } else {
            " No changes to display"
        };
        let block = Block::default()
            .title(" Diff ")
            .borders(theme.borders_for("diff"))
            .border_type(theme.border_type_for("diff"))
            .border_style(border_style);
        let widget = Paragraph::new(msg);
        frame.render_widget(widget.block(block), area);
        return;
    }

    let side_label = match state.side_view {
        DiffSideView::OldOnly => " [old] ",
        DiffSideView::NewOnly => " [new] ",
        DiffSideView::Both => "",
    };
    // Single-list Files view: `README.md  2 Staged  0 Unstaged`, reusing
    // the `MM` badge colors (staged green, unstaged yellow) so the two
    // counts read as the same index/worktree split as the file list.
    // Overflow: the right `[c/t]` hunk counter wins over the left title.
    // Reserve its width first, then ellipsis-truncate the filename to fit;
    // only drop the staged counts when even a stub filename won't fit.
    let hunk_pos = state.hunk_position();
    let right_width = hunk_pos
        .map(|(c, t)| Line::from(format!(" [{c}/{t}] ")).width())
        .unwrap_or(0);
    let available = area.width.saturating_sub(2) as usize;
    let left_budget = available.saturating_sub(right_width + 1);
    let leading = " ";
    let trailing = " ";
    let fixed = leading.len() + side_label.len() + trailing.len();
    let counts_width = match state.staged_counts() {
        Some((s, u)) => format!(" {s} Staged").len() + format!(" {u} Unstaged").len(),
        None => 0,
    };
    // Minimum readable filename stub (`…a.rs`-sized) before counts give way.
    const MIN_FILENAME: usize = 4;
    let keep_counts = state.staged_counts().is_some()
        && left_budget.saturating_sub(fixed + counts_width)
            >= MIN_FILENAME.min(state.filename.len());
    let active_counts_width = if keep_counts { counts_width } else { 0 };
    let filename_budget = left_budget.saturating_sub(fixed + active_counts_width);
    let mut filename = state.filename.clone();
    if Span::raw(filename.clone()).width() > filename_budget {
        filename = truncate_front_ellipsis(&filename, filename_budget);
    }
    let mut title_spans = vec![Span::raw(format!("{leading}{filename}{side_label}"))];
    if keep_counts && let Some((s, u)) = state.staged_counts() {
        // Zero counts mute to dimmed so the nonzero side pops.
        let staged_fg = if s == 0 {
            theme.text_dimmed
        } else {
            theme.file_staged.fg.unwrap_or(theme.text_dimmed)
        };
        let unstaged_fg = if u == 0 {
            theme.text_dimmed
        } else {
            theme.file_unstaged.fg.unwrap_or(theme.text_dimmed)
        };
        title_spans.push(Span::styled(
            format!(" {s} Staged"),
            Style::default().fg(staged_fg),
        ));
        title_spans.push(Span::styled(
            format!(" {u} Unstaged"),
            Style::default().fg(unstaged_fg),
        ));
    }
    title_spans.push(Span::raw(trailing.to_string()));
    let title = Line::from(title_spans);

    let mut block = Block::default()
        .title(title)
        .borders(theme.borders_for("diff"))
        .border_type(theme.border_type_for("diff"))
        .border_style(border_style);

    if let Some((current, total)) = hunk_pos {
        block = block.title(
            Line::from(Span::styled(
                format!(" [{current}/{total}] "),
                Style::default().fg(theme.text_dimmed),
            ))
            .alignment(ratatui::layout::Alignment::Right),
        );
    }

    // Bottom-right footnote: revert-hunk undo indicator. Only shown when
    // there's something to undo; the denominator is the peak stack depth
    // since it last drained, so a streak reads `1/1`, `2/2`, ... and undos
    // walk it back down to `1/3` etc.
    let undo_n = state.revert_undo_stack.len();
    if undo_n > 0 {
        let undo_m = state.revert_undo_high_water.max(undo_n);
        block = block.title_bottom(
            Line::from(vec![
                Span::styled(" ", Style::default().fg(theme.text_dimmed)),
                Span::styled(
                    "u",
                    Style::default()
                        .fg(theme.accent_secondary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" undo revert ({}/{}) ", undo_n, undo_m),
                    Style::default().fg(theme.text_dimmed),
                ),
            ])
            .alignment(ratatui::layout::Alignment::Right),
        );
    }

    // The previous selection's diff stays on screen while the next one loads;
    // flag it so a slow load doesn't pass for current content.
    if diff_loading {
        block = block.title_bottom(
            Line::from(Span::styled(
                " loading… ",
                Style::default().fg(theme.text_dimmed),
            ))
            .alignment(ratatui::layout::Alignment::Right),
        );
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if !state.image_preview_hidden
        && let Some(preview) = &mut state.image_preview
    {
        crate::pager::image_preview::render(frame, inner, preview, state.side_view, theme);
        return;
    }

    // Only a lone unavailable file fills the viewport. In multi-file buffers,
    // the five placeholder rows below remain ordinary scrollable rows.
    // Inline images use compact rows even when lone; never take the fallback.
    if state.inline_images.is_empty()
        && state
            .lines
            .iter()
            .all(|line| line.preview_placeholder.is_some())
    {
        let message = state
            .lines
            .iter()
            .find_map(|line| {
                line.preview_placeholder
                    .filter(|message| !message.trim().is_empty())
            })
            .unwrap_or("");
        render_preview_placeholder(frame.buffer_mut(), inner, message, theme);
        return;
    }

    if inner.width < 10 || inner.height < 2 {
        return;
    }

    let gutter_width = 5u16;
    let divider_width = 2u16;

    // Detect new file: old content is empty, so no left panel needed
    let is_new_file = state.old_content.is_empty() && state.sections.len() <= 1;

    // Single-side view mode ([ for old, ] for new)
    let single_side = match state.side_view {
        DiffSideView::OldOnly => Some(DiffPanel::Old),
        DiffSideView::NewOnly => Some(DiffPanel::New),
        DiffSideView::Both => None,
    };

    let visible_height = inner.height as usize;
    if state.view_layout == DiffViewLayout::Unified {
        {
            let buf = frame.buffer_mut();
            render_unified_diff_body(
                buf,
                inner,
                state,
                theme,
                visible_height,
                show_revert_markers,
                spinner_frame,
            );
        }
        render_inline_image_regions(frame, area, inner, state, theme);
        return;
    }
    let buf = frame.buffer_mut();

    if single_side.is_some() || is_new_file {
        // Single-panel mode: new file, old-only, or new-only
        let show_panel = single_side.unwrap_or(DiffPanel::New); // new-file defaults to New
        let content_width = inner.width.saturating_sub(gutter_width);

        // Sticky file header: pin the current file's separator to the first
        // row when scrolled past it in a multi-file diff.
        let sticky = state.sticky_file_header().map(|s| s.to_string());
        let mut row = 0usize;
        if let Some(ref header) = sticky {
            render_file_header(buf, inner.x, inner.y, inner.width, header, theme);
            row = 1;
        }
        let start = state.scroll_offset.min(state.lines.len());
        for (idx_offset, diff_line) in state.lines[start..].iter().enumerate() {
            if row >= visible_height {
                break;
            }
            let line_idx = start + idx_offset;

            // Handle file header separator lines
            if let Some(ref header) = diff_line.file_header {
                let y = inner.y + row as u16;
                render_file_header(buf, inner.x, y, inner.width, header, theme);
                row += 1;
                continue;
            }

            if let Some(message) = diff_line.preview_placeholder {
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
                continue;
            }
            render_diff_annotations(
                buf,
                state,
                diff_line,
                line_idx,
                inner,
                &mut row,
                visible_height,
                content_width, // panel_width
                gutter_width,
                0,             // divider_width
                inner.x,       // div_x
                content_width, // right_content_width — full width in single-panel mode
                theme,
                spinner_frame,
            );

            let default_hl = FileHighlighter::default();
            let (old_highlighter, new_highlighter) = state
                .highlighters_for_section(diff_line.section_index)
                .unwrap_or((&default_hl, &default_hl));

            // Pick the appropriate side's data
            let (line_data, segments, is_old_side, highlighter) = match show_panel {
                DiffPanel::Old => (
                    &diff_line.old_line,
                    &diff_line.old_segments,
                    true,
                    old_highlighter,
                ),
                DiffPanel::New => (
                    &diff_line.new_line,
                    &diff_line.new_segments,
                    false,
                    new_highlighter,
                ),
            };

            let staged = state.is_staged_line(line_idx);
            let bg = if is_new_file {
                theme.diff_add_bg
            } else {
                match (diff_line.change_type, show_panel) {
                    (ChangeType::Delete, DiffPanel::Old) => theme.diff_remove_bg,
                    (ChangeType::Insert, DiffPanel::New) => theme.diff_add_bg,
                    (ChangeType::Modified, DiffPanel::Old) => theme.diff_remove_bg,
                    (ChangeType::Modified, DiffPanel::New) => theme.diff_add_bg,
                    _ => Color::Reset,
                }
            };
            let bg = dim_unstaged_bg(bg, staged, theme);
            let gutter_bg = if is_new_file {
                theme.diff_add_gutter_bg
            } else {
                match (diff_line.change_type, show_panel) {
                    (ChangeType::Delete, DiffPanel::Old) => theme.diff_remove_gutter_bg,
                    (ChangeType::Insert, DiffPanel::New) => theme.diff_add_gutter_bg,
                    (ChangeType::Modified, DiffPanel::Old) => theme.diff_remove_gutter_bg,
                    (ChangeType::Modified, DiffPanel::New) => theme.diff_add_gutter_bg,
                    _ => Color::Reset,
                }
            };
            let gutter_bg = dim_unstaged_bg(gutter_bg, staged, theme);
            let gutter_fg = if is_new_file {
                theme.diff_add_gutter_fg
            } else {
                match (diff_line.change_type, show_panel) {
                    (ChangeType::Delete, DiffPanel::Old) => theme.diff_remove_gutter_fg,
                    (ChangeType::Insert, DiffPanel::New) => theme.diff_add_gutter_fg,
                    (ChangeType::Modified, DiffPanel::Old) => theme.diff_remove_gutter_fg,
                    (ChangeType::Modified, DiffPanel::New) => theme.diff_add_gutter_fg,
                    _ => theme.diff_gutter,
                }
            };

            let line_num = state
                .file_line_number(line_idx, show_panel)
                .map(|n| format!("{:>4} ", n))
                .unwrap_or_else(|| "     ".to_string());
            let gutter_style = Style::default().fg(gutter_fg).bg(gutter_bg);

            if state.wrap && line_data.is_some() {
                let spans = build_content_spans(
                    line_data.as_ref().map(|(n, t)| (*n, t.as_str())),
                    segments,
                    diff_line.change_type,
                    is_old_side,
                    highlighter,
                    bg,
                    theme,
                    usize::MAX / 2,
                    staged,
                );
                let wrapped = wrap_spans(&spans, content_width as usize);
                for (chunk_idx, chunk) in wrapped.iter().enumerate() {
                    if row >= visible_height {
                        break;
                    }
                    let y = inner.y + row as u16;
                    let gutter_text = if chunk_idx == 0 {
                        line_num.clone()
                    } else {
                        "   · ".to_string()
                    };
                    buf_write_str(buf, inner.x, y, &gutter_text, gutter_style, gutter_width);
                    buf_write_spans(buf, inner.x + gutter_width, y, chunk, content_width, 0, bg);
                    row += 1;
                }
            } else {
                let y = inner.y + row as u16;
                buf_write_str(buf, inner.x, y, &line_num, gutter_style, gutter_width);
                if line_data.is_some() {
                    let spans = build_content_spans(
                        line_data.as_ref().map(|(n, t)| (*n, t.as_str())),
                        segments,
                        diff_line.change_type,
                        is_old_side,
                        highlighter,
                        bg,
                        theme,
                        content_width as usize,
                        staged,
                    );
                    buf_write_spans(
                        buf,
                        inner.x + gutter_width,
                        y,
                        &spans,
                        content_width,
                        state.horizontal_scroll,
                        bg,
                    );
                } else {
                    let fill: String = std::iter::repeat_n(' ', content_width as usize).collect();
                    buf_write_str(
                        buf,
                        inner.x + gutter_width,
                        y,
                        &fill,
                        Style::default().bg(bg),
                        content_width,
                    );
                }
                row += 1;
            }
        }
    } else {
        // Normal side-by-side diff
        let total_chrome = gutter_width * 2 + divider_width;
        let content_width = if inner.width > total_chrome {
            inner.width - total_chrome
        } else {
            inner.width
        };
        let panel_width = content_width / 2;
        let div_x = inner.x + gutter_width + panel_width;
        let right_gutter_x = div_x + divider_width;
        let right_content_x = right_gutter_x + gutter_width;
        let right_content_width = inner
            .width
            .saturating_sub(gutter_width * 2 + panel_width + divider_width);

        let mut hover_tooltip_y: Option<u16> = None;

        // Sticky file header: pin the current file's separator to the first
        // row when scrolled past it in a multi-file diff.
        let sticky = state.sticky_file_header().map(|s| s.to_string());
        let mut row = 0usize;
        if let Some(ref header) = sticky {
            render_file_header(buf, inner.x, inner.y, inner.width, header, theme);
            row = 1;
        }
        let start = state.scroll_offset.min(state.lines.len());
        for (idx_offset, diff_line) in state.lines[start..].iter().enumerate() {
            if row >= visible_height {
                break;
            }
            let line_idx = start + idx_offset;

            // Handle file header separator lines
            if let Some(ref header) = diff_line.file_header {
                let y = inner.y + row as u16;
                render_file_header(buf, inner.x, y, inner.width, header, theme);
                row += 1;
                continue;
            }

            render_diff_annotations(
                buf,
                state,
                diff_line,
                line_idx,
                inner,
                &mut row,
                visible_height,
                panel_width,
                gutter_width,
                divider_width,
                div_x,
                right_content_width,
                theme,
                spinner_frame,
            );

            let default_hl = FileHighlighter::default();
            let (old_highlighter, new_highlighter) = state
                .highlighters_for_section(diff_line.section_index)
                .unwrap_or((&default_hl, &default_hl));

            if let Some(message) = diff_line.preview_placeholder {
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
                continue;
            }

            let staged = state.is_staged_line(line_idx);
            let (left_bg, right_bg) = line_bg_colors(diff_line.change_type, theme, staged);
            let (left_gutter_bg, right_gutter_bg) =
                gutter_bg_colors(diff_line.change_type, theme, staged);
            let (left_gutter_fg, right_gutter_fg) = gutter_fg_colors(diff_line.change_type, theme);
            let gutter_style = Style::default().fg(left_gutter_fg).bg(left_gutter_bg);
            let right_gutter_style = Style::default().fg(right_gutter_fg).bg(right_gutter_bg);
            let divider_style = Style::default().fg(theme.diff_gutter);

            let left_num = state
                .file_line_number(line_idx, DiffPanel::Old)
                .map(|n| format!("{:>4} ", n))
                .unwrap_or_else(|| "     ".to_string());
            let right_num = state
                .file_line_number(line_idx, DiffPanel::New)
                .map(|n| format!("{:>4} ", n))
                .unwrap_or_else(|| "     ".to_string());

            let is_insert = diff_line.change_type == ChangeType::Insert;
            let is_delete = diff_line.change_type == ChangeType::Delete;

            if state.wrap {
                // Build wrapped rows for each side
                let left_wrapped: Vec<Vec<Span<'_>>> = if is_insert {
                    vec![] // placeholder; slash fill rendered per row
                } else {
                    let spans = build_content_spans(
                        diff_line.old_line.as_ref().map(|(n, t)| (*n, t.as_str())),
                        &diff_line.old_segments,
                        diff_line.change_type,
                        true,
                        old_highlighter,
                        left_bg,
                        theme,
                        usize::MAX / 2,
                        staged,
                    );
                    wrap_spans(&spans, panel_width as usize)
                };
                let right_wrapped: Vec<Vec<Span<'_>>> = if is_delete {
                    vec![] // placeholder; slash fill rendered per row
                } else {
                    let spans = build_content_spans(
                        diff_line.new_line.as_ref().map(|(n, t)| (*n, t.as_str())),
                        &diff_line.new_segments,
                        diff_line.change_type,
                        false,
                        new_highlighter,
                        right_bg,
                        theme,
                        usize::MAX / 2,
                        staged,
                    );
                    wrap_spans(&spans, right_content_width as usize)
                };

                let num_rows = if is_insert {
                    right_wrapped.len().max(1)
                } else if is_delete {
                    left_wrapped.len().max(1)
                } else {
                    left_wrapped.len().max(right_wrapped.len()).max(1)
                };

                for chunk_idx in 0..num_rows {
                    if row >= visible_height {
                        break;
                    }
                    let y = inner.y + row as u16;

                    let left_gutter_text = if chunk_idx == 0 {
                        left_num.clone()
                    } else {
                        "   · ".to_string()
                    };
                    let right_gutter_text = if chunk_idx == 0 {
                        right_num.clone()
                    } else {
                        "   · ".to_string()
                    };

                    // Left gutter + content
                    buf_write_str(
                        buf,
                        inner.x,
                        y,
                        &left_gutter_text,
                        gutter_style,
                        gutter_width,
                    );
                    if is_insert {
                        let slash: String =
                            std::iter::repeat_n('╱', panel_width as usize).collect();
                        buf_write_str(
                            buf,
                            inner.x + gutter_width,
                            y,
                            &slash,
                            Style::default().fg(theme.diff_line_number).bg(left_bg),
                            panel_width,
                        );
                    } else if let Some(chunk) = left_wrapped.get(chunk_idx) {
                        buf_write_spans(
                            buf,
                            inner.x + gutter_width,
                            y,
                            chunk,
                            panel_width,
                            0,
                            left_bg,
                        );
                    } else {
                        let fill: String = std::iter::repeat_n(' ', panel_width as usize).collect();
                        buf_write_str(
                            buf,
                            inner.x + gutter_width,
                            y,
                            &fill,
                            Style::default().bg(left_bg),
                            panel_width,
                        );
                    }

                    // Divider or revert marker (first visual row of a hunk only).
                    let show_marker =
                        show_revert_markers && chunk_idx == 0 && state.is_hunk_start_line(line_idx);
                    let marker_hunk_idx = if show_marker {
                        state.hunk_index_for_start_line(line_idx)
                    } else {
                        None
                    };
                    let (divider_char, marker_style) = if show_marker {
                        let hunk_idx = marker_hunk_idx.unwrap_or(usize::MAX);
                        let (glyph, fg) = hunk_marker_glyph_and_fg(state, hunk_idx, theme);
                        (glyph, Style::default().fg(fg).add_modifier(Modifier::BOLD))
                    } else {
                        ("│", divider_style)
                    };
                    buf_write_str(buf, div_x, y, "  ", divider_style, divider_width);
                    buf_write_str(buf, div_x, y, divider_char, marker_style, divider_width);
                    if show_marker
                        && marker_hunk_idx.is_some()
                        && marker_hunk_idx == state.hovered_revert_hunk
                    {
                        hover_tooltip_y = Some(y);
                    }

                    // Right gutter + content
                    buf_write_str(
                        buf,
                        right_gutter_x,
                        y,
                        &right_gutter_text,
                        right_gutter_style,
                        gutter_width,
                    );
                    if is_delete {
                        let slash: String =
                            std::iter::repeat_n('╱', right_content_width as usize).collect();
                        buf_write_str(
                            buf,
                            right_content_x,
                            y,
                            &slash,
                            Style::default().fg(theme.diff_line_number).bg(right_bg),
                            right_content_width,
                        );
                    } else if let Some(chunk) = right_wrapped.get(chunk_idx) {
                        buf_write_spans(
                            buf,
                            right_content_x,
                            y,
                            chunk,
                            right_content_width,
                            0,
                            right_bg,
                        );
                    } else {
                        let fill: String =
                            std::iter::repeat_n(' ', right_content_width as usize).collect();
                        buf_write_str(
                            buf,
                            right_content_x,
                            y,
                            &fill,
                            Style::default().bg(right_bg),
                            right_content_width,
                        );
                    }

                    row += 1;
                }
            } else {
                let y = inner.y + row as u16;

                // Left gutter
                buf_write_str(buf, inner.x, y, &left_num, gutter_style, gutter_width);

                // Left content
                let left_spans = if is_insert {
                    let slash_fill: String =
                        std::iter::repeat_n('╱', panel_width as usize).collect();
                    vec![Span::styled(
                        slash_fill,
                        Style::default().fg(theme.diff_line_number).bg(left_bg),
                    )]
                } else {
                    build_content_spans(
                        diff_line.old_line.as_ref().map(|(n, t)| (*n, t.as_str())),
                        &diff_line.old_segments,
                        diff_line.change_type,
                        true,
                        old_highlighter,
                        left_bg,
                        theme,
                        panel_width as usize,
                        staged,
                    )
                };
                buf_write_spans(
                    buf,
                    inner.x + gutter_width,
                    y,
                    &left_spans,
                    panel_width,
                    state.horizontal_scroll,
                    left_bg,
                );

                // Divider or revert marker.
                let show_marker =
                    show_revert_markers && !state.wrap && state.is_hunk_start_line(line_idx);
                let marker_hunk_idx = if show_marker {
                    state.hunk_index_for_start_line(line_idx)
                } else {
                    None
                };
                let (divider_char, marker_style) = if show_marker {
                    let hunk_idx = marker_hunk_idx.unwrap_or(usize::MAX);
                    let (glyph, fg) = hunk_marker_glyph_and_fg(state, hunk_idx, theme);
                    (glyph, Style::default().fg(fg).add_modifier(Modifier::BOLD))
                } else {
                    ("│", divider_style)
                };
                buf_write_str(buf, div_x, y, "  ", divider_style, divider_width);
                buf_write_str(buf, div_x, y, divider_char, marker_style, divider_width);
                if show_marker
                    && marker_hunk_idx.is_some()
                    && marker_hunk_idx == state.hovered_revert_hunk
                {
                    hover_tooltip_y = Some(y);
                }

                // Right gutter
                buf_write_str(
                    buf,
                    right_gutter_x,
                    y,
                    &right_num,
                    right_gutter_style,
                    gutter_width,
                );

                // Right content
                let right_spans = if is_delete {
                    let slash_fill: String =
                        std::iter::repeat_n('╱', panel_width as usize).collect();
                    vec![Span::styled(
                        slash_fill,
                        Style::default().fg(theme.diff_line_number).bg(right_bg),
                    )]
                } else {
                    build_content_spans(
                        diff_line.new_line.as_ref().map(|(n, t)| (*n, t.as_str())),
                        &diff_line.new_segments,
                        diff_line.change_type,
                        false,
                        new_highlighter,
                        right_bg,
                        theme,
                        panel_width as usize,
                        staged,
                    )
                };
                buf_write_spans(
                    buf,
                    right_content_x,
                    y,
                    &right_spans,
                    right_content_width,
                    state.horizontal_scroll,
                    right_bg,
                );

                row += 1;
            }
        }

        if let Some(y) = hover_tooltip_y {
            let show_key = state.hovered_revert_hunk.is_some()
                && state.hovered_revert_hunk == state.selected_revert_hunk;
            let staged = state
                .hovered_revert_hunk
                .is_some_and(|i| state.is_staged_hunk(i));
            render_revert_tooltip(
                buf,
                div_x + divider_width,
                y,
                right_content_width + gutter_width,
                theme,
                show_key,
                staged,
            );
        }
    }

    render_hover_plus_button(buf, area, state, theme);

    crate::gui::scroll::render_scrollbar(
        buf,
        area,
        state.total_scroll_rows(),
        visible_height,
        state.scroll_offset,
        true,
        border_style,
    );
    render_inline_image_regions(frame, area, inner, state, theme);
}

/// Overlay inline images onto their placeholder rows (viewport only).
///
/// Gathers contiguous visible placeholder rows per file section via
/// `line_chunk_at_row` (which already accounts for wrap, sticky headers and
/// layout), then draws each section's image into its visible region. Text and
/// sticky header rows never join a region; partially scrolled sections use
/// only the visible sub-region so following content is never overwritten.
pub(super) fn render_inline_image_regions(
    frame: &mut Frame,
    area: Rect,
    inner: Rect,
    state: &mut DiffViewState,
    theme: &Theme,
) {
    if state.image_preview_hidden || state.inline_images.is_empty() {
        return;
    }
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let layout = DiffPanelLayout::compute(area, state);
    // Group adjacent visible image rows; headers and wrapped text stay outside
    // these rectangles, so graphics cannot overwrite their source content.
    let mut runs: Vec<(usize, u16, u16)> = Vec::new();
    for y in inner.y..inner.bottom() {
        if state.is_sticky_row(y, &layout) {
            continue;
        }
        let Some((line_index, _)) = state.line_chunk_at_row(y, &layout) else {
            continue;
        };
        let Some(line) = state.lines.get(line_index) else {
            continue;
        };
        if line.file_header.is_some()
            || line.preview_placeholder.is_none()
            || !state.inline_images.contains_key(&line.section_index)
        {
            continue;
        }
        if let Some((section, _, end)) = runs.last_mut()
            && *section == line.section_index
            && *end == y
        {
            *end = y + 1;
            continue;
        }
        runs.push((line.section_index, y, y + 1));
    }
    let visible: std::collections::HashSet<_> =
        runs.iter().map(|(section, _, _)| *section).collect();
    for (section, preview) in &mut state.inline_images {
        if !visible.contains(section) {
            preview.unload();
        }
    }
    let side = state.side_view;
    for (sec, start_y, end_y) in runs {
        let h = end_y.saturating_sub(start_y);
        if h == 0 {
            continue;
        }
        let rect = Rect::new(inner.x, start_y, inner.width, h);
        if let Some(preview) = state.inline_images.get_mut(&sec) {
            preview.render(frame, rect, side, theme);
        }
    }
}
