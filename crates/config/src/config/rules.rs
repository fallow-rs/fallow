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
