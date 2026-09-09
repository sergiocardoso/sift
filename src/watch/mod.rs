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

pub mod daemon;
pub mod eligibility;
pub mod engine;
pub mod platform;
pub mod registry;
pub mod stability;

use registry::{WatchEntry, WatchState};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DAEMON_START_WAIT: Duration = Duration::from_secs(3);
const DAEMON_STOP_WAIT: Duration = Duration::from_secs(5);

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

pub fn cmd_watch_remove(path: String) -> bool {
    let Ok(canonical) = canonicalize_existing(&path) else {
        eprintln!("{path}: no such directory");
        return false;
    };
    match registry::remove(&canonical) {
        Ok(()) => {
            println!("Removed watch: {}", canonical.display());
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
    match registry::transition(&canonical, to) {
        Ok(entry) => {
            println!(
                "{}: {} is now {}",
                verb,
                entry.path.display(),
                entry.state.label()
            );
            if to == WatchState::Running {
                if let Err(e) = daemon::ensure_running(DAEMON_START_WAIT) {
                    eprintln!("warning: watch state updated, but the daemon is not confirmed running: {e}");
                    eprintln!("Check with `sift watch daemon status`.");
                }
            }
            true
        }
        Err(e) => {
            eprintln!("{e}");
            false
        }
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
