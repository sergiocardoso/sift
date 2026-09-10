//! Best-effort auto-launch of the optional `sift-tray` GUI app whenever a
//! watch starts running from the `sift` CLI itself.
//!
//! `sift-tray` is deliberately a separate binary (see its own crate docs)
//! so the CLI never pulls in GUI dependencies, and most installs don't
//! even ship it (`install.sh` only installs `sift`). So every step here
//! is "if present, if it works" — failure is always silent to the
//! caller: `sift watch start`/`resume` must never fail, warn, or block on
//! whether a tray icon could be shown.
//!
//! Singleton detection mirrors `daemon::daemon_status`: an OS file lock at
//! `registry::tray_lock_path()`, held by `sift-tray` for its entire run
//! (see `sift-tray/src/main.rs`), tested here non-blocking.

use super::registry;
use std::fs;
use std::path::PathBuf;

fn open_lock_file() -> Result<fs::File, String> {
    fs::create_dir_all(registry::watch_dir()).map_err(|e| format!("{e}"))?;
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(registry::tray_lock_path())
        .map_err(|e| format!("{e}"))
}

/// Same non-blocking acquire-then-release probe `daemon::daemon_status`
/// uses: the held lock itself is the ground truth, not a PID file.
fn is_running() -> bool {
    let file = match open_lock_file() {
        Ok(f) => f,
        Err(_) => return false,
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            false
        }
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(_) => false,
    }
}

/// Only the `sift` CLI binary itself should ever attempt this — never
/// `sift-tray`, whose own "Resume"/"Add folder" menu actions call straight
/// into the same `cmd_watch_transition` this is wired from (see
/// `sift-tray/src/main.rs`). Without this guard, resuming a watch from the
/// tray's own menu would try to spawn a second tray from inside the first.
fn called_from_cli_binary() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_os_string()))
        .is_some_and(|s| s == "sift")
}

/// Finds a `sift-tray` binary next to the current executable or on
/// `PATH`. Returns `None` (not an error) when it simply isn't installed.
fn find_tray_binary() -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    if let Some(dir) = current.parent() {
        let sibling = dir.join("sift-tray");
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join("sift-tray"))
        .find(|candidate| candidate.is_file())
}

#[cfg(unix)]
fn spawn_detached(exe: PathBuf) {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let Ok(log_out) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(registry::tray_log_path())
    else {
        return;
    };
    let Ok(log_err) = log_out.try_clone() else {
        return;
    };
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(log_out))
        .stderr(Stdio::from(log_err));
    // SAFETY: same async-signal-safe `setsid(2)` pre-exec hook as
    // `platform::spawn_detached_daemon`, for the same reason: the tray
    // must outlive the terminal that ran `sift watch start`.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let _ = cmd.spawn();
}

#[cfg(not(unix))]
fn spawn_detached(exe: PathBuf) {
    let _ = std::process::Command::new(exe).spawn();
}

/// Best-effort: if a `sift-tray` binary can be found and none is already
/// running (per the singleton lock), launch it detached. Every failure
/// mode — not installed, no display server, lock already held, spawn
/// error — is silently ignored.
pub fn ensure_running_best_effort() {
    if !called_from_cli_binary() || is_running() {
        return;
    }
    if let Some(exe) = find_tray_binary() {
        spawn_detached(exe);
    }
}
