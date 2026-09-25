/// Coverage model used for CRAP score computation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageModel {
    /// Legacy binary tested/untested static model; no longer produced.
    #[allow(
        dead_code,
        reason = "retained for backwards-compatible JSON deserialization"
    )]
    StaticBinary,
    /// Coverage estimated from static test reachability.
    StaticEstimated,
    /// Coverage measured from a coverage map: Istanbul JSON, or raw V8
    /// coverage that fallow converts to the same per-statement model.
    /// `HealthSummary::coverage_input_format` names which one.
    Istanbul,
}

/// Input format of the measured coverage behind `CoverageModel::Istanbul`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageInputFormat {
    /// An Istanbul coverage map (`coverage-final.json`).
    Istanbul,
    /// Raw V8 coverage dumps, such as a `NODE_V8_COVERAGE` directory.
    V8,
}
