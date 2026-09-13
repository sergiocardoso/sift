//! The deterministic core of watch: turning one stabilized candidate path
//! into a plan and an execution, by calling straight into the existing
//! scanner/planner/executor pipeline. Nothing here talks to `notify` or
//! any daemon machinery, so it's fully testable against a real tempdir
//! without starting anything in the background.

use super::eligibility::{is_eligible_candidate_path, is_transient_filename};
use super::stability::StabilityTracker;
use crate::classifier::CategoryDB;
use crate::config::EffectivePolicy;
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

/// Whether any directory component between `watch_root` and `path` (i.e.
/// every ancestor, not `path` itself) could be one of *the policy
/// governing its own parent*'s organize destinations — either a rendered
/// Date template component (the same structural question
/// `discover_recursive_dirs_for_date` asks during recursive organize,
/// asked here of one live path instead of a whole tree), or a static
/// `type` destination (`planner::could_be_own_output_dir` — a built-in
/// category name, or an explicit `[[rules]]` `Move` destination). This is
/// what keeps a recursive watch from ever reprocessing its own
/// self-generated destination event (`2026/09/invoice.pdf` right after
/// moving `invoice.pdf` there, or `PDF/report.pdf` right after a
/// `*.pdf -> "PDF"` rule moved it) into `2026/09/2026/09/invoice.pdf` or
/// `PDF/PDF/report.pdf`.
///
/// Walks ancestors top-down from `watch_root`, re-deriving which policy
/// actually governs each one (starting from `root_policy`, switching to a
/// directory's own local `.sift.toml` whenever one exists) rather than
/// checking every ancestor against one flat, already-leaf-resolved
/// policy. That distinction matters whenever an ancestor's name happens
/// to collide with a reserved category name (`Images`, `Documents`, ...):
/// an ordinary `Images/` with no override of its own is still correctly
/// treated as `watch_root`'s own dead-end destination, but an `Images/`
/// that declares its *own* `.sift.toml` is a deliberately governed
/// subtree instead — the same precedence
/// `config::resolve_nested_policy_override` gives it everywhere else —
/// and must not be treated as a boundary just because its name also
/// happens to be a built-in category.
fn crosses_strategy_generated_boundary(
    watch_root: &Path,
    path: &Path,
    root_policy: &EffectivePolicy,
) -> bool {
    let Ok(rel) = path.strip_prefix(watch_root) else {
        return false;
    };
    let components: Vec<_> = rel.components().collect();
    if components.is_empty() {
        return false;
    }
    let mut governing = root_policy.clone();
    let mut dir = watch_root.to_path_buf();
    for c in &components[..components.len() - 1] {
        let std::path::Component::Normal(name) = c else {
            continue;
        };
        let name = name.to_string_lossy();
        if let Some(template) = &governing.template {
            if template.component_could_be_generated(0, &name) {
                return true;
            }
        }
        let child_dir = dir.join(name.as_ref());
        if crate::planner::could_be_own_output_dir(&governing, &name) {
            match crate::config::local_policy_override(&child_dir) {
                Some(Ok(_)) => {}
                // No override of its own (or an invalid one, which fails
                // closed the same way it does everywhere else) — an
                // incidental drop target, not a governed subtree, so it's
                // exactly the self-generated destination this boundary
                // check exists to catch.
                _ => return true,
            }
        }
        if let Some(Ok(p)) = crate::config::local_policy_override(&child_dir) {
            governing = p;
        }
        dir = child_dir;
    }
    false
}

/// Processes exactly one stabilized candidate: re-validates it against the
/// LIVE filesystem (never trusts that the original event is still true),
/// plans its action with the same per-entry planner manual organize uses,
/// and executes with the same executor — never anything file-specific
/// invented here. Touches no file other than `path`.
///
/// `root_policy` and `policy` are deliberately separate: `policy` is
/// whichever policy actually governs `path`'s own containing directory
/// (the caller already resolved this — root's own, or a nested
/// `.sift.toml`'s override — and it's what plans/executes this specific
/// file), while `root_policy` is always `watch_root`'s own policy,
/// needed by `crosses_strategy_generated_boundary` to walk ancestors from
/// the top rather than from whichever policy the leaf happened to
/// resolve to. For a candidate sitting directly under `watch_root`
/// they're the same value.
pub fn process_candidate(
    watch_root: &Path,
    root_policy: &EffectivePolicy,
    policy: &EffectivePolicy,
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
    if crosses_strategy_generated_boundary(watch_root, path, root_policy) {
        return ProcessOutcome {
            organized: false,
            skip_reason: Some("date-organized directory".to_string()),
            failure: None,
            history_id: None,
        };
    }
    let containing_dir = path.parent().unwrap_or(watch_root);
    let entry_plan = planner::plan_entry_with_strategy(&entry, containing_dir, policy, builtin);

    if entry_plan.action.op == Op::Skip {
        return ProcessOutcome {
            organized: false,
            skip_reason: entry_plan.action.reason.clone(),
            failure: None,
            history_id: None,
        };
    }

    let mut actions = entry_plan.create_dirs;
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
    use std::fs;

    fn default_policy() -> EffectivePolicy {
        EffectivePolicy::default()
    }

    #[test]
    fn organizes_a_recognized_file() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let file = root.join("photo.jpg");
        fs::write(&file, b"x").unwrap();

        sift_history_isolated(root, || {
            let outcome = process_candidate(
                root,
                &default_policy(),
                &default_policy(),
                &CategoryDB::default(),
                &file,
            );
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
            let outcome = process_candidate(
                root,
                &default_policy(),
                &default_policy(),
                &CategoryDB::default(),
                &file,
            );
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
            let outcome = process_candidate(
                root,
                &default_policy(),
                &default_policy(),
                &CategoryDB::default(),
                &file,
            );
            assert!(!outcome.organized);
            assert!(outcome.skip_reason.is_some());
            assert!(file.exists());
        });
    }

    #[test]
    fn collision_with_different_content_is_renamed_never_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        fs::create_dir_all(root.join("Images")).unwrap();
        fs::write(root.join("Images/photo.jpg"), b"existing").unwrap();
        let file = root.join("photo.jpg");
        fs::write(&file, b"new").unwrap();

        sift_history_isolated(root, || {
            let outcome = process_candidate(
                root,
                &default_policy(),
                &default_policy(),
                &CategoryDB::default(),
                &file,
            );
            assert!(outcome.organized);
            assert!(!file.exists());
            assert_eq!(
                fs::read_to_string(root.join("Images/photo.jpg")).unwrap(),
                "existing"
            );
            assert_eq!(
                fs::read_to_string(root.join("Images/photo (1).jpg")).unwrap(),
                "new"
            );
        });
    }

    #[test]
    fn collision_with_identical_content_trashes_the_duplicate() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        fs::create_dir_all(root.join("Images")).unwrap();
        fs::write(root.join("Images/photo.jpg"), b"same").unwrap();
        let file = root.join("photo.jpg");
        fs::write(&file, b"same").unwrap();

        sift_history_isolated(root, || {
            let outcome = process_candidate(
                root,
                &default_policy(),
                &default_policy(),
                &CategoryDB::default(),
                &file,
            );
            assert!(!outcome.organized);
            assert!(!file.exists());
            assert_eq!(
                fs::read_to_string(root.join("Images/photo.jpg")).unwrap(),
                "same"
            );
        });
    }

    #[test]
    fn blocked_collision_via_symlink_is_still_skipped() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        fs::create_dir_all(root.join("Images")).unwrap();
        std::os::unix::fs::symlink("/nonexistent", root.join("Images/photo.jpg")).unwrap();
        let file = root.join("photo.jpg");
        fs::write(&file, b"new").unwrap();

        sift_history_isolated(root, || {
            let outcome = process_candidate(
                root,
                &default_policy(),
                &default_policy(),
                &CategoryDB::default(),
                &file,
            );
            assert!(!outcome.organized);
            assert_eq!(outcome.skip_reason.as_deref(), Some("collision"));
            assert!(file.exists());
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
