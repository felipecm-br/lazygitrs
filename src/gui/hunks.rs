//! Hunk-level actions: revert-block clicks, hunk context menu, stage/unstage/revert hunks.

use super::*;

impl Gui {
    pub(super) fn try_handle_revert_block_click(
        &mut self,
        panel_rect: ratatui::layout::Rect,
        layout: DiffPanelLayout,
        col: u16,
        row: u16,
    ) -> bool {
        if self.diff_mode.active {
            return false;
        }
        let Some(hunk_idx) = self.revert_hunk_at_position(panel_rect, &layout, col, row) else {
            return false;
        };
        self.diff_view.selected_revert_hunk = Some(hunk_idx);
        self.show_hunk_context_menu(hunk_idx);
        true
    }

    /// Open the hunk action menu (shown when Enter is pressed on a selected
    /// or hovered revert hunk). Cancel is focused first so an accidental
    /// Enter doesn't revert anything.
    pub(super) fn show_hunk_context_menu(&mut self, hunk_idx: usize) {
        // Directory hovers have no single file to act on — Cancel only.
        let has_file = self.selected_file_index().is_some();
        // The Files pane shows unstaged + staged hunks in one buffer. Offer
        // the action matching the hunk the menu was opened on so the index
        // can't be applied to the wrong side of the index.
        let hunk_is_staged = has_file && self.diff_view.is_staged_hunk(hunk_idx);
        let mut items = vec![popup::MenuItem {
            label: "Cancel".to_string(),
            description: String::new(),
            key: None,
            // No-op: execute_menu_action already drops the menu popup
            // before invoking the action, so returning Ok leaves the
            // menu closed. Esc also closes the menu via the universal
            // menu Esc handler.
            action: Some(Box::new(|_gui| Ok(()))),
        }];
        if has_file && !hunk_is_staged {
            items.push(popup::MenuItem {
                label: "Stage hunk".to_string(),
                description: String::new(),
                key: Some("s".to_string()),
                action: Some(Box::new(move |gui| {
                    if let Err(err) = gui.stage_selected_file_hunk(hunk_idx) {
                        gui.popup = PopupState::Message {
                            title: "Stage hunk failed".to_string(),
                            message: format!("{}", err),
                            kind: MessageKind::Error,
                        };
                    }
                    Ok(())
                })),
            });
            items.push(popup::MenuItem {
                label: "Revert hunk".to_string(),
                description: String::new(),
                key: Some("r".to_string()),
                action: Some(Box::new(move |gui| {
                    if let Err(err) = gui.revert_selected_file_hunk(hunk_idx) {
                        gui.popup = PopupState::Message {
                            title: "Revert block failed".to_string(),
                            message: format!("{}", err),
                            kind: MessageKind::Error,
                        };
                    }
                    Ok(())
                })),
            });
        }
        if hunk_is_staged {
            items.push(popup::MenuItem {
                label: "Unstage hunk".to_string(),
                description: String::new(),
                key: Some("s".to_string()),
                action: Some(Box::new(move |gui| {
                    if let Err(err) = gui.unstage_selected_file_hunk(hunk_idx) {
                        gui.popup = PopupState::Message {
                            title: "Unstage hunk failed".to_string(),
                            message: format!("{}", err),
                            kind: MessageKind::Error,
                        };
                    }
                    Ok(())
                })),
            });
        }

        self.popup = PopupState::Menu {
            title: "Hunk".to_string(),
            items,
            selected: 0,
            loading_index: None,
        };
    }

    /// Match the viewed HEAD block against the blocks of a freshly fetched
    /// per-side diff, returning their slice ranges for patch building.
    /// Matching runs on the side both diffs share (worktree for unstaged,
    /// HEAD for staged) so staged edits elsewhere in the file can't shift
    /// the mapping. Sorted deepest-first so sequential applies don't shift
    /// the line numbers of pending ones.
    pub(super) fn matching_side_blocks(
        &self,
        hunk_idx: usize,
        side_diff: &str,
        new_side: bool,
    ) -> Vec<(Option<(usize, usize)>, Option<(usize, usize)>)> {
        use crate::pager::side_by_side::DiffViewState as DVS;
        let view_spans = DVS::block_spans(&self.diff_view.lines, &self.diff_view.hunk_line_offsets);
        let Some(view) = view_spans.get(hunk_idx) else {
            return Vec::new();
        };
        let mut matched: Vec<(usize, Option<(usize, usize)>, Option<(usize, usize)>)> =
            DVS::block_spans_for_diff(side_diff, 4)
                .into_iter()
                .filter(|s| view.overlaps(s, new_side))
                .map(|s| {
                    let anchor = s.old.map(|(lo, _)| lo).unwrap_or(s.old_point);
                    (anchor, s.old, s.new)
                })
                .collect();
        matched.sort_by_key(|m| std::cmp::Reverse(m.0));
        matched
            .into_iter()
            .map(|(_, old, new)| (old, new))
            .collect()
    }

    pub(super) fn stage_selected_file_hunk(&mut self, hunk_idx: usize) -> Result<()> {
        let Some(file_idx) = self.selected_file_index() else {
            return Ok(());
        };
        let model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(file) = model.files.get(file_idx).cloned() else {
            return Ok(());
        };
        drop(model);

        let path_refs: Vec<String> = file.diff_paths().into_iter().map(str::to_string).collect();
        let refs: Vec<&str> = path_refs.iter().map(String::as_str).collect();
        let diff = self.git.diff_file_paths(&refs)?;
        if diff.is_empty() {
            // Untracked / synthesized diffs have no unified diff to slice —
            // fall back to staging the whole file.
            if !file.tracked {
                self.git.stage_file(file.current_path())?;
                self.needs_files_refresh = true;
                self.needs_diff_refresh = true;
            }
            return Ok(());
        }

        // The view may be a HEAD buffer, so map the hunk onto the unstaged
        // diff's blocks via the shared worktree side.
        let targets = self.matching_side_blocks(hunk_idx, &diff, true);
        if targets.is_empty() {
            self.popup = PopupState::Message {
                title: "Stage hunk".to_string(),
                message: "That hunk moved — the diff was refreshed.".to_string(),
                kind: MessageKind::Info,
            };
            self.needs_diff_refresh = true;
            return Ok(());
        }
        for (want_old, want_new) in targets {
            if want_old.is_none() && want_new.is_none() {
                continue;
            }
            self.git
                .stage_visual_block(file.current_path(), &diff, want_old, want_new)?;
        }
        self.needs_files_refresh = true;
        self.needs_diff_refresh = true;
        Ok(())
    }

    pub(super) fn unstage_selected_file_hunk(&mut self, hunk_idx: usize) -> Result<()> {
        let Some(file_idx) = self.selected_file_index() else {
            return Ok(());
        };
        let model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(file) = model.files.get(file_idx).cloned() else {
            return Ok(());
        };
        drop(model);

        let path_refs: Vec<String> = file.diff_paths().into_iter().map(str::to_string).collect();
        let refs: Vec<&str> = path_refs.iter().map(String::as_str).collect();
        let diff = self.git.diff_file_staged_paths(&refs)?;
        if diff.is_empty() {
            return Ok(());
        }

        // The view may be a HEAD buffer, so map the hunk onto the staged
        // diff's blocks via the shared HEAD side.
        let targets = self.matching_side_blocks(hunk_idx, &diff, false);
        if targets.is_empty() {
            self.popup = PopupState::Message {
                title: "Unstage hunk".to_string(),
                message: "That hunk moved — the diff was refreshed.".to_string(),
                kind: MessageKind::Info,
            };
            self.needs_diff_refresh = true;
            return Ok(());
        }
        for (want_old, want_new) in targets {
            if want_old.is_none() && want_new.is_none() {
                continue;
            }
            self.git
                .unstage_visual_block(file.current_path(), &diff, want_old, want_new)?;
        }
        self.needs_files_refresh = true;
        self.needs_diff_refresh = true;
        Ok(())
    }

    pub(super) fn revert_selected_file_hunk(&mut self, hunk_idx: usize) -> Result<()> {
        let Some(file_idx) = self.selected_file_index() else {
            return Ok(());
        };

        let model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(file) = model.files.get(file_idx) else {
            return Ok(());
        };

        if !file.has_unstaged_changes {
            self.popup = PopupState::Message {
                title: "Revert block".to_string(),
                message: "Block revert is available only for unstaged changes.".to_string(),
                kind: MessageKind::Info,
            };
            return Ok(());
        }

        let file_name = file.name.clone();
        drop(model);

        let diff = self.git.diff_file(&file_name)?;
        if diff.is_empty() {
            return Ok(());
        }

        // The view may be a HEAD buffer, so map the hunk onto the unstaged
        // diff's blocks via the shared worktree side.
        let targets = self.matching_side_blocks(hunk_idx, &diff, true);
        if targets.is_empty() {
            self.popup = PopupState::Message {
                title: "Revert block".to_string(),
                message: "That hunk moved — the diff was refreshed.".to_string(),
                kind: MessageKind::Info,
            };
            self.needs_diff_refresh = true;
            return Ok(());
        }

        // Snapshot the working-tree file before reverting so the user can undo
        // (`u`) within this session. Only keep the snapshot if the revert
        // actually succeeds; otherwise we'd leak unrelated state into the stack.
        let abs_path = self.git.repo_path().join(&file_name);
        let pre_bytes = std::fs::read(&abs_path).ok();

        for (want_old, want_new) in targets {
            if want_old.is_none() && want_new.is_none() {
                continue;
            }
            self.git
                .revert_visual_block_in_worktree(&file_name, &diff, want_old, want_new)?;
        }

        if let Some(bytes) = pre_bytes {
            let stack = &mut self.diff_view.revert_undo_stack;
            if stack.len() >= crate::pager::side_by_side::REVERT_UNDO_STACK_CAP {
                stack.remove(0);
            }
            stack.push(crate::pager::side_by_side::RevertUndoEntry {
                file_path: file_name.clone(),
                pre_revert_bytes: bytes,
            });
            self.diff_view.revert_undo_high_water =
                self.diff_view.revert_undo_high_water.max(stack.len());
        }

        self.diff_view.selection = None;
        self.needs_files_refresh = true;
        self.needs_diff_refresh = true;
        Ok(())
    }

    pub(super) fn undo_last_revert_block(&mut self) -> Result<()> {
        let Some(entry) = self.diff_view.revert_undo_stack.pop() else {
            return Ok(());
        };
        let abs_path = self.git.repo_path().join(&entry.file_path);
        std::fs::write(&abs_path, &entry.pre_revert_bytes)
            .with_context(|| format!("failed to restore {}", entry.file_path))?;
        if self.diff_view.revert_undo_stack.is_empty() {
            self.diff_view.revert_undo_high_water = 0;
        }
        self.needs_files_refresh = true;
        self.needs_diff_refresh = true;
        Ok(())
    }
}
