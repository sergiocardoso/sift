use crate::domain::Entry;
use std::fs::{self};
use std::path::{Path, PathBuf};

fn is_hidden(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}

pub fn is_project_root(p: &Path) -> bool {
    for marker in &[
        ".git",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "pubspec.yaml",
    ] {
        // symlink_metadata (not exists()/Path::exists, which follow links and
        // report false for a broken symlink) so a dangling marker symlink
        // still counts as present rather than silently weakening protection.
        if fs::symlink_metadata(p.join(marker)).is_ok() {
            return true;
        }
    }
    false
}

/// Directory names Sift's built-in `organize` categories map to. Recursive
/// traversal never descends into one of these: entering one would let a
/// later run see its own output as fresh input (Documents/Documents-style
/// repeated nesting). This is the single canonical list — use
/// `is_sift_category_dir` rather than re-checking names ad hoc elsewhere.
pub const CATEGORY_DIR_NAMES: [&str; 9] = [
    "Documents",
    "Images",
    "Audio",
    "Video",
    "Archives",
    "3D",
    "Code",
    "Data",
    "Other",
];

/// Whether `name` is one of Sift's own built-in organize destination
/// directories. Shared by recursive traversal eligibility and by anything
/// else that needs to recognize a category directory without duplicating
/// the list.
pub fn is_sift_category_dir(name: &str) -> bool {
    CATEGORY_DIR_NAMES.contains(&name)
}

/// Directory names that are conventionally build output or dependency
/// caches. Shared by the (non-recursive) doctor "build output" finding and
/// by recursive traversal eligibility.
pub const BUILD_OUTPUT_DIR_NAMES: [&str; 3] = ["node_modules", "target", ".venv"];

/// Shared "categorically off-limits" check for symlinks, software project
/// roots, protected directories, and hidden files/directories. This is the
/// single source of truth for that condition, reused by organize/clean's
/// per-entry skip decisions and by recursive traversal eligibility so the
/// two can never silently diverge.
pub fn protection_reason(entry: &Entry) -> Option<&'static str> {
    if entry.is_symlink {
        Some("symlink")
    } else if entry.project_root {
        Some("software project")
    } else if entry.protected {
        Some("protected directory")
    } else if entry.hidden {
        Some(if entry.is_dir {
            "hidden directory"
        } else {
            "hidden file"
        })
    } else {
        None
    }
}

/// Every reason `traversal_reason` can return, for callers that need to
/// recognize a "traversal boundary" reason without re-deriving the check.
pub const TRAVERSAL_BOUNDARY_REASONS: [&str; 6] = [
    "symlink",
    "software project",
    "protected directory",
    "hidden directory",
    "category directory",
    "build output",
];

/// Whether recursive discovery may descend into this directory entry, or
/// the reason it may not. This is the single source of truth for the
/// traversal boundary, shared by `scan`, `organize`, and `doctor`'s
/// `--recursive` mode. Only meaningful for `is_dir` entries; a symlink
/// (never `is_dir` under no-follow metadata) is excluded via
/// `protection_reason` regardless.
pub fn traversal_reason(entry: &Entry) -> Option<&'static str> {
    protection_reason(entry).or_else(|| {
        let name = entry
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if is_sift_category_dir(name) {
            Some("category directory")
        } else if BUILD_OUTPUT_DIR_NAMES.contains(&name) {
            Some("build output")
        } else {
            None
        }
    })
}

/// Builds the `Entry` for exactly one known path, using the same no-follow
/// metadata rules as `scan_entries`. Returns `None` if the path no longer
/// exists. Shared so "how do I turn a path into an Entry" lives in one
/// place — used by `scan_entries`'s directory listing and by watch's live
/// single-path revalidation, which needs one path's metadata without
/// listing its whole parent directory.
pub fn describe_path(path: &Path) -> Option<Entry> {
    let mds = fs::symlink_metadata(path).ok()?;
    let is_symlink = mds.file_type().is_symlink();
    let is_dir = mds.is_dir();
    let hidden = is_hidden(path);
    let mut project_root = false;
    let mut protected = false;
    if is_dir {
        project_root = is_project_root(path);
        protected = project_root;
    }
    Some(Entry {
        path: path.to_path_buf(),
        is_dir,
        is_symlink,
        hidden,
        size: if is_dir || is_symlink {
            None
        } else {
            Some(mds.len())
        },
        mtime: mds.modified().ok().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
        }),
        project_root,
        protected,
        classified_as: None,
    })
}

pub fn scan_entries(dir: &Path) -> Vec<Entry> {
    let mut entries = vec![];
    if let Ok(walker) = fs::read_dir(dir) {
        for item in walker.flatten() {
            if let Some(entry) = describe_path(&item.path()) {
                entries.push(entry);
            }
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

/// Re-checks, against the LIVE filesystem, that every directory between
/// `root` (inclusive) and `path`'s immediate parent (inclusive) is still
/// traversal-eligible, and that `path` itself is still a plain, visible
/// regular file. This is watch's TOCTOU / "project creation race" guard:
/// an event observed a moment ago may no longer describe a safe candidate
/// by the time anything is about to be planned or executed for it. `root`
/// itself is only checked for still being a real directory — its
/// protection status was already the user's explicit choice at `watch add`
/// time and is not re-litigated here.
pub fn revalidate_candidate(root: &Path, path: &Path) -> Result<Entry, &'static str> {
    let mut chain: Vec<PathBuf> = Vec::new();
    let mut cur = path.parent();
    loop {
        match cur {
            Some(dir) if dir == root => {
                chain.push(dir.to_path_buf());
                break;
            }
            Some(dir) => {
                chain.push(dir.to_path_buf());
                cur = dir.parent();
            }
            None => return Err("path is not under the watch root"),
        }
    }
    chain.reverse(); // root first, then descending toward path's parent

    for dir in &chain {
        let entry = describe_path(dir).ok_or("ancestor directory no longer exists")?;
        if !entry.is_dir {
            return Err("ancestor is no longer a directory");
        }
        if dir == root {
            continue;
        }
        if let Some(reason) = traversal_reason(&entry) {
            // A reserved category name is a dead end only because it's
            // *normally* one of the governing policy's own destinations —
            // a directory with its own `.sift.toml` is a deliberately
            // governed subtree instead (see `planner::could_be_own_output_dir`
            // and its callers for the same exception). Every other
            // boundary here (symlink/project/protected/hidden/build
            // output) is a genuine safety limit, never overridable.
            let has_own_override = reason == "category directory"
                && crate::config::local_policy_override(dir).is_some_and(|r| r.is_ok());
            if !has_own_override {
                return Err("ancestor directory is protected");
            }
        }
    }

    let file_entry = describe_path(path).ok_or("candidate no longer exists")?;
    if file_entry.is_dir {
        return Err("candidate is now a directory");
    }
    if file_entry.is_symlink {
        return Err("candidate is a symlink");
    }
    if file_entry.hidden {
        return Err("candidate is hidden");
    }
    Ok(file_entry)
}

/// Safely discovers every directory eligible for recursive treatment,
/// starting at (and including) `root`. Read-only: only calls `scan_entries`,
/// never mutates. Returns a deterministic, sorted list (`root` first, then
/// nested directories lexicographically) so callers get a stable snapshot
/// to plan against — a directory this call excludes is never entered, and
/// nothing discovered here is re-validated by descending into it twice.
///
/// Does not check whether `root` itself is a project root/protected —
/// callers that must refuse entirely in that case (`organize`, `clean`)
/// check `is_project_root(root)` themselves before calling this.
pub fn discover_recursive_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let mut children: Vec<PathBuf> = scan_entries(&dir)
            .into_iter()
            .filter(|e| e.is_dir && traversal_reason(e).is_none())
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

/// Flat, deterministically sorted union of `scan_entries` over every
/// eligible directory discovered under `root` (root included). Boundary
/// directories (symlinks, hidden, protected, category, build output)
/// appear once, as entries, but their contents never do — same schema as
/// non-recursive `scan_entries`, just spanning more directories.
pub fn scan_entries_recursive(root: &Path) -> Vec<Entry> {
    let dirs = discover_recursive_dirs(root);
    let mut all: Vec<Entry> = dirs.iter().flat_map(|d| scan_entries(d)).collect();
    all.sort_by(|a, b| a.path.cmp(&b.path));
    all
}

pub fn cmd_scan(path: String, json: bool, recursive: bool) {
    let target = Path::new(&path);
    let entries = if recursive {
        scan_entries_recursive(target)
    } else {
        scan_entries(target)
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&entries).unwrap());
    } else {
        crate::render::scan(&path, &entries, recursive);
    }
}

#[derive(serde::Serialize)]
pub struct DoctorFinding {
    pub path: PathBuf,
    pub reason: String,
}

/// doctor_findings collects findings by filename/stat only. No content read. Pure; does not print or mutate FS.
pub fn doctor_findings(path: &std::path::Path) -> Vec<DoctorFinding> {
    let entries = scan_entries(path);
    let mut findings: Vec<DoctorFinding> = Vec::new();
    for e in &entries {
        // A project root is always also `protected`; report it once rather
        // than as two findings describing the same protection state.
        if e.project_root {
            findings.push(DoctorFinding {
                path: e.path.clone(),
                reason: "Software project detected — protected".into(),
            });
        } else if e.protected {
            findings.push(DoctorFinding {
                path: e.path.clone(),
                reason: "Protected directory".into(),
            });
        }
        if e.is_symlink {
            findings.push(DoctorFinding {
                path: e.path.clone(),
                reason: "Symlink".into(),
            });
        }
        if e.hidden {
            findings.push(DoctorFinding {
                path: e.path.clone(),
                reason: "Hidden file".into(),
            });
        }
        let fname = e.path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if crate::utils::is_sensitive_filename(fname) {
            findings.push(DoctorFinding {
                path: e.path.clone(),
                reason: "Sensitive-looking filename".into(),
            });
        }
        if let Some(sz) = e.size {
            if sz > 100_000_000 {
                findings.push(DoctorFinding {
                    path: e.path.clone(),
                    reason: format!("Large file ({})", crate::render::human_size(sz)),
                });
            }
        }
        if let Some(ext) = e.path.extension().and_then(|x| x.to_str()) {
            if ["zip", "tar", "gz", "bz2", "7z", "rar"].contains(&ext.to_ascii_lowercase().as_str())
            {
                if let Some(mtime) = e.mtime {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    if now > mtime + 365 * 24 * 3600 {
                        findings.push(DoctorFinding {
                            path: e.path.clone(),
                            reason: "Stale archive (over a year old)".to_string(),
                        });
                    }
                }
            }
        }
        if e.is_dir {
            let fname = e.path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if BUILD_OUTPUT_DIR_NAMES.contains(&fname) {
                findings.push(DoctorFinding {
                    path: e.path.clone(),
                    reason: format!("Build output directory: {}", fname),
                });
            }
        }
    }
    findings
}

/// Unions `doctor_findings` over every eligible directory discovered under
/// `root` (root included). Each boundary directory (symlink, hidden,
/// protected, etc.) is only ever an *entry* of exactly one eligible
/// directory's scan, so it is reported once — never duplicated, and never
/// inspected past that boundary.
pub fn doctor_findings_recursive(root: &Path) -> Vec<DoctorFinding> {
    discover_recursive_dirs(root)
        .iter()
        .flat_map(|dir| doctor_findings(dir))
        .collect()
}

pub fn cmd_doctor(path: String, json: bool, recursive: bool) {
    let target = Path::new(&path);
    let findings = if recursive {
        doctor_findings_recursive(target)
    } else {
        doctor_findings(target)
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&findings).unwrap());
    } else {
        crate::render::doctor(&path, &findings);
    }
}
