//! Window title builders for list panels.

use super::*;

/// Build a window title like " 4 Commit Files (abc1234 feat: some change) ".
pub(super) fn build_branch_commits_title<'a>(branch_name: &str, theme: &Theme) -> Line<'a> {
    Line::from(vec![
        Span::raw(" 3 Commits "),
        Span::styled(
            format!("({})", branch_name),
            Style::default().fg(theme.accent_secondary),
        ),
        Span::raw(" "),
    ])
}

pub(super) fn build_commit_files_title<'a>(
    ctx: ContextId,
    commit_hash: &str,
    commit_message: &str,
    theme: &Theme,
) -> Line<'a> {
    let short = if commit_hash.len() > 7 {
        &commit_hash[..7]
    } else {
        commit_hash
    };
    let prefix = match ctx {
        ContextId::StashFiles => " 5 Stash Files ",
        ContextId::BranchCommitFiles => " 3 Commit Files ",
        _ => " 4 Commit Files ",
    };
    let mut spans = vec![
        Span::raw(prefix),
        Span::styled(
            format!("({}", short),
            Style::default().fg(theme.accent_secondary),
        ),
    ];
    if !commit_message.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            commit_message.to_string(),
            Style::default().fg(theme.text_dimmed),
        ));
    }
    spans.push(Span::styled(
        ") ",
        Style::default().fg(theme.accent_secondary),
    ));
    Line::from(spans)
}

pub(super) fn build_window_title<'a>(
    window: SideWindow,
    active_ctx: ContextId,
    _ctx_mgr: &ContextManager,
    theme: &Theme,
    all_branches_log: bool,
) -> Line<'a> {
    let tabs = window.tabs();
    let key = window.key_label();

    if tabs.len() == 1 {
        let mut title_str = tabs[0].title().to_string();
        if tabs[0] == ContextId::Commits {
            if all_branches_log {
                title_str.push_str(" (all)");
            } else {
                title_str.push_str(" (HEAD)");
            }
        }
        return Line::from(format!(" {} {} ", key, title_str));
    }

    let mut spans = vec![Span::raw(format!(" {} ", key))];

    for (i, ctx) in tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" | ", Style::default().fg(theme.text_dimmed)));
        }
        let mut title_str = ctx.title().to_string();
        if *ctx == ContextId::Commits {
            if all_branches_log {
                title_str.push_str(" (all)");
            } else {
                title_str.push_str(" (HEAD)");
            }
        }
        if *ctx == active_ctx {
            spans.push(Span::styled(
                title_str,
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                title_str,
                Style::default().fg(theme.text_dimmed),
            ));
        }
    }

    spans.push(Span::raw(" "));
    Line::from(spans)
}
