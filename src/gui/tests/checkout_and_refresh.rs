use super::*;

#[test]
#[ignore = "manual checkout visibility benchmark; timing is machine-dependent"]
fn checkout_visibility_latency() {
    for auto_refresh in [true, false] {
        let repo = TempRepo::new("checkout-latency");
        let mut gui = checkout_test_gui(&repo);
        let mut config = crate::config::AppConfig::default();
        config.user_config.git.auto_refresh = auto_refresh;
        gui.config = Arc::new(config);
        let mut samples = Vec::new();
        for target in ["main", "feature"].into_iter().cycle().take(10) {
            let start = Instant::now();
            request_test_checkout(&mut gui, ' ', target);
            loop {
                if let Some(rx) = &gui.initial_load_rx {
                    while let Ok(part) = rx.try_recv() {
                        let mut model = gui
                            .model
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        match part {
                            ModelPart::Branches(branches) => model.branches = branches,
                            ModelPart::Head { hash, branch_name } => {
                                model.head_hash = hash;
                                model.head_branch_name = branch_name;
                            }
                            _ => {}
                        }
                    }
                }
                gui.receive_remote_op_results();
                {
                    let model = gui
                        .model
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if model.head_branch_name == target
                        && model.branches.iter().any(|b| b.head && b.name == target)
                    {
                        samples.push(start.elapsed().as_secs_f64() * 1000.0);
                        break;
                    }
                }
                assert!(start.elapsed() < Duration::from_secs(10));
                // Mirror the main loop's input wait before dispatching refresh.
                std::thread::sleep(gui.input_wait_timeout());
                if gui.needs_refresh && !gui.refresh_in_progress && gui.initial_load_rx.is_none() {
                    gui.start_background_refresh();
                }
            }
            if let Some(rx) = gui.initial_load_rx.take() {
                for _ in rx {}
            }
            gui.refresh_in_progress = false;
            gui.needs_refresh = false;
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "auto_refresh={auto_refresh}: median={:.1}ms, min={:.1}ms, max={:.1}ms (10 checkouts)",
            (samples[4] + samples[5]) / 2.0,
            samples[0],
            samples[9]
        );
    }
}

#[test]
fn failed_checkout_preserves_head_after_error_dismissal_and_retry() {
    for key in [' ', '-', 'c'] {
        let repo = TempRepo::new("failed-checkout");
        let mut gui = checkout_test_gui(&repo);
        let head = repo.git(&["rev-parse", "HEAD"]);
        std::fs::write(repo.path().join("file"), "local changes").unwrap();

        for _ in 0..2 {
            request_test_checkout(&mut gui, key, "main");
            assert_eq!(gui.remote_op_label.as_deref(), Some("Checking out"));
            wait_for_test_checkout(&mut gui);
            assert!(matches!(
                &gui.popup,
                PopupState::Message { message, kind: MessageKind::Error, .. }
                    if message.contains("would be overwritten by checkout")
            ));
            assert_checkout_head(&gui, "feature", &head);
            assert!(gui.pending_checkout_by_name.is_none());
            gui.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .unwrap();
            assert!(matches!(gui.popup, PopupState::None));
            assert_checkout_head(&gui, "feature", &head);
            assert_eq!(
                std::fs::read_to_string(repo.path().join("file")).unwrap(),
                "local changes"
            );
        }
    }
}

#[test]
fn missing_checkout_ref_preserves_head_and_offers_branch_creation() {
    let repo = TempRepo::new("missing-checkout-ref");
    let mut gui = checkout_test_gui(&repo);
    let head = repo.git(&["rev-parse", "HEAD"]);
    request_test_checkout(&mut gui, 'c', "brand-new-branch");
    wait_for_test_checkout(&mut gui);
    assert_checkout_head(&gui, "feature", &head);
    assert!(matches!(
        &gui.popup,
        PopupState::Confirm { title, .. } if title == "Branch not found"
    ));
    gui.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .unwrap();
    gui.refresh().unwrap();
    assert_checkout_head(&gui, "brand-new-branch", &head);
}

#[test]
fn tag_checkout_updates_detached_head_before_refresh() {
    let repo = TempRepo::new("tag-checkout");
    let mut gui = checkout_test_gui(&repo);
    let head = repo.git(&["rev-parse", "main"]);
    repo.git(&["tag", "release", "main"]);
    gui.model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tags = gui.git.load_tags().unwrap();
    request_test_checkout(&mut gui, 'c', "release");
    wait_for_test_checkout(&mut gui);
    assert!(gui.refresh_in_progress);
    assert!(matches!(gui.popup, PopupState::None));
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(!model.branches.iter().any(|b| b.head));
    assert!(model.head_branch_name.is_empty());
    assert_eq!(model.head_hash, head);
    assert_eq!(gui.git.head_hash().unwrap(), head);
    assert_eq!(repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "HEAD");
    drop(model);
    gui.refresh().unwrap();
    assert_eq!(
        gui.model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .head_hash,
        head
    );
}

#[test]
fn successful_checkout_updates_head_before_refresh() {
    for key in [' ', '-', 'c'] {
        let repo = TempRepo::new("successful-checkout");
        let mut gui = checkout_test_gui(&repo);
        request_test_checkout(&mut gui, key, "main");
        let pending_head = gui
            .model
            .lock()
            .unwrap()
            .branches
            .iter()
            .find(|b| b.head)
            .map(|b| b.name.clone());
        wait_for_test_checkout(&mut gui);
        assert_eq!(pending_head.as_deref(), Some("feature"));
        assert!(gui.refresh_in_progress);
        assert!(gui.initial_load_rx.is_some());
        assert!(matches!(gui.popup, PopupState::None));
        assert!(gui.pending_checkout_by_name.is_none());
        assert_checkout_head(&gui, "main", &repo.git(&["rev-parse", "main"]));
        gui.refresh().unwrap();
        assert_checkout_head(&gui, "main", &repo.git(&["rev-parse", "main"]));
    }
}

#[test]
fn checkout_discards_pre_checkout_refresh_results() {
    let repo = TempRepo::new("checkout-stale-refresh");
    let mut gui = checkout_test_gui(&repo);
    let old_head = repo.git(&["rev-parse", "HEAD"]);
    let (model_tx, model_rx) = mpsc::channel();
    let (files_tx, files_rx) = mpsc::channel();
    gui.initial_load_rx = Some(model_rx);
    gui.initial_load_received = 5;
    gui.refresh_in_progress = true;
    gui.files_refresh_rx = Some(files_rx);
    gui.files_refresh_in_progress = true;
    model_tx
        .send(ModelPart::Head {
            hash: old_head.clone(),
            branch_name: "feature".into(),
        })
        .unwrap();
    files_tx.send(Ok(Vec::new())).unwrap();

    request_test_checkout(&mut gui, ' ', "main");
    wait_for_test_checkout(&mut gui);
    assert_checkout_head(&gui, "main", &repo.git(&["rev-parse", "main"]));
    assert!(
        model_tx
            .send(ModelPart::Head {
                hash: old_head,
                branch_name: "feature".into(),
            })
            .is_err()
    );
    assert!(files_tx.send(Ok(Vec::new())).is_err());
    assert!(gui.files_refresh_rx.is_none());
    assert!(!gui.files_refresh_in_progress);
    assert!(gui.refresh_in_progress);
    assert_eq!(gui.initial_load_received, 0);
    let parts: Vec<_> = gui.initial_load_rx.take().unwrap().into_iter().collect();
    assert_eq!(parts.len(), MODEL_PART_COUNT);
    assert!(parts.iter().any(|part| matches!(
        part, ModelPart::Head { branch_name, .. } if branch_name == "main"
    )));
    assert!(parts.iter().any(|part| matches!(
        part, ModelPart::Branches(branches)
            if branches.iter().any(|b| b.head && b.name == "main")
    )));
}

#[test]
fn pending_refresh_keeps_input_polling_fast() {
    let repo = TempRepo::new("refresh-polling");
    let mut gui = checkout_test_gui(&repo);
    let mut config = crate::config::AppConfig::default();
    config.user_config.git.auto_refresh = false;
    gui.config = Arc::new(config);
    gui.needs_refresh = false;
    gui.last_input_at = Instant::now() - Duration::from_secs(60);
    assert_eq!(gui.input_wait_timeout(), Duration::from_secs(1));
    gui.needs_refresh = true;
    assert_eq!(gui.input_wait_timeout(), Duration::from_millis(16));
}

#[test]
fn wait_timeout_follows_recent_input_and_refresh_deadline() {
    let repo = TempRepo::new("wait-cadence");
    let mut gui = checkout_test_gui(&repo);
    let mut config = crate::config::AppConfig::default();
    config.user_config.git.auto_refresh = true;
    config.user_config.refresher.refresh_interval = 10;
    gui.config = Arc::new(config);
    gui.needs_refresh = false;
    gui.needs_files_refresh = false;
    gui.initial_load_rx = None;
    gui.refresh_in_progress = false;

    // Just pressed a key: stay responsive for async results.
    gui.last_input_at = Instant::now();
    assert_eq!(gui.input_wait_timeout(), Duration::from_millis(50));

    // Idle with the refresh far off: capped at 1s, not a 50ms tick.
    gui.last_input_at = Instant::now() - Duration::from_secs(60);
    gui.last_refresh_at = Instant::now();
    assert_eq!(gui.input_wait_timeout(), Duration::from_secs(1));

    // Idle with the refresh almost due: wake for it, not later.
    gui.last_refresh_at = Instant::now() - Duration::from_millis(9_700);
    let t = gui.input_wait_timeout();
    assert!(t <= Duration::from_millis(300), "{t:?}");
}

#[test]
fn interval_refresh_does_not_rearm_while_refresh_in_flight() {
    let repo = TempRepo::new("interval-refresh-in-flight");
    let mut gui = checkout_test_gui(&repo);
    let mut config = crate::config::AppConfig::default();
    config.user_config.git.auto_refresh = true;
    config.user_config.refresher.refresh_interval = 1;
    gui.config = Arc::new(config);
    gui.last_refresh_at = Instant::now() - Duration::from_secs(5);

    // In flight: the elapsed interval must not queue a duplicate reload.
    gui.needs_refresh = false;
    gui.refresh_in_progress = true;
    gui.arm_interval_refresh();
    assert!(!gui.needs_refresh);

    // Idle, git metadata unchanged: only the Files scope reloads.
    gui.refresh_in_progress = false;
    gui.initial_load_rx = None;
    gui.refs_fingerprint = gui.git.refs_fingerprint();
    gui.arm_interval_refresh();
    assert!(!gui.needs_refresh);
    assert!(
        gui.interval_files_rx.is_some(),
        "files-scope refresh started"
    );
    // ...and it restarted the interval, so the tick doesn't re-fire.
    gui.interval_files_rx = None;
    gui.arm_interval_refresh();
    assert!(gui.interval_files_rx.is_none());

    // Git metadata changed (commit, checkout, fetch…): full reload.
    gui.last_refresh_at = Instant::now() - Duration::from_secs(5);
    gui.refs_fingerprint = Some(0);
    gui.arm_interval_refresh();
    assert!(gui.needs_refresh);
}

#[test]
fn ordinary_remote_operation_keeps_normal_refresh_behavior() {
    let repo = TempRepo::new("remote-op-refresh");
    let mut gui = checkout_test_gui(&repo);
    let head = repo.git(&["rev-parse", "HEAD"]);
    gui.start_remote_op("Fetch", "", |_| Ok(()));
    wait_for_test_checkout(&mut gui);
    assert_checkout_head(&gui, "feature", &head);
    assert!(gui.needs_refresh);
    assert!(!gui.refresh_in_progress);
    assert!(gui.initial_load_rx.is_none());
    assert!(matches!(gui.popup, PopupState::None));
}

#[test]
fn successful_checkout_without_head_metadata_still_refreshes() {
    let repo = TempRepo::new("checkout-metadata-fallback");
    let mut gui = checkout_test_gui(&repo);
    repo.git(&["checkout", "main"]);
    gui.remote_op_label = Some("Checking out".into());
    gui.pending_checkout_by_name = Some("main".into());
    gui.remote_op_tx.send(Ok(None)).unwrap();
    gui.receive_remote_op_results();
    assert!(matches!(gui.popup, PopupState::None));
    assert!(gui.pending_checkout_by_name.is_none());
    assert!(gui.refresh_in_progress);
    gui.refresh().unwrap();
    assert_checkout_head(&gui, "main", &repo.git(&["rev-parse", "main"]));
}

#[test]
fn busy_checkout_does_not_change_head_or_pending_checkout() {
    for key in [' ', '-', 'c'] {
        let repo = TempRepo::new("busy-checkout");
        let mut gui = checkout_test_gui(&repo);
        let head = repo.git(&["rev-parse", "HEAD"]);
        for pending in [None, Some("already-pending".to_string())] {
            gui.remote_op_label = Some("Busy".to_string());
            gui.pending_checkout_by_name = pending.clone();
            request_test_checkout(&mut gui, key, "main");
            assert_checkout_head(&gui, "feature", &head);
            assert_eq!(gui.pending_checkout_by_name, pending);
            assert_eq!(gui.remote_op_label.as_deref(), Some("Busy"));
        }
    }
}

/// A load part that fails sends nothing. Once every sender is gone the load
/// must still finish; otherwise `refresh_in_progress` stays set and every
/// later refresh and interval tick is skipped for the rest of the session.
#[test]
fn refresh_completes_when_a_model_part_never_arrives() {
    let repo = TempRepo::new("refresh-missing-part");
    let mut gui = checkout_test_gui(&repo);
    let (model_tx, model_rx) = mpsc::channel();
    gui.initial_load_rx = Some(model_rx);
    gui.initial_load_received = 0;
    gui.refresh_in_progress = true;

    // Every part except one arrives, then all senders drop.
    model_tx
        .send(ModelPart::Head {
            hash: repo.git(&["rev-parse", "HEAD"]),
            branch_name: "main".into(),
        })
        .unwrap();
    gui.receive_initial_load_parts();
    assert!(gui.refresh_in_progress, "still waiting while senders live");
    assert!(gui.initial_load_rx.is_some());

    drop(model_tx);
    gui.receive_initial_load_parts();
    assert!(!gui.refresh_in_progress, "disconnect must finish the load");
    assert!(gui.initial_load_rx.is_none());
}
