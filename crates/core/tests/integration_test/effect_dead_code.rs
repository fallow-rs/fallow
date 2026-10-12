//! Effect dead-code rules: `unused-effect-layer`, `unused-tagged-error` and
//! `unproduced-catch-tag`.
//!
//! The fixture covers the credit paths that must keep a finding away:
//! - a layer that a live layer composes, and a layer read through an export
//!   alias,
//! - a tagged error that `new`, a namespace member or a schema union reads,
//! - a builtin tag, and a tag that no project class declares,
//! - a layer and an error that the entry module re-exports (`PublicLive`,
//!   `PublicError`), also under an export alias (`RenamedPublicLive`): code
//!   outside the project can use them,
//! - an error read through a default import and through a named import of a
//!   namespace re-export (`nsErrors.ViaNamespaceReExport`).
//!
//! A zero-reference head (`OrphanLive`) stays an `unused-export`, so the
//! layer rule reports only the tail of the dead composition.

use super::common::{create_config, fixture_path};

fn analyze_fixture() -> fallow_core::results::AnalysisResults {
    let config = create_config(fixture_path("effect-dead-code"));
    fallow_core::analyze(&config).expect("analysis should succeed")
}

#[test]
fn reports_layers_composed_only_into_unused_layers() {
    let results = analyze_fixture();
    let mut layers: Vec<(&str, Vec<String>)> = results
        .unused_effect_layers
        .iter()
        .map(|finding| {
            (
                finding.layer.name.as_str(),
                finding.layer.dead_consumers.clone(),
            )
        })
        .collect();
    layers.sort();
    assert_eq!(
        layers,
        vec![
            ("Cache.layer", vec!["CacheLive".to_string()]),
            ("CacheLive", vec!["OrphanLive".to_string()]),
        ],
    );
    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();
    assert!(
        unused_exports.contains(&"OrphanLive"),
        "the zero-reference head stays an unused export: {unused_exports:?}"
    );
}

#[test]
fn reports_tagged_errors_that_no_code_produces() {
    let results = analyze_fixture();
    let errors: Vec<(&str, &str, usize)> = results
        .unused_tagged_errors
        .iter()
        .map(|finding| {
            (
                finding.error.name.as_str(),
                finding.error.tag.as_str(),
                finding.error.catch_sites.len(),
            )
        })
        .collect();
    assert_eq!(errors, vec![("NeverThrown", "NeverThrown", 1)]);
}

#[test]
fn reports_catch_tags_only_when_no_error_finding_covers_them() {
    let results = analyze_fixture();
    let tags: Vec<(&str, Vec<String>)> = results
        .unproduced_catch_tags
        .iter()
        .map(|finding| {
            (
                finding.catch_tag.tag.as_str(),
                finding.catch_tag.candidate_classes.clone(),
            )
        })
        .collect();
    assert_eq!(tags, vec![("Silenced", vec!["Silenced".to_string()])]);
}

#[test]
fn catch_tags_report_every_handler_when_the_error_rule_is_off() {
    let root = fixture_path("effect-dead-code");
    let mut config = create_config(root);
    config.rules.unused_tagged_errors = fallow_config::Severity::Off;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(results.unused_tagged_errors.is_empty());
    let tags: Vec<&str> = results
        .unproduced_catch_tags
        .iter()
        .map(|finding| finding.catch_tag.tag.as_str())
        .collect();
    assert_eq!(
        tags,
        vec!["NeverThrown", "Silenced"],
        "with the error rule off, every unproduced handler is reported"
    );
}
