use std::path::Path;

use fallow_output::{ExceededThreshold, FindingSeverity};

use super::super::tests::make_health_finding_with;
use super::*;
use crate::baseline::{HealthBaselineData, HealthBaselineMode, filter_new_health_findings};

const ROOT: &str = "/project";

/// A complexity finding of the function `name` in `src/utils.ts`.
fn finding(name: &str, severity: FindingSeverity) -> fallow_output::ComplexityViolation {
    make_health_finding_with(
        Path::new(ROOT),
        name,
        1,
        ExceededThreshold::Cyclomatic,
        severity,
    )
}

/// A baseline saved the way `--baseline-mode identity` saves it.
fn identity_baseline(findings: &[fallow_output::ComplexityViolation]) -> HealthBaselineData {
    HealthBaselineData::from_findings(findings, &[], &[], Path::new(ROOT))
        .with_identity(findings, Path::new(ROOT))
}

#[test]
fn used_slots_follow_the_greedy_match() {
    let used = |current, saved| match_severity_slots(current, saved).used;
    assert_eq!(used([0, 1, 0], [1, 1, 0]), [0, 1, 0]);
    assert_eq!(used([1, 0, 0], [0, 0, 1]), [0, 0, 1]);
    assert_eq!(used([0, 0, 1], [1, 0, 0]), [0, 0, 0]);
    assert_eq!(used([2, 1, 0], [1, 1, 2]), [1, 1, 1]);
    assert_eq!(used([5, 5, 5], [1, 1, 1]), [1, 1, 1]);
}

#[test]
fn a_severity_overflows_when_no_compatible_slot_is_free() {
    let overflow = |current, saved| match_severity_slots(current, saved).overflow;
    assert_eq!(overflow([1, 0, 0], [0, 0, 1]), [false, false, false]);
    assert_eq!(overflow([0, 0, 1], [1, 1, 0]), [false, false, true]);
    assert_eq!(overflow([1, 1, 0], [0, 1, 0]), [false, true, false]);
    assert_eq!(overflow([2, 0, 0], [1, 0, 0]), [true, false, false]);
}

#[test]
fn a_placeholder_slot_that_covers_its_own_finding_is_not_lent() {
    let baseline = identity_baseline(&[finding("<arrow>", FindingSeverity::Critical)]);
    let filtered = filter_new_health_findings(
        vec![
            finding("<arrow>", FindingSeverity::Moderate),
            finding("newHotspot", FindingSeverity::Critical),
        ],
        &baseline,
        Path::new(ROOT),
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1, "one saved slot hides one finding");
    assert_eq!(filtered[0].name, "newHotspot");
}
