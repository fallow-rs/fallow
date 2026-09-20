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
    enforced: bool,
) -> Option<GateOutcome> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "issue counts never approach the f64 integer limit"
    )]
    fn delta(baseline: usize, current: usize) -> f64 {
        current as f64 - baseline as f64
    }

    let outcome = outcome?;
    Some(match outcome {
        crate::regression::RegressionOutcome::Pass {
            baseline_total,
            current_total,
        } => GateOutcome {
            status: GateStatus::Pass,
            enforced,
            observed: Some(delta(*baseline_total, *current_total)),
            // A pass records no tolerance, matching `regression.tolerance`,
            // which is null on the same outcome.
            threshold: None,
            threshold_label: None,
        },
        crate::regression::RegressionOutcome::Exceeded {
            baseline_total,
            current_total,
            tolerance,
            ..
        } => GateOutcome {
            status: GateStatus::Fail,
            enforced,
            observed: Some(delta(*baseline_total, *current_total)),
            // The allowance rather than the tolerance's own number: a
            // percentage tolerance beside an absolute delta renders "12 against
            // 50" for a 50% allowance on a baseline of 3, which is a comparison
            // the reader cannot make. `allowed_delta` mirrors the gate's own
            // rule, so the two numbers are the ones the gate compared.
            threshold: Some(tolerance.allowed_delta(*baseline_total)),
            // The spelling the user passed, so the unit survives onto the
            // grouped envelope, which carries no `regression` object to read
            // `tolerance_kind` from.
            threshold_label: Some(tolerance.label()),
        },
        crate::regression::RegressionOutcome::Skipped { .. } => {
            GateOutcome::new(GateStatus::Skipped, enforced)
        }
    })
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

/// The severity rule that decides a dead-code, check or combined run's exit
/// code, whether or not `--fail-on-issues` was passed.
///
/// Severity-aware rather than a count: a project with a rule set to `warn`
/// reports findings and still passes, and `--fail-on-issues` is what promotes
/// those warns into the rule. It is always `enforced`, because this rule always
/// decides the exit code.
///
/// `--fail-on-issues` arms it, and it is ALSO emitted whenever the object
/// exists for any other reason. Without the second half, a run gated only on
/// `--fail-on-regression` could exit 1 for an error-severity finding while
/// every entry in its object reported a pass, leaving the object unable to
/// explain the exit code it sits beside. A run that arms nothing still
/// publishes no object, so nothing on the wire moves for it.
pub const fn error_severity_outcome(has_error_severity: bool) -> GateOutcome {
    GateOutcome::new(status_of(has_error_severity), true)
}

/// The duplication threshold gate, `None` when no threshold was configured.
///
/// A threshold of zero means "no limit", which is the CLI's own reading, so it
/// arms nothing.
pub fn duplication_threshold_outcome(
    threshold: f64,
    duplication_percentage: f64,
    enforced: bool,
) -> Option<GateOutcome> {
    if threshold <= 0.0 {
        return None;
    }
    Some(GateOutcome::measured(
        status_of(crate::dupes::exceeds_threshold(
            threshold,
            duplication_percentage,
        )),
        // Arming, not the verdict: `enforced` answers "would a failure here
        // fail the run", so a passing threshold gate on a command that exits
        // on it still reports true, exactly as the stale-baseline gate does.
        enforced,
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
        GateName::Regression,
        regression_outcome(input.regression, true),
    );
    gates.insert_if(
        GateName::StaleBaseline,
        stale_baseline_outcome(input.baseline_staleness, input.fail_on_stale_baseline),
    );
    gates.insert_if(
        GateName::TypeAwareRequire,
        type_aware_outcome(input.type_aware_require, input.type_aware_meta),
    );
    if input.fail_on_issues || !gates.is_empty() {
        gates.insert(
            GateName::ErrorSeverityFindings,
            error_severity_outcome(input.has_error_severity),
        );
    }
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
        assert!(
            gates.is_none(),
            "an ungated run stays byte-identical, even one the severity rule fails"
        );
    }

    /// The object has to explain the exit code it sits beside, so once it
    /// exists the rule that actually decides that exit code is in it.
    #[test]
    fn the_object_always_carries_the_rule_that_decides_the_exit_code() {
        let gates = check_gate_outcomes(&CheckGateInputs {
            fail_on_issues: false,
            has_error_severity: true,
            regression: Some(&crate::regression::RegressionOutcome::Pass {
                baseline_total: 1,
                current_total: 1,
            }),
            baseline_staleness: None,
            fail_on_stale_baseline: false,
            type_aware_require: fallow_config::TypeAwareRequire::BestEffort,
            type_aware_meta: None,
        })
        .expect("the regression gate armed the object");
        assert_eq!(
            gates.get(GateName::Regression).expect("armed").status,
            GateStatus::Pass
        );
        let outcome = gates
            .get(GateName::ErrorSeverityFindings)
            .expect("the default exit rule joins the object");
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
            unrecognised_format: false,
            scope_reasons: fallow_output::BaselineScopeReasons::empty(),
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
            unrecognised_format: false,
            scope_reasons: fallow_output::BaselineScopeReasons::empty()
                .with(fallow_output::ScopeReason::Production),
        };
        let outcome = stale_baseline_outcome(Some(&staleness), true).expect("verdict published");
        assert_eq!(outcome.status, GateStatus::Skipped);
        assert!(!outcome.fails_run());
    }

    #[test]
    fn a_zero_threshold_arms_no_duplication_gate() {
        assert!(duplication_threshold_outcome(0.0, 100.0, true).is_none());
        let outcome = duplication_threshold_outcome(5.0, 100.0, true).expect("gate armed");
        assert_eq!(outcome.status, GateStatus::Fail);
        assert_eq!(outcome.observed, Some(100.0));
        assert_eq!(outcome.threshold, Some(5.0));
    }

    #[test]
    fn a_skipped_regression_comparison_is_not_a_pass() {
        let outcome = regression_outcome(
            Some(&crate::regression::RegressionOutcome::Skipped {
                reason: "changed-since",
            }),
            true,
        )
        .expect("comparison ran");
        assert_eq!(outcome.status, GateStatus::Skipped);
    }
}
