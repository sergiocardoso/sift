//! `sift folders`: analyzes a directory's *immediate child folders* and
//! safely suggests moving whole folders into Sift's category directories,
//! based on the types of files they contain.
//!
//! This is deliberately distinct from `sift organize --recursive`, which
//! organizes *files inside* each eligible directory but never moves a
//! directory itself. `folders` never dismantles a candidate: the entire
//! subtree travels together, in one `MoveDir` action, or not at all.
//!
//! Classification reuses the existing `CategoryDB` (no separate extension
//! database) and destination names reuse `planner::builtin_destination`
//! (no second taxonomy) — `Other/` is explicitly never a folder
//! destination. Everything here is metadata/filename-based and read-only
//! until `--apply`; no file contents are ever read, no network calls, no
//! AI.

use crate::classifier::CategoryDB;
use crate::domain::{Action, Category, Op, Plan};
use crate::planner::{builtin_destination, createdir_actions, dir_status, DirStatus};
use crate::scanner::{self, scan_entries};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Conservative bounds so `sift folders /` (or any huge tree) can't turn
/// into an unbounded scan. Hitting either limit marks the profile
/// `truncated`, which can never upgrade a decision to `Move`.
pub const MAX_PROFILE_FILES: usize = 5000;
pub const MAX_PROFILE_DEPTH: usize = 8;

const HIGH_CONFIDENCE: f64 = 0.80;
const MEDIUM_CONFIDENCE: f64 = 0.60;

/// Read-only, metadata-only summary of what's inside a candidate folder.
/// Built once per candidate; never mutated, never used to justify a mutation
/// by itself (see `decide`).
#[derive(Debug, Default, Clone)]
pub struct FolderProfile {
    /// Count of eligible regular files per *known* category.
    pub category_counts: HashMap<Category, usize>,
    /// Eligible regular files that classified as Junk/Other/Unknown/etc —
    /// they count toward the evidence total but never toward a specific
    /// destination category.
    pub other_count: usize,
    /// Whether the candidate's own root, or any descendant that scanning
    /// didn't stop short of, is a recognized software project root.
    pub contains_project: bool,
    /// True as soon as either bound (`MAX_PROFILE_FILES`/`MAX_PROFILE_DEPTH`)
    /// is hit — the profile may be incomplete.
    pub truncated: bool,
    /// Whether the candidate directory had literally zero entries (used to
    /// distinguish "empty folder" from "folder full of stuff we can't
    /// classify").
    pub saw_any_entry: bool,
}

impl FolderProfile {
    pub fn total_eligible(&self) -> usize {
        self.category_counts.values().sum::<usize>() + self.other_count
    }

    pub fn dominant(&self) -> Option<(Category, usize)> {
        self.category_counts
            .iter()
            .max_by_key(|(_, count)| **count)
            .map(|(cat, count)| (*cat, *count))
    }
}

/// Safely profiles `root`'s subtree using no-follow metadata only, with an
/// explicit stack (never uncontrolled recursion) and deterministic
/// (sorted) traversal order. Read-only: never mutates. Never follows
/// symlinks and never descends into a nested project root, a hidden
/// directory, a build-output directory, or one of Sift's own category
/// directories — matching the same boundaries `organize --recursive` uses.
pub fn profile_folder(root: &Path) -> FolderProfile {
    let builtin = CategoryDB::default();
    let mut profile = FolderProfile::default();
    let mut visited = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    'outer: while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
        children.sort();
        if !children.is_empty() {
            profile.saw_any_entry = true;
        }

        for path in children {
            if visited >= MAX_PROFILE_FILES {
                profile.truncated = true;
                break 'outer;
            }
            let Some(entry) = scanner::describe_path(&path) else {
                continue;
            };
            visited += 1;

            if entry.is_symlink {
                // Never follow, never use as classification evidence.
                continue;
            }

            if entry.is_dir {
                if entry.hidden {
                    continue;
                }
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if scanner::is_sift_category_dir(name) {
                    continue;
                }
                if scanner::BUILD_OUTPUT_DIR_NAMES.contains(&name) {
                    continue;
                }
                if scanner::is_project_root(&path) {
                    // The whole candidate is protected regardless of file
                    // evidence; don't descend into the project subtree.
                    profile.contains_project = true;
                    continue;
                }
                if depth + 1 > MAX_PROFILE_DEPTH {
                    profile.truncated = true;
                    continue;
                }
                stack.push((path, depth + 1));
                continue;
            }

            // Ordinary regular file.
            if entry.hidden {
                continue;
            }
            let mut classified = entry.clone();
            builtin.classify(&mut classified);
            match classified.classified_as {
                Some(Category::Junk)
                | Some(Category::Other)
                | Some(Category::Unknown)
                | Some(Category::BuildOutput)
                | Some(Category::Sensitive)
                | None => profile.other_count += 1,
                Some(cat) => {
                    *profile.category_counts.entry(cat).or_insert(0) += 1;
                }
            }
        }
    }

    profile
}

/// The outcome of analyzing one candidate folder.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    /// High-confidence classification; eligible for `--apply`.
    Move { category: Category, confidence: f64 },
    /// A `Move`-eligible decision that could not be planned safely (e.g. a
    /// destination collision) — reported, never executed, never
    /// undoable.
    MoveRefused {
        category: Category,
        confidence: f64,
        reason: &'static str,
    },
    /// Medium-confidence classification: report only, never auto-applied.
    Suggest {
        category: Category,
        confidence: f64,
        reason: &'static str,
    },
    /// Insufficient/mixed/empty/unknown evidence: do nothing.
    Leave { reason: &'static str },
    /// A deliberate safety boundary: never moved, ever.
    Protect { reason: &'static str },
}

impl Decision {
    pub fn category(&self) -> Option<Category> {
        match self {
            Decision::Move { category, .. }
            | Decision::MoveRefused { category, .. }
            | Decision::Suggest { category, .. } => Some(*category),
            _ => None,
        }
    }

    pub fn confidence(&self) -> Option<f64> {
        match self {
            Decision::Move { confidence, .. }
            | Decision::MoveRefused { confidence, .. }
            | Decision::Suggest { confidence, .. } => Some(*confidence),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Decision::Move { .. } => "move",
            Decision::MoveRefused { .. } => "move_refused",
            Decision::Suggest { .. } => "suggest",
            Decision::Leave { .. } => "leave",
            Decision::Protect { .. } => "protect",
        }
    }

    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Decision::MoveRefused { reason, .. }
            | Decision::Suggest { reason, .. }
            | Decision::Leave { reason }
            | Decision::Protect { reason } => Some(reason),
            Decision::Move { .. } => None,
        }
    }
}

/// The deterministic, explainable scoring model. A folder's name is never
/// used as evidence here — only the classified contents of the files
/// inside it.
fn decide(profile: &FolderProfile, is_project_root: bool, hidden: bool) -> Decision {
    if hidden {
        return Decision::Protect {
            reason: "hidden directory",
        };
    }
    if is_project_root {
        return Decision::Protect {
            reason: "software project",
        };
    }
    if profile.contains_project {
        return Decision::Protect {
            reason: "contains software project",
        };
    }

    let total = profile.total_eligible();
    if total == 0 {
        return Decision::Leave {
            reason: if profile.saw_any_entry {
                "insufficient evidence"
            } else {
                "empty folder"
            },
        };
    }

    let Some((category, count)) = profile.dominant() else {
        return Decision::Leave {
            reason: "unknown content",
        };
    };
    let confidence = count as f64 / total as f64;

    // A single piece of evidence is never enough to auto-move, even at
    // 100% confidence — it may just be a folder that happens to hold one
    // file so far.
    if total == 1 {
        return Decision::Suggest {
            category,
            confidence,
            reason: "only one evidence file",
        };
    }

    if confidence >= HIGH_CONFIDENCE && !profile.truncated {
        return Decision::Move {
            category,
            confidence,
        };
    }
    if confidence >= HIGH_CONFIDENCE {
        // truncated: safety wins over confidence.
        return Decision::Suggest {
            category,
            confidence,
            reason: "profile truncated",
        };
    }
    if confidence >= MEDIUM_CONFIDENCE {
        return Decision::Suggest {
            category,
            confidence,
            reason: "medium confidence",
        };
    }
    Decision::Leave {
        reason: "mixed content",
    }
}

/// The destination directory name for a folder-move category. Reuses
/// `planner::builtin_destination` (the single source of truth for
/// category → directory name) but — unlike file organizing — explicitly
/// never returns `Other`: an uncertain *folder* is left alone, never
/// dumped into a catch-all.
fn folder_destination_name(cat: Category) -> Option<&'static str> {
    if matches!(cat, Category::Other) {
        return None;
    }
    builtin_destination(cat).map(|(name, _)| name)
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderCandidate {
    pub name: String,
    pub path: PathBuf,
    #[serde(flatten)]
    pub decision: Decision,
    pub evidence_files: usize,
    pub truncated: bool,
}

pub struct FoldersPlan {
    pub root: PathBuf,
    /// `CreateDir` + `MoveDir` actions only — safe to feed straight to
    /// `executor::execute_plan`.
    pub actions: Vec<Action>,
    /// Every analyzed candidate, for the human/JSON report (including
    /// ones with no action, like `Leave`/`Protect`/`Suggest`).
    pub candidates: Vec<FolderCandidate>,
    /// Conservative, name-only "possible duplicate" groups. Report-only;
    /// never affects `actions`.
    pub possible_duplicates: Vec<Vec<String>>,
}

/// Conservative, name-only "possible duplicate" folder detector. Never
/// hashes or reads contents, never claims true duplication, never merges
/// or deletes anything — purely a report-only hint based on common naming
/// patterns (`(2)`, `copy`, `old`, `backup`, ...).
pub fn detect_possible_duplicates(names: &[String]) -> Vec<Vec<String>> {
    fn normalize(name: &str) -> String {
        let mut s = name.trim().to_ascii_lowercase();
        loop {
            let before = s.clone();
            for suffix in [
                " (1)", " (2)", " (3)", " (4)", " (5)", " (6)", " (7)", " (8)", " (9)",
            ] {
                if let Some(stripped) = s.strip_suffix(suffix) {
                    s = stripped.to_string();
                }
            }
            for suffix in [
                " copy 2", " copy 3", " copy", "-copy", "_copy", " old", "-old", "_old", " backup",
                "-backup", "_backup",
            ] {
                if let Some(stripped) = s.strip_suffix(suffix) {
                    s = stripped.to_string();
                }
            }
            s = s.trim_end().to_string();
            if s == before {
                break;
            }
        }
        s
    }

    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for name in names {
        groups
            .entry(normalize(name))
            .or_default()
            .push(name.clone());
    }
    let mut result: Vec<Vec<String>> = groups
        .into_values()
        .filter(|group| group.len() > 1)
        .collect();
    for group in &mut result {
        group.sort();
    }
    result.sort();
    result
}

// -------------------------------------------------------- duplicate files

/// One planned duplicate-file removal, activated only by
/// `--remove-duplicates`. `remove` is verified byte-for-byte identical to
/// `keep` — never removed on a hash match alone. Scope is deliberately
/// narrow: only files inside folders that already share a name-based
/// `possible_duplicates` group are ever compared, never the whole tree,
/// and the alphabetically-first folder in each group is always the keeper
/// — its files are never removed.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateRemoval {
    pub keep: PathBuf,
    pub remove: PathBuf,
    pub size: u64,
}

/// Safely lists every eligible regular file under `root` for duplicate
/// comparison: no symlinks followed, no hidden files/dirs, no descent into
/// a nested project root or one of Sift's own category directories,
/// bounded by the same `MAX_PROFILE_FILES`/`MAX_PROFILE_DEPTH` limits as
/// `profile_folder`. Read-only.
fn collect_files_for_dedupe(root: &Path) -> Vec<(PathBuf, u64)> {
    let mut files = Vec::new();
    let mut visited = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    'outer: while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
        children.sort();

        for path in children {
            if visited >= MAX_PROFILE_FILES {
                break 'outer;
            }
            let Ok(md) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            visited += 1;
            if md.file_type().is_symlink() {
                continue;
            }
            let hidden = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with('.'))
                .unwrap_or(false);
            if hidden {
                continue;
            }
            if md.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if scanner::is_sift_category_dir(name)
                    || scanner::BUILD_OUTPUT_DIR_NAMES.contains(&name)
                {
                    continue;
                }
                if scanner::is_project_root(&path) {
                    // Never use a nested project's files as dedupe evidence.
                    continue;
                }
                if depth + 1 > MAX_PROFILE_DEPTH {
                    continue;
                }
                stack.push((path, depth + 1));
                continue;
            }
            if md.is_file() {
                files.push((path, md.len()));
            }
        }
    }

    files
}

/// A fast, non-cryptographic content hash, used only to narrow candidates
/// before the mandatory byte-for-byte confirmation below — never trusted
/// alone as proof of equality.
fn hash_file(path: &Path) -> Option<u64> {
    use std::hash::Hasher;
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.write(&buf[..n]);
    }
    Some(hasher.finish())
}

/// The actual, authoritative equality check: streamed byte-for-byte
/// comparison. This — not the hash above — is what a removal decision is
/// ultimately based on, so a hash collision can never cause a real removal.
fn files_byte_identical(a: &Path, b: &Path) -> bool {
    use std::io::Read;
    let (Ok(mut fa), Ok(mut fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
        return false;
    };
    let mut buf_a = [0u8; 65536];
    let mut buf_b = [0u8; 65536];
    loop {
        let (Ok(na), Ok(nb)) = (fa.read(&mut buf_a), fb.read(&mut buf_b)) else {
            return false;
        };
        if na != nb {
            return false;
        }
        if na == 0 {
            return true;
        }
        if buf_a[..na] != buf_b[..nb] {
            return false;
        }
    }
}

/// Plans safe, content-verified duplicate-file removals. Only files inside
/// folders that already share a name-based `possible_duplicates` group are
/// ever compared. Within a group, the alphabetically-first folder is
/// always the keeper: only files in the *other* folder(s) that are
/// byte-for-byte identical to some file in the keeper are ever proposed
/// for removal — a file with no match in the keeper is left alone, even
/// sitting inside a "(2)"-suffixed folder. Never touches a folder that is
/// itself protected (a project root, or containing one). Removal always
/// means `Op::Trash` (recoverable via the OS trash, never a permanent
/// delete) and is never undoable via `sift undo`, exactly like every other
/// trashed file in Sift.
pub fn plan_duplicate_removals(
    root: &Path,
    groups: &[Vec<String>],
    candidates: &[FolderCandidate],
) -> (Vec<Action>, Vec<DuplicateRemoval>) {
    let mut actions = Vec::new();
    let mut removals = Vec::new();

    let is_protected = |name: &str| {
        candidates
            .iter()
            .find(|c| c.name == name)
            .map(|c| matches!(c.decision, Decision::Protect { .. }))
            .unwrap_or(false)
    };

    for group in groups {
        if group.len() < 2 {
            continue;
        }
        let mut sorted_group = group.clone();
        sorted_group.sort();
        let primary_name = &sorted_group[0];
        if is_protected(primary_name) {
            continue;
        }
        let primary_dir = root.join(primary_name);
        let primary_files = collect_files_for_dedupe(&primary_dir);
        let mut primary_by_hash: HashMap<u64, Vec<(PathBuf, u64)>> = HashMap::new();
        for (path, size) in &primary_files {
            if let Some(h) = hash_file(path) {
                primary_by_hash
                    .entry(h)
                    .or_default()
                    .push((path.clone(), *size));
            }
        }

        for other_name in &sorted_group[1..] {
            if is_protected(other_name) {
                continue;
            }
            let other_dir = root.join(other_name);
            for (path, size) in collect_files_for_dedupe(&other_dir) {
                let Some(h) = hash_file(&path) else {
                    continue;
                };
                let Some(same_hash) = primary_by_hash.get(&h) else {
                    continue;
                };
                let Some((keep, _)) = same_hash
                    .iter()
                    .find(|(keep_path, keep_size)| {
                        *keep_size == size && files_byte_identical(keep_path, &path)
                    })
                    .cloned()
                else {
                    continue;
                };
                actions.push(Action {
                    src: path.clone(),
                    dst: None,
                    op: Op::Trash,
                    reason: Some(format!("duplicate of {}", keep.display())),
                    undoable: false,
                });
                removals.push(DuplicateRemoval {
                    keep,
                    remove: path,
                    size,
                });
            }
        }
    }

    (actions, removals)
}

/// Prepends duplicate-file removals ahead of the rest of the plan: file
/// removals inside a folder must always run before that folder is
/// potentially moved whole by a later `MoveDir` action, never after.
pub fn combine_with_duplicate_removals(plan: Plan, dup_actions: Vec<Action>) -> Plan {
    let mut actions = dup_actions;
    actions.extend(plan.actions);
    Plan { actions }
}

/// Analyzes `path`'s immediate child directories and builds a safe
/// `MoveDir`/`CreateDir` plan for every high-confidence candidate.
/// Candidate *selection* is one level only — profiling a candidate's
/// contents may look deeper, but only the top folder itself is ever an
/// action target. Reuses `CategoryDB` and `builtin_destination`; no
/// separate classification logic.
pub fn plan_folders(path: &str) -> FoldersPlan {
    let root = Path::new(path);
    let entries = scan_entries(root);
    let mut candidates = Vec::new();
    let mut needed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let mut move_actions: Vec<Action> = Vec::new();
    let mut candidate_names: Vec<String> = Vec::new();

    for entry in &entries {
        if !entry.is_dir {
            // Not a directory (includes symlinks, which are never `is_dir`
            // under no-follow metadata) — never a folder candidate.
            continue;
        }
        let name = match entry.path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if scanner::is_sift_category_dir(&name) {
            // Sift's own category directories are never candidates.
            continue;
        }

        candidate_names.push(name.clone());
        let profile = profile_folder(&entry.path);
        let mut decision = decide(&profile, entry.project_root, entry.hidden);

        if let Decision::Move {
            category,
            confidence,
        } = decision
        {
            let dest_dir_name = folder_destination_name(category)
                .expect("a Move decision always names a real, non-Other category");
            let dest_dir = root.join(dest_dir_name);
            let dest = dest_dir.join(&name);
            match dir_status(&dest_dir) {
                DirStatus::Blocked => {
                    decision = Decision::MoveRefused {
                        category,
                        confidence,
                        reason: "destination category is not a directory",
                    };
                }
                status => {
                    if std::fs::symlink_metadata(&dest).is_ok() {
                        decision = Decision::MoveRefused {
                            category,
                            confidence,
                            reason: "destination already exists",
                        };
                    } else {
                        if matches!(status, DirStatus::Missing) {
                            needed_dirs.insert(dest_dir.clone());
                        }
                        move_actions.push(Action {
                            src: entry.path.clone(),
                            dst: Some(dest),
                            op: Op::MoveDir,
                            reason: Some(format!("mostly {dest_dir_name}")),
                            undoable: true,
                        });
                    }
                }
            }
        }

        candidates.push(FolderCandidate {
            name,
            path: entry.path.clone(),
            decision,
            evidence_files: profile.total_eligible(),
            truncated: profile.truncated,
        });
    }

    candidates.sort_by(|a, b| a.name.cmp(&b.name));
    let mut actions = createdir_actions(needed_dirs);
    actions.extend(move_actions);

    FoldersPlan {
        root: root.to_path_buf(),
        actions,
        candidates,
        possible_duplicates: detect_possible_duplicates(&candidate_names),
    }
}

/// Builds the *full* action list (including `Skip` for every
/// non-moved candidate) for execution, so one `sift folders --apply` run
/// produces exactly one, fully self-describing history record — mirroring
/// how `organize` already represents skipped entries alongside moves.
pub fn full_execution_plan(fp: &FoldersPlan) -> Plan {
    let mut actions: Vec<Action> = fp
        .actions
        .iter()
        .filter(|a| a.op == Op::CreateDir)
        .cloned()
        .collect();
    for candidate in &fp.candidates {
        match &candidate.decision {
            Decision::Move { .. } => {
                if let Some(mv) = fp
                    .actions
                    .iter()
                    .find(|a| a.op == Op::MoveDir && a.src == candidate.path)
                {
                    actions.push(mv.clone());
                }
            }
            other => actions.push(Action {
                src: candidate.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some(skip_reason_text(other)),
                undoable: false,
            }),
        }
    }
    Plan { actions }
}

fn skip_reason_text(decision: &Decision) -> String {
    match decision {
        Decision::MoveRefused { reason, .. } => reason.to_string(),
        Decision::Suggest { reason, .. } => reason.to_string(),
        Decision::Leave { reason } => reason.to_string(),
        Decision::Protect { reason } => reason.to_string(),
        Decision::Move { .. } => unreachable!("Move is handled separately"),
    }
}

pub fn cmd_folders(path: String, apply: bool, json: bool, remove_duplicates: bool) -> bool {
    let fp = plan_folders(&path);
    let (dup_actions, dup_removals) = if remove_duplicates {
        plan_duplicate_removals(Path::new(&path), &fp.possible_duplicates, &fp.candidates)
    } else {
        (Vec::new(), Vec::new())
    };

    if json {
        println!("{}", crate::render::folders_json(&fp, &dup_removals));
    } else if !apply {
        crate::render::folders_dry_run(&fp, &dup_removals);
    }

    if !apply {
        return true;
    }
    let plan = combine_with_duplicate_removals(full_execution_plan(&fp), dup_actions);
    let (hist_id, outcomes) = crate::executor::execute_plan(plan, &path, "folders", None);
    let ok = !outcomes.iter().any(|o| o.result.is_err());
    if !json {
        crate::render::folders_apply_result(&outcomes, &hist_id);
    }
    ok
}
