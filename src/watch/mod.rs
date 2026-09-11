//! Sift Watch: persistent, controllable automatic organization for
//! selected directories.
//!
//! Architecture (each concern in its own module so most of it is testable
//! without a real background process):
//!   - `registry`: persisted watch configuration, concurrency-safe via an
//!     OS file lock (never `static mut`).
//!   - `eligibility`: pure path-shape/transient-name decisions.
//!   - `stability`: deterministic debounce state machine.
//!   - `engine`: turns one stabilized candidate into a plan+execution by
//!     calling straight into the existing planner/executor — the ONLY
//!     organizing authority; watch never reimplements it.
//!   - `daemon`: the single background process — notify integration,
//!     singleton lock, registry polling.
//!   - `platform`: Unix-only detached process spawning.
//!   - `tray`: best-effort auto-launch of the optional `sift-tray` GUI
//!     app alongside a watch (never required, never an error if absent).

pub mod daemon;
pub mod eligibility;
pub mod engine;
pub mod platform;
pub mod registry;
pub mod stability;
pub mod tray;

use registry::{WatchEntry, WatchState};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DAEMON_START_WAIT: Duration = Duration::from_secs(3);
const DAEMON_STOP_WAIT: Duration = Duration::from_secs(5);
/// Bounded wait, *after* the daemon process itself is confirmed running,
/// for it to reconcile and actually install a `notify` watch for one
/// specific root. Comfortably above one `daemon::POLL_INTERVAL` (400ms) —
/// this is the fix for the start/resume readiness race, not a guess.
const WATCH_READY_WAIT: Duration = Duration::from_secs(5);
/// Bounded wait, symmetric with `WATCH_READY_WAIT`, for the daemon to
/// confirm it actually tore down a root's live monitor after `watch
/// pause`/`stop` — so "no longer being monitored" is an observed fact by
/// the time either command reports success, not an assumption about the
/// daemon's poll cadence.
const WATCH_TEARDOWN_WAIT: Duration = Duration::from_secs(5);

fn canonicalize_existing(path: &str) -> Result<PathBuf, String> {
    std::fs::canonicalize(path).map_err(|_| format!("{path}: no such directory"))
}

pub fn cmd_watch_add(path: String, auto_apply: bool, recursive: bool) -> bool {
    if !auto_apply {
        eprintln!("Automatic watch organization requires explicit --auto-apply authorization.");
        return false;
    }
    let canonical = match registry::validate_root(Path::new(&path)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return false;
        }
    };
    // If this root already has a local `.sift.toml`, it must be valid
    // before we ever register an auto-apply watch against it — never
    // register a watch whose policy is already known to be broken.
    match crate::config::resolve_policy(&canonical.to_string_lossy()) {
        Ok(policy) => {
            if recursive && !policy.strategy.supports_recursive() {
                eprintln!(
                    "Refusing to add recursive watch: strategy = \"{}\" does not support --recursive yet.",
                    policy.strategy.as_str()
                );
                return false;
            }
        }
        Err(e) => {
            eprintln!("Refusing to add watch: invalid configuration.");
            eprintln!("{e}");
            return false;
        }
    }
    match registry::add(canonical.clone(), true, recursive) {
        Ok(_) => {
            println!("Added watch: {}", canonical.display());
            println!(
                "State: stopped · auto-apply · recursive {}",
                if recursive { "on" } else { "off" }
            );
            println!(
                "Run `sift watch start {}` to begin monitoring.",
                canonical.display()
            );
            println!(
                "Note: pre-existing files are NOT organized automatically. \
                 Run `sift organize {}` first if you want to clean up what's already there.",
                canonical.display()
            );
            true
        }
        Err(e) => {
            eprintln!("{e}");
            false
        }
    }
}

/// Toggles a registered watch's `--recursive` scope after the fact —
/// `sift watch add --recursive` only ever sets this at registration time
/// otherwise. Enabling it re-runs the exact same strategy-support check
/// `cmd_watch_add` runs (never let a metadata strategy that doesn't
/// support recursion end up recursive); disabling it is always allowed,
/// the same asymmetry `cmd_watch_transition` already uses for state
/// changes (only the more active direction needs validating). Takes
/// effect on the running daemon's next reconcile — no pause/resume
/// needed (see `registry::set_recursive`).
pub fn cmd_watch_set_recursive(path: String, recursive: bool) -> bool {
    let Ok(canonical) = canonicalize_existing(&path) else {
        eprintln!("{path}: no such directory");
        return false;
    };
    if recursive {
        match crate::config::resolve_policy(&canonical.to_string_lossy()) {
            Ok(policy) => {
                if !policy.strategy.supports_recursive() {
                    eprintln!(
                        "Refusing to enable --recursive: strategy = \"{}\" does not support it yet.",
                        policy.strategy.as_str()
                    );
                    return false;
                }
            }
            Err(e) => {
                eprintln!("Refusing to enable --recursive: invalid configuration.");
                eprintln!("{e}");
                return false;
            }
        }
    }
    match registry::set_recursive(&canonical, recursive) {
        Ok(entry) => {
            println!(
                "{}: recursive is now {}",
                entry.path.display(),
                if entry.recursive { "on" } else { "off" }
            );
            true
        }
        Err(e) => {
            eprintln!("{e}");
            false
        }
    }
}

pub fn cmd_watch_remove(path: String) -> bool {
    // A registered watch's path was already made canonical once, at
    // `add` time — removing it should never require re-resolving that
    // path against the live filesystem again. If the folder still
    // exists, canonicalizing first is what lets a relative or
    // symlinked path the user types match the absolute path actually
    // stored in the registry. If it doesn't (the folder was deleted,
    // moved, or was on removable/network storage that's since gone),
    // fall back to matching `path` as given directly against the
    // registry — otherwise a watch on a folder that no longer exists
    // could never be removed at all, by CLI or by `sift-tray`, even
    // though `registry::remove` itself never touches the filesystem.
    let target = canonicalize_existing(&path).unwrap_or_else(|_| PathBuf::from(&path));
    match registry::remove(&target) {
        Ok(()) => {
            println!("Removed watch: {}", target.display());
            true
        }
        Err(e) => {
            eprintln!("{e}");
            false
        }
    }
}

fn cmd_watch_transition(path: String, to: WatchState, verb: &str) -> bool {
    let Ok(canonical) = canonicalize_existing(&path) else {
        eprintln!("{path}: no such directory");
        return false;
    };
    // Never start (or resume) an auto-apply watch against a known-invalid
    // policy — validate before the transition, not after.
    if to == WatchState::Running {
        if let Err(e) = crate::config::resolve_policy(&canonical.to_string_lossy()) {
            eprintln!("Refusing to start watch: invalid configuration.");
            eprintln!("{e}");
            return false;
        }
    }
    // Only meaningful for `to == Running`: what to restore the entry to
    // if readiness can't be confirmed below (Stopped for `start`, Paused
    // for `resume`). Captured *before* the transition, since
    // `registry::transition` immediately overwrites `state` with `to`.
    let previous_state = match registry::find(&canonical) {
        Ok(Some(e)) => e.state,
        _ => WatchState::Stopped,
    };
    let entry = match registry::transition(&canonical, to) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{e}");
            return false;
        }
    };

    if to != WatchState::Running {
        // No live daemon at all means there is nothing left to tear
        // down — the registry update above is already the whole truth,
        // and waiting for an acknowledgment nothing will ever send would
        // just burn the full timeout for no reason.
        if matches!(daemon::daemon_status(), daemon::DaemonStatus::NotRunning) {
            println!(
                "{}: {} is now {}",
                verb,
                entry.path.display(),
                entry.state.label()
            );
            return true;
        }
        // Do not announce success until the daemon has actually
        // unregistered its `notify` watch and discarded any pending
        // candidates for this exact run — otherwise "paused"/"stopped"
        // would mean nothing more than "the registry file says so",
        // which is exactly the readiness gap already closed for
        // start/resume, mirrored here for the teardown direction.
        return if daemon::wait_until_torn_down(
            &canonical,
            entry.run_generation,
            WATCH_TEARDOWN_WAIT,
        ) {
            println!(
                "{}: {} is now {}",
                verb,
                entry.path.display(),
                entry.state.label()
            );
            true
        } else {
            eprintln!(
                "warning: {} is recorded as {} but the daemon has not confirmed it stopped \
                 monitoring within {:?}.",
                canonical.display(),
                entry.state.label(),
                WATCH_TEARDOWN_WAIT
            );
            eprintln!(
                "It may still process an already in-flight filesystem event. \
                 Check `sift watch daemon status` and the daemon log."
            );
            false
        };
    }

    if let Err(e) = daemon::ensure_running(DAEMON_START_WAIT) {
        eprintln!("error: could not confirm the watch daemon is running: {e}");
        eprintln!("Check with `sift watch daemon status`.");
        revert_after_unconfirmed_start(&canonical, previous_state);
        return false;
    }
    // Best-effort only: never affects this command's outcome or output
    // either way (see `tray` module docs).
    tray::ensure_running_best_effort();

    // Do not announce success until the daemon has actually installed a
    // `notify` watch for this exact root, at this exact `run_generation`
    // — otherwise a file created the instant this command returns could
    // race an as-yet-uninstalled watcher and, since Watch never backfills
    // pre-existing files, be missed forever.
    if daemon::wait_until_monitoring(&canonical, entry.run_generation, WATCH_READY_WAIT) {
        println!(
            "{}: {} is now {}",
            verb,
            entry.path.display(),
            entry.state.label()
        );
        true
    } else {
        eprintln!(
            "error: {} was not confirmed as actively monitored within {:?}.",
            canonical.display(),
            WATCH_READY_WAIT
        );
        eprintln!("Check `sift watch daemon status` and the daemon log, then retry.");
        revert_after_unconfirmed_start(&canonical, previous_state);
        false
    }
}

/// Restores the pre-transition state after a start/resume whose readiness
/// could not be confirmed, so a failed command never leaves the registry
/// claiming `running` while nothing is actually watching. Best-effort by
/// necessity (there is no more-authoritative fallback) — the loud error
/// already printed above is what tells the user not to trust the state if
/// even this fails.
fn revert_after_unconfirmed_start(canonical: &Path, previous_state: WatchState) {
    if let Err(e) = registry::transition(canonical, previous_state) {
        eprintln!(
            "warning: could not restore previous state ({}): {e}",
            previous_state.label()
        );
    }
}

pub fn cmd_watch_start(path: String) -> bool {
    cmd_watch_transition(path, WatchState::Running, "Started")
}

pub fn cmd_watch_pause(path: String) -> bool {
    cmd_watch_transition(path, WatchState::Paused, "Paused")
}

pub fn cmd_watch_resume(path: String) -> bool {
    cmd_watch_transition(path, WatchState::Running, "Resumed")
}

pub fn cmd_watch_stop(path: String) -> bool {
    cmd_watch_transition(path, WatchState::Stopped, "Stopped")
}

fn relative_activity(entry: &WatchEntry) -> String {
    match entry.last_success_at.or(entry.last_event_at) {
        Some(t) => crate::render::format_timestamp(t),
        None => "never".to_string(),
    }
}

pub fn cmd_watch_list(json: bool) -> bool {
    let watches = match registry::list() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{e}");
            return false;
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&watches).unwrap());
        return true;
    }
    println!("Sift watch");
    println!();
    if watches.is_empty() {
        println!("  No watches registered. Add one with `sift watch add <path> --auto-apply`.");
        return true;
    }
    for w in &watches {
        let symbol = match w.state {
            WatchState::Running => "\u{25cf}", // ●
            WatchState::Paused => "\u{2161}",  // Ⅱ
            WatchState::Stopped => "\u{25cb}", // ○
        };
        println!("  {symbol}  {}", w.path.display());
        let health = match (w.state, &w.config_error) {
            (WatchState::Running, Some(_)) => " · suspended",
            (WatchState::Running, None) => " · healthy",
            _ => "",
        };
        println!(
            "     {} · auto-apply · recursive {}{health}",
            w.state.label(),
            if w.recursive { "on" } else { "off" }
        );
        if w.state == WatchState::Running {
            if let Some(err) = &w.config_error {
                println!("     Configuration error: {err}");
                println!("     Automatic organization is suspended.");
            }
        }
        if w.organized_count > 0 || w.last_success_at.is_some() {
            println!(
                "     {} organized · last activity {}",
                w.organized_count,
                relative_activity(w)
            );
        }
        if let Some(err) = &w.last_error {
            println!("     last error: {err}");
        }
        println!();
    }
    true
}

pub fn cmd_watch_status(path: Option<String>, json: bool) -> bool {
    match path {
        Some(p) => {
            let Ok(canonical) = canonicalize_existing(&p) else {
                eprintln!("{p}: no such directory");
                return false;
            };
            let entry = match registry::find(&canonical) {
                Ok(Some(e)) => e,
                Ok(None) => {
                    eprintln!("{}: not a registered watch", canonical.display());
                    return false;
                }
                Err(e) => {
                    eprintln!("{e}");
                    return false;
                }
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&entry).unwrap());
                return true;
            }
            println!("Path:          {}", entry.path.display());
            let health = match (entry.state, &entry.config_error) {
                (WatchState::Running, Some(_)) => " · suspended",
                (WatchState::Running, None) => " · healthy",
                _ => "",
            };
            println!("State:         {}{health}", entry.state.label());
            println!("Recursive:     {}", entry.recursive);
            println!("Auto-apply:    {}", entry.auto_apply);
            println!("Organized:     {}", entry.organized_count);
            println!("Errors:        {}", entry.error_count);
            println!("Last activity: {}", relative_activity(&entry));
            if entry.state == WatchState::Running {
                if let Some(err) = &entry.config_error {
                    println!();
                    println!("Configuration error:");
                    println!("  {err}");
                    println!("Automatic organization is suspended.");
                }
            }
            if let Some(err) = &entry.last_error {
                println!("Last error:    {err}");
            }
            print_daemon_status_line();
            true
        }
        None => {
            print_daemon_status_line();
            cmd_watch_list(json)
        }
    }
}

fn print_daemon_status_line() {
    match daemon::daemon_status() {
        daemon::DaemonStatus::Running { pid, .. } => match pid {
            Some(pid) => println!("Daemon:        running (pid {pid})"),
            None => println!("Daemon:        running"),
        },
        daemon::DaemonStatus::NotRunning => println!("Daemon:        not running"),
    }
}

pub fn cmd_watch_daemon_status() -> bool {
    match daemon::daemon_status() {
        daemon::DaemonStatus::Running { pid, started_at } => {
            print!("running");
            if let Some(pid) = pid {
                print!(" (pid {pid})");
            }
            if let Some(t) = started_at {
                print!(", started {}", crate::render::format_timestamp(t));
            }
            println!();
        }
        daemon::DaemonStatus::NotRunning => println!("not running"),
    }
    true
}

pub fn cmd_watch_daemon_stop() -> bool {
    match daemon::request_stop_and_wait(DAEMON_STOP_WAIT) {
        Ok(true) => {
            println!("Daemon stopped.");
            true
        }
        Ok(false) => {
            eprintln!("Stop requested, but the daemon has not exited yet.");
            false
        }
        Err(e) => {
            eprintln!("{e}");
            false
        }
    }
}

/// `sift watch tray`: explicitly launches the optional `sift-tray` GUI
/// app. Unlike `tray::ensure_running_best_effort` (silent, best-effort,
/// invoked as a side effect of `start`/`resume`), this reports exactly
/// what happened.
pub fn cmd_watch_tray() -> bool {
    match tray::launch() {
        tray::LaunchOutcome::AlreadyRunning => {
            println!("sift-tray is already running.");
            true
        }
        tray::LaunchOutcome::Started => {
            println!("Started sift-tray.");
            true
        }
        tray::LaunchOutcome::NotInstalled => {
            eprintln!(
                "sift-tray is not installed. Download it from a GitHub Release, or build \
                 it with `cargo build -p sift-tray --release`, and put it next to `sift` \
                 or somewhere on your PATH — see the README's \"Optional sift-tray\" \
                 section."
            );
            false
        }
        tray::LaunchOutcome::FailedToStart => {
            eprintln!(
                "sift-tray was launched but never reported itself as running — check {} \
                 for details.",
                registry::tray_log_path().display()
            );
            false
        }
    }
}

/// `sift watch daemon run`: the internal foreground entry point for the
/// background worker process itself (normally launched detached via
/// `watch start`, never meant to be run directly by a user in a terminal
/// they intend to keep using).
pub fn cmd_watch_daemon_run() -> bool {
    match daemon::run() {
        Ok(()) => true,
        Err(e) => {
            eprintln!("watch daemon: {e}");
            false
        }
    }
}
