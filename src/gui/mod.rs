mod background;
pub mod context;
pub mod controller;
mod diff_pipeline;
mod event_loop;
pub mod gradient;
mod hunks;
pub mod input;
mod keymap;
mod keys;
pub mod layout;
mod menus;
pub mod modes;
mod mouse;
mod notes;
pub mod popup;
mod popup_keys;
pub mod presentation;
mod refresh;
pub mod scroll;
mod search;
pub mod state;
pub mod views;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{Command, cursor, execute};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, BorderType};

use crate::config::keybindings::matches_key;
use crate::config::{AppConfig, AppState};
use crate::git::{DEFAULT_COMMIT_LIMIT, GitCommands, HeadState, MODEL_PART_COUNT, ModelPart};
use crate::model::Model;
use crate::model::file_tree::{CommitFileTreeNode, FileTreeNode, build_file_tree};
use crate::os::platform::Platform;
use crate::pager::side_by_side::{
    DiffPanel, DiffPanelLayout, DiffViewLayout, DiffViewState, TextSelection, is_rename_only_diff,
};

use self::context::{ContextId, ContextManager, SideWindow};
use self::input::InputReader;
use self::layout::LayoutState;
use self::modes::diff_mode::DiffModeState;
use self::modes::patch_building::PatchBuildingState;
use self::modes::rebase_mode::{EntryStatus, RebaseModeState, RebasePhase};
use self::popup::{CommandAction, CommandEntry, CommandSection};
use self::popup::{
    MessageKind, PopupState, list_picker_clamp_selection_to_matches,
    list_picker_filtered_display_idx, list_picker_matching_indices, list_picker_next_match,
    list_picker_prev_match,
};
pub use self::state::{AcpState, CommitFilterState, EditorHandoff};

/// Compute the visible list height for a list picker popup, given terminal height.
/// Must match the rendering formula: popup 60% height, minus borders (2), search bar + sep + hint (3).
fn list_picker_visible_height(terminal_height: usize) -> usize {
    let popup_h = (terminal_height * 60 / 100)
        .max(10)
        .min(terminal_height.saturating_sub(4));
    popup_h.saturating_sub(2).saturating_sub(3)
}

/// Keep the selection in view after Up/Down (including wrap last↔first).
fn list_picker_scroll_after_nav(
    core: &mut crate::gui::popup::ListPickerCore,
    matching: &[usize],
    list_height: usize,
    moved_down: bool,
) {
    let sdi = list_picker_filtered_display_idx(&core.items, matching, core.selected);
    if moved_down {
        if matching.first() == Some(&core.selected) {
            core.scroll_offset = 0;
        } else if sdi >= core.scroll_offset + list_height {
            core.scroll_offset = sdi.saturating_sub(list_height - 1);
        }
    } else if matching.last() == Some(&core.selected) {
        core.scroll_offset = sdi.saturating_sub(list_height.saturating_sub(1));
    } else if matching.first() == Some(&core.selected) {
        core.scroll_offset = 0;
    } else if sdi <= core.scroll_offset {
        core.scroll_offset = sdi.saturating_sub(1);
    }
}

/// Shared mouse scroll/click handling for free-entry list pickers (RefPicker, ListPicker).
fn handle_list_picker_mouse(
    core: &mut crate::gui::popup::ListPickerCore,
    mouse: crossterm::event::MouseEvent,
    layout_width: u16,
    layout_height: u16,
) {
    use crossterm::event::{MouseButton, MouseEventKind};

    let search = core.search_textarea.lines().join("");
    let matching = list_picker_matching_indices(&core.items, &search);
    let h = layout_height as usize;
    let lh = list_picker_visible_height(h);
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            if let Some(prev) = list_picker_prev_match(&matching, core.selected) {
                core.selected = prev;
            }
            list_picker_scroll_after_nav(core, &matching, lh, false);
        }
        MouseEventKind::ScrollDown => {
            if let Some(next) = list_picker_next_match(&matching, core.selected) {
                core.selected = next;
            }
            list_picker_scroll_after_nav(core, &matching, lh, true);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            // Click to select an item in the list picker
            let area = ratatui::layout::Rect::new(0, 0, layout_width, layout_height);
            let popup_width = (area.width * 60 / 100).clamp(30, 60);
            let max_popup = (area.height * 60 / 100).max(10);
            let popup_height = max_popup.min(area.height.saturating_sub(4));
            let x = (area.width.saturating_sub(popup_width)) / 2;
            let y = (area.height.saturating_sub(popup_height)) / 2;
            let inner_y = y + 1;
            let list_start = inner_y + 2;
            let inner_height = popup_height.saturating_sub(2);
            let list_height = inner_height.saturating_sub(3) as usize;

            if mouse.row >= list_start
                && mouse.row < list_start + list_height as u16
                && mouse.column >= x
                && mouse.column < x + popup_width
            {
                let row_in_list = (mouse.row - list_start) as usize;
                // Build filtered display rows (headers + matching items)
                let has_categories = matching
                    .iter()
                    .any(|&i| core.items.get(i).is_some_and(|it| !it.category.is_empty()));
                let mut display: Vec<(bool, Option<usize>)> = Vec::new();
                if has_categories {
                    let mut last_cat = String::new();
                    for &ei in &matching {
                        let Some(item) = core.items.get(ei) else {
                            continue;
                        };
                        if !item.category.is_empty() && item.category != last_cat {
                            display.push((true, None));
                            last_cat = item.category.clone();
                        }
                        display.push((false, Some(ei)));
                    }
                } else {
                    for &ei in &matching {
                        display.push((false, Some(ei)));
                    }
                }
                let max_scroll = display.len().saturating_sub(list_height);
                let effective_scroll = core.scroll_offset.min(max_scroll);
                let display_idx = effective_scroll + row_in_list;
                if let Some((is_header, item_idx)) = display.get(display_idx)
                    && !*is_header
                    && let Some(ei) = item_idx
                {
                    core.selected = *ei;
                }
            }
        }
        _ => {}
    }
}

pub type Term = Terminal<CrosstermBackend<crate::os::tty::TuiOutput>>;
const COMMIT_DETAILS_DEBOUNCE: Duration = Duration::from_millis(120);
const MAX_CONCURRENT_DIFF_JOBS: usize = 2;
const DIFF_PREVIEW_CACHE_ENTRIES: usize = 24;
const DIFF_PREVIEW_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_CACHED_DIFF_BYTES: usize = 8 * 1024 * 1024;
/// How many diffs below/above the selection to warm in the preview cache.
const DIFF_PREFETCH_AHEAD: usize = 3;
const DIFF_PREFETCH_BEHIND: usize = 1;
/// At most this many prefetch loads queued or running at once.
const DIFF_PREFETCH_INFLIGHT_MAX: usize = 2;
const DIFF_PREFETCH_WORKERS: usize = 2;

fn plain_char_key(key: KeyEvent, expected: char) -> bool {
    let modifiers = if expected.is_uppercase() {
        KeyModifiers::SHIFT
    } else {
        KeyModifiers::NONE
    };
    key.code == KeyCode::Char(expected) && key.modifiers == modifiers
}

fn has_command_modifier(modifiers: KeyModifiers) -> bool {
    modifiers.intersects(KeyModifiers::SUPER | KeyModifiers::META)
}

pub(crate) fn textarea_input(
    textarea: &mut ratatui_textarea::TextArea<'static>,
    key: KeyEvent,
) -> bool {
    use ratatui_textarea::CursorMove;

    let cmd = has_command_modifier(key.modifiers);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        // Cmd+Left/Right (and Ctrl+Left/Right): line head/end.
        // Many macOS terminals remap Cmd+arrows to Home/End, so handle those too.
        KeyCode::Left if cmd || ctrl => textarea.move_cursor(CursorMove::Head),
        KeyCode::Right if cmd || ctrl => textarea.move_cursor(CursorMove::End),
        KeyCode::Home => textarea.move_cursor(CursorMove::Head),
        KeyCode::End => textarea.move_cursor(CursorMove::End),
        // Cmd/Ctrl+Backspace: delete to start of line
        KeyCode::Backspace if cmd => {
            textarea.delete_line_by_head();
        }
        // Option/Alt+Left/Right: move by word
        KeyCode::Left if alt => textarea.move_cursor(CursorMove::WordBack),
        KeyCode::Right if alt => textarea.move_cursor(CursorMove::WordForward),
        // Option/Alt+Backspace: delete previous word
        KeyCode::Backspace if alt => {
            let ratatui_textarea::DataCursor(row, col) = textarea.cursor();
            textarea.move_cursor(CursorMove::WordBack);
            let ratatui_textarea::DataCursor(new_row, new_col) = textarea.cursor();
            if new_row == row {
                for _ in new_col..col {
                    textarea.delete_next_char();
                }
            } else {
                textarea.move_cursor(CursorMove::Jump(row as u16, col as u16));
                for _ in 0..=col {
                    textarea.delete_char();
                }
            }
        }
        KeyCode::Char(_) if cmd => return false,
        KeyCode::Char('a') if ctrl => textarea.move_cursor(CursorMove::Head),
        KeyCode::Char('e') if ctrl => textarea.move_cursor(CursorMove::End),
        KeyCode::Char('u') if ctrl => {
            textarea.delete_line_by_head();
        }
        // Fall through: tui-textarea handles Alt+b/f/h/l and plain chars
        _ => return textarea.input(key),
    };
    true
}

/// A completed diff result from the background thread.
pub(crate) struct DiffResult {
    /// Generation counter to discard stale results.
    pub generation: u64,
    /// The diff key this result corresponds to.
    pub diff_key: String,
    /// The computed diff data: (filename, old_content, new_content) or None for empty.
    pub payload: DiffPayload,
    /// True for speculative neighbor loads: applied if the user is already
    /// waiting on this key, cached for later otherwise. Never generation-gated.
    pub is_prefetch: bool,
}

pub(crate) enum DiffPayload {
    /// Pre-parsed diff ready to apply (parsing done on background thread).
    Parsed(crate::pager::side_by_side::ParsedDiff),
    /// No diff to show.
    Empty,
}

struct DiffJob {
    generation: u64,
    diff_key: String,
    load: Box<dyn FnOnce() -> DiffPayload + Send>,
}

enum DiffSchedulerEvent {
    Job(DiffJob),
    Complete,
}

struct CachedDiffPreview {
    key: String,
    view: DiffViewState,
    estimated_bytes: usize,
}

#[derive(Default)]
struct DiffPreviewCache {
    entries: VecDeque<CachedDiffPreview>,
    estimated_bytes: usize,
}

impl DiffPreviewCache {
    fn insert(&mut self, key: String, mut view: DiffViewState) {
        for image in view.inline_images.values_mut() {
            image.unload_for_cache();
        }
        self.remove(&key);
        let estimated_bytes = estimate_diff_view_bytes(&view);
        if estimated_bytes > MAX_CACHED_DIFF_BYTES {
            return;
        }

        while self.entries.len() >= DIFF_PREVIEW_CACHE_ENTRIES
            || self.estimated_bytes.saturating_add(estimated_bytes) > DIFF_PREVIEW_CACHE_BYTES
        {
            let Some(evicted) = self.entries.pop_front() else {
                break;
            };
            self.estimated_bytes = self.estimated_bytes.saturating_sub(evicted.estimated_bytes);
        }

        self.estimated_bytes = self.estimated_bytes.saturating_add(estimated_bytes);
        self.entries.push_back(CachedDiffPreview {
            key,
            view,
            estimated_bytes,
        });
    }

    fn take(&mut self, key: &str) -> Option<DiffViewState> {
        let index = self.entries.iter().position(|entry| entry.key == key)?;
        let entry = self.entries.remove(index)?;
        self.estimated_bytes = self.estimated_bytes.saturating_sub(entry.estimated_bytes);
        Some(entry.view)
    }

    fn contains(&self, key: &str) -> bool {
        self.entries.iter().any(|entry| entry.key == key)
    }

    /// Drop entries whose content can go stale on refresh (working-tree
    /// diffs), keeping hash-keyed commit/stash diffs that never change.
    fn retain_immutable(&mut self) {
        self.entries
            .retain(|entry| diff_key_is_immutable(&entry.key));
        self.estimated_bytes = self.entries.iter().map(|e| e.estimated_bytes).sum();
    }

    fn remove(&mut self, key: &str) {
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key)
            && let Some(entry) = self.entries.remove(index)
        {
            self.estimated_bytes = self.estimated_bytes.saturating_sub(entry.estimated_bytes);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.estimated_bytes = 0;
    }
}

fn estimate_diff_view_bytes(view: &DiffViewState) -> usize {
    let line_bytes = view.lines.iter().fold(0usize, |total, line| {
        let text_bytes = line
            .old_line
            .as_ref()
            .map(|(_, text)| text.len())
            .unwrap_or(0)
            .saturating_add(
                line.new_line
                    .as_ref()
                    .map(|(_, text)| text.len())
                    .unwrap_or(0),
            );
        let segment_bytes = line
            .old_segments
            .iter()
            .chain(line.new_segments.iter())
            .flatten()
            .map(|segment| segment.text.len())
            .sum::<usize>();
        total
            .saturating_add(text_bytes)
            .saturating_add(segment_bytes)
    });

    view.filename
        .len()
        .saturating_add(view.old_content.len())
        .saturating_add(view.new_content.len())
        .saturating_add(line_bytes)
        .saturating_mul(2)
        .saturating_add(
            view.inline_images
                .values()
                .map(|image| image.estimated_bytes())
                .sum::<usize>(),
        )
        .saturating_add(
            view.image_preview
                .as_ref()
                .map_or(0, |p| p.estimated_bytes()),
        )
}

type BackgroundJob = Box<dyn FnOnce() + Send>;

fn spawn_diff_scheduler(
    rx: mpsc::Receiver<DiffSchedulerEvent>,
    scheduler_tx: mpsc::Sender<DiffSchedulerEvent>,
    result_tx: mpsc::Sender<DiffResult>,
    generation: Arc<AtomicU64>,
) {
    std::thread::spawn(move || {
        let mut active_jobs = 0usize;
        let mut pending_job: Option<DiffJob> = None;

        while let Ok(event) = rx.recv() {
            match event {
                DiffSchedulerEvent::Job(job) => {
                    if generation.load(Ordering::Relaxed) != job.generation {
                        continue;
                    }
                    if active_jobs < MAX_CONCURRENT_DIFF_JOBS {
                        active_jobs += 1;
                        spawn_diff_job(
                            job,
                            result_tx.clone(),
                            scheduler_tx.clone(),
                            Arc::clone(&generation),
                        );
                    } else {
                        pending_job = Some(job);
                    }
                }
                DiffSchedulerEvent::Complete => {
                    active_jobs = active_jobs.saturating_sub(1);
                    if let Some(job) = pending_job.take()
                        && generation.load(Ordering::Relaxed) == job.generation
                    {
                        active_jobs += 1;
                        spawn_diff_job(
                            job,
                            result_tx.clone(),
                            scheduler_tx.clone(),
                            Arc::clone(&generation),
                        );
                    }
                }
            }
        }
    });
}

fn spawn_diff_job(
    job: DiffJob,
    result_tx: mpsc::Sender<DiffResult>,
    scheduler_tx: mpsc::Sender<DiffSchedulerEvent>,
    generation: Arc<AtomicU64>,
) {
    std::thread::spawn(move || {
        if generation.load(Ordering::Relaxed) == job.generation {
            let payload = (job.load)();
            if generation.load(Ordering::Relaxed) == job.generation {
                let _ = result_tx.send(DiffResult {
                    generation: job.generation,
                    diff_key: job.diff_key,
                    payload,
                    is_prefetch: false,
                });
            }
        }
        let _ = scheduler_tx.send(DiffSchedulerEvent::Complete);
    });
}

/// Diff keys derived from a commit or stash hash: the content can never
/// change, so caches keyed this way survive refreshes and never need a
/// same-key reload.
fn diff_key_is_immutable(key: &str) -> bool {
    key.starts_with("Commits:")
        || key.starts_with("Reflog:")
        || key.starts_with("BranchCommits:")
        || key.starts_with("Stash:")
}

struct DiffPrefetchJob {
    diff_key: String,
    load: Box<dyn FnOnce() -> DiffPayload + Send>,
}

/// Low-priority lane that warms the preview cache with neighbor diffs.
/// Every job MUST produce a result: `begin_diff_request` skips spawning an
/// interactive job when a prefetch for the same key is in flight, and waits
/// for this result instead.
fn spawn_diff_prefetch_workers(
    rx: mpsc::Receiver<DiffPrefetchJob>,
    result_tx: mpsc::Sender<DiffResult>,
) {
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..DIFF_PREFETCH_WORKERS {
        let rx = Arc::clone(&rx);
        let result_tx = result_tx.clone();
        std::thread::spawn(move || {
            loop {
                let job = match rx.lock() {
                    Ok(guard) => guard.recv(),
                    Err(_) => return,
                };
                let Ok(job) = job else { return };
                let payload = (job.load)();
                let _ = result_tx.send(DiffResult {
                    generation: 0,
                    diff_key: job.diff_key,
                    payload,
                    is_prefetch: true,
                });
            }
        });
    }
}

/// Load + parse one commit's diff — shared by interactive loads and prefetch
/// so both produce byte-identical payloads for the same key.
fn commit_diff_payload(git: &GitCommands, hash: &str, label_prefix: &str) -> DiffPayload {
    if let Ok(diff) = git.diff_commit(hash) {
        let filename = format!("{}:{}", label_prefix, &hash[..7.min(hash.len())]);
        DiffPayload::Parsed(DiffViewState::parse_diff_output(&filename, &diff, 4, false))
    } else {
        DiffPayload::Empty
    }
}

fn stash_diff_payload(git: &GitCommands, index: usize) -> DiffPayload {
    match git.stash_diff(index) {
        Ok(diff) if diff.is_empty() => DiffPayload::Empty,
        Ok(diff) => {
            let filename = format!("stash@{{{}}}", index);
            let exists = git.repo_path().join(&filename).exists();
            DiffPayload::Parsed(DiffViewState::parse_diff_output(
                &filename, &diff, 4, exists,
            ))
        }
        Err(_) => DiffPayload::Empty,
    }
}

fn spawn_latest_background_worker(rx: mpsc::Receiver<BackgroundJob>) {
    std::thread::spawn(move || {
        while let Ok(mut job) = rx.recv() {
            loop {
                match rx.recv_timeout(COMMIT_DETAILS_DEBOUNCE) {
                    Ok(newer_job) => job = newer_job,
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            job();
        }
    });
}

struct AiCommitJob {
    generation: u64,
    cancel: Arc<AtomicBool>,
    cancel_armed_at: Option<Instant>,
}

struct AiCommitResult {
    generation: u64,
    result: Result<Option<String>>,
}

#[derive(Debug, Clone)]
enum AiCommitSource {
    Staged,
    Commit(String),
}

struct CommitPageResult {
    generation: u64,
    /// When true, replace the commit list (filter apply). When false, append.
    replace: bool,
    result: Result<Vec<crate::model::Commit>>,
}

const COMMIT_PAGE_PREFETCH_THRESHOLD: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuitReason {
    #[default]
    Normal,
    ReturnToCockpit,
}

impl QuitReason {
    pub fn exit_code(&self) -> i32 {
        match self {
            QuitReason::Normal => 0,
            QuitReason::ReturnToCockpit => 10,
        }
    }
}

pub struct Gui {
    pub config: Arc<AppConfig>,
    pub git: Arc<GitCommands>,
    pub model: Arc<Mutex<Model>>,
    pub context_mgr: ContextManager,
    pub layout: LayoutState,
    pub popup: PopupState,
    pub diff_view: DiffViewState,
    /// Cached graph layouts used to render only the visible commit rows.
    commit_list_cache: presentation::commits::CommitListCache,
    pub command_log: crate::os::cmd::CommandLog,
    pub show_command_log: bool,
    pub should_quit: bool,
    pub quit_reason: QuitReason,
    pub needs_refresh: bool,
    pub needs_files_refresh: bool,
    pub needs_diff_refresh: bool,
    pub pending_navigation: Option<crate::acp::NavigateContext>,
    pub start_in_diff: bool,
    pub start_file_filter: Option<String>,
    pub search_query: String,
    /// Whether search input mode is active (typing into search bar).
    pub search_active: bool,
    /// Indices of items matching the current search in the active panel.
    pub search_matches: Vec<usize>,
    /// Current position within search_matches.
    pub search_match_idx: usize,
    pub screen_mode: ScreenMode,
    /// True while the user is dragging the sidebar divider with the mouse.
    sidebar_resizing: bool,
    /// Portrait-only: add this to the mouse row when mapping to side height so
    /// grabs on the expanded panel bottom (above trailing collapsed rows) and
    /// the main/diff top border share one continuous drag.
    sidebar_resize_row_offset: u16,
    pub show_file_tree: bool,
    /// Cached file tree nodes — rebuilt on refresh when tree view is active.
    pub file_tree_nodes: Vec<FileTreeNode>,
    /// Set of collapsed directory paths in the file tree.
    pub collapsed_dirs: HashSet<String>,
    /// Whether the diff/main panel is focused (entered via Enter on a file).
    pub diff_focused: bool,
    /// Whether a diff is currently being loaded on a background thread.
    pub diff_loading: bool,
    /// When the current diff load started (for delayed "Loading..." display).
    pub(crate) diff_loading_since: Option<Instant>,
    /// Track what we last loaded a diff for, to avoid reloading on every frame.
    last_diff_key: String,
    /// Generation counter — incremented on each diff request, used to discard stale results.
    pub(crate) diff_generation: Arc<AtomicU64>,
    /// Sender for background diff loading.
    diff_rx: mpsc::Receiver<DiffResult>,
    /// Bounded scheduler: starts immediately, caps parallelism, and retains
    /// only the newest overflow request while navigation is rapid.
    diff_scheduler_tx: mpsc::Sender<DiffSchedulerEvent>,
    /// Recently completed parsed previews, moved in and out for instant revisits.
    diff_preview_cache: DiffPreviewCache,
    /// The diff key whose content `diff_view` currently shows. Stays on the
    /// outgoing key while a newer selection loads (stale content is kept
    /// visible instead of blanking the pane), and is how the outgoing view
    /// finds its slot in the preview cache when the replacement arrives.
    displayed_diff_key: String,
    /// Sender for the speculative neighbor-diff lane.
    diff_prefetch_tx: mpsc::Sender<DiffPrefetchJob>,
    /// Keys with a prefetch queued or running. An interactive request for one
    /// of these waits for the prefetch result instead of duplicating the work.
    diff_prefetch_inflight: HashSet<String>,
    /// Receiver for AI commit message generation results.
    ai_commit_rx: mpsc::Receiver<AiCommitResult>,
    /// Sender cloned into background threads for AI commit generation.
    ai_commit_tx: mpsc::Sender<AiCommitResult>,
    /// Agent-Client Protocol (ACP) review notes integration state.
    pub(crate) acp: AcpState,
    /// Receiver for incremental commit pages loaded after the first capped page.
    commit_page_rx: mpsc::Receiver<CommitPageResult>,
    /// Sender cloned into background threads for incremental commit loading.
    commit_page_tx: mpsc::Sender<CommitPageResult>,
    /// True while a background commit page is in flight.
    commit_page_loading: bool,
    /// True when the last commit page was shorter than the requested page size.
    commit_history_complete: bool,
    /// Generation counter used to discard stale commit-page results after refresh.
    commit_page_generation: u64,
    /// Active AI commit generation job, if one is running.
    ai_commit_job: Option<AiCommitJob>,
    /// Generation counter used to discard stale AI results after cancellation.
    ai_commit_generation: u64,
    /// Diff source for the next AI commit message generation.
    ai_commit_source: AiCommitSource,
    /// Receiver for background remote operations (push, pull, fetch).
    remote_op_rx: mpsc::Receiver<Result<Option<HeadState>>>,
    /// Sender cloned into background threads for remote operations.
    remote_op_tx: mpsc::Sender<Result<Option<HeadState>>>,
    /// Async light files refresh (status-only) so Space-spam doesn't freeze.
    files_refresh_rx: Option<mpsc::Receiver<Result<Vec<crate::model::File>>>>,
    /// Interval refresh of the Files scope only: files with diff stats, plus
    /// the repo-wide `(added, deleted)` totals.
    #[allow(clippy::type_complexity)]
    interval_files_rx: Option<
        mpsc::Receiver<
            Result<(
                Option<u64>,
                Option<(Vec<crate::model::File>, (usize, usize))>,
            )>,
        >,
    >,
    /// Signature of the last Files scope the interval tick loaded; equal
    /// signatures skip the diff-stat calls (see `GitCommands::files_signature`).
    interval_files_signature: Option<u64>,
    /// Git metadata fingerprint taken when the last full reload started. The
    /// interval refresh only pays for a full reload when it changes.
    refs_fingerprint: Option<u64>,
    files_refresh_in_progress: bool,
    /// Background `git ls-files` for the path-filter picker (ctrl-s → path).
    filter_paths_rx: Option<mpsc::Receiver<Result<Vec<String>>>>,
    /// Receiver for silent auto-fetch results. Kept separate from remote_op
    /// so auto-fetch failures don't show error popups or clobber a
    /// user-initiated push/pull.
    auto_fetch_rx: mpsc::Receiver<Result<bool>>,
    /// Sender cloned into background threads for auto-fetch.
    auto_fetch_tx: mpsc::Sender<Result<bool>>,
    /// When the last auto-fetch started. `None` means we haven't fetched yet;
    /// the main loop kicks off an immediate fetch on startup.
    last_auto_fetch_at: Option<Instant>,
    /// True while a background auto-fetch is in flight, so we don't stack them.
    auto_fetch_in_flight: bool,
    /// Receiver for background menu item operations (e.g. fetching PR URLs).
    menu_async_rx: mpsc::Receiver<Result<popup::MenuAsyncResult>>,
    /// Sender cloned into background threads for menu async operations.
    pub(crate) menu_async_tx: mpsc::Sender<Result<popup::MenuAsyncResult>>,
    /// Undo stack: stores reflog hashes for undo/redo.
    undo_reflog_idx: usize,
    /// Patch building mode state.
    pub patch_building: PatchBuildingState,
    /// Diff/compare mode state.
    pub diff_mode: DiffModeState,
    /// Interactive rebase mode state.
    pub rebase_mode: RebaseModeState,
    /// Stashed commit editor popup while commit menu or AI generation is shown.
    pending_commit_popup: Option<PopupState>,
    /// Persists the commit editor across Esc so re-opening doesn't lose typed text.
    /// Cleared on successful commit or explicit Clear from the commit menu.
    pub(crate) saved_commit_popup: Option<PopupState>,
    /// Temporarily holds a menu popup during action execution so async actions can restore it.
    pending_menu_popup: Option<PopupState>,
    /// Search bar textarea (1-line editor for search input).
    search_textarea: Option<ratatui_textarea::TextArea<'static>>,
    /// Last time a refresh occurred (for 10s background auto-refresh interval).
    last_refresh_at: Instant,
    /// Last terminal event (key, mouse, paste, resize). Drives how eagerly the
    /// loop wakes: results of what the user just did must paint promptly,
    /// while a truly idle UI has nothing to redraw.
    last_input_at: Instant,
    /// Process start, the clock for spinner animation.
    started_at: Instant,
    /// Active filters applied to the commits panel (branch, path, author).
    pub commit_filter: CommitFilterState,
    /// Hash of the commit whose files are being viewed in CommitFiles context.
    pub commit_files_hash: String,
    /// First line of the commit message for the commit being viewed.
    pub commit_files_message: String,
    /// Cached commit file tree nodes for the CommitFiles view.
    pub commit_file_tree_nodes: Vec<CommitFileTreeNode>,
    /// Set of collapsed directory paths in the commit file tree.
    pub commit_files_collapsed_dirs: HashSet<String>,
    /// Whether to show tree view for commit files (mirrors show_file_tree).
    pub show_commit_file_tree: bool,
    /// Name of the branch/tag whose commits are being viewed in BranchCommits context.
    pub branch_commits_name: String,
    /// Name of the remote whose branches are being viewed in RemoteBranches context.
    pub remote_branches_name: String,
    /// Parent context to return to when pressing Esc from BranchCommits.
    pub sub_commits_parent_context: context::ContextId,
    /// Parent context to return to when pressing Esc from CommitFiles.
    pub commit_files_parent_context: Option<context::ContextId>,
    /// Whether to show all branches in commits log or only HEAD.
    pub all_branches_log: bool,
    /// Receiver for streamed model parts during initial load or background
    /// refresh. Each git data type arrives independently so the UI can
    /// waterfall-display results. Set to `None` once all parts received.
    initial_load_rx: Option<mpsc::Receiver<ModelPart>>,
    /// How many model parts have arrived so far (out of MODEL_PART_COUNT).
    initial_load_received: usize,
    /// True while a background `load_model_streaming` refresh is in flight.
    /// Prevents stacking concurrent full refreshes on the UI thread.
    refresh_in_progress: bool,
    /// Frame counter for the loading spinner animation.
    spinner_frame: usize,
    /// Draw a rounded frame around the whole app (`gui.outerFrame`, or a
    /// gradient set through `LAZYGITRS_BORDER_GRADIENT`).
    outer_frame: bool,
    /// Label shown on the head branch during a remote operation (e.g. "Pushing", "Pulling").
    remote_op_label: Option<String>,
    /// Timestamp when the last remote operation succeeded (for showing a temporary ✓).
    remote_op_success_at: Option<Instant>,
    /// Branch name from checkout-by-name; used to offer create-on-miss when checkout fails.
    pub(crate) pending_checkout_by_name: Option<String>,
    /// Editor to run after suspending the TUI (hx/nvim/vim).
    pub editor_handoff: EditorHandoff,
    /// Copied commit hashes for cherry-pick paste (newest first).
    pub cherry_pick_clipboard: Vec<String>,
    /// Anchor index for range selection in commits list (None = not in range mode).
    pub range_select_anchor: Option<usize>,
    /// History of previously submitted commit messages (most recent first).
    pub commit_message_history: Vec<String>,
    /// Current index into commit_message_history when cycling (None = not cycling).
    pub commit_history_idx: Option<usize>,
    /// Stashed current draft when cycling through history.
    commit_history_draft: String,
    /// Current color theme index into COLOR_THEMES.
    pub current_theme_index: usize,
    /// Cache of shortstat summaries per commit hash.  Populated asynchronously
    /// by background threads so the render path never blocks on git.
    pub commit_stats_cache:
        std::sync::Arc<std::sync::Mutex<HashMap<String, crate::model::commit::CommitStat>>>,
    /// Cache of full commit messages (subject + body) per hash, fetched
    /// asynchronously so the details panel can render the full description.
    pub commit_messages_cache: std::sync::Arc<std::sync::Mutex<HashMap<String, String>>>,
    /// Latest-only queue for commit metadata shown below the commit list.
    commit_details_job_tx: mpsc::Sender<BackgroundJob>,
    /// Invalidates commit-detail jobs when selection changes while one is running.
    commit_details_generation: Arc<AtomicU64>,
    /// Commit hash most recently considered for details loading.
    last_commit_details_key: String,
    /// Vertical scroll offset (rows) for the commit-details box.  Reset
    /// whenever the selected commit hash changes.
    pub commit_details_scroll: u16,
    /// Hash the current `commit_details_scroll` value corresponds to.  When
    /// render sees a different hash, it resets the scroll.
    pub commit_details_scroll_hash: String,
    /// Whether the commit-details box is visible.  Toggled with `.` in any
    /// commit-related context.
    pub show_commit_details: bool,
    /// Whether the mouse is currently hovering the AI-generate button (✦)
    /// in the commit message popup. Drives tooltip visibility.
    pub commit_ai_button_hovered: bool,
    /// Whether we started directly in diff view (e.g. lazygit --diff)
    pub started_in_diff_mode: bool,
    /// Whether this is a popup instance (popup.yaml config)
    pub is_popup: bool,
    pub grab_column_hovered: bool,
    pub grab_column_dragging: Option<u16>,
    /// True after pressing `Ctrl+W` while waiting for the second key of a window-resize chord (`h`/`l`/`j`/`k`/`=`).
    pub window_prefix_armed: bool,
    /// Timestamp of the first root-level `Esc` in full-screen mode (requires a second `Esc` within 400ms to quit).
    pub root_esc_armed_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenMode {
    Normal,
    Half,
    Full,
}

/// True when a `git checkout <name>` failure means the ref does not exist.
pub(crate) fn is_checkout_ref_not_found(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("did not match any file(s) known to git")
        || lower.contains("unknown revision or path not in the working tree")
        || lower.contains("invalid reference:")
}

/// Pathspec for a tree-node path: root (".") => empty (whole tree), dirs get a
/// trailing slash so git matches the directory contents.
fn pathspec_for_tree_path(path: &str) -> Option<String> {
    if path.is_empty() || path == "." {
        return None; // whole tree / no path filter
    }
    if path.ends_with('/') {
        Some(path.to_string())
    } else {
        Some(format!("{}/", path))
    }
}

/// Synthesize a unified diff for a new (untracked) file from its raw content.
/// This allows untracked files to be included in combined multi-file diffs.
fn synthesize_new_file_diff(filename: &str, content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let count = lines.len();
    let mut diff = String::new();
    diff.push_str(&format!("diff --git a/{f} b/{f}\n", f = filename));
    diff.push_str("new file mode 100644\n");
    diff.push_str("--- /dev/null\n");
    diff.push_str(&format!("+++ b/{}\n", filename));
    diff.push_str(&format!("@@ -0,0 +1,{} @@\n", count));
    for line in &lines {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

/// Synthetic binary diff so the pager renders its unavailable-preview placeholder.
fn synthesize_binary_file_diff(filename: &str) -> String {
    format!(
        "diff --git a/{f} b/{f}\n\
         new file mode 100644\n\
         index 0000000..1111111\n\
         Binary files /dev/null and b/{f} differ\n",
        f = filename
    )
}

/// Pure renames have no hunks; show file content instead of only path lines.
fn parse_file_diff_payload(
    git: &GitCommands,
    name: &str,
    current_path: &str,
    diff: &str,
    exists: bool,
    prefer_staged: bool,
) -> DiffPayload {
    if is_rename_only_diff(diff) {
        let content = if prefer_staged {
            git.file_content_staged(current_path)
                .or_else(|_| git.file_content(current_path))
        } else {
            git.file_content(current_path)
                .or_else(|_| git.file_content_staged(current_path))
        };
        if let Ok(content) = content
            && !content.is_empty()
        {
            return DiffPayload::Parsed(DiffViewState::parse_content(
                current_path,
                &content,
                &content,
                4,
                exists,
            ));
        }
    }
    DiffPayload::Parsed(DiffViewState::parse_diff_output(name, diff, 4, exists))
}

fn parse_commit_file_diff_payload(
    git: &GitCommands,
    hash: &str,
    name: &str,
    current_path: &str,
    diff: &str,
) -> DiffPayload {
    let paths = crate::git::diff::diff_paths_for_label(name);
    let parent = format!("{hash}^1");
    if let Some(parsed) = image_diff_payload(
        git,
        name,
        diff,
        false,
        crate::pager::image_preview::ImageSource::Revision {
            revision: &parent,
            path: paths[0],
        },
        crate::pager::image_preview::ImageSource::Revision {
            revision: hash,
            path: current_path,
        },
    ) {
        return DiffPayload::Parsed(parsed);
    }
    if is_rename_only_diff(diff)
        && let Ok(content) = git.file_content_at_commit(hash, current_path)
        && !content.is_empty()
    {
        return DiffPayload::Parsed(DiffViewState::parse_content(
            current_path,
            &content,
            &content,
            4,
            false,
        ));
    }
    DiffPayload::Parsed(DiffViewState::parse_diff_output(name, diff, 4, false))
}

/// Only single raster-file selections are previewed; multi-file buffers retain
/// their ordinary binary placeholders. Decoding validates the actual bytes.
fn image_diff_payload(
    git: &GitCommands,
    name: &str,
    diff: &str,
    exists: bool,
    old: crate::pager::image_preview::ImageSource<'_>,
    new: crate::pager::image_preview::ImageSource<'_>,
) -> Option<crate::pager::side_by_side::ParsedDiff> {
    let paths = crate::git::diff::diff_paths_for_label(name);
    if !paths
        .iter()
        .any(|p| crate::pager::image_preview::is_image_path(p))
    {
        return None;
    }
    let preview = crate::pager::image_preview::load(git, old, new)?;
    // A rename-only image has no binary marker, but must still be a nonempty
    // diff view without text hunks (image content cannot be staged by hunk).
    let placeholder = synthesize_binary_file_diff(name);
    let mut parsed = DiffViewState::parse_diff_output(
        name,
        if diff.is_empty() || is_rename_only_diff(diff) {
            &placeholder
        } else {
            diff
        },
        4,
        exists,
    );
    parsed.image_preview = Some(preview);
    Some(parsed)
}

impl Gui {
    fn show_error(&mut self, title: &str, err: anyhow::Error) {
        self.popup = PopupState::Message {
            title: title.to_string(),
            message: format!("{:#}", err),
            kind: MessageKind::Error,
        };
    }

    pub fn new(
        config: AppConfig,
        git: GitCommands,
        start_in_diff: bool,
        filter_file: Option<String>,
        is_popup: bool,
        filter_path: Option<PathBuf>,
        start_in_commits: bool,
    ) -> Result<Self> {
        let (diff_tx, diff_rx) = mpsc::channel();
        let (diff_scheduler_tx, diff_scheduler_rx) = mpsc::channel();
        let (diff_prefetch_tx, diff_prefetch_rx) = mpsc::channel();
        let (commit_details_job_tx, commit_details_job_rx) = mpsc::channel();
        let (ai_commit_tx, ai_commit_rx) = mpsc::channel();
        let (commit_page_tx, commit_page_rx) = mpsc::channel();
        let (remote_op_tx, remote_op_rx) = mpsc::channel();
        let (auto_fetch_tx, auto_fetch_rx) = mpsc::channel();
        let (menu_async_tx, menu_async_rx) = mpsc::channel();
        let (acp_tx, acp_rx) = mpsc::channel();
        // Restore session ID and server URL from .lines.json if available
        // (survives restarts).
        let persisted = crate::pager::notes_store::load(git.repo_path()).session;
        let persisted_session = persisted.as_ref().map(|s| s.session_id.clone());
        let persisted_server_url = persisted
            .as_ref()
            .map(|s| s.server_url.clone())
            .filter(|u| !u.is_empty());
        let persisted_notify_cmd =
            crate::pager::notes_store::trusted_notify_command(git.repo_path());
        let acp_session_id: crate::acp::SessionId = Arc::new(Mutex::new(persisted_session));
        let acp_server_url: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(persisted_server_url));
        let acp_notify_command: Arc<Mutex<Option<String>>> =
            Arc::new(Mutex::new(persisted_notify_cmd));
        let sse_tx = crate::acp::spawn_server(
            acp_tx,
            git.repo_path().to_path_buf(),
            acp_session_id.clone(),
            acp_server_url.clone(),
            acp_notify_command.clone(),
        );
        let show_file_tree = config
            .app_state
            .show_file_tree
            .unwrap_or(config.user_config.gui.show_file_tree);
        let show_command_log_default = config
            .app_state
            .show_command_log
            .unwrap_or(config.user_config.gui.show_command_log);
        let diff_line_wrap = config.app_state.diff_line_wrap.unwrap_or(false);
        let diff_view_layout = config
            .app_state
            .diff_view
            .as_deref()
            .and_then(DiffViewLayout::from_state_value)
            .unwrap_or({
                if config.user_config.gui.side_by_side {
                    DiffViewLayout::SideBySide
                } else {
                    DiffViewLayout::Unified
                }
            });
        let show_commit_details = config.app_state.show_commit_details.unwrap_or(true);
        let command_log = crate::os::cmd::new_command_log();
        crate::os::cmd::set_thread_command_log(command_log.clone());
        if !config.user_config.unknown_keys.is_empty() {
            command_log
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(format!(
                    "[config] Unknown config key(s) ignored: {}",
                    config.user_config.unknown_keys.join(", ")
                ));
        }
        let shadowed = keymap::shadowed_bindings(&config.user_config.keybinding);
        if !shadowed.is_empty() {
            command_log
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(format!(
                    "[config] Keybinding(s) that can never fire: {}",
                    shadowed.join("; ")
                ));
        }

        // Start with an empty model — each piece of data loads in the
        // background and streams in as it becomes ready, so the UI can
        // paint immediately and waterfall-display results.
        let git = Arc::new(git);
        let refs_fingerprint = git.refs_fingerprint();
        let diff_generation = Arc::new(AtomicU64::new(0));
        let commit_details_generation = Arc::new(AtomicU64::new(0));
        spawn_diff_scheduler(
            diff_scheduler_rx,
            diff_scheduler_tx.clone(),
            diff_tx.clone(),
            Arc::clone(&diff_generation),
        );
        spawn_diff_prefetch_workers(diff_prefetch_rx, diff_tx.clone());
        spawn_latest_background_worker(commit_details_job_rx);
        // Compile tree-sitter highlight queries off the critical path so the
        // first diff shown doesn't pay the ~40-60ms lazy-init cost.
        std::thread::spawn(crate::pager::highlight::warm_configs);
        let mut model = Model::default();
        model.repo_name = git.repo_name();
        model.head_hash = git.head_hash().unwrap_or_default();
        model.head_branch_name = git.current_branch_name().unwrap_or_default();

        // Normalize `-f` to a repo-relative path before kicking off the stream
        // so the first Commits part is already filtered (lazygit-style).
        let startup_path_filter = filter_path.and_then(|p| {
            let raw = p.to_string_lossy().trim().to_string();
            if raw.is_empty() {
                return None;
            }
            let relative = p
                .canonicalize()
                .ok()
                .and_then(|abs| {
                    abs.strip_prefix(git.repo_path())
                        .ok()
                        .map(|rel| rel.to_string_lossy().to_string())
                })
                .filter(|rel| !rel.is_empty())
                .unwrap_or(raw);
            Some(relative)
        });
        let startup_commit_filter =
            startup_path_filter
                .as_ref()
                .map(|path| crate::git::commit::CommitFilter {
                    path: Some(path.clone()),
                    ..Default::default()
                });

        let (initial_load_tx, initial_load_rx) = mpsc::channel();
        git.load_model_streaming(&initial_load_tx, startup_commit_filter);

        let commit_history = Self::load_commit_history(&config);

        // Resolve color theme: explicit `gui.theme: <preset>` in config.yml takes precedence over saved state.yml
        let current_theme_index = config
            .user_config
            .gui
            .theme
            .preset
            .as_deref()
            .or(config.app_state.color_theme.as_deref())
            .and_then(|id| {
                let norm = id.trim().to_ascii_lowercase().replace('_', "-");
                crate::config::COLOR_THEMES
                    .iter()
                    .position(|t| t.id == norm)
            })
            .unwrap_or(0);

        let mut context_mgr = ContextManager::new();
        if startup_path_filter.is_some() || start_in_commits {
            context_mgr.set_active(ContextId::Commits);
            context_mgr.set_selection_for(ContextId::Commits, 0);
        }

        // Like waymaker's `WM_BORDER_GRADIENT`: a session-wide gradient also
        // turns the frame on, so the app reads as a popup.
        let outer_frame = config.user_config.gui.outer_frame
            || std::env::var("LAZYGITRS_BORDER_GRADIENT")
                .is_ok_and(|spec| gradient::parse_stops(&spec).is_some());

        Ok(Self {
            config: Arc::new(config),
            git,
            model: Arc::new(Mutex::new(model)),
            initial_load_rx: Some(initial_load_rx),
            initial_load_received: 0,
            refresh_in_progress: false,
            context_mgr,
            layout: LayoutState::default(),
            popup: PopupState::None,
            diff_view: {
                let mut dv = DiffViewState::new();
                dv.wrap = diff_line_wrap;
                dv.view_layout = diff_view_layout;
                dv
            },
            commit_list_cache: presentation::commits::CommitListCache::default(),
            command_log,
            show_command_log: show_command_log_default,
            should_quit: false,
            quit_reason: QuitReason::default(),
            needs_refresh: false,
            needs_files_refresh: false,
            needs_diff_refresh: true,
            pending_navigation: None,
            start_in_diff,
            start_file_filter: filter_file,
            search_query: String::new(),
            search_active: false,
            search_matches: Vec::new(),
            search_match_idx: 0,
            screen_mode: ScreenMode::Normal,
            sidebar_resizing: false,
            sidebar_resize_row_offset: 0,
            show_file_tree,
            file_tree_nodes: Vec::new(),
            collapsed_dirs: HashSet::new(),
            diff_focused: false,
            diff_loading: false,
            diff_loading_since: None,
            last_diff_key: String::new(),
            diff_generation,
            diff_rx,
            diff_scheduler_tx,
            diff_preview_cache: DiffPreviewCache::default(),
            displayed_diff_key: String::new(),
            diff_prefetch_tx,
            diff_prefetch_inflight: HashSet::new(),
            ai_commit_rx,
            ai_commit_tx,
            acp: AcpState {
                rx: acp_rx,
                session_id: acp_session_id,
                sse_tx,
                server_url: acp_server_url,
                notify_command: acp_notify_command,
            },
            commit_page_rx,
            commit_page_tx,
            commit_page_loading: false,
            commit_history_complete: false,
            commit_page_generation: 0,
            ai_commit_job: None,
            ai_commit_generation: 0,
            ai_commit_source: AiCommitSource::Staged,
            remote_op_rx,
            remote_op_tx,
            files_refresh_rx: None,
            interval_files_rx: None,
            interval_files_signature: None,
            refs_fingerprint,
            files_refresh_in_progress: false,
            filter_paths_rx: None,
            auto_fetch_rx,
            auto_fetch_tx,
            last_auto_fetch_at: None,
            auto_fetch_in_flight: false,
            menu_async_rx,
            menu_async_tx,
            undo_reflog_idx: 0,
            patch_building: PatchBuildingState::new(),
            diff_mode: DiffModeState::new(),
            rebase_mode: RebaseModeState::new(),
            pending_commit_popup: None,
            saved_commit_popup: None,
            pending_menu_popup: None,
            search_textarea: None,
            last_refresh_at: Instant::now(),
            last_input_at: Instant::now(),
            started_at: Instant::now(),
            commit_filter: CommitFilterState {
                branch: Vec::new(),
                path: startup_path_filter.clone(),
                author: Vec::new(),
            },
            commit_files_hash: String::new(),
            commit_files_message: String::new(),
            commit_file_tree_nodes: Vec::new(),
            commit_files_collapsed_dirs: HashSet::new(),
            show_commit_file_tree: show_file_tree,
            branch_commits_name: String::new(),
            remote_branches_name: String::new(),
            sub_commits_parent_context: context::ContextId::Branches,
            commit_files_parent_context: None,
            all_branches_log: true,
            spinner_frame: 0,
            outer_frame,
            remote_op_label: None,
            remote_op_success_at: None,
            pending_checkout_by_name: None,
            editor_handoff: EditorHandoff::default(),
            cherry_pick_clipboard: Vec::new(),
            range_select_anchor: None,
            commit_message_history: commit_history,
            commit_history_idx: None,
            commit_history_draft: String::new(),
            current_theme_index,
            commit_stats_cache: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            commit_messages_cache: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            commit_details_job_tx,
            commit_details_generation,
            last_commit_details_key: String::new(),
            commit_details_scroll: 0,
            commit_details_scroll_hash: String::new(),
            show_commit_details,
            commit_ai_button_hovered: false,
            started_in_diff_mode: start_in_diff,
            is_popup,
            grab_column_hovered: false,
            grab_column_dragging: None,
            window_prefix_armed: false,
            root_esc_armed_at: None,
        })
    }
}

/// Split a commit message into (summary, body).
/// The summary is the first line; the body is everything after the first blank line separator.
fn split_commit_message(msg: &str) -> (String, String) {
    match msg.find('\n') {
        Some(idx) => {
            let summary = msg[..idx].to_string();
            let rest = msg[idx + 1..].trim_start_matches('\n').to_string();
            (summary, rest)
        }
        None => (msg.to_string(), String::new()),
    }
}

/// Auto-wrap all lines in a textarea so no line exceeds `wrap_width`.
/// Rebuilds the entire textarea content with hard line breaks at word boundaries.
/// Soft-wrap: like `auto_wrap_textarea` but preserves every character (including
/// spaces at line breaks). Inserts visual newlines only — callers join with `""`
/// at submit time to recover the original string. Used for single-line popup
/// inputs (branch name, tag name, etc.) that need browser-textarea-style visual
/// wrapping without polluting the value sent downstream.
fn soft_wrap_textarea(textarea: &mut ratatui_textarea::TextArea<'static>, wrap_width: usize) {
    if wrap_width == 0 {
        return;
    }

    let raw: String = textarea.lines().join("");
    if raw.is_empty() {
        return;
    }
    let chars: Vec<char> = raw.chars().collect();

    // Skip if already laid out correctly: every line ≤ wrap_width, and every
    // non-final line is exactly wrap_width chars.
    let lines = textarea.lines();
    let last = lines.len().saturating_sub(1);
    let already_ok = lines.iter().enumerate().all(|(i, l)| {
        let n = l.chars().count();
        if i < last {
            n == wrap_width
        } else {
            n <= wrap_width
        }
    });
    if already_ok {
        return;
    }

    // Track absolute char offset of cursor so we can restore it after rewrap.
    let ratatui_textarea::DataCursor(cursor_row, cursor_col) = textarea.cursor();
    let mut cursor_abs = 0usize;
    for (i, line) in textarea.lines().iter().enumerate() {
        let line_chars = line.chars().count();
        if i < cursor_row {
            cursor_abs += line_chars;
        } else {
            cursor_abs += cursor_col.min(line_chars);
            break;
        }
    }

    let mut wrapped: Vec<String> = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + wrap_width).min(chars.len());
        wrapped.push(chars[start..end].iter().collect());
        start = end;
    }
    let new_text = wrapped.join("\n");

    // Map cursor back into the wrapped layout (each row is exactly wrap_width
    // chars except possibly the last).
    let new_row = cursor_abs / wrap_width;
    let new_col = cursor_abs % wrap_width;

    textarea.select_all();
    textarea.cut();
    textarea.insert_str(&new_text);
    textarea.move_cursor(ratatui_textarea::CursorMove::Top);
    textarea.move_cursor(ratatui_textarea::CursorMove::Head);
    for _ in 0..new_row {
        textarea.move_cursor(ratatui_textarea::CursorMove::Down);
    }
    for _ in 0..new_col {
        textarea.move_cursor(ratatui_textarea::CursorMove::Forward);
    }
}

fn auto_wrap_textarea(textarea: &mut ratatui_textarea::TextArea<'static>, wrap_width: usize) {
    if wrap_width == 0 {
        return;
    }

    let needs_wrap = textarea.lines().iter().any(|l| l.len() > wrap_width);
    if !needs_wrap {
        return;
    }

    // Compute cursor's absolute char offset in the original text
    let ratatui_textarea::DataCursor(cursor_row, cursor_col) = textarea.cursor();
    let original_lines: Vec<String> = textarea.lines().iter().map(|s| s.to_string()).collect();

    let mut cursor_abs = 0usize;
    for (i, line) in original_lines.iter().enumerate() {
        if i < cursor_row {
            cursor_abs += line.len() + 1;
        } else {
            cursor_abs += cursor_col.min(line.len());
            break;
        }
    }

    // Word-wrap all lines
    let mut wrapped: Vec<String> = Vec::new();
    for line in &original_lines {
        if line.len() <= wrap_width {
            wrapped.push(line.clone());
        } else {
            let mut remaining = line.as_str();
            while remaining.len() > wrap_width {
                let break_at = remaining[..wrap_width].rfind(' ').unwrap_or(wrap_width);
                let break_at = if break_at == 0 { wrap_width } else { break_at };
                wrapped.push(remaining[..break_at].to_string());
                remaining = remaining[break_at..].trim_start();
            }
            if !remaining.is_empty() {
                wrapped.push(remaining.to_string());
            }
        }
    }

    let new_text = wrapped.join("\n");

    // Map the absolute cursor offset into the new wrapped text
    // The wrapping only adds newlines (replacing spaces), so character content
    // is preserved. Walk the new text to find the right row/col.
    let mut abs = 0usize;
    let mut new_row = 0;
    let mut new_col = 0;
    for (i, wline) in wrapped.iter().enumerate() {
        if abs + wline.len() >= cursor_abs {
            new_row = i;
            new_col = (cursor_abs - abs).min(wline.len());
            break;
        }
        abs += wline.len() + 1; // +1 for newline
        new_row = i + 1;
        new_col = 0;
    }

    // Replace content and restore cursor
    textarea.select_all();
    textarea.cut();
    textarea.insert_str(&new_text);

    textarea.move_cursor(ratatui_textarea::CursorMove::Top);
    textarea.move_cursor(ratatui_textarea::CursorMove::Head);
    for _ in 0..new_row {
        textarea.move_cursor(ratatui_textarea::CursorMove::Down);
    }
    for _ in 0..new_col {
        textarea.move_cursor(ratatui_textarea::CursorMove::Forward);
    }
}

/// Read text from the system clipboard.
fn read_clipboard() -> Option<String> {
    let cmd = if cfg!(target_os = "macos") {
        "pbpaste"
    } else if cfg!(target_os = "windows") {
        "powershell.exe -command Get-Clipboard"
    } else {
        "xclip -selection clipboard -o"
    };

    std::process::Command::new("sh")
        .args(["-c", cmd])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn rect_contains(r: ratatui::layout::Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

fn keyboard_enhancement_flags() -> crossterm::event::KeyboardEnhancementFlags {
    // Keep printable input on the terminal's normal text path. In particular,
    // REPORT_ALL_KEYS_AS_ESCAPE_CODES replaces produced text with a logical key
    // identity. Crossterm 0.28 does not expose the protocol's associated-text
    // field, so keyboard layouts, IMEs, or remappers can otherwise turn a typed
    // character into a different shortcut (for example, `q` into `u`).
    //
    // REPORT_EVENT_TYPES is also unnecessary: the UI handles press events only,
    // and enabling it would turn key auto-repeat into ignored Repeat events.
    crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
}

/// Enables button, drag, and scroll events without passive pointer-motion events.
///
/// Crossterm's `EnableMouseCapture` also enables DEC mode 1003 (all motion), which can
/// cause terminals to repeatedly focus or redraw while merely moving the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnableMouseCaptureWithoutHover;

impl Command for EnableMouseCaptureWithoutHover {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1000h\x1b[?1002h\x1b[?1015h\x1b[?1006h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Command::execute_winapi(&crossterm::event::EnableMouseCapture)
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        Command::is_ansi_code_supported(&crossterm::event::EnableMouseCapture)
    }
}

fn setup_terminal() -> Result<(Term, bool)> {
    terminal::enable_raw_mode()?;
    // Prefer /dev/tty when stdout is piped (Helix `:insert-output`, etc.).
    let mut out =
        crate::os::tty::open_tui_output().context("Failed to open terminal output for TUI")?;
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCaptureWithoutHover,
        crossterm::event::EnableFocusChange,
        crossterm::event::EnableBracketedPaste,
        crossterm::style::Print("\x1b[?1003h"), // Enable Any-event mouse tracking for Hover
        cursor::Hide
    )?;
    // Detect graphics before Crossterm creates its internal response reader.
    crate::pager::image_preview::initialize();
    // Helix leaves progressive kitty keyboard enhancement enabled across
    // `:insert-output` and keeps a `/dev/tty` EventStream open. Probing races
    // that reader (blank hang); instead pop leftover stacks and push our flags
    // so CSI-u keys parse as KeyEvents.
    let keyboard_enhanced = if crate::os::tty::nested_tty_launch() {
        for _ in 0..4 {
            let _ = execute!(out, crossterm::event::PopKeyboardEnhancementFlags);
        }
        true
    } else {
        crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
    };
    if keyboard_enhanced {
        execute!(
            out,
            crossterm::event::PushKeyboardEnhancementFlags(keyboard_enhancement_flags())
        )?;
    }
    let backend = CrosstermBackend::new(out);
    let terminal = Terminal::new(backend)?;
    Ok((terminal, keyboard_enhanced))
}

/// Shell-escape a string so it can be safely passed as a single argument.
/// Wraps in single quotes and escapes any embedded single quotes.
fn shell_escape_arg(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Put the terminal back the way we found it.
///
/// Nothing drains leftover input here: crossterm guards its reader with a
/// process-wide mutex that the input thread holds for the duration of its
/// blocking read, so any drain from this thread would silently no-op.
fn restore_terminal(terminal: &mut Term, keyboard_enhanced: bool) -> Result<()> {
    // Helix `:insert-output` keeps its own alt-screen / raw mode / mouse / focus
    // / bracketed-paste / kitty stack across the child. If we tear those down
    // here, Helix resumes drawing into a world that no longer exists and the
    // user is left staring at the primary-screen `hx` launch line with a dead
    // TUI. Nested exit must only undo *our* transient state and hand the tty
    // foreground back.
    let nested = crate::os::tty::nested_tty_launch();
    if nested {
        // Hide our cursor leftovers, pop the kitty flags we pushed, then put
        // Helix's progressive flags back (DISAMBIGUATE | REPORT_ALTERNATE_KEYS).
        execute!(terminal.backend_mut(), cursor::Show)?;
        if keyboard_enhanced {
            execute!(
                terminal.backend_mut(),
                crossterm::event::PopKeyboardEnhancementFlags
            )?;
            execute!(
                terminal.backend_mut(),
                crossterm::event::PushKeyboardEnhancementFlags(
                    crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | crossterm::event::KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                )
            )?;
        }
        terminal.backend_mut().flush()?;
        crate::os::tty::restore_foreground_tty();
        return Ok(());
    }

    // Standalone / non-nested: full teardown.
    crate::os::tty::restore_foreground_tty();
    if keyboard_enhanced {
        execute!(
            terminal.backend_mut(),
            crossterm::event::PopKeyboardEnhancementFlags
        )?;
    }
    execute!(
        terminal.backend_mut(),
        crossterm::style::Print("\x1b[?1003l"),
        crossterm::event::DisableMouseCapture,
        crossterm::event::DisableFocusChange,
        crossterm::event::DisableBracketedPaste,
        cursor::Show,
        LeaveAlternateScreen
    )?;

    terminal::disable_raw_mode()?;
    Ok(())
}

#[cfg(test)]
mod tests;
