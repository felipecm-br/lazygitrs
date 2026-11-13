use super::*;

use std::sync::Mutex;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("lazygitrs-sgc-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(dir: &Path, path: &str, content: &str) {
    let full = dir.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

fn repo() -> TempDir {
    let temp = TempDir::new("repo");
    let r = &temp.path;
    git(r, &["init", "-q", "-b", "main"]);
    git(r, &["config", "user.name", "t"]);
    git(r, &["config", "user.email", "t@t"]);
    git(r, &["config", "commit.gpgsign", "false"]);
    write(r, "src/lib.rs", "pub fn a() {}\n");
    write(r, "README.md", "# demo\n");
    write(r, "old.txt", "rename me\n");
    git(r, &["add", "-A"]);
    git(r, &["commit", "-q", "-m", "initial"]);
    temp
}

fn changes_of(paths: &[&str]) -> ChangeSet {
    ChangeSet {
        root: PathBuf::from("/tmp"),
        files: paths
            .iter()
            .map(|p| ChangedFile {
                path: p.to_string(),
                old_path: None,
                kind: ChangeKind::Modified,
                added: 1,
                deleted: 0,
                binary: false,
                content_ignored: false,
            })
            .collect(),
        sections: paths.iter().map(|_| "+x\n".to_string()).collect(),
        fingerprint: 1,
    }
}

// ── glob / ignore ──────────────────────────────────────────────────────────

#[test]
fn glob_basics() {
    assert!(glob_match("*.lock", "Cargo.lock"));
    assert!(!glob_match("*.lock", "dir/Cargo.lock"));
    assert!(glob_match("**/*.lock", "dir/Cargo.lock"));
    assert!(glob_match("**/*.lock", "Cargo.lock"));
    assert!(glob_match("src/**", "src/a/b.rs"));
    assert!(glob_match("file?.rs", "file1.rs"));
    assert!(glob_match("[a-c]x", "bx"));
    assert!(!glob_match("[!a-c]x", "bx"));
    assert!(glob_match("*-lock.*", "pnpm-lock.yaml"));
}

#[test]
fn ignore_rules_follow_gitignore_semantics() {
    let rules = IgnoreRules::parse("*.lock\n!keep.lock\ndist/\n/top.txt\n# comment\nvendor/*.js\n");
    assert!(rules.is_ignored("Cargo.lock"));
    assert!(rules.is_ignored("sub/yarn.lock"));
    assert!(!rules.is_ignored("keep.lock"));
    assert!(rules.is_ignored("dist/app.js"));
    assert!(rules.is_ignored("pkg/dist/app.js"));
    assert!(
        !rules.is_ignored("dist"),
        "dir pattern must not match a file"
    );
    assert!(rules.is_ignored("top.txt"));
    assert!(!rules.is_ignored("sub/top.txt"));
    assert!(rules.is_ignored("vendor/x.js"));
    assert!(!rules.is_ignored("src/main.rs"));
}

// ── diff handling ──────────────────────────────────────────────────────────

#[test]
fn compress_diff_strips_metadata_and_context() {
    let diff = "diff --git a/f.rs b/f.rs\nindex 111..222 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -1,5 +1,5 @@ fn main()\n ctx1\n ctx2\n ctx3\n-old\n+new\n\\ No newline at end of file\n";
    let out = compress_diff(diff);
    assert_eq!(out, "@@ fn main()\n ctx1\n-old\n+new\n");
}

#[test]
fn compress_diff_caps_large_hunks() {
    let mut diff = String::from("@@ -1 +1,200 @@\n");
    for i in 0..200 {
        diff.push_str(&format!("+line {i}\n"));
    }
    let out = compress_diff(&diff);
    assert!(out.contains("+line 79\n"));
    assert!(!out.contains("+line 80\n"));
    assert!(out.contains("[... 120 more changed lines elided ...]"));
}

#[test]
fn split_diff_handles_spaces_and_renames() {
    let diff = "diff --git a/a b.txt b/a b.txt\n--- a/a b.txt\n+++ b/a b.txt\n@@ -1 +1 @@\n-x\n+y\ndiff --git a/old.rs b/new.rs\nsimilarity index 90%\nrename from old.rs\nrename to new.rs\n";
    let parts = split_diff_by_file(diff);
    assert_eq!(parts[0].0, "a b.txt");
    assert_eq!(parts[1].0, "new.rs");
    assert_eq!(count_diff_lines(&parts[0].1), (1, 1, false));
}

#[test]
fn parse_status_z_reads_renames_and_rejects_conflicts() {
    let raw = b"R  new.rs\0old.rs\0 M src/a.rs\0?? n.txt\0 D gone\0";
    let entries = parse_status_z(raw).unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].kind, ChangeKind::Renamed);
    assert_eq!(entries[0].old_path.as_deref(), Some("old.rs"));
    assert_eq!(entries[2].kind, ChangeKind::Untracked);
    assert_eq!(entries[3].kind, ChangeKind::Deleted);
    assert!(parse_status_z(b"UU x.rs\0").is_err());
}

#[test]
fn context_budget_elides_largest_file_first() {
    let mut changes = changes_of(&["small.rs", "big.rs"]);
    changes.sections[1] = "+x\n".repeat(1_000);
    let ctx = build_context(&changes, 100);
    assert!(ctx.contains("### small.rs (diff)"));
    assert!(ctx.contains("### big.rs (+1 -0; content elided"));
}

// ── commitlint ─────────────────────────────────────────────────────────────

#[test]
fn commitlint_from_json_config() {
    let cfg: Value = serde_json::json!({
        "rules": {
            "type-enum": [2, "always", ["feat", "fix"]],
            "scope-enum": [2, "always", ["ui", "git"]],
            "header-max-length": [2, "always", 50],
            "subject-case": [2, "never", ["upper-case"]],
            "scope-case": [0, "always", "lower-case"]
        }
    });
    let rules = CommitlintRules::from_config(&cfg);
    assert_eq!(rules.types, Some(vec!["feat".into(), "fix".into()]));
    assert_eq!(rules.scopes, Some(vec!["ui".into(), "git".into()]));
    assert_eq!(rules.header_max(), 50);
    assert_eq!(rules.subject_case_never, vec!["upper-case".to_string()]);
    assert_eq!(rules.scope_case, None, "level 0 disables a rule");
}

#[test]
fn commitlint_from_js_config() {
    let js = r#"
    // commitlint config
    module.exports = {
      extends: ['@commitlint/config-conventional'],
      rules: {
        'type-enum': [2, 'always', ['feat', 'fix', 'chore',]],
        'header-max-length': [2, 'always', 60], // keep it short
      },
    };
    "#;
    let rules =
        CommitlintRules::from_config(&serde_json::json!({ "rules": js_rules_object(js).unwrap() }));
    assert_eq!(rules.types.as_ref().map(Vec::len), Some(3));
    assert_eq!(rules.header_max(), 60);
}

// ── parsing and repair ─────────────────────────────────────────────────────

#[test]
fn extract_json_tolerates_prose_fences_and_wrappers() {
    let raw =
        "Sure! Here is the plan:\n```json\n[{\"title\": \"feat: a [x]\", \"files\": [\"a\"]}]\n```";
    assert_eq!(extract_json(raw).unwrap().len(), 1);
    let wrapped = "{\"commits\": [{\"message\": \"fix: b\\n\\n- why\", \"files\": [\"b\"]}]}";
    assert_eq!(extract_json(wrapped).unwrap().len(), 1);
    assert!(extract_json("no json here").is_none());
}

#[test]
fn repair_resolves_paths_dedupes_and_reports_gaps() {
    let changes = changes_of(&["src/a.rs", "src/b.rs", "docs/x.md", "c.rs"]);
    let raw = extract_json(
        r#"[
          {"title": "feat(core): add a.", "body": ["first", "- second"], "files": ["./src/a.rs", "ghost.rs"]},
          {"message": "docs: update guide\n\n- note", "files": ["docs/", "src/a.rs"]},
          {"title": "wip stuff", "files": ["b/src/b.rs"]}
        ]"#,
    )
    .unwrap();
    let r = repair_plan(raw, &changes, &CommitlintRules::default(), false);
    assert_eq!(r.groups.len(), 3);
    assert_eq!(r.groups[0].title, "feat(core): add a");
    assert_eq!(r.groups[0].body, "- first\n- second");
    assert_eq!(r.groups[0].files, vec!["src/a.rs"]);
    assert_eq!(
        r.groups[1].files,
        vec!["docs/x.md"],
        "dir expands; dup dropped"
    );
    assert_eq!(r.groups[1].body, "- note");
    assert_eq!(r.groups[2].files, vec!["src/b.rs"]);
    assert_eq!(r.unassigned, vec!["c.rs"]);
    assert!(r.issues.iter().any(|i| i.contains("ghost.rs")));
    assert!(r.issues.iter().any(|i| i.contains("wip stuff")));
}

#[test]
fn title_validation_respects_emoji_and_limits() {
    let rules = CommitlintRules::default();
    assert_eq!(title_issue("✨ feat(ui): add popup", &rules, true), None);
    assert_eq!(title_issue("feat!: drop api", &rules, false), None);
    assert!(title_issue("added stuff", &rules, false).is_some());
    assert!(title_issue("yolo: x", &rules, false).is_some());
    assert!(title_issue(&format!("fix: {}", "a".repeat(80)), &rules, false).is_some());
}

#[test]
fn group_message_roundtrip() {
    let mut g = SmartCommitGroup {
        title: "feat: x".into(),
        body: String::new(),
        files: vec![],
        fallback: false,
    };
    assert_eq!(g.message(), "feat: x");
    g.set_message("fix(a): y\n\n- because\n");
    assert_eq!(g.title, "fix(a): y");
    assert_eq!(g.body, "- because");
    assert_eq!(g.message(), "fix(a): y\n\n- because");
}

// ── end to end ─────────────────────────────────────────────────────────────

/// `plan` touches the real cache dir; serialize the e2e tests.
static E2E: Mutex<()> = Mutex::new(());

fn fake_ai(dir: &Path, answer: &str) -> String {
    let script = dir.join("fake-ai.sh");
    let answer_file = dir.join("answer.json");
    std::fs::write(&answer_file, answer).unwrap();
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ncat > \"{}\"\n[ \"$LAZYGITRS_AI_COMMIT_INPUT\" = prompt ] || exit 3\ncat \"{}\"\n",
            dir.join("prompt.txt").display(),
            answer_file.display()
        ),
    )
    .unwrap();
    format!("sh {}", script.display())
}

#[test]
fn end_to_end_plan_and_commit() {
    let _guard = E2E
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = repo();
    let r = &temp.path;
    let tools = TempDir::new("tools");

    write(r, "src/lib.rs", "pub fn a() {}\npub fn b() {}\n");
    write(r, "src/new.rs", "pub fn new() {}\n");
    write(r, "README.md", "# demo\n\nUsage.\n");
    write(r, "Cargo.lock", "lock\n");
    git(r, &["mv", "old.txt", "renamed.txt"]);
    // Something the user staged on purpose that is part of a group.
    git(r, &["add", "README.md"]);

    let cmd = fake_ai(
        &tools.path,
        r#"[
          {"title": "feat(core): add b and new module", "body": "- add b\n- add new", "files": ["src/lib.rs", "src/new.rs", "Cargo.lock"]},
          {"title": "docs: document usage", "body": "", "files": ["README.md", "renamed.txt"]}
        ]"#,
    );
    let options = SmartCommitOptions {
        force: true,
        ..Default::default()
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let plan = plan(r, &cmd, &options, Arc::clone(&cancel), &|_| {})
        .unwrap()
        .unwrap();
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    assert_eq!(plan.groups.len(), 2);
    assert_eq!(
        plan.files["renamed.txt"].old_path.as_deref(),
        Some("old.txt")
    );
    assert!(plan.files["Cargo.lock"].content_ignored);

    let prompt = std::fs::read_to_string(tools.path.join("prompt.txt")).unwrap();
    assert!(prompt.contains("Cargo.lock  +1 -0  [generated/lockfile: content omitted]"));
    assert!(prompt.contains("+pub fn b() {}"), "{prompt}");
    assert!(prompt.contains("renamed.txt (renamed from old.txt)"));

    // Second run without force is served from cache.
    let cached = super::plan(
        r,
        &cmd,
        &SmartCommitOptions::default(),
        Arc::clone(&cancel),
        &|_| {},
    );
    // Different options key (force is not part of it) → same fingerprint.
    assert!(cached.unwrap().unwrap().from_cache);

    // Commit only the first group; the README stays staged.
    let hash = commit_group(r, &plan.groups[0], &plan.files, false).unwrap();
    assert!(!hash.is_empty());
    let shown = git(r, &["show", "--name-only", "--format=%s%n%b", "HEAD"]);
    assert!(shown.starts_with("feat(core): add b and new module\n- add b\n- add new"));
    assert!(shown.contains("src/new.rs") && shown.contains("Cargo.lock"));
    assert!(!shown.contains("README.md"));
    let staged = git(r, &["diff", "--cached", "--name-only"]);
    assert!(
        staged.contains("README.md"),
        "user staging preserved: {staged}"
    );

    save_remaining(r, &cmd, &SmartCommitOptions::default(), &plan.groups[1..]).unwrap();
    let again = super::plan(r, &cmd, &SmartCommitOptions::default(), cancel, &|_| {})
        .unwrap()
        .unwrap();
    assert!(again.from_cache, "leftover group reused");
    assert_eq!(again.groups.len(), 1);

    commit_group(r, &again.groups[0], &again.files, false).unwrap();
    assert_eq!(git(r, &["status", "--porcelain"]), "");
    let shown = git(r, &["show", "--name-status", "--format=", "HEAD"]);
    assert!(
        shown.contains("old.txt") && shown.contains("renamed.txt"),
        "{shown}"
    );
    clear_cache(r);
}

#[test]
fn end_to_end_falls_back_for_unassigned_files_after_retries() {
    let _guard = E2E
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = repo();
    let r = &temp.path;
    let tools = TempDir::new("tools");
    write(r, "src/lib.rs", "changed\n");
    write(r, "README.md", "changed\n");
    let cmd = fake_ai(
        &tools.path,
        r#"[{"title": "fix: lib", "files": ["src/lib.rs"]}]"#,
    );
    let options = SmartCommitOptions {
        force: true,
        ..Default::default()
    };
    let plan = plan(r, &cmd, &options, Arc::new(AtomicBool::new(false)), &|_| {})
        .unwrap()
        .unwrap();
    assert_eq!(plan.attempts, MAX_ATTEMPTS);
    assert_eq!(plan.groups.len(), 2);
    assert!(plan.groups[1].fallback);
    assert_eq!(plan.groups[1].files, vec!["README.md"]);
    let prompt = std::fs::read_to_string(tools.path.join("prompt.txt")).unwrap();
    assert!(prompt.starts_with("Your previous plan had problems:"));
    clear_cache(r);
}

#[test]
fn plan_rejects_clean_tree_and_missing_command() {
    let temp = repo();
    let cancel = Arc::new(AtomicBool::new(false));
    let err = plan(
        &temp.path,
        "true",
        &SmartCommitOptions::default(),
        Arc::clone(&cancel),
        &|_| {},
    )
    .unwrap_err();
    assert!(err.to_string().contains("clean"));
    let err = plan(
        &temp.path,
        " ",
        &SmartCommitOptions::default(),
        cancel,
        &|_| {},
    )
    .unwrap_err();
    assert!(err.to_string().contains("smartCommitCommand"));
}

#[test]
fn generate_messages_from_staged_changes() {
    let _guard = E2E
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = repo();
    let r = &temp.path;
    let tools = TempDir::new("tools");
    write(r, "src/lib.rs", "pub fn a() {}\npub fn c() {}\n");
    write(r, "README.md", "unstaged edit\n");
    git(r, &["add", "src/lib.rs"]);
    git(r, &["mv", "old.txt", "moved.txt"]);

    let cmd = fake_ai(
        &tools.path,
        r#"[{"title": "feat(core): add c", "body": ["add c"]}, {"title": "bad title"}, {"title": "feat: expose c helper"}]"#,
    );
    let options = SmartCommitOptions {
        emoji: false,
        lang: Some("pt".into()),
        ..Default::default()
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let got = generate_messages(r, &cmd, &options, 2, Arc::clone(&cancel), &|_| {})
        .unwrap()
        .unwrap();
    assert_eq!(got.len(), 2, "invalid title dropped");
    assert_eq!(got[0].message(), "feat(core): add c\n\n- add c");
    let prompt = std::fs::read_to_string(tools.path.join("prompt.txt")).unwrap();
    assert!(prompt.contains("+pub fn c() {}"));
    assert!(prompt.contains("moved.txt (renamed from old.txt)"));
    assert!(!prompt.contains("README.md"), "only staged changes");
    assert!(prompt.contains("ISO 639-1 code 'pt'"));
    assert!(prompt.contains("2 genuinely different alternatives"));

    // A plain-text answer (models often skip JSON for one message) is accepted.
    let cmd = fake_ai(&tools.path, "```\nfix(core): handle c\n\n- details\n```\n");
    let got = generate_messages(r, &cmd, &SmartCommitOptions::default(), 1, cancel, &|_| {})
        .unwrap()
        .unwrap();
    assert_eq!(got[0].message(), "fix(core): handle c\n\n- details");
}

#[test]
fn generate_messages_passes_provider_and_model() {
    let _guard = E2E
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp = repo();
    let r = &temp.path;
    write(r, "src/lib.rs", "x\n");
    git(r, &["add", "-A"]);
    let cmd = "cat >/dev/null; printf 'chore: %s %s\\n' \"$LAZYGITRS_AI_PROVIDER\" \"$LAZYGITRS_AI_MODEL\"";
    let options = SmartCommitOptions {
        provider: Some("claude".into()),
        model: Some("haiku".into()),
        ..Default::default()
    };
    let got = generate_messages(
        r,
        cmd,
        &options,
        1,
        Arc::new(AtomicBool::new(false)),
        &|_| {},
    )
    .unwrap()
    .unwrap();
    assert_eq!(got[0].title, "chore: claude haiku");
    let err = generate_messages(
        &repo().path,
        cmd,
        &options,
        1,
        Arc::new(AtomicBool::new(false)),
        &|_| {},
    )
    .unwrap_err();
    assert!(err.to_string().contains("No staged changes"));
}

#[test]
fn body_lines_wrap_at_72_with_hanging_indent() {
    let long = "- Makes formatter output consistent for every contributor instead of relying on rustfmt defaults";
    let body = clean_body(&format!("{long}\n- short\nCo-Authored-By: x <y>"));
    let lines: Vec<&str> = body.lines().collect();
    assert!(lines.iter().all(|l| l.chars().count() <= 72), "{body}");
    assert!(lines[0].starts_with("- Makes"));
    assert!(
        lines[1].starts_with("  "),
        "continuation is indented: {body}"
    );
    assert_eq!(*lines.last().unwrap(), "- short");
}
