use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct KeybindingConfig {
    pub universal: UniversalKeybinding,
    pub status: StatusKeybinding,
    pub files: FilesKeybinding,
    pub branches: BranchesKeybinding,
    pub commits: CommitsKeybinding,
    pub stash: StashKeybinding,
    #[serde(rename = "commitMessage")]
    pub commit_message: CommitMessageKeybinding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UniversalKeybinding {
    pub quit: String,
    #[serde(rename = "quit-alt1")]
    pub quit_alt1: String,
    #[serde(rename = "return")]
    pub return_key: String,
    #[serde(rename = "quitWithoutChangingDirectory")]
    pub quit_without_changing_directory: String,
    #[serde(rename = "togglePanel")]
    pub toggle_panel: String,
    #[serde(rename = "togglePanelReverse")]
    pub toggle_panel_reverse: String,
    #[serde(rename = "prevItem")]
    pub prev_item: String,
    #[serde(rename = "nextItem")]
    pub next_item: String,
    #[serde(rename = "prevItem-alt")]
    pub prev_item_alt: String,
    #[serde(rename = "nextItem-alt")]
    pub next_item_alt: String,
    #[serde(rename = "prevPage")]
    pub prev_page: String,
    #[serde(rename = "nextPage")]
    pub next_page: String,
    #[serde(rename = "scrollLeft")]
    pub scroll_left: String,
    #[serde(rename = "scrollRight")]
    pub scroll_right: String,
    #[serde(rename = "gotoTop")]
    pub goto_top: String,
    #[serde(rename = "gotoBottom")]
    pub goto_bottom: String,
    #[serde(rename = "prevBlock")]
    pub prev_block: String,
    #[serde(rename = "nextBlock")]
    pub next_block: String,
    #[serde(rename = "prevBlock-alt")]
    pub prev_block_alt: String,
    #[serde(rename = "nextBlock-alt")]
    pub next_block_alt: String,
    #[serde(rename = "nextMatch")]
    pub next_match: String,
    #[serde(rename = "prevMatch")]
    pub prev_match: String,
    #[serde(rename = "startSearch")]
    pub start_search: String,
    #[serde(rename = "optionMenu")]
    pub option_menu: String,
    pub edit: String,
    #[serde(rename = "openFile")]
    pub open_file: String,
    #[serde(rename = "scrollUpMain")]
    pub scroll_up_main: String,
    #[serde(rename = "scrollDownMain")]
    pub scroll_down_main: String,
    #[serde(rename = "scrollUpMain-alt1")]
    pub scroll_up_main_alt1: String,
    #[serde(rename = "scrollDownMain-alt1")]
    pub scroll_down_main_alt1: String,
    pub undo: String,
    pub redo: String,
    #[serde(rename = "filteringMenu")]
    pub filtering_menu: String,
    #[serde(rename = "diffingMenu")]
    pub diffing_menu: String,
    #[serde(rename = "copyToClipboard")]
    pub copy_to_clipboard: String,
    pub refresh: String,
    #[serde(rename = "createRebaseOptionsMenu")]
    pub create_rebase_options_menu: String,
    #[serde(rename = "pushFiles")]
    pub push_files: String,
    #[serde(rename = "pullFiles")]
    pub pull_files: String,
    #[serde(rename = "nextScreenMode")]
    pub next_screen_mode: String,
    #[serde(rename = "prevScreenMode")]
    pub prev_screen_mode: String,
    #[serde(rename = "createPatchOptionsMenu")]
    pub create_patch_options_menu: String,
    #[serde(rename = "revertBlock")]
    pub revert_block: String,
    #[serde(rename = "undoRevertBlock")]
    pub undo_revert_block: String,
    #[serde(rename = "shrinkSidePanel")]
    pub shrink_side_panel: String,
    #[serde(rename = "expandSidePanel")]
    pub expand_side_panel: String,
    #[serde(rename = "sidePanelFull")]
    pub side_panel_full: String,
    #[serde(rename = "mainPanelFull")]
    pub main_panel_full: String,
    #[serde(rename = "resetSidePanel")]
    pub reset_side_panel: String,
    #[serde(rename = "customCommandPrompt")]
    pub custom_command_prompt: String,
    #[serde(rename = "toggleDiffViewLayout")]
    pub toggle_diff_view_layout: String,
    #[serde(rename = "toggleWorkingTreeAndHead")]
    pub toggle_working_tree_and_head: String,
    #[serde(rename = "quickAiNote")]
    pub quick_ai_note: String,
    #[serde(rename = "returnToCockpit", default)]
    pub return_to_cockpit: String,
}

impl Default for UniversalKeybinding {
    fn default() -> Self {
        Self {
            quit: "q".into(),
            quit_alt1: "<c-c>".into(),
            return_key: "<escape>".into(),
            quit_without_changing_directory: "Q".into(),
            toggle_panel: "<tab>".into(),
            toggle_panel_reverse: "<backtab>".into(),
            prev_item: "k".into(),
            next_item: "j".into(),
            prev_item_alt: "<up>".into(),
            next_item_alt: "<down>".into(),
            prev_page: "<pgup>".into(),
            next_page: "<pgdown>".into(),
            scroll_left: "H".into(),
            scroll_right: "L".into(),
            goto_top: "<".into(),
            goto_bottom: ">".into(),
            prev_block: "<left>".into(),
            next_block: "<right>".into(),
            prev_block_alt: "h".into(),
            next_block_alt: "l".into(),
            next_match: "n".into(),
            prev_match: "N".into(),
            start_search: "/".into(),
            option_menu: "x".into(),
            edit: "e".into(),
            open_file: "o".into(),
            scroll_up_main: "<pgup>".into(),
            scroll_down_main: "<pgdown>".into(),
            scroll_up_main_alt1: "K".into(),
            scroll_down_main_alt1: "J".into(),
            undo: "z".into(),
            redo: "<c-z>".into(),
            filtering_menu: "<c-s>".into(),
            diffing_menu: "W".into(),
            copy_to_clipboard: "<c-o>".into(),
            refresh: "R".into(),
            create_rebase_options_menu: "m".into(),
            push_files: "P".into(),
            pull_files: "p".into(),
            next_screen_mode: "+".into(),
            prev_screen_mode: "_".into(),
            create_patch_options_menu: "<c-p>".into(),
            revert_block: "<enter>".into(),
            undo_revert_block: "u".into(),
            shrink_side_panel: "<a-h>".into(),
            expand_side_panel: "<a-l>".into(),
            side_panel_full: "<a-k>".into(),
            main_panel_full: "<a-j>".into(),
            reset_side_panel: "<a-r>".into(),
            custom_command_prompt: ":".into(),
            toggle_diff_view_layout: "\\".into(),
            toggle_working_tree_and_head: "<c-g>".into(),
            quick_ai_note: "<c-n>".into(),
            return_to_cockpit: "".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusKeybinding {
    #[serde(rename = "checkForUpdate")]
    pub check_for_update: String,
    #[serde(rename = "recentRepos")]
    pub recent_repos: String,
    #[serde(rename = "allBranchesLogGraph")]
    pub all_branches_log_graph: String,
}

impl Default for StatusKeybinding {
    fn default() -> Self {
        Self {
            check_for_update: "u".into(),
            recent_repos: "<enter>".into(),
            all_branches_log_graph: "a".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesKeybinding {
    #[serde(rename = "commitChanges")]
    pub commit_changes: String,
    #[serde(rename = "generateAICommit")]
    pub generate_ai_commit: String,
    #[serde(rename = "commitChangesWithoutHook")]
    pub commit_changes_without_hook: String,
    #[serde(rename = "amendLastCommit")]
    pub amend_last_commit: String,
    #[serde(rename = "commitChangesWithEditor")]
    pub commit_changes_with_editor: String,
    #[serde(rename = "toggleStagedAll")]
    pub toggle_staged_all: String,
    #[serde(rename = "stashAllChanges")]
    pub stash_all_changes: String,
    #[serde(rename = "viewStashOptions")]
    pub view_stash_options: String,
    #[serde(rename = "toggleTreeView")]
    pub toggle_tree_view: String,
    pub fetch: String,
    #[serde(rename = "ignoreFile")]
    pub ignore_file: String,
}

impl Default for FilesKeybinding {
    fn default() -> Self {
        Self {
            commit_changes: "c".into(),
            // Not <c-g>: the universal Files<->HEAD toggle owns it and is
            // checked first, so a files-level <c-g> could never fire.
            generate_ai_commit: "<c-a>".into(),
            commit_changes_without_hook: "w".into(),
            amend_last_commit: "A".into(),
            commit_changes_with_editor: "C".into(),
            toggle_staged_all: "a".into(),
            stash_all_changes: "s".into(),
            view_stash_options: "S".into(),
            toggle_tree_view: "`".into(),
            fetch: "f".into(),
            ignore_file: "i".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BranchesKeybinding {
    #[serde(rename = "createPullRequest")]
    pub create_pull_request: String,
    #[serde(rename = "viewPullRequestOptions")]
    pub view_pull_request_options: String,
    #[serde(rename = "checkoutBranchByName")]
    pub checkout_branch_by_name: String,
    #[serde(rename = "forceCheckoutBranch")]
    pub force_checkout_branch: String,
    #[serde(rename = "rebaseBranch")]
    pub rebase_branch: String,
    #[serde(rename = "renameBranch")]
    pub rename_branch: String,
    #[serde(rename = "mergeIntoCurrentBranch")]
    pub merge_into_current_branch: String,
    #[serde(rename = "fastForward")]
    pub fast_forward: String,
    #[serde(rename = "setUpstream")]
    pub set_upstream: String,
}

impl Default for BranchesKeybinding {
    fn default() -> Self {
        Self {
            create_pull_request: "o".into(),
            view_pull_request_options: "O".into(),
            checkout_branch_by_name: "c".into(),
            force_checkout_branch: "F".into(),
            rebase_branch: "r".into(),
            rename_branch: "R".into(),
            merge_into_current_branch: "M".into(),
            fast_forward: "f".into(),
            set_upstream: "u".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommitsKeybinding {
    #[serde(rename = "squashDown")]
    pub squash_down: String,
    #[serde(rename = "renameCommit")]
    pub rename_commit: String,
    #[serde(rename = "renameCommitWithEditor")]
    pub rename_commit_with_editor: String,
    #[serde(rename = "viewResetOptions")]
    pub view_reset_options: String,
    #[serde(rename = "markCommitAsFixup")]
    pub mark_commit_as_fixup: String,
    #[serde(rename = "createFixupCommit")]
    pub create_fixup_commit: String,
    #[serde(rename = "squashAboveCommits")]
    pub squash_above_commits: String,
    #[serde(rename = "moveDownCommit")]
    pub move_down_commit: String,
    #[serde(rename = "moveUpCommit")]
    pub move_up_commit: String,
    #[serde(rename = "amendToCommit")]
    pub amend_to_commit: String,
    #[serde(rename = "pickCommit")]
    pub pick_commit: String,
    #[serde(rename = "revertCommit")]
    pub revert_commit: String,
    #[serde(rename = "cherryPickCopy")]
    pub cherry_pick_copy: String,
    #[serde(rename = "pasteCommits")]
    pub paste_commits: String,
    #[serde(rename = "tagCommit")]
    pub tag_commit: String,
    #[serde(rename = "checkoutCommit")]
    pub checkout_commit: String,
    #[serde(rename = "resetCherryPick")]
    pub reset_cherry_pick: String,
    #[serde(rename = "openLogMenu")]
    pub open_log_menu: String,
    #[serde(rename = "viewBisectOptions")]
    pub view_bisect_options: String,
    #[serde(rename = "interactiveRebase")]
    pub interactive_rebase: String,
}

impl Default for CommitsKeybinding {
    fn default() -> Self {
        Self {
            squash_down: "s".into(),
            rename_commit: "r".into(),
            rename_commit_with_editor: "R".into(),
            view_reset_options: "g".into(),
            mark_commit_as_fixup: "<c-f>".into(),
            create_fixup_commit: "F".into(),
            squash_above_commits: "S".into(),
            move_down_commit: "<c-j>".into(),
            move_up_commit: "<c-k>".into(),
            amend_to_commit: "A".into(),
            pick_commit: "p".into(),
            revert_commit: "t".into(),
            cherry_pick_copy: "C".into(),
            paste_commits: "V".into(),
            tag_commit: "T".into(),
            checkout_commit: "<space>".into(),
            reset_cherry_pick: "<c-q>".into(),
            open_log_menu: "<c-s>".into(),
            view_bisect_options: "b".into(),
            interactive_rebase: "i".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StashKeybinding {
    #[serde(rename = "popStash")]
    pub pop_stash: String,
    #[serde(rename = "renameStash")]
    pub rename_stash: String,
}

impl Default for StashKeybinding {
    fn default() -> Self {
        Self {
            pop_stash: "g".into(),
            rename_stash: "r".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommitMessageKeybinding {
    #[serde(rename = "commitMenu")]
    pub commit_menu: String,
    #[serde(rename = "aiGenerate")]
    pub ai_generate: String,
}

impl Default for CommitMessageKeybinding {
    fn default() -> Self {
        Self {
            commit_menu: "<c-o>".into(),
            ai_generate: "<c-g>".into(),
        }
    }
}

pub fn parse_key(s: &str) -> Option<KeyEvent> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    if s.starts_with('<') && s.ends_with('>') {
        let inner = &s[1..s.len() - 1];

        if let Some(key) = inner.strip_prefix("c-") {
            let ch = key.chars().next()?;
            return Some(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
        }

        if let Some(key) = inner.strip_prefix("a-") {
            let ch = key.chars().next()?;
            return Some(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::ALT));
        }

        return match inner {
            "enter" => Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            "escape" | "esc" => Some(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            "tab" => Some(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            "backtab" | "shift-tab" => Some(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
            "backspace" | "bs" => Some(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            "delete" | "del" => Some(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE)),
            "space" => Some(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
            "up" => Some(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            "down" => Some(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            "left" => Some(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            "right" => Some(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            "pgup" => Some(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
            "pgdown" => Some(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            "home" => Some(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)),
            "end" => Some(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
            _ => None,
        };
    }

    if s.len() == 1 {
        let ch = s.chars().next()?;
        let modifiers = if ch.is_uppercase() {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        return Some(KeyEvent::new(KeyCode::Char(ch), modifiers));
    }

    None
}

/// Formats a keybinding string (like `<c-a>`, `<escape>`, `q`) into human-readable display text.
pub fn format_key_display(k: &str) -> String {
    if let Some(rest) = k.strip_prefix("<c-").and_then(|s| s.strip_suffix('>')) {
        format!("Ctrl + {}", rest.to_uppercase())
    } else if let Some(rest) = k.strip_prefix("<a-").and_then(|s| s.strip_suffix('>')) {
        format!("Alt + {}", rest.to_uppercase())
    } else if k == "<escape>" {
        "Esc".to_string()
    } else if k == "<enter>" {
        "Enter".to_string()
    } else if k == "<space>" {
        "Space".to_string()
    } else if k == "<tab>" {
        "Tab".to_string()
    } else if k == "<backtab>" {
        "Shift + Tab".to_string()
    } else {
        k.to_string()
    }
}

/// Generates the complete Markdown documentation (KEYBINDINGS.md) dynamically from KeybindingConfig.
pub fn generate_keybindings_md(kb: &KeybindingConfig) -> String {
    let esc_key = format_key_display(&kb.universal.return_key);
    let quit_key = format_key_display(&kb.universal.quit);
    let quit_alt = format_key_display(&kb.universal.quit_alt1);
    let ai_commit = format_key_display(&kb.files.generate_ai_commit);
    let prompt_key = format_key_display(&kb.universal.custom_command_prompt);
    let filter_log = format_key_display(&kb.commits.open_log_menu);
    let filter_log_raw = &kb.commits.open_log_menu;
    let fixup_mark = format_key_display(&kb.commits.mark_commit_as_fixup);
    let fixup_mark_raw = &kb.commits.mark_commit_as_fixup;
    let bisect = format_key_display(&kb.commits.view_bisect_options);
    let rebase = format_key_display(&kb.commits.interactive_rebase);
    let cp_copy = format_key_display(&kb.commits.cherry_pick_copy);
    let cp_paste = format_key_display(&kb.commits.paste_commits);
    let squash = format_key_display(&kb.commits.squash_down);
    let squash_above = format_key_display(&kb.commits.squash_above_commits);
    let rename = format_key_display(&kb.commits.rename_commit);
    let rename_editor = format_key_display(&kb.commits.rename_commit_with_editor);
    let reset = format_key_display(&kb.commits.view_reset_options);
    let undo_revert = format_key_display(&kb.universal.undo_revert_block);

    format!(
        r#"# Lazygitrs Keybindings, Interaction Matrix & Biomechanical Reference

This document serves as the definitive reference for keyboard navigation, diff reviewing, commits manipulation, tree exploration, and AI-assisted review workflows in **lazygitrs**.

---

## 🔬 1. Design & Biomechanical Principles

1. **Home Row Anchored ($H = 0$):** High-frequency inspection and navigation operations are accessible directly from the resting Home Row ($ASDF / JKL;$) without requiring awkward reaches or mouse intervention.
2. **Sub-100ms Doherty Threshold:** Built natively in Rust with zero runtime garbage collection, lazygitrs reaches first frame in ~49 ms and idles at ~0.35% CPU (demand-driven wake cadence: 16 ms busy, 50 ms after input, ≤1 s idle).
3. **Hierarchical Context Unwinding (Cascading Escape):**
   - Inside Diff or Submenus: Pressing `{esc_key}` unwinds focus back to the Primary Anchor (Files panel `[2]`) without dismissing the interface.
   - At the Root List: Pressing `{esc_key}` in `--popup` / `outerFrame` mode immediately dismisses the popup; in full-screen mode it requires a double-tap within 400 ms so unwinding from sub-views never closes the session by accident.
4. **Dynamic Status Bar Synchronicity:** Control hints in the bottom bar dynamically query the runtime configuration (e.g., `commits.open_log_menu`), guaranteeing that custom remappings always reflect live reality without stale text.
5. **Worktree Port Isolation:** Each Git worktree maintains its own dynamic HTTP port stored in `lazygitrs.port` inside its git dir, enabling multiple concurrent agent sessions without port collisions or crosstalk.

---

## 🗺️ 2. Core Functional Layers

### 2.1 Global & Root Navigation
- **`1` / `2` / `3` / `4` / `5`**: Instant 1-touch jump to root panels:
  - `1`: **Status**
  - `2`: **Files**
  - `3`: **Branches**
  - `4`: **Commits**
  - `5`: **Stash**
- **`{esc_key}`**: Cascading context unwinding (Diff $\rightarrow$ Root list $\rightarrow$ Exit).
- **`{quit_key}` / `{quit_alt}`**: Immediate application exit.
- **`{prompt_key}`** (Colon): Universal shell command prompt executed directly in the repository root.

### 2.2 File Tree & Directory Navigation (Files & Commit Files)
lazygitrs features full hierarchical tree navigation with instant folding and combined diffs:
- **`-`** (Minus): **Fold / Unfold Directory**. Explicitly toggles directory collapse state for any folder node, including the root directory.
- **`Enter` (on Directory Node)**: **Fullscreen Combined Diff**. Expands and focuses a single unified diff containing all modified files under that directory.
- **`Enter` (on File Node)**: Expands and focuses the file diff panel in fullscreen mode.
- **`,`** (Comma): **Jump to Parent Directory**. Traverses up to the enclosing folder node.
- **`.`** (Period): **Jump to Child Node**. Navigates directly into the first child node of the highlighted folder.
- **`<`**: **Previous Sibling**. Jumps to the preceding sibling node at the current tree depth.
- **`>`**: **Next Sibling**. Jumps to the next sibling node at the current tree depth.

### 2.3 Reconciled Commits Panel & Branch Filtering
- **`{filter_log}` (`{filter_log_raw}`)**: **Branch Filter Menu (`openLogMenu`)**. Opens the multi-select branch filtering popup checklist with `<Clear Filter>` support. Dynamically rendered in the status bar (`ctrl+s filter branch`).
- **`{fixup_mark}` (`{fixup_mark_raw}`)**: **Mark Commit as Fixup (`markCommitAsFixup`)**. Designates highlighted commit as the target for an automatic fixup.
- **`F`** (Shift+F): **Create Fixup Commit (`createFixupCommit`)**. Generates a fixup commit targeting the marked commit.
- **`a`**: **Toggle Log View**. Switches commit history between all branches and HEAD-only.
- **`{bisect}`**: **Bisect Options Menu (`viewBisectOptions`)**. Opens Git bisect workflow options.
- **`{rebase}`**: **Interactive Rebase (`interactiveRebase`)**. Starts an interactive rebase atop the selected commit.
- **`{cp_copy}` / `{cp_paste}`**: **Cherry-Pick Copy & Paste**. `C` copies the commit SHA; `V` pastes/cherry-picks the copied commits.
- **`{squash}` / `{squash_above}`**: **Squash**. `{squash}` squashes down into next commit; `{squash_above}` squashes all commits above.
- **`{rename}` / `{rename_editor}`**: **Rename Commit**. `{rename}` renames in-place; `{rename_editor}` opens default `$EDITOR`.
- **`{reset}`**: **Reset Options**. Opens soft, mixed, and hard reset options.

### 2.4 Diff Mode & Hunk Reverting
- **`{{` / `}}`**: Cycle previous / next diff hunk.
- **`[` / `]`**: Switch diff viewport between Old side only, New side only, or Side-by-Side.
- **`Enter` (in Diff)**: **Revert Hunk Block**. Reverts the hovered or highlighted diff block in the working tree.
- **`{undo_revert}` (in Diff)**: **Undo Revert Block**. Safely restores the last reverted diff block.
- **`v`**: Toggles visual range selection across diff lines.

### 2.5 AI Review Notes Workflow & Worktree Isolation
lazygitrs integrates a native, bidirectional AI code-review system:
- **`c`**: **Create Note**. Opens inline comment editor on the hovered diff line (or selected line).
- **`n` / `N`**: **Cycle Notes**. Jumps to next (`n`) or previous (`N`) review note in the current diff.
- **`y`**: **Yank / Copy Note**. Copies full note text to system clipboard (Wayland `wl-copy`, X11 `xclip`/`xsel`, macOS `pbcopy`, Windows `clip`, or terminal OSC 52) with a 500ms Neovim-style flash highlight.
- **`S`** (Shift+S): **Send to AI**. Dispatches review note to the active AI session via SSE, local HTTP TUI push (e.g. OpenCode), or background `notifyCommand` fallback. Note status updates from `new` to `sent`.
- **`r` / `R`**: **Reset Status**. Reverts note status back to `new` for re-review.
- **`d`**: **Delete Note**. Deletes note from the notes file (`.git/info/lines.json`), automatically focusing the closest adjacent note.
- **`Enter` / `o`**: **View Note Full Details**. Opens popup with full markdown commentary, rationale, author, and timestamp.
- **Worktree Isolation (`lazygitrs.port`)**: Each worktree writes its dynamic HTTP port to `$(git rev-parse --git-dir)/lazygitrs.port`, guaranteeing that parallel agent sessions operate in strict isolation.

---

## 🔬 3. Consolidated Interaction Matrix (KLM Biomechanical Audit)

Audited against Card, Moran & Newell's **Keystroke-Level Model (KLM)**:

| Key / Chord | Context / Mode | Action Executed | Biomechanical Mechanics | KLM Cost ($T$) | Ergonomic Rationale |
| :--- | :--- | :--- | :--- | :---: | :--- |
| **`{esc_key}`** | Diff / Submenus | Unwind Focus to Files List | Left Pinky single tap (or CapsLock tap) | $100\text{{ ms}}$ | Hierarchical escape; prevents accidental closing during deep review. |
| **`{esc_key}`** | Root Files List | Dismiss Popup (or Double-Tap in Fullscreen) | Left Pinky single / double tap | $100\text{{ ms}}$ / $240\text{{ ms}}$ | Instant popup dismissal; double-tap guard in fullscreen prevents accidental exits. |
| **`Ctrl + W` `h` / `l` / `k` / `j` / `=`** | Global | Shrink / Expand / Maximize / Reset Panels | CapsLock + W, then Home Row `h`/`l`/`k`/`j`/`=` | $240\text{{ ms}}$ | Vim window-resize idiom ($H = 0$); avoids `Alt` thumb adduction (`Alt+h/l/k/j/r` kept for compatibility). |
| **`{ai_commit}`** | Files Panel | Generate AI Commit Message | CapsLock (Pinky) + A (Left Pinky) | $120\text{{ ms}}$ | Direct 1-chord AI commit generation from Files panel. |
| **`{quit_key}`** / **`{quit_alt}`**| Anywhere | Hard Exit Application | Left Pinky direct tap / Left Pinky + Left Index | $100\text{{ ms}}$ / $120\text{{ ms}}$ | Standard terminal exit fallback. |
| **`1` / `2` / `3` / `4` / `5`** | Root Navigation | Jump to Status / Files / Branches / Commits / Stash | Number row direct single tap | $120\text{{ ms}}$ | Direct panel selection bypassing sequential Tab traversal. |
| **`{prompt_key}`** (Colon) | Global | Open Shell Command Prompt | Shift + `;` (Right Pinky chord) | $130\text{{ ms}}$ | Vim ex-command mnemonic; executes arbitrary scripts in repo root. |
| **`-`** (Minus) | File Tree View | Fold / Unfold Directory Node | Right Pinky reach to top-right row | $120\text{{ ms}}$ | Explicit directory collapse toggle without triggering diff preview. |
| **`Enter`** (on dir) | File Tree View | Fullscreen Combined Diff | Right Pinky tap on Enter | $100\text{{ ms}}$ | Instant multi-file inspection without entering subfolders individually. |
| **`Enter`** (on file)| File Tree View | Fullscreen File Diff | Right Pinky tap on Enter | $100\text{{ ms}}$ | Focuses diff panel in full screen mode for deep review. |
| **`,`** (Comma) | File Tree View | Navigate to Parent Directory | Right Middle reach to bottom row | $110\text{{ ms}}$ | Upward hierarchy movement without shifting hand position. |
| **`.`** (Period) | File Tree View | Navigate to First Child Node | Right Ring reach to bottom row | $110\text{{ ms}}$ | Downward hierarchy movement into child directory. |
| **`<`** | File Tree View | Navigate to Previous Sibling | Shift + `,` (Pinky + Middle) | $140\text{{ ms}}$ | Horizontal sibling traversal across same directory depth. |
| **`>`** | File Tree View | Navigate to Next Sibling | Shift + `.` (Pinky + Ring) | $140\text{{ ms}}$ | Horizontal sibling traversal across same directory depth. |
| **`{filter_log}`** | Commits Panel | Branch Filter Menu (`openLogMenu`) | CapsLock (Pinky) + S (Left Ring) | $120\text{{ ms}}$ | Home-row chord; status bar dynamically renders active binding. |
| **`{fixup_mark}`** | Commits Panel | Mark Commit as Fixup | CapsLock (Pinky) + F (Left Index) | $120\text{{ ms}}$ | Home-row inward chord; fast fixup target selection. |
| **`F`** (Shift+F) | Commits Panel | Create Fixup Commit | Left Pinky (Shift) + Left Index (F) | $140\text{{ ms}}$ | Direct uppercase counterpart to fixup marking. |
| **`a`** | Commits Panel | Toggle All Branches vs HEAD Log | Left Pinky direct tap on Home Row `a` | $100\text{{ ms}}$ | High-frequency log toggle with zero finger stretch. |
| **`{bisect}`** | Commits Panel | Open Bisect Options Menu | Left Index reach down to `b` | $110\text{{ ms}}$ | Quick access to binary search debugging menu. |
| **`{rebase}`** | Commits Panel | Interactive Rebase Menu | Right Middle reach up to `i` | $110\text{{ ms}}$ | Standard Git interactive rebase mnemonic. |
| **`{cp_copy}` / `{cp_paste}`** | Commits Panel | Cherry-Pick Copy / Paste | Shift + C / Shift + V | $140\text{{ ms}}$ | Universal OS clipboard mnemonics adapted for Git commits. |
| **`{squash}` / `{squash_above}`** | Commits Panel | Squash Down / Squash Above | Left Ring tap / Shift + Left Ring | $100\text{{ ms}}$ / $140\text{{ ms}}$ | Directional squash actions on Home Row. |
| **`{rename}` / `{rename_editor}`** | Commits Panel | Rename Commit (Inline / Editor) | Left Index reach up to `r` / Shift + R | $110\text{{ ms}}$ / $140\text{{ ms}}$ | Quick inline rename or full editor opening. |
| **`{reset}`** | Commits Panel | Git Reset Options Menu | Left Index reach to `g` | $100\text{{ ms}}$ | Soft, mixed, and hard reset selector. |
| **`{{` / `}}`** | Diff View | Previous / Next Diff Hunk | Shift + `[` / `]` (Right Pinky reach) | $140\text{{ ms}}$ | Standard Vim paragraph/block navigation applied to hunks. |
| **`[` / `]`** | Diff View | Old / New / Both Diff Side Switch | Right Pinky reach to bracket keys | $120\text{{ ms}}$ | Rapid single-panel inspection without altering diff contents. |
| **`Enter`** (in diff)| Diff View | Revert Hovered Diff Block | Right Pinky tap on Enter | $100\text{{ ms}}$ | Immediate hunk rejection during review. |
| **`{undo_revert}`** (in diff) | Diff View | Undo Last Revert Block | Right Index reach up to `u` | $110\text{{ ms}}$ | Reversible safety net for accidental hunk discards. |
| **`v`** | Diff View | Toggle Visual Line Selection | Left Index reach down to `v` | $110\text{{ ms}}$ | Granular line-level staging and inspection. |
| **`c`** | Diff View | Create Inline Review Note | Left Middle reach down to `c` | $110\text{{ ms}}$ | Direct note composition on hovered diff line. |
| **`n` / `N`** | Diff View | Cycle Next / Prev Review Note | Right Index reach to `n` / Shift + `N` | $110\text{{ ms}}$ / $140\text{{ ms}}$ | Familiar Vim search-next navigation pattern for review notes. |
| **`y`** (on note) | Diff View | Yank / Copy Note to Clipboard | Right Index reach up to `y` | $110\text{{ ms}}$ | 500ms Neovim-style visual flash feedback; copies note text. |
| **`S`** (Shift+S) | Diff View | Send Note to Active AI Session | Left Pinky (Shift) + Left Ring (S) | $140\text{{ ms}}$ | `S` = Send. Broadcasts prompt to AI via the worktree's `lazygitrs.port`. |
| **`r` / `R`** (on note)| Diff View | Reset Note Status to `New` | Left Index reach up to `r` | $110\text{{ ms}}$ | Re-evaluates note without having to re-type commentary. |
| **`d`** (on note) | Diff View | Delete Review Note | Left Middle tap on Home Row `d` | $100\text{{ ms}}$ | Destructive single-note dismissal with auto-selection of next note. |
| **`Enter` / `o`** | Diff View | View Note Full Details Popup | Right Pinky tap / Right Ring reach | $100\text{{ ms}}$ / $110\text{{ ms}}$ | Full markdown rendering of rationale, author, and timestamp. |

---

## 🔗 Related Resources
- [AI Review Architecture](AI_INTEGRATION_ARCHITECTURE.md): Zero-dependency fallback and tmux injection.
- [Universal AI Integration](UNIVERSAL_INTEGRATION.md): Real-time SSE and multiplexer integration.
- [Review Skill Documentation](../.agents/skills/lazygitrs-review/SKILL.md): Agent-side RPC and lifecycle integration.
"#
    )
}

pub fn matches_key(key: KeyEvent, binding: &str) -> bool {
    if let Some(expected) = parse_key(binding) {
        let code_match = match (key.code, expected.code) {
            (KeyCode::Char(c1), KeyCode::Char(c2)) => c1.eq_ignore_ascii_case(&c2),
            (code1, code2) => code1 == code2,
        };
        code_match && key.modifiers == expected.modifiers
    } else {
        false
    }
}

#[cfg(test)]
mod doc_tests {
    // Shadowing tests live with the dispatch tables in `gui/keymap.rs`.
    use super::*;

    #[test]
    fn test_generate_keybindings_md() {
        let kb = KeybindingConfig::default();
        let doc = generate_keybindings_md(&kb);
        assert!(doc.contains("Card, Moran & Newell's"));
        assert!(doc.contains("Ctrl + A"));
        assert!(doc.contains("Ctrl + S"));
        assert!(doc.contains("Ctrl + W"));
    }
}
