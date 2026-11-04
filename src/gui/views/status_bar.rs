//! Bottom status bar / search bar and key-hint formatting.

use super::*;

pub(super) fn format_key_hint(key: &str) -> String {
    let key = key.trim();
    let inner = if let Some(stripped) = key.strip_prefix('<').and_then(|k| k.strip_suffix('>')) {
        stripped.trim()
    } else {
        key
    };
    let inner_lower = inner.to_ascii_lowercase();
    if let Some(rest) = inner_lower
        .strip_prefix("c-")
        .or_else(|| inner_lower.strip_prefix("c+"))
        .or_else(|| inner_lower.strip_prefix("ctrl-"))
        .or_else(|| inner_lower.strip_prefix("ctrl+"))
    {
        format!("ctrl+{}", rest)
    } else if let Some(rest) = inner_lower
        .strip_prefix("a-")
        .or_else(|| inner_lower.strip_prefix("a+"))
        .or_else(|| inner_lower.strip_prefix("alt-"))
        .or_else(|| inner_lower.strip_prefix("alt+"))
    {
        format!("alt+{}", rest)
    } else if let Some(rest) = inner_lower
        .strip_prefix("s-")
        .or_else(|| inner_lower.strip_prefix("s+"))
        .or_else(|| inner_lower.strip_prefix("shift-"))
        .or_else(|| inner_lower.strip_prefix("shift+"))
    {
        format!("shift+{}", rest)
    } else if inner_lower == "enter" || inner_lower == "return" {
        "enter".to_string()
    } else if inner_lower == "escape" || inner_lower == "esc" {
        "esc".to_string()
    } else if inner_lower == "space" {
        "space".to_string()
    } else if inner_lower == "backtab" || inner_lower == "shift-tab" || inner_lower == "shift+tab" {
        "shift+tab".to_string()
    } else {
        key.to_string()
    }
}

pub(super) fn render_search_bar_or_status_bar(
    frame: &mut Frame,
    status_bar: Rect,
    search_state: Option<(&str, usize, usize)>,
    search_textarea: Option<&ratatui_textarea::TextArea<'_>>,
    ctx_mgr: &ContextManager,
    diff_view: &DiffViewState,
    theme: &Theme,
    model: &Model,
    diff_focused: bool,
    has_copied_commits: bool,
    keybindings: &KeybindingConfig,
    has_active_filters: bool,
) {
    if let Some((query, match_count, current_match)) = search_state {
        let match_info = if match_count > 0 {
            format!(" {}/{}", current_match + 1, match_count)
        } else if !query.is_empty() {
            " (no matches)".to_string()
        } else {
            String::new()
        };

        if let Some(ta) = search_textarea {
            // Render: "/" prefix + textarea + match info
            let prefix_width = 2u16; // " /"
            let suffix_text = match_info;
            let suffix_width = suffix_text.len() as u16;
            let ta_width = status_bar.width.saturating_sub(prefix_width + suffix_width);

            let prefix_rect = Rect::new(status_bar.x, status_bar.y, prefix_width, 1);
            let prefix = Paragraph::new(Span::styled(
                " /",
                Style::default().fg(theme.accent_secondary),
            ));
            frame.render_widget(prefix, prefix_rect);

            let ta_rect = Rect::new(status_bar.x + prefix_width, status_bar.y, ta_width, 1);
            frame.render_widget(ta, ta_rect);

            if !suffix_text.is_empty() {
                let suffix_rect = Rect::new(
                    status_bar.x + prefix_width + ta_width,
                    status_bar.y,
                    suffix_width,
                    1,
                );
                let suffix = Paragraph::new(Span::styled(
                    suffix_text,
                    Style::default().fg(theme.accent_secondary),
                ));
                frame.render_widget(suffix, suffix_rect);
            }
        } else {
            let bar = Paragraph::new(Span::styled(
                format!(" /{}{}", query, match_info),
                Style::default().fg(theme.accent_secondary),
            ));
            frame.render_widget(bar, status_bar);
        }
    } else {
        render_status_bar(
            frame,
            status_bar,
            ctx_mgr,
            diff_view,
            theme,
            model,
            diff_focused,
            has_copied_commits,
            keybindings,
            has_active_filters,
        );
    }
}

pub(super) const MAX_STATUS_BAR_HINTS: usize = 7;

pub(super) fn render_status_bar(
    frame: &mut Frame,
    rect: Rect,
    ctx_mgr: &ContextManager,
    diff_view: &DiffViewState,
    theme: &crate::config::Theme,
    model: &Model,
    diff_focused: bool,
    has_copied_commits: bool,
    keybindings: &KeybindingConfig,
    has_active_filters: bool,
) {
    let mut hints: Vec<(&str, &str)> = Vec::new();
    let open_log_menu_key = format_key_hint(&keybindings.commits.open_log_menu);
    let toggle_head_key = format_key_hint(&keybindings.universal.toggle_working_tree_and_head);
    let commit_key = format_key_hint(&keybindings.files.commit_changes);
    let ai_commit_key = format_key_hint(&keybindings.files.generate_ai_commit);
    let quick_ai_note_key = format_key_hint(&keybindings.universal.quick_ai_note);
    let return_to_cockpit_key = if !keybindings.universal.return_to_cockpit.is_empty() {
        Some(format_key_hint(&keybindings.universal.return_to_cockpit))
    } else if std::env::var("LAZYGITRS_COCKPIT").is_ok() {
        Some(format_key_hint(&keybindings.universal.return_key))
    } else {
        None
    };

    // When in a special state (rebasing/merging/cherry-picking), show those options prominently
    if model.is_rebasing {
        hints.push(("m", "continue/abort/skip rebase"));
    } else if model.is_merging {
        hints.push(("m", "continue/abort merge"));
    } else if model.is_cherry_picking {
        hints.push(("m", "continue/abort cherry-pick"));
    }

    if diff_view.selected_note.is_some() {
        hints.insert(0, ("enter", "view note details"));
        hints.insert(1, ("y", "copy note"));
        hints.insert(2, ("S", "send to AI"));
        hints.insert(3, ("r", "reset status"));
    }

    if diff_focused && !diff_view.is_empty() {
        // Diff-focused hint set: only the diff-relevant keys, kept tight.
        // Revert-related keys are grouped together at the front so users see
        // enter right next to its cycle keys. enter itself only appears when a
        // hunk is actually selected (pressing it otherwise is a no-op).
        if ctx_mgr.active() == ContextId::Files {
            let has_selection = diff_view.selected_revert_hunk.is_some();
            let has_undo = !diff_view.revert_undo_stack.is_empty();
            let mut idx = 0;
            if !toggle_head_key.is_empty() {
                hints.insert(idx, (toggle_head_key.as_str(), "head"));
                idx += 1;
            }
            hints.insert(idx, ("space", "stage hunk"));
            idx += 1;
            if has_selection {
                hints.insert(idx, ("enter", "hunk menu"));
                idx += 1;
            }
            hints.insert(idx, ("{ }", "cycle hunks"));
            idx += 1;
            if has_undo {
                hints.insert(idx, ("u", "undo revert"));
            }
        } else {
            if (ctx_mgr.active() == ContextId::Commits
                || ctx_mgr.active() == ContextId::CommitFiles)
                && !toggle_head_key.is_empty()
            {
                hints.push((toggle_head_key.as_str(), "files"));
            }
            hints.push(("{ }", "prev/next hunk"));
        }
        hints.push(("[ ]", "side view"));
        if !quick_ai_note_key.is_empty() {
            hints.push((quick_ai_note_key.as_str(), "agent note"));
        }
        let view_layout_hint = match diff_view.view_layout {
            DiffViewLayout::SideBySide => "unified view",
            DiffViewLayout::Unified => "split view",
        };
        hints.push(("\\", view_layout_hint));
    } else {
        // Sidebar-focused: context-specific hints.
        let view_layout_hint = match diff_view.view_layout {
            DiffViewLayout::SideBySide => "unified view",
            DiffViewLayout::Unified => "split view",
        };
        match ctx_mgr.active() {
            ContextId::Files => {
                if !toggle_head_key.is_empty() {
                    hints.push((toggle_head_key.as_str(), "head"));
                }
                if !commit_key.is_empty() {
                    hints.push((commit_key.as_str(), "commit"));
                }
                if !ai_commit_key.is_empty() {
                    hints.push((ai_commit_key.as_str(), "ai commit"));
                }
                hints.push(("a", "stage all"));
                if !quick_ai_note_key.is_empty() {
                    hints.push((quick_ai_note_key.as_str(), "agent note"));
                }
                hints.extend([
                    ("space", "toggle"),
                    ("`", "tree"),
                    ("\\", view_layout_hint),
                    ("d", "discard"),
                    ("e", "edit"),
                    ("o", "open"),
                ]);
            }
            ContextId::CommitFiles | ContextId::StashFiles | ContextId::BranchCommitFiles => {
                if ctx_mgr.active() == ContextId::CommitFiles && !toggle_head_key.is_empty() {
                    hints.push((toggle_head_key.as_str(), "files"));
                }
                hints.extend([
                    ("enter", "focus diff"),
                    ("`", "tree"),
                    ("\\", view_layout_hint),
                    ("y", "copy"),
                ]);
            }
            ContextId::BranchCommits => {
                hints.extend([
                    ("enter", "commit files"),
                    ("\\", view_layout_hint),
                    (".", "details"),
                ]);
            }
            ContextId::Reflog => {
                hints.extend([
                    ("enter", "commit files"),
                    ("\\", view_layout_hint),
                    (".", "details"),
                ]);
            }
            ContextId::Branches => {
                hints.extend([
                    ("space", "checkout"),
                    ("n", "new"),
                    ("w", "worktree"),
                    ("d", "delete"),
                    ("M", "merge"),
                    ("r", "rebase"),
                ]);
            }
            ContextId::Commits => {
                if !toggle_head_key.is_empty() {
                    hints.push((toggle_head_key.as_str(), "files"));
                }
                if has_active_filters {
                    hints.push(("esc", "reset filter"));
                }
                if has_copied_commits {
                    hints.push(("V", "paste (cherry-pick)"));
                }
                if !open_log_menu_key.is_empty() {
                    hints.push((open_log_menu_key.as_str(), "filter branch"));
                }
                hints.extend([
                    ("r", "reword"),
                    ("g", "reset"),
                    ("C", "copy (cherry-pick)"),
                    ("t", "revert"),
                    ("\\", view_layout_hint),
                ]);
                hints.push(("a", "toggle log view"));
            }
            ContextId::Stash => {
                hints.extend([
                    ("g", "pop"),
                    ("space", "apply"),
                    ("d", "drop"),
                    ("\\", view_layout_hint),
                ]);
            }
            ContextId::Remotes => {
                hints.extend([
                    ("enter", "branches"),
                    ("f", "fetch"),
                    ("e", "edit"),
                    ("P", "push"),
                    ("p", "pull"),
                ]);
            }
            ContextId::RemoteBranches => {
                hints.extend([
                    ("enter", "commits"),
                    ("space", "checkout"),
                    ("M", "merge"),
                    ("r", "rebase"),
                    ("d", "delete"),
                ]);
            }
            ContextId::Tags => {
                hints.extend([("n", "new"), ("d", "delete"), ("P", "push")]);
            }
            ContextId::Worktrees => {
                hints.extend([
                    ("space", "switch"),
                    ("t", "tmux"),
                    ("n", "new"),
                    ("d", "remove"),
                ]);
            }
            ContextId::Submodules => {
                hints.extend([
                    ("space", "update"),
                    ("a", "add"),
                    ("d", "remove"),
                    ("e", "enter"),
                ]);
            }
            _ => {}
        }
        if !diff_view.is_empty() {
            hints.push(("J K", "scroll diff"));
            hints.push(("{ }", "hunks"));
        }
    }

    // Global hints (always last)
    if let Some(ref k) = return_to_cockpit_key {
        hints.push((k.as_str(), "cockpit"));
    }
    hints.extend([
        ("?", "help"),
        ("ctrl+w", "resize"),
        ("q", "quit"),
        (":", "shell"),
        ("tab 1-5", "panels"),
        ("j k", "nav"),
    ]);

    if hints.len() > MAX_STATUS_BAR_HINTS {
        hints.truncate(MAX_STATUS_BAR_HINTS - 1);
        if let Some(ref k) = return_to_cockpit_key {
            hints.push((k.as_str(), "cockpit"));
        } else {
            hints.push(("?", "help"));
        }
    }

    let key_style = Style::default()
        .fg(theme.accent)
        .add_modifier(ratatui::style::Modifier::BOLD);
    let desc_style = Style::default().fg(theme.text_dimmed);
    let sep_style = Style::default().fg(theme.text_dimmed);

    let mut spans: Vec<Span> = Vec::with_capacity(hints.len() * 3);
    for (idx, (key, desc)) in hints.iter().enumerate() {
        if idx == 0 {
            spans.push(Span::styled(format!(" {} ", key), key_style));
        } else {
            spans.push(Span::styled(" │ ", sep_style));
            spans.push(Span::styled(format!("{} ", key), key_style));
        }
        spans.push(Span::styled((*desc).to_string(), desc_style));
    }

    let bar = Paragraph::new(Line::from(spans));
    frame.render_widget(bar, rect);
}
