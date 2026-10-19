//! New-only attribution of complexity findings.
//!
//! A complexity finding is identified by its path and function name. The
//! exceeded category and the line are not part of the identity, so a line
//! shift or a category change keeps the match with the base finding. The
//! metric values then decide the attribution: a head finding is introduced
//! when no base finding matches it, or when a metric that the head finding
//! exceeds has a higher value than in its matched base finding. Unchanged and
//! decreased metrics stay inherited, also when the exceeded category changes
//! (#3277).
//!
//! A matched finding that is not introduced is worsened when a measured value
//! (cyclomatic, cognitive, CRAP or line count) is strictly higher than in its
//! matched base finding. The `audit.worsened` setting decides how a worsened
//! finding affects the verdict.

use std::path::Path;

use fallow_output::{AuditFindingAttribution, WorsenedMetric, WorsenedMetricKind};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::audit_keys::{health_finding_key, remap_key_for_renames};

/// Metric values of one base complexity finding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComplexityMetrics {
    /// 1-based line of the function. Only orders same-named findings.
    pub line: u32,
    /// Cyclomatic complexity.
    pub cyclomatic: u16,
    /// Cognitive complexity.
    pub cognitive: u16,
    /// CRAP score, when coverage data exists.
    pub crap: Option<f64>,
    /// Lines of code in the function body.
    pub line_count: u32,
}

impl ComplexityMetrics {
    /// The metric values of one finding.
    #[must_use]
    pub const fn of(finding: &fallow_output::ComplexityViolation) -> Self {
        Self {
            line: finding.line,
            cyclomatic: finding.cyclomatic,
            cognitive: finding.cognitive,
            crap: finding.crap,
            line_count: finding.line_count,
        }
    }

    /// Whether both findings have the same complexity values. The line and
    /// the line count are not compared.
    fn same_values(&self, other: &Self) -> bool {
        self.cyclomatic == other.cyclomatic
            && self.cognitive == other.cognitive
            && self.crap.map(f64::to_bits) == other.crap.map(f64::to_bits)
    }
}

/// Attribution of one head complexity finding against the base findings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComplexityClass {
    /// `true` when no base finding matches, or when a metric that the finding
    /// exceeds grew (the new-only gate rule since #3277).
    pub introduced: bool,
    /// The measured metrics that are higher than in the matched base finding.
    /// Empty when no base finding matches or when no metric grew.
    pub worsened_metrics: Vec<WorsenedMetric>,
}

impl ComplexityClass {
    /// `true` when a base finding matches, the finding is not introduced, and
    /// a measured metric grew.
    #[must_use]
    pub fn is_worsened(&self) -> bool {
        !self.introduced && !self.worsened_metrics.is_empty()
    }

    /// The audit-mode class of the finding.
    #[must_use]
    pub fn attribution(&self) -> AuditFindingAttribution {
        if self.introduced {
            AuditFindingAttribution::Introduced
        } else if self.worsened_metrics.is_empty() {
            AuditFindingAttribution::Inherited
        } else {
            AuditFindingAttribution::Worsened
        }
    }

    fn of_match(
        head: &fallow_output::ComplexityViolation,
        base: Option<&ComplexityMetrics>,
    ) -> Self {
        base.map_or(
            Self {
                introduced: true,
                worsened_metrics: Vec::new(),
            },
            |base| Self {
                introduced: exceeded_metric_increased(head, base),
                worsened_metrics: grown_metrics(head, base),
            },
        )
    }
}

/// The measured metrics of `head` that are strictly higher than in `base`.
///
/// CRAP compares only when both sides have a score, because a score that
/// only one side has reflects the coverage input, not the code.
fn grown_metrics(
    head: &fallow_output::ComplexityViolation,
    base: &ComplexityMetrics,
) -> Vec<WorsenedMetric> {
    let mut grown = Vec::new();
    let mut push = |metric, base: f64, head: f64| {
        if head > base {
            grown.push(WorsenedMetric { metric, base, head });
        }
    };
    push(
        WorsenedMetricKind::Cyclomatic,
        f64::from(base.cyclomatic),
        f64::from(head.cyclomatic),
    );
    push(
        WorsenedMetricKind::Cognitive,
        f64::from(base.cognitive),
        f64::from(head.cognitive),
    );
    if let (Some(base_crap), Some(head_crap)) = (base.crap, head.crap) {
        push(WorsenedMetricKind::Crap, base_crap, head_crap);
    }
    push(
        WorsenedMetricKind::LineCount,
        f64::from(base.line_count),
        f64::from(head.line_count),
    );
    grown
}

/// Base complexity findings by identity key (`complexity:<path>:<function>`),
/// each list sorted by line.
pub type ComplexityBaseline = FxHashMap<String, Vec<ComplexityMetrics>>;

/// The base complexity findings of a health report, by identity key.
#[must_use]
pub fn complexity_baseline(
    report: &fallow_output::HealthReport,
    root: &Path,
) -> ComplexityBaseline {
    let mut baseline = ComplexityBaseline::default();
    for finding in &report.findings {
        baseline
            .entry(health_finding_key(finding, root))
            .or_default()
            .push(ComplexityMetrics::of(finding));
    }
    sort_by_line(&mut baseline);
    baseline
}

/// The identity keys of a complexity baseline.
#[must_use]
pub fn complexity_baseline_keys(baseline: &ComplexityBaseline) -> FxHashSet<String> {
    baseline.keys().cloned().collect()
}

/// Relocate the base findings of renamed files onto their head paths.
///
/// `renames` maps base-relative old paths to head-relative new paths, as in
/// [`crate::audit_keys::remap_keys_for_renames`]. When two keys merge, their findings join
/// one list, sorted by line.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashMap across audit attribution keys"
)]
pub fn remap_complexity_baseline_for_renames(
    baseline: ComplexityBaseline,
    renames: &FxHashMap<String, String>,
) -> ComplexityBaseline {
    let mut remapped = ComplexityBaseline::default();
    for (key, findings) in baseline {
        remapped
            .entry(remap_key_for_renames(&key, renames))
            .or_default()
            .extend(findings);
    }
    sort_by_line(&mut remapped);
    remapped
}

fn sort_by_line(baseline: &mut ComplexityBaseline) {
    for findings in baseline.values_mut() {
        findings.sort_by_key(|metrics| metrics.line);
    }
}

/// Whether `head` is worse than `base` in a metric that `head` exceeds.
///
/// A CRAP score that the base finding does not have counts as an increase.
fn exceeded_metric_increased(
    head: &fallow_output::ComplexityViolation,
    base: &ComplexityMetrics,
) -> bool {
    let exceeded = head.exceeded;
    (exceeded.includes_cyclomatic() && head.cyclomatic > base.cyclomatic)
        || (exceeded.includes_cognitive() && head.cognitive > base.cognitive)
        || (exceeded.includes_crap()
            && match (head.crap, base.crap) {
                (Some(head), Some(base)) => head > base,
                (Some(_), None) => true,
                (None, _) => false,
            })
}

/// Classify each head complexity finding as introduced (`true`) or inherited
/// (`false`), in report order. See [`classify_complexity_attribution`].
#[must_use]
pub fn classify_complexity_findings(
    findings: &[fallow_output::HealthFinding],
    root: &Path,
    base: &ComplexityBaseline,
) -> Vec<bool> {
    classify_complexity_attribution(findings, root, base)
        .into_iter()
        .map(|class| class.introduced)
        .collect()
}

/// Classify each head complexity finding against one base, in report order.
///
/// Head and base findings match by identity key. When one key holds several
/// findings on one side (same-named class methods or anonymous functions in
/// one file), the match is ambiguous. This conservative rule then applies, per
/// key:
///
/// 1. A head finding with the same complexity values as an unmatched base
///    finding takes that base finding and is inherited. This keeps unchanged
///    findings inherited when a same-named finding is added, removed or
///    edited. Among such base findings, one with the same line count wins.
/// 2. The remaining head findings, in line order, take the remaining base
///    findings, in line order. Each pair compares the exceeded metrics.
/// 3. A head finding without a base finding is introduced.
///
/// Every base finding matches at most one head finding, so a new same-named
/// finding cannot hide behind an existing one. The rule does not identify
/// functions beyond path and name (see #2010). Two same-named findings that
/// swap their metric values in one change stay inherited.
///
/// Each matched pair also lists the measured metrics that grew, so an
/// inherited finding that got worse is visible.
#[must_use]
pub fn classify_complexity_attribution(
    findings: &[fallow_output::HealthFinding],
    root: &Path,
    base: &ComplexityBaseline,
) -> Vec<ComplexityClass> {
    let mut groups: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (index, finding) in findings.iter().enumerate() {
        groups
            .entry(health_finding_key(finding, root))
            .or_default()
            .push(index);
    }
    let mut matches: Vec<Option<&ComplexityMetrics>> = vec![None; findings.len()];
    for (key, mut heads) in groups {
        let Some(bases) = base.get(&key) else {
            continue;
        };
        heads.sort_by_key(|&index| (findings[index].line, findings[index].col));
        let mut unmatched_bases: Vec<Option<&ComplexityMetrics>> = bases.iter().map(Some).collect();
        let mut unmatched_heads = Vec::new();
        for index in heads {
            let head = ComplexityMetrics::of(&findings[index]);
            let same_values =
                |slot: Option<&ComplexityMetrics>| slot.is_some_and(|base| base.same_values(&head));
            let same_lines = unmatched_bases.iter().position(|&slot| {
                same_values(slot) && slot.is_some_and(|base| base.line_count == head.line_count)
            });
            let same =
                same_lines.or_else(|| unmatched_bases.iter().position(|&slot| same_values(slot)));
            match same {
                Some(position) => matches[index] = unmatched_bases[position].take(),
                None => unmatched_heads.push(index),
            }
        }
        let mut remaining_bases = unmatched_bases.into_iter().flatten();
        for index in unmatched_heads {
            matches[index] = remaining_bases.next();
        }
    }
    findings
        .iter()
        .zip(matches)
        .map(|(finding, base)| ComplexityClass::of_match(finding, base))
        .collect()
}

/// Classify each head complexity finding against several bases, in report
/// order. A finding is introduced only when it is introduced against every
/// base, so a finding that one base already has is inherited.
#[must_use]
pub fn classify_complexity_findings_against_all<'a>(
    findings: &[fallow_output::HealthFinding],
    root: &Path,
    bases: impl IntoIterator<Item = &'a ComplexityBaseline>,
) -> Vec<bool> {
    classify_complexity_attribution_against_all(findings, root, bases)
        .into_iter()
        .map(|class| class.introduced)
        .collect()
}

/// Classify each head complexity finding against several bases, in report
/// order.
///
/// A finding is introduced only when it is introduced against every base. A
/// finding that is not introduced takes the smallest list of grown metrics of
/// the bases that it is not introduced against, so a finding that one base
/// already has with the same values is inherited, not worsened. An introduced
/// finding keeps the grown metrics of the first base that matches it.
#[must_use]
pub fn classify_complexity_attribution_against_all<'a>(
    findings: &[fallow_output::HealthFinding],
    root: &Path,
    bases: impl IntoIterator<Item = &'a ComplexityBaseline>,
) -> Vec<ComplexityClass> {
    let per_base: Vec<Vec<ComplexityClass>> = bases
        .into_iter()
        .map(|base| classify_complexity_attribution(findings, root, base))
        .collect();
    (0..findings.len())
        .map(|index| {
            let classes = per_base.iter().map(|classes| &classes[index]);
            let inherited = classes
                .clone()
                .filter(|class| !class.introduced)
                .min_by_key(|class| class.worsened_metrics.len());
            inherited.cloned().unwrap_or_else(|| ComplexityClass {
                introduced: true,
                worsened_metrics: classes
                    .map(|class| &class.worsened_metrics)
                    .find(|metrics| !metrics.is_empty())
                    .cloned()
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// The ledger key of each complexity finding, in report order.
///
/// A repeated identity key gets an occurrence suffix, so each finding counts
/// once in the attribution counts. Without the suffix, same-named findings
/// collapse to one count.
#[must_use]
pub fn complexity_ledger_keys(
    findings: &[fallow_output::HealthFinding],
    root: &Path,
) -> Vec<String> {
    let mut seen: FxHashMap<String, usize> = FxHashMap::default();
    findings
        .iter()
        .map(|finding| {
            let key = health_finding_key(finding, root);
            let occurrence = seen.entry(key.clone()).or_default();
            *occurrence += 1;
            if *occurrence == 1 {
                key
            } else {
                format!("{key}#{occurrence}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_output::{
        ComplexityViolation, ExceededThreshold, FindingSeverity, HealthFinding, HealthReport,
    };

    use super::*;

    const MAX: u16 = 15;

    fn root() -> PathBuf {
        PathBuf::from("/project")
    }

    fn finding(name: &str, line: u32, cyclomatic: u16, cognitive: u16) -> HealthFinding {
        HealthFinding::from(ComplexityViolation {
            path: root().join("src/index.js"),
            name: name.to_string(),
            line,
            col: 0,
            cyclomatic,
            cognitive,
            line_count: 30,
            param_count: 1,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: ExceededThreshold::from_bools(cyclomatic > MAX, cognitive > MAX, false),
            effective_severity: None,
            severity: FindingSeverity::High,
            crap: None,
            mutation: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
        })
    }

    fn baseline(findings: Vec<HealthFinding>) -> ComplexityBaseline {
        complexity_baseline(
            &HealthReport {
                findings,
                ..HealthReport::default()
            },
            &root(),
        )
    }

    fn classify(base: Vec<HealthFinding>, head: &[HealthFinding]) -> Vec<bool> {
        classify_complexity_findings(head, &root(), &baseline(base))
    }

    #[test]
    fn identity_key_excludes_category_and_line() {
        let key = health_finding_key(&finding("hotspot", 4, 17, 16), &root());
        assert_eq!(key, "complexity:src/index.js:hotspot");
    }

    #[test]
    fn issue_cases() {
        // Unchanged.
        assert_eq!(
            classify(vec![finding("f", 1, 17, 16)], &[finding("f", 1, 17, 16)]),
            [false]
        );
        // Increased.
        assert_eq!(
            classify(vec![finding("f", 1, 17, 16)], &[finding("f", 1, 18, 17)]),
            [true]
        );
        // Decreased, same category.
        assert_eq!(
            classify(vec![finding("f", 1, 18, 17)], &[finding("f", 1, 17, 16)]),
            [false]
        );
        // Decreased, category change from both to cyclomatic.
        assert_eq!(
            classify(vec![finding("f", 1, 17, 16)], &[finding("f", 1, 16, 15)]),
            [false]
        );
        // New threshold crossing.
        assert_eq!(classify(vec![], &[finding("f", 1, 17, 16)]), [true]);
    }

    #[test]
    fn only_exceeded_metrics_can_introduce() {
        // Cognitive grows but stays under the limit: only cyclomatic is
        // exceeded, and it did not grow.
        assert_eq!(
            classify(vec![finding("f", 1, 20, 5)], &[finding("f", 1, 20, 9)]),
            [false]
        );
        // A category change to a newly exceeded metric that grew.
        assert_eq!(
            classify(vec![finding("f", 1, 20, 14)], &[finding("f", 1, 19, 16)]),
            [true]
        );
    }

    #[test]
    fn a_finding_that_any_base_has_is_inherited() {
        let primary = baseline(vec![]);
        let merged = baseline(vec![finding("f", 1, 17, 16)]);
        let head = [finding("f", 1, 17, 16), finding("g", 9, 17, 16)];
        assert_eq!(
            classify_complexity_findings_against_all(&head, &root(), [&primary, &merged]),
            [false, true]
        );
        // A finding that grew against every base stays introduced.
        assert_eq!(
            classify_complexity_findings_against_all(
                &[finding("f", 1, 18, 17)],
                &root(),
                [&baseline(vec![finding("f", 1, 17, 16)]), &merged]
            ),
            [true]
        );
    }

    #[test]
    fn line_shift_keeps_the_match() {
        assert_eq!(
            classify(vec![finding("f", 1, 17, 16)], &[finding("f", 40, 17, 16)]),
            [false]
        );
    }

    #[test]
    fn crap_without_base_value_counts_as_increase() {
        let mut head = finding("f", 1, 16, 5);
        head.violation.exceeded = ExceededThreshold::Crap;
        head.violation.crap = Some(50.0);
        let mut base = finding("f", 1, 16, 5);
        base.violation.exceeded = ExceededThreshold::Crap;
        assert_eq!(classify(vec![base.clone()], &[head.clone()]), [true]);
        base.violation.crap = Some(60.0);
        assert_eq!(classify(vec![base], &[head]), [false]);
    }

    #[test]
    fn same_named_findings_match_unchanged_values_first() {
        let base = vec![finding("run", 1, 17, 16), finding("run", 30, 21, 20)];
        // Unchanged.
        assert_eq!(
            classify(
                base.clone(),
                &[finding("run", 1, 17, 16), finding("run", 30, 21, 20)]
            ),
            [false, false]
        );
        // First one removed: the second one keeps its own base finding.
        assert_eq!(
            classify(base.clone(), &[finding("run", 1, 21, 20)]),
            [false]
        );
        // A new one added before both.
        assert_eq!(
            classify(
                base.clone(),
                &[
                    finding("run", 1, 30, 30),
                    finding("run", 20, 17, 16),
                    finding("run", 50, 21, 20)
                ]
            ),
            [true, false, false]
        );
        // The first one worsened, the second one unchanged.
        assert_eq!(
            classify(
                base,
                &[finding("run", 1, 18, 17), finding("run", 30, 21, 20)]
            ),
            [true, false]
        );
    }

    fn with_lines(mut finding: HealthFinding, line_count: u32) -> HealthFinding {
        finding.violation.line_count = line_count;
        finding
    }

    fn attribution(base: Vec<HealthFinding>, head: &[HealthFinding]) -> Vec<ComplexityClass> {
        classify_complexity_attribution(head, &root(), &baseline(base))
    }

    fn metric(kind: WorsenedMetricKind, base: f64, head: f64) -> WorsenedMetric {
        WorsenedMetric {
            metric: kind,
            base,
            head,
        }
    }

    #[test]
    fn grown_line_count_is_worsened_not_introduced() {
        let classes = attribution(
            vec![with_lines(finding("f", 1, 17, 16), 80)],
            &[with_lines(finding("f", 1, 17, 16), 300)],
        );
        assert!(!classes[0].introduced);
        assert!(classes[0].is_worsened());
        assert_eq!(
            classes[0].worsened_metrics,
            [metric(WorsenedMetricKind::LineCount, 80.0, 300.0)]
        );
        assert_eq!(classes[0].attribution(), AuditFindingAttribution::Worsened);
    }

    #[test]
    fn grown_metric_that_the_finding_does_not_exceed_is_worsened() {
        // Only cyclomatic is exceeded; cognitive grows below the limit.
        let classes = attribution(vec![finding("f", 1, 20, 5)], &[finding("f", 1, 20, 9)]);
        assert_eq!(classes[0].attribution(), AuditFindingAttribution::Worsened);
        assert_eq!(
            classes[0].worsened_metrics,
            [metric(WorsenedMetricKind::Cognitive, 5.0, 9.0)]
        );
    }

    #[test]
    fn grown_exceeded_metric_is_introduced_with_values() {
        let classes = attribution(vec![finding("f", 1, 17, 16)], &[finding("f", 1, 40, 16)]);
        assert_eq!(
            classes[0].attribution(),
            AuditFindingAttribution::Introduced
        );
        assert_eq!(
            classes[0].worsened_metrics,
            [metric(WorsenedMetricKind::Cyclomatic, 17.0, 40.0)]
        );
        assert!(!classes[0].is_worsened());
    }

    #[test]
    fn improved_and_moved_functions_are_inherited() {
        let improved = attribution(
            vec![with_lines(finding("f", 1, 30, 30), 90)],
            &[with_lines(finding("f", 1, 20, 18), 60)],
        );
        assert_eq!(
            improved[0].attribution(),
            AuditFindingAttribution::Inherited
        );
        assert!(improved[0].worsened_metrics.is_empty());
        let moved = attribution(vec![finding("f", 1, 17, 16)], &[finding("f", 400, 17, 16)]);
        assert_eq!(moved[0].attribution(), AuditFindingAttribution::Inherited);
    }

    #[test]
    fn new_function_is_introduced_without_values() {
        let classes = attribution(vec![finding("f", 1, 17, 16)], &[finding("g", 1, 17, 16)]);
        assert_eq!(
            classes[0].attribution(),
            AuditFindingAttribution::Introduced
        );
        assert!(classes[0].worsened_metrics.is_empty());
    }

    #[test]
    fn crap_compares_only_when_both_sides_have_a_score() {
        let mut head = finding("f", 1, 20, 5);
        head.violation.crap = Some(50.0);
        let classes = attribution(vec![finding("f", 1, 20, 5)], std::slice::from_ref(&head));
        assert_eq!(classes[0].attribution(), AuditFindingAttribution::Inherited);
        let mut base = finding("f", 1, 20, 5);
        base.violation.crap = Some(40.0);
        let classes = attribution(vec![base], &[head]);
        assert_eq!(
            classes[0].worsened_metrics,
            [metric(WorsenedMetricKind::Crap, 40.0, 50.0)]
        );
    }

    #[test]
    fn same_named_findings_prefer_the_base_with_equal_line_count() {
        // Two same-valued findings; the second one grew in lines.
        let base = vec![
            with_lines(finding("run", 1, 17, 16), 10),
            with_lines(finding("run", 30, 17, 16), 40),
        ];
        let head = [
            with_lines(finding("run", 1, 17, 16), 40),
            with_lines(finding("run", 50, 17, 16), 25),
        ];
        let classes = attribution(base, &head);
        // The first head finding takes the 40-line base finding; the second one
        // takes the 10-line base finding and grew.
        assert_eq!(classes[0].attribution(), AuditFindingAttribution::Inherited);
        assert_eq!(classes[1].attribution(), AuditFindingAttribution::Worsened);
    }

    #[test]
    fn a_base_with_the_same_values_keeps_the_finding_inherited() {
        let primary = baseline(vec![with_lines(finding("f", 1, 17, 16), 20)]);
        let merged = baseline(vec![with_lines(finding("f", 1, 17, 16), 60)]);
        let head = [with_lines(finding("f", 1, 17, 16), 60)];
        let classes =
            classify_complexity_attribution_against_all(&head, &root(), [&primary, &merged]);
        assert_eq!(classes[0].attribution(), AuditFindingAttribution::Inherited);
        let classes = classify_complexity_attribution_against_all(&head, &root(), [&primary]);
        assert_eq!(classes[0].attribution(), AuditFindingAttribution::Worsened);
    }

    #[test]
    fn rename_moves_the_base_findings() {
        let base = baseline(vec![finding("f", 1, 17, 16)]);
        let renames =
            FxHashMap::from_iter([("src/index.js".to_string(), "src/moved.js".to_string())]);
        let remapped = remap_complexity_baseline_for_renames(base, &renames);
        assert!(remapped.contains_key("complexity:src/moved.js:f"));
        assert!(!remapped.contains_key("complexity:src/index.js:f"));
    }

    #[test]
    fn ledger_keys_count_same_named_findings_separately() {
        let keys = complexity_ledger_keys(
            &[finding("run", 1, 17, 16), finding("run", 30, 17, 16)],
            &root(),
        );
        assert_eq!(
            keys,
            [
                "complexity:src/index.js:run",
                "complexity:src/index.js:run#2"
            ]
        );
    }
}
