//! Conservative reconciliation of an interrupted execution journal.
//!
//! A journal can be left with one action `Prepared` when the process died
//! (or the terminal journal write failed) after the intent was recorded but
//! before the outcome was. Recovery here is strictly read-only against the
//! user's files: it inspects the live filesystem, and only *proves* an
//! outcome when the evidence is unambiguous. Everything it cannot prove is
//! marked [`ActionState::Ambiguous`] — never guessed, never retried, never
//! undone. Persisting the result (and thereby making a proven action
//! eligible for undo) is the caller's decision, so a normal read of
//! history never mutates anything.
//!
//! Proof of a completed move relies on the source-identity probe the
//! executor records immediately before mutating (see
//! [`crate::domain::ActionProbe`]). `rename(2)` preserves the inode, so if
//! the destination currently holds the exact `(dev, ino)` the source had at
//! prepare time, the move demonstrably happened. On platforms with no
//! stable file id the probe is not proof, and reconciliation stays
//! conservative (ambiguous).

use crate::domain::{
    Action, ActionProbe, ActionResult, ActionState, ExecutionState, HistoryItem, Op,
};
use std::fs;
use std::time::UNIX_EPOCH;

/// Capture the identity of the source path just before its mutation. Any
/// I/O error yields `None`, which simply means "not provable later"; the
/// mutation itself still proceeds (and its own safety checks still apply).
pub fn probe_action(action: &Action) -> Option<ActionProbe> {
    let md = fs::symlink_metadata(&action.src).ok()?;
    #[cfg(unix)]
    let (dev, ino) = {
        use std::os::unix::fs::MetadataExt;
        (Some(md.dev()), Some(md.ino()))
    };
    #[cfg(not(unix))]
    let (dev, ino) = (None, None);
    Some(ActionProbe {
        dev,
        ino,
        len: md.is_file().then_some(md.len()),
        mtime: md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
        is_dir: md.is_dir(),
        is_symlink: md.file_type().is_symlink(),
    })
}

/// Whether `md` is the same filesystem object the probe captured. Only a
/// stable `(dev, ino)` match counts as proof; the size is checked too as a
/// cheap cross-check. On non-Unix builds this is always `false`.
fn identity_matches(probe: &ActionProbe, md: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Some(dev), Some(ino)) = (probe.dev, probe.ino) {
            return md.dev() == dev && md.ino() == ino && probe.len.is_none_or(|l| md.len() == l);
        }
    }
    let _ = (probe, md);
    false
}

/// Prove a `Prepared` `Move`/`MoveDir` completed: the destination must
/// exist, be the *right kind* of entry (a plain file for `Move`, a real
/// directory for `MoveDir`, never a symlink), not be the source path, and
/// hold the exact object the source had at prepare time. The name-based
/// checks are necessary but not sufficient; the identity match is what
/// makes this proof rather than a guess.
///
/// Crucially, if the source still holds that exact object we must *not*
/// claim success: that is the hardlink case (destination linked to the same
/// inode, source never moved), where a naive "destination has the right
/// inode" check would falsely mark `Succeeded`.
fn prove_completed_move(action: &Action, probe: &ActionProbe) -> bool {
    let Some(dst) = action.dst.as_ref() else {
        return false;
    };
    if dst == &action.src {
        return false;
    }
    // The object must no longer be at the source. If the source exists and
    // still *is* the probed object, the mutation did not move it (e.g. a
    // hardlink was created at the destination) — refuse to prove.
    if let Ok(src_md) = fs::symlink_metadata(&action.src) {
        if identity_matches(probe, &src_md) {
            return false;
        }
    }
    let Ok(md) = fs::symlink_metadata(dst) else {
        return false;
    };
    if md.file_type().is_symlink() {
        return false;
    }
    match action.op {
        Op::Move if md.is_file() => {}
        Op::MoveDir if md.is_dir() => {}
        _ => return false,
    }
    identity_matches(probe, &md)
}

fn terminal_result(action: &Action) -> ActionResult {
    ActionResult {
        src: action.src.clone(),
        dst: action.dst.clone(),
        op: action.op.clone(),
        result: Ok(()),
        undoable: action.undoable && matches!(action.op, Op::Move | Op::MoveDir),
    }
}

/// Reconcile a journal in memory. Returns `true` if anything changed.
///
/// Only actions durably `Prepared` are touched; everything else is left
/// exactly as written. The execution-level state is then resolved: all
/// terminal → `Completed`, otherwise `Interrupted`. This also
/// deterministically finalizes the "every outcome persisted but the final
/// `Completed` write failed" window.
///
/// This function never mutates user files, and never retries a prepared
/// action — it only inspects.
pub fn reconcile_item(item: &mut HistoryItem) -> bool {
    let mut changed = false;
    let n = item.actions.len();

    for i in 0..n {
        if item.action_states.get(i) != Some(&ActionState::Prepared) {
            continue;
        }
        let action = item.actions[i].clone();
        let probe = item
            .prepared_probe
            .as_ref()
            .filter(|p| p.action_index == i)
            .map(|p| p.probe.clone());

        let proven = matches!(action.op, Op::Move | Op::MoveDir)
            && probe
                .as_ref()
                .is_some_and(|p| prove_completed_move(&action, p));

        if proven {
            item.action_states[i] = ActionState::Succeeded;
            if item.outcomes.get(i).is_none() {
                item.outcomes.push(terminal_result(&action));
            }
        } else {
            item.action_states[i] = ActionState::Ambiguous;
        }
        changed = true;
    }

    let resolved = if item.all_actions_terminal() {
        ExecutionState::Completed
    } else {
        ExecutionState::Interrupted
    };
    if item.execution_state != resolved {
        item.execution_state = resolved;
        changed = true;
    }
    if changed {
        // No prepared action remains once reconciliation has run.
        item.prepared_probe = None;
    }
    changed
}
