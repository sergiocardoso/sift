use crate::domain::{ActionResult, HistoryItem, Op, Plan};
use crate::fs::{safe_rename, send_to_trash};
use crate::history::{new_history_id, record_history};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn execute_plan(plan: Plan, _workdir: &str) {
    let mut results = vec![];
    for action in &plan.actions {
        let op_result = match action.op {
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
        .unwrap()
        .as_secs();
    let hist = HistoryItem {
        id: new_history_id(),
        actions: plan.actions,
        timestamp: ts,
        outcomes: results,
    };
    record_history(&hist);
}
