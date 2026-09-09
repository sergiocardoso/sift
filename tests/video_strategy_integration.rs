//! Integration tests for Sift's `strategy = "video"` organize strategy:
//! `MetadataTemplate` parsing/validation, container metadata extraction
//! (via the optional `ffprobe` backend and the pure-Rust `mp4` crate
//! fallback, against real tiny fixture files under `tests/fixtures/`),
//! planning (destination rendering, missing-field skip), rule precedence,
//! config validation, `explain`, `config check`, and the `--recursive`
//! refusal that applies to this strategy in this version.
//!
//! Uses only tempdir/tempfile; every test that touches "global config"
//! isolates it first — never the user's real home or Sift data directory.
//! Tests that compare the `ffprobe` backend against the pure-Rust
//! fallback use `extract_video_metadata_with_ffprobe_search_path` (a
//! deliberate test seam) rather than mutating the process's real `PATH`,
//! which would race with any other test in this binary that legitimately
//! needs `ffprobe` to be found.

use sift::config::{EffectivePolicy, MetadataTemplate, OrganizeStrategy};
use sift::domain::Op;
use sift::explain::explain_path;
use sift::metadata::{extract_video_metadata, extract_video_metadata_with_ffprobe_search_path};
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

fn video_policy(template: &str) -> EffectivePolicy {
    EffectivePolicy {
        strategy: OrganizeStrategy::Video,
        metadata_template: Some(MetadataTemplate::parse_video(template).unwrap()),
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
fn template_video_fields_accepted() {
    for placeholder in ["{width}", "{height}", "{resolution}", "{codec}", "{year}"] {
        assert!(
            MetadataTemplate::parse_video(placeholder).is_ok(),
            "{placeholder} should be a valid video placeholder"
        );
    }
}

#[test]
fn template_audio_fields_rejected_for_video() {
    for placeholder in ["{artist}", "{album}", "{album_artist}", "{track}"] {
        assert!(
            MetadataTemplate::parse_video(placeholder).is_err(),
            "{placeholder} should NOT be a valid video placeholder"
        );
    }
}

#[test]
fn template_unknown_placeholder_rejected() {
    assert!(MetadataTemplate::parse_video("{banana}").is_err());
}

#[test]
fn template_absolute_and_traversal_rejected() {
    assert!(MetadataTemplate::parse_video("/{resolution}").is_err());
    assert!(MetadataTemplate::parse_video("../{resolution}").is_err());
}

#[test]
fn template_adjacent_placeholders_rejected() {
    assert!(MetadataTemplate::parse_video("{width}{height}").is_err());
}

// ----------------------------------------------------------------- probe

#[test]
fn extracts_dimensions_and_codec_from_real_file() {
    let meta = extract_video_metadata(&fixture("tiny.mp4")).unwrap();
    assert_eq!(meta.width, 64);
    assert_eq!(meta.height, 48);
    assert_eq!(meta.codec.as_deref(), Some("avc1"));
}

#[test]
fn creation_time_left_unset_by_encoder_is_unavailable_not_guessed() {
    // ffmpeg does not set mvhd.creation_time for this fixture — this is
    // the common case, and it must render as "no year", never a value
    // silently guessed from the file's mtime.
    let meta = extract_video_metadata(&fixture("tiny.mp4")).unwrap();
    assert!(meta.year.is_none());
}

#[test]
fn unreadable_file_is_a_clear_error() {
    let d = tempdir().unwrap();
    let f = d.path().join("not_video.mp4");
    fs::write(&f, b"this is not a real mp4 file").unwrap();
    assert!(extract_video_metadata(&f).is_err());
}

// ------------------------------------------------------------- ffprobe

#[test]
fn ffprobe_backend_yields_duration_fps_and_year() {
    let meta = extract_video_metadata(&fixture("tiny_with_creation_time.mp4")).unwrap();
    assert_eq!(meta.width, 32);
    assert_eq!(meta.height, 32);
    assert_eq!(meta.codec.as_deref(), Some("avc1"));
    assert_eq!(meta.year.as_deref(), Some("2021"));
    assert_eq!(meta.duration_seconds, Some(2));
    assert_eq!(meta.fps, Some(30));
}

#[test]
fn fallback_backend_agrees_on_shared_fields_but_never_sets_duration_or_fps() {
    let f = fixture("tiny_with_creation_time.mp4");
    let via_ffprobe = extract_video_metadata(&f).unwrap();
    let via_fallback = extract_video_metadata_with_ffprobe_search_path(&f, "").unwrap();

    // Fields both backends can produce must render identically, so a
    // template written before ffmpeg was installed never silently points
    // somewhere new afterward.
    assert_eq!(via_ffprobe.width, via_fallback.width);
    assert_eq!(via_ffprobe.height, via_fallback.height);
    assert_eq!(via_ffprobe.codec, via_fallback.codec);
    assert_eq!(via_ffprobe.year, via_fallback.year);

    // ffprobe-only fields are always absent from the pure-Rust fallback —
    // never guessed.
    assert!(via_fallback.duration_seconds.is_none());
    assert!(via_fallback.fps.is_none());
    assert!(via_ffprobe.duration_seconds.is_some());
    assert!(via_ffprobe.fps.is_some());
}

#[test]
fn empty_ffprobe_search_path_never_spawns_a_process() {
    // A file `ffprobe` itself couldn't parse: if the empty-search-path
    // call still succeeds (via the mp4 crate fallback), `find_ffprobe`
    // never attempted to run anything against a bogus binary.
    let meta = extract_video_metadata_with_ffprobe_search_path(&fixture("tiny.mp4"), "").unwrap();
    assert_eq!((meta.width, meta.height), (64, 48));
}

// -------------------------------------------------------------- planning

#[test]
fn video_file_lands_in_resolution_folder() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.mp4", t, "clip.mp4");
    let policy = video_policy("{resolution}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("clip.mp4"))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("64x48/clip.mp4"));
}

#[test]
fn missing_year_field_is_skipped_never_fabricated() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.mp4", t, "clip.mp4");
    let policy = video_policy("{year}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("clip.mp4"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(a.reason.as_deref().unwrap().contains("metadata"));
    assert!(!plan.actions.iter().any(|a| a.op == Op::CreateDir));
}

#[test]
fn codec_placeholder_renders_fourcc() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.mp4", t, "clip.mp4");
    let policy = video_policy("{codec}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("clip.mp4"))
        .unwrap();
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("avc1/clip.mp4"));
}

#[test]
fn duration_and_fps_placeholders_render_via_ffprobe() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny_with_creation_time.mp4", t, "clip.mp4");
    let policy = video_policy("{duration}s/{fps}fps");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("clip.mp4"))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("2s/30fps/clip.mp4"));
}

// ------------------------------------------------------------ precedence

#[test]
fn explicit_skip_rule_overrides_video() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.mp4", t, "clip.mp4");
    let rule = sift::config::Rule {
        name: "keep".into(),
        pattern: "clip.mp4".into(),
        action: "Skip".into(),
        destination: None,
        priority: 100,
        enabled: true,
        description: None,
    };
    let policy = video_policy("{resolution}").with_rules(vec![rule]);
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("clip.mp4"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(!a.reason.as_deref().unwrap().contains("64x48"));
}

// --------------------------------------------------------------- config

#[test]
fn video_requires_template() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(t.join(".sift.toml"), "[organize]\nstrategy = \"video\"\n").unwrap();
        let err = sift::config::resolve_policy(t.to_str().unwrap()).unwrap_err();
        assert!(err.contains("template"));
    });
}

#[test]
fn video_rejects_date_source_field() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"video\"\ntemplate = \"{resolution}\"\ndate_source = \"modified\"\n",
        )
        .unwrap();
        assert!(sift::config::resolve_policy(t.to_str().unwrap()).is_err());
    });
}

#[test]
fn video_strategy_parses_from_toml() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"video\"\ntemplate = \"{resolution}\"\n",
        )
        .unwrap();
        let policy = sift::config::resolve_policy(t.to_str().unwrap()).unwrap();
        assert_eq!(policy.strategy, OrganizeStrategy::Video);
        assert_eq!(
            policy.metadata_template.as_ref().unwrap().raw(),
            "{resolution}"
        );
    });
}

// ------------------------------------------------------------- recursive

#[test]
fn video_does_not_support_recursive() {
    assert!(!OrganizeStrategy::Video.supports_recursive());
}

#[test]
fn recursive_video_organize_skips_everything_with_clear_reason() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.mp4", t, "clip.mp4");
    let policy = video_policy("{resolution}");
    let rp = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("clip.mp4"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(a.reason.as_deref().unwrap().contains("recursive"));
    assert!(t.join("clip.mp4").exists(), "zero mutation");
}

// ------------------------------------------------------------------- explain

#[test]
fn explain_video_exposes_metadata_and_destination() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = copy_fixture("tiny.mp4", t, "clip.mp4");
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"video\"\ntemplate = \"{resolution}\"\n",
    )
    .unwrap();
    let exp = explain_path(&f, t).unwrap();
    let meta = exp.video_metadata.expect("expected video metadata");
    assert_eq!((meta.width, meta.height), (64, 48));
    assert_eq!(exp.op, Op::Move);
    assert_eq!(exp.destination, Some(t.join("64x48/clip.mp4")));
}

#[test]
fn explain_video_performs_zero_mutation() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = copy_fixture("tiny.mp4", t, "clip.mp4");
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"video\"\ntemplate = \"{resolution}\"\n",
    )
    .unwrap();
    let _ = explain_path(&f, t).unwrap();
    assert!(f.exists());
    assert!(!t.join("64x48").exists());
}

// --------------------------------------------------------------- config check

#[test]
fn config_check_valid_video_policy() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(
        t.join(".sift.toml"),
        "version = 1\n[organize]\nstrategy = \"video\"\ntemplate = \"{resolution}\"\n",
    )
    .unwrap();
    let result = sift::config::resolve_policy(t.to_str().unwrap());
    let json = sift::render::config_check_json(&result);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["valid"], true);
    assert_eq!(value["strategy"], "video");
    assert_eq!(value["template"], "{resolution}");
}
