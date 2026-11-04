use super::source::*;
use super::*;
use image::DynamicImage;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

fn halfblocks_picker() -> Picker {
    let mut p = capabilities::picker_for_font_size((8, 16));
    p.set_protocol_type(ProtocolType::Halfblocks);
    p
}

fn graphics_picker() -> Picker {
    let mut p = capabilities::picker_for_font_size((8, 16));
    p.set_protocol_type(ProtocolType::Kitty);
    p
}

fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = DynamicImage::ImageRgb8(image::ImageBuffer::from_pixel(
        w,
        h,
        image::Rgb([200, 30, 30]),
    ));
    let mut buf = Vec::new();
    let mut cur = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cur, ImageFormat::Png).unwrap();
    buf
}

fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = DynamicImage::ImageRgb8(image::ImageBuffer::from_pixel(
        w,
        h,
        image::Rgb([30, 200, 30]),
    ));
    let mut buf = Vec::new();
    let mut cur = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cur, ImageFormat::Jpeg).unwrap();
    buf
}

fn gif_bytes() -> Vec<u8> {
    vec![
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
    ]
}

#[test]
fn decode_png_detects_type_and_dims() {
    let b = png_bytes(4, 3);
    let d = decode_bytes(&b).expect("png decodes");
    assert_eq!((d.width(), d.height()), (4, 3));
    assert_eq!(d.format, Some(ImageFormat::Png));
    assert_eq!(d.format_label(), "PNG");
}

#[test]
fn decode_jpeg_detects_type() {
    let b = jpeg_bytes(6, 5);
    let d = decode_bytes(&b).expect("jpeg decodes");
    assert_eq!(d.format, Some(ImageFormat::Jpeg));
    assert_eq!(d.format_label(), "JPEG");
}

#[test]
fn decode_gif_detects_type() {
    let d = decode_bytes(&gif_bytes()).expect("gif decodes");
    assert_eq!(d.format, Some(ImageFormat::Gif));
    assert_eq!(d.format_label(), "GIF");
}

#[test]
fn decode_garbage_returns_none() {
    assert!(decode_bytes(b"not an image at all").is_none());
    assert!(decode_bytes(&[]).is_none());
    assert!(decode_bytes(&[0u8; 16]).is_none());
}

#[test]
fn decode_rejects_oversize_compressed() {
    let big = vec![0u8; MAX_COMPRESSED_BYTES + 1];
    assert!(decode_bytes(&big).is_none());
}

#[test]
fn downsize_preserves_aspect_within_box() {
    let b = png_bytes(3000, 2000);
    let d = decode_bytes(&b).expect("large png decodes");
    assert_eq!((d.orig_width, d.orig_height), (3000, 2000));
    assert!(d.width() <= PREVIEW_MAX_WIDTH && d.height() <= PREVIEW_MAX_HEIGHT);
    let r = d.width() as f64 / d.height() as f64;
    assert!((r - 1.5).abs() < 0.05, "aspect kept, got {r}");
}

#[test]
fn small_images_are_not_upscaled() {
    let b = png_bytes(8, 6);
    let d = decode_bytes(&b).unwrap();
    assert_eq!((d.width(), d.height()), (8, 6));
}

#[test]
fn format_labels_cover_contract() {
    assert_eq!(format_label_for(Some(ImageFormat::Png)), "PNG");
    assert_eq!(format_label_for(Some(ImageFormat::Jpeg)), "JPEG");
    assert_eq!(format_label_for(Some(ImageFormat::Gif)), "GIF");
    assert_eq!(format_label_for(Some(ImageFormat::WebP)), "WEBP");
    assert_eq!(format_label_for(Some(ImageFormat::Bmp)), "BMP");
    assert_eq!(format_label_for(Some(ImageFormat::Ico)), "ICO");
    assert_eq!(format_label_for(Some(ImageFormat::Tiff)), "TIFF");
    assert_eq!(format_label_for(None), "IMG");
}

#[test]
fn split_layout_honors_side_view() {
    let area = Rect::new(0, 0, 100, 20);
    let (o, n) = split_preview_area(area, DiffSideView::Both);
    let (o, n) = (o.unwrap(), n.unwrap());
    assert_eq!(o.width + n.width, 100);
    assert_eq!((o.x, o.y, o.height), (0, 0, 20));
    assert_eq!((n.x, n.y, n.height), (50, 0, 20));

    let (o, n) = split_preview_area(area, DiffSideView::OldOnly);
    assert!(o.is_some() && n.is_none());
    let (o, n) = split_preview_area(area, DiffSideView::NewOnly);
    assert!(o.is_none() && n.is_some());

    let empty = Rect::new(0, 0, 0, 0);
    assert_eq!(split_preview_area(empty, DiffSideView::Both), (None, None));
}

#[test]
fn centered_rect_is_centered_and_clamped() {
    let outer = Rect::new(10, 5, 20, 10);
    let c = centered_rect(outer, 8, 4);
    assert_eq!((c.x, c.y, c.width, c.height), (16, 8, 8, 4));
    let big = centered_rect(outer, 100, 100);
    assert_eq!((big.x, big.y, big.width, big.height), (10, 5, 20, 10));
}

#[test]
fn fit_cells_preserves_aspect_and_clamps() {
    let (w, h) = fit_cells(160, 120, (8, 16), 20, 10);
    assert!(w <= 20 && h <= 10 && w >= 1 && h >= 1);
    let (w2, h2) = fit_cells(8, 8, (8, 16), 40, 20);
    assert_eq!((w2, h2), (1, 1));
    assert_eq!(fit_cells(0, 0, (8, 16), 10, 10), (1, 1));
}

#[test]
fn from_decoded_needs_picker_and_a_side() {
    let proto = graphics_picker();
    let d = decode_bytes(&png_bytes(4, 4)).unwrap();
    assert!(ImagePreview::from_decoded(None, None, &proto).is_none());
    assert!(ImagePreview::from_decoded(Some(d.clone()), None, &proto).is_some());
    assert!(ImagePreview::from_decoded(None, Some(d), &proto).is_some());
    let half = halfblocks_picker();
    let d2 = decode_bytes(&png_bytes(4, 4)).unwrap();
    assert!(ImagePreview::from_decoded(Some(d2), None, &half).is_none());
}

#[test]
fn estimated_bytes_counts_decoded_plus_encoded() {
    let proto = graphics_picker();
    let d = decode_bytes(&png_bytes(8, 8)).unwrap();
    let p = ImagePreview::from_decoded(Some(d), None, &proto).unwrap();
    let dec = 8usize * 8 * 4;
    let est = p.estimated_bytes();
    assert!(est >= dec + dec * 4 / 3, "est {est} covers decoded+encoded");
    assert!(est < dec * 10 + 8192, "est {est} stays plausible");
    let d2 = decode_bytes(&png_bytes(8, 8)).unwrap();
    let d3 = decode_bytes(&jpeg_bytes(8, 8)).unwrap();
    let p2 = ImagePreview::from_decoded(Some(d2), Some(d3), &proto).unwrap();
    assert!(p2.estimated_bytes() > est);
}

#[test]
fn kitty_previews_use_placeholders() {
    let picker = graphics_picker();
    let d = decode_bytes(&png_bytes(8, 8)).unwrap();
    let p = ImagePreview::from_decoded(Some(d), None, &picker).unwrap();
    assert!(p.uses_placeholders());
}

#[test]
fn image_source_variants_exist() {
    let a = ImageSource::Worktree("a.png");
    let b = ImageSource::Revision {
        revision: "HEAD",
        path: "a.png",
    };
    let c = ImageSource::Missing;
    assert_ne!(a, b);
    assert_ne!(b, c);
}

#[test]
fn render_halfblocks_body_does_not_panic() {
    let picker = halfblocks_picker();
    let img = decode_bytes(&png_bytes(16, 16)).unwrap().image;
    let mut proto = picker.new_resize_protocol(img);
    let backend = TestBackend::new(40, 12);
    let mut term = Terminal::new(backend).expect("test terminal");
    term.draw(|f| {
        let area = f.area();
        f.render_stateful_widget(
            StatefulImage::new().resize(Resize::Fit(None)),
            area,
            &mut proto,
        );
    })
    .unwrap();
}

#[test]
fn render_preview_body_splits_and_labels() {
    let picker = graphics_picker();
    let old = decode_bytes(&png_bytes(16, 16)).unwrap();
    let new = decode_bytes(&jpeg_bytes(16, 16)).unwrap();
    let mut preview = ImagePreview::from_decoded(Some(old), Some(new), &picker).unwrap();
    let theme = Theme::default();
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).expect("test terminal");
    term.draw(|f| {
        render(f, f.area(), &mut preview, DiffSideView::Both, &theme);
    })
    .unwrap();
    let buf = term.backend().buffer().clone();
    let text: String = buf
        .content()
        .iter()
        .map(|c| c.symbol().chars().next().unwrap_or(' '))
        .collect();
    assert!(text.contains("Before"), "has Before label");
    assert!(text.contains("After"), "has After label");
    assert!(text.contains("PNG") || text.contains("JPEG"));

    term.draw(|f| {
        render(f, f.area(), &mut preview, DiffSideView::OldOnly, &theme);
    })
    .unwrap();
    let buf = term.backend().buffer().clone();
    let text: String = buf
        .content()
        .iter()
        .map(|c| c.symbol().chars().next().unwrap_or(' '))
        .collect();
    assert!(text.contains("Before"));
    assert!(!text.contains("After"));
}

#[test]
fn added_and_deleted_images_use_a_single_full_width_pane() {
    for added in [true, false] {
        let picker = graphics_picker();
        let image = decode_bytes(&png_bytes(8, 8)).unwrap();
        let (old, new) = if added {
            (None, Some(image))
        } else {
            (Some(image), None)
        };
        let mut preview = ImagePreview::from_decoded(old, new, &picker).unwrap();
        let theme = Theme::default();
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| render(f, f.area(), &mut preview, DiffSideView::Both, &theme))
            .unwrap();
        let text = buffer_text(term.backend().buffer());
        assert!(text.starts_with(if added { "Added " } else { "Deleted " }));
        assert!(!text.contains("Missing"));
        assert!(!text.contains("Before"));
        assert!(!text.contains("After"));

        // An explicit request for the absent side is still respected.
        let absent = if added {
            DiffSideView::OldOnly
        } else {
            DiffSideView::NewOnly
        };
        term.draw(|f| render(f, f.area(), &mut preview, absent, &theme))
            .unwrap();
        assert!(buffer_text(term.backend().buffer()).contains("Missing"));
    }
}

#[test]
fn tiny_area_never_panics() {
    let picker = graphics_picker();
    let d = decode_bytes(&png_bytes(8, 8)).unwrap();
    let mut preview = ImagePreview::from_decoded(Some(d.clone()), Some(d), &picker).unwrap();
    let theme = Theme::default();
    for (w, h) in [(0, 0), (1, 1), (2, 1), (1, 2), (3, 3)] {
        let backend = TestBackend::new(w, h);
        let mut term = Terminal::new(backend).expect("test terminal");
        term.draw(|f| {
            render(f, f.area(), &mut preview, DiffSideView::Both, &theme);
        })
        .unwrap();
    }
}

#[test]
fn preview_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<ImagePreview>();
}

// ------------------------------------------------------------------
// Regression helpers: isolated temp git fixtures, no global mutation.
// ------------------------------------------------------------------

fn iterm2_picker() -> Picker {
    let mut p = capabilities::picker_for_font_size((8, 16));
    p.set_protocol_type(ProtocolType::Iterm2);
    p
}

fn colored_png(rgb: [u8; 3], w: u32, h: u32) -> Vec<u8> {
    let img = DynamicImage::ImageRgb8(image::ImageBuffer::from_pixel(w, h, image::Rgb(rgb)));
    let mut buf = Vec::new();
    let mut cur = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cur, ImageFormat::Png).unwrap();
    buf
}

fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
    buf.content()
        .iter()
        .map(|c| c.symbol().chars().next().unwrap_or(' '))
        .collect()
}

fn git_cmd(repo: &Path, args: &[&str]) {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(repo).args(args);
    // Isolated, non-interactive reads/writes; per-command env only,
    // never mutates the test process environment.
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("SSH_ASKPASS_REQUIRE", "never");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
    }
    cmd.env_remove("GIT_DIR");
    cmd.env_remove("GIT_WORK_TREE");
    cmd.env_remove("GIT_INDEX_FILE");
    cmd.env_remove("GIT_PREFIX");
    let status = cmd.status().expect("run git");
    assert!(
        status.success(),
        "git {args:?} failed in {}",
        repo.display()
    );
}

fn git_output(repo: &Path, args: &[&str]) -> String {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(repo).args(args);
    cmd.env("GIT_OPTIONAL_LOCKS", "0");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("SSH_ASKPASS_REQUIRE", "never");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
    }
    cmd.env_remove("GIT_DIR");
    cmd.env_remove("GIT_WORK_TREE");
    cmd.env_remove("GIT_INDEX_FILE");
    cmd.env_remove("GIT_PREFIX");
    let out = cmd.output().expect("git output");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    fn new(prefix: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "lazygitrs-img-{prefix}-{}-{nanos}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        git_cmd(&path, &["init", "-q"]);
        Self { path }
    }

    fn git(&self, args: &[&str]) {
        git_cmd(&self.path, args);
    }

    fn commit(&self, msg: &str) {
        self.git(&[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            msg,
        ]);
    }

    fn write(&self, rel: &str, bytes: &[u8]) {
        let full = self.path.join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("mkdir parent");
        }
        std::fs::write(&full, bytes).expect("write fixture");
    }

    fn git_commands(&self) -> crate::git::GitCommands {
        crate::git::GitCommands::new(&self.path).expect("GitCommands")
    }

    fn repo_path(&self) -> PathBuf {
        self.git_commands().repo_path().to_path_buf()
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn head_index_worktree_preserve_raw_bytes() {
    let repo = TempRepo::new("raw-bytes");
    let red = colored_png([220, 30, 30], 16, 16);
    let green = colored_png([30, 220, 30], 16, 16);
    let blue = colored_png([30, 30, 220], 16, 16);
    assert_ne!(red, green);
    assert_ne!(green, blue);

    // HEAD = red.
    repo.write("img.png", &red);
    repo.git(&["add", "img.png"]);
    repo.commit("head red");

    // Index = green (staged), worktree = blue (unstaged).
    repo.write("img.png", &green);
    repo.git(&["add", "img.png"]);
    repo.write("img.png", &blue);

    let repo_path = repo.repo_path();
    // Raw bytes preserved per location via private helpers.
    match read_revision_blob(&repo_path, "HEAD", "img.png") {
        RevisionRead::Present(b) => assert_eq!(b, red, "HEAD bytes preserved"),
        other => panic!(
            "HEAD should be present, got {}",
            matches!(other, RevisionRead::Absent) as u8
        ),
    }
    match read_revision_blob(&repo_path, "", "img.png") {
        RevisionRead::Present(b) => assert_eq!(b, green, "index (:path) bytes preserved"),
        _ => panic!("empty revision should read index"),
    }
    match read_worktree_bytes(&repo_path, "img.png") {
        Ok(Some(b)) => assert_eq!(b, blue, "worktree bytes preserved"),
        other => panic!("worktree should be present, got {other:?}"),
    }

    // Decoded sides keep distinct colors (no downscale at 16x16).
    let old = load_one_side(
        &repo_path,
        ImageSource::Revision {
            revision: "HEAD",
            path: "img.png",
        },
    )
    .expect("HEAD loads")
    .expect("HEAD decodes");
    let staged = load_one_side(
        &repo_path,
        ImageSource::Revision {
            revision: "",
            path: "img.png",
        },
    )
    .expect("index loads")
    .expect("index decodes");
    let work = load_one_side(&repo_path, ImageSource::Worktree("img.png"))
        .expect("worktree loads")
        .expect("worktree decodes");
    assert_eq!((old.width(), old.height()), (16, 16));
    assert_eq!(
        old.image.to_rgb8().get_pixel(0, 0).0,
        [220, 30, 30],
        "HEAD is red"
    );
    assert_eq!(
        staged.image.to_rgb8().get_pixel(0, 0).0,
        [30, 220, 30],
        "index is green"
    );
    assert_eq!(
        work.image.to_rgb8().get_pixel(0, 0).0,
        [30, 30, 220],
        "worktree is blue"
    );

    // Full preview loads both sides with an injected picker.
    let git = repo.git_commands();
    let picker = graphics_picker();
    let preview = ImagePreview::load_with_picker(
        &git,
        ImageSource::Revision {
            revision: "HEAD",
            path: "img.png",
        },
        ImageSource::Worktree("img.png"),
        &picker,
    )
    .expect("preview loads HEAD vs worktree");
    assert!(preview.old.is_some() && preview.new.is_some());
    assert!(preview.estimated_bytes() > 0);
}

#[test]
fn empty_revision_means_index_not_head() {
    let repo = TempRepo::new("empty-rev");
    let red = colored_png([200, 30, 30], 12, 12);
    let green = colored_png([30, 200, 30], 12, 12);
    repo.write("a.png", &red);
    repo.git(&["add", "a.png"]);
    repo.commit("init");

    // No staged change: index == HEAD.
    let repo_path = repo.repo_path();
    let head = match read_revision_blob(&repo_path, "HEAD", "a.png") {
        RevisionRead::Present(b) => b,
        _ => panic!("HEAD present"),
    };
    let index = match read_revision_blob(&repo_path, "", "a.png") {
        RevisionRead::Present(b) => b,
        _ => panic!("index present"),
    };
    assert_eq!(head, index, "clean index matches HEAD");
    assert_eq!(head, red);

    // Stage a different image: empty revision follows the index.
    repo.write("a.png", &green);
    repo.git(&["add", "a.png"]);
    let head2 = match read_revision_blob(&repo_path, "HEAD", "a.png") {
        RevisionRead::Present(b) => b,
        _ => panic!("HEAD present"),
    };
    let index2 = match read_revision_blob(&repo_path, "", "a.png") {
        RevisionRead::Present(b) => b,
        _ => panic!("index present"),
    };
    assert_eq!(head2, red, "HEAD unchanged");
    assert_eq!(index2, green, "empty revision reads staged bytes");

    let via_helper = load_one_side(
        &repo_path,
        ImageSource::Revision {
            revision: "",
            path: "a.png",
        },
    )
    .expect("index loads")
    .expect("index decodes");
    assert_eq!(via_helper.image.to_rgb8().get_pixel(0, 0).0, [30, 200, 30]);

    // Injected Iterm2 picker behaves like Kitty here.
    let git = repo.git_commands();
    for picker in [graphics_picker(), iterm2_picker()] {
        let preview = ImagePreview::load_with_picker(
            &git,
            ImageSource::Revision {
                revision: "",
                path: "a.png",
            },
            ImageSource::Revision {
                revision: "HEAD",
                path: "a.png",
            },
            &picker,
        )
        .expect("index vs HEAD preview loads");
        assert!(preview.old.is_some() && preview.new.is_some());
        let _ = picker.protocol_type();
    }
}

#[test]
fn root_commit_missing_parent_is_absent() {
    let repo = TempRepo::new("root-parent");
    let red = colored_png([200, 30, 30], 10, 10);
    repo.write("img.png", &red);
    repo.git(&["add", "img.png"]);
    repo.commit("root");

    let repo_path = repo.repo_path();
    let hash = git_output(&repo.path, &["rev-parse", "HEAD"]);
    assert!(!hash.is_empty());
    let parent_spec = format!("{hash}^1");

    // Missing parent blob is legitimately absent, not a failure.
    match read_revision_blob(&repo_path, &parent_spec, "img.png") {
        RevisionRead::Absent => {}
        RevisionRead::Present(_) => panic!("parent should be absent"),
        RevisionRead::Failed => panic!("parent should be absent, not failed"),
    }
    assert!(matches!(
        load_one_side(
            &repo_path,
            ImageSource::Revision {
                revision: &parent_spec,
                path: "img.png",
            }
        ),
        Some(None)
    ));
    // HEAD^1 shorthand behaves the same on a root commit.
    assert!(matches!(
        load_one_side(
            &repo_path,
            ImageSource::Revision {
                revision: "HEAD^1",
                path: "img.png",
            }
        ),
        Some(None)
    ));

    // Preview treats absent old as added (new present).
    let git = repo.git_commands();
    let picker = graphics_picker();
    let preview = ImagePreview::load_with_picker(
        &git,
        ImageSource::Revision {
            revision: &parent_spec,
            path: "img.png",
        },
        ImageSource::Revision {
            revision: &hash,
            path: "img.png",
        },
        &picker,
    )
    .expect("root preview loads");
    assert!(preview.old.is_none(), "old absent on root");
    assert!(preview.new.is_some(), "new present on root");
}

#[test]
fn added_and_deleted_sides_via_missing() {
    // Pure helper contract without git.
    let picker = graphics_picker();
    let decoded = decode_bytes(&colored_png([10, 200, 10], 8, 8)).unwrap();
    assert!(ImagePreview::from_decoded(None, Some(decoded.clone()), &picker).is_some());
    assert!(ImagePreview::from_decoded(Some(decoded), None, &picker).is_some());
    assert!(ImagePreview::from_decoded(None, None, &picker).is_none());

    // Missing via private helper is Some(None), never a failure.
    let repo = TempRepo::new("added-deleted");
    let bytes = colored_png([200, 30, 30], 8, 8);
    repo.write("img.png", &bytes);
    repo.git(&["add", "img.png"]);
    repo.commit("init");
    let repo_path = repo.repo_path();
    assert!(matches!(
        load_one_side(&repo_path, ImageSource::Missing),
        Some(None)
    ));
    // Worktree file removed => legitimately missing.
    std::fs::remove_file(repo.path.join("img.png")).expect("remove worktree file");
    assert!(matches!(
        read_worktree_bytes(&repo_path, "img.png"),
        Ok(None)
    ));
    assert!(matches!(
        load_one_side(&repo_path, ImageSource::Worktree("img.png")),
        Some(None)
    ));

    // Added (old missing) and deleted (new missing) previews load.
    let git = repo.git_commands();
    let added = ImagePreview::load_with_picker(
        &git,
        ImageSource::Missing,
        ImageSource::Revision {
            revision: "HEAD",
            path: "img.png",
        },
        &picker,
    )
    .expect("added loads");
    assert!(added.old.is_none() && added.new.is_some());
    // Worktree file is gone, so deleted shows old only.
    let deleted = ImagePreview::load_with_picker(
        &git,
        ImageSource::Revision {
            revision: "HEAD",
            path: "img.png",
        },
        ImageSource::Missing,
        &picker,
    )
    .expect("deleted loads");
    assert!(deleted.old.is_some() && deleted.new.is_none());
    // Both missing => no preview at all.
    assert!(
        ImagePreview::load_with_picker(&git, ImageSource::Missing, ImageSource::Missing, &picker)
            .is_none()
    );
}

#[test]
fn rename_uses_before_path_after_path() {
    let repo = TempRepo::new("rename-paths");
    let bytes = colored_png([30, 30, 220], 14, 14);
    repo.write("before.png", &bytes);
    repo.git(&["add", "before.png"]);
    repo.commit("add before");
    repo.git(&["mv", "before.png", "after.png"]);
    repo.commit("rename");

    let repo_path = repo.repo_path();
    // Old path only exists at the parent, new path only at HEAD.
    let old_bytes = match read_revision_blob(&repo_path, "HEAD^", "before.png") {
        RevisionRead::Present(b) => b,
        _ => panic!("before.png at HEAD^"),
    };
    let new_bytes = match read_revision_blob(&repo_path, "HEAD", "after.png") {
        RevisionRead::Present(b) => b,
        _ => panic!("after.png at HEAD"),
    };
    assert_eq!(old_bytes, bytes);
    assert_eq!(new_bytes, bytes);
    assert!(matches!(
        load_one_side(
            &repo_path,
            ImageSource::Revision {
                revision: "HEAD",
                path: "before.png",
            }
        ),
        Some(None)
    ));
    assert!(matches!(
        load_one_side(
            &repo_path,
            ImageSource::Revision {
                revision: "HEAD^",
                path: "after.png",
            }
        ),
        Some(None)
    ));

    let git = repo.git_commands();
    let picker = graphics_picker();
    let preview = ImagePreview::load_with_picker(
        &git,
        ImageSource::Revision {
            revision: "HEAD^",
            path: "before.png",
        },
        ImageSource::Revision {
            revision: "HEAD",
            path: "after.png",
        },
        &picker,
    )
    .expect("rename preview loads before/after paths");
    assert!(preview.old.is_some() && preview.new.is_some());
}

#[test]
fn corrupt_side_makes_entire_preview_none() {
    let repo = TempRepo::new("corrupt-side");
    let valid = colored_png([200, 30, 30], 12, 12);
    repo.write("img.png", &valid);
    repo.git(&["add", "img.png"]);
    repo.commit("valid");

    // Corrupt worktree: entire load fails, not a partial preview.
    repo.write("img.png", b"definitely not an image");
    let repo_path = repo.repo_path();
    assert!(decode_bytes(b"definitely not an image").is_none());
    assert!(load_one_side(&repo_path, ImageSource::Worktree("img.png")).is_none());

    let git = repo.git_commands();
    let picker = graphics_picker();
    assert!(
        ImagePreview::load_with_picker(
            &git,
            ImageSource::Revision {
                revision: "HEAD",
                path: "img.png",
            },
            ImageSource::Worktree("img.png"),
            &picker,
        )
        .is_none(),
        "corrupt new side must fail the whole preview"
    );
    assert!(
        ImagePreview::load_with_picker(
            &git,
            ImageSource::Worktree("img.png"),
            ImageSource::Revision {
                revision: "HEAD",
                path: "img.png",
            },
            &picker,
        )
        .is_none(),
        "corrupt old side must fail the whole preview"
    );

    // Corrupt revision side behaves the same.
    repo.write("img.png", b"still not an image");
    repo.git(&["add", "img.png"]);
    repo.commit("corrupt revision");
    assert!(
        ImagePreview::load_with_picker(
            &git,
            ImageSource::Revision {
                revision: "HEAD^",
                path: "img.png",
            },
            ImageSource::Revision {
                revision: "HEAD",
                path: "img.png",
            },
            &picker,
        )
        .is_none(),
        "corrupt revision must fail the whole preview"
    );
    let _ = picker.protocol_type();
}

#[test]
fn decode_rejects_over_dimension_limit() {
    // Width far over MAX_IMAGE_DIMENSION with tiny height keeps the
    // allocation small while still exercising the dimension guard.
    let wide = colored_png([10, 10, 10], MAX_IMAGE_DIMENSION + 1000, 16);
    assert!(
        decode_bytes(&wide).is_none(),
        "width over limit must not decode"
    );
    let tall = colored_png([10, 10, 10], 16, MAX_IMAGE_DIMENSION + 1000);
    assert!(
        decode_bytes(&tall).is_none(),
        "height over limit must not decode"
    );

    // Same failure surfaces through the worktree helper without a repo.
    let repo = TempRepo::new("over-dim");
    repo.write("wide.png", &wide);
    let repo_path = repo.repo_path();
    assert!(load_one_side(&repo_path, ImageSource::Worktree("wide.png")).is_none());
    let git = repo.git_commands();
    let picker = graphics_picker();
    assert!(
        ImagePreview::load_with_picker(
            &git,
            ImageSource::Worktree("wide.png"),
            ImageSource::Missing,
            &picker,
        )
        .is_none()
    );
}

#[test]
fn decode_rejects_over_alloc_limit() {
    // 5000x4500 RGB is ~67MiB decoded (>64MiB cap) while staying inside
    // the 16384 dimension cap. Uniform color keeps the PNG itself small.
    let big = colored_png([40, 40, 200], 5000, 4500);
    assert!(
        decode_bytes(&big).is_none(),
        "decoded alloc over limit must not decode"
    );
}

#[test]
fn worktree_oversize_is_bounded_without_writing_20mib() {
    let repo = TempRepo::new("oversize-worktree");
    let small = colored_png([200, 30, 30], 8, 8);
    repo.write("big.png", &small);
    // Sparse extension: no 20MiB write, just a size bump past the cap.
    let full = repo.path.join("big.png");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&full)
        .expect("open");
    file.set_len((MAX_COMPRESSED_BYTES as u64) + 1)
        .expect("sparse oversize");
    drop(file);
    assert_eq!(
        std::fs::metadata(&full).expect("meta").len(),
        (MAX_COMPRESSED_BYTES as u64) + 1
    );

    let repo_path = repo.repo_path();
    assert!(matches!(
        read_worktree_bytes(&repo_path, "big.png"),
        Err(())
    ));
    assert!(load_one_side(&repo_path, ImageSource::Worktree("big.png")).is_none());
    let git = repo.git_commands();
    let picker = graphics_picker();
    assert!(
        ImagePreview::load_with_picker(
            &git,
            ImageSource::Worktree("big.png"),
            ImageSource::Missing,
            &picker,
        )
        .is_none()
    );
}

#[test]
fn headers_show_original_dimensions_not_downscaled() {
    // 3000x2000 downsizes to ~1600x1066 for the protocol, but headers
    // and cache accounting must preserve the original size.
    let bytes = colored_png([200, 30, 30], 3000, 2000);
    let decoded = decode_bytes(&bytes).expect("large png decodes");
    assert_eq!((decoded.orig_width, decoded.orig_height), (3000, 2000));
    assert!(
        decoded.width() <= PREVIEW_MAX_WIDTH && decoded.height() <= PREVIEW_MAX_HEIGHT,
        "protocol image is downsized"
    );
    assert!(
        decoded.width() != decoded.orig_width,
        "fixture must actually downscale"
    );

    let picker = graphics_picker();
    let preview = ImagePreview::from_decoded(Some(decoded), None, &picker).unwrap();
    let side = preview.old.as_ref().unwrap();
    assert_eq!((side.orig_width, side.orig_height), (3000, 2000));
    // Only the thumbnail is retained; original dimensions are metadata.
    let dec_orig = side.width as usize * side.height as usize * 4;
    assert!(
        preview.estimated_bytes() >= dec_orig + dec_orig * 4 / 3,
        "estimate covers original, got {}",
        preview.estimated_bytes()
    );

    let mut live = preview;
    let theme = Theme::default();
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).expect("test terminal");
    term.draw(|f| {
        render(f, f.area(), &mut live, DiffSideView::Both, &theme);
    })
    .unwrap();
    let text = buffer_text(term.backend().buffer());
    assert!(
        text.contains("3000x2000"),
        "header shows original dims, got {text:?}"
    );
    assert!(
        !text.contains(&format!(
            "{}x{}",
            live.old.as_ref().unwrap().width,
            live.old.as_ref().unwrap().height
        )) || text.contains("3000x2000"),
        "must not show only downsized dims"
    );
}

#[test]
fn async_render_eventually_encodes_with_test_backend() {
    let picker = graphics_picker();
    let decoded = decode_bytes(&colored_png([30, 200, 30], 32, 32)).unwrap();
    let mut preview = ImagePreview::from_decoded(Some(decoded), None, &picker).unwrap();
    let theme = Theme::default();
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).expect("test terminal");

    // First frame kicks the background resize+encode.
    term.draw(|f| {
        render(f, f.area(), &mut preview, DiffSideView::Both, &theme);
    })
    .unwrap();
    let first = buffer_text(term.backend().buffer());
    assert!(first.contains("Deleted"), "header visible while loading");

    // Poll via repeated renders until the worker returns the protocol.
    let mut encoded = false;
    for _ in 0..250 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        term.draw(|f| {
            render(f, f.area(), &mut preview, DiffSideView::Both, &theme);
        })
        .unwrap();
        if preview
            .old
            .as_ref()
            .expect("old side")
            .thread
            .protocol_type()
            .is_some()
        {
            encoded = true;
            break;
        }
    }
    assert!(encoded, "background encode should complete");
    let text = buffer_text(term.backend().buffer());
    assert!(text.contains("Deleted"));
    assert!(text.contains("32x32"));
}

#[test]
fn iterm2_picker_loads_images_like_kitty() {
    let repo = TempRepo::new("iterm2-picker");
    let bytes = colored_png([30, 30, 220], 20, 20);
    repo.write("img.png", &bytes);
    repo.git(&["add", "img.png"]);
    repo.commit("init");
    let git = repo.git_commands();

    for picker in [graphics_picker(), iterm2_picker()] {
        assert_ne!(picker.protocol_type(), ProtocolType::Halfblocks);
        let preview = ImagePreview::load_with_picker(
            &git,
            ImageSource::Revision {
                revision: "HEAD",
                path: "img.png",
            },
            ImageSource::Revision {
                revision: "HEAD",
                path: "img.png",
            },
            &picker,
        )
        .expect("iterm2/kitty loads");
        // No global picker touched; injected picker drives both sides.
        assert!(preview.old.is_some() && preview.new.is_some());
        assert_eq!(
            preview.old.as_ref().unwrap().orig_width,
            preview.new.as_ref().unwrap().orig_width
        );
    }
}

#[test]
fn diff_view_state_clears_preview_to_text_fallback() {
    use crate::pager::side_by_side::DiffViewState;

    let picker = graphics_picker();
    let decoded = decode_bytes(&colored_png([200, 30, 30], 8, 8)).unwrap();
    let preview = ImagePreview::from_decoded(Some(decoded), None, &picker).unwrap();

    // Setting a preview then loading text clears back to the fallback.
    let mut state = DiffViewState::new();
    state.image_preview = Some(preview);
    assert!(state.image_preview.is_some());
    state.load("file.txt", "old\n", "new\n");
    assert!(
        state.image_preview.is_none(),
        "load() must clear the image preview"
    );
    assert!(!state.lines.is_empty(), "text fallback has lines");

    let decoded2 = decode_bytes(&colored_png([30, 200, 30], 8, 8)).unwrap();
    state.image_preview = Some(ImagePreview::from_decoded(Some(decoded2), None, &picker).unwrap());
    state.load_from_diff_output(
            "file.txt",
            "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new\n",
        );
    assert!(
        state.image_preview.is_none(),
        "load_from_diff_output must clear the preview"
    );
    assert!(
        state.lines.iter().any(
            |l| l.old_line.as_ref().is_some_and(|(_, t)| t.contains("old"))
                || l.new_line.as_ref().is_some_and(|(_, t)| t.contains("new"))
        ),
        "fallback text diff is viewable"
    );

    // Parsed text without a preview stays a plain text diff.
    let parsed = DiffViewState::parse_diff_output(
        "file.txt",
        "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new\n",
        4,
        true,
    );
    assert!(parsed.image_preview.is_none());
    let mut applied = DiffViewState::new();
    applied.apply_parsed(parsed);
    assert!(applied.image_preview.is_none());
    assert!(!applied.lines.is_empty());
}

#[test]
fn load_one_side_edge_cases_are_safe() {
    let repo = TempRepo::new("edge-cases");
    let bytes = colored_png([200, 30, 30], 8, 8);
    repo.write("img.png", &bytes);
    repo.git(&["add", "img.png"]);
    repo.commit("init");
    std::fs::create_dir_all(repo.path.join("subdir")).expect("mkdir");
    let repo_path = repo.repo_path();

    // Empty and NUL paths never touch git/disk beyond a failure.
    assert!(load_one_side(&repo_path, ImageSource::Worktree("")).is_none());
    assert!(load_one_side(&repo_path, ImageSource::Worktree("a\0b")).is_none());
    assert!(
        load_one_side(
            &repo_path,
            ImageSource::Revision {
                revision: "HEAD",
                path: "",
            }
        )
        .is_none()
    );
    assert!(
        load_one_side(
            &repo_path,
            ImageSource::Revision {
                revision: "HEAD",
                path: "a\0b",
            }
        )
        .is_none()
    );
    assert!(
        load_one_side(
            &repo_path,
            ImageSource::Revision {
                revision: "HEAD\0",
                path: "img.png",
            }
        )
        .is_none()
    );

    // Missing worktree file and missing blob are absent, not failed.
    assert!(matches!(
        read_worktree_bytes(&repo_path, "missing.png"),
        Ok(None)
    ));
    assert!(matches!(
        load_one_side(&repo_path, ImageSource::Worktree("missing.png")),
        Some(None)
    ));
    assert!(matches!(
        read_revision_blob(&repo_path, "HEAD", "missing.png"),
        RevisionRead::Absent
    ));
    assert!(matches!(
        load_one_side(
            &repo_path,
            ImageSource::Revision {
                revision: "HEAD",
                path: "missing.png",
            }
        ),
        Some(None)
    ));

    // Directories read as absent.
    assert!(matches!(
        read_worktree_bytes(&repo_path, "subdir"),
        Ok(None)
    ));
    assert!(matches!(
        load_one_side(&repo_path, ImageSource::Worktree("subdir")),
        Some(None)
    ));

    // Direct helper coverage for bounded reads.
    assert!(matches!(
        read_worktree_bytes(&repo_path, "img.png"),
        Ok(Some(_))
    ));
    assert!(matches!(
        read_revision_blob(&repo_path, "HEAD", "img.png"),
        RevisionRead::Present(_)
    ));
}
#[test]
fn image_diff_renders_labels_and_hides_for_overlay() {
    use crate::pager::side_by_side::{DiffViewState, render_diff};
    let picker = graphics_picker();
    let mut parsed = DiffViewState::parse_diff_output(
        "image.png",
        "Binary files a/image.png and b/image.png differ\n",
        4,
        true,
    );
    parsed.image_preview = ImagePreview::from_decoded(
        Some(decode_bytes(&png_bytes(32, 32)).unwrap()),
        Some(decode_bytes(&png_bytes(64, 64)).unwrap()),
        &picker,
    );
    assert!(parsed.image_preview.as_ref().unwrap().uses_placeholders());
    let mut view = DiffViewState::new();
    view.apply_parsed(parsed);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let theme = Theme::default();
    terminal
        .draw(|f| render_diff(f, f.area(), &mut view, &theme, true, false, true, 0))
        .unwrap();
    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("Before") && text.contains("After"));
    assert!(!text.contains("Binary cannot be previewed"));
    view.image_preview_hidden = true;
    terminal
        .draw(|f| render_diff(f, f.area(), &mut view, &theme, true, false, true, 0))
        .unwrap();
    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("Binary cannot be previewed"));
    assert!(!text.contains("Before"));
}

// ------------------------------------------------------------------
// Inline mixed-diff helpers (isolated, no global env/detection).
// ------------------------------------------------------------------

fn mixed_three_file_diff() -> String {
    let long_a_old = "a".repeat(200);
    let long_a_new = "b".repeat(200);
    let long_c_old = "c".repeat(200);
    let long_c_new = "d".repeat(200);
    format!(
        "diff --git a/first.txt b/first.txt\n--- a/first.txt\n+++ b/first.txt\n@@ -1,3 +1,3 @@\n-{long_a_old}\n+{long_a_new}\n context line one\n@@ -10,3 +10,3 @@\n context line two\n-{long_c_old}\n+{long_c_new}\n context line three\ndiff --git a/img.png b/img.png\nBinary files a/img.png and b/img.png differ\ndiff --git a/last.txt b/last.txt\n--- a/last.txt\n+++ b/last.txt\n@@ -1,2 +1,2 @@\n-tail old one\n+tail new one\n tail context\n"
    )
}

fn inline_ready_both_sides() -> InlineImagePreview {
    let picker = graphics_picker();
    let old = decode_bytes(&png_bytes(32, 32)).unwrap();
    let new = decode_bytes(&png_bytes(32, 32)).unwrap();
    let preview = ImagePreview::from_decoded(Some(old), Some(new), &picker).unwrap();
    InlineImagePreview::from_preview_for_test(preview)
}

fn inline_ready_single_sided(added: bool) -> InlineImagePreview {
    let picker = graphics_picker();
    let img = decode_bytes(&png_bytes(16, 16)).unwrap();
    let (old, new) = if added {
        (None, Some(img))
    } else {
        (Some(img), None)
    };
    let preview = ImagePreview::from_decoded(old, new, &picker).unwrap();
    InlineImagePreview::from_preview_for_test(preview)
}

fn render_state_buffer(
    state: &mut crate::pager::side_by_side::DiffViewState,
    w: u16,
    h: u16,
) -> ratatui::buffer::Buffer {
    use crate::pager::side_by_side::render_diff;
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| {
        render_diff(
            f,
            f.area(),
            state,
            &crate::config::Theme::default(),
            true,
            false,
            true,
            0,
        )
    })
    .unwrap();
    term.backend().buffer().clone()
}

fn test_row_string(buf: &ratatui::buffer::Buffer, y: u16) -> String {
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect()
}

fn test_header_idx(state: &crate::pager::side_by_side::DiffViewState, name: &str) -> usize {
    state
        .lines
        .iter()
        .position(|l| l.file_header.as_ref().is_some_and(|h| h.contains(name)))
        .expect("header present")
}

fn middle_image_section(state: &crate::pager::side_by_side::ParsedDiff) -> usize {
    state
        .lines
        .iter()
        .find_map(|l| {
            if l.preview_placeholder.is_some() {
                Some(l.section_index)
            } else {
                None
            }
        })
        .expect("binary section present")
}

#[test]
fn inline_with_picker_and_ready_fixture_uses_injected_picker() {
    let picker = graphics_picker();
    let idle = InlineImagePreview::with_picker(
        std::path::Path::new("."),
        InlineImageSource::Missing,
        InlineImageSource::Missing,
        picker,
    );
    assert!(matches!(idle.state, InlineLoadState::Idle));
    let bytes = png_bytes(4, 4);
    assert!(!bytes.is_empty());
    let decoded = decode_bytes(&bytes).expect("png decodes");
    assert_eq!((decoded.width(), decoded.height()), (4, 4));
    let picker2 = graphics_picker();
    let d = decode_bytes(&png_bytes(8, 8)).unwrap();
    let preview = ImagePreview::from_decoded(Some(d), None, &picker2).unwrap();
    let ready = InlineImagePreview::from_preview_for_test(preview);
    assert!(matches!(ready.state, InlineLoadState::Ready(_)));
    assert!(ready.estimated_bytes() > idle.estimated_bytes());
    use crate::pager::side_by_side::DiffViewState;
    let diff = mixed_three_file_diff();
    let mut parsed = DiffViewState::parse_diff_output("mixed", &diff, 4, true);
    let middle = middle_image_section(&parsed);
    let mut map = std::collections::HashMap::new();
    map.insert(middle, inline_ready_both_sides());
    let before = parsed.lines.len();
    parsed.attach_inline_images(map);
    assert_eq!(parsed.inline_images.len(), 1);
    assert!(parsed.lines.len() > before);
    assert_eq!(
        parsed
            .lines
            .iter()
            .filter(|l| l.preview_placeholder.is_some() && l.section_index == middle)
            .count(),
        INLINE_IMAGE_ROWS
    );
}

#[test]
fn inline_lazy_idle_has_no_pixels() {
    let picker = graphics_picker();
    let inline = InlineImagePreview::with_picker(
        std::path::Path::new("."),
        InlineImageSource::Missing,
        InlineImageSource::Missing,
        picker,
    );
    assert!(matches!(inline.state, InlineLoadState::Idle));
    let est = inline.estimated_bytes();
    assert!(est < 4096, "idle estimate small, got {est}");
    let ready = inline_ready_both_sides();
    assert!(ready.estimated_bytes() > est + 1000);
}

#[test]
fn inline_initial_render_triggers_background_load() {
    use crate::pager::side_by_side::DiffSideView;
    let repo = TempRepo::new("inline-initial-load");
    let bytes = png_bytes(16, 16);
    repo.write("img.png", &bytes);
    let repo_path = repo.repo_path();
    let picker = graphics_picker();
    let mut inline = InlineImagePreview::with_picker(
        &repo_path,
        InlineImageSource::Missing,
        InlineImageSource::Worktree("img.png".to_string()),
        picker,
    );
    assert!(matches!(inline.state, InlineLoadState::Idle));
    let theme = crate::config::Theme::default();
    let mut term = Terminal::new(TestBackend::new(60, 14)).unwrap();
    let start = std::time::Instant::now();
    term.draw(|f| {
        inline.render(f, f.area(), DiffSideView::Both, &theme);
    })
    .unwrap();
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "initial render must not block"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut settled = false;
    while std::time::Instant::now() < deadline {
        term.draw(|f| {
            inline.render(f, f.area(), DiffSideView::Both, &theme);
        })
        .unwrap();
        match &inline.state {
            InlineLoadState::Ready(_) => {
                settled = true;
                break;
            }
            InlineLoadState::Unavailable => panic!("valid png must not become unavailable"),
            _ => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(settled, "background load should settle to Ready within 2s");
    assert!(inline.estimated_bytes() > 4096);
}

#[test]
fn inline_corrupt_source_shows_fallback_without_blocking() {
    use crate::pager::side_by_side::DiffSideView;
    let repo = TempRepo::new("inline-corrupt-fallback");
    repo.write("img.png", b"definitely not an image");
    let repo_path = repo.repo_path();
    let picker = graphics_picker();
    let mut inline = InlineImagePreview::with_picker(
        &repo_path,
        InlineImageSource::Missing,
        InlineImageSource::Worktree("img.png".to_string()),
        picker,
    );
    let theme = crate::config::Theme::default();
    let mut term = Terminal::new(TestBackend::new(60, 14)).unwrap();
    let start = std::time::Instant::now();
    term.draw(|f| {
        inline.render(f, f.area(), DiffSideView::Both, &theme);
    })
    .unwrap();
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "corrupt render must not block"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut unavailable = false;
    while std::time::Instant::now() < deadline {
        term.draw(|f| {
            inline.render(f, f.area(), DiffSideView::Both, &theme);
        })
        .unwrap();
        if matches!(inline.state, InlineLoadState::Unavailable) {
            unavailable = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(unavailable, "corrupt must settle to Unavailable");
    let text = buffer_text(term.backend().buffer());
    assert!(
        text.contains("Binary cannot be previewed") || text.contains("╱"),
        "fallback stripes, got {text:?}"
    );
    assert!(inline.estimated_bytes() < 4096);
}

#[test]
fn inline_pending_unload_retains_loading_receiver() {
    let picker = graphics_picker();
    let mut inline = InlineImagePreview::with_picker(
        std::path::Path::new("."),
        InlineImageSource::Missing,
        InlineImageSource::Missing,
        picker,
    );
    let (_tx, rx) = std::sync::mpsc::channel::<Option<ImagePreview>>();
    inline.state = InlineLoadState::Loading(rx);
    inline.unload();
    assert!(
        matches!(inline.state, InlineLoadState::Loading(_)),
        "pending load must be retained"
    );
    let picker2 = graphics_picker();
    let mut inline2 = InlineImagePreview::with_picker(
        std::path::Path::new("."),
        InlineImageSource::Missing,
        InlineImageSource::Missing,
        picker2,
    );
    let (tx2, rx2) = std::sync::mpsc::channel::<Option<ImagePreview>>();
    tx2.send(None).unwrap();
    inline2.state = InlineLoadState::Loading(rx2);
    inline2.unload();
    assert!(
        matches!(inline2.state, InlineLoadState::Unavailable),
        "completed None must become Unavailable"
    );
}

#[test]
fn inline_ready_unload_and_cache_unload_free_pixels() {
    let mut ready = inline_ready_both_sides();
    assert!(matches!(ready.state, InlineLoadState::Ready(_)));
    let before = ready.estimated_bytes();
    assert!(before > 4096, "ready carries pixels, got {before}");
    ready.unload();
    assert!(matches!(ready.state, InlineLoadState::Idle));
    assert!(ready.estimated_bytes() < before);
    assert!(ready.estimated_bytes() < 4096);
    let mut cached = inline_ready_both_sides();
    cached.unload_for_cache();
    assert!(matches!(cached.state, InlineLoadState::Idle));
    assert!(cached.estimated_bytes() < 4096);
    let picker = graphics_picker();
    let mut unavailable = InlineImagePreview::with_picker(
        std::path::Path::new("."),
        InlineImageSource::Missing,
        InlineImageSource::Missing,
        picker,
    );
    unavailable.state = InlineLoadState::Unavailable;
    unavailable.unload();
    assert!(matches!(unavailable.state, InlineLoadState::Unavailable));
    unavailable.unload_for_cache();
    assert!(matches!(unavailable.state, InlineLoadState::Unavailable));
}

#[test]
fn inline_offscreen_stays_idle_until_viewport() {
    use crate::pager::side_by_side::{DiffViewLayout, DiffViewState, render_diff};
    let repo = TempRepo::new("inline-offscreen-idle");
    let bytes = png_bytes(16, 16);
    repo.write("img.png", &bytes);
    let repo_path = repo.repo_path();
    let diff = mixed_three_file_diff();
    let mut parsed = DiffViewState::parse_diff_output("mixed", &diff, 4, true);
    let middle = middle_image_section(&parsed);
    let picker = graphics_picker();
    let inline = InlineImagePreview::with_picker(
        &repo_path,
        InlineImageSource::Missing,
        InlineImageSource::Worktree("img.png".to_string()),
        picker,
    );
    assert!(matches!(inline.state, InlineLoadState::Idle));
    let mut map = std::collections::HashMap::new();
    map.insert(middle, inline);
    parsed.attach_inline_images(map);
    let mut state = DiffViewState::new();
    state.apply_parsed(parsed);
    state.view_layout = DiffViewLayout::SideBySide;
    state.wrap = false;
    state.scroll_offset = 0;
    let img_hdr = test_header_idx(&state, "img.png");
    assert!(img_hdr >= 6, "image below small viewport, got {img_hdr}");
    let theme = crate::config::Theme::default();
    let mut term = Terminal::new(TestBackend::new(60, 8)).unwrap();
    term.draw(|f| {
        render_diff(f, f.area(), &mut state, &theme, true, false, true, 0);
    })
    .unwrap();
    assert!(
        matches!(
            state.inline_images.get(&middle).unwrap().state,
            InlineLoadState::Idle
        ),
        "offscreen must stay Idle"
    );
    state.scroll_offset = img_hdr;
    let mut term2 = Terminal::new(TestBackend::new(60, 20)).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut started = false;
    while std::time::Instant::now() < deadline {
        term2
            .draw(|f| {
                render_diff(f, f.area(), &mut state, &theme, true, false, true, 0);
            })
            .unwrap();
        if !matches!(
            state.inline_images.get(&middle).unwrap().state,
            InlineLoadState::Idle
        ) {
            started = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(started, "viewport render must start load");
}

#[test]
fn inline_mixed_three_file_sticky_headers_and_bounded_image() {
    use crate::pager::side_by_side::{DiffViewLayout, DiffViewState};
    for wrap in [false, true] {
        for layout in [DiffViewLayout::SideBySide, DiffViewLayout::Unified] {
            let diff = mixed_three_file_diff();
            let mut parsed = DiffViewState::parse_diff_output("mixed", &diff, 4, true);
            let middle = middle_image_section(&parsed);
            let mut map = std::collections::HashMap::new();
            map.insert(middle, inline_ready_both_sides());
            parsed.attach_inline_images(map);
            assert_eq!(
                parsed
                    .lines
                    .iter()
                    .filter(|l| l.preview_placeholder.is_some() && l.section_index == middle)
                    .count(),
                INLINE_IMAGE_ROWS,
                "layout {layout:?} wrap {wrap}"
            );
            let mut state = DiffViewState::new();
            state.apply_parsed(parsed);
            state.wrap = wrap;
            let img_hdr = test_header_idx(&state, "img.png");
            let tail_hdr = test_header_idx(&state, "last.txt");
            assert!(tail_hdr > img_hdr, "tail after image");
            if layout == DiffViewLayout::Unified {
                state.view_layout = DiffViewLayout::Unified;
                state.scroll_offset = 0;
                let _ = render_state_buffer(&mut state, 60, 30);
                state.view_layout = DiffViewLayout::SideBySide;
                state.scroll_offset = img_hdr + 1;
                let _ = render_state_buffer(&mut state, 60, 30);
                state.toggle_view_layout();
                assert_eq!(state.view_layout, DiffViewLayout::Unified);
            } else {
                state.view_layout = DiffViewLayout::SideBySide;
                let _ = render_state_buffer(&mut state, 60, 30);
                state.scroll_offset = img_hdr + 1;
            }
            assert!(
                state
                    .sticky_file_header()
                    .is_some_and(|h| h.contains("img.png")),
                "layout {layout:?} wrap {wrap} sticky"
            );
            assert_eq!(state.sticky_header_line_idx(), Some(img_hdr));
            let buf = render_state_buffer(&mut state, 60, 30);
            let text = buffer_text(&buf);
            assert!(
                text.contains("Before") && text.contains("After"),
                "layout {layout:?} wrap {wrap} labels, got {text:?}"
            );
            assert!(
                text.contains("last.txt"),
                "tail header visible, layout {layout:?} wrap {wrap}: {:#?}",
                (0..buf.area.height)
                    .map(|y| test_row_string(&buf, y)
                        .chars()
                        .take(70)
                        .collect::<String>())
                    .collect::<Vec<_>>()
            );
            assert!(text.contains("tail"), "tail content visible");
            let mut before_y = None;
            let mut tail_y = None;
            for y in 0..buf.area.height {
                let row = test_row_string(&buf, y);
                if row.contains("Before") && before_y.is_none() {
                    before_y = Some(y);
                }
                if row.contains("last.txt") && tail_y.is_none() {
                    tail_y = Some(y);
                }
            }
            let (b, t) = (before_y.expect("Before row"), tail_y.expect("tail row"));
            assert!(b < t, "image above tail layout {layout:?} wrap {wrap}");
            assert!(
                (t as usize).saturating_sub(b as usize) <= INLINE_IMAGE_ROWS + 4,
                "tail just after image b={b} t={t}"
            );
            for y in (t + 1)..buf.area.height {
                let row = test_row_string(&buf, y);
                assert!(
                    !row.contains("Before") && !row.contains("After"),
                    "bounded y={y} {row:?}"
                );
            }
            if layout == DiffViewLayout::Unified {
                state.toggle_view_layout();
                assert_eq!(state.view_layout, DiffViewLayout::SideBySide);
                assert!(
                    (state.scroll_offset as isize - (img_hdr as isize + 1)).abs() <= 2,
                    "toggle back near image, got {}",
                    state.scroll_offset
                );
            }
        }
    }
}

#[test]
fn inline_mixed_added_deleted_full_width_and_scroll_unloads() {
    use crate::pager::side_by_side::{DiffSideView, DiffViewLayout, DiffViewState};
    for added in [true, false] {
        let diff = mixed_three_file_diff();
        let mut parsed = DiffViewState::parse_diff_output("mixed", &diff, 4, true);
        let middle = middle_image_section(&parsed);
        let mut map = std::collections::HashMap::new();
        map.insert(middle, inline_ready_single_sided(added));
        parsed.attach_inline_images(map);
        let mut state = DiffViewState::new();
        state.apply_parsed(parsed);
        state.view_layout = DiffViewLayout::SideBySide;
        state.side_view = DiffSideView::Both;
        state.wrap = false;
        let img_hdr = test_header_idx(&state, "img.png");
        state.scroll_offset = img_hdr + 1;
        let buf = render_state_buffer(&mut state, 60, 30);
        let text = buffer_text(&buf);
        let label = if added { "Added" } else { "Deleted" };
        assert!(text.contains(label), "added={added} label, got {text:?}");
        assert!(
            !text.contains("Before") && !text.contains("After"),
            "single-sided no Before/After"
        );
        let tail_hdr = test_header_idx(&state, "last.txt");
        state.scroll_offset = tail_hdr;
        let buf2 = render_state_buffer(&mut state, 60, 30);
        let text2 = buffer_text(&buf2);
        assert!(
            !text2.contains("Before") && !text2.contains("After"),
            "no labels past image"
        );
        assert!(
            !text2.contains(label),
            "label {label} gone after scroll, got {text2:?}"
        );
        let preview = state.inline_images.get(&middle).unwrap();
        assert!(
            matches!(preview.state, InlineLoadState::Idle),
            "scrolled past must unload to Idle"
        );
        assert!(preview.estimated_bytes() < 4096);
    }
}

#[test]
fn inline_concurrent_loads_settle_without_global_counter() {
    use crate::pager::side_by_side::DiffSideView;
    let repo = TempRepo::new("inline-concurrent-loads");
    for (name, color) in [
        ("a.png", [200, 30, 30]),
        ("b.png", [30, 200, 30]),
        ("c.png", [30, 30, 200]),
    ] {
        repo.write(name, &colored_png(color, 12, 12));
    }
    let repo_path = repo.repo_path();
    let picker = graphics_picker();
    let mut inlines: Vec<InlineImagePreview> = ["a.png", "b.png", "c.png"]
        .into_iter()
        .map(|n| {
            InlineImagePreview::with_picker(
                &repo_path,
                InlineImageSource::Missing,
                InlineImageSource::Worktree(n.to_string()),
                picker.clone(),
            )
        })
        .collect();
    let theme = crate::config::Theme::default();
    let mut term = Terminal::new(TestBackend::new(60, 14)).unwrap();
    let start = std::time::Instant::now();
    for inline in inlines.iter_mut() {
        term.draw(|f| {
            inline.render(f, f.area(), DiffSideView::Both, &theme);
        })
        .unwrap();
    }
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "kickoff must not block"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let mut all_ready = true;
        for inline in inlines.iter_mut() {
            term.draw(|f| {
                inline.render(f, f.area(), DiffSideView::Both, &theme);
            })
            .unwrap();
            match &inline.state {
                InlineLoadState::Ready(_) => {}
                InlineLoadState::Unavailable => panic!("valid png unavailable"),
                _ => {
                    all_ready = false;
                }
            }
        }
        if all_ready {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!("concurrent loads should settle within 2s");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
