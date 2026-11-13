//! Smart Commit review popup: plan list, selected group details (or the
//! inline message editor) and a scrollable diff preview of that group.

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::Borders;

use super::*;
use crate::git::smart_commit::ChangeKind;
use crate::gui::smart_commit::SmartCommitState;

fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        if out.width() + 1 >= width {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

pub fn render_smart_commit_plan(
    frame: &mut Frame,
    state: &SmartCommitState,
    area: Rect,
    theme: &Theme,
) {
    let width = (area.width * 90 / 100)
        .clamp(40.min(area.width), 150)
        .min(area.width);
    let height = (area.height * 88 / 100)
        .max(12.min(area.height))
        .min(area.height);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);

    let plan = &state.plan;
    let mut title = format!(
        " Smart Commit · {}/{} selected · {} ",
        state.checked_count(),
        plan.groups.len(),
        state.options.granularity.as_str()
    );
    if plan.from_cache {
        title.push_str("· cached ");
    }
    let block = Block::default()
        .title(title)
        .borders(theme.borders_for("popup"))
        .border_type(theme.border_type_for("popup"))
        .border_style(Style::default().fg(theme.accent_secondary));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if inner.height < 6 || inner.width < 20 {
        return;
    }

    let list_rows = (plan.groups.len() as u16).clamp(1, (inner.height / 3).max(1));
    let detail_rows = if state.editor.is_some() {
        (inner.height / 3).max(5)
    } else {
        let g = plan.groups.get(state.selected);
        let body = g.map_or(0, |g| g.body.lines().count());
        let files = g.map_or(0, |g| g.files.len());
        ((1 + body + files + usize::from(!plan.warnings.is_empty())) as u16)
            .min(inner.height / 3)
            .max(2)
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(list_rows),
            Constraint::Length(1),
            Constraint::Length(detail_rows),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    render_list(frame, state, chunks[0], theme);
    render_rule(frame, chunks[1], theme);
    if let Some(editor) = &state.editor {
        let mut editor = editor.clone();
        editor.set_block(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(theme.accent)),
        );
        frame.render_widget(&editor, chunks[2]);
    } else {
        render_details(frame, state, chunks[2], theme);
    }
    render_rule(frame, chunks[3], theme);
    render_preview(frame, state, chunks[4], theme);
    render_footer(frame, state, chunks[5], theme);
}

fn render_rule(frame: &mut Frame, area: Rect, theme: &Theme) {
    let line = theme.separator_char.repeat(area.width as usize);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().fg(theme.separator)),
        area,
    );
}

fn render_list(frame: &mut Frame, state: &SmartCommitState, area: Rect, theme: &Theme) {
    let rows = area.height as usize;
    let offset = state.selected.saturating_sub(rows.saturating_sub(1));
    let width = area.width as usize;
    let lines: Vec<Line> = state
        .plan
        .groups
        .iter()
        .enumerate()
        .skip(offset)
        .take(rows)
        .map(|(i, group)| {
            let selected = i == state.selected;
            let checked = state.checked.get(i).copied().unwrap_or(false);
            let (added, deleted) = group.files.iter().fold((0, 0), |(a, d), f| {
                state
                    .plan
                    .files
                    .get(f)
                    .map_or((a, d), |m| (a + m.added, d + m.deleted))
            });
            let stats = format!(
                " {} file{} +{added} -{deleted}",
                group.files.len(),
                if group.files.len() == 1 { "" } else { "s" }
            );
            let mark = if checked { "[x] " } else { "[ ] " };
            let prefix = format!("{}{mark}{:>2}. ", if selected { "▸ " } else { "  " }, i + 1);
            let title_width = width.saturating_sub(prefix.width() + stats.width());
            let title = truncate(&group.title, title_width);
            let pad = width.saturating_sub(prefix.width() + title.width() + stats.width());
            let title_style = if !checked {
                Style::default().fg(theme.text_dimmed)
            } else if group.fallback {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(theme.text_strong)
            };
            let mut line = Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(if checked {
                        theme.accent
                    } else {
                        theme.text_dimmed
                    }),
                ),
                Span::styled(title, title_style),
                Span::raw(" ".repeat(pad)),
                Span::styled(stats, Style::default().fg(theme.text_dimmed)),
            ]);
            if selected {
                line = line.style(Style::default().bg(theme.selected_bg));
            }
            line
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_details(frame: &mut Frame, state: &SmartCommitState, area: Rect, theme: &Theme) {
    let Some(group) = state.plan.groups.get(state.selected) else {
        return;
    };
    let width = area.width as usize;
    let mut lines = vec![Line::from(Span::styled(
        truncate(&group.title, width),
        Style::default()
            .fg(theme.text_strong)
            .add_modifier(Modifier::BOLD),
    ))];
    for body in group.body.lines() {
        lines.push(Line::from(Span::styled(
            truncate(body, width),
            Style::default().fg(theme.text),
        )));
    }
    if group.fallback {
        lines.push(Line::from(Span::styled(
            "AI left these files unassigned: edit the message (e) or merge (m)",
            Style::default().fg(Color::Yellow),
        )));
    }
    for path in &group.files {
        let meta = state.plan.files.get(path);
        let kind = meta.map(|m| &m.kind);
        let style = match kind {
            Some(ChangeKind::Untracked) | Some(ChangeKind::Added) => theme.file_untracked,
            Some(ChangeKind::Deleted) => theme.diff_remove,
            _ => theme.file_unstaged,
        };
        let mut spans = vec![
            Span::styled(
                format!("  {} ", kind.map_or(' ', ChangeKind::letter)),
                style,
            ),
            Span::styled(path.clone(), Style::default().fg(theme.text)),
        ];
        if let Some(m) = meta {
            if let Some(old) = &m.old_path {
                spans.push(Span::styled(
                    format!(" ← {old}"),
                    Style::default().fg(theme.text_dimmed),
                ));
            }
            spans.push(Span::styled(format!("  +{}", m.added), theme.diff_add));
            spans.push(Span::styled(format!(" -{}", m.deleted), theme.diff_remove));
            if m.content_ignored {
                spans.push(Span::styled(
                    "  content hidden from AI",
                    Style::default().fg(theme.text_dimmed),
                ));
            }
        }
        lines.push(Line::from(spans));
    }
    if let Some(warning) = state.plan.warnings.first() {
        let more = state.plan.warnings.len() - 1;
        let text = if more > 0 {
            format!("AI note: {warning} (+{more} more)")
        } else {
            format!("AI note: {warning}")
        };
        lines.push(Line::from(Span::styled(
            truncate(&text, width),
            Style::default().fg(theme.text_dimmed),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_preview(frame: &mut Frame, state: &SmartCommitState, area: Rect, theme: &Theme) {
    let Some((_, preview)) = &state.preview else {
        return;
    };
    let rows = area.height as usize;
    let max_scroll = preview.len().saturating_sub(rows);
    let scroll = state.preview_scroll.min(max_scroll);
    let width = area.width as usize;
    let lines: Vec<Line> = preview
        .iter()
        .skip(scroll)
        .take(rows)
        .map(|raw| {
            let text = truncate(&raw.replace('\t', "    "), width);
            let style = if raw.starts_with("diff --git") {
                Style::default()
                    .fg(theme.text_strong)
                    .add_modifier(Modifier::BOLD)
            } else if raw.starts_with("+++") || raw.starts_with("---") || raw.starts_with("index ")
            {
                Style::default().fg(theme.text_dimmed)
            } else if raw.starts_with("@@") {
                Style::default().fg(theme.accent)
            } else if raw.starts_with('+') {
                theme.diff_add
            } else if raw.starts_with('-') {
                theme.diff_remove
            } else {
                theme.diff_context
            };
            Line::from(Span::styled(text, style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_footer(frame: &mut Frame, state: &SmartCommitState, area: Rect, theme: &Theme) {
    let hints: &[(&str, &str)] = if state.editor.is_some() {
        &[("ctrl-s", "save"), ("enter", "newline"), ("esc", "cancel")]
    } else {
        &[
            ("space", "toggle"),
            ("a", "all"),
            ("e", "edit"),
            ("J/K", "move"),
            ("m", "merge ↓"),
            ("g", "granularity"),
            ("r", "regenerate"),
            ("pgup/dn", "scroll"),
            ("enter", "commit"),
            ("esc", "close"),
        ]
    };
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (key, label) in hints {
        let w = key.width() + label.width() + 3;
        if used + w > area.width as usize {
            break;
        }
        used += w;
        spans.push(Span::styled(*key, Style::default().fg(theme.accent)));
        spans.push(Span::styled(
            format!(" {label}  "),
            Style::default().fg(theme.text_dimmed),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
