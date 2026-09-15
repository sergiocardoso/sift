//! Write-ahead execution journal regression suite.
//!
//! These tests exercise the safety contract directly: no mutation before
//! durable intent, no lost mutation when the outcome write fails, no undo
//! of unproven actions, and backward compatibility with legacy history.
//!
//! Failure injection is a thread-local seam (`history::inject_journal_fault`)
//! so the exact journal write stage can be failed without touching the real
//! disk or relying on `chmod`/timing.

use sift::classifier::CategoryDB;
use sift::config::EffectivePolicy;
use sift::domain::{
    Action, ActionProbe, ActionState, ExecutionState, HistoryItem, Op, Plan, PreparedProbe,
    JOURNAL_VERSION,
};
use sift::executor::{execute_plan, ExecutionFailure};
use sift::history::{self, cmd_undo, record_history, JournalStage};
use sift::recovery::{probe_action, reconcile_item};
use sift::render;
use sift::watch::engine::process_candidate;
use std::fs;
use std::path::{Path, PathBuf};

// ----------------------------------------------------------------- helpers

fn mv(src: &Path, dst: &Path) -> Action {
    Action {
        src: src.to_path_buf(),
        dst: Some(dst.to_path_buf()),
        op: Op::Move,
        reason: None,
        undoable: true,
    }
}

fn plan(actions: Vec<Action>) -> Plan {
    Plan { actions }
}

/// Isolates history writes to `<tempdir>/.sift-history` for this thread.
fn setup() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    history::set_test_history_dir(d.path().join(".sift-history"));
    d
}

fn teardown() {
    history::clear_test_history_dir();
    history::clear_journal_fault();
}

fn hist_dir(root: &Path) -> PathBuf {
    root.join(".sift-history")
}

fn journal_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(hist_dir(root))
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn load_only_journal(root: &Path) -> HistoryItem {
    let files = journal_files(root);
    assert_eq!(files.len(), 1, "expected one journal file, got {files:?}");
    serde_json::from_str(&fs::read_to_string(&files[0]).unwrap()).unwrap()
}

fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

fn write_prepared_journal(
    id: &str,
    actions: Vec<Action>,
    probe: Option<ActionProbe>,
) -> HistoryItem {
    let n = actions.len();
    let item = HistoryItem {
        id: id.to_string(),
        actions,
        timestamp: 0,
        outcomes: Vec::new(),
        kind: "organize".to_string(),
        origin: "manual".to_string(),
        watch_root: None,
        execution_state: ExecutionState::Running,
        action_states: vec![ActionState::Prepared; n],
        prepared_probe: probe.map(|probe| PreparedProbe {
            action_index: 0,
            probe,
        }),
        journal_version: JOURNAL_VERSION,
    };
    record_history(&item).unwrap();
    item
}

// ---------------------------------------------------- TEST 1 / 12 / 3 etc.

/// TEST 1 — initial journal persistence failure means zero mutations.
///
/// Two actions: if the executor skipped the initial write and mutated
/// first, no `Initial` fault would ever be triggered and the sources would
/// be gone.
#[test]
fn test1_initial_journal_failure_causes_zero_mutations() {
    let d = setup();
    let t = d.path();
    fs::create_dir_all(t.join("Documents")).unwrap();
    let a_src = t.join("invoice.pdf");
    let a_dst = t.join("Documents/invoice.pdf");
    let b_src = t.join("photo.jpg");
    let b_dst = t.join("Documents/photo.jpg");
    fs::write(&a_src, b"x").unwrap();
    fs::write(&b_src, b"y").unwrap();

    history::inject_journal_fault(JournalStage::Initial, 0);
    let err = execute_plan(
        plan(vec![mv(&a_src, &a_dst), mv(&b_src, &b_dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .unwrap_err();

    assert_eq!(err.failure, ExecutionFailure::NotStarted);
    assert!(err.history_id.is_none(), "no id existed to persist");
    assert!(!err.may_have_mutated);
    assert!(exists(&a_src), "first source must remain");
    assert!(exists(&b_src), "second source must remain");
    assert!(!exists(&a_dst), "first destination must not exist");
    assert!(!exists(&b_dst), "second destination must not exist");
    assert!(journal_files(t).is_empty(), "no journal may be written");
    teardown();
}

/// TEST 2 — the journal is durable before the first mutation.
///
/// Two actions; the *second* action's `prepared` write is failed. The first
/// action's durable `succeeded` state proves the journal was written and
/// updated around the first mutation, while the second action is still
/// `pending`; paired with TEST 1 (which fails the initial write and sees
/// zero mutations) this pins the ordering.
#[test]
fn test2_journal_is_persisted_before_the_first_mutation() {
    let d = setup();
    let t = d.path();
    let a_src = t.join("a.pdf");
    let a_dst = t.join("Documents/a.pdf");
    let b_src = t.join("b.pdf");
    let b_dst = t.join("Documents/b.pdf");
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(&a_src, b"a").unwrap();
    fs::write(&b_src, b"b").unwrap();

    history::inject_journal_fault(JournalStage::Prepare, 1);
    let err = execute_plan(
        plan(vec![mv(&a_src, &a_dst), mv(&b_src, &b_dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .unwrap_err();

    assert_eq!(err.failure, ExecutionFailure::Interrupted);
    assert!(err.history_id.is_some());
    // First action ran and is durably recorded; the second never mutated.
    assert!(exists(&a_dst), "first mutation happened");
    assert!(!exists(&a_src));
    assert!(exists(&b_src), "second action never mutated");
    assert!(!exists(&b_dst));

    let item = load_only_journal(t);
    assert_eq!(item.state_of(0), ActionState::Succeeded);
    assert_eq!(item.state_of(1), ActionState::Pending);
    assert_eq!(item.journal_version, JOURNAL_VERSION);
    teardown();
}

/// TEST 3 — a mutation cannot occur if its `prepared` write fails.
#[test]
fn test3_prepare_failure_blocks_mutation() {
    let d = setup();
    let t = d.path();
    let src = t.join("c.pdf");
    let dst = t.join("Documents/c.pdf");
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(&src, b"c").unwrap();

    history::inject_journal_fault(JournalStage::Prepare, 0);
    let err = execute_plan(
        plan(vec![mv(&src, &dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .unwrap_err();

    assert_eq!(err.failure, ExecutionFailure::Interrupted);
    assert!(!err.may_have_mutated);
    assert!(exists(&src), "source must be unchanged");
    assert!(!exists(&dst), "destination must be absent");

    let item = load_only_journal(t);
    assert_eq!(item.state_of(0), ActionState::Pending);
    assert_eq!(item.execution_state, ExecutionState::Running);
    teardown();
}

/// TEST 4 — the critical window: mutation succeeded, outcome write failed.
#[test]
fn test4_mutation_success_outcome_write_failure_is_never_lost() {
    let d = setup();
    let t = d.path();
    let a_src = t.join("a.jpg");
    let a_dst = t.join("Images/a.jpg");
    let b_src = t.join("b.jpg");
    let b_dst = t.join("Images/b.jpg");
    fs::create_dir_all(t.join("Images")).unwrap();
    fs::write(&a_src, b"a").unwrap();
    fs::write(&b_src, b"b").unwrap();

    history::inject_journal_fault(JournalStage::Outcome, 0);
    let err = execute_plan(
        plan(vec![mv(&a_src, &a_dst), mv(&b_src, &b_dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .unwrap_err();

    assert_eq!(err.failure, ExecutionFailure::Interrupted);
    assert!(err.may_have_mutated, "the mutation may have happened");
    let id = err.history_id.clone().expect("journal was persisted");
    assert!(err.outcomes.is_empty(), "no outcome is durable yet");

    // The mutation really happened...
    assert!(exists(&a_dst));
    assert!(!exists(&a_src));
    // ...and no later action ran.
    assert!(exists(&b_src));
    assert!(!exists(&b_dst));

    // The durable journal truthfully still says `prepared`, never completed.
    let item = load_only_journal(t);
    assert_eq!(item.id, id);
    assert_eq!(item.state_of(0), ActionState::Prepared);
    assert_eq!(item.execution_state, ExecutionState::Running);
    assert_ne!(item.execution_state, ExecutionState::Completed);
    assert_eq!(item.effective_state(), ExecutionState::Interrupted);
    teardown();
}

/// TEST 5 — recovery of a prepared move that did not happen stays unproven.
#[test]
fn test5_recovery_does_not_invent_success_when_nothing_moved() {
    let d = setup();
    let t = d.path();
    let src = t.join("d.pdf");
    let dst = t.join("Documents/d.pdf");
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(&src, b"d").unwrap();

    let action = mv(&src, &dst);
    let probe = probe_action(&action);
    write_prepared_journal("hist-recover-5", vec![action], probe);

    let mut item = load_only_journal(t);
    assert_eq!(item.state_of(0), ActionState::Prepared);
    let changed = reconcile_item(&mut item);

    assert!(changed);
    assert_eq!(item.state_of(0), ActionState::Ambiguous);
    assert_ne!(item.state_of(0), ActionState::Succeeded);
    assert_eq!(item.execution_state, ExecutionState::Interrupted);
    // Reconciliation never touches the filesystem.
    assert!(exists(&src));
    assert!(!exists(&dst));
    teardown();
}

/// TEST 6 — recovery *can* prove a move that really happened, and only then
/// does it become undoable.
#[test]
fn test6_recovery_proves_completed_move_then_undo_works() {
    let d = setup();
    let t = d.path();
    let src = t.join("photo.jpg");
    let dst = t.join("Images/photo.jpg");
    fs::create_dir_all(t.join("Images")).unwrap();
    fs::write(&src, b"photo-data").unwrap();

    history::inject_journal_fault(JournalStage::Outcome, 0);
    let err = execute_plan(
        plan(vec![mv(&src, &dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .unwrap_err();
    let id = err.history_id.clone().unwrap();
    assert!(exists(&dst));
    assert!(!exists(&src));

    let mut item = load_only_journal(t);
    assert_eq!(item.state_of(0), ActionState::Prepared);
    assert!(reconcile_item(&mut item), "identity probe proves the move");
    assert_eq!(item.state_of(0), ActionState::Succeeded);
    assert_eq!(item.execution_state, ExecutionState::Completed);
    record_history(&item).unwrap();

    // Only now is the proven move eligible for undo.
    assert!(cmd_undo(id));
    assert!(exists(&src), "undo restored the proven move");
    assert!(!exists(&dst));
    teardown();
}

/// TEST 7 — an unclassifiable state is marked ambiguous, never guessed or
/// retried or undone.
#[test]
fn test7_recovery_marks_ambiguous_and_never_guesses() {
    let d = setup();
    let t = d.path();
    let src = t.join("orig.txt");
    let dst = t.join("Documents/orig.txt");
    fs::create_dir_all(t.join("Documents")).unwrap();
    // The destination exists, but it is a *different* object than the one
    // the probe captured (no src was ever there), so identity cannot match.
    fs::write(&dst, b"decoy").unwrap();
    let action = mv(&src, &dst);
    let probe = ActionProbe {
        dev: Some(u64::MAX),
        ino: Some(u64::MAX),
        len: Some(5),
        mtime: None,
        is_dir: false,
        is_symlink: false,
    };
    write_prepared_journal("hist-recover-7", vec![action], Some(probe));

    let mut item = load_only_journal(t);
    assert!(reconcile_item(&mut item));
    assert_eq!(item.state_of(0), ActionState::Ambiguous);
    assert_eq!(item.execution_state, ExecutionState::Interrupted);
    // No blind retry, no blind undo.
    assert!(!exists(&src));
    assert!(exists(&dst));
    assert_eq!(fs::read_to_string(&dst).unwrap(), "decoy");
    assert!(cmd_undo(item.id.clone()));
    assert!(!exists(&src), "ambiguous action must not be undone");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "decoy");
    teardown();
}

/// TEST 8 — one action succeeds, a later one fails normally. History must
/// record both, and execution still completes.
#[test]
fn test8_partial_action_failure_is_recorded() {
    let d = setup();
    let t = d.path();
    let ok_src = t.join("ok.pdf");
    let ok_dst = t.join("Documents/ok.pdf");
    let fail_src = t.join("fail.pdf");
    let fail_dst = t.join("Documents/fail.pdf");
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(&ok_src, b"ok").unwrap();
    fs::write(&fail_src, b"fail").unwrap();
    // Occupied destination makes the second Move fail at execution time.
    fs::write(&fail_dst, b"already-here").unwrap();

    let report = execute_plan(
        plan(vec![mv(&ok_src, &ok_dst), mv(&fail_src, &fail_dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .expect("an ordinary action failure is not an execution error");

    assert!(report.outcomes[0].result.is_ok());
    assert!(report.outcomes[1].result.is_err());
    assert_eq!(report.execution_state, ExecutionState::Completed);
    assert!(exists(&ok_dst));
    assert!(exists(&fail_src), "failed action left its source untouched");
    assert_eq!(fs::read_to_string(&fail_dst).unwrap(), "already-here");

    let item = load_only_journal(t);
    assert_eq!(item.state_of(0), ActionState::Succeeded);
    assert_eq!(item.state_of(1), ActionState::Failed);
    assert_eq!(item.execution_state, ExecutionState::Completed);
    teardown();
}

/// TEST 9 — an outcome-write failure stops all later mutations.
#[test]
fn test9_outcome_write_failure_stops_later_actions() {
    let d = setup();
    let t = d.path();
    fs::create_dir_all(t.join("Images")).unwrap();
    let mut actions = Vec::new();
    let mut srcs = Vec::new();
    let mut dsts = Vec::new();
    for name in ["one.jpg", "two.jpg", "three.jpg"] {
        let s = t.join(name);
        let x = t.join("Images").join(name);
        fs::write(&s, name.as_bytes()).unwrap();
        actions.push(mv(&s, &x));
        srcs.push(s);
        dsts.push(x);
    }

    // Fail the *second* outcome write (action index 1).
    history::inject_journal_fault(JournalStage::Outcome, 1);
    let err = execute_plan(plan(actions), t.to_str().unwrap(), "organize", None).unwrap_err();

    assert!(err.may_have_mutated);
    assert_eq!(err.outcomes.len(), 1, "only action 0 is durably recorded");
    assert!(exists(&dsts[1]), "action 1 mutated before its write failed");
    assert!(exists(&srcs[2]), "action 2 never started");
    assert!(!exists(&dsts[2]), "action 2 never mutated");

    let item = load_only_journal(t);
    assert_eq!(item.state_of(0), ActionState::Succeeded);
    assert_eq!(item.state_of(1), ActionState::Prepared);
    assert_eq!(item.state_of(2), ActionState::Pending);
    assert_eq!(item.execution_state, ExecutionState::Running);
    assert_eq!(item.effective_state(), ExecutionState::Interrupted);
    teardown();
}

/// TEST 10 — the final `completed` write can fail without losing outcomes;
/// reconciliation finalizes it deterministically.
#[test]
fn test10_finalize_write_failure_is_recoverable() {
    let d = setup();
    let t = d.path();
    let src = t.join("final.pdf");
    let dst = t.join("Documents/final.pdf");
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(&src, b"final").unwrap();

    history::inject_journal_fault(JournalStage::Finalize, 0);
    let err = execute_plan(
        plan(vec![mv(&src, &dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .unwrap_err();
    assert_eq!(err.failure, ExecutionFailure::Interrupted);
    assert_eq!(err.outcomes.len(), 1, "the action outcome is durable");
    assert!(exists(&dst));

    let mut item = load_only_journal(t);
    assert_eq!(item.state_of(0), ActionState::Succeeded);
    assert!(item.all_actions_terminal());
    assert_eq!(item.execution_state, ExecutionState::Running);
    assert_eq!(item.effective_state(), ExecutionState::Completed);

    // Deterministic finalization.
    assert!(reconcile_item(&mut item));
    assert_eq!(item.execution_state, ExecutionState::Completed);
    teardown();
}

/// TEST 11 — a legacy record (old schema, no new fields) deserializes as a
/// completed execution.
#[test]
fn test11_legacy_history_deserializes_as_completed() {
    let d = setup();
    let t = d.path();
    let src = t.join("legacy.txt");
    let dst = t.join("Documents/legacy.txt");
    let legacy = serde_json::json!({
        "id": "hist-legacy-1",
        "actions": [{"src": src, "dst": dst, "op": "Move", "reason": null, "undoable": true}],
        "timestamp": 1,
        "outcomes": [{"src": src, "dst": dst, "op": "Move", "result": {"Ok": null}, "undoable": false}],
        "kind": "organize",
        "origin": "manual",
        "watch_root": null
    });
    fs::create_dir_all(hist_dir(t)).unwrap();
    fs::write(
        hist_dir(t).join("hist-legacy-1.json"),
        serde_json::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();

    let item: HistoryItem = serde_json::from_value(legacy).unwrap();
    assert_eq!(item.journal_version, 0, "missing version means legacy");
    assert!(item.action_states.is_empty());
    assert_eq!(item.execution_state, ExecutionState::Completed);
    assert_eq!(item.effective_state(), ExecutionState::Completed);
    assert_eq!(item.state_of(0), ActionState::Succeeded);
    assert_eq!(item.unresolved_actions(), 0);

    let text = render::history_text(&[item]);
    assert!(text.contains("hist-legacy-1"));
    assert!(!text.contains("incomplete"), "legacy must not look broken");
    teardown();
}

/// TEST 11b — undo still works for a legacy history record.
#[test]
fn test11b_legacy_history_undo_still_works() {
    let d = setup();
    let t = d.path();
    let src = t.join("legacy2.txt");
    let dst = t.join("Documents/legacy2.txt");
    fs::create_dir_all(t.join("Documents")).unwrap();
    // Represents a file already moved by a pre-upgrade Sift.
    fs::write(&dst, b"moved").unwrap();

    let legacy = serde_json::json!({
        "id": "hist-legacy-2",
        "actions": [{"src": src, "dst": dst, "op": "Move", "reason": null, "undoable": true}],
        "timestamp": 1,
        "outcomes": [{"src": src, "dst": dst, "op": "Move", "result": {"Ok": null}, "undoable": false}],
        "kind": "organize",
        "origin": "manual",
        "watch_root": null
    });
    fs::create_dir_all(hist_dir(t)).unwrap();
    fs::write(
        hist_dir(t).join("hist-legacy-2.json"),
        serde_json::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();

    assert!(cmd_undo("hist-legacy-2".to_string()));
    assert!(exists(&src));
    assert!(!exists(&dst));
    teardown();
}

/// TEST 12 — a normal apply produces a completed journal and undoable move.
#[test]
fn test12_new_completed_history_is_undoable() {
    let d = setup();
    let t = d.path();
    let src = t.join("new.pdf");
    let dst = t.join("Documents/new.pdf");
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(&src, b"new").unwrap();

    let report = execute_plan(
        plan(vec![mv(&src, &dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .unwrap();
    assert_eq!(report.execution_state, ExecutionState::Completed);
    assert!(exists(&dst));

    let item = load_only_journal(t);
    assert_eq!(item.id, report.history_id);
    assert_eq!(item.execution_state, ExecutionState::Completed);
    assert_eq!(item.state_of(0), ActionState::Succeeded);
    let text = render::history_text(&[item]);
    assert!(text.contains(&report.history_id));
    assert!(!text.contains("incomplete"));

    assert!(cmd_undo(report.history_id));
    assert!(exists(&src));
    assert!(!exists(&dst));
    teardown();
}

/// TEST 13 — an interrupted journal is clearly rendered as incomplete.
#[test]
fn test13_interrupted_history_renders_as_incomplete() {
    let d = setup();
    let t = d.path();
    let src = t.join("interrupted.pdf");
    let dst = t.join("Documents/interrupted.pdf");
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(&src, b"x").unwrap();

    history::inject_journal_fault(JournalStage::Outcome, 0);
    let _ = execute_plan(
        plan(vec![mv(&src, &dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .unwrap_err();

    let item = load_only_journal(t);
    assert_eq!(item.effective_state(), ExecutionState::Interrupted);
    let text = render::history_text(&[item]);
    assert!(text.contains("incomplete"), "text was: {text}");
    assert!(text.contains("interrupted"), "text was: {text}");
    teardown();
}

/// TEST 14 — undo refuses a prepared/unproven action and moves nothing.
#[test]
fn test14_undo_refuses_unproven_action() {
    let d = setup();
    let t = d.path();
    let src = t.join("prepared.pdf");
    let dst = t.join("Documents/prepared.pdf");
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(&src, b"x").unwrap();

    let action = mv(&src, &dst);
    let probe = probe_action(&action);
    let item = write_prepared_journal("hist-unproven", vec![action], probe);

    cmd_undo(item.id.clone());

    assert!(exists(&src), "unproven source must not move");
    assert!(!exists(&dst), "undo must not create the destination");

    // The persisted record has been reconciled to ambiguous, not succeeded.
    let after = load_only_journal(t);
    assert_eq!(after.state_of(0), ActionState::Ambiguous);
    teardown();
}

/// TEST 15 — Watch uses the same write-ahead executor; a journal failure is
/// surfaced, not swallowed, and the file is untouched.
#[test]
fn test15_watch_honours_write_ahead_on_initial_failure() {
    let d = setup();
    let root = d.path();
    let file = root.join("photo.jpg");
    fs::write(&file, b"x").unwrap();

    history::inject_journal_fault(JournalStage::Initial, 0);
    let outcome = process_candidate(
        root,
        &EffectivePolicy::default(),
        &EffectivePolicy::default(),
        &CategoryDB::default(),
        &file,
    );

    assert!(!outcome.organized);
    assert!(outcome.history_id.is_none());
    assert!(
        outcome.failure.is_some(),
        "watch must surface the journal failure"
    );
    assert!(exists(&file), "file must be untouched");
    assert!(!exists(&root.join("Images/photo.jpg")));
    teardown();
}

/// TEST 15b — Watch interrupted before mutation leaves the file untouched
/// and keeps the persisted history id traceable.
#[test]
fn test15b_watch_interrupted_before_mutation_is_traceable() {
    let d = setup();
    let root = d.path();
    let file = root.join("photo.jpg");
    fs::write(&file, b"x").unwrap();

    history::inject_journal_fault(JournalStage::Prepare, 0);
    let outcome = process_candidate(
        root,
        &EffectivePolicy::default(),
        &EffectivePolicy::default(),
        &CategoryDB::default(),
        &file,
    );

    assert!(!outcome.organized);
    assert!(outcome.failure.is_some());
    assert!(outcome.history_id.is_some(), "persisted id stays traceable");
    assert!(exists(&file), "file must be untouched");
    assert!(!exists(&root.join("Images/photo.jpg")));
    teardown();
}

/// TEST 16 — a failed journal update cannot corrupt the last good journal.
#[test]
fn test16_failed_update_leaves_previous_journal_valid() {
    let d = setup();
    let t = d.path();
    let src = t.join("atomic.pdf");
    let dst = t.join("Documents/atomic.pdf");
    let outcome = sift::domain::ActionResult {
        src: src.clone(),
        dst: Some(dst.clone()),
        op: Op::Move,
        result: Ok(()),
        undoable: true,
    };
    let item = HistoryItem {
        id: "hist-atomic".to_string(),
        actions: vec![mv(&src, &dst)],
        timestamp: 10,
        outcomes: vec![outcome],
        kind: "organize".to_string(),
        origin: "manual".to_string(),
        watch_root: None,
        execution_state: ExecutionState::Completed,
        action_states: vec![ActionState::Succeeded],
        prepared_probe: None,
        journal_version: JOURNAL_VERSION,
    };
    record_history(&item).unwrap();
    let file = hist_dir(t).join("hist-atomic.json");
    let before = fs::read_to_string(&file).unwrap();

    // Sabotage the temp path so the next write fails before its rename.
    let tmp = history::history_temp_path("hist-atomic");
    fs::create_dir(&tmp).unwrap();
    let mut updated = item.clone();
    updated.timestamp = 999;
    assert!(record_history(&updated).is_err(), "the update must fail");

    let after = fs::read_to_string(&file).unwrap();
    assert_eq!(
        before, after,
        "a failed update must not corrupt the journal"
    );
    let parsed: HistoryItem = serde_json::from_str(&after).unwrap();
    assert_eq!(parsed.timestamp, 10, "the last good record survives intact");
    let _ = fs::remove_dir(&tmp);
    teardown();
}

/// TEST 17 (review finding) — reconciliation must not be fooled by a
/// hardlink at the destination: the source still being the probed object
/// means the move did not happen.
#[test]
fn test17_reconciliation_refuses_hardlink_false_positive() {
    let d = setup();
    let t = d.path();
    let src = t.join("linked.txt");
    let dst = t.join("Documents/linked.txt");
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(&src, b"linked").unwrap();
    let action = mv(&src, &dst);
    let probe = probe_action(&action);
    // The destination is a hardlink to the *same inode* as the source, and
    // the source was never moved: a naive inode check would say "moved".
    fs::hard_link(&src, &dst).unwrap();
    write_prepared_journal("hist-hardlink", vec![action], probe);

    let mut item = load_only_journal(t);
    assert!(reconcile_item(&mut item));
    assert_eq!(item.state_of(0), ActionState::Ambiguous);
    assert_ne!(item.state_of(0), ActionState::Succeeded);
    assert_eq!(item.execution_state, ExecutionState::Interrupted);
    assert!(exists(&src), "source must still be there");
    assert!(exists(&dst), "destination must still be there");
    teardown();
}

/// TEST 18 (review finding) — if the reconciliation proof cannot be
/// persisted, the proven-but-unpersisted action must not be undone.
#[test]
fn test18_undo_does_not_act_when_reconciliation_cannot_be_persisted() {
    let d = setup();
    let t = d.path();
    let src = t.join("provable.jpg");
    let dst = t.join("Images/provable.jpg");
    fs::create_dir_all(t.join("Images")).unwrap();
    fs::write(&src, b"data").unwrap();

    // Produce a real interrupted journal whose move did happen.
    history::inject_journal_fault(JournalStage::Outcome, 0);
    let err = execute_plan(
        plan(vec![mv(&src, &dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .unwrap_err();
    let id = err.history_id.clone().unwrap();
    assert!(exists(&dst));
    assert!(!exists(&src));

    // Now make persisting the reconciliation proof fail.
    history::inject_journal_fault(JournalStage::Recover, 0);
    cmd_undo(id);

    // The proof was not made durable, so nothing may be restored and the
    // on-disk record still says `prepared`.
    assert!(!exists(&src), "must not undo on an unpersisted proof");
    assert!(exists(&dst));
    let item = load_only_journal(t);
    assert_eq!(item.state_of(0), ActionState::Prepared);
    teardown();
}

/// TEST 19 (review finding) — a failed undo *audit record* write is
/// reported as a failure, not stderr-only.
#[test]
fn test19_undo_record_write_failure_is_reported() {
    let d = setup();
    let t = d.path();
    let src = t.join("rec.pdf");
    let dst = t.join("Documents/rec.pdf");
    fs::create_dir_all(t.join("Documents")).unwrap();
    fs::write(&src, b"x").unwrap();

    let report = execute_plan(
        plan(vec![mv(&src, &dst)]),
        t.to_str().unwrap(),
        "organize",
        None,
    )
    .unwrap();
    history::inject_journal_fault(JournalStage::UndoRecord, 0);
    assert!(
        !cmd_undo(report.history_id),
        "a failed audit record must fail the command"
    );
    // The undo itself still happened; only its record failed.
    assert!(exists(&src));
    assert!(!exists(&dst));
    teardown();
}

/// TEST 20 (review finding) — the duplicate re-verification refuses when
/// either side is not a plain regular file (e.g. a symlink pointing back at
/// the source), closing the plan→execute swap.
#[test]
fn test20_duplicate_reverify_refuses_symlink() {
    let d = setup();
    let t = d.path();
    let a = t.join("a.txt");
    let b = t.join("b.txt");
    fs::write(&a, b"same").unwrap();
    fs::write(&b, b"same").unwrap();
    assert!(
        sift::fs::files_have_identical_content(&a, &b).unwrap(),
        "two identical regular files still compare equal"
    );

    // Replace b with a symlink to a: a naive (follow) comparison sees the
    // same bytes and would treat the only real copy as a duplicate.
    fs::remove_file(&b).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&a, &b).unwrap();
    #[cfg(not(unix))]
    fs::write(&b, b"same").unwrap();
    #[cfg(unix)]
    assert!(
        sift::fs::files_have_identical_content(&a, &b).is_err(),
        "a symlink must be refused"
    );
    assert!(
        sift::fs::files_have_identical_content(&a, t).is_err(),
        "a directory must be refused"
    );
    teardown();
}
