//! Pure, deterministic decisions about whether a raw filesystem event path
//! is even worth tracking as a candidate — before any stability waiting or
//! live re-validation. Nothing here touches the filesystem.

use std::path::{Component, Path};

/// Suffixes/names of common in-progress download files. A file matching
/// one of these is never tracked as a candidate under that name — when the
/// download tool finishes and renames it to its final name, that rename is
/// a separate event which creates a fresh, non-transient candidate. This
/// does not blacklist the eventual final file, and it never affects
/// `clean`'s classification — it's watch-specific event filtering only.
const TRANSIENT_SUFFIXES: [&str; 4] = [".crdownload", ".part", ".download", ".tmp"];

pub fn is_transient_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    TRANSIENT_SUFFIXES.iter().any(|s| lower.ends_with(s))
}

/// Whether `path` is even a structurally plausible organize candidate for
/// `root` under the given recursion mode — purely path-shape, no
/// filesystem access. Non-recursive: `path` must be a direct child of
/// `root`. Recursive: `path` must be a strict descendant of `root` at any
/// depth. Either way, `path` must not be `root` itself.
pub fn is_eligible_candidate_path(root: &Path, recursive: bool, path: &Path) -> bool {
    if path == root {
        return false;
    }
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let depth = rel
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    if depth == 0 {
        return false;
    }
    if recursive {
        true
    } else {
        depth == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_suffixes_are_recognized_case_insensitively() {
        assert!(is_transient_filename("video.mp4.crdownload"));
        assert!(is_transient_filename("VIDEO.MP4.CRDOWNLOAD"));
        assert!(is_transient_filename("archive.zip.part"));
        assert!(is_transient_filename("file.download"));
        assert!(is_transient_filename("scratch.tmp"));
        assert!(!is_transient_filename("video.mp4"));
        assert!(!is_transient_filename("archive.zip"));
    }

    #[test]
    fn non_recursive_only_accepts_direct_children() {
        let root = Path::new("/tmp/inbox");
        assert!(is_eligible_candidate_path(root, false, &root.join("a.jpg")));
        assert!(!is_eligible_candidate_path(
            root,
            false,
            &root.join("Client/a.jpg")
        ));
        assert!(!is_eligible_candidate_path(root, false, root));
        assert!(!is_eligible_candidate_path(
            root,
            false,
            Path::new("/tmp/other/a.jpg")
        ));
    }

    #[test]
    fn recursive_accepts_any_descendant_depth() {
        let root = Path::new("/tmp/inbox");
        assert!(is_eligible_candidate_path(root, true, &root.join("a.jpg")));
        assert!(is_eligible_candidate_path(
            root,
            true,
            &root.join("Client/a.jpg")
        ));
        assert!(is_eligible_candidate_path(
            root,
            true,
            &root.join("Client/Sub/a.jpg")
        ));
        assert!(!is_eligible_candidate_path(root, true, root));
    }
}
