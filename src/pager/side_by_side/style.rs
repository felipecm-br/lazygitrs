//! Diff line/gutter colours and content span building (including word diff).

use super::*;

/// Get background colors for a diff line based on change type.
/// Unstaged hunks recede toward the panel tone so staged hunks (full
/// `diff_add_bg`/`diff_remove_bg`) pop. `Reset` passes through undimmed.
pub(super) fn line_bg_colors(
    change_type: ChangeType,
    theme: &Theme,
    staged: bool,
) -> (Color, Color) {
    let (l, r) = match change_type {
        ChangeType::Equal => (Color::Reset, Color::Reset),
        ChangeType::Delete => (theme.diff_remove_bg, Color::Reset),
        ChangeType::Insert => (Color::Reset, theme.diff_add_bg),
        ChangeType::Modified => (theme.diff_remove_bg, theme.diff_add_bg),
    };
    (
        dim_unstaged_bg(l, staged, theme),
        dim_unstaged_bg(r, staged, theme),
    )
}

/// Dim one background for an unstaged hunk. Staged rows and `Reset`
/// (context) rows pass through untouched.
pub(super) fn dim_unstaged_bg(bg: Color, staged: bool, theme: &Theme) -> Color {
    if staged || bg == Color::Reset {
        return bg;
    }
    crate::config::theme::mix_colors(bg, theme.diff_grid_bg, 150)
}

/// Get gutter background colors for a diff line. Slightly darker than the
/// content background so the gutter visually separates from the code area
/// (lumen-style). Dims with the row for unstaged hunks.
pub(super) fn gutter_bg_colors(
    change_type: ChangeType,
    theme: &Theme,
    staged: bool,
) -> (Color, Color) {
    let (l, r) = match change_type {
        ChangeType::Equal => (Color::Reset, Color::Reset),
        ChangeType::Delete => (theme.diff_remove_gutter_bg, Color::Reset),
        ChangeType::Insert => (Color::Reset, theme.diff_add_gutter_bg),
        ChangeType::Modified => (theme.diff_remove_gutter_bg, theme.diff_add_gutter_bg),
    };
    (
        dim_unstaged_bg(l, staged, theme),
        dim_unstaged_bg(r, staged, theme),
    )
}

/// Foreground color for the gutter line number on the (left, right) side.
pub(super) fn gutter_fg_colors(change_type: ChangeType, theme: &Theme) -> (Color, Color) {
    match change_type {
        ChangeType::Equal => (theme.diff_gutter, theme.diff_gutter),
        ChangeType::Delete => (theme.diff_remove_gutter_fg, theme.diff_gutter),
        ChangeType::Insert => (theme.diff_gutter, theme.diff_add_gutter_fg),
        ChangeType::Modified => (theme.diff_remove_gutter_fg, theme.diff_add_gutter_fg),
    }
}

/// Build styled spans for one side of a diff line.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_content_spans<'a>(
    line_data: Option<(usize, &str)>,
    segments: &Option<Vec<InlineSegment>>,
    change_type: ChangeType,
    is_old_side: bool,
    highlighter: &FileHighlighter,
    bg: Color,
    theme: &Theme,
    max_width: usize,
    staged: bool,
) -> Vec<Span<'a>> {
    let Some((line_num, text)) = line_data else {
        // Empty side — fill with background
        let fill = " ".repeat(max_width);
        return vec![Span::styled(fill, Style::default().bg(bg))];
    };

    // If we have word-level segments, use those
    if let Some(segs) = segments {
        return build_word_diff_spans(segs, is_old_side, bg, theme, max_width, staged);
    }

    // Otherwise, try syntax highlighting
    let highlighted = highlighter.get_line_spans(line_num, Some(bg), theme);
    if !highlighted.is_empty() {
        return highlighted;
    }

    // Fallback: plain text with background
    let fg = match change_type {
        ChangeType::Delete => theme.diff_remove.fg.unwrap_or(Color::Red),
        ChangeType::Insert => theme.diff_add.fg.unwrap_or(Color::Green),
        _ => theme.syntax_default,
    };

    let display = if text.len() > max_width {
        // Find a safe byte boundary to avoid slicing mid-character
        let mut end = max_width;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    } else {
        text
    };

    vec![Span::styled(
        display.to_string(),
        Style::default().fg(fg).bg(bg),
    )]
}

/// Build spans with word-level diff emphasis.
pub(super) fn build_word_diff_spans<'a>(
    segments: &[InlineSegment],
    is_old_side: bool,
    bg: Color,
    theme: &Theme,
    _max_width: usize,
    staged: bool,
) -> Vec<Span<'a>> {
    segments
        .iter()
        .map(|seg| {
            if seg.emphasized {
                let emphasis_bg = if is_old_side {
                    theme.diff_remove_word
                } else {
                    theme.diff_add_word
                };
                Span::styled(
                    seg.text.clone(),
                    Style::default()
                        .bg(dim_unstaged_bg(emphasis_bg, staged, theme))
                        .fg(theme.text_strong)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    seg.text.clone(),
                    Style::default().bg(bg).fg(theme.syntax_default),
                )
            }
        })
        .collect()
}
