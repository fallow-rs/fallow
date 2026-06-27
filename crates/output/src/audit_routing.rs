//! Audit reviewer-routing output contracts.

use serde::Serialize;

/// One routed unit with its experts and bus-factor flag.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RoutingUnit {
    /// Root-relative path of the changed file.
    pub file: String,
    /// The routed expert(s): the CODEOWNERS declared owner when present, else the
    /// top git-blame / recency contributor; empty when no signal is available.
    pub expert: Vec<String>,
    /// Whether the only qualified owner is a single contributor (bus-factor-1):
    /// a knowledge-concentration risk worth a second reviewer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bus_factor_one: bool,
}

/// The full routing section: one unit per changed source file with a routable
/// signal. Files with no ownership signal are omitted (no noise).
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RoutingFacts {
    /// Per-changed-file routing units, sorted by file path.
    pub units: Vec<RoutingUnit>,
}
