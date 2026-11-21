//! `effect.schemaExportsAsTypes` reports an unused Effect schema export as an
//! unused type. With the option off, the schema stays an unused export.

use super::common::{create_config, fixture_path};
use fallow_config::Severity;

const FIXTURE: &str = "effect-schema-exports-as-types";

/// Exports that stay value exports with the option on. A forwarded decoder,
/// a forwarded plain value, an alias of a plain value, a `pipe` on a plain
/// value and a re-export whose name matches a schema import are not schemas.
/// A `pipe` whose argument is an `HttpApiSchema` member that is not a
/// transform call (a guard, or a transform factory that is not called) is not
/// a schema. A function that returns a schema (`makeUnusedRule`) and an alias
/// of one (`importedHelperAlias`) are functions, not schemas. A call of a
/// function is not a schema when a `return` of the function is not a schema,
/// or when a path of the function ends without a `return` (#4988).
const VALUE_EXPORTS_WITH_OPTION_ON: &[&str] = &[
    "Collided",
    "NotEffectSchema",
    "NotSchema",
    "aliasedPlain",
    "bareHttpTransform",
    "decodeBaseRule",
    "decodeUsed",
    "fromDecoderPipe",
    "fromImportedPlain",
    "fromMixedReturns",
    "fromOpenEnd",
    "fromPlainArg",
    "fromPlainFn",
    "fromPlainHelper",
    "fromPlainLocal",
    "fromPlainNested",
    "httpStatusProbe",
    "importedHelperAlias",
    "makeUnusedRule",
    "notDerived",
    "plainValue",
    "unusedHelper",
];

/// Exports whose value is a call of a function that returns a schema: a
/// function of the same module, or an imported one (#4988).
const HELPER_SCHEMA_EXPORTS: &[&str] = &[
    "FromArg",
    "FromEither",
    "FromFn",
    "FromHelper",
    "FromHelperPipe",
    "FromImportedHelper",
    "FromImportedHelperPipe",
    "FromLocal",
    "FromNested",
    "FromWrappedHelper",
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
            "AliasedRule",
            "AnnotatedRule",
            "ChainedError",
            "Collided",
            "CreatedBody",
            "DerivedRule",
            "DerivedTwice",
            "FormRequest",
            "ForwardedDefault",
            "ForwardedUnion",
            "FromArg",
            "FromEither",
            "FromFn",
            "FromHelper",
            "FromHelperPipe",
            "FromImportedHelper",
            "FromImportedHelperPipe",
            "FromLocal",
            "FromNested",
            "FromWrappedHelper",
            "NotEffectSchema",
            "NotSchema",
            "PairedSchema",
            "ReExportedRule",
            "UnusedId",
            "UnusedPayload",
            "UnusedUser",
            "aliasedPlain",
            "bareHttpTransform",
            "decodeBaseRule",
            "decodeUsed",
            "default",
            "default",
            "default",
            "fromDecoderPipe",
            "fromImportedPlain",
            "fromMixedReturns",
            "fromOpenEnd",
            "fromPlainArg",
            "fromPlainFn",
            "fromPlainHelper",
            "fromPlainLocal",
            "fromPlainNested",
            "httpStatusProbe",
            "importedHelperAlias",
            "makeUnusedRule",
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
            "AliasedRule",
            "AnnotatedRule",
            "ChainedError",
            "CreatedBody",
            "DerivedRule",
            "DerivedTwice",
            "FormRequest",
            "ForwardedDefault",
            "ForwardedUnion",
            "FromArg",
            "FromEither",
            "FromFn",
            "FromHelper",
            "FromHelperPipe",
            "FromImportedHelper",
            "FromImportedHelperPipe",
            "FromLocal",
            "FromNested",
            "FromWrappedHelper",
            "PairedSchema",
            "ReExportedRule",
            "UnusedAlias",
            "UnusedId",
            "UnusedPayload",
            "UnusedUser",
            "default",
            "default",
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

#[test]
fn option_on_reports_a_schema_from_a_helper_call_as_an_unused_type() {
    let findings = findings(true, Severity::Error);
    for name in HELPER_SCHEMA_EXPORTS {
        assert!(
            findings.types.iter().any(|found| found == name),
            "{name} is a schema, so it is an unused type: {:?}",
            findings.types
        );
        assert!(
            !findings.exports.iter().any(|found| found == name),
            "{name} is a schema, so it is no unused export: {:?}",
            findings.exports
        );
    }
}
