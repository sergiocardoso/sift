//! The single background watch daemon: one OS-level singleton lock, one
//! poll loop that reconciles the registry, drains filesystem events into
//! per-root candidate trackers, and hands stabilized candidates to
//! `engine::process_candidate`. Never mutates directly from a `notify`
//! callback — events only ever land in a `RootMonitor`; mutation happens
//! later, from the poll loop, after stability and live revalidation.
//!
//! Each currently-`running` root gets its **own** `notify::Watcher`
//! instance (see `WatchedRoot`), created fresh every time that root's
//! `run_generation` changes — never one `Watcher` shared across roots or
//! reused across generations. That's deliberate: the watcher's callback
//! closure captures the generation it was built for *by value*, so every
//! `ObservedEvent` it ever sends — no matter how late `notify`/the OS
//! actually delivers it — carries the generation it truly originated
//! under. This is what lets `drain_events` refuse a stale event from an
//! earlier, already-torn-down generation deterministically, without
//! trusting `notify`'s delivery timing (undocumented and backend-specific)
//! or the event's own file metadata (which a legitimately new file can
//! easily carry misleading old values for — `cp -p`, `rsync -a`, archive
//! extraction, restores).

use super::engine::{process_candidate, RootMonitor};
use super::registry::{self, WatchState};
use crate::classifier::CategoryDB;
use crate::config::EffectivePolicy;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
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

/// One root's cached, already-resolved policy (or the error that made it
/// invalid), keyed by the local `.sift.toml`'s mtime at the time it was
/// resolved — `None` means no local file existed at that time. This is
/// the entire hot-reload mechanism: a later check with a different mtime
/// (file created, edited, or removed) simply re-resolves.
struct CachedPolicy {
    local_mtime: Option<SystemTime>,
    resolved: Result<EffectivePolicy, String>,
}

/// One filesystem event as observed by a specific root's watcher, tagged
/// with the `run_generation` that watcher was built for — baked in when
/// the watcher's callback closure was created (see `Daemon::build_watcher`),
/// never reevaluated afterward. This is what makes a stale event from an
/// earlier, already-torn-down generation structurally impossible to
/// confuse with a current one, no matter when `notify`/the OS actually
/// gets around to delivering it.
struct ObservedEvent {
    path: PathBuf,
    generation: u64,
}

/// Everything the daemon owns for one currently-monitored root: its
/// candidate tracker, the `run_generation` it was built for, and the
/// `notify::Watcher` instance dedicated to it. The watcher is never
/// shared across roots or reused across generations — see the module
/// docs for why. Never read directly (hence the leading underscore) —
/// its entire purpose is to stay alive exactly as long as `WatchedRoot`
/// does, so dropping a `WatchedRoot` (see `reconcile`'s `to_drop`
/// handling) drops it along too, which unregisters the OS watch as an
/// ordinary `Drop` side effect; there is no separate `unwatch()` step to
/// remember.
struct WatchedRoot {
    monitor: RootMonitor,
    generation: u64,
    _watcher: RecommendedWatcher,
}

/// Owns the live daemon state: one dedicated `notify` watcher and
/// candidate tracker per currently-monitored root. Exposed as a struct
/// (not a bare `run()` loop) specifically so tests can drive `reconcile`
/// / `drain_events` / `process_ready` deterministically without needing a
/// real detached background process.
pub struct Daemon {
    tx: Sender<ObservedEvent>,
    rx: Receiver<ObservedEvent>,
    roots: HashMap<PathBuf, WatchedRoot>,
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
        Ok(Self {
            tx,
            rx,
            roots: HashMap::new(),
            policy_cache: HashMap::new(),
            unhealthy: HashSet::new(),
        })
    }

    /// Builds a brand-new `notify::Watcher` dedicated to `path`, whose
    /// callback tags every event it will ever produce with `generation` —
    /// fixed at closure-creation time, never looked up again later. Does
    /// not install the OS-level watch itself; the caller still calls
    /// `.watch(path, mode)` on the result.
    fn build_watcher(&self, generation: u64) -> Result<RecommendedWatcher, notify::Error> {
        let tx = self.tx.clone();
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                for path in event.paths {
                    let _ = tx.send(ObservedEvent { path, generation });
                }
            }
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
                    if let Some(w) = self.roots.get_mut(root) {
                        w.monitor.discard_pending();
                    }
                }
            }
        }
        resolved
    }

    /// Whether `root` currently has a live `WatchedRoot` (i.e. is actually
    /// being watched right now, as of the last `reconcile`).
    pub fn is_monitoring(&self, root: &Path) -> bool {
        self.roots.contains_key(root)
    }

    /// The `recursive` scope `root`'s live monitor was built with, if
    /// it's currently being monitored — lets a test confirm that toggling
    /// `recursive` while running actually rebuilds the monitor (see
    /// `reconcile`'s handling of a changed `recursive` value) instead of
    /// silently keeping the stale one.
    pub fn monitor_recursive(&self, root: &Path) -> Option<bool> {
        self.roots.get(root).map(|w| w.monitor.recursive)
    }

    /// Syncs live state (which roots have a dedicated `notify` watcher and
    /// candidate tracker) with the registry's current `running` set. A
    /// watch that becomes paused/stopped/removed — or whose
    /// `run_generation` moved on even while nominally staying `Running`
    /// (a pause immediately followed by a resume, both landing between
    /// two `reconcile` calls) — has its `WatchedRoot` dropped here,
    /// tearing down its dedicated OS watch and discarding its pending
    /// (not-yet-stable) candidates as an ordinary consequence of the drop;
    /// there is no separate "safely stop" step.
    pub fn reconcile(&mut self) {
        let entries = registry::list().unwrap_or_default();
        let running: HashMap<PathBuf, (bool, u64)> = entries
            .iter()
            .filter(|e| e.state == WatchState::Running)
            .map(|e| (e.path.clone(), (e.recursive, e.run_generation)))
            .collect();

        // A root drops out of `roots` because it's no longer running at
        // all, because its `recursive` scope changed since the monitor
        // was built (e.g. toggled from `sift-tray` — only takes effect at
        // watcher-creation time), or because its `run_generation` moved
        // on without `running` ever visibly dropping it. Any of these
        // forces a full rebuild — a fresh `WatchedRoot`, fresh
        // `StabilityTracker`, fresh dedicated watcher tagging events with
        // the new generation.
        let to_drop: Vec<PathBuf> = self
            .roots
            .iter()
            .filter(|(p, w)| match running.get(*p) {
                None => true,
                Some((recursive, generation)) => {
                    w.monitor.recursive != *recursive || w.generation != *generation
                }
            })
            .map(|(p, _)| p.clone())
            .collect();
        for p in &to_drop {
            // Dropping `WatchedRoot` drops its `watcher` field, which
            // unregisters the OS-level watch — no manual `unwatch()`.
            if let Some(dropped) = self.roots.remove(p) {
                // Acknowledge teardown only now that the real watcher is
                // actually gone and every in-memory structure for that
                // generation is gone with it. This is what `sift watch
                // pause`/`stop` wait on (see `wait_until_torn_down`) so
                // "no longer being monitored" is a fact the daemon
                // observed, not a guess about its poll cadence.
                let _ = registry::mark_torn_down(p, dropped.generation);
            }
            self.policy_cache.remove(p);
            self.unhealthy.remove(p);
            // The live monitor this readiness ack described is gone —
            // never leave a stale "ready" behind it (see `watch start`'s
            // wait in `super::cmd_watch_transition`, which would
            // otherwise have no way to notice the root stopped being
            // watched).
            let _ = registry::clear_monitoring_ready(p);
        }

        for (path, (recursive, generation)) in &running {
            if !self.roots.contains_key(path) {
                match self.build_watcher(*generation) {
                    Ok(mut watcher) => {
                        let mode = if *recursive {
                            RecursiveMode::Recursive
                        } else {
                            RecursiveMode::NonRecursive
                        };
                        if watcher.watch(path, mode).is_ok() {
                            self.roots.insert(
                                path.clone(),
                                WatchedRoot {
                                    monitor: RootMonitor::new(path.clone(), *recursive),
                                    generation: *generation,
                                    _watcher: watcher,
                                },
                            );
                            // This is the one place a root's dedicated
                            // `notify` watch actually comes into
                            // existence — only from here can it be true
                            // that a filesystem event for `path` will
                            // ever reach `drain_events`. Ack it
                            // immediately, so `watch start`/`resume` can
                            // stop waiting the instant it's real.
                            let _ = registry::mark_monitoring_ready(path, *generation);
                        }
                        // If `.watch()` failed, `watcher` is simply
                        // dropped here (nothing was ever registered) —
                        // `reconcile` will just try again next tick.
                    }
                    Err(_) => {
                        // Couldn't even create a watcher this tick —
                        // retry on the next `reconcile`.
                    }
                }
            } else {
                // Already watching at exactly this generation — ack it
                // again every tick, not only the tick that just installed
                // it. A pause immediately followed by a resume (both
                // within one poll interval) never shows up in `to_drop`
                // at all if this loop only acked on fresh installs, so
                // `watch resume`'s wait would time out despite the watch
                // having been live the entire time.
                let _ = registry::mark_monitoring_ready(path, *generation);
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
    ///
    /// Every event carries the generation the *specific watcher instance*
    /// that produced it was built for (see `ObservedEvent`/`build_watcher`).
    /// An event whose tag doesn't match the root's *current* generation is
    /// dropped here unconditionally — this is the structural guarantee
    /// that a late-delivered event from an earlier, already-torn-down
    /// generation can never be misattributed to a freshly rebuilt monitor,
    /// independent of `notify`'s delivery timing and independent of the
    /// underlying file's own metadata (which a legitimately new file can
    /// easily carry misleading old values for).
    pub fn drain_events(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self.rx.recv_timeout(remaining) {
                Ok(ObservedEvent { path, generation }) => {
                    let now = Instant::now();
                    let unhealthy = &self.unhealthy;
                    if let Some((root, watched)) = self
                        .roots
                        .iter_mut()
                        .find(|(root, _)| path.starts_with(root.as_path()))
                    {
                        if watched.generation != generation {
                            continue;
                        }
                        // Never even start tracking a candidate while
                        // this root's policy is broken — it must not be
                        // backfilled once the policy is fixed, and the
                        // simplest way to guarantee that is to never have
                        // observed it in the first place.
                        if unhealthy.contains(root) {
                            continue;
                        }
                        let _ = registry::record_event(root);
                        watched.monitor.observe_event(&path, now);
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
    ///
    /// For a recursive watch, a candidate outside `root` itself is planned
    /// against the *nearest* policy governing its containing directory
    /// (`config::resolve_nested_policy_override`), not blindly against `root`'s own
    /// cached policy — this is what lets a subfolder's own local
    /// `.sift.toml` take over its own subtree. `root`'s cached policy still
    /// decides the stability window for every candidate uniformly (a
    /// per-directory debounce window is not worth the added complexity),
    /// and still gates whether this root is watched at all.
    pub fn process_ready(&mut self) {
        let now = Instant::now();
        let unhealthy = self.unhealthy.clone();
        let policy_cache = &self.policy_cache;
        let builtin = CategoryDB::default();
        for (root, watched) in self.roots.iter_mut() {
            if unhealthy.contains(root) {
                continue;
            }
            let Some(policy) = policy_cache
                .get(root)
                .and_then(|c| c.resolved.as_ref().ok())
            else {
                continue;
            };
            let ready = watched.monitor.poll_ready(now, policy.stability);
            if ready.is_empty() {
                continue;
            }
            for path in ready {
                // Re-authorize immediately before every mutation. Every
                // in-memory field checked above (`self.roots`,
                // `self.unhealthy`, `self.policy_cache`) is only ever as
                // fresh as this tick's `reconcile()` call — and a
                // concurrent `sift watch pause`/`stop` (a separate
                // process, writing straight to the shared registry) can
                // land at any wall-clock instant, including exactly
                // between this tick's `reconcile()` and this exact
                // `process_ready()` call, with no intervening reconcile
                // to notice it. A stale in-memory "yes" must never stand
                // in for a fresh check right here: re-read the registry
                // and confirm both that the requested state is still
                // Running *and* that the generation this monitor was
                // built for still matches the current one (a pause
                // immediately followed by a resume bumps the generation
                // without ever visibly dropping `Running`). This is
                // distinct from — and still needed alongside —
                // `drain_events`'s per-event generation tag: that guards
                // whether a candidate is *tracked* at all; this guards
                // whether an already-legitimately-tracked candidate is
                // still *authorized* by the time it's actually mutated.
                let authorized = matches!(
                    registry::find(root),
                    Ok(Some(e)) if e.state == WatchState::Running && e.run_generation == watched.generation
                );
                if !authorized {
                    continue;
                }
                let containing_dir = path.parent().unwrap_or(root.as_path());
                let resolved;
                let candidate_policy =
                    if watched.monitor.recursive && containing_dir != root.as_path() {
                        match crate::config::resolve_nested_policy_override(root, containing_dir) {
                            Some(Ok((p, owner))) => {
                                // The owning directory's own strategy doesn't
                                // support recursion, and this candidate lives
                                // deeper than that directory — same boundary
                                // `plan_recursive_nested` enforces for manual
                                // recursive organize, applied here to one live
                                // candidate instead of a whole tree walk.
                                if !p.strategy.supports_recursive() && containing_dir != owner {
                                    continue;
                                }
                                resolved = p;
                                &resolved
                            }
                            // An invalid nested `.sift.toml` fails closed for
                            // just this one candidate — never the whole
                            // (otherwise healthy) root, and never recorded as a
                            // root-level error.
                            Some(Err(_)) => continue,
                            // No override anywhere between here and root: root's
                            // own cached policy governs, exactly as before.
                            None => policy,
                        }
                    } else {
                        policy
                    };
                let outcome = process_candidate(root, candidate_policy, &builtin, &path);
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
        // Dropping every `WatchedRoot` (and the `notify::Watcher` each
        // one owns) unregisters every OS-level watch as an ordinary
        // `Drop` side effect.
        self.roots.clear();
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

/// Bounded wait for `root` to be actively monitored at exactly
/// `generation` — i.e. for some daemon `reconcile()` tick to have called
/// `registry::mark_monitoring_ready(root, generation)` after successfully
/// installing a real `notify` watch for it. Polls the shared on-disk
/// registry (the only channel between this process and the daemon
/// process) rather than sleeping a fixed amount and hoping; returns as
/// soon as readiness is observed, or `false` once `timeout` elapses
/// without it. This is what lets `watch start`/`watch resume` (see
/// `super::cmd_watch_transition`) hold off printing success until a file
/// created the instant afterward is guaranteed not to race an
/// as-yet-uninstalled watcher.
pub fn wait_until_monitoring(root: &Path, generation: u64, timeout: Duration) -> bool {
    const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
    let start = Instant::now();
    loop {
        if let Ok(Some(entry)) = registry::find(root) {
            if entry.monitoring_generation == Some(generation) {
                return true;
            }
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(READINESS_POLL_INTERVAL);
    }
}

/// Bounded wait for `root`'s live monitor to be confirmed torn down —
/// the pause/stop counterpart to `wait_until_monitoring`. `generation` is
/// the `run_generation` that was actually being monitored (the value
/// from *before* the pause/stop transition, since that transition itself
/// never changes `run_generation` — only entering `Running` does).
pub fn wait_until_torn_down(root: &Path, generation: u64, timeout: Duration) -> bool {
    const TEARDOWN_POLL_INTERVAL: Duration = Duration::from_millis(50);
    let start = Instant::now();
    loop {
        if let Ok(Some(entry)) = registry::find(root) {
            if entry.torn_down_generation == Some(generation) {
                return true;
            }
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(TEARDOWN_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests here (rather than in `tests/watch_integration.rs`) exist
    //! specifically to reach `Daemon`'s private fields directly, so a
    //! candidate can be manufactured as "already ready" — or an event can
    //! be manufactured as "observed under an old generation" — with zero
    //! reliance on real elapsed time, `stability_seconds`, the poll
    //! interval, or any sleep.
    use super::*;
    use crate::config::set_test_global_config_dir;
    use crate::watch::registry::{add, clear_test_watch_dir, find, set_test_watch_dir, transition};

    fn isolate() -> (tempfile::TempDir, tempfile::TempDir) {
        let watch_dir = tempfile::tempdir().unwrap();
        set_test_watch_dir(watch_dir.path().join(".sift-watch-test"));
        let global_dir = tempfile::tempdir().unwrap();
        set_test_global_config_dir(global_dir.path().to_path_buf());
        (watch_dir, global_dir)
    }

    /// Forces the exact interleaving the readiness/authorization design
    /// must survive: `reconcile()` runs while the registry still says
    /// `Running`, installing a live monitor; *then*, with no further
    /// `reconcile()` in between, some other actor (a concurrent `sift
    /// watch pause`/`stop`, modeled here by calling `transition` directly
    /// against the same on-disk registry) revokes that authorization;
    /// *then* `process_ready()` runs against a candidate that is
    /// unconditionally ready by construction (a zero-length stability
    /// window plus one already-consumed baseline poll — never a real
    /// elapsed duration). The only question this test answers is whether
    /// `process_ready()` re-checks authorization for itself, since
    /// nothing else in this sequence ever tells it the state changed.
    #[test]
    fn process_ready_refuses_to_mutate_once_registry_state_changed_after_reconcile() {
        let (_watch_dir, _global_dir) = isolate();
        let root_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path().canonicalize().unwrap();

        add(root.clone(), true, false).unwrap();
        transition(&root, WatchState::Running).unwrap();

        let mut daemon = Daemon::new().unwrap();
        // `reconcile()` while the registry still says Running: installs
        // the real monitor, the real `notify` watch, and records the
        // generation it was built for — exactly what a real `tick()`
        // would have done.
        daemon.reconcile();
        assert!(daemon.roots.contains_key(&root));
        assert_eq!(find(&root).unwrap().unwrap().state, WatchState::Running);

        // Force the stability window to zero so the *second* poll of an
        // already-tracked candidate is unconditionally ready, regardless
        // of how much (or how little) wall-clock time actually elapses
        // between the two calls below — no `stability_seconds`, no sleep.
        daemon.policy_cache.get_mut(&root).unwrap().resolved = Ok(EffectivePolicy {
            stability: Duration::ZERO,
            ..EffectivePolicy::default()
        });

        let file = root.join("invoice.pdf");
        fs::write(&file, b"x").unwrap();
        let now = Instant::now();
        daemon
            .roots
            .get_mut(&root)
            .unwrap()
            .monitor
            .observe_event(&file, now);
        // First poll only ever establishes the baseline snapshot — never
        // ready on its own, by construction of `StabilityTracker`.
        assert!(daemon
            .roots
            .get_mut(&root)
            .unwrap()
            .monitor
            .poll_ready(now, Duration::ZERO)
            .is_empty());

        // Some other actor revokes authorization for this exact
        // generation — no `reconcile()` call happens in between, so
        // `process_ready()` below is the *only* thing that can still
        // catch this.
        transition(&root, WatchState::Paused).unwrap();

        // The candidate is unconditionally ready now (same tracked
        // baseline, zero-length window): `process_ready()` runs with
        // whatever authorization check it has — or doesn't have.
        daemon.process_ready();

        assert!(
            file.exists(),
            "process_ready() must never mutate the filesystem once the registry state \
             changed away from Running, even with no intervening reconcile() to notice it"
        );
        assert!(!root.join("Images/invoice.pdf").exists());

        clear_test_watch_dir();
        crate::config::clear_test_global_config_dir();
    }

    /// Same shape, but the revocation is a generation bump with `state`
    /// staying `Running` the whole time (the pause-immediately-resumed
    /// case) — `process_ready()` must catch this from the generation
    /// mismatch alone, since `state == Running` on its own says nothing
    /// about which run it refers to.
    #[test]
    fn process_ready_refuses_to_mutate_once_run_generation_advanced_after_reconcile() {
        let (_watch_dir, _global_dir) = isolate();
        let root_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path().canonicalize().unwrap();

        add(root.clone(), true, false).unwrap();
        transition(&root, WatchState::Running).unwrap();

        let mut daemon = Daemon::new().unwrap();
        daemon.reconcile();

        daemon.policy_cache.get_mut(&root).unwrap().resolved = Ok(EffectivePolicy {
            stability: Duration::ZERO,
            ..EffectivePolicy::default()
        });

        let file = root.join("invoice.pdf");
        fs::write(&file, b"x").unwrap();
        let now = Instant::now();
        daemon
            .roots
            .get_mut(&root)
            .unwrap()
            .monitor
            .observe_event(&file, now);
        assert!(daemon
            .roots
            .get_mut(&root)
            .unwrap()
            .monitor
            .poll_ready(now, Duration::ZERO)
            .is_empty());

        // Pause then resume, both with no `reconcile()` call in between —
        // `state` is `Running` again by the time `process_ready()` runs,
        // but at a *newer* generation than the live monitor was built for.
        transition(&root, WatchState::Paused).unwrap();
        let resumed = transition(&root, WatchState::Running).unwrap();
        assert_ne!(
            Some(resumed.run_generation),
            daemon.roots.get(&root).map(|w| w.generation),
            "test precondition: the generation must actually have moved past what the \
             live monitor was built for"
        );

        daemon.process_ready();

        assert!(
            file.exists(),
            "process_ready() must never mutate the filesystem for a monitor generation \
             that no longer matches the registry's current run_generation, even though \
             state == Running throughout"
        );
        assert!(!root.join("Images/invoice.pdf").exists());

        clear_test_watch_dir();
        crate::config::clear_test_global_config_dir();
    }

    /// Proves the structural, notify-timing-independent fix for a late
    /// event from an earlier, already-torn-down generation: manufactures
    /// exactly that scenario by hand — a root currently watched at
    /// generation 2, and an `ObservedEvent` tagged with generation 1 sent
    /// directly into the daemon's channel (modeling a `notify` callback
    /// from generation 1's now-dropped watcher, whose event only reaches
    /// the channel *after* generation 2's watcher already exists). No
    /// pause/resume timing, no real `notify` delivery delay, no sleep is
    /// involved — the tag alone must decide this.
    ///
    /// The companion positive case — a legitimately new file whose *file
    /// mtime* happens to predate the current generation (`cp -p`,
    /// `rsync -a`, archive extraction) — is exercised end-to-end with a
    /// real `notify::Watcher` in
    /// `tests/watch_integration.rs::real_notify_preserved_old_mtime_current_generation_event_is_still_organized`,
    /// proving the fix does *not* reject on file metadata the way an
    /// earlier, incorrect version of this fix did.
    #[test]
    fn drain_events_discards_an_event_tagged_with_a_generation_older_than_the_root_is_currently_watched_at(
    ) {
        let (_watch_dir, _global_dir) = isolate();
        let root_dir = tempfile::tempdir().unwrap();
        let root = root_dir.path().canonicalize().unwrap();

        add(root.clone(), true, false).unwrap();
        transition(&root, WatchState::Running).unwrap(); // generation 1
        transition(&root, WatchState::Paused).unwrap();
        let resumed = transition(&root, WatchState::Running).unwrap(); // generation 2

        // Created *before* the generation-2 watcher is ever installed
        // below, so no genuine `notify` event ever fires for it (the
        // usual "no backfill" rule) — the only event this test will see
        // for this path is the synthetic, hand-tagged one sent further
        // down. Without this, the real generation-2 watcher would fire
        // its own (correctly-tagged, legitimately trackable) event for
        // this same file the moment it's created, confounding the
        // assertion below with an event this test isn't trying to test.
        let file = root.join("late-from-generation-1.jpg");
        fs::write(&file, b"x").unwrap();

        let mut daemon = Daemon::new().unwrap();
        daemon.reconcile();
        assert_eq!(
            daemon.roots.get(&root).map(|w| w.generation),
            Some(resumed.run_generation),
            "test precondition: the live monitor must be at the *current* generation"
        );

        // Model a `notify` event that generation 1's (already-dropped)
        // watcher produced, arriving only now — tagged with generation 1
        // at the moment *that* watcher's closure was created, forever
        // fixed, regardless of when it's actually drained.
        let stale_generation = resumed.run_generation - 1;
        daemon
            .tx
            .send(ObservedEvent {
                path: file.clone(),
                generation: stale_generation,
            })
            .unwrap();

        daemon.drain_events(Duration::from_millis(200));

        assert_eq!(
            daemon.roots.get(&root).unwrap().monitor.pending_count(),
            0,
            "an event tagged with a generation older than the root's current one must \
             never be tracked as a candidate at all"
        );

        clear_test_watch_dir();
        crate::config::clear_test_global_config_dir();
    }
}
