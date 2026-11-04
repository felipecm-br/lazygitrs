use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use std::collections::HashMap;

use crate::config::Theme;

use super::highlight::FileHighlighter;
use super::{ChangeType, DiffLine, InlineSegment};

mod decorations;
mod parse;
mod render;
mod search;
mod state;
mod style;
#[cfg(test)]
mod tests;
mod text;
mod unified;

use decorations::*;
pub use parse::*;
pub use render::*;
pub use search::*;
use style::*;
pub use text::*;
use unified::*;

/// A section of a multi-file diff with its own highlighters.
pub struct FileSection {
    pub old_highlighter: FileHighlighter,
    pub new_highlighter: FileHighlighter,
}

/// Pre-parsed diff data that can be sent across threads.
/// Contains all the expensive-to-compute results (diff algorithm, tree-sitter highlighting).
pub struct ParsedDiff {
    pub image_preview: Option<super::image_preview::ImagePreview>,
    pub inline_images: HashMap<usize, super::image_preview::InlineImagePreview>,
    pub filename: String,
    pub old_content: String,
    pub new_content: String,
    pub lines: Vec<DiffLine>,
    pub hunk_starts: Vec<usize>,
    /// Parallel to `hunk_starts`: true when the hunk comes from the staged
    /// (`--cached`) diff, false for unstaged. Empty = unknown/unstaged.
    pub hunk_staged: Vec<bool>,
    pub hunk_line_offsets: Vec<(usize, usize, usize)>,
    pub sections: Vec<FileSection>,
    pub file_exists_on_disk: bool,
}

/// Which panel of the side-by-side diff the selection is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffPanel {
    Old, // Left panel (deleted / original code)
    New, // Right panel (added / new code)
}

/// Inline-image row expansion for multi-file diffs.
///
/// Each key in `images` is a file-section index whose content is replaced by
/// `INLINE_IMAGE_ROWS` placeholder rows. Headers are preserved; text hunks in
/// later sections are remapped via an old-index -> new-index table.
impl ParsedDiff {
    pub fn attach_inline_images(
        &mut self,
        images: HashMap<usize, super::image_preview::InlineImagePreview>,
    ) {
        if images.is_empty() {
            return;
        }
        let sections: std::collections::HashSet<usize> = images.keys().copied().collect();
        self.expand_lines_for_image_sections(&sections);
        self.inline_images = images;
    }

    /// Expand placeholder rows for the given file-section indices.
    ///
    /// Testable without constructing image values: callers pass section ids,
    /// this replaces each section's non-header content with
    /// [`inline_image_placeholder_rows`] and remaps text hunks.
    fn expand_lines_for_image_sections(&mut self, sections: &std::collections::HashSet<usize>) {
        if sections.is_empty() {
            return;
        }
        let old_lines = std::mem::take(&mut self.lines);
        let mut old_to_new = vec![None; old_lines.len()];
        let mut expanded = std::collections::HashSet::new();
        for (old_index, line) in old_lines.iter().enumerate() {
            if line.file_header.is_some() {
                old_to_new[old_index] = Some(self.lines.len());
                self.lines.push(line.clone());
                // Headers exist even for rename-only sections with no body.
                if sections.contains(&line.section_index) && expanded.insert(line.section_index) {
                    self.lines
                        .extend(inline_image_placeholder_rows(line.section_index));
                }
            } else if sections.contains(&line.section_index) {
                if expanded.insert(line.section_index) {
                    self.lines
                        .extend(inline_image_placeholder_rows(line.section_index));
                }
            } else {
                old_to_new[old_index] = Some(self.lines.len());
                self.lines.push(line.clone());
            }
        }
        // A single-file pure rename may have no source rows or header at all.
        if old_lines.is_empty() && sections.contains(&0) {
            self.lines.extend(inline_image_placeholder_rows(0));
        }
        let had_staged = !self.hunk_staged.is_empty();
        let mut staged = Vec::new();
        self.hunk_starts = self
            .hunk_starts
            .iter()
            .enumerate()
            .filter_map(|(index, old_start)| {
                let new_start = old_to_new.get(*old_start).copied().flatten()?;
                if had_staged {
                    staged.push(self.hunk_staged.get(index).copied().unwrap_or(false));
                }
                Some(new_start)
            })
            .collect();
        if had_staged {
            self.hunk_staged = staged;
        }
        self.hunk_line_offsets = self
            .hunk_line_offsets
            .iter()
            .filter_map(|(index, old, new)| {
                Some((old_to_new.get(*index).copied().flatten()?, *old, *new))
            })
            .collect();
    }
}

/// Mouse text selection state in the diff view.
#[derive(Clone, Debug)]
pub struct TextSelection {
    /// Which panel (left/right) the selection lives in.
    pub panel: DiffPanel,
    /// Terminal column where the selection started.
    pub start_col: u16,
    /// Terminal row where the selection started.
    pub start_row: u16,
    /// Terminal column where the selection currently ends.
    pub end_col: u16,
    /// Terminal row where the selection currently ends.
    pub end_row: u16,
    /// Whether the user is still dragging (selection in progress).
    pub dragging: bool,
    /// True when the selection is a single click with no drag (shows edit tooltip only).
    pub is_click: bool,
    /// The extracted selected text (populated after rendering).
    pub text: String,
    /// The file line number at the top of the selection/click (populated after rendering).
    pub edit_line_number: Option<usize>,
    /// The file column number at the click position (1-based, populated after rendering).
    pub edit_column_number: Option<usize>,
}

impl TextSelection {
    /// Returns (top_row, top_col, bottom_row, bottom_col) in normalized order.
    pub fn normalized(&self) -> (u16, u16, u16, u16) {
        if self.start_row < self.end_row
            || (self.start_row == self.end_row && self.start_col <= self.end_col)
        {
            (self.start_row, self.start_col, self.end_row, self.end_col)
        } else {
            (self.end_row, self.end_col, self.start_row, self.start_col)
        }
    }
}

/// Computed column layout for the diff panel's left/right content areas.
/// All coordinates are absolute terminal X positions.
#[derive(Clone, Copy, Debug)]
pub struct DiffPanelLayout {
    /// Whether the diff is rendered as a unified single-column view.
    pub is_unified: bool,
    /// Whether this is a new-file diff (only right panel visible).
    pub is_new_file: bool,
    /// Left panel content start X (after gutter).
    pub old_content_x: u16,
    /// Left panel content end X (exclusive, up to divider).
    pub old_content_end_x: u16,
    /// Right panel content start X (after right gutter).
    pub new_content_x: u16,
    /// Right panel content end X (exclusive).
    pub new_content_end_x: u16,
    /// Inner area Y start (after top border).
    pub inner_y: u16,
    /// Inner area Y end (exclusive, before bottom border).
    pub inner_end_y: u16,
    /// X column used for clicking hunk-revert markers, if visible.
    pub revert_marker_x: Option<u16>,
}

impl DiffPanelLayout {
    /// Compute the panel layout from a main panel Rect and diff state.
    pub fn compute(panel_rect: Rect, state: &DiffViewState) -> Self {
        let inner_x = panel_rect.x + 1; // border
        let inner_y = panel_rect.y + 1;
        let inner_w = panel_rect.width.saturating_sub(2);
        let inner_end_y = inner_y + panel_rect.height.saturating_sub(2);

        let gutter: u16 = 5;
        let divider: u16 = 2;
        let unified_prefix_width: u16 = 2;

        if state.view_layout == DiffViewLayout::Unified {
            let content_x = inner_x + gutter * 2 + unified_prefix_width;
            let content_end_x = inner_x + inner_w;
            return Self {
                is_unified: true,
                is_new_file: false,
                old_content_x: content_x,
                old_content_end_x: content_end_x,
                new_content_x: content_x,
                new_content_end_x: content_end_x,
                inner_y,
                inner_end_y,
                revert_marker_x: Some(inner_x + gutter * 2),
            };
        }

        let is_new_file = state.old_content.is_empty() && state.sections.len() <= 1;

        // Single-side view uses full-width single panel
        let single_side = match state.side_view {
            DiffSideView::OldOnly => Some(DiffPanel::Old),
            DiffSideView::NewOnly => Some(DiffPanel::New),
            DiffSideView::Both => None,
        };

        if single_side.is_some() || is_new_file {
            // Single panel — gutter(5) + content(rest)
            let content_x = inner_x + gutter;
            let content_end_x = inner_x + inner_w;
            let panel = single_side.unwrap_or(DiffPanel::New);
            let (old_x, old_end, new_x, new_end) = match panel {
                DiffPanel::Old => (content_x, content_end_x, 0, 0),
                DiffPanel::New => (0, 0, content_x, content_end_x),
            };
            Self {
                is_unified: false,
                is_new_file: is_new_file && single_side.is_none(),
                old_content_x: old_x,
                old_content_end_x: old_end,
                new_content_x: new_x,
                new_content_end_x: new_end,
                inner_y,
                inner_end_y,
                revert_marker_x: None,
            }
        } else {
            let total_chrome = gutter * 2 + divider;
            let content_w = inner_w.saturating_sub(total_chrome);
            let panel_w = content_w / 2;

            let old_content_x = inner_x + gutter;
            let old_content_end_x = old_content_x + panel_w;
            // divider is at old_content_end_x, right gutter starts at old_content_end_x + 1
            let new_content_x = old_content_end_x + divider + gutter;
            let new_content_end_x = inner_x + inner_w;

            Self {
                is_unified: false,
                is_new_file: false,
                old_content_x,
                old_content_end_x,
                new_content_x,
                new_content_end_x,
                inner_y,
                inner_end_y,
                revert_marker_x: Some(old_content_end_x),
            }
        }
    }

    /// Determine which panel an X coordinate falls in, if any.
    pub fn panel_at_x(&self, x: u16) -> Option<DiffPanel> {
        if self.is_unified {
            if x >= self.old_content_x.saturating_sub(12) && x < self.new_content_end_x {
                return Some(DiffPanel::New);
            }
            return None;
        }

        if self.is_new_file {
            if x >= self.new_content_x && x < self.new_content_end_x {
                return Some(DiffPanel::New);
            }
            // Also count gutter clicks as panel clicks
            if x >= self.new_content_x.saturating_sub(5) && x < self.new_content_end_x {
                return Some(DiffPanel::New);
            }
            return None;
        }
        // Include gutter in the clickable zone for each panel
        if x >= self.old_content_x.saturating_sub(5) && x < self.old_content_end_x {
            Some(DiffPanel::Old)
        } else if x >= self.new_content_x.saturating_sub(5) && x < self.new_content_end_x {
            Some(DiffPanel::New)
        } else {
            None
        }
    }

    /// Get the content column range for a given panel.
    pub fn content_range(&self, panel: DiffPanel) -> (u16, u16) {
        match panel {
            DiffPanel::Old => (self.old_content_x, self.old_content_end_x),
            DiffPanel::New => (self.new_content_x, self.new_content_end_x),
        }
    }

    /// Get the divider X column between old and new panels (both-side view only).
    pub fn divider_x(&self) -> Option<u16> {
        self.revert_marker_x
    }
}

/// Which layout the diff body is rendered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffViewLayout {
    #[default]
    SideBySide,
    Unified,
}

impl DiffViewLayout {
    pub fn from_state_value(value: &str) -> Option<Self> {
        match value {
            "side-by-side" | "sideBySide" | "split" => Some(Self::SideBySide),
            "unified" => Some(Self::Unified),
            _ => None,
        }
    }

    pub fn as_state_value(self) -> &'static str {
        match self {
            Self::SideBySide => "side-by-side",
            Self::Unified => "unified",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::SideBySide => Self::Unified,
            Self::Unified => Self::SideBySide,
        }
    }
}

/// Which side(s) of the diff to display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSideView {
    Both,
    OldOnly,
    NewOnly,
}

/// A search match within the diff content, used for n/N navigation.
#[derive(Clone, Debug)]
pub struct DiffSearchMatch {
    /// Index into `DiffViewState::lines`.
    pub line_idx: usize,
    /// Which panel the match is on.
    pub panel: DiffPanel,
    /// Character (byte) offset within the line text.
    pub col: usize,
}

/// One entry in the diff view's revert-hunk undo stack.
pub struct RevertUndoEntry {
    pub file_path: String,
    pub pre_revert_bytes: Vec<u8>,
}

/// Maximum entries kept in the revert-hunk undo stack.
pub const REVERT_UNDO_STACK_CAP: usize = 20;

pub struct InlineEdit {
    pub line_idx: usize,
    pub panel: DiffPanel,
    pub textarea: ratatui_textarea::TextArea<'static>,
    /// When editing an existing note, its id. Empty for new notes.
    pub editing_id: String,
}

/// State for the diff view panel.
pub struct DiffViewState {
    pub image_preview: Option<super::image_preview::ImagePreview>,
    pub inline_images: HashMap<usize, super::image_preview::InlineImagePreview>,
    /// Graphics are suppressed while an overlay covers the diff pane.
    pub image_preview_hidden: bool,
    pub scroll_offset: usize,
    pub horizontal_scroll: usize,
    pub lines: Vec<DiffLine>,
    pub hunk_starts: Vec<usize>,
    /// Parallel to `hunk_starts`: true when the hunk is staged. Empty for
    /// non-Files diffs (commits, stash, …), where everything is unstaged.
    pub hunk_staged: Vec<bool>,
    pub filename: String,
    pub old_content: String,
    pub new_content: String,
    pub tab_width: usize,
    /// Per-file-section highlighters for multi-file diffs.
    sections: Vec<FileSection>,
    /// Active mouse text selection, if any.
    pub selection: Option<TextSelection>,
    /// Which side(s) of the diff to show (Both, OldOnly, NewOnly).
    pub side_view: DiffSideView,
    /// Whether to show a split or unified diff body.
    pub view_layout: DiffViewLayout,
    /// Whether long lines are wrapped to fit the panel width.
    pub wrap: bool,
    /// Last unified content width used for wrap/scroll accounting.
    /// Updated on each render so scroll helpers can compute visual rows
    /// without a live layout.
    pub last_content_width: usize,
    /// Whether the currently viewed file exists in the working tree on disk.
    pub file_exists_on_disk: bool,
    /// Hunk line number offsets for unified diffs. Each entry is
    /// `(first_diff_line_idx, old_offset, new_offset)`.
    /// The offset is added to the 1-based content line number to get the
    /// actual file line number. Empty for full-content diffs (no offset needed).
    pub hunk_line_offsets: Vec<(usize, usize, usize)>,
    /// Whether the search input is currently active (typing).
    pub search_active: bool,
    /// Current search query string.
    pub search_query: String,
    /// All matches found in the diff content.
    pub search_matches: Vec<DiffSearchMatch>,
    /// Index of the current match (for n/N navigation).
    pub search_match_idx: usize,
    /// Textarea widget for search input.
    pub search_textarea: Option<ratatui_textarea::TextArea<'static>>,
    /// Currently selected revert-button hunk index (for keyboard cycling).
    pub selected_revert_hunk: Option<usize>,
    /// Hunk index currently under the mouse cursor (for tooltip rendering).
    pub hovered_revert_hunk: Option<usize>,
    /// Pre-revert file snapshots, most-recent last. Bounded by
    /// `REVERT_UNDO_STACK_CAP`. Used to undo revert-hunk actions within a
    /// session.
    pub revert_undo_stack: Vec<RevertUndoEntry>,
    /// Peak `revert_undo_stack.len()` since it was last empty. Drives the
    /// `n/m` denominator in the bottom-right footnote; resets to 0 once the
    /// stack drains so a fresh streak starts at `1/1`.
    pub revert_undo_high_water: usize,
    /// Which line is currently hovered by the mouse (and on which panel).
    pub hovered_line: Option<(usize, DiffPanel)>,
    /// Id of the note currently selected via keyboard (for highlight).
    pub selected_note: Option<String>,
    /// Active yank flash for notes: (note_id, instant_when_yanked).
    /// Flashes a Neovim-style `highlight.on_yank` inverse color for ~450ms when copied.
    pub note_yank_flash: Option<(String, std::time::Instant)>,
    /// Active inline note editor.
    pub inline_edit: Option<InlineEdit>,
    /// Whether note boxes are visible (toggled with `t`).
    pub notes_visible: bool,
}

impl Default for DiffViewState {
    fn default() -> Self {
        Self {
            image_preview: None,
            inline_images: HashMap::new(),
            image_preview_hidden: false,
            scroll_offset: 0,
            horizontal_scroll: 0,
            lines: Vec::new(),
            hunk_starts: Vec::new(),
            hunk_staged: Vec::new(),
            filename: String::new(),
            old_content: String::new(),
            new_content: String::new(),
            tab_width: 4,
            sections: Vec::new(),
            selection: None,
            side_view: DiffSideView::Both,
            view_layout: DiffViewLayout::SideBySide,
            wrap: false,
            last_content_width: 0,
            file_exists_on_disk: false,
            hunk_line_offsets: Vec::new(),
            search_active: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_match_idx: 0,
            search_textarea: None,
            selected_revert_hunk: None,
            hovered_revert_hunk: None,
            revert_undo_stack: Vec::new(),
            revert_undo_high_water: 0,
            hovered_line: None,
            selected_note: None,
            note_yank_flash: None,
            inline_edit: None,
            notes_visible: true,
        }
    }
}
