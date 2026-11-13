use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use std::collections::HashSet;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::config::{AppConfig, KeybindingConfig, Theme};
use crate::model::Model;
use crate::model::commit::{Commit, CommitStat};
use crate::model::file_tree::{CommitFileTreeNode, FileTreeNode};
use crate::pager::side_by_side::{self, DiffPanel, DiffPanelLayout, DiffViewLayout, DiffViewState};

use super::ScreenMode;
use super::context::{ContextId, ContextManager, SideWindow};
use super::layout::{self, LayoutState};
use super::popup::{CommitInputFocus, PopupState, list_picker_matching_indices};
use super::presentation;

mod command_log;
mod commit_details;
mod lists;
mod overlays;
mod popups;
mod smart_commit;
mod status_bar;
mod status_panel;
#[cfg(test)]
mod tests;
mod titles;

use command_log::*;
use commit_details::*;
use lists::*;
pub use overlays::*;
pub use popups::*;
pub use smart_commit::*;
use status_bar::*;
use status_panel::*;
use titles::*;

fn context_panel_key(ctx_id: ContextId) -> &'static str {
    match ctx_id {
        ContextId::Status => "status",
        ContextId::Files
        | ContextId::CommitFiles
        | ContextId::BranchCommitFiles
        | ContextId::StashFiles => "files",
        ContextId::RemoteBranches | ContextId::Tags | ContextId::Branches | ContextId::Remotes => {
            "branches"
        }
        ContextId::Commits | ContextId::BranchCommits | ContextId::Reflog => "commits",
        ContextId::Stash => "stash",
        _ => "default",
    }
}

pub fn render(
    frame: &mut Frame,
    model: &Model,
    ctx_mgr: &mut ContextManager,
    layout_state: &LayoutState,
    popup: &PopupState,
    config: &AppConfig,
    theme: &Theme,
    diff_view: &mut DiffViewState,
    commit_list_cache: &mut presentation::commits::CommitListCache,
    screen_mode: ScreenMode,
    show_file_tree: bool,
    file_tree_nodes: &[FileTreeNode],
    collapsed_dirs: &HashSet<String>,
    diff_focused: bool,
    search_state: Option<(&str, usize, usize)>,
    search_textarea: Option<&ratatui_textarea::TextArea<'_>>,
    command_log: &[String],
    show_command_log: bool,
    active_commit_filters: &[String],
    show_commit_file_tree: bool,
    commit_file_tree_nodes: &[CommitFileTreeNode],
    commit_files_collapsed_dirs: &HashSet<String>,
    commit_files_hash: &str,
    commit_files_message: &str,
    branch_commits_name: &str,
    remote_branches_name: &str,
    sub_commits_parent_context: ContextId,
    spinner_frame: usize,
    remote_op_label: Option<&str>,
    remote_op_success: bool,
    cherry_pick_clipboard: &[String],
    range_select_anchor: Option<usize>,
    diff_loading: bool,
    diff_loading_show: bool,
    commit_stats: &Arc<Mutex<HashMap<String, CommitStat>>>,
    commit_messages: &Arc<Mutex<HashMap<String, String>>>,
    commit_details_scroll: &mut u16,
    commit_details_scroll_hash: &mut String,
    show_commit_details: bool,
    ai_button_hovered: bool,
    ai_configured: bool,
    grab_column_hovered: bool,
    all_branches_log: bool,
) {
    let area = frame.area();
    let panel_count = SideWindow::ALL.len();

    // Determine which panel index is active so it gets expanded
    let active_window = ctx_mgr.active_window();
    let active_panel_index = SideWindow::ALL
        .iter()
        .position(|w| *w == active_window)
        .unwrap_or(1); // default to Files

    // Determine if the commit-details panel should be shown.  We show it when
    // the active context is a commit-listing context and a commit is selected.
    let current_commit: Option<&Commit> = resolve_current_commit(model, ctx_mgr, commit_files_hash);
    let show_details = show_commit_details && current_commit.is_some();

    let fl = layout::compute_layout_with_details(
        area,
        layout_state.side_panel_ratio,
        panel_count,
        active_panel_index,
        screen_mode,
        show_details,
        !diff_focused, // sidebar_focused_full: only meaningful in Full mode
    );

    // Full screen mode
    if screen_mode == ScreenMode::Full {
        if diff_focused {
            // Diff is focused: show diff fullscreen
            if !diff_view.is_empty() {
                let show_revert_markers = ctx_mgr.active() == ContextId::Files;
                side_by_side::render_diff(
                    frame,
                    fl.main_panel,
                    diff_view,
                    theme,
                    true,
                    diff_loading_show,
                    show_revert_markers,
                    spinner_frame,
                );
                side_by_side::render_diff_search_highlights(frame, fl.main_panel, diff_view, theme);
                side_by_side::render_diff_search_bar(frame, fl.main_panel, diff_view, theme);
            } else if diff_loading {
                let block = Block::default()
                    .title(" Diff ")
                    .borders(theme.borders_for("diff"))
                    .border_type(theme.border_type_for("diff"))
                    .border_style(theme.active_border);
                if diff_loading_show {
                    let widget = Paragraph::new(" Loading diff...").block(block);
                    frame.render_widget(widget, fl.main_panel);
                } else {
                    frame.render_widget(block, fl.main_panel);
                }
            } else {
                let block = Block::default()
                    .title(" Diff ")
                    .borders(theme.borders_for("diff"))
                    .border_type(theme.border_type_for("diff"))
                    .border_style(theme.active_border);
                let widget = Paragraph::new(" No changes to display").block(block);
                frame.render_widget(widget, fl.main_panel);
            }
        } else {
            // Sidebar is focused: show active sidebar panel fullscreen
            let ctx_id = ctx_mgr.active();
            let selected = ctx_mgr.selected(ctx_id);
            let title = if ctx_id == ContextId::CommitFiles
                || ctx_id == ContextId::StashFiles
                || ctx_id == ContextId::BranchCommitFiles
            {
                build_commit_files_title(ctx_id, commit_files_hash, commit_files_message, theme)
            } else if ctx_id == ContextId::BranchCommits {
                build_branch_commits_title(branch_commits_name, theme)
            } else if ctx_id == ContextId::Commits && !active_commit_filters.is_empty() {
                let filter_label = active_commit_filters.join(", ");
                Line::from(vec![
                    Span::raw(" Commits "),
                    Span::styled(
                        format!("[filter: {}] ", filter_label),
                        Style::default().fg(theme.accent_secondary),
                    ),
                ])
            } else {
                build_window_title(
                    ctx_mgr.active_window(),
                    ctx_id,
                    ctx_mgr,
                    theme,
                    all_branches_log,
                )
            };
            let panel_key = context_panel_key(ctx_id);
            let block = Block::default()
                .title(title)
                .borders(theme.borders_for(panel_key))
                .border_type(theme.border_type_for(panel_key))
                .border_style(theme.active_border);

            match ctx_id {
                ContextId::Status => {
                    render_status_main(frame, fl.main_panel, model, config, theme, block);
                }
                ContextId::Files => {
                    if show_file_tree {
                        let items = presentation::files::render_file_tree(
                            model,
                            theme,
                            file_tree_nodes,
                            collapsed_dirs,
                            fl.main_panel.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            fl.main_panel,
                            block,
                            items,
                            selected,
                            true,
                            theme,
                            ctx_mgr,
                            ctx_id,
                        );
                    } else {
                        let items = presentation::files::render_file_list(
                            model,
                            theme,
                            fl.main_panel.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            fl.main_panel,
                            block,
                            items,
                            selected,
                            true,
                            theme,
                            ctx_mgr,
                            ctx_id,
                        );
                    }
                }
                ContextId::Worktrees | ContextId::Submodules => {
                    let items = if ctx_id == ContextId::Worktrees {
                        render_worktree_list(model, theme)
                    } else {
                        render_submodule_list(model, theme)
                    };
                    render_list_ctx(
                        frame,
                        fl.main_panel,
                        block,
                        items,
                        selected,
                        true,
                        theme,
                        ctx_mgr,
                        ctx_id,
                    );
                }
                ContextId::Branches => {
                    let items = presentation::branches::render_branch_list(
                        model,
                        theme,
                        remote_op_label,
                        spinner_frame,
                        remote_op_success,
                    );
                    render_list_ctx(
                        frame,
                        fl.main_panel,
                        block,
                        items,
                        selected,
                        true,
                        theme,
                        ctx_mgr,
                        ctx_id,
                    );
                }
                ContextId::Remotes | ContextId::RemoteBranches => {
                    if ctx_mgr.active() == ContextId::RemoteBranches {
                        let rb_selected = ctx_mgr.selected(ContextId::RemoteBranches);
                        let rb_block = Block::default()
                            .title(format!(" Remote Branches ({}) ", remote_branches_name))
                            .borders(theme.borders_for("branches"))
                            .border_type(theme.border_type_for("branches"))
                            .border_style(theme.active_border);
                        let items = presentation::remote_branches::render_remote_branch_list(
                            &model.sub_remote_branches,
                            &model.head_branch_name,
                            theme,
                        );
                        render_list_ctx(
                            frame,
                            fl.main_panel,
                            rb_block,
                            items,
                            rb_selected,
                            true,
                            theme,
                            ctx_mgr,
                            ContextId::RemoteBranches,
                        );
                    } else {
                        let items = presentation::remotes::render_remote_list(model, theme);
                        render_list_ctx(
                            frame,
                            fl.main_panel,
                            block,
                            items,
                            selected,
                            true,
                            theme,
                            ctx_mgr,
                            ctx_id,
                        );
                    }
                }
                ContextId::Tags => {
                    let items = presentation::tags::render_tag_list(model, theme);
                    render_list_ctx(
                        frame,
                        fl.main_panel,
                        block,
                        items,
                        selected,
                        true,
                        theme,
                        ctx_mgr,
                        ctx_id,
                    );
                }
                ContextId::Commits => {
                    let range = range_select_anchor.map(|a| (a.min(selected), a.max(selected)));
                    render_commit_list_ctx(
                        frame,
                        fl.main_panel,
                        block,
                        model,
                        theme,
                        cherry_pick_clipboard,
                        selected,
                        true,
                        range,
                        ctx_mgr,
                        ctx_id,
                        commit_list_cache,
                        false,
                        screen_mode != ScreenMode::Normal,
                    );
                }
                ContextId::Stash => {
                    let items = presentation::stash::render_stash_list(model, theme);
                    render_list_ctx(
                        frame,
                        fl.main_panel,
                        block,
                        items,
                        selected,
                        true,
                        theme,
                        ctx_mgr,
                        ctx_id,
                    );
                }
                ContextId::BranchCommits => {
                    render_commit_list_ctx(
                        frame,
                        fl.main_panel,
                        block,
                        model,
                        theme,
                        &[],
                        selected,
                        true,
                        None,
                        ctx_mgr,
                        ctx_id,
                        commit_list_cache,
                        true,
                        screen_mode != ScreenMode::Normal,
                    );
                }
                ContextId::CommitFiles | ContextId::StashFiles | ContextId::BranchCommitFiles => {
                    if show_commit_file_tree {
                        let items = presentation::commit_files::render_commit_file_tree(
                            model,
                            theme,
                            commit_file_tree_nodes,
                            commit_files_collapsed_dirs,
                            fl.main_panel.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            fl.main_panel,
                            block,
                            items,
                            selected,
                            true,
                            theme,
                            ctx_mgr,
                            ctx_id,
                        );
                    } else {
                        let items = presentation::commit_files::render_commit_file_list(
                            model,
                            theme,
                            fl.main_panel.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            fl.main_panel,
                            block,
                            items,
                            selected,
                            true,
                            theme,
                            ctx_mgr,
                            ctx_id,
                        );
                    }
                }
                _ => {
                    let widget = Paragraph::new("").block(block);
                    frame.render_widget(widget, fl.main_panel);
                }
            }
            // Highlight `/` search matches on the full-mode list.
            if let Some((query, _, _)) = search_state
                && !query.is_empty()
            {
                render_list_search_highlights(frame, fl.main_panel, query, theme);
            }
        }
        // Full-mode details strip (sidebar-focused only) — compact, above sidebar.
        if let (Some(details_rect), Some(commit)) = (fl.commit_details_panel, current_commit) {
            if commit_details_scroll_hash.as_str() != commit.hash.as_str() {
                *commit_details_scroll = 0;
                *commit_details_scroll_hash = commit.hash.clone();
            }
            render_commit_details_panel(
                frame,
                details_rect,
                commit,
                commit_stats,
                commit_messages,
                theme,
                true,
                commit_details_scroll,
            );
        }
        render_search_bar_or_status_bar(
            frame,
            fl.status_bar,
            search_state,
            search_textarea,
            ctx_mgr,
            diff_view,
            theme,
            model,
            diff_focused,
            !cherry_pick_clipboard.is_empty(),
            &config.user_config.keybinding,
            !active_commit_filters.is_empty(),
        );
        // Render text selection highlight overlay and tooltip (must be before popup)
        render_selection_overlay(frame, diff_view, fl.main_panel, theme);
        if *popup != PopupState::None {
            render_popup(
                frame,
                popup,
                area,
                spinner_frame,
                theme,
                ai_button_hovered,
                ai_configured,
            );
        }
        render_command_log(frame, &fl, command_log, show_command_log, theme);
        return;
    }

    // Render sidebar panels — one per window
    for (i, window) in SideWindow::ALL.iter().enumerate() {
        if i >= fl.side_panels.len() {
            break;
        }
        let rect = fl.side_panels[i];
        let ctx_id = ctx_mgr.active_context_for_window(*window);
        let is_active = ctx_mgr.active_window() == *window;
        let selected = ctx_mgr.selected(ctx_id);

        let border_style = if is_active && !diff_focused {
            theme.active_border
        } else {
            theme.inactive_border
        };

        // Build title with tab indicators for multi-tab windows
        let title = if *window == SideWindow::Commits
            && ctx_id == ContextId::Commits
            && !active_commit_filters.is_empty()
        {
            let filter_label = active_commit_filters.join(", ");
            Line::from(vec![
                Span::raw(" Commits "),
                Span::styled(
                    format!("[filter: {}] ", filter_label),
                    Style::default().fg(theme.accent_secondary),
                ),
            ])
        } else {
            build_window_title(*window, ctx_id, ctx_mgr, theme, all_branches_log)
        };

        let panel_key = context_panel_key(ctx_id);
        let block = Block::default()
            .title(title)
            .borders(theme.borders_for(panel_key))
            .border_type(theme.border_type_for(panel_key))
            .border_style(border_style);

        match ctx_id {
            ContextId::Status => {
                let inner_width = rect.width.saturating_sub(2) as usize;
                let status_line = render_status_sidebar(model, config, inner_width, theme);
                let widget = Paragraph::new(status_line).block(block);
                frame.render_widget(widget, rect);
            }
            ContextId::Files => {
                if show_file_tree {
                    let items = presentation::files::render_file_tree(
                        model,
                        theme,
                        file_tree_nodes,
                        collapsed_dirs,
                        rect.width.saturating_sub(2) as usize,
                    );
                    render_list_ctx(
                        frame, rect, block, items, selected, is_active, theme, ctx_mgr, ctx_id,
                    );
                } else {
                    let items = presentation::files::render_file_list(
                        model,
                        theme,
                        rect.width.saturating_sub(2) as usize,
                    );
                    render_list_ctx(
                        frame, rect, block, items, selected, is_active, theme, ctx_mgr, ctx_id,
                    );
                }
            }
            ContextId::Worktrees => {
                let items = render_worktree_list(model, theme);
                render_list_ctx(
                    frame, rect, block, items, selected, is_active, theme, ctx_mgr, ctx_id,
                );
            }
            ContextId::Submodules => {
                let items = render_submodule_list(model, theme);
                render_list_ctx(
                    frame, rect, block, items, selected, is_active, theme, ctx_mgr, ctx_id,
                );
            }
            ContextId::Branches => {
                // If BranchCommits or BranchCommitFiles is active, render that instead
                if ctx_mgr.active() == ContextId::BranchCommitFiles {
                    let cf_selected = ctx_mgr.selected(ContextId::BranchCommitFiles);
                    let cf_title = build_commit_files_title(
                        ContextId::BranchCommitFiles,
                        commit_files_hash,
                        commit_files_message,
                        theme,
                    );
                    let cf_block = Block::default()
                        .title(cf_title)
                        .borders(theme.borders_for("files"))
                        .border_type(theme.border_type_for("files"))
                        .border_style(border_style);
                    if show_commit_file_tree {
                        let items = presentation::commit_files::render_commit_file_tree(
                            model,
                            theme,
                            commit_file_tree_nodes,
                            commit_files_collapsed_dirs,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            cf_block,
                            items,
                            cf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::BranchCommitFiles,
                        );
                    } else {
                        let items = presentation::commit_files::render_commit_file_list(
                            model,
                            theme,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            cf_block,
                            items,
                            cf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::BranchCommitFiles,
                        );
                    }
                } else if ctx_mgr.active() == ContextId::BranchCommits {
                    let bc_selected = ctx_mgr.selected(ContextId::BranchCommits);
                    let bc_title = build_branch_commits_title(branch_commits_name, theme);
                    let bc_block = Block::default()
                        .title(bc_title)
                        .borders(theme.borders_for("commits"))
                        .border_type(theme.border_type_for("commits"))
                        .border_style(border_style);
                    render_commit_list_ctx(
                        frame,
                        rect,
                        bc_block,
                        model,
                        theme,
                        &[],
                        bc_selected,
                        is_active,
                        None,
                        ctx_mgr,
                        ContextId::BranchCommits,
                        commit_list_cache,
                        true,
                        screen_mode != ScreenMode::Normal,
                    );
                } else {
                    let items = presentation::branches::render_branch_list(
                        model,
                        theme,
                        remote_op_label,
                        spinner_frame,
                        remote_op_success,
                    );
                    render_list_ctx(
                        frame, rect, block, items, selected, is_active, theme, ctx_mgr, ctx_id,
                    );
                }
            }
            ContextId::Remotes => {
                if ctx_mgr.active() == ContextId::BranchCommitFiles
                    && sub_commits_parent_context == ContextId::RemoteBranches
                {
                    let cf_selected = ctx_mgr.selected(ContextId::BranchCommitFiles);
                    let cf_title = build_commit_files_title(
                        ContextId::BranchCommitFiles,
                        commit_files_hash,
                        commit_files_message,
                        theme,
                    );
                    let cf_block = Block::default()
                        .title(cf_title)
                        .borders(theme.borders_for("files"))
                        .border_type(theme.border_type_for("files"))
                        .border_style(border_style);
                    if show_commit_file_tree {
                        let items = presentation::commit_files::render_commit_file_tree(
                            model,
                            theme,
                            commit_file_tree_nodes,
                            commit_files_collapsed_dirs,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            cf_block,
                            items,
                            cf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::BranchCommitFiles,
                        );
                    } else {
                        let items = presentation::commit_files::render_commit_file_list(
                            model,
                            theme,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            cf_block,
                            items,
                            cf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::BranchCommitFiles,
                        );
                    }
                } else if ctx_mgr.active() == ContextId::BranchCommits
                    && sub_commits_parent_context == ContextId::RemoteBranches
                {
                    let bc_selected = ctx_mgr.selected(ContextId::BranchCommits);
                    let bc_title = build_branch_commits_title(branch_commits_name, theme);
                    let bc_block = Block::default()
                        .title(bc_title)
                        .borders(theme.borders_for("commits"))
                        .border_type(theme.border_type_for("commits"))
                        .border_style(border_style);
                    render_commit_list_ctx(
                        frame,
                        rect,
                        bc_block,
                        model,
                        theme,
                        &[],
                        bc_selected,
                        is_active,
                        None,
                        ctx_mgr,
                        ContextId::BranchCommits,
                        commit_list_cache,
                        true,
                        screen_mode != ScreenMode::Normal,
                    );
                } else if ctx_mgr.active() == ContextId::RemoteBranches {
                    let rb_selected = ctx_mgr.selected(ContextId::RemoteBranches);
                    let rb_title = format!(" Remote Branches ({}) ", remote_branches_name);
                    let rb_block = Block::default()
                        .title(rb_title)
                        .borders(theme.borders_for("branches"))
                        .border_type(theme.border_type_for("branches"))
                        .border_style(border_style);
                    let items = presentation::remote_branches::render_remote_branch_list(
                        &model.sub_remote_branches,
                        &model.head_branch_name,
                        theme,
                    );
                    render_list_ctx(
                        frame,
                        rect,
                        rb_block,
                        items,
                        rb_selected,
                        is_active,
                        theme,
                        ctx_mgr,
                        ContextId::RemoteBranches,
                    );
                } else {
                    let items = presentation::remotes::render_remote_list(model, theme);
                    render_list_ctx(
                        frame, rect, block, items, selected, is_active, theme, ctx_mgr, ctx_id,
                    );
                }
            }
            ContextId::Tags => {
                // If BranchCommits or BranchCommitFiles is active (drill-down from Tags), render that instead
                if ctx_mgr.active() == ContextId::BranchCommitFiles {
                    let cf_selected = ctx_mgr.selected(ContextId::BranchCommitFiles);
                    let cf_title = build_commit_files_title(
                        ContextId::BranchCommitFiles,
                        commit_files_hash,
                        commit_files_message,
                        theme,
                    );
                    let cf_block = Block::default()
                        .title(cf_title)
                        .borders(theme.borders_for("files"))
                        .border_type(theme.border_type_for("files"))
                        .border_style(border_style);
                    if show_commit_file_tree {
                        let items = presentation::commit_files::render_commit_file_tree(
                            model,
                            theme,
                            commit_file_tree_nodes,
                            commit_files_collapsed_dirs,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            cf_block,
                            items,
                            cf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::BranchCommitFiles,
                        );
                    } else {
                        let items = presentation::commit_files::render_commit_file_list(
                            model,
                            theme,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            cf_block,
                            items,
                            cf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::BranchCommitFiles,
                        );
                    }
                } else if ctx_mgr.active() == ContextId::BranchCommits {
                    let bc_selected = ctx_mgr.selected(ContextId::BranchCommits);
                    let bc_title = build_branch_commits_title(branch_commits_name, theme);
                    let bc_block = Block::default()
                        .title(bc_title)
                        .borders(theme.borders_for("commits"))
                        .border_type(theme.border_type_for("commits"))
                        .border_style(border_style);
                    render_commit_list_ctx(
                        frame,
                        rect,
                        bc_block,
                        model,
                        theme,
                        &[],
                        bc_selected,
                        is_active,
                        None,
                        ctx_mgr,
                        ContextId::BranchCommits,
                        commit_list_cache,
                        true,
                        screen_mode != ScreenMode::Normal,
                    );
                } else {
                    let items = presentation::tags::render_tag_list(model, theme);
                    render_list_ctx(
                        frame, rect, block, items, selected, is_active, theme, ctx_mgr, ctx_id,
                    );
                }
            }
            ContextId::Commits => {
                // If CommitFiles is active within this window, render that instead
                if ctx_mgr.active() == ContextId::CommitFiles {
                    let cf_selected = ctx_mgr.selected(ContextId::CommitFiles);
                    let cf_title = build_commit_files_title(
                        ContextId::CommitFiles,
                        commit_files_hash,
                        commit_files_message,
                        theme,
                    );
                    let cf_block = Block::default()
                        .title(cf_title)
                        .borders(theme.borders_for("files"))
                        .border_type(theme.border_type_for("files"))
                        .border_style(border_style);
                    if show_commit_file_tree {
                        let items = presentation::commit_files::render_commit_file_tree(
                            model,
                            theme,
                            commit_file_tree_nodes,
                            commit_files_collapsed_dirs,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            cf_block,
                            items,
                            cf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::CommitFiles,
                        );
                    } else {
                        let items = presentation::commit_files::render_commit_file_list(
                            model,
                            theme,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            cf_block,
                            items,
                            cf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::CommitFiles,
                        );
                    }
                } else {
                    let range = if is_active {
                        range_select_anchor.map(|a| (a.min(selected), a.max(selected)))
                    } else {
                        None
                    };
                    render_commit_list_ctx(
                        frame,
                        rect,
                        block,
                        model,
                        theme,
                        cherry_pick_clipboard,
                        selected,
                        is_active,
                        range,
                        ctx_mgr,
                        ctx_id,
                        commit_list_cache,
                        false,
                        screen_mode != ScreenMode::Normal,
                    );
                }
            }
            ContextId::Reflog => {
                // If CommitFiles is active (drill-down from Reflog), render that instead
                if ctx_mgr.active() == ContextId::CommitFiles {
                    let cf_selected = ctx_mgr.selected(ContextId::CommitFiles);
                    let cf_title = build_commit_files_title(
                        ContextId::CommitFiles,
                        commit_files_hash,
                        commit_files_message,
                        theme,
                    );
                    let cf_block = Block::default()
                        .title(cf_title)
                        .borders(theme.borders_for("files"))
                        .border_type(theme.border_type_for("files"))
                        .border_style(border_style);
                    if show_commit_file_tree {
                        let items = presentation::commit_files::render_commit_file_tree(
                            model,
                            theme,
                            commit_file_tree_nodes,
                            commit_files_collapsed_dirs,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            cf_block,
                            items,
                            cf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::CommitFiles,
                        );
                    } else {
                        let items = presentation::commit_files::render_commit_file_list(
                            model,
                            theme,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            cf_block,
                            items,
                            cf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::CommitFiles,
                        );
                    }
                } else {
                    let items = presentation::reflog::render_reflog_list(model, theme);
                    render_list_ctx(
                        frame, rect, block, items, selected, is_active, theme, ctx_mgr, ctx_id,
                    );
                }
            }
            ContextId::Stash => {
                // If StashFiles is active within this window, render that instead
                if ctx_mgr.active() == ContextId::StashFiles {
                    let sf_selected = ctx_mgr.selected(ContextId::StashFiles);
                    let sf_title = build_commit_files_title(
                        ContextId::StashFiles,
                        commit_files_hash,
                        commit_files_message,
                        theme,
                    );
                    let sf_block = Block::default()
                        .title(sf_title)
                        .borders(theme.borders_for("files"))
                        .border_type(theme.border_type_for("files"))
                        .border_style(border_style);
                    if show_commit_file_tree {
                        let items = presentation::commit_files::render_commit_file_tree(
                            model,
                            theme,
                            commit_file_tree_nodes,
                            commit_files_collapsed_dirs,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            sf_block,
                            items,
                            sf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::StashFiles,
                        );
                    } else {
                        let items = presentation::commit_files::render_commit_file_list(
                            model,
                            theme,
                            rect.width.saturating_sub(2) as usize,
                        );
                        render_list_ctx(
                            frame,
                            rect,
                            sf_block,
                            items,
                            sf_selected,
                            is_active,
                            theme,
                            ctx_mgr,
                            ContextId::StashFiles,
                        );
                    }
                } else {
                    let items = presentation::stash::render_stash_list(model, theme);
                    render_list_ctx(
                        frame, rect, block, items, selected, is_active, theme, ctx_mgr, ctx_id,
                    );
                }
            }
            _ => {
                let widget = Paragraph::new("").block(block);
                frame.render_widget(widget, rect);
            }
        }
    }

    // Highlight `/` search matches on the active list (lazygit-style substring).
    // Applied after list widgets paint so selection styles stay intact.
    if let Some((query, _, _)) = search_state
        && !query.is_empty()
        && let Some(list_rect) = fl.side_panels.get(active_panel_index).copied()
    {
        render_list_search_highlights(frame, list_rect, query, theme);
    }

    // Render main panel (skipped when side panel is fully expanded)
    if fl.main_panel.width > 0 {
        if ctx_mgr.active() == ContextId::Status {
            // Status view: show logo + copyright in the main content area
            let status_block = Block::default()
                .title(" Status ")
                .borders(theme.borders_for("status"))
                .border_type(theme.border_type_for("status"))
                .border_style(theme.active_border);
            render_status_main(frame, fl.main_panel, model, config, theme, status_block);
        } else if !diff_view.is_empty() {
            let show_revert_markers = ctx_mgr.active() == ContextId::Files;
            side_by_side::render_diff(
                frame,
                fl.main_panel,
                diff_view,
                theme,
                diff_focused,
                diff_loading_show,
                show_revert_markers,
                spinner_frame,
            );
            side_by_side::render_diff_search_highlights(frame, fl.main_panel, diff_view, theme);
            side_by_side::render_diff_search_bar(frame, fl.main_panel, diff_view, theme);
        } else if diff_loading {
            // Diff is being loaded — show empty panel during grace period, then "Loading..." after delay
            let block = Block::default()
                .title(" Diff ")
                .borders(theme.borders_for("diff"))
                .border_type(theme.border_type_for("diff"))
                .border_style(theme.inactive_border);
            if diff_loading_show {
                let widget = Paragraph::new(" Loading diff...").block(block);
                frame.render_widget(widget, fl.main_panel);
            } else {
                frame.render_widget(block, fl.main_panel);
            }
        } else {
            // Fallback: show info about selected item
            let block = Block::default()
                .title(" Diff ")
                .borders(theme.borders_for("diff"))
                .border_type(theme.border_type_for("diff"))
                .border_style(theme.inactive_border);

            let info = get_info_content(model, ctx_mgr);
            let widget = Paragraph::new(info).block(block);
            frame.render_widget(widget, fl.main_panel);
        }
    } // end main_panel.width > 0

    // Render grab column if present
    if let Some(grab_rect) = fl.grab_column {
        use ratatui::style::Color;
        let style = if grab_column_hovered {
            Style::default().bg(theme.selected_bg).fg(theme.accent)
        } else {
            Style::default().fg(theme.inactive_border.fg.unwrap_or(Color::DarkGray))
        };
        let buf = frame.buffer_mut();
        for y in grab_rect.top()..grab_rect.bottom() {
            if let Some(cell) = buf.cell_mut((grab_rect.x, y)) {
                if grab_column_hovered {
                    cell.set_char(' ');
                    cell.set_style(style);
                } else {
                    cell.set_char('│');
                    cell.set_style(style);
                }
            }
        }
    }

    // Normal/Half mode: compact details box sits at the bottom of the active
    // sidebar panel (layout carves the rect out of the active side panel).
    if let (Some(details_rect), Some(commit)) = (fl.commit_details_panel, current_commit) {
        if commit_details_scroll_hash.as_str() != commit.hash.as_str() {
            *commit_details_scroll = 0;
            *commit_details_scroll_hash = commit.hash.clone();
        }
        render_commit_details_panel(
            frame,
            details_rect,
            commit,
            commit_stats,
            commit_messages,
            theme,
            true,
            commit_details_scroll,
        );
    }

    render_search_bar_or_status_bar(
        frame,
        fl.status_bar,
        search_state,
        search_textarea,
        ctx_mgr,
        diff_view,
        theme,
        model,
        diff_focused,
        !cherry_pick_clipboard.is_empty(),
        &config.user_config.keybinding,
        !active_commit_filters.is_empty(),
    );

    // Render text selection highlight overlay and tooltip
    render_selection_overlay(frame, diff_view, fl.main_panel, theme);

    // Render popup overlay
    if *popup != PopupState::None {
        render_popup(
            frame,
            popup,
            area,
            spinner_frame,
            theme,
            ai_button_hovered,
            ai_configured,
        );
    }

    // Render command log last so it appears above everything
    render_command_log(frame, &fl, command_log, show_command_log, theme);
}

fn get_info_content<'a>(model: &Model, ctx_mgr: &ContextManager) -> Vec<Line<'a>> {
    let active = ctx_mgr.active();
    let selected = ctx_mgr.selected_active();

    match active {
        ContextId::Files => {
            if model.files.is_empty() {
                vec![Line::from(" No modified files")]
            } else {
                vec![Line::from(" Select a file to view diff")]
            }
        }
        ContextId::Commits => {
            if let Some(commit) = model.commits.get(selected) {
                vec![
                    Line::from(format!(" Commit: {}", commit.short_hash())),
                    Line::from(format!(
                        " Author: {} <{}>",
                        super::presentation::text::plain_text(&commit.author_name),
                        super::presentation::text::plain_text(&commit.author_email)
                    )),
                    Line::from(format!(
                        " Message: {}",
                        super::presentation::text::plain_text(&commit.name)
                    )),
                ]
            } else {
                vec![Line::from(" No commit selected")]
            }
        }
        ContextId::Branches => {
            if let Some(branch) = model.branches.get(selected) {
                let mut lines = vec![
                    Line::from(format!(" Branch: {}", branch.name)),
                    Line::from(format!(" Hash: {}", branch.hash)),
                ];
                if let Some(ref upstream) = branch.upstream {
                    lines.push(Line::from(format!(" Upstream: {}", upstream)));
                }
                lines
            } else {
                vec![Line::from(" No branch selected")]
            }
        }
        ContextId::Stash => {
            if let Some(entry) = model.stash_entries.get(selected) {
                vec![
                    Line::from(format!(" Stash: {}", entry.ref_name())),
                    Line::from(format!(" {}", entry.name)),
                ]
            } else {
                vec![Line::from(" No stash entries")]
            }
        }
        ContextId::Remotes => {
            if let Some(remote) = model.remotes.get(selected) {
                let mut lines = vec![Line::from(format!(" Remote: {}", remote.name))];
                for url in &remote.urls {
                    lines.push(Line::from(format!(" URL: {}", url)));
                }
                lines.push(Line::from(format!(" Branches: {}", remote.branches.len())));
                for branch in &remote.branches {
                    lines.push(Line::from(format!("   {} ({})", branch.name, branch.hash)));
                }
                lines
            } else {
                vec![Line::from(" No remotes")]
            }
        }
        ContextId::RemoteBranches => {
            if let Some(rb) = model.sub_remote_branches.get(selected) {
                vec![
                    Line::from(format!(" Branch: {}/{}", rb.remote_name, rb.name)),
                    Line::from(format!(" Hash: {}", rb.hash)),
                ]
            } else {
                vec![Line::from(" No remote branches")]
            }
        }
        ContextId::Tags => {
            if let Some(tag) = model.tags.get(selected) {
                let mut lines = vec![
                    Line::from(format!(" Tag: {}", tag.name)),
                    Line::from(format!(" Hash: {}", tag.hash)),
                ];
                if !tag.message.is_empty() {
                    lines.push(Line::from(format!(" Message: {}", tag.message)));
                }
                lines
            } else {
                vec![Line::from(" No tags")]
            }
        }
        ContextId::Worktrees => {
            if let Some(wt) = model.worktrees.get(selected) {
                vec![
                    Line::from(format!(" Worktree: {}", wt.branch)),
                    Line::from(format!(" Path: {}", wt.path)),
                    Line::from(format!(" Hash: {}", wt.hash)),
                ]
            } else {
                vec![Line::from(" No worktrees")]
            }
        }
        ContextId::Submodules => {
            if let Some(sub) = model.submodules.get(selected) {
                vec![
                    Line::from(format!(" Submodule: {}", sub.name)),
                    Line::from(format!(" Path: {}", sub.path)),
                ]
            } else {
                vec![Line::from(" No submodules")]
            }
        }
        _ => vec![Line::from(" lazygitrs")],
    }
}
