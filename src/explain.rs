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
    rules_by_priority, AudioMetadata, DateMetadata, DocumentMetadata, EffectivePolicy,
    OrganizeStrategy, PhotoMetadata, PolicySource, VideoMetadata,
};
use crate::domain::{Category, Op};
use crate::fs::ancestors_are_safe;
use crate::planner::{
    classify_for_audio, classify_for_date, classify_for_documents, classify_for_organize,
    classify_for_photos, classify_for_video, destination_levels, dir_status, Decision,
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
    /// The raw `organize.template` string, when `strategy` is `"date"`,
    /// `"audio"`, or `"video"` (for display; not used for anything, since
    /// rendering already happened inside `classify_for_*`).
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
    /// `Some` when `strategy = "audio"` is what decided this file.
    pub audio_metadata: Option<AudioMetadata>,
    /// `Some` when `strategy = "video"` is what decided this file.
    pub video_metadata: Option<VideoMetadata>,
    /// `Some` when `strategy = "photos"` is what decided this file.
    pub photo_metadata: Option<PhotoMetadata>,
    /// `Some` when `strategy = "documents"` is what decided this file.
    pub document_metadata: Option<DocumentMetadata>,
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
    audio_metadata: Option<AudioMetadata>,
    video_metadata: Option<VideoMetadata>,
    photo_metadata: Option<PhotoMetadata>,
    document_metadata: Option<DocumentMetadata>,
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
        DecisionCause::AudioMatched(meta) => DecisionCauseSummary {
            audio_metadata: Some(meta.clone()),
            ..Default::default()
        },
        DecisionCause::VideoMatched(meta) => DecisionCauseSummary {
            video_metadata: Some(meta.clone()),
            ..Default::default()
        },
        DecisionCause::PhotoMatched(meta) => DecisionCauseSummary {
            photo_metadata: Some(meta.clone()),
            ..Default::default()
        },
        DecisionCause::DocumentMatched(meta) => DecisionCauseSummary {
            document_metadata: Some(meta.clone()),
            ..Default::default()
        },
        DecisionCause::Protected(_)
        | DecisionCause::DateUnavailable(_)
        | DecisionCause::AudioUnavailable(_)
        | DecisionCause::VideoUnavailable(_)
        | DecisionCause::PhotoUnavailable(_)
        | DecisionCause::DocumentUnavailable(_) => DecisionCauseSummary::default(),
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
        OrganizeStrategy::Audio => classify_for_audio(
            &entry,
            root,
            &rules,
            policy
                .metadata_template
                .as_ref()
                .expect("validated: Audio always has a metadata_template"),
        ),
        OrganizeStrategy::Video => classify_for_video(
            &entry,
            root,
            &rules,
            policy
                .metadata_template
                .as_ref()
                .expect("validated: Video always has a metadata_template"),
        ),
        OrganizeStrategy::Photos => classify_for_photos(
            &entry,
            root,
            &rules,
            policy
                .metadata_template
                .as_ref()
                .expect("validated: Photos always has a metadata_template"),
        ),
        OrganizeStrategy::Documents => classify_for_documents(
            &entry,
            root,
            &rules,
            policy
                .metadata_template
                .as_ref()
                .expect("validated: Documents always has a metadata_template"),
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
    let (
        matched_rule,
        classification,
        unknown_fallback,
        date_metadata,
        audio_metadata,
        video_metadata,
        photo_metadata,
        document_metadata,
    ) = (
        summary.matched_rule,
        summary.classification,
        summary.unknown_fallback,
        summary.date_metadata,
        summary.audio_metadata,
        summary.video_metadata,
        summary.photo_metadata,
        summary.document_metadata,
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
    match &cause {
        DecisionCause::DateUnavailable(msg) => checks.push(SafetyCheck {
            label: "date metadata available",
            ok: false,
            detail: Some(msg.clone()),
        }),
        DecisionCause::AudioUnavailable(msg) => checks.push(SafetyCheck {
            label: "audio metadata available",
            ok: false,
            detail: Some(msg.clone()),
        }),
        DecisionCause::VideoUnavailable(msg) => checks.push(SafetyCheck {
            label: "video metadata available",
            ok: false,
            detail: Some(msg.clone()),
        }),
        DecisionCause::PhotoUnavailable(msg) => checks.push(SafetyCheck {
            label: "photo metadata available",
            ok: false,
            detail: Some(msg.clone()),
        }),
        DecisionCause::DocumentUnavailable(msg) => checks.push(SafetyCheck {
            label: "document metadata available",
            ok: false,
            detail: Some(msg.clone()),
        }),
        _ => {}
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
        template: policy
            .template
            .as_ref()
            .map(|t| t.raw().to_string())
            .or_else(|| {
                policy
                    .metadata_template
                    .as_ref()
                    .map(|t| t.raw().to_string())
            }),
        matched_rule,
        classification,
        unknown_fallback,
        date_metadata,
        audio_metadata,
        video_metadata,
        photo_metadata,
        document_metadata,
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
