use sift::classifier::CategoryDB;
use sift::domain::{Category, Op};
use sift::history::{clear_test_history_dir, set_test_history_dir};
use sift::planner::{plan_clean, plan_organize};
use sift::scanner::scan_entries;
use std::fs::File;
use tempfile::tempdir;

#[test]
fn test_extension_classification() {
    let db = CategoryDB::default();
    let mut entry = sift::domain::Entry {
        path: std::path::PathBuf::from("foo.mp3"),
        is_dir: false,
        is_symlink: false,
        hidden: false,
        size: Some(1),
        mtime: None,
        project_root: false,
        protected: false,
        classified_as: None,
    };
    db.classify(&mut entry);
    assert_eq!(entry.classified_as, Some(Category::Audio));
}

#[test]
fn test_collision_detection_broken_symlink() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("foo.pdf")).unwrap();
    std::fs::create_dir_all(t.join("Documents")).unwrap();
    symlink("/nonexistent", t.join("Documents/foo.pdf")).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let act = plan
        .actions
        .iter()
        .find(|a| {
            a.dst
                .as_ref()
                .map(|p| p.ends_with("Documents/foo.pdf"))
                .unwrap_or(false)
        })
        .unwrap();
    assert_eq!(act.op, Op::Skip);
}

#[test]
fn test_symlink_scanner_safety() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    // Ensure test isolation from user history
    assert!(hist_dir.starts_with(t)); // Safety: tempdir path only in test
    symlink("/dev/null", t.join("sym")).unwrap();
    let entries = scan_entries(t);
    let sym = entries.iter().find(|e| e.path.ends_with("sym")).unwrap();
    assert!(sym.is_symlink);
    assert!(!sym.project_root);
    clear_test_history_dir(); // Remove test history + clear override, restore normal state
}

#[test]
fn test_protected_project_root() {
    use sift::scanner::is_project_root;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::File::create(t.join("Cargo.toml")).unwrap();
    assert!(is_project_root(t));

    // A project root's *children* are still reported as scannable entries...
    File::create(t.join("photo.jpg")).unwrap();
    let entries = scan_entries(t);
    assert!(entries.iter().any(|e| e.path.ends_with("photo.jpg")));

    // ...but organize/clean must refuse to plan any mutation on the root itself.
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(!plan.actions.is_empty());
    assert!(plan.actions.iter().all(|a| a.op == Op::Skip));
    let clean_plan = plan_clean(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(clean_plan.actions.iter().all(|a| a.op == Op::Skip));
}

#[test]
fn test_organize_dry_run_no_mutation() {
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    // Ensure test isolation from user history
    assert!(hist_dir.starts_with(t)); // Safety: tempdir path only in test
    File::create(t.join("photo.jpg")).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(plan.actions.iter().any(|a| a.dst.is_some()));
    assert!(plan
        .actions
        .iter()
        .all(|a| matches!(a.op, Op::Move | Op::Skip | Op::CreateDir)));
    // Dry-run: the plan only describes actions, nothing on disk has moved.
    assert!(std::fs::metadata(t.join("photo.jpg")).is_ok());
    assert!(std::fs::metadata(t.join("Images")).is_err());
    clear_test_history_dir(); // Remove test history + clear override, restore normal state
}

#[test]
fn test_expected_organize_destinations() {
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    // Ensure test isolation from user history
    assert!(hist_dir.starts_with(t)); // Safety: tempdir path only in test
    File::create(t.join("pic.png")).unwrap();
    File::create(t.join("song.mp3")).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(plan.actions.iter().any(|a| a
        .dst
        .as_ref()
        .map(|p| p.ends_with("Images/pic.png"))
        .unwrap_or(false)));
    assert!(plan.actions.iter().any(|a| a
        .dst
        .as_ref()
        .map(|p| p.ends_with("Audio/song.mp3"))
        .unwrap_or(false)));
    clear_test_history_dir(); // Remove test history + clear override, restore normal state
}

#[test]
fn test_apply_move_and_undo() {
    // use sift::executor::execute_plan;
    use sift::history::cmd_undo;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    // Ensure test isolation from user history
    assert!(hist_dir.starts_with(t)); // Safety: tempdir path only in test
    let src = t.join("z.txt");
    let dst = t.join("Documents/z.txt");
    File::create(&src).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(plan
        .actions
        .iter()
        .any(|a| a.dst.as_ref() == Some(&dst) && matches!(a.op, Op::Move)));
    // Execute the full plan (including the CreateDir for Documents/) rather
    // than a hand-picked single action, since the Move depends on it.
    sift::executor::execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert!(dst.exists());
    assert!(!src.exists());
    for h in std::fs::read_dir(hist_dir.clone()).unwrap().flatten() {
        let s = std::fs::read_to_string(h.path()).unwrap();
        if s.contains("z.txt") {
            if let Some(id) = h.path().file_stem().and_then(|s| s.to_str()) {
                cmd_undo(id.to_string());
                break;
            }
        }
    }
    assert!(src.exists());
}

#[test]
fn test_collision_no_overwrite() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("foo.pdf")).unwrap();
    std::fs::create_dir_all(t.join("Documents")).unwrap();
    File::create(t.join("Documents/foo.pdf")).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let act = plan
        .actions
        .iter()
        .find(|a| {
            a.dst
                .as_ref()
                .map(|p| p.ends_with("Documents/foo.pdf"))
                .unwrap_or(false)
        })
        .unwrap();
    assert_eq!(act.op, Op::Skip);
}

#[test]
fn test_clean_dry_run_only_junk() {
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    // Ensure test isolation from user history
    assert!(hist_dir.starts_with(t)); // Safety: tempdir path only in test
    File::create(t.join("foo.tmp")).unwrap();
    File::create(t.join("foo.log")).unwrap();
    let plan = plan_clean(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(plan
        .actions
        .iter()
        .any(|a| a.src.ends_with("foo.tmp") && matches!(a.op, Op::Trash)));
    assert!(plan
        .actions
        .iter()
        .all(|a| !a.src.ends_with("foo.log") || matches!(a.op, Op::Skip)));
    // Dry-run: planning must never touch the filesystem.
    assert!(t.join("foo.tmp").exists());
    assert!(t.join("foo.log").exists());
}

#[test]
fn test_failed_move_recorded_accurately_in_history() {
    use sift::domain::HistoryItem;
    use sift::executor::execute_plan;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    let src = t.join("race.txt");
    File::create(&src).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    // Simulate a TOCTOU race: the destination appears after planning but
    // before execution. The executor must revalidate immediately before
    // mutating, refuse to overwrite, and record the failure accurately
    // rather than silently succeeding or crashing.
    std::fs::create_dir_all(t.join("Documents")).unwrap();
    File::create(t.join("Documents/race.txt")).unwrap();
    execute_plan(plan, t.to_str().unwrap(), "organize", None);

    assert!(
        src.exists(),
        "a failed move must leave the source untouched"
    );
    let mut found_failure = false;
    for h in std::fs::read_dir(&hist_dir).unwrap().flatten() {
        let s = std::fs::read_to_string(h.path()).unwrap();
        if !s.contains("race.txt") {
            continue;
        }
        let item: HistoryItem = serde_json::from_str(&s).unwrap();
        for outcome in &item.outcomes {
            if outcome.src.ends_with("race.txt") && outcome.op == Op::Move {
                assert!(
                    outcome.result.is_err(),
                    "failed move must be an error outcome"
                );
                assert!(
                    !outcome.undoable,
                    "a failed move must not be marked undoable"
                );
                found_failure = true;
            }
        }
    }
    assert!(
        found_failure,
        "expected a recorded failed Move outcome for race.txt"
    );
    clear_test_history_dir();
}

#[test]
fn test_history_written() {
    use sift::executor::execute_plan;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    // Ensure test isolation from user history
    assert!(hist_dir.starts_with(t)); // Safety: tempdir path only in test
    let src = t.join("hist.txt");
    File::create(&src).unwrap();
    let dst = t.join("Documents/hist.txt");
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(plan
        .actions
        .iter()
        .any(|a| a.dst.as_ref() == Some(&dst) && matches!(a.op, Op::Move)));
    execute_plan(plan, t.to_str().unwrap(), "organize", None);
    let found = std::fs::read_dir(hist_dir.clone())
        .unwrap()
        .flatten()
        .any(|entry| {
            std::fs::read_to_string(entry.path())
                .unwrap()
                .contains("hist.txt")
        });
    assert!(found);
}

#[test]
fn test_undo_refuses_occupied_original() {
    use sift::executor::execute_plan;
    use sift::history::cmd_undo;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    // Ensure test isolation from user history
    assert!(hist_dir.starts_with(t)); // Safety: tempdir path only in test
    let src = t.join("x.txt");
    let dst = t.join("Documents/x.txt");
    File::create(&src).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(plan
        .actions
        .iter()
        .any(|a| a.dst.as_ref() == Some(&dst) && matches!(a.op, Op::Move)));
    execute_plan(plan, t.to_str().unwrap(), "organize", None);
    File::create(&src).unwrap();
    for h in std::fs::read_dir(hist_dir.clone()).unwrap().flatten() {
        let s = std::fs::read_to_string(h.path()).unwrap();
        if s.contains("x.txt") {
            if let Some(id) = h.path().file_stem().and_then(|s| s.to_str()) {
                cmd_undo(id.to_string());
                break;
            }
        }
    }
    assert!(src.exists() && dst.exists());
}

#[test]
fn test_doctor_report_only_sensitive_filename() {
    // This test must not read file content, only filename/stat.
    // If you add more checks, do not open or read file contents.
    use sift::scanner::{cmd_doctor, doctor_findings};
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("mysecret.env")).unwrap();
    // Innocuous filename, but content that would look sensitive if read.
    // Detection must be filename-only, so this must NOT be flagged.
    std::fs::write(t.join("notes.txt"), b"password: hunter2\nsecret token here").unwrap();
    let findings = doctor_findings(t);
    assert!(findings
        .iter()
        .any(|f| f.path.ends_with("mysecret.env") && f.reason.contains("Sensitive-looking")));
    assert!(!findings.iter().any(|f| f.path.ends_with("notes.txt")));
    // cmd_doctor must run without reading contents or mutating anything.
    cmd_doctor(t.display().to_string(), true, false);
    assert!(std::fs::metadata(t.join("mysecret.env")).is_ok());
    assert!(std::fs::metadata(t.join("notes.txt")).is_ok());
}

fn make_rule(
    name: &str,
    pattern: &str,
    action: &str,
    destination: Option<&str>,
) -> sift::config::Rule {
    sift::config::Rule {
        name: name.to_string(),
        pattern: pattern.to_string(),
        action: action.to_string(),
        destination: destination.map(|d| d.to_string()),
        priority: 1,
        enabled: true,
        description: None,
    }
}

#[test]
fn test_config_skip_rule_honored_without_destination() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("abc.txt")).unwrap();
    let rules = vec![make_rule("skip txt", "*.txt", "Skip", None)];
    let plan = plan_organize(t.to_str().unwrap(), &rules, &CategoryDB::default());
    let skip = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("abc.txt"))
        .unwrap();
    assert_eq!(skip.op, Op::Skip);
    assert!(skip.dst.is_none());
}

#[test]
fn test_config_trash_rule_honored_without_destination() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("abc.txt")).unwrap();
    let rules = vec![make_rule("trash txt", "*.txt", "Trash", None)];
    let plan = plan_organize(t.to_str().unwrap(), &rules, &CategoryDB::default());
    let trash = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("abc.txt"))
        .unwrap();
    assert_eq!(trash.op, Op::Trash);
    assert!(trash.dst.is_none());
}

#[test]
fn test_config_safe_move_rule_honored() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("abc.txt")).unwrap();
    let rules = vec![make_rule("move txt", "*.txt", "Move", Some("Stuff"))];
    let plan = plan_organize(t.to_str().unwrap(), &rules, &CategoryDB::default());
    let mv = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("abc.txt"))
        .unwrap();
    assert_eq!(mv.op, Op::Move);
    assert!(mv.dst.as_ref().unwrap().ends_with("Stuff/abc.txt"));
    assert!(plan
        .actions
        .iter()
        .any(|a| a.op == Op::CreateDir && a.src.ends_with("Stuff")));
}

#[test]
fn test_config_unsafe_destination_rejected() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("abc.txt")).unwrap();
    for bad_dest in ["/etc", "../escape", "..", "a/../../escape", ""] {
        let rules = vec![make_rule("bad move", "*.txt", "Move", Some(bad_dest))];
        let plan = plan_organize(t.to_str().unwrap(), &rules, &CategoryDB::default());
        let act = plan
            .actions
            .iter()
            .find(|a| a.src.ends_with("abc.txt"))
            .unwrap();
        assert_eq!(
            act.op,
            Op::Skip,
            "destination {bad_dest:?} should be rejected"
        );
        assert!(act.dst.is_none());
    }
    // The file must never have moved anywhere.
    assert!(t.join("abc.txt").exists());
}

#[test]
fn test_config_rule_priority_highest_wins() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("abc.txt")).unwrap();
    let mut low = make_rule("low priority skip", "*.txt", "Skip", None);
    low.priority = 1;
    let mut high = make_rule("high priority trash", "*.txt", "Trash", None);
    high.priority = 10;
    let plan = plan_organize(t.to_str().unwrap(), &[low, high], &CategoryDB::default());
    let act = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("abc.txt"))
        .unwrap();
    assert_eq!(
        act.op,
        Op::Trash,
        "higher priority rule must win regardless of file order"
    );
}

#[test]
fn test_hidden_files_protected() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join(".hidden.txt")).unwrap();
    let entries = scan_entries(t);
    let hidden = entries
        .iter()
        .find(|e| e.path.ends_with(".hidden.txt"))
        .unwrap();
    assert!(hidden.hidden);
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let act = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with(".hidden.txt"))
        .unwrap();
    assert_eq!(act.op, Op::Skip);
    assert!(t.join(".hidden.txt").exists());
}

#[test]
fn test_undo_ignores_unsuccessful_actions() {
    use sift::domain::{Action, HistoryItem};
    use sift::history::{cmd_undo, record_history};
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    let src = t.join("never-moved.txt");
    let would_be_dst = t.join("Documents/never-moved.txt");
    // Fabricate a history record describing a *failed* move: dst never
    // created, src never removed. Undo must not touch either path.
    let failed_move = Action {
        src: src.clone(),
        dst: Some(would_be_dst.clone()),
        op: Op::Move,
        reason: Some("simulated failure".into()),
        undoable: true,
    };
    let outcome = sift::domain::ActionResult {
        src: src.clone(),
        dst: Some(would_be_dst.clone()),
        op: Op::Move,
        result: Err("simulated failure".into()),
        undoable: false,
    };
    let item = HistoryItem {
        id: "hist-fake-failed".into(),
        actions: vec![failed_move],
        timestamp: 0,
        outcomes: vec![outcome],
        kind: "organize".into(),
        origin: "manual".into(),
        watch_root: None,
    };
    record_history(&item).unwrap();

    cmd_undo("hist-fake-failed".to_string());

    // Nothing should have moved: src was never actually relocated, so it
    // must not exist, and the never-created destination must still be absent.
    assert!(!src.exists());
    assert!(!would_be_dst.exists());
    clear_test_history_dir();
}

#[test]
fn test_undo_does_not_rewrite_original_history_record() {
    use sift::domain::HistoryItem;
    use sift::history::cmd_undo;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    let src = t.join("orig.txt");
    let dst = t.join("Documents/orig.txt");
    File::create(&src).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    sift::executor::execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert!(dst.exists());

    let mut hist_file = None;
    for h in std::fs::read_dir(&hist_dir).unwrap().flatten() {
        let s = std::fs::read_to_string(h.path()).unwrap();
        if s.contains("orig.txt") {
            hist_file = Some(h.path());
            break;
        }
    }
    let hist_file = hist_file.unwrap();
    let before = std::fs::read_to_string(&hist_file).unwrap();
    let id = hist_file.file_stem().and_then(|s| s.to_str()).unwrap();
    cmd_undo(id.to_string());
    let after = std::fs::read_to_string(&hist_file).unwrap();
    assert_eq!(
        before, after,
        "the original history record must be immutable"
    );

    // A separate history record documenting the undo must now exist.
    let undo_item_count = std::fs::read_dir(&hist_dir)
        .unwrap()
        .flatten()
        .filter(|e| {
            std::fs::read_to_string(e.path())
                .map(|s| serde_json::from_str::<HistoryItem>(&s).is_ok())
                .unwrap_or(false)
        })
        .count();
    assert_eq!(
        undo_item_count, 2,
        "expected the original record plus one undo record"
    );
    clear_test_history_dir();
}

#[test]
fn test_cli_defaults_path_to_dot() {
    use clap::Parser;
    use sift::cli::{Cli, Commands};
    let cli = Cli::try_parse_from(["sift", "scan"]).unwrap();
    match cli.command {
        Some(Commands::Scan { path, .. }) => assert_eq!(path, "."),
        _ => panic!("expected Scan command"),
    }
    let cli = Cli::try_parse_from(["sift"]).unwrap();
    assert!(cli.command.is_none());
    assert_eq!(cli.path, ".");
}

#[test]
fn test_no_implicit_apply_behavior() {
    use clap::Parser;
    use sift::cli::{Cli, Commands};
    // Omitting --apply must never set it implicitly true.
    let cli = Cli::try_parse_from(["sift", "organize", "."]).unwrap();
    match cli.command {
        Some(Commands::Organize { apply, .. }) => assert!(!apply),
        _ => panic!("expected Organize command"),
    }
    // There is no config mechanism to enable mutation implicitly in v0.1:
    // plan_organize never touches the filesystem regardless of config, and
    // even a stray/foreign `[general] apply_by_default = true` table in a
    // config file (e.g. left over from an old version) is inert and safely
    // ignored rather than parsed into anything that could trigger mutation.
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("photo.jpg")).unwrap();
    std::fs::write(t.join(".sift.toml"), "[general]\napply_by_default = true\n").unwrap();
    let cfg_path = sift::config::find_config(t.to_str().unwrap()).unwrap();
    let config = sift::config::load_config(&cfg_path).unwrap();
    let _ = plan_organize(t.to_str().unwrap(), &config.rules, &CategoryDB::default());
    assert!(std::fs::metadata(t.join("photo.jpg")).is_ok());
    assert!(std::fs::metadata(t.join("Images")).is_err());
}

#[test]
fn test_undo_refuses_when_destination_replaced_by_directory() {
    use sift::domain::HistoryItem;
    use sift::executor::execute_plan;
    use sift::history::cmd_undo;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    let src = t.join("doc.txt");
    let dst = t.join("Documents/doc.txt");
    File::create(&src).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(plan
        .actions
        .iter()
        .any(|a| a.dst.as_ref() == Some(&dst) && matches!(a.op, Op::Move)));
    execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert!(dst.exists());
    assert!(!src.exists());

    // Something else replaces the moved file with a directory before undo runs.
    std::fs::remove_file(&dst).unwrap();
    std::fs::create_dir(&dst).unwrap();

    let mut hist_file = None;
    for h in std::fs::read_dir(&hist_dir).unwrap().flatten() {
        let s = std::fs::read_to_string(h.path()).unwrap();
        if s.contains("doc.txt") {
            hist_file = Some(h.path());
            break;
        }
    }
    let hist_file = hist_file.unwrap();
    let id = hist_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap()
        .to_string();
    cmd_undo(id);

    // The directory must never have been moved, and the original source
    // location must remain unoccupied (not silently reused or overwritten).
    assert!(
        dst.is_dir(),
        "the directory at the old destination must be untouched"
    );
    assert!(
        !src.exists(),
        "undo must not fabricate a file at the original source"
    );

    // The refusal must be recorded accurately in a separate undo record,
    // not by rewriting the original history item.
    let mut found_refusal = false;
    for h in std::fs::read_dir(&hist_dir).unwrap().flatten() {
        if h.path() == hist_file {
            continue;
        }
        let s = std::fs::read_to_string(h.path()).unwrap();
        if let Ok(item) = serde_json::from_str::<HistoryItem>(&s) {
            for outcome in &item.outcomes {
                let touches_doc = outcome.src.ends_with("doc.txt")
                    || outcome
                        .dst
                        .as_ref()
                        .map(|d| d.ends_with("doc.txt"))
                        .unwrap_or(false);
                if touches_doc {
                    assert!(
                        outcome.result.is_err(),
                        "the refusal must be recorded as an error"
                    );
                    found_refusal = true;
                }
            }
        }
    }
    assert!(
        found_refusal,
        "expected the undo refusal to be recorded in a new history item"
    );
    clear_test_history_dir();
}

#[test]
fn test_config_move_destination_symlink_escape_rejected() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    let outside = tempdir().unwrap();
    File::create(t.join("file.txt")).unwrap();
    symlink(outside.path(), t.join("Stuff")).unwrap();

    let rules = vec![make_rule("escape", "*.txt", "Move", Some("Stuff"))];
    let plan = plan_organize(t.to_str().unwrap(), &rules, &CategoryDB::default());
    // Planning must never emit a CreateDir/Move that would create or write
    // through the symlink.
    assert!(!plan.actions.iter().any(|a| a.op == Op::CreateDir));
    let act = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("file.txt"))
        .unwrap();
    assert_eq!(act.op, Op::Skip);
    assert!(act.dst.is_none());
    assert_eq!(
        std::fs::read_dir(outside.path()).unwrap().count(),
        0,
        "nothing must be created outside the target through the symlink"
    );
    assert!(t.join("file.txt").exists());

    // Executor-side TOCTOU check: even given a hand-built plan pointing
    // straight through the symlink (as if it appeared after planning), the
    // executor must independently refuse rather than trust the plan.
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    let escape_move = sift::domain::Action {
        src: t.join("file.txt"),
        dst: Some(t.join("Stuff").join("file.txt")),
        op: Op::Move,
        reason: Some("hand-crafted TOCTOU attempt".into()),
        undoable: true,
    };
    let escape_mkdir = sift::domain::Action {
        src: t.join("Stuff").join("Nested"),
        dst: None,
        op: Op::CreateDir,
        reason: Some("hand-crafted TOCTOU attempt".into()),
        undoable: false,
    };
    sift::executor::execute_plan(
        sift::domain::Plan {
            actions: vec![escape_move, escape_mkdir],
        },
        t.to_str().unwrap(),
        "organize",
        None,
    );
    assert_eq!(
        std::fs::read_dir(outside.path()).unwrap().count(),
        0,
        "the executor must not escape through the symlink either"
    );
    assert!(
        t.join("file.txt").exists(),
        "source must remain since the move must fail"
    );
    clear_test_history_dir();
}

// ============================================================
// Recursive traversal
// ============================================================

#[test]
fn test_non_recursive_organize_ignores_nested_files() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("root.pdf")).unwrap();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/nested.jpg")).unwrap();

    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(
        !plan
            .actions
            .iter()
            .any(|a| a.src.ends_with("nested/nested.jpg")
                || a.dst
                    .as_ref()
                    .map(|d| d.ends_with("nested.jpg"))
                    .unwrap_or(false)),
        "non-recursive organize must never look inside a subdirectory"
    );
    // The nested directory itself is just skipped, like any other directory.
    let nested_action = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("nested"))
        .unwrap();
    assert_eq!(nested_action.op, Op::Skip);
}

#[test]
fn test_recursive_organize_treats_each_directory_locally() {
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("root.pdf")).unwrap();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/nested.jpg")).unwrap();

    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    let root_move = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("root.pdf"))
        .unwrap();
    assert_eq!(root_move.op, Op::Move);
    assert_eq!(
        root_move.dst.as_ref().unwrap(),
        &t.join("Documents/root.pdf")
    );

    let nested_move = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("nested.jpg"))
        .unwrap();
    assert_eq!(nested_move.op, Op::Move);
    assert_eq!(
        nested_move.dst.as_ref().unwrap(),
        &t.join("nested/Images/nested.jpg"),
        "nested.jpg must land in nested/Images/, not be flattened into the root"
    );
    assert!(
        !rp.plan
            .actions
            .iter()
            .any(|a| a.dst.as_ref() == Some(&t.join("Images/nested.jpg"))),
        "must never flatten a nested file into the root's own category folder"
    );
}

#[test]
fn test_recursive_organize_dry_run_no_mutation() {
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("root.pdf")).unwrap();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/nested.jpg")).unwrap();

    let _ = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(t.join("root.pdf").exists());
    assert!(t.join("nested/nested.jpg").exists());
    assert!(!t.join("Documents").exists());
    assert!(!t.join("nested/Images").exists());
}

#[test]
fn test_recursive_organize_apply_moves_nested_files() {
    use sift::executor::execute_plan;
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    File::create(t.join("root.pdf")).unwrap();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/nested.jpg")).unwrap();

    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    let (_id, outcomes) = execute_plan(rp.plan, t.to_str().unwrap(), "organize-recursive", None);
    assert!(outcomes.iter().all(|o| o.result.is_ok()));

    assert!(t.join("Documents/root.pdf").exists());
    assert!(t.join("nested/Images/nested.jpg").exists());
    assert!(!t.join("root.pdf").exists());
    assert!(!t.join("nested/nested.jpg").exists());
    clear_test_history_dir();
}

#[test]
fn test_recursive_organize_explicit_createdir_per_directory() {
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("root.pdf")).unwrap();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/nested.jpg")).unwrap();

    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(rp
        .plan
        .actions
        .iter()
        .any(|a| a.op == Op::CreateDir && a.src == t.join("Documents")));
    assert!(rp
        .plan
        .actions
        .iter()
        .any(|a| a.op == Op::CreateDir && a.src == t.join("nested/Images")));
}

#[test]
fn test_recursive_organize_does_not_reprocess_category_dirs() {
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("Documents")).unwrap();
    File::create(t.join("Documents/existing.pdf")).unwrap();

    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(
        !rp.plan
            .actions
            .iter()
            .any(|a| a.src.ends_with("existing.pdf")),
        "a file already inside a category directory must never be reprocessed"
    );
    assert!(
        !rp.plan.actions.iter().any(|a| a
            .dst
            .as_ref()
            .map(|d| d.ends_with("Documents/Documents"))
            .unwrap_or(false)
            || a.src.ends_with("Documents/Documents")),
        "must never produce Documents/Documents-style repeated nesting"
    );
    // "Documents" itself is reported once, as a protected/off-limits entry.
    let doc_action = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src == t.join("Documents"))
        .unwrap();
    assert_eq!(doc_action.op, Op::Skip);
    assert_eq!(doc_action.reason.as_deref(), Some("category directory"));
}

#[test]
fn test_recursive_discovery_excludes_category_dirs() {
    use sift::scanner::discover_recursive_dirs;
    let d = tempdir().unwrap();
    let t = d.path();
    for name in sift::scanner::CATEGORY_DIR_NAMES {
        std::fs::create_dir_all(t.join(name)).unwrap();
    }
    let dirs = discover_recursive_dirs(t);
    assert_eq!(
        dirs,
        vec![t.to_path_buf()],
        "no category directory may ever be a traversal target"
    );
}

#[test]
fn test_recursive_discovery_skips_hidden_dirs() {
    use sift::scanner::discover_recursive_dirs;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join(".cache/inner")).unwrap();
    let dirs = discover_recursive_dirs(t);
    assert_eq!(dirs, vec![t.to_path_buf()]);
}

#[test]
fn test_recursive_discovery_skips_symlink_dirs() {
    use sift::scanner::discover_recursive_dirs;
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    let outside = tempdir().unwrap();
    File::create(outside.path().join("secret.txt")).unwrap();
    symlink(outside.path(), t.join("photos")).unwrap();

    let dirs = discover_recursive_dirs(t);
    assert_eq!(
        dirs,
        vec![t.to_path_buf()],
        "a directory symlink must never be traversed, even to a real directory"
    );
}

#[test]
fn test_recursive_discovery_skips_broken_symlinks_safely() {
    use sift::scanner::discover_recursive_dirs;
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    symlink("/nonexistent-target-xyz", t.join("broken")).unwrap();

    let dirs = discover_recursive_dirs(t);
    assert_eq!(dirs, vec![t.to_path_buf()]);
}

#[test]
fn test_recursive_discovery_stops_at_nested_project_root() {
    use sift::planner::plan_organize_recursive;
    use sift::scanner::discover_recursive_dirs;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("app/src")).unwrap();
    std::fs::create_dir_all(t.join("app/target")).unwrap();
    File::create(t.join("app/Cargo.toml")).unwrap();
    File::create(t.join("app/src/main.rs")).unwrap();
    File::create(t.join("app/target/binary")).unwrap();

    let dirs = discover_recursive_dirs(t);
    assert_eq!(
        dirs,
        vec![t.to_path_buf()],
        "the whole nested project subtree (app/, app/src/, app/target/) must be protected"
    );

    // The organize plan must never touch anything inside the nested project.
    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(!rp
        .plan
        .actions
        .iter()
        .any(|a| a.src.ends_with("main.rs") || a.src.ends_with("binary")));
    let app_action = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src == t.join("app"))
        .unwrap();
    assert_eq!(app_action.op, Op::Skip);
    assert_eq!(app_action.reason.as_deref(), Some("software project"));
}

#[test]
fn test_recursive_discovery_skips_known_build_and_vcs_dir_names() {
    use sift::scanner::discover_recursive_dirs;
    let d = tempdir().unwrap();
    let t = d.path();
    // Bare directories with these exact names, with no other project marker,
    // must still never be traversed.
    std::fs::create_dir_all(t.join("node_modules/inner")).unwrap();
    std::fs::create_dir_all(t.join("target/inner")).unwrap();
    std::fs::create_dir_all(t.join(".git/inner")).unwrap();
    std::fs::create_dir_all(t.join(".venv/inner")).unwrap();

    let dirs = discover_recursive_dirs(t);
    assert_eq!(dirs, vec![t.to_path_buf()]);
}

#[test]
fn test_recursive_organize_collision_in_nested_dir() {
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("nested/Documents")).unwrap();
    File::create(t.join("nested/Documents/report.pdf")).unwrap();
    File::create(t.join("nested/report.pdf")).unwrap();

    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    let action = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src == t.join("nested/report.pdf"))
        .unwrap();
    assert_eq!(action.op, Op::Skip);
    assert_eq!(action.reason.as_deref(), Some("collision"));
}

#[test]
fn test_recursive_organize_broken_symlink_collision_in_nested_dir() {
    use sift::planner::plan_organize_recursive;
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("nested/Documents")).unwrap();
    symlink("/nonexistent", t.join("nested/Documents/report.pdf")).unwrap();
    File::create(t.join("nested/report.pdf")).unwrap();

    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    let action = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src == t.join("nested/report.pdf"))
        .unwrap();
    assert_eq!(action.op, Op::Skip);
    assert_eq!(action.reason.as_deref(), Some("collision"));
}

#[test]
fn test_recursive_executor_toctou_collision_in_nested_dir() {
    use sift::domain::HistoryItem;
    use sift::executor::execute_plan;
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    std::fs::create_dir_all(t.join("nested")).unwrap();
    let src = t.join("nested/race.txt");
    File::create(&src).unwrap();
    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());

    // Race: the destination appears after planning but before execution.
    std::fs::create_dir_all(t.join("nested/Documents")).unwrap();
    File::create(t.join("nested/Documents/race.txt")).unwrap();

    let (_id, outcomes) = execute_plan(rp.plan, t.to_str().unwrap(), "organize-recursive", None);
    assert!(
        src.exists(),
        "a failed move must leave the source untouched"
    );

    let mut found_failure = false;
    for h in std::fs::read_dir(&hist_dir).unwrap().flatten() {
        let s = std::fs::read_to_string(h.path()).unwrap();
        if let Ok(item) = serde_json::from_str::<HistoryItem>(&s) {
            for o in &item.outcomes {
                if o.src.ends_with("race.txt") && o.op == Op::Move {
                    assert!(o.result.is_err());
                    found_failure = true;
                }
            }
        }
    }
    assert!(found_failure);
    let _ = outcomes;
    clear_test_history_dir();
}

#[test]
fn test_recursive_executor_refuses_symlink_ancestor_escape() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    let outside = tempdir().unwrap();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/file.txt")).unwrap();
    // Appears after any planning would have happened, deep under `nested/`.
    symlink(outside.path(), t.join("nested/Escape")).unwrap();

    let escape_move = sift::domain::Action {
        src: t.join("nested/file.txt"),
        dst: Some(t.join("nested/Escape").join("file.txt")),
        op: Op::Move,
        reason: Some("hand-crafted TOCTOU attempt".into()),
        undoable: true,
    };
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    sift::executor::execute_plan(
        sift::domain::Plan {
            actions: vec![escape_move],
        },
        t.to_str().unwrap(),
        "organize-recursive",
        None,
    );
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    assert!(t.join("nested/file.txt").exists());
    clear_test_history_dir();
}

#[test]
fn test_recursive_history_records_one_operation() {
    use sift::executor::execute_plan;
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    File::create(t.join("root.pdf")).unwrap();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/nested.jpg")).unwrap();

    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    let (id, _outcomes) = execute_plan(rp.plan, t.to_str().unwrap(), "organize-recursive", None);

    let files: Vec<_> = std::fs::read_dir(&hist_dir).unwrap().flatten().collect();
    assert_eq!(
        files.len(),
        1,
        "one recursive apply must be exactly one history record"
    );
    let content = std::fs::read_to_string(files[0].path()).unwrap();
    assert!(content.contains("root.pdf"));
    assert!(content.contains("nested.jpg"));
    assert!(content.contains(&id));
    clear_test_history_dir();
}

#[test]
fn test_recursive_undo_restores_nested_files() {
    use sift::executor::execute_plan;
    use sift::history::cmd_undo;
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    File::create(t.join("root.pdf")).unwrap();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/nested.jpg")).unwrap();

    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    let (id, _outcomes) = execute_plan(rp.plan, t.to_str().unwrap(), "organize-recursive", None);
    assert!(t.join("Documents/root.pdf").exists());
    assert!(t.join("nested/Images/nested.jpg").exists());

    cmd_undo(id);

    assert!(t.join("root.pdf").exists());
    assert!(t.join("nested/nested.jpg").exists());
    assert!(!t.join("Documents/root.pdf").exists());
    assert!(!t.join("nested/Images/nested.jpg").exists());
    clear_test_history_dir();
}

#[test]
fn test_recursive_doctor_finds_nested_findings() {
    use sift::scanner::doctor_findings_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/SLACK_TOKEN.txt")).unwrap();

    let findings = doctor_findings_recursive(t);
    assert!(findings
        .iter()
        .any(|f| f.path.ends_with("nested/SLACK_TOKEN.txt") && f.reason.contains("Sensitive")));
}

#[test]
fn test_recursive_doctor_does_not_inspect_protected_subtree() {
    use sift::scanner::doctor_findings_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("app")).unwrap();
    File::create(t.join("app/Cargo.toml")).unwrap();
    File::create(t.join("app/SLACK_TOKEN.txt")).unwrap();

    let findings = doctor_findings_recursive(t);
    assert!(
        !findings
            .iter()
            .any(|f| f.path.ends_with("app/SLACK_TOKEN.txt")),
        "doctor must never inspect inside a protected project subtree"
    );
    assert!(findings
        .iter()
        .any(|f| f.path.ends_with("app") && f.reason.contains("Software project")));
}

#[test]
fn test_recursive_scan_includes_nested_entries() {
    use sift::scanner::scan_entries_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("root.pdf")).unwrap();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/nested.jpg")).unwrap();

    let entries = scan_entries_recursive(t);
    assert!(entries.iter().any(|e| e.path == t.join("root.pdf")));
    assert!(entries.iter().any(|e| e.path == t.join("nested")));
    assert!(entries
        .iter()
        .any(|e| e.path == t.join("nested/nested.jpg")));
}

#[test]
fn test_recursive_discovery_is_deterministic() {
    use sift::scanner::discover_recursive_dirs;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("b/inner")).unwrap();
    std::fs::create_dir_all(t.join("a/inner")).unwrap();
    std::fs::create_dir_all(t.join("c")).unwrap();

    let first = discover_recursive_dirs(t);
    let second = discover_recursive_dirs(t);
    assert_eq!(first, second);
    // root first, then lexicographic.
    assert_eq!(first[0], t.to_path_buf());
    assert!(first.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn test_clean_recursive_flag_rejected_by_cli() {
    use clap::Parser;
    use sift::cli::Cli;
    let result = Cli::try_parse_from(["sift", "clean", ".", "--recursive"]);
    assert!(
        result.is_err(),
        "clean must not accept --recursive; clap should reject it as unknown"
    );
}

// ============================================================
// Classification coverage: Code / Data / Other fallback
// ============================================================

fn move_dest<'a>(plan: &'a sift::domain::Plan, name: &str) -> &'a sift::domain::Action {
    plan.actions
        .iter()
        .find(|a| a.src.file_name().and_then(|n| n.to_str()) == Some(name))
        .unwrap_or_else(|| panic!("no action found for {name}"))
}

#[test]
fn test_new_categories_and_fallback_classification() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("photo.svg")).unwrap();
    File::create(t.join("PHOTO.JPG")).unwrap();
    File::create(t.join("data.json")).unwrap();
    File::create(t.join("codigo.js")).unwrap();
    File::create(t.join("model.blend")).unwrap();
    File::create(t.join("model.blend1")).unwrap();
    File::create(t.join("arquivo-desconhecido.xyz")).unwrap();
    File::create(t.join("README")).unwrap(); // no extension at all

    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());

    let expect_dir = |name: &str, dir: &str| {
        let a = move_dest(&plan, name);
        assert_eq!(a.op, Op::Move, "{name} should be moved, not skipped");
        assert_eq!(
            a.dst.as_ref().unwrap(),
            &t.join(dir).join(name),
            "{name} should land in {dir}/"
        );
    };
    expect_dir("photo.svg", "Images");
    expect_dir("PHOTO.JPG", "Images");
    expect_dir("data.json", "Data");
    expect_dir("codigo.js", "Code");
    expect_dir("model.blend", "3D");
    expect_dir("model.blend1", "3D");
    expect_dir("arquivo-desconhecido.xyz", "Other");
    expect_dir("README", "Other");
}

#[test]
fn test_unclassified_no_longer_means_skip() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("mystery.xyz")).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let a = move_dest(&plan, "mystery.xyz");
    assert_eq!(
        a.op,
        Op::Move,
        "an ordinary unrecognized file must be organized into Other/, not skipped"
    );
    assert!(!plan
        .actions
        .iter()
        .any(|a| a.src.ends_with("mystery.xyz") && a.op == Op::Skip));
}

#[test]
fn test_hidden_unknown_file_still_skipped() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join(".hidden.xyz")).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let a = move_dest(&plan, ".hidden.xyz");
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("hidden file"));
}

#[test]
fn test_symlink_unknown_file_still_skipped() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    let target = t.join("real.xyz");
    File::create(&target).unwrap();
    symlink(&target, t.join("link.xyz")).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let a = move_dest(&plan, "link.xyz");
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("symlink"));
}

#[test]
fn test_broken_symlink_unknown_extension_still_skipped() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    symlink("/nonexistent-xyz", t.join("dangling.xyz")).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let a = move_dest(&plan, "dangling.xyz");
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("symlink"));
}

#[test]
fn test_protected_directory_never_gets_other_fallback() {
    let d = tempdir().unwrap();
    let t = d.path();
    // A directory whose *name* looks like a file extension must still be
    // treated as a directory, never classified/moved into Other/.
    std::fs::create_dir_all(t.join("weird.xyz")).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let a = move_dest(&plan, "weird.xyz");
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("directory"));
}

#[test]
fn test_project_root_subtree_never_extracted_into_other() {
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("my-app/src")).unwrap();
    File::create(t.join("my-app/package.json")).unwrap();
    File::create(t.join("my-app/src/index.js")).unwrap();
    File::create(t.join("my-app/notes.xyz")).unwrap();

    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(
        !rp.plan
            .actions
            .iter()
            .any(|a| a.src.ends_with("index.js") || a.src.ends_with("notes.xyz")),
        "nothing inside a protected project root may be classified or moved, \
         even into the Code/Other fallback"
    );
    let app_action = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src == t.join("my-app"))
        .unwrap();
    assert_eq!(app_action.op, Op::Skip);
    assert_eq!(app_action.reason.as_deref(), Some("software project"));
}

#[test]
fn test_config_skip_overrides_builtin_data_category() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("keep.json")).unwrap();
    let rules = vec![make_rule("skip json", "*.json", "Skip", None)];
    let plan = plan_organize(t.to_str().unwrap(), &rules, &CategoryDB::default());
    let a = move_dest(&plan, "keep.json");
    assert_eq!(
        a.op,
        Op::Skip,
        "a config Skip rule must win over the built-in Data category"
    );
}

#[test]
fn test_config_move_overrides_builtin_code_category() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("script.js")).unwrap();
    let rules = vec![make_rule("scripts", "*.js", "Move", Some("Scripts"))];
    let plan = plan_organize(t.to_str().unwrap(), &rules, &CategoryDB::default());
    let a = move_dest(&plan, "script.js");
    assert_eq!(a.op, Op::Move);
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("Scripts/script.js"));
}

#[test]
fn test_collision_in_other_directory_is_skipped() {
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("Other")).unwrap();
    File::create(t.join("Other/mystery.xyz")).unwrap();
    File::create(t.join("mystery.xyz")).unwrap();

    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let a = move_dest(&plan, "mystery.xyz");
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("collision"));
}

#[test]
fn test_broken_symlink_collision_in_other_directory_is_skipped() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("Other")).unwrap();
    symlink("/nonexistent", t.join("Other/mystery.xyz")).unwrap();
    File::create(t.join("mystery.xyz")).unwrap();

    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let a = move_dest(&plan, "mystery.xyz");
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("collision"));
}

#[test]
fn test_other_directory_creation_is_explicit_createdir() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("mystery.xyz")).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(plan
        .actions
        .iter()
        .any(|a| a.op == Op::CreateDir && a.src == t.join("Other")));
}

#[test]
fn test_recursive_new_categories_stay_local() {
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("foo.xyz")).unwrap();
    std::fs::create_dir_all(t.join("nested")).unwrap();
    File::create(t.join("nested/script.js")).unwrap();
    File::create(t.join("nested/payload.json")).unwrap();
    File::create(t.join("nested/model.blend")).unwrap();

    let rp = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    let dst_of = |name: &str| {
        rp.plan
            .actions
            .iter()
            .find(|a| a.src.file_name().and_then(|n| n.to_str()) == Some(name))
            .unwrap()
            .dst
            .clone()
            .unwrap()
    };
    assert_eq!(dst_of("foo.xyz"), t.join("Other/foo.xyz"));
    assert_eq!(dst_of("script.js"), t.join("nested/Code/script.js"));
    assert_eq!(dst_of("payload.json"), t.join("nested/Data/payload.json"));
    assert_eq!(dst_of("model.blend"), t.join("nested/3D/model.blend"));
    assert!(
        !rp.plan.actions.iter().any(|a| a
            .dst
            .as_ref()
            .map(|d| d.starts_with(t.join("Code"))
                || d.starts_with(t.join("Data"))
                || d.starts_with(t.join("3D")))
            .unwrap_or(false)),
        "nested files must never be flattened into the root's own category folders"
    );
}

#[test]
fn test_recursive_new_category_dirs_are_terminal() {
    use sift::scanner::discover_recursive_dirs;
    let d = tempdir().unwrap();
    let t = d.path();
    std::fs::create_dir_all(t.join("Code")).unwrap();
    std::fs::create_dir_all(t.join("Data")).unwrap();
    std::fs::create_dir_all(t.join("Other")).unwrap();
    File::create(t.join("Code/old.js")).unwrap();
    File::create(t.join("Data/old.json")).unwrap();
    File::create(t.join("Other/old.xyz")).unwrap();

    let dirs = discover_recursive_dirs(t);
    assert_eq!(
        dirs,
        vec![t.to_path_buf()],
        "Code/Data/Other must be terminal, exactly like Documents/Images"
    );
}

#[test]
fn test_organize_second_run_is_idempotent() {
    use sift::executor::execute_plan;
    use sift::planner::plan_organize_recursive;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    File::create(t.join("root.pdf")).unwrap();
    File::create(t.join("script.js")).unwrap();
    File::create(t.join("data.json")).unwrap();
    File::create(t.join("mystery.xyz")).unwrap();

    let rp1 = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    execute_plan(rp1.plan, t.to_str().unwrap(), "organize-recursive", None);

    // Run again on the now-organized tree.
    let rp2 = plan_organize_recursive(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(
        !rp2.plan.actions.iter().any(|a| a.op == Op::Move),
        "a second run must find nothing left to move"
    );
    assert!(!t.join("Documents/Documents").exists());
    assert!(!t.join("Code/Code").exists());
    assert!(!t.join("Data/Data").exists());
    assert!(!t.join("Other/Other").exists());
    clear_test_history_dir();
}

#[test]
fn test_executor_toctou_protection_applies_to_other_category() {
    use sift::domain::HistoryItem;
    use sift::executor::execute_plan;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    let src = t.join("race.xyz");
    File::create(&src).unwrap();
    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    assert!(plan
        .actions
        .iter()
        .any(|a| a.src.ends_with("race.xyz") && a.op == Op::Move));

    // Race: destination appears after planning but before execution.
    std::fs::create_dir_all(t.join("Other")).unwrap();
    File::create(t.join("Other/race.xyz")).unwrap();

    execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert!(
        src.exists(),
        "a failed move must leave the source untouched"
    );

    let mut found_failure = false;
    for h in std::fs::read_dir(&hist_dir).unwrap().flatten() {
        let s = std::fs::read_to_string(h.path()).unwrap();
        if let Ok(item) = serde_json::from_str::<HistoryItem>(&s) {
            for o in &item.outcomes {
                if o.src.ends_with("race.xyz") && o.op == Op::Move {
                    assert!(o.result.is_err());
                    found_failure = true;
                }
            }
        }
    }
    assert!(found_failure);
    clear_test_history_dir();
}

#[test]
fn test_dry_run_zero_mutation_for_new_categories() {
    let d = tempdir().unwrap();
    let t = d.path();
    File::create(t.join("photo.svg")).unwrap();
    File::create(t.join("data.json")).unwrap();
    File::create(t.join("codigo.js")).unwrap();
    File::create(t.join("model.blend")).unwrap();
    File::create(t.join("mystery.xyz")).unwrap();

    let _ = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    for name in ["Images", "Data", "Code", "3D", "Other"] {
        assert!(
            !t.join(name).exists(),
            "planning must never create {name}/ before --apply"
        );
    }
    for name in [
        "photo.svg",
        "data.json",
        "codigo.js",
        "model.blend",
        "mystery.xyz",
    ] {
        assert!(t.join(name).exists());
    }
}

#[test]
fn test_undo_restores_files_moved_into_new_categories() {
    use sift::executor::execute_plan;
    use sift::history::cmd_undo;
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t));

    File::create(t.join("codigo.js")).unwrap();
    File::create(t.join("data.json")).unwrap();
    File::create(t.join("mystery.xyz")).unwrap();

    let plan = plan_organize(t.to_str().unwrap(), &[], &CategoryDB::default());
    let (id, _outcomes) = execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert!(t.join("Code/codigo.js").exists());
    assert!(t.join("Data/data.json").exists());
    assert!(t.join("Other/mystery.xyz").exists());

    cmd_undo(id);

    assert!(t.join("codigo.js").exists());
    assert!(t.join("data.json").exists());
    assert!(t.join("mystery.xyz").exists());
    assert!(!t.join("Code/codigo.js").exists());
    assert!(!t.join("Data/data.json").exists());
    assert!(!t.join("Other/mystery.xyz").exists());
    clear_test_history_dir();
}
