use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

const fn default_threshold() -> f64 {
    0.80
}

const fn default_min_lines() -> usize {
    3
}

fn deserialize_threshold<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = f64::deserialize(deserializer)?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(serde::de::Error::custom(format!(
            "similarCode.threshold must be finite and between 0 and 1 (got {value})"
        )));
    }
    Ok(value)
}

fn deserialize_min_lines<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let value = usize::deserialize(deserializer)?;
    if value == 0 {
        return Err(serde::de::Error::custom(
            "similarCode.minLines must be at least 1",
        ));
    }
    Ok(value)
}

/// Project-owned tuning for the explicit `fallow similar-code` workflow.
///
/// Provider identity, executable discovery, model setup, credentials, and
/// consent are intentionally absent. Project configuration cannot select code
/// destinations or authorize model downloads.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimilarCodeConfig {
    /// Minimum model-specific cosine similarity retained as an unverified
    /// candidate. It is not a probability or refactor-safety verdict.
    #[serde(
        default = "default_threshold",
        deserialize_with = "deserialize_threshold"
    )]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub threshold: f64,

    /// Minimum source line count for a function to enter model inference.
    #[serde(
        default = "default_min_lines",
        deserialize_with = "deserialize_min_lines"
    )]
    #[schemars(range(min = 1))]
    pub min_lines: usize,

    /// Additional project-root-relative globs excluded only from similar-code
    /// extraction. Global `ignorePatterns` remain authoritative first.
    #[serde(default)]
    pub ignore: Vec<String>,
}

impl Default for SimilarCodeConfig {
    fn default() -> Self {
        Self {
            threshold: default_threshold(),
            min_lines: default_min_lines(),
            ignore: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_candidate_only_calibration() {
        let config = SimilarCodeConfig::default();
        assert!((config.threshold - 0.80).abs() < f64::EPSILON);
        assert_eq!(config.min_lines, 3);
        assert_eq!(config.ignore, [] as [String; 0]);
    }

    #[test]
    fn invalid_threshold_and_line_floor_fail_loud() {
        let threshold = serde_json::from_str::<SimilarCodeConfig>(r#"{"threshold":1.1}"#)
            .unwrap_err()
            .to_string();
        assert!(threshold.contains("between 0 and 1"));
        let lines = serde_json::from_str::<SimilarCodeConfig>(r#"{"minLines":0}"#)
            .unwrap_err()
            .to_string();
        assert!(lines.contains("at least 1"));
    }
}
