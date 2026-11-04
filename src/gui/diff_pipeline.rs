//! Diff request/response pipeline: scheduling, prefetch, caching, applying payloads, commit details.

use super::*;

impl Gui {
    /// Receive completed diff results from the background thread (non-blocking).
    pub(super) fn receive_diff_results(&mut self) {
        // Drain all available results, keeping only the latest valid one
        let current_gen = self.diff_generation.load(Ordering::Relaxed);
        while let Ok(result) = self.diff_rx.try_recv() {
            if result.is_prefetch {
                self.diff_prefetch_inflight.remove(&result.diff_key);
                if result.diff_key == self.last_diff_key && self.diff_loading {
                    // The user navigated onto this key while the prefetch was
                    // in flight and is waiting on it — apply it directly.
                    self.apply_diff_payload(result.diff_key, result.payload);
                } else if result.diff_key != self.last_diff_key
                    && result.diff_key != self.displayed_diff_key
                    && let DiffPayload::Parsed(parsed) = result.payload
                {
                    let mut view = DiffViewState::new();
                    view.wrap = self.diff_view.wrap;
                    view.view_layout = self.diff_view.view_layout;
                    view.apply_parsed(parsed);
                    self.diff_preview_cache.insert(result.diff_key, view);
                }
                continue;
            }
            // Discard stale results from older generations
            if result.generation != current_gen || result.diff_key != self.last_diff_key {
                continue;
            }
            self.apply_diff_payload(result.diff_key, result.payload);
        }
    }

    /// Swap a completed diff into the view, stashing the outgoing one for
    /// instant revisits.
    pub(super) fn apply_diff_payload(&mut self, diff_key: String, payload: DiffPayload) {
        self.diff_loading = false;
        self.diff_loading_since = None;
        if self.displayed_diff_key != diff_key {
            // The view still shows the previous selection — cache it before
            // overwriting. This also resets scroll/search for the new content.
            self.stash_displayed_diff();
        }
        match payload {
            DiffPayload::Content { filename, old, new } => {
                self.diff_view.load(&filename, &old, &new);
                self.diff_view.file_exists_on_disk = self.git.repo_path().join(&filename).exists();
                self.diff_view.load_notes(self.git.repo_path());
                self.displayed_diff_key = diff_key;
            }
            DiffPayload::UnifiedDiff {
                filename,
                diff_output,
            } => {
                self.diff_view
                    .load_from_diff_output(&filename, &diff_output);
                self.diff_view.file_exists_on_disk = self.git.repo_path().join(&filename).exists();
                self.diff_view.load_notes(self.git.repo_path());
                self.displayed_diff_key = diff_key;
            }
            DiffPayload::Parsed(parsed) => {
                self.diff_view.apply_parsed(parsed);
                self.diff_view.load_notes(self.git.repo_path());
                self.displayed_diff_key = diff_key;
            }
            DiffPayload::Empty => {
                self.diff_view.reset_keep_prefs();
                self.displayed_diff_key.clear();
            }
        }

        self.inject_hunk_comments();

        // Handle pending navigation after diff is loaded
        if let Some(nav) = self.pending_navigation.take()
            && let Some(line) = nav.line
        {
            // Note: `self.diff_view.load_notes` populated `self.diff_view.lines` with notes if they exist.
            // Let's find the matching note ID or just scroll to the line.
            // Scroll so the line is somewhat centered.
            let mut current_file = None;
            let content_offset = self.diff_view.lines.iter().position(|l| {
                if let Some(header) = &l.file_header {
                    current_file = Some(header.clone());
                }
                let file_matches = current_file.as_ref().is_none_or(|f| f == &nav.file_path);

                file_matches
                    && (l.new_line.as_ref().map(|(n, _)| *n) == Some(line)
                        || l.old_line.as_ref().map(|(n, _)| *n) == Some(line))
            });

            if let Some(offset) = content_offset {
                self.diff_view.scroll_offset = offset.saturating_sub(10);
            }

            // To select the note, we look for a note intersecting this line.
            // We can just search the loaded `lines_file.notes` in `notes_store`.
            let lines_file = crate::pager::notes_store::load(self.git.repo_path());
            if let Some(note) = lines_file
                .notes
                .iter()
                .find(|n| n.file == nav.file_path && n.line == line)
            {
                self.diff_view.selected_note = Some(note.id.clone());
            }
        }
    }

    pub(super) fn inject_hunk_comments(&mut self) {
        if self.diff_view.lines.is_empty() {
            return;
        }
        let target_path = self.git.repo_path().join(".hunks.json");
        if !target_path.exists() {
            return;
        }
        let content = match std::fs::read_to_string(&target_path) {
            Ok(c) => c,
            Err(_) => return,
        };
        let comments: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap_or_default();
        if comments.is_empty() {
            return;
        }

        let mut insertions = Vec::new();

        for i in 0..self.diff_view.hunk_starts.len() {
            let start = self.diff_view.hunk_starts[i];
            let end = if i + 1 < self.diff_view.hunk_starts.len() {
                self.diff_view.hunk_starts[i + 1]
            } else {
                self.diff_view.lines.len()
            };

            let mut hunk_text = String::new();
            for line in &self.diff_view.lines[start..end] {
                if let Some((_, ref old)) = line.old_line
                    && let Some((_, ref new)) = line.new_line
                    && old == new
                {
                    hunk_text.push_str("  ");
                    hunk_text.push_str(old);
                    hunk_text.push('\n');
                    continue;
                }
                if let Some((_, ref old)) = line.old_line {
                    hunk_text.push_str("- ");
                    hunk_text.push_str(old);
                    hunk_text.push('\n');
                } else if let Some((_, ref new)) = line.new_line {
                    hunk_text.push_str("+ ");
                    hunk_text.push_str(new);
                    hunk_text.push('\n');
                }
            }

            let file_path = self.diff_view.file_at_line(start).to_string();

            let hunk_comments: Vec<_> = comments
                .iter()
                .filter(|c| {
                    c["file"].as_str() == Some(&file_path) && c["hunk"].as_str() == Some(&hunk_text)
                })
                .collect();

            for comment in hunk_comments {
                if let Some(note) = comment["comment"].as_str() {
                    let is_old = comment["panel"].as_str() == Some("Old");
                    insertions.push((
                        start,
                        is_old,
                        note.to_string(),
                        self.diff_view.lines[start].section_index,
                    ));
                }
            }
        }

        if !insertions.is_empty() {
            insertions.sort_by_key(|k| k.0);
            for (idx, is_old, note, section_index) in insertions.into_iter().rev() {
                self.diff_view.lines.insert(
                    idx,
                    crate::pager::DiffLine {
                        old_line: None,
                        new_line: None,
                        change_type: crate::pager::ChangeType::Equal,
                        old_segments: None,
                        new_segments: None,
                        file_header: None,
                        comment_notes: vec![crate::pager::CommentNote::new_user(
                            String::new(),
                            note,
                            is_old,
                        )],
                        preview_placeholder: None,
                        section_index,
                    },
                );
            }
            self.diff_view.hunk_starts =
                crate::pager::diff_algo::find_hunk_starts(&self.diff_view.lines);
        }
    }

    pub(super) fn current_diff_key(&self) -> String {
        if self.diff_mode.active {
            return self.diff_mode.diff_key();
        }

        let active = self.context_mgr.active();
        let selected = self.context_mgr.selected_active();
        let model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match active {
            ContextId::Files => {
                if self.show_file_tree {
                    self.file_tree_nodes
                        .get(selected)
                        .map(|node| {
                            node.file_index
                                .and_then(|index| model.files.get(index))
                                .map(|file| format!("Files:file:{}", file.name))
                                .unwrap_or_else(|| format!("Files:dir:{}", node.path))
                        })
                        .unwrap_or_else(|| "Files:none".to_string())
                } else {
                    model
                        .files
                        .get(selected)
                        .map(|file| format!("Files:file:{}", file.name))
                        .unwrap_or_else(|| "Files:none".to_string())
                }
            }
            ContextId::Commits => model
                .commits
                .get(selected)
                .map(|commit| format!("Commits:{}", commit.hash))
                .unwrap_or_else(|| "Commits:none".to_string()),
            ContextId::Reflog => model
                .reflog_commits
                .get(selected)
                .map(|commit| format!("Reflog:{}", commit.hash))
                .unwrap_or_else(|| "Reflog:none".to_string()),
            ContextId::Stash => model
                .stash_entries
                .get(selected)
                .map(|entry| format!("Stash:{}", entry.hash))
                .unwrap_or_else(|| "Stash:none".to_string()),
            ContextId::BranchCommits => model
                .sub_commits
                .get(selected)
                .map(|commit| format!("BranchCommits:{}", commit.hash))
                .unwrap_or_else(|| "BranchCommits:none".to_string()),
            ContextId::CommitFiles | ContextId::StashFiles | ContextId::BranchCommitFiles => {
                let prefix = format!("{:?}:{}", active, self.commit_files_hash);
                if self.show_commit_file_tree {
                    self.commit_file_tree_nodes
                        .get(selected)
                        .map(|node| {
                            node.file_index
                                .and_then(|index| model.commit_files.get(index))
                                .map(|file| format!("{}:file:{}", prefix, file.name))
                                .unwrap_or_else(|| format!("{}:dir:{}", prefix, node.path))
                        })
                        .unwrap_or_else(|| format!("{}:none", prefix))
                } else {
                    model
                        .commit_files
                        .get(selected)
                        .map(|file| format!("{}:file:{}", prefix, file.name))
                        .unwrap_or_else(|| format!("{}:none", prefix))
                }
            }
            _ => format!("{:?}:{}", active, selected),
        }
    }

    pub(super) fn begin_diff_request(&mut self, diff_key: String) -> Option<u64> {
        if diff_key == self.last_diff_key && !self.needs_diff_refresh {
            return None;
        }

        let selection_changed = diff_key != self.last_diff_key;

        // Same-key refresh of a hash-keyed diff: the content cannot have
        // changed, so skip the reload when it's already on screen — and when
        // it's already loading, let the in-flight job finish instead of
        // bumping the generation (which would invalidate its result and
        // restart the race on every refresh tick).
        if !selection_changed
            && diff_key_is_immutable(&diff_key)
            && (self.diff_loading
                || (self.displayed_diff_key == diff_key && !self.diff_view.is_empty()))
        {
            self.needs_diff_refresh = false;
            return None;
        }

        self.last_diff_key = diff_key.clone();
        self.needs_diff_refresh = false;

        let generation = self.diff_generation.fetch_add(1, Ordering::Relaxed) + 1;
        if selection_changed {
            if let Some(mut cached) = self.diff_preview_cache.take(&diff_key) {
                cached.wrap = self.diff_view.wrap;
                cached.view_layout = self.diff_view.view_layout;
                self.stash_displayed_diff();
                self.diff_view = cached;
                self.displayed_diff_key = diff_key;
                self.diff_loading = false;
                self.diff_loading_since = None;
                return None;
            }
            // A prefetch for this key is already in flight — wait for its
            // result instead of spawning a duplicate load.
            if self.diff_prefetch_inflight.contains(&diff_key) {
                self.diff_loading = true;
                self.diff_loading_since = Some(Instant::now());
                return None;
            }
            // Cache miss: keep the outgoing diff on screen while the new one
            // loads. It moves into the revisit cache when the result arrives.
        }

        self.diff_loading = false;
        self.diff_loading_since = None;
        Some(generation)
    }

    /// Move the currently displayed diff into the revisit cache, leaving a
    /// fresh view (prefs preserved) in its place.
    pub(super) fn stash_displayed_diff(&mut self) {
        if self.diff_view.is_empty() || self.displayed_diff_key.is_empty() {
            self.displayed_diff_key.clear();
            return;
        }

        let mut replacement = DiffViewState::new();
        replacement.wrap = self.diff_view.wrap;
        replacement.view_layout = self.diff_view.view_layout;
        let view = std::mem::replace(&mut self.diff_view, replacement);
        self.diff_preview_cache
            .insert(std::mem::take(&mut self.displayed_diff_key), view);
    }

    /// Blank the diff pane (nothing selected / context without a diff),
    /// preserving the outgoing view for instant revisits.
    pub(crate) fn clear_diff_view(&mut self) {
        self.stash_displayed_diff();
        self.diff_view.reset_keep_prefs();
    }

    /// Speculatively warm the preview cache with diffs the user is likely to
    /// view next: neighbors of the selection in commit-like panels, and the
    /// Commits selection while another panel is focused (so switching to
    /// Commits shows its diff instantly). Commit/stash diffs are immutable,
    /// so warmed entries never go stale.
    pub(super) fn maybe_prefetch_diffs(&mut self) {
        if self.diff_mode.active || self.rebase_mode.active || self.patch_building.active {
            return;
        }
        if self.diff_prefetch_inflight.len() >= DIFF_PREFETCH_INFLIGHT_MAX {
            return;
        }

        let active = self.context_mgr.active();
        match active {
            ContextId::Commits
            | ContextId::Reflog
            | ContextId::BranchCommits
            | ContextId::Stash => {
                let selected = self.context_mgr.selected_active();
                for step in 1..=DIFF_PREFETCH_AHEAD {
                    self.maybe_prefetch_one(active, selected + step);
                }
                for step in 1..=DIFF_PREFETCH_BEHIND {
                    let Some(index) = selected.checked_sub(step) else {
                        break;
                    };
                    self.maybe_prefetch_one(active, index);
                }
            }
            _ => {
                self.maybe_prefetch_one(
                    ContextId::Commits,
                    self.context_mgr.selected(ContextId::Commits),
                );
            }
        }
    }

    pub(super) fn maybe_prefetch_one(&mut self, context: ContextId, index: usize) {
        if self.diff_prefetch_inflight.len() >= DIFF_PREFETCH_INFLIGHT_MAX {
            return;
        }
        let (diff_key, load): (String, Box<dyn FnOnce() -> DiffPayload + Send>) = {
            let model = self
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let git = Arc::clone(&self.git);
            match context {
                ContextId::Commits => {
                    let Some(commit) = model.commits.get(index) else {
                        return;
                    };
                    let hash = commit.hash.clone();
                    let key = format!("Commits:{}", hash);
                    (
                        key,
                        Box::new(move || commit_diff_payload(&git, &hash, "commit")),
                    )
                }
                ContextId::BranchCommits => {
                    let Some(commit) = model.sub_commits.get(index) else {
                        return;
                    };
                    let hash = commit.hash.clone();
                    let key = format!("BranchCommits:{}", hash);
                    (
                        key,
                        Box::new(move || commit_diff_payload(&git, &hash, "commit")),
                    )
                }
                ContextId::Reflog => {
                    let Some(commit) = model.reflog_commits.get(index) else {
                        return;
                    };
                    let hash = commit.hash.clone();
                    let key = format!("Reflog:{}", hash);
                    (
                        key,
                        Box::new(move || commit_diff_payload(&git, &hash, "reflog")),
                    )
                }
                ContextId::Stash => {
                    let Some(entry) = model.stash_entries.get(index) else {
                        return;
                    };
                    let key = format!("Stash:{}", entry.hash);
                    let stash_index = entry.index;
                    (key, Box::new(move || stash_diff_payload(&git, stash_index)))
                }
                _ => return,
            }
        };

        if diff_key == self.last_diff_key
            || diff_key == self.displayed_diff_key
            || self.diff_prefetch_inflight.contains(&diff_key)
            || self.diff_preview_cache.contains(&diff_key)
        {
            return;
        }
        self.diff_prefetch_inflight.insert(diff_key.clone());
        let _ = self
            .diff_prefetch_tx
            .send(DiffPrefetchJob { diff_key, load });
    }

    pub(super) fn clear_diff_preview_cache(&mut self) {
        self.diff_preview_cache.clear();
    }

    pub(super) fn maybe_request_commit_details(&mut self) {
        if !self.show_commit_details {
            if !self.last_commit_details_key.is_empty() {
                self.last_commit_details_key.clear();
                self.commit_details_generation
                    .fetch_add(1, Ordering::Relaxed);
            }
            return;
        }

        let active = self.context_mgr.active();
        let selected = self.context_mgr.selected_active();
        let hash = {
            let model = self
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match active {
                ContextId::Commits => model
                    .commits
                    .get(selected)
                    .map(|commit| commit.hash.clone()),
                ContextId::BranchCommits => model
                    .sub_commits
                    .get(selected)
                    .map(|commit| commit.hash.clone()),
                ContextId::Reflog => model
                    .reflog_commits
                    .get(selected)
                    .map(|commit| commit.hash.clone()),
                ContextId::CommitFiles | ContextId::StashFiles | ContextId::BranchCommitFiles => {
                    (!self.commit_files_hash.is_empty()).then(|| self.commit_files_hash.clone())
                }
                _ => None,
            }
        };
        let Some(hash) = hash else {
            if !self.last_commit_details_key.is_empty() {
                self.last_commit_details_key.clear();
                self.commit_details_generation
                    .fetch_add(1, Ordering::Relaxed);
            }
            return;
        };

        let details_key = format!("{:?}:{}", active, hash);
        if details_key == self.last_commit_details_key {
            return;
        }
        self.last_commit_details_key = details_key;
        let generation = self
            .commit_details_generation
            .fetch_add(1, Ordering::Relaxed)
            + 1;

        let stat_cached = self
            .commit_stats_cache
            .lock()
            .map(|cache| cache.contains_key(&hash))
            .unwrap_or(false);
        let message_cached = self
            .commit_messages_cache
            .lock()
            .map(|cache| cache.contains_key(&hash))
            .unwrap_or(false);
        if stat_cached && message_cached {
            return;
        }

        let git = Arc::clone(&self.git);
        let stat_cache = Arc::clone(&self.commit_stats_cache);
        let message_cache = Arc::clone(&self.commit_messages_cache);
        let generation_counter = Arc::clone(&self.commit_details_generation);
        let _ = self.commit_details_job_tx.send(Box::new(move || {
            if generation_counter.load(Ordering::Relaxed) != generation {
                return;
            }
            if !stat_cached
                && let Ok(stat) = git.commit_stat(&hash)
                && let Ok(mut cache) = stat_cache.lock()
            {
                cache.insert(hash.clone(), stat);
            }

            if generation_counter.load(Ordering::Relaxed) != generation {
                return;
            }
            if !message_cached
                && let Ok(message) = git.commit_message_full(&hash)
                && let Ok(mut cache) = message_cache.lock()
            {
                cache.insert(hash, message);
            }
        }));
    }

    pub(crate) fn queue_diff_job<F>(&self, generation: u64, diff_key: String, load: F)
    where
        F: FnOnce() -> DiffPayload + Send + 'static,
    {
        let _ = self
            .diff_scheduler_tx
            .send(DiffSchedulerEvent::Job(DiffJob {
                generation,
                diff_key,
                load: Box::new(load),
            }));
    }

    /// Request diff loading on a background thread if selection changed.
    pub(super) fn maybe_request_diff(&mut self) {
        // Rebase mode has no diff to load
        if self.rebase_mode.active {
            return;
        }

        // Diff mode has its own diff loading
        if self.diff_mode.active {
            let diff_key = self.current_diff_key();
            let Some(generation) = self.begin_diff_request(diff_key.clone()) else {
                return;
            };

            self.diff_loading = true;
            self.diff_loading_since = Some(Instant::now());

            controller::diff_mode::maybe_request_diff(self, generation, diff_key);
            return;
        }

        let active = self.context_mgr.active();
        let selected = self.context_mgr.selected_active();
        let diff_key = self.current_diff_key();
        let Some(generation) = self.begin_diff_request(diff_key.clone()) else {
            return;
        };

        let model = self
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match active {
            ContextId::Files => {
                // Files panel: load and parse async on background thread
                let file_idx = if self.show_file_tree {
                    self.file_tree_nodes
                        .get(selected)
                        .and_then(|n| n.file_index)
                } else {
                    Some(selected)
                };
                if let Some(file) = file_idx.and_then(|i| model.files.get(i)) {
                    let name = file.name.clone();
                    let current_path = file.current_path().to_string();
                    let diff_paths: Vec<String> =
                        file.diff_paths().into_iter().map(str::to_string).collect();
                    let has_staged = file.has_staged_changes;
                    let has_unstaged = file.has_unstaged_changes;
                    let tracked = file.tracked;
                    let conflicted = file.has_merge_conflicts;
                    drop(model);

                    let git = Arc::clone(&self.git);

                    self.diff_loading = true;
                    self.diff_loading_since = Some(Instant::now());
                    self.queue_diff_job(generation, diff_key, move || {
                        let path_refs: Vec<&str> = diff_paths.iter().map(String::as_str).collect();
                        use crate::pager::image_preview::ImageSource;
                        let old_path = path_refs[0];
                        let old = if !tracked {
                            ImageSource::Missing
                        } else {
                            ImageSource::Revision {
                                revision: if has_staged { "HEAD" } else { "" },
                                path: old_path,
                            }
                        };
                        let new = if has_staged && !has_unstaged {
                            ImageSource::Revision {
                                revision: "",
                                path: &current_path,
                            }
                        } else {
                            ImageSource::Worktree(&current_path)
                        };
                        if let Some(parsed) = (!conflicted)
                            .then(|| {
                                image_diff_payload(
                                    &git,
                                    &name,
                                    "",
                                    git.repo_path().join(&current_path).exists(),
                                    old,
                                    new,
                                )
                            })
                            .flatten()
                        {
                            return DiffPayload::Parsed(parsed);
                        }
                        // Single HEAD buffer with coherent line numbers;
                        // hunks are dimmed/tinted staged vs unstaged via
                        // overlap with the unstaged diff. Falls back to the
                        // unstaged-only view when HEAD is unavailable
                        // (e.g. unborn HEAD).
                        if has_unstaged && has_staged {
                            let exists = git.repo_path().join(&current_path).exists();
                            let unstaged = git.diff_file_paths(&path_refs).unwrap_or_default();
                            match git.diff_paths_vs_head(&path_refs) {
                                Ok(head) if !head.is_empty() => {
                                    let parsed = DiffViewState::parse_head_with_staged(
                                        &name, &head, &unstaged, 4, exists,
                                    );
                                    if parsed.lines.is_empty() {
                                        DiffPayload::Empty
                                    } else {
                                        DiffPayload::Parsed(parsed)
                                    }
                                }
                                _ => {
                                    let mut payload = parse_file_diff_payload(
                                        &git,
                                        &name,
                                        &current_path,
                                        &unstaged,
                                        exists,
                                        false,
                                    );
                                    if let DiffPayload::Parsed(ref mut parsed) = payload {
                                        parsed.hunk_staged = vec![false; parsed.hunk_starts.len()];
                                    }
                                    payload
                                }
                            }
                        } else {
                            let diff_result = if has_unstaged {
                                git.diff_file_paths(&path_refs)
                            } else if has_staged {
                                git.diff_file_staged_paths(&path_refs)
                            } else {
                                Ok(String::new())
                            };

                            let exists = git.repo_path().join(&current_path).exists();
                            match diff_result {
                                Ok(diff) if diff.is_empty() && !tracked => {
                                    // Untracked file: everything is unstaged.
                                    let parsed = if git.is_binary_path(&current_path) {
                                        DiffViewState::parse_diff_output(
                                            &current_path,
                                            &synthesize_binary_file_diff(&current_path),
                                            4,
                                            exists,
                                        )
                                    } else {
                                        match git.file_content(&current_path) {
                                            Ok(content) if !content.is_empty() => {
                                                DiffViewState::parse_content(
                                                    &current_path,
                                                    "",
                                                    &content,
                                                    4,
                                                    exists,
                                                )
                                            }
                                            _ => return DiffPayload::Empty,
                                        }
                                    };
                                    let mut parsed = parsed;
                                    parsed.hunk_staged = vec![false; parsed.hunk_starts.len()];
                                    DiffPayload::Parsed(parsed)
                                }
                                Ok(diff) if diff.is_empty() => DiffPayload::Empty,
                                Ok(diff) => {
                                    // Single-side view: classify every hunk
                                    // so the title counts + dimming + menu
                                    // stay correct even at 0-staged or
                                    // all-staged.
                                    let staged_only = has_staged && !has_unstaged;
                                    let mut payload = parse_file_diff_payload(
                                        &git,
                                        &name,
                                        &current_path,
                                        &diff,
                                        exists,
                                        staged_only,
                                    );
                                    if let DiffPayload::Parsed(ref mut parsed) = payload {
                                        parsed.hunk_staged =
                                            vec![staged_only; parsed.hunk_starts.len()];
                                    }
                                    payload
                                }
                                Err(_) => DiffPayload::Empty,
                            }
                        }
                    });
                } else if self.show_file_tree {
                    // Directory node: show combined diff of all child files (async)
                    if let Some(node) = self.file_tree_nodes.get(selected) {
                        if node.is_dir && !node.child_file_indices.is_empty() {
                            // One `git diff HEAD -- dir/` for tracked files under the
                            // directory; only untracked children still need synthesize.
                            let untracked: Vec<String> = node
                                .child_file_indices
                                .iter()
                                .filter_map(|&i| model.files.get(i))
                                .filter(|f| !f.tracked)
                                .map(|f| f.current_path().to_string())
                                .collect();
                            let excluded: std::collections::HashSet<String> = node
                                .child_file_indices
                                .iter()
                                .filter_map(|&i| model.files.get(i))
                                .filter(|f| f.has_merge_conflicts)
                                .flat_map(|f| f.diff_paths().into_iter().map(str::to_string))
                                .collect();
                            let pathspec = pathspec_for_tree_path(&node.path);
                            let dir_name = node.name.clone();
                            drop(model);

                            let git = Arc::clone(&self.git);
                            let gen_counter = Arc::clone(&self.diff_generation);

                            self.diff_loading = true;
                            self.diff_loading_since = Some(Instant::now());
                            self.queue_diff_job(generation, diff_key, move || {
                                if gen_counter.load(Ordering::Relaxed) != generation {
                                    return DiffPayload::Empty;
                                }
                                let paths: Vec<&str> = match pathspec.as_deref() {
                                    Some(p) => vec![p],
                                    None => Vec::new(),
                                };
                                // Directory hover: single HEAD buffer for all
                                // tracked files under the directory, with
                                // hunks classified staged/unstaged via
                                // overlap with the unstaged diff. Untracked
                                // children only exist unstaged. Hunk actions
                                // stay Cancel-only here (no single file).
                                let mut combined_diff =
                                    git.diff_paths_vs_head(&paths).unwrap_or_default();
                                let mut unstaged_combined =
                                    git.diff_file_paths(&paths).unwrap_or_default();
                                for path in &untracked {
                                    if gen_counter.load(Ordering::Relaxed) != generation {
                                        return DiffPayload::Empty;
                                    }
                                    let synth = if git.is_binary_path(path) {
                                        synthesize_binary_file_diff(path)
                                    } else {
                                        let content = git.file_content(path).unwrap_or_default();
                                        if content.is_empty() {
                                            continue;
                                        }
                                        synthesize_new_file_diff(path, &content)
                                    };
                                    if !combined_diff.is_empty() {
                                        combined_diff.push('\n');
                                    }
                                    combined_diff.push_str(&synth);
                                    // Mirror the synthesized section so the
                                    // untracked file classifies as unstaged.
                                    if !unstaged_combined.is_empty() {
                                        unstaged_combined.push('\n');
                                    }
                                    unstaged_combined.push_str(&synth);
                                }

                                if combined_diff.trim().is_empty() {
                                    DiffPayload::Empty
                                } else {
                                    use crate::pager::side_by_side::{
                                        DiffViewState as DVS, head_block_staged_flags,
                                    };
                                    let mut parsed =
                                        DVS::parse_diff_output(&dir_name, &combined_diff, 4, true);
                                    parsed.hunk_staged = head_block_staged_flags(
                                        &combined_diff,
                                        &unstaged_combined,
                                        4,
                                    );
                                    if parsed.hunk_staged.len() != parsed.hunk_starts.len() {
                                        parsed.hunk_staged = vec![false; parsed.hunk_starts.len()];
                                    }
                                    crate::pager::image_preview::attach_inline_image_previews(
                                        &mut parsed,
                                        git.repo_path(),
                                        &combined_diff,
                                        "HEAD",
                                        None,
                                        &excluded,
                                    );
                                    if parsed.lines.is_empty() {
                                        DiffPayload::Empty
                                    } else {
                                        DiffPayload::Parsed(parsed)
                                    }
                                }
                            });
                        } else {
                            drop(model);
                            self.clear_diff_view();
                        }
                    } else {
                        drop(model);
                        self.clear_diff_view();
                    }
                } else {
                    drop(model);
                    self.clear_diff_view();
                }
            }
            ContextId::Commits => {
                // Commits: load and parse async on background thread
                if let Some(commit) = model.commits.get(selected) {
                    let hash = commit.hash.clone();
                    drop(model);

                    let git = Arc::clone(&self.git);

                    self.diff_loading = true;
                    self.diff_loading_since = Some(Instant::now());
                    self.queue_diff_job(generation, diff_key, move || {
                        commit_diff_payload(&git, &hash, "commit")
                    });
                } else {
                    drop(model);
                    self.clear_diff_view();
                }
            }
            ContextId::Reflog => {
                // Reflog: load and parse commit diff async
                if let Some(commit) = model.reflog_commits.get(selected) {
                    let hash = commit.hash.clone();
                    drop(model);

                    let git = Arc::clone(&self.git);

                    self.diff_loading = true;
                    self.diff_loading_since = Some(Instant::now());
                    self.queue_diff_job(generation, diff_key, move || {
                        commit_diff_payload(&git, &hash, "reflog")
                    });
                } else {
                    drop(model);
                    self.clear_diff_view();
                }
            }
            ContextId::Stash => {
                // Stash: load and parse async
                if let Some(entry) = model.stash_entries.get(selected) {
                    let index = entry.index;
                    drop(model);

                    let git = Arc::clone(&self.git);

                    self.diff_loading = true;
                    self.diff_loading_since = Some(Instant::now());
                    self.queue_diff_job(generation, diff_key, move || {
                        stash_diff_payload(&git, index)
                    });
                } else {
                    drop(model);
                    self.clear_diff_view();
                }
            }
            ContextId::BranchCommits => {
                // BranchCommits: load and parse commit diff async
                if let Some(commit) = model.sub_commits.get(selected) {
                    let hash = commit.hash.clone();
                    drop(model);

                    let git = Arc::clone(&self.git);

                    self.diff_loading = true;
                    self.diff_loading_since = Some(Instant::now());
                    self.queue_diff_job(generation, diff_key, move || {
                        commit_diff_payload(&git, &hash, "commit")
                    });
                } else {
                    drop(model);
                    self.clear_diff_view();
                }
            }
            ContextId::CommitFiles | ContextId::StashFiles | ContextId::BranchCommitFiles => {
                // CommitFiles/StashFiles/BranchCommitFiles: load and parse diff async
                let file_idx = if self.show_commit_file_tree {
                    self.commit_file_tree_nodes
                        .get(selected)
                        .and_then(|n| n.file_index)
                } else {
                    Some(selected)
                };
                if let Some(commit_file) = file_idx.and_then(|i| model.commit_files.get(i)) {
                    let name = commit_file.name.clone();
                    let current_path = commit_file.current_path().to_string();
                    let hash = self.commit_files_hash.clone();
                    drop(model);

                    let git = Arc::clone(&self.git);

                    self.diff_loading = true;
                    self.diff_loading_since = Some(Instant::now());
                    self.queue_diff_job(generation, diff_key, move || {
                        if let Ok(diff) = git.diff_commit_file(&hash, &name) {
                            if diff.is_empty() {
                                DiffPayload::Empty
                            } else {
                                parse_commit_file_diff_payload(
                                    &git,
                                    &hash,
                                    &name,
                                    &current_path,
                                    &diff,
                                )
                            }
                        } else {
                            DiffPayload::Empty
                        }
                    });
                } else if self.show_commit_file_tree {
                    // Directory node in tree view: show combined diff of all child files
                    if let Some(node) = self.commit_file_tree_nodes.get(selected) {
                        if node.is_dir && !node.child_file_indices.is_empty() {
                            // Single pathspec-filtered `git show`/`git diff` — not N× per file.
                            let pathspec = pathspec_for_tree_path(&node.path);
                            let dir_name = node.name.clone();
                            let hash = self.commit_files_hash.clone();
                            drop(model);

                            let git = Arc::clone(&self.git);
                            let gen_counter = Arc::clone(&self.diff_generation);

                            self.diff_loading = true;
                            self.diff_loading_since = Some(Instant::now());
                            self.queue_diff_job(generation, diff_key, move || {
                                if gen_counter.load(Ordering::Relaxed) != generation {
                                    return DiffPayload::Empty;
                                }
                                let paths: Vec<&str> = match pathspec.as_deref() {
                                    Some(p) => vec![p],
                                    None => Vec::new(),
                                };
                                let combined_diff =
                                    git.diff_commit_paths(&hash, &paths).unwrap_or_default();
                                if combined_diff.is_empty() {
                                    DiffPayload::Empty
                                } else {
                                    let mut parsed = DiffViewState::parse_diff_output(
                                        &dir_name,
                                        &combined_diff,
                                        4,
                                        true,
                                    );
                                    let parent = format!("{hash}^1");
                                    crate::pager::image_preview::attach_inline_image_previews(
                                        &mut parsed,
                                        git.repo_path(),
                                        &combined_diff,
                                        &parent,
                                        Some(&hash),
                                        &HashSet::new(),
                                    );
                                    DiffPayload::Parsed(parsed)
                                }
                            });
                        } else {
                            drop(model);
                            self.clear_diff_view();
                        }
                    } else {
                        drop(model);
                        self.clear_diff_view();
                    }
                } else {
                    // No file selected — clear diff
                    drop(model);
                    self.clear_diff_view();
                }
            }
            _ => {
                drop(model);
                self.clear_diff_view();
            }
        }
    }
}
