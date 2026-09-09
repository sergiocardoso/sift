//! `sift explain <file>`: a read-only, single-file report of exactly what
//! `sift organize` would do to it and why, under the effective policy for
//! `--root`.
//!
//! Reuses the same primitives real planning uses
//! (`config::resolve_policy`, `planner::classify_for_organize`,
//! `planner::dir_status`, `fs::ancestors_are_safe`) — there is no second
//! classifier and no second rule engine here, only a different, more
//! granular way of reporting the same decision. Performs zero filesystem
//! mutation: no `CreateDir`, no `Move`, no `Trash`, no history entry.

use crate::classifier::CategoryDB;
use crate::config::{
    rules_by_priority, DateMetadata, EffectivePolicy, OrganizeStrategy, PolicySource,
};
use crate::domain::{Category, Op};
use crate::fs::ancestors_are_safe;
use crate::planner::{
    classify_for_date, classify_for_organize, destination_levels, dir_status, Decision,
    DecisionCause, DirStatus,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct SafetyCheck {
    pub label: &'static str,
    pub ok: bool,
    pub detail: Option<String>,
}

#[derive(Serialize)]
pub struct Explanation {
    pub file: PathBuf,
    pub root: PathBuf,
    pub source: PolicySource,
    pub strategy: OrganizeStrategy,
    /// The raw `organize.template` string, when `strategy = "date"` (for
    /// display; not used for anything, since rendering already happened
    /// inside `classify_for_date`).
    pub template: Option<String>,
    /// `Some((pattern, action, destination))` if an explicit `[[rules]]`
    /// entry is what decided this file.
    pub matched_rule: Option<MatchedRule>,
    /// The classifier's category, when `strategy = "type"` is what
    /// decided this file (rule wins and categorical protection both leave
    /// this `None`).
    pub classification: Option<Category>,
    /// `Some(policy)` when `organize.unknown` is what actually decided the
    /// outcome (as opposed to a specific classified category).
    pub unknown_fallback: Option<crate::config::UnknownPolicy>,
    /// `Some` when `strategy = "date"` is what decided this file (rule
    /// wins and categorical protection both leave this `None`).
    pub date_metadata: Option<DateMetadata>,
    pub op: Op,
    pub reason: String,
    pub destination: Option<PathBuf>,
    pub checks: Vec<SafetyCheck>,
}

/// `(pattern, action, destination)` for a matched `[[rules]]` entry.
type MatchedRule = (String, String, Option<String>);

#[derive(Default)]
struct DecisionCauseSummary {
    matched_rule: Option<MatchedRule>,
    classification: Option<Category>,
    unknown_fallback: Option<crate::config::UnknownPolicy>,
    date_metadata: Option<DateMetadata>,
}

fn decision_cause_summary(cause: &DecisionCause) -> DecisionCauseSummary {
    match cause {
        DecisionCause::Rule {
            pattern,
            action,
            destination,
        } => DecisionCauseSummary {
            matched_rule: Some((pattern.clone(), action.clone(), destination.clone())),
            ..Default::default()
        },
        DecisionCause::BuiltinJunk => DecisionCauseSummary {
            classification: Some(Category::Junk),
            ..Default::default()
        },
        DecisionCause::Classified(cat) => DecisionCauseSummary {
            classification: Some(*cat),
            ..Default::default()
        },
        DecisionCause::UnknownFallback(policy) => DecisionCauseSummary {
            classification: Some(Category::Other),
            unknown_fallback: Some(*policy),
            ..Default::default()
        },
        DecisionCause::DateMatched(date) => DecisionCauseSummary {
            date_metadata: Some(*date),
            ..Default::default()
        },
        DecisionCause::Protected(_) | DecisionCause::DateUnavailable(_) => {
            DecisionCauseSummary::default()
        }
    }
}

/// Resolves `root`'s effective policy and reports, read-only, what would
/// happen to `file` under it. `file` must currently exist (no-follow
/// metadata) — this describes live state, not a hypothetical path.
pub fn explain_path(file: &Path, root: &Path) -> Result<Explanation, String> {
    let policy: EffectivePolicy = crate::config::resolve_policy(&root.to_string_lossy())?;
    let entry = crate::scanner::describe_path(file)
        .ok_or_else(|| format!("{}: no such file", file.display()))?;
    let builtin = CategoryDB::default();
    let rules = rules_by_priority(&policy.rules);

    let decision = match policy.strategy {
        OrganizeStrategy::Type => {
            classify_for_organize(&entry, root, &rules, &builtin, policy.unknown_policy)
        }
        OrganizeStrategy::Date => classify_for_date(
            &entry,
            root,
            &rules,
            policy
                .template
                .as_ref()
                .expect("validated: Date always has a template"),
            policy
                .date_source
                .expect("validated: Date always has a date_source"),
        ),
    };

    let (mut op, mut reason, cause, dest_dir) = match decision {
        Decision::Skip(reason, cause) => (Op::Skip, reason, cause, None),
        Decision::Trash(reason, cause) => (Op::Trash, reason, cause, None),
        Decision::Move {
            dest_dir,
            reason,
            cause,
        } => (Op::Move, reason, cause, Some(dest_dir)),
    };
    let summary = decision_cause_summary(&cause);
    let (matched_rule, classification, unknown_fallback, date_metadata) = (
        summary.matched_rule,
        summary.classification,
        summary.unknown_fallback,
        summary.date_metadata,
    );

    let mut checks = vec![
        SafetyCheck {
            label: "regular file",
            ok: !entry.is_dir && !entry.is_symlink,
            detail: if entry.is_dir {
                Some("is a directory".into())
            } else if entry.is_symlink {
                Some("is a symlink".into())
            } else {
                None
            },
        },
        SafetyCheck {
            label: "not protected",
            ok: !matches!(cause, DecisionCause::Protected(_)),
            detail: match &cause {
                DecisionCause::Protected(r) => Some((*r).to_string()),
                _ => None,
            },
        },
    ];
    if let DecisionCause::DateUnavailable(msg) = &cause {
        checks.push(SafetyCheck {
            label: "date metadata available",
            ok: false,
            detail: Some(msg.clone()),
        });
    }

    let mut destination = None;
    if op == Op::Move {
        if let Some(dest_dir) = &dest_dir {
            match file.file_name() {
                None => {
                    checks.push(SafetyCheck {
                        label: "destination available",
                        ok: false,
                        detail: Some("entry has no file name".into()),
                    });
                    op = Op::Skip;
                    reason = "entry has no file name".into();
                }
                Some(filename) => {
                    let dest = dest_dir.join(filename);
                    // `type` destinations are always exactly one level;
                    // `date` destinations may be several (e.g. `2026/09`)
                    // — check every level between `root` and `dest_dir`,
                    // the same way `resolve_date_action` does, so a
                    // multi-level Date destination is reported accurately
                    // rather than only checking its final component.
                    let levels = destination_levels(root, dest_dir).unwrap_or_default();
                    let blocked = levels
                        .iter()
                        .any(|l| matches!(dir_status(l), DirStatus::Blocked));
                    let collision = std::fs::symlink_metadata(&dest).is_ok();
                    let safe_ancestors = ancestors_are_safe(&dest);
                    checks.push(SafetyCheck {
                        label: "destination available",
                        ok: !blocked,
                        detail: if blocked {
                            Some("destination category exists but is not a directory".into())
                        } else {
                            None
                        },
                    });
                    checks.push(SafetyCheck {
                        label: "no symlink ancestor",
                        ok: safe_ancestors,
                        detail: None,
                    });
                    checks.push(SafetyCheck {
                        label: "no collision",
                        ok: !collision,
                        detail: if collision {
                            Some(format!("{} already exists", dest.display()))
                        } else {
                            None
                        },
                    });
                    if blocked || collision {
                        op = Op::Skip;
                        reason = "collision".into();
                    } else if !safe_ancestors {
                        op = Op::Skip;
                        reason = "unsafe destination ancestor".into();
                    } else {
                        destination = Some(dest);
                    }
                }
            }
        }
    }

    Ok(Explanation {
        file: file.to_path_buf(),
        root: root.to_path_buf(),
        source: policy.source,
        strategy: policy.strategy,
        template: policy.template.as_ref().map(|t| t.raw().to_string()),
        matched_rule,
        classification,
        unknown_fallback,
        date_metadata,
        op,
        reason,
        destination,
        checks,
    })
}

pub fn cmd_explain(file: String, root: Option<String>, json: bool) -> bool {
    let file_path = PathBuf::from(&file);
    let root_path = match root {
        Some(r) => PathBuf::from(r),
        None => match file_path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        },
    };
    match explain_path(&file_path, &root_path) {
        Ok(exp) => {
            if json {
                println!("{}", crate::render::explain_json(&exp));
            } else {
                crate::render::explain(&exp);
            }
            true
        }
        Err(e) => {
            if json {
                println!("{{\"error\": {}}}", serde_json::to_string(&e).unwrap());
            } else {
                eprintln!("{e}");
            }
            false
        }
    }
}
