use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FSActionError {
    #[error("Destination exists")]
    DestExists,
    #[error("Source missing")]
    SourceMissing,
    #[error("FS rename error: {0}")]
    RenameErr(String),
    #[error("Cross-FS unsupported")]
    CrossFS,
    #[error("Symlink/hardlink not allowed")]
    Link,
    #[error("Is directory (not supported v0.1)")]
    Dir,
    #[error("Permission denied")]
    Perm,
    #[error("Unsafe path: an ancestor directory is a symlink")]
    UnsafeAncestor,
    #[error("Other: {0}")]
    Other(String),
}

/// Returns true if every *existing* ancestor directory of `path` is a real
/// directory, never a symlink. Walks upward from `path`'s parent and stops
/// at the first missing ancestor, since a single (non-recursive) create or
/// rename can't traverse through one anyway. This is the executor-side
/// re-check that a destination validated at plan time hasn't since had one
/// of its directory components swapped for a symlink (TOCTOU).
fn ancestors_are_safe(path: &Path) -> bool {
    let mut cur = path.parent();
    while let Some(dir) = cur {
        match fs::symlink_metadata(dir) {
            Ok(md) if md.is_dir() => {}
            Ok(_) => return false,
            Err(_) => break,
        }
        cur = dir.parent();
    }
    true
}

pub fn safe_rename(src: &PathBuf, dst: &PathBuf) -> Result<(), FSActionError> {
    if !ancestors_are_safe(dst) {
        return Err(FSActionError::UnsafeAncestor);
    }
    // Destination check: occupied means anything (file, dir, symlink, broken symlink)
    match dst.symlink_metadata() {
        Ok(_) => return Err(FSActionError::DestExists),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(FSActionError::Other(e.to_string()))
        }
        Err(_) => {}
    }
    // Source check: must exist and NOT a symlink
    let md = match src.symlink_metadata() {
        Ok(md) => md,
        Err(_) => return Err(FSActionError::SourceMissing),
    };
    if md.file_type().is_symlink() {
        return Err(FSActionError::Link);
    }
    if md.is_dir() {
        return Err(FSActionError::Dir);
    }
    fs::rename(src, dst).map_err(|e| {
        if e.kind() == std::io::ErrorKind::CrossesDevices {
            FSActionError::CrossFS
        } else if e.kind() == std::io::ErrorKind::AlreadyExists {
            FSActionError::DestExists
        } else if e.kind() == std::io::ErrorKind::PermissionDenied {
            FSActionError::Perm
        } else {
            FSActionError::RenameErr(e.to_string())
        }
    })
}

// Safe, atomic, idempotent directory creation, never overwrites, no-follow symlinks, error if exists and not a directory
pub fn safe_create_dir(dir: &PathBuf) -> Result<(), FSActionError> {
    if !ancestors_are_safe(dir) {
        return Err(FSActionError::UnsafeAncestor);
    }
    match fs::symlink_metadata(dir) {
        Ok(md) => {
            if md.is_dir() {
                // Already a dir
                Ok(())
            } else {
                // Exists but is file/symlink
                Err(FSActionError::DestExists)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(dir).map_err(|e| FSActionError::Other(e.to_string()))
        }
        Err(e) => Err(FSActionError::Other(e.to_string())),
    }
}

pub fn send_to_trash(src: &PathBuf) -> Result<(), FSActionError> {
    let md = src
        .symlink_metadata()
        .map_err(|_| FSActionError::SourceMissing)?;
    if md.is_dir() {
        return Err(FSActionError::Dir);
    }
    if md.file_type().is_symlink() {
        return Err(FSActionError::Link);
    }
    trash::delete(src).map_err(|e| FSActionError::Other(e.to_string()))
}
