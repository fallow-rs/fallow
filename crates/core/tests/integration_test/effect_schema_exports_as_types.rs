//! `effect.schemaExportsAsTypes` reports an unused Effect schema export as an
//! unused type. With the option off, the schema stays an unused export.

use super::common::{create_config, fixture_path};
use fallow_config::Severity;

const FIXTURE: &str = "effect-schema-exports-as-types";

/// Exports that stay value exports with the option on. A forwarded decoder,
/// a forwarded plain value and a `pipe` on a plain value are not schemas.
const VALUE_EXPORTS_WITH_OPTION_ON: &[&str] = &[
    "NotEffectSchema",
    "decodeBaseRule",
    "decodeUsed",
    "notDerived",
    "plainValue",
    "unusedHelper",
];

struct Findings {
    exports: Vec<String>,
    types: Vec<String>,
    stale_suppressions: usize,
}

fn findings(schema_exports_as_types: bool, unused_types: Severity) -> Findings {
    let mut config = create_config(fixture_path(FIXTURE));
    config.effect.schema_exports_as_types = schema_exports_as_types;
    config.rules.unused_types = unused_types;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let names = |findings: Vec<String>| {
        let mut findings = findings;
        findings.sort_unstable();
        findings
    };
    Findings {
        exports: names(
            results
                .unused_exports
                .iter()
                .map(|finding| finding.export.export_name.clone())
                .collect(),
        ),
        types: names(
            results
                .unused_types
                .iter()
                .map(|finding| finding.export.export_name.clone())
                .collect(),
        ),
        stale_suppressions: results.stale_suppressions.len(),
    }
}

#[test]
fn option_off_reports_schemas_as_unused_exports() {
    let findings = findings(false, Severity::Error);
    assert_eq!(
        findings.exports,
        vec![
            "ChainedError",
            "DerivedRule",
            "DerivedTwice",
            "ForwardedUnion",
            "NotEffectSchema",
            "PairedSchema",
            "ReExportedRule",
            "UnusedId",
            "UnusedPayload",
            "UnusedUser",
            "decodeBaseRule",
            "decodeUsed",
            "default",
            "notDerived",
            "plainValue",
            "unusedHelper",
        ]
    );
    assert_eq!(findings.types, vec!["PairedSchema", "UnusedAlias"]);
    assert_eq!(findings.stale_suppressions, 0);
}

#[test]
fn option_on_reports_schemas_as_unused_types() {
    let findings = findings(true, Severity::Error);
    assert_eq!(
        findings.exports, VALUE_EXPORTS_WITH_OPTION_ON,
        "a local Schema object, a decoder and a forwarded non-schema value stay value exports"
    );
    assert_eq!(
        findings.types,
        vec![
            "ChainedError",
            "DerivedRule",
            "DerivedTwice",
            "ForwardedUnion",
            "PairedSchema",
            "ReExportedRule",
            "UnusedAlias",
            "UnusedId",
            "UnusedPayload",
            "UnusedUser",
            "default",
        ],
        "a schema and its same-name type alias give one finding, because they share a finding id"
    );
    assert_eq!(
        findings.stale_suppressions, 0,
        "an unused-export comment on a schema keeps its effect"
    );
}

#[test]
fn option_on_with_unused_types_off_hides_schemas() {
    let findings = findings(true, Severity::Off);
    assert_eq!(findings.exports, VALUE_EXPORTS_WITH_OPTION_ON);
    assert!(findings.types.is_empty(), "{:?}", findings.types);
}
