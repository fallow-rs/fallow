//! Integration tests for framework-template XSS sink capture (#883).

use fallow_config::Severity;
use fallow_core::results::{AnalysisResults, SecurityFindingKind};

use super::common::{create_config, create_config_with_rules, fixture_path};

fn analyze_with_security_sink() -> AnalysisResults {
    let root = fixture_path("security-template-xss-sinks");
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn finding_for<'a>(
    results: &'a AnalysisResults,
    suffix: &str,
) -> &'a fallow_core::results::SecurityFinding {
    results
        .security_findings
        .iter()
        .find(|finding| {
            matches!(finding.kind, SecurityFindingKind::TaintedSink)
                && finding.category.as_deref() == Some("dangerous-html")
                && finding
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with(suffix)
        })
        .unwrap_or_else(|| panic!("{suffix} should produce a dangerous-html finding"))
}

fn assert_template_sink(results: &AnalysisResults, suffix: &str, line: u32) {
    let finding = finding_for(results, suffix);
    assert_eq!(finding.cwe, Some(79));
    assert_eq!(finding.line, line);
}

#[test]
fn svelte_html_block_sink_fires_with_source_span() {
    let results = analyze_with_security_sink();
    assert_template_sink(&results, "src/App.svelte", 8);
}

#[test]
fn vue_v_html_sink_fires_with_source_span() {
    let results = analyze_with_security_sink();
    assert_template_sink(&results, "src/App.vue", 6);
}

#[test]
fn angular_external_inner_html_sink_fires_with_source_span() {
    let results = analyze_with_security_sink();
    assert_template_sink(&results, "src/app.component.html", 3);
}

#[test]
fn angular_pipe_inner_html_sink_falls_back_when_expression_is_not_typescript() {
    let results = analyze_with_security_sink();
    assert_template_sink(&results, "src/pipe.component.html", 3);
}

#[test]
fn angular_inline_inner_html_sink_fires_on_component_file() {
    let results = analyze_with_security_sink();
    assert_template_sink(&results, "src/inline.component.ts", 8);
}

#[test]
fn literal_template_html_bindings_do_not_fire() {
    let results = analyze_with_security_sink();
    for suffix in [
        "src/Safe.svelte",
        "src/Safe.vue",
        "src/safe.component.html",
        "src/safe-inline.component.ts",
    ] {
        assert!(
            !results.security_findings.iter().any(|finding| {
                finding
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with(suffix)
            }),
            "{suffix} should not produce a template XSS finding"
        );
    }
}

#[test]
fn default_off_emits_no_template_sink_findings() {
    let root = fixture_path("security-template-xss-sinks");
    let config = create_config(root);
    assert_eq!(config.rules.security_sink, Severity::Off);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results
            .security_findings
            .iter()
            .all(|finding| !matches!(finding.kind, SecurityFindingKind::TaintedSink)),
        "default-off security_sink must not populate template sink findings"
    );
}
