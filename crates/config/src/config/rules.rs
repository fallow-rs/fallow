use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Severity level for rules.
///
/// Controls whether an issue type causes CI failure (`error`), is reported
/// without failing (`warn`), or is suppressed entirely (`off`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Report and fail CI (non-zero exit code).
    #[default]
    Error,
    /// Report but don't fail CI.
    Warn,
    /// Don't detect or report.
    Off,
}

impl Severity {
    /// Default value for fields that should default to `Warn` instead of `Error`.
    const fn default_warn() -> Self {
        Self::Warn
    }

    /// Default value for fields that should default to `Off`.
    const fn default_off() -> Self {
        Self::Off
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warn => write!(f, "warn"),
            Self::Off => write!(f, "off"),
        }
    }
}

impl std::str::FromStr for Severity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "error" => Ok(Self::Error),
            "warn" | "warning" => Ok(Self::Warn),
            "off" | "none" => Ok(Self::Off),
            other => Err(format!(
                "unknown severity: '{other}' (expected error, warn, or off)"
            )),
        }
    }
}

/// Per-issue-type severity configuration.
///
/// Controls which issue types cause CI failure, are reported as warnings,
/// or are suppressed entirely. Most fields default to `Severity::Error`.
///
/// Rule names use kebab-case in config files (e.g., `"unused-files": "error"`).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct RulesConfig {
    #[serde(default, alias = "unused-file")]
    pub unused_files: Severity,
    #[serde(default, alias = "unused-export")]
    pub unused_exports: Severity,
    #[serde(default, alias = "unused-type")]
    pub unused_types: Severity,
    #[serde(default = "Severity::default_off", alias = "private-type-leak")]
    pub private_type_leaks: Severity,
    #[serde(default, alias = "unused-dependency")]
    pub unused_dependencies: Severity,
    #[serde(default = "Severity::default_warn", alias = "unused-dev-dependency")]
    pub unused_dev_dependencies: Severity,
    #[serde(
        default = "Severity::default_warn",
        alias = "unused-optional-dependency"
    )]
    pub unused_optional_dependencies: Severity,
    #[serde(default, alias = "unused-enum-member")]
    pub unused_enum_members: Severity,
    #[serde(default, alias = "unused-class-member")]
    pub unused_class_members: Severity,
    #[serde(default, alias = "unresolved-import")]
    pub unresolved_imports: Severity,
    #[serde(default, alias = "unlisted-dependency")]
    pub unlisted_dependencies: Severity,
    #[serde(default, alias = "duplicate-export")]
    pub duplicate_exports: Severity,
    #[serde(default = "Severity::default_warn", alias = "type-only-dependency")]
    pub type_only_dependencies: Severity,
    #[serde(default = "Severity::default_warn", alias = "test-only-dependency")]
    pub test_only_dependencies: Severity,
    #[serde(default, alias = "circular-dependency")]
    pub circular_dependencies: Severity,
    #[serde(default, alias = "boundary-violations")]
    pub boundary_violation: Severity,
    #[serde(default, alias = "coverage-gap")]
    pub coverage_gaps: Severity,
    #[serde(default = "Severity::default_off", alias = "feature-flag")]
    pub feature_flags: Severity,
    #[serde(default = "Severity::default_warn", alias = "stale-suppression")]
    pub stale_suppressions: Severity,
    #[serde(default = "Severity::default_warn", alias = "unused-catalog-entry")]
    pub unused_catalog_entries: Severity,
    #[serde(default = "Severity::default_warn", alias = "empty-catalog-group")]
    pub empty_catalog_groups: Severity,
    #[serde(default, alias = "unresolved-catalog-reference")]
    pub unresolved_catalog_references: Severity,
    #[serde(
        default = "Severity::default_warn",
        alias = "unused-dependency-override"
    )]
    pub unused_dependency_overrides: Severity,
    #[serde(default, alias = "misconfigured-dependency-override")]
    pub misconfigured_dependency_overrides: Severity,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            unused_files: Severity::Error,
            unused_exports: Severity::Error,
            unused_types: Severity::Error,
            private_type_leaks: Severity::Off,
            unused_dependencies: Severity::Error,
            unused_dev_dependencies: Severity::Warn,
            unused_optional_dependencies: Severity::Warn,
            unused_enum_members: Severity::Error,
            unused_class_members: Severity::Error,
            unresolved_imports: Severity::Error,
            unlisted_dependencies: Severity::Error,
            duplicate_exports: Severity::Error,
            type_only_dependencies: Severity::Warn,
            test_only_dependencies: Severity::Warn,
            circular_dependencies: Severity::Error,
            boundary_violation: Severity::Error,
            coverage_gaps: Severity::Off,
            feature_flags: Severity::Off,
            stale_suppressions: Severity::Warn,
            unused_catalog_entries: Severity::Warn,
            empty_catalog_groups: Severity::Warn,
            unresolved_catalog_references: Severity::Error,
            unused_dependency_overrides: Severity::Warn,
            misconfigured_dependency_overrides: Severity::Error,
        }
    }
}

impl RulesConfig {
    /// Apply a partial rules config on top. Only `Some` fields override.
    pub const fn apply_partial(&mut self, partial: &PartialRulesConfig) {
        if let Some(s) = partial.unused_files {
            self.unused_files = s;
        }
        if let Some(s) = partial.unused_exports {
            self.unused_exports = s;
        }
        if let Some(s) = partial.unused_types {
            self.unused_types = s;
        }
        if let Some(s) = partial.private_type_leaks {
            self.private_type_leaks = s;
        }
        if let Some(s) = partial.unused_dependencies {
            self.unused_dependencies = s;
        }
        if let Some(s) = partial.unused_dev_dependencies {
            self.unused_dev_dependencies = s;
        }
        if let Some(s) = partial.unused_optional_dependencies {
            self.unused_optional_dependencies = s;
        }
        if let Some(s) = partial.unused_enum_members {
            self.unused_enum_members = s;
        }
        if let Some(s) = partial.unused_class_members {
            self.unused_class_members = s;
        }
        if let Some(s) = partial.unresolved_imports {
            self.unresolved_imports = s;
        }
        if let Some(s) = partial.unlisted_dependencies {
            self.unlisted_dependencies = s;
        }
        if let Some(s) = partial.duplicate_exports {
            self.duplicate_exports = s;
        }
        if let Some(s) = partial.type_only_dependencies {
            self.type_only_dependencies = s;
        }
        if let Some(s) = partial.test_only_dependencies {
            self.test_only_dependencies = s;
        }
        if let Some(s) = partial.circular_dependencies {
            self.circular_dependencies = s;
        }
        if let Some(s) = partial.boundary_violation {
            self.boundary_violation = s;
        }
        if let Some(s) = partial.coverage_gaps {
            self.coverage_gaps = s;
        }
        if let Some(s) = partial.feature_flags {
            self.feature_flags = s;
        }
        if let Some(s) = partial.stale_suppressions {
            self.stale_suppressions = s;
        }
        if let Some(s) = partial.unused_catalog_entries {
            self.unused_catalog_entries = s;
        }
        if let Some(s) = partial.empty_catalog_groups {
            self.empty_catalog_groups = s;
        }
        if let Some(s) = partial.unresolved_catalog_references {
            self.unresolved_catalog_references = s;
        }
        if let Some(s) = partial.unused_dependency_overrides {
            self.unused_dependency_overrides = s;
        }
        if let Some(s) = partial.misconfigured_dependency_overrides {
            self.misconfigured_dependency_overrides = s;
        }
    }
}

/// Partial per-issue-type severity for overrides. All fields optional.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct PartialRulesConfig {
    #[serde(
        default,
        alias = "unused-file",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_files: Option<Severity>,
    #[serde(
        default,
        alias = "unused-export",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_exports: Option<Severity>,
    #[serde(
        default,
        alias = "unused-type",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_types: Option<Severity>,
    #[serde(
        default,
        alias = "private-type-leak",
        skip_serializing_if = "Option::is_none"
    )]
    pub private_type_leaks: Option<Severity>,
    #[serde(
        default,
        alias = "unused-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "unused-dev-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_dev_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "unused-optional-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_optional_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "unused-enum-member",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_enum_members: Option<Severity>,
    #[serde(
        default,
        alias = "unused-class-member",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_class_members: Option<Severity>,
    #[serde(
        default,
        alias = "unresolved-import",
        skip_serializing_if = "Option::is_none"
    )]
    pub unresolved_imports: Option<Severity>,
    #[serde(
        default,
        alias = "unlisted-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unlisted_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "duplicate-export",
        skip_serializing_if = "Option::is_none"
    )]
    pub duplicate_exports: Option<Severity>,
    #[serde(
        default,
        alias = "type-only-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub type_only_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "test-only-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub test_only_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "circular-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub circular_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "boundary-violations",
        skip_serializing_if = "Option::is_none"
    )]
    pub boundary_violation: Option<Severity>,
    #[serde(
        default,
        alias = "coverage-gap",
        skip_serializing_if = "Option::is_none"
    )]
    pub coverage_gaps: Option<Severity>,
    #[serde(
        default,
        alias = "feature-flag",
        skip_serializing_if = "Option::is_none"
    )]
    pub feature_flags: Option<Severity>,
    #[serde(
        default,
        alias = "stale-suppression",
        skip_serializing_if = "Option::is_none"
    )]
    pub stale_suppressions: Option<Severity>,
    #[serde(
        default,
        alias = "unused-catalog-entry",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_catalog_entries: Option<Severity>,
    #[serde(
        default,
        alias = "empty-catalog-group",
        skip_serializing_if = "Option::is_none"
    )]
    pub empty_catalog_groups: Option<Severity>,
    #[serde(
        default,
        alias = "unresolved-catalog-reference",
        skip_serializing_if = "Option::is_none"
    )]
    pub unresolved_catalog_references: Option<Severity>,
    #[serde(
        default,
        alias = "unused-dependency-override",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_dependency_overrides: Option<Severity>,
    #[serde(
        default,
        alias = "misconfigured-dependency-override",
        skip_serializing_if = "Option::is_none"
    )]
    pub misconfigured_dependency_overrides: Option<Severity>,
}

/// Every rule name accepted by `RulesConfig` deserialization, in kebab-case.
///
/// Includes both the canonical name produced by `#[serde(rename_all = "kebab-case")]`
/// and every `#[serde(alias = ...)]` value. Used by
/// [`find_unknown_rule_keys`] to detect typos in user-supplied configs and
/// emit a `tracing::warn!` suggestion at config load time.
///
/// Keep in sync with the `#[serde]` attributes on `RulesConfig` and
/// `PartialRulesConfig`; the `known_rule_names_count_matches_struct` test
/// fails when the lists drift.
pub const KNOWN_RULE_NAMES: &[&str] = &[
    // canonical kebab-case names (rename_all = "kebab-case")
    "unused-files",
    "unused-exports",
    "unused-types",
    "private-type-leaks",
    "unused-dependencies",
    "unused-dev-dependencies",
    "unused-optional-dependencies",
    "unused-enum-members",
    "unused-class-members",
    "unresolved-imports",
    "unlisted-dependencies",
    "duplicate-exports",
    "type-only-dependencies",
    "test-only-dependencies",
    "circular-dependencies",
    "boundary-violation",
    "coverage-gaps",
    "feature-flags",
    "stale-suppressions",
    "unused-catalog-entries",
    "empty-catalog-groups",
    "unresolved-catalog-references",
    "unused-dependency-overrides",
    "misconfigured-dependency-overrides",
    // serde aliases (singular forms, plus the `boundary-violations` legacy plural)
    "unused-file",
    "unused-export",
    "unused-type",
    "private-type-leak",
    "unused-dependency",
    "unused-dev-dependency",
    "unused-optional-dependency",
    "unused-enum-member",
    "unused-class-member",
    "unresolved-import",
    "unlisted-dependency",
    "duplicate-export",
    "type-only-dependency",
    "test-only-dependency",
    "circular-dependency",
    "boundary-violations",
    "coverage-gap",
    "feature-flag",
    "stale-suppression",
    "unused-catalog-entry",
    "empty-catalog-group",
    "unresolved-catalog-reference",
    "unused-dependency-override",
    "misconfigured-dependency-override",
];

/// Levenshtein edit distance between two ASCII-leaning strings.
///
/// Uses two `Vec<usize>` rows so the working set stays bounded; rule names are
/// short (max ~33 chars) so allocation cost is negligible at config-load time.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let (a_len, b_len) = (a_bytes.len(), b_bytes.len());

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr: Vec<usize> = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = usize::from(a_bytes[i - 1] != b_bytes[j - 1]);
            curr[j] = (prev[j] + 1) // deletion
                .min(curr[j - 1] + 1) // insertion
                .min(prev[j - 1] + cost); // substitution
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Find the closest known rule name to `input` when it is plausibly a typo.
///
/// Returns the best match when the Levenshtein distance is at most 2 AND
/// the input is long enough that the match isn't coincidental
/// (`input.len() / 2 > distance`). Returns `None` for completely novel
/// strings where a suggestion would be misleading.
#[must_use]
pub fn closest_known_rule_name(input: &str) -> Option<&'static str> {
    let input_lower = input.to_ascii_lowercase();
    let mut best: Option<(&'static str, usize)> = None;

    for &candidate in KNOWN_RULE_NAMES {
        let d = levenshtein(&input_lower, candidate);
        if best.is_none_or(|(_, b_dist)| d < b_dist) {
            best = Some((candidate, d));
        }
    }

    best.filter(|&(_, d)| d > 0 && d <= 2 && input_lower.len() / 2 > d)
        .map(|(name, _)| name)
}

/// An unknown key found inside a `rules` (or `overrides[].rules`) object.
///
/// Surfaced by [`find_unknown_rule_keys`] so the caller (config loader) can
/// emit one `tracing::warn!` per entry without coupling the detection logic
/// to a tracing subscriber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownRuleKey {
    /// Human-readable source label, e.g. `"rules"` or `"overrides[2].rules"`.
    pub context: String,
    /// The unknown key as it appeared in the user's config.
    pub key: String,
    /// Closest known rule name when one is within plausible-typo distance.
    pub suggestion: Option<&'static str>,
}

/// Collect every unknown key from a `rules`-shaped JSON object.
///
/// Returns an empty `Vec` when `value` is not an object or every key is
/// recognized (canonical kebab-case or a documented alias). Called from
/// [`crate::config::parsing`] after `extends` merge and before
/// `serde_json::from_value::<FallowConfig>`, so the warning lists keys from
/// the final merged config rather than per-file partials.
#[must_use]
pub fn find_unknown_rule_keys(value: &serde_json::Value, context: &str) -> Vec<UnknownRuleKey> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };

    map.keys()
        .filter(|key| !KNOWN_RULE_NAMES.contains(&key.as_str()))
        .map(|key| UnknownRuleKey {
            context: context.to_owned(),
            key: key.clone(),
            suggestion: closest_known_rule_name(key),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_default_severities() {
        let rules = RulesConfig::default();
        assert_eq!(rules.unused_files, Severity::Error);
        assert_eq!(rules.unused_exports, Severity::Error);
        assert_eq!(rules.unused_types, Severity::Error);
        assert_eq!(rules.private_type_leaks, Severity::Off);
        assert_eq!(rules.unused_dependencies, Severity::Error);
        assert_eq!(rules.unused_dev_dependencies, Severity::Warn);
        assert_eq!(rules.unused_optional_dependencies, Severity::Warn);
        assert_eq!(rules.unused_enum_members, Severity::Error);
        assert_eq!(rules.unused_class_members, Severity::Error);
        assert_eq!(rules.unresolved_imports, Severity::Error);
        assert_eq!(rules.unlisted_dependencies, Severity::Error);
        assert_eq!(rules.duplicate_exports, Severity::Error);
        assert_eq!(rules.type_only_dependencies, Severity::Warn);
        assert_eq!(rules.test_only_dependencies, Severity::Warn);
        assert_eq!(rules.circular_dependencies, Severity::Error);
        assert_eq!(rules.boundary_violation, Severity::Error);
        assert_eq!(rules.coverage_gaps, Severity::Off);
        assert_eq!(rules.feature_flags, Severity::Off);
        assert_eq!(rules.stale_suppressions, Severity::Warn);
        assert_eq!(rules.unused_catalog_entries, Severity::Warn);
        assert_eq!(rules.empty_catalog_groups, Severity::Warn);
        assert_eq!(rules.unresolved_catalog_references, Severity::Error);
    }

    #[test]
    fn rules_deserialize_kebab_case() {
        let json_str = r#"{
            "unused-files": "error",
            "unused-exports": "warn",
            "unused-types": "off"
        }"#;
        let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(rules.unused_files, Severity::Error);
        assert_eq!(rules.unused_exports, Severity::Warn);
        assert_eq!(rules.unused_types, Severity::Off);
        // Unset fields default to error
        assert_eq!(rules.unresolved_imports, Severity::Error);
    }

    #[test]
    fn rules_deserialize_circular_dependency_alias() {
        let json_str = r#"{
            "circular-dependency": "off"
        }"#;
        let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(rules.circular_dependencies, Severity::Off);
    }

    #[test]
    fn rules_deserialize_boundary_violations_alias() {
        let json_str = r#"{
            "boundary-violations": "off"
        }"#;
        let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(rules.boundary_violation, Severity::Off);

        let partial: PartialRulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(partial.boundary_violation, Some(Severity::Off));
    }

    #[test]
    fn rules_deserialize_singular_aliases_for_every_plural_rule() {
        // Mirrors the LSP's per-diagnostic singular codes (e.g. "unused-export")
        // so users who copy the form they see in IDE warnings into their config
        // get their stated severity honored instead of silently dropped.
        let json_str = r#"{
            "unused-file": "off",
            "unused-export": "off",
            "unused-type": "off",
            "private-type-leak": "warn",
            "unused-dependency": "off",
            "unused-dev-dependency": "off",
            "unused-optional-dependency": "off",
            "unused-enum-member": "off",
            "unused-class-member": "off",
            "unresolved-import": "off",
            "unlisted-dependency": "off",
            "duplicate-export": "off",
            "type-only-dependency": "off",
            "test-only-dependency": "off",
            "coverage-gap": "warn",
            "feature-flag": "warn",
            "stale-suppression": "off",
            "unused-catalog-entry": "error",
            "empty-catalog-group": "error",
            "unresolved-catalog-reference": "warn"
        }"#;

        let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(rules.unused_files, Severity::Off);
        assert_eq!(rules.unused_exports, Severity::Off);
        assert_eq!(rules.unused_types, Severity::Off);
        assert_eq!(rules.private_type_leaks, Severity::Warn);
        assert_eq!(rules.unused_dependencies, Severity::Off);
        assert_eq!(rules.unused_dev_dependencies, Severity::Off);
        assert_eq!(rules.unused_optional_dependencies, Severity::Off);
        assert_eq!(rules.unused_enum_members, Severity::Off);
        assert_eq!(rules.unused_class_members, Severity::Off);
        assert_eq!(rules.unresolved_imports, Severity::Off);
        assert_eq!(rules.unlisted_dependencies, Severity::Off);
        assert_eq!(rules.duplicate_exports, Severity::Off);
        assert_eq!(rules.type_only_dependencies, Severity::Off);
        assert_eq!(rules.test_only_dependencies, Severity::Off);
        assert_eq!(rules.coverage_gaps, Severity::Warn);
        assert_eq!(rules.feature_flags, Severity::Warn);
        assert_eq!(rules.stale_suppressions, Severity::Off);
        assert_eq!(rules.unused_catalog_entries, Severity::Error);
        assert_eq!(rules.empty_catalog_groups, Severity::Error);
        assert_eq!(rules.unresolved_catalog_references, Severity::Warn);

        let partial: PartialRulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(partial.unused_files, Some(Severity::Off));
        assert_eq!(partial.unused_exports, Some(Severity::Off));
        assert_eq!(partial.unused_types, Some(Severity::Off));
        assert_eq!(partial.private_type_leaks, Some(Severity::Warn));
        assert_eq!(partial.unused_dependencies, Some(Severity::Off));
        assert_eq!(partial.unused_dev_dependencies, Some(Severity::Off));
        assert_eq!(partial.unused_optional_dependencies, Some(Severity::Off));
        assert_eq!(partial.unused_enum_members, Some(Severity::Off));
        assert_eq!(partial.unused_class_members, Some(Severity::Off));
        assert_eq!(partial.unresolved_imports, Some(Severity::Off));
        assert_eq!(partial.unlisted_dependencies, Some(Severity::Off));
        assert_eq!(partial.duplicate_exports, Some(Severity::Off));
        assert_eq!(partial.type_only_dependencies, Some(Severity::Off));
        assert_eq!(partial.test_only_dependencies, Some(Severity::Off));
        assert_eq!(partial.coverage_gaps, Some(Severity::Warn));
        assert_eq!(partial.feature_flags, Some(Severity::Warn));
        assert_eq!(partial.stale_suppressions, Some(Severity::Off));
        assert_eq!(partial.unused_catalog_entries, Some(Severity::Error));
        assert_eq!(partial.empty_catalog_groups, Some(Severity::Error));
        assert_eq!(partial.unresolved_catalog_references, Some(Severity::Warn));
    }

    #[test]
    fn severity_from_str() {
        assert_eq!("error".parse::<Severity>().unwrap(), Severity::Error);
        assert_eq!("warn".parse::<Severity>().unwrap(), Severity::Warn);
        assert_eq!("warning".parse::<Severity>().unwrap(), Severity::Warn);
        assert_eq!("off".parse::<Severity>().unwrap(), Severity::Off);
        assert_eq!("none".parse::<Severity>().unwrap(), Severity::Off);
        assert!("invalid".parse::<Severity>().is_err());
    }

    #[test]
    fn apply_partial_only_some_fields() {
        let mut rules = RulesConfig::default();
        let partial = PartialRulesConfig {
            unused_files: Some(Severity::Warn),
            unused_exports: Some(Severity::Off),
            ..Default::default()
        };
        rules.apply_partial(&partial);
        assert_eq!(rules.unused_files, Severity::Warn);
        assert_eq!(rules.unused_exports, Severity::Off);
        // Unset fields unchanged
        assert_eq!(rules.unused_types, Severity::Error);
        assert_eq!(rules.unresolved_imports, Severity::Error);
    }

    #[test]
    fn severity_display() {
        assert_eq!(Severity::Error.to_string(), "error");
        assert_eq!(Severity::Warn.to_string(), "warn");
        assert_eq!(Severity::Off.to_string(), "off");
    }

    #[test]
    fn apply_partial_all_none_changes_nothing() {
        let mut rules = RulesConfig::default();
        let original = rules.clone();
        let partial = PartialRulesConfig::default(); // all None
        rules.apply_partial(&partial);
        assert_eq!(rules.unused_files, original.unused_files);
        assert_eq!(rules.unused_exports, original.unused_exports);
        assert_eq!(
            rules.type_only_dependencies,
            original.type_only_dependencies
        );
    }

    #[test]
    fn apply_partial_all_fields_set() {
        let mut rules = RulesConfig::default();
        let partial = PartialRulesConfig {
            unused_files: Some(Severity::Off),
            unused_exports: Some(Severity::Off),
            unused_types: Some(Severity::Off),
            private_type_leaks: Some(Severity::Off),
            unused_dependencies: Some(Severity::Off),
            unused_dev_dependencies: Some(Severity::Off),
            unused_optional_dependencies: Some(Severity::Off),
            unused_enum_members: Some(Severity::Off),
            unused_class_members: Some(Severity::Off),
            unresolved_imports: Some(Severity::Off),
            unlisted_dependencies: Some(Severity::Off),
            duplicate_exports: Some(Severity::Off),
            type_only_dependencies: Some(Severity::Off),
            test_only_dependencies: Some(Severity::Off),
            circular_dependencies: Some(Severity::Off),
            boundary_violation: Some(Severity::Off),
            coverage_gaps: Some(Severity::Off),
            feature_flags: Some(Severity::Off),
            stale_suppressions: Some(Severity::Off),
            unused_catalog_entries: Some(Severity::Off),
            empty_catalog_groups: Some(Severity::Off),
            unresolved_catalog_references: Some(Severity::Off),
            unused_dependency_overrides: Some(Severity::Off),
            misconfigured_dependency_overrides: Some(Severity::Off),
        };
        rules.apply_partial(&partial);
        assert_eq!(rules.unused_files, Severity::Off);
        assert_eq!(rules.private_type_leaks, Severity::Off);
        assert_eq!(rules.circular_dependencies, Severity::Off);
        assert_eq!(rules.type_only_dependencies, Severity::Off);
        assert_eq!(rules.test_only_dependencies, Severity::Off);
        assert_eq!(rules.boundary_violation, Severity::Off);
        assert_eq!(rules.coverage_gaps, Severity::Off);
        assert_eq!(rules.feature_flags, Severity::Off);
        assert_eq!(rules.stale_suppressions, Severity::Off);
    }

    #[test]
    fn rules_config_defaults_include_optional_deps() {
        let rules = RulesConfig::default();
        assert_eq!(rules.unused_optional_dependencies, Severity::Warn);
    }

    #[test]
    fn severity_from_str_case_insensitive() {
        assert_eq!("ERROR".parse::<Severity>().unwrap(), Severity::Error);
        assert_eq!("Warn".parse::<Severity>().unwrap(), Severity::Warn);
        assert_eq!("OFF".parse::<Severity>().unwrap(), Severity::Off);
        assert_eq!("Warning".parse::<Severity>().unwrap(), Severity::Warn);
        assert_eq!("NONE".parse::<Severity>().unwrap(), Severity::Off);
    }

    #[test]
    fn severity_from_str_invalid_returns_error() {
        let result = "critical".parse::<Severity>();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("unknown severity"),
            "Expected descriptive error, got: {err}"
        );
    }

    // ── Unknown-rule-name detection (issue #467 phase 1) ─────────────

    #[test]
    fn known_rule_names_count_matches_struct() {
        // Drift guard. Bump both numbers together when adding a rule.
        // 24 canonical kebab-case names + 24 aliases = 48 entries.
        assert_eq!(KNOWN_RULE_NAMES.len(), 48);
    }

    #[test]
    fn known_rule_names_has_no_duplicates() {
        let mut sorted: Vec<&str> = KNOWN_RULE_NAMES.to_vec();
        sorted.sort_unstable();
        let original_len = sorted.len();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            original_len,
            "KNOWN_RULE_NAMES contains a duplicate"
        );
    }

    #[test]
    fn known_rule_names_covers_every_serde_alias_in_source() {
        // Source-level drift guard: parse this file's text and extract every
        // `alias = "<kebab>"` literal that appears on a `RulesConfig` /
        // `PartialRulesConfig` field. Assert each one is in
        // `KNOWN_RULE_NAMES`.
        //
        // Complements `known_rule_names_count_matches_struct` (catches new
        // fields) and `every_known_rule_name_round_trips_through_partial`
        // (catches stale or renamed entries). This one catches a new alias
        // added to an existing field without a matching KNOWN_RULE_NAMES
        // update; that's invisible to the count guard (count stays the
        // same), invisible to the canonical-coverage walk (the canonical
        // name is already present), and invisible to the roundtrip guard
        // (the roundtrip walks KNOWN_RULE_NAMES, never discovering an
        // alias that was added to the struct but not to the list).
        let source = include_str!("rules.rs");

        let mut aliases_found = Vec::new();
        for line in source.lines() {
            let trimmed = line.trim();
            // Skip line comments (the test's own doc strings would otherwise
            // pollute the count with placeholder examples).
            if trimmed.starts_with("//") {
                continue;
            }
            let Some(after) = trimmed.split("alias = \"").nth(1) else {
                continue;
            };
            let Some(end) = after.find('"') else {
                continue;
            };
            let alias = &after[..end];
            // Real aliases are kebab-case ASCII; placeholder examples in any
            // accidentally-included strings (`<kebab>`, `...`) get filtered.
            if alias.is_empty() || !alias.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                continue;
            }
            aliases_found.push(alias.to_owned());
        }

        // 24 alias attrs on RulesConfig + 24 on PartialRulesConfig = 48.
        assert_eq!(
            aliases_found.len(),
            48,
            "expected 48 source-level alias attrs (24 per struct); got {}: {:?}",
            aliases_found.len(),
            aliases_found
        );

        for alias in &aliases_found {
            assert!(
                KNOWN_RULE_NAMES.contains(&alias.as_str()),
                "serde alias '{alias}' is in rules.rs source but missing from KNOWN_RULE_NAMES"
            );
        }
    }

    #[test]
    fn every_known_rule_name_round_trips_through_partial() {
        // Stronger drift guard than the count + canonical-coverage tests:
        // every entry in KNOWN_RULE_NAMES must deserialize successfully via
        // `PartialRulesConfig` and resolve to exactly one field. Catches the
        // case where a developer renames an alias on the struct but forgets
        // to update KNOWN_RULE_NAMES (the count test still passes; this one
        // fails).
        for &name in KNOWN_RULE_NAMES {
            let json = format!(r#"{{"{name}": "warn"}}"#);
            let partial: PartialRulesConfig = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("'{name}' should deserialize: {e}"));

            let serialized = serde_json::to_value(&partial).unwrap();
            let map = serialized.as_object().unwrap();
            assert_eq!(
                map.len(),
                1,
                "'{name}' should resolve to exactly one field, got: {map:?}"
            );
        }
    }

    #[test]
    fn known_rule_names_covers_every_struct_field() {
        // Every canonical rule must serialize to a name in KNOWN_RULE_NAMES.
        // Iterates the serialized form of a default RulesConfig (which emits
        // canonical kebab-case for every field) and asserts each appears in
        // the const list. Drift guard for adding a new field but forgetting
        // to update KNOWN_RULE_NAMES.
        let json = serde_json::to_value(RulesConfig::default()).unwrap();
        let obj = json.as_object().unwrap();
        for key in obj.keys() {
            assert!(
                KNOWN_RULE_NAMES.contains(&key.as_str()),
                "field '{key}' is serialized but missing from KNOWN_RULE_NAMES"
            );
        }
    }

    #[test]
    fn closest_known_rule_name_suggests_for_obvious_typo() {
        assert_eq!(
            closest_known_rule_name("unsued-files"),
            Some("unused-files")
        );
        assert_eq!(
            closest_known_rule_name("circular-dependnecy"),
            Some("circular-dependency")
        );
        assert_eq!(
            closest_known_rule_name("unused-dep"),
            None,
            "too short for a confident suggestion"
        );
    }

    #[test]
    fn closest_known_rule_name_returns_none_for_novel_input() {
        assert_eq!(closest_known_rule_name("totally-fabricated"), None);
        assert_eq!(closest_known_rule_name("foo"), None);
    }

    #[test]
    fn closest_known_rule_name_is_case_insensitive() {
        assert_eq!(
            closest_known_rule_name("UNSUED-FILES"),
            Some("unused-files")
        );
    }

    #[test]
    fn closest_known_rule_name_returns_none_for_exact_match() {
        // A match with distance 0 is not a typo, so no suggestion.
        assert_eq!(closest_known_rule_name("unused-files"), None);
    }

    #[test]
    fn find_unknown_rule_keys_flags_typo() {
        let v = serde_json::json!({
            "unsued-files": "warn",
            "unused-exports": "off",
        });
        let unknown = find_unknown_rule_keys(&v, "rules");
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].key, "unsued-files");
        assert_eq!(unknown[0].context, "rules");
        assert_eq!(unknown[0].suggestion, Some("unused-files"));
    }

    #[test]
    fn find_unknown_rule_keys_passes_aliases() {
        let v = serde_json::json!({
            "unused-file": "warn",
            "circular-dependency": "off",
            "boundary-violations": "warn",
        });
        let unknown = find_unknown_rule_keys(&v, "rules");
        assert!(
            unknown.is_empty(),
            "documented aliases must not flag as unknown: {unknown:?}"
        );
    }

    #[test]
    fn find_unknown_rule_keys_returns_multiple_typos() {
        let v = serde_json::json!({
            "unsued-files": "warn",
            "circular-dependnecy": "off",
        });
        let unknown = find_unknown_rule_keys(&v, "rules");
        assert_eq!(unknown.len(), 2);
    }

    #[test]
    fn find_unknown_rule_keys_carries_context() {
        let v = serde_json::json!({ "unsued-files": "warn" });
        let unknown = find_unknown_rule_keys(&v, "overrides[2].rules");
        assert_eq!(unknown[0].context, "overrides[2].rules");
    }

    #[test]
    fn find_unknown_rule_keys_empty_when_not_object() {
        let v = serde_json::json!(null);
        assert!(find_unknown_rule_keys(&v, "rules").is_empty());

        let v = serde_json::json!([1, 2, 3]);
        assert!(find_unknown_rule_keys(&v, "rules").is_empty());
    }

    #[test]
    fn find_unknown_rule_keys_no_suggestion_for_novel_name() {
        let v = serde_json::json!({ "totally-fabricated-rule": "warn" });
        let unknown = find_unknown_rule_keys(&v, "rules");
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].suggestion, None);
    }

    // ── PartialRulesConfig deserialization ───────────────────────────

    #[test]
    fn partial_rules_empty_json() {
        let partial: PartialRulesConfig = serde_json::from_str("{}").unwrap();
        assert!(partial.unused_files.is_none());
        assert!(partial.unused_exports.is_none());
        assert!(partial.unused_types.is_none());
        assert!(partial.unused_dependencies.is_none());
        assert!(partial.circular_dependencies.is_none());
        assert!(partial.boundary_violation.is_none());
        assert!(partial.coverage_gaps.is_none());
        assert!(partial.feature_flags.is_none());
        assert!(partial.stale_suppressions.is_none());
    }

    #[test]
    fn partial_rules_subset_json() {
        let json = r#"{
            "unused-files": "warn",
            "circular-dependencies": "off"
        }"#;
        let partial: PartialRulesConfig = serde_json::from_str(json).unwrap();
        assert_eq!(partial.unused_files, Some(Severity::Warn));
        assert_eq!(partial.circular_dependencies, Some(Severity::Off));
        assert!(partial.unused_exports.is_none());
    }

    #[test]
    fn partial_rules_deserialize_circular_dependency_alias() {
        let json = r#"{
            "circular-dependency": "warn"
        }"#;
        let partial: PartialRulesConfig = serde_json::from_str(json).unwrap();
        assert_eq!(partial.circular_dependencies, Some(Severity::Warn));
    }

    #[test]
    fn partial_rules_all_fields_json() {
        let json = r#"{
            "unused-files": "error",
            "unused-exports": "warn",
            "unused-types": "off",
            "unused-dependencies": "error",
            "unused-dev-dependencies": "warn",
            "unused-optional-dependencies": "off",
            "unused-enum-members": "error",
            "unused-class-members": "warn",
            "unresolved-imports": "off",
            "unlisted-dependencies": "error",
            "duplicate-exports": "warn",
            "type-only-dependencies": "off",
            "test-only-dependencies": "error",
            "circular-dependencies": "warn",
            "boundary-violation": "off",
            "coverage-gaps": "warn",
            "feature-flags": "error",
            "stale-suppressions": "off"
        }"#;
        let partial: PartialRulesConfig = serde_json::from_str(json).unwrap();
        assert_eq!(partial.unused_files, Some(Severity::Error));
        assert_eq!(partial.unused_exports, Some(Severity::Warn));
        assert_eq!(partial.unused_types, Some(Severity::Off));
        assert_eq!(partial.unused_dependencies, Some(Severity::Error));
        assert_eq!(partial.unused_dev_dependencies, Some(Severity::Warn));
        assert_eq!(partial.unused_optional_dependencies, Some(Severity::Off));
        assert_eq!(partial.unused_enum_members, Some(Severity::Error));
        assert_eq!(partial.unused_class_members, Some(Severity::Warn));
        assert_eq!(partial.unresolved_imports, Some(Severity::Off));
        assert_eq!(partial.unlisted_dependencies, Some(Severity::Error));
        assert_eq!(partial.duplicate_exports, Some(Severity::Warn));
        assert_eq!(partial.type_only_dependencies, Some(Severity::Off));
        assert_eq!(partial.test_only_dependencies, Some(Severity::Error));
        assert_eq!(partial.circular_dependencies, Some(Severity::Warn));
        assert_eq!(partial.boundary_violation, Some(Severity::Off));
        assert_eq!(partial.coverage_gaps, Some(Severity::Warn));
        assert_eq!(partial.feature_flags, Some(Severity::Error));
        assert_eq!(partial.stale_suppressions, Some(Severity::Off));
    }

    // ── PartialRulesConfig serialization skip_serializing_if ────────

    #[test]
    fn partial_rules_none_fields_not_serialized() {
        let partial = PartialRulesConfig::default();
        let json = serde_json::to_string(&partial).unwrap();
        assert_eq!(
            json, "{}",
            "all-None partial should serialize to empty object"
        );
    }

    #[test]
    fn partial_rules_some_fields_serialized() {
        let partial = PartialRulesConfig {
            unused_files: Some(Severity::Warn),
            ..Default::default()
        };
        let json = serde_json::to_string(&partial).unwrap();
        assert!(json.contains("unused-files"));
        assert!(!json.contains("unused-exports"));
    }

    // ── Severity JSON deserialization ────────────────────────────────

    #[test]
    fn severity_json_deserialization() {
        let error: Severity = serde_json::from_str(r#""error""#).unwrap();
        assert_eq!(error, Severity::Error);

        let warn: Severity = serde_json::from_str(r#""warn""#).unwrap();
        assert_eq!(warn, Severity::Warn);

        let off: Severity = serde_json::from_str(r#""off""#).unwrap();
        assert_eq!(off, Severity::Off);
    }

    #[test]
    fn severity_invalid_json_value_rejected() {
        let result: Result<Severity, _> = serde_json::from_str(r#""critical""#);
        assert!(result.is_err());
    }

    // ── Severity default ────────────────────────────────────────────

    #[test]
    fn severity_default_is_error() {
        assert_eq!(Severity::default(), Severity::Error);
    }

    // ── RulesConfig JSON serialize roundtrip ─────────────────────────

    #[test]
    fn rules_config_json_roundtrip() {
        let rules = RulesConfig {
            unused_files: Severity::Warn,
            unused_exports: Severity::Off,
            type_only_dependencies: Severity::Error,
            ..RulesConfig::default()
        };
        let json = serde_json::to_string(&rules).unwrap();
        let restored: RulesConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.unused_files, Severity::Warn);
        assert_eq!(restored.unused_exports, Severity::Off);
        assert_eq!(restored.type_only_dependencies, Severity::Error);
        assert_eq!(restored.unused_dependencies, Severity::Error); // default
    }

    // ── apply_partial preserves type_only/test_only defaults ────────

    #[test]
    fn apply_partial_preserves_type_only_default() {
        let mut rules = RulesConfig::default();
        let partial = PartialRulesConfig {
            unused_files: Some(Severity::Off),
            ..Default::default()
        };
        rules.apply_partial(&partial);
        // type_only_dependencies defaults to Warn, should be preserved
        assert_eq!(rules.type_only_dependencies, Severity::Warn);
        assert_eq!(rules.test_only_dependencies, Severity::Warn);
    }
}
