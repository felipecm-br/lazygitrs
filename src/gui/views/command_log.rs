//! Command log panel rendering and geometry.

use super::*;

pub(super) fn render_command_log(
    frame: &mut Frame,
    fl: &layout::FrameLayout,
    command_log: &[String],
    show_command_log: bool,
    theme: &Theme,
) {
    if !show_command_log || command_log.is_empty() {
        return;
    }

    let Some((log_rect, log_height)) = command_log_geometry(fl.main_panel, command_log.len())
    else {
        return;
    };

    let border_color = theme.cmd_log_border;
    let title_color = theme.cmd_log_title;
    let hint_color = theme.cmd_log_hint;
    let log_block = Block::default()
        .title(Line::from(vec![
            Span::styled(" ", Style::default().fg(title_color)),
            Span::styled(
                "Command Log",
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().fg(title_color)),
        ]))
        .title_bottom(
            Line::from(vec![
                Span::styled(" ", Style::default().fg(hint_color)),
                Span::styled(
                    ";",
                    Style::default()
                        .fg(theme.cmd_log_timestamp)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" toggle ", Style::default().fg(hint_color)),
            ])
            .alignment(ratatui::layout::Alignment::Right),
        )
        .borders(theme.borders_for("cmd_log"))
        .border_type(theme.border_type_for("cmd_log"))
        .border_style(Style::default().fg(border_color));

    let cmd_color = theme.cmd_log_text;
    let visible_count = command_log.len().min(log_height as usize);
    let log_lines: Vec<Line> = command_log
        .iter()
        .rev()
        .take(log_height as usize)
        .rev()
        .enumerate()
        .map(|(i, s)| {
            let is_latest = i == visible_count - 1;
            let fg = if is_latest {
                theme.cmd_log_timestamp
            } else {
                cmd_color
            };
            Line::from(vec![
                Span::styled(" $ ", Style::default().fg(theme.cmd_log_success)),
                Span::styled(s.to_string(), Style::default().fg(fg)),
            ])
        })
        .collect();

    frame.render_widget(Clear, log_rect);
    let log_widget = Paragraph::new(log_lines).block(log_block);
    frame.render_widget(log_widget, log_rect);
}

pub(super) fn command_log_geometry(main_panel: Rect, command_count: usize) -> Option<(Rect, u16)> {
    if main_panel.width == 0 {
        return None;
    }

    let log_height = command_count
        .min(5)
        .min(main_panel.height.saturating_sub(1) as usize) as u16;
    if log_height == 0 {
        return None;
    }

    let log_width = main_panel.width.min(50);
    let log_x = main_panel
        .x
        .saturating_add(main_panel.width.saturating_sub(log_width));
    let log_y = main_panel
        .y
        .saturating_add(main_panel.height - log_height - 1);
    Some((
        Rect::new(log_x, log_y, log_width, log_height + 2),
        log_height,
    ))
}
