use super::*;

#[test]
fn plain_character_shortcuts_reject_extra_modifiers() {
    assert!(plain_char_key(
        KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT),
        'I'
    ));
    assert!(!plain_char_key(
        KeyEvent::new(
            KeyCode::Char('I'),
            KeyModifiers::SHIFT | KeyModifiers::SUPER
        ),
        'I'
    ));
    // Global reset picker (G) uses the same plain-char matching as I.
    assert!(plain_char_key(
        KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
        'G'
    ));
    assert!(!plain_char_key(
        KeyEvent::new(
            KeyCode::Char('G'),
            KeyModifiers::SHIFT | KeyModifiers::CONTROL
        ),
        'G'
    ));
    // Lowercase g must not match the global reset picker binding.
    assert!(!plain_char_key(
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        'G'
    ));
}

#[test]
fn toggle_working_tree_and_head_toggles_between_files_and_head() {
    let repo = TempRepo::new("toggle-files-head");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();

    // Starts in Files by default
    assert_eq!(gui.context_mgr.active(), ContextId::Files);

    // Toggle to Commits (HEAD at index 0)
    gui.toggle_working_tree_and_head();
    assert_eq!(gui.context_mgr.active(), ContextId::Commits);
    assert_eq!(gui.context_mgr.selected(ContextId::Commits), 0);
    assert_eq!(gui.context_mgr.scroll_offset(ContextId::Commits), 0);
    assert!(gui.needs_diff_refresh);

    // Move commit selection, then toggle back to Files
    gui.context_mgr.set_selection_for(ContextId::Commits, 3);
    gui.toggle_working_tree_and_head();
    assert_eq!(gui.context_mgr.active(), ContextId::Files);
    assert!(gui.needs_diff_refresh);

    // Toggle from CommitFiles subcontext back to Files
    gui.context_mgr.set_active(ContextId::CommitFiles);
    gui.toggle_working_tree_and_head();
    assert_eq!(gui.context_mgr.active(), ContextId::Files);

    // Toggle from outside context (e.g. Branches) jumps to Commits at HEAD
    gui.context_mgr.set_active(ContextId::Branches);
    gui.toggle_working_tree_and_head();
    assert_eq!(gui.context_mgr.active(), ContextId::Commits);
    assert_eq!(gui.context_mgr.selected(ContextId::Commits), 0);
}

#[test]
fn start_in_commits_initializes_context_to_commits_head() {
    let repo = TempRepo::new("start-in-commits");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let gui = Gui::new(config, git, false, None, true, None, true).unwrap();

    assert_eq!(gui.context_mgr.active(), ContextId::Commits);
    assert_eq!(gui.context_mgr.selected(ContextId::Commits), 0);
}

#[test]
fn ctrl_g_key_event_triggers_toggle() {
    let repo = TempRepo::new("ctrl-g-toggle");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();

    assert_eq!(gui.context_mgr.active(), ContextId::Files);

    // Send Ctrl+g
    let ctrl_g = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL);
    gui.handle_key(ctrl_g).unwrap();
    assert_eq!(gui.context_mgr.active(), ContextId::Commits);

    // Send Ctrl+g again to toggle back
    gui.handle_key(ctrl_g).unwrap();
    assert_eq!(gui.context_mgr.active(), ContextId::Files);

    // Send Ctrl+g while diff is focused
    gui.diff_focused = true;
    gui.handle_key(ctrl_g).unwrap();
    assert_eq!(gui.context_mgr.active(), ContextId::Commits);
    assert!(gui.diff_focused);

    // Send Ctrl+G (uppercase) while diff is focused to toggle back to Files
    let ctrl_g_upper = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::CONTROL);
    gui.handle_key(ctrl_g_upper).unwrap();
    assert_eq!(gui.context_mgr.active(), ContextId::Files);
    assert!(gui.diff_focused);

    // Test custom keybinding (e.g. <c-t>)
    let mut custom_config = crate::config::AppConfig::default();
    custom_config
        .user_config
        .keybinding
        .universal
        .toggle_working_tree_and_head = "<c-t>".into();
    let git2 = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui2 = Gui::new(custom_config, git2, false, None, true, None, false).unwrap();
    let ctrl_t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
    gui2.handle_key(ctrl_t).unwrap();
    assert_eq!(gui2.context_mgr.active(), ContextId::Commits);
}

#[test]
fn ctrl_n_key_event_opens_quick_ai_note_prompt() {
    let repo = TempRepo::new("ctrl-n-quick-ai-note");
    std::fs::write(repo.path().join("test.txt"), "hello world\n").unwrap();
    let mut config = crate::config::AppConfig::default();
    config.user_config.ai_notes.enabled = true;
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();

    // Populate model files
    {
        let mut model = gui.model.lock().unwrap();
        model.files.push(crate::model::file::File {
            name: "test.txt".into(),
            display_name: "test.txt".into(),
            status: crate::model::file::FileStatus::Modified,
            has_staged_changes: false,
            has_unstaged_changes: true,
            tracked: true,
            added: false,
            deleted: false,
            has_merge_conflicts: false,
            short_status: " M".into(),
            hunk_count: 1,
            additions: 1,
            deletions: 0,
        });
    }

    // Send Ctrl+n in Files view
    let ctrl_n = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
    gui.handle_key(ctrl_n).unwrap();
    assert!(matches!(gui.popup, PopupState::Input { .. }));

    // Send Esc to cancel popup
    gui.popup = PopupState::None;

    // In Diff view
    gui.diff_focused = true;
    gui.diff_view.filename = "test.txt".to_string();
    let ctrl_n_upper = KeyEvent::new(KeyCode::Char('N'), KeyModifiers::CONTROL);
    gui.handle_key(ctrl_n_upper).unwrap();
    assert!(matches!(gui.popup, PopupState::Input { .. }));
}

#[test]
fn return_to_cockpit_key_triggers_quit_with_cockpit_reason() {
    let _env_guard = COCKPIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let repo = TempRepo::new("return-to-cockpit");
    let mut config = crate::config::AppConfig::default();
    config.user_config.keybinding.universal.return_to_cockpit = "g".into();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();

    assert!(!gui.should_quit);
    assert_eq!(gui.quit_reason, QuitReason::Normal);

    // Press 'g'
    let g_key = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
    gui.handle_key(g_key).unwrap();

    assert!(gui.should_quit);
    assert_eq!(gui.quit_reason, QuitReason::ReturnToCockpit);
    assert_eq!(gui.quit_reason.exit_code(), 10);
}

#[test]
fn return_to_cockpit_via_env_var() {
    let _env_guard = COCKPIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let repo = TempRepo::new("return-to-cockpit-env");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, true).unwrap();

    unsafe { std::env::set_var("LAZYGITRS_COCKPIT", "1") };

    assert!(!gui.should_quit);
    assert_eq!(gui.context_mgr.active(), ContextId::Commits);

    // Pressing 'g' when return_to_cockpit is empty does NOT quit to cockpit
    gui.diff_focused = true;
    let g_key = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE);
    gui.handle_key(g_key).unwrap();
    assert!(!gui.should_quit);

    // Pressing Esc at root Commits returns to cockpit
    gui.diff_focused = false;
    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    gui.handle_key(esc_key).unwrap();

    assert!(gui.should_quit);
    assert_eq!(gui.quit_reason, QuitReason::ReturnToCockpit);
    assert_eq!(gui.quit_reason.exit_code(), 10);

    unsafe { std::env::remove_var("LAZYGITRS_COCKPIT") };
}

#[test]
fn esc_at_root_files_returns_to_cockpit_when_cockpit_env_set() {
    let _env_guard = COCKPIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let repo = TempRepo::new("esc-cockpit-env");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();

    unsafe { std::env::set_var("LAZYGITRS_COCKPIT", "1") };

    assert!(!gui.should_quit);

    // Press Esc while at root files list
    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    gui.handle_key(esc_key).unwrap();

    assert!(gui.should_quit);
    assert_eq!(gui.quit_reason, QuitReason::ReturnToCockpit);
    assert_eq!(gui.quit_reason.exit_code(), 10);

    unsafe { std::env::remove_var("LAZYGITRS_COCKPIT") };
}

#[test]
fn esc_at_root_files_quits_normally_when_no_cockpit_env() {
    let _env_guard = COCKPIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("LAZYGITRS_COCKPIT") };
    let repo = TempRepo::new("esc-normal-quit");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();

    assert!(!gui.should_quit);

    // Press Esc while at root files list without LAZYGITRS_COCKPIT
    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    gui.handle_key(esc_key).unwrap();

    assert!(gui.should_quit);
    assert_eq!(gui.quit_reason, QuitReason::Normal);
    assert_eq!(gui.quit_reason.exit_code(), 0);
}

#[test]
fn esc_in_diff_unwinds_to_files_first_then_to_cockpit() {
    let _env_guard = COCKPIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let repo = TempRepo::new("esc-diff-unwind");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();

    unsafe { std::env::set_var("LAZYGITRS_COCKPIT", "1") };

    // Start in diff focused
    gui.diff_focused = true;
    gui.diff_view.selected_revert_hunk = Some(0);

    // 1st Esc: should unwind diff view to files panel without quitting
    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    gui.handle_key(esc_key).unwrap();

    assert!(!gui.should_quit);
    assert!(!gui.diff_focused);
    assert_eq!(gui.diff_view.selected_revert_hunk, None);

    // 2nd Esc: should exit lazygitrs back to Cockpit (exit code 10)
    gui.handle_key(esc_key).unwrap();

    assert!(gui.should_quit);
    assert_eq!(gui.quit_reason, QuitReason::ReturnToCockpit);
    assert_eq!(gui.quit_reason.exit_code(), 10);

    unsafe { std::env::remove_var("LAZYGITRS_COCKPIT") };
}

#[test]
fn space_key_in_diff_focused_advances_to_next_hunk() {
    let repo = TempRepo::new("space-diff-cycle");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();

    gui.diff_focused = true;
    gui.diff_view.hunk_starts = vec![5, 15, 25];
    gui.diff_view.selected_revert_hunk = Some(0);

    let space_key = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
    gui.handle_key(space_key).unwrap();

    assert_eq!(gui.diff_view.selected_revert_hunk, Some(1));

    gui.handle_key(space_key).unwrap();
    assert_eq!(gui.diff_view.selected_revert_hunk, Some(2));

    gui.handle_key(space_key).unwrap();
    // Last hunk: stays put instead of wrapping back to the top.
    assert_eq!(gui.diff_view.selected_revert_hunk, Some(2));
}

#[test]
fn space_key_on_staged_hunk_keeps_selection() {
    let repo = TempRepo::new("space-diff-unstage");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();

    gui.diff_focused = true;
    gui.diff_view.hunk_starts = vec![5, 15, 25];
    gui.diff_view.hunk_staged = vec![false, true, false];
    gui.diff_view.selected_revert_hunk = Some(1);

    let space_key = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
    gui.handle_key(space_key).unwrap();

    // Unstaging is a correction: the cursor stays on the same hunk.
    assert_eq!(gui.diff_view.selected_revert_hunk, Some(1));
}

#[test]
fn normal_quit_sets_normal_quit_reason() {
    let repo = TempRepo::new("normal-quit");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();

    let q_key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
    gui.handle_key(q_key).unwrap();

    assert!(gui.should_quit);
    assert_eq!(gui.quit_reason, QuitReason::Normal);
    assert_eq!(gui.quit_reason.exit_code(), 0);
}

#[test]
fn ctrl_w_prefix_resizes_side_panel_without_alt() {
    let repo = TempRepo::new("ctrl-w-resize");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, false, None, false).unwrap();
    gui.layout.width = 160;
    gui.layout.height = 40;
    let initial = gui.layout.side_panel_ratio;

    // Ctrl+W l expands side panel by 0.05
    gui.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL))
        .unwrap();
    gui.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE))
        .unwrap();
    assert!((gui.layout.side_panel_ratio - (initial + 0.05)).abs() < 1e-6);

    // Ctrl+W h shrinks side panel by 0.05
    gui.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL))
        .unwrap();
    gui.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
        .unwrap();
    assert!((gui.layout.side_panel_ratio - initial).abs() < 1e-6);

    // Ctrl+W k maximizes side panel in landscape
    gui.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL))
        .unwrap();
    gui.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE))
        .unwrap();
    assert!((gui.layout.side_panel_ratio - 1.0).abs() < 1e-6);

    // Ctrl+W j maximizes main panel in landscape
    gui.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL))
        .unwrap();
    gui.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
        .unwrap();
    assert!(gui.layout.side_panel_ratio.abs() < 1e-6);

    // Ctrl+W = resets side panel width
    gui.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL))
        .unwrap();
    gui.handle_key(KeyEvent::new(KeyCode::Char('='), KeyModifiers::NONE))
        .unwrap();
    assert!(
        (gui.layout.side_panel_ratio - gui.config.user_config.gui.side_panel_width).abs() < 1e-6
    );
}

#[test]
fn fullscreen_single_esc_at_root_does_not_quit_double_tap_quits() {
    let _env_guard = COCKPIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("LAZYGITRS_COCKPIT") };
    let repo = TempRepo::new("fullscreen-esc-double-tap");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    // is_popup = false, outer_frame = false -> full-screen mode
    let mut gui = Gui::new(config, git, false, None, false, None, false).unwrap();
    gui.outer_frame = false;

    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);

    // First Esc in full-screen mode must NOT quit
    gui.handle_key(esc_key).unwrap();
    assert!(!gui.should_quit);
    assert!(gui.root_esc_armed_at.is_some());

    // If another key is pressed in between, the double-tap window disarms
    gui.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
        .unwrap();
    assert!(gui.root_esc_armed_at.is_none());

    // Single Esc again -> still does not quit
    gui.handle_key(esc_key).unwrap();
    assert!(!gui.should_quit);

    // Second Esc within 400ms -> quits cleanly
    gui.handle_key(esc_key).unwrap();
    assert!(gui.should_quit);
    assert_eq!(gui.quit_reason, QuitReason::Normal);
}

#[test]
fn fullscreen_esc_unwinding_diff_does_not_arm_root_quit() {
    let _env_guard = COCKPIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("LAZYGITRS_COCKPIT") };
    let repo = TempRepo::new("fullscreen-esc-unwind-diff");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(config, git, false, None, false, None, false).unwrap();
    gui.outer_frame = false;
    gui.diff_focused = true;

    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);

    // 1st Esc unwinds diff focus
    gui.handle_key(esc_key).unwrap();
    assert!(!gui.diff_focused);
    assert!(!gui.should_quit);
    assert!(gui.root_esc_armed_at.is_none());

    // 2nd Esc at root Files only arms root quit (does NOT immediately quit after unwinding)
    gui.handle_key(esc_key).unwrap();
    assert!(!gui.should_quit);
    assert!(gui.root_esc_armed_at.is_some());
}

#[test]
fn esc_at_root_files_clears_active_search_before_quitting() {
    let _env_guard = COCKPIT_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("LAZYGITRS_COCKPIT") };
    let repo = TempRepo::new("esc-clears-search-first");
    let config = crate::config::AppConfig::default();
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    // Even in popup mode, Esc should clear an active search query before quitting
    let mut gui = Gui::new(config, git, false, None, true, None, false).unwrap();
    gui.search_query = "main".to_string();

    let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    gui.handle_key(esc_key).unwrap();
    assert!(!gui.should_quit);
    assert!(gui.search_query.is_empty());

    // Now that search is cleared, next Esc in popup mode quits
    gui.handle_key(esc_key).unwrap();
    assert!(gui.should_quit);
}
