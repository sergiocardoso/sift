use crate::domain::*;
use directories::ProjectDirs;
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// A thread-local (not a process-global) override: cargo test runs each test
// function on its own thread, so this keeps concurrently running tests from
// racing over a shared override and momentarily falling back to the real
// global history directory.
thread_local! {
    static TEST_HISTORY_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub fn cmd_history() {
    let hist_dir = get_history_dir();
    let mut items = vec![];
    if let Ok(files) = fs::read_dir(&hist_dir) {
        for f in files.flatten() {
            let fp = f.path();
            if fp.extension().is_some_and(|e| e == "json") {
                if let Ok(s) = fs::read_to_string(&fp) {
                    if let Ok(item) = serde_json::from_str::<HistoryItem>(&s) {
                        items.push(item);
                    }
                }
            }
        }
    }
    items.sort_by_key(|h| h.timestamp);
    for h in items {
        println!("{} {} actions {}", h.id, h.timestamp, h.actions.len());
    }
}

/// Attempts to reverse one successful, undoable Move outcome. Never
/// overwrites anything; revalidates every assumption against the live
/// filesystem via `symlink_metadata` immediately before acting.
fn undo_one(action: &Action, outcome_was_ok: bool) -> Option<ActionResult> {
    if action.op != Op::Move || !action.undoable || !outcome_was_ok {
        return None;
    }
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
    // directory (directories are never within v0.1's move scope, so a
    // directory sitting at the recorded destination means something else
    // replaced the moved file after the fact).
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

/// Undoes the moves recorded in history item `id`. The original history
/// record is left untouched; a separate undo record is written describing
/// what the undo attempt actually did.
pub fn cmd_undo(id: String) {
    let hist_dir = get_history_dir();
    let fp = hist_dir.join(format!("{}.json", id));
    let data = match fs::read_to_string(&fp) {
        Ok(s) => s,
        Err(_) => {
            println!("record not found");
            return;
        }
    };
    let hist: HistoryItem = match serde_json::from_str(&data) {
        Ok(h) => h,
        Err(_) => {
            println!("history parse error");
            return;
        }
    };
    let mut undo_outcomes: Vec<ActionResult> = Vec::new();
    for (act, outcome) in hist.actions.iter().zip(hist.outcomes.iter()) {
        if act.op == Op::Trash {
            println!("Undo for trash not supported: {}", act.src.display());
            continue;
        }
        if let Some(result) = undo_one(act, outcome.result.is_ok()) {
            if let Err(ref e) = result.result {
                println!("{}", e);
            }
            undo_outcomes.push(result);
        }
    }
    if undo_outcomes.is_empty() {
        return;
    }
    let undo_actions = undo_outcomes
        .iter()
        .map(|o| Action {
            src: o.src.clone(),
            dst: o.dst.clone(),
            op: o.op.clone(),
            reason: Some(format!("Undo of {}", hist.id)),
            undoable: false,
        })
        .collect();
    let undo_item = HistoryItem {
        id: new_history_id(),
        actions: undo_actions,
        timestamp: now_secs(),
        outcomes: undo_outcomes,
    };
    if let Err(e) = record_history(&undo_item) {
        eprintln!("history: failed to record undo: {}", e);
    }
}

/// Writes a history item to disk via a temp-file-then-rename, reporting
/// errors instead of panicking.
pub fn record_history(item: &HistoryItem) -> Result<(), String> {
    let hist_dir = get_history_dir();
    fs::create_dir_all(&hist_dir).map_err(|e| format!("error creating history dir: {}", e))?;
    let fp = hist_dir.join(format!("{}.json", item.id));
    let tmp = fp.with_extension("tmp");
    let s = serde_json::to_string_pretty(item).map_err(|e| format!("serialize error: {}", e))?;
    fs::write(&tmp, &s).map_err(|e| format!("write error: {}", e))?;
    fs::rename(&tmp, &fp).map_err(|e| format!("rename error: {}", e))
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
