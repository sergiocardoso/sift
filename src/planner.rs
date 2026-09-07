use crate::classifier::CategoryDB;
use crate::config::{load_config, Rule};
use crate::domain::{Action, Category, Op, Plan};
use crate::scanner::scan_entries;
use std::path::Path;

pub fn plan_organize(path: &str, config_rules: &[Rule], builtin: &CategoryDB) -> Plan {
    let target = Path::new(path);
    let mut entries = scan_entries(target);
    let mut actions = vec![];
    for entry in entries.iter_mut() {
        // Built-in: skip directory, symlink, protected, unknown
        if entry.is_dir || entry.is_symlink || entry.protected {
            actions.push(Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some("Directory/symlink/protected".into()),
                undoable: false,
            });
            continue;
        }
        // Config exact rule takes priority
        let mut applied_rule = None;
        for rule in config_rules.iter().filter(|r| r.enabled) {
            if glob_match(&rule.pattern, &entry.path) {
                match rule.action.as_str() {
                    "Skip" => actions.push(Action {
                        src: entry.path.clone(),
                        dst: None,
                        op: Op::Skip,
                        reason: rule.description.clone(),
                        undoable: false,
                    }),
                    "Trash" => actions.push(Action {
                        src: entry.path.clone(),
                        dst: None,
                        op: Op::Trash,
                        reason: rule.description.clone(),
                        undoable: false,
                    }),
                    "Move" if rule.destination.is_some() => actions.push(Action {
                        src: entry.path.clone(),
                        dst: Some(target.join(rule.destination.as_ref().unwrap())),
                        op: Op::Move,
                        reason: rule.description.clone(),
                        undoable: true,
                    }),
                    _ => actions.push(Action {
                        src: entry.path.clone(),
                        dst: None,
                        op: Op::Skip,
                        reason: Some("Invalid rule config".into()),
                        undoable: false,
                    }),
                }
                applied_rule = Some(1);
                break;
            }
        }
        if applied_rule.is_some() {
            continue;
        }
        // Built-in classifier
        builtin.classify(entry);
        match entry.classified_as {
            Some(Category::Unknown) | None => actions.push(Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some("Unclassified/unknown".into()),
                undoable: false,
            }),
            Some(Category::Junk) => actions.push(Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Trash,
                reason: Some("Built-in junk file".into()),
                undoable: false,
            }),
            Some(Category::ThreeD) => actions.push(Action {
                src: entry.path.clone(),
                dst: Some(target.join("3D")),
                op: Op::Move,
                reason: Some("3D asset".into()),
                undoable: true,
            }),
            Some(Category::Archive) => actions.push(Action {
                src: entry.path.clone(),
                dst: Some(target.join("Archives")),
                op: Op::Move,
                reason: Some("Archive".into()),
                undoable: true,
            }),
            Some(Category::Document) => actions.push(Action {
                src: entry.path.clone(),
                dst: Some(target.join("Documents")),
                op: Op::Move,
                reason: Some("Document".into()),
                undoable: true,
            }),
            Some(Category::Image) => actions.push(Action {
                src: entry.path.clone(),
                dst: Some(target.join("Images")),
                op: Op::Move,
                reason: Some("Image".into()),
                undoable: true,
            }),
            Some(Category::Video) => actions.push(Action {
                src: entry.path.clone(),
                dst: Some(target.join("Video")),
                op: Op::Move,
                reason: Some("Video".into()),
                undoable: true,
            }),
            Some(Category::Audio) => actions.push(Action {
                src: entry.path.clone(),
                dst: Some(target.join("Audio")),
                op: Op::Move,
                reason: Some("Audio".into()),
                undoable: true,
            }),
            _ => actions.push(Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some("Uncategorized".into()),
                undoable: false,
            }),
        }
    }
    Plan { actions }
}

pub fn plan_clean(path: &str, config_rules: &[Rule], builtin: &CategoryDB) -> Plan {
    let target = Path::new(path);
    let mut entries = scan_entries(target);
    let mut actions = vec![];
    for entry in entries.iter_mut() {
        if entry.is_dir || entry.is_symlink || entry.protected {
            actions.push(Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some("Directory/symlink/protected".into()),
                undoable: false,
            });
            continue;
        }
        // Config Trash rule strictly only
        let mut applied = false;
        for rule in config_rules
            .iter()
            .filter(|r| r.enabled && r.action.as_str() == "Trash")
        {
            if glob_match(&rule.pattern, &entry.path) {
                actions.push(Action {
                    src: entry.path.clone(),
                    dst: None,
                    op: Op::Trash,
                    reason: rule.description.clone(),
                    undoable: false,
                });
                applied = true;
                break;
            }
        }
        if applied {
            continue;
        }
        // Built-in junk
        builtin.classify(entry);
        if matches!(entry.classified_as, Some(Category::Junk)) {
            actions.push(Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Trash,
                reason: Some("Built-in junk file".into()),
                undoable: false,
            });
        } else {
            actions.push(Action {
                src: entry.path.clone(),
                dst: None,
                op: Op::Skip,
                reason: Some("Not trash candidate".into()),
                undoable: false,
            });
        }
    }
    Plan { actions }
}

fn glob_match(pattern: &str, path: &std::path::Path) -> bool {
    if pattern.contains('*') {
        // crude: only *.ext or *foo*
        let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let pattern_ = pattern.replace("*", "");
        fname.contains(&pattern_)
    } else {
        path.file_name().and_then(|s| s.to_str()) == Some(pattern)
    }
}

pub fn cmd_organize(path: String, apply: bool, json: bool) {
    let cfg_path = crate::config::find_config(&path);
    let config = cfg_path
        .and_then(|p| load_config(&p).ok())
        .unwrap_or_default();
    let builtins = CategoryDB::new();
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
    let builtins = CategoryDB::new();
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
