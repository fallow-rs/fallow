//! Regression matrix for sink shapes that hide a known security sink behind
//! syntax: indirect `eval`, global-object prefixes, computed property names,
//! constructor chains, JSX spread props, string timers, local aliases, and the
//! iframe `srcdoc` document sink.
//!
//! Fixture `tests/fixtures/security-evasion-shapes/` holds one case per file.
//! Each case must end as a candidate or as a blind spot. A case must never be a
//! silent miss.

use fallow_config::Severity;
use fallow_core::results::{AnalysisResults, SecurityFindingKind};

use super::common::{create_config_with_rules, fixture_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    Candidate(&'static str),
    BlindSpot,
    Nothing,
}

const CASES: &[(&str, Expected)] = &[
    ("src/direct-eval.ts", Expected::Candidate("code-injection")),
    (
        "src/indirect-eval.ts",
        Expected::Candidate("code-injection"),
    ),
    (
        "src/global-this-eval.ts",
        Expected::Candidate("code-injection"),
    ),
    ("src/window-eval.ts", Expected::Candidate("code-injection")),
    ("src/self-eval.ts", Expected::Candidate("code-injection")),
    (
        "src/window-computed-eval.ts",
        Expected::Candidate("code-injection"),
    ),
    (
        "src/function-prototype-constructor.ts",
        Expected::Candidate("code-injection"),
    ),
    (
        "src/constructor-constructor.ts",
        Expected::Candidate("code-injection"),
    ),
    ("src/new-function.ts", Expected::Candidate("code-injection")),
    (
        "src/function-call.ts",
        Expected::Candidate("code-injection"),
    ),
    (
        "src/computed-inner-html.ts",
        Expected::Candidate("dangerous-html"),
    ),
    (
        "src/literal-key-inner-html.ts",
        Expected::Candidate("dangerous-html"),
    ),
    ("src/unresolved-key-assign.ts", Expected::BlindSpot),
    ("src/index-arithmetic-assign.ts", Expected::Nothing),
    ("src/dictionary-assign.ts", Expected::Nothing),
    (
        "src/jsx-inline-spread.tsx",
        Expected::Candidate("dangerous-html"),
    ),
    (
        "src/jsx-local-spread.tsx",
        Expected::Candidate("dangerous-html"),
    ),
    (
        "src/timer-string-literal.ts",
        Expected::Candidate("code-injection"),
    ),
    (
        "src/timer-string-concat.ts",
        Expected::Candidate("code-injection"),
    ),
    ("src/timer-callback.ts", Expected::Nothing),
    ("src/eval-alias.ts", Expected::Candidate("code-injection")),
    (
        "src/eval-alias-shadowed.ts",
        Expected::Candidate("code-injection"),
    ),
    (
        "src/iframe-srcdoc-assign.ts",
        Expected::Candidate("dangerous-html"),
    ),
    (
        "src/iframe-srcdoc-set-attribute.ts",
        Expected::Candidate("dangerous-html"),
    ),
    (
        "src/iframe-srcdoc-jsx.tsx",
        Expected::Candidate("dangerous-html"),
    ),
    ("src/iframe-srcdoc-literal.tsx", Expected::Nothing),
    ("src/SrcdocFrame.vue", Expected::Candidate("dangerous-html")),
    ("src/SrcdocStatic.vue", Expected::Nothing),
    (
        "src/SrcdocFrame.svelte",
        Expected::Candidate("dangerous-html"),
    ),
    (
        "src/srcdoc-frame.component.ts",
        Expected::Candidate("dangerous-html"),
    ),
];

fn analyze() -> AnalysisResults {
    let root = fixture_path("security-evasion-shapes");
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn normalized(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn observed(results: &AnalysisResults, suffix: &str) -> Vec<Expected> {
    let mut seen: Vec<Expected> = results
        .security_findings
        .iter()
        .filter(|finding| matches!(finding.kind, SecurityFindingKind::TaintedSink))
        .filter(|finding| normalized(&finding.path).ends_with(suffix))
        .filter_map(|finding| {
            let category = finding.category.as_deref()?;
            ["code-injection", "dangerous-html"]
                .into_iter()
                .find(|known| *known == category)
                .map(Expected::Candidate)
        })
        .collect();
    let blind_spot = results
        .security_unresolved_callee_diagnostics
        .iter()
        .any(|diagnostic| normalized(&diagnostic.path).ends_with(suffix));
    if blind_spot {
        seen.push(Expected::BlindSpot);
    }
    seen
}

#[test]
fn every_evasion_shape_is_a_candidate_or_a_blind_spot() {
    let results = analyze();
    let mismatches: Vec<String> = CASES
        .iter()
        .filter_map(|(suffix, expected)| {
            let seen = observed(&results, suffix);
            let ok = match expected {
                Expected::Nothing => seen.is_empty(),
                Expected::BlindSpot | Expected::Candidate(_) => seen.contains(expected),
            };
            (!ok).then(|| format!("{suffix}: expected {expected:?}, observed {seen:?}"))
        })
        .collect();
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn srcdoc_candidates_carry_srcdoc_evidence() {
    let results = analyze();
    let finding = results
        .security_findings
        .iter()
        .find(|finding| normalized(&finding.path).ends_with("src/iframe-srcdoc-assign.ts"))
        .expect("srcdoc assignment should produce a candidate");
    assert_eq!(finding.cwe, Some(79));
    assert!(
        finding.evidence.contains("srcdoc"),
        "evidence should name the srcdoc document sink: {}",
        finding.evidence
    );
}

/// A module alias of `eval` reaches only the calls that resolve to it. A
/// parameter, a local or a catch parameter of the same name hides the alias,
/// and a local in one function does not hide it in a later function.
#[test]
fn eval_alias_stops_at_a_shadowing_binding() {
    let results = analyze();
    let mut lines: Vec<u32> = results
        .security_findings
        .iter()
        .filter(|finding| matches!(finding.kind, SecurityFindingKind::TaintedSink))
        .filter(|finding| normalized(&finding.path).ends_with("src/eval-alias-shadowed.ts"))
        .map(|finding| finding.line)
        .collect();
    lines.sort_unstable();
    lines.dedup();
    assert_eq!(lines, vec![2, 6]);
}
