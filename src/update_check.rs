//! Best-effort, opt-out, cached check for a newer `sift` release.
//!
//! Sift is otherwise local-first and makes no network calls of its own.
//! This is the one exception, so it's deliberately built to never add
//! latency or noise to a normal command:
//!   - The check itself never runs inline. `spawn_background_check_if_due`
//!     only ever reads a small cache file (cheap, local) and, at most once
//!     per [`CHECK_INTERVAL_SECS`], spawns a fully detached
//!     `sift __update-check` process to do the actual network call —
//!     the invoking command never waits on it, so a slow or unreachable
//!     network never adds latency, and a machine that's offline just
//!     never gets a notice (no error, no retry storm).
//!   - `print_notice_if_cached` only ever reads that same cache file and
//!     prints to stderr, so it can never interfere with `--json` output
//!     on stdout.
//!   - `SIFT_NO_UPDATE_CHECK` (any value) disables the network side
//!     entirely; the cache is simply never refreshed.
//!
//! Uses `curl` (already a hard dependency of `install.sh`, and the exact
//! same GitHub Releases endpoint it queries) via a detached subprocess
//! rather than adding an HTTP client dependency to the CLI itself — the
//! same "shell out opportunistically, degrade silently if absent"
//! pattern already used for `ffprobe` and the OS file-manager openers.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const RELEASES_API_URL: &str = "https://api.github.com/repos/sergiocardoso/sift/releases/latest";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Cache {
    last_checked_at: u64,
    /// The newest version this cache has ever seen (e.g. `"0.3.0"`, no
    /// leading `v`), kept across a failed refresh so a transient network
    /// error never erases a real pending-update notice.
    latest_seen: Option<String>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> PathBuf {
    if let Some(proj) = directories::ProjectDirs::from("org", "flokin", "Sift") {
        return proj.data_dir().join("update-check.json");
    }
    PathBuf::from(".sift-update-check.json")
}

fn read_cache() -> Cache {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_cache(cache: &Cache) {
    let path = cache_path();
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    if let Ok(s) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(path, s);
    }
}

/// Parses `"major.minor.patch"` (a leading `v` and anything past the
/// first three dot-separated components, e.g. `-beta.1`, are ignored) for
/// a numeric, non-lexicographic comparison — a plain string compare would
/// rank `"0.9.0"` above `"0.10.0"`.
fn parse_semver(v: &str) -> (u64, u64, u64) {
    let mut parts = v.trim_start_matches('v').splitn(3, '.').map(|p| {
        p.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0)
    });
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn opted_out() -> bool {
    std::env::var_os("SIFT_NO_UPDATE_CHECK").is_some()
}

/// Called once near the start of every real command. Reads the cache
/// (cheap, local, never blocks); if it's due for a refresh, spawns the
/// detached background checker and returns immediately without waiting
/// on it — the notice from *this* refresh only ever shows up starting
/// with the next command invocation, never this one.
pub fn spawn_background_check_if_due() {
    if opted_out() {
        return;
    }
    let cache = read_cache();
    if now_secs().saturating_sub(cache.last_checked_at) < CHECK_INTERVAL_SECS {
        return;
    }
    spawn_detached_checker();
}

/// Called once near the end of every real command, after its own output.
/// Prints a one-line notice to stderr — never stdout, so it can never
/// land inside `--json` output — if the cache's most recently seen
/// version is newer than the binary actually running right now.
pub fn print_notice_if_cached() {
    if opted_out() {
        return;
    }
    if let Some(msg) = pending_notice(read_cache().latest_seen, crate::VERSION) {
        eprintln!();
        eprintln!("{msg}");
    }
}

/// Pure decision: given the cache's most recently seen version and the
/// version actually running, the notice text to print, or `None` if
/// there's nothing newer to report (no cache yet, or already up to date
/// — including the ordinary case where `latest_seen` just repeats the
/// current version). Split out from `print_notice_if_cached` so the
/// decision itself is testable without capturing stderr.
fn pending_notice(latest_seen: Option<String>, current_version: &str) -> Option<String> {
    let latest = latest_seen?;
    if parse_semver(&latest) <= parse_semver(current_version) {
        return None;
    }
    Some(format!(
        "A new version of sift is available: v{current_version} -> v{latest}. Run ./install.sh \
         again to update (see https://github.com/sergiocardoso/sift#installation), or set \
         SIFT_NO_UPDATE_CHECK=1 to stop checking."
    ))
}

fn fetch_latest_tag() -> Option<String> {
    let output = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "3", RELEASES_API_URL])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let body = String::from_utf8(output.stdout).ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    Some(tag.trim_start_matches('v').to_string())
}

/// Entry point for the hidden `sift __update-check` subcommand: the
/// detached process `spawn_background_check_if_due` spawns. Does the one
/// network call, then unconditionally refreshes `last_checked_at` (even
/// on failure — an offline machine must not retry on every single
/// command, only once per [`CHECK_INTERVAL_SECS`]) while preserving
/// whatever `latest_seen` was already cached if this fetch didn't
/// succeed.
pub fn run_hidden_check() -> std::process::ExitCode {
    let previous = read_cache();
    let cache = Cache {
        last_checked_at: now_secs(),
        latest_seen: fetch_latest_tag().or(previous.latest_seen),
    };
    write_cache(&cache);
    std::process::ExitCode::SUCCESS
}

#[cfg(unix)]
fn spawn_detached_checker() {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = Command::new(exe);
    cmd.arg("__update-check")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: same async-signal-safe `setsid(2)` pre-exec hook as
    // `watch::platform::spawn_detached_daemon`, for the same reason: this
    // must outlive the short-lived command that triggered it.
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
fn spawn_detached_checker() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = std::process::Command::new(exe)
        .arg("__update-check")
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_semver_orders_numerically_not_lexicographically() {
        // A plain string compare would rank "0.9.0" above "0.10.0".
        assert!(parse_semver("0.10.0") > parse_semver("0.9.0"));
    }

    #[test]
    fn parse_semver_strips_leading_v() {
        assert_eq!(parse_semver("v1.2.3"), parse_semver("1.2.3"));
    }

    #[test]
    fn parse_semver_ignores_prerelease_suffix() {
        assert_eq!(parse_semver("1.2.3-beta.1"), (1, 2, 3));
    }

    #[test]
    fn parse_semver_defaults_missing_components_to_zero() {
        assert_eq!(parse_semver("1"), (1, 0, 0));
        assert_eq!(parse_semver("1.2"), (1, 2, 0));
    }

    #[test]
    fn pending_notice_is_none_without_a_cached_version() {
        assert!(pending_notice(None, "0.2.2").is_none());
    }

    #[test]
    fn pending_notice_is_none_when_up_to_date() {
        assert!(pending_notice(Some("0.2.2".to_string()), "0.2.2").is_none());
    }

    #[test]
    fn pending_notice_is_none_when_cache_is_older_than_current() {
        // Can't happen in practice (the cache only ever records what the
        // GitHub API reports), but must never claim a downgrade is an
        // update either way.
        assert!(pending_notice(Some("0.1.0".to_string()), "0.2.2").is_none());
    }

    #[test]
    fn pending_notice_reports_both_versions_when_newer() {
        let msg = pending_notice(Some("0.3.0".to_string()), "0.2.2").unwrap();
        assert!(msg.contains("v0.2.2"));
        assert!(msg.contains("v0.3.0"));
    }

    #[test]
    fn cache_round_trips_through_json() {
        let cache = Cache {
            last_checked_at: 12345,
            latest_seen: Some("0.3.0".to_string()),
        };
        let json = serde_json::to_string(&cache).unwrap();
        let back: Cache = serde_json::from_str(&json).unwrap();
        assert_eq!(back.last_checked_at, 12345);
        assert_eq!(back.latest_seen.as_deref(), Some("0.3.0"));
    }

    #[test]
    fn read_cache_on_garbage_json_falls_back_to_default_not_a_panic() {
        let cache: Cache = serde_json::from_str("not json").unwrap_or_default();
        assert_eq!(cache.last_checked_at, 0);
        assert!(cache.latest_seen.is_none());
    }
}
