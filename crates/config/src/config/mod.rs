mod boundaries;
mod duplicates_config;
mod flags;
mod format;
pub mod glob_validation;
mod health;
mod parsing;
mod resolution;
mod resolve;
mod rules;
mod used_class_members;

#[expect(
    clippy::redundant_pub_crate,
    reason = "this module is glob re-exported from lib.rs, so `pub` would leak the helper into the public API; pub(crate) keeps it internal to the crate"
)]
pub(crate) use boundaries::wildcard_placement_error;
pub use boundaries::{
    AuthoredRule, BoundaryCallsConfig, BoundaryConfig, BoundaryCoverageConfig, BoundaryPreset,
    BoundaryRule, BoundaryZone, ForbiddenCallRule, ForbiddenCallee, InvalidForbiddenCallee,
    LogicalGroup, LogicalGroupStatus, RedundantRootPrefix, ResolvedBoundaryConfig,
    ResolvedBoundaryCoverageConfig, ResolvedBoundaryRule, ResolvedZone, UnknownZoneRef,
    ZoneReferenceKind, ZoneValidationError,
};
pub use duplicates_config::{
    DetectionMode, DuplicatesConfig, NormalizationConfig, ResolvedNormalization,
};
pub use flags::{FlagsConfig, SdkPattern};
pub use format::OutputFormat;
pub use health::{EmailMode, HealthConfig, HealthThresholdOverride, OwnershipConfig};
pub use resolution::{
    CompiledIgnoreCatalogReferenceRule, CompiledIgnoreDependencyOverrideRule,
    CompiledIgnoreExportRule, ConfigOverride, DEFAULT_MAX_FILE_SIZE_BYTES,
    DEFAULT_MAX_FILE_SIZE_MB, IgnoreCatalogReferenceRule, IgnoreDependencyOverrideRule,
    IgnoreExportRule, ResolvedConfig, ResolvedOverride, resolve_max_file_size_bytes,
};
pub use resolve::ResolveConfig;
pub use rules::{PartialRulesConfig, RulesConfig, Severity};
pub use used_class_members::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::Not;
use std::path::PathBuf;

use crate::external_plugin::ExternalPluginDef;
use crate::workspace::WorkspaceConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged, rename_all = "camelCase")]
pub enum IgnoreExportsUsedInFileConfig {
    Bool(bool),
    ByKind(IgnoreExportsUsedInFileByKind),
}

impl Default for IgnoreExportsUsedInFileConfig {
    fn default() -> Self {
        Self::Bool(false)
    }
}

impl From<bool> for IgnoreExportsUsedInFileConfig {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<IgnoreExportsUsedInFileByKind> for IgnoreExportsUsedInFileConfig {
    fn from(value: IgnoreExportsUsedInFileByKind) -> Self {
        Self::ByKind(value)
    }
}

impl IgnoreExportsUsedInFileConfig {
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        match self {
            Self::Bool(value) => value,
            Self::ByKind(kind) => kind.type_ || kind.interface,
        }
    }

    #[must_use]
    pub const fn suppresses(self, is_type_only: bool) -> bool {
        match self {
            Self::Bool(value) => value,
            Self::ByKind(kind) => is_type_only && (kind.type_ || kind.interface),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IgnoreExportsUsedInFileByKind {
    #[serde(default, rename = "type")]
    pub type_: bool,
    #[serde(default)]
    pub interface: bool,
}

/// Options for the `unused-component-props` rule.
///
/// Lets a project exempt component props whose local destructure binding name
/// matches a regex from `unused-component-props`, honoring the
/// "accepted-but-intentionally-unused" leading-underscore convention (Svelte 5
/// `$props()`, React destructure) that mirrors TypeScript `noUnusedParameters`
/// and ESLint `@typescript-eslint/no-unused-vars` `varsIgnorePattern` /
/// `argsIgnorePattern`. Opt-in; an unset `ignorePattern` leaves the rule's
/// behavior unchanged.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct UnusedComponentPropsConfig {
    /// Regex matched against each declared prop's LOCAL destructure binding name
    /// (e.g. `_stage` in `let { stage: _stage } = $props()`), which falls back
    /// to the public prop name when there is no alias. A prop whose local name
    /// matches is treated as intentionally unused and never reported as
    /// `unused-component-props`. Matching is unanchored (substring), like
    /// ESLint's `RegExp.test`, so anchor with `^_` to match a leading
    /// underscore. Compiled and validated at config load (an invalid regex fails
    /// load). Applies to Vue, Svelte, Astro, and React/Preact props.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_pattern: Option<String>,
}

impl UnusedComponentPropsConfig {
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.ignore_pattern.is_none()
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FixConfig {
    #[serde(default)]
    pub catalog: CatalogFixConfig,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFixConfig {
    #[serde(default)]
    pub delete_preceding_comments: CatalogPrecedingCommentPolicy,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CatalogPrecedingCommentPolicy {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FallowConfig {
    #[serde(rename = "$schema", default, skip_serializing)]
    pub schema: Option<String>,

    #[serde(default, skip_serializing)]
    pub extends: Vec<String>,

    #[serde(default)]
    pub entry: Vec<String>,

    #[serde(default)]
    pub ignore_patterns: Vec<String>,

    #[serde(default)]
    pub framework: Vec<ExternalPluginDef>,

    #[serde(default)]
    pub workspaces: Option<WorkspaceConfig>,

    #[serde(default)]
    pub ignore_dependencies: Vec<String>,

    #[serde(default)]
    pub ignore_unresolved_imports: Vec<String>,

    #[serde(default)]
    pub ignore_exports: Vec<IgnoreExportRule>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_catalog_references: Vec<IgnoreCatalogReferenceRule>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_dependency_overrides: Vec<IgnoreDependencyOverrideRule>,

    #[serde(default)]
    pub ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_decorators: Vec<String>,

    #[serde(default)]
    pub used_class_members: Vec<UsedClassMemberRule>,

    #[serde(default)]
    pub duplicates: DuplicatesConfig,

    #[serde(default)]
    pub health: HealthConfig,

    #[serde(default)]
    pub rules: RulesConfig,

    #[serde(
        default,
        skip_serializing_if = "UnusedComponentPropsConfig::is_default"
    )]
    pub unused_component_props: UnusedComponentPropsConfig,

    #[serde(default)]
    pub boundaries: BoundaryConfig,

    #[serde(default)]
    pub flags: FlagsConfig,

    #[serde(default)]
    pub security: SecurityConfig,

    #[serde(default)]
    pub fix: FixConfig,

    #[serde(default)]
    pub resolve: ResolveConfig,

    #[serde(default)]
    pub production: ProductionConfig,

    #[serde(default)]
    pub plugins: Vec<String>,

    /// Paths to declarative rule-pack files (JSON or JSONC), relative to the
    /// project root. Each pack declares `banned-call`, `banned-import`, or
    /// `banned-effect` rules that report as `policy-violation` findings. Packs
    /// are pure data: no project code is executed. Invalid or missing packs
    /// fail config load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rule_packs: Vec<String>,

    #[serde(default)]
    pub dynamically_loaded: Vec<String>,

    #[serde(default)]
    pub overrides: Vec<ConfigOverride>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codeowners: Option<String>,

    #[serde(default)]
    pub public_packages: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regression: Option<RegressionConfig>,

    #[serde(default, skip_serializing_if = "AuditConfig::is_empty")]
    pub audit: AuditConfig,

    #[serde(default)]
    pub sealed: bool,

    #[serde(default)]
    pub include_entry_exports: bool,

    #[serde(default)]
    pub auto_imports: bool,

    #[serde(default, skip_serializing_if = "CacheConfig::is_default")]
    pub cache: CacheConfig,
}

/// Scopes `fallow security` catalogue behavior. An absent category block admits
/// every catalogue category. `hardcoded-secret` is include-required and only
/// runs when explicitly listed in `security.categories.include`.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SecurityConfig {
    /// Include/exclude filter over category ids (e.g. `dangerous-html`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<SecurityCategories>,
    /// Additional project-local names for HTTP request objects. These names
    /// extend the built-in receiver allowlist for `*.query`, `*.params`, and
    /// `*.body` source patterns. They do not replace the built-ins and do not
    /// gate `*.searchParams`, which intentionally stays ungated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_receivers: Vec<String>,
}

impl SecurityConfig {
    #[must_use]
    pub fn normalized_request_receivers(&self) -> Vec<String> {
        let mut receivers = Vec::new();
        for receiver in &self.request_receivers {
            let normalized = receiver.trim().to_ascii_lowercase();
            if !normalized.is_empty() && !receivers.contains(&normalized) {
                receivers.push(normalized);
            }
        }
        receivers
    }

    #[must_use]
    pub fn request_receivers_are_valid(&self) -> bool {
        self.request_receivers
            .iter()
            .all(|receiver| !receiver.trim().is_empty())
    }
}

/// Include/exclude lists scoping the active security categories. When `include`
/// is set, only those categories are active; `exclude` removes categories from
/// the admitted set. Both unset admits catalogue categories. `hardcoded-secret`
/// still requires explicit inclusion.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SecurityCategories {
    /// Catalogue category ids to admit. When set, all others are excluded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// Catalogue category ids to remove from the admitted set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CacheConfig {
    /// Directory for fallow's persistent analysis cache. Relative paths resolve
    /// from the project root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<PathBuf>,
    /// Maximum size of the persistent extraction cache, in megabytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_size_mb: Option<u32>,
}

impl CacheConfig {
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.dir.is_none() && self.max_size_mb.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionAnalysis {
    DeadCode,
    Health,
    Dupes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ProductionConfig {
    Global(bool),
    PerAnalysis(PerAnalysisProductionConfig),
}

impl<'de> Deserialize<'de> for ProductionConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ProductionConfigVisitor;

        impl<'de> serde::de::Visitor<'de> for ProductionConfigVisitor {
            type Value = ProductionConfig;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a boolean or per-analysis production config object")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ProductionConfig::Global(value))
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                PerAnalysisProductionConfig::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )
                .map(ProductionConfig::PerAnalysis)
            }
        }

        deserializer.deserialize_any(ProductionConfigVisitor)
    }
}

impl Default for ProductionConfig {
    fn default() -> Self {
        Self::Global(false)
    }
}

impl From<bool> for ProductionConfig {
    fn from(value: bool) -> Self {
        Self::Global(value)
    }
}

impl Not for ProductionConfig {
    type Output = bool;

    fn not(self) -> Self::Output {
        !self.any_enabled()
    }
}

impl ProductionConfig {
    #[must_use]
    pub const fn for_analysis(self, analysis: ProductionAnalysis) -> bool {
        match self {
            Self::Global(value) => value,
            Self::PerAnalysis(config) => match analysis {
                ProductionAnalysis::DeadCode => config.dead_code,
                ProductionAnalysis::Health => config.health,
                ProductionAnalysis::Dupes => config.dupes,
            },
        }
    }

    #[must_use]
    pub const fn global(self) -> bool {
        match self {
            Self::Global(value) => value,
            Self::PerAnalysis(_) => false,
        }
    }

    #[must_use]
    pub const fn any_enabled(self) -> bool {
        match self {
            Self::Global(value) => value,
            Self::PerAnalysis(config) => config.dead_code || config.health || config.dupes,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct PerAnalysisProductionConfig {
    pub dead_code: bool,
    pub health: bool,
    pub dupes: bool,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditConfig {
    #[serde(default, skip_serializing_if = "AuditGate::is_default")]
    pub gate: AuditGate,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css_deep: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code_baseline: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_baseline: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes_baseline: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_max_age_days: Option<u32>,
}

impl AuditConfig {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.gate.is_default()
            && self.css.is_none()
            && self.css_deep.is_none()
            && self.dead_code_baseline.is_none()
            && self.health_baseline.is_none()
            && self.dupes_baseline.is_none()
            && self.cache_max_age_days.is_none()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AuditGate {
    #[default]
    NewOnly,
    All,
}

impl AuditGate {
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::NewOnly)
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegressionConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<RegressionBaseline>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegressionBaseline {
    #[serde(default)]
    pub total_issues: usize,
    #[serde(default)]
    pub unused_files: usize,
    #[serde(default)]
    pub unused_exports: usize,
    #[serde(default)]
    pub unused_types: usize,
    #[serde(default)]
    pub unused_dependencies: usize,
    #[serde(default)]
    pub unused_dev_dependencies: usize,
    #[serde(default)]
    pub unused_optional_dependencies: usize,
    #[serde(default)]
    pub unused_enum_members: usize,
    #[serde(default)]
    pub unused_class_members: usize,
    #[serde(default)]
    pub unresolved_imports: usize,
    #[serde(default)]
    pub unlisted_dependencies: usize,
    #[serde(default)]
    pub duplicate_exports: usize,
    #[serde(default)]
    pub circular_dependencies: usize,
    #[serde(default)]
    pub re_export_cycles: usize,
    #[serde(default)]
    pub type_only_dependencies: usize,
    #[serde(default)]
    pub test_only_dependencies: usize,
    #[serde(default)]
    pub dev_dependencies_in_production: usize,
    #[serde(default)]
    pub boundary_violations: usize,
    #[serde(default)]
    pub boundary_coverage_violations: usize,
    #[serde(default)]
    pub boundary_call_violations: usize,
    #[serde(default)]
    pub policy_violations: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_empty_collections() {
        let config = FallowConfig::default();
        assert!(config.schema.is_none());
        assert!(config.extends.is_empty());
        assert!(config.entry.is_empty());
        assert!(config.ignore_patterns.is_empty());
        assert!(config.framework.is_empty());
        assert!(config.workspaces.is_none());
        assert!(config.ignore_dependencies.is_empty());
        assert!(config.ignore_exports.is_empty());
        assert!(config.used_class_members.is_empty());
        assert!(config.plugins.is_empty());
        assert!(config.dynamically_loaded.is_empty());
        assert!(config.overrides.is_empty());
        assert!(config.public_packages.is_empty());
        assert_eq!(
            config.fix.catalog.delete_preceding_comments,
            CatalogPrecedingCommentPolicy::Auto
        );
        assert!(!config.production);
    }

    #[test]
    fn default_config_rules_are_error() {
        let config = FallowConfig::default();
        assert_eq!(config.rules.unused_files, Severity::Error);
        assert_eq!(config.rules.unused_exports, Severity::Error);
        assert_eq!(config.rules.unused_dependencies, Severity::Error);
    }

    #[test]
    fn default_config_duplicates_enabled() {
        let config = FallowConfig::default();
        assert!(config.duplicates.enabled);
        assert_eq!(config.duplicates.min_tokens, 50);
        assert_eq!(config.duplicates.min_lines, 5);
    }

    #[test]
    fn default_config_health_thresholds() {
        let config = FallowConfig::default();
        assert_eq!(config.health.max_cyclomatic, 20);
        assert_eq!(config.health.max_cognitive, 15);
    }

    #[test]
    fn deserialize_empty_json_object() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(config.entry.is_empty());
        assert!(!config.production);
    }

    #[test]
    fn deserialize_json_with_all_top_level_fields() {
        let json = r#"{
            "$schema": "https://fallow.dev/schema.json",
            "entry": ["src/main.ts"],
            "ignorePatterns": ["generated/**"],
            "ignoreDependencies": ["postcss"],
            "production": true,
            "plugins": ["custom-plugin.toml"],
            "rules": {"unused-files": "warn"},
            "duplicates": {"enabled": false},
            "health": {"maxCyclomatic": 30}
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.schema.as_deref(),
            Some("https://fallow.dev/schema.json")
        );
        assert_eq!(config.entry, vec!["src/main.ts"]);
        assert_eq!(config.ignore_patterns, vec!["generated/**"]);
        assert_eq!(config.ignore_dependencies, vec!["postcss"]);
        assert!(config.production);
        assert_eq!(config.plugins, vec!["custom-plugin.toml"]);
        assert_eq!(config.rules.unused_files, Severity::Warn);
        assert!(!config.duplicates.enabled);
        assert_eq!(config.health.max_cyclomatic, 30);
    }

    #[test]
    fn deserialize_json_deny_unknown_fields() {
        let json = r#"{"unknownField": true}"#;
        let result: Result<FallowConfig, _> = serde_json::from_str(json);
        assert!(result.is_err(), "unknown fields should be rejected");
    }

    #[test]
    fn deserialize_json_production_mode_default_false() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.production);
    }

    #[test]
    fn deserialize_json_production_mode_true() {
        let config: FallowConfig = serde_json::from_str(r#"{"production": true}"#).unwrap();
        assert!(config.production);
    }

    #[test]
    fn deserialize_json_per_analysis_production_mode() {
        let config: FallowConfig = serde_json::from_str(
            r#"{"production": {"deadCode": false, "health": true, "dupes": false}}"#,
        )
        .unwrap();
        assert!(!config.production.for_analysis(ProductionAnalysis::DeadCode));
        assert!(config.production.for_analysis(ProductionAnalysis::Health));
        assert!(!config.production.for_analysis(ProductionAnalysis::Dupes));
    }

    #[test]
    fn deserialize_json_per_analysis_production_mode_rejects_unknown_fields() {
        let err = serde_json::from_str::<FallowConfig>(r#"{"production": {"healthTypo": true}}"#)
            .unwrap_err();
        assert!(
            err.to_string().contains("healthTypo"),
            "error should name the unknown field: {err}"
        );
    }

    #[test]
    fn deserialize_json_dynamically_loaded() {
        let json = r#"{"dynamicallyLoaded": ["plugins/**/*.ts", "locales/**/*.json"]}"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.dynamically_loaded,
            vec!["plugins/**/*.ts", "locales/**/*.json"]
        );
    }

    #[test]
    fn deserialize_json_dynamically_loaded_defaults_empty() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(config.dynamically_loaded.is_empty());
    }

    #[test]
    fn deserialize_json_fix_catalog_delete_preceding_comments() {
        let config: FallowConfig =
            serde_json::from_str(r#"{"fix": {"catalog": {"deletePrecedingComments": "always"}}}"#)
                .unwrap();
        assert_eq!(
            config.fix.catalog.delete_preceding_comments,
            CatalogPrecedingCommentPolicy::Always
        );
    }

    #[test]
    fn deserialize_json_fix_catalog_delete_preceding_comments_rejects_unknown_policy() {
        let err = serde_json::from_str::<FallowConfig>(
            r#"{"fix": {"catalog": {"deletePrecedingComments": "sometimes"}}}"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("sometimes"),
            "error should name the bad policy: {err}"
        );
    }

    #[test]
    fn deserialize_json_used_class_members_supports_strings_and_scoped_rules() {
        let json = r#"{
            "usedClassMembers": [
                "agInit",
                { "implements": "ICellRendererAngularComp", "members": ["refresh"] },
                { "extends": "BaseCommand", "implements": "CanActivate", "members": ["execute"] }
            ]
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.used_class_members,
            vec![
                UsedClassMemberRule::from("agInit"),
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: None,
                    implements: Some("ICellRendererAngularComp".to_string()),
                    members: vec!["refresh".to_string()],
                }),
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: Some("BaseCommand".to_string()),
                    implements: Some("CanActivate".to_string()),
                    members: vec!["execute".to_string()],
                }),
            ]
        );
    }

    #[test]
    fn deserialize_toml_minimal() {
        let toml_str = r#"
entry = ["src/index.ts"]
production = true
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.entry, vec!["src/index.ts"]);
        assert!(config.production);
    }

    #[test]
    fn deserialize_toml_per_analysis_production_mode() {
        let toml_str = r"
[production]
deadCode = false
health = true
dupes = false
";
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.production.for_analysis(ProductionAnalysis::DeadCode));
        assert!(config.production.for_analysis(ProductionAnalysis::Health));
        assert!(!config.production.for_analysis(ProductionAnalysis::Dupes));
    }

    #[test]
    fn deserialize_toml_per_analysis_production_mode_rejects_unknown_fields() {
        let err = toml::from_str::<FallowConfig>(
            r"
[production]
healthTypo = true
",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("healthTypo"),
            "error should name the unknown field: {err}"
        );
    }

    #[test]
    fn deserialize_toml_with_inline_framework() {
        let toml_str = r#"
[[framework]]
name = "my-framework"
enablers = ["my-framework-pkg"]
entryPoints = ["src/routes/**/*.tsx"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.framework.len(), 1);
        assert_eq!(config.framework[0].name, "my-framework");
        assert_eq!(config.framework[0].enablers, vec!["my-framework-pkg"]);
        assert_eq!(
            config.framework[0].entry_points,
            vec!["src/routes/**/*.tsx"]
        );
    }

    #[test]
    fn deserialize_toml_fix_catalog_delete_preceding_comments() {
        let toml_str = r#"
[fix.catalog]
deletePrecedingComments = "never"
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.fix.catalog.delete_preceding_comments,
            CatalogPrecedingCommentPolicy::Never
        );
    }

    #[test]
    fn deserialize_toml_with_workspace_config() {
        let toml_str = r#"
[workspaces]
patterns = ["packages/*", "apps/*"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert!(config.workspaces.is_some());
        let ws = config.workspaces.unwrap();
        assert_eq!(ws.patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn deserialize_toml_with_ignore_exports() {
        let toml_str = r#"
[[ignoreExports]]
file = "src/types/**/*.ts"
exports = ["*"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.ignore_exports.len(), 1);
        assert_eq!(config.ignore_exports[0].file, "src/types/**/*.ts");
        assert_eq!(config.ignore_exports[0].exports, vec!["*"]);
    }

    #[test]
    fn deserialize_toml_used_class_members_supports_scoped_rules() {
        let toml_str = r#"
usedClassMembers = [
  { implements = "ICellRendererAngularComp", members = ["refresh"] },
  { extends = "BaseCommand", members = ["execute"] },
]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.used_class_members,
            vec![
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: None,
                    implements: Some("ICellRendererAngularComp".to_string()),
                    members: vec!["refresh".to_string()],
                }),
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: Some("BaseCommand".to_string()),
                    implements: None,
                    members: vec!["execute".to_string()],
                }),
            ]
        );
    }

    #[test]
    fn deserialize_json_used_class_members_rejects_unconstrained_scoped_rules() {
        let result = serde_json::from_str::<FallowConfig>(
            r#"{"usedClassMembers":[{"members":["refresh"]}]}"#,
        );
        assert!(
            result.is_err(),
            "unconstrained scoped rule should be rejected"
        );
    }

    #[test]
    fn deserialize_ignore_exports_used_in_file_bool() {
        let config: FallowConfig =
            serde_json::from_str(r#"{"ignoreExportsUsedInFile":true}"#).unwrap();

        assert!(config.ignore_exports_used_in_file.suppresses(false));
        assert!(config.ignore_exports_used_in_file.suppresses(true));
    }

    #[test]
    fn deserialize_ignore_exports_used_in_file_kind_form() {
        let config: FallowConfig =
            serde_json::from_str(r#"{"ignoreExportsUsedInFile":{"type":true}}"#).unwrap();

        assert!(!config.ignore_exports_used_in_file.suppresses(false));
        assert!(config.ignore_exports_used_in_file.suppresses(true));
    }

    #[test]
    fn deserialize_toml_deny_unknown_fields() {
        let toml_str = r"bogus_field = true";
        let result: Result<FallowConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "unknown fields should be rejected");
    }

    #[test]
    fn json_serialize_roundtrip() {
        let config = FallowConfig {
            entry: vec!["src/main.ts".to_string()],
            production: true.into(),
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: FallowConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.entry, vec!["src/main.ts"]);
        assert!(restored.production);
    }

    #[test]
    fn schema_field_not_serialized() {
        let config = FallowConfig {
            schema: Some("https://example.com/schema.json".to_string()),
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            !json.contains("$schema"),
            "schema field should be skipped in serialization"
        );
    }

    #[test]
    fn extends_field_not_serialized() {
        let config = FallowConfig {
            extends: vec!["base.json".to_string()],
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            !json.contains("extends"),
            "extends field should be skipped in serialization"
        );
    }

    #[test]
    fn regression_config_deserialize_json() {
        let json = r#"{
            "regression": {
                "baseline": {
                    "totalIssues": 42,
                    "unusedFiles": 10,
                    "unusedExports": 5,
                    "circularDependencies": 2
                }
            }
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        let regression = config.regression.unwrap();
        let baseline = regression.baseline.unwrap();
        assert_eq!(baseline.total_issues, 42);
        assert_eq!(baseline.unused_files, 10);
        assert_eq!(baseline.unused_exports, 5);
        assert_eq!(baseline.circular_dependencies, 2);
        assert_eq!(baseline.unused_types, 0);
        assert_eq!(baseline.boundary_violations, 0);
    }

    #[test]
    fn regression_config_defaults_to_none() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(config.regression.is_none());
    }

    #[test]
    fn regression_baseline_all_zeros_by_default() {
        let baseline = RegressionBaseline::default();
        assert_eq!(baseline.total_issues, 0);
        assert_eq!(baseline.unused_files, 0);
        assert_eq!(baseline.unused_exports, 0);
        assert_eq!(baseline.unused_types, 0);
        assert_eq!(baseline.unused_dependencies, 0);
        assert_eq!(baseline.unused_dev_dependencies, 0);
        assert_eq!(baseline.unused_optional_dependencies, 0);
        assert_eq!(baseline.unused_enum_members, 0);
        assert_eq!(baseline.unused_class_members, 0);
        assert_eq!(baseline.unresolved_imports, 0);
        assert_eq!(baseline.unlisted_dependencies, 0);
        assert_eq!(baseline.duplicate_exports, 0);
        assert_eq!(baseline.circular_dependencies, 0);
        assert_eq!(baseline.type_only_dependencies, 0);
        assert_eq!(baseline.test_only_dependencies, 0);
        assert_eq!(baseline.boundary_violations, 0);
    }

    #[test]
    fn regression_config_serialize_roundtrip() {
        let baseline = RegressionBaseline {
            total_issues: 100,
            unused_files: 20,
            unused_exports: 30,
            ..RegressionBaseline::default()
        };
        let regression = RegressionConfig {
            baseline: Some(baseline),
        };
        let config = FallowConfig {
            regression: Some(regression),
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: FallowConfig = serde_json::from_str(&json).unwrap();
        let restored_baseline = restored.regression.unwrap().baseline.unwrap();
        assert_eq!(restored_baseline.total_issues, 100);
        assert_eq!(restored_baseline.unused_files, 20);
        assert_eq!(restored_baseline.unused_exports, 30);
        assert_eq!(restored_baseline.unused_types, 0);
    }

    #[test]
    fn regression_config_empty_baseline_deserialize() {
        let json = r#"{"regression": {}}"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        let regression = config.regression.unwrap();
        assert!(regression.baseline.is_none());
    }

    #[test]
    fn regression_baseline_not_serialized_when_none() {
        let config = FallowConfig {
            regression: None,
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            !json.contains("regression"),
            "regression should be skipped when None"
        );
    }

    #[test]
    fn deserialize_json_with_overrides() {
        let json = r#"{
            "overrides": [
                {
                    "files": ["*.test.ts", "*.spec.ts"],
                    "rules": {
                        "unused-exports": "off",
                        "unused-files": "warn"
                    }
                }
            ]
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.overrides.len(), 1);
        assert_eq!(config.overrides[0].files.len(), 2);
        assert_eq!(
            config.overrides[0].rules.unused_exports,
            Some(Severity::Off)
        );
        assert_eq!(config.overrides[0].rules.unused_files, Some(Severity::Warn));
    }

    #[test]
    fn deserialize_json_with_boundaries() {
        let json = r#"{
            "boundaries": {
                "preset": "layered"
            }
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.boundaries.preset, Some(BoundaryPreset::Layered));
    }

    #[test]
    fn deserialize_toml_with_regression_baseline() {
        let toml_str = r"
[regression.baseline]
totalIssues = 50
unusedFiles = 10
unusedExports = 15
";
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        let baseline = config.regression.unwrap().baseline.unwrap();
        assert_eq!(baseline.total_issues, 50);
        assert_eq!(baseline.unused_files, 10);
        assert_eq!(baseline.unused_exports, 15);
    }

    #[test]
    fn deserialize_toml_with_overrides() {
        let toml_str = r#"
[[overrides]]
files = ["*.test.ts"]

[overrides.rules]
unused-exports = "off"

[[overrides]]
files = ["*.stories.tsx"]

[overrides.rules]
unused-files = "off"
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.overrides.len(), 2);
        assert_eq!(
            config.overrides[0].rules.unused_exports,
            Some(Severity::Off)
        );
        assert_eq!(config.overrides[1].rules.unused_files, Some(Severity::Off));
    }

    #[test]
    fn regression_config_default_is_none_baseline() {
        let config = RegressionConfig::default();
        assert!(config.baseline.is_none());
    }

    #[test]
    fn deserialize_json_multiple_ignore_export_rules() {
        let json = r#"{
            "ignoreExports": [
                {"file": "src/types/**/*.ts", "exports": ["*"]},
                {"file": "src/constants.ts", "exports": ["FOO", "BAR"]},
                {"file": "src/index.ts", "exports": ["default"]}
            ]
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.ignore_exports.len(), 3);
        assert_eq!(config.ignore_exports[2].exports, vec!["default"]);
    }

    #[test]
    fn deserialize_json_public_packages_camel_case() {
        let json = r#"{"publicPackages": ["@myorg/shared-lib", "@myorg/utils"]}"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.public_packages,
            vec!["@myorg/shared-lib", "@myorg/utils"]
        );
    }

    #[test]
    fn deserialize_json_public_packages_rejects_snake_case() {
        let json = r#"{"public_packages": ["@myorg/shared-lib"]}"#;
        let result: Result<FallowConfig, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "snake_case should be rejected by deny_unknown_fields + rename_all camelCase"
        );
    }

    #[test]
    fn deserialize_json_public_packages_empty() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(config.public_packages.is_empty());
    }

    #[test]
    fn deserialize_toml_public_packages() {
        let toml_str = r#"
publicPackages = ["@myorg/shared-lib", "@myorg/ui"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.public_packages,
            vec!["@myorg/shared-lib", "@myorg/ui"]
        );
    }

    #[test]
    fn public_packages_serialize_roundtrip() {
        let config = FallowConfig {
            public_packages: vec!["@myorg/shared-lib".to_string()],
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: FallowConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.public_packages, vec!["@myorg/shared-lib"]);
    }
}
