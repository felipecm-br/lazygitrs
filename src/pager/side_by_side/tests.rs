
use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn diff_line(change_type: ChangeType) -> DiffLine {
    DiffLine {
        old_line: None,
        new_line: None,
        change_type,
        old_segments: None,
        new_segments: None,
        file_header: None,
        comment_notes: Vec::new(),
        preview_placeholder: None,
        section_index: 0,
    }
}

#[test]
fn head_buffer_classifies_staged_hunks_without_separator() {
    // Real `git diff HEAD` output: staged edit at line 2, unstaged edit
    // at line 9, merged into a single `@@` by git. Block-level overlap
    // still tells them apart.
    let head = "diff --git a/f.txt b/f.txt\nindex f00c965..edee7ac 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,10 +1,10 @@\n 1\n-2\n+TWO\n 3\n 4\n 5\n 6\n 7\n 8\n-9\n+NINE\n 10\n";
    let unstaged = "diff --git a/f.txt b/f.txt\nindex 9935360..edee7ac 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -6,5 +6,5 @@ TWO\n 6\n 7\n 8\n-9\n+NINE\n 10\n";
    let parsed = DiffViewState::parse_head_with_staged("f.txt", head, unstaged, 4, true);
    assert_eq!(parsed.hunk_starts.len(), 2);
    assert_eq!(parsed.hunk_staged, vec![true, false]);
    // No separator: a single coherent buffer.
    assert!(
        parsed.lines.iter().all(|l| l.file_header.is_none()),
        "single buffer must not contain section separators"
    );
    let mut state = DiffViewState::new();
    state.apply_parsed(parsed);
    assert_eq!(state.staged_counts(), Some((1, 1)));
    assert!(state.is_staged_hunk(0));
    assert!(!state.is_staged_hunk(1));
    // Staged hunk keeps full color; unstaged line dims.
    assert!(state.is_staged_line(state.hunk_starts[0]));
    assert!(!state.is_staged_line(state.hunk_starts[1]));
}

#[test]
fn staged_deletion_classifies_as_staged() {
    // Staged deletion of line 2; unstaged edit of line 9.
    let head = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,3 @@\n 1\n-2\n 3\n 4\n@@ -7,4 +6,4 @@\n 7\n 8\n-9\n+NINE\n";
    let unstaged = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -5,5 +5,5 @@\n 6\n 7\n 8\n-9\n+NINE\n 10\n";
    let parsed = DiffViewState::parse_head_with_staged("f.txt", head, unstaged, 4, true);
    assert_eq!(parsed.hunk_staged, vec![true, false]);
}

#[test]
fn head_flags_mark_missing_unstaged_files_as_staged() {
    // Multi-file HEAD buffer where b.txt has no unstaged counterpart.
    let head = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n a\n-b\n+B\ndiff --git a/b.txt b/b.txt\n--- a/b.txt\n+++ b/b.txt\n@@ -1,2 +1,2 @@\n x\n-y\n+Y\n";
    let unstaged =
        "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n";
    let flags = head_block_staged_flags(head, unstaged, 4);
    assert_eq!(flags, vec![false, true]);
}

#[test]
fn unclassified_diff_has_no_staged_counts() {
    // Commits/stash carry no staged classification: full color, no counts.
    let parsed = DiffViewState::parse_diff_output(
        "f.txt",
        "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n",
        4,
        true,
    );
    assert!(parsed.hunk_staged.is_empty());
    let mut state = DiffViewState::new();
    state.apply_parsed(parsed);
    assert_eq!(state.staged_counts(), None);
    assert!(state.is_staged_line(state.hunk_starts[0]));
}

#[test]
fn hunk_position_tracks_the_viewport() {
    let mut state = DiffViewState::new();
    state.hunk_starts = vec![3, 8, 14];

    state.scroll_offset = 0;
    assert_eq!(state.hunk_position(), Some((1, 3)));

    state.scroll_offset = 3;
    assert_eq!(state.hunk_position(), Some((1, 3)));

    state.scroll_offset = 12;
    assert_eq!(state.hunk_position(), Some((2, 3)));

    state.scroll_offset = 14;
    assert_eq!(state.hunk_position(), Some((3, 3)));
}

#[test]
fn diff_border_shows_hunk_position_at_top_right() {
    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut state = DiffViewState::new();
    state.filename = "file.txt".to_string();
    state.lines = (0..5).map(|_| diff_line(ChangeType::Equal)).collect();
    state.hunk_starts = vec![1, 4];
    state.scroll_offset = 4;

    terminal
        .draw(|frame| {
            render_diff(
                frame,
                Rect::new(0, 0, 40, 6),
                &mut state,
                &Theme::dark(),
                true,
                false,
                false,
                0,
            );
        })
        .expect("diff should render");

    let top_border: String = (0..40)
        .map(|x| {
            terminal
                .backend()
                .buffer()
                .cell((x, 0))
                .and_then(|cell| cell.symbol().chars().next())
                .unwrap_or(' ')
        })
        .collect();
    assert!(
        top_border.ends_with(" [2/2] ╮") || top_border.ends_with(" [2/2] ┐"),
        "{top_border:?}"
    );
}

#[test]
fn unified_deleted_row_maps_to_old_panel() {
    let mut state = DiffViewState::new();
    state.view_layout = DiffViewLayout::Unified;
    let mut line = diff_line(ChangeType::Delete);
    line.old_line = Some((7, "removed".to_string()));
    state.lines = vec![line];

    let layout = DiffPanelLayout::compute(Rect::new(0, 0, 80, 6), &state);

    assert_eq!(
        state.line_chunk_panel_at_row(layout.inner_y, &layout, DiffPanel::New),
        Some((0, 0, DiffPanel::Old))
    );
}

#[test]
fn unified_modified_rows_map_old_then_new() {
    let mut state = DiffViewState::new();
    state.view_layout = DiffViewLayout::Unified;
    let mut line = diff_line(ChangeType::Modified);
    line.old_line = Some((7, "old".to_string()));
    line.new_line = Some((8, "new".to_string()));
    state.lines = vec![line];

    let layout = DiffPanelLayout::compute(Rect::new(0, 0, 80, 6), &state);

    assert_eq!(
        state.line_chunk_panel_at_row(layout.inner_y, &layout, DiffPanel::New),
        Some((0, 0, DiffPanel::Old))
    );
    assert_eq!(
        state.line_chunk_panel_at_row(layout.inner_y + 1, &layout, DiffPanel::New),
        Some((0, 1, DiffPanel::New))
    );
}

#[test]
fn unified_modified_blocks_render_all_deletions_before_insertions() {
    let diff = "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1,4 +1,5 @@\n context before\n-old one\n-old two\n+new one\n+new two\n+new three\n context after\n";
    let parsed = DiffViewState::parse_diff_output("file.txt", diff, 4, true);
    let mut state = DiffViewState::new();
    state.view_layout = DiffViewLayout::Unified;
    state.apply_parsed(parsed);

    let mut buf = Buffer::empty(Rect::new(0, 0, 80, 10));
    render_unified_diff_body(
        &mut buf,
        Rect::new(0, 0, 80, 10),
        &mut state,
        &Theme::dark(),
        10,
        false,
        0,
    );

    let signs: String = (0..7)
        .map(|row| {
            buf.cell((11, row))
                .and_then(|cell| cell.symbol().chars().next())
                .unwrap_or(' ')
        })
        .collect();
    assert_eq!(signs, " --+++ ");
}

#[test]
fn no_newline_marker_does_not_shift_unified_markdown_highlights() {
    let diff = "diff --git a/_plans/TODOS.md b/_plans/TODOS.md\nindex f13d2a0..6186190 100644\n--- a/_plans/TODOS.md\n+++ b/_plans/TODOS.md\n@@ -63,4 +63,5 @@ Sidequests\n \n - [ ] Onboarding\n \n-- [ ] Cell wrapping when saving textarea for instance or description\n\\ No newline at end of file\n+- [ ] Cell wrapping when saving textarea for instance or description\n+- [ ] Currency input and parsing natural inputs like 1k. or even formulas like 4+12+34+12k\n";
    let parsed = DiffViewState::parse_diff_output("_plans/TODOS.md", diff, 4, true);

    assert!(!parsed.old_content.contains("No newline at end of file"));
    assert!(!parsed.new_content.contains("No newline at end of file"));

    let mut state = DiffViewState::new();
    state.view_layout = DiffViewLayout::Unified;
    state.apply_parsed(parsed);

    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 8));
    render_unified_diff_body(
        &mut buf,
        Rect::new(0, 0, 120, 8),
        &mut state,
        &Theme::dark(),
        8,
        false,
        0,
    );

    let row_text = |buf: &Buffer, row: u16| -> String {
        (0..buf.area().width)
            .map(|x| {
                buf.cell((x, row))
                    .and_then(|cell| cell.symbol().chars().next())
                    .unwrap_or(' ')
            })
            .collect::<String>()
    };

    assert!(row_text(&buf, 4).contains("Cell wrapping"));
    assert!(row_text(&buf, 5).contains("Currency input"));
}

#[test]
fn unified_mid_block_scroll_keeps_earlier_deletes_visible() {
    // Regression: DiffLine-indexed scroll used to drop delete N and insert N
    // together when scrolling through a Modified block, so earlier lines
    // vanished as you scrolled down. Visual-row scroll keeps the flattened
    // delete-then-insert stream contiguous.
    let mut state = DiffViewState::new();
    state.view_layout = DiffViewLayout::Unified;
    state.last_content_width = 80;
    let mut lines = Vec::new();
    for i in 0..4 {
        let mut line = diff_line(ChangeType::Modified);
        line.old_line = Some((100 + i, format!("old {i}")));
        line.new_line = Some((200 + i, format!("new {i}")));
        lines.push(line);
    }
    state.lines = lines;

    // Visual stream is: -old0 -old1 -old2 -old3 +new0 +new1 +new2 +new3
    assert_eq!(state.unified_total_visual_rows(80), 8);

    // Scroll past the first delete only.
    state.scroll_offset = 1;
    let mut buf = Buffer::empty(Rect::new(0, 0, 80, 6));
    render_unified_diff_body(
        &mut buf,
        Rect::new(0, 0, 80, 6),
        &mut state,
        &Theme::dark(),
        6,
        false,
        0,
    );

    let signs: String = (0..6)
        .map(|row| {
            buf.cell((11, row))
                .and_then(|cell| cell.symbol().chars().next())
                .unwrap_or(' ')
        })
        .collect();
    // First delete is scrolled off; remaining deletes then inserts stay.
    assert_eq!(signs, "---+++");
    // old 0 must be gone, old 1 still present.
    let row_text = |row: u16| -> String {
        (0..80)
            .map(|x| {
                buf.cell((x, row))
                    .and_then(|cell| cell.symbol().chars().next())
                    .unwrap_or(' ')
            })
            .collect()
    };
    assert!(row_text(0).contains("old 1"));
    assert!(!row_text(0).contains("old 0"));
    assert!(row_text(3).contains("new 0"));
}

#[test]
fn unified_modified_block_rows_map_old_block_then_new_block() {
    let mut state = DiffViewState::new();
    state.view_layout = DiffViewLayout::Unified;
    let mut first = diff_line(ChangeType::Modified);
    first.old_line = Some((7, "old one".to_string()));
    first.new_line = Some((8, "new one".to_string()));
    let mut second = diff_line(ChangeType::Modified);
    second.old_line = Some((8, "old two".to_string()));
    second.new_line = Some((9, "new two".to_string()));
    let mut third = diff_line(ChangeType::Insert);
    third.new_line = Some((10, "new three".to_string()));
    state.lines = vec![first, second, third];

    let layout = DiffPanelLayout::compute(Rect::new(0, 0, 80, 8), &state);

    assert_eq!(
        state.line_chunk_panel_at_row(layout.inner_y, &layout, DiffPanel::New),
        Some((0, 0, DiffPanel::Old))
    );
    assert_eq!(
        state.line_chunk_panel_at_row(layout.inner_y + 1, &layout, DiffPanel::New),
        Some((1, 0, DiffPanel::Old))
    );
    assert_eq!(
        state.line_chunk_panel_at_row(layout.inner_y + 2, &layout, DiffPanel::New),
        Some((0, 1, DiffPanel::New))
    );
    assert_eq!(
        state.line_chunk_panel_at_row(layout.inner_y + 3, &layout, DiffPanel::New),
        Some((1, 1, DiffPanel::New))
    );
    assert_eq!(
        state.line_chunk_panel_at_row(layout.inner_y + 4, &layout, DiffPanel::New),
        Some((2, 0, DiffPanel::New))
    );
}

#[test]
fn pure_rename_diff_produces_visible_modified_row() {
    let diff = "diff --git a/src/views/openai_oauth_flow.rs b/src/views/provider_oauth_flow.rs\nsimilarity index 100%\nrename from src/views/openai_oauth_flow.rs\nrename to src/views/provider_oauth_flow.rs\n";

    let parsed = DiffViewState::parse_diff_output(
        "src/views/openai_oauth_flow.rs -> src/views/provider_oauth_flow.rs",
        diff,
        4,
        true,
    );

    assert_eq!(parsed.old_content, "src/views/openai_oauth_flow.rs");
    assert_eq!(parsed.new_content, "src/views/provider_oauth_flow.rs");
    assert_eq!(parsed.lines.len(), 1);
    assert_eq!(parsed.lines[0].change_type, ChangeType::Modified);
    assert_eq!(
        parsed.lines[0].old_line,
        Some((1, "src/views/openai_oauth_flow.rs".to_string()))
    );
    assert_eq!(
        parsed.lines[0].new_line,
        Some((1, "src/views/provider_oauth_flow.rs".to_string()))
    );
}

#[test]
fn detects_rename_only_without_hunks() {
    let diff = "diff --git a/old/tab-indent.ts b/new/tab-indent.ts\n\
                    similarity index 100%\n\
                    rename from old/tab-indent.ts\n\
                    rename to new/tab-indent.ts\n";
    assert!(is_rename_only_diff(diff));
    assert!(!is_rename_only_diff(
        "diff --git a/a.rs b/b.rs\n\
             similarity index 90%\n\
             rename from a.rs\n\
             rename to b.rs\n\
             @@ -1 +1 @@\n\
             -old\n\
             +new\n"
    ));
}

#[test]
fn binary_diff_shows_not_viewable_placeholder() {
    let diff = "diff --git a/foo.png b/foo.png\n\
                    new file mode 100644\n\
                    index 0000000..e8ef7b2\n\
                    Binary files /dev/null and b/foo.png differ\n";
    let parsed = DiffViewState::parse_diff_output("foo.png", diff, 4, true);
    assert_eq!(parsed.lines.len(), 5);
    assert_eq!(
        parsed.lines[2].preview_placeholder,
        Some("Binary cannot be previewed")
    );
    assert!(
        parsed
            .lines
            .iter()
            .all(|line| line.old_line.is_none() && line.new_line.is_none())
    );
    assert!(parsed.hunk_starts.is_empty());
    assert!(parsed.hunk_line_offsets.is_empty());
}

const BINARY_DIFF: &str =
    "diff --git a/foo.png b/foo.png\nBinary files a/foo.png and b/foo.png differ\n";
const TEXT_DIFF: &str =
    "diff --git a/text.txt b/text.txt\n--- a/text.txt\n+++ b/text.txt\n@@ -1 +1 @@\n-old\n+new\n";

fn render_preview(state: &mut DiffViewState, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            render_diff(
                frame,
                Rect::new(0, 0, width, height),
                state,
                &Theme::dark(),
                true,
                false,
                true,
                0,
            );
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn buffer_row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

#[test]
fn lone_binary_fills_pane_in_every_layout() {
    for layout in [DiffViewLayout::SideBySide, DiffViewLayout::Unified] {
        for side in [
            DiffSideView::Both,
            DiffSideView::OldOnly,
            DiffSideView::NewOnly,
        ] {
            let mut state = DiffViewState::new();
            state.load_from_diff_output("foo.png", BINARY_DIFF);
            state.view_layout = layout;
            state.side_view = side;
            let buf = render_preview(&mut state, 60, 12);
            assert!(buffer_row(&buf, 6).contains("  Binary cannot be previewed  "));
            for y in [1, 4, 8, 10] {
                assert_eq!(
                    (1..59).map(|x| buf[(x, y)].symbol()).collect::<String>(),
                    "╱".repeat(58)
                );
            }
            for y in [5, 7] {
                assert_eq!(
                    buffer_row(&buf, y),
                    format!("│{}{}{}│", "╱".repeat(14), " ".repeat(30), "╱".repeat(14))
                );
            }
            assert_eq!(buf[(1, 1)].fg, Theme::dark().diff_line_number);
            assert!(state.file_line_number(1, DiffPanel::New).is_none());
        }
    }
}

#[test]
fn mixed_binary_placeholder_is_compact_in_every_layout() {
    let diff = format!("{BINARY_DIFF}{TEXT_DIFF}{BINARY_DIFF}");
    for layout in [DiffViewLayout::SideBySide, DiffViewLayout::Unified] {
        for side in [
            DiffSideView::Both,
            DiffSideView::OldOnly,
            DiffSideView::NewOnly,
        ] {
            for wrap in [false, true] {
                for width in [24, 80] {
                    let mut state = DiffViewState::new();
                    state.apply_parsed(DiffViewState::parse_diff_output("mixed", &diff, 4, true));
                    state.view_layout = layout;
                    state.side_view = side;
                    state.wrap = wrap;
                    let buf = render_preview(&mut state, width, 18);
                    assert!(buffer_row(&buf, 1).contains("foo.png"));
                    assert!(buffer_row(&buf, 4).contains("Binary"));
                    for y in [2, 6] {
                        assert_eq!(
                            (1..width - 1)
                                .map(|x| buf[(x, y)].symbol())
                                .collect::<String>(),
                            "╱".repeat(width as usize - 2)
                        );
                    }
                    for y in [3, 5] {
                        let row = buffer_row(&buf, y);
                        assert!(row.contains(&" ".repeat(30.min(width as usize - 2))));
                        if width == 80 {
                            assert!(row.contains('╱'));
                        }
                    }
                    assert!(buffer_row(&buf, 7).contains("text.txt"));
                    assert!(
                        buffer_row(&buf, 8).contains(if side == DiffSideView::NewOnly {
                            "new"
                        } else {
                            "old"
                        })
                    );
                    // Exactly five rows per binary, not viewport-sized blocks.
                    assert_eq!(
                        state
                            .lines
                            .iter()
                            .filter(|line| line.preview_placeholder.is_some())
                            .count(),
                        10
                    );
                    assert_eq!(state.hunk_starts, vec![7]);
                    assert_eq!(state.file_line_number(7, DiffPanel::New), Some(1));
                    let panel = DiffPanelLayout::compute(buf.area, &state);
                    assert_eq!(state.line_chunk_at_row(7, &panel), Some((6, 0)));
                    state.start_search();
                    state.search_query = "Binary".to_string();
                    state.update_search();
                    assert!(state.search_matches.is_empty());
                }
            }
        }
    }
}

#[test]
fn scrolling_into_binary_keeps_sticky_header_and_next_file_aligned() {
    for layout in [DiffViewLayout::SideBySide, DiffViewLayout::Unified] {
        for wrap in [false, true] {
            let mut state = DiffViewState::new();
            state.load_from_diff_output("mixed", &format!("{BINARY_DIFF}{TEXT_DIFF}"));
            state.view_layout = layout;
            state.wrap = wrap;
            state.scroll_offset = 3; // Middle row of the binary placeholder.
            let buf = render_preview(&mut state, 80, 10);
            assert!(buffer_row(&buf, 1).contains("foo.png"));
            assert!(buffer_row(&buf, 2).contains("Binary cannot be previewed"));
            assert!(buffer_row(&buf, 3).contains('╱'));
            assert_eq!(
                (1..79).map(|x| buf[(x, 4)].symbol()).collect::<String>(),
                "╱".repeat(78)
            );
            assert!(buffer_row(&buf, 5).contains("text.txt"));
            let panel = DiffPanelLayout::compute(buf.area, &state);
            assert_eq!(state.line_chunk_at_row(5, &panel), Some((6, 0)));
        }
    }
}

#[test]
fn placeholder_clips_to_tiny_offset_areas() {
    for width in 0..32 {
        for height in 0..4 {
            let mut buf = Buffer::empty(Rect::new(0, 0, 40, 8));
            let area = Rect::new(3, 2, width, height);
            render_preview_placeholder(
                &mut buf,
                area,
                "Binary cannot be previewed",
                &Theme::dark(),
            );
            for y in 0..8 {
                for x in 0..40 {
                    if x < area.x || x >= area.right() || y < area.y || y >= area.bottom() {
                        assert_eq!(buf[(x, y)].symbol(), " ");
                    }
                }
            }
        }
    }
}

#[test]
fn binary_patch_is_placeholder_but_binary_words_in_source_are_not() {
    let parsed = DiffViewState::parse_diff_output(
        "foo.png",
        "diff --git a/foo.png b/foo.png\nGIT binary patch\nliteral 3\nabc\n",
        4,
        true,
    );
    assert_eq!(
        parsed.lines[2].preview_placeholder,
        Some("Binary cannot be previewed")
    );
    let text = "diff --git a/text.txt b/text.txt\n--- a/text.txt\n+++ b/text.txt\n@@ -1,2 +1,2 @@\n Binary files are normal text here\n-old\n+new\n";
    let parsed = DiffViewState::parse_diff_output("text.txt", text, 4, true);
    assert!(
        parsed
            .lines
            .iter()
            .all(|line| line.preview_placeholder.is_none())
    );
    assert!(!parsed.hunk_starts.is_empty());
}

#[test]
fn diff_border_renders_scrollbar_on_right_border() {
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut state = DiffViewState::new();
    state.filename = "file.txt".to_string();
    state.lines = (0..50).map(|_| diff_line(ChangeType::Equal)).collect();
    state.scroll_offset = 0;

    terminal
        .draw(|frame| {
            render_diff(
                frame,
                Rect::new(0, 0, 40, 10),
                &mut state,
                &Theme::dark(),
                true,
                false,
                false,
                0,
            );
        })
        .expect("diff should render");

    let buffer = terminal.backend().buffer();
    // The right border column is at x = 39. Rows 1..=8 are the inner border cells.
    let right_border: Vec<String> = (1..9)
        .map(|y| buffer[(39, y)].symbol().to_string())
        .collect();

    // There should be at least one '▐' (Right Half Block) scrollbar character rendered on the right border
    assert!(
        right_border.iter().any(|c| c == "▐"),
        "Expected scrollbar '▐' on right border, but got: {:?}",
        right_border
    );
}

#[test]
fn inline_rows_match_const_and_middle_label() {
    assert_eq!(super::super::image_preview::INLINE_IMAGE_ROWS, 12);
    let rows = super::inline_image_placeholder_rows(3);
    assert_eq!(rows.len(), 12);
    assert!(rows.iter().all(|l| l.file_header.is_none()));
    assert!(rows.iter().all(|l| l.section_index == 3));
    assert!(
        rows.iter()
            .all(|l| l.old_line.is_none() && l.new_line.is_none())
    );
    let mid = 12 / 2;
    assert_eq!(
        rows[mid].preview_placeholder,
        Some("Binary cannot be previewed")
    );
    // Neighbours are blank padding, rest are empty hatch rows.
    assert_eq!(
        rows[mid - 1].preview_placeholder,
        Some("                          ")
    );
    assert_eq!(
        rows[mid + 1].preview_placeholder,
        Some("                          ")
    );
    assert_eq!(rows[0].preview_placeholder, Some(""));
}

#[test]
fn attach_empty_is_noop() {
    let mut parsed = DiffViewState::parse_diff_output("foo.png", BINARY_DIFF, 4, true);
    let before = parsed.lines.len();
    parsed.attach_inline_images(std::collections::HashMap::<
        usize,
        super::super::image_preview::InlineImagePreview,
    >::new());
    assert_eq!(parsed.lines.len(), before);
    assert!(parsed.inline_images.is_empty());
}

#[test]
fn image_expansion_replaces_binary_and_remaps_text_hunks() {
    let diff = format!("{BINARY_DIFF}{TEXT_DIFF}");
    let mut parsed = DiffViewState::parse_diff_output("mixed", &diff, 4, true);
    // Preconditions: 2 file headers + 5 binary rows + text hunk.
    let headers: Vec<usize> = parsed
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.file_header.as_ref().map(|_| i))
        .collect();
    assert_eq!(headers.len(), 2);
    let binary_section = parsed.lines[headers[0] + 1].section_index;
    let text_section = parsed.lines[headers[1] + 1].section_index;
    assert_ne!(binary_section, text_section);
    let binary_rows = parsed
        .lines
        .iter()
        .filter(|l| l.preview_placeholder.is_some())
        .count();
    assert_eq!(binary_rows, 5);
    let old_hunk = parsed.hunk_starts[0];
    let old_offsets = parsed.hunk_line_offsets.clone();
    let text_header = parsed.lines[headers[1]]
        .file_header
        .clone()
        .expect("text file header");

    let mut sections = std::collections::HashSet::new();
    sections.insert(binary_section);
    parsed.expand_lines_for_image_sections(&sections);

    // Binary block grew 5 -> 12 (+7); header preserved.
    assert!(parsed.lines[headers[0]].file_header.is_some());
    let new_binary_rows: Vec<_> = parsed
        .lines
        .iter()
        .filter(|l| l.preview_placeholder.is_some() && l.section_index == binary_section)
        .collect();
    assert_eq!(new_binary_rows.len(), 12);
    assert_eq!(
        new_binary_rows[6].preview_placeholder,
        Some("Binary cannot be previewed")
    );
    // Text hunks shifted by +7, offsets remapped, staged untouched.
    assert_eq!(parsed.hunk_starts.len(), 1);
    assert_eq!(parsed.hunk_starts[0], old_hunk + 7);
    assert_eq!(parsed.hunk_line_offsets.len(), old_offsets.len());
    for ((new_idx, _, _), (old_idx, _, _)) in
        parsed.hunk_line_offsets.iter().zip(old_offsets.iter())
    {
        assert_eq!(*new_idx, *old_idx + 7);
    }
    assert!(parsed.hunk_staged.is_empty());
    // Text content still resolves to the same file.
    let mut state = DiffViewState::new();
    state.apply_parsed(parsed);
    assert_eq!(
        state.file_line_number(state.hunk_starts[0], DiffPanel::New),
        Some(1)
    );
    assert_eq!(state.file_at_line(state.hunk_starts[0]), text_header);
}

#[test]
fn rename_only_section_expands_to_image_rows() {
    let diff = "diff --git a/old.png b/new.png\nsimilarity index 100%\nrename from old.png\nrename to new.png\n";
    let mut parsed = DiffViewState::parse_diff_output("old.png -> new.png", diff, 4, true);
    assert_eq!(parsed.lines.len(), 1);
    let mut sections = std::collections::HashSet::new();
    sections.insert(0);
    parsed.expand_lines_for_image_sections(&sections);
    assert_eq!(parsed.lines.len(), 12);
    assert!(parsed.lines.iter().all(|l| l.preview_placeholder.is_some()));
    assert_eq!(
        parsed.lines[6].preview_placeholder,
        Some("Binary cannot be previewed")
    );
    assert!(parsed.hunk_starts.is_empty());
    assert!(parsed.hunk_line_offsets.is_empty());
}

#[test]
fn image_rows_map_via_line_chunk_and_sticky() {
    let diff = format!("{BINARY_DIFF}{TEXT_DIFF}");
    let mut parsed = DiffViewState::parse_diff_output("mixed", &diff, 4, true);
    let binary_section = parsed.lines[1].section_index;
    let mut sections = std::collections::HashSet::new();
    sections.insert(binary_section);
    parsed.expand_lines_for_image_sections(&sections);
    let mut state = DiffViewState::new();
    state.apply_parsed(parsed);
    for layout in [DiffViewLayout::SideBySide, DiffViewLayout::Unified] {
        for wrap in [false, true] {
            state.view_layout = layout;
            state.wrap = wrap;
            state.scroll_offset = 0;
            let area = Rect::new(0, 0, 80, 18);
            let panel = DiffPanelLayout::compute(area, &state);
            // First content row after the header is an image placeholder.
            // Header at row 1, image rows follow; sticky only when scrolled.
            assert_eq!(state.line_chunk_at_row(1, &panel), Some((0, 0)));
            // Image block rows map to placeholder lines in the image section.
            let mapped = state.line_chunk_at_row(2, &panel).expect("image row maps");
            let line = &state.lines[mapped.0];
            assert!(line.preview_placeholder.is_some());
            assert_eq!(line.section_index, binary_section);
            // Scroll into the middle of the image block: sticky pins header,
            // next file stays aligned and text hunks still resolve.
            state.scroll_offset = 3;
            let buf = render_preview(&mut state, 80, 10);
            assert!(buffer_row(&buf, 1).contains("foo.png"));
            // No text row is mistaken for an image row.
            let panel2 = DiffPanelLayout::compute(buf.area, &state);
            for y in panel2.inner_y..panel2.inner_end_y {
                if state.is_sticky_row(y, &panel2) {
                    continue;
                }
                if let Some((idx, _)) = state.line_chunk_at_row(y, &panel2) {
                    let l = &state.lines[idx];
                    if l.file_header.is_some() {
                        continue;
                    }
                    if l.preview_placeholder.is_none() {
                        assert_ne!(l.section_index, binary_section);
                    }
                }
            }
            state.scroll_offset = 0;
        }
    }
}

#[test]
fn inline_regions_noop_when_hidden_or_empty() {
    let mut state = DiffViewState::new();
    state.load_from_diff_output("foo.png", BINARY_DIFF);
    // Empty inline map: normal stripes still render, overlay is a noop.
    let buf = render_preview(&mut state, 60, 12);
    assert!(buffer_row(&buf, 6).contains("Binary cannot be previewed"));
    // Hidden flag with empty map also renders stripes without panic.
    state.image_preview_hidden = true;
    let buf2 = render_preview(&mut state, 60, 12);
    assert!(buffer_row(&buf2, 6).contains("Binary cannot be previewed"));
}
#[test]
fn split_binary_sections_keep_paths_for_lazy_image_loading() {
    let diff = "diff --git a/assets/old name.png b/assets/new name.png\n\
                    similarity index 90%\nrename from assets/old name.png\n\
                    rename to assets/new name.png\n\
                    Binary files a/assets/old name.png and b/assets/new name.png differ\n\
                    diff --git a/text.txt b/text.txt\n--- a/text.txt\n+++ b/text.txt\n\
                    @@ -1 +1 @@\n-old\n+new\n";
    let sections = parse_multi_file_diff(diff);
    assert_eq!(sections.len(), 2);
    assert!(sections[0].1.starts_with("diff --git "));
    let paths = crate::git::diff_paths::paths_from_diff(sections[0].1).unwrap();
    assert_eq!(paths.old.as_deref(), Some("assets/old name.png"));
    assert_eq!(paths.new.as_deref(), Some("assets/new name.png"));
    assert_eq!(sections[1].0, "text.txt");
}
