//! Audit weakening-signal output contracts.

use serde::Serialize;

/// The category of a single weakening signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum WeakeningKind {
    /// A test was removed or skipped.
    TestWeakened,
    /// A coverage or quality threshold was lowered.
    ThresholdLowered,
    /// A suppression was added.
    SuppressionAdded,
    /// A security check or step was removed from CI.
    SecurityCheckRemoved,
}

/// One weakening signal.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WeakeningSignal {
    /// What kind of guardrail was weakened.
    pub kind: WeakeningKind,
    /// Root-relative changed file path.
    pub file: String,
    /// Short evidence string.
    pub evidence: String,
}
