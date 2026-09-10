//! Integration tests for Sift's `strategy = "audio"` organize strategy:
//! `MetadataTemplate` parsing/validation, tag extraction (via `lofty`,
//! against real tiny fixture files under `tests/fixtures/`), planning
//! (destination rendering, missing-tag skip, sanitization), rule
//! precedence, config validation, `explain`, `config check`, Watch
//! integration (registration-time and hot-reload recursive rejection,
//! non-recursive watch organizing a real file), and the `--recursive`
//! refusal that applies to this strategy in this version.
//!
//! Uses only tempdir/tempfile; every test that touches the watch registry
//! or "global config" isolates it first — never the user's real home or
//! Sift data directory.

use sift::config::{EffectivePolicy, MetadataTemplate, OrganizeStrategy};
use sift::domain::Op;
use sift::explain::explain_path;
use sift::metadata::extract_audio_metadata;
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

fn audio_policy(template: &str) -> EffectivePolicy {
    EffectivePolicy {
        strategy: OrganizeStrategy::Audio,
        metadata_template: Some(MetadataTemplate::parse_audio(template).unwrap()),
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
fn template_audio_fields_accepted() {
    for placeholder in [
        "{artist}",
        "{album}",
        "{album_artist}",
        "{genre}",
        "{track}",
        "{title}",
        "{year}",
    ] {
        assert!(
            MetadataTemplate::parse_audio(placeholder).is_ok(),
            "{placeholder} should be a valid audio placeholder"
        );
    }
}

#[test]
fn template_video_fields_rejected_for_audio() {
    for placeholder in ["{width}", "{height}", "{resolution}", "{codec}"] {
        assert!(
            MetadataTemplate::parse_audio(placeholder).is_err(),
            "{placeholder} should NOT be a valid audio placeholder"
        );
    }
}

#[test]
fn template_unknown_placeholder_rejected() {
    assert!(MetadataTemplate::parse_audio("{banana}").is_err());
}

#[test]
fn template_empty_rejected() {
    assert!(MetadataTemplate::parse_audio("").is_err());
    assert!(MetadataTemplate::parse_audio("   ").is_err());
}

#[test]
fn template_absolute_and_traversal_rejected() {
    assert!(MetadataTemplate::parse_audio("/{artist}").is_err());
    assert!(MetadataTemplate::parse_audio("../{artist}").is_err());
    assert!(MetadataTemplate::parse_audio("{artist}/../../foo").is_err());
}

#[test]
fn template_adjacent_placeholders_rejected() {
    assert!(MetadataTemplate::parse_audio("{artist}{album}").is_err());
}

#[test]
fn template_static_component() {
    let t = MetadataTemplate::parse_audio("Music/{artist}/{album}").unwrap();
    assert_eq!(t.raw(), "Music/{artist}/{album}");
}

// ----------------------------------------------------------------- tags

#[test]
fn extracts_full_tag_set_from_real_file() {
    let meta = extract_audio_metadata(&fixture("tagged.mp3")).unwrap();
    assert_eq!(meta.artist.as_deref(), Some("Test Artist"));
    assert_eq!(meta.album.as_deref(), Some("Test Album"));
    assert_eq!(meta.album_artist.as_deref(), Some("Test Album Artist"));
    assert_eq!(meta.genre.as_deref(), Some("Test Genre"));
    assert_eq!(meta.year.as_deref(), Some("1979"));
    assert_eq!(meta.track.as_deref(), Some("3"));
    assert_eq!(meta.title.as_deref(), Some("Test Title"));
}

#[test]
fn untagged_file_yields_all_none_not_an_error() {
    let meta = extract_audio_metadata(&fixture("untagged.mp3")).unwrap();
    assert!(meta.artist.is_none());
    assert!(meta.album.is_none());
}

#[test]
fn unreadable_file_is_a_clear_error() {
    let d = tempdir().unwrap();
    let f = d.path().join("not_audio.mp3");
    fs::write(&f, b"this is not a real mp3 file").unwrap();
    assert!(extract_audio_metadata(&f).is_err());
}

// -------------------------------------------------------------- planning

#[test]
fn tagged_file_lands_in_artist_album() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tagged.mp3", t, "song.mp3");
    let policy = audio_policy("{artist}/{album}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("song.mp3"))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(
        a.dst.as_ref().unwrap(),
        &t.join("Test Artist/Test Album/song.mp3")
    );
}

#[test]
fn untagged_file_is_skipped_never_fabricated_bucket() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("untagged.mp3", t, "song.mp3");
    let policy = audio_policy("{artist}/{album}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("song.mp3"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(a.reason.as_deref().unwrap().contains("metadata"));
    assert!(
        !plan.actions.iter().any(|a| a.op == Op::CreateDir),
        "no 'Unknown Artist' folder is ever created"
    );
}

#[test]
fn nested_createdir_actions_for_two_level_template() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tagged.mp3", t, "song.mp3");
    let policy = audio_policy("{artist}/{album}");
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
    assert_eq!(creates.len(), 2);
    assert_eq!(creates[0].src, t.join("Test Artist"));
    assert_eq!(creates[1].src, t.join("Test Artist/Test Album"));
}

#[test]
fn destination_collision_with_different_content_is_renamed() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("Test Artist/Test Album")).unwrap();
    fs::write(t.join("Test Artist/Test Album/song.mp3"), b"existing").unwrap();
    copy_fixture("tagged.mp3", t, "song.mp3");
    let policy = audio_policy("{artist}/{album}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("song.mp3") && a.src.parent() == Some(t))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(
        a.dst.as_ref().unwrap(),
        &t.join("Test Artist/Test Album/song (1).mp3")
    );
    assert!(a
        .reason
        .as_deref()
        .unwrap_or("")
        .ends_with("(renamed: a different file already exists at that name)"));
    assert_eq!(
        fs::read_to_string(t.join("Test Artist/Test Album/song.mp3")).unwrap(),
        "existing",
        "the pre-existing file at the colliding name must never be touched"
    );
}

// ------------------------------------------------------------ precedence

#[test]
fn explicit_skip_rule_overrides_audio() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tagged.mp3", t, "song.mp3");
    let rule = sift::config::Rule {
        name: "keep".into(),
        pattern: "song.mp3".into(),
        action: "Skip".into(),
        destination: None,
        priority: 100,
        enabled: true,
        description: None,
    };
    let policy = audio_policy("{artist}/{album}").with_rules(vec![rule]);
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("song.mp3"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(!a.reason.as_deref().unwrap().contains("Artist"));
}

// --------------------------------------------------------------- config

#[test]
fn audio_requires_template() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(t.join(".sift.toml"), "[organize]\nstrategy = \"audio\"\n").unwrap();
        let err = sift::config::resolve_policy(t.to_str().unwrap()).unwrap_err();
        assert!(err.contains("template"));
    });
}

#[test]
fn audio_rejects_date_source_field() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}\"\ndate_source = \"modified\"\n",
        )
        .unwrap();
        assert!(sift::config::resolve_policy(t.to_str().unwrap()).is_err());
    });
}

#[test]
fn audio_strategy_parses_from_toml() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}/{album}\"\n",
        )
        .unwrap();
        let policy = sift::config::resolve_policy(t.to_str().unwrap()).unwrap();
        assert_eq!(policy.strategy, OrganizeStrategy::Audio);
        assert_eq!(
            policy.metadata_template.as_ref().unwrap().raw(),
            "{artist}/{album}"
        );
    });
}

// ------------------------------------------------------------- recursive

#[test]
fn audio_does_not_support_recursive() {
    assert!(!OrganizeStrategy::Audio.supports_recursive());
}

#[test]
fn recursive_audio_organize_skips_everything_with_clear_reason() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tagged.mp3", t, "song.mp3");
    let policy = audio_policy("{artist}/{album}");
    let rp = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("song.mp3"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(a.reason.as_deref().unwrap().contains("recursive"));
    assert!(t.join("song.mp3").exists(), "zero mutation");
}

// ------------------------------------------------------------------- watch

mod watch_audio {
    use super::*;
    use sift::watch::daemon::Daemon;
    use sift::watch::registry::{self, add, transition, WatchState};
    use std::time::{Duration, Instant};

    fn with_isolated_registry(f: impl FnOnce(&Path)) {
        let d = tempdir().unwrap();
        registry::set_test_watch_dir(d.path().join(".sift-watch-test"));
        let global_dir = d.path().join(".sift-global-config-test");
        fs::create_dir_all(&global_dir).unwrap();
        sift::config::set_test_global_config_dir(global_dir);
        f(d.path());
        sift::config::clear_test_global_config_dir();
        registry::clear_test_watch_dir();
    }

    #[test]
    fn cmd_watch_add_refuses_recursive_audio() {
        with_isolated_registry(|_reg| {
            let d = tempdir().unwrap();
            let root = d.path().canonicalize().unwrap();
            fs::write(
                root.join(".sift.toml"),
                "[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}/{album}\"\n",
            )
            .unwrap();
            let ok = sift::watch::cmd_watch_add(root.to_string_lossy().to_string(), true, true);
            assert!(!ok, "registering a recursive audio watch must be refused");
            assert!(
                registry::find(&root).unwrap().is_none(),
                "a refused watch must never be registered"
            );
        });
    }

    #[test]
    fn cmd_watch_add_allows_non_recursive_audio() {
        with_isolated_registry(|_reg| {
            let d = tempdir().unwrap();
            let root = d.path().canonicalize().unwrap();
            fs::write(
                root.join(".sift.toml"),
                "[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}/{album}\"\n",
            )
            .unwrap();
            let ok = sift::watch::cmd_watch_add(root.to_string_lossy().to_string(), true, false);
            assert!(ok);
            assert!(registry::find(&root).unwrap().is_some());
        });
    }

    #[test]
    fn hot_reload_into_recursive_incompatible_strategy_suspends() {
        with_isolated_registry(|_reg| {
            let d = tempdir().unwrap();
            let root = d.path().canonicalize().unwrap();
            // Starts as `type` (recursive-compatible) with recursive=true.
            fs::write(
                root.join(".sift.toml"),
                "[organize]\nstrategy = \"type\"\n[watch]\nstability_seconds = 1\n",
            )
            .unwrap();
            add(root.clone(), true, true).unwrap();
            transition(&root, WatchState::Running).unwrap();
            let mut daemon = Daemon::new().unwrap();
            daemon.reconcile();
            assert!(registry::find(&root)
                .unwrap()
                .unwrap()
                .config_error
                .is_none());

            // Hot-swap to `audio`, still registered recursive — must
            // suspend even though the TOML itself parses fine.
            fs::write(
                root.join(".sift.toml"),
                "[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}/{album}\"\n[watch]\nstability_seconds = 1\n",
            )
            .unwrap();
            daemon.reconcile();
            let entry = registry::find(&root).unwrap().unwrap();
            assert!(
                entry.config_error.is_some(),
                "recursive + audio must fail closed on hot reload"
            );
            assert!(entry.config_error.as_deref().unwrap().contains("recursive"));
        });
    }

    #[test]
    fn watch_audio_organizes_real_file_no_backfill() {
        with_isolated_registry(|_reg| {
            let d = tempdir().unwrap();
            let root = d.path().canonicalize().unwrap();
            copy_fixture("tagged.mp3", &root, "preexisting.mp3");
            fs::write(
                root.join(".sift.toml"),
                "[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}/{album}\"\n[watch]\nstability_seconds = 1\n",
            )
            .unwrap();
            add(root.clone(), true, false).unwrap();
            transition(&root, WatchState::Running).unwrap();
            sift::history::set_test_history_dir(root.join(".sift-history-test"));
            let mut daemon = Daemon::new().unwrap();
            daemon.reconcile();

            let f = root.join("newsong.mp3");
            copy_fixture("tagged.mp3", &root, "newsong.mp3");
            let dest = root.join("Test Artist/Test Album/newsong.mp3");

            let deadline = Instant::now() + Duration::from_secs(10);
            let mut done = false;
            while Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if dest.exists() {
                    done = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(done, "expected newsong.mp3 to be organized");
            assert!(!f.exists());
            assert!(
                root.join("preexisting.mp3").exists(),
                "no backfill of the pre-existing file"
            );
            sift::history::clear_test_history_dir();
        });
    }
}

// ------------------------------------------------------------------- explain

#[test]
fn explain_audio_exposes_metadata_and_destination() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = copy_fixture("tagged.mp3", t, "song.mp3");
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}/{album}\"\n",
    )
    .unwrap();
    let exp = explain_path(&f, t).unwrap();
    let meta = exp.audio_metadata.expect("expected audio metadata");
    assert_eq!(meta.artist.as_deref(), Some("Test Artist"));
    assert_eq!(exp.op, Op::Move);
    assert_eq!(
        exp.destination,
        Some(t.join("Test Artist/Test Album/song.mp3"))
    );
}

#[test]
fn explain_audio_performs_zero_mutation() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = copy_fixture("tagged.mp3", t, "song.mp3");
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}/{album}\"\n",
    )
    .unwrap();
    let _ = explain_path(&f, t).unwrap();
    assert!(f.exists());
    assert!(!t.join("Test Artist").exists());
}

// --------------------------------------------------------------- config check

#[test]
fn config_check_valid_audio_policy() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(
        t.join(".sift.toml"),
        "version = 1\n[organize]\nstrategy = \"audio\"\ntemplate = \"{artist}/{album}\"\n",
    )
    .unwrap();
    let result = sift::config::resolve_policy(t.to_str().unwrap());
    let json = sift::render::config_check_json(&result);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["valid"], true);
    assert_eq!(value["strategy"], "audio");
    assert_eq!(value["template"], "{artist}/{album}");
}
