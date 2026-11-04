use super::{checklist_item_at, command_log_geometry, format_key_hint, menu_item_at, render_popup};
use crate::config::Theme;
use crate::gui::popup::{ChecklistItem, MenuItem, MessageKind, PopupState};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn render_tab(
    model: &crate::model::Model,
    context: super::ContextId,
    mode: super::ScreenMode,
) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let mut contexts = super::ContextManager::new();
    contexts.set_active(context);
    terminal
        .draw(|frame| {
            super::render(
                frame,
                model,
                &mut contexts,
                &super::LayoutState::default(),
                &PopupState::None,
                &crate::config::AppConfig {
                    debug: false,
                    version: String::new(),
                    user_config: Default::default(),
                    app_state: Default::default(),
                    config_dir: Default::default(),
                    state_dir: Default::default(),
                    state_path: Default::default(),
                },
                &Theme::default(),
                &mut super::DiffViewState::default(),
                &mut super::presentation::commits::CommitListCache::default(),
                mode,
                false,
                &[],
                &Default::default(),
                false,
                None,
                None,
                &[],
                false,
                &[],
                false,
                &[],
                &Default::default(),
                "",
                "",
                "",
                "",
                super::ContextId::Commits,
                0,
                None,
                false,
                &[],
                None,
                false,
                false,
                &Default::default(),
                &Default::default(),
                &mut 0,
                &mut String::new(),
                false,
                false,
                false,
                false,
                false,
            );
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(160)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect()
}

fn sample_worktree(path: &str, branch: &str, current: bool, main: bool) -> crate::model::Worktree {
    crate::model::Worktree {
        path: path.into(),
        branch: branch.into(),
        hash: String::new(),
        is_current: current,
        is_main: main,
    }
}

#[test]
fn worktrees_render_names_branches_and_main_label_in_every_mode() {
    let model = crate::model::Model {
        worktrees: vec![
            sample_worktree("/repos/quarta", "feat/main", true, true),
            sample_worktree(
                "/repos/worktree-pfparser",
                "experiment/parser",
                false,
                false,
            ),
        ],
        ..Default::default()
    };
    for mode in [
        super::ScreenMode::Normal,
        super::ScreenMode::Half,
        super::ScreenMode::Full,
    ] {
        let rows = render_tab(&model, super::ContextId::Worktrees, mode);
        let main = rows.iter().find(|row| row.contains("* quarta")).unwrap();
        assert!(main.contains("* quarta"));
        assert!(main.contains("feat/main (main worktree)"));
        assert!(!main.contains("/repos/"));
        let linked = rows
            .iter()
            .find(|row| row.contains("worktree-pfparser experiment/parser"))
            .unwrap();
        assert!(linked.contains("worktree-pfparser experiment/parser"));
        assert!(!linked.contains("(main worktree)"));
        assert!(!linked.contains("/repos/"));
        assert_eq!(main.find("feat/main"), linked.find("experiment/parser"));
    }
}

#[test]
fn worktree_columns_use_unicode_display_width() {
    let model = crate::model::Model {
        worktrees: vec![
            sample_worktree("/repos/树", "branch-one", false, false),
            sample_worktree("/repos/abc", "branch-two", true, false),
        ],
        ..Default::default()
    };
    let rows = render_tab(&model, super::ContextId::Worktrees, super::ScreenMode::Full);
    assert!(rows.iter().any(|row| row.contains("树   branch-one")));
    assert!(rows.iter().any(|row| row.contains("* abc branch-two")));
}

#[test]
fn submodules_render_in_every_mode_and_empty_lists_stay_blank() {
    let mut model = crate::model::Model::default();
    for mode in [
        super::ScreenMode::Normal,
        super::ScreenMode::Half,
        super::ScreenMode::Full,
    ] {
        let rows = render_tab(&model, super::ContextId::Submodules, mode);
        assert!(!rows.join("\n").contains("(no submodules)"));
    }
    model.submodules.push(crate::git::submodule::Submodule {
        name: "shared-lib".into(),
        path: "vendor/shared-lib".into(),
        url: String::new(),
    });
    for mode in [
        super::ScreenMode::Normal,
        super::ScreenMode::Half,
        super::ScreenMode::Full,
    ] {
        let rows = render_tab(&model, super::ContextId::Submodules, mode);
        assert!(
            rows.iter()
                .any(|row| row.contains("shared-lib vendor/shared-lib"))
        );
    }
}

#[test]
fn format_key_hint_formats_modifiers_and_preserves_plain() {
    assert_eq!(format_key_hint("<c-s>"), "ctrl+s");
    assert_eq!(format_key_hint("<C-s>"), "ctrl+s");
    assert_eq!(format_key_hint("<ctrl-s>"), "ctrl+s");
    assert_eq!(format_key_hint("<c+s>"), "ctrl+s");
    assert_eq!(format_key_hint("Ctrl+S"), "ctrl+s");
    assert_eq!(format_key_hint("ctrl-s"), "ctrl+s");
    assert_eq!(format_key_hint("<c-l>"), "ctrl+l");
    assert_eq!(format_key_hint("<c-f>"), "ctrl+f");
    assert_eq!(format_key_hint("<a-h>"), "alt+h");
    assert_eq!(format_key_hint("<alt-h>"), "alt+h");
    assert_eq!(format_key_hint("<s-f>"), "shift+f");
    assert_eq!(format_key_hint("<shift-f>"), "shift+f");
    assert_eq!(format_key_hint("<shift-tab>"), "shift+tab");
    assert_eq!(format_key_hint("<space>"), "space");
    assert_eq!(format_key_hint("<Space>"), "space");
    assert_eq!(format_key_hint("<Enter>"), "enter");
    assert_eq!(format_key_hint("<escape>"), "esc");
    assert_eq!(format_key_hint("f"), "f");
    assert_eq!(format_key_hint("F"), "F");
    assert_eq!(format_key_hint("ctrl+s"), "ctrl+s");
    assert_eq!(format_key_hint(""), "");
}

#[test]
fn status_bar_reflects_dynamic_open_log_menu_keybinding() {
    use crate::config::KeybindingConfig;
    use crate::gui::context::{ContextId, ContextManager};
    use crate::model::Model;
    use crate::pager::side_by_side::DiffViewState;

    let backend = TestBackend::new(140, 1);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = Theme::default();
    let mut ctx_mgr = ContextManager::new();
    ctx_mgr.set_active(ContextId::Commits);
    let diff_view = DiffViewState::new();
    let model = Model::default();

    let mut keybindings = KeybindingConfig::default();
    keybindings.commits.open_log_menu = "<c-s>".into();

    terminal
        .draw(|f| {
            super::render_status_bar(
                f,
                Rect::new(0, 0, 140, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                false,
                false,
                &keybindings,
                false,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let content: String = (0..140)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    assert!(
        content.contains("ctrl+s filter branch"),
        "Expected 'ctrl+s filter branch' in status bar, got: {}",
        content
    );

    // Remap to 'f'
    keybindings.commits.open_log_menu = "f".into();
    terminal
        .draw(|f| {
            super::render_status_bar(
                f,
                Rect::new(0, 0, 140, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                false,
                false,
                &keybindings,
                false,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let content_f: String = (0..140)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    assert!(
        content_f.contains("f filter branch"),
        "Expected 'f filter branch' in status bar, got: {}",
        content_f
    );
}

#[test]
fn status_bar_reflects_toggle_working_tree_and_head() {
    use crate::config::KeybindingConfig;
    use crate::gui::context::{ContextId, ContextManager};
    use crate::model::Model;
    use crate::pager::side_by_side::DiffViewState;

    let backend = TestBackend::new(160, 1);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = Theme::default();
    let mut ctx_mgr = ContextManager::new();
    let diff_view = DiffViewState::new();
    let model = Model::default();
    let keybindings = KeybindingConfig::default();

    // 1. Files context -> shows "ctrl+g head"
    ctx_mgr.set_active(ContextId::Files);
    terminal
        .draw(|f| {
            super::render_status_bar(
                f,
                Rect::new(0, 0, 160, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                false,
                false,
                &keybindings,
                false,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let content: String = (0..160)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    assert!(
        content.contains("ctrl+g head"),
        "Expected 'ctrl+g head' in status bar for Files, got: {}",
        content
    );

    // 2. Commits context -> shows "ctrl+g files"
    ctx_mgr.set_active(ContextId::Commits);
    terminal
        .draw(|f| {
            super::render_status_bar(
                f,
                Rect::new(0, 0, 160, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                false,
                false,
                &keybindings,
                false,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let content: String = (0..160)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    assert!(
        content.contains("ctrl+g files"),
        "Expected 'ctrl+g files' in status bar for Commits, got: {}",
        content
    );

    // 3. CommitFiles context -> shows "ctrl+g files"
    ctx_mgr.set_active(ContextId::CommitFiles);
    terminal
        .draw(|f| {
            super::render_status_bar(
                f,
                Rect::new(0, 0, 160, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                false,
                false,
                &keybindings,
                false,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let content: String = (0..160)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    assert!(
        content.contains("ctrl+g files"),
        "Expected 'ctrl+g files' in status bar for CommitFiles, got: {}",
        content
    );

    // 4. Diff-focused mode: Files context -> shows "ctrl+g head"
    ctx_mgr.set_active(ContextId::Files);
    terminal
        .draw(|f| {
            super::render_status_bar(
                f,
                Rect::new(0, 0, 160, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                true,
                false,
                &keybindings,
                false,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let content: String = (0..160)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    assert!(
        content.contains("ctrl+g head"),
        "Expected 'ctrl+g head' in diff-focused status bar for Files, got: {}",
        content
    );

    // 5. Diff-focused mode: Commits context -> shows "ctrl+g files"
    ctx_mgr.set_active(ContextId::Commits);
    terminal
        .draw(|f| {
            super::render_status_bar(
                f,
                Rect::new(0, 0, 160, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                true,
                false,
                &keybindings,
                false,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let content: String = (0..160)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    assert!(
        content.contains("ctrl+g files"),
        "Expected 'ctrl+g files' in diff-focused status bar for Commits, got: {}",
        content
    );

    // 6. Custom keybinding (e.g. <c-t>) -> shows "ctrl+t head" and "ctrl+t files"
    let mut custom_kb = KeybindingConfig::default();
    custom_kb.universal.toggle_working_tree_and_head = "<c-t>".into();
    ctx_mgr.set_active(ContextId::Files);
    terminal
        .draw(|f| {
            super::render_status_bar(
                f,
                Rect::new(0, 0, 160, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                false,
                false,
                &custom_kb,
                false,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let content: String = (0..160)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    assert!(
        content.contains("ctrl+t head"),
        "Expected 'ctrl+t head' with custom keybinding, got: {}",
        content
    );

    // 7. returnToCockpit keybinding (e.g. g) -> shows "g cockpit"
    let mut cockpit_kb = KeybindingConfig::default();
    cockpit_kb.universal.return_to_cockpit = "g".into();
    terminal
        .draw(|f| {
            super::render_status_bar(
                f,
                Rect::new(0, 0, 160, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                false,
                false,
                &cockpit_kb,
                false,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let content: String = (0..160)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    assert!(
        content.contains("g cockpit"),
        "Expected 'g cockpit' with returnToCockpit keybinding, got: {}",
        content
    );
}

#[test]
fn command_log_is_hidden_when_main_panel_is_absent() {
    assert_eq!(command_log_geometry(Rect::default(), 1), None);
}

#[test]
fn command_log_visible_lines_are_clamped_to_short_main_panel() {
    let (rect, visible_lines) =
        command_log_geometry(Rect::new(10, 4, 80, 2), 5).expect("log should fit");

    assert_eq!(visible_lines, 1);
    assert_eq!(rect, Rect::new(40, 4, 50, 3));
}

#[test]
fn long_error_message_popup_renders_in_short_terminal() {
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let message = (0..40)
        .map(|i| {
            format!("hint: divergent branches need reconciliation before pull can continue ({i})")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let popup = PopupState::Message {
        title: "Pull error".to_string(),
        message,
        kind: MessageKind::Error,
    };

    terminal
        .draw(|frame| {
            render_popup(
                frame,
                &popup,
                Rect::new(0, 0, 40, 8),
                0,
                &Theme::default(),
                false,
                false,
            );
        })
        .expect("long popup message should render without panicking");
}

fn sample_menu() -> PopupState {
    PopupState::Menu {
        title: "Copy".to_string(),
        items: vec![
            MenuItem {
                label: "commit hash".to_string(),
                description: String::new(),
                key: Some("c".to_string()),
                action: None,
            },
            MenuItem {
                label: "commit message".to_string(),
                description: String::new(),
                key: Some("m".to_string()),
                action: None,
            },
            MenuItem {
                label: "author name".to_string(),
                description: String::new(),
                key: Some("a".to_string()),
                action: None,
            },
        ],
        selected: 0,
        loading_index: None,
    }
}

#[test]
fn menu_item_at_hits_first_and_second_options() {
    let area = Rect::new(0, 0, 80, 24);
    let popup = sample_menu();
    // height = 3 items + 2 borders = 5; y = (24-5)/2 = 9; list starts at y+1 = 10
    let x = (area
        .width
        .saturating_sub((area.width * 60 / 100).clamp(30, 60)))
        / 2
        + 2;
    assert_eq!(menu_item_at(&popup, area, x, 10), Some(0));
    assert_eq!(menu_item_at(&popup, area, x, 11), Some(1));
    assert_eq!(menu_item_at(&popup, area, x, 12), Some(2));
    assert_eq!(menu_item_at(&popup, area, x, 9), None); // border
    assert_eq!(menu_item_at(&popup, area, x, 13), None); // below
}

#[test]
fn checklist_item_at_skips_search_and_separator() {
    let area = Rect::new(0, 0, 80, 30);
    let popup = PopupState::Checklist {
        title: "Pick".to_string(),
        items: vec![
            ChecklistItem {
                label: "one".to_string(),
                checked: false,
                is_free_entry: false,
            },
            ChecklistItem {
                label: "two".to_string(),
                checked: true,
                is_free_entry: false,
            },
        ],
        selected: 0,
        search_textarea: crate::gui::popup::make_checklist_search_textarea(),
        free_entry_category: None,
        on_confirm: Box::new(|_gui, _ids| Ok(())),
    };
    // height = max(8, 2+6)=8; y=(30-8)/2=11; list_start=y+1+2=14
    let x = (area
        .width
        .saturating_sub((area.width * 60 / 100).clamp(30, 60)))
        / 2
        + 2;
    assert_eq!(checklist_item_at(&popup, area, x, 14), Some(0));
    assert_eq!(checklist_item_at(&popup, area, x, 15), Some(1));
    assert_eq!(checklist_item_at(&popup, area, x, 12), None); // search row
    assert_eq!(checklist_item_at(&popup, area, x, 13), None); // separator
}

#[test]
fn list_panel_renders_scrollbar_when_scrollable() {
    use ratatui::widgets::{Block, Borders, ListItem};
    let backend = TestBackend::new(30, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = Theme::default();
    let items: Vec<ListItem> = (0..20)
        .map(|i| ListItem::new(format!("item {}", i)))
        .collect();
    let rect = Rect::new(0, 0, 30, 8);
    let block = Block::default().borders(Borders::ALL);
    let mut scroll_offset = 0;

    terminal
        .draw(|f| {
            super::render_list_with_range_raw(
                f,
                rect,
                block,
                items,
                0,
                true,
                &theme,
                None,
                &mut scroll_offset,
                true,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let right_x = 29;
    let mut found_thumb = false;
    for y in 0..8 {
        if buf.cell((right_x, y)).map(|c| c.symbol()) == Some("▐") {
            found_thumb = true;
            break;
        }
    }
    assert!(
        found_thumb,
        "Scrollbar thumb '▐' should be rendered on right border when list is scrollable"
    );
}

#[test]
fn list_panel_does_not_render_scrollbar_when_content_fits() {
    use ratatui::widgets::{Block, Borders, ListItem};
    let backend = TestBackend::new(30, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = Theme::default();
    let items: Vec<ListItem> = (0..3)
        .map(|i| ListItem::new(format!("item {}", i)))
        .collect();
    let rect = Rect::new(0, 0, 30, 8);
    let block = Block::default().borders(Borders::ALL);
    let mut scroll_offset = 0;

    terminal
        .draw(|f| {
            super::render_list_with_range_raw(
                f,
                rect,
                block,
                items,
                0,
                true,
                &theme,
                None,
                &mut scroll_offset,
                true,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    for y in 0..8 {
        for x in 0..30 {
            assert_ne!(
                buf.cell((x, y)).map(|c| c.symbol()),
                Some("▐"),
                "Scrollbar should NOT be rendered when all content fits"
            );
        }
    }
}

#[test]
fn format_key_hint_normalizes_standard_and_custom_bindings() {
    use super::format_key_hint;

    assert_eq!(format_key_hint("<c-s>"), "ctrl+s");
    assert_eq!(format_key_hint("<c-l>"), "ctrl+l");
    assert_eq!(format_key_hint("<c-L>"), "ctrl+l");
    assert_eq!(format_key_hint("ctrl+l"), "ctrl+l");
    assert_eq!(format_key_hint("<space>"), "space");
    assert_eq!(format_key_hint("<esc>"), "esc");
    assert_eq!(format_key_hint("<enter>"), "enter");
    assert_eq!(format_key_hint("c-s"), "ctrl+s");
    assert_eq!(format_key_hint("alt-x"), "alt+x");
    assert_eq!(format_key_hint("shift-tab"), "shift+tab");
}

#[test]
fn render_status_bar_displays_filter_branch_and_esc_reset() {
    use super::{DiffViewState, render_status_bar};
    use crate::config::KeybindingConfig;
    use crate::gui::context::{ContextId, ContextManager};
    use crate::model::Model;

    let backend = TestBackend::new(120, 2);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let model = Model::default();
    let keybindings = KeybindingConfig::default();
    let mut ctx_mgr = ContextManager::new();
    ctx_mgr.set_active(ContextId::Commits);
    let diff_view = DiffViewState::new();
    let theme = Theme::default();

    // With active filters
    terminal
        .draw(|frame| {
            render_status_bar(
                frame,
                Rect::new(0, 0, 120, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                false,
                false,
                &keybindings,
                true,
            );
        })
        .expect("should render status bar with active filters");

    let buffer = terminal.backend().buffer();
    let content: String = buffer.content().iter().map(|c| c.symbol()).collect();
    assert!(
        content.contains("reset filter"),
        "expected 'reset filter' in status bar: {}",
        content
    );
    assert!(
        content.contains("filter branch"),
        "expected 'filter branch' in status bar: {}",
        content
    );

    // Without active filters
    terminal
        .draw(|frame| {
            render_status_bar(
                frame,
                Rect::new(0, 0, 120, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                false,
                false,
                &keybindings,
                false,
            );
        })
        .expect("should render status bar without active filters");

    let buffer = terminal.backend().buffer();
    let content: String = buffer.content().iter().map(|c| c.symbol()).collect();
    assert!(
        !content.contains("reset filter"),
        "did not expect 'reset filter' when no filters active: {}",
        content
    );
    assert!(
        content.contains("filter branch"),
        "expected 'filter branch' in status bar: {}",
        content
    );
}

#[test]
fn status_bar_snapshot_per_context_caps_at_seven_pairs_with_separators_and_dynamic_keys() {
    use super::{DiffViewState, MAX_STATUS_BAR_HINTS, render_status_bar};
    use crate::config::KeybindingConfig;
    use crate::gui::context::{ContextId, ContextManager};
    use crate::model::Model;

    let backend = TestBackend::new(160, 1);
    let mut terminal = Terminal::new(backend).expect("should create test terminal");
    let theme = crate::config::Theme::cockpit();
    let model = Model::default();
    let mut diff_view = DiffViewState::new();
    diff_view.load_from_diff_output("src/main.rs", "@@ -1,1 +1,1 @@\n-old\n+new\n");

    let mut keybindings = KeybindingConfig::default();
    keybindings.files.commit_changes = "C".to_string();
    keybindings.universal.quick_ai_note = "<c-a>".to_string();

    let contexts = [
        ContextId::Files,
        ContextId::Commits,
        ContextId::Branches,
        ContextId::Stash,
        ContextId::Tags,
        ContextId::Worktrees,
        ContextId::Status,
    ];

    for ctx in contexts {
        let mut ctx_mgr = ContextManager::new();
        ctx_mgr.set_active(ctx);
        terminal
            .draw(|frame| {
                render_status_bar(
                    frame,
                    Rect::new(0, 0, 160, 1),
                    &ctx_mgr,
                    &diff_view,
                    &theme,
                    &model,
                    false,
                    false,
                    &keybindings,
                    false,
                );
            })
            .expect("should render status bar");

        let buffer = terminal.backend().buffer();
        let content: String = buffer.content().iter().map(|c| c.symbol()).collect();
        let trimmed = content.trim_end();
        let segments: Vec<&str> = trimmed.split(" │ ").collect();
        assert!(
            segments.len() <= MAX_STATUS_BAR_HINTS,
            "context {:?} exceeded {} pairs ({}): {}",
            ctx,
            MAX_STATUS_BAR_HINTS,
            segments.len(),
            trimmed
        );
        assert!(
            !trimmed.contains("[/]") && !trimmed.contains("{/}") && !trimmed.contains("<c-n>"),
            "context {:?} has legacy ambiguous hint label: {}",
            ctx,
            trimmed
        );

        if ctx == ContextId::Files {
            assert!(
                trimmed.contains("C commit"),
                "Files status bar should reflect remapped commit_changes key: {}",
                trimmed
            );
            assert!(
                trimmed.contains("ctrl+a agent note"),
                "Files status bar should reflect remapped quick_ai_note key: {}",
                trimmed
            );
        }
    }

    // Also verify diff-focused Files uses `[ ] side view` and `{ } cycle hunks`
    let mut ctx_mgr = ContextManager::new();
    ctx_mgr.set_active(ContextId::Files);
    terminal
        .draw(|frame| {
            render_status_bar(
                frame,
                Rect::new(0, 0, 160, 1),
                &ctx_mgr,
                &diff_view,
                &theme,
                &model,
                true,
                false,
                &keybindings,
                false,
            );
        })
        .expect("should render diff-focused status bar");
    let buffer = terminal.backend().buffer();
    let content: String = buffer.content().iter().map(|c| c.symbol()).collect();
    let trimmed = content.trim_end();
    assert!(trimmed.contains("{ } cycle hunks"), "got: {}", trimmed);
    assert!(trimmed.contains("[ ] side view"), "got: {}", trimmed);
    assert!(trimmed.contains("ctrl+a agent note"), "got: {}", trimmed);
    assert!(trimmed.split(" │ ").count() <= MAX_STATUS_BAR_HINTS);
}

#[test]
fn cockpit_theme_paints_cyan_selection_gutter_at_x_equals_1() {
    use super::render_list_with_range_raw;
    use ratatui::text::Line;
    use ratatui::widgets::{Block, Borders, ListItem};

    let backend = TestBackend::new(40, 6);
    let mut terminal = Terminal::new(backend).expect("should create test terminal");
    let theme = crate::config::Theme::cockpit();
    let items = vec![
        ListItem::new(Line::from("first item")),
        ListItem::new(Line::from("second item")),
    ];
    let mut scroll_offset = 0;

    terminal
        .draw(|frame| {
            let block = Block::default().borders(Borders::ALL).title("Files");
            render_list_with_range_raw(
                frame,
                Rect::new(0, 0, 40, 6),
                block,
                items.clone(),
                1,
                true,
                &theme,
                None,
                &mut scroll_offset,
                true,
            );
        })
        .expect("should render list with cockpit gutter");

    let buf = terminal.backend().buffer();
    // Row y=1 is item 0 (unselected -> space in X=1 gutter, shifted text starts at X=2)
    assert_eq!(buf[(1, 1)].symbol(), " ");
    assert_eq!(buf[(2, 1)].symbol(), "f");
    // Row y=2 is item 1 (selected -> cyan '│' at X=1, shifted text starts at X=2)
    assert_eq!(buf[(1, 2)].symbol(), "│");
    assert_eq!(buf[(1, 2)].fg, theme.selection_gutter.unwrap());
    assert_eq!(buf[(2, 2)].symbol(), "s");
}

/// Golden snapshot of the status bar for every context, default keybindings,
/// in both list focus and diff focus. Any wording, order, separator or key
/// label change shows up as a diff here. Regenerate deliberately with
/// `UPDATE_SNAPSHOTS=1 cargo test status_bar_golden_snapshot`.
#[test]
fn status_bar_golden_snapshot() {
    use super::{DiffViewState, render_status_bar};
    use crate::config::KeybindingConfig;
    use crate::gui::context::{ContextId, ContextManager};
    use crate::model::Model;

    const WIDTH: u16 = 200;
    let backend = TestBackend::new(WIDTH, 1);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = crate::config::Theme::default();
    let model = Model::default();
    let keybindings = KeybindingConfig::default();
    let mut diff_view = DiffViewState::new();
    diff_view.load_from_diff_output("src/main.rs", "@@ -1,1 +1,1 @@\n-old\n+new\n");

    let contexts = [
        ContextId::Status,
        ContextId::Files,
        ContextId::Worktrees,
        ContextId::Submodules,
        ContextId::Branches,
        ContextId::Remotes,
        ContextId::Tags,
        ContextId::Commits,
        ContextId::Reflog,
        ContextId::Stash,
        ContextId::CommitFiles,
        ContextId::StashFiles,
        ContextId::BranchCommits,
        ContextId::BranchCommitFiles,
        ContextId::RemoteBranches,
    ];

    let mut snapshot = String::new();
    for diff_focused in [false, true] {
        for ctx in contexts {
            let mut ctx_mgr = ContextManager::new();
            ctx_mgr.set_active(ctx);
            terminal
                .draw(|frame| {
                    render_status_bar(
                        frame,
                        Rect::new(0, 0, WIDTH, 1),
                        &ctx_mgr,
                        &diff_view,
                        &theme,
                        &model,
                        diff_focused,
                        false,
                        &keybindings,
                        false,
                    );
                })
                .expect("render status bar");
            let line: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect();
            let focus = if diff_focused { "diff" } else { "list" };
            snapshot.push_str(&format!("{ctx:?}/{focus}: {}\n", line.trim_end()));
        }
    }

    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/gui/views/snapshots/status_bar.txt"
    );
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, &snapshot).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(path)
        .expect("missing snapshot: run `UPDATE_SNAPSHOTS=1 cargo test status_bar_golden_snapshot`");
    if expected != snapshot {
        let diff: Vec<String> = expected
            .lines()
            .zip(snapshot.lines())
            .filter(|(e, a)| e != a)
            .map(|(e, a)| format!("- {e}\n+ {a}"))
            .collect();
        panic!(
            "status bar snapshot changed (UPDATE_SNAPSHOTS=1 to accept):\n{}",
            diff.join("\n")
        );
    }
}

#[test]
fn status_bar_snapshot_never_repeats_a_hint() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/gui/views/snapshots/status_bar.txt"
    );
    let snapshot = std::fs::read_to_string(path).expect("snapshot");
    for line in snapshot.lines() {
        let (_, bar) = line.split_once(": ").unwrap();
        let pairs: Vec<&str> = bar.trim().split(" │ ").collect();
        let mut seen = std::collections::HashSet::new();
        for p in &pairs {
            assert!(seen.insert(*p), "repeated hint {p:?} in: {line}");
        }
    }
}
