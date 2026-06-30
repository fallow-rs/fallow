use crate::duplicates::{self, DuplicationReport};
use fallow_output::{
    COGNITIVE_EXTRACTION_THRESHOLD, CloneSiblingEvidence, Confidence, ContributingFactor,
    DirectCallerEvidence, EffortEstimate, EvidenceFunction, FileHealthScore, HotspotEntry,
    RecommendationCategory, RefactoringTarget, TargetEvidence, TargetThresholds,
};

const MAX_CLONE_SIBLING_EVIDENCE: usize = 5;

/// Auxiliary data used by `compute_refactoring_targets` to generate evidence and apply rules.
pub(super) struct TargetAuxData<'a> {
    pub circular_files: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
    pub top_complex_fns: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<(String, u32, u16)>>,
    pub entry_points: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
    pub value_export_counts: &'a rustc_hash::FxHashMap<std::path::PathBuf, usize>,
    pub unused_export_names: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
    pub cycle_members: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
    pub direct_callers: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<DirectCallerEvidence>>,
    pub clone_siblings: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<CloneSiblingEvidence>>,
}

impl<'a> TargetAuxData<'a> {
    pub(super) fn from_output(
        output: &'a super::scoring::FileScoreOutput,
        clone_siblings: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<CloneSiblingEvidence>>,
    ) -> Self {
        Self {
            circular_files: &output.circular_files,
            top_complex_fns: &output.top_complex_fns,
            entry_points: &output.entry_points,
            value_export_counts: &output.value_export_counts,
            unused_export_names: &output.unused_export_names,
            cycle_members: &output.cycle_members,
            direct_callers: &output.direct_callers,
            clone_siblings,
        }
    }
}

/// Adaptive thresholds derived from the project's metric distribution.
///
/// Replaces hardcoded constants (fan_in=20, fan_out=30) with percentile-based
/// values that adapt to the codebase size. Floors prevent degenerate thresholds
/// in small projects.
#[expect(
    clippy::struct_field_names,
    reason = "fan_in/fan_out prefix clarifies the metric"
)]
struct DistributionThresholds {
    /// Fan-in saturation point for priority formula (p95, floor 5).
    fan_in_p95: f64,
    /// Fan-in "moderate" threshold for contributing factors and rule 3 (p75, floor 3).
    fan_in_p75: f64,
    /// Fan-in "low" ceiling for effort estimation (p25, floor 2).
    fan_in_p25: usize,
    /// Fan-out saturation point for priority formula (p95, floor 8).
    fan_out_p95: f64,
    /// Fan-out "high" threshold for contributing factors and rule 6 (p90, floor 5).
    fan_out_p90: usize,
}

/// Compute percentile-based thresholds from the file score distribution.
#[expect(
    clippy::cast_possible_truncation,
    reason = "percentile values are bounded by fan-in/fan-out counts"
)]
fn compute_thresholds(file_scores: &[FileHealthScore]) -> DistributionThresholds {
    if file_scores.is_empty() {
        return DistributionThresholds {
            fan_in_p95: 5.0,
            fan_in_p75: 3.0,
            fan_in_p25: 2,
            fan_out_p95: 8.0,
            fan_out_p90: 5,
        };
    }

    let mut fan_ins: Vec<usize> = file_scores.iter().map(|s| s.fan_in).collect();
    let mut fan_outs: Vec<usize> = file_scores.iter().map(|s| s.fan_out).collect();
    fan_ins.sort_unstable();
    fan_outs.sort_unstable();

    DistributionThresholds {
        fan_in_p95: percentile_usize(&fan_ins, 0.95).max(5.0),
        fan_in_p75: percentile_usize(&fan_ins, 0.75).max(3.0),
        fan_in_p25: (percentile_usize(&fan_ins, 0.25) as usize).max(2),
        fan_out_p95: percentile_usize(&fan_outs, 0.95).max(8.0),
        fan_out_p90: (percentile_usize(&fan_outs, 0.90) as usize).max(5),
    }
}

/// Compute a percentile value from a sorted slice of usize values.
#[expect(
    clippy::cast_possible_truncation,
    reason = "index from percentile of slice length is bounded by slice length"
)]
fn percentile_usize(sorted: &[usize], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (sorted.len() as f64 * p).ceil() as usize;
    let idx = idx.min(sorted.len()) - 1;
    sorted[idx] as f64
}

/// Compute the refactoring priority score for a file.
///
/// Formula (avoids double-counting with MI):
/// ```text
/// priority = min(density, 1) * 30 + hotspot_boost * 25 + dead_code * 20 + fan_in_norm * 15 + fan_out_norm * 10
/// ```
/// Fan-in and fan-out normalization uses adaptive percentile-based thresholds.
/// All inputs are clamped to \[0, 1\] so each weight is a true percentage share.
fn compute_target_priority(
    score: &FileHealthScore,
    hotspot_score: Option<f64>,
    thresholds: &DistributionThresholds,
) -> f64 {
    let density_norm = score.complexity_density.min(1.0);
    let fan_in_norm = (score.fan_in as f64 / thresholds.fan_in_p95).min(1.0);
    let fan_out_norm = (score.fan_out as f64 / thresholds.fan_out_p95).min(1.0);
    let hotspot_boost = hotspot_score.map_or(0.0, |s| s / 100.0);

    #[expect(
        clippy::suboptimal_flops,
        reason = "formula matches documented specification"
    )]
    let priority = density_norm * 30.0
        + hotspot_boost * 25.0
        + score.dead_code_ratio * 20.0
        + fan_in_norm * 15.0
        + fan_out_norm * 10.0;

    (priority.clamp(0.0, 100.0) * 10.0).round() / 10.0
}

/// Compute refactoring targets by applying rules to file scores and auxiliary data.
///
/// Rules are evaluated in priority order; first match determines the category and
/// recommendation. All contributing factors are collected regardless of which rule wins.
/// Files matching no rule are skipped.
///
/// Targets are sorted by efficiency (priority / effort) descending to surface quick wins first.
pub(super) fn compute_refactoring_targets(
    file_scores: &[FileHealthScore],
    aux: &TargetAuxData,
    hotspots: &[HotspotEntry],
) -> (Vec<RefactoringTarget>, TargetThresholds) {
    let thresholds = compute_thresholds(file_scores);

    let hotspot_map: rustc_hash::FxHashMap<&std::path::Path, &HotspotEntry> =
        hotspots.iter().map(|h| (h.path.as_path(), h)).collect();

    let mut targets = Vec::new();

    for score in file_scores {
        let hotspot = hotspot_map.get(score.path.as_path()).copied();
        if let Some(target) = build_target_for_score(score, hotspot, aux, &thresholds) {
            targets.push(target);
        }
    }

    sort_refactoring_targets(&mut targets);
    let exported_thresholds = export_target_thresholds(&thresholds);

    (targets, exported_thresholds)
}

/// Build a refactoring target for one file score, or `None` if no rule matches
/// (or the file has no contributing factors).
fn build_target_for_score(
    score: &FileHealthScore,
    hotspot: Option<&HotspotEntry>,
    aux: &TargetAuxData<'_>,
    thresholds: &DistributionThresholds,
) -> Option<RefactoringTarget> {
    let hotspot_score = hotspot.map(|h| h.score);
    let is_circular = aux.circular_files.contains(&score.path);
    let is_entry = aux.entry_points.contains(&score.path);
    let top_fns = aux.top_complex_fns.get(&score.path);
    let value_exports = aux
        .value_export_counts
        .get(&score.path)
        .copied()
        .unwrap_or(0);

    let mut factors = Vec::new();

    push_structural_target_factors(&mut factors, score, value_exports, is_circular, thresholds);
    push_runtime_target_factors(&mut factors, score, hotspot, top_fns.map(Vec::as_slice));

    if factors.is_empty() {
        return None;
    }

    let (category, recommendation) = try_match_rules(
        RuleMatchContext {
            score,
            hotspot,
            is_circular,
            is_entry,
            top_fns,
            value_exports,
        },
        thresholds,
    )?;

    let priority = compute_target_priority(score, hotspot_score, thresholds);
    let effort = compute_effort_estimate(score, thresholds);
    let confidence = confidence_for_category(&category);
    let efficiency = (priority / effort.numeric() * 10.0).round() / 10.0;
    let evidence = build_evidence(
        &category,
        &score.path,
        top_fns,
        EvidenceSources {
            unused_export_names: aux.unused_export_names,
            cycle_members: aux.cycle_members,
            direct_callers: aux.direct_callers,
            clone_siblings: aux.clone_siblings,
        },
    );

    Some(RefactoringTarget {
        path: score.path.clone(),
        priority,
        efficiency,
        recommendation,
        category,
        effort,
        confidence,
        factors,
        evidence,
    })
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "unused export estimate is capped by the value export count"
)]
fn push_structural_target_factors(
    factors: &mut Vec<ContributingFactor>,
    score: &FileHealthScore,
    value_exports: usize,
    is_circular: bool,
    thresholds: &DistributionThresholds,
) {
    if score.complexity_density > 0.3 {
        factors.push(ContributingFactor {
            metric: "complexity_density",
            value: score.complexity_density,
            threshold: 0.3,
            detail: format!("density {:.2} exceeds 0.3", score.complexity_density),
        });
    }
    if score.fan_in as f64 >= thresholds.fan_in_p75 {
        factors.push(ContributingFactor {
            metric: "fan_in",
            value: score.fan_in as f64,
            threshold: thresholds.fan_in_p75,
            detail: format!("{} files depend on this", score.fan_in),
        });
    }
    if score.dead_code_ratio >= 0.5 && value_exports >= 3 {
        let unused_count = (score.dead_code_ratio * value_exports as f64)
            .round()
            .min(value_exports as f64) as usize;
        factors.push(ContributingFactor {
            metric: "dead_code_ratio",
            value: score.dead_code_ratio,
            threshold: 0.5,
            detail: format!(
                "{} unused of {} value exports ({:.0}%)",
                unused_count,
                value_exports,
                score.dead_code_ratio * 100.0
            ),
        });
    }
    if score.fan_out >= thresholds.fan_out_p90 {
        factors.push(ContributingFactor {
            metric: "fan_out",
            value: score.fan_out as f64,
            threshold: thresholds.fan_out_p90 as f64,
            detail: format!("imports {} modules", score.fan_out),
        });
    }
    if is_circular {
        factors.push(ContributingFactor {
            metric: "circular_dependency",
            value: 1.0,
            threshold: 1.0,
            detail: "participates in an import cycle".into(),
        });
    }
}

fn push_runtime_target_factors(
    factors: &mut Vec<ContributingFactor>,
    score: &FileHealthScore,
    hotspot: Option<&HotspotEntry>,
    top_fns: Option<&[(String, u32, u16)]>,
) {
    if let Some(h) = hotspot
        && h.score >= 30.0
    {
        factors.push(ContributingFactor {
            metric: "hotspot_score",
            value: h.score,
            threshold: 30.0,
            detail: format!(
                "hotspot score {:.0} ({} commits, {} trend)",
                h.score,
                h.commits,
                match h.trend {
                    crate::ChurnTrend::Accelerating => "accelerating",
                    crate::ChurnTrend::Cooling => "cooling",
                    crate::ChurnTrend::Stable => "stable",
                }
            ),
        });
    }
    if let Some(fns) = top_fns
        && let Some((name, _, cog)) = fns.first()
        && *cog >= COGNITIVE_EXTRACTION_THRESHOLD
    {
        factors.push(ContributingFactor {
            metric: "cognitive_complexity",
            value: f64::from(*cog),
            threshold: f64::from(COGNITIVE_EXTRACTION_THRESHOLD),
            detail: format!("{name} has cognitive complexity {cog}"),
        });
    }
    if score.crap_above_threshold >= 2 && score.crap_max >= super::scoring::CRAP_THRESHOLD {
        factors.push(ContributingFactor {
            metric: "crap_max",
            value: score.crap_max,
            threshold: super::scoring::CRAP_THRESHOLD,
            detail: format!(
                "{} functions with untested complexity risk",
                score.crap_above_threshold,
            ),
        });
    }
}

fn sort_refactoring_targets(targets: &mut [RefactoringTarget]) {
    targets.sort_by(|a, b| {
        b.efficiency
            .partial_cmp(&a.efficiency)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.priority
                    .partial_cmp(&a.priority)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.path.cmp(&b.path))
    });
}

fn export_target_thresholds(thresholds: &DistributionThresholds) -> TargetThresholds {
    TargetThresholds {
        fan_in_p95: thresholds.fan_in_p95,
        fan_in_p75: thresholds.fan_in_p75,
        fan_out_p95: thresholds.fan_out_p95,
        fan_out_p90: thresholds.fan_out_p90,
    }
}

/// Try to match a file against refactoring rules in priority order.
///
/// Returns the first matching `(category, recommendation)`, or `None` if no rule matches.
/// Per-file inputs to the refactoring-target rule matcher, bundled so
/// `try_match_rules` takes the file context plus shared thresholds instead of
/// seven positional parameters.
#[derive(Clone, Copy)]
struct RuleMatchContext<'a> {
    score: &'a FileHealthScore,
    hotspot: Option<&'a HotspotEntry>,
    is_circular: bool,
    is_entry: bool,
    top_fns: Option<&'a Vec<(String, u32, u16)>>,
    value_exports: usize,
}

fn try_match_rules(
    ctx: RuleMatchContext<'_>,
    thresholds: &DistributionThresholds,
) -> Option<(RecommendationCategory, String)> {
    let RuleMatchContext {
        score,
        hotspot,
        is_circular,
        is_entry,
        top_fns,
        value_exports,
    } = ctx;
    match_churn_complexity(score, hotspot)
        .or_else(|| match_circular_impact(score, is_circular))
        .or_else(|| match_high_impact_split(score, thresholds))
        .or_else(|| match_dead_code(score, value_exports))
        .or_else(|| match_complex_function_extraction(score, top_fns))
        .or_else(|| match_dependency_extraction(score, thresholds, is_entry))
        .or_else(|| match_coverage_gap(score))
        .or_else(|| match_circular_fallback(is_circular))
}

fn match_churn_complexity(
    score: &FileHealthScore,
    hotspot: Option<&HotspotEntry>,
) -> Option<(RecommendationCategory, String)> {
    let h = hotspot?;
    if h.score < 50.0
        || !matches!(h.trend, crate::ChurnTrend::Accelerating)
        || score.complexity_density <= 0.5
    {
        return None;
    }

    Some((
        RecommendationCategory::UrgentChurnComplexity,
        "Actively-changing file with growing complexity, stabilize before adding features".into(),
    ))
}

fn match_circular_impact(
    score: &FileHealthScore,
    is_circular: bool,
) -> Option<(RecommendationCategory, String)> {
    if !is_circular || score.fan_in < 5 {
        return None;
    }

    Some((
        RecommendationCategory::BreakCircularDependency,
        format!(
            "Break import cycle, {} files depend on this, changes cascade through the cycle",
            score.fan_in
        ),
    ))
}

fn match_high_impact_split(
    score: &FileHealthScore,
    thresholds: &DistributionThresholds,
) -> Option<(RecommendationCategory, String)> {
    let fan_in_high = thresholds.fan_in_p95 as usize;
    let fan_in_moderate = thresholds.fan_in_p75 as usize;
    if score.complexity_density <= 0.3
        || (score.fan_in < fan_in_high
            && (score.fan_in < fan_in_moderate || score.function_count < 5))
    {
        return None;
    }

    Some((
        RecommendationCategory::SplitHighImpact,
        format!(
            "Split high-impact file ({} LOC), {} dependents amplify every change",
            score.lines, score.fan_in
        ),
    ))
}

fn match_dead_code(
    score: &FileHealthScore,
    value_exports: usize,
) -> Option<(RecommendationCategory, String)> {
    if score.dead_code_ratio < 0.5 || value_exports < 3 {
        return None;
    }

    let unused_count = (score.dead_code_ratio * value_exports as f64).round() as usize;
    Some((
        RecommendationCategory::RemoveDeadCode,
        format!(
            "Remove {} unused exports to reduce surface area ({:.0}% dead)",
            unused_count,
            score.dead_code_ratio * 100.0
        ),
    ))
}

fn match_complex_function_extraction(
    score: &FileHealthScore,
    top_fns: Option<&Vec<(String, u32, u16)>>,
) -> Option<(RecommendationCategory, String)> {
    let fns = top_fns?;
    let high: Vec<&(String, u32, u16)> = fns
        .iter()
        .filter(|(_, _, cog)| *cog >= COGNITIVE_EXTRACTION_THRESHOLD)
        .collect();
    if high.is_empty() {
        return None;
    }

    let desc = match high.len() {
        1 => format!(
            "Extract {} (cognitive: {}) in {}-LOC file into smaller functions",
            high[0].0, high[0].2, score.lines
        ),
        _ => format!(
            "Extract {} (cognitive: {}) and {} (cognitive: {}) in {}-LOC file into smaller functions",
            high[0].0, high[0].2, high[1].0, high[1].2, score.lines
        ),
    };
    Some((RecommendationCategory::ExtractComplexFunctions, desc))
}

fn match_dependency_extraction(
    score: &FileHealthScore,
    thresholds: &DistributionThresholds,
    is_entry: bool,
) -> Option<(RecommendationCategory, String)> {
    if is_entry || score.fan_out < thresholds.fan_out_p90 || score.maintainability_index >= 60.0 {
        return None;
    }

    Some((
        RecommendationCategory::ExtractDependencies,
        format!(
            "Reduce coupling, {}-LOC file imports {} modules, limiting testability",
            score.lines, score.fan_out
        ),
    ))
}

fn match_coverage_gap(score: &FileHealthScore) -> Option<(RecommendationCategory, String)> {
    if score.crap_above_threshold < 2 || score.complexity_density <= 0.3 {
        return None;
    }

    Some((
        RecommendationCategory::AddTestCoverage,
        format!(
            "{} complex functions lack test coverage path, add tests before modifying",
            score.crap_above_threshold
        ),
    ))
}

fn match_circular_fallback(is_circular: bool) -> Option<(RecommendationCategory, String)> {
    if !is_circular {
        return None;
    }

    Some((
        RecommendationCategory::BreakCircularDependency,
        "Break import cycle to reduce change cascade risk".into(),
    ))
}

/// Map recommendation category to confidence level based on data source reliability.
const fn confidence_for_category(category: &RecommendationCategory) -> Confidence {
    match category {
        RecommendationCategory::RemoveDeadCode
        | RecommendationCategory::BreakCircularDependency
        | RecommendationCategory::ExtractComplexFunctions
        | RecommendationCategory::AddTestCoverage => Confidence::High,
        RecommendationCategory::SplitHighImpact | RecommendationCategory::ExtractDependencies => {
            Confidence::Medium
        }
        RecommendationCategory::UrgentChurnComplexity => Confidence::Low,
    }
}

/// Compute effort estimate based on file size, function count, and fan-in.
///
/// Uses adaptive thresholds for fan-in.
#[expect(
    clippy::cast_possible_truncation,
    reason = "percentile threshold values are bounded by project size"
)]
fn compute_effort_estimate(
    score: &FileHealthScore,
    thresholds: &DistributionThresholds,
) -> EffortEstimate {
    let fan_in_high = thresholds.fan_in_p95 as usize;
    if score.lines >= 500
        || score.fan_in >= fan_in_high
        || (score.function_count >= 15 && score.complexity_density > 0.5)
    {
        EffortEstimate::High
    } else if score.lines < 100 && score.function_count <= 3 && score.fan_in < thresholds.fan_in_p25
    {
        EffortEstimate::Low
    } else {
        EffortEstimate::Medium
    }
}

/// Build structured evidence for a refactoring target based on its category.
/// The per-file evidence lookup maps, bundled so `build_evidence` takes the
/// category and path plus one sources struct instead of seven parameters.
#[derive(Clone, Copy)]
struct EvidenceSources<'a> {
    unused_export_names: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<String>>,
    cycle_members: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
    direct_callers: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<DirectCallerEvidence>>,
    clone_siblings: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<CloneSiblingEvidence>>,
}

fn build_evidence(
    category: &RecommendationCategory,
    path: &std::path::Path,
    top_fns: Option<&Vec<(String, u32, u16)>>,
    sources: EvidenceSources<'_>,
) -> Option<TargetEvidence> {
    let EvidenceSources {
        unused_export_names,
        cycle_members,
        direct_callers,
        clone_siblings,
    } = sources;
    let mut evidence = TargetEvidence {
        direct_callers: direct_callers.get(path).cloned().unwrap_or_default(),
        clone_siblings: clone_siblings.get(path).cloned().unwrap_or_default(),
        ..Default::default()
    };

    match category {
        RecommendationCategory::RemoveDeadCode => {
            evidence.unused_exports = unused_export_names.get(path).cloned().unwrap_or_default();
        }
        RecommendationCategory::ExtractComplexFunctions => {
            evidence.complex_functions = evidence_functions(top_fns, true);
        }
        RecommendationCategory::BreakCircularDependency => {
            evidence.cycle_path = cycle_path_evidence(cycle_members, path);
        }
        RecommendationCategory::AddTestCoverage => {
            evidence.complex_functions = evidence_functions(top_fns, false);
        }
        _ => {}
    }

    if target_evidence_is_empty(&evidence) {
        None
    } else {
        Some(evidence)
    }
}

/// Map top complex functions to evidence entries. When `extraction_only`, keep
/// only functions at or above the cognitive extraction threshold.
fn evidence_functions(
    top_fns: Option<&Vec<(String, u32, u16)>>,
    extraction_only: bool,
) -> Vec<EvidenceFunction> {
    top_fns
        .map(|fns| {
            fns.iter()
                .filter(|(_, _, cog)| !extraction_only || *cog >= COGNITIVE_EXTRACTION_THRESHOLD)
                .map(|(name, line, cog)| EvidenceFunction {
                    name: name.clone(),
                    line: *line,
                    cognitive: *cog,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Render a file's circular-dependency cycle members as display strings.
fn cycle_path_evidence(
    cycle_members: &rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>>,
    path: &std::path::Path,
) -> Vec<String> {
    cycle_members
        .get(path)
        .map(|files| {
            files
                .iter()
                .map(|f| f.to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn target_evidence_is_empty(evidence: &TargetEvidence) -> bool {
    evidence.unused_exports.is_empty()
        && evidence.complex_functions.is_empty()
        && evidence.cycle_path.is_empty()
        && evidence.direct_callers.is_empty()
        && evidence.clone_siblings.is_empty()
}

pub(super) fn build_clone_sibling_evidence(
    report: &DuplicationReport,
) -> rustc_hash::FxHashMap<std::path::PathBuf, Vec<CloneSiblingEvidence>> {
    let mut by_path: rustc_hash::FxHashMap<std::path::PathBuf, Vec<CloneSiblingEvidence>> =
        rustc_hash::FxHashMap::default();

    for group in &report.clone_groups {
        let fingerprint = duplicates::clone_fingerprint(&group.instances);
        for (idx, instance) in group.instances.iter().enumerate() {
            let siblings = by_path.entry(instance.file.clone()).or_default();
            for (sibling_idx, sibling) in group.instances.iter().enumerate() {
                if sibling_idx == idx {
                    continue;
                }
                siblings.push(CloneSiblingEvidence {
                    path: sibling.file.clone(),
                    start_line: sibling.start_line,
                    end_line: sibling.end_line,
                    fingerprint: fingerprint.clone(),
                });
            }
        }
    }

    for siblings in by_path.values_mut() {
        siblings.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| a.start_line.cmp(&b.start_line))
                .then_with(|| a.end_line.cmp(&b.end_line))
                .then_with(|| a.fingerprint.cmp(&b.fingerprint))
        });
        siblings.dedup_by(|a, b| {
            a.path == b.path
                && a.start_line == b.start_line
                && a.end_line == b.end_line
                && a.fingerprint == b.fingerprint
        });
        siblings.truncate(MAX_CLONE_SIBLING_EVIDENCE);
    }

    by_path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_score(overrides: impl FnOnce(&mut FileHealthScore)) -> FileHealthScore {
        let mut s = FileHealthScore {
            path: std::path::PathBuf::from("/src/foo.ts"),
            fan_in: 0,
            fan_out: 0,
            dead_code_ratio: 0.0,
            complexity_density: 0.0,
            maintainability_index: 100.0,
            total_cyclomatic: 0,
            total_cognitive: 0,
            function_count: 1,
            lines: 100,
            crap_max: 0.0,
            crap_above_threshold: 0,
        };
        overrides(&mut s);
        s
    }

    /// Default thresholds matching the old hardcoded values for test compatibility.
    fn default_thresholds() -> DistributionThresholds {
        DistributionThresholds {
            fan_in_p95: 20.0,
            fan_in_p75: 10.0,
            fan_in_p25: 5,
            fan_out_p95: 30.0,
            fan_out_p90: 15,
        }
    }

    #[test]
    fn thresholds_empty_scores_use_floors() {
        let t = compute_thresholds(&[]);
        assert!((t.fan_in_p95 - 5.0).abs() < f64::EPSILON);
        assert!((t.fan_in_p75 - 3.0).abs() < f64::EPSILON);
        assert_eq!(t.fan_in_p25, 2);
        assert!((t.fan_out_p95 - 8.0).abs() < f64::EPSILON);
        assert_eq!(t.fan_out_p90, 5);
    }

    #[test]
    fn thresholds_floors_prevent_degenerate_values() {
        let scores: Vec<FileHealthScore> = (0..10)
            .map(|i| {
                make_score(|s| {
                    s.path = std::path::PathBuf::from(format!("/src/{i}.ts"));
                    s.fan_in = 1;
                    s.fan_out = 1;
                })
            })
            .collect();
        let t = compute_thresholds(&scores);
        assert!(t.fan_in_p95 >= 5.0, "floor should apply: {}", t.fan_in_p95);
        assert!(t.fan_in_p75 >= 3.0, "floor should apply: {}", t.fan_in_p75);
        assert!(
            t.fan_out_p95 >= 8.0,
            "floor should apply: {}",
            t.fan_out_p95
        );
        assert!(t.fan_out_p90 >= 5, "floor should apply: {}", t.fan_out_p90);
    }

    #[test]
    fn thresholds_adapt_to_large_project() {
        let mut scores: Vec<FileHealthScore> = (0..80)
            .map(|i| {
                make_score(|s| {
                    s.path = std::path::PathBuf::from(format!("/src/{i}.ts"));
                    s.fan_in = i % 5; // 0..4
                    s.fan_out = i % 8; // 0..7
                })
            })
            .collect();
        for i in 80..100 {
            scores.push(make_score(|s| {
                s.path = std::path::PathBuf::from(format!("/src/{i}.ts"));
                s.fan_in = 15 + (i - 80); // 15..34
                s.fan_out = 10 + (i - 80); // 10..29
            }));
        }
        let t = compute_thresholds(&scores);
        assert!(
            t.fan_in_p95 > 5.0,
            "p95 should exceed floor: {}",
            t.fan_in_p95
        );
        assert!(
            t.fan_out_p95 > 8.0,
            "p95 should exceed floor: {}",
            t.fan_out_p95
        );
    }

    #[test]
    fn target_priority_all_zero() {
        let score = make_score(|_| {});
        let t = default_thresholds();
        let priority = compute_target_priority(&score, None, &t);
        assert!((priority).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_max_all_inputs() {
        let score = make_score(|s| {
            s.complexity_density = 2.0; // clamped to 1.0
            s.fan_in = 40; // clamped to 1.0
            s.fan_out = 60; // clamped to 1.0
            s.dead_code_ratio = 1.0;
        });
        let t = default_thresholds();
        let priority = compute_target_priority(&score, Some(100.0), &t);
        assert!((priority - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_complexity_density_weight() {
        let score = make_score(|s| s.complexity_density = 1.0);
        let t = default_thresholds();
        let priority = compute_target_priority(&score, None, &t);
        assert!((priority - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_hotspot_weight() {
        let score = make_score(|_| {});
        let t = default_thresholds();
        let priority = compute_target_priority(&score, Some(100.0), &t);
        assert!((priority - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_dead_code_weight() {
        let score = make_score(|s| s.dead_code_ratio = 1.0);
        let t = default_thresholds();
        let priority = compute_target_priority(&score, None, &t);
        assert!((priority - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_fan_in_weight() {
        let score = make_score(|s| s.fan_in = 20);
        let t = default_thresholds();
        let priority = compute_target_priority(&score, None, &t);
        assert!((priority - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_fan_out_weight() {
        let score = make_score(|s| s.fan_out = 30);
        let t = default_thresholds();
        let priority = compute_target_priority(&score, None, &t);
        assert!((priority - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn target_priority_adapts_to_thresholds() {
        let score = make_score(|s| s.fan_in = 10);
        let t_default = default_thresholds();
        let p1 = compute_target_priority(&score, None, &t_default);
        let t_small = DistributionThresholds {
            fan_in_p95: 10.0,
            ..default_thresholds()
        };
        let p2 = compute_target_priority(&score, None, &t_small);
        assert!(
            p2 > p1,
            "smaller project threshold should yield higher priority"
        );
    }

    #[test]
    fn confidence_mapping() {
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::RemoveDeadCode),
            Confidence::High
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::BreakCircularDependency),
            Confidence::High
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::ExtractComplexFunctions),
            Confidence::High
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::SplitHighImpact),
            Confidence::Medium
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::ExtractDependencies),
            Confidence::Medium
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::UrgentChurnComplexity),
            Confidence::Low
        ));
        assert!(matches!(
            confidence_for_category(&RecommendationCategory::AddTestCoverage),
            Confidence::High
        ));
    }

    #[test]
    fn efficiency_surfaces_quick_wins() {
        let low_effort_priority = 60.0_f64;
        let high_effort_priority = 90.0_f64;
        let low_eff = low_effort_priority / EffortEstimate::Low.numeric();
        let high_eff = high_effort_priority / EffortEstimate::High.numeric();
        assert!(
            low_eff > high_eff,
            "low effort (eff={low_eff}) should rank above high effort (eff={high_eff})"
        );
    }

    #[test]
    fn targets_sorted_by_efficiency_descending() {
        let scores = vec![
            make_score(|s| {
                s.path = std::path::PathBuf::from("/src/big.ts");
                s.complexity_density = 0.8;
                s.fan_in = 25;
                s.lines = 600;
                s.function_count = 20;
                s.dead_code_ratio = 0.6;
            }),
            make_score(|s| {
                s.path = std::path::PathBuf::from("/src/small.ts");
                s.dead_code_ratio = 0.7;
                s.lines = 50;
                s.function_count = 2;
                s.fan_in = 1;
            }),
        ];
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &[
                (std::path::PathBuf::from("/src/big.ts"), 10_usize),
                (std::path::PathBuf::from("/src/small.ts"), 5_usize),
            ]
            .into_iter()
            .collect(),
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
            direct_callers: &rustc_hash::FxHashMap::default(),
            clone_siblings: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _thresholds) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(targets.len() >= 2, "expected at least 2 targets");
        assert!(
            targets[0].efficiency >= targets[1].efficiency,
            "targets should be sorted by efficiency desc: {} >= {}",
            targets[0].efficiency,
            targets[1].efficiency
        );
    }

    #[test]
    fn target_evidence_includes_direct_callers_and_clone_siblings() {
        let path = std::path::PathBuf::from("/src/foo.ts");
        let caller_path = std::path::PathBuf::from("/src/consumer.ts");
        let clone_path = std::path::PathBuf::from("/src/peer.ts");
        let mut direct_callers = rustc_hash::FxHashMap::default();
        direct_callers.insert(
            path.clone(),
            vec![DirectCallerEvidence {
                path: caller_path.clone(),
                symbols: vec![fallow_output::DirectCallerSymbolEvidence {
                    imported: "foo".into(),
                    local: "fooAlias".into(),
                    type_only: false,
                }],
            }],
        );
        let mut clone_siblings = rustc_hash::FxHashMap::default();
        clone_siblings.insert(
            path.clone(),
            vec![CloneSiblingEvidence {
                path: clone_path.clone(),
                start_line: 10,
                end_line: 18,
                fingerprint: "dup:12345678".into(),
            }],
        );

        let evidence = build_evidence(
            &RecommendationCategory::ExtractDependencies,
            &path,
            None,
            EvidenceSources {
                unused_export_names: &rustc_hash::FxHashMap::default(),
                cycle_members: &rustc_hash::FxHashMap::default(),
                direct_callers: &direct_callers,
                clone_siblings: &clone_siblings,
            },
        )
        .expect("generic evidence should keep target evidence present");

        assert_eq!(evidence.direct_callers[0].path, caller_path);
        assert_eq!(evidence.direct_callers[0].symbols[0].local, "fooAlias");
        assert_eq!(evidence.clone_siblings[0].path, clone_path);
        assert_eq!(evidence.clone_siblings[0].fingerprint, "dup:12345678");
    }

    #[test]
    fn clone_sibling_evidence_maps_each_instance_to_peers() {
        let report = duplicates::DuplicationReport {
            clone_groups: vec![duplicates::CloneGroup {
                instances: vec![
                    duplicates::CloneInstance {
                        file: "/src/a.ts".into(),
                        start_line: 1,
                        end_line: 5,
                        start_col: 0,
                        end_col: 1,
                        fragment: "const value = call();".into(),
                    },
                    duplicates::CloneInstance {
                        file: "/src/b.ts".into(),
                        start_line: 20,
                        end_line: 24,
                        start_col: 0,
                        end_col: 1,
                        fragment: "const value = call();".into(),
                    },
                ],
                token_count: 8,
                line_count: 5,
            }],
            ..Default::default()
        };

        let evidence = build_clone_sibling_evidence(&report);
        let a_siblings = evidence
            .get(&std::path::PathBuf::from("/src/a.ts"))
            .expect("a.ts should have sibling evidence");
        let b_siblings = evidence
            .get(&std::path::PathBuf::from("/src/b.ts"))
            .expect("b.ts should have sibling evidence");

        assert_eq!(a_siblings[0].path, std::path::PathBuf::from("/src/b.ts"));
        assert_eq!(a_siblings[0].start_line, 20);
        assert_eq!(b_siblings[0].path, std::path::PathBuf::from("/src/a.ts"));
        assert!(a_siblings[0].fingerprint.starts_with("dup:"));
    }

    #[test]
    fn rule_no_match_clean_file() {
        let score = make_score(|_| {});
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: false,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_none());
    }

    #[test]
    fn rule_circular_dep_high_fan_in() {
        let score = make_score(|s| s.fan_in = 5);
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: true,
                is_entry: false,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(matches!(
            cat,
            RecommendationCategory::BreakCircularDependency
        ));
    }

    #[test]
    fn rule_circular_dep_low_fan_in_fallback() {
        let score = make_score(|s| s.fan_in = 1);
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: true,
                is_entry: false,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(matches!(
            cat,
            RecommendationCategory::BreakCircularDependency
        ));
    }

    #[test]
    fn rule_add_test_coverage() {
        let score = make_score(|s| {
            s.crap_above_threshold = 2;
            s.crap_max = 72.0;
            s.complexity_density = 0.5;
        });
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: false,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, rec) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::AddTestCoverage));
        assert!(rec.contains("2 complex functions"));
    }

    #[test]
    fn rule_add_test_coverage_below_density_threshold() {
        let score = make_score(|s| {
            s.crap_above_threshold = 3;
            s.crap_max = 72.0;
            s.complexity_density = 0.2;
        });
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: false,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_none());
    }

    #[test]
    fn rule_split_high_impact() {
        let score = make_score(|s| {
            s.complexity_density = 0.5;
            s.fan_in = 20;
        });
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: false,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, rec) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::SplitHighImpact));
        assert!(
            rec.contains("100 LOC"),
            "recommendation should include LOC: {rec}"
        );
    }

    #[test]
    fn rule_remove_dead_code() {
        let score = make_score(|s| s.dead_code_ratio = 0.6);
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: false,
                top_fns: None,
                value_exports: 5,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::RemoveDeadCode));
    }

    #[test]
    fn rule_dead_code_gate_too_few_exports() {
        let score = make_score(|s| s.dead_code_ratio = 0.8);
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: false,
                top_fns: None,
                value_exports: 2,
            },
            &t,
        );
        assert!(result.is_none());
    }

    #[test]
    fn rule_extract_complex_functions() {
        let score = make_score(|_| {});
        let fns = vec![("handleSubmit".to_string(), 10u32, 35u16)];
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: false,
                top_fns: Some(&fns),
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, rec) = result.unwrap();
        assert!(matches!(
            cat,
            RecommendationCategory::ExtractComplexFunctions
        ));
        assert!(rec.contains("handleSubmit"));
        assert!(
            rec.contains("100-LOC"),
            "recommendation should include LOC: {rec}"
        );
    }

    #[test]
    fn rule_extract_dependencies_not_entry() {
        let score = make_score(|s| {
            s.fan_out = 20;
            s.maintainability_index = 50.0;
        });
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: false,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, rec) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::ExtractDependencies));
        assert!(
            rec.contains("100-LOC"),
            "recommendation should include LOC: {rec}"
        );
    }

    #[test]
    fn rule_extract_dependencies_skipped_for_entry() {
        let score = make_score(|s| {
            s.fan_out = 20;
            s.maintainability_index = 50.0;
        });
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: true,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_none());
    }

    #[test]
    fn rule_urgent_churn_complexity() {
        let score = make_score(|s| s.complexity_density = 0.8);
        let hotspot = HotspotEntry {
            path: std::path::PathBuf::from("/src/foo.ts"),
            score: 60.0,
            commits: 20,
            weighted_commits: 15.0,
            lines_added: 500,
            lines_deleted: 100,
            complexity_density: 0.8,
            fan_in: 5,
            trend: crate::ChurnTrend::Accelerating,
            ownership: None,
            is_test_path: false,
        };
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: Some(&hotspot),
                is_circular: false,
                is_entry: false,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::UrgentChurnComplexity));
    }

    #[test]
    fn effort_high_for_large_file() {
        let score = make_score(|s| s.lines = 600);
        let t = default_thresholds();
        assert!(matches!(
            compute_effort_estimate(&score, &t),
            EffortEstimate::High
        ));
    }

    #[test]
    fn effort_high_for_high_fan_in() {
        let score = make_score(|s| s.fan_in = 25);
        let t = default_thresholds();
        assert!(matches!(
            compute_effort_estimate(&score, &t),
            EffortEstimate::High
        ));
    }

    #[test]
    fn effort_high_for_many_complex_functions() {
        let score = make_score(|s| {
            s.function_count = 20;
            s.complexity_density = 0.6;
        });
        let t = default_thresholds();
        assert!(matches!(
            compute_effort_estimate(&score, &t),
            EffortEstimate::High
        ));
    }

    #[test]
    fn effort_low_for_small_simple_file() {
        let score = make_score(|s| {
            s.lines = 50;
            s.function_count = 2;
            s.fan_in = 1;
        });
        let t = default_thresholds();
        assert!(matches!(
            compute_effort_estimate(&score, &t),
            EffortEstimate::Low
        ));
    }

    #[test]
    fn effort_medium_for_moderate_file() {
        let score = make_score(|s| {
            s.lines = 200;
            s.function_count = 8;
            s.fan_in = 3;
        });
        let t = default_thresholds();
        assert!(matches!(
            compute_effort_estimate(&score, &t),
            EffortEstimate::Medium
        ));
    }

    #[test]
    fn evidence_dead_code_includes_unused_exports() {
        let mut unused = rustc_hash::FxHashMap::default();
        unused.insert(
            std::path::PathBuf::from("/src/foo.ts"),
            vec!["bar".to_string(), "baz".to_string()],
        );
        let cycle_members = rustc_hash::FxHashMap::default();
        let ev = build_evidence(
            &RecommendationCategory::RemoveDeadCode,
            std::path::Path::new("/src/foo.ts"),
            None,
            EvidenceSources {
                unused_export_names: &unused,
                cycle_members: &cycle_members,
                direct_callers: &rustc_hash::FxHashMap::default(),
                clone_siblings: &rustc_hash::FxHashMap::default(),
            },
        );
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert_eq!(ev.unused_exports, vec!["bar", "baz"]);
        assert!(ev.complex_functions.is_empty());
        assert!(ev.cycle_path.is_empty());
    }

    #[test]
    fn evidence_dead_code_none_when_no_exports() {
        let unused = rustc_hash::FxHashMap::default();
        let cycle_members = rustc_hash::FxHashMap::default();
        let ev = build_evidence(
            &RecommendationCategory::RemoveDeadCode,
            std::path::Path::new("/src/foo.ts"),
            None,
            EvidenceSources {
                unused_export_names: &unused,
                cycle_members: &cycle_members,
                direct_callers: &rustc_hash::FxHashMap::default(),
                clone_siblings: &rustc_hash::FxHashMap::default(),
            },
        );
        assert!(ev.is_none());
    }

    #[test]
    fn evidence_extract_complex_functions() {
        let unused = rustc_hash::FxHashMap::default();
        let cycle_members = rustc_hash::FxHashMap::default();
        let fns = vec![
            ("processData".to_string(), 10u32, 40u16),
            ("handleEvent".to_string(), 25u32, 35u16),
            ("simpleHelper".to_string(), 50u32, 5u16),
        ];
        let ev = build_evidence(
            &RecommendationCategory::ExtractComplexFunctions,
            std::path::Path::new("/src/foo.ts"),
            Some(&fns),
            EvidenceSources {
                unused_export_names: &unused,
                cycle_members: &cycle_members,
                direct_callers: &rustc_hash::FxHashMap::default(),
                clone_siblings: &rustc_hash::FxHashMap::default(),
            },
        );
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert!(ev.unused_exports.is_empty());
        assert_eq!(ev.complex_functions.len(), 2);
        assert_eq!(ev.complex_functions[0].name, "processData");
        assert_eq!(ev.complex_functions[1].name, "handleEvent");
    }

    #[test]
    fn evidence_break_circular_dep() {
        let unused = rustc_hash::FxHashMap::default();
        let mut cycle_members = rustc_hash::FxHashMap::default();
        cycle_members.insert(
            std::path::PathBuf::from("/src/a.ts"),
            vec![
                std::path::PathBuf::from("/src/b.ts"),
                std::path::PathBuf::from("/src/c.ts"),
            ],
        );
        let ev = build_evidence(
            &RecommendationCategory::BreakCircularDependency,
            std::path::Path::new("/src/a.ts"),
            None,
            EvidenceSources {
                unused_export_names: &unused,
                cycle_members: &cycle_members,
                direct_callers: &rustc_hash::FxHashMap::default(),
                clone_siblings: &rustc_hash::FxHashMap::default(),
            },
        );
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert_eq!(ev.cycle_path.len(), 2);
        assert!(ev.unused_exports.is_empty());
    }

    #[test]
    fn evidence_add_test_coverage_includes_all_fns() {
        let unused = rustc_hash::FxHashMap::default();
        let cycle_members = rustc_hash::FxHashMap::default();
        let fns = vec![("render".to_string(), 5u32, 12u16)];
        let ev = build_evidence(
            &RecommendationCategory::AddTestCoverage,
            std::path::Path::new("/src/foo.ts"),
            Some(&fns),
            EvidenceSources {
                unused_export_names: &unused,
                cycle_members: &cycle_members,
                direct_callers: &rustc_hash::FxHashMap::default(),
                clone_siblings: &rustc_hash::FxHashMap::default(),
            },
        );
        assert!(ev.is_some());
        let ev = ev.unwrap();
        assert_eq!(ev.complex_functions.len(), 1);
        assert_eq!(ev.complex_functions[0].name, "render");
    }

    #[test]
    fn evidence_split_high_impact_returns_none() {
        let unused = rustc_hash::FxHashMap::default();
        let cycle_members = rustc_hash::FxHashMap::default();
        let ev = build_evidence(
            &RecommendationCategory::SplitHighImpact,
            std::path::Path::new("/src/foo.ts"),
            None,
            EvidenceSources {
                unused_export_names: &unused,
                cycle_members: &cycle_members,
                direct_callers: &rustc_hash::FxHashMap::default(),
                clone_siblings: &rustc_hash::FxHashMap::default(),
            },
        );
        assert!(ev.is_none());
    }

    #[test]
    fn percentile_empty_returns_zero() {
        assert!((percentile_usize(&[], 0.5)).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_single_element() {
        assert!((percentile_usize(&[42], 0.5) - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_p50_median() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let p50 = percentile_usize(&data, 0.50);
        assert!((p50 - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rule_urgent_churn_overrides_circular_dep() {
        let score = make_score(|s| {
            s.complexity_density = 0.8;
            s.fan_in = 10;
        });
        let hotspot = HotspotEntry {
            path: std::path::PathBuf::from("/src/foo.ts"),
            score: 60.0,
            commits: 20,
            weighted_commits: 15.0,
            lines_added: 500,
            lines_deleted: 100,
            complexity_density: 0.8,
            fan_in: 10,
            trend: crate::ChurnTrend::Accelerating,
            ownership: None,
            is_test_path: false,
        };
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: Some(&hotspot),
                is_circular: true,
                is_entry: false,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(
            matches!(cat, RecommendationCategory::UrgentChurnComplexity),
            "Rule 1 should win over Rule 2"
        );
    }

    #[test]
    fn rule_extract_two_complex_functions() {
        let score = make_score(|_| {});
        let fns = vec![
            ("processData".to_string(), 10u32, 40u16),
            ("handleEvent".to_string(), 25u32, 35u16),
        ];
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: false,
                top_fns: Some(&fns),
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, rec) = result.unwrap();
        assert!(matches!(
            cat,
            RecommendationCategory::ExtractComplexFunctions
        ));
        assert!(rec.contains("processData"));
        assert!(rec.contains("handleEvent"));
        assert!(
            rec.contains("100-LOC"),
            "two-function recommendation should include LOC: {rec}"
        );
    }

    #[test]
    fn contributing_factor_hotspot() {
        let scores = vec![make_score(|s| {
            s.complexity_density = 0.5;
            s.fan_in = 15;
        })];
        let hotspots = vec![HotspotEntry {
            path: std::path::PathBuf::from("/src/foo.ts"),
            score: 45.0,
            commits: 10,
            weighted_commits: 8.0,
            lines_added: 200,
            lines_deleted: 50,
            complexity_density: 0.5,
            fan_in: 15,
            trend: crate::ChurnTrend::Stable,
            ownership: None,
            is_test_path: false,
        }];
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
            direct_callers: &rustc_hash::FxHashMap::default(),
            clone_siblings: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &hotspots);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(target.factors.iter().any(|f| f.metric == "hotspot_score"));
    }

    #[test]
    fn contributing_factor_crap() {
        let scores = vec![make_score(|s| {
            s.complexity_density = 0.5;
            s.crap_above_threshold = 3;
            s.crap_max = 72.0;
        })];
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
            direct_callers: &rustc_hash::FxHashMap::default(),
            clone_siblings: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(target.factors.iter().any(|f| f.metric == "crap_max"));
    }

    #[test]
    fn contributing_factor_circular_dependency() {
        let mut circular = rustc_hash::FxHashSet::default();
        circular.insert(std::path::PathBuf::from("/src/foo.ts"));
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let scores = vec![make_score(|s| s.complexity_density = 0.1)];
        let aux = TargetAuxData {
            circular_files: &circular,
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
            direct_callers: &rustc_hash::FxHashMap::default(),
            clone_siblings: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(
            target
                .factors
                .iter()
                .any(|f| f.metric == "circular_dependency")
        );
    }

    #[test]
    fn contributing_factor_dead_code_with_value_exports() {
        let mut value_exports = rustc_hash::FxHashMap::default();
        value_exports.insert(std::path::PathBuf::from("/src/foo.ts"), 6_usize);
        let scores = vec![make_score(|s| s.dead_code_ratio = 0.7)];
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
            direct_callers: &rustc_hash::FxHashMap::default(),
            clone_siblings: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(target.factors.iter().any(|f| f.metric == "dead_code_ratio"));
    }

    #[test]
    fn contributing_factor_fan_out() {
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let scores = vec![make_score(|s| {
            s.fan_out = 20;
            s.maintainability_index = 50.0;
        })];
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
            direct_callers: &rustc_hash::FxHashMap::default(),
            clone_siblings: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(target.factors.iter().any(|f| f.metric == "fan_out"));
    }

    #[test]
    fn contributing_factor_cognitive_complexity() {
        let mut top_fns = rustc_hash::FxHashMap::default();
        top_fns.insert(
            std::path::PathBuf::from("/src/foo.ts"),
            vec![("complexFn".to_string(), 10u32, 30u16)],
        );
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let scores = vec![make_score(|_| {})];
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &top_fns,
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
            direct_callers: &rustc_hash::FxHashMap::default(),
            clone_siblings: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(!targets.is_empty());
        let target = &targets[0];
        assert!(
            target
                .factors
                .iter()
                .any(|f| f.metric == "cognitive_complexity")
        );
    }

    #[test]
    fn no_targets_for_clean_files() {
        let scores = vec![make_score(|s| {
            s.path = std::path::PathBuf::from("/src/clean.ts");
            s.complexity_density = 0.1;
            s.fan_in = 2;
            s.fan_out = 3;
            s.dead_code_ratio = 0.0;
        })];
        let value_exports: rustc_hash::FxHashMap<std::path::PathBuf, usize> =
            rustc_hash::FxHashMap::default();
        let aux = TargetAuxData {
            circular_files: &rustc_hash::FxHashSet::default(),
            top_complex_fns: &rustc_hash::FxHashMap::default(),
            entry_points: &rustc_hash::FxHashSet::default(),
            value_export_counts: &value_exports,
            unused_export_names: &rustc_hash::FxHashMap::default(),
            cycle_members: &rustc_hash::FxHashMap::default(),
            direct_callers: &rustc_hash::FxHashMap::default(),
            clone_siblings: &rustc_hash::FxHashMap::default(),
        };
        let (targets, _) = compute_refactoring_targets(&scores, &aux, &[]);
        assert!(targets.is_empty());
    }

    #[test]
    fn rule_split_high_impact_moderate_fan_in_many_functions() {
        let score = make_score(|s| {
            s.complexity_density = 0.5;
            s.fan_in = 10; // equals p75 in default thresholds
            s.function_count = 8;
        });
        let t = default_thresholds();
        let result = try_match_rules(
            RuleMatchContext {
                score: &score,
                hotspot: None,
                is_circular: false,
                is_entry: false,
                top_fns: None,
                value_exports: 0,
            },
            &t,
        );
        assert!(result.is_some());
        let (cat, _) = result.unwrap();
        assert!(matches!(cat, RecommendationCategory::SplitHighImpact));
    }
}
