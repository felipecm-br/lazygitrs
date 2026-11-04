//! Before/after image preview for binary diffs.
//!
//! Draws a centered, aspect-preserving image per pane inside the diff panel.
//! The caller owns the outer border/title and hides the preview under
//! overlays via `image_preview_hidden`. Kitty previews use unicode
//! placeholders, so they vanish with the text cells that carry them; Sixel and
//! iTerm2 draw straight onto the screen and persist once drawn. ratatui-image
//! does not expose the Kitty image id, so transmitted image data is left for
//! the terminal to evict.
//!
//! Private modules own capability probing, bounded source loading, and inline
//! diff eligibility. This module owns rendering and graphics lifetimes.

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use image::ImageFormat;
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocolType;
use ratatui_image::thread::{ResizeRequest, ResizeResponse, ThreadProtocol};
use ratatui_image::{Resize, ResizeEncodeRender, StatefulImage};

mod capabilities;
mod inline;
mod inline_diff;
mod source;

use capabilities::global_picker_ref;
pub use capabilities::initialize;
pub use inline::*;
pub(crate) use inline_diff::{attach_inline_image_previews, is_image_path};
pub use source::ImageSource;
use source::{DecodedImage, format_label_for, load_one_side};

use super::side_by_side::DiffSideView;
use crate::config::Theme;

struct PreviewSide {
    width: u32,
    height: u32,
    orig_width: u32,
    orig_height: u32,
    format: Option<ImageFormat>,
    thread: ThreadProtocol,
    rx: Receiver<ResizeResponse>,
    created: Instant,
    // Kitty renders through unicode placeholders; recorded before the protocol
    // moves into the worker.
    placeholders: bool,
}

impl PreviewSide {
    fn new(picker: &Picker, decoded: DecodedImage) -> Self {
        let (tx_req, rx_req) = mpsc::channel::<ResizeRequest>();
        let (tx_resp, rx_resp) = mpsc::channel::<ResizeResponse>();
        // Move the image into the protocol: no retained clone, so memory and
        // cache accounting stay single-copy.
        let width = decoded.image.width();
        let height = decoded.image.height();
        let orig_width = decoded.orig_width;
        let orig_height = decoded.orig_height;
        let format = decoded.format;
        let proto = picker.new_resize_protocol(decoded.image);
        let placeholders = matches!(proto.protocol_type(), StatefulProtocolType::Kitty(_));
        let thread = ThreadProtocol::new(tx_req, Some(proto));
        // Worker only resize+encodes (CPU); it never touches the terminal.
        // It exits when the preview is dropped (sender disconnect).
        std::thread::Builder::new()
            .name("lazygitrs-image-preview".into())
            .spawn(move || {
                while let Ok(req) = rx_req.recv() {
                    match req.resize_encode() {
                        Ok(resp) => {
                            let _ = tx_resp.send(resp);
                        }
                        Err(_) => {
                            // Encode failed; the protocol is consumed by the
                            // ratatui-image API. The UI shows a fallback
                            // message instead of hanging (see `render_side`).
                        }
                    }
                }
            })
            .ok();
        Self {
            width,
            height,
            orig_width,
            orig_height,
            format,
            thread,
            rx: rx_resp,
            created: Instant::now(),
            placeholders,
        }
    }

    fn poll_completed(&mut self) {
        while let Ok(resp) = self.rx.try_recv() {
            let _ = self.thread.update_resized_protocol(resp);
        }
    }

    fn estimated_bytes(&self) -> usize {
        // Only the thumbnail and its encoded payload are retained.
        let dec = (self.width as usize) * (self.height as usize) * 4;
        dec + dec * 4 / 3 + 1024
    }

    fn format_label(&self) -> &'static str {
        format_label_for(self.format)
    }
}

/// Before/after image preview. `Send` so the diff loader can move it across
/// threads; rendering stays on the UI thread.
pub struct ImagePreview {
    old: Option<PreviewSide>,
    new: Option<PreviewSide>,
    font_size: (u16, u16),
}

const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<ImagePreview>();
};

impl ImagePreview {
    /// Load both sides with the global picker. `None` when graphics are
    /// unavailable, both sides are absent, or any present side fails.
    pub fn load(
        git: &crate::git::GitCommands,
        old: ImageSource<'_>,
        new: ImageSource<'_>,
    ) -> Option<Self> {
        let picker = global_picker_ref().cloned()?;
        Self::load_with_picker(git, old, new, &picker)
    }

    /// Picker-injecting load (tests use this, never the global).
    pub(crate) fn load_with_picker(
        git: &crate::git::GitCommands,
        old: ImageSource<'_>,
        new: ImageSource<'_>,
        picker: &Picker,
    ) -> Option<Self> {
        if picker.protocol_type() == ProtocolType::Halfblocks {
            return None;
        }
        let repo = git.repo_path();
        let old_dec = load_one_side(repo, old)?;
        let new_dec = load_one_side(repo, new)?;
        Self::from_decoded(old_dec, new_dec, picker)
    }

    /// Build from already-decoded sides. `None` when both are `None` or the
    /// picker is Halfblocks.
    pub(crate) fn from_decoded(
        old: Option<DecodedImage>,
        new: Option<DecodedImage>,
        picker: &Picker,
    ) -> Option<Self> {
        if picker.protocol_type() == ProtocolType::Halfblocks {
            return None;
        }
        if old.is_none() && new.is_none() {
            return None;
        }
        let font_size = {
            let font = picker.font_size();
            (font.width, font.height)
        };
        Some(Self {
            old: old.map(|d| PreviewSide::new(picker, d)),
            new: new.map(|d| PreviewSide::new(picker, d)),
            font_size,
        })
    }

    /// Approx memory held: decoded + encoded estimates for both sides.
    pub fn estimated_bytes(&self) -> usize {
        let mut n = 0usize;
        if let Some(s) = &self.old {
            n += s.estimated_bytes();
        }
        if let Some(s) = &self.new {
            n += s.estimated_bytes();
        }
        n + std::mem::size_of::<Self>()
    }

    pub fn uses_placeholders(&self) -> bool {
        [&self.old, &self.new]
            .into_iter()
            .flatten()
            .all(|side| side.placeholders)
    }
}

/// Free-function load (forwards to [`ImagePreview::load`]).
pub fn load(
    git: &crate::git::GitCommands,
    old: ImageSource<'_>,
    new: ImageSource<'_>,
) -> Option<ImagePreview> {
    ImagePreview::load(git, old, new)
}

/// Split `area` into `(old_pane, new_pane)` honoring `side`.
pub(crate) fn split_preview_area(area: Rect, side: DiffSideView) -> (Option<Rect>, Option<Rect>) {
    if area.width == 0 || area.height == 0 {
        return (None, None);
    }
    match side {
        DiffSideView::OldOnly => (Some(area), None),
        DiffSideView::NewOnly => (None, Some(area)),
        DiffSideView::Both => {
            if area.width < 2 {
                return (Some(area), None);
            }
            let left_w = area.width / 2;
            let right_w = area.width - left_w;
            (
                Some(Rect::new(area.x, area.y, left_w, area.height)),
                Some(Rect::new(area.x + left_w, area.y, right_w, area.height)),
            )
        }
    }
}

/// Center `inner` inside `outer`.
pub(crate) fn centered_rect(outer: Rect, inner_w: u16, inner_h: u16) -> Rect {
    let w = inner_w.min(outer.width);
    let h = inner_h.min(outer.height);
    let x = outer.x + outer.width.saturating_sub(w) / 2;
    let y = outer.y + outer.height.saturating_sub(h) / 2;
    Rect::new(x, y, w, h)
}

/// Cell-space fit preserving aspect, given decoded pixels + font size.
pub(crate) fn fit_cells(
    img_w: u32,
    img_h: u32,
    font: (u16, u16),
    pane_w: u16,
    pane_h: u16,
) -> (u16, u16) {
    if img_w == 0 || img_h == 0 || font.0 == 0 || font.1 == 0 || pane_w == 0 || pane_h == 0 {
        return (pane_w.min(1), pane_h.min(1));
    }
    let max_px_w = pane_w as u64 * font.0 as u64;
    let max_px_h = pane_h as u64 * font.1 as u64;
    let scale = (max_px_w as f64 / img_w as f64).min(max_px_h as f64 / img_h as f64);
    let scale = scale.min(1.0);
    let fit_px_w = ((img_w as f64 * scale).round() as u64).max(1);
    let fit_px_h = ((img_h as f64 * scale).round() as u64).max(1);
    let cells_w = fit_px_w.div_ceil(font.0 as u64).min(pane_w as u64) as u16;
    let cells_h = fit_px_h.div_ceil(font.1 as u64).min(pane_h as u64) as u16;
    (cells_w.max(1), cells_h.max(1))
}

/// Draw the image body (outer border/title owned by the caller).
pub fn render(
    frame: &mut Frame,
    area: Rect,
    preview: &mut ImagePreview,
    side: DiffSideView,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if let Some(s) = preview.old.as_mut() {
        s.poll_completed();
    }
    if let Some(s) = preview.new.as_mut() {
        s.poll_completed();
    }

    let added = preview.old.is_none() && preview.new.is_some();
    let deleted = preview.new.is_none() && preview.old.is_some();
    // A missing version isn't a comparison: let the available image use the
    // full pane, just like added text files. Explicit side controls still win.
    let effective_side = match (side, added, deleted) {
        (DiffSideView::Both, true, _) => DiffSideView::NewOnly,
        (DiffSideView::Both, _, true) => DiffSideView::OldOnly,
        _ => side,
    };
    let (old_area, new_area) = split_preview_area(area, effective_side);
    if let Some(a) = old_area {
        render_side(
            frame,
            a,
            preview.old.as_mut(),
            true,
            if deleted { "Deleted" } else { "Before" },
            preview.font_size,
            theme,
        );
    }
    if let Some(a) = new_area {
        render_side(
            frame,
            a,
            preview.new.as_mut(),
            false,
            if added { "Added" } else { "After" },
            preview.font_size,
            theme,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_side(
    frame: &mut Frame,
    pane: Rect,
    side: Option<&mut PreviewSide>,
    is_old: bool,
    title: &str,
    font_size: (u16, u16),
    theme: &Theme,
) {
    if pane.width == 0 || pane.height == 0 {
        return;
    }

    let Some(state) = side else {
        let status = if is_old { "Added" } else { "Deleted" };
        let header = Line::from(vec![
            Span::styled(format!("{title} "), Style::default().fg(theme.text_strong)),
            Span::styled("Missing", Style::default().fg(theme.text_dimmed)),
            Span::styled(
                format!(" · {status}"),
                Style::default().fg(theme.accent_secondary),
            ),
        ]);
        if pane.height >= 1 {
            frame.render_widget(
                Paragraph::new(header),
                Rect::new(pane.x, pane.y, pane.width, 1),
            );
        }
        if pane.height >= 3 {
            let msg = Paragraph::new(Line::from(Span::styled(
                "—",
                Style::default().fg(theme.text_dimmed),
            )));
            let centered = Rect::new(
                pane.x,
                pane.y + pane.height / 2,
                pane.width,
                1.min(pane.height.saturating_sub(1)),
            );
            frame.render_widget(msg, centered);
        }
        return;
    };

    let dims = format!("{}x{}", state.orig_width, state.orig_height);
    let fmt = state.format_label();
    let header = Line::from(vec![
        Span::styled(format!("{title} "), Style::default().fg(theme.text_strong)),
        Span::styled(dims, Style::default().fg(theme.text)),
        Span::styled(format!(" · {fmt}"), Style::default().fg(theme.text_dimmed)),
    ]);
    let header_h: u16 = 1;
    if pane.height <= header_h {
        frame.render_widget(Paragraph::new(header), pane);
        return;
    }
    frame.render_widget(
        Paragraph::new(header),
        Rect::new(pane.x, pane.y, pane.width, header_h),
    );
    let img_area = Rect::new(
        pane.x,
        pane.y + header_h,
        pane.width,
        pane.height.saturating_sub(header_h),
    );
    if img_area.width == 0 || img_area.height == 0 {
        return;
    }

    let fitted: Option<Rect> = state
        .thread
        .size_for(
            Resize::Fit(None),
            Size::new(img_area.width, img_area.height),
        )
        .map(|r| centered_rect(img_area, r.width, r.height))
        .or_else(|| {
            let (cw, ch) = fit_cells(
                state.width,
                state.height,
                font_size,
                img_area.width,
                img_area.height,
            );
            Some(centered_rect(img_area, cw, ch))
        });
    let Some(dst) = fitted else {
        return;
    };
    if dst.width == 0 || dst.height == 0 {
        return;
    }

    // Pending first encode: brief Loading, then a bounded fallback message.
    // Encode failures drop the protocol (ratatui-image API), so this never
    // blocks; it just shows the message.
    let pending = state.thread.protocol_type().is_none();
    if pending {
        let waiting = state.created.elapsed();
        let label = if waiting > Duration::from_secs(2) {
            "Preview unavailable"
        } else {
            "Loading…"
        };
        let w = (label.chars().count() as u16).min(dst.width).max(1);
        let msg_area = centered_rect(dst, w, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                label,
                Style::default().fg(theme.text_dimmed),
            ))),
            msg_area,
        );
        state
            .thread
            .resize_encode(&Resize::Fit(None), Size::new(dst.width, dst.height));
        frame.render_stateful_widget(
            StatefulImage::new().resize(Resize::Fit(None)),
            dst,
            &mut state.thread,
        );
        return;
    }

    frame.render_stateful_widget(
        StatefulImage::new().resize(Resize::Fit(None)),
        dst,
        &mut state.thread,
    );
}

#[cfg(test)]
mod tests;
