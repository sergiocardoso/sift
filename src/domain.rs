use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Journal schema version written by the current executor. Legacy files
/// (written before the write-ahead journal existed) have no such field and
/// deserialize as `0`.
pub const JOURNAL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub hidden: bool,
    pub size: Option<u64>,
    pub mtime: Option<u64>,
    pub project_root: bool,
    pub protected: bool,
    pub classified_as: Option<Category>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Category {
    Document,
    Image,
    Video,
    Audio,
    Archive,
    ThreeD,
    /// Source code and scripts (js, py, rs, sh, ...).
    Code,
    /// Structured/tabular data (json, csv, yaml, sql, ...).
    Data,
    /// An ordinary regular file with no more specific category: the
    /// conservative fallback destination, distinct from `Unknown` (which
    /// means "not an ordinary classifiable file at all", e.g. a directory).
    Other,
    Junk,
    BuildOutput,
    Sensitive,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub src: PathBuf,
    pub dst: Option<PathBuf>, // For Move, None for Trash/Skip
    pub op: Op,
    pub reason: Option<String>,
    pub undoable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Op {
    CreateDir,
    /// Moves a regular file. The executor requires the source to actually
    /// be a plain file — never a directory. See `MoveDir` for moving a
    /// whole directory; the two are never interchangeable.
    Move,
    /// Moves an entire directory intact (used by `sift folders`). The
    /// executor requires the source to actually be a directory — never a
    /// regular file. Kept as its own variant specifically so `Move`'s
    /// regular-file requirement is never loosened to "anything but a
    /// symlink".
    MoveDir,
    Trash,
    Skip,
}

/// Execution-level lifecycle state of a journal.
///
/// Written durably to disk: `Running` while the executor is still working,
/// `Completed` once every action has a terminal outcome, `Interrupted`
/// once reconciliation (or the executor itself) knows the run will never
/// finish normally.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionState {
    Running,
    Completed,
    Interrupted,
}

impl Default for ExecutionState {
    /// History records written before this field existed were only ever
    /// persisted *after* execution finished, so a missing `execution_state`
    /// means the run completed. Legacy records must never default to
    /// `Running`.
    fn default() -> Self {
        ExecutionState::Completed
    }
}

/// Durable per-action state.
///
/// The distinction that matters for safety is between an action that is
/// merely `Pending` (we know it never began), one that is `Prepared` (its
/// intent was durably authorized but no terminal outcome was recorded —
/// the mutation may or may not have happened), and the terminal states
/// `Succeeded`/`Failed`. `Ambiguous` is the explicit "we cannot prove it"
/// state produced by reconciliation for a `Prepared` action; it is never
/// silently collapsed into `Succeeded`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActionState {
    /// The action is part of the plan but execution never authorized it.
    Pending,
    /// Execution durably recorded the intent and was about to mutate, but
    /// no terminal outcome is durably known. May or may not have mutated.
    Prepared,
    /// The mutation happened and its success is durably recorded.
    Succeeded,
    /// The mutation was attempted and its failure is durably recorded.
    Failed,
    /// Reconciliation could not prove whether the prepared mutation
    /// happened. Never undoable, never retried blindly.
    Ambiguous,
}

/// Identity of a source path captured immediately before its mutation.
///
/// `dev`/`ino` (Unix) survive a `rename(2)`, so comparing them against the
/// live destination *proves* the same filesystem object moved. On
/// platforms without those fields the probe is still recorded for audit
/// but is never treated as proof (reconciliation stays conservative).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionProbe {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ino: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<u64>,
    pub is_dir: bool,
    pub is_symlink: bool,
}

/// The single prepared action's source probe, stored on the journal.
///
/// The executor is strictly sequential, so at most one action is ever
/// `Prepared` at a time; a single optional probe is therefore enough, and
/// avoids a parallel array of mostly-`None` values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedProbe {
    /// Index into `actions` of the prepared action this probe describes.
    pub action_index: usize,
    pub probe: ActionProbe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub id: String,
    pub actions: Vec<Action>,
    pub timestamp: u64,
    pub outcomes: Vec<ActionResult>,
    /// What kind of operation this was ("organize", "clean", "undo"), for
    /// display purposes only. Empty/absent for records written before this
    /// field existed; `#[serde(default)]` keeps old history files loadable.
    #[serde(default)]
    pub kind: String,
    /// Who initiated this operation: "manual" (CLI-invoked, the default)
    /// or "watch" (automatic, from a running watch). Empty for records
    /// written before this field existed; treat empty the same as
    /// "manual". Display-only, like `kind`.
    #[serde(default)]
    pub origin: String,
    /// The watch root responsible, set only when `origin == "watch"`.
    #[serde(default)]
    pub watch_root: Option<PathBuf>,
    /// Execution-level lifecycle state; see [`ExecutionState`]. Absent in
    /// legacy records, where it defaults to `Completed`.
    #[serde(default)]
    pub execution_state: ExecutionState,
    /// Per-action durable state, parallel to `actions`. Empty for legacy
    /// records; derive the state from `outcomes` with [`HistoryItem::state_of`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action_states: Vec<ActionState>,
    /// Source identity for the one currently-`Prepared` action, present
    /// only in an interrupted journal. Used by reconciliation to prove a
    /// move; never to guess.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared_probe: Option<PreparedProbe>,
    /// Journal schema version (`0` = legacy record). Informational; it does
    /// not affect execution or undo.
    #[serde(default)]
    pub journal_version: u32,
}

impl HistoryItem {
    /// Durable state of one action. New records store `action_states`
    /// explicitly; legacy records have none, so their state is derived
    /// from the matching outcome (`Ok` → succeeded, `Err` → failed).
    pub fn state_of(&self, index: usize) -> ActionState {
        if let Some(state) = self.action_states.get(index) {
            return *state;
        }
        match self.outcomes.get(index) {
            Some(o) if o.result.is_ok() => ActionState::Succeeded,
            Some(_) => ActionState::Failed,
            None => ActionState::Pending,
        }
    }

    /// Whether every action has a terminal, durably known outcome.
    pub fn all_actions_terminal(&self) -> bool {
        (0..self.actions.len()).all(|i| {
            matches!(
                self.state_of(i),
                ActionState::Succeeded | ActionState::Failed
            )
        })
    }

    /// Effective execution state for display and reconciliation. A journal
    /// left `Running` whose actions are all terminal is deterministically
    /// completable; otherwise it is reported as interrupted (the process
    /// that wrote it is gone by the time anyone reads it). Legacy records
    /// have no per-action states and are always completed.
    pub fn effective_state(&self) -> ExecutionState {
        let all_terminal = self.all_actions_terminal();
        match self.execution_state {
            // `Running` on disk means the writer is gone by the time anyone
            // reads it; it is completable iff every action is already
            // terminal, otherwise it was interrupted.
            ExecutionState::Running => {
                if all_terminal {
                    ExecutionState::Completed
                } else {
                    ExecutionState::Interrupted
                }
            }
            // A record can only honestly claim `Completed` if every action
            // really is terminal. Anything else is contradictory (e.g. a
            // corrupted or hand-edited file) and must not be presented as a
            // clean completion.
            ExecutionState::Completed => {
                if all_terminal {
                    ExecutionState::Completed
                } else {
                    ExecutionState::Interrupted
                }
            }
            ExecutionState::Interrupted => ExecutionState::Interrupted,
        }
    }

    /// Actions that never reached a terminal outcome (pending, prepared or
    /// ambiguous). Non-zero exactly when an execution was interrupted.
    pub fn unresolved_actions(&self) -> usize {
        (0..self.actions.len())
            .filter(|&i| {
                matches!(
                    self.state_of(i),
                    ActionState::Pending | ActionState::Prepared | ActionState::Ambiguous
                )
            })
            .count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
    pub op: Op,
    pub result: Result<(), String>,
    pub undoable: bool,
}
