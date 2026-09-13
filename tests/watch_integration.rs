//! Sift Watch test suite. Uses only tempdir/tempfile; every test isolates
//! itself from the user's real Sift data directory via
//! `watch::registry::set_test_watch_dir` (and `history::set_test_history_dir`
//! where a watch move is actually executed). Most behavior is exercised
//! against the deterministic registry/eligibility/stability/engine layer;
//! one test at the bottom drives a real `notify` watcher end to end.

use sift::config::EffectivePolicy;
use sift::domain::{HistoryItem, Op};
use sift::watch::daemon;
use sift::watch::daemon::Daemon;
use sift::watch::engine::{process_candidate, RootMonitor};
use sift::watch::registry::{
    self, add, clear_test_watch_dir, find, list, remove, set_recursive, set_test_watch_dir,
    transition, validate_root, WatchState,
};
use sift::watch::{
    cmd_watch_add, cmd_watch_list, cmd_watch_pause, cmd_watch_resume, cmd_watch_set_recursive,
    cmd_watch_start,
};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tempfile::tempdir;

/// Runs `f` with the watch registry isolated to a fresh temp directory,
/// restoring the override afterward regardless of outcome.
fn with_isolated_registry(f: impl FnOnce(&std::path::Path)) {
    let d = tempdir().unwrap();
    set_test_watch_dir(d.path().join(".sift-watch-test"));
    // Also isolate the "global config" lookup to an empty directory: any
    // test root here that has no local `.sift.toml` must resolve to
    // built-in defaults, never the real user's `~/.config/sift/config.toml`.
    let global_dir = d.path().join(".sift-global-config-test");
    std::fs::create_dir_all(&global_dir).unwrap();
    sift::config::set_test_global_config_dir(global_dir);
    f(d.path());
    sift::config::clear_test_global_config_dir();
    clear_test_watch_dir();
}

fn with_isolated_history(root: &std::path::Path, f: impl FnOnce()) {
    sift::history::set_test_history_dir(root.join(".sift-history-test"));
    f();
    sift::history::clear_test_history_dir();
}

// ============================================================
// Registry
// ============================================================

#[test]
fn registry_add_valid_watch() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox");
        fs::create_dir_all(&inbox).unwrap();
        let entry = add(inbox.canonicalize().unwrap(), true, false).unwrap();
        assert_eq!(entry.state, WatchState::Stopped);
        assert!(entry.auto_apply);
        assert!(!entry.recursive);
    });
}

#[test]
fn registry_duplicate_add_rejected() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, false).unwrap();
        let err = add(inbox, true, false).unwrap_err();
        assert!(err.contains("already"));
    });
}

#[test]
fn registry_add_requires_auto_apply() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        let err = add(inbox, false, false).unwrap_err();
        assert!(err.contains("--auto-apply"));
    });
}

#[test]
fn registry_set_recursive_updates_an_existing_entry() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, false).unwrap();
        let entry = set_recursive(&inbox, true).unwrap();
        assert!(entry.recursive);
        // Confirms it's actually persisted, not just returned.
        assert!(find(&inbox).unwrap().unwrap().recursive);
    });
}

#[test]
fn registry_set_recursive_errors_for_unregistered_path() {
    with_isolated_registry(|root| {
        let never_added = root.join("never-added").canonicalize_dir();
        let err = set_recursive(&never_added, true).unwrap_err();
        assert!(err.contains("not a registered watch"));
    });
}

#[test]
fn registry_symlink_root_rejected() {
    use std::os::unix::fs::symlink;
    with_isolated_registry(|root| {
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        let link = root.join("link");
        symlink(&real, &link).unwrap();
        let err = validate_root(&link).unwrap_err();
        assert!(err.contains("symlink"));
    });
}

#[test]
fn registry_project_root_rejected() {
    with_isolated_registry(|root| {
        let proj = root.join("app");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("package.json"), b"{}").unwrap();
        let err = validate_root(&proj).unwrap_err();
        assert!(err.contains("project"));
    });
}

#[test]
fn registry_filesystem_root_rejected() {
    let err = validate_root(std::path::Path::new("/")).unwrap_err();
    assert!(err.contains("filesystem root"));
}

#[test]
fn registry_category_dir_rejected() {
    with_isolated_registry(|root| {
        let cat = root.join("Documents");
        fs::create_dir_all(&cat).unwrap();
        let err = validate_root(&cat).unwrap_err();
        assert!(err.contains("category"));
    });
}

#[test]
fn registry_remove() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, false).unwrap();
        assert!(find(&inbox).unwrap().is_some());
        remove(&inbox).unwrap();
        assert!(find(&inbox).unwrap().is_none());
    });
}

#[test]
fn registry_remove_unknown_is_an_error() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        assert!(remove(&inbox).is_err());
    });
}

#[test]
fn registry_persists_across_separate_calls() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, true).unwrap();
        // A fresh `list()` call re-reads from disk each time (no in-memory
        // cache), which is what "persists" means for a design shared by two
        // separate OS processes.
        let all = list().unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].recursive);
    });
}

#[test]
fn registry_deserializes_pre_readiness_format_missing_generation_fields() {
    // A registry.json written by a Sift version before `run_generation`/
    // `monitoring_generation` existed. Old installs must upgrade in
    // place without losing (or failing to load) their existing watches.
    with_isolated_registry(|_root| {
        let old_format = r#"{
  "watches": [
    {
      "path": "/tmp/old-watch",
      "state": "running",
      "auto_apply": true,
      "recursive": false,
      "created_at": 1700000000,
      "updated_at": 1700000000
    }
  ]
}"#;
        let registry_path = registry::watch_dir().join("registry.json");
        fs::create_dir_all(registry_path.parent().unwrap()).unwrap();
        fs::write(&registry_path, old_format).unwrap();

        let all = list().unwrap();
        assert_eq!(all.len(), 1, "an old-format entry must still load");
        let entry = &all[0];
        assert_eq!(entry.path, PathBuf::from("/tmp/old-watch"));
        assert_eq!(entry.state, WatchState::Running);
        assert_eq!(
            entry.run_generation, 0,
            "a missing run_generation must default to 0, not fail to parse"
        );
        assert_eq!(
            entry.monitoring_generation, None,
            "a missing monitoring_generation must default to None, not fail to parse"
        );

        // And the registry must still be writable afterward — loading an
        // old entry must not corrupt the file or wedge future writes.
        let found = find(&PathBuf::from("/tmp/old-watch")).unwrap().unwrap();
        assert_eq!(found.run_generation, 0);
        let promoted = transition(&PathBuf::from("/tmp/old-watch"), WatchState::Paused).unwrap();
        assert_eq!(promoted.state, WatchState::Paused);
    });
}

#[test]
fn registry_concurrent_writes_are_not_lost() {
    with_isolated_registry(|root| {
        for i in 0..20 {
            fs::create_dir_all(root.join(format!("w{i}"))).unwrap();
        }
        let dir = registry::watch_dir();
        let handles: Vec<_> = (0..20)
            .map(|i| {
                let p = root.join(format!("w{i}")).canonicalize_dir();
                let dir = dir.clone();
                std::thread::spawn(move || {
                    set_test_watch_dir(dir);
                    add(p, true, false).unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let all = list().unwrap();
        assert_eq!(
            all.len(),
            20,
            "every concurrent add must be preserved, none lost to a lock race"
        );
    });
}

// ============================================================
// State transitions
// ============================================================

#[test]
fn state_stopped_to_running() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, false).unwrap();
        let e = transition(&inbox, WatchState::Running).unwrap();
        assert_eq!(e.state, WatchState::Running);
    });
}

#[test]
fn state_running_to_paused() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, false).unwrap();
        transition(&inbox, WatchState::Running).unwrap();
        let e = transition(&inbox, WatchState::Paused).unwrap();
        assert_eq!(e.state, WatchState::Paused);
    });
}

#[test]
fn state_paused_to_running() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, false).unwrap();
        transition(&inbox, WatchState::Running).unwrap();
        transition(&inbox, WatchState::Paused).unwrap();
        let e = transition(&inbox, WatchState::Running).unwrap();
        assert_eq!(e.state, WatchState::Running);
    });
}

#[test]
fn state_running_to_stopped() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, false).unwrap();
        transition(&inbox, WatchState::Running).unwrap();
        let e = transition(&inbox, WatchState::Stopped).unwrap();
        assert_eq!(e.state, WatchState::Stopped);
    });
}

#[test]
fn state_invalid_transition_rejected() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, false).unwrap();
        // stopped -> paused is not a valid direct transition.
        assert!(transition(&inbox, WatchState::Paused).is_err());
    });
}

#[test]
fn state_remove_while_running_succeeds() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, false).unwrap();
        transition(&inbox, WatchState::Running).unwrap();
        assert!(remove(&inbox).is_ok());
        assert!(find(&inbox).unwrap().is_none());
    });
}

#[test]
fn state_persists_across_reload() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox").canonicalize_dir();
        add(inbox.clone(), true, false).unwrap();
        transition(&inbox, WatchState::Running).unwrap();
        // Simulate a fresh process by just re-reading the registry.
        let reloaded = find(&inbox).unwrap().unwrap();
        assert_eq!(reloaded.state, WatchState::Running);
    });
}

// ============================================================
// Daemon reconciliation (no real background process needed)
// ============================================================

#[test]
fn daemon_reconcile_only_watches_running_entries() {
    with_isolated_registry(|root| {
        let running = root.join("running").canonicalize_dir();
        let paused = root.join("paused").canonicalize_dir();
        let stopped = root.join("stopped").canonicalize_dir();
        add(running.clone(), true, false).unwrap();
        add(paused.clone(), true, false).unwrap();
        add(stopped.clone(), true, false).unwrap();
        transition(&running, WatchState::Running).unwrap();
        transition(&paused, WatchState::Running).unwrap();
        transition(&paused, WatchState::Paused).unwrap();

        let mut d = Daemon::new().unwrap();
        d.reconcile();
        assert!(d.is_monitoring(&running));
        assert!(!d.is_monitoring(&paused));
        assert!(!d.is_monitoring(&stopped));
    });
}

#[test]
fn daemon_reconcile_drops_monitor_when_watch_stops() {
    with_isolated_registry(|root| {
        let w = root.join("w").canonicalize_dir();
        add(w.clone(), true, false).unwrap();
        transition(&w, WatchState::Running).unwrap();

        let mut d = Daemon::new().unwrap();
        d.reconcile();
        assert!(d.is_monitoring(&w));

        transition(&w, WatchState::Stopped).unwrap();
        d.reconcile();
        assert!(!d.is_monitoring(&w));
    });
}

#[test]
fn daemon_reconcile_rebuilds_monitor_when_recursive_changes_while_running() {
    with_isolated_registry(|root| {
        let w = root.join("w").canonicalize_dir();
        add(w.clone(), true, false).unwrap();
        transition(&w, WatchState::Running).unwrap();

        let mut d = Daemon::new().unwrap();
        d.reconcile();
        assert_eq!(d.monitor_recursive(&w), Some(false));

        // Toggled live, the same way sift-tray's "Recursive" checkbox
        // does — the watch stays Running throughout, no pause/resume.
        sift::watch::registry::set_recursive(&w, true).unwrap();
        d.reconcile();
        assert_eq!(
            d.monitor_recursive(&w),
            Some(true),
            "reconcile must rebuild the RootMonitor (and its notify watch mode) \
             when recursive changes for an already-running watch, not keep the stale one"
        );
    });
}

#[test]
fn daemon_start_does_not_backfill_existing_files() {
    with_isolated_registry(|root| {
        let w = root.join("w").canonicalize_dir();
        fs::write(w.join("preexisting.jpg"), b"x").unwrap();
        add(w.clone(), true, false).unwrap();
        transition(&w, WatchState::Running).unwrap();

        with_isolated_history(root, || {
            let mut d = Daemon::new().unwrap();
            // Reconciling and immediately processing (no events observed)
            // must never sweep pre-existing files.
            d.reconcile();
            d.process_ready();
            assert!(w.join("preexisting.jpg").exists());
            assert!(!w.join("Images").exists());
        });
    });
}

#[test]
fn daemon_singleton_lock_prevents_second_daemon() {
    with_isolated_registry(|_root| {
        let lock_path = registry::daemon_lock_path();
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let f1 = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        f1.lock().unwrap();

        let f2 = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        let second = f2.try_lock();
        assert!(
            matches!(second, Err(std::fs::TryLockError::WouldBlock)),
            "a second daemon must never acquire the singleton lock while the first holds it"
        );
        f1.unlock().unwrap();
    });
}

#[test]
fn daemon_status_reports_not_running_when_lock_is_free() {
    with_isolated_registry(|_root| {
        assert!(matches!(
            sift::watch::daemon::daemon_status(),
            sift::watch::daemon::DaemonStatus::NotRunning
        ));
    });
}

#[test]
fn tray_singleton_lock_prevents_second_tray() {
    with_isolated_registry(|_root| {
        let lock_path = registry::tray_lock_path();
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let f1 = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        f1.lock().unwrap();

        let f2 = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        let second = f2.try_lock();
        assert!(
            matches!(second, Err(std::fs::TryLockError::WouldBlock)),
            "a second sift-tray must never acquire the singleton lock while the first holds it"
        );
        f1.unlock().unwrap();
    });
}

#[test]
fn tray_ensure_running_best_effort_is_a_safe_noop_outside_the_cli_binary() {
    // The test binary is never literally named `sift`, so this must
    // short-circuit before touching the lock file at all — it must never
    // panic, block, or attempt to spawn a real process from a test run.
    with_isolated_registry(|_root| {
        sift::watch::tray::ensure_running_best_effort();
        assert!(
            !registry::tray_lock_path().exists(),
            "must not even open the tray lock file when not called from the `sift` binary"
        );
    });
}

// ============================================================
// Event eligibility / ancestor protection via the engine
// ============================================================

#[test]
fn engine_ignores_symlink_directory_ancestor() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let root = d.path();
    let outside = tempdir().unwrap();
    symlink(outside.path(), root.join("linked")).unwrap();
    // A candidate "inside" the symlinked directory can't really be placed
    // there without following the link ourselves, so directly exercise the
    // ancestor check: revalidate must refuse once "linked" is the parent.
    let fake_candidate = root.join("linked").join("x.jpg");
    let result = sift::scanner::revalidate_candidate(root, &fake_candidate);
    assert!(result.is_err());
}

#[test]
fn engine_ignores_dot_git_ancestor() {
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git/config"), b"x").unwrap();
    let result = sift::scanner::revalidate_candidate(root, &root.join(".git/config"));
    assert!(result.is_err());
}

#[test]
fn engine_ignores_node_modules_ancestor() {
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    fs::write(root.join("node_modules/pkg/index.js"), b"x").unwrap();
    let result = sift::scanner::revalidate_candidate(root, &root.join("node_modules/pkg/index.js"));
    assert!(result.is_err());
}

#[test]
fn engine_ignores_venv_ancestor() {
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join(".venv/lib")).unwrap();
    fs::write(root.join(".venv/lib/x.py"), b"x").unwrap();
    let result = sift::scanner::revalidate_candidate(root, &root.join(".venv/lib/x.py"));
    assert!(result.is_err());
}

#[test]
fn engine_ignores_target_ancestor() {
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("target/debug")).unwrap();
    fs::write(root.join("target/debug/bin"), b"x").unwrap();
    let result = sift::scanner::revalidate_candidate(root, &root.join("target/debug/bin"));
    assert!(result.is_err());
}

#[test]
fn engine_ignores_category_directory_ancestor() {
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("Images")).unwrap();
    fs::write(root.join("Images/again.jpg"), b"x").unwrap();
    // This is exactly the self-generated-loop scenario: Sift itself just
    // moved a file into Images/, and a resulting event must not re-trigger.
    let result = sift::scanner::revalidate_candidate(root, &root.join("Images/again.jpg"));
    assert!(result.is_err());
}

#[test]
fn engine_transient_download_name_is_never_tracked() {
    let root = PathBuf::from("/tmp/inbox-watch-test");
    let mut m = RootMonitor::new(root.clone(), false);
    let now = Instant::now();
    m.observe_event(&root.join("movie.mp4.crdownload"), now);
    m.observe_event(&root.join("archive.zip.part"), now);
    assert_eq!(m.pending_count(), 0);
    // Once renamed to a final name, the (separate) event is tracked.
    m.observe_event(&root.join("movie.mp4"), now);
    assert_eq!(m.pending_count(), 1);
}

#[test]
fn engine_pause_discards_pending_candidate() {
    let root = PathBuf::from("/tmp/inbox-watch-test-2");
    let mut m = RootMonitor::new(root.clone(), false);
    let now = Instant::now();
    m.observe_event(&root.join("a.jpg"), now);
    assert_eq!(m.pending_count(), 1);
    m.discard_pending(); // what the daemon does on pause/stop
    assert_eq!(m.pending_count(), 0);
    let ready = m.poll_ready(now + Duration::from_secs(30), Duration::from_millis(1));
    assert!(
        ready.is_empty(),
        "a discarded candidate must never later become ready"
    );
}

// ============================================================
// Organization coverage via process_candidate
// ============================================================

#[test]
fn watched_json_goes_to_data() {
    let d = tempdir().unwrap();
    let root = d.path();
    let f = root.join("data.json");
    fs::write(&f, b"{}").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(outcome.organized);
        assert!(root.join("Data/data.json").exists());
    });
}

#[test]
fn watched_unknown_extension_goes_to_other() {
    let d = tempdir().unwrap();
    let root = d.path();
    let f = root.join("mystery.xyz");
    fs::write(&f, b"x").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(outcome.organized);
        assert!(root.join("Other/mystery.xyz").exists());
    });
}

#[test]
fn watched_blend_goes_to_3d() {
    let d = tempdir().unwrap();
    let root = d.path();
    let f = root.join("model.blend");
    fs::write(&f, b"x").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(outcome.organized);
        assert!(root.join("3D/model.blend").exists());
    });
}

#[test]
fn watched_config_rule_overrides_builtin_category() {
    let d = tempdir().unwrap();
    let root = d.path();
    let f = root.join("script.js");
    fs::write(&f, b"x").unwrap();
    let rules = vec![sift::config::Rule {
        name: "scripts".into(),
        pattern: "*.js".into(),
        action: "Move".into(),
        destination: Some("Scripts".into()),
        priority: 1,
        enabled: true,
        description: None,
    }];
    let policy = EffectivePolicy::default().with_rules(rules);
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &policy,
            &policy,
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(outcome.organized);
        assert!(root.join("Scripts/script.js").exists());
        assert!(!root.join("Code/script.js").exists());
    });
}

#[test]
fn process_candidate_ancestor_category_dir_with_no_override_is_a_dead_end() {
    // The self-generated-loop protection this guards must keep working:
    // a file already sitting in a reserved category directory that has no
    // `.sift.toml` of its own must never be reprocessed.
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("Images")).unwrap();
    let f = root.join("Images").join("already-here.jpg");
    fs::write(&f, b"x").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(!outcome.organized);
        assert_eq!(
            outcome.skip_reason.as_deref(),
            Some("ancestor directory is protected")
        );
        assert!(
            f.exists(),
            "must never move a file already sitting in its own category's directory"
        );
    });
}

#[test]
fn process_candidate_lets_a_nested_sift_toml_govern_a_directory_named_like_a_builtin_category() {
    // Real-world scenario this regression-tests: the watch daemon's own
    // `type` strategy just moved an image into `Images/`, which has its
    // own `.sift.toml` splitting things further by extension. Before this
    // fix, `Images` being a reserved category name made the boundary
    // check treat it as a dead end unconditionally, so the daemon's own
    // subsequent event for the file landing there was silently dropped
    // and the nested config never got a chance to run — exactly the bug
    // this reproduces (`root_policy` is root's own, unrelated policy;
    // `policy` is what the daemon would already have resolved for this
    // file's containing directory via `resolve_nested_policy_override`).
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("Images")).unwrap();
    fs::write(
        root.join("Images").join(".sift.toml"),
        "[organize]\nstrategy = \"type\"\nunknown = \"other\"\n\n\
         [[rules]]\nenabled = true\npattern = \"*.png\"\naction = \"Move\"\ndestination = \"PNG\"\npriority = 100\n",
    )
    .unwrap();
    let f = root.join("Images").join("photo.png");
    fs::write(&f, b"x").unwrap();
    let (nested_policy, owner) =
        sift::config::resolve_nested_policy_override(root, &root.join("Images"))
            .expect("Images/ declares its own .sift.toml")
            .expect("that .sift.toml is valid");
    assert_eq!(owner, root.join("Images"));

    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &nested_policy,
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(outcome.organized);
        assert!(root.join("Images/PNG/photo.png").exists());
    });
}

#[test]
fn watched_explicit_createdir_is_recorded_in_history() {
    let d = tempdir().unwrap();
    let root = d.path();
    let f = root.join("photo.jpg");
    fs::write(&f, b"x").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        let id = outcome.history_id.unwrap();
        let hist_dir = root.join(".sift-history-test");
        let content = fs::read_to_string(hist_dir.join(format!("{id}.json"))).unwrap();
        let item: HistoryItem = serde_json::from_str(&content).unwrap();
        assert!(item.actions.iter().any(|a| a.op == Op::CreateDir));
    });
}

#[test]
fn watched_collision_with_different_content_is_renamed_never_overwrites() {
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("Images")).unwrap();
    fs::write(root.join("Images/photo.jpg"), b"existing").unwrap();
    let f = root.join("photo.jpg");
    fs::write(&f, b"new").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(outcome.organized);
        assert!(!f.exists());
        assert_eq!(
            fs::read_to_string(root.join("Images/photo.jpg")).unwrap(),
            "existing",
            "the pre-existing file at the colliding name must never be touched"
        );
        assert_eq!(
            fs::read_to_string(root.join("Images/photo (1).jpg")).unwrap(),
            "new",
            "the new file must land at a disambiguated name instead of being dropped"
        );
    });
}

#[test]
fn watched_collision_with_identical_content_trashes_the_duplicate() {
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("Images")).unwrap();
    fs::write(root.join("Images/photo.jpg"), b"same bytes").unwrap();
    let f = root.join("photo.jpg");
    fs::write(&f, b"same bytes").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(!outcome.organized, "a trash is not a move");
        assert!(
            !f.exists(),
            "the redundant duplicate must be gone once trashed"
        );
        assert_eq!(
            fs::read_to_string(root.join("Images/photo.jpg")).unwrap(),
            "same bytes",
            "the already-organized copy must be untouched"
        );
    });
}

#[test]
fn watched_broken_symlink_collision_never_overwrites() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("Images")).unwrap();
    symlink("/nonexistent", root.join("Images/photo.jpg")).unwrap();
    let f = root.join("photo.jpg");
    fs::write(&f, b"new").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(!outcome.organized);
        assert_eq!(outcome.skip_reason.as_deref(), Some("collision"));
        assert!(f.exists());
    });
}

#[test]
fn watched_toctou_collision_is_refused_and_recorded() {
    // A genuine plan-then-race: `process_candidate` bundles planning and
    // execution together with no seam to inject a race between them (and
    // by now, a same-name collision found *at planning time* is resolved
    // proactively — trashed or renamed — never deferred to execution).
    // So this drives the same two steps `process_candidate` does, but
    // separately, planning first while the destination is still clear,
    // *then* introducing the race, exactly like
    // `test_recursive_executor_toctou_collision_in_nested_dir` does for
    // recursive organize.
    use sift::planner::plan_entry_with_strategy;
    let d = tempdir().unwrap();
    let root = d.path();
    let f = root.join("race.jpg");
    fs::write(&f, b"x").unwrap();
    let entry = sift::scanner::revalidate_candidate(root, &f).unwrap();
    let entry_plan = plan_entry_with_strategy(
        &entry,
        root,
        &EffectivePolicy::default(),
        &sift::classifier::CategoryDB::default(),
    );
    assert_eq!(
        entry_plan.action.op,
        Op::Move,
        "nothing occupies Images/race.jpg yet"
    );

    // Race: the destination appears after planning but before execution.
    fs::create_dir_all(root.join("Images")).unwrap();
    fs::write(root.join("Images/race.jpg"), b"existing").unwrap();

    with_isolated_history(root, || {
        let mut actions = entry_plan.create_dirs;
        actions.push(entry_plan.action);
        let (_id, outcomes) = sift::executor::execute_plan(
            sift::domain::Plan { actions },
            root.to_str().unwrap(),
            "organize",
            Some(root),
        );
        let move_outcome = outcomes.iter().find(|o| o.op == Op::Move).unwrap();
        assert!(
            move_outcome.result.is_err(),
            "the race must be caught at execution time"
        );
        assert!(f.exists(), "a failed move must leave the source untouched");
        assert_eq!(
            fs::read_to_string(root.join("Images/race.jpg")).unwrap(),
            "existing"
        );
    });
}

#[test]
fn watched_symlink_ancestor_introduced_after_planning_refused() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let root = d.path();
    let outside = tempdir().unwrap();
    let f = root.join("file.xyz");
    fs::write(&f, b"x").unwrap();
    // "Other" would normally be created as a real directory; here it's
    // already a symlink escape target before we ever process the event.
    symlink(outside.path(), root.join("Other")).unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(!outcome.organized);
        assert_eq!(
            std::fs::read_dir(outside.path()).unwrap().count(),
            0,
            "must never write through the symlink"
        );
        assert!(f.exists());
    });
}

// ============================================================
// Recursive watch
// ============================================================

#[test]
fn recursive_watch_organizes_in_local_containing_directory() {
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("Client")).unwrap();
    let f = root.join("Client/invoice.pdf");
    fs::write(&f, b"x").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(outcome.organized);
        assert!(root.join("Client/Documents/invoice.pdf").exists());
        assert!(!root.join("Documents").exists());
    });
}

#[test]
fn recursive_watch_protects_newly_created_project_subtree() {
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("app/src")).unwrap();
    let f = root.join("app/src/index.js");
    fs::write(&f, b"x").unwrap();
    fs::write(root.join("app/package.json"), b"{}").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(!outcome.organized);
        assert!(f.exists());
    });
}

// ============================================================
// History / undo integration
// ============================================================

#[test]
fn watch_move_records_origin_metadata() {
    let d = tempdir().unwrap();
    let root = d.path();
    let f = root.join("photo.jpg");
    fs::write(&f, b"x").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        let id = outcome.history_id.unwrap();
        let content =
            fs::read_to_string(root.join(".sift-history-test").join(format!("{id}.json"))).unwrap();
        let item: HistoryItem = serde_json::from_str(&content).unwrap();
        assert_eq!(item.origin, "watch");
        assert_eq!(item.watch_root.as_deref(), Some(root));
    });
}

#[test]
fn watch_move_can_be_undone_normally() {
    let d = tempdir().unwrap();
    let root = d.path();
    let f = root.join("photo.jpg");
    fs::write(&f, b"x").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(root.join("Images/photo.jpg").exists());
        sift::history::cmd_undo(outcome.history_id.unwrap());
        assert!(f.exists());
        assert!(!root.join("Images/photo.jpg").exists());
    });
}

#[test]
fn watch_blocked_collision_is_not_recorded_and_not_undoable() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("Images")).unwrap();
    symlink("/nonexistent", root.join("Images/photo.jpg")).unwrap();
    let f = root.join("photo.jpg");
    fs::write(&f, b"new").unwrap();
    with_isolated_history(root, || {
        // A symlink at the destination is still genuinely blocked (never
        // compared, never disambiguated) — this is a plan-time collision
        // skip, so no history is written at all (nothing was executed).
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(outcome.history_id.is_none());
        assert!(outcome.failure.is_none());
        assert_eq!(outcome.skip_reason.as_deref(), Some("collision"));
    });
}

// ============================================================
// Start/resume readiness (the daemon-monitoring race)
//
// These drive `cmd_watch_start`/`cmd_watch_resume` themselves — the real
// production entry points, including `daemon::ensure_running` and the new
// `daemon::wait_until_monitoring` — against a real `Daemon` (real
// `notify` watcher, real singleton OS lock) run on a background thread
// via `daemon::run()` instead of a spawned OS process, so no actual
// `sift` binary needs to be reachable from the test process. Because the
// test overrides (`set_test_watch_dir` etc.) are thread-local, the
// spawned thread re-applies them for itself before calling `daemon::run`
// — everything still goes through the same on-disk registry/lock files,
// which is the only channel the real CLI-process/daemon-process split
// uses anyway.
// ============================================================

/// Owns a background thread running the real `daemon::run()` loop, for
/// exactly one test's isolated watch dir. `spawn` does not return until
/// the thread has actually acquired the singleton daemon lock, so a test
/// calling `cmd_watch_start` right after never races `ensure_running`
/// into trying to spawn a *real* second daemon process.
struct TestDaemon {
    handle: Option<std::thread::JoinHandle<()>>,
}

impl TestDaemon {
    fn spawn(watch_dir: PathBuf, global_config_dir: PathBuf, history_dir: PathBuf) -> Self {
        let handle = std::thread::spawn(move || {
            set_test_watch_dir(watch_dir);
            sift::config::set_test_global_config_dir(global_config_dir);
            sift::history::set_test_history_dir(history_dir);
            let _ = daemon::run();
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if matches!(
                daemon::daemon_status(),
                daemon::DaemonStatus::Running { .. }
            ) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            matches!(
                daemon::daemon_status(),
                daemon::DaemonStatus::Running { .. }
            ),
            "test daemon thread did not acquire the singleton lock in time"
        );
        Self {
            handle: Some(handle),
        }
    }

    fn stop(mut self) {
        let _ = daemon::request_stop_and_wait(Duration::from_secs(5));
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            let _ = daemon::request_stop_and_wait(Duration::from_secs(5));
            let _ = h.join();
        }
    }
}

/// Waits (bounded) for `path` to exist. Used only to observe the eventual
/// effect of a background daemon organizing a file — never to paper over
/// the readiness race itself, which is proven by there being no delay
/// between the triggering `cmd_watch_start`/`cmd_watch_resume` call
/// returning and the file being created.
fn wait_for(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    path.exists()
}

#[test]
fn cli_watch_start_is_not_racy_a_file_created_immediately_after_return_is_organized() {
    let d = tempdir().unwrap();
    let watch_dir = d.path().join(".sift-watch-test");
    let global_dir = d.path().join(".sift-global-config-test");
    fs::create_dir_all(&global_dir).unwrap();
    let root = d.path().join("Inbox");
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let history_dir = root.join(".sift-history-test");

    set_test_watch_dir(watch_dir.clone());
    sift::config::set_test_global_config_dir(global_dir.clone());
    sift::history::set_test_history_dir(history_dir.clone());

    let test_daemon = TestDaemon::spawn(watch_dir.clone(), global_dir.clone(), history_dir.clone());

    assert!(cmd_watch_add(
        root.to_string_lossy().to_string(),
        true,
        false
    ));

    assert!(
        cmd_watch_start(root.to_string_lossy().to_string()),
        "start must succeed once the daemon confirms it is actually monitoring the root"
    );

    // The invariant under test: no delay here at all. If this file's
    // create event could still race an as-yet-uninstalled `notify`
    // watcher, it would never be organized (Watch never backfills).
    let photo = root.join("photo.jpg");
    fs::write(&photo, b"x").unwrap();

    assert!(
        wait_for(&root.join("Images/photo.jpg"), Duration::from_secs(10)),
        "a file created immediately after a successful `watch start` must be organized"
    );

    test_daemon.stop();
    clear_test_watch_dir();
    sift::config::clear_test_global_config_dir();
    sift::history::clear_test_history_dir();
}

#[test]
fn cli_watch_resume_is_not_racy_and_still_never_backfills() {
    let d = tempdir().unwrap();
    let watch_dir = d.path().join(".sift-watch-test");
    let global_dir = d.path().join(".sift-global-config-test");
    fs::create_dir_all(&global_dir).unwrap();
    let root = d.path().join("Inbox");
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let history_dir = root.join(".sift-history-test");
    // Present before the watch is ever started — per the "no backfill"
    // rule, this must never be organized, including across a later
    // pause/resume cycle.
    let preexisting = root.join("preexisting.jpg");
    fs::write(&preexisting, b"x").unwrap();

    set_test_watch_dir(watch_dir.clone());
    sift::config::set_test_global_config_dir(global_dir.clone());
    sift::history::set_test_history_dir(history_dir.clone());

    let test_daemon = TestDaemon::spawn(watch_dir.clone(), global_dir.clone(), history_dir.clone());

    assert!(cmd_watch_add(
        root.to_string_lossy().to_string(),
        true,
        false
    ));
    assert!(cmd_watch_start(root.to_string_lossy().to_string()));
    assert!(cmd_watch_pause(root.to_string_lossy().to_string()));
    assert!(
        cmd_watch_resume(root.to_string_lossy().to_string()),
        "resume must succeed once the daemon confirms it is actually monitoring the root again"
    );

    // The invariant under test: no delay here at all.
    let after_resume = root.join("after-resume.jpg");
    fs::write(&after_resume, b"y").unwrap();

    assert!(
        wait_for(
            &root.join("Images/after-resume.jpg"),
            Duration::from_secs(10)
        ),
        "a file created immediately after a successful `watch resume` must be organized"
    );
    assert!(
        preexisting.exists(),
        "a file that predates `watch start` must never be backfilled, even across a \
         later pause/resume cycle"
    );
    assert!(!root.join("Images/preexisting.jpg").exists());

    test_daemon.stop();
    clear_test_watch_dir();
    sift::config::clear_test_global_config_dir();
    sift::history::clear_test_history_dir();
}

#[test]
fn cli_watch_start_times_out_and_reverts_to_stopped_when_daemon_never_confirms_readiness() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox");
        fs::create_dir_all(&inbox).unwrap();
        let inbox = inbox.canonicalize().unwrap();
        add(inbox.clone(), true, false).unwrap();

        // Simulate a daemon *process* that is alive (holds the singleton
        // lock, so `ensure_running` never tries to spawn a second one)
        // but stuck — it never reconciles, so this root can never
        // actually become monitored. `watch start` must not be fooled by
        // the process merely being alive.
        let lock_path = registry::daemon_lock_path();
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        lock_file.lock().unwrap();

        let ok = cmd_watch_start(inbox.to_string_lossy().to_string());
        assert!(
            !ok,
            "start must fail rather than report success when readiness can't be confirmed"
        );

        let entry = find(&inbox).unwrap().unwrap();
        assert_eq!(
            entry.state,
            WatchState::Stopped,
            "a failed start must revert to the previous (stopped) state, never leave the \
             registry claiming running while nothing is actually watching"
        );

        lock_file.unlock().unwrap();
    });
}

#[test]
fn cli_watch_pause_does_not_return_until_the_daemon_confirms_teardown() {
    let d = tempdir().unwrap();
    let watch_dir = d.path().join(".sift-watch-test");
    let global_dir = d.path().join(".sift-global-config-test");
    fs::create_dir_all(&global_dir).unwrap();
    let root = d.path().join("Inbox");
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let history_dir = root.join(".sift-history-test");

    set_test_watch_dir(watch_dir.clone());
    sift::config::set_test_global_config_dir(global_dir.clone());
    sift::history::set_test_history_dir(history_dir.clone());

    let test_daemon = TestDaemon::spawn(watch_dir.clone(), global_dir.clone(), history_dir.clone());

    assert!(cmd_watch_add(
        root.to_string_lossy().to_string(),
        true,
        false
    ));
    assert!(cmd_watch_start(root.to_string_lossy().to_string()));
    let running_generation = find(&root).unwrap().unwrap().run_generation;

    assert!(
        cmd_watch_pause(root.to_string_lossy().to_string()),
        "pause must succeed once the daemon confirms it actually tore the monitor down"
    );

    // The contract under test: by the moment `cmd_watch_pause` returns,
    // the daemon has *already* acknowledged tearing down this exact
    // generation's monitor — not merely that the registry file says
    // "paused".
    let paused = find(&root).unwrap().unwrap();
    assert_eq!(paused.state, WatchState::Paused);
    assert_eq!(
        paused.torn_down_generation,
        Some(running_generation),
        "cmd_watch_pause must not return success before the daemon has acknowledged \
         tearing down this exact run_generation"
    );

    test_daemon.stop();
    clear_test_watch_dir();
    sift::config::clear_test_global_config_dir();
    sift::history::clear_test_history_dir();
}

#[test]
fn cli_watch_pause_times_out_but_leaves_the_requested_state_when_daemon_never_confirms_teardown() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox");
        fs::create_dir_all(&inbox).unwrap();
        let inbox = inbox.canonicalize().unwrap();
        add(inbox.clone(), true, false).unwrap();
        transition(&inbox, WatchState::Running).unwrap();

        // Simulate a daemon *process* that is alive (holds the singleton
        // lock, so `cmd_watch_pause` doesn't take the "no daemon at all"
        // shortcut) but stuck — it never reconciles, so it can never
        // acknowledge tearing this root's monitor down.
        let lock_path = registry::daemon_lock_path();
        fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        lock_file.lock().unwrap();

        let ok = cmd_watch_pause(inbox.to_string_lossy().to_string());
        assert!(
            !ok,
            "pause must fail rather than report success when teardown can't be confirmed"
        );

        let entry = find(&inbox).unwrap().unwrap();
        assert_eq!(
            entry.state,
            WatchState::Paused,
            "the requested state is still honored even though the daemon hasn't caught up \
             yet — unlike start/resume, there is no safer state to revert pause/stop to"
        );

        lock_file.unlock().unwrap();
    });
}

// ============================================================
// CLI parsing
// ============================================================

#[test]
fn cli_parses_all_watch_commands() {
    use clap::Parser;
    use sift::cli::Cli;
    let cases = [
        vec!["sift", "watch", "add", "/tmp/x", "--auto-apply"],
        vec![
            "sift",
            "watch",
            "add",
            "/tmp/x",
            "--auto-apply",
            "--recursive",
        ],
        vec!["sift", "watch", "list"],
        vec!["sift", "watch", "status"],
        vec!["sift", "watch", "status", "/tmp/x"],
        vec!["sift", "watch", "start", "/tmp/x"],
        vec!["sift", "watch", "pause", "/tmp/x"],
        vec!["sift", "watch", "resume", "/tmp/x"],
        vec!["sift", "watch", "stop", "/tmp/x"],
        vec!["sift", "watch", "remove", "/tmp/x"],
        vec!["sift", "watch", "daemon", "status"],
        vec!["sift", "watch", "daemon", "stop"],
        vec!["sift", "watch", "daemon", "run"],
    ];
    for case in cases {
        assert!(
            Cli::try_parse_from(&case).is_ok(),
            "failed to parse: {case:?}"
        );
    }
}

#[test]
fn cli_watch_add_without_auto_apply_is_rejected_by_handler() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox");
        fs::create_dir_all(&inbox).unwrap();
        let ok = cmd_watch_add(inbox.to_string_lossy().to_string(), false, false);
        assert!(!ok);
        assert!(list().unwrap().is_empty());
    });
}

// ============================================================
// cmd_watch_set_recursive (sift-tray's "Recursive" toggle)
// ============================================================

#[test]
fn cmd_watch_set_recursive_enables_for_a_type_strategy_watch() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox");
        fs::create_dir_all(&inbox).unwrap();
        let inbox = inbox.canonicalize().unwrap();
        add(inbox.clone(), true, false).unwrap();

        let ok = cmd_watch_set_recursive(inbox.to_string_lossy().to_string(), true);
        assert!(ok);
        assert!(find(&inbox).unwrap().unwrap().recursive);
    });
}

#[test]
fn cmd_watch_set_recursive_refuses_enabling_for_a_strategy_that_does_not_support_it() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox");
        fs::create_dir_all(&inbox).unwrap();
        fs::write(
            inbox.join(".sift.toml"),
            "[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}\"\n",
        )
        .unwrap();
        let inbox = inbox.canonicalize().unwrap();
        add(inbox.clone(), true, false).unwrap();

        let ok = cmd_watch_set_recursive(inbox.to_string_lossy().to_string(), true);
        assert!(!ok);
        // Refused, not silently applied.
        assert!(!find(&inbox).unwrap().unwrap().recursive);
    });
}

#[test]
fn cmd_watch_set_recursive_always_allows_disabling_even_for_an_unsupported_strategy() {
    with_isolated_registry(|root| {
        let inbox = root.join("Inbox");
        fs::create_dir_all(&inbox).unwrap();
        fs::write(
            inbox.join(".sift.toml"),
            "[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}\"\n",
        )
        .unwrap();
        let inbox = inbox.canonicalize().unwrap();
        add(inbox.clone(), true, false).unwrap();
        // Bypasses cmd_watch_add's own guard on purpose, exactly like a
        // stale/hand-edited registry entry could end up recursive=true
        // under a strategy that no longer supports it.
        set_recursive(&inbox, true).unwrap();

        let ok = cmd_watch_set_recursive(inbox.to_string_lossy().to_string(), false);
        assert!(ok);
        assert!(!find(&inbox).unwrap().unwrap().recursive);
    });
}

#[test]
fn cli_no_auto_clean_flag_exists_on_watch_add() {
    use clap::Parser;
    use sift::cli::Cli;
    let result = Cli::try_parse_from([
        "sift",
        "watch",
        "add",
        "/tmp/x",
        "--auto-apply",
        "--auto-clean",
    ]);
    assert!(
        result.is_err(),
        "there must be no --auto-clean flag anywhere in watch"
    );
}

#[test]
fn cli_watch_list_human_output_does_not_panic_when_empty() {
    with_isolated_registry(|_root| {
        assert!(cmd_watch_list(false));
        assert!(cmd_watch_list(true));
    });
}

// ============================================================
// Real notify integration (bounded, real filesystem watcher)
// ============================================================

#[test]
fn real_notify_organizes_a_new_file_end_to_end() {
    let d = tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();

    with_isolated_registry(|_reg_root| {
        add(root.clone(), true, false).unwrap();
        transition(&root, WatchState::Running).unwrap();

        with_isolated_history(&root, || {
            let mut daemon = Daemon::new().unwrap();
            daemon.reconcile();

            // Create the file only *after* the watcher is attached, per the
            // "no backfill" rule this test is also implicitly verifying.
            let file = root.join("photo.jpg");
            fs::write(&file, b"hello").unwrap();

            // Drive real ticks for a bounded time: drain real notify events,
            // then poll stability, until the move happens or we time out.
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut done = false;
            while Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if root.join("Images/photo.jpg").exists() {
                    done = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }

            assert!(
                done,
                "expected Images/photo.jpg to exist within the timeout; the real OS \
                 filesystem watcher may be unavailable in this environment"
            );
            assert!(
                !file.exists(),
                "source must be gone after a successful move"
            );

            let hist_dir = root.join(".sift-history-test");
            let has_history = fs::read_dir(&hist_dir)
                .map(|rd| rd.flatten().count() > 0)
                .unwrap_or(false);
            assert!(
                has_history,
                "expected a history record for the automatic move"
            );
        });
    });
}

#[test]
fn real_notify_preserved_old_mtime_current_generation_event_is_still_organized() {
    // A file copied into a watched folder with `cp -p`, synced with
    // `rsync -a`, extracted from an archive, or restored from a backup
    // can legitimately carry an mtime from long before this watch's
    // current generation even started. That is a completely ordinary,
    // legitimate *current-generation* filesystem event — an old mtime is
    // never, on its own, evidence that the underlying `notify` event is
    // a stale leftover from an earlier, already-torn-down generation.
    let d = tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();

    with_isolated_registry(|_reg_root| {
        add(root.clone(), true, false).unwrap();
        transition(&root, WatchState::Running).unwrap();

        with_isolated_history(&root, || {
            let mut daemon = Daemon::new().unwrap();
            daemon.reconcile();

            let file = root.join("invoice.pdf");
            fs::write(&file, b"x").unwrap();
            // Simulate `cp -p`/`rsync -a`/archive extraction: brand new
            // to this directory, but an old preserved mtime (~1 year).
            let old_mtime = std::time::SystemTime::now() - Duration::from_secs(365 * 24 * 3600);
            fs::OpenOptions::new()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(old_mtime)
                .unwrap();

            let deadline = Instant::now() + Duration::from_secs(10);
            let mut done = false;
            while Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if root.join("Documents/invoice.pdf").exists() {
                    done = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }

            assert!(
                done,
                "a legitimately new file whose mtime happens to predate this watch's \
                 current generation must still be organized — an old mtime alone is not \
                 evidence of a stale/leftover event"
            );
        });
    });
}

#[test]
fn real_notify_recursive_watch_uses_nested_local_sift_toml() {
    // The core scenario this test guards: a recursive watch rooted at
    // `root` must let `root/client/`'s own local `.sift.toml` govern files
    // inside `client/`, instead of always applying `root`'s own resolved
    // policy to everything under it regardless of depth.
    let d = tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("client")).unwrap();
    fs::write(
        root.join("client").join(".sift.toml"),
        "[organize]\nstrategy = \"type\"\nunknown = \"skip\"\n",
    )
    .unwrap();

    with_isolated_registry(|_reg_root| {
        add(root.clone(), true, true).unwrap(); // recursive
        transition(&root, WatchState::Running).unwrap();

        with_isolated_history(&root, || {
            let mut daemon = Daemon::new().unwrap();
            daemon.reconcile();

            // Root's own (default) policy is `unknown = "other"`; an
            // unrecognized extension at the root must still land in
            // `Other/`. `client/`'s own policy is `unknown = "skip"`; the
            // identically-unrecognized file inside `client/` must stay
            // put. Both are created only *after* the watcher attaches, per
            // the "no backfill" rule.
            let root_file = root.join("weird.xyzabc");
            let nested_file = root.join("client").join("weird.xyzabc");
            fs::write(&root_file, b"x").unwrap();
            fs::write(&nested_file, b"x").unwrap();

            let deadline = Instant::now() + Duration::from_secs(10);
            let mut root_moved = false;
            while Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if root.join("Other/weird.xyzabc").exists() {
                    root_moved = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                root_moved,
                "expected root's own unknown=other policy to move the root-level file \
                 within the timeout; the real OS filesystem watcher may be unavailable \
                 in this environment"
            );

            // Give client/'s candidate the same stabilize-and-process
            // treatment a few more ticks over, so a wrongly-applied
            // root policy (which would have moved it to Other/ or
            // client/Other/ by now) has every chance to show up.
            for _ in 0..20 {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                std::thread::sleep(Duration::from_millis(50));
            }

            assert!(
                nested_file.exists(),
                "client/'s own unknown=skip policy must keep this file in place"
            );
            assert!(!root.join("client/Other/weird.xyzabc").exists());
        });
    });
}

#[test]
fn real_notify_pause_then_resume_invisible_to_reconcile_never_organizes_the_pause_window_file() {
    // The scenario: pause immediately followed by resume, both landing
    // strictly between two of the daemon's reconcile ticks, so
    // `reconcile` never once observes the intermediate `Paused` state —
    // `running` looks unbroken across the gap, only `run_generation`
    // moved. A file created during that invisible pause must still never
    // be organized, even though the real OS `notify` watch may never
    // technically have gone down, and even though its create event may
    // already be sitting in the daemon's event channel by the time the
    // resume's `reconcile` runs.
    let d = tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();

    with_isolated_registry(|_reg_root| {
        add(root.clone(), true, false).unwrap();
        transition(&root, WatchState::Running).unwrap();

        with_isolated_history(&root, || {
            let mut daemon = Daemon::new().unwrap();
            daemon.tick(Duration::from_millis(100));
            assert!(daemon.is_monitoring(&root));

            // Pause and resume back-to-back, with no `daemon.tick()`
            // call in between — this *is* "invisible to reconcile".
            transition(&root, WatchState::Paused).unwrap();
            let during_pause = root.join("during-pause.jpg");
            fs::write(&during_pause, b"x").unwrap();
            // Bounded wait purely for the real kernel to deliver the
            // create event into notify's channel before resuming — this
            // is what makes the test actually exercise "an event queued
            // while the OS watcher still exists", not a guess about
            // whether it does. Not a wait for any daemon-side state.
            std::thread::sleep(Duration::from_millis(300));
            transition(&root, WatchState::Running).unwrap();

            // Drive real ticks for a bounded time — long enough that,
            // absent a fix, the default 2.5s stability window would have
            // elapsed several times over and the file would have been
            // organized.
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                daemon.tick(Duration::from_millis(100));
                std::thread::sleep(Duration::from_millis(50));
            }

            assert!(
                during_pause.exists(),
                "a file created during a pause/resume round-trip invisible to the daemon's \
                 poll loop must never be organized"
            );
            assert!(!root.join("Images/during-pause.jpg").exists());

            // The pipeline itself must still be alive after the resume:
            // a genuinely new file must be organized normally.
            let after_resume = root.join("after-resume.jpg");
            fs::write(&after_resume, b"y").unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut organized = false;
            while Instant::now() < deadline {
                daemon.tick(Duration::from_millis(100));
                if root.join("Images/after-resume.jpg").exists() {
                    organized = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                organized,
                "a file created after the resume must still be organized normally"
            );
        });
    });
}

#[test]
fn real_notify_reverted_start_never_organizes_an_event_from_the_failed_generation() {
    // Simulates the readiness-timeout rollback path in
    // `cmd_watch_transition`/`revert_after_unconfirmed_start`: the CLI
    // has already reverted the registry to `Stopped` and returned
    // failure, but the daemon — which never got a chance to reconcile
    // that revert yet — still has a live monitor (and a live OS `notify`
    // watch) from the generation that just failed. A file created in
    // that exact window must never be organized once the daemon finally
    // catches up.
    let d = tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();

    with_isolated_registry(|_reg_root| {
        add(root.clone(), true, false).unwrap();
        transition(&root, WatchState::Running).unwrap();

        with_isolated_history(&root, || {
            let mut daemon = Daemon::new().unwrap();
            // The daemon has genuinely installed a live monitor + real
            // OS watch for the "failed" generation, exactly as it would
            // have by the time a real `watch start` gives up waiting.
            daemon.tick(Duration::from_millis(100));
            assert!(daemon.is_monitoring(&root));

            // The CLI's rollback: reverts the registry directly, with no
            // daemon tick in between — the daemon has no way to know yet.
            transition(&root, WatchState::Stopped).unwrap();

            let after_revert = root.join("should-never-organize.jpg");
            fs::write(&after_revert, b"x").unwrap();
            // Bounded wait purely for the kernel to deliver the create
            // event into notify's channel while the OS watch is still
            // live (the daemon hasn't reconciled the revert yet).
            std::thread::sleep(Duration::from_millis(300));

            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                daemon.tick(Duration::from_millis(100));
                std::thread::sleep(Duration::from_millis(50));
            }

            assert!(
                after_revert.exists(),
                "a file created after a failed start's rollback must never be organized, \
                 even though the daemon's live monitor from that generation was still up \
                 at the moment the file was created"
            );
            assert!(!root.join("Images/should-never-organize.jpg").exists());
            assert!(
                !daemon.is_monitoring(&root),
                "the daemon must have torn the monitor down once it reconciled the revert"
            );
        });
    });
}

/// Small local helper trait so tests can turn "a not-yet-existing directory
/// path" into a real, canonical directory in one line.
trait CanonicalizeDirExt {
    fn canonicalize_dir(&self) -> PathBuf;
}

impl CanonicalizeDirExt for PathBuf {
    fn canonicalize_dir(&self) -> PathBuf {
        fs::create_dir_all(self).unwrap();
        self.canonicalize().unwrap()
    }
}
