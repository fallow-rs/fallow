use super::Tolerance;

/// Result of a regression check.
#[derive(Debug)]
pub enum RegressionOutcome {
    /// No regression — current issues are within tolerance.
    Pass {
        baseline_total: usize,
        current_total: usize,
    },
    /// Regression exceeded tolerance.
    Exceeded {
        baseline_total: usize,
        current_total: usize,
        tolerance: Tolerance,
        /// Per-type deltas for human output.
        type_deltas: Vec<(&'static str, isize)>,
    },
    /// Regression check was skipped (e.g., --changed-since active).
    Skipped { reason: &'static str },
}

impl RegressionOutcome {
    /// Whether this outcome should cause a non-zero exit code.
    #[must_use]
    pub const fn is_failure(&self) -> bool {
        matches!(self, Self::Exceeded { .. })
    }
}

/// Print regression outcome to stderr (human-readable summary).
pub fn print_regression_outcome(outcome: &RegressionOutcome) {
    match outcome {
        RegressionOutcome::Pass {
            baseline_total,
            current_total,
        } => {
            let delta = *current_total as isize - *baseline_total as isize;
            let sign = if delta >= 0 { "+" } else { "" };
            eprintln!(
                "Regression check passed: {current_total} issues (baseline: {baseline_total}, \
                 delta: {sign}{delta})"
            );
        }
        RegressionOutcome::Exceeded {
            baseline_total,
            current_total,
            tolerance,
            type_deltas,
        } => {
            let delta = *current_total as isize - *baseline_total as isize;
            let tol_str = match tolerance {
                Tolerance::Percentage(pct) => format!("{pct}%"),
                Tolerance::Absolute(abs) => format!("{abs}"),
            };
            eprintln!(
                "Regression detected: {current_total} issues (baseline: {baseline_total}, \
                 delta: +{delta}, tolerance: {tol_str})"
            );
            for (name, d) in type_deltas {
                let sign = if *d > 0 { "+" } else { "" };
                eprintln!("  {name}: {sign}{d}");
            }
        }
        RegressionOutcome::Skipped { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_outcome_is_failure() {
        let pass = RegressionOutcome::Pass {
            baseline_total: 10,
            current_total: 10,
        };
        assert!(!pass.is_failure());

        let exceeded = RegressionOutcome::Exceeded {
            baseline_total: 10,
            current_total: 15,
            tolerance: Tolerance::Absolute(2),
            type_deltas: vec![],
        };
        assert!(exceeded.is_failure());

        let skipped = RegressionOutcome::Skipped { reason: "test" };
        assert!(!skipped.is_failure());
    }
}
