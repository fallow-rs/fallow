use super::super::{assert_docs_footer_layout, plain, strip_ansi};
use super::*;
use std::path::PathBuf;

fn report_with_join_counts(
    matched: Option<usize>,
    total: Option<usize>,
) -> fallow_output::HealthReport {
    fallow_output::HealthReport {
        summary: fallow_output::HealthSummary {
            istanbul_files_matched: matched,
            istanbul_files_total: total,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A coverage file written for another root joins nothing, which without
/// this line is indistinguishable from code that has no tests.
#[test]
fn a_coverage_file_that_joins_nothing_says_so() {
    let warning = coverage_join_warning(&report_with_join_counts(Some(0), Some(12)))
        .expect("a map that joined no file warns");
    assert!(
        warning.contains("12 files that no analyzed file matched"),
        "{warning}"
    );
    assert!(warning.contains("--coverage-root"), "{warning}");
}

#[test]
fn a_partial_join_reports_the_ratio() {
    let warning = coverage_join_warning(&report_with_join_counts(Some(3), Some(12)))
        .expect("a partial join warns");
    assert!(warning.contains("3 of 12 files"), "{warning}");
}

/// Nothing is added to a healthy run, and nothing to a run without a
/// coverage file at all.
#[test]
fn a_complete_join_and_a_run_without_coverage_stay_quiet() {
    assert_eq!(
        coverage_join_warning(&report_with_join_counts(Some(12), Some(12))),
        None
    );
    assert_eq!(
        coverage_join_warning(&report_with_join_counts(None, None)),
        None
    );
    assert_eq!(
        coverage_join_warning(&report_with_join_counts(Some(0), Some(0))),
        None
    );
}

#[test]
fn health_empty_findings_produces_no_header() {
    let root = PathBuf::from("/project");
    let report = fallow_output::HealthReport {
        summary: fallow_output::HealthSummary {
            files_analyzed: 10,
            functions_analyzed: 50,
            ..Default::default()
        },
        ..Default::default()
    };
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(!text.contains("High complexity functions"));
}

#[test]
fn health_findings_show_function_details() {
    let root = PathBuf::from("/project");
    let report = fallow_output::HealthReport {
        findings: vec![
            fallow_output::ComplexityViolation {
                path: root.join("src/parser.ts"),
                name: "parseExpression".to_string(),
                line: 42,
                col: 0,
                cyclomatic: 25,
                cognitive: 30,
                line_count: 80,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: fallow_output::ExceededThreshold::Both,
                effective_severity: None,
                severity: fallow_output::FindingSeverity::High,
                ..Default::default()
            }
            .into(),
        ],
        summary: fallow_output::HealthSummary {
            files_analyzed: 10,
            functions_analyzed: 50,
            functions_above_threshold: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("High complexity functions (1)"));
    assert!(text.contains("src/parser.ts"));
    assert!(text.contains(":42"));
    assert!(text.contains("parseExpression"));
    assert!(text.contains("25 ! cyclomatic"));
    assert!(text.contains("30 ! cognitive"));
    assert!(text.contains("80 lines"));
}

#[test]
fn health_findings_label_template_complexity_entries() {
    let root = PathBuf::from("/project");
    let report = fallow_output::HealthReport {
        findings: vec![
            fallow_output::ComplexityViolation {
                path: root.join("src/Card.vue"),
                name: "<template>".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 8,
                cognitive: 12,
                line_count: 40,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: fallow_output::ExceededThreshold::Cognitive,
                effective_severity: None,
                severity: fallow_output::FindingSeverity::Moderate,
                ..Default::default()
            }
            .into(),
        ],
        summary: fallow_output::HealthSummary {
            files_analyzed: 1,
            functions_analyzed: 1,
            functions_above_threshold: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("High complexity findings (1)"));
    assert!(text.contains("<template> (template complexity)"));
    assert!(text.contains("Functions and synthetic template or component entries"));
}

/// Minimal complexity violation for the issue-2163 disclosure tests.
fn disclosure_violation(
    path: PathBuf,
    name: &str,
    exceeded: fallow_output::ExceededThreshold,
) -> fallow_output::ComplexityViolation {
    fallow_output::ComplexityViolation {
        path,
        name: name.to_string(),
        line: 5,
        col: 0,
        cyclomatic: 9,
        cognitive: 21,
        line_count: 13,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        exceeded,
        effective_severity: None,
        severity: fallow_output::FindingSeverity::High,
        crap: Some(90.0),
        ..Default::default()
    }
}

fn disclosure_report(
    findings: Vec<fallow_output::ComplexityViolation>,
) -> fallow_output::HealthReport {
    let count = findings.len();
    fallow_output::HealthReport {
        findings: findings.into_iter().map(Into::into).collect(),
        summary: fallow_output::HealthSummary {
            files_analyzed: 1,
            functions_analyzed: 1,
            functions_above_threshold: count,
            ..Default::default()
        },
        ..Default::default()
    }
}

const OVERRIDE_THRESHOLDS: fallow_output::HealthEffectiveThresholds =
    fallow_output::HealthEffectiveThresholds {
        max_cyclomatic: 500,
        max_cognitive: 500,
        max_crap: 30.0,
        max_unit_size: 60,
    };

#[test]
fn crap_only_breach_marks_crap_and_not_complexity() {
    let root = PathBuf::from("/project");
    let mut finding = disclosure_violation(
        root.join("src/Widget.svelte"),
        "<template>",
        fallow_output::ExceededThreshold::Crap,
    );
    finding.effective_thresholds = Some(OVERRIDE_THRESHOLDS);
    finding.threshold_source = Some(fallow_output::ThresholdSource::Override);
    let report = disclosure_report(vec![finding]);

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("90.0 ! CRAP"), "{text}");
    assert!(!text.contains("9 ! cyclomatic"), "{text}");
    assert!(!text.contains("21 ! cognitive"), "{text}");
}

#[test]
fn override_affected_finding_names_the_ceilings_in_force() {
    let root = PathBuf::from("/project");
    let mut finding = disclosure_violation(
        root.join("src/Widget.svelte"),
        "<template>",
        fallow_output::ExceededThreshold::Crap,
    );
    finding.effective_thresholds = Some(OVERRIDE_THRESHOLDS);
    finding.threshold_source = Some(fallow_output::ThresholdSource::Override);
    let report = disclosure_report(vec![finding]);

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(
        text.contains("override: cyclomatic 500, cognitive 500, CRAP 30.0, unit size 60"),
        "{text}"
    );
}

#[test]
fn finding_without_override_renders_no_ceilings_line() {
    let root = PathBuf::from("/project");
    let finding = disclosure_violation(
        root.join("src/a.ts"),
        "parse",
        fallow_output::ExceededThreshold::Crap,
    );
    let report = disclosure_report(vec![finding]);

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(!text.contains("override:"), "{text}");
}

#[test]
fn all_dimensions_breach_marks_every_metric() {
    let root = PathBuf::from("/project");
    let finding = disclosure_violation(
        root.join("src/a.ts"),
        "parse",
        fallow_output::ExceededThreshold::All,
    );
    let report = disclosure_report(vec![finding]);

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("9 ! cyclomatic"), "{text}");
    assert!(text.contains("21 ! cognitive"), "{text}");
    assert!(text.contains("90.0 ! CRAP"), "{text}");
}

/// A colour-free reader has no other way to learn what `!` means, and the
/// footer is the only line in the section that can carry the legend.
#[test]
fn findings_footer_explains_the_breach_marker() {
    let root = PathBuf::from("/project");
    let report = disclosure_report(vec![disclosure_violation(
        root.join("src/a.ts"),
        "parse",
        fallow_output::ExceededThreshold::All,
    )]);

    let text = plain(&build_health_human_lines(&report, &root));
    // The footer wraps at 80 columns, so compare the text with joined lines.
    let flowed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flowed.contains("! marks the dimension that breached"),
        "{text}"
    );
}

/// The renderer's styling rule is contract-tested against `exceeded` alone
/// and deliberately does not assume the engine's current attach-on-breach
/// behavior, so this input pairs an attached `crap` with an `exceeded` that
/// omits it. The producing invariant is pinned in the engine instead.
#[test]
fn crap_is_dimmed_when_exceeded_does_not_include_crap() {
    let root = PathBuf::from("/project");
    let mut finding = disclosure_violation(
        root.join("src/a.ts"),
        "parse",
        fallow_output::ExceededThreshold::Cyclomatic,
    );
    finding.crap = Some(4.0);
    let report = disclosure_report(vec![finding]);

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("9 ! cyclomatic"), "{text}");
    assert!(!text.contains("4.0 ! CRAP"), "{text}");
    assert!(text.contains("4.0   CRAP"), "{text}");
}

#[test]
fn threshold_override_section_counts_configured_overrides_not_rows() {
    let root = PathBuf::from("/project");
    let mut report = disclosure_report(vec![disclosure_violation(
        root.join("src/Widget.svelte"),
        "<template>",
        fallow_output::ExceededThreshold::Crap,
    )]);
    let base = fallow_output::ThresholdOverrideState {
        status: fallow_output::ThresholdOverrideStatus::Active,
        override_index: 0,
        dimension: fallow_output::ThresholdOverrideDimension::Complexity,
        outstanding: vec![fallow_output::ThresholdOverrideDimension::Crap],
        path: Some(PathBuf::from("src/Widget.svelte")),
        function: Some("<template>".to_string()),
        line: Some(7),
        col: Some(0),
        configured_thresholds: fallow_output::HealthConfiguredThresholds {
            max_cyclomatic: Some(500),
            max_cognitive: Some(500),
            max_crap: None,
            max_unit_size: None,
            max_file_lines: None,
        },
        effective_thresholds: OVERRIDE_THRESHOLDS,
        metrics: Some(fallow_output::ThresholdOverrideMetrics {
            cyclomatic: 9,
            cognitive: 21,
            crap: None,
            line_count: Some(100),
        }),
        reason: None,
    };
    let crap_row = fallow_output::ThresholdOverrideState {
        dimension: fallow_output::ThresholdOverrideDimension::Crap,
        outstanding: Vec::new(),
        metrics: Some(fallow_output::ThresholdOverrideMetrics {
            cyclomatic: 9,
            cognitive: 21,
            crap: Some(90.0),
            line_count: None,
        }),
        ..base.clone()
    };
    report.threshold_overrides = vec![base, crap_row];

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("Health threshold overrides (1)"), "{text}");
    assert!(text.contains("#0 complexity active"), "{text}");
    assert!(text.contains("#0 crap active"), "{text}");
    assert!(text.contains("(still breaches: CRAP)"), "{text}");
}

/// The measured span renders only when the engine attached one: complexity
/// rows carry it, CRAP rows never do, so a CRAP-breach claim is never
/// juxtaposed with an unrelated unit-size number (issue #2163 follow-up).
#[test]
fn threshold_override_row_shows_line_count_only_when_present() {
    let root = PathBuf::from("/project");
    let mut report = disclosure_report(vec![disclosure_violation(
        root.join("src/Widget.svelte"),
        "<template>",
        fallow_output::ExceededThreshold::Crap,
    )]);
    report.threshold_overrides = vec![
        threshold_override_row(
            0,
            fallow_output::ThresholdOverrideStatus::Active,
            fallow_output::ThresholdOverrideDimension::Complexity,
            "src/Widget.svelte",
        ),
        threshold_override_row(
            0,
            fallow_output::ThresholdOverrideStatus::Active,
            fallow_output::ThresholdOverrideDimension::Crap,
            "src/Widget.svelte",
        ),
    ];

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(
            text.contains("#0 complexity active src/Widget.svelte:7:<template> cyclomatic=9 cognitive=21 lines=100"),
            "{text}"
        );
    assert!(
        text.contains(
            "#0 crap active src/Widget.svelte:7:<template> cyclomatic=9 cognitive=21 crap=90.0"
        ),
        "{text}"
    );
}

fn threshold_override_row(
    override_index: usize,
    status: fallow_output::ThresholdOverrideStatus,
    dimension: fallow_output::ThresholdOverrideDimension,
    path: &str,
) -> fallow_output::ThresholdOverrideState {
    let no_match = matches!(status, fallow_output::ThresholdOverrideStatus::NoMatch);
    let is_crap = matches!(dimension, fallow_output::ThresholdOverrideDimension::Crap);
    fallow_output::ThresholdOverrideState {
        status,
        override_index,
        dimension,
        outstanding: Vec::new(),
        path: (!no_match).then(|| PathBuf::from(path)),
        function: (!no_match).then(|| "<template>".to_string()),
        line: (!no_match).then_some(7),
        col: (!no_match).then_some(0),
        configured_thresholds: fallow_output::HealthConfiguredThresholds {
            max_cyclomatic: Some(500),
            max_cognitive: Some(500),
            max_crap: None,
            max_unit_size: None,
            max_file_lines: None,
        },
        effective_thresholds: OVERRIDE_THRESHOLDS,
        metrics: (!no_match).then(|| fallow_output::ThresholdOverrideMetrics {
            cyclomatic: 9,
            cognitive: 21,
            crap: is_crap.then_some(90.0),
            line_count: (!is_crap).then_some(100),
        }),
        reason: None,
    }
}

/// A wide glob emits one row per matched function, so the section is capped
/// like file scores and large functions. Actionable statuses survive the
/// cap ahead of `stale`, the header keeps counting configured entries, and
/// the overflow line names what was hidden (issue #2163 follow-up).
#[test]
fn threshold_override_section_caps_rows_and_keeps_actionable_statuses() {
    let root = PathBuf::from("/project");
    let mut report = disclosure_report(vec![disclosure_violation(
        root.join("src/Widget.svelte"),
        "<template>",
        fallow_output::ExceededThreshold::Crap,
    )]);
    let mut rows = Vec::new();
    for index in 0..12 {
        rows.push(threshold_override_row(
            0,
            fallow_output::ThresholdOverrideStatus::Stale,
            fallow_output::ThresholdOverrideDimension::Complexity,
            &format!("src/stale{index}.spec.ts"),
        ));
    }
    rows.push(threshold_override_row(
        1,
        fallow_output::ThresholdOverrideStatus::NoMatch,
        fallow_output::ThresholdOverrideDimension::Complexity,
        "",
    ));
    rows.push(threshold_override_row(
        0,
        fallow_output::ThresholdOverrideStatus::Insufficient,
        fallow_output::ThresholdOverrideDimension::Complexity,
        "src/big.spec.ts",
    ));
    report.threshold_overrides = rows;

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("Health threshold overrides (2)"), "{text}");
    let row_count = text
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .count();
    assert_eq!(row_count, MAX_FLAT_ITEMS, "{text}");
    assert!(text.contains("#1 complexity no_match"), "{text}");
    assert!(text.contains("#0 complexity insufficient"), "{text}");
    assert!(
        text.contains("... and 4 more rows (4 stale) (--format json for full list)"),
        "{text}"
    );
}

#[test]
fn sfc_template_findings_get_markup_suppression_hint() {
    let root = PathBuf::from("/project");
    let report = disclosure_report(vec![disclosure_violation(
        root.join("src/Widget.svelte"),
        "<template>",
        fallow_output::ExceededThreshold::Crap,
    )]);

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(
        text.contains(fallow_output::SFC_TEMPLATE_SUPPRESS_COMMENT),
        "{text}"
    );
    assert!(!text.contains("above @Component"), "{text}");
}

#[test]
fn inline_angular_template_findings_keep_decorator_hint() {
    let root = PathBuf::from("/project");
    for file in [
        "src/host.component.ts",
        "src/host.component.tsx",
        "src/host.component.mjs",
        "src/host.component.gts",
    ] {
        let report = disclosure_report(vec![disclosure_violation(
            root.join(file),
            "<template>",
            fallow_output::ExceededThreshold::Crap,
        )]);

        let text = plain(&build_health_human_lines(&report, &root));
        assert!(text.contains("above @Component"), "{file}: {text}");
        assert!(
            !text.contains("To suppress SFC templates"),
            "{file}: {text}"
        );
    }
}

/// The human hint and the JSON `suppress-line` action are one contract: any
/// finding routed to `above-angular-decorator` must also produce the human
/// decorator hint. Narrowing either side to an extension allowlist silently
/// splits them (issue #2163 follow-up). The lockstep holds for the default
/// action options asserted here; `health.suggestInlineSuppression: false`
/// drops both surfaces together, which is why this pins the default.
#[test]
fn angular_decorator_action_and_human_hint_stay_in_lockstep() {
    let root = PathBuf::from("/project");
    let ctx = fallow_output::HealthActionContext {
        opts: fallow_output::HealthActionOptions::default(),
        max_cyclomatic_threshold: 20,
        max_cognitive_threshold: 15,
        max_crap_threshold: 30.0,
        crap_refactor_band: 5,
    };
    for file in [
        "src/host.component.ts",
        "src/host.component.js",
        "src/host.component.mts",
        "src/host.component.cts",
        "src/host.component.mjs",
        "src/host.component.cjs",
        "src/host.component.tsx",
        "src/host.component.jsx",
        "src/host.component.gts",
        "src/host.component.gjs",
        "src/host.component.html",
        "src/Widget.svelte",
        "src/Panel.vue",
        "src/Board.astro",
    ] {
        let violation = disclosure_violation(
            root.join(file),
            "<template>",
            fallow_output::ExceededThreshold::Crap,
        );
        let is_angular = fallow_output::build_health_finding_actions(&violation, &ctx)
            .iter()
            .any(|action| action.placement.as_deref() == Some("above-angular-decorator"));
        let text = plain(&build_health_human_lines(
            &disclosure_report(vec![violation]),
            &root,
        ));
        assert_eq!(
            is_angular,
            text.contains("above @Component"),
            "{file}: JSON action and human hint disagree: {text}"
        );
    }
}

#[test]
fn mixed_template_findings_emit_both_suppression_hints() {
    let root = PathBuf::from("/project");
    let report = disclosure_report(vec![
        disclosure_violation(
            root.join("src/Widget.svelte"),
            "<template>",
            fallow_output::ExceededThreshold::Crap,
        ),
        disclosure_violation(
            root.join("src/host.component.ts"),
            "<template>",
            fallow_output::ExceededThreshold::Crap,
        ),
    ]);

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("To suppress SFC templates"), "{text}");
    assert!(text.contains("above @Component"), "{text}");
}

#[test]
fn health_shown_vs_total_when_truncated() {
    let root = PathBuf::from("/project");
    let report = fallow_output::HealthReport {
        findings: vec![
            fallow_output::ComplexityViolation {
                path: root.join("src/a.ts"),
                name: "fn1".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 50,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: fallow_output::ExceededThreshold::Both,
                effective_severity: None,
                severity: fallow_output::FindingSeverity::High,
                ..Default::default()
            }
            .into(),
        ],
        summary: fallow_output::HealthSummary {
            files_analyzed: 100,
            functions_analyzed: 500,
            functions_above_threshold: 10,
            ..Default::default()
        },
        ..Default::default()
    };
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("1 shown, 10 total"));
}

#[test]
fn health_findings_explain_estimated_crap_scores() {
    let root = PathBuf::from("/project");
    let report = fallow_output::HealthReport {
        findings: vec![
            fallow_output::ComplexityViolation {
                path: root.join("src/risky.ts"),
                name: "risky".to_string(),
                line: 7,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 80,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: fallow_output::ExceededThreshold::Crap,
                effective_severity: None,
                severity: fallow_output::FindingSeverity::High,
                crap: Some(650.0),
                mutation: None,
                coverage_pct: None,
                coverage_tier: Some(fallow_output::CoverageTier::None),
                coverage_source: Some(fallow_output::CoverageSource::Estimated),
                ..Default::default()
            }
            .into(),
        ],
        summary: fallow_output::HealthSummary {
            files_analyzed: 1,
            functions_analyzed: 1,
            functions_above_threshold: 1,
            coverage_model: Some(fallow_output::CoverageModel::StaticEstimated),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("CRAP scores are estimated from export references"));
    assert!(text.contains("fallow health --coverage <coverage-final.json>"));
}

#[test]
fn health_findings_explain_mixed_istanbul_crap_scores() {
    let root = PathBuf::from("/project");
    let report = fallow_output::HealthReport {
        findings: vec![
            fallow_output::ComplexityViolation {
                path: root.join("src/risky.ts"),
                name: "risky".to_string(),
                line: 7,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 80,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: fallow_output::ExceededThreshold::Crap,
                effective_severity: None,
                severity: fallow_output::FindingSeverity::High,
                crap: Some(45.0),
                mutation: None,
                coverage_pct: Some(40.0),
                coverage_tier: Some(fallow_output::CoverageTier::Partial),
                coverage_source: Some(fallow_output::CoverageSource::Istanbul),
                ..Default::default()
            }
            .into(),
        ],
        summary: fallow_output::HealthSummary {
            files_analyzed: 1,
            functions_analyzed: 2,
            functions_above_threshold: 1,
            coverage_model: Some(fallow_output::CoverageModel::Istanbul),
            coverage_source_consistency: None,
            istanbul_matched: Some(1),
            istanbul_total: Some(2),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = plain(&build_health_human_lines(&report, &root));
    assert!(
            text.contains(
                "CRAP scores use Istanbul coverage where matched (1/2 functions); the rest fall back to the static estimate."
            ),
            "mixed Istanbul note missing from output: {text}"
        );
}

#[test]
fn crap_notes_name_v8_coverage_input() {
    let report = |format| fallow_output::HealthReport {
        summary: fallow_output::HealthSummary {
            coverage_model: Some(fallow_output::CoverageModel::Istanbul),
            coverage_input_format: format,
            istanbul_matched: Some(2),
            istanbul_total: Some(2),
            ..Default::default()
        },
        ..Default::default()
    };
    let v8 = report(Some(fallow_output::CoverageInputFormat::V8));
    assert_eq!(
        file_scores_crap_note(&v8),
        "CRAP from V8 coverage data (2/2 functions matched)."
    );
    let istanbul = report(Some(fallow_output::CoverageInputFormat::Istanbul));
    assert_eq!(
        file_scores_crap_note(&istanbul),
        "CRAP from Istanbul coverage data (2/2 functions matched)."
    );
}

#[test]
fn health_findings_explain_istanbul_counts_without_summary_model() {
    let root = PathBuf::from("/project");
    let report = fallow_output::HealthReport {
        findings: vec![
            fallow_output::ComplexityViolation {
                path: root.join("src/risky.ts"),
                name: "risky".to_string(),
                line: 7,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 80,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: fallow_output::ExceededThreshold::Crap,
                effective_severity: None,
                severity: fallow_output::FindingSeverity::High,
                crap: Some(45.0),
                mutation: None,
                coverage_pct: None,
                coverage_tier: Some(fallow_output::CoverageTier::None),
                coverage_source: Some(fallow_output::CoverageSource::Estimated),
                ..Default::default()
            }
            .into(),
        ],
        summary: fallow_output::HealthSummary {
            files_analyzed: 1,
            functions_analyzed: 2,
            functions_above_threshold: 1,
            coverage_model: None,
            coverage_source_consistency: None,
            istanbul_matched: Some(1),
            istanbul_total: Some(2),
            ..Default::default()
        },
        ..Default::default()
    };
    let text = plain(&build_health_human_lines(&report, &root));
    assert!(
            text.contains(
                "CRAP scores use Istanbul coverage where matched (1/2 functions); the rest fall back to the static estimate."
            ),
            "Istanbul counts should drive the note even when coverage_model is omitted: {text}"
        );
}

#[test]
fn health_findings_grouped_by_file() {
    let root = PathBuf::from("/project");
    let report = fallow_output::HealthReport {
        findings: vec![
            fallow_output::ComplexityViolation {
                path: root.join("src/parser.ts"),
                name: "fn1".to_string(),
                line: 10,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 40,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: fallow_output::ExceededThreshold::Both,
                effective_severity: None,
                severity: fallow_output::FindingSeverity::High,
                ..Default::default()
            }
            .into(),
            fallow_output::ComplexityViolation {
                path: root.join("src/parser.ts"),
                name: "fn2".to_string(),
                line: 60,
                col: 0,
                cyclomatic: 22,
                cognitive: 18,
                line_count: 30,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: fallow_output::ExceededThreshold::Both,
                effective_severity: None,
                severity: fallow_output::FindingSeverity::High,
                ..Default::default()
            }
            .into(),
        ],
        summary: fallow_output::HealthSummary {
            files_analyzed: 10,
            functions_analyzed: 50,
            functions_above_threshold: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    let count = text.matches("src/parser.ts").count();
    assert_eq!(count, 1, "File header should appear once for grouped items");
}

fn empty_report() -> fallow_output::HealthReport {
    fallow_output::HealthReport {
        summary: fallow_output::HealthSummary {
            files_analyzed: 10,
            functions_analyzed: 50,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn health_runtime_coverage_renders_section() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.runtime_coverage = Some(fallow_output::RuntimeCoverageReport {
        schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
        verdict: fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected,
        signals: Vec::new(),
        summary: fallow_output::RuntimeCoverageSummary {
            data_source: fallow_output::RuntimeCoverageDataSource::Local,
            last_received_at: None,
            functions_tracked: Some(4),
            functions_hit: 2,
            functions_unhit: Some(1),
            functions_untracked: 1,
            coverage_percent: Some(50.0),
            trace_count: 2_847_291,
            period_days: 30,
            deployments_seen: 14,
            ..Default::default()
        },
        findings: vec![fallow_output::RuntimeCoverageFinding {
            id: "fallow:prod:deadbeef".to_owned(),
            stable_id: None,
            path: root.join("src/cold.ts"),
            function: "coldPath".to_owned(),
            line: 14,
            verdict: fallow_output::RuntimeCoverageVerdict::ReviewRequired,
            invocations: Some(0),
            confidence: fallow_output::RuntimeCoverageConfidence::Medium,
            evidence: fallow_output::RuntimeCoverageEvidence {
                static_status: "used".to_owned(),
                test_coverage: "not_covered".to_owned(),
                test_only_reference: None,
                v8_tracking: "tracked".to_owned(),
                untracked_reason: None,
                observation_days: 30,
                deployments_observed: 14,
            },
            actions: vec![],
            source_hash: None,
            discriminators: None,
        }],
        hot_paths: vec![fallow_output::RuntimeCoverageHotPath {
            id: "fallow:hot:cafebabe".to_owned(),
            stable_id: None,
            path: root.join("src/hot.ts"),
            function: "hotPath".to_owned(),
            line: 3,
            end_line: 9,
            invocations: 250,
            percentile: 99,
            actions: vec![],
            optimization_target: None,
        }],
        blast_radius: vec![],
        importance: vec![],
        watermark: Some(fallow_output::RuntimeCoverageWatermark::LicenseExpiredGrace),
        warnings: vec![],
        actionable: true,
        actionability_reason: None,
        actionability_verdict: None,
        provenance: fallow_output::RuntimeCoverageProvenance::default(),
    });

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("Runtime coverage: cold code detected"));
    assert!(text.contains("src/cold.ts:14 coldPath [0 invocations, review required]"));
    assert!(text.contains("license expired grace active"));
    assert!(text.contains("hot paths:"));
    assert!(text.contains("src/hot.ts:3 hotPath (250 invocations, p99)"));
    assert!(!text.contains("short capture:"));
    assert!(!text.contains("start a trial"));
}

#[test]
fn health_coverage_intelligence_renders_findings_and_ambiguity_summary() {
    use fallow_output::{
        CoverageIntelligenceAction, CoverageIntelligenceConfidence, CoverageIntelligenceEvidence,
        CoverageIntelligenceFinding, CoverageIntelligenceMatchConfidence,
        CoverageIntelligenceRecommendation, CoverageIntelligenceReport,
        CoverageIntelligenceSchemaVersion, CoverageIntelligenceSignal, CoverageIntelligenceSummary,
        CoverageIntelligenceVerdict,
    };

    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.coverage_intelligence = Some(CoverageIntelligenceReport {
        schema_version: CoverageIntelligenceSchemaVersion::V1,
        verdict: CoverageIntelligenceVerdict::HighConfidenceDelete,
        summary: CoverageIntelligenceSummary {
            findings: 1,
            high_confidence_deletes: 1,
            ..Default::default()
        },
        findings: vec![CoverageIntelligenceFinding {
            id: "fallow:coverage-intel:abc123".to_owned(),
            path: root.join("src/dead.ts"),
            identity: Some("deadPath".to_owned()),
            line: 9,
            verdict: CoverageIntelligenceVerdict::HighConfidenceDelete,
            signals: vec![CoverageIntelligenceSignal::RuntimeCold],
            recommendation: CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner,
            confidence: CoverageIntelligenceConfidence::High,
            related_ids: vec![],
            evidence: CoverageIntelligenceEvidence {
                match_confidence: CoverageIntelligenceMatchConfidence::Direct,
                ..Default::default()
            },
            actions: vec![CoverageIntelligenceAction {
                kind: "delete-after-confirming-owner".to_owned(),
                description: "Confirm ownership before deleting".to_owned(),
                auto_fixable: false,
            }],
        }],
    });

    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("Coverage intelligence"));
    assert!(text.contains("src/dead.ts:9 deadPath high-confidence-delete"));
    assert!(text.contains("Confirm ownership before deleting"));

    report.coverage_intelligence = Some(CoverageIntelligenceReport {
        schema_version: CoverageIntelligenceSchemaVersion::V1,
        verdict: CoverageIntelligenceVerdict::Clean,
        summary: CoverageIntelligenceSummary {
            skipped_ambiguous_matches: 2,
            ..Default::default()
        },
        findings: vec![],
    });
    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("skipped 2 ambiguous evidence matches"));
}

fn runtime_coverage_report_with_quality(
    quality: Option<fallow_output::RuntimeCoverageCaptureQuality>,
) -> fallow_output::RuntimeCoverageReport {
    fallow_output::RuntimeCoverageReport {
        schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
        verdict: fallow_output::RuntimeCoverageReportVerdict::Clean,
        signals: Vec::new(),
        summary: fallow_output::RuntimeCoverageSummary {
            data_source: fallow_output::RuntimeCoverageDataSource::Local,
            last_received_at: None,
            functions_tracked: Some(10),
            functions_hit: 7,
            functions_unhit: Some(0),
            functions_untracked: 3,
            coverage_percent: Some(70.0),
            trace_count: 1_000,
            period_days: 1,
            deployments_seen: 1,
            capture_quality: quality,
            capture_policy: None,
        },
        findings: vec![],
        hot_paths: vec![],
        blast_radius: vec![],
        importance: vec![],
        watermark: None,
        warnings: vec![],
        actionable: true,
        actionability_reason: None,
        actionability_verdict: None,
        provenance: fallow_output::RuntimeCoverageProvenance::default(),
    }
}

#[test]
fn health_runtime_coverage_short_capture_shows_warning_and_prompt() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.runtime_coverage = Some(runtime_coverage_report_with_quality(Some(
        fallow_output::RuntimeCoverageCaptureQuality {
            window_seconds: 720, // 12 min
            instances_observed: 1,
            lazy_parse_warning: true,
            untracked_ratio_percent: 42.5,
        },
    )));
    let text = plain(&build_health_human_lines(&report, &root));
    assert!(
            text.contains(
                "note: short capture (12 min from 1 instance); 42.5% of functions untracked, lazy-parsed scripts may not appear."
            ),
            "warning banner missing or malformed in:\n{text}"
        );
    assert!(
        text.contains("extend the capture or switch to continuous monitoring"),
        "warning follow-up line missing in:\n{text}"
    );
    assert!(
        text.contains("captured 12 min from 1 instance."),
        "upgrade prompt header missing in:\n{text}"
    );
    assert!(
        text.contains("continuous monitoring over 30 days evaluates more paths"),
        "upgrade prompt body missing in:\n{text}"
    );
    assert!(
        text.contains("fallow license activate --trial --email you@company.com"),
        "trial CTA command missing in:\n{text}"
    );
}

#[test]
fn health_runtime_coverage_long_capture_shows_neither_warning_nor_prompt() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.runtime_coverage = Some(runtime_coverage_report_with_quality(Some(
        fallow_output::RuntimeCoverageCaptureQuality {
            window_seconds: 7 * 24 * 3600, // 7 days
            instances_observed: 4,
            lazy_parse_warning: false,
            untracked_ratio_percent: 3.1,
        },
    )));
    let text = plain(&build_health_human_lines(&report, &root));
    assert!(
        !text.contains("short capture"),
        "long capture should not emit short-capture warning:\n{text}"
    );
    assert!(
        !text.contains("start a trial"),
        "long capture should not emit trial CTA:\n{text}"
    );
}

#[test]
fn format_window_labels() {
    assert_eq!(super::format_window(30), "30 s");
    assert_eq!(super::format_window(60), "1 min");
    assert_eq!(super::format_window(720), "12 min");
    assert_eq!(super::format_window(3600 * 3), "3 h");
    assert_eq!(super::format_window(3600 * 24 * 3), "3 d");
}

#[test]
fn health_coverage_gaps_render_section() {
    use fallow_output::*;

    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.coverage_gaps = Some(CoverageGaps {
        summary: CoverageGapSummary {
            runtime_files: 1,
            covered_files: 0,
            file_coverage_pct: 0.0,
            untested_files: 1,
            untested_exports: 1,
        },
        files: vec![UntestedFileFinding::with_actions(
            UntestedFile {
                path: root.join("src/app.ts"),
                value_export_count: 2,
            },
            &root,
        )],
        exports: vec![UntestedExportFinding::with_actions(
            UntestedExport {
                path: root.join("src/app.ts"),
                export_name: "loader".into(),
                line: 12,
                col: 4,
            },
            &root,
        )],
    });

    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(
        text.contains("Coverage gaps (1 untested file, 1 untested export, 0.0% file coverage)")
    );
    assert!(text.contains("src/app.ts"));
    assert!(text.contains("loader"));
    assert_docs_footer_layout(&lines, &format!("{DOCS_HEALTH}#coverage-gaps"));
}

#[test]
fn large_functions_docs_footer_fits_eighty_columns() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.large_functions = vec![fallow_output::LargeFunctionEntry {
        path: root.join("src/a.ts"),
        name: "render".to_string(),
        line: 3,
        line_count: 120,
    }];
    let lines = build_health_human_lines(&report, &root);
    assert_docs_footer_layout(&lines, &format!("{DOCS_HEALTH}#unit-size"));
}

#[test]
fn fmt_trend_val_percentage() {
    assert_eq!(fmt_trend_val(15.5, "%"), "15.5%");
    assert_eq!(fmt_trend_val(0.0, "%"), "0.0%");
}

#[test]
fn fmt_trend_val_integer_when_round() {
    assert_eq!(fmt_trend_val(72.0, ""), "72");
    assert_eq!(fmt_trend_val(5.0, "pts"), "5");
}

#[test]
fn fmt_trend_val_decimal_when_fractional() {
    assert_eq!(fmt_trend_val(4.7, ""), "4.7");
    assert_eq!(fmt_trend_val(1.3, "pts"), "1.3");
}

#[test]
fn fmt_trend_delta_percentage() {
    assert_eq!(fmt_trend_delta(2.5, "%"), "+2.5%");
    assert_eq!(fmt_trend_delta(-1.3, "%"), "-1.3%");
}

#[test]
fn fmt_trend_delta_integer_when_round() {
    assert_eq!(fmt_trend_delta(5.0, ""), "+5");
    assert_eq!(fmt_trend_delta(-3.0, "pts"), "-3");
}

#[test]
fn fmt_trend_delta_decimal_when_fractional() {
    assert_eq!(fmt_trend_delta(4.9, ""), "+4.9");
    assert_eq!(fmt_trend_delta(-0.7, "pts"), "-0.7");
}

#[test]
fn health_score_grade_a_display() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_score = Some(fallow_output::HealthScore {
        formula_version: fallow_output::HEALTH_SCORE_FORMULA_VERSION,
        score: 92.0,
        grade: "A",
        penalties: fallow_output::HealthScorePenalties {
            dead_files: Some(3.0),
            dead_exports: Some(2.0),
            complexity: 1.5,
            p90_complexity: 1.5,
            maintainability: Some(0.0),
            hotspots: Some(0.0),
            unused_deps: Some(0.0),
            circular_deps: Some(0.0),
            unit_size: None,
            coupling: None,
            duplication: None,
            prop_drilling: None,
        },
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("Health score:"));
    assert!(text.contains("92 A"));
    assert!(text.contains("dead files -3.0"));
    assert!(text.contains("dead exports -2.0"));
    assert!(text.contains("complexity -1.5"));
    assert!(text.contains("p90 -1.5"));
}

#[test]
fn health_score_grade_b_display() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_score = Some(fallow_output::HealthScore {
        formula_version: fallow_output::HEALTH_SCORE_FORMULA_VERSION,
        score: 76.0,
        grade: "B",
        penalties: fallow_output::HealthScorePenalties {
            dead_files: Some(5.0),
            dead_exports: Some(6.0),
            complexity: 3.0,
            p90_complexity: 2.0,
            maintainability: Some(4.0),
            hotspots: Some(2.0),
            unused_deps: Some(1.0),
            circular_deps: Some(1.0),
            unit_size: None,
            coupling: None,
            duplication: None,
            prop_drilling: None,
        },
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("76 B"));
    assert!(text.contains("dead exports -6.0"));
    assert!(text.contains("maintainability -4.0"));
    assert!(text.contains("hotspots -2.0"));
    assert!(text.contains("unused deps -1.0"));
    assert!(text.contains("circular deps -1.0"));
}

#[test]
fn health_score_grade_c_display() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_score = Some(fallow_output::HealthScore {
        formula_version: fallow_output::HEALTH_SCORE_FORMULA_VERSION,
        score: 60.0,
        grade: "C",
        penalties: fallow_output::HealthScorePenalties {
            dead_files: Some(10.0),
            dead_exports: Some(10.0),
            complexity: 10.0,
            p90_complexity: 5.0,
            maintainability: Some(5.0),
            hotspots: None,
            unused_deps: None,
            circular_deps: None,
            unit_size: None,
            coupling: None,
            duplication: None,
            prop_drilling: None,
        },
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("60 C"));
}

#[test]
fn health_score_grade_f_display() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_score = Some(fallow_output::HealthScore {
        formula_version: fallow_output::HEALTH_SCORE_FORMULA_VERSION,
        score: 30.0,
        grade: "F",
        penalties: fallow_output::HealthScorePenalties {
            dead_files: Some(15.0),
            dead_exports: Some(15.0),
            complexity: 20.0,
            p90_complexity: 10.0,
            maintainability: Some(10.0),
            hotspots: None,
            unused_deps: None,
            circular_deps: None,
            unit_size: None,
            coupling: None,
            duplication: None,
            prop_drilling: None,
        },
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("30 F"));
}

#[test]
fn health_score_na_components_shown() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_score = Some(fallow_output::HealthScore {
        formula_version: fallow_output::HEALTH_SCORE_FORMULA_VERSION,
        score: 90.0,
        grade: "A",
        penalties: fallow_output::HealthScorePenalties {
            dead_files: None,
            dead_exports: None,
            complexity: 0.0,
            p90_complexity: 0.0,
            maintainability: None,
            hotspots: None,
            unused_deps: None,
            circular_deps: None,
            unit_size: None,
            coupling: None,
            duplication: None,
            prop_drilling: None,
        },
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("N/A: dead code, maintainability, hotspots"));
    assert!(text.contains("enable the corresponding analysis flags"));
}

#[test]
fn health_score_no_na_when_all_present() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_score = Some(fallow_output::HealthScore {
        formula_version: fallow_output::HEALTH_SCORE_FORMULA_VERSION,
        score: 85.0,
        grade: "A",
        penalties: fallow_output::HealthScorePenalties {
            dead_files: Some(0.0),
            dead_exports: Some(0.0),
            complexity: 0.0,
            p90_complexity: 0.0,
            maintainability: Some(0.0),
            hotspots: Some(0.0),
            unused_deps: Some(0.0),
            circular_deps: Some(0.0),
            unit_size: None,
            coupling: None,
            duplication: None,
            prop_drilling: None,
        },
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(!text.contains("N/A:"));
}

#[test]
fn health_score_zero_penalties_suppressed() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_score = Some(fallow_output::HealthScore {
        formula_version: fallow_output::HEALTH_SCORE_FORMULA_VERSION,
        score: 100.0,
        grade: "A",
        penalties: fallow_output::HealthScorePenalties {
            dead_files: Some(0.0),
            dead_exports: Some(0.0),
            complexity: 0.0,
            p90_complexity: 0.0,
            maintainability: Some(0.0),
            hotspots: Some(0.0),
            unused_deps: Some(0.0),
            circular_deps: Some(0.0),
            unit_size: None,
            coupling: None,
            duplication: None,
            prop_drilling: None,
        },
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(!text.contains("dead files"));
    assert!(!text.contains("complexity -"));
}

#[test]
fn health_trend_improving_display() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_trend = Some(fallow_output::HealthTrend {
        compared_to: fallow_output::TrendPoint {
            timestamp: "2026-03-25T14:30:00Z".into(),
            git_sha: Some("abc1234".into()),
            score: Some(72.0),
            grade: Some("B".into()),
            score_formula_version: Some(fallow_output::HEALTH_SCORE_FORMULA_VERSION),
            coverage_model: None,
            snapshot_schema_version: None,
        },
        metrics: vec![
            fallow_output::TrendMetric {
                name: "score",
                label: "Health Score",
                previous: 72.0,
                current: 85.0,
                delta: 13.0,
                direction: fallow_output::TrendDirection::Improving,
                unit: "",
                previous_count: None,
                current_count: None,
            },
            fallow_output::TrendMetric {
                name: "dead_file_pct",
                label: "Dead Files",
                previous: 10.0,
                current: 5.0,
                delta: -5.0,
                direction: fallow_output::TrendDirection::Improving,
                unit: "%",
                previous_count: None,
                current_count: None,
            },
        ],
        snapshots_loaded: 2,
        overall_direction: fallow_output::TrendDirection::Improving,
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("Trend:"));
    assert!(text.contains("improving"));
    assert!(text.contains("vs 2026-03-25"));
    assert!(text.contains("abc1234"));
    assert!(text.contains("Health Score"));
    assert!(text.contains("+13"));
    assert!(text.contains("Dead Files"));
    assert!(text.contains("-5.0%"));
}

#[test]
fn health_trend_declining_display() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_trend = Some(fallow_output::HealthTrend {
        compared_to: fallow_output::TrendPoint {
            timestamp: "2026-03-20T10:00:00Z".into(),
            git_sha: None,
            score: None,
            grade: None,
            score_formula_version: None,
            coverage_model: None,
            snapshot_schema_version: None,
        },
        metrics: vec![fallow_output::TrendMetric {
            name: "unused_deps",
            label: "Unused Deps",
            previous: 5.0,
            current: 10.0,
            delta: 5.0,
            direction: fallow_output::TrendDirection::Declining,
            unit: "",
            previous_count: None,
            current_count: None,
        }],
        snapshots_loaded: 1,
        overall_direction: fallow_output::TrendDirection::Declining,
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("declining"));
    assert!(text.contains("Unused Deps"));
}

#[test]
fn health_trend_all_stable_collapsed() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_trend = Some(fallow_output::HealthTrend {
        compared_to: fallow_output::TrendPoint {
            timestamp: "2026-03-25T14:30:00Z".into(),
            git_sha: Some("def5678".into()),
            score: Some(80.0),
            grade: Some("B".into()),
            score_formula_version: Some(fallow_output::HEALTH_SCORE_FORMULA_VERSION),
            coverage_model: None,
            snapshot_schema_version: None,
        },
        metrics: vec![
            fallow_output::TrendMetric {
                name: "score",
                label: "Health Score",
                previous: 80.0,
                current: 80.0,
                delta: 0.0,
                direction: fallow_output::TrendDirection::Stable,
                unit: "",
                previous_count: None,
                current_count: None,
            },
            fallow_output::TrendMetric {
                name: "avg_cyclomatic",
                label: "Avg Cyclomatic",
                previous: 2.0,
                current: 2.0,
                delta: 0.0,
                direction: fallow_output::TrendDirection::Stable,
                unit: "",
                previous_count: None,
                current_count: None,
            },
        ],
        snapshots_loaded: 3,
        overall_direction: fallow_output::TrendDirection::Stable,
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("stable"));
    assert!(text.contains("All 2 metrics unchanged"));
    assert!(!text.contains("Health Score"));
}

#[test]
fn health_trend_without_sha() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.health_trend = Some(fallow_output::HealthTrend {
        compared_to: fallow_output::TrendPoint {
            timestamp: "2026-03-20T10:00:00Z".into(),
            git_sha: None,
            score: None,
            grade: None,
            score_formula_version: None,
            coverage_model: None,
            snapshot_schema_version: None,
        },
        metrics: vec![fallow_output::TrendMetric {
            name: "score",
            label: "Health Score",
            previous: 80.0,
            current: 82.0,
            delta: 2.0,
            direction: fallow_output::TrendDirection::Improving,
            unit: "",
            previous_count: None,
            current_count: None,
        }],
        snapshots_loaded: 1,
        overall_direction: fallow_output::TrendDirection::Improving,
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("vs 2026-03-20"));
    assert!(!text.contains("\u{00b7}"));
}

#[test]
fn vital_signs_shown_without_trend() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.vital_signs = Some(fallow_output::VitalSigns {
        dead_file_pct: Some(3.2),
        dead_export_pct: Some(8.1),
        avg_cyclomatic: 4.7,
        p90_cyclomatic: 12,
        duplication_pct: None,
        hotspot_count: Some(2),
        maintainability_avg: Some(72.4),
        unused_dep_count: Some(3),
        circular_dep_count: Some(1),
        counts: None,
        unit_size_profile: None,
        unit_interfacing_profile: None,
        p95_fan_in: None,
        coupling_high_pct: None,
        total_loc: 42_381,
        ..Default::default()
    });
    report.hotspot_summary = Some(fallow_output::HotspotSummary {
        since: "6 months".to_string(),
        min_commits: 3,
        files_analyzed: 50,
        files_excluded: 20,
        shallow_clone: false,
        clock: None,
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("42,381 LOC"));
    assert!(text.contains("dead files 3.2%"));
    assert!(text.contains("dead exports 8.1%"));
    assert!(text.contains("avg cyclomatic 4.7"));
    assert!(text.contains("p90 cyclomatic 12"));
    assert!(text.contains("maintainability 72.4"));
    assert!(text.contains("2 churn hotspots (since 6 months)"));
    assert!(text.contains("3 unused deps"));
    assert!(text.contains("1 circular dep"));
}

#[test]
fn vital_signs_zero_hotspots_still_show_window() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.vital_signs = Some(fallow_output::VitalSigns {
        avg_cyclomatic: 2.0,
        p90_cyclomatic: 5,
        hotspot_count: Some(0),
        total_loc: 1_000,
        ..Default::default()
    });
    report.hotspot_summary = Some(fallow_output::HotspotSummary {
        since: "90 days".to_string(),
        min_commits: 3,
        files_analyzed: 10,
        files_excluded: 0,
        shallow_clone: false,
        clock: None,
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("0 churn hotspots (since 90 days)"));
    assert!(!text.contains("Hotspots ("));
}

#[test]
fn vital_signs_hotspot_count_without_summary_omits_window() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.vital_signs = Some(fallow_output::VitalSigns {
        avg_cyclomatic: 2.0,
        p90_cyclomatic: 5,
        hotspot_count: Some(1),
        total_loc: 1_000,
        ..Default::default()
    });
    report.hotspot_summary = None;
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("1 churn hotspot"));
    assert!(!text.contains("(since"));
}

#[test]
fn vital_signs_suppressed_when_trend_active() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.vital_signs = Some(fallow_output::VitalSigns {
        dead_file_pct: Some(3.2),
        dead_export_pct: Some(8.1),
        avg_cyclomatic: 4.7,
        p90_cyclomatic: 12,
        duplication_pct: None,
        hotspot_count: Some(2),
        maintainability_avg: Some(72.4),
        unused_dep_count: None,
        circular_dep_count: None,
        counts: None,
        unit_size_profile: None,
        unit_interfacing_profile: None,
        p95_fan_in: None,
        coupling_high_pct: None,
        total_loc: 0,
        ..Default::default()
    });
    report.health_trend = Some(fallow_output::HealthTrend {
        compared_to: fallow_output::TrendPoint {
            timestamp: "2026-03-25T14:30:00Z".into(),
            git_sha: None,
            score: None,
            grade: None,
            score_formula_version: None,
            coverage_model: None,
            snapshot_schema_version: None,
        },
        metrics: vec![],
        snapshots_loaded: 1,
        overall_direction: fallow_output::TrendDirection::Stable,
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(!text.contains("dead files"));
    assert!(!text.contains("avg cyclomatic"));
}

#[test]
fn vital_signs_optional_fields_omitted_when_none() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.vital_signs = Some(fallow_output::VitalSigns {
        dead_file_pct: None,
        dead_export_pct: None,
        avg_cyclomatic: 2.0,
        p90_cyclomatic: 5,
        duplication_pct: None,
        hotspot_count: None,
        maintainability_avg: None,
        unused_dep_count: None,
        circular_dep_count: None,
        counts: None,
        unit_size_profile: None,
        unit_interfacing_profile: None,
        p95_fan_in: None,
        coupling_high_pct: None,
        total_loc: 0,
        ..Default::default()
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(!text.contains("dead files"));
    assert!(!text.contains("dead exports"));
    assert!(!text.contains("maintainability "));
    assert!(!text.contains("hotspot"));
    assert!(text.contains("avg cyclomatic 2.0"));
    assert!(text.contains("p90 cyclomatic 5"));
}

#[test]
fn vital_signs_zero_counts_suppressed() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.vital_signs = Some(fallow_output::VitalSigns {
        dead_file_pct: None,
        dead_export_pct: None,
        avg_cyclomatic: 1.0,
        p90_cyclomatic: 2,
        duplication_pct: None,
        hotspot_count: None,
        maintainability_avg: None,
        unused_dep_count: Some(0),
        circular_dep_count: Some(0),
        counts: None,
        unit_size_profile: None,
        unit_interfacing_profile: None,
        p95_fan_in: None,
        coupling_high_pct: None,
        total_loc: 0,
        ..Default::default()
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(!text.contains("unused dep"));
    assert!(!text.contains("circular dep"));
}

#[test]
fn vital_signs_plural_vs_singular() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.vital_signs = Some(fallow_output::VitalSigns {
        dead_file_pct: None,
        dead_export_pct: None,
        avg_cyclomatic: 1.0,
        p90_cyclomatic: 2,
        duplication_pct: None,
        hotspot_count: Some(1),
        maintainability_avg: None,
        unused_dep_count: Some(1),
        circular_dep_count: Some(2),
        counts: None,
        unit_size_profile: None,
        unit_interfacing_profile: None,
        p95_fan_in: None,
        coupling_high_pct: None,
        total_loc: 0,
        ..Default::default()
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("1 churn hotspot"));
    assert!(!text.contains("1 churn hotspots"));
    assert!(text.contains("1 unused dep"));
    assert!(!text.contains("1 unused deps"));
    assert!(text.contains("2 circular deps"));
}

#[test]
fn file_scores_single_entry() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.file_scores = vec![fallow_output::FileHealthScore {
        path: root.join("src/utils.ts"),
        fan_in: 5,
        fan_out: 3,
        dead_code_ratio: 0.15,
        complexity_density: 0.42,
        maintainability_index: 85.3,
        total_cyclomatic: 12,
        total_cognitive: 8,
        function_count: 4,
        lines: 200,
        crap_max: 0.0,
        crap_above_threshold: 0,
        crap_exempted: 0,
        crap_suppressed: 0,
        crap_unsuppressed_max: None,
        crap_unsuppressed_effective_threshold: None,
        crap_effective_threshold: None,
    }];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("File health scores (1 file)"));
    assert!(!text.contains("(1 files)"));
    assert!(text.contains("85.3"));
    assert!(text.contains("src/utils.ts"));
    assert!(text.contains("200 LOC"));
    assert!(text.contains("5 fan-in"));
    assert!(text.contains("3 fan-out"));
    assert!(text.contains("15% dead"));
    assert!(text.contains("0.42 density"));
}

#[test]
fn file_score_rows_normalize_backslash_separators() {
    // A Windows path reaches the renderer with native separators. The human
    // report is read by people and matched by tooling, so it must show the
    // same forward-slash form on every platform.
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.file_scores = vec![fallow_output::FileHealthScore {
        path: root.join("src\\utils.ts"),
        fan_in: 5,
        fan_out: 3,
        dead_code_ratio: 0.15,
        complexity_density: 0.42,
        maintainability_index: 85.3,
        total_cyclomatic: 12,
        total_cognitive: 8,
        function_count: 4,
        lines: 200,
        crap_max: 0.0,
        crap_above_threshold: 0,
        crap_exempted: 0,
        crap_suppressed: 0,
        crap_unsuppressed_max: None,
        crap_unsuppressed_effective_threshold: None,
        crap_effective_threshold: None,
    }];
    let text = plain(&build_health_human_lines(&report, &root));

    assert!(
        text.contains("src/utils.ts"),
        "file-score rows must use forward slashes: {text}"
    );
    assert!(
        !text.contains("src\\utils.ts"),
        "file-score rows must not leak native separators: {text}"
    );
}

#[test]
fn file_scores_concern_tag_marks_risk_vs_structure() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.file_scores = vec![
        fallow_output::FileHealthScore {
            path: root.join("src/risky.ts"),
            fan_in: 0,
            fan_out: 0,
            dead_code_ratio: 0.0,
            complexity_density: 0.2,
            maintainability_index: 85.0,
            total_cyclomatic: 10,
            total_cognitive: 8,
            function_count: 1,
            lines: 100,
            crap_max: 552.0,
            crap_above_threshold: 1,
            crap_exempted: 0,
            crap_suppressed: 0,
            crap_unsuppressed_max: None,
            crap_unsuppressed_effective_threshold: None,
            crap_effective_threshold: None,
        },
        fallow_output::FileHealthScore {
            path: root.join("src/messy.ts"),
            fan_in: 0,
            fan_out: 0,
            dead_code_ratio: 0.0,
            complexity_density: 0.3,
            maintainability_index: 30.0,
            total_cyclomatic: 5,
            total_cognitive: 3,
            function_count: 1,
            lines: 100,
            crap_max: 2.0,
            crap_above_threshold: 0,
            crap_exempted: 0,
            crap_suppressed: 0,
            crap_unsuppressed_max: None,
            crap_unsuppressed_effective_threshold: None,
            crap_effective_threshold: None,
        },
    ];
    let text = plain(&build_health_human_lines(&report, &root));
    let risky_line = text
        .lines()
        .find(|l| l.contains("risky.ts"))
        .expect("risky path line");
    assert!(
        risky_line.trim_end().ends_with("risk"),
        "expected risk tag, got: {risky_line:?}"
    );
    let messy_line = text
        .lines()
        .find(|l| l.contains("messy.ts"))
        .expect("messy path line");
    assert!(
        messy_line.trim_end().ends_with("structure"),
        "expected structure tag, got: {messy_line:?}"
    );
}

fn exempt_score(
    root: &Path,
    crap_effective_threshold: Option<f64>,
) -> fallow_output::FileHealthScore {
    fallow_output::FileHealthScore {
        path: root.join("src/legacy.ts"),
        fan_in: 1,
        fan_out: 0,
        dead_code_ratio: 0.0,
        complexity_density: 0.77,
        maintainability_index: 88.0,
        total_cyclomatic: 20,
        total_cognitive: 18,
        function_count: 2,
        lines: 26,
        crap_max: 110.0,
        crap_above_threshold: 0,
        crap_exempted: 2,
        crap_suppressed: 0,
        crap_unsuppressed_max: None,
        crap_unsuppressed_effective_threshold: None,
        crap_effective_threshold,
    }
}

#[test]
fn file_scores_override_exempt_row_reads_structure_with_marker() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.file_scores = vec![exempt_score(&root, Some(500.0))];
    let text = plain(&build_health_human_lines(&report, &root));
    let tag_line = text
        .lines()
        .find(|l| l.contains("legacy.ts"))
        .expect("legacy path line");
    assert!(
        tag_line.trim_end().ends_with("structure"),
        "expected structure tag on an override-exempt row, got: {tag_line:?}"
    );
    let metrics_line = text
        .lines()
        .find(|l| l.contains("110.0 risk"))
        .expect("risk metrics line");
    assert!(
        metrics_line.contains("exempt (override)"),
        "expected override marker, got: {metrics_line:?}"
    );
}

#[test]
fn file_scores_global_raise_marker_and_relative_note_bands() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.summary.max_crap_threshold = 5000.0;
    report.file_scores = vec![exempt_score(&root, None)];
    let text = plain(&build_health_human_lines(&report, &root));
    let metrics_line = text
        .lines()
        .find(|l| l.contains("110.0 risk"))
        .expect("risk metrics line");
    assert!(
        metrics_line.contains("exempt (raised threshold)"),
        "expected global-raise marker, got: {metrics_line:?}"
    );
    assert!(
        text.contains("Risk: low <2500, moderate 2500-5000, high >=5000."),
        "expected bands relative to the run global"
    );
    assert!(!text.contains("moderate 15-30"));
}

#[test]
fn file_scores_disabled_crap_enforcement_notes_and_marks_rows() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.summary.max_crap_threshold = 0.0;
    report.file_scores = vec![exempt_score(&root, None)];
    let text = plain(&build_health_human_lines(&report, &root));
    let metrics_line = text
        .lines()
        .find(|l| l.contains("110.0 risk"))
        .expect("risk metrics line");
    assert!(
        metrics_line.contains("exempt (crap disabled)"),
        "expected disabled-enforcement marker, got: {metrics_line:?}"
    );
    assert!(text.contains("CRAP enforcement is disabled (maxCrap 0)"));
}

#[test]
fn file_scores_default_note_keeps_canonical_bands_without_markers() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.file_scores = vec![fallow_output::FileHealthScore {
        path: root.join("src/risky.ts"),
        fan_in: 0,
        fan_out: 0,
        dead_code_ratio: 0.0,
        complexity_density: 0.5,
        maintainability_index: 85.0,
        total_cyclomatic: 20,
        total_cognitive: 18,
        function_count: 2,
        lines: 40,
        crap_max: 110.0,
        crap_above_threshold: 2,
        crap_exempted: 0,
        crap_suppressed: 0,
        crap_unsuppressed_max: None,
        crap_unsuppressed_effective_threshold: None,
        crap_effective_threshold: None,
    }];
    let text = plain(&build_health_human_lines(&report, &root));
    assert!(text.contains("Risk: low <15, moderate 15-30, high >=30."));
    assert!(!text.contains("exempt ("));
    let tag_line = text
        .lines()
        .find(|l| l.contains("risky.ts"))
        .expect("risky path line");
    assert!(tag_line.trim_end().ends_with("risk"));
}

#[test]
fn file_scores_insufficient_override_row_keeps_risk_story() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    let mut score = exempt_score(&root, Some(50.0));
    score.crap_above_threshold = 2;
    score.crap_exempted = 0;
    report.file_scores = vec![score];
    let text = plain(&build_health_human_lines(&report, &root));
    let tag_line = text
        .lines()
        .find(|l| l.contains("legacy.ts"))
        .expect("legacy path line");
    assert!(
        tag_line.trim_end().ends_with("risk"),
        "expected risk tag when the override is insufficient, got: {tag_line:?}"
    );
    assert!(!text.contains("exempt ("));
}

#[test]
fn file_scores_mi_color_thresholds() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.file_scores = vec![
        fallow_output::FileHealthScore {
            path: root.join("src/good.ts"),
            fan_in: 1,
            fan_out: 1,
            dead_code_ratio: 0.0,
            complexity_density: 0.1,
            maintainability_index: 90.0, // green: >= 80
            total_cyclomatic: 2,
            total_cognitive: 1,
            function_count: 1,
            lines: 50,
            crap_max: 0.0,
            crap_above_threshold: 0,
            crap_exempted: 0,
            crap_suppressed: 0,
            crap_unsuppressed_max: None,
            crap_unsuppressed_effective_threshold: None,
            crap_effective_threshold: None,
        },
        fallow_output::FileHealthScore {
            path: root.join("src/okay.ts"),
            fan_in: 2,
            fan_out: 3,
            dead_code_ratio: 0.1,
            complexity_density: 0.3,
            maintainability_index: 65.0, // yellow: >= 50
            total_cyclomatic: 8,
            total_cognitive: 5,
            function_count: 3,
            lines: 100,
            crap_max: 0.0,
            crap_above_threshold: 0,
            crap_exempted: 0,
            crap_suppressed: 0,
            crap_unsuppressed_max: None,
            crap_unsuppressed_effective_threshold: None,
            crap_effective_threshold: None,
        },
        fallow_output::FileHealthScore {
            path: root.join("src/bad.ts"),
            fan_in: 8,
            fan_out: 12,
            dead_code_ratio: 0.5,
            complexity_density: 0.9,
            maintainability_index: 30.0, // red: < 50
            total_cyclomatic: 40,
            total_cognitive: 30,
            function_count: 10,
            lines: 500,
            crap_max: 0.0,
            crap_above_threshold: 0,
            crap_exempted: 0,
            crap_suppressed: 0,
            crap_unsuppressed_max: None,
            crap_unsuppressed_effective_threshold: None,
            crap_effective_threshold: None,
        },
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("File health scores (3 files)"));
    assert!(text.contains("90.0"));
    assert!(text.contains("65.0"));
    assert!(text.contains("30.0"));
}

#[test]
fn file_scores_truncation_above_max_flat_items() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    for i in 0..12 {
        report.file_scores.push(fallow_output::FileHealthScore {
            path: root.join(format!("src/file{i}.ts")),
            fan_in: 1,
            fan_out: 1,
            dead_code_ratio: 0.0,
            complexity_density: 0.1,
            maintainability_index: 80.0,
            total_cyclomatic: 2,
            total_cognitive: 1,
            function_count: 1,
            lines: 50,
            crap_max: 0.0,
            crap_above_threshold: 0,
            crap_exempted: 0,
            crap_suppressed: 0,
            crap_unsuppressed_max: None,
            crap_unsuppressed_effective_threshold: None,
            crap_effective_threshold: None,
        });
    }
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("File health scores (12 files)"));
    assert!(text.contains("... and 2 more files"));
    assert!(text.contains("file0.ts"));
    assert!(text.contains("file9.ts"));
    assert!(!text.contains("file10.ts"));
    assert!(!text.contains("file11.ts"));
}

#[test]
fn file_scores_docs_link() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.file_scores = vec![fallow_output::FileHealthScore {
        path: root.join("src/a.ts"),
        fan_in: 1,
        fan_out: 1,
        dead_code_ratio: 0.0,
        complexity_density: 0.1,
        maintainability_index: 80.0,
        total_cyclomatic: 2,
        total_cognitive: 1,
        function_count: 1,
        lines: 50,
        crap_max: 0.0,
        crap_above_threshold: 0,
        crap_exempted: 0,
        crap_suppressed: 0,
        crap_unsuppressed_max: None,
        crap_unsuppressed_effective_threshold: None,
        crap_effective_threshold: None,
    }];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("fallow.tools/docs/explanations/health/#file-health-scores"));
    assert_docs_footer_layout(&lines, &format!("{DOCS_HEALTH}#file-health-scores"));
}

#[test]
fn hotspots_accelerating_trend() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.hotspots = vec![
        fallow_output::HotspotEntry {
            path: root.join("src/core.ts"),
            score: 75.0,
            commits: 42,
            weighted_commits: 30.0,
            lines_added: 500,
            lines_deleted: 200,
            complexity_density: 0.85,
            fan_in: 10,
            trend: fallow_engine::churn::ChurnTrend::Accelerating,
            median_files_per_commit: None,
            median_lines_per_commit: None,
            ownership: None,
            is_test_path: false,
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("Hotspots (1 file)"));
    assert!(text.contains("75.0"));
    assert!(text.contains("src/core.ts"));
    assert!(text.contains("42 commits"));
    assert!(text.contains("700 churn"));
    assert!(text.contains("0.85 density"));
    assert!(text.contains("10 fan-in"));
    assert!(text.contains("accelerating"));
}

#[test]
fn hotspots_cooling_trend() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.hotspots = vec![
        fallow_output::HotspotEntry {
            path: root.join("src/old.ts"),
            score: 20.0,
            commits: 5,
            weighted_commits: 2.0,
            lines_added: 50,
            lines_deleted: 30,
            complexity_density: 0.3,
            fan_in: 2,
            trend: fallow_engine::churn::ChurnTrend::Cooling,
            median_files_per_commit: None,
            median_lines_per_commit: None,
            ownership: None,
            is_test_path: false,
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("20.0"));
    assert!(text.contains("cooling"));
}

#[test]
fn hotspots_stable_trend() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.hotspots = vec![
        fallow_output::HotspotEntry {
            path: root.join("src/mid.ts"),
            score: 45.0,
            commits: 15,
            weighted_commits: 10.0,
            lines_added: 200,
            lines_deleted: 100,
            complexity_density: 0.5,
            fan_in: 5,
            trend: fallow_engine::churn::ChurnTrend::Stable,
            median_files_per_commit: None,
            median_lines_per_commit: None,
            ownership: None,
            is_test_path: false,
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("45.0"));
    assert!(text.contains("stable"));
}

#[test]
fn hotspots_with_summary_and_since() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.hotspots = vec![
        fallow_output::HotspotEntry {
            path: root.join("src/a.ts"),
            score: 50.0,
            commits: 10,
            weighted_commits: 8.0,
            lines_added: 100,
            lines_deleted: 50,
            complexity_density: 0.4,
            fan_in: 3,
            trend: fallow_engine::churn::ChurnTrend::Stable,
            median_files_per_commit: None,
            median_lines_per_commit: None,
            ownership: None,
            is_test_path: false,
        }
        .into(),
    ];
    report.hotspot_summary = Some(fallow_output::HotspotSummary {
        since: "6 months".to_string(),
        min_commits: 3,
        files_analyzed: 50,
        files_excluded: 20,
        shallow_clone: false,
        clock: None,
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("Hotspots (1 file, since 6 months)"));
    assert!(text.contains("20 files excluded (< 3 commits)"));
}

#[test]
fn hotspots_summary_no_exclusions() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.hotspots = vec![
        fallow_output::HotspotEntry {
            path: root.join("src/a.ts"),
            score: 50.0,
            commits: 10,
            weighted_commits: 8.0,
            lines_added: 100,
            lines_deleted: 50,
            complexity_density: 0.4,
            fan_in: 3,
            trend: fallow_engine::churn::ChurnTrend::Stable,
            median_files_per_commit: None,
            median_lines_per_commit: None,
            ownership: None,
            is_test_path: false,
        }
        .into(),
    ];
    report.hotspot_summary = Some(fallow_output::HotspotSummary {
        since: "3 months".to_string(),
        min_commits: 2,
        files_analyzed: 50,
        files_excluded: 0,
        shallow_clone: false,
        clock: None,
    });
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(!text.contains("files excluded"));
}

#[test]
fn hotspots_docs_link() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.hotspots = vec![
        fallow_output::HotspotEntry {
            path: root.join("src/a.ts"),
            score: 50.0,
            commits: 10,
            weighted_commits: 8.0,
            lines_added: 100,
            lines_deleted: 50,
            complexity_density: 0.4,
            fan_in: 3,
            trend: fallow_engine::churn::ChurnTrend::Stable,
            median_files_per_commit: None,
            median_lines_per_commit: None,
            ownership: None,
            is_test_path: false,
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("fallow.tools/docs/explanations/health/#hotspot-metrics"));
    assert_docs_footer_layout(&lines, &format!("{DOCS_HEALTH}#hotspot-metrics"));
}

#[test]
fn refactoring_targets_single_low_effort() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.targets = vec![
        fallow_output::RefactoringTarget {
            path: root.join("src/legacy.ts"),
            priority: 65.0,
            efficiency: 65.0,
            recommendation: "Extract complex logic into helper functions".to_string(),
            category: fallow_output::RecommendationCategory::ExtractComplexFunctions,
            effort: fallow_output::EffortEstimate::Low,
            confidence: fallow_output::Confidence::High,
            factors: vec![],
            evidence: None,
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("Refactoring targets (1)"));
    assert!(text.contains("1 low effort"));
    assert!(text.contains("65.0"));
    assert!(text.contains("pri:65.0"));
    assert!(text.contains("src/legacy.ts"));
    assert!(text.contains("complexity"));
    assert!(text.contains("effort:low"));
    assert!(text.contains("confidence:high"));
    assert!(text.contains("Extract complex logic into helper functions"));
}

#[test]
fn refactoring_targets_render_non_empty_relation_evidence() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.targets = vec![
        fallow_output::RefactoringTarget {
            path: root.join("src/legacy.ts"),
            priority: 65.0,
            efficiency: 65.0,
            recommendation: "Extract complex logic into helper functions".to_string(),
            category: fallow_output::RecommendationCategory::ExtractComplexFunctions,
            effort: fallow_output::EffortEstimate::Low,
            confidence: fallow_output::Confidence::High,
            factors: vec![],
            evidence: Some(fallow_output::TargetEvidence {
                direct_callers: vec![fallow_output::DirectCallerEvidence {
                    path: root.join("src/consumer.ts"),
                    symbols: vec![
                        fallow_output::DirectCallerSymbolEvidence {
                            imported: "loadLegacy".into(),
                            local: "load".into(),
                            type_only: false,
                        },
                        fallow_output::DirectCallerSymbolEvidence {
                            imported: "side-effect".into(),
                            local: String::new(),
                            type_only: false,
                        },
                    ],
                }],
                clone_siblings: vec![fallow_output::CloneSiblingEvidence {
                    path: root.join("src/peer.ts"),
                    start_line: 12,
                    end_line: 20,
                    fingerprint: "dup:12345678".into(),
                }],
                ..Default::default()
            }),
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("importers: src/consumer.ts (loadLegacy as load, side effect)"));
    assert!(!text.contains("side-effect"));
    assert!(text.contains("clones: src/peer.ts:12-20 dup:12345678"));
}

#[test]
fn refactoring_targets_mixed_effort() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.targets = vec![
        fallow_output::RefactoringTarget {
            path: root.join("src/a.ts"),
            priority: 80.0,
            efficiency: 80.0,
            recommendation: "Remove dead exports".to_string(),
            category: fallow_output::RecommendationCategory::RemoveDeadCode,
            effort: fallow_output::EffortEstimate::Low,
            confidence: fallow_output::Confidence::High,
            factors: vec![],
            evidence: None,
        }
        .into(),
        fallow_output::RefactoringTarget {
            path: root.join("src/b.ts"),
            priority: 60.0,
            efficiency: 30.0,
            recommendation: "Split into smaller modules".to_string(),
            category: fallow_output::RecommendationCategory::SplitHighImpact,
            effort: fallow_output::EffortEstimate::Medium,
            confidence: fallow_output::Confidence::Medium,
            factors: vec![],
            evidence: None,
        }
        .into(),
        fallow_output::RefactoringTarget {
            path: root.join("src/c.ts"),
            priority: 50.0,
            efficiency: 16.7,
            recommendation: "Break circular dependency".to_string(),
            category: fallow_output::RecommendationCategory::BreakCircularDependency,
            effort: fallow_output::EffortEstimate::High,
            confidence: fallow_output::Confidence::Low,
            factors: vec![],
            evidence: None,
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("Refactoring targets (3)"));
    assert!(text.contains("1 low effort"));
    assert!(text.contains("1 medium"));
    assert!(text.contains("1 high"));
    assert!(text.contains("effort:low"));
    assert!(text.contains("effort:medium"));
    assert!(text.contains("effort:high"));
    assert!(text.contains("confidence:high"));
    assert!(text.contains("confidence:medium"));
    assert!(text.contains("confidence:low"));
}

#[test]
fn refactoring_targets_truncation_above_max_flat_items() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    for i in 0..12 {
        report.targets.push(
            fallow_output::RefactoringTarget {
                path: root.join(format!("src/target{i}.ts")),
                priority: 50.0,
                efficiency: 25.0,
                recommendation: format!("Fix target {i}"),
                category: fallow_output::RecommendationCategory::ExtractComplexFunctions,
                effort: fallow_output::EffortEstimate::Medium,
                confidence: fallow_output::Confidence::Medium,
                factors: vec![],
                evidence: None,
            }
            .into(),
        );
    }
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("Refactoring targets (12)"));
    assert!(text.contains("... and 2 more targets"));
    assert!(text.contains("target0.ts"));
    assert!(text.contains("target9.ts"));
    assert!(!text.contains("target10.ts"));
}

#[test]
fn refactoring_targets_docs_link() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.targets = vec![
        fallow_output::RefactoringTarget {
            path: root.join("src/a.ts"),
            priority: 50.0,
            efficiency: 50.0,
            recommendation: "Fix it".to_string(),
            category: fallow_output::RecommendationCategory::ExtractDependencies,
            effort: fallow_output::EffortEstimate::Low,
            confidence: fallow_output::Confidence::High,
            factors: vec![],
            evidence: None,
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("fallow.tools/docs/explanations/health/#refactoring-targets"));
    assert_docs_footer_layout(&lines, &format!("{DOCS_HEALTH}#refactoring-targets"));
}

#[test]
fn refactoring_targets_all_categories() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    let categories = [
        (
            fallow_output::RecommendationCategory::UrgentChurnComplexity,
            "churn+complexity",
        ),
        (
            fallow_output::RecommendationCategory::BreakCircularDependency,
            "circular dependency",
        ),
        (
            fallow_output::RecommendationCategory::SplitHighImpact,
            "high impact",
        ),
        (
            fallow_output::RecommendationCategory::RemoveDeadCode,
            "dead code",
        ),
        (
            fallow_output::RecommendationCategory::ExtractComplexFunctions,
            "complexity",
        ),
        (
            fallow_output::RecommendationCategory::ExtractDependencies,
            "coupling",
        ),
        (
            fallow_output::RecommendationCategory::AddTestCoverage,
            "untested risk",
        ),
    ];
    for (i, (cat, _label)) in categories.iter().enumerate() {
        report.targets.push(
            fallow_output::RefactoringTarget {
                path: root.join(format!("src/cat{i}.ts")),
                priority: 50.0,
                efficiency: 50.0,
                recommendation: format!("Fix cat{i}"),
                category: cat.clone(),
                effort: fallow_output::EffortEstimate::Low,
                confidence: fallow_output::Confidence::High,
                factors: vec![],
                evidence: None,
            }
            .into(),
        );
    }
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    for (_cat, label) in &categories {
        assert!(
            text.contains(label),
            "Expected category label '{label}' in output"
        );
    }
}

#[test]
fn refactoring_targets_efficiency_color_thresholds() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.targets = vec![
        fallow_output::RefactoringTarget {
            path: root.join("src/high.ts"),
            priority: 50.0,
            efficiency: 50.0, // green: >= 40
            recommendation: "High eff".to_string(),
            category: fallow_output::RecommendationCategory::RemoveDeadCode,
            effort: fallow_output::EffortEstimate::Low,
            confidence: fallow_output::Confidence::High,
            factors: vec![],
            evidence: None,
        }
        .into(),
        fallow_output::RefactoringTarget {
            path: root.join("src/mid.ts"),
            priority: 50.0,
            efficiency: 25.0, // yellow: >= 20
            recommendation: "Mid eff".to_string(),
            category: fallow_output::RecommendationCategory::RemoveDeadCode,
            effort: fallow_output::EffortEstimate::Medium,
            confidence: fallow_output::Confidence::Medium,
            factors: vec![],
            evidence: None,
        }
        .into(),
        fallow_output::RefactoringTarget {
            path: root.join("src/low.ts"),
            priority: 50.0,
            efficiency: 10.0, // dimmed: < 20
            recommendation: "Low eff".to_string(),
            category: fallow_output::RecommendationCategory::RemoveDeadCode,
            effort: fallow_output::EffortEstimate::High,
            confidence: fallow_output::Confidence::Low,
            factors: vec![],
            evidence: None,
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("50.0"));
    assert!(text.contains("25.0"));
    assert!(text.contains("10.0"));
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn all_sections_combined() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.summary.functions_above_threshold = 1;
    report.findings = vec![
        fallow_output::ComplexityViolation {
            path: root.join("src/complex.ts"),
            name: "bigFn".to_string(),
            line: 10,
            col: 0,
            cyclomatic: 25,
            cognitive: 20,
            line_count: 80,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: fallow_output::ExceededThreshold::Both,
            effective_severity: None,
            severity: fallow_output::FindingSeverity::Moderate,
            ..Default::default()
        }
        .into(),
    ];
    report.health_score = Some(fallow_output::HealthScore {
        formula_version: fallow_output::HEALTH_SCORE_FORMULA_VERSION,
        score: 75.0,
        grade: "B",
        penalties: fallow_output::HealthScorePenalties {
            dead_files: Some(5.0),
            dead_exports: Some(5.0),
            complexity: 5.0,
            p90_complexity: 2.0,
            maintainability: Some(3.0),
            hotspots: Some(2.0),
            unused_deps: Some(2.0),
            circular_deps: Some(1.0),
            unit_size: None,
            coupling: None,
            duplication: None,
            prop_drilling: None,
        },
    });
    report.file_scores = vec![fallow_output::FileHealthScore {
        path: root.join("src/complex.ts"),
        fan_in: 5,
        fan_out: 3,
        dead_code_ratio: 0.1,
        complexity_density: 0.5,
        maintainability_index: 60.0,
        total_cyclomatic: 15,
        total_cognitive: 10,
        function_count: 3,
        lines: 200,
        crap_max: 0.0,
        crap_above_threshold: 0,
        crap_exempted: 0,
        crap_suppressed: 0,
        crap_unsuppressed_max: None,
        crap_unsuppressed_effective_threshold: None,
        crap_effective_threshold: None,
    }];
    report.hotspots = vec![
        fallow_output::HotspotEntry {
            path: root.join("src/complex.ts"),
            score: 65.0,
            commits: 20,
            weighted_commits: 15.0,
            lines_added: 300,
            lines_deleted: 100,
            complexity_density: 0.5,
            fan_in: 5,
            trend: fallow_engine::churn::ChurnTrend::Accelerating,
            median_files_per_commit: None,
            median_lines_per_commit: None,
            ownership: None,
            is_test_path: false,
        }
        .into(),
    ];
    report.targets = vec![
        fallow_output::RefactoringTarget {
            path: root.join("src/complex.ts"),
            priority: 70.0,
            efficiency: 70.0,
            recommendation: "Extract complex functions".to_string(),
            category: fallow_output::RecommendationCategory::ExtractComplexFunctions,
            effort: fallow_output::EffortEstimate::Low,
            confidence: fallow_output::Confidence::High,
            factors: vec![],
            evidence: None,
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("Health score:"));
    assert!(text.contains("High complexity functions"));
    assert!(text.contains("File health scores"));
    assert!(text.contains("Hotspots"));
    assert!(text.contains("Refactoring targets"));
}

#[test]
fn completely_empty_report_produces_no_lines() {
    let root = PathBuf::from("/project");
    let report = empty_report();
    let lines = build_health_human_lines(&report, &root);
    assert!(lines.is_empty());
}

#[test]
fn finding_only_cyclomatic_exceeds() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.summary.functions_above_threshold = 1;
    report.findings = vec![
        fallow_output::ComplexityViolation {
            path: root.join("src/a.ts"),
            name: "fn1".to_string(),
            line: 1,
            col: 0,
            cyclomatic: 25, // exceeds 20
            cognitive: 10,  // does not exceed 15
            line_count: 50,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: fallow_output::ExceededThreshold::Cyclomatic,
            effective_severity: None,
            severity: fallow_output::FindingSeverity::Moderate,
            ..Default::default()
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("25 ! cyclomatic"));
    assert!(text.contains("10   cognitive"));
}

#[test]
fn finding_only_cognitive_exceeds() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.summary.functions_above_threshold = 1;
    report.findings = vec![
        fallow_output::ComplexityViolation {
            path: root.join("src/a.ts"),
            name: "fn1".to_string(),
            line: 1,
            col: 0,
            cyclomatic: 10, // does not exceed 20
            cognitive: 25,  // exceeds 15
            line_count: 50,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: fallow_output::ExceededThreshold::Cognitive,
            effective_severity: None,
            severity: fallow_output::FindingSeverity::High,
            ..Default::default()
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("10   cyclomatic"));
    assert!(text.contains("25 ! cognitive"));
}

#[test]
fn findings_across_multiple_files() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.summary.functions_above_threshold = 2;
    report.findings = vec![
        fallow_output::ComplexityViolation {
            path: root.join("src/a.ts"),
            name: "fn1".to_string(),
            line: 1,
            col: 0,
            cyclomatic: 25,
            cognitive: 20,
            line_count: 50,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: fallow_output::ExceededThreshold::Both,
            effective_severity: None,
            severity: fallow_output::FindingSeverity::Moderate,
            ..Default::default()
        }
        .into(),
        fallow_output::ComplexityViolation {
            path: root.join("src/b.ts"),
            name: "fn2".to_string(),
            line: 5,
            col: 0,
            cyclomatic: 22,
            cognitive: 18,
            line_count: 40,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: fallow_output::ExceededThreshold::Both,
            effective_severity: None,
            severity: fallow_output::FindingSeverity::Moderate,
            ..Default::default()
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("src/a.ts"));
    assert!(text.contains("src/b.ts"));
}

#[test]
fn findings_docs_link() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.summary.functions_above_threshold = 1;
    report.findings = vec![
        fallow_output::ComplexityViolation {
            path: root.join("src/a.ts"),
            name: "fn1".to_string(),
            line: 1,
            col: 0,
            cyclomatic: 25,
            cognitive: 20,
            line_count: 50,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: fallow_output::ExceededThreshold::Both,
            effective_severity: None,
            severity: fallow_output::FindingSeverity::Moderate,
            ..Default::default()
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("fallow.tools/docs/explanations/health/#complexity-metrics"));
    assert_docs_footer_layout(&lines, &format!("{DOCS_HEALTH}#complexity-metrics"));
}

#[test]
fn hotspot_score_high_medium_low() {
    let root = PathBuf::from("/project");
    let mut report = empty_report();
    report.hotspots = vec![
        fallow_output::HotspotEntry {
            path: root.join("src/high.ts"),
            score: 80.0, // red: >= 70
            commits: 30,
            weighted_commits: 25.0,
            lines_added: 400,
            lines_deleted: 200,
            complexity_density: 0.9,
            fan_in: 8,
            trend: fallow_engine::churn::ChurnTrend::Accelerating,
            median_files_per_commit: None,
            median_lines_per_commit: None,
            ownership: None,
            is_test_path: false,
        }
        .into(),
        fallow_output::HotspotEntry {
            path: root.join("src/medium.ts"),
            score: 45.0, // yellow: >= 30
            commits: 15,
            weighted_commits: 10.0,
            lines_added: 200,
            lines_deleted: 100,
            complexity_density: 0.5,
            fan_in: 4,
            trend: fallow_engine::churn::ChurnTrend::Stable,
            median_files_per_commit: None,
            median_lines_per_commit: None,
            ownership: None,
            is_test_path: false,
        }
        .into(),
        fallow_output::HotspotEntry {
            path: root.join("src/low.ts"),
            score: 15.0, // green: < 30
            commits: 5,
            weighted_commits: 3.0,
            lines_added: 50,
            lines_deleted: 20,
            complexity_density: 0.2,
            fan_in: 1,
            trend: fallow_engine::churn::ChurnTrend::Cooling,
            median_files_per_commit: None,
            median_lines_per_commit: None,
            ownership: None,
            is_test_path: false,
        }
        .into(),
    ];
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(text.contains("80.0"));
    assert!(text.contains("45.0"));
    assert!(text.contains("15.0"));
    assert!(text.contains("Hotspots (3 files)"));
}

#[test]
fn rollup_breakdown_renders_workspace_relative_template_path() {
    let root = PathBuf::from("/project");
    let template =
        root.join("apps/admin/src/app/payments/payment-list/payment-list.component.html");
    let finding = fallow_output::ComplexityViolation {
        path: root.join("apps/admin/src/app/payments/payment-list/payment-list.component.ts"),
        name: "<component>".to_string(),
        line: 1,
        col: 0,
        cyclomatic: 25,
        cognitive: 28,
        line_count: 0,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        exceeded: fallow_output::ExceededThreshold::Both,
        effective_severity: None,
        severity: fallow_output::FindingSeverity::High,
        crap: None,
        mutation: None,
        coverage_pct: None,
        coverage_tier: None,
        coverage_source: None,
        inherited_from: None,
        component_rollup: Some(fallow_output::ComponentRollup {
            component: "PaymentListComponent".to_string(),
            class_worst_function: "ngOnInit".to_string(),
            class_cyclomatic: 12,
            class_cognitive: 16,
            template_path: template,
            template_cyclomatic: 13,
            template_cognitive: 12,
        }),
        ..Default::default()
    };
    let line = render_component_rollup_breakdown(&finding, &root)
        .expect("rollup payload should render a breakdown line");
    assert!(
        line.contains("apps/admin/src/app/payments/payment-list/payment-list.component.html"),
        "breakdown must include workspace-relative template path: {line}"
    );
    assert!(
        !line.contains(" payment-list.component.html"),
        "bare basename token must not be the rendered template: {line}"
    );
}

#[test]
fn inherited_from_renders_workspace_relative_owner_path() {
    let root = PathBuf::from("/project");
    let owner = root.join("apps/admin/src/app/auth/permissions/permissions.component.ts");
    let template_path = root.join("apps/admin/src/app/auth/permissions/permissions.component.html");
    let report = fallow_output::HealthReport {
        findings: vec![
            fallow_output::ComplexityViolation {
                path: template_path,
                name: "<template>".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 12,
                cognitive: 14,
                line_count: 0,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                react_hook_profile: None,
                exceeded: fallow_output::ExceededThreshold::Both,
                effective_severity: None,
                severity: fallow_output::FindingSeverity::High,
                crap: Some(45.0),
                mutation: None,
                coverage_pct: None,
                coverage_tier: Some(fallow_output::CoverageTier::Partial),
                coverage_source: Some(fallow_output::CoverageSource::EstimatedComponentInherited),
                inherited_from: Some(owner),
                ..Default::default()
            }
            .into(),
        ],
        summary: fallow_output::HealthSummary {
            files_analyzed: 1,
            functions_analyzed: 1,
            functions_above_threshold: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let lines = build_health_human_lines(&report, &root);
    let text = plain(&lines);
    assert!(
        text.contains(
            "(inherited from apps/admin/src/app/auth/permissions/permissions.component.ts)"
        ),
        "inherited-from suffix must use workspace-relative path: {text}"
    );
    assert!(
        !text.contains("(inherited from permissions.component.ts)"),
        "bare basename suffix must not be rendered: {text}"
    );
}

fn react_finding(
    react_hook_count: u16,
    react_prop_count: u16,
    react_jsx_max_depth: u16,
    profile: Option<fallow_output::ReactHookProfile>,
) -> fallow_output::ComplexityViolation {
    fallow_output::ComplexityViolation {
        path: PathBuf::from("src/Dashboard.tsx"),
        name: "Dashboard".to_string(),
        line: 1,
        col: 0,
        cyclomatic: 40,
        cognitive: 30,
        line_count: 90,
        param_count: 1,
        react_hook_count,
        react_jsx_max_depth,
        react_prop_count,
        react_hook_profile: profile,
        exceeded: fallow_output::ExceededThreshold::Both,
        effective_severity: None,
        severity: fallow_output::FindingSeverity::High,
        ..Default::default()
    }
}

#[test]
fn react_context_renders_hook_breakdown_and_max_effect_deps() {
    let finding = react_finding(
        9,
        14,
        7,
        Some(fallow_output::ReactHookProfile {
            state: 3,
            effect: 4,
            memo: 2,
            callback: 0,
            custom: 0,
            max_effect_dep_arity: Some(5),
        }),
    );
    let line = render_react_context(&finding).expect("react context line");
    let plain_line = strip_ansi(&line);
    assert!(
        plain_line.contains(
            "react: 14 props, 9 hooks (3 state, 4 effect, 2 memo), max effect deps 5, JSX depth 7"
        ),
        "breakdown line: {plain_line}"
    );
}

#[test]
fn react_context_without_profile_keeps_bare_hook_count() {
    let finding = react_finding(5, 0, 0, None);
    let line = render_react_context(&finding).expect("react context line");
    let plain_line = strip_ansi(&line);
    assert!(plain_line.contains("react: 5 hooks"), "{plain_line}");
    assert!(
        !plain_line.contains('('),
        "no breakdown parenthetical without a profile: {plain_line}"
    );
    assert!(
        !plain_line.contains("max effect deps"),
        "no effect-deps segment without a profile: {plain_line}"
    );
}

#[test]
fn react_context_omits_max_effect_deps_when_arity_absent() {
    let finding = react_finding(
        2,
        0,
        0,
        Some(fallow_output::ReactHookProfile {
            state: 1,
            effect: 1,
            memo: 0,
            callback: 0,
            custom: 0,
            max_effect_dep_arity: None,
        }),
    );
    let line = render_react_context(&finding).expect("react context line");
    let plain_line = strip_ansi(&line);
    assert!(
        plain_line.contains("2 hooks (1 state, 1 effect)"),
        "{plain_line}"
    );
    assert!(
        !plain_line.contains("max effect deps"),
        "absent arity must omit the effect-deps segment: {plain_line}"
    );
}

#[test]
fn react_context_uses_singular_for_one_prop_and_one_hook() {
    let finding = react_finding(1, 1, 0, None);
    let line = render_react_context(&finding).expect("react context line");
    assert!(
        strip_ansi(&line).contains("react: 1 prop, 1 hook"),
        "{line}"
    );

    let finding = react_finding(
        1,
        0,
        0,
        Some(fallow_output::ReactHookProfile {
            state: 1,
            effect: 0,
            memo: 0,
            callback: 0,
            custom: 0,
            max_effect_dep_arity: None,
        }),
    );
    let line = render_react_context(&finding).expect("react context line");
    assert!(
        strip_ansi(&line).contains("react: 1 hook (1 state)"),
        "{line}"
    );
}

#[test]
fn react_context_none_for_non_react_finding() {
    let finding = react_finding(0, 0, 0, None);
    assert!(render_react_context(&finding).is_none());
}
