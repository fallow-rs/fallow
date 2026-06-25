//! Audit focus-map output contracts.

use serde::Serialize;

/// The focus label for a review unit. There is no `Skip` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FocusLabel {
    /// Review this unit.
    ReviewHere,
    /// Not prioritized, but still visible in the escape-hatch list.
    NotPrioritized,
}

impl FocusLabel {
    /// The wire token.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::ReviewHere => "review-here",
            Self::NotPrioritized => "not-prioritized",
        }
    }
}

/// A per-unit confidence flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ConfidenceFlag {
    /// The unit is dynamically wired.
    DynamicDispatch,
    /// The unit's reachability runs through re-export barrels.
    ReExportIndirection,
}

impl ConfidenceFlag {
    /// The wire message for this flag.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::DynamicDispatch => "low: dynamic dispatch detected",
            Self::ReExportIndirection => "low: re-export indirection",
        }
    }
}

/// The composite attention score and component breakdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FocusScore {
    /// Fan-in/out blast-radius component.
    pub fan_io: u32,
    /// Security source to sink taint-touch component.
    pub security_taint: u32,
    /// Risk-zone component.
    pub risk_zone: u32,
    /// Change-shape component.
    pub change_shape: u32,
    /// The summed total.
    pub total: u32,
}

/// One review unit on the focus map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FocusUnit {
    /// Root-relative path of the changed file this unit covers.
    pub file: String,
    /// The composite attention score and its component breakdown.
    pub score: FocusScore,
    /// The focus label.
    pub label: FocusLabel,
    /// Human-readable reason for the label.
    pub reason: String,
    /// Confidence flags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub confidence: Vec<ConfidenceFlag>,
}

/// The weighted focus map.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FocusMap {
    /// Units labeled `review-here`.
    pub review_here: Vec<FocusUnit>,
    /// Every `not-prioritized` unit.
    pub deprioritized: Vec<FocusUnit>,
}

impl FocusMap {
    /// Total number of units.
    #[must_use]
    pub fn total_units(&self) -> usize {
        self.review_here.len() + self.deprioritized.len()
    }
}
