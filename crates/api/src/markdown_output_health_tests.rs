use std::path::Path;

use fallow_output::{HealthReport, StylingFinding, StylingFindingSeverity};

use super::build_health_markdown;

fn one_file_report(root: &Path, since: Option<&str>) -> HealthReport {
    HealthReport {
        file_scores: vec![fallow_output::FileHealthScore {
            path: root.join("src/core.ts"),
            fan_in: 1,
            fan_out: 0,
            dead_code_ratio: 0.0,
            complexity_density: 0.5,
            maintainability_index: 80.0,
            total_cyclomatic: 4,
            total_cognitive: 2,
            function_count: 1,
            lines: 10,
            crap_max: 4.0,
            crap_above_threshold: 0,
            crap_exempted: 0,
            crap_suppressed: 0,
            crap_unsuppressed_max: None,
            crap_unsuppressed_effective_threshold: None,
            crap_effective_threshold: None,
        }],
        hotspots: vec![
            fallow_output::HotspotEntry {
                path: root.join("src/core.ts"),
                score: 75.0,
                commits: 4,
                weighted_commits: 3.0,
                lines_added: 10,
                lines_deleted: 2,
                complexity_density: 0.5,
                fan_in: 1,
                trend: fallow_types::churn::ChurnTrend::Stable,
                median_files_per_commit: None,
                median_lines_per_commit: None,
                ownership: None,
                is_test_path: false,
            }
            .into(),
        ],
        hotspot_summary: since.map(|since| fallow_output::HotspotSummary {
            since: since.to_string(),
            min_commits: 3,
            files_analyzed: 1,
            files_excluded: 0,
            shallow_clone: false,
            clock: None,
        }),
        ..HealthReport::default()
    }
}

/// A score-only report has no finding list, but its summary still counts
/// the functions above a threshold. The section must give that count.
#[test]
fn score_only_markdown_counts_unlisted_functions() {
    let root = Path::new("/project");
    let mut report = HealthReport::default();
    report.summary.functions_analyzed = 5;
    report.summary.functions_above_threshold = 1;
    let output = build_health_markdown(&report, root);
    assert!(
        output.contains("## Fallow: 1 function exceeds complexity thresholds"),
        "{output}"
    );
    assert!(output.contains("`fallow health --complexity`"), "{output}");
    assert!(!output.contains("baseline"), "{output}");

    report.summary.functions_above_threshold = 0;
    let output = build_health_markdown(&report, root);
    assert!(
        output.contains("## Fallow: no functions exceed complexity thresholds"),
        "{output}"
    );
}

fn accepting_baseline() -> fallow_output::BaselineStaleness {
    fallow_output::BaselineStaleness {
        baseline_entries: 1,
        matched_entries: 1,
        stale_entries: 0,
        current_findings: 1,
        remaining_findings: None,
        change_scoped: false,
        stale: false,
        warning: fallow_output::BaselineStalenessAdvisory::None,
        gate_trips: false,
        moved_entries: 0,
        unrecognised_format: false,
        saved_by: None,
        format: None,
        scope_reasons: fallow_output::BaselineScopeReasons::empty(),
    }
}

/// A score-only run does not list findings, so a baseline does not make
/// it clean. The count comes before the baseline, and the section says so.
#[test]
fn baselined_score_only_markdown_counts_unlisted_functions() {
    let root = Path::new("/project");
    let mut report = HealthReport::default();
    report.summary.functions_analyzed = 5;
    report.summary.functions_above_threshold = 1;
    report.summary.baseline_staleness = Some(accepting_baseline());
    let output = build_health_markdown(&report, root);
    assert!(
        output.contains("## Fallow: 1 function exceeds complexity thresholds"),
        "{output}"
    );
    assert!(
        output.contains("The count includes the functions that the baseline accepts."),
        "{output}"
    );
    assert!(
        output.contains("Run `fallow health --complexity` with the same `--baseline`"),
        "{output}"
    );
    assert!(!output.contains("no functions exceed"), "{output}");
}

/// A run that lists findings and has a baseline that accepts every
/// finding is clean. The section must not give the count or ask for a
/// rerun.
#[test]
fn baselined_listed_markdown_stays_clean() {
    let root = Path::new("/project");
    let mut report = HealthReport {
        vital_signs: Some(fallow_output::VitalSigns::default()),
        ..HealthReport::default()
    };
    report.summary.functions_analyzed = 5;
    report.summary.functions_above_threshold = 1;
    report.summary.baseline_staleness = Some(accepting_baseline());
    let output = build_health_markdown(&report, root);
    assert!(!output.contains("exceeds complexity"), "{output}");
    assert!(!output.contains("--complexity"), "{output}");
}

/// A score-only report with `sections` and `remaining_findings` gives the
/// number of accepted and new functions instead of the older note.
#[test]
fn score_only_markdown_names_the_new_functions() {
    let root = Path::new("/project");
    let mut report = HealthReport {
        sections: Some(vec![fallow_output::HealthSection::Score]),
        ..HealthReport::default()
    };
    report.summary.functions_analyzed = 5;
    report.summary.functions_above_threshold = 3;
    report.summary.baseline_staleness = Some(fallow_output::BaselineStaleness {
        remaining_findings: Some(1),
        ..accepting_baseline()
    });
    let output = build_health_markdown(&report, root);
    assert!(
        output.contains("## Fallow: 3 functions exceed complexity thresholds"),
        "{output}"
    );
    assert!(
        output.contains("The baseline accepts 2. 1 is new."),
        "{output}"
    );
    assert!(!output.contains("The count includes"), "{output}");
}

/// A report whose `sections` names `complexity` and has an empty list is
/// clean, also without `vital_signs`.
#[test]
fn listed_markdown_with_sections_stays_clean() {
    let root = Path::new("/project");
    let mut report = HealthReport {
        sections: Some(vec![fallow_output::HealthSection::Complexity]),
        ..HealthReport::default()
    };
    report.summary.functions_above_threshold = 1;
    report.summary.baseline_staleness = Some(accepting_baseline());
    let output = build_health_markdown(&report, root);
    assert!(
        output.contains("## Fallow: no functions exceed complexity thresholds"),
        "{output}"
    );
    assert!(!output.contains("--complexity"), "{output}");
}

#[test]
fn health_markdown_headers_use_the_singular_for_one_file() {
    let root = Path::new("/project");

    let output = build_health_markdown(&one_file_report(root, None), root);
    assert!(
        output.contains("### File Health Scores (1 file)"),
        "{output}"
    );
    assert!(output.contains("### Hotspots (1 file)"), "{output}");

    let output = build_health_markdown(&one_file_report(root, Some("6 months")), root);
    assert!(
        output.contains("### Hotspots (1 file, since 6 months)"),
        "{output}"
    );
}

fn broken_reference_report(value: &str) -> HealthReport {
    HealthReport {
        styling_findings: vec![StylingFinding {
            internal: fallow_output::StylingFindingInternal::default(),
            code: "css-broken-reference".to_string(),
            sub_kind: "unresolved-class-reference".to_string(),
            path: "src/app.css".to_string(),
            line: 9,
            value: value.to_string(),
            effective_severity: StylingFindingSeverity::Warn,
            blast_radius: None,
            confidence: None,
            agent_disposition: None,
            nearest_token: None,
            fix_hint: None,
            actions: Vec::new(),
        }],
        ..HealthReport::default()
    }
}

#[test]
fn health_markdown_includes_styling_findings() {
    let report = broken_reference_report("btn-prmary | btn-primary");

    let output = build_health_markdown(&report, Path::new("/project"));

    assert!(output.contains("## Styling Findings"));
    assert!(output.contains("css-broken-reference"));
    assert!(output.contains("btn-prmary \\| btn-primary"));
}

#[test]
fn health_markdown_fences_untrusted_styling_values() {
    let report = broken_reference_report("btn` **injected** | btn``primary");

    let output = build_health_markdown(&report, Path::new("/project"));

    assert!(output.contains("```btn` **injected** \\| btn``primary```"));
}

#[test]
fn health_markdown_escapes_pipes_in_target_recommendation_cell() {
    use fallow_output::{
        Confidence, EffortEstimate, RecommendationCategory, RefactoringTarget,
        RefactoringTargetFinding,
    };

    let report = HealthReport {
        targets: vec![RefactoringTargetFinding {
            target: RefactoringTarget {
                path: "/project/src/big.ts".into(),
                priority: 80.0,
                efficiency: 4.0,
                recommendation: "Extract render|inject (cognitive: 30) into smaller functions"
                    .to_string(),
                category: RecommendationCategory::ExtractComplexFunctions,
                effort: EffortEstimate::Medium,
                confidence: Confidence::Medium,
                factors: Vec::new(),
                evidence: None,
            },
            actions: Vec::new(),
        }],
        ..HealthReport::default()
    };

    let output = build_health_markdown(&report, Path::new("/project"));

    assert!(output.contains("Extract render\\|inject (cognitive: 30)"));
    assert!(!output.contains("Extract render|inject"));
}
