use std::collections::HashSet;

use anyhow::Result;

use super::GitCommands;
use crate::model::Tag;

impl GitCommands {
    pub fn load_tags(&self) -> Result<Vec<Tag>> {
        // Peel annotated tags to the commit they point at.
        let format = "%(refname:short)|%(if)%(*objectname)%(then)%(*objectname:short)%(else)%(objectname:short)%(end)|%(subject)";
        let result = self
            .git()
            .args(&[
                "for-each-ref",
                "--sort=-creatordate",
                &format!("--format={}", format),
                "refs/tags/",
            ])
            .run()?;

        if !result.success {
            return Ok(Vec::new());
        }

        let remote_tags = self.remote_tag_names();

        let tags = result
            .stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                if parts.len() >= 2 {
                    let name = parts[0].to_string();
                    let on_remote = remote_tags.contains(name.as_str());
                    Some(Tag {
                        name,
                        hash: parts[1].to_string(),
                        message: parts.get(2).unwrap_or(&"").to_string(),
                        on_remote,
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(tags)
    }

    /// Tag names known to exist on any configured remote (best-effort via ls-remote).
    ///
    /// Served from `remote_tags_cache` when warm. A cold cache queries every
    /// remote in parallel, each with a short wall-clock budget, so an offline
    /// remote costs one timeout rather than one per remote per refresh.
    fn remote_tag_names(&self) -> HashSet<String> {
        if let Some(cached) = self
            .remote_tags_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            return cached.clone();
        }

        let remotes = match self.git().args(&["remote"]).run() {
            Ok(r) if r.success => r
                .stdout
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>(),
            _ => return HashSet::new(),
        };

        let outputs: Vec<Option<String>> = std::thread::scope(|s| {
            let handles: Vec<_> = remotes
                .iter()
                .map(|remote| s.spawn(move || self.ls_remote_tags(remote)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().ok().flatten())
                .collect()
        });

        // Only cache a complete answer: if a remote timed out (offline), try
        // again on the next refresh rather than pinning "not on remote".
        let complete = outputs.iter().all(Option::is_some);
        let mut names = HashSet::new();
        for stdout in outputs.into_iter().flatten() {
            names.extend(parse_ls_remote_tags(&stdout));
        }
        if complete {
            *self
                .remote_tags_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(names.clone());
        }
        names
    }

    /// Drop the cached remote tag set; the next `load_tags` re-queries.
    /// Call after anything that can change what the remotes hold.
    pub(crate) fn invalidate_remote_tags(&self) {
        *self
            .remote_tags_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    fn ls_remote_tags(&self, remote: &str) -> Option<String> {
        const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
        match crate::os::cmd::CmdBuilder::git_no_optional_locks()
            .cwd_path(&self.repo_path)
            .args(&["ls-remote", "--tags", remote])
            .run_with_timeout(TIMEOUT)
        {
            Ok(Some(result)) if result.success => Some(result.stdout),
            _ => None,
        }
    }

    pub fn create_tag(&self, name: &str, message: &str) -> Result<()> {
        if message.is_empty() {
            self.git().args(&["tag", name]).run_expecting_success()?;
        } else {
            self.git()
                .args(&["tag", "-a", name, "-m", message])
                .run_expecting_success()?;
        }
        Ok(())
    }

    pub fn delete_tag(&self, name: &str) -> Result<()> {
        self.git()
            .args(&["tag", "-d", name])
            .run_expecting_success()?;
        Ok(())
    }

    pub fn delete_remote_tag(&self, remote: &str, name: &str) -> Result<()> {
        let refspec = format!("refs/tags/{}", name);
        self.git()
            .args(&["push", remote, "--delete", &refspec])
            .run_expecting_success()?;
        self.invalidate_remote_tags();
        Ok(())
    }

    pub fn push_tag(&self, name: &str) -> Result<()> {
        self.git()
            .args(&["push", "origin", name])
            .run_expecting_success()?;
        self.invalidate_remote_tags();
        Ok(())
    }
}

/// Tag names from `git ls-remote --tags` output
/// (`<hash>\trefs/tags/<name>` and peeled `...<name>^{}` lines).
fn parse_ls_remote_tags(stdout: &str) -> impl Iterator<Item = String> + '_ {
    stdout.lines().filter_map(|line| {
        let refname = line.split_whitespace().nth(1)?;
        let name = refname.strip_prefix("refs/tags/")?;
        let name = name.strip_suffix("^{}").unwrap_or(name);
        (!name.is_empty()).then(|| name.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::parse_ls_remote_tags;

    #[test]
    fn parses_plain_and_peeled_tags() {
        let out = "aaa\trefs/tags/v1.0\nbbb\trefs/tags/v1.0^{}\nccc\trefs/tags/v2\nddd\trefs/heads/main\n";
        let mut names: Vec<_> = parse_ls_remote_tags(out).collect();
        names.sort();
        names.dedup();
        assert_eq!(names, vec!["v1.0", "v2"]);
    }
}
