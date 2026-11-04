//! `DiffViewState` behaviour: loading diffs, navigation, hunks, selection, search, staging state.

use super::*;

impl DiffViewState {
    pub fn new() -> Self {
        Self {
            tab_width: 4,
            ..Default::default()
        }
    }

    /// Reset to a fresh state while keeping user preferences (`wrap`) that
    /// should survive file/commit navigation. Without this, every reassignment
    /// of `diff_view = DiffViewState::new()` would clobber the wrap setting
    /// loaded from `state.yml`.
    pub fn reset_keep_prefs(&mut self) {
        let wrap = self.wrap;
        let view_layout = self.view_layout;
        *self = Self::new();
        self.wrap = wrap;
        self.view_layout = view_layout;
    }

    pub fn toggle_view_layout(&mut self) {
        // Convert scroll between DiffLine-index (split) and visual-row (unified)
        // so the same content stays near the top after toggling.
        let width = self.last_content_width.max(1);
        if self.view_layout == DiffViewLayout::SideBySide {
            let line = self.scroll_offset.min(self.lines.len().saturating_sub(1));
            self.view_layout = DiffViewLayout::Unified;
            self.scroll_offset = self.unified_visual_row_for_line(line, width);
        } else {
            let line = self.current_scroll_line();
            self.view_layout = DiffViewLayout::SideBySide;
            self.scroll_offset = line;
        }
        self.horizontal_scroll = 0;
        self.selection = None;
    }

    /// Get the actual file line number for a DiffLine, applying hunk offsets.
    /// Returns the display/file line number (e.g. for gutter or editAtLine).
    pub fn file_line_number(&self, line_idx: usize, panel: DiffPanel) -> Option<usize> {
        let dl = self.lines.get(line_idx)?;
        let content_num = match panel {
            DiffPanel::Old => dl.old_line.as_ref()?.0,
            DiffPanel::New => dl.new_line.as_ref()?.0,
        };
        let offset = self
            .hunk_line_offsets
            .iter()
            .rev()
            .find(|(start_idx, _, _)| *start_idx <= line_idx)
            .map(|(_, old_off, new_off)| match panel {
                DiffPanel::Old => *old_off,
                DiffPanel::New => *new_off,
            })
            .unwrap_or(0);
        Some(content_num + offset)
    }

    /// Get the filename that a given line index belongs to.
    /// For single-file diffs, returns `self.filename`.
    /// For multi-file diffs, walks backwards to find the nearest file header.
    pub fn file_at_line(&self, line_idx: usize) -> &str {
        for i in (0..=line_idx).rev() {
            if let Some(header) = self.lines.get(i).and_then(|l| l.file_header.as_ref()) {
                if header.starts_with("diff --git ") {
                    let parts: Vec<&str> = header.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let path2 = parts[3];
                        if path2.len() > 2 && path2.as_bytes()[1] == b'/' {
                            return &path2[2..];
                        }
                        return path2;
                    }
                }
                return header;
            }
        }
        &self.filename
    }

    /// True when this diff contains per-file separator lines (multi-file diff).
    pub fn has_file_headers(&self) -> bool {
        self.lines.iter().any(|l| l.file_header.is_some())
    }

    pub(super) fn header_idx_at_or_before(&self, line_idx: usize) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let clamped = line_idx.min(self.lines.len().saturating_sub(1));
        (0..=clamped).rev().find(|&i| {
            self.lines
                .get(i)
                .and_then(|l| l.file_header.as_ref())
                .is_some()
        })
    }

    /// Sticky header for a given DiffLine index.
    /// Returns `(header_line_idx, header_text)` when `line_idx` is content
    /// scrolled past its file header (so the header is no longer visible).
    /// Returns `None` when the top row already is a header or when this
    /// is a single-file diff with no separators.
    pub fn sticky_header_for_line(&self, line_idx: usize) -> Option<(usize, &str)> {
        if !self.has_file_headers() {
            return None;
        }
        let line = self.lines.get(line_idx)?;
        if line.file_header.is_some() {
            return None;
        }
        let h_idx = self.header_idx_at_or_before(line_idx)?;
        let header = self.lines.get(h_idx)?.file_header.as_ref()?;
        Some((h_idx, header.as_str()))
    }

    pub(super) fn sticky_for_unified_with_width(
        &self,
        content_width: usize,
    ) -> Option<(usize, &str)> {
        let w = content_width.max(1);
        let (line_idx, _, _) = self.unified_line_at_visual_row(self.scroll_offset, w)?;
        self.sticky_header_for_line(line_idx)
    }

    /// Header text to pin to the first row of the viewport, if any.
    /// Split layouts use `scroll_offset` directly; unified maps the visual
    /// scroll row back to a DiffLine first.
    pub fn sticky_file_header(&self) -> Option<&str> {
        if self.view_layout == DiffViewLayout::Unified {
            let w = self.last_content_width.max(1);
            self.sticky_for_unified_with_width(w).map(|(_, h)| h)
        } else {
            self.sticky_header_for_line(self.scroll_offset)
                .map(|(_, h)| h)
        }
    }

    /// DiffLine index of the header currently pinned by [`Self::sticky_file_header`].
    pub fn sticky_header_line_idx(&self) -> Option<usize> {
        if self.view_layout == DiffViewLayout::Unified {
            let w = self.last_content_width.max(1);
            self.sticky_for_unified_with_width(w).map(|(idx, _)| idx)
        } else {
            self.sticky_header_for_line(self.scroll_offset)
                .map(|(idx, _)| idx)
        }
    }

    /// True when `row` is the pinned sticky header row for this layout.
    /// Used to skip highlight/selection work on the pinned row and to
    /// adjust row -> line mapping for the reserved top row.
    pub fn is_sticky_row(&self, row: u16, layout: &DiffPanelLayout) -> bool {
        if row != layout.inner_y {
            return false;
        }
        if self.view_layout == DiffViewLayout::Unified {
            let content_width = layout
                .new_content_end_x
                .saturating_sub(layout.new_content_x) as usize;
            self.sticky_for_unified_with_width(content_width.max(1))
                .is_some()
        } else {
            self.sticky_header_for_line(self.scroll_offset).is_some()
        }
    }

    /// Rows reserved at the top of the viewport by the pinned sticky header
    /// (0 or 1), for callers that sum line heights from `inner_y`.
    pub fn sticky_row_height(&self, layout: &DiffPanelLayout) -> u16 {
        u16::from(self.is_sticky_row(layout.inner_y, layout))
    }

    /// Best-effort DiffLine index for a terminal row when hit-testing fails
    /// (past end of diff). Mirrors [`Self::line_chunk_at_row`] including the
    /// reserved sticky row so callers don't drift by one.
    pub fn fallback_line_idx_for_row(&self, row: u16, layout: &DiffPanelLayout) -> usize {
        if row < layout.inner_y {
            return 0;
        }
        let raw_off = (row - layout.inner_y) as usize;
        if self.view_layout == DiffViewLayout::Unified {
            let content_width = layout
                .new_content_end_x
                .saturating_sub(layout.new_content_x) as usize;
            let sticky = self
                .sticky_for_unified_with_width(content_width.max(1))
                .is_some();
            let adj = raw_off.saturating_sub(if sticky { 1 } else { 0 });
            // Map the adjusted visual offset precisely instead of guessing.
            if let Some((line_idx, _, _)) =
                self.unified_line_chunk_panel_from(self.scroll_offset, adj, content_width.max(1))
            {
                return line_idx;
            }
            return self.lines.len();
        }
        let sticky = self.sticky_header_for_line(self.scroll_offset).is_some();
        let adj = raw_off.saturating_sub(if sticky { 1 } else { 0 });
        if sticky && raw_off == 0 {
            return self.sticky_header_line_idx().unwrap_or(self.scroll_offset);
        }
        self.scroll_offset.saturating_add(adj)
    }

    /// Activate search mode with an empty query.
    pub fn start_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
        self.search_matches.clear();
        self.search_match_idx = 0;
        let mut ta = ratatui_textarea::TextArea::default();
        ta.set_cursor_line_style(Style::default());
        self.search_textarea = Some(ta);
    }

    /// Update search matches after the query changes.
    pub fn update_search(&mut self) {
        self.search_matches.clear();
        if self.search_query.is_empty() {
            self.search_match_idx = 0;
            return;
        }
        let query_lower = self.search_query.to_lowercase();
        for (line_idx, line) in self.lines.iter().enumerate() {
            if line.file_header.is_some() {
                continue;
            }
            // Search old side
            if let Some((_, ref text)) = line.old_line {
                let text_lower = text.to_lowercase();
                let mut start = 0;
                while let Some(pos) = text_lower[start..].find(&query_lower) {
                    self.search_matches.push(DiffSearchMatch {
                        line_idx,
                        panel: DiffPanel::Old,
                        col: start + pos,
                    });
                    start += pos + 1;
                }
            }
            // Search new side
            if let Some((_, ref text)) = line.new_line {
                let text_lower = text.to_lowercase();
                let mut start = 0;
                while let Some(pos) = text_lower[start..].find(&query_lower) {
                    self.search_matches.push(DiffSearchMatch {
                        line_idx,
                        panel: DiffPanel::New,
                        col: start + pos,
                    });
                    start += pos + 1;
                }
            }
        }
        // Clamp match index
        if self.search_matches.is_empty() {
            self.search_match_idx = 0;
        } else {
            self.search_match_idx = self.search_match_idx.min(self.search_matches.len() - 1);
        }
    }

    /// Dismiss search input but keep the query and highlights.
    pub fn dismiss_search(&mut self) {
        self.search_active = false;
        self.search_textarea = None;
    }

    /// Clear search entirely.
    pub fn clear_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.search_matches.clear();
        self.search_match_idx = 0;
        self.search_textarea = None;
    }

    /// Navigate to the next search match and scroll to it.
    pub fn next_search_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_match_idx = (self.search_match_idx + 1) % self.search_matches.len();
        self.scroll_to_current_match();
    }

    /// Navigate to the previous search match and scroll to it.
    pub fn prev_search_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        if self.search_match_idx == 0 {
            self.search_match_idx = self.search_matches.len() - 1;
        } else {
            self.search_match_idx -= 1;
        }
        self.scroll_to_current_match();
    }

    /// Scroll so the current search match is visible.
    pub fn scroll_to_current_match(&mut self) {
        if let Some(m) = self.search_matches.get(self.search_match_idx) {
            let target = self.scroll_target_for_line(m.line_idx);
            // Scroll so the match line is visible (roughly centered).
            if target < self.scroll_offset || target >= self.scroll_offset + 20 {
                self.scroll_offset = target.saturating_sub(5);
            }
        }
    }

    /// Parse old/new content into a ParsedDiff on any thread (no &self needed).
    pub fn parse_content(
        filename: &str,
        old: &str,
        new: &str,
        tab_width: usize,
        file_exists_on_disk: bool,
    ) -> ParsedDiff {
        let lines = crate::pager::diff_algo::compute_side_by_side(old, new, tab_width);
        let hunk_starts = crate::pager::diff_algo::find_hunk_starts(&lines);
        let sections = vec![FileSection {
            old_highlighter: FileHighlighter::new(old, filename),
            new_highlighter: FileHighlighter::new(new, filename),
        }];
        ParsedDiff {
            image_preview: None,
            inline_images: HashMap::new(),
            filename: filename.to_string(),
            old_content: old.to_string(),
            new_content: new.to_string(),
            lines,
            hunk_starts,
            hunk_staged: Vec::new(),
            hunk_line_offsets: Vec::new(),
            sections,
            file_exists_on_disk,
        }
    }

    /// Parse a `git diff HEAD` buffer for a file that has both staged and
    /// unstaged changes, classifying each hunk via `unstaged_diff`.
    ///
    /// True single buffer: one coherent set of line numbers, no separator,
    /// no duplicated context. Both diffs share worktree (new-side)
    /// numbering, so a HEAD hunk overlapping no unstaged hunk is fully
    /// staged; anything touching unstaged work counts as unstaged.
    pub fn parse_head_with_staged(
        filename: &str,
        head_diff: &str,
        unstaged_diff: &str,
        tab_width: usize,
        file_exists_on_disk: bool,
    ) -> ParsedDiff {
        let mut parsed =
            Self::parse_diff_output(filename, head_diff, tab_width, file_exists_on_disk);
        parsed.hunk_staged = head_block_staged_flags(head_diff, unstaged_diff, tab_width);
        if parsed.hunk_staged.len() != parsed.hunk_starts.len() {
            parsed.hunk_staged = vec![false; parsed.hunk_starts.len()];
        }
        parsed
    }

    /// File-relative (old, new) line spans for every visual change block of
    /// a raw unified diff. Used to map a hunk the user acts on in the HEAD
    /// buffer onto the matching block(s) of the staged/unstaged diff that
    /// patch slicing consumes.
    pub fn block_spans_for_diff(diff_text: &str, tab_width: usize) -> Vec<BlockSpan> {
        let lines = diff_lines_from_unified_or_rename_only(diff_text, tab_width);
        let hunks = parse_hunk_headers(diff_text);
        let file_header_count = lines.iter().take_while(|l| l.file_header.is_some()).count();
        let offsets = build_hunk_line_offsets(&hunks, &lines, file_header_count);
        Self::block_spans(&lines, &offsets)
    }

    /// File-relative spans for visual change blocks from already-parsed
    /// lines. `old`/`new` are `None` for pure insertions/deletions; the
    /// `*_point` gap positions still allow matching those blocks across
    /// diffs that share a side (worktree for unstaged, HEAD for staged).
    pub fn block_spans(
        lines: &[DiffLine],
        hunk_line_offsets: &[(usize, usize, usize)],
    ) -> Vec<BlockSpan> {
        let offsets_at = |idx: usize| {
            let mut offsets = (0usize, 0usize);
            for &(start_idx, old_off, new_off) in hunk_line_offsets {
                if start_idx <= idx {
                    offsets = (old_off, new_off);
                } else {
                    break;
                }
            }
            offsets
        };
        let file_num = |idx: usize, new_side: bool| -> Option<usize> {
            let (old_off, new_off) = offsets_at(idx);
            let line = lines.get(idx)?;
            if new_side {
                line.new_line.as_ref().map(|(n, _)| *n + new_off)
            } else {
                line.old_line.as_ref().map(|(n, _)| *n + old_off)
            }
        };

        let mut spans = Vec::new();
        let mut start = 0usize;
        while start < lines.len() {
            if lines[start].file_header.is_some()
                || matches!(lines[start].change_type, ChangeType::Equal)
            {
                start += 1;
                continue;
            }
            let mut end = start;
            while end < lines.len()
                && lines[end].file_header.is_none()
                && !matches!(lines[end].change_type, ChangeType::Equal)
            {
                end += 1;
            }
            let mut old_range: Option<(usize, usize)> = None;
            let mut new_range: Option<(usize, usize)> = None;
            for idx in start..end {
                let line = &lines[idx];
                if matches!(line.change_type, ChangeType::Delete | ChangeType::Modified)
                    && let Some(n) = file_num(idx, false)
                {
                    old_range = Some(match old_range {
                        None => (n, n),
                        Some((lo, hi)) => (lo.min(n), hi.max(n)),
                    });
                }
                if matches!(line.change_type, ChangeType::Insert | ChangeType::Modified)
                    && let Some(n) = file_num(idx, true)
                {
                    new_range = Some(match new_range {
                        None => (n, n),
                        Some((lo, hi)) => (lo.min(n), hi.max(n)),
                    });
                }
            }
            let gap_point = |new_side: bool| -> usize {
                for idx in (0..start).rev() {
                    if let Some(n) = file_num(idx, new_side) {
                        return n + 1;
                    }
                }
                0
            };
            spans.push(BlockSpan {
                old: old_range,
                new: new_range,
                old_point: old_range
                    .map(|(lo, _)| lo)
                    .unwrap_or_else(|| gap_point(false)),
                new_point: new_range
                    .map(|(lo, _)| lo)
                    .unwrap_or_else(|| gap_point(true)),
            });
            start = end;
        }
        spans
    }

    /// Parse raw diff output into a ParsedDiff on any thread (no &self needed).
    pub fn parse_diff_output(
        filename: &str,
        diff_output: &str,
        tab_width: usize,
        file_exists_on_disk: bool,
    ) -> ParsedDiff {
        let file_diffs = parse_multi_file_diff(diff_output);

        if file_diffs.len() <= 1 {
            let (old, new) = parse_unified_diff(diff_output);
            let actual_name = file_diffs
                .first()
                .map(|(name, _)| name.as_str())
                .unwrap_or(filename);
            let lines = diff_lines_from_unified_or_rename_only(diff_output, tab_width);
            let hunk_starts = crate::pager::diff_algo::find_hunk_starts(&lines);
            let hunks = parse_hunk_headers(diff_output);
            let hunk_line_offsets = build_hunk_line_offsets(&hunks, &lines, 0);
            let sections = vec![FileSection {
                old_highlighter: FileHighlighter::new(&old, actual_name),
                new_highlighter: FileHighlighter::new(&new, actual_name),
            }];
            ParsedDiff {
                image_preview: None,
                inline_images: HashMap::new(),
                filename: actual_name.to_string(),
                old_content: old,
                new_content: new,
                lines,
                hunk_staged: Vec::new(),
                hunk_starts,
                hunk_line_offsets,
                sections,
                file_exists_on_disk,
            }
        } else {
            // Multi-file: keep syntax highlighting, but build highlighters in
            // parallel — sequential tree-sitter on hundreds of files was the
            // main cost for large commits / directory hovers.
            let file_count = file_diffs.len();
            let new_filename = format!("{} ({} files)", filename, file_count);
            let mut lines = Vec::with_capacity(diff_output.len() / 32 + file_count);
            let mut hunk_line_offsets = Vec::new();
            let mut section_meta: Vec<(&str, &str)> = Vec::with_capacity(file_count);

            for (section_idx, (file_name, file_diff)) in file_diffs.iter().enumerate() {
                lines.push(DiffLine {
                    old_line: None,
                    new_line: None,
                    change_type: ChangeType::Equal,
                    old_segments: None,
                    new_segments: None,
                    file_header: Some(file_name.clone()),
                    comment_notes: Vec::new(),
                    preview_placeholder: None,
                    section_index: section_idx,
                });

                let section_start = lines.len();
                let mut section_lines =
                    diff_lines_from_unified_or_rename_only(file_diff, tab_width);
                for line in &mut section_lines {
                    line.section_index = section_idx;
                }
                lines.append(&mut section_lines);

                let hunks = parse_hunk_headers(file_diff);
                let section_offsets = build_hunk_line_offsets(&hunks, &lines[section_start..], 0);
                for (idx, old_off, new_off) in section_offsets {
                    hunk_line_offsets.push((section_start + idx, old_off, new_off));
                }
                section_meta.push((file_name.as_str(), file_diff));
            }

            let sections = build_file_sections_parallel(&section_meta);
            let hunk_starts = crate::pager::diff_algo::find_hunk_starts(&lines);

            ParsedDiff {
                image_preview: None,
                inline_images: HashMap::new(),
                filename: new_filename,
                old_content: String::new(),
                new_content: String::new(),
                lines,
                hunk_staged: Vec::new(),
                hunk_starts,
                hunk_line_offsets,
                sections,
                file_exists_on_disk,
            }
        }
    }

    pub fn load_notes(&mut self, repo_path: &std::path::Path) {
        let lines_file = crate::pager::notes_store::load(repo_path);
        let existing = &lines_file.notes;

        if existing.is_empty() {
            for diff_line in self.lines.iter_mut() {
                diff_line.comment_notes.clear();
            }
            return;
        }

        let hunk_line_offsets = self.hunk_line_offsets.clone();
        let mut current_file = self.filename.clone();
        for (line_idx, diff_line) in self.lines.iter_mut().enumerate() {
            if let Some(ref header) = diff_line.file_header {
                current_file = header.clone();
                // Diff might say "diff --git a/foo b/foo".
                // Strip "diff --git a/" and " b/" if they exist.
                if current_file.starts_with("diff --git ") {
                    let parts: Vec<&str> = current_file.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let path2 = parts[3];
                        if path2.len() > 2 && path2.as_bytes()[1] == b'/' {
                            current_file = path2[2..].to_string();
                        } else {
                            current_file = path2.to_string();
                        }
                    }
                }
            }

            diff_line.comment_notes.clear();

            let (old_off, new_off) = hunk_line_offsets
                .iter()
                .rev()
                .find(|(start_idx, _, _)| *start_idx <= line_idx)
                .map(|(_, o, n)| (*o, *n))
                .unwrap_or((0, 0));

            // Old side notes
            if let Some((line_num, _)) = diff_line.old_line.as_ref() {
                let file_line = *line_num + old_off;
                for c in existing.iter() {
                    if c.file == current_file && c.line == file_line && c.panel == "Old" {
                        diff_line.comment_notes.push(c.to_comment_note());
                    }
                }
            }
            // New side notes
            if let Some((line_num, _)) = diff_line.new_line.as_ref() {
                let file_line = *line_num + new_off;
                for c in existing.iter() {
                    if c.file == current_file && c.line == file_line && c.panel == "New" {
                        diff_line.comment_notes.push(c.to_comment_note());
                    }
                }
            }
        }
    }

    /// Apply a pre-parsed diff result, preserving scroll position for same-file reloads.
    pub fn apply_parsed(&mut self, parsed: ParsedDiff) {
        self.image_preview = parsed.image_preview;
        self.inline_images = parsed.inline_images;
        let same_file = self.filename == parsed.filename;
        let prev_selected_revert_hunk = self.selected_revert_hunk;
        let prev_hovered_revert_hunk = self.hovered_revert_hunk;
        self.filename = parsed.filename;
        self.old_content = parsed.old_content;
        self.new_content = parsed.new_content;
        self.lines = parsed.lines;
        self.hunk_starts = parsed.hunk_starts;
        self.hunk_staged = parsed.hunk_staged;
        self.hunk_line_offsets = parsed.hunk_line_offsets;
        self.sections = parsed.sections;
        self.file_exists_on_disk = parsed.file_exists_on_disk;
        self.selected_revert_hunk = if same_file {
            prev_selected_revert_hunk.filter(|&i| i < self.hunk_starts.len())
        } else {
            None
        };
        self.hovered_revert_hunk = if same_file {
            prev_hovered_revert_hunk.filter(|&i| i < self.hunk_starts.len())
        } else {
            None
        };
        if same_file {
            let max = self.max_scroll();
            self.scroll_offset = self.scroll_offset.min(max);
        } else {
            self.scroll_offset = 0;
            self.horizontal_scroll = 0;
            self.selection = None;
            self.clear_search();
        }
    }

    /// Load a diff from old/new content (single file).
    pub fn load(&mut self, filename: &str, old: &str, new: &str) {
        self.image_preview = None;
        self.inline_images.clear();
        // Preserve scroll position when reloading the same file (e.g. periodic refresh)
        let same_file = self.filename == filename;
        let prev_selected_revert_hunk = self.selected_revert_hunk;
        let prev_hovered_revert_hunk = self.hovered_revert_hunk;
        self.filename = filename.to_string();
        self.old_content = old.to_string();
        self.new_content = new.to_string();
        self.lines = crate::pager::diff_algo::compute_side_by_side(old, new, self.tab_width);
        self.hunk_starts = crate::pager::diff_algo::find_hunk_starts(&self.lines);
        self.hunk_staged.clear();
        self.hunk_line_offsets = Vec::new(); // Full content — no offsets needed
        if same_file {
            // Clamp scroll in case the diff got shorter
            let max = self.max_scroll();
            self.scroll_offset = self.scroll_offset.min(max);
        } else {
            self.scroll_offset = 0;
            self.horizontal_scroll = 0;
            self.selection = None;
            self.clear_search();
        }
        self.selected_revert_hunk = if same_file {
            prev_selected_revert_hunk.filter(|&i| i < self.hunk_starts.len())
        } else {
            None
        };
        self.hovered_revert_hunk = if same_file {
            prev_hovered_revert_hunk.filter(|&i| i < self.hunk_starts.len())
        } else {
            None
        };
        // Preserve side_view across reloads so periodic refresh doesn't reset it
        // Single section with index 0
        self.sections = vec![FileSection {
            old_highlighter: FileHighlighter::new(old, filename),
            new_highlighter: FileHighlighter::new(new, filename),
        }];
    }

    /// Load from raw diff output (git diff).
    /// Automatically detects multi-file diffs and splits into per-file sections.
    pub fn load_from_diff_output(&mut self, filename: &str, diff_output: &str) {
        self.image_preview = None;
        self.inline_images.clear();
        let file_diffs = parse_multi_file_diff(diff_output);

        if file_diffs.len() <= 1 {
            // Single file diff — use the simple path
            let (old, new) = parse_unified_diff(diff_output);
            // Use the actual filename from the diff header if available
            let actual_name = file_diffs
                .first()
                .map(|(name, _)| name.as_str())
                .unwrap_or(filename);
            // Build lines directly from the unified diff's per-line markers,
            // bypassing Myers re-diff so multi-hunk content can't alias.
            let same_file = self.filename == actual_name;
            self.filename = actual_name.to_string();
            self.old_content = old.clone();
            self.new_content = new.clone();
            self.lines = diff_lines_from_unified_or_rename_only(diff_output, self.tab_width);
            self.hunk_starts = crate::pager::diff_algo::find_hunk_starts(&self.lines);
            self.hunk_staged.clear();
            let hunks = parse_hunk_headers(diff_output);
            self.hunk_line_offsets = build_hunk_line_offsets(&hunks, &self.lines, 0);
            self.sections = vec![FileSection {
                old_highlighter: FileHighlighter::new(&old, actual_name),
                new_highlighter: FileHighlighter::new(&new, actual_name),
            }];
            if same_file {
                let max = self.max_scroll();
                self.scroll_offset = self.scroll_offset.min(max);
            } else {
                self.scroll_offset = 0;
                self.horizontal_scroll = 0;
                self.selection = None;
                self.clear_search();
            }
        } else {
            // Multi-file: parallel highlighters (same as parse_diff_output).
            let file_count = file_diffs.len();
            let new_filename = format!("{} ({} files)", filename, file_count);
            let same_file = self.filename == new_filename;
            let prev_selected_revert_hunk = self.selected_revert_hunk;
            let prev_hovered_revert_hunk = self.hovered_revert_hunk;
            self.filename = new_filename;
            self.old_content = String::new();
            self.new_content = String::new();
            self.lines = Vec::with_capacity(diff_output.len() / 32 + file_count);
            if !same_file {
                self.scroll_offset = 0;
                self.horizontal_scroll = 0;
                self.selection = None;
                self.clear_search();
            }
            self.selected_revert_hunk = if same_file {
                prev_selected_revert_hunk
            } else {
                None
            };
            self.hovered_revert_hunk = if same_file {
                prev_hovered_revert_hunk
            } else {
                None
            };

            self.hunk_line_offsets = Vec::new();
            let mut section_meta: Vec<(&str, &str)> = Vec::with_capacity(file_count);

            for (section_idx, (file_name, file_diff)) in file_diffs.iter().enumerate() {
                self.lines.push(DiffLine {
                    old_line: None,
                    new_line: None,
                    change_type: ChangeType::Equal,
                    old_segments: None,
                    new_segments: None,
                    file_header: Some(file_name.clone()),
                    comment_notes: Vec::new(),
                    preview_placeholder: None,
                    section_index: section_idx,
                });

                let section_start = self.lines.len();
                let mut section_lines =
                    diff_lines_from_unified_or_rename_only(file_diff, self.tab_width);
                for line in &mut section_lines {
                    line.section_index = section_idx;
                }
                self.lines.append(&mut section_lines);

                let hunks = parse_hunk_headers(file_diff);
                let section_offsets =
                    build_hunk_line_offsets(&hunks, &self.lines[section_start..], 0);
                for (idx, old_off, new_off) in section_offsets {
                    self.hunk_line_offsets
                        .push((section_start + idx, old_off, new_off));
                }
                section_meta.push((file_name.as_str(), file_diff));
            }

            self.sections = build_file_sections_parallel(&section_meta);
            self.hunk_starts = crate::pager::diff_algo::find_hunk_starts(&self.lines);
            self.hunk_staged.clear();
            self.selected_revert_hunk = if same_file {
                self.selected_revert_hunk
                    .filter(|&i| i < self.hunk_starts.len())
            } else {
                None
            };
            self.hovered_revert_hunk = if same_file {
                self.hovered_revert_hunk
                    .filter(|&i| i < self.hunk_starts.len())
            } else {
                None
            };

            if same_file {
                let max = self.max_scroll();
                self.scroll_offset = self.scroll_offset.min(max);
            }
        }
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let max = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + amount).min(max);
    }

    pub fn scroll_left(&mut self, amount: usize) {
        self.horizontal_scroll = self.horizontal_scroll.saturating_sub(amount);
    }

    pub fn scroll_right(&mut self, amount: usize) {
        self.horizontal_scroll += amount;
    }

    /// Max scroll offset (last visual/DiffLine row can reach the top).
    pub fn max_scroll(&self) -> usize {
        self.total_scroll_rows().saturating_sub(1)
    }

    /// Total scrollable rows for the active layout.
    /// Unified uses the flattened delete-then-insert visual stream.
    pub(super) fn total_scroll_rows(&self) -> usize {
        if self.view_layout == DiffViewLayout::Unified {
            self.unified_total_visual_rows(self.last_content_width.max(1))
        } else {
            self.lines.len()
        }
    }

    /// DiffLine index currently at the top of the viewport.
    pub(super) fn current_scroll_line(&self) -> usize {
        if self.view_layout == DiffViewLayout::Unified {
            self.unified_line_at_visual_row(self.scroll_offset, self.last_content_width.max(1))
                .map(|(line_idx, _, _)| line_idx)
                .unwrap_or(0)
        } else {
            self.scroll_offset
        }
    }

    /// Scroll offset that puts `line_idx` at the top of the viewport.
    pub(super) fn scroll_target_for_line(&self, line_idx: usize) -> usize {
        if self.view_layout == DiffViewLayout::Unified {
            self.unified_visual_row_for_line(line_idx, self.last_content_width.max(1))
        } else {
            line_idx
        }
    }

    /// Visual row where `line_idx` begins in the unified stream.
    pub(super) fn unified_visual_row_for_line(
        &self,
        line_idx: usize,
        content_width: usize,
    ) -> usize {
        let mut row = 0usize;
        let mut idx = 0usize;
        while idx < self.lines.len() {
            if idx == line_idx {
                return row;
            }
            let diff_line = &self.lines[idx];
            if !is_unified_change_line(diff_line) {
                row += unified_line_visual_height(diff_line, content_width, self, idx);
                idx += 1;
                continue;
            }
            let block_end = next_unified_change_block_end(&self.lines, idx);
            if line_idx < block_end {
                // Target is inside this reordered block: all deletes first,
                // then all inserts. A DiffLine's "start" is its first visible
                // contribution (delete for Modified/Delete, insert for Insert).
                if self.side_view != DiffSideView::NewOnly {
                    for i in idx..block_end {
                        let line = &self.lines[i];
                        if !matches!(line.change_type, ChangeType::Delete | ChangeType::Modified) {
                            continue;
                        }
                        if i == line_idx {
                            return row;
                        }
                        row += unified_line_row_count(&line.old_line, content_width, self);
                    }
                }
                if self.side_view != DiffSideView::OldOnly {
                    for i in idx..block_end {
                        let line = &self.lines[i];
                        if !matches!(line.change_type, ChangeType::Insert | ChangeType::Modified) {
                            continue;
                        }
                        if i == line_idx {
                            // Modified already returned above on its delete row.
                            // Insert-only lands here.
                            return row;
                        }
                        row += unified_line_row_count(&line.new_line, content_width, self);
                    }
                }
                return row;
            }
            row += self.unified_block_visual_rows(idx, block_end, content_width);
            idx = block_end;
        }
        row
    }

    /// Map a unified visual row to `(line_idx, chunk_idx, panel)`.
    pub(super) fn unified_line_at_visual_row(
        &self,
        visual_row: usize,
        content_width: usize,
    ) -> Option<(usize, usize, DiffPanel)> {
        self.unified_line_chunk_panel_from(0, visual_row, content_width)
    }

    pub(super) fn unified_total_visual_rows(&self, content_width: usize) -> usize {
        let mut row = 0usize;
        let mut idx = 0usize;
        while idx < self.lines.len() {
            let diff_line = &self.lines[idx];
            if !is_unified_change_line(diff_line) {
                row += unified_line_visual_height(diff_line, content_width, self, idx);
                idx += 1;
                continue;
            }
            let block_end = next_unified_change_block_end(&self.lines, idx);
            row += self.unified_block_visual_rows(idx, block_end, content_width);
            idx = block_end;
        }
        row
    }

    pub(super) fn unified_block_visual_rows(
        &self,
        block_start: usize,
        block_end: usize,
        content_width: usize,
    ) -> usize {
        let mut rows = 0usize;
        if self.side_view != DiffSideView::NewOnly {
            for idx in block_start..block_end {
                let line = &self.lines[idx];
                if matches!(line.change_type, ChangeType::Delete | ChangeType::Modified) {
                    rows += unified_line_row_count(&line.old_line, content_width, self);
                }
            }
        }
        if self.side_view != DiffSideView::OldOnly {
            for idx in block_start..block_end {
                let line = &self.lines[idx];
                if matches!(line.change_type, ChangeType::Insert | ChangeType::Modified) {
                    rows += unified_line_row_count(&line.new_line, content_width, self);
                }
            }
        }
        rows
    }

    /// Like `unified_line_chunk_panel_at_offset`, but starting from absolute
    /// visual row `start_visual` (0 = top of file) and seeking `target_off`
    /// rows past that.
    pub(super) fn unified_line_chunk_panel_from(
        &self,
        start_visual: usize,
        target_off: usize,
        content_width: usize,
    ) -> Option<(usize, usize, DiffPanel)> {
        let target = start_visual + target_off;
        let mut acc = 0usize;
        let mut line_idx = 0usize;

        while line_idx < self.lines.len() {
            let diff_line = &self.lines[line_idx];

            if !is_unified_change_line(diff_line) {
                let num_rows = unified_line_visual_height(diff_line, content_width, self, line_idx);
                if target < acc + num_rows {
                    return Some((line_idx, target - acc, DiffPanel::New));
                }
                acc += num_rows;
                line_idx += 1;
                continue;
            }

            let block_end = next_unified_change_block_end(&self.lines, line_idx);

            if self.side_view != DiffSideView::NewOnly {
                for idx in line_idx..block_end {
                    let line = &self.lines[idx];
                    if !matches!(line.change_type, ChangeType::Delete | ChangeType::Modified) {
                        continue;
                    }
                    let num_rows = unified_line_row_count(&line.old_line, content_width, self);
                    if target < acc + num_rows {
                        return Some((idx, target - acc, DiffPanel::Old));
                    }
                    acc += num_rows;
                }
            }

            if self.side_view != DiffSideView::OldOnly {
                for idx in line_idx..block_end {
                    let line = &self.lines[idx];
                    if !matches!(line.change_type, ChangeType::Insert | ChangeType::Modified) {
                        continue;
                    }
                    let num_rows = unified_line_row_count(&line.new_line, content_width, self);
                    if target < acc + num_rows {
                        let local_chunk_idx = target - acc;
                        let chunk_idx = if matches!(line.change_type, ChangeType::Modified)
                            && self.side_view != DiffSideView::NewOnly
                        {
                            unified_line_row_count(&line.old_line, content_width, self)
                                + local_chunk_idx
                        } else {
                            local_chunk_idx
                        };
                        return Some((idx, chunk_idx, DiffPanel::New));
                    }
                    acc += num_rows;
                }
            }

            line_idx = block_end;
        }

        None
    }

    pub fn next_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            return;
        }
        let current_line = self.current_scroll_line();
        if let Some(next) = self.hunk_starts.iter().find(|&&h| h > current_line) {
            self.scroll_offset = self.scroll_target_for_line(*next);
        } else if let Some(&first) = self.hunk_starts.first() {
            // Wrap past the last hunk to the first so { / } always cycles.
            self.scroll_offset = self.scroll_target_for_line(first);
        }
    }

    pub fn prev_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            return;
        }
        let current_line = self.current_scroll_line();
        if let Some(prev) = self.hunk_starts.iter().rev().find(|&&h| h < current_line) {
            self.scroll_offset = self.scroll_target_for_line(*prev);
        } else if let Some(&last) = self.hunk_starts.last() {
            // Wrap before the first hunk to the last so { / } always cycles.
            self.scroll_offset = self.scroll_target_for_line(last);
        }
    }

    /// Return the one-based hunk at the current viewport position and the
    /// total number of hunks. Context before the first hunk is considered part
    /// of the first hunk so the indicator starts at `1/N`.
    pub fn hunk_position(&self) -> Option<(usize, usize)> {
        let total = self.hunk_starts.len();
        if total == 0 {
            return None;
        }

        let current = self
            .hunk_starts
            .partition_point(|&start| start <= self.current_scroll_line())
            .max(1);
        Some((current, total))
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Map a terminal row within the diff panel inner area to `(line_idx, chunk_idx)`.
    /// `chunk_idx` is the wrapped-chunk position within the line (0 = first visual
    /// row). Always 0 when wrapping is off.
    pub fn line_chunk_at_row(&self, row: u16, layout: &DiffPanelLayout) -> Option<(usize, usize)> {
        if row < layout.inner_y || row >= layout.inner_end_y {
            return None;
        }
        let mut target_off = (row - layout.inner_y) as usize;
        let content_width = layout
            .new_content_end_x
            .saturating_sub(layout.new_content_x) as usize;

        // The pinned sticky file header owns the first row; everything below
        // it is shifted down by one.
        if self.is_sticky_row(layout.inner_y, layout) {
            if target_off == 0 {
                return self.sticky_header_line_idx().map(|idx| (idx, 0));
            }
            target_off -= 1;
            if self.view_layout == DiffViewLayout::Unified {
                return self
                    .unified_line_chunk_panel_at_offset(target_off, content_width)
                    .map(|(line_idx, chunk_idx, _)| (line_idx, chunk_idx));
            }
        }
        // Guard when scroll_offset is past the end (e.g. transient reload).
        if self.scroll_offset >= self.lines.len() {
            return None;
        }

        if self.view_layout == DiffViewLayout::Unified || self.side_view != DiffSideView::Both {
            let mut acc = 0usize;
            for (offset, diff_line) in self.lines[self.scroll_offset..].iter().enumerate() {
                let line_idx = self.scroll_offset + offset;
                let num_rows = unified_line_visual_height(diff_line, content_width, self, line_idx);
                if target_off < acc + num_rows {
                    return Some((line_idx, target_off - acc));
                }
                acc += num_rows;
            }
            None
        } else {
            let panel_width = layout
                .old_content_end_x
                .saturating_sub(layout.old_content_x) as usize;
            let right_content_width = layout
                .new_content_end_x
                .saturating_sub(layout.new_content_x)
                as usize;
            let mut acc = 0usize;
            for (offset, diff_line) in self.lines[self.scroll_offset..].iter().enumerate() {
                let line_idx = self.scroll_offset + offset;
                let num_rows =
                    line_visual_height(diff_line, panel_width, right_content_width, self, line_idx);
                if target_off < acc + num_rows {
                    return Some((line_idx, target_off - acc));
                }
                acc += num_rows;
            }
            None
        }
    }

    /// Map a terminal row to a diff line and the side whose content is rendered
    /// on that visual row. In unified mode, modified lines render old chunks
    /// followed by new chunks, so the panel cannot be inferred from X alone.
    pub fn line_chunk_panel_at_row(
        &self,
        row: u16,
        layout: &DiffPanelLayout,
        fallback_panel: DiffPanel,
    ) -> Option<(usize, usize, DiffPanel)> {
        if row < layout.inner_y || row >= layout.inner_end_y {
            return None;
        }
        if self.view_layout == DiffViewLayout::Unified {
            let content_width = layout
                .new_content_end_x
                .saturating_sub(layout.new_content_x) as usize;
            if self
                .sticky_for_unified_with_width(content_width.max(1))
                .is_some()
            {
                if row == layout.inner_y {
                    let (h_idx, _) = self.sticky_for_unified_with_width(content_width.max(1))?;
                    return Some((h_idx, 0, fallback_panel));
                }
                let adj = (row - layout.inner_y - 1) as usize;
                return self.unified_line_chunk_panel_at_offset(adj, content_width);
            }
            let off = (row - layout.inner_y) as usize;
            return self.unified_line_chunk_panel_at_offset(off, content_width);
        }
        let (line_idx, chunk_idx) = self.line_chunk_at_row(row, layout)?;
        Some((line_idx, chunk_idx, fallback_panel))
    }

    pub(super) fn unified_line_chunk_panel_at_offset(
        &self,
        target_off: usize,
        content_width: usize,
    ) -> Option<(usize, usize, DiffPanel)> {
        // `scroll_offset` is a visual-row index in unified layout.
        self.unified_line_chunk_panel_from(self.scroll_offset, target_off, content_width)
    }

    /// Return true when the given line index is the first line of a diff hunk.
    pub fn is_hunk_start_line(&self, line_idx: usize) -> bool {
        self.hunk_starts.binary_search(&line_idx).is_ok()
    }

    /// Get the zero-based hunk index for a hunk-start line.
    pub fn hunk_index_for_start_line(&self, line_idx: usize) -> Option<usize> {
        self.hunk_starts.binary_search(&line_idx).ok()
    }

    /// True when hunk `hunk_idx` comes from the staged diff. Missing entries
    /// (other contexts, legacy state) count as unstaged.
    pub fn is_staged_hunk(&self, hunk_idx: usize) -> bool {
        self.hunk_staged.get(hunk_idx).copied().unwrap_or(false)
    }

    /// `(staged, unstaged)` hunk counts. Returns `None` when the view has
    /// no staged/unstaged classification (commits, stash, …).
    pub fn staged_counts(&self) -> Option<(usize, usize)> {
        if self.hunk_staged.len() != self.hunk_starts.len() || self.hunk_starts.is_empty() {
            return None;
        }
        let staged = self.hunk_staged.iter().filter(|&&s| s).count();
        Some((staged, self.hunk_starts.len() - staged))
    }

    /// Zero-based hunk index owning `line_idx`, or `None` for file headers
    /// and context lines outside any change block.
    pub fn hunk_index_for_line(&self, line_idx: usize) -> Option<usize> {
        let line = self.lines.get(line_idx)?;
        if line.file_header.is_some() || line.change_type == ChangeType::Equal {
            return None;
        }
        let pos = self.hunk_starts.partition_point(|&start| start <= line_idx);
        if pos == 0 {
            return None;
        }
        Some(pos - 1)
    }

    /// True when `line_idx` sits in a staged hunk. Context/header lines
    /// (no owning hunk) count as staged so their `Reset` backgrounds pass
    /// through undimmed — as does every line when the view carries no
    /// staged/unstaged classification (commits, stash, …).
    pub fn is_staged_line(&self, line_idx: usize) -> bool {
        if self.hunk_staged.len() != self.hunk_starts.len() {
            return true;
        }
        self.hunk_index_for_line(line_idx)
            .map(|i| self.is_staged_hunk(i))
            .unwrap_or(true)
    }

    /// Jump to the next hunk and select it as the revert target. Always
    /// scrolls to the hunk's start line — same motion as `next_hunk` —
    /// even if it's already in the viewport. Wraps to the first hunk
    /// after the last.
    pub fn cycle_next_revert_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            self.selected_revert_hunk = None;
            return;
        }
        let next = match self.selected_revert_hunk {
            Some(i) => (i + 1) % self.hunk_starts.len(),
            None => self
                .hunk_starts
                .iter()
                .position(|&h| h > self.current_scroll_line())
                .unwrap_or(0),
        };
        self.selected_revert_hunk = Some(next);
        self.scroll_offset = self.scroll_target_for_line(self.hunk_starts[next]);
    }

    /// Advance to the next hunk and select it as the revert target, like
    /// `cycle_next_revert_hunk` but without wrapping: on the last hunk the
    /// selection stays put. Used after staging a hunk with Space so that
    /// finishing the file doesn't yank the viewport back to the top.
    pub fn advance_revert_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            self.selected_revert_hunk = None;
            return;
        }
        let last = self.hunk_starts.len() - 1;
        let next = match self.selected_revert_hunk {
            Some(i) => (i + 1).min(last),
            None => self
                .hunk_starts
                .iter()
                .position(|&h| h > self.current_scroll_line())
                .unwrap_or(last),
        };
        self.selected_revert_hunk = Some(next);
        self.scroll_offset = self.scroll_target_for_line(self.hunk_starts[next]);
    }

    /// Jump to the previous hunk and select it as the revert target.
    /// Wraps to the last hunk before the first.
    pub fn cycle_prev_revert_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            self.selected_revert_hunk = None;
            return;
        }
        let prev = match self.selected_revert_hunk {
            Some(0) => self.hunk_starts.len() - 1,
            Some(i) => i - 1,
            None => self
                .hunk_starts
                .iter()
                .rposition(|&h| h < self.current_scroll_line())
                .unwrap_or(self.hunk_starts.len() - 1),
        };
        self.selected_revert_hunk = Some(prev);
        self.scroll_offset = self.scroll_target_for_line(self.hunk_starts[prev]);
    }

    /// Get the highlighters for a given section index.
    pub(super) fn highlighters_for_section(
        &self,
        section_index: usize,
    ) -> Option<(&FileHighlighter, &FileHighlighter)> {
        self.sections
            .get(section_index)
            .map(|s| (&s.old_highlighter, &s.new_highlighter))
    }
}
