use crate::domain::{
    Action, ActionResult, ActionState, ExecutionState, HistoryItem, Op, Plan, PreparedProbe,
    JOURNAL_VERSION,
};
use crate::fs::{safe_create_dir, safe_rename, safe_rename_dir, send_to_trash};
use crate::history::{new_history_id, record_journal, JournalStage};
use crate::recovery;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// A completed execution: every action reached a terminal, durably-recorded
/// outcome.
#[derive(Debug)]
pub struct ExecutionReport {
    pub history_id: String,
    pub outcomes: Vec<ActionResult>,
    pub execution_state: ExecutionState,
}

/// How far execution got before it could not continue. Callers must be able
/// to tell these apart without parsing a message string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionFailure {
    /// The initial journal write failed: nothing was persisted and no
    /// filesystem mutation was attempted. Zero changes were made.
    NotStarted,
    /// The journal was persisted, so execution began, but it could not be
    /// carried to completion. `history_id` is always `Some` here.
    Interrupted,
}

/// A non-recoverable execution failure. The journal may still be on disk
/// and must be inspectable; `outcomes` lists only outcomes that are
/// durably recorded (never one whose write failed).
#[derive(Debug)]
pub struct ExecutionError {
    pub failure: ExecutionFailure,
    /// Present whenever the initial journal was persisted, so the user can
    /// run `sift history` and inspect (or reconcile) the interrupted run.
    pub history_id: Option<String>,
    /// Outcomes that are durably recorded.
    pub outcomes: Vec<ActionResult>,
    /// True iff a mutation may have occurred whose terminal outcome is not
    /// durably recorded (the mutation-succeeded / outcome-write-failed
    /// window). This is the flag that must never be silently dropped.
    pub may_have_mutated: bool,
    pub message: String,
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Performs one action's filesystem mutation. Read-only re-verification
/// (e.g. Trash's duplicate recheck) lives here too; it is only ever called
/// after the action's intent has been durably recorded.
fn run_action(action: &Action) -> Result<(), String> {
    match action.op {
        Op::CreateDir => safe_create_dir(&action.src)
            .map(|_| ())
            .map_err(|e| format!("CreateDir failed: {}", e)),
        Op::Move => {
            if let Some(ref dst) = action.dst {
                safe_rename(&action.src, dst).map_err(|e| format!("Move failed: {}", e))
            } else {
                Err("Missing destination".to_string())
            }
        }
        Op::MoveDir => {
            if let Some(ref dst) = action.dst {
                safe_rename_dir(&action.src, dst).map_err(|e| format!("MoveDir failed: {}", e))
            } else {
                Err("Missing destination".to_string())
            }
        }
        // A duplicate-collision `Trash` (see
        // `planner::finalize_move_action`) carries the existing file it was
        // found identical to on `dst` — re-verify that's *still* true right
        // before trashing, since planning and execution can be moments (or,
        // for Watch, much longer) apart and the other file could have
        // changed or vanished in between. An ordinary junk-file `Trash` has
        // no `dst` and is never held to this extra check.
        Op::Trash => match &action.dst {
            Some(existing) => {
                match crate::fs::files_have_identical_content(&action.src, existing) {
                    Ok(true) => {
                        send_to_trash(&action.src).map_err(|e| format!("Trash failed: {}", e))
                    }
                    Ok(false) => Err(format!(
                        "refusing to trash: {} is no longer identical to {}",
                        action.src.display(),
                        existing.display()
                    )),
                    Err(e) => Err(format!("cannot re-verify duplicate before trashing: {e}")),
                }
            }
            None => send_to_trash(&action.src).map_err(|e| format!("Trash failed: {}", e)),
        },
        Op::Skip => Ok(()),
    }
}

fn outcome_for(action: &Action, op_result: Result<(), String>) -> ActionResult {
    ActionResult {
        src: action.src.clone(),
        dst: action.dst.clone(),
        op: action.op.clone(),
        undoable: action.undoable
            && op_result.is_ok()
            && matches!(action.op, Op::Move | Op::MoveDir),
        result: op_result,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Executes a plan under a write-ahead journal.
///
/// The safety invariant: **no mutating action begins until its intent has
/// been durably recorded.** Concretely:
///
/// 1. the whole plan is written once with `execution_state = running` and
///    every action `pending` — *before any mutation*;
/// 2. for each action, its state is flipped to `prepared` and flushed;
/// 3. only then does the filesystem mutation run;
/// 4. the terminal `succeeded`/`failed` outcome is flushed;
/// 5. once all actions are terminal, `execution_state = completed` is
///    flushed.
///
/// If step 1 fails, zero mutations occur and the caller gets
/// [`ExecutionFailure::NotStarted`]. If any later journal write fails,
/// mutation stops immediately and the caller gets
/// [`ExecutionFailure::Interrupted`] with the persisted `history_id`; the
/// journal on disk remains a truthful record of how far execution got.
///
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
) -> Result<ExecutionReport, ExecutionError> {
    let id = new_history_id();
    let n = plan.actions.len();
    let mut item = HistoryItem {
        id: id.clone(),
        actions: plan.actions,
        timestamp: now_secs(),
        outcomes: Vec::new(),
        kind: kind.to_string(),
        origin: if watch_root.is_some() {
            "watch"
        } else {
            "manual"
        }
        .to_string(),
        watch_root: watch_root.map(|p| p.to_path_buf()),
        execution_state: ExecutionState::Running,
        action_states: vec![ActionState::Pending; n],
        prepared_probe: None,
        journal_version: JOURNAL_VERSION,
    };

    // ---- 1. Write-ahead: persist the complete intent before ANY mutation.
    if let Err(e) = record_journal(&item, JournalStage::Initial) {
        return Err(ExecutionError {
            failure: ExecutionFailure::NotStarted,
            history_id: None,
            outcomes: Vec::new(),
            may_have_mutated: false,
            message: format!(
                "cannot start execution: the history journal could not be written ({e})"
            ),
        });
    }

    for i in 0..n {
        let action = item.actions[i].clone();

        // Skip is a no-op: it has no mutation to guard, so it needs no
        // `prepared` step — its terminal outcome is recorded directly.
        if action.op == Op::Skip {
            item.action_states[i] = ActionState::Succeeded;
            item.outcomes.push(outcome_for(&action, Ok(())));
            if let Err(e) = record_journal(&item, JournalStage::Outcome) {
                return Err(ExecutionError {
                    failure: ExecutionFailure::Interrupted,
                    history_id: Some(id.clone()),
                    outcomes: item.outcomes.clone(),
                    may_have_mutated: false,
                    message: format!("execution interrupted while recording a no-op outcome ({e})"),
                });
            }
            continue;
        }

        // ---- 2. Write-ahead: persist this action's intent before mutating.
        let probe = if matches!(action.op, Op::Move | Op::MoveDir) {
            recovery::probe_action(&action)
        } else {
            None
        };
        let prepared = probe.map(|probe| PreparedProbe {
            action_index: i,
            probe,
        });
        item.action_states[i] = ActionState::Prepared;
        item.prepared_probe = prepared.clone();
        if let Err(e) = record_journal(&item, JournalStage::Prepare) {
            // Nothing was mutated; the durable journal still says `pending`
            // (we never got the `prepared` write). Revert our in-memory
            // view to match and stop.
            item.action_states[i] = ActionState::Pending;
            item.prepared_probe = None;
            return Err(ExecutionError {
                failure: ExecutionFailure::Interrupted,
                history_id: Some(id.clone()),
                outcomes: item.outcomes.clone(),
                may_have_mutated: false,
                message: format!(
                    "execution interrupted before changing {}: its intent could not be recorded ({e})",
                    action.src.display()
                ),
            });
        }

        // ---- 3. The mutation itself, only now that intent is durable.
        let op_result = run_action(&action);

        // ---- 4. Persist the terminal outcome.
        item.action_states[i] = if op_result.is_ok() {
            ActionState::Succeeded
        } else {
            ActionState::Failed
        };
        item.prepared_probe = None;
        item.outcomes.push(outcome_for(&action, op_result.clone()));
        if let Err(e) = record_journal(&item, JournalStage::Outcome) {
            // The hardest window: the mutation may have happened, but its
            // outcome is not durable. The durable journal still says
            // `prepared`. Report only what *is* durable, flag the possible
            // mutation, and stop — never continue to another action.
            let mutated = op_result.is_ok();
            item.outcomes.pop();
            item.action_states[i] = ActionState::Prepared;
            item.prepared_probe = prepared;
            return Err(ExecutionError {
                failure: ExecutionFailure::Interrupted,
                history_id: Some(id.clone()),
                outcomes: item.outcomes.clone(),
                may_have_mutated: mutated,
                message: format!(
                    "execution interrupted after {}: its outcome could not be recorded ({e})",
                    action.src.display()
                ),
            });
        }
    }

    // ---- 5. Finalize: every action now has a durable terminal outcome.
    item.execution_state = ExecutionState::Completed;
    item.prepared_probe = None;
    if let Err(e) = record_journal(&item, JournalStage::Finalize) {
        // All outcomes *are* durable, so nothing is lost: a journal left
        // `running` with all-terminal actions is deterministically
        // completable by reconciliation. Surface it as an interruption so
        // the user can inspect it, but no mutation is unaccounted for.
        return Err(ExecutionError {
            failure: ExecutionFailure::Interrupted,
            history_id: Some(id.clone()),
            outcomes: item.outcomes.clone(),
            may_have_mutated: false,
            message: format!(
                "all actions were recorded, but the completion record could not be persisted ({e})"
            ),
        });
    }

    Ok(ExecutionReport {
        history_id: id,
        outcomes: item.outcomes,
        execution_state: ExecutionState::Completed,
    })
}
