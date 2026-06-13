use fallow_config::{FallowConfig, OutputFormat, RulesConfig, Severity};

use crate::common::fixture_path;

/// Resolve a fixture with the `invalid-client-export` rule at `warn` (its
/// default). The detector is gated on the project declaring `next`, which the
/// fixture's `package.json` does.
fn fixture_config(name: &str) -> fallow_config::ResolvedConfig {
    FallowConfig {
        rules: RulesConfig {
            invalid_client_export: Severity::Warn,
            ..RulesConfig::default()
        },
        ..Default::default()
    }
    .resolve(fixture_path(name), OutputFormat::Human, 4, true, true, None)
}

#[test]
fn use_client_metadata_export_is_flagged_once() {
    let config = fixture_config("invalid-client-export");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let findings: Vec<(&str, String)> = results
        .invalid_client_exports
        .iter()
        .map(|f| {
            (
                f.export.export_name.as_str(),
                f.export.path.to_string_lossy().replace('\\', "/"),
            )
        })
        .collect();

    assert_eq!(
        findings.len(),
        1,
        "exactly one invalid client export expected: {findings:?}"
    );
    assert_eq!(findings[0].0, "metadata");
    assert!(
        findings[0].1.ends_with("app/page.tsx"),
        "finding should anchor at app/page.tsx, got {}",
        findings[0].1
    );

    // The directive is carried verbatim for the message.
    assert_eq!(
        results.invalid_client_exports[0].export.directive,
        "use client"
    );
}

#[test]
fn default_export_and_hook_in_client_file_are_not_flagged() {
    let config = fixture_config("invalid-client-export");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // The clean client file (app/widget.tsx) exports only `default` and an
    // ordinary hook; neither is illegal, so nothing from it is flagged.
    assert!(
        !results.invalid_client_exports.iter().any(|f| f
            .export
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("app/widget.tsx")),
        "clean client file must not produce a finding"
    );
}

#[test]
fn server_file_exporting_metadata_is_not_flagged() {
    let config = fixture_config("invalid-client-export");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // The server file (no "use client") exporting `metadata` is the
    // legitimate pattern and must never be flagged.
    assert!(
        !results.invalid_client_exports.iter().any(|f| f
            .export
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("app/server/config.tsx")),
        "server file exporting metadata must not produce a finding"
    );
}

#[test]
fn no_findings_when_next_is_absent() {
    let config = fixture_config("invalid-client-export-no-next");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.invalid_client_exports.is_empty(),
        "without `next` declared, the rule must not fire: {:?}",
        results.invalid_client_exports
    );
}
