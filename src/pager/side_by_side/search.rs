//! In-diff search highlights and the search bar.

use super::*;

/// Render search highlights over the diff buffer by scanning visible content areas
/// for occurrences of the search query and applying a highlight style.
/// `current_match_line` is the line index of the currently selected match (for emphasis).
pub fn render_diff_search_highlights(
    frame: &mut Frame,
    area: Rect,
    state: &DiffViewState,
    theme: &Theme,
) {
    if state.search_query.is_empty() || state.search_matches.is_empty() {
        return;
    }

    let pl = DiffPanelLayout::compute(area, state);
    let query_lower = state.search_query.to_lowercase();
    let query_len = query_lower.len();
    let current_match_line = state
        .search_matches
        .get(state.search_match_idx)
        .map(|m| m.line_idx);

    let buf = frame.buffer_mut();
    let buf_area = *buf.area();

    // Scan each content area (old panel, new panel) for matches
    let panel_ranges: Vec<(u16, u16)> = {
        let mut ranges = Vec::new();
        if pl.old_content_x > 0 && pl.old_content_end_x > pl.old_content_x {
            ranges.push((pl.old_content_x, pl.old_content_end_x));
        }
        if pl.new_content_x > 0 && pl.new_content_end_x > pl.new_content_x {
            ranges.push((pl.new_content_x, pl.new_content_end_x));
        }
        ranges
    };

    let visible_height = area.height.saturating_sub(2) as usize; // -2 for borders
    let highlight_style = Style::default()
        .bg(theme.diff_search_highlight_bg)
        .fg(theme.diff_search_highlight_fg);
    let current_highlight_style = Style::default()
        .bg(theme.diff_search_cursor_bg)
        .fg(theme.diff_search_cursor_fg)
        .add_modifier(Modifier::BOLD);

    for row_offset in 0..visible_height {
        let y = pl.inner_y + row_offset as u16;
        if y >= pl.inner_end_y || y >= buf_area.y + buf_area.height {
            break;
        }
        // Never highlight the pinned sticky header row.
        if state.is_sticky_row(y, &pl) {
            continue;
        }
        let line_idx = state
            .line_chunk_at_row(y, &pl)
            .map(|(line_idx, _)| line_idx)
            .unwrap_or_else(|| state.fallback_line_idx_for_row(y, &pl));

        let is_current_line = current_match_line == Some(line_idx);

        for &(range_start, range_end) in &panel_ranges {
            // Read the row text from buffer cells in this range
            let mut row_chars: Vec<(u16, char)> = Vec::new();
            for x in range_start..range_end.min(buf_area.x + buf_area.width) {
                if let Some(cell) = buf.cell((x, y)) {
                    let ch = cell.symbol().chars().next().unwrap_or(' ');
                    row_chars.push((x, ch));
                }
            }

            // Build string for searching
            let row_text: String = row_chars.iter().map(|(_, ch)| *ch).collect();
            let row_lower = row_text.to_lowercase();

            let mut start = 0;
            while let Some(pos) = row_lower[start..].find(&query_lower) {
                let match_start = start + pos;
                let match_end = match_start + query_len;
                let style = if is_current_line {
                    current_highlight_style
                } else {
                    highlight_style
                };
                for i in match_start..match_end {
                    if i < row_chars.len() {
                        let x = row_chars[i].0;
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            cell.set_style(style);
                        }
                    }
                }
                start = match_start + 1;
            }
        }
    }
}

/// Render a search bar at the bottom of the diff panel area.
pub fn render_diff_search_bar(frame: &mut Frame, area: Rect, state: &DiffViewState, theme: &Theme) {
    // Only render if search is active (typing) or has a query (dismissed but results shown)
    if !state.search_active && state.search_query.is_empty() {
        return;
    }

    // Position at the bottom row of the panel (inside the border)
    let bar_y = area.y + area.height.saturating_sub(2);
    let bar_x = area.x + 1;
    let bar_width = area.width.saturating_sub(2);

    if bar_width < 10 {
        return;
    }

    let bar_rect = Rect::new(bar_x, bar_y, bar_width, 1);

    // Clear the bar area
    let buf = frame.buffer_mut();
    for x in bar_rect.x..bar_rect.x + bar_rect.width {
        if let Some(cell) = buf.cell_mut((x, bar_y)) {
            cell.set_char(' ');
            cell.set_style(Style::default().bg(theme.diff_grid_bg));
        }
    }

    let match_info = if !state.search_matches.is_empty() {
        format!(
            " {}/{}",
            state.search_match_idx + 1,
            state.search_matches.len()
        )
    } else if !state.search_query.is_empty() {
        " (no matches)".to_string()
    } else {
        String::new()
    };

    if state.search_active {
        // Render with textarea
        let prefix_width = 2u16; // " /"
        let suffix_width = match_info.len() as u16;
        let ta_width = bar_width.saturating_sub(prefix_width + suffix_width);

        let prefix_rect = Rect::new(bar_rect.x, bar_y, prefix_width, 1);
        let prefix = Paragraph::new(Span::styled(
            " /",
            Style::default()
                .fg(theme.diff_grid_fg)
                .bg(theme.diff_grid_bg),
        ));
        frame.render_widget(prefix, prefix_rect);

        if let Some(ref ta) = state.search_textarea {
            let ta_rect = Rect::new(bar_rect.x + prefix_width, bar_y, ta_width, 1);
            frame.render_widget(ta, ta_rect);
        }

        if !match_info.is_empty() {
            let suffix_rect =
                Rect::new(bar_rect.x + prefix_width + ta_width, bar_y, suffix_width, 1);
            let suffix = Paragraph::new(Span::styled(
                match_info,
                Style::default()
                    .fg(theme.diff_grid_fg)
                    .bg(theme.diff_grid_bg),
            ));
            frame.render_widget(suffix, suffix_rect);
        }
    } else {
        // Dismissed search — show query + match info
        let text = format!(" /{}{}", state.search_query, match_info);
        let style = Style::default()
            .fg(theme.diff_grid_fg)
            .bg(theme.diff_grid_bg);
        buf_write_str(
            frame.buffer_mut(),
            bar_rect.x,
            bar_y,
            &text,
            style,
            bar_width,
        );
    }
}
