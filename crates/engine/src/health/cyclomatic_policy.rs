//! The cyclomatic value that the health gates read under
//! `health.cyclomaticPolicy`.

use fallow_config::CyclomaticPolicy;
use fallow_types::extract::{ComplexityContribution, ComplexityMetric, FunctionComplexity};

/// The cyclomatic value that the `maxCyclomatic` gate and the CRAP score read.
///
/// Under `standard` this is the extracted `McCabe` value. Under
/// `literal-membership` the `||` increments of literal membership chains are
/// removed, so a chain counts as one predicate. The value never drops below 1.
#[must_use]
pub(super) fn gate_cyclomatic(function: &FunctionComplexity, policy: CyclomaticPolicy) -> u16 {
    match policy {
        CyclomaticPolicy::Standard => function.cyclomatic,
        CyclomaticPolicy::LiteralMembership => function
            .cyclomatic
            .saturating_sub(literal_membership_increments(function)),
    }
}

/// Cyclomatic increments that belong to a literal membership chain, capped so
/// that the result never drops below 1.
fn literal_membership_increments(function: &FunctionComplexity) -> u16 {
    let count = function
        .contributions
        .iter()
        .filter(|contribution| {
            contribution.literal_membership
                && matches!(contribution.metric, ComplexityMetric::Cyclomatic)
        })
        .map(|contribution| contribution.weight)
        .fold(0_u16, u16::saturating_add);
    count.min(function.cyclomatic.saturating_sub(1))
}

/// The standard value for a finding when the policy changed it, else `None`.
#[must_use]
pub(super) fn standard_cyclomatic(
    function: &FunctionComplexity,
    policy: CyclomaticPolicy,
) -> Option<u16> {
    let gated = gate_cyclomatic(function, policy);
    (gated != function.cyclomatic).then_some(function.cyclomatic)
}

/// The breakdown for a finding. Under `standard` the `literal_membership`
/// flags are cleared, so the default breakdown output does not change.
#[must_use]
pub(super) fn breakdown_contributions(
    function: &FunctionComplexity,
    policy: CyclomaticPolicy,
) -> Vec<ComplexityContribution> {
    let mut contributions = function.contributions.clone();
    if policy.is_standard() {
        for contribution in &mut contributions {
            contribution.literal_membership = false;
        }
    }
    contributions
}
