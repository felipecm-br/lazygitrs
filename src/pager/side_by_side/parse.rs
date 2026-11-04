//! Unified diff parsing: multi-file sections, renames, binaries, hunk headers, staged-block flags.

use super::*;

/// Parse a multi-file unified diff into per-file sections.
/// Returns Vec of (filename, raw_diff_slice) without re-joining bodies.
pub(crate) fn parse_multi_file_diff(diff: &str) -> Vec<(String, &str)> {
    let bytes = diff.as_bytes();
    let mut sections: Vec<(String, &str)> = Vec::new();
    let mut current_filename = String::new();
    let mut section_start: Option<usize> = None;
    let mut line_start = 0usize;

    while line_start <= bytes.len() {
        let line_end = bytes[line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| line_start + i)
            .unwrap_or(bytes.len());
        let line = &diff[line_start..line_end];

        if line.starts_with("diff --git ") {
            if let Some(start) = section_start {
                let end = line_start.saturating_sub(1).max(start);
                sections.push((
                    std::mem::take(&mut current_filename),
                    &diff[start..end.min(diff.len()).max(start)],
                ));
            }
            current_filename = extract_filename_from_diff_header(line);
            // Keep the header: binary sections have no ---/+++ paths, and
            // delayed image loads need the exact old/new source identities.
            section_start = Some(line_start);
        }

        if line_end >= bytes.len() {
            break;
        }
        line_start = line_end + 1;
    }

    if let Some(start) = section_start
        && !current_filename.is_empty()
    {
        sections.push((current_filename, &diff[start..]));
    }

    sections
}

/// Build tree-sitter highlighters for many file sections in parallel.
pub(super) fn build_file_sections_parallel(section_meta: &[(&str, &str)]) -> Vec<FileSection> {
    let n = section_meta.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        let (name, body) = section_meta[0];
        let (old, new) = parse_unified_diff(body);
        return vec![FileSection {
            old_highlighter: FileHighlighter::new(&old, name),
            new_highlighter: FileHighlighter::new(&new, name),
        }];
    }

    let workers = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
        .min(n)
        .max(1);
    let chunk = n.div_ceil(workers);
    let mut handles = Vec::with_capacity(workers);

    for w in 0..workers {
        let start = w * chunk;
        if start >= n {
            break;
        }
        let end = (start + chunk).min(n);
        // Copy the string data the worker needs (bodies may be large but this
        // avoids lifetime issues across threads and runs once per load).
        let owned: Vec<(String, String)> = section_meta[start..end]
            .iter()
            .map(|(name, body)| ((*name).to_string(), (*body).to_string()))
            .collect();
        handles.push(std::thread::spawn(move || {
            owned
                .into_iter()
                .map(|(name, body)| {
                    let (old, new) = parse_unified_diff(&body);
                    FileSection {
                        old_highlighter: FileHighlighter::new(&old, &name),
                        new_highlighter: FileHighlighter::new(&new, &name),
                    }
                })
                .collect::<Vec<_>>()
        }));
    }

    let mut sections = Vec::with_capacity(n);
    for handle in handles {
        if let Ok(part) = handle.join() {
            sections.extend(part);
        }
    }
    sections
}

/// Extract the filename from a "diff --git a/path b/path" header line.
pub(super) fn extract_filename_from_diff_header(header: &str) -> String {
    crate::git::diff_paths::new_path_from_header(header).unwrap_or_default()
}

/// Parse a unified diff into old/new content for side-by-side display.
/// This handles `git diff` output format.
pub(super) fn parse_unified_diff(diff: &str) -> (String, String) {
    let mut old_lines = Vec::new();
    let mut new_lines = Vec::new();
    let mut in_hunk = false;

    for line in diff.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }

        if !in_hunk {
            continue;
        }

        if let Some(old) = line.strip_prefix('-') {
            old_lines.push(old);
        } else if let Some(new) = line.strip_prefix('+') {
            new_lines.push(new);
        } else if let Some(ctx) = line.strip_prefix(' ') {
            old_lines.push(ctx);
            new_lines.push(ctx);
        } else if line.starts_with('\\') {
            // Git emits metadata like `\ No newline at end of file` inside
            // hunks. It describes the preceding diff line, but it is not part
            // of either file's contents. Including it shifts highlighter line
            // numbers and can make later highlighted rows render the wrong
            // text (for example duplicating the previous added markdown line).
            continue;
        } else {
            // Bare context line in unusual diff output.
            old_lines.push(line);
            new_lines.push(line);
        }
    }

    if old_lines.is_empty()
        && new_lines.is_empty()
        && let Some((old_path, new_path)) = rename_only_paths(diff)
    {
        return (old_path, new_path);
    }

    (old_lines.join("\n"), new_lines.join("\n"))
}

pub(super) fn diff_lines_from_unified_or_rename_only(
    diff: &str,
    tab_width: usize,
) -> Vec<DiffLine> {
    if is_binary_diff(diff) {
        return binary_file_placeholder_lines();
    }
    if let Some((old_path, new_path)) = rename_only_paths(diff) {
        return rename_only_lines(&old_path, &new_path, tab_width);
    }

    crate::pager::diff_algo::compute_side_by_side_from_unified_diff(diff, tab_width)
}

/// Git emits `Binary files A and B differ` (and similar) instead of hunks.
pub(super) fn is_binary_diff(diff: &str) -> bool {
    diff.lines().any(|line| {
        // Do not trim: a leading space marks ordinary hunk context, not metadata.
        line.starts_with("Binary files ")
            || line.starts_with("Binary file ")
            || line.starts_with("GIT binary patch")
    })
}

pub(super) fn binary_file_placeholder_lines() -> Vec<DiffLine> {
    [
        "",
        "                          ",
        "Binary cannot be previewed",
        "                          ",
        "",
    ]
    .into_iter()
    .map(|message| DiffLine {
        old_line: None,
        new_line: None,
        change_type: ChangeType::Equal,
        old_segments: None,
        new_segments: None,
        file_header: None,
        comment_notes: Vec::new(),
        preview_placeholder: Some(message),
        section_index: 0,
    })
    .collect()
}

/// Placeholder rows backing one inline-image section.
///
/// Uses the existing `DiffLine` placeholder model (no new field): all rows are
/// hatch stripes, the middle row carries the fallback label so hidden images
/// still read usefully. Length is `INLINE_IMAGE_ROWS`.
pub(super) fn inline_image_placeholder_rows(section_index: usize) -> Vec<DiffLine> {
    let total_rows = crate::pager::image_preview::INLINE_IMAGE_ROWS;
    let mid = total_rows / 2;
    (0..total_rows)
        .map(|r| {
            let message: &'static str = if r == mid {
                "Binary cannot be previewed"
            } else if r + 1 == mid || r == mid + 1 {
                "                          "
            } else {
                ""
            };
            DiffLine {
                old_line: None,
                new_line: None,
                change_type: ChangeType::Equal,
                old_segments: None,
                new_segments: None,
                file_header: None,
                comment_notes: Vec::new(),
                preview_placeholder: Some(message),
                section_index,
            }
        })
        .collect()
}

/// True when git emitted a rename/copy with no content hunks (pure move).
pub fn is_rename_only_diff(diff: &str) -> bool {
    rename_only_paths(diff).is_some()
}

pub(super) fn rename_only_paths(diff: &str) -> Option<(String, String)> {
    if diff.lines().any(|line| line.starts_with("@@")) {
        return None;
    }

    let mut old_path = None;
    let mut new_path = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("rename from ") {
            old_path = Some(path.to_string());
        } else if let Some(path) = line.strip_prefix("rename to ") {
            new_path = Some(path.to_string());
        } else if let Some(path) = line.strip_prefix("copy from ") {
            old_path = Some(path.to_string());
        } else if let Some(path) = line.strip_prefix("copy to ") {
            new_path = Some(path.to_string());
        }
    }

    Some((old_path?, new_path?))
}

pub(super) fn rename_only_lines(old_path: &str, new_path: &str, tab_width: usize) -> Vec<DiffLine> {
    vec![DiffLine {
        old_line: Some((1, crate::pager::expand_tabs(old_path, tab_width))),
        new_line: Some((1, crate::pager::expand_tabs(new_path, tab_width))),
        change_type: ChangeType::Modified,
        old_segments: None,
        new_segments: None,
        file_header: None,
        comment_notes: Vec::new(),
        preview_placeholder: None,
        section_index: 0,
    }]
}

/// Parse hunk headers from a unified diff, returning
/// `(old_start, new_start, old_count, new_count)` for each hunk.
pub(super) fn parse_hunk_headers(diff: &str) -> Vec<(usize, usize, usize, usize)> {
    let mut hunks = Vec::new();
    for line in diff.lines() {
        if !line.starts_with("@@") {
            continue;
        }
        // Format: @@ -OLD_START[,OLD_COUNT] +NEW_START[,NEW_COUNT] @@
        let inner = line
            .trim_start_matches('@')
            .trim_start()
            .split("@@")
            .next()
            .unwrap_or("");
        let mut old_start = 1usize;
        let mut old_count = 1usize;
        let mut new_start = 1usize;
        let mut new_count = 1usize;
        for token in inner.split_whitespace() {
            if let Some(rest) = token.strip_prefix('-') {
                let mut parts = rest.splitn(2, ',');
                old_start = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
                old_count = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
            } else if let Some(rest) = token.strip_prefix('+') {
                let mut parts = rest.splitn(2, ',');
                new_start = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
                new_count = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
            }
        }
        hunks.push((old_start, new_start, old_count, new_count));
    }
    hunks
}

/// File-relative span of one visual change block.
pub struct BlockSpan {
    pub old: Option<(usize, usize)>,
    pub new: Option<(usize, usize)>,
    pub old_point: usize,
    pub new_point: usize,
}

impl BlockSpan {
    /// Effective inclusive span on one side, using the gap point for pure
    /// insertions (`old`) and pure deletions (`new`).
    pub fn eff(&self, new_side: bool) -> (usize, usize) {
        if new_side {
            self.new.unwrap_or((self.new_point, self.new_point))
        } else {
            self.old.unwrap_or((self.old_point, self.old_point))
        }
    }

    /// True when both spans touch on the given side.
    pub fn overlaps(&self, other: &BlockSpan, new_side: bool) -> bool {
        let (lo1, hi1) = self.eff(new_side);
        let (lo2, hi2) = other.eff(new_side);
        lo1 <= hi2 && lo2 <= hi1
    }
}

/// One `staged` flag per visual change block of a `git diff HEAD` buffer,
/// in block order. Both diffs share worktree (new-side) numbering, so a
/// HEAD block overlapping no unstaged block is fully staged; anything
/// touching unstaged work counts as unstaged. Handles multi-file buffers
/// by matching sections on filename; files missing from the unstaged diff
/// are fully staged.
pub fn head_block_staged_flags(
    head_diff: &str,
    unstaged_diff: &str,
    tab_width: usize,
) -> Vec<bool> {
    let head_sections = parse_multi_file_diff(head_diff);
    if head_sections.is_empty() {
        return head_blocks_staged_flags(head_diff, unstaged_diff, tab_width);
    }
    let unstaged_sections = parse_multi_file_diff(unstaged_diff);
    let mut flags = Vec::new();
    for (name, body) in &head_sections {
        let peer = unstaged_sections
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| *b)
            .unwrap_or("");
        flags.extend(head_blocks_staged_flags(body, peer, tab_width));
    }
    flags
}

/// One `staged` flag per visual change block of a single-file
/// `git diff HEAD` buffer.
pub(super) fn head_blocks_staged_flags(
    head_diff: &str,
    unstaged_diff: &str,
    tab_width: usize,
) -> Vec<bool> {
    let head_spans = DiffViewState::block_spans_for_diff(head_diff, tab_width);
    let unstaged_spans = DiffViewState::block_spans_for_diff(unstaged_diff, tab_width);
    head_spans
        .iter()
        .map(|h| !unstaged_spans.iter().any(|u| h.overlaps(u, true)))
        .collect()
}

/// Build the hunk line offset table from parsed hunk headers and
/// computed DiffLines. Each entry is `(first_diff_line_idx, old_offset, new_offset)`.
pub(super) fn build_hunk_line_offsets(
    hunks: &[(usize, usize, usize, usize)],
    lines: &[DiffLine],
    file_header_count: usize,
) -> Vec<(usize, usize, usize)> {
    if hunks.is_empty() {
        return Vec::new();
    }

    let mut offsets = Vec::new();
    let mut cumulative_old = 0usize; // content-relative old line count before this hunk
    let mut cumulative_new = 0usize;

    for (hunk_idx, &(old_start, new_start, old_count, new_count)) in hunks.iter().enumerate() {
        // The content line numbers for this hunk start at cumulative + 1
        let content_old_start = cumulative_old + 1;
        let content_new_start = cumulative_new + 1;

        // Find the first DiffLine that belongs to this hunk by matching
        // content line numbers.
        let first_line_idx = if hunk_idx == 0 {
            // First hunk: starts at the first non-header DiffLine
            file_header_count
        } else {
            // Find the first DiffLine whose old_line or new_line number
            // matches the content start of this hunk.
            lines
                .iter()
                .enumerate()
                .skip(file_header_count)
                .find(|(_, dl)| {
                    dl.old_line
                        .as_ref()
                        .map(|(n, _)| *n >= content_old_start)
                        .unwrap_or(false)
                        || dl
                            .new_line
                            .as_ref()
                            .map(|(n, _)| *n >= content_new_start)
                            .unwrap_or(false)
                })
                .map(|(idx, _)| idx)
                .unwrap_or(0)
        };

        // Offset: actual file line - content line
        let old_offset = old_start.saturating_sub(content_old_start);
        let new_offset = new_start.saturating_sub(content_new_start);

        offsets.push((first_line_idx, old_offset, new_offset));

        cumulative_old += old_count;
        cumulative_new += new_count;
    }

    offsets
}
