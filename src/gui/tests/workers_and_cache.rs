use super::*;
use std::sync::atomic::AtomicUsize;

#[test]
fn split_commit_message_keeps_summary_and_body_separate() {
    assert_eq!(
        split_commit_message("feat: add editor\n\nExplain the change.\nKeep this line."),
        (
            "feat: add editor".to_string(),
            "Explain the change.\nKeep this line.".to_string()
        )
    );
}

#[test]
fn split_commit_message_handles_subject_only() {
    assert_eq!(
        split_commit_message("fix: subject only"),
        ("fix: subject only".to_string(), String::new())
    );
}

#[test]
fn keyboard_enhancement_preserves_terminal_text_input() {
    let flags = keyboard_enhancement_flags();

    assert_eq!(
        flags,
        crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
    );
    assert!(
        !flags
            .contains(crossterm::event::KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES)
    );
    assert!(!flags.contains(crossterm::event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
}

#[test]
fn mouse_capture_does_not_request_passive_motion_events() {
    let mut ansi = String::new();
    EnableMouseCaptureWithoutHover
        .write_ansi(&mut ansi)
        .unwrap();

    assert!(ansi.contains("\x1b[?1000h"));
    assert!(ansi.contains("\x1b[?1002h"));
    assert!(ansi.contains("\x1b[?1006h"));
    assert!(!ansi.contains("\x1b[?1003h"));
}

#[test]
fn latest_background_worker_coalesces_rapid_jobs() {
    let (job_tx, job_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    spawn_latest_background_worker(job_rx);

    for value in 1..=3 {
        let done_tx = done_tx.clone();
        job_tx
            .send(Box::new(move || {
                done_tx.send(value).unwrap();
            }))
            .unwrap();
    }

    assert_eq!(done_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 3);
    assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
}

#[test]
fn diff_scheduler_starts_immediately_and_keeps_latest_overflow_job() {
    let (scheduler_tx, scheduler_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let generation = Arc::new(AtomicU64::new(1));
    let executed = Arc::new(AtomicUsize::new(0));
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    spawn_diff_scheduler(
        scheduler_rx,
        scheduler_tx.clone(),
        result_tx,
        Arc::clone(&generation),
    );

    for value in 1..=3 {
        let executed = Arc::clone(&executed);
        let release_rx = Arc::clone(&release_rx);
        scheduler_tx
            .send(DiffSchedulerEvent::Job(DiffJob {
                generation: 1,
                diff_key: format!("commit:{value}"),
                load: Box::new(move || {
                    executed.fetch_or(1 << value, Ordering::Relaxed);
                    release_rx.lock().unwrap().recv().unwrap();
                    DiffPayload::Empty
                }),
            }))
            .unwrap();
    }

    let deadline = Instant::now() + Duration::from_secs(1);
    while executed.load(Ordering::Relaxed).count_ones() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(executed.load(Ordering::Relaxed).count_ones(), 2);

    release_tx.send(()).unwrap();
    let _ = result_rx.recv_timeout(Duration::from_secs(1)).unwrap();

    let deadline = Instant::now() + Duration::from_secs(1);
    while executed.load(Ordering::Relaxed) & (1 << 3) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_ne!(executed.load(Ordering::Relaxed) & (1 << 3), 0);

    release_tx.send(()).unwrap();
    release_tx.send(()).unwrap();
}

#[test]
fn diff_preview_cache_moves_recent_views_and_enforces_entry_limit() {
    let mut cache = DiffPreviewCache::default();
    for index in 0..=DIFF_PREVIEW_CACHE_ENTRIES {
        let mut view = DiffViewState::new();
        view.filename = format!("file-{index}");
        view.lines.push(crate::pager::DiffLine {
            old_line: Some((1, "old".to_string())),
            new_line: Some((1, "new".to_string())),
            change_type: crate::pager::ChangeType::Modified,
            old_segments: None,
            new_segments: None,
            file_header: None,
            comment_notes: Vec::new(),
            preview_placeholder: None,
            section_index: 0,
        });
        cache.insert(format!("key-{index}"), view);
    }

    assert_eq!(cache.entries.len(), DIFF_PREVIEW_CACHE_ENTRIES);
    assert!(cache.take("key-0").is_none());
    let restored = cache.take(&format!("key-{DIFF_PREVIEW_CACHE_ENTRIES}"));
    assert_eq!(
        restored.map(|view| view.filename),
        Some(format!("file-{DIFF_PREVIEW_CACHE_ENTRIES}"))
    );
}

#[test]
fn immutable_diff_keys_are_hash_scoped_only() {
    assert!(diff_key_is_immutable("Commits:abc123"));
    assert!(diff_key_is_immutable("Reflog:abc123"));
    assert!(diff_key_is_immutable("BranchCommits:abc123"));
    assert!(diff_key_is_immutable("Stash:abc123"));
    // Working-tree and ref-relative diffs can go stale on refresh.
    assert!(!diff_key_is_immutable("Files:file:src/main.rs"));
    assert!(!diff_key_is_immutable(
        "DiffMode:main..dev:file:src/main.rs"
    ));
    // Prefix cousins must not ride along.
    assert!(!diff_key_is_immutable("CommitFiles:abc:file:src/main.rs"));
    assert!(!diff_key_is_immutable("StashFiles:abc:file:src/main.rs"));
    assert!(!diff_key_is_immutable(
        "BranchCommitFiles:abc:file:src/main.rs"
    ));
}

#[test]
fn retain_immutable_keeps_commit_diffs_and_recomputes_bytes() {
    let mut cache = DiffPreviewCache::default();
    for key in ["Commits:abc", "Files:file:a.rs", "Stash:def"] {
        let mut view = DiffViewState::new();
        view.filename = key.to_string();
        view.lines.push(crate::pager::DiffLine {
            old_line: Some((1, "old".to_string())),
            new_line: Some((1, "new".to_string())),
            change_type: crate::pager::ChangeType::Modified,
            old_segments: None,
            new_segments: None,
            file_header: None,
            comment_notes: Vec::new(),
            preview_placeholder: None,
            section_index: 0,
        });
        cache.insert(key.to_string(), view);
    }

    cache.retain_immutable();

    assert!(cache.contains("Commits:abc"));
    assert!(cache.contains("Stash:def"));
    assert!(!cache.contains("Files:file:a.rs"));
    let expected: usize = cache.entries.iter().map(|e| e.estimated_bytes).sum();
    assert_eq!(cache.estimated_bytes, expected);
}

#[test]
fn prefetch_workers_always_deliver_a_result() {
    // begin_diff_request skips spawning an interactive job while a
    // prefetch for the key is in flight, so a lost result would leave the
    // pane loading forever.
    let (job_tx, job_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    spawn_diff_prefetch_workers(job_rx, result_tx);

    for index in 0..8 {
        job_tx
            .send(DiffPrefetchJob {
                diff_key: format!("Commits:{index}"),
                load: Box::new(|| DiffPayload::Empty),
            })
            .unwrap();
    }

    let mut keys = HashSet::new();
    for _ in 0..8 {
        let result = result_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(result.is_prefetch);
        keys.insert(result.diff_key);
    }
    assert_eq!(keys.len(), 8);
}
