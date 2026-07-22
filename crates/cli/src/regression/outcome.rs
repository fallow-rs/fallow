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

    /// Build a JSON value for the regression outcome (added to JSON output envelope).
    #[must_use]
    #[cfg(test)]
    pub(crate) fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Pass {
                baseline_total,
                current_total,
            } => serde_json::json!({
                "status": "pass",
                "baseline_total": baseline_total,
                "current_total": current_total,
                "delta": *current_total as isize - *baseline_total as isize,
                "exceeded": false,
            }),
            Self::Exceeded {
                baseline_total,
                current_total,
                tolerance,
                ..
            } => {
                let (tolerance_value, tolerance_kind) = match tolerance {
                    Tolerance::Percentage(pct) => (*pct, "percentage"),
                    Tolerance::Absolute(abs) => (*abs as f64, "absolute"),
                };
                serde_json::json!({
                    "status": "exceeded",
                    "baseline_total": baseline_total,
                    "current_total": current_total,
                    "delta": *current_total as isize - *baseline_total as isize,
                    "tolerance": tolerance_value,
                    "tolerance_kind": tolerance_kind,
                    "exceeded": true,
                })
            }
            Self::Skipped { reason } => serde_json::json!({
                "status": "skipped",
                "reason": reason,
                "exceeded": false,
            }),
        }
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
    fn pass_outcome_json() {
        let outcome = RegressionOutcome::Pass {
            baseline_total: 10,
            current_total: 10,
        };
        let json = outcome.to_json();
        assert_eq!(json["status"], "pass");
        assert_eq!(json["exceeded"], false);
        assert_eq!(json["delta"], 0);
    }

    #[test]
    fn exceeded_outcome_json() {
        let outcome = RegressionOutcome::Exceeded {
            baseline_total: 10,
            current_total: 15,
            tolerance: Tolerance::Percentage(2.0),
            type_deltas: vec![("unused_files", 5)],
        };
        let json = outcome.to_json();
        assert_eq!(json["status"], "exceeded");
        assert_eq!(json["exceeded"], true);
        assert_eq!(json["delta"], 5);
        assert_eq!(json["tolerance_kind"], "percentage");
    }

    #[test]
    fn skipped_outcome_json() {
        let outcome = RegressionOutcome::Skipped {
            reason: "test reason",
        };
        let json = outcome.to_json();
        assert_eq!(json["status"], "skipped");
        assert_eq!(json["exceeded"], false);
    }

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

    #[test]
    fn exceeded_outcome_json_absolute() {
        let outcome = RegressionOutcome::Exceeded {
            baseline_total: 10,
            current_total: 15,
            tolerance: Tolerance::Absolute(2),
            type_deltas: vec![("unused_files", 5)],
        };
        let json = outcome.to_json();
        assert_eq!(json["status"], "exceeded");
        assert_eq!(json["tolerance_kind"], "absolute");
        assert_eq!(json["tolerance"], 2.0);
        assert_eq!(json["delta"], 5);
    }

    #[test]
    fn pass_outcome_json_with_improvement() {
        let outcome = RegressionOutcome::Pass {
            baseline_total: 10,
            current_total: 5,
        };
        let json = outcome.to_json();
        assert_eq!(json["status"], "pass");
        assert_eq!(json["delta"], -5);
        assert_eq!(json["exceeded"], false);
    }

    #[test]
    fn print_pass_outcome_does_not_panic() {
        let outcome = RegressionOutcome::Pass {
            baseline_total: 10,
            current_total: 8,
        };
        print_regression_outcome(&outcome);
    }

    #[test]
    fn print_exceeded_outcome_does_not_panic() {
        let outcome = RegressionOutcome::Exceeded {
            baseline_total: 10,
            current_total: 15,
            tolerance: Tolerance::Percentage(2.0),
            type_deltas: vec![("unused_files", 5), ("unused_exports", -2)],
        };
        print_regression_outcome(&outcome);
    }

    #[test]
    fn print_exceeded_outcome_absolute_does_not_panic() {
        let outcome = RegressionOutcome::Exceeded {
            baseline_total: 10,
            current_total: 15,
            tolerance: Tolerance::Absolute(2),
            type_deltas: vec![("unused_files", 3), ("unresolved_imports", 2)],
        };
        print_regression_outcome(&outcome);
    }

    #[test]
    fn print_skipped_outcome_does_not_panic() {
        let outcome = RegressionOutcome::Skipped {
            reason: "test reason",
        };
        print_regression_outcome(&outcome);
    }

    #[test]
    fn print_exceeded_with_empty_deltas_does_not_panic() {
        let outcome = RegressionOutcome::Exceeded {
            baseline_total: 10,
            current_total: 15,
            tolerance: Tolerance::Absolute(0),
            type_deltas: vec![],
        };
        print_regression_outcome(&outcome);
    }
}
