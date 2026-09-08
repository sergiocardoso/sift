//! The deterministic core of watch: turning one stabilized candidate path
//! into a plan and an execution, by calling straight into the existing
//! scanner/planner/executor pipeline. Nothing here talks to `notify` or
//! any daemon machinery, so it's fully testable against a real tempdir
//! without starting anything in the background.

use super::eligibility::{is_eligible_candidate_path, is_transient_filename};
use super::stability::StabilityTracker;
use crate::classifier::CategoryDB;
use crate::config::Rule;
use crate::domain::{Op, Plan};
use crate::{executor, planner, scanner};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What happened to one candidate after processing.
pub struct ProcessOutcome {
    /// A file was actually moved successfully.
    pub organized: bool,
    /// The candidate was safely skipped (protected, collision, unknown
    /// type with no destination — though in practice organize always has
    /// a destination now) rather than moved. Not an error.
    pub skip_reason: Option<String>,
    /// Execution was attempted and failed (e.g. a TOCTOU collision).
    pub failure: Option<String>,
    /// Set whenever a history record was written (successful or failed
    /// moves both produce one; a pre-execution skip does not).
    pub history_id: Option<String>,
}

/// Processes exactly one stabilized candidate: re-validates it against the
/// LIVE filesystem (never trusts that the original event is still true),
/// plans its action with the same per-entry planner manual organize uses,
/// and executes with the same executor — never anything file-specific
/// invented here. Touches no file other than `path`.
pub fn process_candidate(
    watch_root: &Path,
    config_rules: &[Rule],
    builtin: &CategoryDB,
    path: &Path,
) -> ProcessOutcome {
    let entry = match scanner::revalidate_candidate(watch_root, path) {
        Ok(e) => e,
        Err(reason) => {
            return ProcessOutcome {
                organized: false,
                skip_reason: Some(reason.to_string()),
                failure: None,
                history_id: None,
            }
        }
    };
    let containing_dir = path.parent().unwrap_or(watch_root);
    let entry_plan = planner::plan_entry_organize(&entry, containing_dir, config_rules, builtin);

    if entry_plan.action.op == Op::Skip {
        return ProcessOutcome {
            organized: false,
            skip_reason: entry_plan.action.reason.clone(),
            failure: None,
            history_id: None,
        };
    }

    let mut actions = Vec::new();
    if let Some(cd) = entry_plan.create_dir {
        actions.push(cd);
    }
    actions.push(entry_plan.action);
    let plan = Plan { actions };

    let (id, outcomes) = executor::execute_plan(
        plan,
        watch_root.to_string_lossy().as_ref(),
        "organize",
        Some(watch_root),
    );
    let failure = outcomes
        .iter()
        .find(|o| o.result.is_err())
        .and_then(|o| o.result.clone().err());
    let organized = failure.is_none()
        && outcomes
            .iter()
            .any(|o| o.op == Op::Move && o.result.is_ok());
    ProcessOutcome {
        organized,
        skip_reason: None,
        failure,
        history_id: Some(id),
    }
}

/// Live, per-root state: which candidates are being debounced right now.
/// One of these exists per `running` watch; the daemon drops it (and any
/// pending candidates) the moment a watch stops being `running`.
pub struct RootMonitor {
    pub root: PathBuf,
    pub recursive: bool,
    tracker: StabilityTracker,
}

impl RootMonitor {
    pub fn new(root: PathBuf, recursive: bool) -> Self {
        Self {
            root,
            recursive,
            tracker: StabilityTracker::new(),
        }
    }

    /// Feeds one raw filesystem event path in. Filters out anything
    /// structurally ineligible (wrong depth, transient download name)
    /// before it's ever tracked — cheap, no filesystem access beyond the
    /// path string itself.
    pub fn observe_event(&mut self, path: &Path, now: Instant) {
        if !is_eligible_candidate_path(&self.root, self.recursive, path) {
            return;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if is_transient_filename(name) {
            return;
        }
        self.tracker.track(path.to_path_buf(), now);
    }

    /// Drops all pending (not-yet-stable) candidates without processing
    /// them — used on pause/stop/removal, per the "no queued catch-up"
    /// invariant: only events observed *after* a resume/restart matter.
    pub fn discard_pending(&mut self) {
        self.tracker.discard_all();
    }

    pub fn pending_count(&self) -> usize {
        self.tracker.len()
    }

    pub fn poll_ready(&mut self, now: Instant, window: Duration) -> Vec<PathBuf> {
        self.tracker.poll_ready(now, window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Rule;
    use std::fs;

    fn no_rules() -> Vec<Rule> {
        vec![]
    }

    #[test]
    fn organizes_a_recognized_file() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let file = root.join("photo.jpg");
        fs::write(&file, b"x").unwrap();

        sift_history_isolated(root, || {
            let outcome = process_candidate(root, &no_rules(), &CategoryDB::default(), &file);
            assert!(outcome.organized);
            assert!(outcome.failure.is_none());
            assert!(root.join("Images/photo.jpg").exists());
            assert!(!file.exists());
        });
    }

    #[test]
    fn skips_a_protected_candidate() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let file = root.join(".hidden.jpg");
        fs::write(&file, b"x").unwrap();

        sift_history_isolated(root, || {
            let outcome = process_candidate(root, &no_rules(), &CategoryDB::default(), &file);
            assert!(!outcome.organized);
            assert_eq!(outcome.skip_reason.as_deref(), Some("candidate is hidden"));
            assert!(file.exists());
        });
    }

    #[test]
    fn revalidates_ancestor_that_became_a_project_root() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        fs::create_dir_all(root.join("app")).unwrap();
        let file = root.join("app/index.js");
        fs::write(&file, b"x").unwrap();
        // The candidate is inside `app/`; if `app/` becomes a project root
        // before we get to processing it, the candidate must be refused.
        fs::write(root.join("app/package.json"), b"{}").unwrap();

        sift_history_isolated(root, || {
            let outcome = process_candidate(root, &no_rules(), &CategoryDB::default(), &file);
            assert!(!outcome.organized);
            assert!(outcome.skip_reason.is_some());
            assert!(file.exists());
        });
    }

    #[test]
    fn collision_is_skipped_never_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        fs::create_dir_all(root.join("Images")).unwrap();
        fs::write(root.join("Images/photo.jpg"), b"existing").unwrap();
        let file = root.join("photo.jpg");
        fs::write(&file, b"new").unwrap();

        sift_history_isolated(root, || {
            let outcome = process_candidate(root, &no_rules(), &CategoryDB::default(), &file);
            assert!(!outcome.organized);
            assert_eq!(outcome.skip_reason.as_deref(), Some("collision"));
            assert!(file.exists());
            assert_eq!(
                fs::read_to_string(root.join("Images/photo.jpg")).unwrap(),
                "existing"
            );
        });
    }

    #[test]
    fn root_monitor_ignores_nested_events_when_not_recursive() {
        let root = PathBuf::from("/tmp/inbox");
        let mut m = RootMonitor::new(root.clone(), false);
        let now = Instant::now();
        m.observe_event(&root.join("Client/a.jpg"), now);
        assert_eq!(m.pending_count(), 0);
        m.observe_event(&root.join("a.jpg"), now);
        assert_eq!(m.pending_count(), 1);
    }

    #[test]
    fn root_monitor_ignores_transient_download_names() {
        let root = PathBuf::from("/tmp/inbox");
        let mut m = RootMonitor::new(root.clone(), false);
        let now = Instant::now();
        m.observe_event(&root.join("video.mp4.crdownload"), now);
        assert_eq!(m.pending_count(), 0);
    }

    #[test]
    fn root_monitor_discard_pending_drops_candidates() {
        let root = PathBuf::from("/tmp/inbox");
        let mut m = RootMonitor::new(root.clone(), false);
        let now = Instant::now();
        m.observe_event(&root.join("a.jpg"), now);
        assert_eq!(m.pending_count(), 1);
        m.discard_pending();
        assert_eq!(m.pending_count(), 0);
    }

    /// Runs `f` with history writes isolated to a subdirectory of `root`,
    /// restoring the thread-local override afterward. Kept local to these
    /// tests since `process_candidate` calls straight into the executor,
    /// which records history.
    fn sift_history_isolated(root: &Path, f: impl FnOnce()) {
        crate::history::set_test_history_dir(root.join(".sift-history-test"));
        f();
        crate::history::clear_test_history_dir();
    }
}
