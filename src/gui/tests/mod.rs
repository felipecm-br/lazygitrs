pub(super) use super::*;

mod checkout_and_refresh;
mod keybindings_and_navigation;
mod workers_and_cache;

pub(super) static COCKPIT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    pub(super) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "lazygitrs-gui-test-{}-{}-{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        let _ = std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&path)
            .output();
        Self { path }
    }

    pub(super) fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub(super) fn git(&self, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(args)
            .current_dir(&self.path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    pub(super) fn commit(&self, path: &str, contents: &str) -> String {
        std::fs::write(self.path.join(path), contents).unwrap();
        self.git(&["add", path]);
        self.git(&["commit", "-m", contents]);
        self.git(&["rev-parse", "HEAD"])
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(super) fn checkout_test_gui(repo: &TempRepo) -> Gui {
    repo.commit("file", "main contents");
    repo.git(&["checkout", "-b", "feature"]);
    repo.commit("file", "feature contents");
    let git = crate::git::GitCommands::new(repo.path()).unwrap();
    let mut gui = Gui::new(
        crate::config::AppConfig::default(),
        git,
        false,
        None,
        false,
        None,
        false,
    )
    .unwrap();
    // Finish startup loading before exercising checkout and its refresh.
    for _ in gui.initial_load_rx.take().unwrap() {}
    *gui.model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        gui.git.load_model(DEFAULT_COMMIT_LIMIT, false).unwrap();
    gui.context_mgr.set_active(ContextId::Branches);
    gui
}

pub(super) fn request_test_checkout(gui: &mut Gui, key: char, target: &str) {
    let selected = gui
        .model
        .lock()
        .unwrap()
        .branches
        .iter()
        .position(|b| b.name == target)
        .unwrap_or(0);
    gui.context_mgr.set_selection(selected);
    gui.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE))
        .unwrap();
    if key == 'c' {
        let PopupState::RefPicker { core, .. } = &mut gui.popup else {
            panic!("expected checkout ref picker");
        };
        core.search_textarea = popup::make_command_palette_search_textarea();
        core.search_textarea.insert_str(target);
        core.selected = core
            .items
            .iter()
            .position(|i| i.value == target)
            .unwrap_or(core.items.len());
        gui.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();
    }
}

pub(super) fn wait_for_test_checkout(gui: &mut Gui) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while gui.remote_op_label.is_some() {
        assert!(Instant::now() < deadline, "checkout did not finish");
        gui.receive_remote_op_results();
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(super) fn assert_checkout_head(gui: &Gui, branch: &str, hash: &str) {
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let heads: Vec<_> = model
        .branches
        .iter()
        .filter(|b| b.head)
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(heads, vec![branch]);
    assert_eq!(model.head_branch_name, branch);
    assert_eq!(model.head_hash, hash);
    assert_eq!(gui.git.current_branch_name().unwrap(), branch);
    assert_eq!(gui.git.head_hash().unwrap(), hash);
}
