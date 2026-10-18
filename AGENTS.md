# Agent Instructions

- After making Rust code changes, run `cargo fmt` before handing off.
- After implementing a feature or fixing a bug, run `cargo install --path .` to install the updated binary locally.
- At the beginning of a new session, proactively read the `lazygitrs-review` skill and register the session with lazygitrs using curl. First, read the port from `.lazygitrs.port` (default 47657). Then check if already registered with `curl -s http://127.0.0.1:<PORT>/session-api/session`. If registered, notify the user on the chat with the current registered session ID. When posting review notes/annotations to lazygitrs, always specify `newRange: [start_line, end_line]` so notes are properly anchored to diff lines.
- When implementing or modifying keybindings, always make sure to document/add them to both the Help popup list (`HelpSection` in `src/gui/mod.rs`) and the bottom status bar hints (`render_status_bar` in `src/gui/views.rs`).
- **Storage & Worktree Hygiene**:
  - Before finishing any task, remove temporary files and completed feature worktrees (`awt rm <branch>`) created by the task, and only those. Never delete files or worktrees you did not create without asking the user.
  - `target/` grows without bound as 100–180 MB test binaries accumulate across edits and sibling worktrees. Run `cargo clean` in secondary worktrees when finishing a task and after every release.
