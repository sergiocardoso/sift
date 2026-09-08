//! Persistent watch configuration: which folders are watched, their state,
//! and their stats. Shared by the CLI process and the daemon process, so
//! every read-modify-write goes through an OS file lock (`with_registry`)
//! rather than any in-process synchronization — this is the only design
//! that's safe across two separate processes, and it avoids `static mut`
//! entirely.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

thread_local! {
    static TEST_WATCH_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Set a test-only watch data directory override (current thread only).
/// Never used in production; keeps tests from ever touching the user's
/// real Sift data directory.
pub fn set_test_watch_dir(path: PathBuf) {
    TEST_WATCH_DIR.with(|c| *c.borrow_mut() = Some(path));
}

pub fn clear_test_watch_dir() {
    TEST_WATCH_DIR.with(|c| *c.borrow_mut() = None);
}

pub fn watch_dir() -> PathBuf {
    if let Some(p) = TEST_WATCH_DIR.with(|c| c.borrow().clone()) {
        return p;
    }
    if let Some(proj) = directories::ProjectDirs::from("org", "flokin", "Sift") {
        return proj.data_dir().join("watch");
    }
    // Extremely unlikely fallback: no home directory could be resolved.
    PathBuf::from(".sift-watch")
}

fn registry_path() -> PathBuf {
    watch_dir().join("registry.json")
}
fn registry_lock_path() -> PathBuf {
    watch_dir().join("registry.lock")
}
pub fn daemon_lock_path() -> PathBuf {
    watch_dir().join("daemon.lock")
}
pub fn daemon_info_path() -> PathBuf {
    watch_dir().join("daemon.info")
}
pub fn daemon_stop_path() -> PathBuf {
    watch_dir().join("daemon.stop")
}
pub fn daemon_log_path() -> PathBuf {
    watch_dir().join("daemon.log")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchState {
    Running,
    Paused,
    Stopped,
}

impl WatchState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEntry {
    pub path: PathBuf,
    pub state: WatchState,
    /// Always `true` for a registered entry: the presence of an entry at
    /// all already required `--auto-apply` at `add` time. Kept as an
    /// explicit field (rather than implied) so the registry record is a
    /// self-contained, auditable statement of the authorization granted.
    pub auto_apply: bool,
    pub recursive: bool,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub last_event_at: Option<u64>,
    #[serde(default)]
    pub last_success_at: Option<u64>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub organized_count: u64,
    #[serde(default)]
    pub error_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub watches: Vec<WatchEntry>,
}

impl Registry {
    pub fn find(&self, path: &Path) -> Option<&WatchEntry> {
        self.watches.iter().find(|w| w.path == path)
    }
    pub fn find_mut(&mut self, path: &Path) -> Option<&mut WatchEntry> {
        self.watches.iter_mut().find(|w| w.path == path)
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_registry_unlocked(path: &Path) -> Registry {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_registry_unlocked(path: &Path, reg: &Registry) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let s = serde_json::to_string_pretty(reg).map_err(|e| format!("serialize error: {e}"))?;
    fs::write(&tmp, s).map_err(|e| format!("write error: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename error: {e}"))
}

/// Runs `f` with exclusive access to the on-disk registry: acquires an OS
/// file lock (safe across the CLI and daemon processes, and across
/// threads within either), loads the current registry, lets `f`
/// inspect/mutate it, then atomically saves it back (temp-file + rename)
/// before releasing the lock. No `static mut` anywhere — the lock file is
/// the single source of truth for "who may write right now".
pub fn with_registry<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut Registry) -> R,
{
    let dir = watch_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create watch dir: {e}"))?;
    let lock_file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(registry_lock_path())
        .map_err(|e| format!("cannot open registry lock: {e}"))?;
    lock_file
        .lock()
        .map_err(|e| format!("cannot lock registry: {e}"))?;
    let path = registry_path();
    let mut registry = load_registry_unlocked(&path);
    let result = f(&mut registry);
    save_registry_unlocked(&path, &registry)?;
    let _ = lock_file.unlock();
    Ok(result)
}

pub fn list() -> Result<Vec<WatchEntry>, String> {
    with_registry(|reg| {
        let mut v = reg.watches.clone();
        v.sort_by(|a, b| a.path.cmp(&b.path));
        v
    })
}

pub fn find(canonical: &Path) -> Result<Option<WatchEntry>, String> {
    with_registry(|reg| reg.find(canonical).cloned())
}

/// Registers a new watch, in `Stopped` state — `add` never starts
/// monitoring by itself. Requires `auto_apply` to be explicitly `true`;
/// this is the persistent authorization the user is granting for future
/// automatic mutation, and there is no implicit way to obtain it.
pub fn add(canonical: PathBuf, auto_apply: bool, recursive: bool) -> Result<WatchEntry, String> {
    if !auto_apply {
        return Err(
            "Automatic watch organization requires explicit --auto-apply authorization."
                .to_string(),
        );
    }
    with_registry(|reg| -> Result<WatchEntry, String> {
        if reg.find(&canonical).is_some() {
            return Err(format!(
                "{} is already a registered watch",
                canonical.display()
            ));
        }
        let now = now_secs();
        let entry = WatchEntry {
            path: canonical,
            state: WatchState::Stopped,
            auto_apply: true,
            recursive,
            created_at: now,
            updated_at: now,
            last_event_at: None,
            last_success_at: None,
            last_error: None,
            organized_count: 0,
            error_count: 0,
        };
        reg.watches.push(entry.clone());
        Ok(entry)
    })?
}

/// Removes a watch's registry entry entirely. The daemon notices the
/// entry is gone on its next registry poll and tears down that root's
/// live watcher and any pending unstabilized candidates for it — the
/// "safely stop first" behavior happens there, not as a separate step
/// here, so there is no window where the entry exists in two
/// inconsistent states.
pub fn remove(canonical: &Path) -> Result<(), String> {
    with_registry(|reg| -> Result<(), String> {
        let before = reg.watches.len();
        reg.watches.retain(|w| w.path != canonical);
        if reg.watches.len() == before {
            Err(format!("{} is not a registered watch", canonical.display()))
        } else {
            Ok(())
        }
    })?
}

fn allowed_transition(from: WatchState, to: WatchState) -> bool {
    matches!(
        (from, to),
        (WatchState::Stopped, WatchState::Running)
            | (WatchState::Running, WatchState::Paused)
            | (WatchState::Paused, WatchState::Running)
            | (WatchState::Running, WatchState::Stopped)
            | (WatchState::Paused, WatchState::Stopped)
    )
}

pub fn transition(canonical: &Path, to: WatchState) -> Result<WatchEntry, String> {
    with_registry(|reg| -> Result<WatchEntry, String> {
        let entry = reg
            .find_mut(canonical)
            .ok_or_else(|| format!("{} is not a registered watch", canonical.display()))?;
        if !allowed_transition(entry.state, to) {
            return Err(format!(
                "cannot go from {} to {}",
                entry.state.label(),
                to.label()
            ));
        }
        entry.state = to;
        entry.updated_at = now_secs();
        Ok(entry.clone())
    })?
}

pub fn record_event(canonical: &Path) -> Result<(), String> {
    with_registry(|reg| {
        if let Some(e) = reg.find_mut(canonical) {
            let now = now_secs();
            e.last_event_at = Some(now);
            e.updated_at = now;
        }
    })
}

pub fn record_success(canonical: &Path, moved: u64) -> Result<(), String> {
    with_registry(|reg| {
        if let Some(e) = reg.find_mut(canonical) {
            let now = now_secs();
            e.organized_count += moved;
            e.last_success_at = Some(now);
            e.updated_at = now;
        }
    })
}

pub fn record_error(canonical: &Path, message: String) -> Result<(), String> {
    with_registry(|reg| {
        if let Some(e) = reg.find_mut(canonical) {
            let now = now_secs();
            e.error_count += 1;
            e.last_error = Some(message);
            e.updated_at = now;
        }
    })
}

/// Validates a candidate watch root using no-follow metadata on the
/// supplied path *before* any canonicalization (canonicalize would
/// silently follow a symlink), then checks it's not the filesystem root,
/// not one of Sift's own category directories, and not a recognized
/// software project root.
pub fn validate_root(path: &Path) -> Result<PathBuf, String> {
    let md = fs::symlink_metadata(path).map_err(|_| "the path does not exist".to_string())?;
    if md.file_type().is_symlink() {
        return Err("the watch root itself must not be a symlink".to_string());
    }
    if !md.is_dir() {
        return Err("the watch root must be a directory".to_string());
    }
    let canonical = fs::canonicalize(path).map_err(|e| format!("cannot resolve path: {e}"))?;
    if canonical.parent().is_none() {
        return Err("refusing to watch the filesystem root".to_string());
    }
    let name = canonical.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if crate::scanner::is_sift_category_dir(name) {
        return Err(format!(
            "'{name}' is one of Sift's own category directories and cannot be watched"
        ));
    }
    if crate::scanner::is_project_root(&canonical) {
        return Err(
            "the watch root is a recognized software project root and cannot be watched"
                .to_string(),
        );
    }
    Ok(canonical)
}
