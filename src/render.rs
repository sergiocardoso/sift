//! Human-readable ("pretty") output for every Sift command.
//!
//! Nothing here decides what happens on disk — it only describes, after the
//! fact, what a `Plan` or a set of `ActionResult`s means to a person. All
//! `--json` output bypasses this module entirely and is untouched by it.

use crate::domain::{Action, ActionResult, Entry, HistoryItem, Op};
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
/// external date dependency. Implements the well-known `civil_from_days`
/// algorithm (Howard Hinnant), which is exact for the proleptic Gregorian
/// calendar over the full `i64` range; only non-negative input is expected
/// here since timestamps come from `SystemTime::now()`.
pub fn format_timestamp(secs: u64) -> String {
    let secs = secs as i64;
    let days = secs / 86400;
    let rem = secs % 86400;
    let (hour, minute) = (rem / 3600, (rem % 3600) / 60);

    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

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
    for a in trashes {
        println!("  {}", display_rel(&a.src, target));
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

/// Reports what execution actually did (never assumes the plan succeeded),
/// shared by `organize --apply` and `clean --apply`.
fn render_apply_outcome_summary(outcomes: &[ActionResult], hist_id: &str) {
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

    println!();
    println!("History");
    println!("  {hist_id}");

    if outcomes.iter().any(|o| o.undoable) {
        println!();
        println!("Undo");
        println!("  sift undo {hist_id}");
    }
}

pub fn organize_apply_result(path: &str, outcomes: &[ActionResult], hist_id: &str) {
    println!("Sift organize");
    println!("{path}");
    println!();
    render_apply_outcome_summary(outcomes, hist_id);
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

pub fn clean_apply_result(path: &str, outcomes: &[ActionResult], hist_id: &str) {
    println!("Sift clean");
    println!("{path}");
    println!();
    render_apply_outcome_summary(outcomes, hist_id);
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
            "{} {restored} file{} restored",
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
