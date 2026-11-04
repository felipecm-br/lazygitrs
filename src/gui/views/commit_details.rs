//! Commit details panel and commit resolution for the active context.

use super::*;

/// Resolve the commit to display in the details panel based on the active
/// context.  Returns `None` when the context isn't commit-listing or nothing
/// is selected.  For `CommitFiles`/`BranchCommitFiles`/`StashFiles` we look
/// up the commit by the drilled-in hash.
pub(super) fn resolve_current_commit<'a>(
    model: &'a Model,
    ctx_mgr: &ContextManager,
    commit_files_hash: &str,
) -> Option<&'a Commit> {
    let ctx = ctx_mgr.active();
    let sel = ctx_mgr.selected(ctx);
    match ctx {
        ContextId::Commits => model.commits.get(sel),
        ContextId::BranchCommits => model.sub_commits.get(sel),
        ContextId::Reflog => model.reflog_commits.get(sel),
        ContextId::CommitFiles | ContextId::BranchCommitFiles | ContextId::StashFiles => {
            if commit_files_hash.is_empty() {
                return None;
            }
            find_commit_by_hash(model, commit_files_hash)
        }
        _ => None,
    }
}

pub(super) fn find_commit_by_hash<'a>(model: &'a Model, hash: &str) -> Option<&'a Commit> {
    model
        .commits
        .iter()
        .find(|c| c.hash == hash)
        .or_else(|| model.sub_commits.iter().find(|c| c.hash == hash))
        .or_else(|| model.reflog_commits.iter().find(|c| c.hash == hash))
}

pub(super) fn render_commit_details_panel(
    frame: &mut Frame,
    rect: Rect,
    commit: &Commit,
    commit_stats: &Arc<Mutex<HashMap<String, CommitStat>>>,
    commit_messages: &Arc<Mutex<HashMap<String, String>>>,
    theme: &Theme,
    compact: bool,
    scroll: &mut u16,
) {
    let stat_owned = commit_stats
        .lock()
        .ok()
        .and_then(|map| map.get(&commit.hash).copied());
    let message_owned = commit_messages
        .lock()
        .ok()
        .and_then(|map| map.get(&commit.hash).cloned());
    presentation::commit_details::render_commit_details(
        frame,
        rect,
        commit,
        stat_owned.as_ref(),
        message_owned.as_deref(),
        theme,
        compact,
        scroll,
    );
}
