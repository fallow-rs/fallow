//! New-only attribution of complexity findings.
//!
//! A complexity finding is identified by its path and function name. The
//! exceeded category and the line are not part of the identity, so a line
//! shift or a category change keeps the match with the base finding. The
//! metric values then decide the attribution. A head finding is introduced
//! when:
//!
//! - no base finding matches it,
//! - it exceeds a metric that the base finding did not exceed, and that metric
//!   grew (see [`newly_exceeded_metric_grew`]), or
//! - a metric that it exceeds grew, and its severity tier is higher than the
//!   tier of the base finding (see [`severity_tier_rose`]).
//!
//! The exceeded flags and the tier of the base finding come from the base run,
//! so a threshold override in the same change cannot make an old finding
//! introduced. Unchanged and decreased metrics stay inherited, also when the
//! exceeded category changes (#3277).
//!
//! A matched finding that is not introduced is worsened when its cyclomatic,
//! cognitive or CRAP value is higher than in its matched base finding. Growth
//! of a metric that the base finding already exceeded, in the same tier, is
//! worsened too. A line count must grow by a minimum (see
//! [`line_count_grew`]). The `audit.worsened` setting decides how a worsened
//! finding affects the verdict.
//!
//! A CRAP score from estimated coverage does not make a finding introduced.
//! Under the `new-only` gate, a finding that exceeds only that CRAP score gets
//! the gate severity `warn` (see [`demote_estimated_crap_gates`]).
//!
//! Under every gate, a finding in a test file gets the gate severity `warn`
//! and the gate exemption `test-file` (see [`exempt_test_file_gates`]). The
//! attribution of the finding does not change.

use std::path::Path;

use fallow_output::{
    AuditFindingAttribution, AuditGateExemption, CoverageSource, DEFAULT_COGNITIVE_CRITICAL,
    DEFAULT_COGNITIVE_HIGH, DEFAULT_CYCLOMATIC_CRITICAL, DEFAULT_CYCLOMATIC_HIGH,
    ExceededThreshold, FindingSeverity, WorsenedMetric, WorsenedMetricKind,
    compute_finding_severity,
};
use fallow_types::output_dead_code::EffectiveSeverity;
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
    /// The thresholds that the finding exceeded in its own run.
    pub exceeded: ExceededThreshold,
    /// The severity tier of the finding in its own run.
    pub severity: FindingSeverity,
    /// Whether the CRAP score comes from estimated coverage.
    pub crap_estimated: bool,
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
            exceeded: finding.exceeded,
            severity: finding.severity,
            crap_estimated: crap_is_estimated(finding),
        }
    }

    /// Whether both findings have the same complexity values. The line and
    /// the line count are not compared.
    fn same_values(&self, other: &Self) -> bool {
        self.cyclomatic == other.cyclomatic
            && self.cognitive == other.cognitive
            && self.crap.map(f64::to_bits) == other.crap.map(f64::to_bits)
    }

    /// Whether both findings have the same complexity values and the same
    /// line count.
    fn same_values_and_lines(&self, other: &Self) -> bool {
        self.same_values(other) && self.line_count == other.line_count
    }

    /// Whether both findings have the same cyclomatic and cognitive values.
    /// The CRAP score is not compared, because it also changes when only the
    /// coverage input changes (#3815).
    const fn same_structure(&self, other: &Self) -> bool {
        self.cyclomatic == other.cyclomatic && self.cognitive == other.cognitive
    }

    /// Whether both findings have the same cyclomatic and cognitive values and
    /// the same line count.
    const fn same_structure_and_lines(&self, other: &Self) -> bool {
        self.same_structure(other) && self.line_count == other.line_count
    }
}

/// Attribution of one head complexity finding against the base findings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComplexityClass {
    /// `true` when no base finding matches, when a newly exceeded metric
    /// grew, or when growth moved the finding to a higher severity tier.
    pub introduced: bool,
    /// The measured metrics that are higher than in the matched base finding.
    /// Empty when no base finding matches or when no metric grew.
    pub worsened_metrics: Vec<WorsenedMetric>,
    /// `true` when a metric that the finding exceeds grew. Before this rule,
    /// that growth made the finding introduced, so `audit.worsened: off`
    /// keeps it worsened (see [`Self::clear_worsened_for_off`]).
    pub exceeded_metric_grew: bool,
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

    /// Clear the grown metrics for `audit.worsened: off`. Growth only in
    /// metrics that the finding does not exceed then stays inherited. Growth
    /// of an exceeded metric stays worsened, so it gives at least `warn` and
    /// does not disappear from the report.
    pub fn clear_worsened_for_off(&mut self) {
        if !self.introduced && !self.exceeded_metric_grew {
            self.worsened_metrics.clear();
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
                exceeded_metric_grew: false,
            },
            |base| {
                let exceeded_metric_grew = exceeded_metric_increased(head, base);
                Self {
                    introduced: newly_exceeded_metric_grew(head, base)
                        || (exceeded_metric_grew && severity_tier_rose(head, base)),
                    worsened_metrics: grown_metrics(head, base),
                    exceeded_metric_grew,
                }
            },
        )
    }
}

/// The measured metrics of `head` that are higher than in `base`.
///
/// The line count must grow by a minimum, see [`line_count_grew`]. CRAP
/// compares only when both sides have a score, because a score that
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
    if line_count_grew(base.line_count, head.line_count) {
        grown.push(WorsenedMetric {
            metric: WorsenedMetricKind::LineCount,
            base: f64::from(base.line_count),
            head: f64::from(head.line_count),
        });
    }
    grown
}

/// The smallest line count growth, in lines, that makes a finding worsened.
const LINE_COUNT_MIN_GROWTH_LINES: u32 = 5;

/// Whether the line count grew by at least [`LINE_COUNT_MIN_GROWTH_LINES`]
/// lines. The floor is fixed and does not scale with the function size, so
/// growth in a large function is not hidden.
///
/// The line count is the physical span of the function, so comments and blank
/// lines in the body count. The minimum keeps a comment or a formatter rewrap
/// from making a finding worsened.
fn line_count_grew(base: u32, head: u32) -> bool {
    head.saturating_sub(base) >= LINE_COUNT_MIN_GROWTH_LINES
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
/// CRAP counts only when both sides have a measured score (see
/// [`measured_crap_grew`]).
fn exceeded_metric_increased(
    head: &fallow_output::ComplexityViolation,
    base: &ComplexityMetrics,
) -> bool {
    let exceeded = head.exceeded;
    (exceeded.includes_cyclomatic() && head.cyclomatic > base.cyclomatic)
        || (exceeded.includes_cognitive() && head.cognitive > base.cognitive)
        || (exceeded.includes_crap() && measured_crap_grew(head, base))
        || (exceeded.is_file_lines() && line_count_grew(base.line_count, head.line_count))
}

/// Whether `head` exceeds a metric that `base` did not exceed in its own run,
/// and that metric grew. A metric that crosses its limit only because a
/// threshold changed does not grow, so it stays inherited.
fn newly_exceeded_metric_grew(
    head: &fallow_output::ComplexityViolation,
    base: &ComplexityMetrics,
) -> bool {
    let (now, before) = (head.exceeded, base.exceeded);
    (now.includes_cyclomatic()
        && !before.includes_cyclomatic()
        && head.cyclomatic > base.cyclomatic)
        || (now.includes_cognitive()
            && !before.includes_cognitive()
            && head.cognitive > base.cognitive)
        || (now.includes_crap() && !before.includes_crap() && measured_crap_grew(head, base))
}

/// Whether both sides have a CRAP score from measured coverage and the head
/// score is higher. A score that only one side has, or a score from estimated
/// coverage, reflects the coverage input, not the code.
fn measured_crap_grew(head: &fallow_output::ComplexityViolation, base: &ComplexityMetrics) -> bool {
    crap_is_comparable(head, base)
        && matches!((head.crap, base.crap), (Some(head), Some(base)) if head > base)
}

fn crap_is_comparable(head: &fallow_output::ComplexityViolation, base: &ComplexityMetrics) -> bool {
    head.crap.is_some() && base.crap.is_some() && !crap_is_estimated(head) && !base.crap_estimated
}

/// Whether the severity tier of `head` is higher than the tier of `base`.
///
/// The tiers use fixed limits, not the configured thresholds. The CRAP score
/// takes part only when both sides have a measured score; else both tiers come
/// from cyclomatic and cognitive complexity only.
fn severity_tier_rose(head: &fallow_output::ComplexityViolation, base: &ComplexityMetrics) -> bool {
    if crap_is_comparable(head, base) {
        return head.severity > base.severity;
    }
    complexity_tier(head.cognitive, head.cyclomatic)
        > complexity_tier(base.cognitive, base.cyclomatic)
}

fn complexity_tier(cognitive: u16, cyclomatic: u16) -> FindingSeverity {
    compute_finding_severity(
        cognitive,
        cyclomatic,
        None,
        DEFAULT_COGNITIVE_HIGH,
        DEFAULT_COGNITIVE_CRITICAL,
        DEFAULT_CYCLOMATIC_HIGH,
        DEFAULT_CYCLOMATIC_CRITICAL,
    )
}

/// Whether the CRAP score of the finding comes from estimated coverage, not
/// from a measured coverage map (`--coverage` or runtime coverage).
#[must_use]
pub const fn crap_is_estimated(finding: &fallow_output::ComplexityViolation) -> bool {
    matches!(
        finding.coverage_source,
        Some(CoverageSource::Estimated | CoverageSource::EstimatedComponentInherited)
    )
}

/// Whether CRAP from estimated coverage is the only threshold that the
/// finding exceeds.
#[must_use]
pub const fn exceeds_only_estimated_crap(finding: &fallow_output::ComplexityViolation) -> bool {
    matches!(finding.exceeded, ExceededThreshold::Crap) && crap_is_estimated(finding)
}

/// Give the gate severity `warn` to each finding that exceeds only CRAP from
/// estimated coverage, for the `new-only` gate.
///
/// The estimate comes from static test reachability, so it does not measure
/// the change. The finding stays in the report, and the verdict reads `warn`
/// for it. A finding that also exceeds cyclomatic or cognitive complexity
/// keeps its gate severity. Measured coverage keeps the gate severity too.
pub fn demote_estimated_crap_gates(findings: &mut [fallow_output::HealthFinding]) {
    for finding in findings {
        if exceeds_only_estimated_crap(finding)
            && finding.effective_severity == Some(EffectiveSeverity::Error)
        {
            finding.violation.effective_severity = Some(EffectiveSeverity::Warn);
        }
    }
}

/// Give the gate severity `warn` to each finding in a test file, and mark it
/// with [`AuditGateExemption::TestFile`].
///
/// The shared test-path classifier ([`fallow_engine::test_paths::is_test_path`])
/// decides what a test file is: specs, test directories, end-to-end suites,
/// mocks and fixtures. The finding stays in the report. `fallow health` does
/// not call this, so it keeps the rule severity there.
pub fn exempt_test_file_gates(findings: &mut [fallow_output::HealthFinding], root: &Path) {
    for finding in findings {
        let relative = finding.path.strip_prefix(root).unwrap_or(&finding.path);
        if fallow_engine::test_paths::is_test_path(root, relative) {
            finding.violation.effective_severity = Some(EffectiveSeverity::Warn);
            finding.gate_exemption = Some(AuditGateExemption::TestFile);
        }
    }
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
/// 1. Each head finding, in line order, takes an unmatched base finding with
///    the same complexity values and the same line count.
/// 2. Each remaining head finding, in line order, takes an unmatched base
///    finding with the same complexity values. Rules 1 and 2 keep unchanged
///    findings inherited when a same-named finding is added, removed or
///    edited. Rule 1 runs for all head findings first, so an earlier head
///    finding cannot take the exact match of a later one.
/// 3. Each remaining head finding, in line order, takes an unmatched base
///    finding with the same cyclomatic and cognitive values and the same line
///    count, and then one with the same cyclomatic and cognitive values. The
///    CRAP score is not compared in this rule, because a change of the
///    coverage input changes the CRAP score of an unchanged function (#3815).
///    The pair still compares the CRAP score, so CRAP growth stays visible.
/// 4. The remaining head findings, in line order, take the remaining base
///    findings, in line order. Each pair compares the exceeded metrics.
/// 5. A head finding without a base finding is introduced.
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
        let rules: [fn(&ComplexityMetrics, &ComplexityMetrics) -> bool; 4] = [
            ComplexityMetrics::same_values_and_lines,
            ComplexityMetrics::same_values,
            ComplexityMetrics::same_structure_and_lines,
            ComplexityMetrics::same_structure,
        ];
        for rule in rules {
            for &index in &heads {
                if matches[index].is_some() {
                    continue;
                }
                let head = ComplexityMetrics::of(&findings[index]);
                if let Some(slot) = unmatched_bases
                    .iter_mut()
                    .find(|slot| slot.is_some_and(|base| rule(base, &head)))
                {
                    matches[index] = slot.take();
                }
            }
        }
        let mut remaining_bases = unmatched_bases.into_iter().flatten();
        for &index in &heads {
            if matches[index].is_none() {
                matches[index] = remaining_bases.next();
            }
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
                exceeded_metric_grew: false,
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
            standard_cyclomatic: None,
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
            max_file_lines: None,
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
        // Increased in the same severity tier: worsened, not introduced.
        assert_eq!(
            classify(vec![finding("f", 1, 17, 16)], &[finding("f", 1, 18, 17)]),
            [false]
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
        // A finding that moved to a higher tier against every base stays
        // introduced.
        assert_eq!(
            classify_complexity_findings_against_all(
                &[finding("f", 1, 31, 17)],
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

    fn crap_finding(crap: Option<f64>, source: Option<CoverageSource>) -> HealthFinding {
        let mut finding = finding("f", 1, 16, 5);
        finding.violation.exceeded = ExceededThreshold::Crap;
        finding.violation.crap = crap;
        finding.violation.coverage_source = source;
        finding.violation.effective_severity = Some(EffectiveSeverity::Error);
        finding
    }

    #[test]
    fn crap_without_base_value_is_not_an_increase() {
        // A base without a CRAP score ran without that coverage input. The
        // code did not grow.
        let head = crap_finding(Some(50.0), Some(CoverageSource::Istanbul));
        assert_eq!(
            classify(vec![crap_finding(None, None)], std::slice::from_ref(&head)),
            [false]
        );
        // Measured CRAP growth in the same tier is worsened.
        assert_eq!(
            classify(
                vec![crap_finding(Some(40.0), Some(CoverageSource::Istanbul))],
                std::slice::from_ref(&head)
            ),
            [false]
        );
        assert_eq!(
            classify(
                vec![crap_finding(Some(60.0), Some(CoverageSource::Istanbul))],
                &[head]
            ),
            [false]
        );
    }

    #[test]
    fn estimated_crap_growth_is_worsened_not_introduced() {
        let estimated = Some(CoverageSource::Estimated);
        let classes = attribution(
            vec![crap_finding(Some(40.0), estimated)],
            &[crap_finding(Some(50.0), estimated)],
        );
        assert_eq!(classes[0].attribution(), AuditFindingAttribution::Worsened);
        assert_eq!(
            classes[0].worsened_metrics,
            [metric(WorsenedMetricKind::Crap, 40.0, 50.0)]
        );
        // A grown cognitive value that the base already exceeds is worsened.
        let mut base = crap_finding(Some(40.0), estimated);
        base.violation.exceeded = ExceededThreshold::CognitiveCrap;
        base.violation.cognitive = 20;
        let mut head = base.clone();
        head.violation.cognitive = 21;
        head.violation.crap = Some(50.0);
        assert_eq!(classify(vec![base.clone()], &[head.clone()]), [false]);
        // A grown cognitive value that reaches the next tier is introduced.
        head.violation.cognitive = 25;
        assert_eq!(classify(vec![base], &[head]), [true]);
    }

    #[test]
    fn test_file_findings_get_warn_gate_severity_and_the_exemption() {
        // The project itself lives below a `tests` directory. Only the path
        // below the project root decides.
        let project = root().join("tests").join("project");
        let at = |relative: &str| {
            let mut finding = finding("f", 1, 16, 5);
            finding.violation.path = project.join(relative);
            finding.violation.effective_severity = Some(EffectiveSeverity::Error);
            finding
        };
        let mut findings = [
            at("src/__tests__/app.js"),
            at("src/app.spec.ts"),
            at("src/app.test.tsx"),
            at("src/__mocks__/api.js"),
            at("src/app.js"),
            at("src/latest.js"),
        ];
        exempt_test_file_gates(&mut findings, &project);
        let marks: Vec<_> = findings
            .iter()
            .map(|finding| (finding.effective_severity, finding.gate_exemption))
            .collect();
        let exempt = (
            Some(EffectiveSeverity::Warn),
            Some(AuditGateExemption::TestFile),
        );
        let gated = (Some(EffectiveSeverity::Error), None);
        assert_eq!(marks, [exempt, exempt, exempt, exempt, gated, gated]);
    }

    #[test]
    fn estimated_crap_only_findings_get_warn_gate_severity() {
        let mut both = crap_finding(Some(50.0), Some(CoverageSource::Estimated));
        both.violation.exceeded = ExceededThreshold::CognitiveCrap;
        let mut warn_rule = crap_finding(Some(50.0), Some(CoverageSource::Estimated));
        warn_rule.violation.effective_severity = Some(EffectiveSeverity::Warn);
        let mut findings = [
            crap_finding(Some(50.0), Some(CoverageSource::Estimated)),
            crap_finding(
                Some(50.0),
                Some(CoverageSource::EstimatedComponentInherited),
            ),
            crap_finding(Some(50.0), Some(CoverageSource::Istanbul)),
            both,
            warn_rule,
        ];
        demote_estimated_crap_gates(&mut findings);
        let severities: Vec<_> = findings
            .iter()
            .map(|finding| finding.effective_severity)
            .collect();
        assert_eq!(
            severities,
            [
                Some(EffectiveSeverity::Warn),
                Some(EffectiveSeverity::Warn),
                Some(EffectiveSeverity::Error),
                Some(EffectiveSeverity::Error),
                Some(EffectiveSeverity::Warn),
            ]
        );
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
                base.clone(),
                &[finding("run", 1, 18, 17), finding("run", 30, 21, 20)]
            ),
            [false, false]
        );
        // The first one moved to a higher tier, the second one unchanged.
        assert_eq!(
            classify(
                base,
                &[finding("run", 1, 30, 17), finding("run", 30, 21, 20)]
            ),
            [true, false]
        );
    }

    /// An anonymous finding with a CRAP score from the given coverage source.
    fn arrow(
        line: u32,
        cyclomatic: u16,
        cognitive: u16,
        crap: Option<f64>,
        source: CoverageSource,
    ) -> HealthFinding {
        let mut finding = finding("<arrow>", line, MAX + 1, MAX + 1);
        finding.violation.cyclomatic = cyclomatic;
        finding.violation.cognitive = cognitive;
        finding.violation.crap = crap;
        finding.violation.coverage_source = crap.map(|_| source);
        finding.violation.exceeded = ExceededThreshold::from_bools(
            cyclomatic > MAX,
            cognitive > MAX,
            crap.is_some_and(|crap| crap >= 30.0),
        );
        finding
    }

    #[test]
    fn unchanged_arrow_keeps_its_base_when_only_crap_changes() {
        // Issue #3815: the CRAP score of an unchanged arrow went from an
        // estimated score to a measured score. Its cyclomatic and cognitive
        // values did not change, so it must keep its own base finding, not the
        // first remaining base finding in line order.
        let estimated = CoverageSource::Estimated;
        let measured = CoverageSource::Istanbul;
        let base = [
            (122, 36, 37, 1332.0),
            (284, 26, 28, 702.0),
            (352, 9, 6, 90.0),
            (560, 7, 6, 56.0),
            (637, 30, 42, 930.0),
            (732, 13, 5, 182.0),
            (872, 13, 12, 182.0),
            (889, 6, 2, 42.0),
            (945, 5, 3, 30.0),
            (972, 16, 4, 272.0),
            (1017, 11, 13, 132.0),
            (1184, 14, 15, 210.0),
            (1202, 12, 5, 156.0),
        ]
        .into_iter()
        .map(|(line, cyclomatic, cognitive, crap)| {
            arrow(line, cyclomatic, cognitive, Some(crap), estimated)
        })
        .collect();
        let head = [
            arrow(128, 36, 37, Some(36.0), measured),
            arrow(294, 27, 29, None, measured),
            arrow(663, 30, 42, Some(30.0), measured),
        ];
        let classes = attribution(base, &head);
        assert!(!classes[0].introduced);
        assert!(!classes[2].introduced, "{:?}", classes[2]);
        assert!(classes[2].worsened_metrics.is_empty());
    }

    #[test]
    fn crap_growth_of_an_unchanged_arrow_still_compares() {
        let measured = CoverageSource::Istanbul;
        let classes = attribution(
            vec![
                arrow(10, 9, 6, Some(40.0), measured),
                arrow(40, 30, 42, Some(30.0), measured),
            ],
            &[arrow(40, 30, 42, Some(60.0), measured)],
        );
        assert_eq!(
            classes[0].worsened_metrics,
            [metric(WorsenedMetricKind::Crap, 30.0, 60.0)]
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
    fn grown_exceeded_metric_in_the_same_tier_is_worsened() {
        // Cyclomatic 100 to 103 stays in the critical tier.
        let classes = attribution(vec![finding("f", 1, 100, 16)], &[finding("f", 1, 103, 16)]);
        assert_eq!(classes[0].attribution(), AuditFindingAttribution::Worsened);
        assert!(classes[0].exceeded_metric_grew);
        assert_eq!(
            classes[0].worsened_metrics,
            [metric(WorsenedMetricKind::Cyclomatic, 100.0, 103.0)]
        );
        // Growth only in a metric that the finding does not exceed.
        let below = attribution(vec![finding("f", 1, 20, 5)], &[finding("f", 1, 20, 9)]);
        assert!(!below[0].exceeded_metric_grew);
    }

    #[test]
    fn severity_tier_uses_the_base_values_not_todays_thresholds() {
        // The base finding exceeded only cyclomatic. A threshold override in
        // the same change lowers the cognitive limit, so the head finding
        // exceeds cognitive too, without cognitive growth.
        let mut head = finding("f", 1, 18, 10);
        head.violation.exceeded = ExceededThreshold::Both;
        assert_eq!(
            classify(vec![finding("f", 1, 17, 10)], &[head.clone()]),
            [false]
        );
        // Cognitive grew and the base did not exceed it: newly exceeded.
        head.violation.cognitive = 11;
        assert_eq!(classify(vec![finding("f", 1, 17, 10)], &[head]), [true]);
    }

    #[test]
    fn measured_crap_tier_crossing_is_introduced() {
        let istanbul = Some(CoverageSource::Istanbul);
        let mut base = crap_finding(Some(40.0), istanbul);
        base.violation.severity = FindingSeverity::Moderate;
        let mut head = crap_finding(Some(120.0), istanbul);
        head.violation.severity = FindingSeverity::Critical;
        assert_eq!(classify(vec![base.clone()], &[head.clone()]), [true]);
        // The same values from estimated coverage do not cross a tier.
        let estimated = Some(CoverageSource::Estimated);
        base.violation.coverage_source = estimated;
        head.violation.coverage_source = estimated;
        assert_eq!(classify(vec![base], &[head]), [false]);
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
    fn an_exact_match_of_a_later_head_is_not_taken_by_an_earlier_head() {
        // The 10-line function moved up and grew to 30 lines. The 40-line
        // function did not change, but it now comes later in line order.
        let base = vec![
            with_lines(finding("run", 1, 17, 16), 40),
            with_lines(finding("run", 50, 17, 16), 10),
        ];
        let head = [
            with_lines(finding("run", 1, 17, 16), 30),
            with_lines(finding("run", 20, 17, 16), 40),
        ];
        let classes = attribution(base, &head);
        assert_eq!(classes[0].attribution(), AuditFindingAttribution::Worsened);
        assert_eq!(
            classes[0].worsened_metrics,
            [metric(WorsenedMetricKind::LineCount, 10.0, 30.0)]
        );
        assert_eq!(classes[1].attribution(), AuditFindingAttribution::Inherited);
        assert!(classes[1].worsened_metrics.is_empty());
    }

    #[test]
    fn small_line_growth_is_not_worsened() {
        // A comment line or a formatter rewrap adds a few physical lines.
        let classes = attribution(
            vec![with_lines(finding("f", 1, 17, 16), 80)],
            &[with_lines(finding("f", 1, 17, 16), 84)],
        );
        assert_eq!(classes[0].attribution(), AuditFindingAttribution::Inherited);
        assert!(classes[0].worsened_metrics.is_empty());
    }

    #[test]
    fn line_growth_needs_five_lines_at_any_function_size() {
        // 4 lines are below the floor, also on a short function.
        let short = attribution(
            vec![with_lines(finding("f", 1, 17, 16), 20)],
            &[with_lines(finding("f", 1, 17, 16), 24)],
        );
        assert_eq!(short[0].attribution(), AuditFindingAttribution::Inherited);
        let grown = attribution(
            vec![with_lines(finding("f", 1, 17, 16), 20)],
            &[with_lines(finding("f", 1, 17, 16), 25)],
        );
        assert_eq!(grown[0].attribution(), AuditFindingAttribution::Worsened);
        // The floor does not scale: 9 lines on a 300-line function count.
        let long = attribution(
            vec![with_lines(finding("f", 1, 17, 16), 300)],
            &[with_lines(finding("f", 1, 17, 16), 309)],
        );
        assert_eq!(long[0].attribution(), AuditFindingAttribution::Worsened);
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
