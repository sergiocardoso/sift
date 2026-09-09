//! Integration tests for `sift folders`: safe whole-folder organization.
//!
//! Every test uses `tempfile::tempdir()` — never the real filesystem — and
//! always overrides the history directory to a path inside that tempdir
//! before doing anything that might record history, exactly like
//! `tests/integration.rs`.

use sift::domain::{Category, Op};
use sift::folders::{
    cmd_folders, combine_with_duplicate_removals, detect_possible_duplicates, full_execution_plan,
    plan_duplicate_removals, plan_folders, profile_folder, Decision, MAX_PROFILE_DEPTH,
    MAX_PROFILE_FILES,
};
use sift::history::{clear_test_history_dir, cmd_undo, set_test_history_dir};
use std::fs::{self, File};
use std::path::Path;
use tempfile::tempdir;

fn touch(p: &Path) {
    File::create(p).unwrap();
}

fn isolate_history(t: &Path) {
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    assert!(hist_dir.starts_with(t)); // Safety: tempdir path only in test
}

fn candidate<'a>(
    plan: &'a sift::folders::FoldersPlan,
    name: &str,
) -> &'a sift::folders::FolderCandidate {
    plan.candidates
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no candidate named {name}"))
}

// ------------------------------------------------------------ classification

#[test]
fn test_high_confidence_documents_move() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("curriculo");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("cv.pdf"));
    touch(&dir.join("cv.docx"));
    touch(&dir.join("carta.pdf"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "curriculo");
    assert!(matches!(
        c.decision,
        Decision::Move {
            category: Category::Document,
            ..
        }
    ));
}

#[test]
fn test_high_confidence_images_move() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("artes");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("logo.svg"));
    touch(&dir.join("banner.png"));
    touch(&dir.join("photo.jpg"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "artes");
    assert!(matches!(
        c.decision,
        Decision::Move {
            category: Category::Image,
            ..
        }
    ));
}

#[test]
fn test_high_confidence_threed_move_with_minority_image() {
    // Mostly 3D assets with one preview thumbnail: still dominant enough
    // (5/6 ≈ 83%) to clear the 80% high-confidence bar.
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("olho_grego");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("model_a.stl"));
    touch(&dir.join("model_b.stl"));
    touch(&dir.join("model_c.stl"));
    touch(&dir.join("scene.3mf"));
    touch(&dir.join("part.obj"));
    touch(&dir.join("preview.png"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "olho_grego");
    match &c.decision {
        Decision::Move {
            category: Category::ThreeD,
            confidence,
        } => assert!(*confidence >= 0.80),
        other => panic!("expected Move(ThreeD), got {other:?}"),
    }
}

#[test]
fn test_medium_confidence_suggest_never_moves() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("half_and_half");
    fs::create_dir(&dir).unwrap();
    // 3 documents, 2 images: 60% dominant -> medium, SUGGEST only.
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));
    touch(&dir.join("c.pdf"));
    touch(&dir.join("d.png"));
    touch(&dir.join("e.png"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "half_and_half");
    assert!(matches!(
        c.decision,
        Decision::Suggest {
            category: Category::Document,
            ..
        }
    ));
    // No MoveDir action was ever planned for a Suggest-only candidate.
    assert!(!plan
        .actions
        .iter()
        .any(|a| a.op == Op::MoveDir && a.src == c.path));
}

#[test]
fn test_mixed_content_leaves() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("mixed");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("photo.jpg"));
    touch(&dir.join("data.json"));
    touch(&dir.join("movie.mp4"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "mixed");
    assert!(matches!(
        c.decision,
        Decision::Leave {
            reason: "mixed content"
        }
    ));
}

#[test]
fn test_empty_folder_leaves() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir(t.join("empty")).unwrap();

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "empty");
    assert!(matches!(
        c.decision,
        Decision::Leave {
            reason: "empty folder"
        }
    ));
}

#[test]
fn test_single_unknown_file_leaves() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("Simone");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("unknown.xyz"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "Simone");
    assert!(matches!(c.decision, Decision::Leave { .. }));
}

#[test]
fn test_single_known_file_suggests_never_moves() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("one_pdf");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("only.pdf"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "one_pdf");
    assert!(matches!(
        c.decision,
        Decision::Suggest {
            reason: "only one evidence file",
            ..
        }
    ));
}

#[test]
fn test_folder_name_is_not_used_as_evidence() {
    // Named like an unrelated category, but its actual file contents (all
    // PDFs) determine the classification — never the folder's own name.
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("photos"); // name suggests Images, contents say otherwise
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));
    touch(&dir.join("c.pdf"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "photos");
    assert!(matches!(
        c.decision,
        Decision::Move {
            category: Category::Document,
            ..
        }
    ));
}

#[test]
fn test_nested_files_inform_classification_but_dont_move_individually() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("nested_docs");
    let sub = dir.join("scans");
    fs::create_dir_all(&sub).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&sub.join("b.pdf"));
    touch(&sub.join("c.pdf"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "nested_docs");
    assert!(matches!(
        c.decision,
        Decision::Move {
            category: Category::Document,
            ..
        }
    ));
    assert_eq!(c.evidence_files, 3);
    // Only the top folder itself is a move target; the nested file never is.
    let mv = plan
        .actions
        .iter()
        .find(|a| a.op == Op::MoveDir)
        .expect("expected one MoveDir action");
    assert_eq!(mv.src, dir);
}

#[test]
fn test_code_and_data_categories_classified() {
    let d = tempdir().unwrap();
    let t = d.path();
    let code_dir = t.join("scripts");
    fs::create_dir(&code_dir).unwrap();
    touch(&code_dir.join("a.py"));
    touch(&code_dir.join("b.sh"));
    let data_dir = t.join("datasets");
    fs::create_dir(&data_dir).unwrap();
    touch(&data_dir.join("a.csv"));
    touch(&data_dir.join("b.json"));

    let plan = plan_folders(t.to_str().unwrap());
    assert!(matches!(
        candidate(&plan, "scripts").decision,
        Decision::Move {
            category: Category::Code,
            ..
        }
    ));
    assert!(matches!(
        candidate(&plan, "datasets").decision,
        Decision::Move {
            category: Category::Data,
            ..
        }
    ));
}

#[test]
fn test_other_files_count_as_evidence_not_a_category() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("mostly_docs");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));
    touch(&dir.join("c.pdf"));
    touch(&dir.join("d.pdf"));
    touch(&dir.join("weird.xyz")); // Other: counts toward total, not Document
    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "mostly_docs");
    assert_eq!(c.evidence_files, 5);
    // 4/5 = 80% exactly meets the high-confidence bar.
    match &c.decision {
        Decision::Move {
            category: Category::Document,
            confidence,
        } => assert!((*confidence - 0.8).abs() < 1e-9),
        other => panic!("expected Move(Document) at 80%, got {other:?}"),
    }
}

// ------------------------------------------------------------------- safety

#[test]
fn test_project_root_candidate_is_protected() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("flokin-landing");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("package.json"));
    fs::create_dir(dir.join("src")).unwrap();
    touch(&dir.join("src").join("index.js"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "flokin-landing");
    assert!(matches!(
        c.decision,
        Decision::Protect {
            reason: "software project"
        }
    ));
    assert!(!plan.actions.iter().any(|a| a.src == c.path));
}

#[test]
fn test_nested_project_protects_parent() {
    let d = tempdir().unwrap();
    let t = d.path();
    let work = t.join("Work");
    let app = work.join("app");
    fs::create_dir_all(app.join("src")).unwrap();
    touch(&app.join("Cargo.toml"));
    touch(&app.join("src").join("main.rs"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "Work");
    assert!(matches!(
        c.decision,
        Decision::Protect {
            reason: "contains software project"
        }
    ));
}

#[test]
fn test_hidden_candidate_protected() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join(".secret");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, ".secret");
    assert!(matches!(
        c.decision,
        Decision::Protect {
            reason: "hidden directory"
        }
    ));
}

#[test]
fn test_symlink_folder_never_a_candidate() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    let real = t.join("real_dir");
    fs::create_dir(&real).unwrap();
    touch(&real.join("a.pdf"));
    symlink(&real, t.join("link_to_dir")).unwrap();

    let plan = plan_folders(t.to_str().unwrap());
    assert!(!plan.candidates.iter().any(|c| c.name == "link_to_dir"));
}

#[test]
fn test_symlink_inside_subtree_not_followed_or_counted() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("has_symlink");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));
    // A symlink to a directory full of other-category evidence; must not be
    // traversed, and must not contribute to the profile at all.
    let elsewhere = t.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    touch(&elsewhere.join("video.mp4"));
    symlink(&elsewhere, dir.join("escape")).unwrap();
    // A symlinked *file* alongside real evidence, also never counted.
    symlink(dir.join("a.pdf"), dir.join("a_link.pdf")).unwrap();

    let profile = profile_folder(&dir);
    assert_eq!(profile.total_eligible(), 2);
    assert!(!profile.contains_project);
}

#[test]
fn test_sift_category_dirs_never_candidates() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir(t.join("Documents")).unwrap();
    fs::create_dir(t.join("Images")).unwrap();
    touch(&t.join("Documents").join("a.pdf"));

    let plan = plan_folders(t.to_str().unwrap());
    assert!(!plan.candidates.iter().any(|c| c.name == "Documents"));
    assert!(!plan.candidates.iter().any(|c| c.name == "Images"));
}

#[test]
fn test_dry_run_never_mutates() {
    let d = tempdir().unwrap();
    let t = d.path();
    isolate_history(t);
    let dir = t.join("curriculo");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));

    assert!(cmd_folders(
        t.to_str().unwrap().to_string(),
        false,
        false,
        false
    ));
    assert!(dir.exists());
    assert!(!t.join("Documents").exists());
    clear_test_history_dir();
}

#[test]
fn test_apply_refuses_on_destination_collision() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("curriculo");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));
    // Pre-occupy the destination.
    fs::create_dir_all(t.join("Documents").join("curriculo")).unwrap();

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "curriculo");
    assert!(matches!(
        c.decision,
        Decision::MoveRefused {
            reason: "destination already exists",
            ..
        }
    ));
    assert!(!plan.actions.iter().any(|a| a.op == Op::MoveDir));
}

#[test]
fn test_apply_refuses_when_destination_category_is_a_file() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("curriculo");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));
    // "Documents" exists but as a plain file, not a directory.
    touch(&t.join("Documents"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "curriculo");
    assert!(matches!(c.decision, Decision::MoveRefused { .. }));
}

#[test]
fn test_createdir_reused_not_duplicated() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir(t.join("docs_a")).unwrap();
    touch(&t.join("docs_a").join("a.pdf"));
    touch(&t.join("docs_a").join("b.pdf"));
    fs::create_dir(t.join("docs_b")).unwrap();
    touch(&t.join("docs_b").join("a.pdf"));
    touch(&t.join("docs_b").join("b.pdf"));

    let plan = plan_folders(t.to_str().unwrap());
    let create_dirs: Vec<_> = plan
        .actions
        .iter()
        .filter(|a| a.op == Op::CreateDir)
        .collect();
    assert_eq!(create_dirs.len(), 1);
    assert_eq!(create_dirs[0].src, t.join("Documents"));
}

// ----------------------------------------------------------------- planning

#[test]
fn test_candidates_sorted_by_name() {
    let d = tempdir().unwrap();
    let t = d.path();
    for name in ["zeta", "alpha", "mid"] {
        fs::create_dir(t.join(name)).unwrap();
    }
    let plan = plan_folders(t.to_str().unwrap());
    let names: Vec<&str> = plan.candidates.iter().map(|c| c.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn test_files_at_top_level_are_never_candidates() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch(&t.join("loose.pdf"));
    let plan = plan_folders(t.to_str().unwrap());
    assert!(plan.candidates.is_empty());
}

#[test]
fn test_multiple_folders_analyzed_independently() {
    let d = tempdir().unwrap();
    let t = d.path();
    let a = t.join("docs_only");
    fs::create_dir(&a).unwrap();
    touch(&a.join("x.pdf"));
    touch(&a.join("y.pdf"));
    let b = t.join("empty_one");
    fs::create_dir(&b).unwrap();

    let plan = plan_folders(t.to_str().unwrap());
    assert_eq!(plan.candidates.len(), 2);
    assert!(matches!(
        candidate(&plan, "docs_only").decision,
        Decision::Move { .. }
    ));
    assert!(matches!(
        candidate(&plan, "empty_one").decision,
        Decision::Leave { .. }
    ));
}

#[test]
fn test_destination_is_a_subfolder_never_flattened() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("olho_grego");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.stl"));
    touch(&dir.join("b.stl"));

    let plan = plan_folders(t.to_str().unwrap());
    let mv = plan
        .actions
        .iter()
        .find(|a| a.op == Op::MoveDir)
        .expect("expected a MoveDir action");
    assert_eq!(mv.dst.as_ref().unwrap(), &t.join("3D").join("olho_grego"));
}

#[test]
fn test_never_moves_to_other_directory() {
    let d = tempdir().unwrap();
    let t = d.path();
    // A folder full of files with no recognized extension: even at 100%
    // "Other" evidence, a folder is never dumped into Other/.
    let dir = t.join("junk_drawer");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.xyz"));
    touch(&dir.join("b.xyz"));
    touch(&dir.join("c.xyz"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "junk_drawer");
    assert!(!matches!(c.decision, Decision::Move { .. }));
    assert!(!plan.actions.iter().any(|a| a
        .dst
        .as_ref()
        .map(|p| p.starts_with(t.join("Other")))
        .unwrap_or(false)));
}

#[test]
fn test_truncated_depth_never_auto_moves() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("deep_docs");
    fs::create_dir(&dir).unwrap();
    // High-confidence evidence at the top level...
    for name in ["a.pdf", "b.pdf", "c.pdf", "d.pdf"] {
        touch(&dir.join(name));
    }
    // ...plus a chain deeper than MAX_PROFILE_DEPTH, which must never be
    // fully profiled (and must never upgrade this to a Move regardless).
    let mut cur = dir.clone();
    for i in 0..(MAX_PROFILE_DEPTH + 2) {
        cur = cur.join(format!("d{i}"));
    }
    fs::create_dir_all(&cur).unwrap();
    touch(&cur.join("deep.pdf"));

    let plan = plan_folders(t.to_str().unwrap());
    let c = candidate(&plan, "deep_docs");
    assert!(c.truncated);
    assert!(matches!(
        c.decision,
        Decision::Suggest {
            reason: "profile truncated",
            ..
        }
    ));
    assert!(!plan.actions.iter().any(|a| a.op == Op::MoveDir));
}

#[test]
fn test_truncated_file_count_never_auto_moves() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("huge_docs");
    fs::create_dir(&dir).unwrap();
    for i in 0..(MAX_PROFILE_FILES + 5) {
        touch(&dir.join(format!("f{i}.pdf")));
    }

    let profile = profile_folder(&dir);
    assert!(profile.truncated);
    assert_eq!(profile.total_eligible(), MAX_PROFILE_FILES);
}

// ------------------------------------------------------------- history/undo

#[test]
fn test_apply_records_one_history_item_with_movedir() {
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    let dir = t.join("curriculo");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));

    let fp = plan_folders(t.to_str().unwrap());
    let plan = full_execution_plan(&fp);
    let (hist_id, outcomes) =
        sift::executor::execute_plan(plan, t.to_str().unwrap(), "folders", None);
    assert!(outcomes
        .iter()
        .any(|o| o.op == Op::MoveDir && o.result.is_ok()));
    assert!(t.join("Documents").join("curriculo").is_dir());
    assert!(!dir.exists());

    let files: Vec<_> = fs::read_dir(&hist_dir).unwrap().flatten().collect();
    assert_eq!(
        files.len(),
        1,
        "exactly one history record for the whole run"
    );
    let content = fs::read_to_string(files[0].path()).unwrap();
    assert!(content.contains(&hist_id));
    clear_test_history_dir();
}

#[test]
fn test_undo_reverses_movedir() {
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    let dir = t.join("curriculo");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));

    let fp = plan_folders(t.to_str().unwrap());
    let plan = full_execution_plan(&fp);
    let (hist_id, _) = sift::executor::execute_plan(plan, t.to_str().unwrap(), "folders", None);
    assert!(t.join("Documents").join("curriculo").is_dir());

    assert!(cmd_undo(hist_id));
    assert!(dir.is_dir());
    assert!(!t.join("Documents").join("curriculo").exists());
    assert!(dir.join("a.pdf").exists());
    clear_test_history_dir();
}

#[test]
fn test_undo_refuses_when_original_location_occupied() {
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    let dir = t.join("curriculo");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));

    let fp = plan_folders(t.to_str().unwrap());
    let plan = full_execution_plan(&fp);
    let (hist_id, _) = sift::executor::execute_plan(plan, t.to_str().unwrap(), "folders", None);
    assert!(t.join("Documents").join("curriculo").is_dir());

    // Something now occupies the original location.
    fs::create_dir(&dir).unwrap();

    assert!(!cmd_undo(hist_id));
    // The moved directory must remain untouched: never overwritten, never merged.
    assert!(t.join("Documents").join("curriculo").is_dir());
    assert!(t.join("Documents").join("curriculo").join("a.pdf").exists());
    clear_test_history_dir();
}

#[test]
fn test_undo_refuses_when_destination_no_longer_a_directory() {
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    let dir = t.join("curriculo");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));

    let fp = plan_folders(t.to_str().unwrap());
    let plan = full_execution_plan(&fp);
    let (hist_id, _) = sift::executor::execute_plan(plan, t.to_str().unwrap(), "folders", None);
    let moved = t.join("Documents").join("curriculo");
    assert!(moved.is_dir());

    // Replace the moved directory with something else entirely.
    fs::remove_dir_all(&moved).unwrap();
    touch(&moved);

    assert!(!cmd_undo(hist_id));
    assert!(moved.is_file());
    clear_test_history_dir();
}

#[test]
fn test_full_execution_plan_records_skips_for_non_moves() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir(t.join("mixed")).unwrap();
    touch(&t.join("mixed").join("a.pdf"));
    touch(&t.join("mixed").join("b.mp4"));
    touch(&t.join("mixed").join("c.json"));

    let fp = plan_folders(t.to_str().unwrap());
    let plan = full_execution_plan(&fp);
    assert!(plan
        .actions
        .iter()
        .any(|a| a.op == Op::Skip && a.src == t.join("mixed")));
}

// ----------------------------------------------------------------------- cli

#[test]
fn test_cmd_folders_apply_moves_eligible_folder() {
    let d = tempdir().unwrap();
    let t = d.path();
    isolate_history(t);
    let dir = t.join("artes");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("logo.svg"));
    touch(&dir.join("banner.png"));

    assert!(cmd_folders(
        t.to_str().unwrap().to_string(),
        true,
        false,
        false
    ));
    assert!(t.join("Images").join("artes").is_dir());
    clear_test_history_dir();
}

#[test]
fn test_cmd_folders_on_empty_directory_is_a_noop() {
    let d = tempdir().unwrap();
    let t = d.path();
    isolate_history(t);
    assert!(cmd_folders(
        t.to_str().unwrap().to_string(),
        true,
        false,
        false
    ));
    clear_test_history_dir();
}

#[test]
fn test_cmd_folders_json_does_not_mutate() {
    let d = tempdir().unwrap();
    let t = d.path();
    isolate_history(t);
    let dir = t.join("curriculo");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));

    assert!(cmd_folders(
        t.to_str().unwrap().to_string(),
        false,
        true,
        false
    ));
    assert!(dir.exists());
    assert!(!t.join("Documents").exists());
    clear_test_history_dir();
}

#[test]
fn test_json_output_shape() {
    let d = tempdir().unwrap();
    let t = d.path();
    let dir = t.join("curriculo");
    fs::create_dir(&dir).unwrap();
    touch(&dir.join("a.pdf"));
    touch(&dir.join("b.pdf"));

    let fp = plan_folders(t.to_str().unwrap());
    let json = sift::render::folders_json(&fp, &[]);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value.get("root").is_some());
    let folders = value.get("folders").unwrap().as_array().unwrap();
    let entry = folders
        .iter()
        .find(|f| f.get("name").unwrap() == "curriculo")
        .unwrap();
    assert_eq!(entry.get("decision").unwrap(), "move");
    assert_eq!(entry.get("category").unwrap(), "Document");
    assert!(value.get("possible_duplicates").is_some());
}

// no --recursive flag exists on `folders` at all; this is a compile-time
// guarantee (there is no field to pass), so there is no runtime test for it.

// -------------------------------------------------------------- duplicates

#[test]
fn test_duplicate_detection_numbered_suffix() {
    let names = vec![
        "alerta-verificar".to_string(),
        "alerta-verificar (2)".to_string(),
        "unrelated".to_string(),
    ];
    let groups = detect_possible_duplicates(&names);
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0],
        vec![
            "alerta-verificar".to_string(),
            "alerta-verificar (2)".to_string()
        ]
    );
}

#[test]
fn test_duplicate_detection_copy_and_backup_suffixes() {
    let names = vec![
        "project".to_string(),
        "project copy".to_string(),
        "foo".to_string(),
        "foo-backup".to_string(),
        "bar".to_string(),
        "bar-old".to_string(),
    ];
    let groups = detect_possible_duplicates(&names);
    assert_eq!(groups.len(), 3);
    assert!(groups.contains(&vec!["foo".to_string(), "foo-backup".to_string()]));
    assert!(groups.contains(&vec!["bar".to_string(), "bar-old".to_string()]));
    assert!(groups.contains(&vec!["project".to_string(), "project copy".to_string()]));
}

#[test]
fn test_duplicate_detection_no_false_positives() {
    let names = vec![
        "documents".to_string(),
        "downloads".to_string(),
        "projects".to_string(),
    ];
    assert!(detect_possible_duplicates(&names).is_empty());
}

#[test]
fn test_duplicates_never_influence_move_decisions() {
    let d = tempdir().unwrap();
    let t = d.path();
    let a = t.join("alerta-verificar");
    let b = t.join("alerta-verificar (2)");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    // Both single-file, ambiguous enough to never auto-move; the duplicate
    // relationship must not change that (or force any other decision).
    touch(&a.join("note.txt"));
    touch(&b.join("note.txt"));

    let plan = plan_folders(t.to_str().unwrap());
    assert!(!plan.possible_duplicates.is_empty());
    assert!(!plan.actions.iter().any(|act| act.op == Op::MoveDir));
    for name in ["alerta-verificar", "alerta-verificar (2)"] {
        assert!(!matches!(
            candidate(&plan, name).decision,
            Decision::Move { .. }
        ));
    }
}

// ------------------------------------------------------ duplicate removal

#[test]
fn test_dedupe_removes_identical_content_keeps_primary() {
    let d = tempdir().unwrap();
    let t = d.path();
    let a = t.join("foo");
    let b = t.join("foo (2)");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(a.join("x.txt"), b"same content").unwrap();
    fs::write(b.join("x.txt"), b"same content").unwrap();

    let fp = plan_folders(t.to_str().unwrap());
    let (actions, removals) = plan_duplicate_removals(t, &fp.possible_duplicates, &fp.candidates);
    assert_eq!(removals.len(), 1);
    assert_eq!(removals[0].keep, a.join("x.txt"));
    assert_eq!(removals[0].remove, b.join("x.txt"));
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].op, Op::Trash);
    assert_eq!(actions[0].src, b.join("x.txt"));

    // Files still exist: planning never mutates.
    assert!(a.join("x.txt").exists());
    assert!(b.join("x.txt").exists());
}

#[test]
fn test_dedupe_ignores_files_with_different_content() {
    let d = tempdir().unwrap();
    let t = d.path();
    let a = t.join("foo");
    let b = t.join("foo (2)");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(a.join("x.txt"), b"content A").unwrap();
    fs::write(b.join("x.txt"), b"different content B").unwrap();

    let fp = plan_folders(t.to_str().unwrap());
    let (actions, removals) = plan_duplicate_removals(t, &fp.possible_duplicates, &fp.candidates);
    assert!(actions.is_empty());
    assert!(removals.is_empty());
}

#[test]
fn test_dedupe_leaves_unique_file_alone() {
    let d = tempdir().unwrap();
    let t = d.path();
    let a = t.join("foo");
    let b = t.join("foo (2)");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(a.join("shared.txt"), b"shared").unwrap();
    fs::write(b.join("shared.txt"), b"shared").unwrap();
    fs::write(b.join("unique.txt"), b"only in the copy").unwrap();

    let fp = plan_folders(t.to_str().unwrap());
    let (actions, removals) = plan_duplicate_removals(t, &fp.possible_duplicates, &fp.candidates);
    assert_eq!(removals.len(), 1);
    assert_eq!(removals[0].remove, b.join("shared.txt"));
    assert!(!actions.iter().any(|a| a.src == b.join("unique.txt")));
    assert!(b.join("unique.txt").exists());
}

#[test]
fn test_dedupe_never_removes_from_primary_folder() {
    let d = tempdir().unwrap();
    let t = d.path();
    let a = t.join("foo");
    let b = t.join("foo (2)");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(a.join("x.txt"), b"same").unwrap();
    fs::write(b.join("x.txt"), b"same").unwrap();

    let fp = plan_folders(t.to_str().unwrap());
    let (actions, _) = plan_duplicate_removals(t, &fp.possible_duplicates, &fp.candidates);
    assert!(!actions.iter().any(|act| act.src.starts_with(&a)));
}

#[test]
fn test_dedupe_never_touches_protected_folder() {
    let d = tempdir().unwrap();
    let t = d.path();
    // "app" is a project root (has Cargo.toml); "app (2)" is a plain folder
    // with byte-identical content. Dedupe must never touch either side.
    let a = t.join("app");
    let b = t.join("app (2)");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(a.join("Cargo.toml"), b"[package]").unwrap();
    fs::write(b.join("Cargo.toml"), b"[package]").unwrap();

    let fp = plan_folders(t.to_str().unwrap());
    assert!(matches!(
        candidate(&fp, "app").decision,
        Decision::Protect { .. }
    ));
    let (actions, removals) = plan_duplicate_removals(t, &fp.possible_duplicates, &fp.candidates);
    assert!(actions.is_empty());
    assert!(removals.is_empty());
}

#[test]
fn test_dedupe_dry_run_never_mutates() {
    let d = tempdir().unwrap();
    let t = d.path();
    isolate_history(t);
    let a = t.join("foo");
    let b = t.join("foo (2)");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(a.join("x.txt"), b"same content").unwrap();
    fs::write(b.join("x.txt"), b"same content").unwrap();

    assert!(cmd_folders(
        t.to_str().unwrap().to_string(),
        false,
        false,
        true
    ));
    assert!(a.join("x.txt").exists());
    assert!(b.join("x.txt").exists());
    clear_test_history_dir();
}

#[test]
fn test_dedupe_apply_sends_duplicate_to_trash() {
    let d = tempdir().unwrap();
    let t = d.path();
    isolate_history(t);
    let a = t.join("foo");
    let b = t.join("foo (2)");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(a.join("x.txt"), b"same content").unwrap();
    fs::write(b.join("x.txt"), b"same content").unwrap();

    assert!(cmd_folders(
        t.to_str().unwrap().to_string(),
        true,
        false,
        true
    ));
    assert!(a.join("x.txt").exists(), "keeper must survive");
    assert!(
        !b.join("x.txt").exists(),
        "the duplicate copy must be gone from its original path"
    );
    clear_test_history_dir();
}

#[test]
fn test_dedupe_removal_not_undoable() {
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());
    let a = t.join("foo");
    let b = t.join("foo (2)");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    fs::write(a.join("x.txt"), b"same content").unwrap();
    fs::write(b.join("x.txt"), b"same content").unwrap();

    let fp = plan_folders(t.to_str().unwrap());
    let (dup_actions, _) = plan_duplicate_removals(t, &fp.possible_duplicates, &fp.candidates);
    let plan = combine_with_duplicate_removals(full_execution_plan(&fp), dup_actions);
    let (hist_id, outcomes) =
        sift::executor::execute_plan(plan, t.to_str().unwrap(), "folders", None);
    assert!(outcomes
        .iter()
        .any(|o| o.op == Op::Trash && o.result.is_ok() && !o.undoable));

    // Undo must not attempt (or claim) to restore a trashed file.
    assert!(cmd_undo(hist_id));
    assert!(!b.join("x.txt").exists());
    clear_test_history_dir();
}

// -------------------------------------------------------------------- smoke

#[test]
fn test_realistic_smoke_fixture_end_to_end() {
    let d = tempdir().unwrap();
    let t = d.path();
    let hist_dir = t.join(".sift-history");
    set_test_history_dir(hist_dir.clone());

    // 3D asset folder: mostly 3D files, one preview thumbnail.
    let olho = t.join("olho_grego");
    fs::create_dir(&olho).unwrap();
    for f in ["a.stl", "b.stl", "c.stl", "d.3mf", "e.obj"] {
        touch(&olho.join(f));
    }
    touch(&olho.join("preview.png"));

    // Documents folder.
    let curriculo = t.join("curriculo");
    fs::create_dir(&curriculo).unwrap();
    touch(&curriculo.join("cv.pdf"));
    touch(&curriculo.join("cv.docx"));
    touch(&curriculo.join("carta.pdf"));

    // Images folder.
    let artes = t.join("artes");
    fs::create_dir(&artes).unwrap();
    touch(&artes.join("logo.svg"));
    touch(&artes.join("banner.png"));
    touch(&artes.join("photo.jpg"));

    // Mixed content: left alone.
    let mixed = t.join("mixed");
    fs::create_dir(&mixed).unwrap();
    touch(&mixed.join("photo.jpg"));
    touch(&mixed.join("data.json"));
    touch(&mixed.join("movie.mp4"));

    // Unknown single file: left alone.
    let simone = t.join("Simone");
    fs::create_dir(&simone).unwrap();
    touch(&simone.join("unknown.xyz"));

    // Software project: protected outright.
    let landing = t.join("flokin-landing");
    fs::create_dir(&landing).unwrap();
    touch(&landing.join("package.json"));
    fs::create_dir(landing.join("src")).unwrap();
    touch(&landing.join("src").join("index.js"));

    // Folder containing a nested project: protected too.
    let work = t.join("Work");
    let app = work.join("app");
    fs::create_dir_all(app.join("src")).unwrap();
    touch(&app.join("Cargo.toml"));
    touch(&app.join("src").join("main.rs"));

    // Possible-duplicate pair: report only, no move.
    let dup_a = t.join("alerta-verificar");
    let dup_b = t.join("alerta-verificar (2)");
    fs::create_dir(&dup_a).unwrap();
    fs::create_dir(&dup_b).unwrap();
    touch(&dup_a.join("note.txt"));
    touch(&dup_b.join("note.txt"));

    // An existing category directory: never itself a candidate.
    fs::create_dir(t.join("Images")).unwrap();

    // --- dry run: zero mutation ---
    let dry = plan_folders(t.to_str().unwrap());
    assert!(olho.exists());
    assert!(curriculo.exists());
    assert!(artes.exists());
    assert!(!t.join("3D").exists());
    assert!(!t.join("Documents").exists());

    // --- JSON structure sanity ---
    let json = sift::render::folders_json(&dry, &[]);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value.get("folders").unwrap().as_array().unwrap().len() >= 8);

    // --- decisions ---
    assert!(matches!(
        candidate(&dry, "olho_grego").decision,
        Decision::Move {
            category: Category::ThreeD,
            ..
        }
    ));
    assert!(matches!(
        candidate(&dry, "curriculo").decision,
        Decision::Move {
            category: Category::Document,
            ..
        }
    ));
    assert!(matches!(
        candidate(&dry, "artes").decision,
        Decision::Move {
            category: Category::Image,
            ..
        }
    ));
    assert!(matches!(
        candidate(&dry, "mixed").decision,
        Decision::Leave { .. }
    ));
    assert!(matches!(
        candidate(&dry, "Simone").decision,
        Decision::Leave { .. }
    ));
    assert!(matches!(
        candidate(&dry, "flokin-landing").decision,
        Decision::Protect {
            reason: "software project"
        }
    ));
    assert!(matches!(
        candidate(&dry, "Work").decision,
        Decision::Protect {
            reason: "contains software project"
        }
    ));
    assert!(!dry.possible_duplicates.is_empty());
    assert!(!plan_folders(t.to_str().unwrap())
        .actions
        .iter()
        .any(|a| a.src == dup_a || a.src == dup_b));
    assert!(!dry.candidates.iter().any(|c| c.name == "Images"));

    // --- apply ---
    let plan = full_execution_plan(&dry);
    let (_hist_id, outcomes) =
        sift::executor::execute_plan(plan, t.to_str().unwrap(), "folders", None);
    assert!(!outcomes.iter().any(|o| o.result.is_err()));

    assert!(t.join("3D").join("olho_grego").is_dir());
    assert!(t.join("Documents").join("curriculo").is_dir());
    assert!(t.join("Images").join("artes").is_dir());
    assert!(t.join("3D").join("olho_grego").join("a.stl").exists());
    assert!(t
        .join("Documents")
        .join("curriculo")
        .join("cv.pdf")
        .exists());
    assert!(t.join("Images").join("artes").join("logo.svg").exists());
    assert!(!olho.exists());
    assert!(!curriculo.exists());
    assert!(!artes.exists());

    // Untouched.
    assert!(mixed.exists());
    assert!(simone.exists());
    assert!(landing.exists());
    assert!(work.exists());
    assert!(dup_a.exists());
    assert!(dup_b.exists());

    // --- idempotence: a second run must never produce Documents/Documents,
    // 3D/3D, or Images/Images, and must not re-touch what already moved ---
    let second = plan_folders(t.to_str().unwrap());
    assert!(!second
        .candidates
        .iter()
        .any(|c| ["3D", "Documents", "Images"].contains(&c.name.as_str())));
    assert!(!t.join("3D").join("3D").exists());
    assert!(!t.join("Documents").join("Documents").exists());
    assert!(!t.join("Images").join("Images").exists());

    clear_test_history_dir();
}
