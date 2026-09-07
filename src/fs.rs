use std::fs;
use std::path::PathBuf;
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
    #[error("Other: {0}")]
    Other(String),
}

pub fn safe_rename(src: &PathBuf, dst: &PathBuf) -> Result<(), FSActionError> {
    if dst.exists() {
        return Err(FSActionError::DestExists);
    }
    if !src.exists() {
        return Err(FSActionError::SourceMissing);
    }
    let md = src
        .symlink_metadata()
        .map_err(|_| FSActionError::SourceMissing)?;
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

pub fn send_to_trash(src: &PathBuf) -> Result<(), FSActionError> {
    if !src.exists() {
        return Err(FSActionError::SourceMissing);
    }
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
