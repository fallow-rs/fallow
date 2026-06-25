//! Shared churn output contracts.

/// Churn trend indicator based on comparing recent vs older halves of the
/// analysis period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ChurnTrend {
    /// Recent half has >1.5x the commits of the older half.
    Accelerating,
    /// Churn is roughly stable between halves.
    Stable,
    /// Recent half has <0.67x the commits of the older half.
    Cooling,
}

impl std::fmt::Display for ChurnTrend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Accelerating => write!(f, "accelerating"),
            Self::Stable => write!(f, "stable"),
            Self::Cooling => write!(f, "cooling"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn churn_trend_display_matches_wire_values() {
        assert_eq!(ChurnTrend::Accelerating.to_string(), "accelerating");
        assert_eq!(ChurnTrend::Stable.to_string(), "stable");
        assert_eq!(ChurnTrend::Cooling.to_string(), "cooling");
    }

    #[test]
    fn churn_trend_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&ChurnTrend::Accelerating).unwrap(),
            r#""accelerating""#,
        );
        assert_eq!(
            serde_json::to_string(&ChurnTrend::Stable).unwrap(),
            r#""stable""#,
        );
        assert_eq!(
            serde_json::to_string(&ChurnTrend::Cooling).unwrap(),
            r#""cooling""#,
        );
    }
}
