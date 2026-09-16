//! Builders for the envelope's `gate_outcomes`, one per command.
//!
//! Each builder projects the rules the command's exit path already applies, so
//! the published verdict and the process status cannot disagree. Nothing here
//! evaluates a condition of its own: every outcome reads the same value the
//! gate reads, which is why the identity tests in `crates/cli/tests` can assert
//! `gate_outcomes[g].status` against the feature-local field beside it.
//!
//! # When an entry appears
//!
//! An entry appears when the gate was ARMED on this run, never merely because
//! the rule behind it exists. `--fail-on-issues` arms
//! [`GateName::ErrorSeverityFindings`], a loaded baseline arms
//! [`GateName::StaleBaseline`], `--threshold` arms
//! [`GateName::DuplicationThreshold`], and so on. A run that arms nothing emits
//! no object at all, which is what keeps every existing consumer's wire shape
//! byte-identical.
//!
//! The one deliberate exception is a gate that is armed but cannot be enforced:
//! `health --report-only` and a change-scoped baseline comparison both publish
//! their verdict with `enforced: false` rather than hiding it, because a gate a
//! repository asked for that then says nothing is the failure mode this object
//! exists to remove.

use fallow_output::{GateName, GateOutcome, GateOutcomes, GateStatus};

/// `pass` or `fail` from a plain predicate.
pub const fn status_of(failed: bool) -> GateStatus {
    if failed {
        GateStatus::Fail
    } else {
        GateStatus::Pass
    }
}

/// The regression gate's outcome, `None` when no regression comparison ran.
///
/// `Skipped` maps to [`GateStatus::Skipped`] rather than to `pass`: the
/// comparison stood down (a change-scoped run, typically), and reporting that
/// as a pass would assert a judgement nothing made.
pub fn regression_outcome(
    outcome: Option<&crate::regression::RegressionOutcome>,
) -> Option<GateOutcome> {
    let outcome = outcome?;
    let status = match outcome {
        crate::regression::RegressionOutcome::Pass { .. } => GateStatus::Pass,
        crate::regression::RegressionOutcome::Exceeded { .. } => GateStatus::Fail,
        crate::regression::RegressionOutcome::Skipped { .. } => GateStatus::Skipped,
    };
    Some(GateOutcome::new(status, true))
}

/// The stale-baseline gate's outcome, `None` when no baseline was loaded.
///
/// Reads `gate_trips` off the envelope object the run already publishes, so the
/// two can never drift. `enforced` is the opt-in flag: the verdict is published
/// either way, and only the flag decides whether it fails the run.
pub const fn stale_baseline_outcome(
    staleness: Option<&fallow_output::BaselineStaleness>,
    fail_on_stale_baseline: bool,
) -> Option<GateOutcome> {
    let Some(staleness) = staleness else {
        return None;
    };
    if staleness.change_scoped {
        return Some(GateOutcome::new(GateStatus::Skipped, false));
    }
    Some(GateOutcome::new(
        status_of(staleness.gate_trips),
        fail_on_stale_baseline,
    ))
}

/// The type-aware completeness gate's outcome, `None` unless the `complete`
/// policy was requested.
pub fn type_aware_outcome(
    require: fallow_config::TypeAwareRequire,
    meta: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> Option<GateOutcome> {
    if require != fallow_config::TypeAwareRequire::Complete {
        return None;
    }
    let incomplete = crate::report::ci::required_type_aware_incomplete(meta);
    Some(GateOutcome::new(status_of(incomplete), true))
}

/// The severity gate `--fail-on-issues` arms.
///
/// Absent unless the flag was passed. The rule is severity-aware rather than a
/// count, so a project with a rule set to `warn` reports findings and still
/// passes; `--fail-on-issues` is what promotes those warns into the rule.
pub const fn error_severity_outcome(
    fail_on_issues: bool,
    has_error_severity: bool,
) -> Option<GateOutcome> {
    if !fail_on_issues {
        return None;
    }
    Some(GateOutcome::new(status_of(has_error_severity), true))
}

/// The duplication threshold gate, `None` when no threshold was configured.
///
/// A threshold of zero means "no limit", which is the CLI's own reading, so it
/// arms nothing.
pub fn duplication_threshold_outcome(
    threshold: f64,
    duplication_percentage: f64,
    exceeded: bool,
) -> Option<GateOutcome> {
    if threshold <= 0.0 {
        return None;
    }
    Some(GateOutcome::measured(
        status_of(exceeded),
        true,
        duplication_percentage,
        threshold,
    ))
}

/// Collect the gates a dead-code or check run evaluated.
pub struct CheckGateInputs<'a> {
    pub fail_on_issues: bool,
    pub has_error_severity: bool,
    pub regression: Option<&'a crate::regression::RegressionOutcome>,
    pub baseline_staleness: Option<&'a fallow_output::BaselineStaleness>,
    pub fail_on_stale_baseline: bool,
    pub type_aware_require: fallow_config::TypeAwareRequire,
    pub type_aware_meta: Option<&'a fallow_types::envelope::TypeAwareMeta>,
}

/// Build the dead-code envelope's `gate_outcomes`, `None` when nothing armed.
pub fn check_gate_outcomes(input: &CheckGateInputs<'_>) -> Option<GateOutcomes> {
    let mut gates = GateOutcomes::new();
    gates.insert_if(
        GateName::ErrorSeverityFindings,
        error_severity_outcome(input.fail_on_issues, input.has_error_severity),
    );
    gates.insert_if(GateName::Regression, regression_outcome(input.regression));
    gates.insert_if(
        GateName::StaleBaseline,
        stale_baseline_outcome(input.baseline_staleness, input.fail_on_stale_baseline),
    );
    gates.insert_if(
        GateName::TypeAwareRequire,
        type_aware_outcome(input.type_aware_require, input.type_aware_meta),
    );
    gates.into_option()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_that_arms_nothing_emits_no_object() {
        let gates = check_gate_outcomes(&CheckGateInputs {
            fail_on_issues: false,
            has_error_severity: true,
            regression: None,
            baseline_staleness: None,
            fail_on_stale_baseline: false,
            type_aware_require: fallow_config::TypeAwareRequire::BestEffort,
            type_aware_meta: None,
        });
        assert!(gates.is_none());
    }

    #[test]
    fn fail_on_issues_arms_the_severity_gate() {
        let gates = check_gate_outcomes(&CheckGateInputs {
            fail_on_issues: true,
            has_error_severity: true,
            regression: None,
            baseline_staleness: None,
            fail_on_stale_baseline: false,
            type_aware_require: fallow_config::TypeAwareRequire::BestEffort,
            type_aware_meta: None,
        })
        .expect("severity gate armed");
        let outcome = gates
            .get(GateName::ErrorSeverityFindings)
            .expect("entry present");
        assert_eq!(outcome.status, GateStatus::Fail);
        assert!(outcome.enforced);
    }

    #[test]
    fn a_published_baseline_verdict_is_unenforced_without_the_flag() {
        let staleness = fallow_output::BaselineStaleness {
            baseline_entries: 8,
            matched_entries: 3,
            stale_entries: 5,
            current_findings: 0,
            change_scoped: false,
            stale: false,
            warning: fallow_output::BaselineStalenessAdvisory::None,
            gate_trips: true,
            moved_entries: 0,
        };
        let unarmed = stale_baseline_outcome(Some(&staleness), false).expect("verdict published");
        assert_eq!(unarmed.status, GateStatus::Fail);
        assert!(!unarmed.enforced);
        assert!(!unarmed.fails_run());

        let armed = stale_baseline_outcome(Some(&staleness), true).expect("verdict published");
        assert!(armed.fails_run());
    }

    #[test]
    fn a_change_scoped_baseline_run_stands_down() {
        let staleness = fallow_output::BaselineStaleness {
            baseline_entries: 8,
            matched_entries: 0,
            stale_entries: 8,
            current_findings: 0,
            change_scoped: true,
            stale: false,
            warning: fallow_output::BaselineStalenessAdvisory::None,
            gate_trips: false,
            moved_entries: 0,
        };
        let outcome = stale_baseline_outcome(Some(&staleness), true).expect("verdict published");
        assert_eq!(outcome.status, GateStatus::Skipped);
        assert!(!outcome.fails_run());
    }

    #[test]
    fn a_zero_threshold_arms_no_duplication_gate() {
        assert!(duplication_threshold_outcome(0.0, 100.0, false).is_none());
        let outcome = duplication_threshold_outcome(5.0, 100.0, true).expect("gate armed");
        assert_eq!(outcome.status, GateStatus::Fail);
        assert_eq!(outcome.observed, Some(100.0));
        assert_eq!(outcome.threshold, Some(5.0));
    }

    #[test]
    fn a_skipped_regression_comparison_is_not_a_pass() {
        let outcome = regression_outcome(Some(&crate::regression::RegressionOutcome::Skipped {
            reason: "changed-since",
        }))
        .expect("comparison ran");
        assert_eq!(outcome.status, GateStatus::Skipped);
    }
}
