//! The single background watch daemon: one OS-level singleton lock, one
//! `notify` watcher, one poll loop that reconciles the registry, drains
//! filesystem events into per-root candidate trackers, and hands
//! stabilized candidates to `engine::process_candidate`. Never mutates
//! directly from a `notify` callback — events only ever land in a
//! `RootMonitor`; mutation happens later, from the poll loop, after
//! stability and live revalidation.

use super::engine::{process_candidate, RootMonitor};
use super::registry::{self, WatchState};
use crate::classifier::CategoryDB;
use crate::config::EffectivePolicy;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

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
/// One root's cached, already-resolved policy (or the error that made it
/// invalid), keyed by the local `.sift.toml`'s mtime at the time it was
/// resolved — `None` means no local file existed at that time. This is
/// the entire hot-reload mechanism: a later check with a different mtime
/// (file created, edited, or removed) simply re-resolves.
struct CachedPolicy {
    local_mtime: Option<SystemTime>,
    resolved: Result<EffectivePolicy, String>,
}

pub struct Daemon {
    watcher: RecommendedWatcher,
    rx: Receiver<notify::Event>,
    monitors: HashMap<PathBuf, RootMonitor>,
    watched_paths: HashSet<PathBuf>,
    policy_cache: HashMap<PathBuf, CachedPolicy>,
    /// Roots whose policy is *currently* known to be invalid — automatic
    /// mutation is suspended for exactly these roots. Tracked in memory
    /// (mirrored into the registry's `config_error` for display) so a
    /// healthy→unhealthy transition can be detected without a registry
    /// round trip.
    unhealthy: HashSet<PathBuf>,
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
            policy_cache: HashMap::new(),
            unhealthy: HashSet::new(),
        })
    }

    /// Resolves (using the cache above when the local file's mtime hasn't
    /// changed) the effective policy for `root`, and updates that root's
    /// runtime health: a fresh transition into an invalid policy discards
    /// any not-yet-stable candidates for `root` immediately (so they can
    /// never later "become ready" once the policy is fixed — no backfill,
    /// ever, by construction) and records the error on the registry entry
    /// for `sift watch status`; a transition back to valid clears it.
    ///
    /// `recursive` is this root's registered watch mode — a property of
    /// the watch registration, not of `.sift.toml` itself, so it's applied
    /// *after* the mtime-cached `resolve_policy` result (which is cached
    /// purely on config content) rather than folded into the cache. If
    /// `recursive` is true but the resolved strategy doesn't support it
    /// (`audio`/`video` — see `OrganizeStrategy::supports_recursive`),
    /// that's treated exactly like a broken config: fail-closed, via the
    /// same `unhealthy`/`set_config_health` path below. This is what
    /// catches a hot-reloaded `.sift.toml` that switches a
    /// already-recursive watch to `strategy = "audio"` after the fact —
    /// `cmd_watch_add` only catches this combination at registration time.
    fn refresh_policy(&mut self, root: &Path, recursive: bool) -> Result<EffectivePolicy, String> {
        let local_path = root.join(".sift.toml");
        let current_mtime = fs::metadata(&local_path).and_then(|m| m.modified()).ok();

        let cached_mtime = self.policy_cache.get(root).map(|c| c.local_mtime);
        let resolved = if current_mtime.is_some() && cached_mtime == Some(current_mtime) {
            self.policy_cache.get(root).unwrap().resolved.clone()
        } else {
            let resolved = crate::config::resolve_policy(&root.to_string_lossy());
            self.policy_cache.insert(
                root.to_path_buf(),
                CachedPolicy {
                    local_mtime: current_mtime,
                    resolved: resolved.clone(),
                },
            );
            resolved
        };
        let resolved = resolved.and_then(|policy| {
            if recursive && !policy.strategy.supports_recursive() {
                Err(format!(
                    "strategy = \"{}\" does not support recursive watch",
                    policy.strategy.as_str()
                ))
            } else {
                Ok(policy)
            }
        });

        match &resolved {
            Ok(_) => {
                if self.unhealthy.remove(root) {
                    let _ = registry::set_config_health(root, None);
                }
            }
            Err(e) => {
                let just_became_unhealthy = self.unhealthy.insert(root.to_path_buf());
                let _ = registry::set_config_health(root, Some(e.clone()));
                if just_became_unhealthy {
                    if let Some(m) = self.monitors.get_mut(root) {
                        m.discard_pending();
                    }
                }
            }
        }
        resolved
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
            self.policy_cache.remove(&p);
            self.unhealthy.remove(&p);
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
            // Resolve/refresh this root's policy every reconcile — cheap
            // (mtime-cached), and this is what makes hot reload and
            // fail-closed suspension work without any separate polling
            // mechanism for `.sift.toml` itself.
            let _ = self.refresh_policy(path, *recursive);
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
                    let unhealthy = &self.unhealthy;
                    for path in event.paths {
                        if let Some((root, monitor)) = self
                            .monitors
                            .iter_mut()
                            .find(|(root, _)| path.starts_with(root.as_path()))
                        {
                            // Never even start tracking a candidate while
                            // this root's policy is broken — it must not
                            // be backfilled once the policy is fixed, and
                            // the simplest way to guarantee that is to
                            // never have observed it in the first place.
                            if unhealthy.contains(root) {
                                continue;
                            }
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
    /// execute, then update that watch's registry stats. A root whose
    /// policy is currently invalid is skipped entirely — no candidate for
    /// it is ever moved, and none of its pending candidates can have
    /// survived to this point anyway (see `refresh_policy`).
    pub fn process_ready(&mut self) {
        let now = Instant::now();
        let unhealthy = self.unhealthy.clone();
        let policy_cache = &self.policy_cache;
        let builtin = CategoryDB::default();
        for (root, monitor) in self.monitors.iter_mut() {
            if unhealthy.contains(root) {
                continue;
            }
            let Some(policy) = policy_cache
                .get(root)
                .and_then(|c| c.resolved.as_ref().ok())
            else {
                continue;
            };
            let ready = monitor.poll_ready(now, policy.stability);
            if ready.is_empty() {
                continue;
            }
            for path in ready {
                let outcome = process_candidate(root, policy, &builtin, &path);
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
