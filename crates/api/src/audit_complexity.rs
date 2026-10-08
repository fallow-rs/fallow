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

use std::path::Path;

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
        }
    }

    /// Whether both findings have the same metric values. The line is not
    /// compared.
    fn same_values(&self, other: &Self) -> bool {
        self.cyclomatic == other.cyclomatic
            && self.cognitive == other.cognitive
            && self.crap.map(f64::to_bits) == other.crap.map(f64::to_bits)
    }
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
/// (`false`), in report order.
///
/// Head and base findings match by identity key. When one key holds several
/// findings on one side (same-named class methods or anonymous functions in
/// one file), the match is ambiguous. This conservative rule then applies, per
/// key:
///
/// 1. A head finding with the same metric values as an unmatched base finding
///    takes that base finding and is inherited. This keeps unchanged findings
///    inherited when a same-named finding is added, removed or edited.
/// 2. The remaining head findings, in line order, take the remaining base
///    findings, in line order. Each pair compares the exceeded metrics.
/// 3. A head finding without a base finding is introduced.
///
/// Every base finding matches at most one head finding, so a new same-named
/// finding cannot hide behind an existing one. The rule does not identify
/// functions beyond path and name (see #2010). Two same-named findings that
/// swap their metric values in one change stay inherited.
#[must_use]
pub fn classify_complexity_findings(
    findings: &[fallow_output::HealthFinding],
    root: &Path,
    base: &ComplexityBaseline,
) -> Vec<bool> {
    let mut groups: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (index, finding) in findings.iter().enumerate() {
        groups
            .entry(health_finding_key(finding, root))
            .or_default()
            .push(index);
    }
    let mut introduced = vec![true; findings.len()];
    for (key, mut heads) in groups {
        let Some(bases) = base.get(&key) else {
            continue;
        };
        heads.sort_by_key(|&index| (findings[index].line, findings[index].col));
        let mut unmatched_bases: Vec<Option<&ComplexityMetrics>> = bases.iter().map(Some).collect();
        let mut unmatched_heads = Vec::new();
        for index in heads {
            let head = ComplexityMetrics::of(&findings[index]);
            let same = unmatched_bases
                .iter_mut()
                .find(|slot| slot.is_some_and(|base| base.same_values(&head)));
            match same {
                Some(slot) => {
                    *slot = None;
                    introduced[index] = false;
                }
                None => unmatched_heads.push(index),
            }
        }
        let mut remaining_bases = unmatched_bases.into_iter().flatten();
        for index in unmatched_heads {
            if let Some(base) = remaining_bases.next() {
                introduced[index] = exceeded_metric_increased(&findings[index], base);
            }
        }
    }
    introduced
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
