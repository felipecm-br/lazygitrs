//! Menus, pickers and palettes (command palette contents, rebase/commit-editor menus, recent repos) plus undo/redo.

use super::*;

impl Gui {
    pub fn show_interactive_rebase_picker(&mut self) {
        use crate::gui::popup::{ListPickerCore, make_command_palette_search_textarea};

        // Skip HEAD branch / HEAD commit — rebasing onto current tip is a no-op.
        let items = self.collect_reset_rebase_picker_items(/*skip_head=*/ true);

        self.popup = PopupState::RefPicker {
            title: "Interactive rebase current branch onto".to_string(),
            core: ListPickerCore {
                items,
                selected: 0,
                search_textarea: make_command_palette_search_textarea(),
                scroll_offset: 0,
            },
            on_confirm: Box::new(|gui, ref_name| {
                controller::branches::enter_interactive_rebase_onto(gui, ref_name)
            }),
        };
    }

    /// Global reset picker (uppercase `G`): choose a branch/commit/tag, then
    /// soft/mixed/hard reset options (lazygit `universal.viewResetOptions` /
    /// `CreateGitResetMenu`). Reuses the same searchable ref list as the
    /// interactive rebase picker, then the shared reset-options menu used by
    /// contextual lowercase `g`.
    pub fn show_reset_picker(&mut self) {
        use crate::gui::popup::{ListPickerCore, make_command_palette_search_textarea};

        // Include the current branch name so users can pick it by name; skip
        // HEAD itself in the commit list (resetting to HEAD is a no-op).
        let items = self.collect_reset_rebase_picker_items(/*skip_head=*/ false);

        self.popup = PopupState::RefPicker {
            title: "Reset to:".to_string(),
            core: ListPickerCore {
                items,
                selected: 0,
                search_textarea: make_command_palette_search_textarea(),
                scroll_offset: 0,
            },
            on_confirm: Box::new(|gui, ref_name| {
                controller::commits::show_reset_menu_for_ref(gui, ref_name)
            }),
        };
    }

    /// Shared branch/remote/tag/commit items for the global I and G pickers.
    /// When `skip_head` is true, the current HEAD branch is omitted (rebase);
    /// when false it is included (reset). HEAD itself is always omitted from
    /// the commits section because operating on the tip is a no-op.
    pub(super) fn collect_reset_rebase_picker_items(
        &self,
        skip_head: bool,
    ) -> Vec<crate::gui::popup::ListPickerItem> {
        use crate::gui::popup::ListPickerItem;

        let model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut items = Vec::new();

        for branch in &model.branches {
            if skip_head && branch.head {
                continue;
            }
            items.push(ListPickerItem {
                value: branch.name.clone(),
                label: branch.name.clone(),
                category: "Branches".to_string(),
                description: None,
            });
        }

        for remote in &model.remotes {
            for branch in &remote.branches {
                let full_name = format!("{}/{}", remote.name, branch.name);
                items.push(ListPickerItem {
                    value: full_name.clone(),
                    label: full_name,
                    category: "Remote Branches".to_string(),
                    description: None,
                });
            }
        }

        for tag in &model.tags {
            items.push(ListPickerItem {
                value: tag.name.clone(),
                label: tag.name.clone(),
                category: "Tags".to_string(),
                description: None,
            });
        }

        for commit in model.commits.iter().skip(1) {
            items.push(ListPickerItem {
                value: commit.hash.clone(),
                label: format!(
                    "{} {}",
                    commit.short_hash(),
                    presentation::text::plain_text(&commit.name)
                ),
                category: "Commits".to_string(),
                description: None,
            });
        }

        items
    }

    pub(super) fn show_command_palette(&mut self) {
        let kb = &self.config.user_config.keybinding;
        let active = self.context_mgr.active();

        // Universal keybindings
        let universal = CommandSection {
            title: "Universal".into(),
            entries: vec![
                CommandEntry::keybinding(kb.universal.quit.clone(), "Quit".into()),
                CommandEntry::keybinding(kb.universal.quit_alt1.clone(), "Quit (alt)".into()),
                CommandEntry::keybinding(kb.universal.return_key.clone(), "Return / Cancel".into()),
                CommandEntry::keybinding(kb.universal.toggle_panel.clone(), "Next panel".into()),
                CommandEntry::keybinding(
                    kb.universal.toggle_panel_reverse.clone(),
                    "Previous panel".into(),
                ),
                CommandEntry::keybinding(kb.universal.prev_item.clone(), "Previous item".into()),
                CommandEntry::keybinding(kb.universal.next_item.clone(), "Next item".into()),
                CommandEntry::keybinding(kb.universal.prev_page.clone(), "Page up".into()),
                CommandEntry::keybinding(kb.universal.next_page.clone(), "Page down".into()),
                CommandEntry::keybinding(kb.universal.goto_top.clone(), "Go to top".into()),
                CommandEntry::keybinding(kb.universal.goto_bottom.clone(), "Go to bottom".into()),
                CommandEntry::keybinding(kb.universal.prev_block.clone(), "Previous panel".into()),
                CommandEntry::keybinding(kb.universal.next_block.clone(), "Next panel".into()),
                CommandEntry::keybinding(kb.universal.start_search.clone(), "Search".into()),
                CommandEntry::keybinding(
                    kb.universal.next_match.clone(),
                    "Next search match".into(),
                ),
                CommandEntry::keybinding(
                    kb.universal.prev_match.clone(),
                    "Previous search match".into(),
                ),
                CommandEntry::keybinding(
                    kb.universal.scroll_up_main_alt1.clone(),
                    "Scroll diff up".into(),
                ),
                CommandEntry::keybinding(
                    kb.universal.scroll_down_main_alt1.clone(),
                    "Scroll diff down".into(),
                ),
                CommandEntry::keybinding(kb.universal.scroll_left.clone(), "Scroll left".into()),
                CommandEntry::keybinding(kb.universal.scroll_right.clone(), "Scroll right".into()),
                CommandEntry::keybinding(kb.universal.undo.clone(), "Undo".into()),
                CommandEntry::keybinding(kb.universal.redo.clone(), "Redo".into()),
                CommandEntry::keybinding(kb.universal.refresh.clone(), "Refresh".into()),
                CommandEntry::keybinding(kb.universal.push_files.clone(), "Push".into()),
                CommandEntry::keybinding(kb.universal.pull_files.clone(), "Pull".into()),
                CommandEntry::keybinding(
                    kb.universal.next_screen_mode.clone(),
                    "Enlarge panel".into(),
                ),
                CommandEntry::keybinding(
                    kb.universal.prev_screen_mode.clone(),
                    "Shrink panel".into(),
                ),
                CommandEntry::keybinding(
                    format!("<c-w> h / {}", kb.universal.shrink_side_panel),
                    "Shrink side panel".into(),
                ),
                CommandEntry::keybinding(
                    format!("<c-w> l / {}", kb.universal.expand_side_panel),
                    "Expand side panel".into(),
                ),
                CommandEntry::keybinding(
                    format!("<c-w> k / {}", kb.universal.side_panel_full),
                    "Side panel full width".into(),
                ),
                CommandEntry::keybinding(
                    format!("<c-w> j / {}", kb.universal.main_panel_full),
                    "Main panel full width".into(),
                ),
                CommandEntry::keybinding(
                    format!("<c-w> = / {}", kb.universal.reset_side_panel),
                    "Reset side panel width".into(),
                ),
                CommandEntry::keybinding(
                    kb.universal.create_rebase_options_menu.clone(),
                    "Rebase options".into(),
                ),
                CommandEntry::keybinding(
                    kb.universal.create_patch_options_menu.clone(),
                    "Patch options".into(),
                ),
                CommandEntry::keybinding("{/}".into(), "Previous/next hunk".into()),
                CommandEntry::keybinding(";".into(), "Toggle command log".into()),
                CommandEntry::keybinding("W".into(), "Compare / Diff mode".into()),
                CommandEntry::keybinding("I".into(), "Interactive rebase onto...".into()),
                CommandEntry::keybinding("G".into(), "Reset to...".into()),
                CommandEntry::keybinding("1-5".into(), "Jump to panel".into()),
                CommandEntry::keybinding(
                    kb.universal.custom_command_prompt.clone(),
                    "Execute shell command".into(),
                ),
                CommandEntry::keybinding(
                    kb.universal.quick_ai_note.clone(),
                    "Quick AI review note".into(),
                ),
                CommandEntry::keybinding("?".into(), "Show command palette".into()),
                CommandEntry::action(
                    "".into(),
                    "Color theme...".into(),
                    CommandAction::OpenThemePicker,
                ),
            ],
        };

        // Context-specific keybindings
        let context_section = match active {
            ContextId::Files => CommandSection {
                title: "Files".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "Toggle dir / Focus diff".into()),
                    CommandEntry::keybinding("<space>".into(), "Stage / Unstage".into()),
                    CommandEntry::keybinding(
                        kb.universal.toggle_diff_view_layout.clone(),
                        "Toggle unified / side-by-side view".into(),
                    ),
                    CommandEntry::keybinding(kb.files.commit_changes.clone(), "Commit".into()),
                    CommandEntry::keybinding(
                        kb.files.generate_ai_commit.clone(),
                        "Generate AI commit".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.files.amend_last_commit.clone(),
                        "Amend last commit".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.files.commit_changes_with_editor.clone(),
                        "Commit with editor".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.files.toggle_staged_all.clone(),
                        "Toggle stage all".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.files.stash_all_changes.clone(),
                        "Stash changes".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.files.view_stash_options.clone(),
                        "Stash options".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.files.toggle_tree_view.clone(),
                        "Toggle tree view".into(),
                    ),
                    CommandEntry::keybinding(kb.files.fetch.clone(), "Fetch".into()),
                    CommandEntry::keybinding(kb.files.ignore_file.clone(), "Ignore file".into()),
                    CommandEntry::keybinding("d".into(), "Discard changes".into()),
                    CommandEntry::keybinding(kb.universal.edit.clone(), "Open in editor".into()),
                    CommandEntry::keybinding(
                        kb.universal.open_file.clone(),
                        "Open in default program".into(),
                    ),
                    CommandEntry::keybinding("y".into(), "Copy to clipboard / note content".into()),
                    CommandEntry::keybinding(
                        "<enter> / o".into(),
                        "View full note details popup".into(),
                    ),
                    CommandEntry::keybinding(
                        "{/}".into(),
                        "Cycle prev/next revert block in diff".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.universal.revert_block.clone(),
                        "Open hunk menu (revert selected block)".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.universal.undo_revert_block.clone(),
                        "Undo last revert (session)".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.universal.quick_ai_note.clone(),
                        "Quick AI review note".into(),
                    ),
                    CommandEntry::keybinding("<c-f>".into(), "Grep diff contents".into()),
                ],
            },
            ContextId::Worktrees => CommandSection {
                title: "Worktrees".into(),
                entries: vec![
                    CommandEntry::keybinding("<space>".into(), "Switch to worktree".into()),
                    CommandEntry::keybinding("t".into(), "Open in Tmux session".into()),
                    CommandEntry::keybinding("n".into(), "Create worktree".into()),
                    CommandEntry::keybinding("d".into(), "Remove worktree".into()),
                ],
            },
            ContextId::Submodules => CommandSection {
                title: "Submodules".into(),
                entries: vec![
                    CommandEntry::keybinding("<space>".into(), "Update submodule".into()),
                    CommandEntry::keybinding("a".into(), "Add submodule".into()),
                    CommandEntry::keybinding("d".into(), "Remove submodule".into()),
                    CommandEntry::keybinding("e".into(), "Enter submodule".into()),
                    CommandEntry::keybinding("u".into(), "Update all submodules".into()),
                    CommandEntry::keybinding("i".into(), "Init submodules".into()),
                ],
            },
            ContextId::Branches => CommandSection {
                title: "Branches".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "View branch commits".into()),
                    CommandEntry::keybinding("<space>".into(), "Checkout branch".into()),
                    CommandEntry::keybinding("c".into(), "Checkout ref".into()),
                    CommandEntry::keybinding("-".into(), "Checkout previous branch".into()),
                    CommandEntry::keybinding("n".into(), "New branch".into()),
                    CommandEntry::keybinding("d".into(), "Delete branch".into()),
                    CommandEntry::keybinding(
                        kb.branches.merge_into_current_branch.clone(),
                        "Merge into current".into(),
                    ),
                    CommandEntry::keybinding(kb.branches.rebase_branch.clone(), "Rebase".into()),
                    CommandEntry::keybinding(
                        kb.branches.rename_branch.clone(),
                        "Rename branch".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.branches.fast_forward.clone(),
                        "Fast-forward".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.branches.set_upstream.clone(),
                        "Set upstream".into(),
                    ),
                    CommandEntry::keybinding("y".into(), "Copy to clipboard menu".into()),
                    CommandEntry::keybinding(
                        kb.branches.create_pull_request.clone(),
                        "Open in browser menu".into(),
                    ),
                ],
            },
            ContextId::BranchCommits | ContextId::BranchCommitFiles => CommandSection {
                title: "Branch Commits".into(),
                entries: {
                    let mut entries = vec![
                        CommandEntry::keybinding("<enter>".into(), "View commit files".into()),
                        CommandEntry::keybinding("<esc>".into(), "Back to branches".into()),
                        CommandEntry::keybinding(
                            kb.universal.toggle_diff_view_layout.clone(),
                            "Toggle unified / side-by-side view".into(),
                        ),
                        CommandEntry::keybinding(".".into(), "Toggle commit details panel".into()),
                    ];
                    // Grep only applies to the files list, not the commits list.
                    if active == ContextId::BranchCommitFiles {
                        entries.push(CommandEntry::keybinding(
                            "<c-f>".into(),
                            "Grep diff contents".into(),
                        ));
                    }
                    entries
                },
            },
            ContextId::Commits => {
                let mut entries = vec![
                    CommandEntry::keybinding(
                        kb.commits.cherry_pick_copy.clone(),
                        "Copy (cherry-pick)".into(),
                    ),
                    CommandEntry::keybinding("<enter>".into(), "View commit files".into()),
                    CommandEntry::keybinding(kb.commits.squash_down.clone(), "Squash down".into()),
                    CommandEntry::keybinding(
                        kb.commits.rename_commit.clone(),
                        "Reword commit".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.rename_commit_with_editor.clone(),
                        "Reword commit in editor".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.view_reset_options.clone(),
                        "Reset options".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.mark_commit_as_fixup.clone(),
                        "Fixup commit".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.create_fixup_commit.clone(),
                        "Create fixup commit".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.squash_above_commits.clone(),
                        "Apply fixup commits".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.move_up_commit.clone(),
                        "Move commit up".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.move_down_commit.clone(),
                        "Move commit down".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.amend_to_commit.clone(),
                        "Amend to commit".into(),
                    ),
                    CommandEntry::keybinding("d".into(), "Drop commit".into()),
                    CommandEntry::keybinding(
                        kb.commits.revert_commit.clone(),
                        "Revert commit".into(),
                    ),
                    CommandEntry::keybinding("v".into(), "Toggle range select".into()),
                    CommandEntry::keybinding(
                        kb.universal.toggle_diff_view_layout.clone(),
                        "Toggle unified / side-by-side view".into(),
                    ),
                    CommandEntry::keybinding(kb.commits.tag_commit.clone(), "Tag commit".into()),
                    CommandEntry::keybinding(
                        kb.commits.checkout_commit.clone(),
                        "Checkout commit".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.view_bisect_options.clone(),
                        "Bisect options".into(),
                    ),
                    CommandEntry::keybinding("o".into(), "Open in browser".into()),
                    CommandEntry::keybinding("y".into(), "Copy to clipboard menu".into()),
                    CommandEntry::keybinding(
                        kb.commits.interactive_rebase.clone(),
                        "Interactive rebase".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.open_log_menu.clone(),
                        "Filter commits".into(),
                    ),
                    CommandEntry::keybinding(".".into(), "Toggle commit details panel".into()),
                ];
                if !self.cherry_pick_clipboard.is_empty() {
                    entries.insert(
                        0,
                        CommandEntry::keybinding(
                            kb.commits.paste_commits.clone(),
                            "Paste (cherry-pick)".into(),
                        ),
                    );
                }
                if self.commit_filter.is_active() {
                    entries.insert(
                        0,
                        CommandEntry::keybinding("<esc>".into(), "Reset commit filter".into()),
                    );
                }
                CommandSection {
                    title: "Commits".into(),
                    entries,
                }
            }
            ContextId::CommitFiles => CommandSection {
                title: "Commit Files".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "Toggle dir / Focus diff".into()),
                    CommandEntry::keybinding("<esc>".into(), "Back to commits".into()),
                    CommandEntry::keybinding("<c-f>".into(), "Grep diff contents".into()),
                    CommandEntry::keybinding(kb.universal.edit.clone(), "Edit file".into()),
                    CommandEntry::keybinding(kb.universal.open_file.clone(), "Open file".into()),
                    CommandEntry::keybinding(
                        kb.universal.toggle_diff_view_layout.clone(),
                        "Toggle unified / side-by-side view".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.files.toggle_tree_view.clone(),
                        "Toggle tree view".into(),
                    ),
                    CommandEntry::keybinding("y".into(), "Copy to clipboard menu".into()),
                    CommandEntry::keybinding(".".into(), "Toggle commit details panel".into()),
                ],
            },
            ContextId::Reflog => CommandSection {
                title: "Reflog".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "View commit files".into()),
                    CommandEntry::keybinding(
                        kb.universal.toggle_diff_view_layout.clone(),
                        "Toggle unified / side-by-side view".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.checkout_commit.clone(),
                        "Checkout commit".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.view_reset_options.clone(),
                        "Reset options".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.commits.cherry_pick_copy.clone(),
                        "Copy (cherry-pick)".into(),
                    ),
                    CommandEntry::keybinding("y".into(), "Copy to clipboard menu".into()),
                    CommandEntry::keybinding(".".into(), "Toggle commit details panel".into()),
                ],
            },
            ContextId::Stash => CommandSection {
                title: "Stash".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "View stash files".into()),
                    CommandEntry::keybinding("<space>".into(), "Apply stash".into()),
                    CommandEntry::keybinding(
                        kb.universal.toggle_diff_view_layout.clone(),
                        "Toggle unified / side-by-side view".into(),
                    ),
                    CommandEntry::keybinding(kb.stash.pop_stash.clone(), "Pop stash".into()),
                    CommandEntry::keybinding(kb.stash.rename_stash.clone(), "Rename stash".into()),
                    CommandEntry::keybinding("d".into(), "Drop stash".into()),
                ],
            },
            ContextId::StashFiles => CommandSection {
                title: "Stash Files".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "Toggle dir / Focus diff".into()),
                    CommandEntry::keybinding("<esc>".into(), "Back to stash".into()),
                    CommandEntry::keybinding("<c-f>".into(), "Grep diff contents".into()),
                    CommandEntry::keybinding(
                        kb.universal.toggle_diff_view_layout.clone(),
                        "Toggle unified / side-by-side view".into(),
                    ),
                    CommandEntry::keybinding(
                        kb.files.toggle_tree_view.clone(),
                        "Toggle tree view".into(),
                    ),
                    CommandEntry::keybinding("y".into(), "Copy to clipboard menu".into()),
                ],
            },
            ContextId::Remotes => CommandSection {
                title: "Remotes".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "View remote branches".into()),
                    CommandEntry::keybinding("f".into(), "Fetch from remote".into()),
                    CommandEntry::keybinding("F".into(), "Add fork remote".into()),
                    CommandEntry::keybinding("n".into(), "Add new remote".into()),
                    CommandEntry::keybinding("e".into(), "Edit remote".into()),
                    CommandEntry::keybinding("d".into(), "Delete remote".into()),
                    CommandEntry::keybinding(kb.universal.push_files.clone(), "Push".into()),
                    CommandEntry::keybinding(kb.universal.pull_files.clone(), "Pull".into()),
                ],
            },
            ContextId::RemoteBranches => CommandSection {
                title: "Remote Branches".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "View branch commits".into()),
                    CommandEntry::keybinding("<space>".into(), "Checkout as local branch".into()),
                    CommandEntry::keybinding(
                        kb.branches.merge_into_current_branch.clone(),
                        "Merge into current".into(),
                    ),
                    CommandEntry::keybinding(kb.branches.rebase_branch.clone(), "Rebase".into()),
                    CommandEntry::keybinding("d".into(), "Delete remote branch".into()),
                    CommandEntry::keybinding("<esc>".into(), "Back to remotes".into()),
                ],
            },
            ContextId::Tags => CommandSection {
                title: "Tags".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "View tag commits".into()),
                    CommandEntry::keybinding("n".into(), "Create tag".into()),
                    CommandEntry::keybinding("d".into(), "Delete tag".into()),
                    CommandEntry::keybinding("P".into(), "Push tag".into()),
                    CommandEntry::keybinding("g".into(), "Reset options".into()),
                ],
            },
            ContextId::Status => CommandSection {
                title: "Status".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "Recent repos".into()),
                    CommandEntry::keybinding("y".into(), "Copy to clipboard menu".into()),
                    CommandEntry::keybinding("o".into(), "Open in browser menu".into()),
                ],
            },
            _ => CommandSection {
                title: "Navigation".into(),
                entries: vec![
                    CommandEntry::keybinding("<enter>".into(), "Select / Open".into()),
                    CommandEntry::keybinding("<space>".into(), "Toggle / Confirm".into()),
                ],
            },
        };

        let sections = vec![context_section, universal];

        self.popup = PopupState::CommandPalette {
            sections,
            selected: 0,
            search_textarea: popup::make_command_palette_search_textarea(),
            scroll_offset: 0,
        };
    }

    pub(super) fn show_diff_command_palette(&mut self) {
        use crate::gui::context::ContextId;
        let grep_supported = self.diff_mode.active
            || matches!(
                self.context_mgr.active(),
                ContextId::Files
                    | ContextId::CommitFiles
                    | ContextId::StashFiles
                    | ContextId::BranchCommitFiles
            );
        let mut entries = vec![
            CommandEntry::keybinding("j/k".into(), "Scroll down / up".into()),
            CommandEntry::keybinding("h/l".into(), "Scroll left / right".into()),
            CommandEntry::keybinding(
                "{/}".into(),
                "Cycle prev / next hunk (selects revert block in Files)".into(),
            ),
            CommandEntry::keybinding("[".into(), "Toggle old-only view".into()),
            CommandEntry::keybinding("]".into(), "Toggle new-only view".into()),
            CommandEntry::keybinding(
                self.config
                    .user_config
                    .keybinding
                    .universal
                    .toggle_diff_view_layout
                    .clone(),
                "Toggle unified / side-by-side view".into(),
            ),
            CommandEntry::keybinding("z".into(), "Toggle line wrap".into()),
            CommandEntry::keybinding("g/G".into(), "Go to top / bottom".into()),
            CommandEntry::keybinding("PgUp/PgDn".into(), "Page up / down".into()),
            CommandEntry::keybinding("/".into(), "Search in diff".into()),
            CommandEntry::keybinding("n/N".into(), "Next / previous search match".into()),
        ];
        if grep_supported {
            entries.push(CommandEntry::keybinding(
                "<c-f>".into(),
                "Grep diff contents".into(),
            ));
        }
        entries.extend([
            CommandEntry::keybinding(
                "<enter>".into(),
                "Open hunk menu on selected block (Files)".into(),
            ),
            CommandEntry::keybinding(
                "click 󰧛".into(),
                "Click revert icon to revert that block".into(),
            ),
            CommandEntry::keybinding(
                "u".into(),
                if self.diff_view.revert_undo_stack.is_empty() {
                    "Undo last revert (nothing to undo)".into()
                } else {
                    format!(
                        "Undo last revert ({}/{})",
                        self.diff_view.revert_undo_stack.len(),
                        self.diff_view.revert_undo_high_water,
                    )
                },
            ),
            CommandEntry::keybinding("e".into(), "Edit file at line".into()),
            CommandEntry::keybinding("o".into(), "Open file in default program".into()),
            CommandEntry::keybinding("y".into(), "Copy selected text".into()),
            CommandEntry::keybinding("c".into(), "Create note on line".into()),
            CommandEntry::keybinding("t".into(), "Toggle notes visibility".into()),
            CommandEntry::keybinding("n/N".into(), "Cycle next / previous note".into()),
            CommandEntry::keybinding("e".into(), "Edit selected note (user notes only)".into()),
            CommandEntry::keybinding("d".into(), "Delete selected note".into()),
            CommandEntry::keybinding("S".into(), "Send selected note to AI".into()),
            CommandEntry::keybinding("r/R".into(), "Reset note status to New".into()),
            CommandEntry::keybinding("".into(), "Click to add note on a line".into()),
            CommandEntry::keybinding("q".into(), "Quit".into()),
            CommandEntry::keybinding("+/_".into(), "Enlarge / shrink panel".into()),
            CommandEntry::keybinding(";".into(), "Toggle command log".into()),
            CommandEntry::keybinding("1-5".into(), "Jump to sidebar panel".into()),
            CommandEntry::keybinding("esc".into(), "Return to sidebar".into()),
            CommandEntry::keybinding("?".into(), "Show command palette".into()),
            CommandEntry::action(
                "".into(),
                "Color theme...".into(),
                CommandAction::OpenThemePicker,
            ),
        ]);
        let diff_section = CommandSection {
            title: "Diff Viewer".into(),
            entries,
        };

        self.popup = PopupState::CommandPalette {
            sections: vec![diff_section],
            selected: 0,
            search_textarea: popup::make_command_palette_search_textarea(),
            scroll_offset: 0,
        };
    }

    pub(super) fn show_rebase_options_menu(
        &mut self,
        is_rebasing: bool,
        is_merging: bool,
        is_cherry_picking: bool,
    ) -> Result<()> {
        let mut items = Vec::new();

        if is_rebasing {
            items.push(popup::MenuItem {
                label: "Continue rebase".to_string(),
                description: "git rebase --continue".to_string(),
                key: Some("c".to_string()),
                action: Some(Box::new(|gui| {
                    gui.git.continue_rebase()?;
                    gui.needs_refresh = true;
                    Ok(())
                })),
            });
            items.push(popup::MenuItem {
                label: "Abort rebase".to_string(),
                description: "git rebase --abort".to_string(),
                key: Some("a".to_string()),
                action: Some(Box::new(|gui| {
                    gui.git.abort_rebase()?;
                    gui.needs_refresh = true;
                    Ok(())
                })),
            });
            items.push(popup::MenuItem {
                label: "Skip this commit".to_string(),
                description: "git rebase --skip".to_string(),
                key: Some("s".to_string()),
                action: Some(Box::new(|gui| {
                    gui.git.rebase_skip()?;
                    gui.needs_refresh = true;
                    Ok(())
                })),
            });
        }

        if is_merging {
            items.push(popup::MenuItem {
                label: "Abort merge".to_string(),
                description: "git merge --abort".to_string(),
                key: Some("a".to_string()),
                action: Some(Box::new(|gui| {
                    gui.git.abort_merge()?;
                    gui.needs_refresh = true;
                    Ok(())
                })),
            });
        }

        if is_cherry_picking {
            items.push(popup::MenuItem {
                label: "Continue cherry-pick".to_string(),
                description: "git cherry-pick --continue".to_string(),
                key: Some("c".to_string()),
                action: Some(Box::new(|gui| {
                    gui.git.continue_cherry_pick()?;
                    gui.needs_refresh = true;
                    Ok(())
                })),
            });
            items.push(popup::MenuItem {
                label: "Abort cherry-pick".to_string(),
                description: "git cherry-pick --abort".to_string(),
                key: Some("a".to_string()),
                action: Some(Box::new(|gui| {
                    gui.git.abort_cherry_pick()?;
                    gui.cherry_pick_clipboard.clear();
                    gui.needs_refresh = true;
                    Ok(())
                })),
            });
            items.push(popup::MenuItem {
                label: "Skip this commit".to_string(),
                description: "git cherry-pick --skip".to_string(),
                key: Some("s".to_string()),
                action: Some(Box::new(|gui| {
                    gui.git.skip_cherry_pick()?;
                    gui.needs_refresh = true;
                    Ok(())
                })),
            });
        }

        self.popup = PopupState::Menu {
            title: "Rebase/Merge/Cherry-pick options".to_string(),
            items,
            selected: 0,
            loading_index: None,
        };
        Ok(())
    }

    /// Show the commit menu from within the commit message editor (<c-o>).
    pub(super) fn show_commit_editor_menu(&mut self) -> Result<()> {
        // Stash the current commit editor popup
        let stashed = std::mem::replace(&mut self.popup, PopupState::None);
        self.pending_commit_popup = Some(stashed);

        let generate_cmd = self.config.user_config.git.commit.generate_command.clone();
        let has_generate = !generate_cmd.is_empty();

        let ai_label = if has_generate {
            format!("Generate w/ AI ({})", generate_cmd)
        } else {
            "Generate w/ AI (not configured)".to_string()
        };

        let mut items = vec![
            popup::MenuItem {
                label: "Open in editor".to_string(),
                description: String::new(),
                key: Some("e".to_string()),
                action: Some(Box::new(|gui| {
                    // Restore the stashed editor — user can continue typing
                    // TODO: full $EDITOR integration would suspend the TUI
                    if let Some(stashed) = gui.pending_commit_popup.take() {
                        gui.popup = stashed;
                    }
                    Ok(())
                })),
            },
            popup::MenuItem {
                label: "Add co-author".to_string(),
                description: String::new(),
                key: Some("c".to_string()),
                action: Some(Box::new(|gui| {
                    // Restore editor, then open a prompt for co-author
                    let stashed = gui.pending_commit_popup.take();
                    gui.popup = PopupState::Input {
                        title: "Co-author (Name <email>)".to_string(),
                        textarea: popup::make_textarea("Name <email@example.com>"),
                        on_confirm: Box::new(move |gui, coauthor| {
                            if let Some(mut editor) = stashed {
                                if !coauthor.is_empty() {
                                    // Append co-author trailer to the body
                                    if let PopupState::CommitInput {
                                        ref mut body_textarea,
                                        ref mut body_state,
                                        ..
                                    } = editor
                                    {
                                        // Move logical cursor to end before appending so the
                                        // trailer goes at the bottom no matter where the user
                                        // last clicked.
                                        body_state.cursor = body_state.raw().chars().count();
                                        body_state.insert_str(&format!(
                                            "\n\nCo-authored-by: {}",
                                            coauthor
                                        ));
                                        let wrap = gui.commit_body_wrap_width();
                                        body_state.render_into(body_textarea, wrap);
                                    }
                                }
                                gui.popup = editor;
                            }
                            Ok(())
                        }),
                        is_commit: false,
                        confirm_focused: false,
                    };
                    Ok(())
                })),
            },
            popup::MenuItem {
                label: "Paste commit message from clipboard".to_string(),
                description: String::new(),
                key: Some("p".to_string()),
                action: Some(Box::new(|gui| {
                    let clipboard_text = read_clipboard();
                    if let Some(mut editor) = gui.pending_commit_popup.take() {
                        if let Some(text) = clipboard_text
                            && !text.is_empty()
                            && let PopupState::CommitInput {
                                ref mut summary_textarea,
                                ref mut body_textarea,
                                ref mut body_state,
                                ..
                            } = editor
                        {
                            // Split pasted text: first line → summary, rest → body
                            let (summary, body) = match text.find('\n') {
                                Some(idx) => {
                                    let s = text[..idx].to_string();
                                    let b = text[idx + 1..].trim_start_matches('\n').to_string();
                                    (s, b)
                                }
                                None => (text.clone(), String::new()),
                            };
                            popup::set_commit_summary_text(summary_textarea, &summary);
                            // Preserve pasted newlines; soft-wrap is display-only.
                            body_state.set_text(popup::unwrap_commit_body(&body));
                            let wrap = gui.commit_body_wrap_width();
                            body_state.render_into(body_textarea, wrap);
                        }
                        gui.popup = editor;
                    }
                    Ok(())
                })),
            },
        ];

        items.push(popup::MenuItem {
            label: "Clear summary and description".to_string(),
            description: String::new(),
            key: Some("x".to_string()),
            action: Some(Box::new(|gui| {
                if let Some(mut editor) = gui.pending_commit_popup.take() {
                    if let PopupState::CommitInput {
                        ref mut summary_textarea,
                        ref mut body_textarea,
                        ref mut body_state,
                        ref mut focus,
                        ..
                    } = editor
                    {
                        popup::set_commit_summary_text(summary_textarea, "");
                        body_state.set_text(String::new());
                        let wrap = gui.commit_body_wrap_width();
                        body_state.render_into(body_textarea, wrap);
                        *focus = popup::CommitInputFocus::Summary;
                    }
                    gui.popup = editor;
                }
                Ok(())
            })),
        });

        if has_generate {
            items.push(popup::MenuItem {
                label: ai_label,
                description: String::new(),
                key: Some("g".to_string()),
                action: Some(Box::new(|gui| {
                    gui.begin_ai_commit_generation_ui();
                    Ok(())
                })),
            });
        } else {
            items.push(popup::MenuItem {
                label: ai_label,
                description: String::new(),
                key: Some("g".to_string()),
                action: None, // Disabled — no generateCommand configured
            });
        }

        self.popup = PopupState::Menu {
            title: "Commit menu".to_string(),
            items,
            selected: 0,
            loading_index: None,
        };
        Ok(())
    }

    pub(super) fn show_recent_repos(&mut self) -> Result<()> {
        let recent = self.config.app_state.recent_repos.clone();
        if recent.is_empty() {
            return Ok(());
        }

        let items: Vec<popup::MenuItem> = recent
            .into_iter()
            .map(|path| {
                let display = path.clone();
                let p = path.clone();
                popup::MenuItem {
                    label: display,
                    description: String::new(),
                    key: None,
                    action: Some(Box::new(move |gui| {
                        // Switch to the selected repo
                        let new_git = crate::git::GitCommands::new(std::path::Path::new(&p))?;
                        let new_model =
                            new_git.load_model(DEFAULT_COMMIT_LIMIT, gui.all_branches_log)?;
                        gui.git = std::sync::Arc::new(new_git);
                        *gui.model
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = new_model;
                        gui.commit_list_cache = presentation::commits::CommitListCache::default();
                        gui.commit_stats_cache
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .clear();
                        gui.commit_messages_cache
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .clear();
                        gui.clear_diff_preview_cache();
                        gui.last_commit_details_key.clear();
                        gui.commit_details_generation
                            .fetch_add(1, Ordering::Relaxed);
                        gui.last_diff_key.clear();
                        gui.diff_generation.fetch_add(1, Ordering::Relaxed);
                        gui.diff_loading = false;
                        gui.diff_loading_since = None;
                        gui.needs_refresh = false;
                        gui.needs_diff_refresh = true;
                        gui.context_mgr = context::ContextManager::new();
                        gui.displayed_diff_key.clear();
                        gui.diff_view.reset_keep_prefs();
                        if gui.show_file_tree {
                            gui.update_file_tree_state();
                        }
                        Ok(())
                    })),
                }
            })
            .collect();

        self.popup = PopupState::Menu {
            title: "Recent repos".to_string(),
            items,
            selected: 0,
            loading_index: None,
        };
        Ok(())
    }

    pub(super) fn undo(&mut self) -> Result<()> {
        // Get reflog entries
        let result = self
            .git
            .git_cmd()
            .args(&["reflog", "--format=%H", "-n", "20"])
            .run()?;
        if !result.success {
            return Ok(());
        }
        let entries: Vec<&str> = result.stdout.lines().collect();
        let next_idx = self.undo_reflog_idx + 1;
        if next_idx >= entries.len() {
            return Ok(()); // Nothing more to undo
        }

        let target_hash = entries[next_idx].to_string();
        let short = &target_hash[..7.min(target_hash.len())];

        self.popup = PopupState::Confirm {
            title: "Undo".to_string(),
            message: format!("Undo to reflog entry {}? ({})", next_idx, short),
            on_confirm: Box::new(move |gui| {
                gui.git.reset_to_commit(&target_hash, "--mixed")?;
                gui.undo_reflog_idx = next_idx;
                gui.needs_refresh = true;
                Ok(())
            }),
        };
        Ok(())
    }

    pub(super) fn redo(&mut self) -> Result<()> {
        if self.undo_reflog_idx == 0 {
            return Ok(()); // Nothing to redo
        }

        let result = self
            .git
            .git_cmd()
            .args(&["reflog", "--format=%H", "-n", "20"])
            .run()?;
        if !result.success {
            return Ok(());
        }
        let entries: Vec<&str> = result.stdout.lines().collect();
        let prev_idx = self.undo_reflog_idx - 1;
        if prev_idx >= entries.len() {
            return Ok(());
        }

        let target_hash = entries[prev_idx].to_string();
        let short = &target_hash[..7.min(target_hash.len())];

        self.popup = PopupState::Confirm {
            title: "Redo".to_string(),
            message: format!("Redo to reflog entry {}? ({})", prev_idx, short),
            on_confirm: Box::new(move |gui| {
                gui.git.reset_to_commit(&target_hash, "--mixed")?;
                gui.undo_reflog_idx = prev_idx;
                gui.needs_refresh = true;
                Ok(())
            }),
        };
        Ok(())
    }
}
