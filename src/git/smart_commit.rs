//! Smart multi-commit planner (`sgc`): split a dirty working tree into
//! atomic Conventional Commits with an AI, then commit the accepted groups.
//!
//! Pipeline:
//! 1. [`collect_changes`] reads `git status` + `git diff HEAD` + untracked
//!    contents. `.sgcignore` files keep their place in the plan but their
//!    *content* is withheld from the model (lockfiles, minified bundles).
//! 2. Each file's diff is compressed (metadata stripped, context trimmed,
//!    hunks capped) and the whole context is fitted to a byte budget by
//!    collapsing the largest files to a `+A/-D` summary.
//! 3. [`plan`] sends one prompt to the configured command, parses the JSON
//!    answer leniently and *repairs* it: hallucinated paths are dropped,
//!    directories expand to their files, duplicates keep their first group.
//!    Unassigned files or malformed titles trigger a corrective retry.
//! 4. Plans are cached by a content fingerprint so re-running on an
//!    unchanged tree is instant, and leftovers survive partial acceptance.
//! 5. [`commit_group`] commits each group with `git commit --only`, so
//!    anything the user had staged outside the group is left untouched.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, atomic::AtomicBool};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ai_commit::run_shell_command_cancellable;

/// Bumped whenever the prompt or plan format changes, invalidating caches.
const PLAN_FORMAT_VERSION: u32 = 2;
/// Changed lines kept per hunk before the rest is elided.
const MAX_CHANGED_LINES_PER_HUNK: usize = 80;
/// Bytes of an untracked file's content shown to the model.
const MAX_UNTRACKED_BYTES: usize = 3_000;
/// Default byte budget for the whole diff context in the prompt.
pub const DEFAULT_CONTEXT_BUDGET: usize = 48_000;
/// Attempts (first try + corrective retries) before giving up / falling back.
const MAX_ATTEMPTS: usize = 3;
/// Default subject length limit when commitlint does not set one.
const DEFAULT_HEADER_MAX: usize = 72;

const BUILTIN_CONTENT_IGNORES: &[&str] = &[
    "*-lock.*",
    "*.lock",
    "go.sum",
    "*.min.js",
    "*.min.css",
    "*.map",
];

const DEFAULT_TYPES: &[&str] = &[
    "feat", "fix", "refactor", "perf", "docs", "style", "test", "chore", "ci", "build", "revert",
];

// ── Options ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Granularity {
    Coarse,
    #[default]
    Medium,
    Fine,
}

impl Granularity {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "coarse" | "c" => Some(Self::Coarse),
            "medium" | "m" | "" => Some(Self::Medium),
            "fine" | "f" => Some(Self::Fine),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Coarse => "coarse",
            Self::Medium => "medium",
            Self::Fine => "fine",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Coarse => Self::Medium,
            Self::Medium => Self::Fine,
            Self::Fine => Self::Coarse,
        }
    }

    fn rule(self) -> &'static str {
        match self {
            Self::Coarse => {
                "GROUPING: coarse. Use as FEW commits as possible. Bundle loosely related changes \
                 (same feature area or subsystem). Split only clearly unrelated work, e.g. a bug \
                 fix mixed with a new feature."
            }
            Self::Medium => {
                "GROUPING: medium. One commit per feature, fix or concern. An implementation, its \
                 tests, its styles and its docs belong together. Never one commit per file unless \
                 every file is a distinct concern."
            }
            Self::Fine => {
                "GROUPING: fine. Maximize atomicity: each commit is exactly one logical concern. \
                 Prefer more, smaller commits. Mechanical changes (renames, formatting) go apart \
                 from behavioral ones."
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SmartCommitOptions {
    pub granularity: Granularity,
    pub emoji: bool,
    /// ISO 639-1 language code for the messages.
    pub lang: Option<String>,
    /// Ignore a cached plan and ask the model again.
    pub force: bool,
    /// Extra free-form guidance appended to the prompt (e.g. "scope is tui").
    pub hint: Option<String>,
    /// Byte budget for diff context; `0` means [`DEFAULT_CONTEXT_BUDGET`].
    pub context_budget: usize,
}

impl SmartCommitOptions {
    fn budget(&self) -> usize {
        if self.context_budget == 0 {
            DEFAULT_CONTEXT_BUDGET
        } else {
            self.context_budget
        }
    }

    fn cache_key(&self) -> String {
        format!(
            "g={};e={};l={};h={}",
            self.granularity.as_str(),
            self.emoji,
            self.lang.as_deref().unwrap_or(""),
            self.hint.as_deref().unwrap_or("")
        )
    }
}

// ── Change collection ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChanged,
    Untracked,
}

impl ChangeKind {
    pub fn letter(&self) -> char {
        match self {
            Self::Added => 'A',
            Self::Modified => 'M',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::TypeChanged => 'T',
            Self::Untracked => '?',
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    /// Source path of a rename/copy; must be committed together with `path`.
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    pub added: usize,
    pub deleted: usize,
    pub binary: bool,
    /// Content withheld from the model by `.sgcignore`.
    pub content_ignored: bool,
}

/// Everything the planner knows about the working tree.
pub struct ChangeSet {
    pub root: PathBuf,
    pub files: Vec<ChangedFile>,
    /// Compressed per-file context in `files` order (diff or new content).
    sections: Vec<String>,
    /// Content-only fingerprint (options are mixed in by the caller).
    fingerprint: u64,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Collect every staged, unstaged and untracked change under `repo_path`.
pub fn collect_changes(repo_path: &Path) -> Result<ChangeSet> {
    let root = PathBuf::from(
        git_output(repo_path, &["rev-parse", "--show-toplevel"])?
            .trim()
            .to_string(),
    );
    let ignore = IgnoreRules::load(&root);

    let status = git_output_raw(
        &root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--find-renames",
        ],
    )?;
    let entries = parse_status_z(&status)?;
    if entries.is_empty() {
        return Ok(ChangeSet {
            root,
            files: Vec::new(),
            sections: Vec::new(),
            fingerprint: 0,
        });
    }

    let has_head = git_output(&root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok();
    let diff_text = if has_head {
        git_output(
            &root,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "HEAD",
                "--no-color",
                "--no-ext-diff",
                "--find-renames",
                "--src-prefix=a/",
                "--dst-prefix=b/",
            ],
        )?
    } else {
        // Unborn branch: everything staged is new; unstaged edits layer on top.
        let mut text = git_output(
            &root,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--cached",
                "--no-color",
                "--no-ext-diff",
                "--src-prefix=a/",
                "--dst-prefix=b/",
            ],
        )?;
        text.push_str(&git_output(
            &root,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--src-prefix=a/",
                "--dst-prefix=b/",
            ],
        )?);
        text
    };
    let mut diff_by_path: HashMap<String, String> = HashMap::new();
    for (path, section) in split_diff_by_file(&diff_text) {
        diff_by_path.entry(path).or_default().push_str(&section);
    }

    let mut hasher = Fnv64::new();
    hasher.write(&status);
    hasher.write(diff_text.as_bytes());

    let mut files = Vec::with_capacity(entries.len());
    let mut sections = Vec::with_capacity(entries.len());
    for entry in entries {
        let content_ignored = ignore.is_ignored(&entry.path);
        let mut file = ChangedFile {
            path: entry.path.clone(),
            old_path: entry.old_path.clone(),
            kind: entry.kind.clone(),
            added: 0,
            deleted: 0,
            binary: false,
            content_ignored,
        };

        let section = if entry.kind == ChangeKind::Untracked {
            let bytes = std::fs::read(root.join(&entry.path)).unwrap_or_default();
            hasher.write(entry.path.as_bytes());
            hasher.write(&bytes);
            file.binary = bytes.contains(&0);
            if !file.binary {
                file.added = String::from_utf8_lossy(&bytes).lines().count();
            }
            if content_ignored || file.binary {
                String::new()
            } else {
                truncate_content(&String::from_utf8_lossy(&bytes), MAX_UNTRACKED_BYTES)
            }
        } else if let Some(raw) = diff_by_path.get(&entry.path) {
            let (added, deleted, binary) = count_diff_lines(raw);
            file.added = added;
            file.deleted = deleted;
            file.binary = binary;
            if content_ignored || binary {
                String::new()
            } else {
                compress_diff(raw)
            }
        } else {
            String::new()
        };

        files.push(file);
        sections.push(section);
    }

    Ok(ChangeSet {
        root,
        files,
        sections,
        fingerprint: hasher.finish(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusEntry {
    path: String,
    old_path: Option<String>,
    kind: ChangeKind,
}

fn parse_status_z(raw: &[u8]) -> Result<Vec<StatusEntry>> {
    let mut parts = raw
        .split(|b| *b == 0)
        .map(|p| String::from_utf8_lossy(p).into_owned());
    let mut entries = Vec::new();
    while let Some(part) = parts.next() {
        if part.len() < 4 {
            continue;
        }
        let (x, y) = (part.as_bytes()[0], part.as_bytes()[1]);
        let path = part[3..].to_string();
        if x == b'!' {
            continue;
        }
        let unmerged =
            x == b'U' || y == b'U' || (x == b'A' && y == b'A') || (x == b'D' && y == b'D');
        if unmerged {
            bail!("'{path}' has merge conflicts. Resolve them before smart commit");
        }
        let old_path = if x == b'R' || x == b'C' {
            parts.next()
        } else {
            None
        };
        let kind = match (x, y) {
            (b'?', b'?') => ChangeKind::Untracked,
            (b'R', _) | (b'C', _) => ChangeKind::Renamed,
            (b'D', _) | (_, b'D') => ChangeKind::Deleted,
            (b'A', _) => ChangeKind::Added,
            (b'T', _) | (_, b'T') => ChangeKind::TypeChanged,
            _ => ChangeKind::Modified,
        };
        entries.push(StatusEntry {
            path,
            old_path,
            kind,
        });
    }
    Ok(entries)
}

/// Split a unified diff into `(new_path, section)` pairs.
fn split_diff_by_file(diff: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in diff.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(done) = current.take() {
                out.push(done);
            }
            let path = header_path(rest.trim_end_matches('\n')).unwrap_or_default();
            current = Some((path, String::new()));
        }
        if let Some((path, body)) = current.as_mut() {
            if let Some(to) = line.strip_prefix("rename to ") {
                *path = to.trim_end_matches('\n').to_string();
            } else if let Some(to) = line.strip_prefix("+++ b/") {
                *path = to.trim_end_matches('\n').to_string();
            }
            body.push_str(line);
        }
    }
    if let Some(done) = current {
        out.push(done);
    }
    out
}

/// Path from `a/X b/X` (the unambiguous same-path form).
fn header_path(rest: &str) -> Option<String> {
    let rest = rest.strip_prefix("a/")?;
    let len = rest.len();
    if len < 4 || !(len - 3).is_multiple_of(2) {
        return rest.rsplit_once(" b/").map(|(_, b)| b.to_string());
    }
    let n = (len - 3) / 2;
    let (a, b) = (&rest[..n], &rest[n..]);
    if b.strip_prefix(" b/") == Some(a) {
        Some(a.to_string())
    } else {
        rest.rsplit_once(" b/").map(|(_, b)| b.to_string())
    }
}

fn count_diff_lines(section: &str) -> (usize, usize, bool) {
    let (mut added, mut deleted, mut binary) = (0, 0, false);
    let mut in_hunk = false;
    for line in section.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
        } else if line.starts_with("diff --git ") {
            in_hunk = false;
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            binary = true;
        } else if in_hunk && line.starts_with('+') {
            added += 1;
        } else if in_hunk && line.starts_with('-') {
            deleted += 1;
        }
    }
    (added, deleted, binary)
}

/// Shrink one file's diff while keeping everything a commit message needs.
///
/// Drops `diff --git`/`index`/mode/`---`/`+++` metadata (the file header in
/// the prompt carries path and status), keeps every `+`/`-` line up to
/// [`MAX_CHANGED_LINES_PER_HUNK`] per hunk, and keeps only the first context
/// line of each unchanged run so the model still sees where code moved.
pub fn compress_diff(section: &str) -> String {
    let mut out = String::new();
    let mut in_hunk = false;
    let mut changed = 0usize;
    let mut skipped = 0usize;
    let mut ctx_streak = 0usize;

    let flush = |out: &mut String, skipped: &mut usize| {
        if *skipped > 0 {
            let _ = writeln!(out, " [... {skipped} more changed lines elided ...]");
            *skipped = 0;
        }
    };

    for line in section.lines() {
        if line.starts_with("diff --git ") {
            flush(&mut out, &mut skipped);
            in_hunk = false;
            continue;
        }
        if line.starts_with("@@") {
            flush(&mut out, &mut skipped);
            in_hunk = true;
            changed = 0;
            ctx_streak = 0;
            // Keep the function-context part of the header; drop line numbers.
            let label = line
                .splitn(3, "@@")
                .nth(2)
                .map(str::trim)
                .unwrap_or_default();
            if label.is_empty() {
                out.push_str("@@\n");
            } else {
                let _ = writeln!(out, "@@ {label}");
            }
            continue;
        }
        if !in_hunk {
            // Metadata: index, modes, ---/+++, similarity. Rename info is in the header.
            continue;
        }
        if line.starts_with('\\') {
            continue; // "\ No newline at end of file"
        }
        if line.starts_with('+') || line.starts_with('-') {
            ctx_streak = 0;
            changed += 1;
            if changed > MAX_CHANGED_LINES_PER_HUNK {
                skipped += 1;
            } else {
                out.push_str(line);
                out.push('\n');
            }
        } else {
            ctx_streak += 1;
            if ctx_streak == 1 && changed <= MAX_CHANGED_LINES_PER_HUNK {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    flush(&mut out, &mut skipped);
    out
}

fn truncate_content(content: &str, max: usize) -> String {
    if content.len() <= max {
        return content.to_string();
    }
    let mut end = max;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    let cut = &content[..end];
    let cut = cut.rsplit_once('\n').map(|(head, _)| head).unwrap_or(cut);
    let remaining = content[cut.len()..].lines().count();
    format!("{cut}\n[... {remaining} more lines elided ...]\n")
}

// ── .sgcignore ─────────────────────────────────────────────────────────────

/// gitignore-flavored rules: last match wins, `!` re-includes, a pattern
/// without `/` matches any path segment, a leading `/` anchors to the root.
#[derive(Debug, Default)]
pub struct IgnoreRules {
    rules: Vec<(String, bool)>,
}

impl IgnoreRules {
    pub fn load(root: &Path) -> Self {
        let mut text = BUILTIN_CONTENT_IGNORES.join("\n");
        if let Some(home) = dirs::home_dir()
            && let Ok(global) = std::fs::read_to_string(home.join(".sgcignore"))
        {
            text.push('\n');
            text.push_str(&global);
        }
        if let Ok(local) = std::fs::read_to_string(root.join(".sgcignore")) {
            text.push('\n');
            text.push_str(&local);
        }
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Self {
        let rules = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| match l.strip_prefix('!') {
                Some(p) => (p.to_string(), false),
                None => (l.to_string(), true),
            })
            .collect();
        Self { rules }
    }

    pub fn is_ignored(&self, path: &str) -> bool {
        let mut ignored = false;
        for (pattern, is_ignore) in &self.rules {
            if pattern_matches(pattern, path) {
                ignored = *is_ignore;
            }
        }
        ignored
    }
}

fn pattern_matches(pattern: &str, path: &str) -> bool {
    let dir_only = pattern.ends_with('/');
    let pattern = pattern.trim_end_matches('/');
    if pattern.is_empty() {
        return false;
    }
    let segments: Vec<&str> = path.split('/').collect();

    if let Some(anchored) = pattern.strip_prefix('/') {
        return prefix_matches(anchored, &segments, dir_only);
    }
    if !pattern.contains('/') {
        // Match any segment; a directory segment covers everything below it.
        let last = segments.len().saturating_sub(1);
        return segments
            .iter()
            .enumerate()
            .any(|(i, seg)| (!dir_only || i < last) && glob_match(pattern, seg));
    }
    prefix_matches(pattern, &segments, dir_only)
}

/// Does `pattern` match `path` itself or one of its parent directories?
fn prefix_matches(pattern: &str, segments: &[&str], dir_only: bool) -> bool {
    let end = if dir_only {
        segments.len().saturating_sub(1)
    } else {
        segments.len()
    };
    (1..=end).any(|n| glob_match(pattern, &segments[..n].join("/")))
}

/// `*` and `?` stop at `/`, `**` crosses it, `[a-z]` / `[!x]` are classes.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_inner(&p, &t)
}

fn glob_inner(p: &[char], t: &[char]) -> bool {
    match p.first() {
        None => t.is_empty(),
        Some('*') if p.get(1) == Some(&'*') => {
            let mut rest = &p[2..];
            if rest.first() == Some(&'/') {
                // `**/x` also matches `x` at the top level.
                if glob_inner(&rest[1..], t) {
                    return true;
                }
                rest = &p[2..];
            }
            (0..=t.len()).any(|i| glob_inner(rest, &t[i..]))
        }
        Some('*') => {
            let rest = &p[1..];
            for i in 0..=t.len() {
                if glob_inner(rest, &t[i..]) {
                    return true;
                }
                if t.get(i) == Some(&'/') {
                    break;
                }
            }
            false
        }
        Some('?') => t.first().is_some_and(|c| *c != '/') && glob_inner(&p[1..], &t[1..]),
        Some('[') => {
            let Some(c) = t.first() else {
                return false;
            };
            match match_class(&p[1..], *c) {
                Some((matched, consumed)) => matched && glob_inner(&p[1 + consumed..], &t[1..]),
                None => *c == '[' && glob_inner(&p[1..], &t[1..]),
            }
        }
        Some('\\') if p.len() > 1 => t.first() == Some(&p[1]) && glob_inner(&p[2..], &t[1..]),
        Some(pc) => t.first() == Some(pc) && glob_inner(&p[1..], &t[1..]),
    }
}

/// Returns `(matched, chars consumed including the closing ']')`.
fn match_class(p: &[char], c: char) -> Option<(bool, usize)> {
    let mut i = 0;
    let negate = matches!(p.first(), Some('!') | Some('^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < p.len() {
        if p[i] == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            if p[i] <= c && c <= p[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

// ── commitlint ─────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CommitlintRules {
    pub types: Option<Vec<String>>,
    pub forbidden_types: Vec<String>,
    pub scopes: Option<Vec<String>>,
    pub header_max: Option<usize>,
    pub subject_case: Option<String>,
    pub subject_case_never: Vec<String>,
    pub scope_case: Option<String>,
}

impl CommitlintRules {
    pub fn load(root: &Path) -> Self {
        const CANDIDATES: &[&str] = &[
            ".commitlintrc",
            ".commitlintrc.json",
            ".commitlintrc.yml",
            ".commitlintrc.yaml",
            ".commitlintrc.js",
            ".commitlintrc.cjs",
            ".commitlintrc.mjs",
            ".commitlintrc.ts",
            "commitlint.config.js",
            "commitlint.config.cjs",
            "commitlint.config.mjs",
            "commitlint.config.ts",
        ];
        for name in CANDIDATES {
            let Ok(text) = std::fs::read_to_string(root.join(name)) else {
                continue;
            };
            let is_js = [".js", ".cjs", ".mjs", ".ts"]
                .iter()
                .any(|ext| name.ends_with(ext));
            let value = if is_js {
                js_rules_object(&text).map(|rules| serde_json::json!({ "rules": rules }))
            } else {
                serde_json::from_str::<Value>(&text)
                    .ok()
                    .or_else(|| serde_yaml::from_str::<Value>(&text).ok())
            };
            return value.map(|v| Self::from_config(&v)).unwrap_or_default();
        }
        if let Ok(text) = std::fs::read_to_string(root.join("package.json"))
            && let Ok(pkg) = serde_json::from_str::<Value>(&text)
            && let Some(cfg) = pkg.get("commitlint")
        {
            return Self::from_config(cfg);
        }
        Self::default()
    }

    fn from_config(config: &Value) -> Self {
        let mut out = Self::default();
        let Some(rules) = config.get("rules").and_then(Value::as_object) else {
            return out;
        };
        // Rule shape: [level, "always" | "never", value]; level 0 disables it.
        let rule = |name: &str| -> Option<(String, Value)> {
            let arr = rules.get(name)?.as_array()?;
            if arr.first()?.as_u64()? == 0 {
                return None;
            }
            let cond = arr
                .get(1)
                .and_then(Value::as_str)
                .unwrap_or("always")
                .to_string();
            Some((cond, arr.get(2).cloned().unwrap_or(Value::Null)))
        };
        let strings = |v: &Value| -> Vec<String> {
            match v {
                Value::String(s) => vec![s.clone()],
                Value::Array(items) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect(),
                _ => Vec::new(),
            }
        };

        if let Some((cond, value)) = rule("type-enum") {
            let list = strings(&value);
            if !list.is_empty() {
                if cond == "never" {
                    out.forbidden_types = list;
                } else {
                    out.types = Some(list);
                }
            }
        }
        if let Some((cond, value)) = rule("scope-enum") {
            let list = strings(&value);
            if cond != "never" && !list.is_empty() {
                out.scopes = Some(list);
            }
        }
        for name in ["header-max-length", "subject-max-length"] {
            if let Some((_, value)) = rule(name)
                && let Some(n) = value.as_u64()
            {
                out.header_max = Some(n as usize);
                break;
            }
        }
        if let Some((cond, value)) = rule("subject-case") {
            let list = strings(&value);
            if cond == "never" {
                out.subject_case_never = list;
            } else {
                out.subject_case = list.into_iter().next();
            }
        }
        if let Some((cond, value)) = rule("scope-case")
            && cond != "never"
        {
            out.scope_case = strings(&value).into_iter().next();
        }
        out
    }

    pub fn header_max(&self) -> usize {
        self.header_max.unwrap_or(DEFAULT_HEADER_MAX)
    }

    fn allowed_types(&self) -> Vec<String> {
        let base: Vec<String> = match &self.types {
            Some(types) => types.clone(),
            None => DEFAULT_TYPES.iter().map(|s| s.to_string()).collect(),
        };
        base.into_iter()
            .filter(|t| !self.forbidden_types.contains(t))
            .collect()
    }

    fn prompt_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(scopes) = &self.scopes {
            lines.push(format!(
                "- Allowed scopes (commitlint): {}",
                scopes.join(", ")
            ));
        }
        if let Some(case) = &self.subject_case {
            lines.push(format!("- Subject must be {case} (commitlint)"));
        }
        if !self.subject_case_never.is_empty() {
            lines.push(format!(
                "- Subject must NOT be {} (commitlint)",
                self.subject_case_never.join(", ")
            ));
        }
        if let Some(case) = &self.scope_case {
            lines.push(format!("- Scope must be {case} (commitlint)"));
        }
        lines
    }
}

/// Remove `//` and `/* */` comments outside string literals.
fn strip_js_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match (c, chars.get(i + 1)) {
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            ('\'' | '"' | '`', _) => {
                quote = Some(c);
                out.push(c);
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Pull the `rules: { ... }` object out of a JS/TS commitlint config and
/// coerce it to JSON (quote keys, single to double quotes, trailing commas).
fn js_rules_object(source: &str) -> Option<Value> {
    let without_comments = strip_js_comments(source);
    let start = without_comments.find("rules")?;
    let after = &without_comments[start + "rules".len()..];
    let brace = after.find('{')?;
    if !after[..brace]
        .trim()
        .trim_start_matches(':')
        .trim()
        .is_empty()
    {
        return None;
    }
    let body = &after[brace..];
    let mut depth = 0usize;
    let mut end = None;
    for (i, ch) in body.char_indices() {
        match ch {
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let object = &body[..end?];

    let mut json = String::with_capacity(object.len() + 32);
    let chars: Vec<char> = object.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\'' || ch == '"' {
            let quote = ch;
            json.push('"');
            i += 1;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '"' {
                    json.push('\\');
                }
                json.push(chars[i]);
                i += 1;
            }
            json.push('"');
            i += 1;
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let mut word = String::new();
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || "_-".contains(chars[i])) {
                word.push(chars[i]);
                i += 1;
            }
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if chars.get(j) == Some(&':') {
                let _ = write!(json, "\"{word}\"");
            } else {
                json.push_str(&word);
            }
            continue;
        }
        json.push(ch);
        i += 1;
    }
    let mut cleaned = String::with_capacity(json.len());
    let jchars: Vec<char> = json.chars().collect();
    for (idx, ch) in jchars.iter().enumerate() {
        if *ch == ',' {
            let next = jchars[idx + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        cleaned.push(*ch);
    }
    serde_json::from_str(&cleaned).ok()
}

// ── Prompt ─────────────────────────────────────────────────────────────────

/// Build the context block: file index plus per-file compressed diffs,
/// collapsing the largest sections to a summary until it fits `budget`.
fn build_context(changes: &ChangeSet, budget: usize) -> String {
    let mut sections: Vec<Option<&str>> = changes
        .sections
        .iter()
        .map(|s| (!s.is_empty()).then_some(s.as_str()))
        .collect();
    let mut total: usize = sections.iter().flatten().map(|s| s.len()).sum();
    if total > budget {
        let mut order: Vec<usize> = (0..sections.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(sections[i].map_or(0, str::len)));
        for i in order {
            if total <= budget {
                break;
            }
            if let Some(s) = sections[i].take() {
                total -= s.len();
            }
        }
    }

    let mut out = String::new();
    out.push_str("## Changed files\n");
    for (i, file) in changes.files.iter().enumerate() {
        let _ = write!(out, "{}. [{}] {}", i + 1, file.kind.letter(), file.path);
        if let Some(old) = &file.old_path {
            let _ = write!(out, " (renamed from {old})");
        }
        let _ = write!(out, "  +{} -{}", file.added, file.deleted);
        if file.binary {
            out.push_str("  [binary]");
        }
        if file.content_ignored {
            out.push_str("  [generated/lockfile: content omitted]");
        }
        out.push('\n');
    }

    out.push_str("\n## Diffs\n");
    for (i, file) in changes.files.iter().enumerate() {
        match sections[i] {
            Some(section) => {
                let label = if file.kind == ChangeKind::Untracked {
                    "new file"
                } else {
                    "diff"
                };
                let _ = writeln!(out, "\n### {} ({label})\n{}", file.path, section.trim_end());
            }
            None if !changes.sections[i].is_empty() => {
                let _ = writeln!(
                    out,
                    "\n### {} (+{} -{}; content elided to fit the prompt budget)",
                    file.path, file.added, file.deleted
                );
            }
            None => {}
        }
    }
    out
}

fn build_prompt(
    changes: &ChangeSet,
    options: &SmartCommitOptions,
    commitlint: &CommitlintRules,
) -> String {
    let types = commitlint.allowed_types().join(", ");
    let header_max = commitlint.header_max();
    let mut rules = vec![
        format!("- Title: <type>(<scope>): <summary>. Types: {types}."),
        format!(
            "- Title is imperative mood, lowercase after the colon, no trailing period, at most {header_max} characters."
        ),
        "- Scope is a short module/area name taken from the paths (optional).".to_string(),
        "- Body: 1-4 '-' bullet lines explaining what changed and WHY, wrapped at 72 columns. Empty string for trivial commits.".to_string(),
        "- Every changed file listed above appears in exactly one commit, including lockfiles and binaries. Use the exact paths.".to_string(),
        "- A lockfile or generated file goes with the change that produced it (e.g. Cargo.lock with Cargo.toml).".to_string(),
        "- Order commits so each one builds on the previous: foundations (build, refactor) before the features that use them.".to_string(),
        "- Never use breaking-change markers or trailers unless the diff clearly removes a public API.".to_string(),
        format!("- {}", options.granularity.rule()),
    ];
    rules.extend(commitlint.prompt_lines());
    if let Some(lang) = options.lang.as_deref().filter(|l| !l.trim().is_empty()) {
        rules.push(format!(
            "- Write titles and bodies in the language with ISO 639-1 code '{lang}'. Keep the type and scope in English."
        ));
    }
    if options.emoji {
        rules.push(
            "- Prefix each title with one gitmoji matching its type: feat ✨, fix 🐛, refactor ♻️, perf ⚡️, docs 📝, style 🎨, test ✅, chore 🔧, ci 👷, build 📦, revert ⏪️. Format: '<emoji> <type>(<scope>): <summary>'."
                .to_string(),
        );
    }
    if let Some(hint) = options.hint.as_deref().filter(|h| !h.trim().is_empty()) {
        rules.push(format!("- Author guidance: {}", hint.trim()));
    }

    format!(
        "You are a senior engineer splitting a working tree into clean, atomic Conventional Commits.\n\
         \n\
         {context}\n\
         ## Rules\n\
         {rules}\n\
         \n\
         ## Output\n\
         Output ONLY a JSON array, no prose, no markdown fences. Each element:\n\
         {{\"title\": \"type(scope): summary\", \"body\": \"- why 1\\n- why 2\", \"files\": [\"exact/path\"]}}\n",
        context = build_context(changes, options.budget()),
        rules = rules.join("\n"),
    )
}

// ── Plan parsing and repair ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartCommitGroup {
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub files: Vec<String>,
    /// Synthesized for files the model never assigned.
    #[serde(default)]
    pub fallback: bool,
}

impl SmartCommitGroup {
    pub fn message(&self) -> String {
        let body = self.body.trim();
        if body.is_empty() {
            self.title.trim().to_string()
        } else {
            format!("{}\n\n{}", self.title.trim(), body)
        }
    }

    /// Replace title/body from an edited full commit message.
    pub fn set_message(&mut self, message: &str) {
        let message = message.trim();
        match message.split_once('\n') {
            Some((title, body)) => {
                self.title = title.trim().to_string();
                self.body = body.trim().to_string();
            }
            None => {
                self.title = message.to_string();
                self.body.clear();
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SmartCommitPlan {
    pub groups: Vec<SmartCommitGroup>,
    /// Per-path metadata for display (status letter, +/- counts).
    pub files: BTreeMap<String, ChangedFile>,
    pub from_cache: bool,
    /// Non-fatal issues the model left after all retries.
    pub warnings: Vec<String>,
    /// Model attempts used (0 when served from cache).
    pub attempts: usize,
}

#[derive(Debug, Deserialize)]
struct RawGroup {
    #[serde(default)]
    title: Option<String>,
    #[serde(default, alias = "description")]
    body: Option<Value>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default, alias = "paths")]
    files: Vec<Value>,
}

/// Find the outermost JSON array (or `{"commits": [...]}` object) in `raw`.
fn extract_json(raw: &str) -> Option<Vec<RawGroup>> {
    let parse_value = |v: Value| -> Option<Vec<RawGroup>> {
        let arr = match v {
            Value::Array(_) => v,
            Value::Object(mut map) => ["commits", "groups", "plan"]
                .iter()
                .find_map(|k| map.remove(*k))?,
            _ => return None,
        };
        serde_json::from_value(arr).ok()
    };

    for (open, close) in [('[', ']'), ('{', '}')] {
        let bytes: Vec<(usize, char)> = raw.char_indices().collect();
        for (start_idx, &(start, ch)) in bytes.iter().enumerate() {
            if ch != open {
                continue;
            }
            let mut depth = 0i32;
            let mut in_str = false;
            let mut escaped = false;
            for &(pos, c) in &bytes[start_idx..] {
                if in_str {
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        in_str = false;
                    }
                    continue;
                }
                match c {
                    '"' => in_str = true,
                    c if c == open => depth += 1,
                    c if c == close => {
                        depth -= 1;
                        if depth == 0 {
                            let candidate = &raw[start..pos + c.len_utf8()];
                            for text in [candidate.to_string(), candidate.replace("\\\"", "\"")] {
                                if let Ok(v) = serde_json::from_str::<Value>(&text)
                                    && let Some(groups) = parse_value(v)
                                    && !groups.is_empty()
                                {
                                    return Some(groups);
                                }
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

fn clean_title(title: &str) -> String {
    let mut t = title
        .trim()
        .trim_matches('`')
        .trim_matches('"')
        .trim()
        .to_string();
    if let Some(rest) = t
        .strip_prefix("Title:")
        .or_else(|| t.strip_prefix("title:"))
    {
        t = rest.trim().to_string();
    }
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    t.trim_end_matches('.').to_string()
}

fn clean_body(body: &str) -> String {
    body.lines()
        .filter(|l| {
            let lower = l.trim_start().to_ascii_lowercase();
            !(lower.starts_with("co-authored-by:")
                || lower.starts_with("signed-off-by:")
                || lower.starts_with("```"))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Is `title` `[emoji ]type[(scope)][!]: summary` with an allowed type?
fn title_issue(title: &str, commitlint: &CommitlintRules, emoji: bool) -> Option<String> {
    let mut rest = title;
    if emoji
        && let Some((first, tail)) = rest.split_once(' ')
        && !first.chars().any(|c| c.is_ascii_alphanumeric())
    {
        rest = tail;
    }
    let Some((head, summary)) = rest.split_once(": ") else {
        return Some(format!("'{title}' is not '<type>(<scope>): <summary>'"));
    };
    let kind = head
        .split('(')
        .next()
        .unwrap_or_default()
        .trim_end_matches('!');
    let allowed = commitlint.allowed_types();
    if !allowed.iter().any(|t| t == kind) {
        return Some(format!(
            "'{title}' uses type '{kind}', allowed: {}",
            allowed.join(", ")
        ));
    }
    if summary.trim().is_empty() {
        return Some(format!("'{title}' has an empty summary"));
    }
    let max = commitlint.header_max();
    if title.chars().count() > max {
        return Some(format!("'{title}' is longer than {max} characters"));
    }
    None
}

struct Repaired {
    groups: Vec<SmartCommitGroup>,
    unassigned: Vec<String>,
    issues: Vec<String>,
}

/// Map the model's answer onto real paths and report what still needs fixing.
fn repair_plan(
    raw: Vec<RawGroup>,
    changes: &ChangeSet,
    commitlint: &CommitlintRules,
    emoji: bool,
) -> Repaired {
    let known: Vec<&str> = changes.files.iter().map(|f| f.path.as_str()).collect();
    let known_set: HashSet<&str> = known.iter().copied().collect();
    // Renames may be referenced by their old path.
    let by_old: HashMap<&str, &str> = changes
        .files
        .iter()
        .filter_map(|f| f.old_path.as_deref().map(|o| (o, f.path.as_str())))
        .collect();

    let mut assigned: HashSet<String> = HashSet::new();
    let mut groups = Vec::new();
    let mut issues = Vec::new();

    for item in raw {
        let (mut title, mut body) = match (&item.title, &item.message) {
            (Some(title), _) => (title.clone(), String::new()),
            (None, Some(message)) => match message.split_once('\n') {
                Some((t, b)) => (t.to_string(), b.to_string()),
                None => (message.clone(), String::new()),
            },
            (None, None) => (String::new(), String::new()),
        };
        match &item.body {
            Some(Value::String(b)) => body = b.clone(),
            Some(Value::Array(lines)) => {
                body = lines
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|l| {
                        if l.trim_start().starts_with('-') {
                            l.to_string()
                        } else {
                            format!("- {l}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            _ => {}
        }
        title = clean_title(&title);
        body = clean_body(&body);

        let mut files = Vec::new();
        for value in &item.files {
            let Some(raw_path) = value.as_str() else {
                continue;
            };
            let raw_path = raw_path.trim().trim_matches('`');
            let mut candidates = vec![raw_path.to_string()];
            for prefix in ["./", "a/", "b/", "/"] {
                if let Some(stripped) = raw_path.strip_prefix(prefix) {
                    candidates.push(stripped.to_string());
                }
            }
            let resolved: Vec<String> = candidates
                .iter()
                .find_map(|c| {
                    if known_set.contains(c.as_str()) {
                        Some(vec![c.clone()])
                    } else if let Some(new) = by_old.get(c.as_str()) {
                        Some(vec![(*new).to_string()])
                    } else {
                        // Directory reference: expand to unassigned files below it.
                        let dir = format!("{}/", c.trim_end_matches('/'));
                        let below: Vec<String> = known
                            .iter()
                            .filter(|k| k.starts_with(&dir))
                            .map(|k| k.to_string())
                            .collect();
                        (!below.is_empty()).then_some(below)
                    }
                })
                .unwrap_or_default();
            if resolved.is_empty() {
                issues.push(format!("'{raw_path}' is not a changed file"));
            }
            for path in resolved {
                if assigned.insert(path.clone()) {
                    files.push(path);
                }
            }
        }
        if files.is_empty() {
            continue;
        }
        if title.is_empty() {
            issues.push(format!("a commit for {} has no title", files.join(", ")));
            title = "chore: update files".to_string();
        } else if let Some(issue) = title_issue(&title, commitlint, emoji) {
            issues.push(issue);
        }
        groups.push(SmartCommitGroup {
            title,
            body,
            files,
            fallback: false,
        });
    }

    let unassigned: Vec<String> = known
        .iter()
        .filter(|k| !assigned.contains(**k))
        .map(|k| k.to_string())
        .collect();
    if !unassigned.is_empty() {
        issues.push(format!(
            "files not assigned to any commit: {}",
            unassigned.join(", ")
        ));
    }
    Repaired {
        groups,
        unassigned,
        issues,
    }
}

// ── Cache ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    fingerprint: String,
    groups: Vec<SmartCommitGroup>,
}

fn cache_path(root: &Path) -> Option<PathBuf> {
    let mut h = Fnv64::new();
    h.write(root.to_string_lossy().as_bytes());
    Some(
        dirs::cache_dir()?
            .join("lazygitrs")
            .join("sgc")
            .join(format!("{:016x}.json", h.finish())),
    )
}

fn plan_fingerprint(
    changes: &ChangeSet,
    options: &SmartCommitOptions,
    commitlint: &CommitlintRules,
    command: &str,
) -> String {
    let mut h = Fnv64::new();
    h.write(&changes.fingerprint.to_le_bytes());
    h.write(&PLAN_FORMAT_VERSION.to_le_bytes());
    h.write(options.cache_key().as_bytes());
    h.write(format!("{commitlint:?}").as_bytes());
    h.write(command.as_bytes());
    format!("{:016x}", h.finish())
}

fn read_cache(root: &Path, fingerprint: &str) -> Option<Vec<SmartCommitGroup>> {
    let text = std::fs::read_to_string(cache_path(root)?).ok()?;
    let cache: CacheFile = serde_json::from_str(&text).ok()?;
    (cache.version == PLAN_FORMAT_VERSION
        && cache.fingerprint == fingerprint
        && !cache.groups.is_empty())
    .then_some(cache.groups)
}

fn write_cache(root: &Path, fingerprint: &str, groups: &[SmartCommitGroup]) {
    let Some(path) = cache_path(root) else {
        return;
    };
    if groups.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let cache = CacheFile {
        version: PLAN_FORMAT_VERSION,
        fingerprint: fingerprint.to_string(),
        groups: groups.to_vec(),
    };
    if let Ok(text) = serde_json::to_string_pretty(&cache) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

/// Drop a cached plan for this repo (e.g. after the user rejects it).
pub fn clear_cache(repo_path: &Path) {
    if let Ok(root) = git_output(repo_path, &["rev-parse", "--show-toplevel"])
        && let Some(path) = cache_path(Path::new(root.trim()))
    {
        let _ = std::fs::remove_file(path);
    }
}

/// Store the groups the user did not commit so the next run reuses them.
/// The fingerprint is recomputed against the post-commit working tree.
pub fn save_remaining(
    repo_path: &Path,
    command: &str,
    options: &SmartCommitOptions,
    remaining: &[SmartCommitGroup],
) -> Result<()> {
    let changes = collect_changes(repo_path)?;
    let still_changed: HashSet<&str> = changes.files.iter().map(|f| f.path.as_str()).collect();
    let remaining: Vec<SmartCommitGroup> = remaining
        .iter()
        .filter_map(|g| {
            let files: Vec<String> = g
                .files
                .iter()
                .filter(|f| still_changed.contains(f.as_str()))
                .cloned()
                .collect();
            (!files.is_empty()).then(|| SmartCommitGroup { files, ..g.clone() })
        })
        .collect();
    let commitlint = CommitlintRules::load(&changes.root);
    let fingerprint = plan_fingerprint(&changes, options, &commitlint, command);
    write_cache(&changes.root, &fingerprint, &remaining);
    Ok(())
}

// ── Planning ───────────────────────────────────────────────────────────────

/// Progress callback: receives short human-readable status lines.
pub type Progress<'a> = &'a (dyn Fn(&str) + Send + Sync);

/// Produce a commit plan for every change in the working tree.
///
/// `command` receives the full prompt on stdin (env
/// `LAZYGITRS_AI_COMMIT_INPUT=prompt`) and must print the model's answer.
/// Returns `Ok(None)` when `cancel` is set.
pub fn plan(
    repo_path: &Path,
    command: &str,
    options: &SmartCommitOptions,
    cancel: Arc<AtomicBool>,
    progress: Progress<'_>,
) -> Result<Option<SmartCommitPlan>> {
    if command.trim().is_empty() {
        bail!(
            "No AI command configured. Set git.commit.smartCommitCommand (or generateCommand) \
             to a command that reads a prompt on stdin and prints the answer."
        );
    }
    progress("Reading working tree");
    let changes = collect_changes(repo_path)?;
    if changes.is_empty() {
        bail!("Nothing to commit: the working tree is clean");
    }
    let commitlint = CommitlintRules::load(&changes.root);
    let fingerprint = plan_fingerprint(&changes, options, &commitlint, command);
    let files: BTreeMap<String, ChangedFile> = changes
        .files
        .iter()
        .map(|f| (f.path.clone(), f.clone()))
        .collect();

    if !options.force
        && let Some(groups) = read_cache(&changes.root, &fingerprint)
    {
        // Only trust a cached plan that still covers exactly the changed files.
        let cached: HashSet<&str> = groups
            .iter()
            .flat_map(|g| g.files.iter().map(String::as_str))
            .collect();
        if cached.len() == files.len() && cached.iter().all(|p| files.contains_key(*p)) {
            return Ok(Some(SmartCommitPlan {
                groups,
                files,
                from_cache: true,
                warnings: Vec::new(),
                attempts: 0,
            }));
        }
    }

    let prompt = build_prompt(&changes, options, &commitlint);
    let mut active_prompt = prompt.clone();
    let mut best: Option<Repaired> = None;
    let mut attempts = 0;
    let mut last_error = String::new();

    while attempts < MAX_ATTEMPTS {
        attempts += 1;
        progress(&format!(
            "Asking AI to group {} files (attempt {attempts}/{MAX_ATTEMPTS})",
            changes.files.len()
        ));
        let envs = [
            ("LAZYGITRS_AI_COMMIT_INPUT", "prompt".to_string()),
            ("LAZYGITRS_AI_PROMPT_BYTES", active_prompt.len().to_string()),
            ("LAZYGITRS_SMART_COMMIT", "1".to_string()),
        ];
        let Some(output) = run_shell_command_cancellable(
            &changes.root,
            command,
            Some(&active_prompt),
            &envs,
            Arc::clone(&cancel),
        )?
        else {
            return Ok(None);
        };
        if !output.success {
            last_error = format!("AI command failed: {}", output.stderr.trim());
            continue;
        }
        let Some(raw) = extract_json(&output.stdout) else {
            last_error = "AI answer contained no JSON commit array".to_string();
            active_prompt = format!(
                "Your previous answer could not be parsed. It must be ONLY a JSON array of \
                 {{\"title\", \"body\", \"files\"}} objects.\n\nPrevious answer:\n{}\n\n---\n\n{prompt}",
                truncate_content(output.stdout.trim(), 4_000)
            );
            continue;
        };
        let repaired = repair_plan(raw, &changes, &commitlint, options.emoji);
        let done = repaired.issues.is_empty();
        let better = best.as_ref().is_none_or(|b| {
            (repaired.unassigned.len(), repaired.issues.len())
                <= (b.unassigned.len(), b.issues.len())
        });
        if done {
            best = Some(repaired);
            break;
        }
        active_prompt = format!(
            "Your previous plan had problems:\n{}\n\nReturn the COMPLETE corrected JSON array \
             (all commits, every file exactly once).\n\nPrevious plan:\n{}\n\n---\n\n{prompt}",
            repaired
                .issues
                .iter()
                .map(|i| format!("- {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
            serde_json::to_string(&repaired.groups).unwrap_or_default(),
        );
        if better {
            best = Some(repaired);
        }
    }

    let Some(mut repaired) = best else {
        bail!(
            "{}",
            if last_error.is_empty() {
                "AI produced no usable plan".to_string()
            } else {
                last_error
            }
        );
    };
    if !repaired.unassigned.is_empty() {
        repaired.groups.push(SmartCommitGroup {
            title: "chore: update remaining files".to_string(),
            body: String::new(),
            files: std::mem::take(&mut repaired.unassigned),
            fallback: true,
        });
    }
    // Renames must travel with their source path; both are in `files`, the
    // commit step adds `old_path` automatically.
    write_cache(&changes.root, &fingerprint, &repaired.groups);

    Ok(Some(SmartCommitPlan {
        groups: repaired.groups,
        files,
        from_cache: false,
        warnings: repaired.issues,
        attempts,
    }))
}

// ── Committing ─────────────────────────────────────────────────────────────

/// Commit exactly `group.files` (plus rename sources) and return the new
/// short hash. Uses `git commit --only` so unrelated staged content stays.
pub fn commit_group(
    repo_path: &Path,
    group: &SmartCommitGroup,
    files: &BTreeMap<String, ChangedFile>,
    sign_off: bool,
) -> Result<String> {
    let root = PathBuf::from(git_output(repo_path, &["rev-parse", "--show-toplevel"])?.trim());
    let mut paths: Vec<String> = Vec::new();
    for path in &group.files {
        paths.push(path.clone());
        if let Some(old) = files.get(path).and_then(|f| f.old_path.clone())
            && !paths.contains(&old)
        {
            paths.push(old);
        }
    }
    if paths.is_empty() {
        bail!("Commit '{}' has no files", group.title);
    }

    // A path gone from both disk and index (the source of a staged rename)
    // is not a valid `git add` pathspec, but `commit --only` still needs it.
    let indexed = git_output(&root, &["ls-files", "-z", "--"])
        .map(|out| out.split('\0').map(String::from).collect::<HashSet<_>>())
        .unwrap_or_default();
    let addable: Vec<&str> = paths
        .iter()
        .filter(|p| root.join(p).symlink_metadata().is_ok() || indexed.contains(p.as_str()))
        .map(String::as_str)
        .collect();
    if !addable.is_empty() {
        let mut add = vec!["add", "-A", "--"];
        add.extend(addable);
        git_run(&root, &add).context("git add failed")?;
    }

    let message = group.message();
    let mut commit = vec!["commit", "--only", "-F", "-"];
    if sign_off {
        commit.push("--signoff");
    }
    commit.push("--");
    commit.extend(paths.iter().map(String::as_str));
    git_run_stdin(&root, &commit, &message).context("git commit failed")?;

    Ok(git_output(&root, &["rev-parse", "--short", "HEAD"])?
        .trim()
        .to_string())
}

/// Full (uncompressed) diff of one group for the review pane: tracked
/// changes against HEAD plus untracked contents rendered as additions.
pub fn group_preview(
    repo_path: &Path,
    group: &SmartCommitGroup,
    files: &BTreeMap<String, ChangedFile>,
) -> Result<String> {
    let root = PathBuf::from(git_output(repo_path, &["rev-parse", "--show-toplevel"])?.trim());
    let mut tracked: Vec<&str> = Vec::new();
    let mut out = String::new();
    for path in &group.files {
        let meta = files.get(path);
        if meta.is_some_and(|f| f.kind == ChangeKind::Untracked) {
            let _ = writeln!(
                out,
                "diff --git a/{path} b/{path}\nnew file\n--- /dev/null\n+++ b/{path}"
            );
            match std::fs::read(root.join(path)) {
                Ok(bytes) if bytes.contains(&0) => out.push_str("Binary file\n"),
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let _ = writeln!(out, "@@ -0,0 +1,{} @@", text.lines().count());
                    for line in text.lines() {
                        let _ = writeln!(out, "+{line}");
                    }
                }
                Err(e) => {
                    let _ = writeln!(out, "(unreadable: {e})");
                }
            }
            continue;
        }
        tracked.push(path);
        if let Some(old) = meta.and_then(|f| f.old_path.as_deref()) {
            tracked.push(old);
        }
    }
    if !tracked.is_empty() {
        let has_head = git_output(&root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok();
        let base: &[&str] = if has_head { &["HEAD"] } else { &["--cached"] };
        let mut args = vec![
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--find-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ];
        args.extend_from_slice(base);
        args.push("--");
        args.extend(tracked);
        let diff = git_output(&root, &args)?;
        out.insert_str(0, &diff);
    }
    Ok(out)
}

// ── git helpers ────────────────────────────────────────────────────────────

fn git_output_raw(dir: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .stdin(Stdio::null())
        .output()
        .context("failed to run git")?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn git_output(dir: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&git_output_raw(dir, args)?).into_owned())
}

fn git_run(dir: &Path, args: &[&str]) -> Result<()> {
    git_output_raw(dir, args).map(|_| ())
}

fn git_run_stdin(dir: &Path, args: &[&str], input: &str) -> Result<()> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_LITERAL_PATHSPECS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to run git")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        bail!("{}", detail.trim());
    }
    Ok(())
}

/// FNV-1a 64: stable across Rust versions, unlike `DefaultHasher`.
struct Fnv64(u64);

impl Fnv64 {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
        // Separator so ("ab","c") and ("a","bc") hash differently.
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
#[path = "smart_commit_tests.rs"]
mod tests;
