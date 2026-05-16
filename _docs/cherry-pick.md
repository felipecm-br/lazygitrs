# Cherry Pick

Cherry pick lets you copy one or more commits from any branch and apply them onto
the currently checked-out branch, without merging the entire branch.

---

## Key bindings

| Key | Action | Where |
|-----|--------|--------|
| `C` | Copy selected commit(s) to the cherry-pick clipboard | Commits panel, Reflog panel |
| `v` | Toggle range selection | Commits panel |
| `V` | Paste (apply) all clipboard commits onto the current branch | Commits panel |
| `<Ctrl-q>` | Clear the cherry-pick clipboard (discard copies) | Commits panel |
| `<Esc>` | Cancel range selection; then clear clipboard on second press | Commits panel |

When a cherry-pick is in progress (conflict state), press the **rebase/merge key**
(same key you use to open the rebase menu) to access:

| Option key | Action |
|------------|--------|
| `c` | `git cherry-pick --continue` |
| `a` | `git cherry-pick --abort` |
| `s` | `git cherry-pick --skip` |

---

## Workflow: copy commits to another branch

### 1. Single commit

1. Navigate to the **Commits** panel (or **Reflog** panel).
2. Move the cursor to the commit you want.
3. Press `C` — the commit is added to the clipboard (its hash turns cyan/bold).
4. Switch to the target branch (Branches panel → `<space>` or `Enter`).
5. Go back to the Commits panel and press `V`.
6. Confirm the prompt — the commit is applied.

### 2. Multiple commits (range)

1. Navigate to the **Commits** panel.
2. Move the cursor to the **oldest** commit in the range.
3. Press `v` to anchor the range selection.
4. Move the cursor to the **newest** commit in the range.
5. Press `C` — all commits in the range are added to the clipboard.
   - You can repeat steps 2-5 across different parts of the list to accumulate more commits.
6. Switch to the target branch.
7. Press `V` and confirm — commits are applied oldest-first, preserving history order.

> **Note:** You can add commits from both the Commits panel and the Reflog panel
> to the same clipboard before pasting.

### 3. Discard the clipboard without pasting

Press `<Ctrl-q>` at any time to clear all copied commits and start over.
Alternatively, press `<Esc>` (once to cancel range select, once more to clear the clipboard).

---

## Handling conflicts

If `git cherry-pick` encounters a conflict the TUI enters "cherry-pick in progress" state
(the status bar shows **CHERRY-PICKING**).

1. Resolve conflicts in your editor (or use the Files panel).
2. Stage the resolved files (`a` in the Files panel, or via the staging view).
3. Press the rebase/merge options key (shown in the hint bar) to open the menu.
4. Choose **Continue cherry-pick** (`c`) to finish.

To give up entirely, choose **Abort cherry-pick** (`a`). This rewinds all commits
applied so far and returns the branch to its pre-cherry-pick state.

To skip a commit that conflicts entirely and move on to the next, choose
**Skip this commit** (`s`).

---

## Visual indicators

Commits currently in the cherry-pick clipboard are highlighted in **cyan bold** in
the commit list so you can see what is queued before pasting.

---

## Tips

- Cherry-pick from the **Reflog** panel to recover commits that are no longer on
  any branch (e.g., after a bad reset).
- For a single "transplant", cherry-pick is cleaner than merging: only the chosen
  work ends up on the target branch.
- After a successful paste the clipboard is cleared automatically.
