use crate::classifier::CategoryDB;
use crate::config::{
    load_config, rules_by_priority, safe_join_under, validate_rule_destination, Rule,
};
use crate::domain::{Action, Category, Entry, Op, Plan};
use crate::scanner::{is_project_root, scan_entries};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Result of classifying a single entry for `organize`, before collision
/// checking against the real filesystem.
enum Decision {
    Skip(String),
    Trash(String),
    Move { dest_dir: PathBuf, reason: String },
}

enum DirStatus {
    Exists,
    Missing,
    Blocked,
}

fn dir_status(dir: &Path) -> DirStatus {
    match std::fs::symlink_metadata(dir) {
        Ok(md) if md.is_dir() => DirStatus::Exists,
        Ok(_) => DirStatus::Blocked,
        Err(_) => DirStatus::Missing,
    }
}

/// Built-in destination directory name and human-readable reason for a
/// classified category. Categories with no built-in organize destination
/// (Junk is trashed, not moved; Unknown/BuildOutput/Sensitive are never
/// produced for an ordinary file organize actually classifies — see
/// `CategoryDB::classify`) return `None`.
fn builtin_destination(cat: Category) -> Option<(&'static str, &'static str)> {
    match cat {
        Category::Document => Some(("Documents", "Document")),
        Category::Image => Some(("Images", "Image")),
        Category::Video => Some(("Video", "Video")),
        Category::Audio => Some(("Audio", "Audio")),
        Category::Archive => Some(("Archives", "Archive")),
        Category::ThreeD => Some(("3D", "3D asset")),
        Category::Code => Some(("Code", "Code")),
        Category::Data => Some(("Data", "Data")),
        Category::Other => Some(("Other", "Other")),
        Category::Junk | Category::BuildOutput | Category::Sensitive | Category::Unknown => None,
    }
}

fn is_builtin_junk(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ["tmp", "swp", "swo"].contains(&ext))
        .unwrap_or(false)
}

/// Canonical, short skip reason for an entry that's categorically excluded
/// from mutation (never a rule/collision reason — those are decided later).
/// These exact strings are what the human-readable renderer keys off of to
/// group/label skips, so keep them short and stable. Delegates the shared
/// symlink/project/protected/hidden condition to `scanner::protection_reason`
/// (also used by recursive traversal eligibility) and adds the one case
/// specific to organize/clean: a plain directory is never itself moved.
fn categorical_skip_reason(entry: &Entry) -> Option<&'static str> {
    crate::scanner::protection_reason(entry).or(if entry.is_dir {
        Some("directory")
    } else {
        None
    })
}

fn glob_match(pattern: &str, path: &Path) -> bool {
    let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if pattern.contains('*') {
        let needle = pattern.replace('*', "");
        fname.contains(&needle)
    } else {
        fname == pattern
    }
}

fn skip_all(entries: &[Entry], reason: &str) -> Plan {
    Plan {
        actions: entries
            .iter()
            .map(|e| Action {
                src: e.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some(reason.to_string()),
                undoable: false,
            })
            .collect(),
    }
}

fn classify_for_organize(
    entry: &Entry,
    target: &Path,
    rules: &[&Rule],
    builtin: &CategoryDB,
) -> Decision {
    if let Some(reason) = categorical_skip_reason(entry) {
        return Decision::Skip(reason.into());
    }
    for rule in rules {
        if !glob_match(&rule.pattern, &entry.path) {
            continue;
        }
        let reason = rule
            .description
            .clone()
            .unwrap_or_else(|| format!("config rule: {}", rule.name));
        return match rule.action.as_str() {
            "Skip" => Decision::Skip(reason),
            "Trash" => Decision::Trash(reason),
            "Move" => {
                let dest = match rule.destination.as_deref() {
                    Some(d) => d,
                    None => return Decision::Skip("move rule missing destination".into()),
                };
                if !validate_rule_destination(dest) {
                    return Decision::Skip("unsafe config destination".into());
                }
                match safe_join_under(target, Path::new(dest)) {
                    Some(dest_dir) => Decision::Move { dest_dir, reason },
                    None => Decision::Skip("unsafe config destination".into()),
                }
            }
            _ => Decision::Skip("invalid rule config".into()),
        };
    }
    let mut classified = entry.clone();
    builtin.classify(&mut classified);
    match classified.classified_as {
        Some(Category::Junk) => Decision::Trash("built-in junk file".into()),
        Some(cat) => match builtin_destination(cat) {
            Some((name, reason)) => Decision::Move {
                dest_dir: target.join(name),
                reason: reason.into(),
            },
            None => Decision::Skip("unknown type".into()),
        },
        None => Decision::Skip("unknown type".into()),
    }
}

/// Resolves one entry's final action, checking real filesystem collisions
/// and recording any directory that still needs `Op::CreateDir`.
fn resolve_organize_action(
    entry: &Entry,
    target: &Path,
    rules: &[&Rule],
    builtin: &CategoryDB,
    needed_dirs: &mut BTreeSet<PathBuf>,
) -> Action {
    let skip = |reason: &str| Action {
        src: entry.path.clone(),
        dst: None,
        op: Op::Skip,
        reason: Some(reason.to_string()),
        undoable: false,
    };
    // Collision skips keep the intended destination visible in the plan so
    // the user can see what blocked the move, even though it never happens.
    let skip_with_intended_dst = |dest: PathBuf, reason: &str| Action {
        src: entry.path.clone(),
        dst: Some(dest),
        op: Op::Skip,
        reason: Some(reason.to_string()),
        undoable: false,
    };
    match classify_for_organize(entry, target, rules, builtin) {
        Decision::Skip(reason) => Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Skip,
            reason: Some(reason),
            undoable: false,
        },
        Decision::Trash(reason) => Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Trash,
            reason: Some(reason),
            undoable: false,
        },
        Decision::Move { dest_dir, reason } => match dir_status(&dest_dir) {
            DirStatus::Blocked => skip_with_intended_dst(dest_dir, "collision"),
            status => {
                let Some(filename) = entry.path.file_name() else {
                    return skip("entry has no file name");
                };
                let dest = dest_dir.join(filename);
                if std::fs::symlink_metadata(&dest).is_ok() {
                    skip_with_intended_dst(dest, "collision")
                } else {
                    if matches!(status, DirStatus::Missing) {
                        needed_dirs.insert(dest_dir.clone());
                    }
                    Action {
                        src: entry.path.clone(),
                        dst: Some(dest),
                        op: Op::Move,
                        reason: Some(reason),
                        undoable: true,
                    }
                }
            }
        },
    }
}

/// One entry's organize plan: the `CreateDir` its destination needs (if
/// any, and if not already a real directory), and the entry's own action.
pub struct EntryPlan {
    pub create_dir: Option<Action>,
    pub action: Action,
}

/// Plans the organize action for exactly one already-described entry
/// within `containing_dir`, reusing the identical per-entry logic manual
/// `organize` uses (config precedence, built-in classification, the
/// Code/Data/Other fallback, collision detection via live
/// `symlink_metadata`, explicit `CreateDir`). This is watch's sole
/// authority for turning one filesystem candidate into a plan — it must
/// never duplicate that decision logic itself.
///
/// Safety notes for callers: this only decides *what* to do; it does not
/// re-validate that `entry`/`containing_dir` are still safe against the
/// live filesystem (see `scanner::revalidate_candidate` for that), and it
/// does not execute anything (see `executor::execute_plan`, which
/// independently re-validates immediately before mutating).
pub fn plan_entry_organize(
    entry: &Entry,
    containing_dir: &Path,
    config_rules: &[Rule],
    builtin: &CategoryDB,
) -> EntryPlan {
    let rules = rules_by_priority(config_rules);
    let mut needed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let action = resolve_organize_action(entry, containing_dir, &rules, builtin, &mut needed_dirs);
    let create_dir = needed_dirs.into_iter().next().map(|dir| Action {
        src: dir,
        dst: None,
        op: Op::CreateDir,
        reason: Some("ensure directory exists".into()),
        undoable: false,
    });
    EntryPlan { create_dir, action }
}

pub fn plan_organize(path: &str, config_rules: &[Rule], builtin: &CategoryDB) -> Plan {
    let target = Path::new(path);
    let entries = scan_entries(target);
    if is_project_root(target) {
        return skip_all(&entries, "target is a software project root");
    }
    let rules = rules_by_priority(config_rules);
    let mut needed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let entry_actions: Vec<Action> = entries
        .iter()
        .map(|entry| resolve_organize_action(entry, target, &rules, builtin, &mut needed_dirs))
        .collect();
    let mut actions = createdir_actions(needed_dirs);
    actions.extend(entry_actions);
    Plan { actions }
}

fn createdir_actions(needed_dirs: BTreeSet<PathBuf>) -> Vec<Action> {
    needed_dirs
        .into_iter()
        .map(|dir| Action {
            src: dir,
            dst: None,
            op: Op::CreateDir,
            reason: Some("ensure directory exists".into()),
            undoable: false,
        })
        .collect()
}

/// Result of planning a recursive organize: the flattened, multi-directory
/// plan plus how many directories were actually scanned (a plain directory
/// eligible for descent produces zero actions of its own, so this can't be
/// recovered from the plan alone — it's needed for the "N directories
/// scanned" summary in the human-readable view).
pub struct RecursivePlan {
    pub plan: Plan,
    pub dirs_scanned: usize,
}

/// Organizes every eligible directory under `path` *in place*: each
/// directory is its own local organize context (its files are classified
/// and moved into local `Documents/`, `Images/`, etc. subfolders), never
/// flattened into `path` itself. Discovery (`discover_recursive_dirs`) is a
/// read-only snapshot taken up front — a directory this plan creates is
/// never itself treated as a new traversal target.
pub fn plan_organize_recursive(
    path: &str,
    config_rules: &[Rule],
    builtin: &CategoryDB,
) -> RecursivePlan {
    let root = Path::new(path);
    if is_project_root(root) {
        return RecursivePlan {
            plan: skip_all(&scan_entries(root), "target is a software project root"),
            dirs_scanned: 0,
        };
    }
    let dirs = crate::scanner::discover_recursive_dirs(root);
    let rules = rules_by_priority(config_rules);
    let mut needed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let mut entry_actions: Vec<Action> = Vec::new();

    for dir in &dirs {
        for entry in scan_entries(dir) {
            if entry.is_dir {
                // Eligible subdirectories are transparent: they're in
                // `dirs` and get their own iteration of this same loop.
                // Only report a directory here if traversal stopped at it.
                if let Some(reason) = crate::scanner::traversal_reason(&entry) {
                    entry_actions.push(Action {
                        src: entry.path.clone(),
                        dst: None,
                        op: Op::Skip,
                        reason: Some(reason.into()),
                        undoable: false,
                    });
                }
                continue;
            }
            entry_actions.push(resolve_organize_action(
                &entry,
                dir,
                &rules,
                builtin,
                &mut needed_dirs,
            ));
        }
    }

    let mut actions = createdir_actions(needed_dirs);
    actions.extend(entry_actions);
    RecursivePlan {
        plan: Plan { actions },
        dirs_scanned: dirs.len(),
    }
}

fn resolve_clean_action(entry: &Entry, rules: &[&Rule]) -> Action {
    if let Some(reason) = categorical_skip_reason(entry) {
        return Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Skip,
            reason: Some(reason.into()),
            undoable: false,
        };
    }
    for rule in rules {
        if !glob_match(&rule.pattern, &entry.path) {
            continue;
        }
        let reason = rule
            .description
            .clone()
            .unwrap_or_else(|| format!("config rule: {}", rule.name));
        return match rule.action.as_str() {
            "Trash" => Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Trash,
                reason: Some(reason),
                undoable: false,
            },
            "Skip" => Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some(reason),
                undoable: false,
            },
            "Move" => Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some("move rule not applicable to clean".into()),
                undoable: false,
            },
            _ => Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some("invalid rule config".into()),
                undoable: false,
            },
        };
    }
    if is_builtin_junk(&entry.path) {
        Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Trash,
            reason: Some("built-in junk file".into()),
            undoable: false,
        }
    } else {
        Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Skip,
            reason: Some("not trash candidate".into()),
            undoable: false,
        }
    }
}

pub fn plan_clean(path: &str, config_rules: &[Rule], _builtin: &CategoryDB) -> Plan {
    let target = Path::new(path);
    let entries = scan_entries(target);
    if is_project_root(target) {
        return skip_all(&entries, "target is a software project root");
    }
    let rules = rules_by_priority(config_rules);
    let actions = entries
        .iter()
        .map(|entry| resolve_clean_action(entry, &rules))
        .collect();
    Plan { actions }
}

pub fn cmd_organize(path: String, apply: bool, json: bool, verbose: bool, recursive: bool) -> bool {
    let cfg_path = crate::config::find_config(&path);
    let config = cfg_path
        .and_then(|p| load_config(&p).ok())
        .unwrap_or_default();
    let builtins = CategoryDB::default();
    let root = is_project_root(Path::new(&path));

    let (plan, dirs_scanned) = if recursive {
        let recursive_plan = plan_organize_recursive(&path, &config.rules, &builtins);
        (recursive_plan.plan, Some(recursive_plan.dirs_scanned))
    } else {
        (plan_organize(&path, &config.rules, &builtins), None)
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&plan).unwrap());
    } else if !apply {
        match dirs_scanned {
            Some(n) => {
                crate::render::organize_dry_run_recursive(&path, &plan.actions, n, root, verbose)
            }
            None => crate::render::organize_dry_run(&path, &plan.actions, root, verbose),
        }
    }

    if !apply {
        return true;
    }
    let kind = if recursive {
        "organize-recursive"
    } else {
        "organize"
    };
    let (hist_id, outcomes) = crate::executor::execute_plan(plan, &path, kind, None);
    let ok = !outcomes.iter().any(|o| o.result.is_err());
    if !json {
        crate::render::organize_apply_result(&path, &outcomes, &hist_id);
    }
    ok
}

pub fn cmd_clean(path: String, apply: bool, json: bool, verbose: bool) -> bool {
    let cfg_path = crate::config::find_config(&path);
    let config = cfg_path
        .and_then(|p| load_config(&p).ok())
        .unwrap_or_default();
    let builtins = CategoryDB::default();
    let plan = plan_clean(&path, &config.rules, &builtins);
    let root = is_project_root(Path::new(&path));

    if json {
        println!("{}", serde_json::to_string_pretty(&plan).unwrap());
    } else if !apply {
        crate::render::clean_dry_run(&path, &plan.actions, root, verbose);
    }

    if !apply {
        return true;
    }
    let (hist_id, outcomes) = crate::executor::execute_plan(plan, &path, "clean", None);
    let ok = !outcomes.iter().any(|o| o.result.is_err());
    if !json {
        crate::render::clean_apply_result(&path, &outcomes, &hist_id);
    }
    ok
}
