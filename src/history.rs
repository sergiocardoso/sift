use crate::domain::*;
use directories::ProjectDirs;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// A thread-local (not a process-global) override: cargo test runs each test
// function on its own thread, so this keeps concurrently running tests from
// racing over a shared override and momentarily falling back to the real
// global history directory.
thread_local! {
    static TEST_HISTORY_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// The point in the write-ahead lifecycle a journal write belongs to.
/// Tags exist so tests can deterministically fail a *specific* stage
/// (initial / prepare / outcome / finalize / reconcile) without touching
/// the real disk or relying on timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalStage {
    /// The journal's first write, before any mutation.
    Initial,
    /// The per-action write-ahead intent, before a mutation.
    Prepare,
    /// A terminal outcome (succeeded/failed) for one action.
    Outcome,
    /// The final `execution_state = completed` write.
    Finalize,
    /// Persisting a reconciliation result (used by `sift undo`).
    Recover,
    /// Persisting the separate record describing an undo attempt.
    UndoRecord,
}

thread_local! {
    // (stage, number of matching writes to let through before failing).
    static JOURNAL_FAULT: RefCell<Option<(JournalStage, usize)>> = const { RefCell::new(None) };
}

/// Test-only: make the `(skip + 1)`-th journal write of `stage` fail.
/// Thread-local, so parallel tests cannot interfere with each other.
pub fn inject_journal_fault(stage: JournalStage, skip: usize) {
    JOURNAL_FAULT.with(|cell| *cell.borrow_mut() = Some((stage, skip)));
}

/// Test-only: clear any injected journal fault for the current thread.
pub fn clear_journal_fault() {
    JOURNAL_FAULT.with(|cell| *cell.borrow_mut() = None);
}

fn journal_fault_triggered(stage: JournalStage) -> bool {
    JOURNAL_FAULT.with(|cell| {
        let mut slot = cell.borrow_mut();
        match *slot {
            Some((s, 0)) if s == stage => {
                *slot = None;
                true
            }
            Some((s, n)) if s == stage => {
                *slot = Some((s, n - 1));
                false
            }
            _ => false,
        }
    })
}

pub fn cmd_history() {
    let hist_dir = get_history_dir();
    let mut items = vec![];
    let mut unreadable = 0usize;
    if let Ok(files) = fs::read_dir(&hist_dir) {
        for f in files.flatten() {
            let fp = f.path();
            if fp.extension().is_some_and(|e| e == "json") {
                match fs::read_to_string(&fp)
                    .map_err(|e| e.to_string())
                    .and_then(|s| {
                        serde_json::from_str::<HistoryItem>(&s).map_err(|e| e.to_string())
                    }) {
                    Ok(item) => items.push(item),
                    Err(_) => unreadable += 1,
                }
            }
        }
    }
    crate::render::history(&items);
    if unreadable > 0 {
        // Never let an unreadable record vanish silently: it might describe
        // an interrupted execution that changed the filesystem.
        eprintln!(
            "history: {unreadable} record{} could not be read (corrupted or from a newer Sift); they are not shown.",
            if unreadable == 1 { "" } else { "s" }
        );
    }
}

/// Dispatches to the type-specific undo for one successful, undoable
/// outcome. `Op::Move` (files) and `Op::MoveDir` (whole directories, from
/// `sift folders`) are deliberately handled by separate functions with
/// opposite type checks, so neither can be weakened into accepting the
/// other's source type.
fn undo_one(action: &Action, outcome_was_ok: bool) -> Option<ActionResult> {
    if !action.undoable || !outcome_was_ok {
        return None;
    }
    match action.op {
        Op::Move => undo_move_file(action),
        Op::MoveDir => undo_move_dir(action),
        _ => None,
    }
}

/// Attempts to reverse one successful, undoable file Move. Never
/// overwrites anything; revalidates every assumption against the live
/// filesystem via `symlink_metadata` immediately before acting.
fn undo_move_file(action: &Action) -> Option<ActionResult> {
    let dst = action.dst.as_ref()?;
    let refuse = |reason: &str| {
        Some(ActionResult {
            src: dst.clone(),
            dst: Some(action.src.clone()),
            op: Op::Move,
            result: Err(format!("Undo refused: {}", reason)),
            undoable: false,
        })
    };
    // Original source location must be unoccupied before we restore into it.
    if fs::symlink_metadata(&action.src).is_ok() {
        return refuse("original location is occupied");
    }
    let md = match fs::symlink_metadata(dst) {
        Ok(md) => md,
        Err(_) => return refuse("moved destination no longer exists"),
    };
    // Only a plain regular file may be moved back: never a symlink or a
    // directory (a directory sitting at the recorded destination means
    // something else replaced the moved file after the fact).
    if md.file_type().is_symlink() {
        return refuse("moved destination is a symlink");
    }
    if md.is_dir() {
        return refuse("moved destination is a directory");
    }
    if !md.is_file() {
        return refuse("moved destination is not a regular file");
    }
    match fs::rename(dst, &action.src) {
        Ok(()) => Some(ActionResult {
            src: dst.clone(),
            dst: Some(action.src.clone()),
            op: Op::Move,
            result: Ok(()),
            undoable: false,
        }),
        Err(e) => Some(ActionResult {
            src: dst.clone(),
            dst: Some(action.src.clone()),
            op: Op::Move,
            result: Err(format!("Undo failed: {}", e)),
            undoable: false,
        }),
    }
}

/// Attempts to reverse one successful, undoable directory MoveDir (from
/// `sift folders`). Mirrors `undo_move_file`'s exact safety pattern with
/// the opposite type check: the destination must still be a real
/// directory, never a symlink or a plain file.
fn undo_move_dir(action: &Action) -> Option<ActionResult> {
    let dst = action.dst.as_ref()?;
    let refuse = |reason: &str| {
        Some(ActionResult {
            src: dst.clone(),
            dst: Some(action.src.clone()),
            op: Op::MoveDir,
            result: Err(format!("Undo refused: {}", reason)),
            undoable: false,
        })
    };
    if fs::symlink_metadata(&action.src).is_ok() {
        return refuse("original location is occupied");
    }
    let md = match fs::symlink_metadata(dst) {
        Ok(md) => md,
        Err(_) => return refuse("moved destination no longer exists"),
    };
    if md.file_type().is_symlink() {
        return refuse("moved destination is a symlink");
    }
    if !md.is_dir() {
        return refuse("moved destination is not a directory");
    }
    match fs::rename(dst, &action.src) {
        Ok(()) => Some(ActionResult {
            src: dst.clone(),
            dst: Some(action.src.clone()),
            op: Op::MoveDir,
            result: Ok(()),
            undoable: false,
        }),
        Err(e) => Some(ActionResult {
            src: dst.clone(),
            dst: Some(action.src.clone()),
            op: Op::MoveDir,
            result: Err(format!("Undo failed: {}", e)),
            undoable: false,
        }),
    }
}

/// Undoes the moves recorded in history item `id`. The original history
/// record is left untouched; a separate undo record is written describing
/// what the undo attempt actually did. Returns `true` iff nothing was
/// refused (used as the process exit code signal).
///
/// Before touching the filesystem this runs [`crate::recovery`] over the
/// record. Reconciliation can *prove* that an interrupted `Prepared` move
/// completed; when it does, the reconciled state is persisted **first**, so
/// undo never acts on an in-memory guess. Only actions whose state is
/// durably `Succeeded` are ever undone — never `Pending`, `Prepared`,
/// `Ambiguous` or `Failed`.
pub fn cmd_undo(id: String) -> bool {
    let hist_dir = get_history_dir();
    let fp = hist_dir.join(format!("{}.json", id));
    let data = match fs::read_to_string(&fp) {
        Ok(s) => s,
        Err(_) => {
            crate::render::undo_not_found(&id);
            return false;
        }
    };
    let original: HistoryItem = match serde_json::from_str(&data) {
        Ok(h) => h,
        Err(_) => {
            crate::render::undo_parse_error(&id);
            return false;
        }
    };

    // Reconcile in memory first. If it proves anything, the proof must be
    // durable before we rely on it; a failed persist means we fall back to
    // the on-disk states and simply don't undo the newly-proven actions.
    let mut reconciled = original.clone();
    let changed = crate::recovery::reconcile_item(&mut reconciled);
    let hist = if changed {
        match record_journal(&reconciled, JournalStage::Recover) {
            Ok(()) => reconciled,
            Err(e) => {
                crate::render::undo_reconcile_persist_error(&id, &e);
                original
            }
        }
    } else {
        reconciled
    };

    let mut undo_outcomes: Vec<ActionResult> = Vec::new();
    let mut unproven = 0usize;
    let mut trash_skipped = 0usize;
    for i in 0..hist.actions.len() {
        let act = &hist.actions[i];
        if act.op == Op::Trash {
            trash_skipped += 1;
            continue;
        }
        match hist.state_of(i) {
            ActionState::Succeeded => {
                if let Some(result) = undo_one(act, true) {
                    undo_outcomes.push(result);
                }
            }
            // Pending / Prepared / Ambiguous / Failed are unproven: undo
            // must never touch them, and must say so rather than silently
            // doing nothing.
            _ => {
                if matches!(act.op, Op::Move | Op::MoveDir) {
                    unproven += 1;
                }
            }
        }
    }
    let restored = undo_outcomes.iter().filter(|o| o.result.is_ok()).count();
    let refused: Vec<ActionResult> = undo_outcomes
        .iter()
        .filter(|o| o.result.is_err())
        .cloned()
        .collect();
    let mut ok = refused.is_empty();
    crate::render::undo_result(&id, restored, &refused, trash_skipped, unproven);

    if !undo_outcomes.is_empty() {
        let undo_actions = undo_outcomes
            .iter()
            .map(|o| Action {
                src: o.src.clone(),
                dst: o.dst.clone(),
                op: o.op.clone(),
                reason: Some(format!("undo of {}", hist.id)),
                undoable: false,
            })
            .collect();
        let undo_states = undo_outcomes
            .iter()
            .map(|o| {
                if o.result.is_ok() {
                    ActionState::Succeeded
                } else {
                    ActionState::Failed
                }
            })
            .collect();
        let undo_item = HistoryItem {
            id: new_history_id(),
            actions: undo_actions,
            timestamp: now_secs(),
            outcomes: undo_outcomes,
            kind: "undo".to_string(),
            origin: "manual".to_string(),
            watch_root: None,
            execution_state: ExecutionState::Completed,
            action_states: undo_states,
            prepared_probe: None,
            journal_version: JOURNAL_VERSION,
        };
        if let Err(e) = record_journal(&undo_item, JournalStage::UndoRecord) {
            // The undo itself already happened on disk; its audit record is
            // what failed. Surface it as a real error (visible output +
            // failure exit code) rather than a bare stderr line.
            crate::render::undo_record_error(&e);
            ok = false;
        }
    }
    ok
}

/// Test-only: the exact temp path `record_history` writes to before its
/// atomic rename. Lets a test sabotage the temp write (e.g. by placing a
/// directory there) to prove a failed update never corrupts the last good
/// journal.
#[doc(hidden)]
pub fn history_temp_path(id: &str) -> PathBuf {
    get_history_dir().join(temp_name(id))
}

fn temp_name(id: &str) -> String {
    // PID-scoped so a stale temp left by a crashed process can never be
    // mistaken for (or clobber) the in-progress write of a new one.
    format!(".{}.{}.tmp", id, std::process::id())
}

/// Persist a history item atomically. Writes a sibling temp file, flushes
/// it to stable storage, then `rename`s it over the destination — so a
/// failed or partial write can never corrupt the previous valid journal.
pub fn record_history(item: &HistoryItem) -> Result<(), String> {
    write_journal_atomic(item)
}

/// Like [`record_history`], but tagged with the lifecycle [`JournalStage`]
/// so tests can inject a deterministic failure at exactly that stage.
pub fn record_journal(item: &HistoryItem, stage: JournalStage) -> Result<(), String> {
    if journal_fault_triggered(stage) {
        return Err(format!("injected journal write failure at {stage:?}"));
    }
    write_journal_atomic(item)
}

fn write_journal_atomic(item: &HistoryItem) -> Result<(), String> {
    let hist_dir = get_history_dir();
    fs::create_dir_all(&hist_dir).map_err(|e| format!("error creating history dir: {}", e))?;
    let fp = hist_dir.join(format!("{}.json", item.id));
    let tmp = hist_dir.join(temp_name(&item.id));
    let s = serde_json::to_string_pretty(item).map_err(|e| format!("serialize error: {}", e))?;

    // Write + flush the temp file *before* the rename. `sync_all` is what
    // turns "the OS has the bytes" into "stable storage has the bytes";
    // the rename itself is then the atomic commit point. Any failure here
    // removes the partial temp so a later writer never trips on it.
    if let Err(e) = write_temp_file(&tmp, &s) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = fs::rename(&tmp, &fp) {
        // Leave no half-written temp behind for the next writer to trip on.
        let _ = fs::remove_file(&tmp);
        return Err(format!("rename error: {}", e));
    }
    // Best-effort directory fsync so the rename itself survives power loss
    // on filesystems that support it. Absent/failed on some platforms and
    // filesystems; that only weakens the power-loss guarantee, never the
    // process-crash guarantee. It cannot be made fail-closed portably:
    // Windows cannot open a directory as a file at all, and after the
    // rename the commit point has already passed, so returning `Err` here
    // would misreport a journal that is actually in place.
    if let Ok(dir) = fs::File::open(&hist_dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn write_temp_file(tmp: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    let mut f = fs::File::create(tmp).map_err(|e| format!("write error: {}", e))?;
    f.write_all(contents.as_bytes())
        .map_err(|e| format!("write error: {}", e))?;
    f.sync_all().map_err(|e| format!("fsync error: {}", e))?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn new_history_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("hist-{}", ts)
}

/// Set test‑only history directory override (current thread only).
pub fn set_test_history_dir(path: PathBuf) {
    TEST_HISTORY_DIR.with(|cell| *cell.borrow_mut() = Some(path));
}

/// Clear test‑only history directory override (current thread only).
pub fn clear_test_history_dir() {
    TEST_HISTORY_DIR.with(|cell| *cell.borrow_mut() = None);
}

fn get_history_dir() -> PathBuf {
    if let Some(path) = TEST_HISTORY_DIR.with(|cell| cell.borrow().clone()) {
        return path;
    }
    if let Some(proj) = ProjectDirs::from("org", "flokin", "Sift") {
        return proj.data_dir().join(".sift-history");
    }
    // Extremely unlikely fallback: no home directory could be resolved.
    PathBuf::from(".sift-history")
}
