//! Review notes and AI note dispatch.

use super::*;

impl Gui {
    /// If a click lands on a saved note box for the given line, return its id.
    pub(super) fn note_id_at_click(
        &self,
        line_idx: usize,
        panel: crate::pager::side_by_side::DiffPanel,
        row: u16,
        pl: &crate::pager::side_by_side::DiffPanelLayout,
    ) -> Option<String> {
        use crate::pager::side_by_side::{DiffPanel, DiffSideView, DiffViewLayout};

        let diff_line = self.diff_view.lines.get(line_idx)?;
        if diff_line.comment_notes.is_empty() {
            return None;
        }
        // Notes are not clickable when hidden
        if !self.diff_view.notes_visible {
            return None;
        }

        let panel_is_old = match panel {
            DiffPanel::Old => true,
            DiffPanel::New => false,
        };

        let content_width = pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
        let panel_width = pl.old_content_end_x.saturating_sub(pl.old_content_x) as usize;
        let right_content_width = pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;

        let mut acc = 0usize;
        for (offset, dl) in self.diff_view.lines[self.diff_view.scroll_offset..=line_idx]
            .iter()
            .enumerate()
        {
            let idx = self.diff_view.scroll_offset + offset;
            if idx == line_idx {
                break;
            }
            if self.diff_view.view_layout == DiffViewLayout::Unified
                || self.diff_view.side_view != DiffSideView::Both
            {
                acc += crate::pager::side_by_side::unified_line_visual_height(
                    dl,
                    content_width,
                    &self.diff_view,
                    idx,
                );
            } else {
                acc += crate::pager::side_by_side::line_visual_height(
                    dl,
                    panel_width,
                    right_content_width,
                    &self.diff_view,
                    idx,
                );
            }
        }

        let note_start_y = pl.inner_y + self.diff_view.sticky_row_height(pl) + acc as u16;
        let click_off = row.saturating_sub(note_start_y);

        // Iterate ALL notes in render order; each takes 5 rows.
        // Return the one whose block the click falls within AND matches the panel.
        for (i, note) in diff_line.comment_notes.iter().enumerate() {
            let block_start = (i * 5) as u16;
            let block_end = block_start + 5;
            if click_off >= block_start && click_off < block_end && note.is_old == panel_is_old {
                return Some(note.id.clone());
            }
        }

        None
    }

    pub(super) fn delete_note(&mut self, note_id: String) {
        let mut lines_file = crate::pager::notes_store::load(self.git.repo_path());
        let original_notes = lines_file.notes.clone();
        let deleted_note_opt = lines_file.notes.iter().find(|c| c.id == note_id).cloned();
        lines_file.notes.retain(|c| c.id != note_id);
        crate::pager::notes_store::save(self.git.repo_path(), lines_file.clone());
        self.diff_view.load_notes(self.git.repo_path());

        if let Some(deleted) = deleted_note_opt {
            let notes_on_same_line: Vec<&crate::pager::notes_store::LinesEntry> = original_notes
                .iter()
                .filter(|n| {
                    n.file == deleted.file && n.line == deleted.line && n.panel == deleted.panel
                })
                .collect();

            let mut found_next_on_same_line = None;
            if let Some(pos) = notes_on_same_line.iter().position(|n| n.id == deleted.id) {
                if pos + 1 < notes_on_same_line.len() {
                    found_next_on_same_line = Some(notes_on_same_line[pos + 1].id.clone());
                } else if pos > 0 {
                    found_next_on_same_line = Some(notes_on_same_line[pos - 1].id.clone());
                }
            }

            if found_next_on_same_line.is_some() {
                self.diff_view.selected_note = found_next_on_same_line;
                return;
            }

            let main_panel = self.compute_main_panel_rect();
            let pl = DiffPanelLayout::compute(main_panel, &self.diff_view);
            let content_width = pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
            let panel_width = pl.old_content_end_x.saturating_sub(pl.old_content_x) as usize;
            let right_content_width =
                pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
            let visible_height = pl.inner_end_y.saturating_sub(pl.inner_y) as usize;

            let start = self.diff_view.scroll_offset;
            let mut acc = 0usize;
            let mut visible_note_ids = HashSet::new();
            for idx in start..self.diff_view.lines.len() {
                if acc >= visible_height {
                    break;
                }
                let dl = &self.diff_view.lines[idx];
                for note in &dl.comment_notes {
                    visible_note_ids.insert(note.id.clone());
                }
                if self.diff_view.view_layout == crate::pager::side_by_side::DiffViewLayout::Unified
                    || self.diff_view.side_view != crate::pager::side_by_side::DiffSideView::Both
                {
                    acc += crate::pager::side_by_side::unified_line_visual_height(
                        dl,
                        content_width,
                        &self.diff_view,
                        idx,
                    );
                } else {
                    acc += crate::pager::side_by_side::line_visual_height(
                        dl,
                        panel_width,
                        right_content_width,
                        &self.diff_view,
                        idx,
                    );
                }
            }

            let candidate_notes: Vec<&crate::pager::notes_store::LinesEntry> = lines_file
                .notes
                .iter()
                .filter(|n| n.file == deleted.file)
                .collect();

            let visible_candidates: Vec<&crate::pager::notes_store::LinesEntry> = candidate_notes
                .iter()
                .filter(|n| visible_note_ids.contains(&n.id))
                .cloned()
                .collect();

            let subset = if !visible_candidates.is_empty() {
                visible_candidates
            } else {
                candidate_notes
            };

            let mut closest: Option<(&crate::pager::notes_store::LinesEntry, usize)> = None;
            for &note in &subset {
                let dist = (note.line as isize - deleted.line as isize).unsigned_abs();
                match closest {
                    None => {
                        closest = Some((note, dist));
                    }
                    Some((prev_note, prev_dist)) => {
                        // Nearer wins; on a tie prefer the deleted note's panel.
                        let same_panel_tiebreak = dist == prev_dist
                            && note.panel == deleted.panel
                            && prev_note.panel != deleted.panel;
                        if dist < prev_dist || same_panel_tiebreak {
                            closest = Some((note, dist));
                        }
                    }
                }
            }

            if let Some((note, _)) = closest {
                self.diff_view.selected_note = Some(note.id.clone());
            } else {
                self.diff_view.selected_note = None;
            }
        } else {
            self.diff_view.selected_note = None;
        }
    }

    pub(super) fn ensure_note_visible(&mut self, note_id: &str) -> bool {
        let main_panel = self.compute_main_panel_rect();
        let pl = DiffPanelLayout::compute(main_panel, &self.diff_view);
        let content_width = pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
        let panel_width = pl.old_content_end_x.saturating_sub(pl.old_content_x) as usize;
        let right_content_width = pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
        let visible_height = pl.inner_end_y.saturating_sub(pl.inner_y) as usize;

        let start = self.diff_view.scroll_offset;
        let mut acc = 0usize;
        let mut visible_note_ids = HashSet::new();
        for idx in start..self.diff_view.lines.len() {
            if acc >= visible_height {
                break;
            }
            let dl = &self.diff_view.lines[idx];
            for note in &dl.comment_notes {
                visible_note_ids.insert(note.id.clone());
            }
            if self.diff_view.view_layout == crate::pager::side_by_side::DiffViewLayout::Unified
                || self.diff_view.side_view != crate::pager::side_by_side::DiffSideView::Both
            {
                acc += crate::pager::side_by_side::unified_line_visual_height(
                    dl,
                    content_width,
                    &self.diff_view,
                    idx,
                );
            } else {
                acc += crate::pager::side_by_side::line_visual_height(
                    dl,
                    panel_width,
                    right_content_width,
                    &self.diff_view,
                    idx,
                );
            }
        }

        if visible_note_ids.contains(note_id) {
            return true;
        }

        // Scroll to make it visible
        let mut target_line_idx = None;
        for (i, dl) in self.diff_view.lines.iter().enumerate() {
            if dl.comment_notes.iter().any(|n| n.id == note_id) {
                target_line_idx = Some(i);
                break;
            }
        }

        if let Some(line_idx) = target_line_idx {
            if line_idx < self.diff_view.scroll_offset {
                self.diff_view.scroll_offset = line_idx;
            }

            let start = self.diff_view.scroll_offset.min(line_idx);
            let mut acc = 0usize;
            for (offset, dl) in self.diff_view.lines[start..=line_idx].iter().enumerate() {
                let idx = start + offset;
                if idx == line_idx {
                    break;
                }
                if self.diff_view.view_layout == crate::pager::side_by_side::DiffViewLayout::Unified
                    || self.diff_view.side_view != crate::pager::side_by_side::DiffSideView::Both
                {
                    acc += crate::pager::side_by_side::unified_line_visual_height(
                        dl,
                        content_width,
                        &self.diff_view,
                        idx,
                    );
                } else {
                    acc += crate::pager::side_by_side::line_visual_height(
                        dl,
                        panel_width,
                        right_content_width,
                        &self.diff_view,
                        idx,
                    );
                }
            }

            if acc >= visible_height {
                self.diff_view.scroll_offset = line_idx.saturating_sub(visible_height / 3);
            }
        }

        false
    }

    pub fn open_quick_ai_note_prompt(&mut self) {
        let ai_config = self.config.user_config.ai_notes.clone();
        if !ai_config.enabled {
            self.show_error(
                "AI Notes Not Configured",
                anyhow::anyhow!(
                    "Set `aiNotes.enabled` to true in config.yml to enable sending notes to an AI session."
                ),
            );
            return;
        }

        // Determine target file and line
        let (target_file, target_line, panel) = if self.diff_focused {
            if !self.diff_view.lines.is_empty() {
                let line_idx = if let Some((idx, _)) = self.diff_view.hovered_line {
                    idx
                } else {
                    self.diff_view
                        .scroll_offset
                        .min(self.diff_view.lines.len().saturating_sub(1))
                };
                let p = if let Some((_, panel)) = self.diff_view.hovered_line {
                    panel
                } else {
                    crate::pager::side_by_side::DiffPanel::New
                };
                let file = self.diff_view.file_at_line(line_idx).to_string();
                let line = self.diff_view.file_line_number(line_idx, p).unwrap_or(1);
                (file, line, p)
            } else if !self.diff_view.filename.is_empty() {
                (
                    self.diff_view.filename.clone(),
                    1,
                    crate::pager::side_by_side::DiffPanel::New,
                )
            } else {
                (
                    "".to_string(),
                    1,
                    crate::pager::side_by_side::DiffPanel::New,
                )
            }
        } else {
            match self.context_mgr.active() {
                ContextId::Files => {
                    let selected = self.context_mgr.selected(ContextId::Files);
                    let model = self
                        .model
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let file = model
                        .files
                        .get(selected)
                        .map(|f| f.name.clone())
                        .unwrap_or_default();
                    (file, 1, crate::pager::side_by_side::DiffPanel::New)
                }
                ContextId::CommitFiles | ContextId::BranchCommitFiles | ContextId::StashFiles => {
                    let selected = self.context_mgr.selected(self.context_mgr.active());
                    let model = self
                        .model
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let file = model
                        .commit_files
                        .get(selected)
                        .map(|f| f.name.clone())
                        .unwrap_or_default();
                    (file, 1, crate::pager::side_by_side::DiffPanel::New)
                }
                _ => (
                    "".to_string(),
                    1,
                    crate::pager::side_by_side::DiffPanel::New,
                ),
            }
        };

        if target_file.is_empty() {
            self.show_error(
                "Quick AI Note",
                anyhow::anyhow!("No file selected. Focus a file or diff line to send an AI note."),
            );
            return;
        }

        let panel_str = match panel {
            crate::pager::side_by_side::DiffPanel::Old => "Old",
            crate::pager::side_by_side::DiffPanel::New => "New",
        };

        let title = if target_line > 1 {
            format!("Quick Note for AI Agent [{}:{}]", target_file, target_line)
        } else {
            format!("Quick Note for AI Agent [{}]", target_file)
        };

        let file_path = target_file.clone();
        self.popup = PopupState::Input {
            title,
            textarea: crate::gui::popup::make_textarea(""),
            on_confirm: Box::new(move |gui, input| {
                let comment = input.trim().to_string();
                if comment.is_empty() {
                    return Ok(());
                }

                // Create and persist note
                let id = format!(
                    "{}-{}-{}-{}",
                    file_path,
                    target_line,
                    panel_str,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                );

                let mut lines_file = crate::pager::notes_store::load(gui.git.repo_path());
                lines_file
                    .notes
                    .push(crate::pager::notes_store::LinesEntry::new_user(
                        id.clone(),
                        file_path.clone(),
                        target_line,
                        panel_str,
                        comment,
                    ));
                crate::pager::notes_store::save(gui.git.repo_path(), lines_file);
                gui.diff_view.load_notes(gui.git.repo_path());

                // Dispatch immediately to AI session
                gui.notify_ai_for_note(id);

                gui.needs_refresh = true;
                Ok(())
            }),
            is_commit: false,
            confirm_focused: false,
        };
    }

    /// Send a user note to the AI session.
    ///
    /// Delivery priority:
    /// 1. **TUI push** — if the AI CLI registered a `serverUrl` (e.g.
    ///    opencode's `http://127.0.0.1:4096`), call `POST /tui/append-prompt`
    ///    + `POST /tui/submit-prompt` to inject the prompt inline.
    /// 2. **SSE** — push an event to any connected SSE clients.
    /// 3. **Spawn** — fall back to `notifyCommand` subprocess.
    ///
    /// After sending, the note's `status` is updated from `New` → `Sent`
    /// in `lines.json` so the TUI can show a visual indicator.
    pub(super) fn notify_ai_for_note(&mut self, note_id: String) {
        let ai_config = self.config.user_config.ai_notes.clone();
        if !ai_config.enabled {
            self.show_error(
                "AI Notes Not Configured",
                anyhow::anyhow!(
                    "Set `aiNotes.enabled` to true in config.yml to enable sending notes to an AI session."
                ),
            );
            return;
        }

        // Find the note and its file/line context.
        let mut note_info: Option<(String, usize, String)> = None;
        for (i, dl) in self.diff_view.lines.iter().enumerate() {
            if let Some(note) = dl.comment_notes.iter().find(|n| n.id == note_id) {
                let file = self.diff_view.file_at_line(i).to_string();
                let panel = if note.is_old {
                    crate::pager::side_by_side::DiffPanel::Old
                } else {
                    crate::pager::side_by_side::DiffPanel::New
                };
                let line = self.diff_view.file_line_number(i, panel).unwrap_or(0);
                note_info = Some((file, line, note.text.clone()));
                break;
            }
        }

        // If not found in current diff view lines, look up in lines.json
        if note_info.is_none() {
            let lines_file = crate::pager::notes_store::load(self.git.repo_path());
            if let Some(entry) = lines_file.notes.iter().find(|n| n.id == note_id) {
                note_info = Some((entry.file.clone(), entry.line, entry.comment.clone()));
            }
        }

        let (file, line, note_text) = match note_info {
            Some(info) => info,
            None => return,
        };

        // Update note status -> Sent.
        self.mark_note_sent(&note_id);

        // Build the prompt that tells the AI to check the notes endpoint.
        let prompt = format!(
            "A review note was created in lazygitrs on file '{}' at line {}:\n\n\"{}\"\n\n\
             Load the lazygitrs-review skill if available. \
             Fetch all notes with: PORT=$(cat \"$(git rev-parse --git-dir)/lazygitrs.port\") && curl -s http://127.0.0.1:$PORT/session-api/notes\n\
             Review the note above and respond by POSTing your annotations to \
             http://127.0.0.1:$PORT/session-api using the AgentContext JSON format:\n\
             {{\"version\":1,\"files\":[{{\"path\":\"...\",\"annotations\":[{{\"summary\":\"...\",\"rationale\":\"...\",\"newRange\":[L,L]}}]}}]}}",
            file, line, note_text
        );

        self.dispatch_prompt_to_ai(prompt, note_id, file, line, note_text);
    }

    pub(super) fn dispatch_prompt_to_ai(
        &self,
        prompt: String,
        note_id: String,
        file: String,
        line: usize,
        note_text: String,
    ) {
        let repo_path = self.git.repo_path().to_path_buf();
        let acp_server_url = Arc::clone(&self.acp.server_url);
        let acp_notify_command = Arc::clone(&self.acp.notify_command);
        let acp_session_id = Arc::clone(&self.acp.session_id);
        let sse_tx = self.acp.sse_tx.clone();

        std::thread::spawn(move || {
            // 1. Try TUI push (opencode /tui/append-prompt + /tui/submit-prompt).
            let server_url = acp_server_url
                .lock()
                .map(|g| g.clone())
                .unwrap_or(None)
                .or_else(|| {
                    crate::pager::notes_store::load(&repo_path)
                        .session
                        .filter(|s| !s.server_url.is_empty())
                        .map(|s| s.server_url)
                });

            if let Some(ref url) = server_url
                && Self::push_prompt_to_tui(url, &prompt)
            {
                return;
            }

            // 2. Try SSE — if there are connected listeners, push the event.
            let sse_payload = serde_json::json!({
                "type": "note-sent",
                "noteId": note_id,
                "file": file,
                "line": line,
                "note": note_text,
                "prompt": prompt,
            })
            .to_string();

            let sse_delivered = sse_tx.receiver_count() > 0 && sse_tx.send(sse_payload).is_ok();

            if sse_delivered {
                return;
            }

            // 3. Fallback: spawn the notifyCommand if configured by the active session.
            let notify_command = acp_notify_command
                .lock()
                .map(|g| g.clone())
                .unwrap_or(None)
                .or_else(|| crate::pager::notes_store::trusted_notify_command(&repo_path))
                .or_else(|| {
                    if let Ok(home) = std::env::var("HOME") {
                        let global_path =
                            std::path::PathBuf::from(home).join(".lazygitrs_active_session.json");
                        if let Ok(data) = std::fs::read_to_string(&global_path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&data)
                            && let Some(cmd) = json.get("notifyCommand").and_then(|v| v.as_str())
                            && !cmd.is_empty()
                        {
                            return Some(cmd.to_string());
                        }
                    }
                    if let Ok(home) = std::env::var("HOME") {
                        let script =
                            std::path::PathBuf::from(home).join(".local/bin/agent-note.sh");
                        if script.exists() {
                            return Some(format!(
                                "\"{}\" {{{{workspace_path}}}} {{{{prompt}}}}",
                                script.display()
                            ));
                        }
                    }
                    None
                });

            let cmd_template = match notify_command {
                Some(cmd) if !cmd.is_empty() => cmd,
                _ => return,
            };

            // Expand the command template.
            let session_id = acp_session_id
                .lock()
                .map(|g| g.clone())
                .unwrap_or(None)
                .or_else(|| {
                    crate::pager::notes_store::load(&repo_path)
                        .session
                        .map(|s| s.session_id)
                })
                .or_else(|| {
                    if let Ok(home) = std::env::var("HOME") {
                        let global_path =
                            std::path::PathBuf::from(home).join(".lazygitrs_active_session.json");
                        if let Ok(data) = std::fs::read_to_string(&global_path)
                            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&data)
                            && let Some(sid) = json.get("sessionId").and_then(|v| v.as_str())
                            && !sid.is_empty()
                        {
                            return Some(sid.to_string());
                        }
                    }
                    None
                })
                .unwrap_or_default();
            let workspace_path = repo_path.to_string_lossy().to_string();
            let cmd_str = cmd_template
                .replace("{{session_id}}", &shell_escape_arg(&session_id))
                .replace("{{workspace_path}}", &shell_escape_arg(&workspace_path))
                .replace("{{prompt}}", &shell_escape_arg(&prompt));

            crate::os::cmd::log_command(&cmd_str);
            let log_path = crate::pager::notes_store::notify_log_path(&repo_path);
            let _ = match std::fs::File::create(&log_path) {
                Ok(log_file) => {
                    let stderr_file = log_file.try_clone().unwrap_or_else(|_| {
                        std::fs::OpenOptions::new()
                            .write(true)
                            .open("/dev/null")
                            .unwrap()
                    });
                    std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&cmd_str)
                        .stdout(std::process::Stdio::from(log_file))
                        .stderr(std::process::Stdio::from(stderr_file))
                        .spawn()
                }
                Err(_) => std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd_str)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn(),
            };
        });
    }

    /// Mark all new review notes as `Sent` in `lines.json`, format a consolidated review prompt,
    /// and dispatch to the active AI session.
    pub(super) fn notify_ai_for_all_notes(&mut self) {
        let ai_config = self.config.user_config.ai_notes.clone();
        if !ai_config.enabled {
            self.show_error(
                "AI Notes Not Configured",
                anyhow::anyhow!(
                    "Set `aiNotes.enabled` to true in config.yml to enable sending notes to an AI session."
                ),
            );
            return;
        }

        let mut lines_file = crate::pager::notes_store::load(self.git.repo_path());
        let pending_notes: Vec<_> = lines_file
            .notes
            .iter()
            .filter(|n| n.status == crate::pager::NoteStatus::New)
            .cloned()
            .collect();

        if pending_notes.is_empty() {
            self.popup = PopupState::Message {
                title: "No New Notes".to_string(),
                message: "No pending review notes found to dispatch. All notes are already marked Sent or Resolved.".to_string(),
                kind: crate::gui::popup::MessageKind::Info,
            };
            return;
        }

        let count = pending_notes.len();
        let mut prompt = format!(
            "Please review and fix the following {} code review comment{} from lazygitrs:\n\n",
            count,
            if count == 1 { "" } else { "s" }
        );

        for (idx, note) in pending_notes.iter().enumerate() {
            prompt.push_str(&format!(
                "{}. **`{}`** (line {}):\n   > {}\n\n",
                idx + 1,
                note.file,
                note.line,
                note.comment
            ));
        }

        prompt.push_str(
            "Once addressed, run the relevant verification tests and summarize the changes made.",
        );

        // Mark all as sent in lines_file
        for note in &mut lines_file.notes {
            if note.status == crate::pager::NoteStatus::New {
                note.status = crate::pager::NoteStatus::Sent;
            }
        }
        crate::pager::notes_store::save(self.git.repo_path(), lines_file);
        self.diff_view.load_notes(self.git.repo_path());
        self.needs_refresh = true;

        self.dispatch_prompt_to_ai(
            prompt,
            "all".to_string(),
            "multiple files".to_string(),
            0,
            format!("{} review notes", count),
        );

        self.popup = PopupState::Message {
            title: "Review Notes Dispatched".to_string(),
            message: format!(
                "Successfully dispatched {} review note{} to the active AI agent.",
                count,
                if count == 1 { "" } else { "s" }
            ),
            kind: crate::gui::popup::MessageKind::Info,
        };
    }

    /// Push a prompt to the AI CLI's TUI via its HTTP server.
    /// Returns `true` on success.
    pub(super) fn push_prompt_to_tui(server_url: &str, prompt: &str) -> bool {
        let append_url = format!("{}/tui/append-prompt", server_url);
        let submit_url = format!("{}/tui/submit-prompt", server_url);

        let append_body = serde_json::json!({"text": prompt}).to_string();

        // Append the prompt text.
        let append_resp = std::process::Command::new("curl")
            .args([
                "-s",
                "-X",
                "POST",
                &append_url,
                "-H",
                "content-type: application/json",
                "--data",
                &append_body,
                "--max-time",
                "3",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        if !matches!(append_resp, Ok(s) if s.success()) {
            return false;
        }

        // Submit the prompt.
        let submit_resp = std::process::Command::new("curl")
            .args(["-s", "-X", "POST", &submit_url, "--max-time", "3"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        matches!(submit_resp, Ok(s) if s.success())
    }

    /// Mark a note as `Sent` in `lines.json` and reload the diff view.
    pub(super) fn mark_note_sent(&mut self, note_id: &str) {
        let mut lines_file = crate::pager::notes_store::load(self.git.repo_path());
        if let Some(entry) = lines_file.notes.iter_mut().find(|c| c.id == note_id) {
            entry.status = crate::pager::NoteStatus::Sent;
        }
        crate::pager::notes_store::save(self.git.repo_path(), lines_file);
        self.diff_view.load_notes(self.git.repo_path());
    }

    /// Reset a note's status back to `New` in `lines.json` and reload the diff view.
    pub(super) fn reset_note_status(&mut self, note_id: &str) {
        let mut lines_file = crate::pager::notes_store::load(self.git.repo_path());
        if let Some(entry) = lines_file.notes.iter_mut().find(|c| c.id == note_id) {
            entry.status = crate::pager::NoteStatus::New;
        }
        crate::pager::notes_store::save(self.git.repo_path(), lines_file);
        self.diff_view.load_notes(self.git.repo_path());
    }

    pub(super) fn cycle_note(&mut self, forward: bool) {
        // Collect all notes in render order: (line_idx, note_id)
        let mut all_notes: Vec<(usize, String)> = Vec::new();
        for (i, dl) in self.diff_view.lines.iter().enumerate() {
            for n in &dl.comment_notes {
                all_notes.push((i, n.id.clone()));
            }
        }
        if all_notes.is_empty() {
            return;
        }

        let current = self.diff_view.selected_note.as_deref();
        let next = if let Some(cur) = current {
            let pos = all_notes.iter().position(|(_, id)| id == cur);
            if forward {
                pos.and_then(|p| {
                    let np = (p + 1) % all_notes.len();
                    all_notes.get(np).cloned()
                })
            } else {
                pos.and_then(|p| {
                    let np = if p == 0 { all_notes.len() - 1 } else { p - 1 };
                    all_notes.get(np).cloned()
                })
            }
        } else if forward {
            Some(all_notes[0].clone())
        } else {
            Some(all_notes.last().cloned().unwrap())
        };

        if let Some((line_idx, note_id)) = next {
            // If the target note is above the current scroll offset,
            // scroll up to it first to avoid an invalid slice range.
            if line_idx < self.diff_view.scroll_offset {
                self.diff_view.scroll_offset = line_idx;
            }

            // Scroll to make the note visible.
            let main_panel = self.compute_main_panel_rect();
            let pl = DiffPanelLayout::compute(main_panel, &self.diff_view);
            let content_width = pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
            let panel_width = pl.old_content_end_x.saturating_sub(pl.old_content_x) as usize;
            let right_content_width =
                pl.new_content_end_x.saturating_sub(pl.new_content_x) as usize;
            let visible_height = pl.inner_end_y.saturating_sub(pl.inner_y) as usize;

            let start = self.diff_view.scroll_offset.min(line_idx);
            let mut acc = 0usize;
            for (offset, dl) in self.diff_view.lines[start..=line_idx].iter().enumerate() {
                let idx = start + offset;
                if idx == line_idx {
                    break;
                }
                if self.diff_view.view_layout == crate::pager::side_by_side::DiffViewLayout::Unified
                    || self.diff_view.side_view != crate::pager::side_by_side::DiffSideView::Both
                {
                    acc += crate::pager::side_by_side::unified_line_visual_height(
                        dl,
                        content_width,
                        &self.diff_view,
                        idx,
                    );
                } else {
                    acc += crate::pager::side_by_side::line_visual_height(
                        dl,
                        panel_width,
                        right_content_width,
                        &self.diff_view,
                        idx,
                    );
                }
            }

            if acc >= visible_height {
                // Note is below viewport — scroll up so it becomes visible.
                self.diff_view.scroll_offset = line_idx.saturating_sub(visible_height / 3);
            }

            self.diff_view.selected_note = Some(note_id);
            self.diff_focused = true;
        }
    }

    pub(super) fn open_inline_note_editor(
        &mut self,
        line_idx: usize,
        panel: crate::pager::side_by_side::DiffPanel,
        editing_id: Option<String>,
    ) {
        let file_path = self.diff_view.file_at_line(line_idx).to_string();
        let line_num = self
            .diff_view
            .file_line_number(line_idx, panel)
            .unwrap_or(0);

        let title = format!("Draft Note - {} - {}", file_path, line_num);
        let mut textarea = crate::gui::popup::make_textarea(&title);
        textarea.set_cursor_line_style(ratatui::style::Style::default());

        let editing_id = editing_id.unwrap_or_default();

        // Load existing note text if editing.
        if !editing_id.is_empty() {
            let lines_file = crate::pager::notes_store::load(self.git.repo_path());
            if let Some(note) = lines_file.notes.iter().find(|c| c.id == editing_id) {
                textarea.insert_str(&note.comment);
            }
        }

        self.diff_view.inline_edit = Some(crate::pager::side_by_side::InlineEdit {
            line_idx,
            panel,
            textarea,
            editing_id,
        });
        self.diff_focused = true;
    }
}
