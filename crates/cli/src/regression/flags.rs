//! Regression gate for `fallow flags --retirement`.
//!
//! The gate compares the number of distinct flags, and the count of each
//! reason from `--reason`, with a baseline file. It works only together with
//! `--retirement`, so a run without that option keeps exit code 0.

use std::process::ExitCode;

use fallow_types::envelope::{RegressionStatus, RegressionToleranceKind};
use fallow_types::flag_retirement::{
    FlagRegressionMetric, FlagRegressionResult, RetirementReason, RetirementSummary,
};

use super::baseline::{RegressionOpts, load_regression_baseline};
use super::counts::FlagsCounts;
use super::tolerance::Tolerance;
use crate::error::emit_error;

/// Metric name of the distinct flag count.
pub const DISTINCT_FLAGS_METRIC: &str = "distinct_flags";

impl FlagsCounts {
    /// The counts of one retirement report. `total_flags` is the number of
    /// per-site findings in scope, before `--top`.
    #[must_use]
    pub fn from_summary(summary: &RetirementSummary, total_flags: usize) -> Self {
        Self {
            total_flags,
            distinct_flags: summary.distinct_flags,
            by_reason: summary
                .by_reason
                .iter()
                .map(|(reason, count)| (reason.code().to_string(), *count))
                .collect(),
        }
    }

    fn reason_count(&self, reason: RetirementReason) -> usize {
        self.by_reason.get(reason.code()).copied().unwrap_or(0)
    }
}

/// Compare the current counts with the flags section of the baseline file.
///
/// Returns `None` without `--fail-on-regression`.
///
/// # Errors
///
/// Exits with code 2 when no baseline file is given, when the file cannot be
/// read, or when it has no flags section.
pub fn compare_flags_regression(
    opts: &RegressionOpts<'_>,
    current: &FlagsCounts,
    reasons: &[RetirementReason],
) -> Result<Option<FlagRegressionResult>, ExitCode> {
    if !opts.fail_on_regression {
        return Ok(None);
    }
    if opts.scoped {
        let reason = "--changed-since or --workspace is active; regression check skipped \
                      (counts not comparable to full-project baseline)";
        if !opts.quiet {
            eprintln!("Warning: {reason}");
        }
        return Ok(Some(FlagRegressionResult {
            status: RegressionStatus::Skipped,
            tolerance: None,
            tolerance_kind: None,
            metrics: Vec::new(),
            exceeded: false,
            reason: Some(reason.to_string()),
        }));
    }
    let Some(path) = opts.regression_baseline_file else {
        return Err(emit_error(
            "fallow flags --fail-on-regression needs --regression-baseline <PATH>.\n\
             Create the file with: fallow flags --retirement --save-regression-baseline <PATH>",
            2,
            opts.output,
        ));
    };
    let baseline = load_regression_baseline(path, opts.output)?;
    let Some(stored) = baseline.flags else {
        return Err(emit_error(
            &format!(
                "regression baseline '{}' has no flags data.\n\
                 Create it with: fallow flags --retirement --save-regression-baseline {}",
                path.display(),
                path.display()
            ),
            2,
            opts.output,
        ));
    };

    let mut metrics = vec![metric(
        DISTINCT_FLAGS_METRIC,
        stored.distinct_flags,
        current.distinct_flags,
        opts.tolerance,
    )];
    for reason in RetirementReason::ALL
        .into_iter()
        .filter(|reason| reasons.contains(reason))
    {
        metrics.push(metric(
            reason.code(),
            stored.reason_count(reason),
            current.reason_count(reason),
            opts.tolerance,
        ));
    }
    let exceeded = metrics.iter().any(|metric| metric.exceeded);
    let (tolerance, tolerance_kind) = match opts.tolerance {
        Tolerance::Percentage(percent) => (percent, RegressionToleranceKind::Percentage),
        #[expect(
            clippy::cast_precision_loss,
            reason = "a flag-count tolerance never approaches the f64 integer limit"
        )]
        Tolerance::Absolute(count) => (count as f64, RegressionToleranceKind::Absolute),
    };
    Ok(Some(FlagRegressionResult {
        status: if exceeded {
            RegressionStatus::Exceeded
        } else {
            RegressionStatus::Pass
        },
        tolerance: Some(tolerance),
        tolerance_kind: Some(tolerance_kind),
        metrics,
        exceeded,
        reason: None,
    }))
}

fn metric(
    name: &str,
    baseline: usize,
    current: usize,
    tolerance: Tolerance,
) -> FlagRegressionMetric {
    FlagRegressionMetric {
        metric: name.to_string(),
        baseline,
        current,
        delta: i64::try_from(current).unwrap_or(i64::MAX)
            - i64::try_from(baseline).unwrap_or(i64::MAX),
        exceeded: tolerance.exceeded(baseline, current),
    }
}

/// Print the verdict of the flags regression gate to stderr.
pub fn print_flags_regression(result: &FlagRegressionResult) {
    match result.status {
        RegressionStatus::Skipped => {}
        RegressionStatus::Pass => {
            eprintln!("Flags regression check passed: {}", metric_summary(result));
        }
        RegressionStatus::Exceeded => {
            eprintln!("Flags regression detected: {}", metric_summary(result));
        }
    }
}

fn metric_summary(result: &FlagRegressionResult) -> String {
    result
        .metrics
        .iter()
        .map(|metric| {
            let sign = if metric.delta >= 0 { "+" } else { "" };
            format!(
                "{} {} (baseline: {}, delta: {sign}{})",
                metric.metric, metric.current, metric.baseline, metric.delta
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regression::SaveRegressionTarget;
    use fallow_config::OutputFormat;

    fn counts(distinct: usize, test_only: usize) -> FlagsCounts {
        FlagsCounts {
            total_flags: distinct,
            distinct_flags: distinct,
            by_reason: std::iter::once(("test-only".to_string(), test_only)).collect(),
        }
    }

    fn opts(path: &std::path::Path, tolerance: Tolerance) -> RegressionOpts<'_> {
        RegressionOpts {
            fail_on_regression: true,
            tolerance,
            regression_baseline_file: Some(path),
            save_target: SaveRegressionTarget::None,
            scoped: false,
            quiet: true,
            output: OutputFormat::Json,
        }
    }

    fn saved(dir: &std::path::Path, stored: &FlagsCounts) -> std::path::PathBuf {
        let path = dir.join("flags-baseline.json");
        crate::regression::save_flags_regression_baseline(&path, dir, stored, OutputFormat::Json)
            .expect("save");
        path
    }

    #[test]
    fn growth_past_the_tolerance_fails() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = saved(dir.path(), &counts(3, 1));
        let result =
            compare_flags_regression(&opts(&path, Tolerance::Absolute(0)), &counts(4, 1), &[])
                .expect("compare")
                .expect("gate ran");
        assert!(result.exceeded);
        assert_eq!(result.metrics.len(), 1);
        assert_eq!(result.metrics[0].delta, 1);

        let tolerated =
            compare_flags_regression(&opts(&path, Tolerance::Absolute(1)), &counts(4, 1), &[])
                .expect("compare")
                .expect("gate ran");
        assert!(!tolerated.exceeded);
    }

    #[test]
    fn a_chosen_reason_count_is_gated_too() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = saved(dir.path(), &counts(3, 1));
        let result = compare_flags_regression(
            &opts(&path, Tolerance::Absolute(0)),
            &counts(3, 2),
            &[RetirementReason::TestOnly],
        )
        .expect("compare")
        .expect("gate ran");
        assert!(result.exceeded);
        assert!(!result.metrics[0].exceeded, "distinct_flags did not grow");
        assert_eq!(result.metrics[1].metric, "test-only");
        assert!(result.metrics[1].exceeded);
    }

    #[test]
    fn no_gate_without_fail_on_regression() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = saved(dir.path(), &counts(3, 1));
        let off = RegressionOpts {
            fail_on_regression: false,
            ..opts(&path, Tolerance::Absolute(0))
        };
        assert!(
            compare_flags_regression(&off, &counts(9, 9), &[])
                .expect("compare")
                .is_none()
        );
    }
}
