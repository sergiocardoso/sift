//! File-stability debouncing: never organize a file while something else
//! might still be writing it. `now` is always passed in by the caller
//! rather than read internally, which makes the threshold logic fully
//! deterministic and testable without any real waiting.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Conservative default: a candidate must be observed unchanged for this
/// long before it's considered stable enough to organize.
pub const DEFAULT_STABILITY_WINDOW: Duration = Duration::from_millis(2500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Snapshot {
    size: u64,
    mtime_secs: u64,
}

fn snapshot_of(path: &Path) -> Option<Snapshot> {
    let md = std::fs::symlink_metadata(path).ok()?;
    if !md.is_file() {
        return None;
    }
    let mtime_secs = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(Snapshot {
        size: md.len(),
        mtime_secs,
    })
}

struct Pending {
    snapshot: Option<Snapshot>,
    stable_since: Instant,
}

/// Tracks candidates waiting to prove they've stopped changing before
/// they're handed off for organizing. Never spawns a thread per file —
/// callers drive this with periodic `poll_ready` calls from one loop.
#[derive(Default)]
pub struct StabilityTracker {
    pending: HashMap<PathBuf, Pending>,
}

impl StabilityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts (or keeps) tracking `path`. A no-op if already tracked.
    pub fn track(&mut self, path: PathBuf, now: Instant) {
        self.pending.entry(path).or_insert_with(|| Pending {
            snapshot: None,
            stable_since: now,
        });
    }

    pub fn discard(&mut self, path: &Path) {
        self.pending.remove(path);
    }

    /// Drops every tracked candidate without processing it — used on
    /// pause/stop, per the "no queued catch-up" invariant.
    pub fn discard_all(&mut self) {
        self.pending.clear();
    }

    pub fn is_tracking(&self, path: &Path) -> bool {
        self.pending.contains_key(path)
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Re-checks every tracked candidate's live size/mtime. A candidate
    /// whose metadata changed since the last check has its stability timer
    /// reset (it keeps being deferred for as long as it keeps changing). A
    /// candidate that has vanished, or is no longer a plain file, is
    /// dropped silently — never marked ready. A candidate unchanged for at
    /// least `window` is returned (and stops being tracked).
    pub fn poll_ready(&mut self, now: Instant, window: Duration) -> Vec<PathBuf> {
        let mut ready = Vec::new();
        let mut gone = Vec::new();
        for (path, pending) in self.pending.iter_mut() {
            let Some(snap) = snapshot_of(path) else {
                gone.push(path.clone());
                continue;
            };
            if pending.snapshot != Some(snap) {
                pending.snapshot = Some(snap);
                pending.stable_since = now;
                continue;
            }
            if now.saturating_duration_since(pending.stable_since) >= window {
                ready.push(path.clone());
            }
        }
        for p in &gone {
            self.pending.remove(p);
        }
        for p in &ready {
            self.pending.remove(p);
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn new_file_is_not_ready_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        File::create(&path).unwrap();

        let mut t = StabilityTracker::new();
        let t0 = Instant::now();
        t.track(path.clone(), t0);
        let window = Duration::from_millis(100);
        assert!(t.poll_ready(t0, window).is_empty());
    }

    #[test]
    fn unchanged_file_becomes_ready_after_window() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, b"hello").unwrap();

        let mut t = StabilityTracker::new();
        let t0 = Instant::now();
        t.track(path.clone(), t0);
        let window = Duration::from_millis(200);

        // First poll establishes the baseline snapshot; not ready yet.
        assert!(t.poll_ready(t0, window).is_empty());
        // Still within the window.
        assert!(t
            .poll_ready(t0 + Duration::from_millis(50), window)
            .is_empty());
        // Past the window with no changes: ready.
        let ready = t.poll_ready(t0 + Duration::from_millis(250), window);
        assert_eq!(ready, vec![path.clone()]);
        assert!(!t.is_tracking(&path));
    }

    #[test]
    fn changing_file_keeps_deferring() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        let mut f = File::create(&path).unwrap();
        f.write_all(b"a").unwrap();
        drop(f);

        let mut t = StabilityTracker::new();
        let t0 = Instant::now();
        t.track(path.clone(), t0);
        let window = Duration::from_millis(200);
        assert!(t.poll_ready(t0, window).is_empty());

        // The file grows right before the window would have elapsed.
        fs::write(&path, b"a longer payload now").unwrap();
        assert!(t
            .poll_ready(t0 + Duration::from_millis(250), window)
            .is_empty());

        // No further changes: now it should stabilize, timer reset.
        let ready = t.poll_ready(t0 + Duration::from_millis(500), window);
        assert_eq!(ready, vec![path]);
    }

    #[test]
    fn vanished_file_is_dropped_not_marked_ready() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, b"x").unwrap();

        let mut t = StabilityTracker::new();
        let t0 = Instant::now();
        t.track(path.clone(), t0);
        t.poll_ready(t0, Duration::from_millis(100));
        fs::remove_file(&path).unwrap();

        let ready = t.poll_ready(t0 + Duration::from_millis(200), Duration::from_millis(100));
        assert!(ready.is_empty());
        assert!(!t.is_tracking(&path));
    }

    #[test]
    fn discard_all_drops_pending_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"y").unwrap();

        let mut t = StabilityTracker::new();
        let t0 = Instant::now();
        t.track(a.clone(), t0);
        t.track(b.clone(), t0);
        assert_eq!(t.len(), 2);
        t.discard_all();
        assert!(t.is_empty());
        let ready = t.poll_ready(t0 + Duration::from_secs(10), Duration::from_millis(1));
        assert!(ready.is_empty());
    }
}
