//! The single background watch daemon: one OS-level singleton lock, one
//! `notify` watcher, one poll loop that reconciles the registry, drains
//! filesystem events into per-root candidate trackers, and hands
//! stabilized candidates to `engine::process_candidate`. Never mutates
//! directly from a `notify` callback — events only ever land in a
//! `RootMonitor`; mutation happens later, from the poll loop, after
//! stability and live revalidation.

use super::engine::{process_candidate, RootMonitor};
use super::registry::{self, WatchState};
use super::stability::DEFAULT_STABILITY_WINDOW;
use crate::classifier::CategoryDB;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(400);
const EVENT_DRAIN_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonInfo {
    pid: u32,
    started_at: u64,
}

pub enum DaemonStatus {
    Running {
        pid: Option<u32>,
        started_at: Option<u64>,
    },
    NotRunning,
}

fn open_lock_file() -> Result<fs::File, String> {
    fs::create_dir_all(registry::watch_dir())
        .map_err(|e| format!("cannot create watch dir: {e}"))?;
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(registry::daemon_lock_path())
        .map_err(|e| format!("cannot open daemon lock: {e}"))
}

/// Distinguishes "running" from "not running" using the held OS file lock
/// itself as ground truth, not just a PID file (which can go stale if a
/// process is killed without cleanup). Briefly acquires the lock
/// non-blocking to test it, then releases it immediately if acquired.
pub fn daemon_status() -> DaemonStatus {
    let file = match open_lock_file() {
        Ok(f) => f,
        Err(_) => return DaemonStatus::NotRunning,
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            DaemonStatus::NotRunning
        }
        Err(std::fs::TryLockError::WouldBlock) => {
            let info = fs::read_to_string(registry::daemon_info_path())
                .ok()
                .and_then(|s| serde_json::from_str::<DaemonInfo>(&s).ok());
            DaemonStatus::Running {
                pid: info.as_ref().map(|i| i.pid),
                started_at: info.as_ref().map(|i| i.started_at),
            }
        }
        Err(_) => DaemonStatus::NotRunning,
    }
}

/// Requests the running daemon to shut down: writes a stop-flag file the
/// daemon's own poll loop checks each tick. Not an OS signal — this fits
/// the same polling design already needed for registry reconciliation, and
/// avoids sharing the correctness of shutdown with async-signal-safety
/// concerns. Waits up to a few seconds for the daemon to actually exit.
pub fn request_stop_and_wait(timeout: Duration) -> Result<bool, String> {
    if matches!(daemon_status(), DaemonStatus::NotRunning) {
        return Ok(true);
    }
    fs::create_dir_all(registry::watch_dir()).map_err(|e| format!("{e}"))?;
    fs::write(
        registry::daemon_stop_path(),
        registry::now_secs().to_string(),
    )
    .map_err(|e| format!("cannot write stop flag: {e}"))?;
    let start = Instant::now();
    while start.elapsed() < timeout {
        if matches!(daemon_status(), DaemonStatus::NotRunning) {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(matches!(daemon_status(), DaemonStatus::NotRunning))
}

/// Owns the live daemon state: the notify watcher/channel, and one
/// `RootMonitor` per currently-`running` watch. Exposed as a struct (not a
/// bare `run()` loop) specifically so tests can drive `reconcile` /
/// `drain_events` / `process_ready` deterministically without needing a
/// real detached background process.
pub struct Daemon {
    watcher: RecommendedWatcher,
    rx: Receiver<notify::Event>,
    monitors: HashMap<PathBuf, RootMonitor>,
    watched_paths: HashSet<PathBuf>,
}

impl Daemon {
    pub fn new() -> Result<Self, String> {
        let (tx, rx) = channel();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        })
        .map_err(|e| format!("cannot create filesystem watcher: {e}"))?;
        Ok(Self {
            watcher,
            rx,
            monitors: HashMap::new(),
            watched_paths: HashSet::new(),
        })
    }

    /// Whether `root` currently has a live `RootMonitor` (i.e. is actually
    /// being watched right now, as of the last `reconcile`).
    pub fn is_monitoring(&self, root: &Path) -> bool {
        self.monitors.contains_key(root)
    }

    /// Syncs live state (which roots have an active `notify` watch and a
    /// `RootMonitor`) with the registry's current `running` set. A watch
    /// that becomes paused/stopped/removed has its OS watch torn down and
    /// its pending (not-yet-stable) candidates discarded here — this *is*
    /// the "safely stop" behavior; there is no separate step.
    pub fn reconcile(&mut self) {
        let entries = registry::list().unwrap_or_default();
        let running: HashMap<PathBuf, bool> = entries
            .iter()
            .filter(|e| e.state == WatchState::Running)
            .map(|e| (e.path.clone(), e.recursive))
            .collect();

        let to_drop: Vec<PathBuf> = self
            .monitors
            .keys()
            .filter(|p| !running.contains_key(*p))
            .cloned()
            .collect();
        for p in to_drop {
            self.monitors.remove(&p);
        }
        let to_unwatch: Vec<PathBuf> = self
            .watched_paths
            .iter()
            .filter(|p| !running.contains_key(*p))
            .cloned()
            .collect();
        for p in to_unwatch {
            let _ = self.watcher.unwatch(&p);
            self.watched_paths.remove(&p);
        }

        for (path, recursive) in &running {
            self.monitors
                .entry(path.clone())
                .or_insert_with(|| RootMonitor::new(path.clone(), *recursive));
            if !self.watched_paths.contains(path) {
                let mode = if *recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                };
                if self.watcher.watch(path, mode).is_ok() {
                    self.watched_paths.insert(path.clone());
                }
            }
        }
    }

    /// Drains whatever `notify` events have arrived within `timeout` into
    /// the matching root's `RootMonitor`. Never plans or executes anything
    /// here — only records that a path might be a candidate.
    pub fn drain_events(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self.rx.recv_timeout(remaining) {
                Ok(event) => {
                    let now = Instant::now();
                    for path in event.paths {
                        if let Some((root, monitor)) = self
                            .monitors
                            .iter_mut()
                            .find(|(root, _)| path.starts_with(root.as_path()))
                        {
                            let _ = registry::record_event(root);
                            monitor.observe_event(&path, now);
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    /// Polls every root's stability tracker and processes whatever has
    /// become stable: revalidate, plan (via the shared planner helper),
    /// execute, then update that watch's registry stats.
    pub fn process_ready(&mut self) {
        let now = Instant::now();
        for (root, monitor) in self.monitors.iter_mut() {
            let ready = monitor.poll_ready(now, DEFAULT_STABILITY_WINDOW);
            if ready.is_empty() {
                continue;
            }
            let cfg_path = crate::config::find_config(&root.to_string_lossy());
            let config = cfg_path
                .and_then(|p| crate::config::load_config(&p).ok())
                .unwrap_or_default();
            let builtin = CategoryDB::default();
            for path in ready {
                let outcome = process_candidate(root, &config.rules, &builtin, &path);
                if outcome.organized {
                    let _ = registry::record_success(root, 1);
                } else if let Some(err) = &outcome.failure {
                    let _ = registry::record_error(root, err.clone());
                }
            }
        }
    }

    fn stop_requested(&self) -> bool {
        registry::daemon_stop_path().exists()
    }

    fn shutdown(mut self) {
        for p in self.watched_paths.clone() {
            let _ = self.watcher.unwatch(&p);
        }
        let _ = fs::remove_file(registry::daemon_stop_path());
        let _ = fs::remove_file(registry::daemon_info_path());
    }

    /// One full iteration: reconcile, drain, process. Exposed separately
    /// from `run_forever` for deterministic tests.
    pub fn tick(&mut self, drain_timeout: Duration) {
        self.reconcile();
        self.drain_events(drain_timeout);
        self.process_ready();
    }

    /// Runs until a stop is requested (via `request_stop_and_wait`) or an
    /// unrecoverable error occurs. Holds the singleton lock for its entire
    /// lifetime; releases it only on the way out.
    pub fn run_forever(mut self) -> Result<(), String> {
        loop {
            if self.stop_requested() {
                break;
            }
            self.tick(EVENT_DRAIN_TIMEOUT);
            std::thread::sleep(POLL_INTERVAL);
        }
        self.shutdown();
        Ok(())
    }
}

/// Entry point for `sift watch daemon run`: acquires the singleton lock
/// (refusing to start a second daemon), records daemon info, clears any
/// stale stop-flag from a previous run, and runs until stopped.
pub fn run() -> Result<(), String> {
    let lock_file = open_lock_file()?;
    match lock_file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Err("another sift watch daemon is already running".to_string())
        }
        Err(e) => return Err(format!("cannot acquire daemon lock: {e}")),
    }
    let _ = fs::remove_file(registry::daemon_stop_path());
    let info = DaemonInfo {
        pid: std::process::id(),
        started_at: registry::now_secs(),
    };
    if let Ok(s) = serde_json::to_string_pretty(&info) {
        let _ = fs::write(registry::daemon_info_path(), s);
    }

    let result = Daemon::new().and_then(|d| d.run_forever());
    let _ = lock_file.unlock();
    result
}

/// Ensures a daemon is running, spawning a detached one if not. Returns
/// once the daemon has actually acquired the singleton lock (bounded
/// wait), so `watch start` can report an accurate outcome instead of a
/// guess.
pub fn ensure_running(wait: Duration) -> Result<(), String> {
    if matches!(daemon_status(), DaemonStatus::Running { .. }) {
        return Ok(());
    }
    super::platform::spawn_detached_daemon(&registry::daemon_log_path())?;
    let start = Instant::now();
    while start.elapsed() < wait {
        if matches!(daemon_status(), DaemonStatus::Running { .. }) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("daemon did not report as running within the expected time".to_string())
}
