//! Ordered key dispatch tables for the normal (non-modal) UI layer.
//!
//! `Gui::handle_key` resolves a key in layers:
//!
//! 1. Modal layers in code (AI-cancel, popup, search input, rebase mode,
//!    compare mode, panel-resize prefix, inline edit).
//! 2. [`PRE_DIFF_FOCUS`]: bindings that also apply while the diff panel is
//!    focused.
//! 3. Diff-focused layer (`handle_diff_focused_key`) when the diff has focus.
//! 4. [`NORMAL`]: universal bindings, in table order.
//! 5. The active context's controller (`handle_context_key`).
//! 6. Custom commands.
//!
//! **Precedence is the position in the table.** The first entry whose
//! `matches` returns true runs; if its handler returns `Ok(false)` (state
//! precondition not met), dispatch continues with the next entry and, past
//! the table, to the context controller.
//!
//! Entries tagged [`Fires::Always`] consume every key they match, so any
//! context binding on the same key is dead. The `shadowing` tests below
//! enforce that no default context binding is shadowed that way.

use super::*;
use crate::config::KeybindingConfig;
use crate::config::keybindings::parse_key;
use serde::Serialize;

/// Whether an entry consumes every key it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fires {
    /// Matching always consumes the key: later layers can never see it.
    Always,
    /// The handler checks Gui state and may fall through (`Ok(false)`).
    Conditionally,
}

pub(crate) struct KeymapEntry {
    /// Stable identifier; the config path when the key is configurable.
    pub name: &'static str,
    /// Pure key test (no Gui state), shared by dispatch and the tests.
    pub matches: fn(KeyEvent, &KeybindingConfig) -> bool,
    pub fires: Fires,
    /// Returns `Ok(true)` when the key was consumed.
    pub run: fn(&mut Gui, KeyEvent) -> Result<bool>,
}

/// Run the first matching entry that consumes `key`.
pub(crate) fn dispatch(gui: &mut Gui, key: KeyEvent, table: &[KeymapEntry]) -> Result<bool> {
    for entry in table {
        if (entry.matches)(key, &gui.config.user_config.keybinding) && (entry.run)(gui, key)? {
            return Ok(true);
        }
    }
    Ok(false)
}

const fn always(
    name: &'static str,
    matches: fn(KeyEvent, &KeybindingConfig) -> bool,
    run: fn(&mut Gui, KeyEvent) -> Result<bool>,
) -> KeymapEntry {
    KeymapEntry {
        name,
        matches,
        fires: Fires::Always,
        run,
    }
}

const fn when(
    name: &'static str,
    matches: fn(KeyEvent, &KeybindingConfig) -> bool,
    run: fn(&mut Gui, KeyEvent) -> Result<bool>,
) -> KeymapEntry {
    KeymapEntry {
        name,
        matches,
        fires: Fires::Conditionally,
        run,
    }
}

fn is_diff_list_context(ctx: ContextId) -> bool {
    matches!(
        ctx,
        ContextId::Files
            | ContextId::CommitFiles
            | ContextId::StashFiles
            | ContextId::BranchCommitFiles
    )
}

/// Bindings that run before the diff-focused layer, so they also apply while
/// the diff panel has focus.
pub(crate) const PRE_DIFF_FOCUS: &[KeymapEntry] = &[
    always(
        "universal.customCommandPrompt",
        |k, kb| matches_key(k, &kb.universal.custom_command_prompt),
        |gui, _| controller::custom_commands::open_custom_command_prompt(gui).map(|()| true),
    ),
    always(
        "universal.toggleDiffViewLayout",
        |k, kb| matches_key(k, &kb.universal.toggle_diff_view_layout),
        |gui, _| {
            gui.diff_view.toggle_view_layout();
            gui.persist_diff_view_layout();
            Ok(true)
        },
    ),
    // Ctrl-F stays in the same Files/CommitFiles context: grep dialog over
    // hunk contents (Enter jumps to the file in the current list).
    when(
        "diffGrep",
        |k, _| controller::diff_grep::is_diff_grep_key(k),
        |gui, _| {
            if gui.diff_view.search_active || !is_diff_list_context(gui.context_mgr.active()) {
                return Ok(false);
            }
            controller::diff_grep::open_diff_grep_picker(gui).map(|()| true)
        },
    ),
];

/// Universal bindings for the normal UI, checked before the context
/// controller. Order is precedence.
pub(crate) const NORMAL: &[KeymapEntry] = &[
    // Esc / next / prev match dismiss or walk search results before anything
    // else (including the root-Esc quit guard).
    when(
        "search.dismiss",
        |k, _| k.code == KeyCode::Esc,
        |gui, _| {
            if gui.search_query.is_empty() {
                return Ok(false);
            }
            gui.search_query.clear();
            gui.search_matches.clear();
            gui.search_match_idx = 0;
            gui.root_esc_armed_at = None;
            Ok(true)
        },
    ),
    when(
        "universal.nextMatch",
        |k, kb| matches_key(k, &kb.universal.next_match),
        |gui, _| {
            if gui.search_query.is_empty() {
                return Ok(false);
            }
            gui.goto_next_search_match();
            Ok(true)
        },
    ),
    when(
        "universal.prevMatch",
        |k, kb| matches_key(k, &kb.universal.prev_match),
        |gui, _| {
            if gui.search_query.is_empty() {
                return Ok(false);
            }
            gui.goto_prev_search_match();
            Ok(true)
        },
    ),
    when(
        "universal.returnToCockpit",
        |k, kb| {
            matches_key(k, &kb.universal.return_key)
                || (!kb.universal.return_to_cockpit.is_empty()
                    && matches_key(k, &kb.universal.return_to_cockpit))
        },
        |gui, key| {
            if !gui.is_return_to_cockpit_key(key) {
                return Ok(false);
            }
            gui.quit_reason = QuitReason::ReturnToCockpit;
            gui.should_quit = true;
            Ok(true)
        },
    ),
    always(
        "universal.quit",
        |k, kb| matches_key(k, &kb.universal.quit) || matches_key(k, &kb.universal.quit_alt1),
        |gui, _| {
            gui.quit_reason = QuitReason::Normal;
            gui.should_quit = true;
            Ok(true)
        },
    ),
    // Universal return_key (Esc) at the root files list:
    // - In popup / outer_frame mode, a single Esc dismisses immediately.
    // - In full-screen mode, requires a double-tap within 400ms so unwinding
    //   from sub-views or tapping Esc once never kills the session.
    when(
        "universal.return (root quit)",
        |k, kb| matches_key(k, &kb.universal.return_key),
        |gui, _| {
            if !matches!(gui.context_mgr.active(), ContextId::Files) {
                return Ok(false);
            }
            const ROOT_ESC_DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(400);
            if gui.is_popup
                || gui.outer_frame
                || gui
                    .root_esc_armed_at
                    .is_some_and(|t| t.elapsed() <= ROOT_ESC_DOUBLE_TAP_WINDOW)
            {
                gui.quit_reason = QuitReason::Normal;
                gui.should_quit = true;
            } else {
                gui.root_esc_armed_at = Some(Instant::now());
            }
            Ok(true)
        },
    ),
    always(
        "universal.toggleWorkingTreeAndHead",
        |k, kb| matches_key(k, &kb.universal.toggle_working_tree_and_head),
        |gui, _| {
            gui.toggle_working_tree_and_head();
            Ok(true)
        },
    ),
    always(
        "universal.quickAiNote",
        |k, kb| matches_key(k, &kb.universal.quick_ai_note),
        |gui, _| {
            gui.open_quick_ai_note_prompt();
            Ok(true)
        },
    ),
    // 1-5 jump to a window (press again to cycle tabs).
    always(
        "jumpToWindow (1-5)",
        |k, _| k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('1'..='5')),
        |gui, key| {
            let KeyCode::Char(c) = key.code else {
                return Ok(false);
            };
            let Some(window) = c.to_digit(10).and_then(SideWindow::from_number) else {
                return Ok(false);
            };
            gui.jump_to_window_from_key(window);
            Ok(true)
        },
    ),
    always(
        "universal.togglePanel",
        |k, kb| matches_key(k, &kb.universal.toggle_panel),
        |gui, _| {
            gui.exit_sub_contexts();
            gui.context_mgr.next_window();
            Ok(true)
        },
    ),
    always(
        "universal.togglePanelReverse",
        |k, kb| matches_key(k, &kb.universal.toggle_panel_reverse),
        |gui, _| {
            gui.exit_sub_contexts();
            gui.context_mgr.prev_window();
            Ok(true)
        },
    ),
    always(
        "universal.prevBlock",
        |k, kb| {
            matches_key(k, &kb.universal.prev_block) || matches_key(k, &kb.universal.prev_block_alt)
        },
        |gui, _| {
            gui.exit_sub_contexts();
            gui.context_mgr.prev_window();
            Ok(true)
        },
    ),
    always(
        "universal.nextBlock",
        |k, kb| {
            matches_key(k, &kb.universal.next_block) || matches_key(k, &kb.universal.next_block_alt)
        },
        |gui, _| {
            gui.exit_sub_contexts();
            gui.context_mgr.next_window();
            Ok(true)
        },
    ),
    always(
        "universal.prevItem",
        |k, kb| {
            matches_key(k, &kb.universal.prev_item) || matches_key(k, &kb.universal.prev_item_alt)
        },
        |gui, _| {
            gui.move_selection_by(-1);
            Ok(true)
        },
    ),
    always(
        "universal.nextItem",
        |k, kb| {
            matches_key(k, &kb.universal.next_item) || matches_key(k, &kb.universal.next_item_alt)
        },
        |gui, _| {
            gui.move_selection_by(1);
            Ok(true)
        },
    ),
    always(
        "universal.gotoTop",
        |k, kb| matches_key(k, &kb.universal.goto_top),
        |gui, _| {
            gui.context_mgr.set_selection(0);
            Ok(true)
        },
    ),
    always(
        "universal.gotoBottom",
        |k, kb| matches_key(k, &kb.universal.goto_bottom),
        |gui, _| {
            let model = gui
                .model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let len = gui.context_mgr.list_len(&model);
            drop(model);
            if len > 0 {
                gui.context_mgr.set_selection(len - 1);
            }
            Ok(true)
        },
    ),
    // Main panel scroll (J/K or shift+arrows for diff scrolling).
    always(
        "universal.scrollDownMain-alt1",
        |k, kb| matches_key(k, &kb.universal.scroll_down_main_alt1),
        |gui, _| {
            gui.diff_view.scroll_down(1);
            Ok(true)
        },
    ),
    always(
        "universal.scrollUpMain-alt1",
        |k, kb| matches_key(k, &kb.universal.scroll_up_main_alt1),
        |gui, _| {
            gui.diff_view.scroll_up(1);
            Ok(true)
        },
    ),
    always(
        "pageDown",
        |k, _| k.code == KeyCode::PageDown,
        |gui, _| {
            gui.diff_view.scroll_down(20);
            Ok(true)
        },
    ),
    always(
        "pageUp",
        |k, _| k.code == KeyCode::PageUp,
        |gui, _| {
            gui.diff_view.scroll_up(20);
            Ok(true)
        },
    ),
    always(
        "universal.scrollLeft",
        |k, kb| matches_key(k, &kb.universal.scroll_left),
        |gui, _| {
            gui.diff_view.scroll_left(4);
            Ok(true)
        },
    ),
    always(
        "universal.scrollRight",
        |k, kb| matches_key(k, &kb.universal.scroll_right),
        |gui, _| {
            gui.diff_view.scroll_right(4);
            Ok(true)
        },
    ),
    always(
        "prevHunk ({)",
        |k, _| k.code == KeyCode::Char('{'),
        |gui, _| {
            if gui.context_mgr.active() == ContextId::Files {
                gui.diff_view.cycle_prev_revert_hunk();
            } else {
                gui.diff_view.prev_hunk();
            }
            Ok(true)
        },
    ),
    always(
        "nextHunk (})",
        |k, _| k.code == KeyCode::Char('}'),
        |gui, _| {
            if gui.context_mgr.active() == ContextId::Files {
                gui.diff_view.cycle_next_revert_hunk();
            } else {
                gui.diff_view.next_hunk();
            }
            Ok(true)
        },
    ),
    always(
        "universal.refresh",
        |k, kb| matches_key(k, &kb.universal.refresh),
        |gui, _| {
            gui.needs_refresh = true;
            Ok(true)
        },
    ),
    // Only active while rebasing / merging / cherry-picking.
    when(
        "universal.createRebaseOptionsMenu",
        |k, kb| matches_key(k, &kb.universal.create_rebase_options_menu),
        |gui, _| gui.try_rebase_options_key(),
    ),
    always(
        "universal.pushFiles / pullFiles",
        |k, kb| {
            matches_key(k, &kb.universal.push_files) || matches_key(k, &kb.universal.pull_files)
        },
        |gui, key| {
            let kb = gui.config.user_config.keybinding.clone();
            controller::remotes::handle_key(gui, key, &kb)?;
            Ok(true)
        },
    ),
    // Screen mode toggle (+ to enlarge, _ to shrink, matching lazygit).
    always(
        "universal.nextScreenMode",
        |k, kb| matches_key(k, &kb.universal.next_screen_mode),
        |gui, _| {
            gui.next_screen_mode();
            Ok(true)
        },
    ),
    always(
        "universal.prevScreenMode",
        |k, kb| matches_key(k, &kb.universal.prev_screen_mode),
        |gui, _| {
            gui.prev_screen_mode();
            Ok(true)
        },
    ),
    always(
        "diffMode (W)",
        |k, _| k.code == KeyCode::Char('W'),
        |gui, _| {
            gui.diff_mode.enter(gui.show_file_tree);
            gui.clear_diff_view();
            Ok(true)
        },
    ),
    always(
        "toggleCommandLog (;)",
        |k, _| k.code == KeyCode::Char(';'),
        |gui, _| {
            gui.show_command_log = !gui.show_command_log;
            gui.persist_command_log_visibility();
            Ok(true)
        },
    ),
    always(
        "universal.undo",
        |k, kb| matches_key(k, &kb.universal.undo),
        |gui, _| gui.undo().map(|()| true),
    ),
    always(
        "universal.redo",
        |k, kb| matches_key(k, &kb.universal.redo),
        |gui, _| gui.redo().map(|()| true),
    ),
    // Patch building mode: Commits context, or while a patch is in progress.
    when(
        "universal.createPatchOptionsMenu",
        |k, kb| matches_key(k, &kb.universal.create_patch_options_menu),
        |gui, _| {
            if gui.context_mgr.active() != ContextId::Commits && !gui.patch_building.active {
                return Ok(false);
            }
            controller::patch_building::show_patch_menu(gui).map(|()| true)
        },
    ),
    always(
        "help (?)",
        |k, _| k.code == KeyCode::Char('?'),
        |gui, _| {
            gui.show_command_palette();
            Ok(true)
        },
    ),
    always(
        "universal.startSearch",
        |k, kb| matches_key(k, &kb.universal.start_search),
        |gui, _| {
            gui.search_active = true;
            gui.search_query.clear();
            gui.search_matches.clear();
            gui.search_match_idx = 0;
            let mut ta = ratatui_textarea::TextArea::default();
            ta.set_cursor_line_style(ratatui::style::Style::default());
            gui.search_textarea = Some(ta);
            Ok(true)
        },
    ),
    // `I`: interactive rebase picker. SHIFT alone is accepted because
    // terminals report uppercase letters that way.
    always(
        "interactiveRebasePicker (I)",
        |k, _| plain_char_key(k, 'I'),
        |gui, _| {
            gui.show_interactive_rebase_picker();
            Ok(true)
        },
    ),
    // `G`: global reset picker (lazygit `universal.viewResetOptions`).
    // Lowercase `g` stays contextual (commits.viewResetOptions).
    always(
        "resetPicker (G)",
        |k, _| plain_char_key(k, 'G'),
        |gui, _| {
            gui.show_reset_picker();
            Ok(true)
        },
    ),
    // `.` toggles the commit-details box in commit-related contexts, except
    // where the commit-file tree uses `.` for child navigation.
    when(
        "toggleCommitDetails (.)",
        |k, _| k.code == KeyCode::Char('.'),
        |gui, _| {
            let tree_nav = gui.show_commit_file_tree
                && matches!(
                    gui.context_mgr.active(),
                    ContextId::CommitFiles | ContextId::BranchCommitFiles | ContextId::StashFiles
                );
            if !gui.context_has_commit_details() || tree_nav {
                return Ok(false);
            }
            gui.show_commit_details = !gui.show_commit_details;
            gui.persist_commit_details_visibility();
            Ok(true)
        },
    ),
];

/// Every string-valued binding in a context section, as (field, key).
fn bindings_of<T: Serialize>(section: &T) -> Vec<(String, String)> {
    let value = serde_json::to_value(section).unwrap_or_default();
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .filter(|(_, s)| !s.is_empty())
        .collect()
}

/// Context sections routed through `handle_context_key`, i.e. after
/// both tables.
fn context_sections(kb: &KeybindingConfig) -> Vec<(&'static str, Vec<(String, String)>)> {
    vec![
        ("files", bindings_of(&kb.files)),
        ("branches", bindings_of(&kb.branches)),
        ("commits", bindings_of(&kb.commits)),
        ("stash", bindings_of(&kb.stash)),
        ("status", bindings_of(&kb.status)),
    ]
}

fn always_entries() -> impl Iterator<Item = &'static KeymapEntry> {
    PRE_DIFF_FOCUS
        .iter()
        .chain(NORMAL)
        .filter(|e| e.fires == Fires::Always)
}

/// Default context bindings that are already shadowed by a `Fires::Always`
/// entry. They are listed here so the change is deliberate: resolving one
/// means changing behaviour, and the test fails if one is added or fixed
/// without updating this list.
///
/// - `branches.renameBranch` / `commits.renameCommitWithEditor` = `R`:
///   `universal.refresh` wins (lazygit resolves context-first instead).
///   `renameCommitWithEditor` has no handler at all.
/// - `commits.pickCommit` = `p`: `universal.pullFiles` wins. The commits
///   controller maps `pickCommit` to *drop*, so un-shadowing it as-is would
///   turn `p` from pull into drop.
pub(crate) const KNOWN_DEFAULT_SHADOWED: &[&str] = &[
    "branches.renameBranch",
    "commits.pickCommit",
    "commits.renameCommitWithEditor",
];

/// The shadowing a config produces: context bindings that can never
/// fire because an earlier `Fires::Always` entry consumes their key.
/// Each item is `(context.field, human-readable description)`.
pub(crate) fn shadowed_bindings(kb: &KeybindingConfig) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (ctx, binds) in context_sections(kb) {
        for (field, key) in binds {
            let Some(ev) = parse_key(&key) else { continue };
            if let Some(e) = always_entries().find(|e| (e.matches)(ev, kb)) {
                out.push((
                    format!("{ctx}.{field}"),
                    format!("{ctx}.{field} = {key} (shadowed by {})", e.name),
                ));
            }
        }
    }
    out
}

/// Shadowing introduced by the user's config, i.e. excluding the known
/// default cases. Reported once in the command log at startup.
pub(crate) fn user_shadowed_bindings(kb: &KeybindingConfig) -> Vec<String> {
    let defaults = KeybindingConfig::default();
    let default_shadowed = shadowed_bindings(&defaults);
    shadowed_bindings(kb)
        .into_iter()
        .filter(|(path, desc)| {
            !(KNOWN_DEFAULT_SHADOWED.contains(&path.as_str())
                && default_shadowed.iter().any(|(_, d)| d == desc))
        })
        .map(|(_, desc)| desc)
        .collect()
}

#[cfg(test)]
mod shadowing {
    use super::*;

    fn shadowed(kb: &KeybindingConfig) -> Vec<String> {
        shadowed_bindings(kb).into_iter().map(|(_, d)| d).collect()
    }

    #[test]
    fn default_shadowing_matches_the_known_list() {
        let mut paths: Vec<String> = shadowed_bindings(&KeybindingConfig::default())
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        paths.sort();
        assert_eq!(
            paths, KNOWN_DEFAULT_SHADOWED,
            "default shadowing changed; update KNOWN_DEFAULT_SHADOWED deliberately"
        );
    }

    #[test]
    fn defaults_produce_no_user_warning() {
        assert!(user_shadowed_bindings(&KeybindingConfig::default()).is_empty());
    }

    #[test]
    fn user_remap_onto_a_universal_key_is_warned() {
        let mut kb = KeybindingConfig::default();
        kb.files.generate_ai_commit = kb.universal.quick_ai_note.clone();
        let warned = user_shadowed_bindings(&kb);
        assert_eq!(warned.len(), 1, "{warned:#?}");
        assert!(warned[0].contains("universal.quickAiNote"));
    }

    #[test]
    fn detector_reports_a_shadowed_context_binding() {
        // The P1-2 regression: a context key equal to an early universal key.
        let mut kb = KeybindingConfig::default();
        kb.files.generate_ai_commit = kb.universal.toggle_working_tree_and_head.clone();
        let shadowed = shadowed(&kb);
        assert!(
            shadowed
                .iter()
                .any(|s| s.starts_with("files.generateAICommit")
                    && s.contains("universal.toggleWorkingTreeAndHead")),
            "{shadowed:#?}"
        );
    }

    #[test]
    fn remapping_the_head_toggle_frees_its_old_key() {
        let mut kb = KeybindingConfig::default();
        let old = kb.universal.toggle_working_tree_and_head.clone();
        kb.universal.toggle_working_tree_and_head = "<c-t>".into();
        let ev = parse_key(&old).unwrap();
        assert!(
            !NORMAL
                .iter()
                .any(|e| e.name == "universal.toggleWorkingTreeAndHead" && (e.matches)(ev, &kb))
        );
    }

    #[test]
    fn entry_names_are_unique() {
        let mut names: Vec<_> = PRE_DIFF_FOCUS
            .iter()
            .chain(NORMAL)
            .map(|e| e.name)
            .collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
    }
}
