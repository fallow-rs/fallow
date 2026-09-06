//! Shared health CodeClimate issue construction.

use std::path::Path;

use fallow_output::{
    CodeClimateIssue, CodeClimateIssueInput, CodeClimateSeverity, ComplexityViolation,
    CoverageIntelligenceFinding, CoverageIntelligenceRecommendation, CoverageIntelligenceVerdict,
    ExceededThreshold, FindingSeverity, HealthReport, HealthSummary, RuntimeCoverageFinding,
    RuntimeCoverageVerdict, StylingFinding, StylingFindingSeverity, UntestedExportFinding,
    UntestedFileFinding, build_codeclimate_issue, codeclimate_fingerprint_hash, normalize_uri,
};

struct HealthCodeClimateContext<'a> {
    root: &'a Path,
    /// Held instead of the three run-global ceilings so every description
    /// resolves per finding: an override-affected finding must never be
    /// described against a ceiling it was not measured with (issue #2163).
    summary: &'a HealthSummary,
}

impl HealthCodeClimateContext<'_> {
    fn complexity_issue(&self, finding: &ComplexityViolation) -> CodeClimateIssue {
        let path = codeclimate_path(&finding.path, self.root);
        let check_name = complexity_check_name(finding);
        let line_str = finding.line.to_string();
        let fp = codeclimate_fingerprint_hash(&[check_name, &path, &line_str, &finding.name]);
        build_codeclimate_issue(CodeClimateIssueInput {
            check_name,
            description: &self.complexity_description(finding),
            severity: health_finding_severity(finding.severity),
            category: "Complexity",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        })
    }

    fn styling_issue(&self, finding: &StylingFinding) -> CodeClimateIssue {
        let path = codeclimate_path(Path::new(&finding.path), self.root);
        let check_name = format!("fallow/{}", finding.code);
        let description = format!("[{}] {}: {}", finding.code, finding.sub_kind, finding.value);
        let line_str = finding.line.to_string();
        let fp = codeclimate_fingerprint_hash(&[
            &check_name,
            &path,
            &line_str,
            &finding.sub_kind,
            &finding.value,
        ]);
        build_codeclimate_issue(CodeClimateIssueInput {
            check_name: &check_name,
            description: &description,
            severity: styling_finding_severity(finding.effective_severity),
            category: "Style",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        })
    }

    fn complexity_description(&self, finding: &ComplexityViolation) -> String {
        let thresholds = finding.resolved_thresholds(self.summary);
        match finding.exceeded {
            ExceededThreshold::Both => format!(
                "'{}' has cyclomatic complexity {} (threshold: {}) and cognitive complexity {} (threshold: {})",
                finding.name,
                finding.cyclomatic,
                thresholds.max_cyclomatic,
                finding.cognitive,
                thresholds.max_cognitive
            ),
            ExceededThreshold::Cyclomatic => format!(
                "'{}' has cyclomatic complexity {} (threshold: {})",
                finding.name, finding.cyclomatic, thresholds.max_cyclomatic
            ),
            ExceededThreshold::Cognitive => format!(
                "'{}' has cognitive complexity {} (threshold: {})",
                finding.name, finding.cognitive, thresholds.max_cognitive
            ),
            ExceededThreshold::Crap
            | ExceededThreshold::CyclomaticCrap
            | ExceededThreshold::CognitiveCrap
            | ExceededThreshold::All => {
                let crap = finding.crap.unwrap_or(0.0);
                let coverage = finding
                    .coverage_pct
                    .map(|pct| format!(", coverage {pct:.0}%"))
                    .unwrap_or_default();
                format!(
                    "'{}' has CRAP score {crap:.1} (threshold: {:.1}, cyclomatic {}{coverage})",
                    finding.name, thresholds.max_crap, finding.cyclomatic,
                )
            }
        }
    }

    fn runtime_coverage_issue(&self, finding: &RuntimeCoverageFinding) -> CodeClimateIssue {
        let path = codeclimate_path(&finding.path, self.root);
        let check_name = runtime_coverage_check_name(finding.verdict);
        let invocations_hint = finding.invocations.map_or_else(
            || "untracked".to_owned(),
            |hits| format!("{hits} invocations"),
        );
        let description = format!(
            "'{}' runtime coverage verdict: {} ({})",
            finding.function,
            finding.verdict.human_label(),
            invocations_hint,
        );
        let fp = codeclimate_fingerprint_hash(&[
            check_name,
            &path,
            &finding.line.to_string(),
            &finding.function,
        ]);
        build_codeclimate_issue(CodeClimateIssueInput {
            check_name,
            description: &description,
            severity: runtime_coverage_severity(finding.verdict),
            category: "Bug Risk",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        })
    }

    fn coverage_intelligence_issue(
        &self,
        finding: &CoverageIntelligenceFinding,
    ) -> Option<CodeClimateIssue> {
        let severity = coverage_intelligence_severity(finding.verdict)?;
        let path = codeclimate_path(&finding.path, self.root);
        let check_name = coverage_intelligence_check_name(finding.recommendation);
        let identity = finding.identity.as_deref().unwrap_or("code");
        let description = format!(
            "'{}' coverage intelligence verdict: {} ({})",
            identity, finding.verdict, finding.recommendation,
        );
        let fp = codeclimate_fingerprint_hash(&[
            check_name,
            &path,
            &finding.line.to_string(),
            identity,
            &finding.id,
        ]);
        Some(build_codeclimate_issue(CodeClimateIssueInput {
            check_name,
            description: &description,
            severity,
            category: "Bug Risk",
            path: &path,
            begin_line: Some(finding.line),
            fingerprint: &fp,
        }))
    }

    fn untested_file_issue(&self, item: &UntestedFileFinding) -> CodeClimateIssue {
        let path = codeclimate_path(&item.file.path, self.root);
        let description = format!(
            "File is runtime-reachable but has no test dependency path ({} value export{})",
            item.file.value_export_count,
            if item.file.value_export_count == 1 {
                ""
            } else {
                "s"
            },
        );
        let fp = codeclimate_fingerprint_hash(&["fallow/untested-file", &path]);
        build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/untested-file",
            description: &description,
            severity: CodeClimateSeverity::Minor,
            category: "Coverage",
            path: &path,
            begin_line: None,
            fingerprint: &fp,
        })
    }

    fn untested_export_issue(&self, item: &UntestedExportFinding) -> CodeClimateIssue {
        let path = codeclimate_path(&item.export.path, self.root);
        let description = format!(
            "Export '{}' is runtime-reachable but never referenced by test-reachable modules",
            item.export.export_name
        );
        let line_str = item.export.line.to_string();
        let fp = codeclimate_fingerprint_hash(&[
            "fallow/untested-export",
            &path,
            &line_str,
            &item.export.export_name,
        ]);
        build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/untested-export",
            description: &description,
            severity: CodeClimateSeverity::Minor,
            category: "Coverage",
            path: &path,
            begin_line: Some(item.export.line),
            fingerprint: &fp,
        })
    }
}

/// Build CodeClimate issues from health / complexity analysis results.
#[must_use]
pub fn build_health_codeclimate(report: &HealthReport, root: &Path) -> Vec<CodeClimateIssue> {
    let mut issues = Vec::new();
    let ctx = HealthCodeClimateContext {
        root,
        summary: &report.summary,
    };

    for finding in &report.findings {
        issues.push(ctx.complexity_issue(finding));
    }
    for finding in &report.styling_findings {
        issues.push(ctx.styling_issue(finding));
    }

    if let Some(ref production) = report.runtime_coverage {
        for finding in &production.findings {
            issues.push(ctx.runtime_coverage_issue(finding));
        }
    }

    if let Some(ref intelligence) = report.coverage_intelligence {
        for finding in &intelligence.findings {
            if let Some(issue) = ctx.coverage_intelligence_issue(finding) {
                issues.push(issue);
            }
        }
    }

    if let Some(ref gaps) = report.coverage_gaps {
        for item in &gaps.files {
            issues.push(ctx.untested_file_issue(item));
        }

        for item in &gaps.exports {
            issues.push(ctx.untested_export_issue(item));
        }
    }

    issues
}

fn codeclimate_path(path: &Path, root: &Path) -> String {
    normalize_uri(
        &path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string(),
    )
}

const fn coverage_intelligence_check_name(
    recommendation: CoverageIntelligenceRecommendation,
) -> &'static str {
    match recommendation {
        CoverageIntelligenceRecommendation::AddTestOrSplitBeforeMerge => {
            "fallow/coverage-intelligence-risky-change"
        }
        CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner => {
            "fallow/coverage-intelligence-delete"
        }
        CoverageIntelligenceRecommendation::ReviewBeforeChanging => {
            "fallow/coverage-intelligence-review"
        }
        CoverageIntelligenceRecommendation::RefactorCarefullyKeepBehavior => {
            "fallow/coverage-intelligence-refactor"
        }
    }
}

const fn complexity_check_name(finding: &ComplexityViolation) -> &'static str {
    match finding.exceeded {
        ExceededThreshold::Both => "fallow/high-complexity",
        ExceededThreshold::Cyclomatic => "fallow/high-cyclomatic-complexity",
        ExceededThreshold::Cognitive => "fallow/high-cognitive-complexity",
        ExceededThreshold::Crap
        | ExceededThreshold::CyclomaticCrap
        | ExceededThreshold::CognitiveCrap
        | ExceededThreshold::All => "fallow/high-crap-score",
    }
}

const fn health_finding_severity(severity: FindingSeverity) -> CodeClimateSeverity {
    match severity {
        FindingSeverity::Critical => CodeClimateSeverity::Critical,
        FindingSeverity::High => CodeClimateSeverity::Major,
        FindingSeverity::Moderate => CodeClimateSeverity::Minor,
    }
}

const fn styling_finding_severity(severity: StylingFindingSeverity) -> CodeClimateSeverity {
    match severity {
        StylingFindingSeverity::Error => CodeClimateSeverity::Major,
        StylingFindingSeverity::Warn => CodeClimateSeverity::Minor,
    }
}

const fn runtime_coverage_check_name(verdict: RuntimeCoverageVerdict) -> &'static str {
    match verdict {
        RuntimeCoverageVerdict::SafeToDelete => "fallow/runtime-safe-to-delete",
        RuntimeCoverageVerdict::ReviewRequired => "fallow/runtime-review-required",
        RuntimeCoverageVerdict::LowTraffic => "fallow/runtime-low-traffic",
        RuntimeCoverageVerdict::CoverageUnavailable => "fallow/runtime-coverage-unavailable",
        RuntimeCoverageVerdict::Active | RuntimeCoverageVerdict::Unknown => {
            "fallow/runtime-coverage"
        }
    }
}

const fn runtime_coverage_severity(verdict: RuntimeCoverageVerdict) -> CodeClimateSeverity {
    match verdict {
        RuntimeCoverageVerdict::SafeToDelete => CodeClimateSeverity::Critical,
        RuntimeCoverageVerdict::ReviewRequired => CodeClimateSeverity::Major,
        _ => CodeClimateSeverity::Minor,
    }
}

const fn coverage_intelligence_severity(
    verdict: CoverageIntelligenceVerdict,
) -> Option<CodeClimateSeverity> {
    match verdict {
        CoverageIntelligenceVerdict::RiskyChangeDetected
        | CoverageIntelligenceVerdict::HighConfidenceDelete => Some(CodeClimateSeverity::Major),
        CoverageIntelligenceVerdict::ReviewRequired
        | CoverageIntelligenceVerdict::RefactorCarefully => Some(CodeClimateSeverity::Minor),
        CoverageIntelligenceVerdict::Clean | CoverageIntelligenceVerdict::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_output::{
        ComplexityViolation, ExceededThreshold, FindingSeverity, HealthReport, HealthSummary,
        StylingFinding, StylingFindingSeverity,
    };

    use super::*;

    #[test]
    fn health_codeclimate_uses_relative_normalized_paths() {
        let report = HealthReport {
            summary: HealthSummary {
                max_cyclomatic_threshold: 10,
                max_cognitive_threshold: 8,
                max_crap_threshold: 30.0,
                ..HealthSummary::default()
            },
            findings: vec![
                ComplexityViolation {
                    path: PathBuf::from("/root/app/[id]/page.tsx"),
                    name: "render".to_string(),
                    line: 7,
                    col: 0,
                    cyclomatic: 12,
                    cognitive: 9,
                    line_count: 20,
                    param_count: 1,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
                    exceeded: ExceededThreshold::Both,
                    severity: FindingSeverity::High,
                    coverage_pct: None,
                    crap: None,
                    coverage_tier: None,
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
            ..HealthReport::default()
        };

        let issues = build_health_codeclimate(&report, Path::new("/root"));

        assert_eq!(issues.len(), 1);
        let issue = &issues[0];
        assert_eq!(issue.check_name, "fallow/high-complexity");
        assert_eq!(issue.location.path, "app/%5Bid%5D/page.tsx");
        assert_eq!(issue.location.lines.begin, 7);
        assert_eq!(issue.severity, CodeClimateSeverity::Major);
    }

    #[test]
    fn health_codeclimate_includes_styling_findings() {
        let report = HealthReport {
            styling_findings: vec![StylingFinding {
                code: "css-selector-complexity".to_string(),
                sub_kind: "high-specificity".to_string(),
                path: "src/styles.css".to_string(),
                line: 4,
                value: "#app .card .title".to_string(),
                effective_severity: StylingFindingSeverity::Error,
                blast_radius: None,
                confidence: None,
                agent_disposition: None,
                nearest_token: None,
                fix_hint: None,
                actions: Vec::new(),
            }],
            ..HealthReport::default()
        };

        let issues = build_health_codeclimate(&report, Path::new("/root"));

        assert_eq!(issues.len(), 1);
        let issue = &issues[0];
        assert_eq!(issue.check_name, "fallow/css-selector-complexity");
        assert_eq!(issue.location.path, "src/styles.css");
        assert_eq!(issue.location.lines.begin, 4);
        assert_eq!(issue.severity, CodeClimateSeverity::Major);
    }

    fn crap_violation(
        effective_thresholds: Option<fallow_output::HealthEffectiveThresholds>,
    ) -> ComplexityViolation {
        ComplexityViolation {
            path: PathBuf::from("/root/src/Board.astro"),
            name: "<template>".to_string(),
            line: 6,
            col: 3,
            cyclomatic: 11,
            cognitive: 4,
            line_count: 20,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: ExceededThreshold::Crap,
            severity: FindingSeverity::Critical,
            coverage_pct: None,
            crap: Some(132.0),
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            threshold_source: effective_thresholds
                .map(|_| fallow_output::ThresholdSource::Override),
            effective_thresholds,
        }
    }

    fn crap_report(
        effective_thresholds: Option<fallow_output::HealthEffectiveThresholds>,
    ) -> HealthReport {
        HealthReport {
            summary: HealthSummary {
                max_crap_threshold: 30.0,
                ..HealthSummary::default()
            },
            findings: vec![crap_violation(effective_thresholds).into()],
            ..HealthReport::default()
        }
    }

    /// The CodeClimate description feeds `pr-comment-*` and `review-*` too, so
    /// it must quote the ceiling the finding was measured with, matching SARIF
    /// (issue #2163).
    #[test]
    fn codeclimate_description_uses_the_override_ceiling_not_the_global_one() {
        let report = crap_report(Some(fallow_output::HealthEffectiveThresholds {
            max_cyclomatic: 20,
            max_cognitive: 15,
            max_crap: 100.0,
            max_unit_size: 60,
        }));

        let issues = build_health_codeclimate(&report, Path::new("/root"));

        assert_eq!(issues.len(), 1);
        assert!(
            issues[0].description.contains("threshold: 100.0"),
            "{}",
            issues[0].description
        );
        assert!(
            !issues[0].description.contains("threshold: 30.0"),
            "{}",
            issues[0].description
        );
    }

    #[test]
    fn codeclimate_description_falls_back_to_the_global_ceiling() {
        let issues = build_health_codeclimate(&crap_report(None), Path::new("/root"));

        assert_eq!(issues.len(), 1);
        assert!(
            issues[0].description.contains("threshold: 30.0"),
            "{}",
            issues[0].description
        );
    }

    #[test]
    fn health_severities_reach_serialized_codeclimate_issues() {
        for (severity, expected) in [
            (FindingSeverity::Critical, "critical"),
            (FindingSeverity::High, "major"),
            (FindingSeverity::Moderate, "minor"),
        ] {
            let mut violation = crap_violation(None);
            violation.severity = severity;
            let report = HealthReport {
                findings: vec![violation.into()],
                ..HealthReport::default()
            };
            let issues = fallow_output::codeclimate_issues_to_value(&build_health_codeclimate(
                &report,
                Path::new("/root"),
            ));
            let issues = issues
                .as_array()
                .expect("CodeClimate output must be an array");
            assert_eq!(issues.len(), 1);
            assert_eq!(issues[0]["severity"], expected);
            assert_eq!(issues[0]["check_name"], "fallow/high-crap-score");
        }
    }

    #[test]
    fn runtime_verdicts_reach_serialized_codeclimate_issues() {
        use fallow_output::{
            RuntimeCoverageConfidence, RuntimeCoverageEvidence, RuntimeCoverageReport,
        };

        for (verdict, check_name, severity) in [
            (
                RuntimeCoverageVerdict::SafeToDelete,
                "fallow/runtime-safe-to-delete",
                "critical",
            ),
            (
                RuntimeCoverageVerdict::ReviewRequired,
                "fallow/runtime-review-required",
                "major",
            ),
            (
                RuntimeCoverageVerdict::LowTraffic,
                "fallow/runtime-low-traffic",
                "minor",
            ),
            (
                RuntimeCoverageVerdict::CoverageUnavailable,
                "fallow/runtime-coverage-unavailable",
                "minor",
            ),
            (
                RuntimeCoverageVerdict::Active,
                "fallow/runtime-coverage",
                "minor",
            ),
            (
                RuntimeCoverageVerdict::Unknown,
                "fallow/runtime-coverage",
                "minor",
            ),
        ] {
            let report = HealthReport {
                runtime_coverage: Some(RuntimeCoverageReport {
                    findings: vec![RuntimeCoverageFinding {
                        id: "runtime-fixture".to_string(),
                        stable_id: None,
                        source_hash: None,
                        path: PathBuf::from("/root/src/legacy.ts"),
                        function: "legacyHelper".to_string(),
                        line: 12,
                        verdict,
                        invocations: Some(0),
                        confidence: RuntimeCoverageConfidence::High,
                        evidence: RuntimeCoverageEvidence {
                            static_status: "unused".to_string(),
                            test_coverage: "not_covered".to_string(),
                            v8_tracking: "tracked".to_string(),
                            untracked_reason: None,
                            observation_days: 30,
                            deployments_observed: 3,
                        },
                        actions: Vec::new(),
                        discriminators: None,
                    }],
                    ..RuntimeCoverageReport::default()
                }),
                ..HealthReport::default()
            };
            let issues = fallow_output::codeclimate_issues_to_value(&build_health_codeclimate(
                &report,
                Path::new("/root"),
            ));
            let issues = issues
                .as_array()
                .expect("CodeClimate output must be an array");
            assert_eq!(issues.len(), 1, "{verdict:?}");
            assert_eq!(issues[0]["check_name"], check_name);
            assert_eq!(issues[0]["severity"], severity);
            assert_eq!(issues[0]["location"]["path"], "src/legacy.ts");
            assert_eq!(issues[0]["location"]["lines"]["begin"], 12);
        }
    }

    #[test]
    fn coverage_intelligence_verdicts_and_recommendations_reach_serialized_issues() {
        use fallow_output::{
            CoverageIntelligenceConfidence, CoverageIntelligenceEvidence,
            CoverageIntelligenceReport,
        };

        let recommendations = [
            (
                CoverageIntelligenceRecommendation::AddTestOrSplitBeforeMerge,
                "fallow/coverage-intelligence-risky-change",
            ),
            (
                CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner,
                "fallow/coverage-intelligence-delete",
            ),
            (
                CoverageIntelligenceRecommendation::ReviewBeforeChanging,
                "fallow/coverage-intelligence-review",
            ),
            (
                CoverageIntelligenceRecommendation::RefactorCarefullyKeepBehavior,
                "fallow/coverage-intelligence-refactor",
            ),
        ];
        for (verdict, severity) in [
            (
                CoverageIntelligenceVerdict::RiskyChangeDetected,
                Some("major"),
            ),
            (
                CoverageIntelligenceVerdict::HighConfidenceDelete,
                Some("major"),
            ),
            (CoverageIntelligenceVerdict::ReviewRequired, Some("minor")),
            (
                CoverageIntelligenceVerdict::RefactorCarefully,
                Some("minor"),
            ),
            (CoverageIntelligenceVerdict::Clean, None),
            (CoverageIntelligenceVerdict::Unknown, None),
        ] {
            for (recommendation, check_name) in recommendations {
                let report = HealthReport {
                    coverage_intelligence: Some(CoverageIntelligenceReport {
                        schema_version: fallow_output::CoverageIntelligenceSchemaVersion::default(),
                        verdict,
                        summary: fallow_output::CoverageIntelligenceSummary::default(),
                        findings: vec![CoverageIntelligenceFinding {
                            id: "coverage-fixture".to_string(),
                            path: PathBuf::from("/root/src/legacy.ts"),
                            identity: Some("legacyHelper".to_string()),
                            line: 12,
                            verdict,
                            signals: Vec::new(),
                            recommendation,
                            confidence: CoverageIntelligenceConfidence::High,
                            related_ids: Vec::new(),
                            evidence: CoverageIntelligenceEvidence::default(),
                            actions: Vec::new(),
                        }],
                    }),
                    ..HealthReport::default()
                };
                let issues = fallow_output::codeclimate_issues_to_value(&build_health_codeclimate(
                    &report,
                    Path::new("/root"),
                ));
                let issues = issues
                    .as_array()
                    .expect("CodeClimate output must be an array");
                if let Some(severity) = severity {
                    assert_eq!(issues.len(), 1, "{verdict:?}");
                    assert_eq!(issues[0]["check_name"], check_name);
                    assert_eq!(issues[0]["severity"], severity);
                    assert_eq!(issues[0]["location"]["path"], "src/legacy.ts");
                } else {
                    assert!(issues.is_empty(), "{verdict:?}");
                }
            }
        }
    }
}
