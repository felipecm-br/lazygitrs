//! List panel rendering: generic lists, commit lists, selection gutter, search highlights.

use super::*;

pub(super) fn render_submodule_list<'a>(model: &Model, theme: &Theme) -> Vec<ListItem<'a>> {
    model
        .submodules
        .iter()
        .map(|sub| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {} ", sub.name),
                    Style::default().fg(theme.accent),
                ),
                Span::styled(sub.path.clone(), Style::default().fg(theme.text_dimmed)),
            ]))
        })
        .collect()
}

pub(super) fn render_worktree_list<'a>(model: &Model, theme: &Theme) -> Vec<ListItem<'a>> {
    let names: Vec<_> = model
        .worktrees
        .iter()
        .map(|wt| {
            std::path::Path::new(&wt.path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&wt.path)
        })
        .collect();
    let name_width = names.iter().map(|name| name.width()).max().unwrap_or(0);
    model
        .worktrees
        .iter()
        .zip(names)
        .map(|(wt, name)| {
            let marker = if wt.is_current { "* " } else { "  " };
            let line = Line::from(vec![
                Span::styled(marker.to_string(), Style::default().fg(theme.accent)),
                Span::styled(name.to_string(), Style::default().fg(theme.text)),
                Span::raw(" ".repeat(name_width - name.width() + 1)),
                Span::styled(wt.branch.clone(), Style::default().fg(theme.ref_head)),
                Span::styled(
                    if wt.is_main { " (main worktree)" } else { "" },
                    Style::default().fg(theme.text),
                ),
            ]);
            ListItem::new(line)
        })
        .collect()
}

/// Render a list using persistent scroll offsets from ContextManager.
pub(super) fn render_list_ctx(
    frame: &mut Frame,
    rect: Rect,
    block: Block<'_>,
    items: Vec<ListItem<'_>>,
    selected: usize,
    is_active: bool,
    theme: &crate::config::Theme,
    ctx_mgr: &mut ContextManager,
    ctx: ContextId,
) {
    render_list_with_range_ctx(
        frame, rect, block, items, selected, is_active, theme, None, ctx_mgr, ctx,
    );
}

/// Render a list with range selection using persistent scroll offsets from ContextManager.
pub(super) fn render_list_with_range_ctx(
    frame: &mut Frame,
    rect: Rect,
    block: Block<'_>,
    items: Vec<ListItem<'_>>,
    selected: usize,
    is_active: bool,
    theme: &crate::config::Theme,
    range: Option<(usize, usize)>,
    ctx_mgr: &mut ContextManager,
    ctx: ContextId,
) {
    let mut so = ctx_mgr.scroll_offset(ctx);
    let follow = !ctx_mgr.viewport_manually_scrolled;
    render_list_with_range_raw(
        frame, rect, block, items, selected, is_active, theme, range, &mut so, follow,
    );
    ctx_mgr.set_scroll_offset(ctx, so);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_commit_list_ctx(
    frame: &mut Frame,
    rect: Rect,
    block: Block<'_>,
    model: &Model,
    theme: &crate::config::Theme,
    cherry_picked: &[String],
    selected: usize,
    is_active: bool,
    range: Option<(usize, usize)>,
    ctx_mgr: &mut ContextManager,
    ctx: ContextId,
    cache: &mut presentation::commits::CommitListCache,
    sub_commits: bool,
    full: bool,
) {
    let total_len = if sub_commits {
        model.sub_commits.len()
    } else {
        model.commits.len()
    };
    if total_len == 0 {
        frame.render_widget(block, rect);
        return;
    }

    let inner = block.inner(rect);
    let visible_height = inner.height as usize;
    if visible_height == 0 {
        frame.render_widget(block, rect);
        return;
    }

    let mut offset = ctx_mgr.scroll_offset(ctx);
    if !ctx_mgr.viewport_manually_scrolled {
        crate::gui::scroll::ensure_visible(selected, &mut offset, visible_height);
    }
    offset = offset.min(total_len.saturating_sub(visible_height));
    ctx_mgr.set_scroll_offset(ctx, offset);

    let items = if sub_commits {
        presentation::commits::render_sub_commit_list_window(
            model,
            theme,
            offset,
            visible_height,
            full,
            cache,
        )
    } else {
        presentation::commits::render_commit_list_window(
            model,
            theme,
            cherry_picked,
            offset,
            visible_height,
            full,
            cache,
        )
    };
    let range_style = Style::default()
        .bg(theme.selected_bg)
        .add_modifier(theme.selected_line.add_modifier);
    let visible_items: Vec<ListItem> = items
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            let idx = offset + i;
            if is_active && idx == selected {
                item.style(theme.selected_line)
            } else if is_active && range.is_some_and(|(lo, hi)| idx >= lo && idx <= hi) {
                item.style(range_style)
            } else {
                item
            }
        })
        .collect();
    let border_style = if is_active {
        theme.active_border
    } else {
        theme.inactive_border
    };
    frame.render_widget(List::new(visible_items).block(block), rect);
    if let Some(gutter_color) = theme.selection_gutter {
        let selected_row = if is_active && selected >= offset && selected < offset + visible_height
        {
            Some(inner.y + (selected - offset) as u16)
        } else {
            None
        };
        paint_selection_gutter(frame.buffer_mut(), inner, selected_row, gutter_color);
    }
    crate::gui::scroll::render_scrollbar(
        frame.buffer_mut(),
        rect,
        total_len,
        visible_height,
        offset,
        false,
        border_style,
    );
}

/// Paint a 1-cell selection gutter at `inner.x` (`X=1` inside a bordered list panel)
/// for zero-background themes like `cockpit`.
/// If any visible row in `inner` starts with a non-space character at `inner.x`,
/// shifts all rows in `inner` right by 1 column first so the gutter never overwrites text.
pub(super) fn paint_selection_gutter(
    buf: &mut ratatui::buffer::Buffer,
    inner: Rect,
    selected_row: Option<u16>,
    gutter_color: ratatui::style::Color,
) {
    if inner.width < 2 || inner.height == 0 {
        return;
    }
    let buf_area = *buf.area();
    let max_x = (inner.x + inner.width).min(buf_area.x + buf_area.width);
    let max_y = (inner.y + inner.height).min(buf_area.y + buf_area.height);
    if inner.x >= max_x || inner.y >= max_y {
        return;
    }

    let needs_shift = (inner.y..max_y).any(|y| {
        buf.cell((inner.x, y))
            .is_some_and(|c| c.symbol() != " " && !c.symbol().is_empty())
    });

    if needs_shift {
        for y in inner.y..max_y {
            for x in (inner.x + 1..max_x).rev() {
                if let Some(prev) = buf.cell((x - 1, y)).cloned()
                    && let Some(cur) = buf.cell_mut((x, y))
                {
                    *cur = prev;
                }
            }
            if let Some(first) = buf.cell_mut((inner.x, y)) {
                first.set_symbol(" ");
            }
        }
    }

    if let Some(sel_y) = selected_row
        && sel_y >= inner.y
        && sel_y < max_y
        && let Some(cell) = buf.cell_mut((inner.x, sel_y))
    {
        cell.set_symbol("│");
        cell.set_style(
            Style::default()
                .fg(gutter_color)
                .add_modifier(ratatui::style::Modifier::BOLD),
        );
    }
}

/// Highlight contiguous `/` search matches in a list panel (lazygit-style).
/// Scans already-painted buffer cells so item styling (selection, colors) is preserved.
pub(super) fn render_list_search_highlights(
    frame: &mut Frame,
    area: Rect,
    query: &str,
    theme: &crate::config::Theme,
) {
    if query.is_empty() || area.width < 3 || area.height < 3 {
        return;
    }

    let query_lower: Vec<char> = query.to_lowercase().chars().collect();
    if query_lower.is_empty() {
        return;
    }

    let highlight = theme.search_match;
    let buf = frame.buffer_mut();
    let buf_area = *buf.area();

    let inner_x = area.x + 1;
    let inner_end_x = area.x + area.width.saturating_sub(1);
    let inner_y = area.y + 1;
    let inner_end_y = area.y + area.height.saturating_sub(1);

    for y in inner_y..inner_end_y {
        if y >= buf_area.y + buf_area.height {
            break;
        }

        let mut row_chars: Vec<(u16, char)> = Vec::new();
        for x in inner_x..inner_end_x.min(buf_area.x + buf_area.width) {
            if let Some(cell) = buf.cell((x, y)) {
                let ch = cell.symbol().chars().next().unwrap_or(' ');
                row_chars.push((x, ch));
            }
        }
        if row_chars.is_empty() {
            continue;
        }

        let row_text: String = row_chars.iter().map(|(_, c)| *c).collect();
        let row_lower = row_text.to_lowercase();
        let row_lower_chars: Vec<char> = row_lower.chars().collect();

        let mut start = 0usize;
        while start + query_lower.len() <= row_lower_chars.len() {
            let is_match = row_lower_chars[start..start + query_lower.len()]
                .iter()
                .zip(query_lower.iter())
                .all(|(a, b)| a == b);
            if is_match {
                for i in 0..query_lower.len() {
                    let (x, _) = row_chars[start + i];
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_style(highlight);
                    }
                }
                start += query_lower.len();
            } else {
                start += 1;
            }
        }
    }
}

pub(super) fn render_list_with_range_raw(
    frame: &mut Frame,
    rect: Rect,
    block: Block<'_>,
    items: Vec<ListItem<'_>>,
    selected: usize,
    is_active: bool,
    theme: &crate::config::Theme,
    range: Option<(usize, usize)>,
    scroll_offset: &mut usize,
    follow_selection: bool,
) {
    if items.is_empty() {
        frame.render_widget(block, rect);
        return;
    }

    let inner = block.inner(rect);
    let visible_height = inner.height as usize;

    // Ensure selected item is visible, only adjusting scroll when necessary.
    // Skip when viewport was manually scrolled (mouse scroll) to avoid snapping back.
    if visible_height == 0 {
        frame.render_widget(block, rect);
        return;
    }
    if follow_selection {
        crate::gui::scroll::ensure_visible(selected, scroll_offset, visible_height);
    }
    // Clamp scroll offset to valid range
    let max_offset = items.len().saturating_sub(visible_height);
    if *scroll_offset > max_offset {
        *scroll_offset = max_offset;
    }
    let offset = *scroll_offset;
    let total_len = items.len();

    let range_style = Style::default()
        .bg(theme.selected_bg)
        .add_modifier(theme.selected_line.add_modifier);
    let visible_items: Vec<ListItem> = items
        .into_iter()
        .skip(offset)
        .take(visible_height)
        .enumerate()
        .map(|(i, item)| {
            let idx = i + offset;
            if is_active && idx == selected {
                item.style(theme.selected_line)
            } else if is_active && range.is_some_and(|(lo, hi)| idx >= lo && idx <= hi) {
                item.style(range_style)
            } else {
                item
            }
        })
        .collect();
    let border_style = if is_active {
        theme.active_border
    } else {
        theme.inactive_border
    };
    let list = List::new(visible_items).block(block);
    frame.render_widget(list, rect);
    if let Some(gutter_color) = theme.selection_gutter {
        let selected_row = if is_active && selected >= offset && selected < offset + visible_height
        {
            Some(inner.y + (selected - offset) as u16)
        } else {
            None
        };
        paint_selection_gutter(frame.buffer_mut(), inner, selected_row, gutter_color);
    }
    crate::gui::scroll::render_scrollbar(
        frame.buffer_mut(),
        rect,
        total_len,
        visible_height,
        offset,
        false,
        border_style,
    );
}
