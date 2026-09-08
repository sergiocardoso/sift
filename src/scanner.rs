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

pub fn scan_entries(dir: &Path) -> Vec<Entry> {
    let mut entries = vec![];
    let walker = fs::read_dir(dir);
    if let Ok(walker) = walker {
        for item in walker.flatten() {
            let path = item.path();
            // No-follow symlink metadata
            let mds = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let is_symlink = mds.file_type().is_symlink();
            let is_dir = mds.is_dir();
            let hidden = is_hidden(&path);
            let mut project_root = false;
            let mut protected = false;
            if is_dir {
                project_root = is_project_root(&path);
                protected = project_root;
            }
            let entry = Entry {
                path: path.clone(),
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
            };
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

pub fn cmd_scan(path: String, json: bool) {
    let entries = scan_entries(Path::new(&path));
    if json {
        println!("{}", serde_json::to_string_pretty(&entries).unwrap());
    } else {
        for e in entries {
            println!("{:?}", e);
        }
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
        if e.project_root {
            findings.push(DoctorFinding {
                path: e.path.clone(),
                reason: "Project root directory detected (protected)".into(),
            });
        }
        if e.protected {
            findings.push(DoctorFinding {
                path: e.path.clone(),
                reason: "Protected directory (not mutated)".into(),
            });
        }
        if e.is_symlink {
            findings.push(DoctorFinding {
                path: e.path.clone(),
                reason: "Symlink (not mutated)".into(),
            });
        }
        if e.hidden {
            findings.push(DoctorFinding {
                path: e.path.clone(),
                reason: "Hidden file (not mutated)".into(),
            });
        }
        let fname = e.path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if crate::utils::is_sensitive_filename(fname) {
            findings.push(DoctorFinding {
                path: e.path.clone(),
                reason: "Suspicious filename (token-like)".into(),
            });
        }
        if let Some(sz) = e.size {
            if sz > 100_000_000 {
                findings.push(DoctorFinding {
                    path: e.path.clone(),
                    reason: format!("Large file ({} bytes)", sz),
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
                            reason: "Stale archive (>1y old)".to_string(),
                        });
                    }
                }
            }
        }
        if e.is_dir {
            let fname = e.path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if ["node_modules", "target", ".venv"].contains(&fname) {
                findings.push(DoctorFinding {
                    path: e.path.clone(),
                    reason: format!("Build output dir: {} (not mutated)", fname),
                });
            }
        }
    }
    findings
}

pub fn cmd_doctor(path: String, json: bool) {
    let findings = doctor_findings(Path::new(&path));
    if json {
        println!("{}", serde_json::to_string_pretty(&findings).unwrap());
    } else {
        for f in &findings {
            println!("{}: {}", f.path.display(), f.reason);
        }
    }
}
