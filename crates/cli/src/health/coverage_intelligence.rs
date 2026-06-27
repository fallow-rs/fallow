use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use xxhash_rust::xxh3::xxh3_64;

use crate::health_types::{
    CoverageGaps, CoverageIntelligenceAction, CoverageIntelligenceConfidence,
    CoverageIntelligenceEvidence, CoverageIntelligenceFinding, CoverageIntelligenceMatchConfidence,
    CoverageIntelligenceRecommendation, CoverageIntelligenceReport,
    CoverageIntelligenceSchemaVersion, CoverageIntelligenceSignal, CoverageIntelligenceSummary,
    CoverageIntelligenceVerdict, CoverageTier, HealthFinding, HealthReport, HotspotFinding,
    OwnershipState, RuntimeCoverageFinding, RuntimeCoverageReport, RuntimeCoverageVerdict,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct CoverageIntelligenceContext {
    pub has_change_scope: bool,
}

#[must_use]
pub(super) fn build_coverage_intelligence(
    report: &HealthReport,
    root: &Path,
    ctx: CoverageIntelligenceContext,
) -> Option<CoverageIntelligenceReport> {
    let runtime = report.runtime_coverage.as_ref()?;
    let mut skipped_ambiguous_matches = 0;
    let mut findings = Vec::new();

    findings.extend(build_risky_change_findings(
        report,
        runtime,
        root,
        ctx,
        &mut skipped_ambiguous_matches,
    ));
    findings.extend(build_delete_findings(runtime, root));
    findings.extend(build_review_findings(
        report.coverage_gaps.as_ref(),
        &report.hotspots,
        runtime,
        root,
    ));
    findings.extend(build_refactor_findings(
        report,
        runtime,
        root,
        &mut skipped_ambiguous_matches,
    ));

    dedupe_and_sort(&mut findings);

    if findings.is_empty() && skipped_ambiguous_matches == 0 {
        return None;
    }

    let summary = summarize(&findings, skipped_ambiguous_matches);
    let verdict = findings
        .first()
        .map_or(CoverageIntelligenceVerdict::Clean, |finding| {
            finding.verdict
        });

    Some(CoverageIntelligenceReport {
        schema_version: CoverageIntelligenceSchemaVersion::V1,
        verdict,
        summary,
        findings,
    })
}

fn build_risky_change_findings(
    report: &HealthReport,
    runtime: &RuntimeCoverageReport,
    root: &Path,
    ctx: CoverageIntelligenceContext,
    skipped_ambiguous_matches: &mut usize,
) -> Vec<CoverageIntelligenceFinding> {
    if !ctx.has_change_scope {
        return Vec::new();
    }

    runtime
        .hot_paths
        .iter()
        .filter_map(|hot_path| {
            risky_finding_for_hot_path(report, root, hot_path, skipped_ambiguous_matches)
        })
        .collect()
}

/// Assemble the evidence block for a matched risky-change hot path.
fn build_risky_finding_evidence(
    report: &HealthReport,
    root: &Path,
    hot_path: &crate::health_types::RuntimeCoverageHotPath,
    matched: &MatchedComplexity<'_>,
) -> CoverageIntelligenceEvidence {
    CoverageIntelligenceEvidence {
        coverage_pct: matched.finding.coverage_pct,
        crap: matched.finding.crap,
        runtime_verdict: None,
        invocations: Some(hot_path.invocations),
        static_status: None,
        test_coverage: matched
            .finding
            .coverage_tier
            .map(|tier| coverage_tier_label(tier).to_owned()),
        changed: true,
        ownership_state: ownership_state_for(&report.hotspots, root, &hot_path.path),
        match_confidence: matched.confidence,
    }
}

/// Resolve one hot path into a risky-change finding, gating on coverage + CRAP.
fn risky_finding_for_hot_path(
    report: &HealthReport,
    root: &Path,
    hot_path: &crate::health_types::RuntimeCoverageHotPath,
    skipped_ambiguous_matches: &mut usize,
) -> Option<CoverageIntelligenceFinding> {
    let matched = match_complexity(
        report,
        root,
        &hot_path.path,
        &hot_path.function,
        hot_path.line,
        skipped_ambiguous_matches,
    )?;
    if !is_low_test_coverage(matched.finding) || !is_high_crap(matched.finding) {
        return None;
    }

    let signals = ordered_signals([
        CoverageIntelligenceSignal::Changed,
        CoverageIntelligenceSignal::HotPath,
        CoverageIntelligenceSignal::LowTestCoverage,
        CoverageIntelligenceSignal::HighCrap,
    ]);
    let evidence = build_risky_finding_evidence(report, root, hot_path, &matched);
    Some(make_finding(FindingInput {
        path: root_relative_path(root, &hot_path.path),
        identity: Some(hot_path.function.clone()),
        line: hot_path.line,
        verdict: CoverageIntelligenceVerdict::RiskyChangeDetected,
        signals,
        recommendation: CoverageIntelligenceRecommendation::AddTestOrSplitBeforeMerge,
        confidence: confidence_from_match(matched.confidence),
        related_ids: related_ids([Some(hot_path.id.as_str()), hot_path.stable_id.as_deref()]),
        evidence,
    }))
}

fn build_delete_findings(
    runtime: &RuntimeCoverageReport,
    root: &Path,
) -> Vec<CoverageIntelligenceFinding> {
    runtime
        .findings
        .iter()
        .filter(|finding| {
            finding.verdict == RuntimeCoverageVerdict::SafeToDelete
                && finding.evidence.static_status == "unused"
                && finding.evidence.test_coverage == "not_covered"
        })
        .map(|finding| {
            let signals = ordered_signals([
                CoverageIntelligenceSignal::StaticUnused,
                CoverageIntelligenceSignal::RuntimeCold,
                CoverageIntelligenceSignal::NoTestPath,
            ]);
            let evidence = runtime_finding_evidence(
                finding,
                CoverageIntelligenceMatchConfidence::Direct,
                false,
                None,
            );
            make_finding(FindingInput {
                path: root_relative_path(root, &finding.path),
                identity: Some(finding.function.clone()),
                line: finding.line,
                verdict: CoverageIntelligenceVerdict::HighConfidenceDelete,
                signals,
                recommendation: CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner,
                confidence: CoverageIntelligenceConfidence::High,
                related_ids: related_ids([
                    Some(finding.id.as_str()),
                    finding.stable_id.as_deref(),
                    finding.source_hash.as_deref(),
                ]),
                evidence,
            })
        })
        .collect()
}

fn build_review_findings(
    coverage_gaps: Option<&CoverageGaps>,
    hotspots: &[HotspotFinding],
    runtime: &RuntimeCoverageReport,
    root: &Path,
) -> Vec<CoverageIntelligenceFinding> {
    runtime
        .findings
        .iter()
        .filter(|finding| {
            finding.verdict == RuntimeCoverageVerdict::ReviewRequired
                && finding.evidence.static_status != "unused"
                && finding.evidence.test_coverage == "not_covered"
                && has_coverage_gap(coverage_gaps, root, &finding.path, &finding.function)
        })
        .filter_map(|finding| {
            let ownership_state = ownership_state_for(hotspots, root, &finding.path)?;
            if !is_risky_ownership(&ownership_state) {
                return None;
            }

            let signals = ordered_signals([
                CoverageIntelligenceSignal::RuntimeReachable,
                CoverageIntelligenceSignal::RuntimeCold,
                CoverageIntelligenceSignal::NoTestPath,
                CoverageIntelligenceSignal::OwnershipDrift,
            ]);
            let evidence = runtime_finding_evidence(
                finding,
                CoverageIntelligenceMatchConfidence::Direct,
                false,
                Some(ownership_state),
            );
            Some(make_finding(FindingInput {
                path: root_relative_path(root, &finding.path),
                identity: Some(finding.function.clone()),
                line: finding.line,
                verdict: CoverageIntelligenceVerdict::ReviewRequired,
                signals,
                recommendation: CoverageIntelligenceRecommendation::ReviewBeforeChanging,
                confidence: CoverageIntelligenceConfidence::Medium,
                related_ids: related_ids([
                    Some(finding.id.as_str()),
                    finding.stable_id.as_deref(),
                    finding.source_hash.as_deref(),
                ]),
                evidence,
            }))
        })
        .collect()
}

fn build_refactor_findings(
    report: &HealthReport,
    runtime: &RuntimeCoverageReport,
    root: &Path,
    skipped_ambiguous_matches: &mut usize,
) -> Vec<CoverageIntelligenceFinding> {
    runtime
        .hot_paths
        .iter()
        .filter_map(|hot_path| {
            let matched = match_complexity(
                report,
                root,
                &hot_path.path,
                &hot_path.function,
                hot_path.line,
                skipped_ambiguous_matches,
            )?;
            if !is_test_covered(matched.finding) || !is_high_crap(matched.finding) {
                return None;
            }

            let signals = ordered_signals([
                CoverageIntelligenceSignal::HotPath,
                CoverageIntelligenceSignal::TestCovered,
                CoverageIntelligenceSignal::HighCrap,
            ]);
            let evidence = CoverageIntelligenceEvidence {
                coverage_pct: matched.finding.coverage_pct,
                crap: matched.finding.crap,
                runtime_verdict: None,
                invocations: Some(hot_path.invocations),
                static_status: None,
                test_coverage: Some("covered".to_owned()),
                changed: false,
                ownership_state: ownership_state_for(&report.hotspots, root, &hot_path.path),
                match_confidence: matched.confidence,
            };
            Some(make_finding(FindingInput {
                path: root_relative_path(root, &hot_path.path),
                identity: Some(hot_path.function.clone()),
                line: hot_path.line,
                verdict: CoverageIntelligenceVerdict::RefactorCarefully,
                signals,
                recommendation: CoverageIntelligenceRecommendation::RefactorCarefullyKeepBehavior,
                confidence: confidence_from_match(matched.confidence),
                related_ids: related_ids([
                    Some(hot_path.id.as_str()),
                    hot_path.stable_id.as_deref(),
                ]),
                evidence,
            }))
        })
        .collect()
}

struct MatchedComplexity<'a> {
    finding: &'a HealthFinding,
    confidence: CoverageIntelligenceMatchConfidence,
}

fn match_complexity<'a>(
    report: &'a HealthReport,
    root: &Path,
    path: &Path,
    function: &str,
    line: u32,
    skipped_ambiguous_matches: &mut usize,
) -> Option<MatchedComplexity<'a>> {
    let path_key = normalize_path(root, path);
    let function_matches: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| {
            normalize_path(root, &finding.path) == path_key
                && finding.name == function
                && finding.line == line
        })
        .collect();
    if let Some(finding) = only(function_matches, skipped_ambiguous_matches) {
        return Some(MatchedComplexity {
            finding,
            confidence: CoverageIntelligenceMatchConfidence::PathFunctionLine,
        });
    }

    let line_matches: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| normalize_path(root, &finding.path) == path_key && finding.line == line)
        .collect();
    only(line_matches, skipped_ambiguous_matches).map(|finding| MatchedComplexity {
        finding,
        confidence: CoverageIntelligenceMatchConfidence::PathLine,
    })
}

fn only<'a>(
    matches: Vec<&'a HealthFinding>,
    skipped_ambiguous_matches: &mut usize,
) -> Option<&'a HealthFinding> {
    match matches.len() {
        0 => None,
        1 => matches.into_iter().next(),
        _ => {
            *skipped_ambiguous_matches += 1;
            None
        }
    }
}

#[derive(Clone)]
struct FindingInput {
    path: PathBuf,
    identity: Option<String>,
    line: u32,
    verdict: CoverageIntelligenceVerdict,
    signals: Vec<CoverageIntelligenceSignal>,
    recommendation: CoverageIntelligenceRecommendation,
    confidence: CoverageIntelligenceConfidence,
    related_ids: Vec<String>,
    evidence: CoverageIntelligenceEvidence,
}

fn make_finding(input: FindingInput) -> CoverageIntelligenceFinding {
    let id = coverage_intelligence_id(&input);
    let action = CoverageIntelligenceAction {
        kind: input.recommendation.as_str().to_owned(),
        description: action_description(
            input.recommendation,
            &input.path,
            input.identity.as_deref(),
        ),
        auto_fixable: false,
    };

    CoverageIntelligenceFinding {
        id,
        path: input.path,
        identity: input.identity,
        line: input.line,
        verdict: input.verdict,
        signals: input.signals,
        recommendation: input.recommendation,
        confidence: input.confidence,
        related_ids: input.related_ids,
        evidence: input.evidence,
        actions: vec![action],
    }
}

fn coverage_intelligence_id(input: &FindingInput) -> String {
    let mut key = String::new();
    key.push_str("v1|");
    key.push_str(&input.path.to_string_lossy().replace('\\', "/"));
    key.push('|');
    key.push_str(input.identity.as_deref().unwrap_or(""));
    key.push('|');
    key.push_str(&input.line.to_string());
    key.push('|');
    key.push_str(input.recommendation.as_str());
    key.push('|');
    for signal in &input.signals {
        key.push_str(signal.as_str());
        key.push(',');
    }
    if let Some(preferred) = input
        .related_ids
        .iter()
        .find(|id| id.starts_with("fallow:fn:") || id.len() == 16)
    {
        key.push('|');
        key.push_str(preferred);
    }
    format!("fallow:coverage-intel:{:016x}", xxh3_64(key.as_bytes()))
}

fn action_description(
    recommendation: CoverageIntelligenceRecommendation,
    path: &Path,
    identity: Option<&str>,
) -> String {
    let target = identity.map_or_else(
        || path.display().to_string(),
        |name| format!("{}:{name}", path.display()),
    );
    match recommendation {
        CoverageIntelligenceRecommendation::AddTestOrSplitBeforeMerge => {
            format!("Add focused tests or split the risky change before merging `{target}`")
        }
        CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner => {
            format!("Confirm ownership, then remove cold unused code in `{target}`")
        }
        CoverageIntelligenceRecommendation::ReviewBeforeChanging => {
            format!("Route `{target}` to an owner before changing or deleting it")
        }
        CoverageIntelligenceRecommendation::RefactorCarefullyKeepBehavior => {
            format!("Refactor `{target}` behind behavior-preserving tests")
        }
    }
}

fn runtime_finding_evidence(
    finding: &RuntimeCoverageFinding,
    match_confidence: CoverageIntelligenceMatchConfidence,
    changed: bool,
    ownership_state: Option<String>,
) -> CoverageIntelligenceEvidence {
    CoverageIntelligenceEvidence {
        coverage_pct: None,
        crap: None,
        runtime_verdict: Some(finding.verdict.as_str().to_owned()),
        invocations: finding.invocations,
        static_status: Some(finding.evidence.static_status.clone()),
        test_coverage: Some(finding.evidence.test_coverage.clone()),
        changed,
        ownership_state,
        match_confidence,
    }
}

fn summarize(
    findings: &[CoverageIntelligenceFinding],
    skipped_ambiguous_matches: usize,
) -> CoverageIntelligenceSummary {
    CoverageIntelligenceSummary {
        findings: findings.len(),
        risky_changes: findings
            .iter()
            .filter(|finding| finding.verdict == CoverageIntelligenceVerdict::RiskyChangeDetected)
            .count(),
        high_confidence_deletes: findings
            .iter()
            .filter(|finding| finding.verdict == CoverageIntelligenceVerdict::HighConfidenceDelete)
            .count(),
        review_required: findings
            .iter()
            .filter(|finding| finding.verdict == CoverageIntelligenceVerdict::ReviewRequired)
            .count(),
        refactor_carefully: findings
            .iter()
            .filter(|finding| finding.verdict == CoverageIntelligenceVerdict::RefactorCarefully)
            .count(),
        skipped_ambiguous_matches,
    }
}

fn dedupe_and_sort(findings: &mut Vec<CoverageIntelligenceFinding>) {
    let mut by_id = BTreeMap::new();
    for finding in findings.drain(..) {
        by_id.entry(finding.id.clone()).or_insert(finding);
    }
    findings.extend(by_id.into_values());
    findings.sort_by(|a, b| {
        verdict_rank(a.verdict)
            .cmp(&verdict_rank(b.verdict))
            .then_with(|| confidence_rank(a.confidence).cmp(&confidence_rank(b.confidence)))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.identity.cmp(&b.identity))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn verdict_rank(verdict: CoverageIntelligenceVerdict) -> u8 {
    match verdict {
        CoverageIntelligenceVerdict::RiskyChangeDetected => 0,
        CoverageIntelligenceVerdict::HighConfidenceDelete => 1,
        CoverageIntelligenceVerdict::ReviewRequired => 2,
        CoverageIntelligenceVerdict::RefactorCarefully => 3,
        CoverageIntelligenceVerdict::Clean => 4,
        CoverageIntelligenceVerdict::Unknown => 5,
    }
}

fn confidence_rank(confidence: CoverageIntelligenceConfidence) -> u8 {
    match confidence {
        CoverageIntelligenceConfidence::High => 0,
        CoverageIntelligenceConfidence::Medium => 1,
        CoverageIntelligenceConfidence::Low => 2,
    }
}

fn confidence_from_match(
    confidence: CoverageIntelligenceMatchConfidence,
) -> CoverageIntelligenceConfidence {
    match confidence {
        CoverageIntelligenceMatchConfidence::Direct => CoverageIntelligenceConfidence::High,
        CoverageIntelligenceMatchConfidence::PathFunctionLine => {
            CoverageIntelligenceConfidence::Medium
        }
        CoverageIntelligenceMatchConfidence::PathLine => CoverageIntelligenceConfidence::Low,
    }
}

fn ordered_signals<const N: usize>(
    signals: [CoverageIntelligenceSignal; N],
) -> Vec<CoverageIntelligenceSignal> {
    let mut signals = signals.to_vec();
    signals.sort();
    signals.dedup();
    signals
}

fn related_ids<const N: usize>(ids: [Option<&str>; N]) -> Vec<String> {
    let mut out: Vec<_> = ids
        .into_iter()
        .flatten()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    out.sort();
    out.dedup();
    out
}

fn is_low_test_coverage(finding: &HealthFinding) -> bool {
    finding
        .coverage_pct
        .is_some_and(|coverage_pct| coverage_pct < 70.0)
        || matches!(
            finding.coverage_tier,
            Some(CoverageTier::None | CoverageTier::Partial)
        )
}

fn is_test_covered(finding: &HealthFinding) -> bool {
    finding
        .coverage_pct
        .is_some_and(|coverage_pct| coverage_pct >= 70.0)
        || matches!(finding.coverage_tier, Some(CoverageTier::High))
}

fn is_high_crap(finding: &HealthFinding) -> bool {
    finding.exceeded.includes_crap()
}

fn coverage_tier_label(tier: CoverageTier) -> &'static str {
    match tier {
        CoverageTier::None => "not_covered",
        CoverageTier::Partial => "partially_covered",
        CoverageTier::High => "covered",
    }
}

fn has_coverage_gap(
    coverage_gaps: Option<&CoverageGaps>,
    root: &Path,
    path: &Path,
    function: &str,
) -> bool {
    let Some(coverage_gaps) = coverage_gaps else {
        return false;
    };
    let path_key = normalize_path(root, path);
    coverage_gaps
        .files
        .iter()
        .any(|file| normalize_path(root, &file.file.path) == path_key)
        || coverage_gaps.exports.iter().any(|export| {
            normalize_path(root, &export.export.path) == path_key
                && export.export.export_name == function
        })
}

fn ownership_state_for(hotspots: &[HotspotFinding], root: &Path, path: &Path) -> Option<String> {
    let path_key = normalize_path(root, path);
    hotspots
        .iter()
        .find(|hotspot| normalize_path(root, &hotspot.path) == path_key)
        .and_then(|hotspot| hotspot.ownership.as_ref())
        .map(|ownership| ownership.ownership_state)
        .map(ownership_state_label)
        .map(str::to_owned)
}

fn is_risky_ownership(state: &str) -> bool {
    matches!(state, "unowned" | "declared_inactive" | "drifting")
}

fn ownership_state_label(state: OwnershipState) -> &'static str {
    match state {
        OwnershipState::Active => "active",
        OwnershipState::Unowned => "unowned",
        OwnershipState::DeclaredInactive => "declared_inactive",
        OwnershipState::Drifting => "drifting",
    }
}

fn root_relative_path(root: &Path, path: &Path) -> PathBuf {
    PathBuf::from(normalize_path(root, path))
}

fn normalize_path(root: &Path, path: &Path) -> String {
    let stripped = path.strip_prefix(root).unwrap_or(path);
    stripped.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health_types::{
        ComplexityViolation, ContributorEntry, ContributorIdentifierFormat, CoverageGapSummary,
        CoverageGaps, ExceededThreshold, FindingSeverity, HealthSummary, HotspotEntry,
        OwnershipMetrics, RuntimeCoverageEvidence, RuntimeCoverageHotPath,
        RuntimeCoverageReportVerdict, RuntimeCoverageSummary, UntestedFile, UntestedFileFinding,
    };

    #[test]
    fn changed_hot_low_coverage_high_crap_is_risky_change() {
        let root = Path::new("/repo");
        let report = report_with(
            root,
            vec![complexity(
                root,
                "src/hot.ts",
                "handler",
                10,
                Some(20.0),
                Some(45.0),
            )],
            None,
            Some(runtime_with_hot("src/hot.ts", "handler", 10)),
            vec![],
        );

        let intelligence = build_coverage_intelligence(
            &report,
            root,
            CoverageIntelligenceContext {
                has_change_scope: true,
            },
        )
        .expect("coverage intelligence");

        assert_eq!(
            intelligence.verdict,
            CoverageIntelligenceVerdict::RiskyChangeDetected
        );
        assert_eq!(intelligence.findings[0].path, PathBuf::from("src/hot.ts"));
        assert!(intelligence.findings[0].evidence.changed);
    }

    #[test]
    fn static_unused_runtime_cold_and_uncovered_is_delete() {
        let root = Path::new("/repo");
        let report = report_with(
            root,
            vec![],
            None,
            Some(runtime_with_cold_delete("src/dead.ts", "deadPath", 4)),
            vec![],
        );

        let intelligence = build_coverage_intelligence(
            &report,
            root,
            CoverageIntelligenceContext {
                has_change_scope: false,
            },
        )
        .expect("coverage intelligence");

        assert_eq!(
            intelligence.verdict,
            CoverageIntelligenceVerdict::HighConfidenceDelete
        );
        assert_eq!(
            intelligence.findings[0].recommendation,
            CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner
        );
    }

    #[test]
    fn ambiguous_complexity_match_is_skipped() {
        let root = Path::new("/repo");
        let report = report_with(
            root,
            vec![
                complexity(root, "src/hot.ts", "handler", 10, Some(20.0), Some(45.0)),
                complexity(root, "src/hot.ts", "other", 10, Some(20.0), Some(45.0)),
            ],
            None,
            Some(runtime_with_hot("src/hot.ts", "missing", 10)),
            vec![],
        );

        let intelligence = build_coverage_intelligence(
            &report,
            root,
            CoverageIntelligenceContext {
                has_change_scope: true,
            },
        )
        .expect("ambiguous skip summary");

        assert!(intelligence.findings.is_empty());
        assert_eq!(intelligence.summary.skipped_ambiguous_matches, 2);
    }

    #[test]
    fn cold_reachable_uncovered_with_ownership_drift_requires_review() {
        let root = Path::new("/repo");
        let report = report_with(
            root,
            vec![],
            Some(coverage_gap(root, "src/cold.ts")),
            Some(runtime_with_review_required("src/cold.ts", "coldPath", 9)),
            vec![hotspot(root, "src/cold.ts", OwnershipState::Drifting)],
        );

        let intelligence = build_coverage_intelligence(
            &report,
            root,
            CoverageIntelligenceContext {
                has_change_scope: false,
            },
        )
        .expect("coverage intelligence");

        assert_eq!(
            intelligence.verdict,
            CoverageIntelligenceVerdict::ReviewRequired
        );
        assert_eq!(
            intelligence.findings[0].evidence.ownership_state.as_deref(),
            Some("drifting")
        );
    }

    #[test]
    fn covered_hot_high_crap_is_refactor_carefully() {
        let root = Path::new("/repo");
        let report = report_with(
            root,
            vec![complexity(
                root,
                "src/hot.ts",
                "handler",
                10,
                Some(85.0),
                Some(40.0),
            )],
            None,
            Some(runtime_with_hot("src/hot.ts", "handler", 10)),
            vec![],
        );

        let intelligence = build_coverage_intelligence(
            &report,
            root,
            CoverageIntelligenceContext {
                has_change_scope: false,
            },
        )
        .expect("coverage intelligence");

        assert_eq!(
            intelligence.verdict,
            CoverageIntelligenceVerdict::RefactorCarefully
        );
        assert_eq!(
            intelligence.findings[0].recommendation,
            CoverageIntelligenceRecommendation::RefactorCarefullyKeepBehavior
        );
    }

    #[test]
    fn ids_are_normalized_across_windows_separators() {
        let a = FindingInput {
            path: PathBuf::from("src/app.ts"),
            identity: Some("handler".to_owned()),
            line: 3,
            verdict: CoverageIntelligenceVerdict::RefactorCarefully,
            signals: ordered_signals([
                CoverageIntelligenceSignal::HotPath,
                CoverageIntelligenceSignal::HighCrap,
            ]),
            recommendation: CoverageIntelligenceRecommendation::RefactorCarefullyKeepBehavior,
            confidence: CoverageIntelligenceConfidence::Medium,
            related_ids: vec!["fallow:fn:abc".to_owned()],
            evidence: CoverageIntelligenceEvidence::default(),
        };
        let mut b = a.clone();
        b.path = PathBuf::from("src\\app.ts");

        assert_eq!(coverage_intelligence_id(&a), coverage_intelligence_id(&b));
    }

    fn report_with(
        _root: &Path,
        findings: Vec<ComplexityViolation>,
        coverage_gaps: Option<CoverageGaps>,
        runtime_coverage: Option<RuntimeCoverageReport>,
        hotspots: Vec<HotspotFinding>,
    ) -> HealthReport {
        HealthReport {
            findings: findings.into_iter().map(HealthFinding::from).collect(),
            summary: HealthSummary::default(),
            coverage_gaps,
            runtime_coverage,
            hotspots,
            ..HealthReport::default()
        }
    }

    fn complexity(
        root: &Path,
        path: &str,
        name: &str,
        line: u32,
        coverage_pct: Option<f64>,
        crap: Option<f64>,
    ) -> ComplexityViolation {
        ComplexityViolation {
            path: root.join(path),
            name: name.to_owned(),
            line,
            col: 0,
            cyclomatic: 20,
            cognitive: 20,
            line_count: 20,
            param_count: 1,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: ExceededThreshold::Crap,
            severity: FindingSeverity::High,
            crap,
            coverage_pct,
            coverage_tier: coverage_pct.map(|pct| {
                if pct >= 70.0 {
                    CoverageTier::High
                } else if pct > 0.0 {
                    CoverageTier::Partial
                } else {
                    CoverageTier::None
                }
            }),
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
        }
    }

    fn runtime_with_hot(path: &str, function: &str, line: u32) -> RuntimeCoverageReport {
        let mut report = RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: RuntimeCoverageReportVerdict::HotPathTouched,
            signals: vec![crate::health_types::RuntimeCoverageSignal::HotPathTouched],
            summary: RuntimeCoverageSummary::default(),
            findings: vec![],
            hot_paths: vec![RuntimeCoverageHotPath {
                id: "fallow:hot:deadbeef".to_owned(),
                stable_id: None,
                path: PathBuf::from(path),
                function: function.to_owned(),
                line,
                end_line: line,
                invocations: 500,
                percentile: 99,
                actions: vec![],
            }],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: crate::health_types::RuntimeCoverageProvenance::default(),
        };
        report.summary.functions_tracked = 1;
        report
    }

    fn runtime_with_cold_delete(path: &str, function: &str, line: u32) -> RuntimeCoverageReport {
        RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: RuntimeCoverageReportVerdict::ColdCodeDetected,
            signals: vec![crate::health_types::RuntimeCoverageSignal::ColdCodeDetected],
            summary: RuntimeCoverageSummary::default(),
            findings: vec![RuntimeCoverageFinding {
                id: "fallow:prod:deadbeef".to_owned(),
                stable_id: Some("fallow:fn:deadbeef".to_owned()),
                source_hash: Some("feedfacefeedface".to_owned()),
                path: PathBuf::from(path),
                function: function.to_owned(),
                line,
                verdict: RuntimeCoverageVerdict::SafeToDelete,
                invocations: Some(0),
                confidence: crate::health_types::RuntimeCoverageConfidence::High,
                evidence: RuntimeCoverageEvidence {
                    static_status: "unused".to_owned(),
                    test_coverage: "not_covered".to_owned(),
                    v8_tracking: "tracked".to_owned(),
                    untracked_reason: None,
                    observation_days: 7,
                    deployments_observed: 2,
                },
                actions: vec![],
                discriminators: None,
            }],
            hot_paths: vec![],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: crate::health_types::RuntimeCoverageProvenance::default(),
        }
    }

    fn runtime_with_review_required(
        path: &str,
        function: &str,
        line: u32,
    ) -> RuntimeCoverageReport {
        RuntimeCoverageReport {
            schema_version: crate::health_types::RuntimeCoverageSchemaVersion::V1,
            verdict: RuntimeCoverageReportVerdict::ColdCodeDetected,
            signals: vec![crate::health_types::RuntimeCoverageSignal::ColdCodeDetected],
            summary: RuntimeCoverageSummary::default(),
            findings: vec![RuntimeCoverageFinding {
                id: "fallow:prod:review001".to_owned(),
                stable_id: Some("fallow:fn:review001".to_owned()),
                source_hash: Some("feedfacefeedface".to_owned()),
                path: PathBuf::from(path),
                function: function.to_owned(),
                line,
                verdict: RuntimeCoverageVerdict::ReviewRequired,
                invocations: Some(0),
                confidence: crate::health_types::RuntimeCoverageConfidence::Medium,
                evidence: RuntimeCoverageEvidence {
                    static_status: "used".to_owned(),
                    test_coverage: "not_covered".to_owned(),
                    v8_tracking: "tracked".to_owned(),
                    untracked_reason: None,
                    observation_days: 7,
                    deployments_observed: 2,
                },
                actions: vec![],
                discriminators: None,
            }],
            hot_paths: vec![],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: crate::health_types::RuntimeCoverageProvenance::default(),
        }
    }

    fn coverage_gap(root: &Path, path: &str) -> CoverageGaps {
        CoverageGaps {
            summary: CoverageGapSummary::default(),
            files: vec![UntestedFileFinding::with_actions(
                UntestedFile {
                    path: root.join(path),
                    value_export_count: 1,
                },
                root,
            )],
            exports: vec![],
        }
    }

    fn hotspot(root: &Path, path: &str, ownership_state: OwnershipState) -> HotspotFinding {
        HotspotFinding::with_actions(
            HotspotEntry {
                path: root.join(path),
                score: 80.0,
                commits: 10,
                weighted_commits: 8.0,
                lines_added: 100,
                lines_deleted: 20,
                complexity_density: 0.5,
                fan_in: 3,
                trend: fallow_core::churn::ChurnTrend::Stable,
                ownership: Some(OwnershipMetrics {
                    bus_factor: 1,
                    contributor_count: 1,
                    top_contributor: contributor(),
                    recent_contributors: vec![],
                    suggested_reviewers: vec![],
                    declared_owner: None,
                    unowned: Some(false),
                    ownership_state,
                    drift: ownership_state == OwnershipState::Drifting,
                    drift_reason: None,
                }),
                is_test_path: false,
            },
            root,
        )
    }

    fn contributor() -> ContributorEntry {
        ContributorEntry {
            identifier: "dev".to_owned(),
            format: ContributorIdentifierFormat::Handle,
            share: 1.0,
            stale_days: 180,
            commits: 10,
        }
    }
}
