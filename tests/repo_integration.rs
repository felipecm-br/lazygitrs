use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use lazygitrs::config::AppConfig;
use lazygitrs::git::GitCommands;
use lazygitrs::gui::Gui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    fn new(prefix: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("lazygitrs-integ-{prefix}-{pid}-{n}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");

        let mut repo = Self { path };
        repo.init_git();
        repo
    }

    fn init_git(&mut self) {
        self.cmd(&["init"]);
        self.cmd(&["config", "user.name", "Lazygitrs Tester"]);
        self.cmd(&["config", "user.email", "test@lazygitrs.local"]);
        self.cmd(&["config", "commit.gpgsign", "false"]);
    }

    fn cmd(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .unwrap_or_else(|e| panic!("failed to run git {:?}: {}", args, e));
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn write_file(&self, name: &str, content: &str) {
        let p = self.path.join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(p, content).expect("write file");
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn test_temp_repo_load_model_and_git_commands() {
    let repo = TempRepo::new("load-model");

    // Initial commit
    repo.write_file("file1.txt", "hello world\n");
    repo.cmd(&["add", "file1.txt"]);
    repo.cmd(&["commit", "-m", "Initial commit"]);

    // Create a feature branch
    repo.cmd(&["checkout", "-b", "feature/awesome"]);
    repo.write_file("file2.rs", "fn awesome() {}\n");
    repo.cmd(&["add", "file2.rs"]);
    repo.cmd(&["commit", "-m", "Add awesome feature"]);

    // Make an unstaged modification
    repo.write_file("file1.txt", "hello modified world\n");

    let git = GitCommands::new(repo.path()).expect("GitCommands::new");
    let model = git.load_model(300, false).expect("load model");

    // Assert branches
    assert!(
        model
            .branches
            .iter()
            .any(|b| b.name == "feature/awesome" && b.head),
        "HEAD should be feature/awesome"
    );

    // Assert files
    assert!(
        model.files.iter().any(|f| f.name == "file1.txt"),
        "file1.txt should be modified"
    );

    // Assert commits
    assert!(
        model.commits.len() >= 2,
        "should have at least 2 commits in history"
    );
    assert_eq!(model.commits[0].name, "Add awesome feature");
}

#[test]
fn test_gui_headless_initialization_and_rendering() {
    let repo = TempRepo::new("gui-render");

    repo.write_file(
        "Cargo.toml",
        "[package]\nname = \"dummy\"\nversion = \"0.1.0\"\n",
    );
    repo.write_file("src/main.rs", "fn main() { println!(\"hi\"); }\n");
    fs::create_dir_all(repo.path().join("src")).unwrap();
    repo.cmd(&["add", "."]);
    repo.cmd(&["commit", "-m", "feat: initial cargo setup"]);

    // Modify a file so there is an active diff
    repo.write_file("src/main.rs", "fn main() { println!(\"hello world!\"); }\n");

    let config = AppConfig::default();
    let git = GitCommands::new(repo.path()).expect("GitCommands::new");
    let mut gui = Gui::new(config, git, false, None, false, None, false).expect("Gui::new");

    // Initialize a test terminal backend
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create test terminal");

    // Draw the UI once with default theme
    let theme = lazygitrs::config::theme::Theme::default();
    terminal
        .draw(|f| {
            gui.render_ui(f, &theme);
        })
        .expect("draw UI");

    // Ensure terminal buffer was written
    let buffer = terminal.backend().buffer();
    let text = buffer
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        text.contains("Files") || text.contains("Branches") || text.contains("Commits"),
        "rendered buffer should display standard panels"
    );
}

#[test]
fn test_stage_unstage_roundtrip_integration() {
    let repo = TempRepo::new("stage-unstage");

    repo.write_file("test.txt", "v1\n");
    repo.cmd(&["add", "test.txt"]);
    repo.cmd(&["commit", "-m", "c1"]);

    repo.write_file("test.txt", "v2 modified\n");

    let git = GitCommands::new(repo.path()).expect("GitCommands::new");

    // Initially unstaged
    let model1 = git.load_model(300, false).expect("load model 1");
    let f1 = model1
        .files
        .iter()
        .find(|f| f.name == "test.txt")
        .expect("find file");
    assert!(f1.has_unstaged_changes);

    // Stage file
    git.stage_file("test.txt").expect("stage_file");
    let model2 = git.load_model(300, false).expect("load model 2");
    let f2 = model2
        .files
        .iter()
        .find(|f| f.name == "test.txt")
        .expect("find file");
    assert!(f2.has_staged_changes);

    // Unstage file
    git.unstage_file("test.txt").expect("unstage_file");
    let model3 = git.load_model(300, false).expect("load model 3");
    let f3 = model3
        .files
        .iter()
        .find(|f| f.name == "test.txt")
        .expect("find file");
    assert!(f3.has_unstaged_changes);
}

#[test]
fn test_idle_fingerprint_skips_redundant_work() {
    let repo = TempRepo::new("idle-fingerprint");

    repo.write_file("init.txt", "initial\n");
    repo.cmd(&["add", "init.txt"]);
    repo.cmd(&["commit", "-m", "init"]);

    let git = GitCommands::new(repo.path()).expect("GitCommands::new");

    let fp1 = git.refs_fingerprint().expect("fingerprint 1");
    // Repeating without repository mutations yields identical fingerprint (zero overhead)
    let fp2 = git.refs_fingerprint().expect("fingerprint 2");
    assert_eq!(fp1, fp2, "idle repository must have identical fingerprint");

    // Making a commit must advance the fingerprint
    repo.write_file("second.txt", "second\n");
    repo.cmd(&["add", "second.txt"]);
    repo.cmd(&["commit", "-m", "second commit"]);

    let fp3 = git.refs_fingerprint().expect("fingerprint 3");
    assert_ne!(fp1, fp3, "new commit must change git metadata fingerprint");

    // Creating and checking out a branch must advance the fingerprint
    repo.cmd(&["checkout", "-b", "topic-branch"]);
    let fp4 = git.refs_fingerprint().expect("fingerprint 4");
    assert_ne!(
        fp3, fp4,
        "branch checkout must change git metadata fingerprint"
    );
}
