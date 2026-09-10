//! Integration tests for Sift's `strategy = "photos"` organize strategy:
//! `MetadataTemplate` parsing/validation, EXIF extraction (via the
//! pure-Rust `kamadak-exif` crate, against real tiny fixture files under
//! `tests/fixtures/`), planning (destination rendering, missing-field
//! skip, camera/make deduplication), rule precedence, config validation,
//! `explain`, `config check`, and the `--recursive` refusal that applies
//! to this strategy in this version.
//!
//! Uses only tempdir/tempfile; every test that touches "global config"
//! isolates it first — never the user's real home or Sift data directory.

use sift::config::{EffectivePolicy, MetadataTemplate, OrganizeStrategy};
use sift::domain::Op;
use sift::explain::explain_path;
use sift::metadata::extract_photo_metadata;
use sift::planner::{plan_with_strategy, plan_with_strategy_recursive};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(FIXTURES).join(name)
}

fn copy_fixture(name: &str, dest_dir: &Path, dest_name: &str) -> std::path::PathBuf {
    let dest = dest_dir.join(dest_name);
    fs::copy(fixture(name), &dest).unwrap();
    dest
}

fn photos_policy(template: &str) -> EffectivePolicy {
    EffectivePolicy {
        strategy: OrganizeStrategy::Photos,
        metadata_template: Some(MetadataTemplate::parse_photos(template).unwrap()),
        ..EffectivePolicy::default()
    }
}

fn isolated_global<R>(f: impl FnOnce() -> R) -> R {
    let empty = tempdir().unwrap();
    sift::config::set_test_global_config_dir(empty.path().to_path_buf());
    let r = f();
    sift::config::clear_test_global_config_dir();
    r
}

// -------------------------------------------------------------- template

#[test]
fn template_photos_fields_accepted() {
    for placeholder in ["{camera}", "{year}", "{month}", "{day}"] {
        assert!(
            MetadataTemplate::parse_photos(placeholder).is_ok(),
            "{placeholder} should be a valid photos placeholder"
        );
    }
}

#[test]
fn template_audio_video_fields_rejected_for_photos() {
    for placeholder in ["{artist}", "{album}", "{width}", "{codec}", "{fps}"] {
        assert!(
            MetadataTemplate::parse_photos(placeholder).is_err(),
            "{placeholder} should NOT be a valid photos placeholder"
        );
    }
}

#[test]
fn template_unknown_placeholder_rejected() {
    assert!(MetadataTemplate::parse_photos("{banana}").is_err());
}

#[test]
fn template_empty_rejected() {
    assert!(MetadataTemplate::parse_photos("").is_err());
}

#[test]
fn template_absolute_and_traversal_rejected() {
    assert!(MetadataTemplate::parse_photos("/{camera}").is_err());
    assert!(MetadataTemplate::parse_photos("../{camera}").is_err());
}

#[test]
fn template_adjacent_placeholders_rejected() {
    assert!(MetadataTemplate::parse_photos("{year}{month}").is_err());
}

// ------------------------------------------------------------------ exif

#[test]
fn extracts_camera_and_capture_date_from_real_file() {
    let meta = extract_photo_metadata(&fixture("tiny.jpg")).unwrap();
    assert_eq!(meta.camera.as_deref(), Some("Apple iPhone 13"));
    assert_eq!(meta.year.as_deref(), Some("2022"));
    assert_eq!(meta.month.as_deref(), Some("07"));
    assert_eq!(meta.day.as_deref(), Some("04"));
}

#[test]
fn untagged_photo_yields_all_none_not_an_error() {
    let meta = extract_photo_metadata(&fixture("untagged.jpg")).unwrap();
    assert!(meta.camera.is_none());
    assert!(meta.year.is_none());
}

#[test]
fn unreadable_file_is_a_clear_error() {
    let d = tempdir().unwrap();
    let f = d.path().join("not_a_photo.jpg");
    fs::write(&f, b"this is not a real jpeg file").unwrap();
    assert!(extract_photo_metadata(&f).is_err());
}

// -------------------------------------------------------------- planning

#[test]
fn tagged_photo_lands_in_camera_year_month() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.jpg", t, "photo.jpg");
    let policy = photos_policy("{camera}/{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("photo.jpg"))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(
        a.dst.as_ref().unwrap(),
        &t.join("Apple iPhone 13/2022/07/photo.jpg")
    );
}

#[test]
fn untagged_photo_is_skipped_never_fabricated_bucket() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("untagged.jpg", t, "photo.jpg");
    let policy = photos_policy("{camera}/{year}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("photo.jpg"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(a.reason.as_deref().unwrap().contains("metadata"));
    assert!(
        !plan.actions.iter().any(|a| a.op == Op::CreateDir),
        "no 'Unknown Camera' folder is ever created"
    );
}

#[test]
fn nested_createdir_actions_for_three_level_template() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.jpg", t, "photo.jpg");
    let policy = photos_policy("{camera}/{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let creates: Vec<_> = plan
        .actions
        .iter()
        .filter(|a| a.op == Op::CreateDir)
        .collect();
    assert_eq!(creates.len(), 3);
    assert_eq!(creates[0].src, t.join("Apple iPhone 13"));
    assert_eq!(creates[1].src, t.join("Apple iPhone 13/2022"));
    assert_eq!(creates[2].src, t.join("Apple iPhone 13/2022/07"));
}

#[test]
fn destination_collision_with_different_content_is_renamed() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("Apple iPhone 13/2022")).unwrap();
    fs::write(t.join("Apple iPhone 13/2022/photo.jpg"), b"existing").unwrap();
    copy_fixture("tiny.jpg", t, "photo.jpg");
    let policy = photos_policy("{camera}/{year}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("photo.jpg") && a.src.parent() == Some(t))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(
        a.dst.as_ref().unwrap(),
        &t.join("Apple iPhone 13/2022/photo (1).jpg")
    );
    assert_eq!(
        fs::read_to_string(t.join("Apple iPhone 13/2022/photo.jpg")).unwrap(),
        "existing",
        "the pre-existing file at the colliding name must never be touched"
    );
}

// ------------------------------------------------------------ precedence

#[test]
fn explicit_skip_rule_overrides_photos() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.jpg", t, "photo.jpg");
    let rule = sift::config::Rule {
        name: "keep".into(),
        pattern: "photo.jpg".into(),
        action: "Skip".into(),
        destination: None,
        priority: 100,
        enabled: true,
        description: None,
    };
    let policy = photos_policy("{camera}/{year}").with_rules(vec![rule]);
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("photo.jpg"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(!a.reason.as_deref().unwrap().contains("iPhone"));
}

// --------------------------------------------------------------- config

#[test]
fn photos_requires_template() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(t.join(".sift.toml"), "[organize]\nstrategy = \"photos\"\n").unwrap();
        let err = sift::config::resolve_policy(t.to_str().unwrap()).unwrap_err();
        assert!(err.contains("template"));
    });
}

#[test]
fn photos_rejects_date_source_field() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"photos\"\ntemplate = \"{camera}\"\ndate_source = \"modified\"\n",
        )
        .unwrap();
        assert!(sift::config::resolve_policy(t.to_str().unwrap()).is_err());
    });
}

#[test]
fn photos_strategy_parses_from_toml() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"photos\"\ntemplate = \"{camera}/{year}\"\n",
        )
        .unwrap();
        let policy = sift::config::resolve_policy(t.to_str().unwrap()).unwrap();
        assert_eq!(policy.strategy, OrganizeStrategy::Photos);
        assert_eq!(
            policy.metadata_template.as_ref().unwrap().raw(),
            "{camera}/{year}"
        );
    });
}

// ------------------------------------------------------------- recursive

#[test]
fn photos_does_not_support_recursive() {
    assert!(!OrganizeStrategy::Photos.supports_recursive());
}

#[test]
fn recursive_photos_organize_skips_everything_with_clear_reason() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.jpg", t, "photo.jpg");
    let policy = photos_policy("{camera}/{year}");
    let rp = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("photo.jpg"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(a.reason.as_deref().unwrap().contains("recursive"));
    assert!(t.join("photo.jpg").exists(), "zero mutation");
}

// ------------------------------------------------------------------- explain

#[test]
fn explain_photos_exposes_metadata_and_destination() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = copy_fixture("tiny.jpg", t, "photo.jpg");
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"photos\"\ntemplate = \"{camera}/{year}\"\n",
    )
    .unwrap();
    let exp = explain_path(&f, t).unwrap();
    let meta = exp.photo_metadata.expect("expected photo metadata");
    assert_eq!(meta.camera.as_deref(), Some("Apple iPhone 13"));
    assert_eq!(exp.op, Op::Move);
    assert_eq!(
        exp.destination,
        Some(t.join("Apple iPhone 13/2022/photo.jpg"))
    );
}

#[test]
fn explain_photos_performs_zero_mutation() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = copy_fixture("tiny.jpg", t, "photo.jpg");
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"photos\"\ntemplate = \"{camera}/{year}\"\n",
    )
    .unwrap();
    let _ = explain_path(&f, t).unwrap();
    assert!(f.exists());
    assert!(!t.join("Apple iPhone 13").exists());
}

// --------------------------------------------------------------- config check

#[test]
fn config_check_valid_photos_policy() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(
        t.join(".sift.toml"),
        "version = 1\n[organize]\nstrategy = \"photos\"\ntemplate = \"{camera}/{year}\"\n",
    )
    .unwrap();
    let result = sift::config::resolve_policy(t.to_str().unwrap());
    let json = sift::render::config_check_json(&result);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["valid"], true);
    assert_eq!(value["strategy"], "photos");
    assert_eq!(value["template"], "{camera}/{year}");
}
