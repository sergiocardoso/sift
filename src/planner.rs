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
/// (Unknown, Junk, BuildOutput, Sensitive) return `None`.
fn builtin_destination(cat: Category) -> Option<(&'static str, &'static str)> {
    match cat {
        Category::Document => Some(("Documents", "Document")),
        Category::Image => Some(("Images", "Image")),
        Category::Video => Some(("Video", "Video")),
        Category::Audio => Some(("Audio", "Audio")),
        Category::Archive => Some(("Archives", "Archive")),
        Category::ThreeD => Some(("3D", "3D asset")),
        Category::Junk | Category::BuildOutput | Category::Sensitive | Category::Unknown => None,
    }
}

fn is_builtin_junk(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ["tmp", "swp", "swo"].contains(&ext))
        .unwrap_or(false)
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
    if entry.is_dir || entry.is_symlink || entry.protected || entry.hidden {
        return Decision::Skip("Directory/symlink/protected/hidden".into());
    }
    for rule in rules {
        if !glob_match(&rule.pattern, &entry.path) {
            continue;
        }
        let reason = rule
            .description
            .clone()
            .unwrap_or_else(|| format!("Config rule: {}", rule.name));
        return match rule.action.as_str() {
            "Skip" => Decision::Skip(reason),
            "Trash" => Decision::Trash(reason),
            "Move" => {
                let dest = match rule.destination.as_deref() {
                    Some(d) => d,
                    None => return Decision::Skip("Move rule missing destination".into()),
                };
                if !validate_rule_destination(dest) {
                    return Decision::Skip("Unsafe config destination rejected".into());
                }
                match safe_join_under(target, Path::new(dest)) {
                    Some(dest_dir) => Decision::Move { dest_dir, reason },
                    None => Decision::Skip("Unsafe config destination rejected".into()),
                }
            }
            _ => Decision::Skip("Invalid rule config".into()),
        };
    }
    let mut classified = entry.clone();
    builtin.classify(&mut classified);
    match classified.classified_as {
        Some(Category::Junk) => Decision::Trash("Built-in junk file".into()),
        Some(cat) => match builtin_destination(cat) {
            Some((name, reason)) => Decision::Move {
                dest_dir: target.join(name),
                reason: reason.into(),
            },
            None => Decision::Skip("Unclassified/unknown".into()),
        },
        None => Decision::Skip("Unclassified/unknown".into()),
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
            DirStatus::Blocked => {
                skip_with_intended_dst(dest_dir, "Destination directory occupied by non-directory")
            }
            status => {
                let Some(filename) = entry.path.file_name() else {
                    return skip("Entry has no file name");
                };
                let dest = dest_dir.join(filename);
                if std::fs::symlink_metadata(&dest).is_ok() {
                    skip_with_intended_dst(dest, "Destination exists (collision)")
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

pub fn plan_organize(path: &str, config_rules: &[Rule], builtin: &CategoryDB) -> Plan {
    let target = Path::new(path);
    let entries = scan_entries(target);
    if is_project_root(target) {
        return skip_all(&entries, "Target is project root");
    }
    let rules = rules_by_priority(config_rules);
    let mut needed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let entry_actions: Vec<Action> = entries
        .iter()
        .map(|entry| resolve_organize_action(entry, target, &rules, builtin, &mut needed_dirs))
        .collect();
    let mut actions: Vec<Action> = needed_dirs
        .into_iter()
        .map(|dir| Action {
            src: dir,
            dst: None,
            op: Op::CreateDir,
            reason: Some("Ensure directory exists".into()),
            undoable: false,
        })
        .collect();
    actions.extend(entry_actions);
    Plan { actions }
}

fn resolve_clean_action(entry: &Entry, rules: &[&Rule]) -> Action {
    if entry.is_dir || entry.is_symlink || entry.protected || entry.hidden {
        return Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Skip,
            reason: Some("Directory/symlink/protected/hidden".into()),
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
            .unwrap_or_else(|| format!("Config rule: {}", rule.name));
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
                reason: Some("Config rule specifies Move (not applicable to clean)".into()),
                undoable: false,
            },
            _ => Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some("Invalid rule config".into()),
                undoable: false,
            },
        };
    }
    if is_builtin_junk(&entry.path) {
        Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Trash,
            reason: Some("Built-in junk file".into()),
            undoable: false,
        }
    } else {
        Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Skip,
            reason: Some("Not trash candidate".into()),
            undoable: false,
        }
    }
}

pub fn plan_clean(path: &str, config_rules: &[Rule], _builtin: &CategoryDB) -> Plan {
    let target = Path::new(path);
    let entries = scan_entries(target);
    if is_project_root(target) {
        return skip_all(&entries, "Target is project root");
    }
    let rules = rules_by_priority(config_rules);
    let actions = entries
        .iter()
        .map(|entry| resolve_clean_action(entry, &rules))
        .collect();
    Plan { actions }
}

pub fn cmd_organize(path: String, apply: bool, json: bool) {
    let cfg_path = crate::config::find_config(&path);
    let config = cfg_path
        .and_then(|p| load_config(&p).ok())
        .unwrap_or_default();
    let builtins = CategoryDB::default();
    let plan = plan_organize(&path, &config.rules, &builtins);
    if json {
        println!("{}", serde_json::to_string_pretty(&plan).unwrap());
    } else {
        println!("Plan: {} actions", plan.actions.len());
        for (i, a) in plan.actions.iter().enumerate() {
            let dst = a
                .dst
                .as_ref()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "-".to_string());
            println!(
                "{:2}: {:?} {} -> {} | reason: {}",
                i + 1,
                a.op,
                a.src.display(),
                dst,
                a.reason.clone().unwrap_or_default()
            );
        }
    }
    if apply {
        crate::executor::execute_plan(plan, &path);
    }
}

pub fn cmd_clean(path: String, apply: bool, json: bool) {
    let cfg_path = crate::config::find_config(&path);
    let config = cfg_path
        .and_then(|p| load_config(&p).ok())
        .unwrap_or_default();
    let builtins = CategoryDB::default();
    let plan = plan_clean(&path, &config.rules, &builtins);
    if json {
        println!("{}", serde_json::to_string_pretty(&plan).unwrap());
    } else {
        println!("Clean plan: {} actions", plan.actions.len());
        for (i, a) in plan.actions.iter().enumerate() {
            println!(
                "{:2}: {:?} {} | reason: {}",
                i + 1,
                a.op,
                a.src.display(),
                a.reason.clone().unwrap_or_default()
            );
        }
    }
    if apply {
        crate::executor::execute_plan(plan, &path);
    }
}
