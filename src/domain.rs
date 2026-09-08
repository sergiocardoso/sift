use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
    Move,
    Trash,
    Skip,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
    pub op: Op,
    pub result: Result<(), String>,
    pub undoable: bool,
}
