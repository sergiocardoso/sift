use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: Option<u64>,
    pub mtime: Option<u64>,
    pub project_root: bool,
    pub protected: bool,
    pub classified_as: Option<Category>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Category {
    Document,
    Image,
    Video,
    Audio,
    Archive,
    ThreeD,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
    pub op: Op,
    pub result: Result<(), String>,
    pub undoable: bool,
}
