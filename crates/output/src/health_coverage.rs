/// Coverage model used for CRAP score computation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageModel {
    #[allow(
        dead_code,
        reason = "retained for backwards-compatible JSON deserialization"
    )]
    StaticBinary,
    StaticEstimated,
    Istanbul,
}
