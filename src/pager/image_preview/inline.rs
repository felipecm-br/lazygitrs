//! Lazy, viewport-driven inline image previews for multi-file diffs.

use super::*;

/// Fixed scroll height of an inline image section, including labels.
pub const INLINE_IMAGE_ROWS: usize = 12;
pub(super) const MAX_INLINE_LOADS: usize = 2;
pub(super) static INLINE_LOADS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Owned source identity; delayed loads must not borrow a diff job's buffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InlineImageSource {
    Worktree(String),
    Revision { revision: String, path: String },
    Missing,
}

impl InlineImageSource {
    pub(super) fn borrowed(&self) -> ImageSource<'_> {
        match self {
            Self::Worktree(path) => ImageSource::Worktree(path),
            Self::Revision { revision, path } => ImageSource::Revision { revision, path },
            Self::Missing => ImageSource::Missing,
        }
    }
}

pub(super) enum InlineLoadState {
    Idle,
    Loading(Receiver<Option<ImagePreview>>),
    Ready(ImagePreview),
    Unavailable,
}

/// Lazy, viewport-driven image state for one file in a multi-file diff.
/// Offscreen sections keep only source metadata; at most two decodes run at once.
pub struct InlineImagePreview {
    pub(super) repo: std::path::PathBuf,
    pub(super) old: InlineImageSource,
    pub(super) new: InlineImageSource,
    pub(super) picker: Picker,
    pub(super) state: InlineLoadState,
}

impl InlineImagePreview {
    pub(super) fn new(repo: &Path, old: InlineImageSource, new: InlineImageSource) -> Option<Self> {
        let picker = global_picker_ref()?.clone();
        // Inline overlays need placeholder semantics to clear individual rows
        // without erasing adjacent text. Experimental Sixel/iTerm2 stay striped.
        if picker.protocol_type() != ProtocolType::Kitty {
            return None;
        }
        Some(Self::with_picker(repo, old, new, picker))
    }

    pub(super) fn with_picker(
        repo: &Path,
        old: InlineImageSource,
        new: InlineImageSource,
        picker: Picker,
    ) -> Self {
        Self {
            repo: repo.to_path_buf(),
            old,
            new,
            picker,
            state: InlineLoadState::Idle,
        }
    }

    /// Release pixel/protocol memory when this section leaves the viewport.
    pub fn unload(&mut self) {
        if let InlineLoadState::Loading(rx) = &self.state {
            match rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => return, // Keep a pending job; don't duplicate it.
                Ok(None) => {
                    self.state = InlineLoadState::Unavailable;
                    return;
                }
                _ => {} // Discard offscreen pixels, including a completed result.
            }
        }
        if !matches!(self.state, InlineLoadState::Unavailable) {
            self.state = InlineLoadState::Idle;
        }
    }

    /// Cached folders retain identities, not image pixels or in-flight results.
    pub fn unload_for_cache(&mut self) {
        if !matches!(self.state, InlineLoadState::Unavailable) {
            self.state = InlineLoadState::Idle;
        }
    }

    #[cfg(test)]
    pub(crate) fn from_preview_for_test(preview: ImagePreview) -> Self {
        let mut picker = capabilities::picker_for_font_size((8, 16));
        picker.set_protocol_type(ProtocolType::Kitty);
        let mut inline = Self::with_picker(
            Path::new("."),
            InlineImageSource::Missing,
            InlineImageSource::Missing,
            picker,
        );
        inline.state = InlineLoadState::Ready(preview);
        inline
    }

    pub fn estimated_bytes(&self) -> usize {
        let pixels = match &self.state {
            InlineLoadState::Ready(preview) => preview.estimated_bytes(),
            _ => 0,
        };
        pixels
            + std::mem::size_of::<Self>()
            + self.repo.as_os_str().len()
            + source_bytes(&self.old)
            + source_bytes(&self.new)
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, side: DiffSideView, theme: &Theme) {
        if let InlineLoadState::Loading(rx) = &self.state {
            match rx.try_recv() {
                Ok(Some(preview)) => self.state = InlineLoadState::Ready(preview),
                Ok(None) | Err(mpsc::TryRecvError::Disconnected) => {
                    self.state = InlineLoadState::Unavailable
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if matches!(self.state, InlineLoadState::Idle) && area.height > 1 {
            self.start_load();
        }
        match &mut self.state {
            InlineLoadState::Ready(preview) => render(frame, area, preview, side, theme),
            InlineLoadState::Unavailable => {
                inline_placeholder(frame, area, "Binary cannot be previewed", theme)
            }
            _ => inline_placeholder(frame, area, "Loading image…", theme),
        }
    }

    pub(super) fn start_load(&mut self) {
        use std::sync::atomic::Ordering;
        if INLINE_LOADS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_INLINE_LOADS).then_some(n + 1)
            })
            .is_err()
        {
            return;
        }
        struct LoadPermit;
        impl Drop for LoadPermit {
            fn drop(&mut self) {
                INLINE_LOADS.fetch_sub(1, Ordering::AcqRel);
            }
        }
        let permit = LoadPermit;
        let repo = self.repo.clone();
        let old = self.old.clone();
        let new = self.new.clone();
        let picker = self.picker.clone();
        let (tx, rx) = mpsc::channel();
        match std::thread::Builder::new()
            .name("inline-image-load".into())
            .spawn(move || {
                let _permit = permit;
                let preview = (|| {
                    let old = load_one_side(&repo, old.borrowed())?;
                    let new = load_one_side(&repo, new.borrowed())?;
                    ImagePreview::from_decoded(old, new, &picker)
                })();
                let _ = tx.send(preview);
            }) {
            Ok(_) => self.state = InlineLoadState::Loading(rx),
            Err(_) => self.state = InlineLoadState::Unavailable,
        }
    }
}

pub(super) fn source_bytes(source: &InlineImageSource) -> usize {
    match source {
        InlineImageSource::Worktree(path) => path.len(),
        InlineImageSource::Revision { revision, path } => revision.len() + path.len(),
        InlineImageSource::Missing => 0,
    }
}

pub(super) fn inline_placeholder(frame: &mut Frame, area: Rect, message: &str, theme: &Theme) {
    let style = Style::default().fg(theme.text_dimmed);
    frame.render_widget(
        Paragraph::new("╱".repeat(area.width as usize)).style(style),
        area,
    );
    if area.height > 0 {
        let label = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
        frame.render_widget(
            Paragraph::new(message)
                .alignment(ratatui::layout::Alignment::Center)
                .style(style),
            label,
        );
    }
}
