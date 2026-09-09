//! Integration tests for Sift's `.sift.toml` Smart Folder policy: strategy
//! dispatch, rule precedence, manual/recursive organize integration,
//! `sift config check`/`sift explain`, and Watch hot-reload/fail-closed
//! behavior. Uses only tempdir/tempfile; every test that touches history,
//! the watch registry, or "global config" isolates it to a tempdir first —
//! never the user's real home or Sift data directory.

use sift::classifier::CategoryDB;
use sift::config::{
    clear_test_global_config_dir, set_test_global_config_dir, EffectivePolicy, OrganizeStrategy,
    PolicySource, UnknownPolicy,
};
use sift::domain::{Category, Op};
use sift::explain::explain_path;
use sift::planner::{plan_with_strategy, plan_with_strategy_recursive};
use std::fs;
use tempfile::tempdir;

fn isolated_global<R>(f: impl FnOnce() -> R) -> R {
    let empty = tempdir().unwrap();
    set_test_global_config_dir(empty.path().to_path_buf());
    let r = f();
    clear_test_global_config_dir();
    r
}

// ------------------------------------------------------------- resolution

#[test]
fn config_source_reported_accurately_for_each_level() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let policy = sift::config::resolve_policy(d.path().to_str().unwrap()).unwrap();
        assert_eq!(policy.source, PolicySource::Default);
        assert_eq!(policy.source.label(), "built-in defaults");

        fs::write(d.path().join(".sift.toml"), "version = 1\n").unwrap();
        let policy = sift::config::resolve_policy(d.path().to_str().unwrap()).unwrap();
        assert_eq!(policy.source.label(), ".sift.toml");
        assert!(policy.source.describe().ends_with(".sift.toml"));
    });
}

// -------------------------------------------------------------- strategy

fn policy_with_unknown(unknown: UnknownPolicy) -> EffectivePolicy {
    EffectivePolicy {
        unknown_policy: unknown,
        ..EffectivePolicy::default()
    }
}

#[test]
fn strategy_type_classifies_every_builtin_category() {
    let d = tempdir().unwrap();
    let t = d.path();
    for (name, cat) in [
        ("a.pdf", Category::Document),
        ("a.svg", Category::Image),
        ("a.json", Category::Data),
        ("a.js", Category::Code),
        ("a.blend", Category::ThreeD),
    ] {
        fs::write(t.join(name), b"x").unwrap();
        let plan = plan_with_strategy(
            t.to_str().unwrap(),
            &EffectivePolicy::default(),
            &CategoryDB::default(),
        );
        let action = plan.actions.iter().find(|a| a.src.ends_with(name)).unwrap();
        assert_eq!(action.op, Op::Move, "{name} should move ({cat:?})");
        fs::remove_file(t.join(name)).unwrap();
    }
}

#[test]
fn unknown_defaults_to_other() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("weird.xyzabc"), b"x").unwrap();
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &EffectivePolicy::default(),
        &CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("weird.xyzabc"))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert!(a.dst.as_ref().unwrap().to_string_lossy().contains("Other"));
}

#[test]
fn unknown_skip_leaves_file_untouched() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("weird.xyzabc"), b"x").unwrap();
    let policy = policy_with_unknown(UnknownPolicy::Skip);
    let plan = plan_with_strategy(t.to_str().unwrap(), &policy, &CategoryDB::default());
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("weird.xyzabc"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(t.join("weird.xyzabc").exists());
}

// ------------------------------------------------------------ precedence

#[test]
fn rule_move_overrides_type_classification() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("video.mp4"), b"x").unwrap();
    let rule = sift::config::Rule {
        name: "movies".into(),
        pattern: "*.mp4".into(),
        action: "Move".into(),
        destination: Some("Movies".into()),
        priority: 1,
        enabled: true,
        description: None,
    };
    let policy = EffectivePolicy::default().with_rules(vec![rule]);
    let plan = plan_with_strategy(t.to_str().unwrap(), &policy, &CategoryDB::default());
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("video.mp4"))
        .unwrap();
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("Movies/video.mp4"));
}

#[test]
fn rule_skip_overrides_type_classification() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("keep.json"), b"x").unwrap();
    let rule = sift::config::Rule {
        name: "no-touch".into(),
        pattern: "*.json".into(),
        action: "Skip".into(),
        destination: None,
        priority: 1,
        enabled: true,
        description: None,
    };
    let policy = EffectivePolicy::default().with_rules(vec![rule]);
    let plan = plan_with_strategy(t.to_str().unwrap(), &policy, &CategoryDB::default());
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("keep.json"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
}

#[test]
fn safety_still_overrides_a_config_move_rule_for_symlinks() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    let target = tempdir().unwrap();
    symlink(target.path(), t.join("link.mp4")).unwrap();
    let rule = sift::config::Rule {
        name: "movies".into(),
        pattern: "*.mp4".into(),
        action: "Move".into(),
        destination: Some("Movies".into()),
        priority: 1,
        enabled: true,
        description: None,
    };
    let policy = EffectivePolicy::default().with_rules(vec![rule]);
    let plan = plan_with_strategy(t.to_str().unwrap(), &policy, &CategoryDB::default());
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("link.mp4"))
        .unwrap();
    assert_eq!(a.op, Op::Skip, "a rule must never make a symlink movable");
    assert_eq!(a.reason.as_deref(), Some("symlink"));
}

#[test]
fn safety_still_overrides_a_config_move_rule_for_project_roots() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir(t.join("app")).unwrap();
    fs::write(t.join("app").join("Cargo.toml"), b"[package]").unwrap();
    let rule = sift::config::Rule {
        name: "sweep-dirs".into(),
        pattern: "app".into(),
        action: "Move".into(),
        destination: Some("Elsewhere".into()),
        priority: 1,
        enabled: true,
        description: None,
    };
    let policy = EffectivePolicy::default().with_rules(vec![rule]);
    let plan = plan_with_strategy(t.to_str().unwrap(), &policy, &CategoryDB::default());
    // `app` is a directory, categorically skipped before rules ever run —
    // a rule can never bypass project/categorical protection.
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("app"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
}

// ------------------------------------------------------- manual organize

#[test]
fn manual_organize_dry_run_with_local_policy_is_non_mutating() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(t.join(".sift.toml"), "[organize]\nunknown = \"skip\"\n").unwrap();
        fs::write(t.join("a.pdf"), b"x").unwrap();
        let ok = sift::planner::cmd_organize(
            t.to_str().unwrap().to_string(),
            false,
            false,
            false,
            false,
        );
        assert!(ok);
        assert!(t.join("a.pdf").exists());
        assert!(!t.join("Documents").exists());
    });
}

#[test]
fn manual_organize_apply_requires_apply_flag() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(t.join("a.pdf"), b"x").unwrap();
        let hist_dir = t.join(".sift-history-test");
        sift::history::set_test_history_dir(hist_dir);
        sift::planner::cmd_organize(t.to_str().unwrap().to_string(), false, false, false, false);
        assert!(t.join("a.pdf").exists());
        assert!(!t.join("Documents").exists());
        sift::history::clear_test_history_dir();
    });
}

#[test]
fn dot_sift_toml_itself_is_never_organized() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(t.join(".sift.toml"), "version = 1\n").unwrap();
        let hist_dir = t.join(".sift-history-test");
        sift::history::set_test_history_dir(hist_dir);
        let ok =
            sift::planner::cmd_organize(t.to_str().unwrap().to_string(), true, false, false, false);
        assert!(ok);
        assert!(
            t.join(".sift.toml").is_file(),
            "the control file must never move"
        );
        sift::history::clear_test_history_dir();
    });
}

// ---------------------------------------------------------------- recursive

#[test]
fn recursive_uses_root_policy_for_every_nested_file() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("client")).unwrap();
    fs::write(t.join("client").join("weird.xyzabc"), b"x").unwrap();
    let policy = policy_with_unknown(UnknownPolicy::Skip);
    let rp = plan_with_strategy_recursive(t.to_str().unwrap(), &policy, &CategoryDB::default());
    let a = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("weird.xyzabc"))
        .unwrap();
    assert_eq!(
        a.op,
        Op::Skip,
        "nested file must follow the ROOT's unknown policy"
    );
}

#[test]
fn recursive_never_discovers_a_nested_local_sift_toml() {
    // A nested directory's own `.sift.toml` (if one existed) must never be
    // discovered or applied — the root's resolved policy governs the whole
    // recursive operation. Simulate this by placing a nested `.sift.toml`
    // and confirming it's simply skipped as an ordinary hidden file, never
    // read as policy.
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("client")).unwrap();
    fs::write(
        t.join("client").join(".sift.toml"),
        "[organize]\nunknown = \"skip\"\n",
    )
    .unwrap();
    fs::write(t.join("client").join("weird.xyzabc"), b"x").unwrap();
    // Root policy says unknown -> other (default); if the nested file were
    // (wrongly) discovered, this candidate would be Skip instead.
    let rp = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &EffectivePolicy::default(),
        &CategoryDB::default(),
    );
    let a = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("weird.xyzabc"))
        .unwrap();
    assert_eq!(
        a.op,
        Op::Move,
        "root policy (unknown=other) must win, not the nested file"
    );
    let hidden = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with(".sift.toml"))
        .unwrap();
    assert_eq!(hidden.op, Op::Skip);
    assert_eq!(hidden.reason.as_deref(), Some("hidden file"));
}

// ------------------------------------------------------------------ explain

#[test]
fn explain_known_type_classification() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("movie.mp4"), b"x").unwrap();
    let exp = explain_path(&t.join("movie.mp4"), t).unwrap();
    assert_eq!(exp.strategy, OrganizeStrategy::Type);
    assert_eq!(exp.classification, Some(Category::Video));
    assert_eq!(exp.op, Op::Move);
    assert_eq!(exp.destination, Some(t.join("Video").join("movie.mp4")));
    assert!(exp.checks.iter().all(|c| c.ok));
}

#[test]
fn explain_explicit_rule_winner() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(
        t.join(".sift.toml"),
        "[[rules]]\npattern = \"*.mp4\"\naction = \"Move\"\ndestination = \"Movies\"\npriority = 1\nenabled = true\n",
    )
    .unwrap();
    fs::write(t.join("movie.mp4"), b"x").unwrap();
    let exp = explain_path(&t.join("movie.mp4"), t).unwrap();
    assert!(exp.matched_rule.is_some());
    assert_eq!(exp.destination, Some(t.join("Movies").join("movie.mp4")));
    assert_eq!(exp.op, Op::Move);
}

#[test]
fn explain_unknown_other_fallback() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("weird.xyzabc"), b"x").unwrap();
    let exp = explain_path(&t.join("weird.xyzabc"), t).unwrap();
    assert_eq!(exp.unknown_fallback, Some(UnknownPolicy::Other));
    assert_eq!(exp.op, Op::Move);
    assert_eq!(exp.destination, Some(t.join("Other").join("weird.xyzabc")));
}

#[test]
fn explain_unknown_skip() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join(".sift.toml"), "[organize]\nunknown = \"skip\"\n").unwrap();
    fs::write(t.join("weird.xyzabc"), b"x").unwrap();
    let exp = explain_path(&t.join("weird.xyzabc"), t).unwrap();
    assert_eq!(exp.unknown_fallback, Some(UnknownPolicy::Skip));
    assert_eq!(exp.op, Op::Skip);
    assert!(exp.destination.is_none());
}

#[test]
fn explain_protected_entry() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join(".hidden.pdf"), b"x").unwrap();
    let exp = explain_path(&t.join(".hidden.pdf"), t).unwrap();
    assert_eq!(exp.op, Op::Skip);
    assert!(exp
        .checks
        .iter()
        .any(|c| c.label == "not protected" && !c.ok));
}

#[test]
fn explain_collision() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(t.join("Documents").join("a.pdf"), b"existing").unwrap();
    fs::write(t.join("a.pdf"), b"new").unwrap();
    let exp = explain_path(&t.join("a.pdf"), t).unwrap();
    assert_eq!(exp.op, Op::Skip);
    assert_eq!(exp.reason, "collision");
    assert!(exp
        .checks
        .iter()
        .any(|c| c.label == "no collision" && !c.ok));
}

#[test]
fn explain_performs_zero_mutation() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("movie.mp4"), b"x").unwrap();
    let _ = explain_path(&t.join("movie.mp4"), t).unwrap();
    assert!(t.join("movie.mp4").exists());
    assert!(!t.join("Video").exists());
}

// --------------------------------------------------------------- config check

#[test]
fn config_check_json_reports_valid_policy() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(
        t.join(".sift.toml"),
        "version = 1\n[organize]\nstrategy = \"type\"\nunknown = \"skip\"\n[watch]\nstability_seconds = 5\n",
    )
    .unwrap();
    let result = sift::config::resolve_policy(t.to_str().unwrap());
    let json = sift::render::config_check_json(&result);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["valid"], true);
    assert_eq!(value["strategy"], "type");
    assert_eq!(value["unknown"], "skip");
    assert_eq!(value["stability_seconds"], 5);
}

#[test]
fn config_check_json_reports_invalid_policy() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join(".sift.toml"), "[organize]\nstrategy = \"banana\"\n").unwrap();
    let result = sift::config::resolve_policy(t.to_str().unwrap());
    let json = sift::render::config_check_json(&result);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["valid"], false);
    assert!(value["error"].as_str().unwrap().contains("banana"));
}

// ------------------------------------------------------------------- init

#[test]
fn init_writes_canonical_v1_config_that_resolves_cleanly() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        sift::config::cmd_init(t.to_str().unwrap().to_string(), false);
        assert!(t.join(".sift.toml").is_file());
        let policy = sift::config::resolve_policy(t.to_str().unwrap()).unwrap();
        assert_eq!(policy.version, 1);
        assert_eq!(policy.strategy, OrganizeStrategy::Type);
        assert_eq!(policy.unknown_policy, UnknownPolicy::Other);
        assert!(!policy.rules.is_empty());
    });
}

/// Release-audit regression: `sift init` must never write through a
/// symlinked `.sift.toml`, even with `--force` — no-follow safety applies
/// to every filesystem write in this crate, not only the organize/clean/
/// folders/watch executor path.
#[test]
fn init_refuses_to_write_through_a_symlinked_config() {
    use std::os::unix::fs::symlink;
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        let outside = tempdir().unwrap();
        let escape_target = outside.path().join("evil.toml");
        symlink(&escape_target, t.join(".sift.toml")).unwrap();

        sift::config::cmd_init(t.to_str().unwrap().to_string(), false);
        assert!(
            !escape_target.exists(),
            "must never write through the symlink, even without --force"
        );

        sift::config::cmd_init(t.to_str().unwrap().to_string(), true);
        assert!(
            !escape_target.exists(),
            "must never write through the symlink, even WITH --force"
        );
        assert!(
            std::fs::symlink_metadata(t.join(".sift.toml"))
                .unwrap()
                .file_type()
                .is_symlink(),
            "the symlink itself must be left exactly as it was"
        );
    });
}

// ----------------------------------------------------------------- watch

mod watch_policy {
    use sift::watch::daemon::Daemon;
    use sift::watch::registry::{self, add, transition, WatchState};
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;

    fn with_isolated_registry(f: impl FnOnce(&std::path::Path)) {
        let d = tempdir().unwrap();
        registry::set_test_watch_dir(d.path().join(".sift-watch-test"));
        let global_dir = d.path().join(".sift-global-config-test");
        fs::create_dir_all(&global_dir).unwrap();
        sift::config::set_test_global_config_dir(global_dir);
        f(d.path());
        sift::config::clear_test_global_config_dir();
        registry::clear_test_watch_dir();
    }

    trait CanonicalizeDirExt {
        fn canonicalize_dir(&self) -> std::path::PathBuf;
    }
    impl CanonicalizeDirExt for std::path::PathBuf {
        fn canonicalize_dir(&self) -> std::path::PathBuf {
            fs::create_dir_all(self).unwrap();
            self.canonicalize().unwrap()
        }
    }

    #[test]
    fn watch_add_refuses_existing_invalid_local_config() {
        with_isolated_registry(|root| {
            let w = root.join("w").canonicalize_dir();
            fs::write(w.join(".sift.toml"), "[organize]\nstrategy = \"banana\"\n").unwrap();
            assert!(!sift::watch::cmd_watch_add(
                w.to_str().unwrap().to_string(),
                true,
                false
            ));
            assert!(registry::find(&w).unwrap().is_none());
        });
    }

    #[test]
    fn watch_start_refuses_invalid_config() {
        with_isolated_registry(|root| {
            let w = root.join("w").canonicalize_dir();
            add(w.clone(), true, false).unwrap();
            // Break the config only after registering.
            fs::write(w.join(".sift.toml"), "[organize]\nstrategy = \"banana\"\n").unwrap();
            assert!(!sift::watch::cmd_watch_start(
                w.to_str().unwrap().to_string()
            ));
            assert_eq!(
                registry::find(&w).unwrap().unwrap().state,
                WatchState::Stopped
            );
        });
    }

    #[test]
    fn reconcile_detects_and_clears_config_error() {
        with_isolated_registry(|root| {
            let w = root.join("w").canonicalize_dir();
            add(w.clone(), true, false).unwrap();
            transition(&w, WatchState::Running).unwrap();

            let mut d = Daemon::new().unwrap();
            d.reconcile();
            assert!(registry::find(&w).unwrap().unwrap().config_error.is_none());

            fs::write(w.join(".sift.toml"), "[organize]\nstrategy = \"banana\"\n").unwrap();
            d.reconcile();
            let entry = registry::find(&w).unwrap().unwrap();
            assert!(entry.config_error.is_some());
            assert!(entry.config_error.unwrap().contains("banana"));

            fs::write(w.join(".sift.toml"), "version = 1\n").unwrap();
            d.reconcile();
            assert!(registry::find(&w).unwrap().unwrap().config_error.is_none());
        });
    }

    #[test]
    fn removing_local_config_reloads_default_policy() {
        with_isolated_registry(|root| {
            let w = root.join("w").canonicalize_dir();
            fs::write(w.join(".sift.toml"), "[organize]\nunknown = \"skip\"\n").unwrap();
            add(w.clone(), true, false).unwrap();
            transition(&w, WatchState::Running).unwrap();

            let mut d = Daemon::new().unwrap();
            d.reconcile();
            assert!(registry::find(&w).unwrap().unwrap().config_error.is_none());

            fs::remove_file(w.join(".sift.toml")).unwrap();
            d.reconcile();
            // Still healthy: falling back to defaults is a valid policy,
            // not an error.
            assert!(registry::find(&w).unwrap().unwrap().config_error.is_none());
        });
    }

    #[test]
    fn stopping_a_watch_clears_stale_health() {
        with_isolated_registry(|root| {
            let w = root.join("w").canonicalize_dir();
            add(w.clone(), true, false).unwrap();
            transition(&w, WatchState::Running).unwrap();
            fs::write(w.join(".sift.toml"), "[organize]\nstrategy = \"banana\"\n").unwrap();

            let mut d = Daemon::new().unwrap();
            d.reconcile();
            assert!(registry::find(&w).unwrap().unwrap().config_error.is_some());

            transition(&w, WatchState::Stopped).unwrap();
            assert!(registry::find(&w).unwrap().unwrap().config_error.is_none());
        });
    }

    /// End-to-end, real-`notify`-driven fail-closed + hot-reload + no-backfill
    /// flow: mirrors `real_notify_organizes_a_new_file_end_to_end` in
    /// `watch_integration.rs`, extended to also break and then fix the
    /// policy mid-run.
    #[test]
    fn real_daemon_suspends_and_resumes_on_policy_change() {
        let d = tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        with_isolated_registry(|_reg_root| {
            fs::write(root.join(".sift.toml"), "version = 1\n").unwrap();
            add(root.clone(), true, false).unwrap();
            transition(&root, WatchState::Running).unwrap();

            sift::history::set_test_history_dir(root.join(".sift-history-test"));
            let mut daemon = Daemon::new().unwrap();
            daemon.reconcile();

            // Break the policy, then create a file: it must never be moved.
            fs::write(
                root.join(".sift.toml"),
                "[organize]\nstrategy = \"banana\"\n",
            )
            .unwrap();
            let blocked = root.join("blocked.pdf");
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if !blocked.exists() {
                    fs::write(&blocked, b"x").unwrap();
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(root.join("blocked.pdf").exists(), "still at the root");
            assert!(
                !root.join("Documents").exists(),
                "never organized while suspended"
            );
            assert!(registry::find(&root)
                .unwrap()
                .unwrap()
                .config_error
                .is_some());

            // Fix the policy: a NEW file must be organized, the blocked one
            // must never be swept up retroactively (no backfill).
            fs::write(root.join(".sift.toml"), "version = 1\n").unwrap();
            let after_fix = root.join("after_fix.pdf");
            fs::write(&after_fix, b"x").unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut done = false;
            while std::time::Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if root.join("Documents/after_fix.pdf").exists() {
                    done = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                done,
                "expected after_fix.pdf to be organized once policy recovered"
            );
            assert!(
                root.join("blocked.pdf").exists(),
                "blocked.pdf must never be backfilled"
            );
            sift::history::clear_test_history_dir();
        });
    }
}
