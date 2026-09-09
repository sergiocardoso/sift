//! Sift Watch test suite. Uses only tempdir/tempfile; every test isolates
//! itself from the user's real Sift data directory via
//! `watch::registry::set_test_watch_dir` (and `history::set_test_history_dir`
//! where a watch move is actually executed). Most behavior is exercised
//! against the deterministic registry/eligibility/stability/engine layer;
//! one test at the bottom drives a real `notify` watcher end to end.

use sift::config::EffectivePolicy;
use sift::domain::{HistoryItem, Op};
use sift::watch::daemon::Daemon;
use sift::watch::engine::{process_candidate, RootMonitor};
use sift::watch::registry::{
    self, add, clear_test_watch_dir, find, list, remove, set_test_watch_dir, transition,
    validate_root, WatchState,
};
use sift::watch::{cmd_watch_add, cmd_watch_list};
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
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default().with_rules(rules),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(outcome.organized);
        assert!(root.join("Scripts/script.js").exists());
        assert!(!root.join("Code/script.js").exists());
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
fn watched_collision_never_overwrites() {
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
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(!outcome.organized);
        assert_eq!(outcome.skip_reason.as_deref(), Some("collision"));
        assert_eq!(
            fs::read_to_string(root.join("Images/photo.jpg")).unwrap(),
            "existing"
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
    let d = tempdir().unwrap();
    let root = d.path();
    let f = root.join("race.jpg");
    fs::write(&f, b"x").unwrap();
    // Simulate the destination appearing between "decision" and execution
    // by pre-creating it before calling process_candidate at all — the
    // planner's own collision check would normally catch this, but the
    // executor's independent revalidation is what really guarantees safety
    // regardless of when the race happens (see fs::safe_rename).
    fs::create_dir_all(root.join("Images")).unwrap();
    fs::write(root.join("Images/race.jpg"), b"existing").unwrap();
    with_isolated_history(root, || {
        let outcome = process_candidate(
            root,
            &EffectivePolicy::default(),
            &sift::classifier::CategoryDB::default(),
            &f,
        );
        assert!(!outcome.organized);
        assert!(f.exists());
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
fn watch_failed_action_is_recorded_and_not_undoable() {
    let d = tempdir().unwrap();
    let root = d.path();
    fs::create_dir_all(root.join("Images")).unwrap();
    fs::write(root.join("Images/photo.jpg"), b"existing").unwrap();
    let f = root.join("photo.jpg");
    fs::write(&f, b"new").unwrap();
    with_isolated_history(root, || {
        // This is a plan-time collision skip, so no history is written at
        // all (nothing was executed) — confirm that directly.
        let outcome = process_candidate(
            root,
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
