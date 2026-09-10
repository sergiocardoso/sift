//! Human-readable ("pretty") output for every Sift command.
//!
//! Nothing here decides what happens on disk — it only describes, after the
//! fact, what a `Plan` or a set of `ActionResult`s means to a person. All
//! `--json` output bypasses this module entirely and is untouched by it.

use crate::domain::{Action, ActionResult, Entry, HistoryItem, Op};
use crate::folders::{Decision, FolderCandidate, FoldersPlan};
use crate::scanner::DoctorFinding;
use std::path::Path;

// ---------------------------------------------------------------- helpers

pub fn use_color() -> bool {
    use std::io::IsTerminal;
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

pub fn colorize(s: &str, code: &str, enabled: bool) -> String {
    if enabled {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

/// Renders a path relative to `base`, purely for display; never changes
/// what is stored, serialized, or acted on.
pub fn display_rel(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

/// Formats a byte count for humans, e.g. `512 B`, `1.3 KB`, `4.0 MB`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Formats a UNIX timestamp (seconds) as `YYYY-MM-DD HH:MM UTC`, with no
/// external date dependency. Only non-negative input is expected here
/// since timestamps come from `SystemTime::now()`. The calendar-date math
/// itself lives in `utils::civil_from_unix_secs` — the Date organize
/// strategy uses the exact same function, so there's one implementation
/// of "timestamp to calendar date" in the whole crate.
pub fn format_timestamp(secs: u64) -> String {
    let secs = secs as i64;
    let rem = secs.rem_euclid(86400);
    let (hour, minute) = (rem / 3600, (rem % 3600) / 60);
    let (year, month, day) = crate::utils::civil_from_unix_secs(secs);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Skip reasons that only ever apply to directories, so listings can show
/// a trailing `/` without needing a live filesystem check.
const DIR_LIKE_REASONS: &[&str] = &[
    "software project",
    "protected directory",
    "directory",
    "hidden directory",
];

fn with_dir_suffix(name: String, reason: &str) -> String {
    if DIR_LIKE_REASONS.contains(&reason) {
        format!("{name}/")
    } else {
        name
    }
}

fn filter_op(actions: &[Action], op: Op) -> Vec<&Action> {
    actions.iter().filter(|a| a.op == op).collect()
}

// ------------------------------------------------------------- organize

/// Above this many skipped entries, list by reason with counts instead of
/// one line per entry (unless `verbose`), so a large messy directory
/// doesn't drown the useful part of the plan.
const SKIP_LIST_THRESHOLD: usize = 12;

fn render_moves_section(moves: &[&Action], target: &Path) {
    if moves.is_empty() {
        return;
    }
    println!();
    println!("Moves");
    let width = moves
        .iter()
        .map(|a| display_rel(&a.src, target).len())
        .max()
        .unwrap_or(0);
    for a in moves {
        let dest_dir = a
            .dst
            .as_ref()
            .and_then(|d| d.parent())
            .map(|p| display_rel(p, target))
            .unwrap_or_default();
        println!("  {:<width$}  → {dest_dir}/", display_rel(&a.src, target));
    }
}

fn render_trash_section(title: &str, trashes: &[&Action], target: &Path) {
    if trashes.is_empty() {
        return;
    }
    println!();
    println!("{title}");
    let width = trashes
        .iter()
        .map(|a| display_rel(&a.src, target).len())
        .max()
        .unwrap_or(0);
    for a in trashes {
        let reason = a.reason.as_deref().unwrap_or("");
        println!("  {:<width$}  {reason}", display_rel(&a.src, target));
    }
}

/// Whether `reason` names one of the automatic duplicate-collision
/// resolutions (`planner::IDENTICAL_DUPLICATE_REASON_PREFIX`/
/// `RENAMED_COLLISION_REASON_SUFFIX`) — the one thing this renderer always
/// surfaces on its own, in a dedicated section, regardless of `--verbose`
/// or the skip-list threshold. Silently trashing or renaming something
/// because it collided with an existing same-name file is exactly the
/// kind of automatic decision a user must never have to go looking for in
/// a terminal.
fn is_duplicate_collision_reason(reason: &str) -> bool {
    reason.starts_with(crate::planner::IDENTICAL_DUPLICATE_REASON_PREFIX)
        || reason.ends_with(crate::planner::RENAMED_COLLISION_REASON_SUFFIX)
}

/// Always-shown warning block for every action `is_duplicate_collision_reason`
/// recognizes, called from both dry-run renderers (`&[Action]`, before
/// anything happens) — the apply-result renderer has its own
/// `render_apply_duplicate_warnings`, since by then execution may have
/// refused one of these on re-check.
fn render_duplicate_warnings(actions: &[Action], target: &Path) {
    let warnings: Vec<&Action> = actions
        .iter()
        .filter(|a| {
            a.reason
                .as_deref()
                .is_some_and(is_duplicate_collision_reason)
        })
        .collect();
    if warnings.is_empty() {
        return;
    }
    println!();
    println!(
        "{} Duplicate name{} handled automatically",
        colorize("⚠", "33", use_color()),
        plural(warnings.len())
    );
    for a in &warnings {
        println!("  {}", display_rel(&a.src, target));
        println!("    {}", a.reason.as_deref().unwrap_or(""));
    }
}

fn render_skip_section(skips: &[&Action], target: &Path, verbose: bool) {
    if skips.is_empty() {
        return;
    }
    println!();
    if verbose || skips.len() <= SKIP_LIST_THRESHOLD {
        println!("Skipped");
        let width = skips
            .iter()
            .map(|a| display_rel(&a.src, target).len())
            .max()
            .unwrap_or(0);
        for a in skips {
            let reason = a.reason.as_deref().unwrap_or("");
            let name = with_dir_suffix(display_rel(&a.src, target), reason);
            println!("  {name:<width$}  {reason}");
        }
    } else {
        println!("Skipped ({})", skips.len());
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for a in skips {
            *counts
                .entry(a.reason.as_deref().unwrap_or("unspecified"))
                .or_insert(0) += 1;
        }
        let mut counted: Vec<(&str, usize)> = counts.into_iter().collect();
        counted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        for (reason, n) in counted {
            println!("  {n:>3}  {reason}");
        }
        println!("  (run with --verbose to list them individually)");
    }
}

fn refusal_notice(command: &str, path: &str) {
    println!();
    println!("Refusing: {path} is itself a software project root.");
    println!(
        "{command} never mutates a project root — run `sift doctor {path}` to inspect it safely."
    );
}

/// Reported when `resolve_policy` fails for a manual command: the local
/// (or global) `.sift.toml` exists but is invalid. Refuses to run rather
/// than silently falling back to defaults — a broken policy is never
/// quietly ignored, whether or not Watch is involved.
pub fn policy_error(command: &str, path: &str, error: &str) {
    println!("Sift {command}");
    println!("{path}");
    println!();
    println!("{} Invalid configuration", colorize("✗", "31", use_color()));
    println!();
    for line in error.lines() {
        println!("  {line}");
    }
    println!();
    println!("No changes made.");
}

pub fn organize_dry_run(path: &str, actions: &[Action], is_project_root: bool, verbose: bool) {
    println!("Sift organize");
    println!("{path}");
    if is_project_root {
        refusal_notice("organize", path);
        return;
    }

    let target = Path::new(path);
    let creates = filter_op(actions, Op::CreateDir);
    let moves = filter_op(actions, Op::Move);
    let trashes = filter_op(actions, Op::Trash);
    let skips = filter_op(actions, Op::Skip);

    println!();
    println!("Plan");
    if !moves.is_empty() {
        println!("  {} move{}", moves.len(), plural(moves.len()));
    }
    if !creates.is_empty() {
        println!(
            "  {} director{}",
            creates.len(),
            if creates.len() == 1 { "y" } else { "ies" }
        );
    }
    if !trashes.is_empty() {
        println!("  {} trashed", trashes.len());
    }
    if !skips.is_empty() {
        println!("  {} skipped", skips.len());
    }
    if moves.is_empty() && creates.is_empty() && trashes.is_empty() && skips.is_empty() {
        println!("  nothing to do");
    }

    render_moves_section(&moves, target);
    render_trash_section("Trash", &trashes, target);
    render_skip_section(&skips, target, verbose);
    render_duplicate_warnings(actions, target);

    println!();
    if moves.is_empty() && trashes.is_empty() {
        println!("Nothing to do.");
    } else {
        println!("No changes made.");
        println!("Run with --apply to execute.");
    }
}

fn is_traversal_boundary(action: &Action) -> bool {
    action
        .reason
        .as_deref()
        .map(|r| crate::scanner::TRAVERSAL_BOUNDARY_REASONS.contains(&r))
        .unwrap_or(false)
}

/// Header for a directory's own section in the recursive plan view: the
/// recursion root shows its own name (there's nothing to show it relative
/// to), everything else shows its path relative to the root so depth stays
/// unambiguous.
fn dir_header(dir: &Path, target: &Path) -> String {
    if dir == target {
        target
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| format!("{s}/"))
            .unwrap_or_else(|| ".".to_string())
    } else {
        format!("{}/", display_rel(dir, target))
    }
}

/// Groups moves by the directory they were found in (preserving the
/// deterministic directory-by-directory order the planner already
/// produced), rendering one small table per directory — this is what
/// keeps a recursive plan from reading as one undifferentiated wall of
/// moves spanning unrelated directories.
fn render_moves_by_directory(moves: &[&Action], target: &Path) {
    if moves.is_empty() {
        return;
    }
    let mut groups: Vec<(&Path, Vec<&Action>)> = Vec::new();
    for a in moves {
        let dir = a.src.parent().unwrap_or(target);
        match groups.iter_mut().find(|(d, _)| *d == dir) {
            Some((_, v)) => v.push(a),
            None => groups.push((dir, vec![a])),
        }
    }
    for (dir, group) in &groups {
        println!();
        println!("{}", dir_header(dir, target));
        let width = group
            .iter()
            .map(|a| {
                a.src
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .len()
            })
            .max()
            .unwrap_or(0);
        for a in group {
            let fname = a.src.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let dest_name = a
                .dst
                .as_ref()
                .and_then(|d| d.parent())
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("");
            println!("  {fname:<width$}  → {dest_name}/");
        }
    }
}

fn render_protected_section(dir_skips: &[&Action], target: &Path) {
    if dir_skips.is_empty() {
        return;
    }
    println!();
    println!("Protected");
    let width = dir_skips
        .iter()
        .map(|a| display_rel(&a.src, target).len() + 1)
        .max()
        .unwrap_or(0);
    for a in dir_skips {
        let name = format!("{}/", display_rel(&a.src, target));
        let reason = a.reason.as_deref().unwrap_or("");
        println!("  {name:<width$}  {reason}");
    }
}

/// The recursive counterpart to `organize_dry_run`: same safety semantics
/// and the same underlying `Plan`, but grouped so a plan spanning many
/// directories still reads as a set of small, local organize operations
/// rather than one undifferentiated list. `dirs_scanned` comes from the
/// planner because an eligible plain directory produces zero actions of
/// its own and so can't be recovered from `actions` alone.
pub fn organize_dry_run_recursive(
    path: &str,
    actions: &[Action],
    dirs_scanned: usize,
    is_project_root: bool,
    verbose: bool,
) {
    println!("Sift organize");
    println!("{path}");
    if is_project_root {
        refusal_notice("organize", path);
        return;
    }

    let target = Path::new(path);
    let creates = filter_op(actions, Op::CreateDir);
    let moves = filter_op(actions, Op::Move);
    let trashes = filter_op(actions, Op::Trash);
    let all_skips = filter_op(actions, Op::Skip);
    let (dir_skips, file_skips): (Vec<&Action>, Vec<&Action>) = all_skips
        .into_iter()
        .partition(|a| is_traversal_boundary(a));

    println!();
    println!("Recursive scan");
    println!(
        "  {dirs_scanned} director{} scanned",
        if dirs_scanned == 1 { "y" } else { "ies" }
    );
    println!(
        "  {} director{} protected",
        dir_skips.len(),
        if dir_skips.len() == 1 { "y" } else { "ies" }
    );
    let files_inspected = moves.len() + trashes.len() + file_skips.len();
    println!(
        "  {files_inspected} file{} inspected",
        plural(files_inspected)
    );

    println!();
    println!("Plan");
    if !moves.is_empty() {
        println!("  {} move{}", moves.len(), plural(moves.len()));
    }
    if !creates.is_empty() {
        println!(
            "  {} director{} to create",
            creates.len(),
            if creates.len() == 1 { "y" } else { "ies" }
        );
    }
    if !trashes.is_empty() {
        println!("  {} trashed", trashes.len());
    }
    let skipped_total = dir_skips.len() + file_skips.len();
    if skipped_total > 0 {
        println!("  {skipped_total} skipped");
    }

    render_moves_by_directory(&moves, target);
    render_trash_section("Trash", &trashes, target);
    render_skip_section(&file_skips, target, verbose);
    render_protected_section(&dir_skips, target);
    render_duplicate_warnings(actions, target);

    println!();
    if moves.is_empty() && trashes.is_empty() {
        println!("Nothing to do.");
    } else {
        println!("No changes made.");
        println!(
            "Run with --apply to execute {} move{}.",
            moves.len(),
            plural(moves.len())
        );
    }
}

fn count_ok(outcomes: &[ActionResult], op: Op) -> usize {
    outcomes
        .iter()
        .filter(|o| o.op == op && o.result.is_ok())
        .count()
}

/// The apply-result counterpart to `render_duplicate_warnings`: same
/// always-shown treatment, but reflecting what execution actually did —
/// `dst` on a matching `Action` carries the existing duplicate it was
/// re-verified against right before trashing (see
/// `executor::execute_plan`'s `Op::Trash` arm), so a collision resolved at
/// plan time can still show up here as refused if the world changed
/// underneath it in between.
fn render_apply_duplicate_warnings(actions: &[Action], outcomes: &[ActionResult]) {
    let warnings: Vec<(&Action, &ActionResult)> = actions
        .iter()
        .zip(outcomes.iter())
        .filter(|(a, _)| {
            a.reason
                .as_deref()
                .is_some_and(is_duplicate_collision_reason)
        })
        .collect();
    if warnings.is_empty() {
        return;
    }
    println!();
    println!(
        "{} Duplicate name{} handled automatically",
        colorize("⚠", "33", use_color()),
        plural(warnings.len())
    );
    for (a, o) in &warnings {
        println!("  {}", a.src.display());
        match &o.result {
            Ok(()) => println!("    {}", a.reason.as_deref().unwrap_or("")),
            Err(e) => println!("    refused — {e}"),
        }
    }
}

/// Reports what execution actually did (never assumes the plan succeeded),
/// shared by `organize --apply` and `clean --apply`. `actions` is the
/// pre-execution plan `outcomes` came from (same order, one-to-one) —
/// needed here purely to recover each action's `reason` for
/// `render_apply_duplicate_warnings`, since `ActionResult` itself doesn't
/// carry one.
fn render_apply_outcome_summary(actions: &[Action], outcomes: &[ActionResult], hist_id: &str) {
    let color = use_color();
    let has_moves = outcomes.iter().any(|o| o.op == Op::Move);
    let has_creates = outcomes.iter().any(|o| o.op == Op::CreateDir);
    let has_trash = outcomes.iter().any(|o| o.op == Op::Trash);

    let moved = count_ok(outcomes, Op::Move);
    let created = count_ok(outcomes, Op::CreateDir);
    let trashed = count_ok(outcomes, Op::Trash);
    let skipped = outcomes.iter().filter(|o| o.op == Op::Skip).count();
    let failed = outcomes.iter().filter(|o| o.result.is_err()).count();

    if failed == 0 {
        println!("{} Applied successfully", colorize("✓", "32", color));
    } else {
        println!(
            "{} Applied with {failed} failure{}",
            colorize("⚠", "33", color),
            plural(failed)
        );
    }
    println!();
    if has_moves {
        println!("  {moved} file{} moved", plural(moved));
    }
    if has_creates {
        println!(
            "  {created} director{} created",
            if created == 1 { "y" } else { "ies" }
        );
    }
    if has_trash {
        println!("  {trashed} file{} trashed", plural(trashed));
    }
    println!(
        "  {skipped} entr{} skipped",
        if skipped == 1 { "y" } else { "ies" }
    );
    println!("  {failed} failure{}", plural(failed));

    if failed > 0 {
        println!();
        println!("Failures");
        for o in outcomes.iter().filter(|o| o.result.is_err()) {
            if let Err(ref e) = o.result {
                println!("  {}  {e}", o.src.display());
            }
        }
    }

    render_apply_duplicate_warnings(actions, outcomes);

    println!();
    println!("History");
    println!("  {hist_id}");

    if outcomes.iter().any(|o| o.undoable) {
        println!();
        println!("Undo");
        println!("  sift undo {hist_id}");
    }
}

pub fn organize_apply_result(
    path: &str,
    actions: &[Action],
    outcomes: &[ActionResult],
    hist_id: &str,
) {
    println!("Sift organize");
    println!("{path}");
    println!();
    render_apply_outcome_summary(actions, outcomes, hist_id);
}

// ----------------------------------------------------------------- clean

pub fn clean_dry_run(path: &str, actions: &[Action], is_project_root: bool, verbose: bool) {
    println!("Sift clean");
    println!("{path}");
    if is_project_root {
        refusal_notice("clean", path);
        return;
    }

    let target = Path::new(path);
    let trashes = filter_op(actions, Op::Trash);
    let skips = filter_op(actions, Op::Skip);

    if trashes.is_empty() && skips.is_empty() {
        println!();
        println!("Nothing to do.");
        return;
    }

    if trashes.is_empty() {
        println!();
        println!("No trash candidates found.");
    } else {
        render_trash_section("Trash candidates", &trashes, target);
    }

    if !skips.is_empty() {
        println!();
        println!("Skipped");
        if verbose {
            let width = skips
                .iter()
                .map(|a| display_rel(&a.src, target).len())
                .max()
                .unwrap_or(0);
            for a in &skips {
                let reason = a.reason.as_deref().unwrap_or("");
                let name = with_dir_suffix(display_rel(&a.src, target), reason);
                println!("  {name:<width$}  {reason}");
            }
        } else {
            println!(
                "  {} other entr{}",
                skips.len(),
                if skips.len() == 1 { "y" } else { "ies" }
            );
        }
    }

    println!();
    println!("No changes made.");
    if !trashes.is_empty() {
        println!(
            "Run with --apply to send {} file{} to Trash.",
            trashes.len(),
            plural(trashes.len())
        );
    }
}

pub fn clean_apply_result(
    path: &str,
    actions: &[Action],
    outcomes: &[ActionResult],
    hist_id: &str,
) {
    println!("Sift clean");
    println!("{path}");
    println!();
    render_apply_outcome_summary(actions, outcomes, hist_id);
}

// ------------------------------------------------------------------ scan

fn entry_type_label(e: &Entry) -> &'static str {
    if e.is_symlink {
        "LINK"
    } else if e.is_dir {
        "DIR"
    } else {
        "FILE"
    }
}

fn entry_type_color(e: &Entry) -> &'static str {
    if e.is_symlink {
        "33" // yellow
    } else if e.is_dir {
        "36" // cyan
    } else {
        "37" // default/white
    }
}

fn entry_flags(e: &Entry) -> String {
    let mut flags = Vec::new();
    if e.project_root {
        flags.push("project root");
    } else if e.protected {
        flags.push("protected");
    }
    if e.hidden {
        flags.push("hidden");
    }
    if flags.is_empty() {
        String::new()
    } else {
        format!("  ({})", flags.join(", "))
    }
}

pub fn scan(path: &str, entries: &[Entry], recursive: bool) {
    if recursive {
        return scan_recursive(path, entries);
    }
    let target = Path::new(path);
    let dirs = entries.iter().filter(|e| e.is_dir).count();
    let symlinks = entries.iter().filter(|e| e.is_symlink).count();
    let files = entries.len() - dirs - symlinks;

    println!("Sift scan");
    println!("{path}");
    println!();
    println!(
        "{} entries · {dirs} dir{}, {files} file{}, {symlinks} symlink{}",
        entries.len(),
        plural(dirs),
        plural(files),
        plural(symlinks)
    );
    if entries.is_empty() {
        return;
    }
    println!();

    let color = use_color();
    let width = entries
        .iter()
        .map(|e| display_rel(&e.path, target).len())
        .max()
        .unwrap_or(0);
    for e in entries {
        let label = colorize(
            &format!("{:<4}", entry_type_label(e)),
            entry_type_color(e),
            color,
        );
        let size = e.size.map(human_size).unwrap_or_else(|| "-".to_string());
        println!(
            "  {label}  {:<width$}  {size:>8}{}",
            display_rel(&e.path, target),
            entry_flags(e)
        );
    }
}

/// A flat, `scan_entries_recursive`-shaped list already separates traversal
/// boundaries from visible entries via their own fields, so this splits
/// that data purely for display: everything visited normally, then the
/// directories where traversal stopped and why.
fn scan_recursive(path: &str, entries: &[Entry]) {
    let target = Path::new(path);
    let mut protected: Vec<(&Entry, &'static str)> = Vec::new();
    let mut visible: Vec<&Entry> = Vec::new();
    for e in entries {
        // A directory symlink is never `is_dir` under no-follow metadata, but
        // it's still a traversal boundary worth reporting alongside real
        // protected directories rather than in the plain entry list.
        if e.is_dir || e.is_symlink {
            if let Some(reason) = crate::scanner::traversal_reason(e) {
                protected.push((e, reason));
                continue;
            }
        }
        visible.push(e);
    }

    println!("Sift scan");
    println!("{path}");
    println!();
    println!("Recursive scan");
    println!();
    if visible.is_empty() {
        println!("  (nothing found)");
    } else {
        for e in &visible {
            let rel = display_rel(&e.path, target);
            let rel = if e.is_dir { format!("{rel}/") } else { rel };
            println!("  {rel}");
        }
    }

    if !protected.is_empty() {
        println!();
        println!("Protected");
        let width = protected
            .iter()
            .map(|(e, _)| display_rel(&e.path, target).len() + 1)
            .max()
            .unwrap_or(0);
        for (e, reason) in &protected {
            let name = format!("{}/", display_rel(&e.path, target));
            println!("  {name:<width$}  {reason}");
        }
    }
}

// ---------------------------------------------------------------- doctor

pub fn doctor(path: &str, findings: &[DoctorFinding]) {
    println!("Sift doctor");
    println!("{path}");
    println!();
    if findings.is_empty() {
        println!("No issues found.");
        return;
    }
    println!("Issues found");
    println!();

    let target = Path::new(path);
    let color = use_color();
    for f in findings {
        let is_dir = std::fs::symlink_metadata(&f.path)
            .map(|m| m.is_dir())
            .unwrap_or(false);
        let name = display_rel(&f.path, target);
        let name = if is_dir { format!("{name}/") } else { name };
        println!("  {} {name}", colorize("⚠", "33", color));
        println!("    {}", f.reason);
        println!();
    }
    println!("{} finding{}", findings.len(), plural(findings.len()));
}

// --------------------------------------------------------------- history

pub fn history(items: &[HistoryItem]) {
    println!("History");
    println!();
    if items.is_empty() {
        println!("No operations recorded yet.");
        return;
    }
    let mut sorted: Vec<&HistoryItem> = items.iter().collect();
    sorted.sort_by_key(|item| std::cmp::Reverse(item.timestamp));

    for item in sorted {
        let moved = count_ok(&item.outcomes, Op::Move);
        let moved_dirs = count_ok(&item.outcomes, Op::MoveDir);
        let created = count_ok(&item.outcomes, Op::CreateDir);
        let trashed = count_ok(&item.outcomes, Op::Trash);
        let skipped = item.outcomes.iter().filter(|o| o.op == Op::Skip).count();
        let failed = item.outcomes.iter().filter(|o| o.result.is_err()).count();

        let mut parts = Vec::new();
        if moved > 0 {
            let label = if item.kind == "undo" {
                "restored"
            } else {
                "moved"
            };
            parts.push(format!("{moved} {label}"));
        }
        if moved_dirs > 0 {
            let label = if item.kind == "undo" {
                "folders restored"
            } else {
                "folders moved"
            };
            parts.push(format!("{moved_dirs} {label}"));
        }
        if created > 0 {
            parts.push(format!("{created} directories"));
        }
        if trashed > 0 {
            parts.push(format!("{trashed} trashed"));
        }
        if skipped > 0 {
            parts.push(format!("{skipped} skipped"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        let breakdown = if parts.is_empty() {
            "no actions".to_string()
        } else {
            parts.join(" · ")
        };
        let kind = if item.kind.is_empty() {
            "operation"
        } else {
            item.kind.as_str()
        };
        let origin = match (item.origin.as_str(), &item.watch_root) {
            ("watch", Some(root)) => format!(" · watch · {}", root.display()),
            ("watch", None) => " · watch".to_string(),
            _ => String::new(),
        };

        println!("  {}", item.id);
        println!("  {}", format_timestamp(item.timestamp));
        println!("  {kind} · {breakdown}{origin}");
        println!();
    }
    println!("To undo one of these:");
    println!("  sift undo <id>");
}

// ------------------------------------------------------------------ undo

pub fn undo_not_found(id: &str) {
    println!("Sift undo {id}");
    println!();
    println!(
        "{} No history record found for {id}",
        colorize("✗", "31", use_color())
    );
}

pub fn undo_parse_error(id: &str) {
    println!("Sift undo {id}");
    println!();
    println!(
        "{} Could not read that history record (it may be corrupted)",
        colorize("✗", "31", use_color())
    );
}

pub fn undo_result(id: &str, restored: usize, refused: &[ActionResult], trash_skipped: usize) {
    println!("Sift undo {id}");
    println!();
    let color = use_color();
    if restored == 0 && refused.is_empty() && trash_skipped == 0 {
        println!("Nothing to undo.");
    } else if refused.is_empty() {
        println!(
            "{} {restored} item{} restored",
            colorize("✓", "32", color),
            plural(restored)
        );
    } else {
        let symbol = if restored > 0 { "⚠" } else { "✗" };
        let symbol_color = if restored > 0 { "33" } else { "31" };
        println!(
            "{} {restored} restored, {} refused",
            colorize(symbol, symbol_color, color),
            refused.len()
        );
    }

    if !refused.is_empty() {
        println!();
        println!("Refused");
        for o in refused {
            if let Err(ref e) = o.result {
                println!("  {}  {e}", o.src.display());
            }
        }
    }

    if trash_skipped > 0 {
        println!();
        println!(
            "{trash_skipped} trashed file{} can't be undone here — restore from your system Trash if needed.",
            plural(trash_skipped)
        );
    }
}

// ---------------------------------------------------------- config check

#[derive(serde::Serialize)]
struct ConfigCheckJson {
    valid: bool,
    source: Option<String>,
    version: Option<i64>,
    strategy: Option<String>,
    unknown: Option<String>,
    template: Option<String>,
    date_source: Option<String>,
    stability_seconds: Option<u64>,
    rules: Option<usize>,
    error: Option<String>,
}

pub fn config_check_json(result: &Result<crate::config::EffectivePolicy, String>) -> String {
    let payload = match result {
        Ok(p) => ConfigCheckJson {
            valid: true,
            source: Some(p.source.describe()),
            version: Some(p.version),
            strategy: Some(p.strategy.as_str().to_string()),
            unknown: Some(p.unknown_policy.as_str().to_string()),
            template: p
                .template
                .as_ref()
                .map(|t| t.raw().to_string())
                .or_else(|| p.metadata_template.as_ref().map(|t| t.raw().to_string())),
            date_source: p.date_source.map(|d| d.as_str().to_string()),
            stability_seconds: Some(p.stability.as_secs()),
            rules: Some(p.rules.len()),
            error: None,
        },
        Err(e) => ConfigCheckJson {
            valid: false,
            source: None,
            version: None,
            strategy: None,
            unknown: None,
            template: None,
            date_source: None,
            stability_seconds: None,
            rules: None,
            error: Some(e.clone()),
        },
    };
    serde_json::to_string_pretty(&payload).unwrap()
}

pub fn config_check(path: &str, result: &Result<crate::config::EffectivePolicy, String>) {
    use crate::config::OrganizeStrategy;
    let color = use_color();
    println!("Sift config");
    println!("{path}");
    println!();
    match result {
        Ok(policy) => {
            println!("{} Configuration valid", colorize("✓", "32", color));
            println!();
            println!("Source");
            println!("  {}", policy.source.describe());
            println!();
            println!("Version");
            println!("  {}", policy.version);
            println!();
            println!("Organization");
            println!("  strategy     {}", policy.strategy.as_str());
            match policy.strategy {
                OrganizeStrategy::Type => {
                    println!("  unknown      {}", policy.unknown_policy.as_str());
                }
                OrganizeStrategy::Date => {
                    println!(
                        "  date source  {}",
                        policy
                            .date_source
                            .expect("validated: Date always has a date_source")
                            .as_str()
                    );
                    println!(
                        "  template     {}",
                        policy
                            .template
                            .as_ref()
                            .expect("validated: Date always has a template")
                            .raw()
                    );
                }
                OrganizeStrategy::Audio
                | OrganizeStrategy::Video
                | OrganizeStrategy::Photos
                | OrganizeStrategy::Documents => {
                    println!(
                        "  template     {}",
                        policy
                            .metadata_template
                            .as_ref()
                            .expect(
                                "validated: Audio/Video/Photos/Documents always has a metadata_template"
                            )
                            .raw()
                    );
                    println!("  recursive    not supported yet");
                }
            }
            println!();
            println!("Watch");
            println!("  stability    {}s", policy.stability.as_secs());
            println!();
            println!("Rules");
            println!("  {}", policy.rules.len());
        }
        Err(e) => {
            println!("{} Invalid configuration", colorize("✗", "31", color));
            println!();
            for line in e.lines() {
                println!("  {line}");
            }
        }
    }
    println!();
    println!("No filesystem changes were made.");
}

// -------------------------------------------------------------- explain

pub fn explain_json(exp: &crate::explain::Explanation) -> String {
    serde_json::to_string_pretty(exp).unwrap()
}

pub fn explain(exp: &crate::explain::Explanation) {
    use crate::config::OrganizeStrategy;
    use crate::domain::Op;

    println!("Sift explain");
    println!(
        "{}",
        exp.file
            .file_name()
            .map(|n| n.display().to_string())
            .unwrap_or_else(|| exp.file.display().to_string())
    );
    println!();
    println!("Policy");
    println!("  {}", exp.source.describe());
    println!();
    println!("Strategy");
    if exp.matched_rule.is_some() {
        println!(
            "  {} (not used because rule matched)",
            exp.strategy.as_str()
        );
    } else {
        println!("  {}", exp.strategy.as_str());
    }
    println!();

    match &exp.matched_rule {
        Some((pattern, action, destination)) => {
            println!("Rule");
            match destination {
                Some(d) => println!("  {pattern} → {action} {d}/"),
                None => println!("  {pattern} → {action}"),
            }
        }
        None => {
            println!("Rules");
            println!("  no matching rule");
        }
    }
    println!();

    // Only report strategy-specific evidence when a rule didn't already
    // short-circuit it — matches `classify_for_organize`/`classify_for_date`,
    // which never even reach classification/date-metadata once a rule
    // matches.
    if exp.matched_rule.is_some() {
        // Nothing to add: the Rule section above already explains the
        // decision fully.
    } else {
        match exp.strategy {
            OrganizeStrategy::Type => match (&exp.classification, &exp.unknown_fallback) {
                (Some(_), Some(policy)) => {
                    println!("Classification");
                    println!("  unknown");
                    println!();
                    println!("Policy (unknown)");
                    println!("  unknown → {}", policy.as_str());
                }
                (Some(cat), None) => {
                    let cat = *cat;
                    println!("Classification");
                    let ext = exp
                        .file
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| format!(".{e}"))
                        .unwrap_or_else(|| "(no extension)".to_string());
                    println!("  {ext} → {cat:?}");
                }
                (None, _) => {
                    println!("Classification");
                    println!("  n/a");
                }
            },
            OrganizeStrategy::Date => {
                println!("Metadata");
                match &exp.date_metadata {
                    Some(meta) => {
                        println!(
                            "  modified     {:04}-{:02}-{:02}",
                            meta.year, meta.month, meta.day
                        );
                        println!("  year         {:04}", meta.year);
                        println!("  month        {:02}", meta.month);
                        println!("  day          {:02}", meta.day);
                    }
                    None => println!("  unavailable"),
                }
                println!();
                println!("Template");
                println!("  {}", exp.template.as_deref().unwrap_or("(none)"));
            }
            OrganizeStrategy::Audio => {
                println!("Metadata");
                match &exp.audio_metadata {
                    Some(meta) => {
                        println!(
                            "  title        {}",
                            meta.title.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  artist       {}",
                            meta.artist.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  album        {}",
                            meta.album.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  album artist {}",
                            meta.album_artist.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  genre        {}",
                            meta.genre.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  year         {}",
                            meta.year.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  track        {}",
                            meta.track.as_deref().unwrap_or("(none)")
                        );
                    }
                    None => println!("  unavailable"),
                }
                println!();
                println!("Template");
                println!("  {}", exp.template.as_deref().unwrap_or("(none)"));
            }
            OrganizeStrategy::Video => {
                println!("Metadata");
                match &exp.video_metadata {
                    Some(meta) => {
                        println!("  resolution   {}x{}", meta.width, meta.height);
                        println!(
                            "  codec        {}",
                            meta.codec.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  year         {}",
                            meta.year.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  duration     {}",
                            meta.duration_seconds
                                .map(|d| format!("{d}s"))
                                .unwrap_or_else(|| "(none, requires ffprobe)".to_string())
                        );
                        println!(
                            "  fps          {}",
                            meta.fps
                                .map(|f| f.to_string())
                                .unwrap_or_else(|| "(none, requires ffprobe)".to_string())
                        );
                    }
                    None => println!("  unavailable"),
                }
                println!();
                println!("Template");
                println!("  {}", exp.template.as_deref().unwrap_or("(none)"));
            }
            OrganizeStrategy::Photos => {
                println!("Metadata");
                match &exp.photo_metadata {
                    Some(meta) => {
                        println!(
                            "  camera       {}",
                            meta.camera.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  year         {}",
                            meta.year.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  month        {}",
                            meta.month.as_deref().unwrap_or("(none)")
                        );
                        println!("  day          {}", meta.day.as_deref().unwrap_or("(none)"));
                    }
                    None => println!("  unavailable"),
                }
                println!();
                println!("Template");
                println!("  {}", exp.template.as_deref().unwrap_or("(none)"));
            }
            OrganizeStrategy::Documents => {
                println!("Metadata");
                match &exp.document_metadata {
                    Some(meta) => {
                        println!(
                            "  author       {}",
                            meta.author.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  title        {}",
                            meta.title.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  year         {}",
                            meta.year.as_deref().unwrap_or("(none)")
                        );
                        println!(
                            "  month        {}",
                            meta.month.as_deref().unwrap_or("(none)")
                        );
                        println!("  day          {}", meta.day.as_deref().unwrap_or("(none)"));
                    }
                    None => println!("  unavailable"),
                }
                println!();
                println!("Template");
                println!("  {}", exp.template.as_deref().unwrap_or("(none)"));
            }
        }
    }
    println!();

    println!("Destination");
    match &exp.destination {
        Some(dest) => println!("  {}", dest.display()),
        None => println!("  (none)"),
    }
    println!();

    println!("Decision");
    println!(
        "  {}",
        match exp.op {
            Op::Move => "MOVE",
            Op::Trash => "TRASH",
            Op::Skip => "SKIP",
            Op::CreateDir => "CREATEDIR",
            Op::MoveDir => "MOVEDIR",
        }
    );
    println!("  {}", exp.reason);
    println!();

    println!("Safety");
    let color = use_color();
    for check in &exp.checks {
        let mark = if check.ok {
            colorize("✓", "32", color)
        } else {
            colorize("✗", "31", color)
        };
        match &check.detail {
            Some(detail) => println!("  {mark} {} — {detail}", check.label),
            None => println!("  {mark} {}", check.label),
        }
    }

    println!();
    println!("No changes were made.");
}

// --------------------------------------------------------------- folders

#[derive(serde::Serialize)]
struct FoldersJson<'a> {
    root: &'a Path,
    folders: &'a [FolderCandidate],
    possible_duplicates: &'a [Vec<String>],
    duplicate_removals: &'a [crate::folders::DuplicateRemoval],
}

/// `sift folders --json`: the full, unfiltered analysis (every candidate,
/// whatever its decision), the report-only possible-duplicate groups, and
/// (only when `--remove-duplicates` was requested) the content-verified
/// removals planned. Bypasses every function below this — the JSON shape
/// has no dependency on how the human-readable view is worded.
pub fn folders_json(fp: &FoldersPlan, dup_removals: &[crate::folders::DuplicateRemoval]) -> String {
    let payload = FoldersJson {
        root: &fp.root,
        folders: &fp.candidates,
        possible_duplicates: &fp.possible_duplicates,
        duplicate_removals: dup_removals,
    };
    serde_json::to_string_pretty(&payload).unwrap()
}

fn folder_dest_label(cat: crate::domain::Category) -> &'static str {
    crate::planner::builtin_destination(cat)
        .map(|(name, _)| name)
        .unwrap_or("?")
}

pub fn folders_dry_run(fp: &FoldersPlan, dup_removals: &[crate::folders::DuplicateRemoval]) {
    println!("Sift folders");
    println!("{}", fp.root.display());

    let mut moves: Vec<&FolderCandidate> = Vec::new();
    let mut refused: Vec<&FolderCandidate> = Vec::new();
    let mut suggests: Vec<&FolderCandidate> = Vec::new();
    let mut protects: Vec<&FolderCandidate> = Vec::new();
    let mut leaves: Vec<&FolderCandidate> = Vec::new();
    for c in &fp.candidates {
        match &c.decision {
            Decision::Move { .. } => moves.push(c),
            Decision::MoveRefused { .. } => refused.push(c),
            Decision::Suggest { .. } => suggests.push(c),
            Decision::Protect { .. } => protects.push(c),
            Decision::Leave { .. } => leaves.push(c),
        }
    }

    println!();
    println!("Plan");
    println!("  {} folder{} to move", moves.len(), plural(moves.len()));
    if !refused.is_empty() {
        println!("  {} move{} refused", refused.len(), plural(refused.len()));
    }
    println!("  {} suggestion{}", suggests.len(), plural(suggests.len()));
    println!("  {} protected", protects.len());
    println!("  {} uncertain", leaves.len());

    if !moves.is_empty() {
        println!();
        println!("Move");
        let width = moves.iter().map(|c| c.name.len() + 1).max().unwrap_or(0);
        for c in &moves {
            if let Decision::Move {
                category,
                confidence,
            } = &c.decision
            {
                let name = format!("{}/", c.name);
                println!(
                    "  {name:<width$}  → {}/   {:.0}%  ({} file{})",
                    folder_dest_label(*category),
                    confidence * 100.0,
                    c.evidence_files,
                    plural(c.evidence_files)
                );
            }
        }
    }

    if !refused.is_empty() {
        println!();
        println!("Move refused");
        let width = refused.iter().map(|c| c.name.len() + 1).max().unwrap_or(0);
        for c in &refused {
            if let Decision::MoveRefused {
                category, reason, ..
            } = &c.decision
            {
                let name = format!("{}/", c.name);
                println!(
                    "  {name:<width$}  → {}/   {reason}",
                    folder_dest_label(*category)
                );
            }
        }
    }

    if !suggests.is_empty() {
        println!();
        println!("Suggestions");
        let width = suggests.iter().map(|c| c.name.len() + 1).max().unwrap_or(0);
        for c in &suggests {
            if let Decision::Suggest {
                category,
                confidence,
                reason,
            } = &c.decision
            {
                let name = format!("{}/", c.name);
                println!(
                    "  {name:<width$}  → {}/   {:.0}%  {reason} ({} file{})",
                    folder_dest_label(*category),
                    confidence * 100.0,
                    c.evidence_files,
                    plural(c.evidence_files)
                );
            }
        }
    }

    if !protects.is_empty() {
        println!();
        println!("Protected");
        let width = protects.iter().map(|c| c.name.len() + 1).max().unwrap_or(0);
        for c in &protects {
            if let Decision::Protect { reason } = &c.decision {
                let name = format!("{}/", c.name);
                println!("  {name:<width$}  {reason}");
            }
        }
    }

    if !leaves.is_empty() {
        println!();
        println!("Uncertain");
        let width = leaves.iter().map(|c| c.name.len() + 1).max().unwrap_or(0);
        for c in &leaves {
            if let Decision::Leave { reason } = &c.decision {
                let name = format!("{}/", c.name);
                println!("  {name:<width$}  {reason}");
            }
        }
    }

    if !fp.possible_duplicates.is_empty() {
        println!();
        println!("Possible duplicates (report only — never merged or moved)");
        for group in &fp.possible_duplicates {
            println!("  {}", group.join(", "));
        }
    }

    if !dup_removals.is_empty() {
        println!();
        println!("Duplicate files (identical content — will be sent to Trash, not undoable via sift undo)");
        let target = fp.root.as_path();
        let width = dup_removals
            .iter()
            .map(|r| display_rel(&r.remove, target).len())
            .max()
            .unwrap_or(0);
        for r in dup_removals {
            println!(
                "  {:<width$}  = {}  ({})",
                display_rel(&r.remove, target),
                display_rel(&r.keep, target),
                human_size(r.size)
            );
        }
    }

    println!();
    if moves.is_empty() && dup_removals.is_empty() {
        println!("No changes made.");
    } else {
        println!("No changes made.");
        let mut todo = Vec::new();
        if !moves.is_empty() {
            todo.push(format!(
                "move {} folder{}",
                moves.len(),
                plural(moves.len())
            ));
        }
        if !dup_removals.is_empty() {
            todo.push(format!(
                "remove {} duplicate file{}",
                dup_removals.len(),
                plural(dup_removals.len())
            ));
        }
        println!("Run with --apply to {}.", todo.join(" and "));
    }
}

pub fn folders_apply_result(outcomes: &[ActionResult], hist_id: &str) {
    let color = use_color();
    let moved = count_ok(outcomes, Op::MoveDir);
    let created = count_ok(outcomes, Op::CreateDir);
    let trashed = count_ok(outcomes, Op::Trash);
    let skipped = outcomes.iter().filter(|o| o.op == Op::Skip).count();
    let failed = outcomes.iter().filter(|o| o.result.is_err()).count();

    println!("Sift folders");
    println!();
    if failed == 0 {
        println!("{} Applied successfully", colorize("✓", "32", color));
    } else {
        println!(
            "{} Applied with {failed} failure{}",
            colorize("⚠", "33", color),
            plural(failed)
        );
    }
    println!();
    println!("  {moved} folder{} moved", plural(moved));
    if created > 0 {
        println!(
            "  {created} categor{} created",
            if created == 1 { "y" } else { "ies" }
        );
    }
    if trashed > 0 {
        println!(
            "  {trashed} duplicate file{} sent to Trash",
            plural(trashed)
        );
    }
    println!(
        "  {skipped} entr{} left untouched",
        if skipped == 1 { "y" } else { "ies" }
    );
    println!("  {failed} failure{}", plural(failed));

    if failed > 0 {
        println!();
        println!("Failures");
        for o in outcomes.iter().filter(|o| o.result.is_err()) {
            if let Err(ref e) = o.result {
                println!("  {}  {e}", o.src.display());
            }
        }
    }

    println!();
    println!("History");
    println!("  {hist_id}");

    if outcomes.iter().any(|o| o.undoable) {
        println!();
        println!("Undo");
        println!("  sift undo {hist_id}");
    } else if moved > 0 {
        println!();
        println!("Folder moves are not undoable in this version.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_identical_duplicate_reason() {
        let reason = format!(
            "{}/tmp/Documents/foo.pdf",
            crate::planner::IDENTICAL_DUPLICATE_REASON_PREFIX
        );
        assert!(is_duplicate_collision_reason(&reason));
    }

    #[test]
    fn recognizes_renamed_collision_reason() {
        let reason = format!(
            "Document{}",
            crate::planner::RENAMED_COLLISION_REASON_SUFFIX
        );
        assert!(is_duplicate_collision_reason(&reason));
    }

    #[test]
    fn does_not_flag_an_ordinary_reason() {
        assert!(!is_duplicate_collision_reason("collision"));
        assert!(!is_duplicate_collision_reason("hidden file"));
        assert!(!is_duplicate_collision_reason("built-in junk file"));
    }
}
