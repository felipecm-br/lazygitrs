//! Status panel: compact sidebar line and the main repo-status view.

use super::*;

/// Compact 1-line status for the sidebar: "reponame → branch          +N -N"
pub(super) fn render_status_sidebar<'a>(
    model: &Model,
    _config: &AppConfig,
    inner_width: usize,
    theme: &Theme,
) -> Line<'a> {
    // Determine the working-tree state prefix (rebasing/merging/cherry-picking)
    let state_prefix = if model.is_rebasing {
        Some("rebasing")
    } else if model.is_merging {
        Some("merging")
    } else if model.is_cherry_picking {
        Some("cherry-picking")
    } else {
        None
    };

    let head_branch = model.branches.iter().find(|b| b.head);
    let branch_name = head_branch.map(|b| b.name.clone()).unwrap_or_else(|| {
        if model.head_branch_name.is_empty() {
            "HEAD (no branch)".to_string()
        } else {
            model.head_branch_name.clone()
        }
    });
    let ahead_behind = head_branch.and_then(|b| b.ahead_behind());

    let repo_name = model.repo_name.clone();

    // Build the right-side stats string to measure its width
    let additions = model.total_additions;
    let deletions = model.total_deletions;
    let has_changes = additions > 0 || deletions > 0;

    let stats_text = if has_changes {
        let mut s = String::new();
        if additions > 0 {
            s.push_str(&format!("+{}", additions));
        }
        if additions > 0 && deletions > 0 {
            s.push(' ');
        }
        if deletions > 0 {
            s.push_str(&format!("-{}", deletions));
        }
        s
    } else {
        String::new()
    };

    // Build ahead/behind prefix
    let ab_text = match ahead_behind {
        Some((ahead, behind)) if ahead > 0 && behind > 0 => format!("↑{}↓{} ", ahead, behind),
        Some((ahead, _)) if ahead > 0 => format!("↑{} ", ahead),
        Some((_, behind)) if behind > 0 => format!("↓{} ", behind),
        _ => String::new(),
    };

    let mut spans = Vec::new();

    // When in a special state (rebasing, merging, etc.), show lazygit-style:
    //   (rebasing) reponame → <hash>
    if let Some(state) = state_prefix {
        let right_side = if model.is_rebasing && !model.rebase_onto_hash.is_empty() {
            model.rebase_onto_hash.clone()
        } else {
            branch_name.clone()
        };

        let prefix = format!("({})", state);
        let left_len = 1
            + prefix.len()
            + 1
            + repo_name.len()
            + 1
            + UnicodeWidthStr::width("→ ")
            + right_side.len();
        let right_len = if has_changes { stats_text.len() + 1 } else { 0 };
        let padding = if has_changes {
            inner_width.saturating_sub(left_len + right_len).max(1)
        } else {
            inner_width.saturating_sub(left_len + right_len)
        };

        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            prefix,
            Style::default().fg(theme.accent_secondary),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("{} ", repo_name),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("→ ", Style::default().fg(theme.text_dimmed)));
        spans.push(Span::styled(
            right_side,
            Style::default().fg(theme.accent_secondary),
        ));

        if has_changes {
            spans.push(Span::raw(" ".repeat(padding)));
            if additions > 0 {
                spans.push(Span::styled(
                    format!("+{}", additions),
                    Style::default().fg(theme.file_staged.fg.unwrap_or(theme.accent)),
                ));
            }
            if additions > 0 && deletions > 0 {
                spans.push(Span::raw(" "));
            }
            if deletions > 0 {
                spans.push(Span::styled(
                    format!("-{}", deletions),
                    Style::default().fg(theme.file_unstaged.fg.unwrap_or(theme.change_deleted)),
                ));
            }
            spans.push(Span::raw(" "));
        }

        return Line::from(spans);
    }

    // Normal state: " ↑N reponame → branch"
    let left_len = 1
        + UnicodeWidthStr::width(ab_text.as_str())
        + repo_name.len()
        + 1
        + UnicodeWidthStr::width("→ ")
        + branch_name.len();
    let right_len = if has_changes { stats_text.len() + 1 } else { 0 };
    let padding = if has_changes {
        inner_width.saturating_sub(left_len + right_len).max(1)
    } else {
        inner_width.saturating_sub(left_len + right_len)
    };

    if !ab_text.is_empty() {
        spans.push(Span::styled(
            format!(" {}", ab_text),
            Style::default().fg(theme.file_staged.fg.unwrap_or(theme.accent)),
        ));
    } else {
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        format!("{} ", repo_name),
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled("→ ", Style::default().fg(theme.text_dimmed)));
    spans.push(Span::styled(
        branch_name,
        Style::default().fg(theme.branch_local.fg.unwrap_or(theme.accent)),
    ));

    if has_changes {
        spans.push(Span::raw(" ".repeat(padding)));
        if additions > 0 {
            spans.push(Span::styled(
                format!("+{}", additions),
                Style::default().fg(theme.file_staged.fg.unwrap_or(theme.accent)),
            ));
        }
        if additions > 0 && deletions > 0 {
            spans.push(Span::raw(" "));
        }
        if deletions > 0 {
            spans.push(Span::styled(
                format!("-{}", deletions),
                Style::default().fg(theme.file_unstaged.fg.unwrap_or(theme.change_deleted)),
            ));
        }
        spans.push(Span::raw(" "));
    }

    Line::from(spans)
}

/// Full status view for the main content area: logo + copyright + repo info
pub(super) fn render_status_main<'a>(
    frame: &mut Frame,
    rect: Rect,
    model: &Model,
    _config: &AppConfig,
    theme: &crate::config::Theme,
    block: Block<'a>,
) {
    let branch_name = model
        .branches
        .iter()
        .find(|b| b.head)
        .map(|b| b.name.as_str())
        .unwrap_or_else(|| {
            if model.head_branch_name.is_empty() {
                "HEAD (no branch)"
            } else {
                model.head_branch_name.as_str()
            }
        });

    let logo = include_str!("../../../logo.txt");
    let mut lines: Vec<Line> = logo
        .lines()
        .map(|l| {
            Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(theme.accent),
            ))
        })
        .collect();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Copyright 2026 Carlo Taleon (Blankeos)",
        Style::default().fg(theme.text_dimmed),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(format!(" Branch: {}", branch_name)));
    if !model.repo_url.is_empty() {
        lines.push(Line::from(format!(" Repo:   {}", model.repo_url)));
    }
    lines.push(Line::from(format!(" Commits: {}", model.commits.len())));
    lines.push(Line::from(format!(" Files: {}", model.files.len())));
    lines.push(Line::from(format!(
        " Version: v{}",
        env!("CARGO_PKG_VERSION")
    )));

    if !model.contributors.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Contributors",
            Style::default().fg(theme.text_dimmed),
        )));
        for (name, count) in model.contributors.iter().take(10) {
            lines.push(Line::from(format!("   {:>4}  {}", count, name)));
        }
    }

    // In-progress operation banners
    if model.is_rebasing {
        lines.push(Line::from(Span::styled(
            " REBASING",
            Style::default().fg(theme.accent_secondary),
        )));
    }
    if model.is_merging {
        lines.push(Line::from(Span::styled(
            " MERGING",
            Style::default().fg(theme.accent_secondary),
        )));
    }
    if model.is_cherry_picking {
        lines.push(Line::from(Span::styled(
            " CHERRY-PICKING",
            Style::default().fg(theme.accent_secondary),
        )));
    }
    if model.is_bisecting {
        lines.push(Line::from(Span::styled(
            " BISECTING",
            Style::default().fg(theme.accent_secondary),
        )));
    }

    let widget = Paragraph::new(lines).block(block);
    frame.render_widget(widget, rect);
}
