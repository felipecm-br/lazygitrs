# Agent Instructions

- After making Rust code changes, run `cargo fmt` before handing off.
- **Storage & Worktree Hygiene**:
  - Before finishing any task, remove temporary files and completed feature worktrees (`awt rm <branch>`) created by the task, and only those. Never delete files or worktrees you did not create without asking the user.
  - `target/` grows without bound as 100–180 MB test binaries accumulate across edits and sibling worktrees. Run `cargo clean` in secondary worktrees when finishing a task and after every release.
