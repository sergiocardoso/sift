use crate::classifier::CategoryDB;
use crate::config::{
    resolve_policy, rules_by_priority, safe_join_under, validate_rule_destination, EffectivePolicy,
    OrganizeStrategy, Rule, UnknownPolicy,
};
use crate::domain::{Action, Category, Entry, Op, Plan};
use crate::scanner::{is_project_root, scan_entries};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// What actually drove one entry's `Decision` — carried alongside the
/// existing human-readable `reason` string so `sift explain` can report a
/// structured "why" without re-deriving it via a second, separate
/// classification pass. This is metadata only; it never affects planning
/// itself.
#[derive(Debug, Clone)]
pub(crate) enum DecisionCause {
    /// A categorical safety boundary (symlink/hidden/project/protected/
    /// plain-directory) — see `categorical_skip_reason`.
    Protected(&'static str),
    /// An explicit `[[rules]]` entry matched.
    Rule {
        pattern: String,
        action: String,
        destination: Option<String>,
    },
    /// The built-in junk list (`.tmp`/`.swp`/`.swo`).
    BuiltinJunk,
    /// `strategy = "type"` classified the file into a specific category.
    Classified(Category),
    /// `strategy = "type"` found no specific category; `organize.unknown`
    /// decided what happens next.
    UnknownFallback(UnknownPolicy),
    /// `strategy = "date"` rendered a destination from this file's date
    /// metadata.
    DateMatched(crate::config::DateMetadata),
    /// `strategy = "date"` could not safely obtain date metadata for this
    /// file. Never silently reclassified by extension instead — the user
    /// selected Date, so an unavailable date is a Skip, not a fallback to
    /// Type.
    DateUnavailable(String),
}

/// Result of classifying a single entry for `organize`, before collision
/// checking against the real filesystem.
pub(crate) enum Decision {
    Skip(String, DecisionCause),
    Trash(String, DecisionCause),
    Move {
        dest_dir: PathBuf,
        reason: String,
        cause: DecisionCause,
    },
}

pub(crate) enum DirStatus {
    Exists,
    Missing,
    Blocked,
}

pub(crate) fn dir_status(dir: &Path) -> DirStatus {
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
pub(crate) fn builtin_destination(cat: Category) -> Option<(&'static str, &'static str)> {
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

/// Shared by every strategy: a categorical safety boundary (symlink,
/// hidden, project, protected, plain directory) always wins before any
/// rule or strategy is even consulted.
fn protected_decision(entry: &Entry) -> Option<Decision> {
    categorical_skip_reason(entry)
        .map(|reason| Decision::Skip(reason.into(), DecisionCause::Protected(reason)))
}

/// Shared by every strategy: an explicit `[[rules]]` match always wins
/// over whatever the selected strategy would otherwise decide. Lives here
/// once so `type` and `date` (and any future strategy) can never diverge
/// in how rule precedence works.
fn rule_decision(entry: &Entry, target: &Path, rules: &[&Rule]) -> Option<Decision> {
    for rule in rules {
        if !glob_match(&rule.pattern, &entry.path) {
            continue;
        }
        let reason = rule
            .description
            .clone()
            .unwrap_or_else(|| format!("config rule: {}", rule.label()));
        let cause = DecisionCause::Rule {
            pattern: rule.pattern.clone(),
            action: rule.action.clone(),
            destination: rule.destination.clone(),
        };
        return Some(match rule.action.as_str() {
            "Skip" => Decision::Skip(reason, cause),
            "Trash" => Decision::Trash(reason, cause),
            "Move" => {
                let dest = match rule.destination.as_deref() {
                    Some(d) => d,
                    None => {
                        return Some(Decision::Skip(
                            "move rule missing destination".into(),
                            cause,
                        ))
                    }
                };
                if !validate_rule_destination(dest) {
                    return Some(Decision::Skip("unsafe config destination".into(), cause));
                }
                match safe_join_under(target, Path::new(dest)) {
                    Some(dest_dir) => Decision::Move {
                        dest_dir,
                        reason,
                        cause,
                    },
                    None => Decision::Skip("unsafe config destination".into(), cause),
                }
            }
            _ => Decision::Skip("invalid rule config".into(), cause),
        });
    }
    None
}

pub(crate) fn classify_for_organize(
    entry: &Entry,
    target: &Path,
    rules: &[&Rule],
    builtin: &CategoryDB,
    unknown_policy: UnknownPolicy,
) -> Decision {
    if let Some(d) = protected_decision(entry) {
        return d;
    }
    if let Some(d) = rule_decision(entry, target, rules) {
        return d;
    }
    let mut classified = entry.clone();
    builtin.classify(&mut classified);
    match classified.classified_as {
        Some(Category::Junk) => {
            Decision::Trash("built-in junk file".into(), DecisionCause::BuiltinJunk)
        }
        Some(Category::Other) => match unknown_policy {
            UnknownPolicy::Other => match builtin_destination(Category::Other) {
                Some((name, reason)) => Decision::Move {
                    dest_dir: target.join(name),
                    reason: reason.into(),
                    cause: DecisionCause::UnknownFallback(UnknownPolicy::Other),
                },
                None => Decision::Skip(
                    "unknown type".into(),
                    DecisionCause::UnknownFallback(UnknownPolicy::Other),
                ),
            },
            UnknownPolicy::Skip => Decision::Skip(
                "unknown type (policy: skip)".into(),
                DecisionCause::UnknownFallback(UnknownPolicy::Skip),
            ),
        },
        Some(cat) => match builtin_destination(cat) {
            Some((name, reason)) => Decision::Move {
                dest_dir: target.join(name),
                reason: reason.into(),
                cause: DecisionCause::Classified(cat),
            },
            None => Decision::Skip("unknown type".into(), DecisionCause::Classified(cat)),
        },
        None => Decision::Skip(
            "unknown type".into(),
            DecisionCause::Classified(Category::Unknown),
        ),
    }
}

/// Reads *only* the metadata Date needs (currently: mtime) — never file
/// contents. No-follow (`symlink_metadata`), matching every other safety
/// check in this module; in practice a symlink never reaches here anyway
/// since `protected_decision` already skipped it.
pub fn extract_date_metadata(
    path: &Path,
    source: crate::config::DateSource,
) -> Result<crate::config::DateMetadata, String> {
    match source {
        crate::config::DateSource::Modified => {
            let md = std::fs::symlink_metadata(path)
                .map_err(|e| format!("cannot read metadata: {e}"))?;
            let modified = md
                .modified()
                .map_err(|e| format!("modification time unavailable: {e}"))?;
            let secs = modified
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "modification time predates the Unix epoch".to_string())?
                .as_secs();
            Ok(crate::config::DateMetadata::from_unix_secs(secs as i64))
        }
    }
}

/// `strategy = "date"`'s classification: the same categorical-safety and
/// rule precedence every strategy shares, then date metadata rendered
/// through the configured `template`. If the date can't be safely
/// obtained, this is a `Skip` with a clear reason — never a silent
/// reclassification by extension (that would be switching strategies
/// without being asked).
pub(crate) fn classify_for_date(
    entry: &Entry,
    target: &Path,
    rules: &[&Rule],
    template: &crate::config::Template,
    date_source: crate::config::DateSource,
) -> Decision {
    if let Some(d) = protected_decision(entry) {
        return d;
    }
    if let Some(d) = rule_decision(entry, target, rules) {
        return d;
    }
    let date = match extract_date_metadata(&entry.path, date_source) {
        Ok(d) => d,
        Err(e) => {
            return Decision::Skip(
                format!("date metadata unavailable: {e}"),
                DecisionCause::DateUnavailable(e),
            )
        }
    };
    match template.render(&date) {
        Ok(rel_dir) => Decision::Move {
            dest_dir: target.join(&rel_dir),
            reason: format!("{:04}-{:02}-{:02}", date.year, date.month, date.day),
            cause: DecisionCause::DateMatched(date),
        },
        Err(e) => Decision::Skip(
            format!("date template error: {e}"),
            DecisionCause::DateUnavailable(e),
        ),
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
    unknown_policy: UnknownPolicy,
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
    match classify_for_organize(entry, target, rules, builtin, unknown_policy) {
        Decision::Skip(reason, _cause) => Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Skip,
            reason: Some(reason),
            undoable: false,
        },
        Decision::Trash(reason, _cause) => Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Trash,
            reason: Some(reason),
            undoable: false,
        },
        Decision::Move {
            dest_dir,
            reason,
            cause: _,
        } => match dir_status(&dest_dir) {
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

/// The sequence of directories, from `target`'s immediate child down to
/// (and including) `dest_dir`, that must each be a real directory before
/// `dest_dir` is usable. `type` destinations are always exactly one level
/// (e.g. `Documents`); `date` destinations can be several (e.g.
/// `2026/09`) — this is what lets one shared collision/CreateDir routine
/// handle both without `type` ever needing to change.
pub(crate) fn destination_levels(target: &Path, dest_dir: &Path) -> Option<Vec<PathBuf>> {
    let rel = dest_dir.strip_prefix(target).ok()?;
    let mut level = target.to_path_buf();
    let mut levels = Vec::new();
    for comp in rel.components() {
        level.push(comp);
        levels.push(level.clone());
    }
    Some(levels)
}

/// Resolves one entry's final Date action: same collision/no-overwrite/
/// broken-symlink-counts-as-occupied rules as `resolve_organize_action`,
/// generalized to a destination that may be several directory levels
/// deep. Every level between `target` and the rendered destination is
/// checked; a real existing directory is left alone (never re-created,
/// never re-listed), a missing one is queued for `Op::CreateDir` in
/// shallow-to-deep order (via the `BTreeSet`'s own ordering — a shorter
/// path always sorts before its own deeper extensions), and anything else
/// occupying a level (file, symlink, broken symlink) refuses the whole
/// move safely.
fn resolve_date_action(
    entry: &Entry,
    target: &Path,
    rules: &[&Rule],
    template: &crate::config::Template,
    date_source: crate::config::DateSource,
    needed_dirs: &mut BTreeSet<PathBuf>,
) -> Action {
    let skip = |reason: &str| Action {
        src: entry.path.clone(),
        dst: None,
        op: Op::Skip,
        reason: Some(reason.to_string()),
        undoable: false,
    };
    let skip_with_intended_dst = |dest: PathBuf, reason: &str| Action {
        src: entry.path.clone(),
        dst: Some(dest),
        op: Op::Skip,
        reason: Some(reason.to_string()),
        undoable: false,
    };
    match classify_for_date(entry, target, rules, template, date_source) {
        Decision::Skip(reason, _cause) => skip(&reason),
        Decision::Trash(reason, _cause) => Action {
            src: entry.path.clone(),
            dst: None,
            op: Op::Trash,
            reason: Some(reason),
            undoable: false,
        },
        Decision::Move {
            dest_dir,
            reason,
            cause: _,
        } => {
            let Some(levels) = destination_levels(target, &dest_dir) else {
                return skip("destination escaped target");
            };
            let mut to_create = Vec::new();
            for level in &levels {
                match dir_status(level) {
                    DirStatus::Blocked => return skip_with_intended_dst(dest_dir, "collision"),
                    DirStatus::Missing => to_create.push(level.clone()),
                    DirStatus::Exists => {}
                }
            }
            let Some(filename) = entry.path.file_name() else {
                return skip("entry has no file name");
            };
            let dest = dest_dir.join(filename);
            if std::fs::symlink_metadata(&dest).is_ok() {
                return skip_with_intended_dst(dest, "collision");
            }
            for dir in to_create {
                needed_dirs.insert(dir);
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
}

/// One entry's organize plan: the `CreateDir` action(s) its destination
/// needs (zero, one for `type`, or several in shallow-to-deep order for
/// `date`), and the entry's own action.
pub struct EntryPlan {
    pub create_dirs: Vec<Action>,
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
    unknown_policy: UnknownPolicy,
) -> EntryPlan {
    let rules = rules_by_priority(config_rules);
    let mut needed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let action = resolve_organize_action(
        entry,
        containing_dir,
        &rules,
        builtin,
        &mut needed_dirs,
        unknown_policy,
    );
    EntryPlan {
        create_dirs: createdir_actions(needed_dirs),
        action,
    }
}

/// The `date`-strategy counterpart to `plan_entry_organize`, used by
/// Watch through `plan_entry_with_strategy` exactly the same way — never
/// a separate, Watch-specific Date implementation. May produce several
/// `create_dirs` (shallow-to-deep) where `type` only ever produces one.
pub fn plan_entry_date(
    entry: &Entry,
    containing_dir: &Path,
    config_rules: &[Rule],
    template: &crate::config::Template,
    date_source: crate::config::DateSource,
) -> EntryPlan {
    let rules = rules_by_priority(config_rules);
    let mut needed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let action = resolve_date_action(
        entry,
        containing_dir,
        &rules,
        template,
        date_source,
        &mut needed_dirs,
    );
    EntryPlan {
        create_dirs: createdir_actions(needed_dirs),
        action,
    }
}

/// Watch's sole authority for turning one live candidate into a plan,
/// dispatching on the resolved policy's strategy the same way manual
/// organize does — see `plan_with_strategy`. This is the one seam a
/// future strategy extends; Watch itself never reimplements
/// classification or template rendering.
pub fn plan_entry_with_strategy(
    entry: &Entry,
    containing_dir: &Path,
    policy: &EffectivePolicy,
    builtin: &CategoryDB,
) -> EntryPlan {
    match policy.strategy {
        OrganizeStrategy::Type => plan_entry_organize(
            entry,
            containing_dir,
            &policy.rules,
            builtin,
            policy.unknown_policy,
        ),
        OrganizeStrategy::Date => plan_entry_date(
            entry,
            containing_dir,
            &policy.rules,
            policy
                .template
                .as_ref()
                .expect("validated: Date always has a template"),
            policy
                .date_source
                .expect("validated: Date always has a date_source"),
        ),
    }
}

pub fn plan_organize(
    path: &str,
    config_rules: &[Rule],
    builtin: &CategoryDB,
    unknown_policy: UnknownPolicy,
) -> Plan {
    let target = Path::new(path);
    let entries = scan_entries(target);
    if is_project_root(target) {
        return skip_all(&entries, "target is a software project root");
    }
    let rules = rules_by_priority(config_rules);
    let mut needed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let entry_actions: Vec<Action> = entries
        .iter()
        .map(|entry| {
            resolve_organize_action(
                entry,
                target,
                &rules,
                builtin,
                &mut needed_dirs,
                unknown_policy,
            )
        })
        .collect();
    let mut actions = createdir_actions(needed_dirs);
    actions.extend(entry_actions);
    Plan { actions }
}

/// The `date`-strategy counterpart to `plan_organize`: same immutable-
/// safety-first, rules-win-over-strategy shape, but the fallback path
/// renders each eligible file's destination through `template` instead of
/// classifying by extension, and a shared destination directory spanning
/// several levels (e.g. `2026/09`) is created in shallow-to-deep order
/// exactly once no matter how many files land in it (`needed_dirs` is a
/// `BTreeSet`, deduplicated by construction).
pub fn plan_organize_date(
    path: &str,
    config_rules: &[Rule],
    template: &crate::config::Template,
    date_source: crate::config::DateSource,
) -> Plan {
    let target = Path::new(path);
    let entries = scan_entries(target);
    if is_project_root(target) {
        return skip_all(&entries, "target is a software project root");
    }
    let rules = rules_by_priority(config_rules);
    let mut needed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let entry_actions: Vec<Action> = entries
        .iter()
        .map(|entry| {
            resolve_date_action(
                entry,
                target,
                &rules,
                template,
                date_source,
                &mut needed_dirs,
            )
        })
        .collect();
    let mut actions = createdir_actions(needed_dirs);
    actions.extend(entry_actions);
    Plan { actions }
}

/// The single "policy → strategy → planner" dispatch point for a
/// non-recursive organize. `policy` is always already validated (see
/// `config::resolve_policy`) by the time it reaches here, so this match is
/// total today and stays total as future strategies are added — each gets
/// one new arm, never a second parser or a second classifier.
pub fn plan_with_strategy(path: &str, policy: &EffectivePolicy, builtin: &CategoryDB) -> Plan {
    match policy.strategy {
        OrganizeStrategy::Type => {
            plan_organize(path, &policy.rules, builtin, policy.unknown_policy)
        }
        OrganizeStrategy::Date => plan_organize_date(
            path,
            &policy.rules,
            policy
                .template
                .as_ref()
                .expect("validated: Date always has a template"),
            policy
                .date_source
                .expect("validated: Date always has a date_source"),
        ),
    }
}

pub(crate) fn createdir_actions(needed_dirs: BTreeSet<PathBuf>) -> Vec<Action> {
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
    unknown_policy: UnknownPolicy,
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
                unknown_policy,
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

/// Discovers eligible directories for a recursive Date organize exactly
/// like `discover_recursive_dirs`, plus one extra exclusion: a directory
/// whose name could have been generated by rendering this same
/// `template`'s first path component (e.g. `2026` for `"{year}/{month}"`)
/// is never descended into. Excluding it at the entry point is sufficient
/// — since it's never entered, nothing deeper inside it (`2026/09`, its
/// files) is ever discovered either, which is exactly what keeps a second
/// run from ever nesting `2026/09/2026/09/...`.
fn discover_recursive_dirs_for_date(
    root: &Path,
    template: &crate::config::Template,
) -> Vec<PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let mut children: Vec<PathBuf> = scan_entries(&dir)
            .into_iter()
            .filter(|e| e.is_dir && crate::scanner::traversal_reason(e).is_none())
            .filter(|e| {
                let name = e.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                !template.component_could_be_generated(0, name)
            })
            .map(|e| e.path)
            .collect();
        children.sort();
        for child in children {
            dirs.push(child.clone());
            pending.push(child);
        }
    }
    dirs.sort();
    dirs
}

/// The `date`-strategy counterpart to `plan_organize_recursive`. Each
/// discovered directory is still its own local organize context (a file
/// in `Client/` gets `Client/2026/09/`, never flattened to the
/// recursion root), and `discover_recursive_dirs_for_date` keeps this
/// same-run and cross-run idempotent by never re-entering a directory the
/// template itself would generate.
pub fn plan_organize_date_recursive(
    path: &str,
    config_rules: &[Rule],
    template: &crate::config::Template,
    date_source: crate::config::DateSource,
) -> RecursivePlan {
    let root = Path::new(path);
    if is_project_root(root) {
        return RecursivePlan {
            plan: skip_all(&scan_entries(root), "target is a software project root"),
            dirs_scanned: 0,
        };
    }
    let dirs = discover_recursive_dirs_for_date(root, template);
    let rules = rules_by_priority(config_rules);
    let mut needed_dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let mut entry_actions: Vec<Action> = Vec::new();

    for dir in &dirs {
        for entry in scan_entries(dir) {
            if entry.is_dir {
                if let Some(reason) = crate::scanner::traversal_reason(&entry) {
                    entry_actions.push(Action {
                        src: entry.path.clone(),
                        dst: None,
                        op: Op::Skip,
                        reason: Some(reason.into()),
                        undoable: false,
                    });
                    continue;
                }
                let name = entry
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if template.component_could_be_generated(0, name) {
                    entry_actions.push(Action {
                        src: entry.path.clone(),
                        dst: None,
                        op: Op::Skip,
                        reason: Some("date-organized directory".into()),
                        undoable: false,
                    });
                }
                continue;
            }
            entry_actions.push(resolve_date_action(
                &entry,
                dir,
                &rules,
                template,
                date_source,
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

/// The recursive counterpart to `plan_with_strategy`: same one-strategy
/// dispatch, applied consistently across every directory the recursive
/// scan discovers under the root's own policy (there is no per-directory
/// nested `.sift.toml` discovery in this MVP — the root's resolved policy
/// is the policy for the whole recursive operation).
pub fn plan_with_strategy_recursive(
    path: &str,
    policy: &EffectivePolicy,
    builtin: &CategoryDB,
) -> RecursivePlan {
    match policy.strategy {
        OrganizeStrategy::Type => {
            plan_organize_recursive(path, &policy.rules, builtin, policy.unknown_policy)
        }
        OrganizeStrategy::Date => plan_organize_date_recursive(
            path,
            &policy.rules,
            policy
                .template
                .as_ref()
                .expect("validated: Date always has a template"),
            policy
                .date_source
                .expect("validated: Date always has a date_source"),
        ),
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
            .unwrap_or_else(|| format!("config rule: {}", rule.label()));
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
    let policy = match resolve_policy(&path) {
        Ok(p) => p,
        Err(e) => {
            crate::render::policy_error("organize", &path, &e);
            return false;
        }
    };
    let builtins = CategoryDB::default();
    let root = is_project_root(Path::new(&path));

    let (plan, dirs_scanned) = if recursive {
        let recursive_plan = plan_with_strategy_recursive(&path, &policy, &builtins);
        (recursive_plan.plan, Some(recursive_plan.dirs_scanned))
    } else {
        (plan_with_strategy(&path, &policy, &builtins), None)
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
    let policy = match resolve_policy(&path) {
        Ok(p) => p,
        Err(e) => {
            crate::render::policy_error("clean", &path, &e);
            return false;
        }
    };
    let builtins = CategoryDB::default();
    let plan = plan_clean(&path, &policy.rules, &builtins);
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
