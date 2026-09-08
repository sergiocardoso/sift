use crate::domain::{ActionResult, HistoryItem, Op, Plan};
use crate::fs::{safe_create_dir, safe_rename, send_to_trash};
use crate::history::{new_history_id, record_history};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Executes the plan and returns the history record id together with the
/// actual per-action outcomes, so callers can report what really happened
/// (and point at `sift history`/`sift undo`) rather than assuming success.
/// `kind` ("organize", "clean", ...) is stored on the history record purely
/// for display in `sift history`; it has no effect on execution.
/// `watch_root`: `None` for a manually (CLI) invoked plan; `Some(root)` for
/// a plan the watch daemon executed automatically for that watch root —
/// recorded as `origin`/`watch_root` on the history item, display-only.
pub fn execute_plan(
    plan: Plan,
    _workdir: &str,
    kind: &str,
    watch_root: Option<&Path>,
) -> (String, Vec<ActionResult>) {
    let mut results = vec![];
    for action in &plan.actions {
        let op_result = match action.op {
            Op::CreateDir => match safe_create_dir(&action.src) {
                Ok(()) => Ok(()),
                Err(e) => Err(format!("CreateDir failed: {}", e)),
            },
            Op::Move => {
                if let Some(ref dst) = action.dst {
                    match safe_rename(&action.src, dst) {
                        Ok(()) => Ok(()),
                        Err(e) => Err(format!("Move failed: {}", e)),
                    }
                } else {
                    Err("Missing destination".to_string())
                }
            }
            Op::Trash => match send_to_trash(&action.src) {
                Ok(()) => Ok(()),
                Err(e) => Err(format!("Trash failed: {}", e)),
            },
            Op::Skip => Ok(()),
        };
        results.push(ActionResult {
            src: action.src.clone(),
            dst: action.dst.clone(),
            op: action.op.clone(),
            result: op_result.clone(),
            undoable: action.undoable && op_result.is_ok() && matches!(action.op, Op::Move),
        });
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let id = new_history_id();
    let hist = HistoryItem {
        id: id.clone(),
        actions: plan.actions,
        timestamp: ts,
        outcomes: results.clone(),
        kind: kind.to_string(),
        origin: if watch_root.is_some() {
            "watch"
        } else {
            "manual"
        }
        .to_string(),
        watch_root: watch_root.map(|p| p.to_path_buf()),
    };
    if let Err(e) = record_history(&hist) {
        eprintln!("history: failed to record execution: {}", e);
    }
    (id, results)
}
