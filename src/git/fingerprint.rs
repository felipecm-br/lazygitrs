//! Cheap "did git state change?" check for the interval refresh.
//!
//! A full model reload spawns ~26 git processes. Most interval ticks find
//! nothing new outside the working tree, so the refresher first compares a
//! fingerprint built from `stat()` calls on the files git rewrites whenever
//! refs, HEAD, stashes, config, or an in-progress operation change. Only a
//! changed fingerprint pays for the full reload; otherwise just the Files scope
//! (status + diff stats) is refreshed. The index is deliberately left out:
//! staging rewrites it, and staged state is exactly what the Files scope
//! reloads, so including it would force a full reload after every stage.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use super::GitCommands;

/// Files in the per-worktree git dir.
const GIT_DIR_FILES: &[&str] = &[
    "HEAD",
    "logs/HEAD",
    "ORIG_HEAD",
    "FETCH_HEAD",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "BISECT_LOG",
    "rebase-merge",
    "rebase-apply",
];

/// Files in the common dir (shared by all worktrees).
const COMMON_DIR_FILES: &[&str] = &[
    "packed-refs",
    "FETCH_HEAD",
    "config",
    "logs/refs/stash",
    "worktrees",
];

/// Loose refs are walked so ref updates outside HEAD (`git branch -f`, a push
/// updating `refs/remotes/*`, new tags) are noticed. Past this many entries the
/// walk is not worth it and the caller falls back to full refreshes.
const MAX_REF_ENTRIES: usize = 5_000;

impl GitCommands {
    /// Fingerprint of git metadata, or `None` when it cannot be computed (no
    /// resolved git dir, or too many loose refs). `None` means "assume changed".
    pub fn refs_fingerprint(&self) -> Option<u64> {
        let (git_dir, common_dir) = self.git_dirs.as_ref()?;
        fingerprint(git_dir, common_dir)
    }
}

fn fingerprint(git_dir: &Path, common_dir: &Path) -> Option<u64> {
    let mut h = DefaultHasher::new();
    for name in GIT_DIR_FILES {
        stat_into(&mut h, &git_dir.join(name));
    }
    for name in COMMON_DIR_FILES {
        stat_into(&mut h, &common_dir.join(name));
    }
    let mut budget = MAX_REF_ENTRIES;
    walk_into(&mut h, &common_dir.join("refs"), &mut budget)?;
    Some(h.finish())
}

/// Hash path + existence + size + mtime (ns). A missing file hashes as such, so
/// e.g. `MERGE_HEAD` appearing or disappearing changes the fingerprint.
fn stat_into(h: &mut DefaultHasher, path: &Path) {
    path.hash(h);
    match std::fs::metadata(path) {
        Ok(meta) => {
            true.hash(h);
            meta.len().hash(h);
            if let Ok(modified) = meta.modified()
                && let Ok(since) = modified.duration_since(std::time::UNIX_EPOCH)
            {
                since.as_nanos().hash(h);
            }
        }
        Err(_) => false.hash(h),
    }
}

fn walk_into(h: &mut DefaultHasher, dir: &Path, budget: &mut usize) -> Option<()> {
    stat_into(h, dir);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Some(());
    };
    // read_dir order is unspecified; sort so the hash is stable.
    let mut paths: Vec<_> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        *budget = budget.checked_sub(1)?;
        if path.is_dir() {
            walk_into(h, &path, budget)?;
        } else {
            stat_into(h, &path);
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::fingerprint;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    struct Repo(PathBuf);
    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .expect("run git")
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo(name: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!(
            "lazygitrs-fp-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-qm", "init"]);
        Repo(dir)
    }

    fn fp(r: &Repo) -> u64 {
        let g = r.0.join(".git");
        fingerprint(&g, &g).expect("fingerprint")
    }

    #[test]
    fn stable_when_nothing_changes_and_ignores_worktree_edits() {
        let r = repo("stable");
        let before = fp(&r);
        assert_eq!(before, fp(&r));
        // Working-tree edits are covered by the Files-scope refresh instead.
        std::fs::write(r.0.join("a.txt"), "changed\n").unwrap();
        assert_eq!(before, fp(&r));
    }

    #[test]
    fn changes_on_commit_branch_and_stash() {
        let r = repo("changes");
        let mut last = fp(&r);
        let mut step = |args: &[&str]| {
            git(&r.0, args);
            let now = fp(&r);
            assert_ne!(now, last, "git {args:?} should change the fingerprint");
            last = now;
        };
        std::fs::write(r.0.join("a.txt"), "b\n").unwrap();
        step(&["commit", "-qam", "second"]);
        // Ref update outside HEAD: only refs/heads/other is written.
        step(&["branch", "other"]);
        step(&["branch", "-f", "other", "HEAD~1"]);
        std::fs::write(r.0.join("a.txt"), "stash me\n").unwrap();
        step(&["stash", "-q"]);
    }
}
