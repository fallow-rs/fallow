//! Integration tests for the catalogue-driven `dangerous-html` tainted-sink
//! candidate (CWE-79), the first ship of the data-driven security matcher
//! catalogue.
//!
//! Fixture `tests/fixtures/security-dangerous-html/` carries a non-literal
//! `innerHTML` assignment (positive), a literal `innerHTML` assignment
//! (negative), and a non-literal `dangerouslySetInnerHTML` JSX attribute
//! (positive).

use fallow_config::Severity;
use fallow_core::results::{AnalysisResults, SecurityFindingKind};

use super::common::{create_config, create_config_with_rules, fixture_path};

fn analyze_with_security_sink() -> AnalysisResults {
    let root = fixture_path("security-dangerous-html");
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn anchored_on(results: &AnalysisResults, suffix: &str) -> bool {
    results.security_findings.iter().any(|f| {
        f.path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with(suffix)
    })
}

#[test]
fn non_literal_inner_html_assignment_fires_a_candidate() {
    // Criterion 1 (positive half): `el.innerHTML = userInput` emits a
    // dangerous-html candidate carrying category + CWE-79.
    let results = analyze_with_security_sink();
    let finding = results
        .security_findings
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/sink.ts")
        })
        .expect("sink.ts should produce a dangerous-html candidate");
    assert!(matches!(finding.kind, SecurityFindingKind::TaintedSink));
    assert_eq!(finding.category.as_deref(), Some("dangerous-html"));
    assert_eq!(finding.cwe, Some(79));
    assert!(
        !finding.actions.is_empty(),
        "candidate must carry a suppress action"
    );
}

#[test]
fn literal_inner_html_assignment_does_not_fire() {
    // Criterion 1 (negative half): `el.innerHTML = "<b>x</b>"` (literal) is
    // never captured, so it produces no candidate.
    let results = analyze_with_security_sink();
    assert!(
        !anchored_on(&results, "src/safe.ts"),
        "a literal innerHTML assignment must not be flagged"
    );
}

#[test]
fn non_literal_dangerously_set_inner_html_fires() {
    // JSX `dangerouslySetInnerHTML={{ __html: props.html }}` with a non-literal
    // value is a dangerous-html candidate.
    let results = analyze_with_security_sink();
    assert!(
        anchored_on(&results, "src/component.tsx"),
        "a non-literal dangerouslySetInnerHTML must be flagged"
    );
}

#[test]
fn dompurify_sanitized_html_sinks_do_not_fire() {
    let results = analyze_with_security_sink();
    for suffix in [
        "src/dompurify-default.ts",
        "src/dompurify-namespace.ts",
        "src/dompurify-require.ts",
        "src/isomorphic-dompurify.ts",
        "src/sanitized-component.tsx",
    ] {
        assert!(
            !anchored_on(&results, suffix),
            "{suffix} should be suppressed by DOMPurify provenance"
        );
    }
}

#[test]
fn sanitizer_near_misses_still_fire() {
    let results = analyze_with_security_sink();
    assert!(
        anchored_on(&results, "src/near-miss.ts"),
        "local sanitize-like helpers must not suppress HTML sink candidates"
    );
}

#[test]
fn trusted_local_html_escape_helper_does_not_fire() {
    let results = analyze_with_security_sink();
    for suffix in ["src/trusted-local-direct.ts", "src/trusted-local-escape.ts"] {
        assert!(
            !anchored_on(&results, suffix),
            "{suffix} must not be flagged"
        );
    }
}

#[test]
fn trusted_local_html_mixed_flow_still_fires() {
    let results = analyze_with_security_sink();
    assert!(
        anchored_on(&results, "src/trusted-local-mixed.ts"),
        "a renderer with an unsanitized dynamic fragment must be flagged"
    );
}

#[test]
fn shadowed_local_html_helper_still_fires() {
    let results = analyze_with_security_sink();
    assert!(
        anchored_on(&results, "src/trusted-local-shadowed.ts"),
        "a shadowing helper parameter must not suppress HTML sink candidates"
    );
}

#[test]
fn shadowed_sanitized_local_still_fires() {
    let results = analyze_with_security_sink();
    assert!(
        anchored_on(&results, "src/shadowed-sanitized-local.ts"),
        "a shadowing parameter must not inherit an outer sanitized binding"
    );
}

#[test]
fn dompurify_does_not_suppress_non_html_sinks() {
    let results = analyze_with_security_sink();
    let finding = results
        .security_findings
        .iter()
        .find(|f| {
            f.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/code-sink.ts")
        })
        .expect("eval with DOMPurify output must remain a code-injection candidate");
    assert_eq!(finding.category.as_deref(), Some("code-injection"));
}

#[test]
fn sink_in_test_or_config_file_does_not_fire() {
    // Build-config and test files are excluded from security candidate
    // generation (production-mode parity): an unsafe innerHTML sink inside a
    // `*.test.ts` or `vite.config.ts` must NOT produce a candidate.
    let results = analyze_with_security_sink();
    assert!(
        !anchored_on(&results, "src/component.test.ts"),
        "a sink inside a *.test.ts file must not be flagged"
    );
    assert!(
        !anchored_on(&results, "vite.config.ts"),
        "a sink inside a build-config file must not be flagged"
    );
}

#[test]
fn default_off_emits_no_tainted_sink_findings() {
    // Criterion 3: with the `security_sink` rule at its default `off`, bare
    // analysis produces zero tainted-sink findings.
    let root = fixture_path("security-dangerous-html");
    let config = create_config(root);
    assert_eq!(config.rules.security_sink, Severity::Off);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results
            .security_findings
            .iter()
            .all(|f| !matches!(f.kind, SecurityFindingKind::TaintedSink)),
        "default-off security_sink must not populate tainted-sink findings"
    );
}
