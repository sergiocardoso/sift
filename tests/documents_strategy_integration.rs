//! Integration tests for Sift's `strategy = "documents"` organize
//! strategy: `MetadataTemplate` parsing/validation, metadata extraction
//! (PDF's `/Info` dictionary via the pure-Rust `lopdf` crate; Office's
//! `docProps/core.xml` via the pure-Rust `zip`+`roxmltree` crates,
//! against real tiny fixture files under `tests/fixtures/`), planning
//! (destination rendering, missing-field skip), rule precedence, config
//! validation, `explain`, `config check`, and the `--recursive` refusal
//! that applies to this strategy in this version.
//!
//! Uses only tempdir/tempfile; every test that touches "global config"
//! isolates it first — never the user's real home or Sift data directory.

use sift::config::{EffectivePolicy, MetadataTemplate, OrganizeStrategy};
use sift::domain::Op;
use sift::explain::explain_path;
use sift::metadata::extract_document_metadata;
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

fn documents_policy(template: &str) -> EffectivePolicy {
    EffectivePolicy {
        strategy: OrganizeStrategy::Documents,
        metadata_template: Some(MetadataTemplate::parse_documents(template).unwrap()),
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
fn template_documents_fields_accepted() {
    for placeholder in ["{author}", "{title}", "{year}", "{month}", "{day}"] {
        assert!(
            MetadataTemplate::parse_documents(placeholder).is_ok(),
            "{placeholder} should be a valid documents placeholder"
        );
    }
}

#[test]
fn template_other_domain_fields_rejected_for_documents() {
    for placeholder in ["{artist}", "{camera}", "{width}", "{codec}"] {
        assert!(
            MetadataTemplate::parse_documents(placeholder).is_err(),
            "{placeholder} should NOT be a valid documents placeholder"
        );
    }
}

#[test]
fn template_unknown_placeholder_rejected() {
    assert!(MetadataTemplate::parse_documents("{banana}").is_err());
}

#[test]
fn template_empty_rejected() {
    assert!(MetadataTemplate::parse_documents("").is_err());
}

#[test]
fn template_absolute_and_traversal_rejected() {
    assert!(MetadataTemplate::parse_documents("/{author}").is_err());
    assert!(MetadataTemplate::parse_documents("../{author}").is_err());
}

#[test]
fn template_adjacent_placeholders_rejected() {
    assert!(MetadataTemplate::parse_documents("{year}{month}").is_err());
}

// -------------------------------------------------------------- extraction

#[test]
fn extracts_author_title_and_creation_date_from_real_pdf() {
    let meta = extract_document_metadata(&fixture("tiny.pdf")).unwrap();
    assert_eq!(meta.author.as_deref(), Some("Jane Doe"));
    assert_eq!(meta.title.as_deref(), Some("Test Document"));
    assert_eq!(meta.year.as_deref(), Some("2023"));
    assert_eq!(meta.month.as_deref(), Some("11"));
    assert_eq!(meta.day.as_deref(), Some("15"));
}

#[test]
fn extracts_author_title_and_creation_date_from_real_docx() {
    let meta = extract_document_metadata(&fixture("tiny.docx")).unwrap();
    assert_eq!(meta.author.as_deref(), Some("Jane Doe"));
    assert_eq!(meta.title.as_deref(), Some("Quarterly Report"));
    assert_eq!(meta.year.as_deref(), Some("2023"));
    assert_eq!(meta.month.as_deref(), Some("11"));
    assert_eq!(meta.day.as_deref(), Some("15"));
}

#[test]
fn untagged_pdf_yields_all_none_not_an_error() {
    let meta = extract_document_metadata(&fixture("untagged.pdf")).unwrap();
    assert!(meta.author.is_none());
    assert!(meta.title.is_none());
    assert!(meta.year.is_none());
}

#[test]
fn unsupported_extension_is_a_clear_error() {
    let d = tempdir().unwrap();
    let f = d.path().join("notes.txt");
    fs::write(&f, b"plain text").unwrap();
    let err = extract_document_metadata(&f).unwrap_err();
    assert!(err.contains("unsupported"));
}

#[test]
fn unreadable_pdf_is_a_clear_error() {
    let d = tempdir().unwrap();
    let f = d.path().join("broken.pdf");
    fs::write(&f, b"this is not a real pdf file").unwrap();
    assert!(extract_document_metadata(&f).is_err());
}

#[test]
fn unreadable_docx_is_a_clear_error() {
    let d = tempdir().unwrap();
    let f = d.path().join("broken.docx");
    fs::write(&f, b"this is not a real zip/docx file").unwrap();
    assert!(extract_document_metadata(&f).is_err());
}

// -------------------------------------------------------------- planning

#[test]
fn pdf_lands_in_author_year_month() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.pdf", t, "report.pdf");
    let policy = documents_policy("{author}/{year}/{month}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("report.pdf"))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(
        a.dst.as_ref().unwrap(),
        &t.join("Jane Doe/2023/11/report.pdf")
    );
}

#[test]
fn docx_lands_in_title_folder() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.docx", t, "report.docx");
    let policy = documents_policy("{title}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("report.docx"))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(
        a.dst.as_ref().unwrap(),
        &t.join("Quarterly Report/report.docx")
    );
}

#[test]
fn untagged_pdf_is_skipped_never_fabricated_bucket() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("untagged.pdf", t, "mystery.pdf");
    let policy = documents_policy("{author}/{year}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("mystery.pdf"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(a.reason.as_deref().unwrap().contains("metadata"));
    assert!(
        !plan.actions.iter().any(|a| a.op == Op::CreateDir),
        "no 'Unknown Author' folder is ever created"
    );
}

#[test]
fn nested_createdir_actions_for_three_level_template() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.pdf", t, "report.pdf");
    let policy = documents_policy("{author}/{year}/{month}");
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
    assert_eq!(creates[0].src, t.join("Jane Doe"));
    assert_eq!(creates[1].src, t.join("Jane Doe/2023"));
    assert_eq!(creates[2].src, t.join("Jane Doe/2023/11"));
}

#[test]
fn destination_collision_with_different_content_is_renamed() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::create_dir_all(t.join("Jane Doe/2023")).unwrap();
    fs::write(t.join("Jane Doe/2023/report.pdf"), b"existing").unwrap();
    copy_fixture("tiny.pdf", t, "report.pdf");
    let policy = documents_policy("{author}/{year}");
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("report.pdf") && a.src.parent() == Some(t))
        .unwrap();
    assert_eq!(a.op, Op::Move);
    assert_eq!(
        a.dst.as_ref().unwrap(),
        &t.join("Jane Doe/2023/report (1).pdf")
    );
    assert_eq!(
        fs::read_to_string(t.join("Jane Doe/2023/report.pdf")).unwrap(),
        "existing",
        "the pre-existing file at the colliding name must never be touched"
    );
}

// ------------------------------------------------------------ precedence

#[test]
fn explicit_skip_rule_overrides_documents() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.pdf", t, "report.pdf");
    let rule = sift::config::Rule {
        name: "keep".into(),
        pattern: "report.pdf".into(),
        action: "Skip".into(),
        destination: None,
        priority: 100,
        enabled: true,
        description: None,
    };
    let policy = documents_policy("{author}/{year}").with_rules(vec![rule]);
    let plan = plan_with_strategy(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("report.pdf"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(!a.reason.as_deref().unwrap().contains("Jane"));
}

// --------------------------------------------------------------- config

#[test]
fn documents_requires_template() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"documents\"\n",
        )
        .unwrap();
        let err = sift::config::resolve_policy(t.to_str().unwrap()).unwrap_err();
        assert!(err.contains("template"));
    });
}

#[test]
fn documents_rejects_date_source_field() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"documents\"\ntemplate = \"{author}\"\ndate_source = \"modified\"\n",
        )
        .unwrap();
        assert!(sift::config::resolve_policy(t.to_str().unwrap()).is_err());
    });
}

#[test]
fn documents_strategy_parses_from_toml() {
    isolated_global(|| {
        let d = tempdir().unwrap();
        let t = d.path();
        fs::write(
            t.join(".sift.toml"),
            "[organize]\nstrategy = \"documents\"\ntemplate = \"{author}/{year}\"\n",
        )
        .unwrap();
        let policy = sift::config::resolve_policy(t.to_str().unwrap()).unwrap();
        assert_eq!(policy.strategy, OrganizeStrategy::Documents);
        assert_eq!(
            policy.metadata_template.as_ref().unwrap().raw(),
            "{author}/{year}"
        );
    });
}

// ------------------------------------------------------------- recursive

#[test]
fn documents_does_not_support_recursive() {
    assert!(!OrganizeStrategy::Documents.supports_recursive());
}

#[test]
fn recursive_documents_organize_skips_everything_with_clear_reason() {
    let d = tempdir().unwrap();
    let t = d.path();
    copy_fixture("tiny.pdf", t, "report.pdf");
    let policy = documents_policy("{author}/{year}");
    let rp = plan_with_strategy_recursive(
        t.to_str().unwrap(),
        &policy,
        &sift::classifier::CategoryDB::default(),
    );
    let a = rp
        .plan
        .actions
        .iter()
        .find(|a| a.src.ends_with("report.pdf"))
        .unwrap();
    assert_eq!(a.op, Op::Skip);
    assert!(a.reason.as_deref().unwrap().contains("recursive"));
    assert!(t.join("report.pdf").exists(), "zero mutation");
}

// ------------------------------------------------------------------- explain

#[test]
fn explain_documents_exposes_metadata_and_destination() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = copy_fixture("tiny.pdf", t, "report.pdf");
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"documents\"\ntemplate = \"{author}/{year}\"\n",
    )
    .unwrap();
    let exp = explain_path(&f, t).unwrap();
    let meta = exp.document_metadata.expect("expected document metadata");
    assert_eq!(meta.author.as_deref(), Some("Jane Doe"));
    assert_eq!(exp.op, Op::Move);
    assert_eq!(exp.destination, Some(t.join("Jane Doe/2023/report.pdf")));
}

#[test]
fn explain_documents_performs_zero_mutation() {
    let d = tempdir().unwrap();
    let t = d.path();
    let f = copy_fixture("tiny.pdf", t, "report.pdf");
    fs::write(
        t.join(".sift.toml"),
        "[organize]\nstrategy = \"documents\"\ntemplate = \"{author}/{year}\"\n",
    )
    .unwrap();
    let _ = explain_path(&f, t).unwrap();
    assert!(f.exists());
    assert!(!t.join("Jane Doe").exists());
}

// --------------------------------------------------------------- config check

#[test]
fn config_check_valid_documents_policy() {
    let d = tempdir().unwrap();
    let t = d.path();
    fs::write(
        t.join(".sift.toml"),
        "version = 1\n[organize]\nstrategy = \"documents\"\ntemplate = \"{author}/{year}\"\n",
    )
    .unwrap();
    let result = sift::config::resolve_policy(t.to_str().unwrap());
    let json = sift::render::config_check_json(&result);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["valid"], true);
    assert_eq!(value["strategy"], "documents");
    assert_eq!(value["template"], "{author}/{year}");
}
