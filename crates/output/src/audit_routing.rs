//! Audit reviewer-routing output contracts.

use serde::Serialize;

/// One routed unit with its experts and bus-factor flag.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RoutingUnit {
    /// Root-relative path of the changed file.
    pub file: String,
    /// Routed expert(s), when ownership signals are available.
    pub expert: Vec<String>,
    /// Whether the only qualified owner is a single contributor.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bus_factor_one: bool,
}

/// The full routing section.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RoutingFacts {
    /// Per-changed-file routing units, sorted by file path.
    pub units: Vec<RoutingUnit>,
}
