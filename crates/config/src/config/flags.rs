use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A custom SDK call pattern for feature flag detection.
///
/// Describes a function call that evaluates a feature flag, e.g.,
/// `useFlag('new-checkout')` or `client.getFeatureValue('parser', false)`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SdkPattern {
    /// Function name to match (e.g., `"useFlag"`, `"variation"`).
    pub function: String,
    /// Zero-based index of the argument containing the flag name.
    #[serde(default)]
    pub name_arg: usize,
    /// Optional SDK/provider label shown in output (e.g., `"LaunchDarkly"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// Feature flag detection configuration.
///
/// Controls which patterns fallow uses to detect feature flags in source code.
/// Configured via the `flags` section in `.fallowrc.json`, `.fallowrc.jsonc`, `fallow.toml`, or `.fallow.toml`.
///
/// # Examples
///
/// ```json
/// {
///   "flags": {
///     "sdkPatterns": [
///       { "function": "useFlag", "nameArg": 0, "provider": "LaunchDarkly" }
///     ],
///     "envPrefixes": ["FEATURE_", "NEXT_PUBLIC_ENABLE_"],
///     "configObjectHeuristics": false
///   }
/// }
/// ```
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FlagsConfig {
    /// Additional SDK call patterns to detect as feature flags.
    /// These are merged with the built-in patterns for common providers
    /// including LaunchDarkly, Statsig, Unleash, GrowthBook, Split, PostHog,
    /// Vercel Flags, ConfigCat, Flagsmith, Optimizely, and Eppo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sdk_patterns: Vec<SdkPattern>,

    /// Environment variable prefixes that indicate feature flags.
    /// Merged with built-in prefixes. Only `process.env.*` and
    /// `import.meta.env.*` accesses matching these prefixes are reported as
    /// feature flags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_prefixes: Vec<String>,

    /// Enable config object heuristic detection.
    /// When true, property accesses on objects whose name contains "feature",
    /// "flag", or "toggle" are reported as low-confidence feature flags.
    /// Default: false (opt-in due to higher false positive rate).
    #[serde(default)]
    pub config_object_heuristics: bool,

    /// Prefix to remove from each key of a `--flag-state` vendor export
    /// before the key is compared with the flag names in the code. For
    /// example, `"web."` makes the vendor key `web.new-checkout` match the
    /// code flag `new-checkout`. A key without the prefix is compared as it
    /// is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor_key_prefix: Option<String>,
}

impl FlagsConfig {
    /// The detection patterns this config adds to the built-in ones.
    #[must_use]
    pub fn patterns(&self) -> fallow_types::extract::FlagPatterns {
        fallow_types::extract::FlagPatterns {
            sdk_patterns: self
                .sdk_patterns
                .iter()
                .map(|pattern| {
                    (
                        pattern.function.clone(),
                        pattern.name_arg,
                        pattern.provider.clone().unwrap_or_default(),
                    )
                })
                .collect(),
            env_prefixes: self.env_prefixes.clone(),
            config_object_heuristics: self.config_object_heuristics,
        }
    }
}
