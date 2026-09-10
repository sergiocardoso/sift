//! Integration tests for Sift's `strategy = "date"` destination-template
//! organize strategy: template parsing/validation, date metadata
//! extraction, planning (single-level and multi-level CreateDir,
//! deduplication, partial hierarchies, collisions), rule precedence,
//! `type` strategy regression, recursive organize (local context, no
//! self-nesting, idempotence), Watch integration (hot reload, fail-closed,
//! no backfill, self-generated-event suppression), `explain`, `config
//! check`, executor safety, and history/undo.
//!
//! Uses only tempdir/tempfile; every test that touches the watch registry
//! or "global config" isolates it first — never the user's real home or
//! Sift data directory. File mtimes are set deterministically (noon UTC on
//! a fixed date) so no test depends on the machine's timezone or on when
//! it happens to run.

use sift::config::{DateMetadata, DateSource, EffectivePolicy, Template};
use sift::domain::Op;
use sift::explain::explain_path;
use sift::planner::{plan_with_strategy, plan_with_strategy_recursive};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};
use tempfile::tempdir;

// ------------------------------------------------------------- fixtures

/// Exact inverse of `utils::civil_from_unix_secs` (the well-known
/// Howard Hinnant `days_from_civil` algorithm), used only here to build
/// deterministic test fixtures — never part of the shipped crate, since
/// Sift only ever needs timestamp -> calendar date, not the reverse.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m + 9) % 12; // [0, 11]: Mar=0 .. Feb=11
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

fn unix_secs_for(year: i64, month: u32, day: u32) -> i64 {
    days_from_civil(year, month as i64, day as i64) * 86400 + 12 * 3600 // noon UTC
}

fn touch_dated(path: &Path, year: i64, month: u32, day: u32) {
    fs::write(path, b"x").unwrap();
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    let secs = unix_secs_for(year, month, day);
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64))
        .unwrap();
}

fn date_policy(template: &str) -> EffectivePolicy {
    EffectivePolicy {
        strategy: sift::config::OrganizeStrategy::Date,
        template: Some(Template::parse(template).unwrap()),
        date_source: Some(DateSource::Modified),
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

#[test]
fn fixture_civil_date_roundtrips() {
    for (y, m, d) in [
        (2026, 9, 9),
        (2026, 8, 15),
        (2025, 12, 1),
        (2000, 1, 1),
        (1999, 2, 28),
    ] {
        let secs = unix_secs_for(y, m, d);
        let meta = DateMetadata::from_unix_secs(secs);
        assert_eq!((meta.year, meta.month, meta.day), (y, m, d));
    }
}

// --------------------------------------------------------------- template

#[test]
fn template_year_only() {
    let t = Template::parse("{year}").unwrap();
    let d = DateMetadata {
        year: 2026,
        month: 9,
        day: 9,
    };
    assert_eq!(t.render(&d).unwrap(), Path::new("2026"));
}

#[test]
fn template_year_month() {
    let t = Template::parse("{year}/{month}").unwrap();
    let d = DateMetadata {
        year: 2026,
        month: 9,
        day: 9,
    };
    assert_eq!(t.render(&d).unwrap(), Path::new("2026/09"));
}

#[test]
fn template_year_month_day() {
    let t = Template::parse("{year}/{month}/{day}").unwrap();
    let d = DateMetadata {
        year: 2026,
        month: 9,
        day: 9,
    };
    assert_eq!(t.render(&d).unwrap(), Path::new("2026/09/09"));
}

#[test]
fn template_static_component() {
    let t = Template::parse("{year}/Invoices").unwrap();
    let d = DateMetadata {
        year: 2026,
        month: 9,
        day: 9,
    };
    assert_eq!(t.render(&d).unwrap(), Path::new("2026/Invoices"));

    let t2 = Template::parse("Archive/{year}/{month}").unwrap();
    assert_eq!(t2.render(&d).unwrap(), Path::new("Archive/2026/09"));
}

#[test]
fn template_unknown_placeholder_rejected() {
    assert!(Template::parse("{banana}").is_err());
    assert!(Template::parse("{year}/{banana}").is_err());
}

#[test]
fn template_absolute_rejected() {
    assert!(Template::parse("/{year}").is_err());
}

#[test]
fn template_parent_traversal_rejected() {
    assert!(Template::parse("../{year}").is_err());
    assert!(Template::parse("{year}/../../foo").is_err());
}

#[test]
fn template_empty_rejected() {
    assert!(Template::parse("").is_err());
    assert!(Template::parse("   ").is_err());
}

#[test]
fn template_rendered_destination_is_relative() {
    let t = Template::parse("{year}/{month}").unwrap();
    let d = DateMetadata {
        year: 2026,
        month: 9,
        day: 9,
    };
    let rendered = t.render(&d).unwrap();
    assert!(rendered.is_relative());
    assert!(!rendered.to_string_lossy().contains(".."));
}

#[test]
fn template_month_and_day_zero_padded() {
    let t = Template::parse("{year}/{month}/{day}").unwrap();
    let d = DateMetadata {
        year: 2026,
        month: 1,
        day: 5,
    };
    assert_eq!(t.render(&d).unwrap(), Path::new("2026/01/05"));
}

// ------------------------------------------------------------------ date

#[test]
fn known_mtime_yields_expected_year() {
    let d = tempdir().unwrap();
    let f = d.path().join("a.pdf");
    touch_dated(&f, 2026, 9, 9);
    let meta = sift::planner::extract_date_metadata(&f, DateSource::Modified).unwrap();
    assert_eq!(meta.year, 2026);
    assert_eq!(meta.month, 9);
    assert_eq!(meta.day, 9);
}

#[test]
fn date_source_obtains_metadata_without_content_reads() {
    let d = tempdir().unwrap();
    let f = d.path().join("a.pdf");
    // A file with NO readable content-derived signal at all — if this
    // still resolves correctly, the extraction can't be reading bytes.
    fs::write(&f, b"").unwrap();
    let secs = unix_secs_for(2026, 9, 9);
    let file = fs::OpenOptions::new().write(true).open(&f).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64))
        .unwrap();
    let meta = sift::planner::extract_date_metadata(&f, DateSource::Modified).unwrap();
    assert_eq!((meta.year, meta.month, meta.day), (2026, 9, 9));
}

// -------------------------------------------------------------- planning

#[test]
fn date_file_correct_destination() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("invoice.pdf"))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("2026/09/invoice.pdf"));
}

#[test]
fn explicit_nested_createdir_actions_in_order() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
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
    assert_eq!(creates[0].src, t.join("2026"));
    assert_eq!(creates[1].src, t.join("2026/09"));
}

#[test]
fn shared_destination_directories_deduplicated() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    touch_dated(&t.join("contract.pdf"), 2026, 9, 9);
    touch_dated(&t.join("report.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
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
    assert_eq!(
        creates.len(),
        2,
        "one 2026/ and one 2026/09/, never duplicated per file"
    );
    let moves = plan.actions.iter().filter(|a| a.op == Op::Move).count();
    assert_eq!(moves, 3);
}

#[test]
fn partially_existing_hierarchy_only_creates_missing_level() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir(t.join("2026")).unwrap();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
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
    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0].src, t.join("2026/09"));
}

#[test]
fn file_occupying_year_directory_blocks() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("2026"), b"not a directory").unwrap();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("invoice.pdf"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("collision"));
    assert!(!plan.actions.iter().any(|a| a.op == Op::CreateDir));
}

#[test]
fn symlink_occupying_year_directory_blocks() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    let outside = tempdir().unwrap();
    symlink(outside.path(), t.join("2026")).unwrap();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("invoice.pdf"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("collision"));
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn broken_symlink_occupying_destination_blocks() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir(t.join("2026")).unwrap();
    symlink(t.join("2026/09-nonexistent-target"), t.join("2026/09")).unwrap();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("invoice.pdf"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("collision"));
}

#[test]
fn destination_file_collision_with_different_content_is_renamed() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("2026/09")).unwrap();
    fs::write(t.join("2026/09/invoice.pdf"), b"existing").unwrap();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("invoice.pdf") && a.src.parent() == Some(t))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("2026/09/invoice (1).pdf"));
    assert_eq!(
        fs::read_to_string(t.join("2026/09/invoice.pdf")).unwrap(),
        "existing",
        "the pre-existing file at the colliding name must never be touched"
    );
}

#[test]
fn blocked_date_collision_produces_no_move_action() {
    // A destination occupied by something automatic resolution must never
    // touch on its own (here: a broken symlink) still refuses at plan
    // time — no Move action is ever produced, so nothing could later be
    // wrongly marked successful.
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("2026/09")).unwrap();
    std::os::unix::fs::symlink("/nonexistent", t.join("2026/09/invoice.pdf")).unwrap();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    assert!(!plan
        .actions
        .iter()
        .any(|a| a.op == Op::Move && a.src.parent() == Some(t)));
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("invoice.pdf") && a.src.parent() == Some(t))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("collision"));
}

#[test]
fn date_apply_renames_on_different_content_never_overwrites() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("2026/09")).unwrap();
    fs::write(t.join("2026/09/invoice.pdf"), b"existing").unwrap();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let hist_dir = t.join(".sift-history-test");
    sift::history::set_test_history_dir(hist_dir);
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    sift::executor::execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert_eq!(
        fs::read_to_string(t.join("2026/09/invoice.pdf")).unwrap(),
        "existing",
        "the pre-existing file must never be overwritten"
    );
    assert_eq!(
        fs::read_to_string(t.join("2026/09/invoice (1).pdf")).unwrap(),
        "x",
        "the new file must land at a disambiguated name"
    );
    assert!(
        !t.join("invoice.pdf").exists(),
        "source is gone once successfully organized under the disambiguated name"
    );
    sift::history::clear_test_history_dir();
}

// ------------------------------------------------------------ precedence

#[test]
fn explicit_move_rule_overrides_date() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("movie.torrent"), 2026, 9, 9);
    let rule = sift::config::Rule {
        name: "torrents".into(),
        pattern: "*.torrent".into(),
        action: "Move".into(),
        destination: Some("Torrents".into()),
        priority: 100,
        enabled: true,
        description: None,
    };
    let policy = date_policy("{year}/{month}").with_rules(vec![rule]);
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("movie.torrent"))
        .unwrap();
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("Torrents/movie.torrent"));
}

#[test]
fn explicit_skip_rule_overrides_date() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("keep.pdf"), 2026, 9, 9);
    let rule = sift::config::Rule {
        name: "keep".into(),
        pattern: "keep.pdf".into(),
        action: "Skip".into(),
        destination: None,
        priority: 100,
        enabled: true,
        description: None,
    };
    let policy = date_policy("{year}/{month}").with_rules(vec![rule]);
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("keep.pdf"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(!a.reason.as_deref().unwrap().contains("2026"));
}

#[test]
fn immutable_safety_overrides_rule_and_date() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    let target = tempdir().unwrap();
    symlink(target.path(), t.join("link.pdf")).unwrap();
    let rule = sift::config::Rule {
        name: "any".into(),
        pattern: "*.pdf".into(),
        action: "Move".into(),
        destination: Some("Documents".into()),
        priority: 100,
        enabled: true,
        description: None,
    };
    let policy = date_policy("{year}/{month}").with_rules(vec![rule]);
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("link.pdf"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert_eq!(a.reason.as_deref(), Some("symlink"));
}

// ------------------------------------------------------------- type regression

#[test]
fn type_pdf_still_documents() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("a.pdf"), b"x").unwrap();
    let policy = EffectivePolicy::default();
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("a.pdf"))
        .unwrap();
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("Documents/a.pdf"));
}

#[test]
fn type_json_still_data() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("a.json"), b"x").unwrap();
    let policy = EffectivePolicy::default();
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("a.json"))
        .unwrap();
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("Data/a.json"));
}

#[test]
fn type_unknown_still_other() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(t.join("a.xyzabc"), b"x").unwrap();
    let policy = EffectivePolicy::default();
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("a.xyzabc"))
        .unwrap();
    assert_eq!(a.dst.as_ref().unwrap(), &t.join("Other/a.xyzabc"));
}

#[test]
fn type_strategy_does_not_require_template() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(t.join(".sift.toml"), "[organize]\nstrategy = \"type\"\n").unwrap();
        let policy = sift::config::resolve_policy(t.to_str().unwrap()).unwrap();
        assert!(policy.template.is_none());
    });
}

#[test]
fn type_strategy_rejects_template_field() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"type\"\ntemplate = \"{year}\"\n",
        )
        .unwrap();
        assert!(sift::config::resolve_policy(t.to_str().unwrap()).is_err());
    });
}

#[test]
fn date_requires_template() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(t.join(".sift.toml"), "[organize]\nstrategy = \"date\"\n").unwrap();
        let err = sift::config::resolve_policy(t.to_str().unwrap()).unwrap_err();
        assert!(err.contains("template"));
    });
}

#[test]
fn unsupported_date_source_rejected() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"date\"\ntemplate = \"{year}\"\ndate_source = \"created\"\n",
        )
        .unwrap();
        assert!(sift::config::resolve_policy(t.to_str().unwrap()).is_err());
    });
}

// ------------------------------------------------------------------- recursive

#[test]
fn nested_files_use_own_local_date_context() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("one.pdf"), 2026, 9, 9);
    fs::create_dir(t.join("Client")).unwrap();
    touch_dated(&t.join("Client/two.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let rp = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let one = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("one.pdf"))
        .unwrap();
    assert_eq!(one.dst.as_ref().unwrap(), &t.join("2026/09/one.pdf"));
    let two = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("two.pdf"))
        .unwrap();
    assert_eq!(two.dst.as_ref().unwrap(), &t.join("Client/2026/09/two.pdf"));
    assert!(!rp.plan.actions.iter().any(|a| a
        .dst
        .as_ref()
        .is_some_and(|d| d.ends_with("2026/09/two.pdf") && !d.starts_with(t.join("Client")))));
}

#[test]
fn recursive_date_dry_run_zero_mutation() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("one.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let _ = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    assert!(t.join("one.pdf").exists());
    assert!(!t.join("2026").exists());
}

#[test]
fn recursive_date_apply_works() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("one.pdf"), 2026, 9, 9);
    fs::create_dir(t.join("Client")).unwrap();
    touch_dated(&t.join("Client/two.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let hist_dir = t.join(".sift-history-test");
    sift::history::set_test_history_dir(hist_dir);
    let rp = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    sift::executor::execute_plan(rp.plan, t.to_str().unwrap(), "organize", None);
    assert!(t.join("2026/09/one.pdf").exists());
    assert!(t.join("Client/2026/09/two.pdf").exists());
    sift::history::clear_test_history_dir();
}

#[test]
fn discovery_frozen_before_mutation_no_date_recursion_same_run() {
    // A directory literally named like a rendered destination, sitting
    // there BEFORE this run starts, must never be entered or reprocessed
    // — discovery is a read-only snapshot taken once, up front.
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("2026/09")).unwrap();
    fs::write(t.join("2026/09/preexisting.pdf"), b"x").unwrap();
    touch_dated(&t.join("new.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let rp = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    assert!(!rp
        .plan
        .actions
        .iter()
        .any(|a| a.src.ends_with("preexisting.pdf")));
    assert!(rp
        .plan
        .actions
        .iter()
        .any(|a| a.src.ends_with("2026") && a.op == Op::Skip));
}

#[test]
fn no_date_destination_recursion_into_generated_dir() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("2026/09")).unwrap();
    fs::write(t.join("2026/09/already-there.pdf"), b"x").unwrap();
    let policy = date_policy("{year}/{month}");
    let rp = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    assert_eq!(
        rp.dirs_scanned, 1,
        "must never descend into the generated 2026/ tree"
    );
}

#[test]
fn second_date_run_is_idempotent() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("one.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let hist_dir = t.join(".sift-history-test");
    sift::history::set_test_history_dir(hist_dir);

    let rp1 = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    sift::executor::execute_plan(rp1.plan, t.to_str().unwrap(), "organize", None);
    assert!(t.join("2026/09/one.pdf").exists());

    let rp2 = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    sift::executor::execute_plan(rp2.plan, t.to_str().unwrap(), "organize", None);
    assert!(
        !t.join("2026/09/2026").exists(),
        "second run must never nest 2026/09/2026/09"
    );
    assert!(t.join("2026/09/one.pdf").exists());
    sift::history::clear_test_history_dir();
}

// ---------------------------------------------------------------------- watch

mod watch_date {
    use super::*;
    use sift::watch::daemon::Daemon;
    use sift::watch::registry::{self, add, transition, WatchState};
    use std::time::Instant;

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

    fn canon(p: &Path) -> std::path::PathBuf {
        fs::create_dir_all(p).unwrap();
        p.canonicalize().unwrap()
    }

    #[test]
    fn watch_date_organizes_new_event_no_backfill() {
        let d = tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        with_isolated_registry(|_reg| {
            fs::write(root.join("preexisting.pdf"), b"x").unwrap();
            fs::write(
                root.join(".sift.toml"),
                "version = 1\n[organize]\nstrategy = \"date\"\ntemplate = \"{year}/{month}\"\n[watch]\nstability_seconds = 1\n",
            )
            .unwrap();
            add(root.clone(), true, false).unwrap();
            transition(&root, WatchState::Running).unwrap();

            sift::history::set_test_history_dir(root.join(".sift-history-test"));
            let mut daemon = Daemon::new().unwrap();
            daemon.reconcile();

            let f = root.join("newfile.pdf");
            fs::write(&f, b"x").unwrap();

            let deadline = Instant::now() + Duration::from_secs(10);
            let mut done = false;
            while Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if fs::read_dir(&root)
                    .unwrap()
                    .flatten()
                    .any(|e| e.path().is_dir() && e.file_name().to_string_lossy().len() == 4)
                {
                    done = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(done, "expected a YYYY/ directory to appear");
            assert!(!f.exists(), "newfile.pdf moved");
            assert!(
                root.join("preexisting.pdf").exists(),
                "no backfill of the pre-existing file"
            );
            sift::history::clear_test_history_dir();
        });
    }

    #[test]
    fn watch_date_hot_reload_deepens_template_future_events_only() {
        let d = tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        with_isolated_registry(|_reg| {
            fs::write(
                root.join(".sift.toml"),
                "version = 1\n[organize]\nstrategy = \"date\"\ntemplate = \"{year}/{month}\"\n[watch]\nstability_seconds = 1\n",
            )
            .unwrap();
            add(root.clone(), true, false).unwrap();
            transition(&root, WatchState::Running).unwrap();
            sift::history::set_test_history_dir(root.join(".sift-history-test"));
            let mut daemon = Daemon::new().unwrap();
            daemon.reconcile();

            let first = root.join("first.pdf");
            fs::write(&first, b"x").unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut moved_year_dir = None;
            while Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if let Some(dir) = fs::read_dir(&root)
                    .unwrap()
                    .flatten()
                    .find(|e| e.path().is_dir() && e.file_name().to_string_lossy().len() == 4)
                {
                    moved_year_dir = Some(dir.path());
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let year_dir = moved_year_dir.expect("first.pdf should have been organized");

            // Deepen the template.
            fs::write(
                root.join(".sift.toml"),
                "version = 1\n[organize]\nstrategy = \"date\"\ntemplate = \"{year}/{month}/{day}\"\n[watch]\nstability_seconds = 1\n",
            )
            .unwrap();
            let second = root.join("second.pdf");
            fs::write(&second, b"x").unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut done = false;
            while Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if !second.exists() {
                    done = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                done,
                "second.pdf should have been organized under the new (deeper) template"
            );
            // Old file must not be retroactively moved under the new template.
            assert!(canon(&year_dir).exists());
            let old_month_dir = fs::read_dir(&year_dir)
                .unwrap()
                .flatten()
                .find(|e| e.path().is_dir())
                .unwrap()
                .path();
            assert!(
                old_month_dir.join("first.pdf").exists(),
                "first.pdf must remain exactly where the OLD template put it"
            );
            sift::history::clear_test_history_dir();
        });
    }

    #[test]
    fn watch_date_invalid_template_suspends_and_recovers() {
        let d = tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        with_isolated_registry(|_reg| {
            fs::write(
                root.join(".sift.toml"),
                "version = 1\n[organize]\nstrategy = \"date\"\ntemplate = \"{year}\"\n",
            )
            .unwrap();
            add(root.clone(), true, false).unwrap();
            transition(&root, WatchState::Running).unwrap();
            sift::history::set_test_history_dir(root.join(".sift-history-test"));
            let mut daemon = Daemon::new().unwrap();
            daemon.reconcile();
            assert!(registry::find(&root)
                .unwrap()
                .unwrap()
                .config_error
                .is_none());

            // Attempt a parent-traversal escape via the template.
            fs::write(
                root.join(".sift.toml"),
                "version = 1\n[organize]\nstrategy = \"date\"\ntemplate = \"../../escape/{year}\"\n",
            )
            .unwrap();
            daemon.reconcile();
            let entry = registry::find(&root).unwrap().unwrap();
            assert!(entry.config_error.is_some());

            let blocked = root.join("blocked.pdf");
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if !blocked.exists() {
                    fs::write(&blocked, b"x").unwrap();
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(blocked.exists(), "never moved while suspended");
            assert!(
                !root.parent().unwrap().join("escape").exists(),
                "never escapes the root"
            );

            // Restore a valid template.
            fs::write(
                root.join(".sift.toml"),
                "version = 1\n[organize]\nstrategy = \"date\"\ntemplate = \"{year}\"\n[watch]\nstability_seconds = 1\n",
            )
            .unwrap();
            daemon.reconcile();
            assert!(registry::find(&root)
                .unwrap()
                .unwrap()
                .config_error
                .is_none());

            let after_fix = root.join("after_fix.pdf");
            fs::write(&after_fix, b"x").unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut done = false;
            while Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if !after_fix.exists() {
                    done = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(done, "future events resume once policy is valid again");
            assert!(blocked.exists(), "blocked.pdf is never backfilled");
            sift::history::clear_test_history_dir();
        });
    }

    #[test]
    fn watch_self_generated_destination_event_ignored_recursive() {
        let d = tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        with_isolated_registry(|_reg| {
            fs::write(
                root.join(".sift.toml"),
                "version = 1\n[organize]\nstrategy = \"date\"\ntemplate = \"{year}/{month}\"\n[watch]\nstability_seconds = 1\n",
            )
            .unwrap();
            add(root.clone(), true, true).unwrap(); // recursive
            transition(&root, WatchState::Running).unwrap();
            sift::history::set_test_history_dir(root.join(".sift-history-test"));
            let mut daemon = Daemon::new().unwrap();
            daemon.reconcile();

            let f = root.join("invoice.pdf");
            fs::write(&f, b"x").unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
                if !f.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(!f.exists(), "invoice.pdf should have moved");

            // Give the daemon a few more ticks to observe (and potentially
            // mis-process) the self-generated destination-creation events.
            for _ in 0..10 {
                daemon.reconcile();
                daemon.drain_events(Duration::from_millis(100));
                daemon.process_ready();
            }
            // Locate wherever it landed and confirm no re-nesting occurred.
            let year_dir = fs::read_dir(&root)
                .unwrap()
                .flatten()
                .find(|e| e.path().is_dir() && e.file_name().to_string_lossy().len() == 4)
                .expect("expected a YYYY directory")
                .path();
            let month_dir = fs::read_dir(&year_dir)
                .unwrap()
                .flatten()
                .find(|e| e.path().is_dir())
                .expect("expected a MM directory")
                .path();
            assert!(month_dir.join("invoice.pdf").exists());
            assert!(!month_dir.join(year_dir.file_name().unwrap()).exists());
            sift::history::clear_test_history_dir();
        });
    }
}

// ------------------------------------------------------------------- explain

#[test]
fn explain_date_exposes_metadata() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = t.join("invoice.pdf");
    touch_dated(&f, 2026, 9, 9);
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"date\"\ntemplate = \"{year}/{month}\"\n",
    )
    .unwrap();
    let exp = explain_path(&f, t).unwrap();
    let meta = exp.date_metadata.expect("expected date metadata");
    assert_eq!((meta.year, meta.month, meta.day), (2026, 9, 9));
}

#[test]
fn explain_date_shows_rendered_destination() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = t.join("invoice.pdf");
    touch_dated(&f, 2026, 9, 9);
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"date\"\ntemplate = \"{year}/{month}\"\n",
    )
    .unwrap();
    let exp = explain_path(&f, t).unwrap();
    assert_eq!(exp.destination, Some(t.join("2026/09/invoice.pdf")));
    assert_eq!(exp.op, Op::Move);
}

#[test]
fn explain_date_rule_override_is_explicit() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = t.join("invoice.pdf");
    touch_dated(&f, 2026, 9, 9);
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"date\"\ntemplate = \"{year}/{month}\"\n[[rules]]\npattern = \"*.pdf\"\naction = \"Move\"\ndestination = \"Documents\"\npriority = 1\nenabled = true\n",
    )
    .unwrap();
    let exp = explain_path(&f, t).unwrap();
    assert!(exp.matched_rule.is_some());
    assert_eq!(exp.destination, Some(t.join("Documents/invoice.pdf")));
    assert!(
        exp.date_metadata.is_none(),
        "date metadata never computed once a rule wins"
    );
}

#[test]
fn explain_date_performs_zero_mutation() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = t.join("invoice.pdf");
    touch_dated(&f, 2026, 9, 9);
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"date\"\ntemplate = \"{year}/{month}\"\n",
    )
    .unwrap();
    let _ = explain_path(&f, t).unwrap();
    assert!(f.exists());
    assert!(!t.join("2026").exists());
}

// --------------------------------------------------------------- config check

#[test]
fn config_check_valid_date_policy() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(
        t.join(".sift.toml"),
        "version = 1\n[organize]\nstrategy = \"date\"\ntemplate = \"{year}/{month}\"\ndate_source = \"modified\"\n",
    )
    .unwrap();
    let result = sift::config::resolve_policy(t.to_str().unwrap());
    let json = sift::render::config_check_json(&result);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["valid"], true);
    assert_eq!(value["strategy"], "date");
    assert_eq!(value["template"], "{year}/{month}");
    assert_eq!(value["date_source"], "modified");
}

#[test]
fn config_check_invalid_template_fails() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"date\"\ntemplate = \"../{year}\"\n",
    )
    .unwrap();
    let result = sift::config::resolve_policy(t.to_str().unwrap());
    assert!(result.is_err());
    let json = sift::render::config_check_json(&result);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["valid"], false);
}

#[test]
fn config_check_unsupported_source_fails() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"date\"\ntemplate = \"{year}\"\ndate_source = \"exif\"\n",
    )
    .unwrap();
    assert!(sift::config::resolve_policy(t.to_str().unwrap()).is_err());
}

// ------------------------------------------------------------- executor safety

#[test]
fn toctou_destination_collision_refused() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );

    // Something occupies the destination AFTER planning, before executing.
    fs::create_dir_all(t.join("2026/09")).unwrap();
    fs::write(t.join("2026/09/invoice.pdf"), b"raced").unwrap();

    let hist_dir = t.join(".sift-history-test");
    sift::history::set_test_history_dir(hist_dir);
    let (_id, outcomes) = sift::executor::execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert!(outcomes
        .iter()
        .any(|o| o.op == Op::Move && o.result.is_err()));
    assert_eq!(
        fs::read_to_string(t.join("2026/09/invoice.pdf")).unwrap(),
        "raced"
    );
    assert!(t.join("invoice.pdf").exists());
    sift::history::clear_test_history_dir();
}

#[test]
fn destination_ancestor_symlink_introduced_after_planning_refused() {
    use std::os::unix::fs::symlink;
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );

    // Swap the "2026" ancestor for a symlink after planning, before executing.
    let outside = tempdir().unwrap();
    symlink(outside.path(), t.join("2026")).unwrap();

    let hist_dir = t.join(".sift-history-test");
    sift::history::set_test_history_dir(hist_dir);
    let (_id, outcomes) = sift::executor::execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert!(outcomes.iter().any(|o| o.result.is_err()));
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert!(t.join("invoice.pdf").exists());
    sift::history::clear_test_history_dir();
}

#[test]
fn source_type_changed_before_execution_refused() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );

    // The source becomes a directory after planning, before executing.
    fs::remove_file(t.join("invoice.pdf")).unwrap();
    fs::create_dir(t.join("invoice.pdf")).unwrap();

    let hist_dir = t.join(".sift-history-test");
    sift::history::set_test_history_dir(hist_dir);
    let (_id, outcomes) = sift::executor::execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert!(outcomes
        .iter()
        .any(|o| o.op == Op::Move && o.result.is_err()));
    assert!(t.join("invoice.pdf").is_dir());
    sift::history::clear_test_history_dir();
}

// -------------------------------------------------------------- history/undo

#[test]
fn successful_date_move_recorded_in_history() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let hist_dir = t.join(".sift-history-test");
    sift::history::set_test_history_dir(hist_dir.clone());
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let (id, outcomes) = sift::executor::execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert!(outcomes
        .iter()
        .any(|o| o.op == Op::Move && o.result.is_ok()));
    assert!(hist_dir.join(format!("{id}.json")).exists());
    sift::history::clear_test_history_dir();
}

#[test]
fn undo_safely_restores_date_organized_file() {
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    let policy = date_policy("{year}/{month}");
    let hist_dir = t.join(".sift-history-test");
    sift::history::set_test_history_dir(hist_dir);
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let (id, _) = sift::executor::execute_plan(plan, t.to_str().unwrap(), "organize", None);
    assert!(t.join("2026/09/invoice.pdf").exists());

    assert!(sift::history::cmd_undo(id));
    assert!(t.join("invoice.pdf").exists());
    assert!(!t.join("2026/09/invoice.pdf").exists());
    sift::history::clear_test_history_dir();
}

#[test]
fn renamed_date_collision_move_is_undoable() {
    // A rename-on-collision Move is a real, successful move — it must be
    // just as undoable as any other Move, not quietly excluded because it
    // came from automatic collision handling.
    let d = tempdir().unwrap();
    let t = d.path();
    touch_dated(&t.join("invoice.pdf"), 2026, 9, 9);
    fs::create_dir_all(t.join("2026/09")).unwrap();
    fs::write(t.join("2026/09/invoice.pdf"), b"existing").unwrap();
    let policy = date_policy("{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.op == Op::Move && a.src.parent() == Some(t))
        .unwrap();
    assert!(a.undoable);
}
