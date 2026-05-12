mod boundaries;
mod duplicates_config;
mod flags;
mod format;
mod health;
mod parsing;
mod resolution;
mod resolve;
mod rules;
mod used_class_members;

pub use boundaries::{
    BoundaryConfig, BoundaryPreset, BoundaryRule, BoundaryZone, ResolvedBoundaryConfig,
    ResolvedBoundaryRule, ResolvedZone,
};
pub use duplicates_config::{
    DetectionMode, DuplicatesConfig, NormalizationConfig, ResolvedNormalization,
};
pub use flags::{FlagsConfig, SdkPattern};
pub use format::OutputFormat;
pub use health::{EmailMode, HealthConfig, OwnershipConfig};
pub use resolution::{
    CompiledIgnoreCatalogReferenceRule, CompiledIgnoreExportRule, ConfigOverride,
    IgnoreCatalogReferenceRule, IgnoreExportRule, ResolvedConfig, ResolvedOverride,
};
pub use resolve::ResolveConfig;
pub use rules::{PartialRulesConfig, RulesConfig, Severity};
pub use used_class_members::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::Not;

use crate::external_plugin::ExternalPluginDef;
use crate::workspace::WorkspaceConfig;

/// Controls whether exports referenced only inside their defining file are
/// reported as unused exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged, rename_all = "camelCase")]
pub enum IgnoreExportsUsedInFileConfig {
    /// `true` suppresses both value and type exports that are referenced in
    /// their defining file. `false` preserves the default cross-file behavior.
    Bool(bool),
    /// Knip-compatible fine-grained form. Fallow groups type aliases and
    /// interfaces under `unused_types`, so either field enables type-export
    /// suppression for same-file references.
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
    /// Whether this option can suppress at least one kind of export.
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        match self {
            Self::Bool(value) => value,
            Self::ByKind(kind) => kind.type_ || kind.interface,
        }
    }

    /// Whether same-file references should suppress this export kind.
    #[must_use]
    pub const fn suppresses(self, is_type_only: bool) -> bool {
        match self {
            Self::Bool(value) => value,
            Self::ByKind(kind) => is_type_only && (kind.type_ || kind.interface),
        }
    }
}

/// Knip-compatible `ignoreExportsUsedInFile` object form.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IgnoreExportsUsedInFileByKind {
    /// Suppress same-file references for exported type aliases.
    #[serde(default, rename = "type")]
    pub type_: bool,
    /// Suppress same-file references for exported interfaces.
    #[serde(default)]
    pub interface: bool,
}

/// User-facing configuration loaded from `.fallowrc.json`, `.fallowrc.jsonc`, `fallow.toml`, or `.fallow.toml`.
///
/// # Examples
///
/// ```
/// use fallow_config::FallowConfig;
///
/// // Default config has sensible defaults
/// let config = FallowConfig::default();
/// assert!(config.entry.is_empty());
/// assert!(!config.production);
///
/// // Deserialize from JSON
/// let config: FallowConfig = serde_json::from_str(r#"{
///     "entry": ["src/main.ts"],
///     "production": true
/// }"#).unwrap();
/// assert_eq!(config.entry, vec!["src/main.ts"]);
/// assert!(config.production);
/// ```
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FallowConfig {
    /// JSON Schema reference (ignored during deserialization).
    #[serde(rename = "$schema", default, skip_serializing)]
    pub schema: Option<String>,

    /// Base config files to extend from.
    ///
    /// Supports three resolution strategies:
    /// - **Relative paths**: `"./base.json"` — resolved relative to the config file.
    /// - **npm packages**: `"npm:@co/config"` — resolved by walking up `node_modules/`.
    ///   Package resolution checks `package.json` `exports`/`main` first, then falls back
    ///   to standard config file names. Subpaths are supported (e.g., `npm:@co/config/strict.json`).
    /// - **HTTPS URLs**: `"https://example.com/fallow-base.json"` — fetched remotely.
    ///   Only HTTPS is supported (no plain HTTP). URL-sourced configs may extend other
    ///   URLs or `npm:` packages, but not relative paths. Only JSON/JSONC format is
    ///   supported for remote configs. Timeout is configurable via
    ///   `FALLOW_EXTENDS_TIMEOUT_SECS` (default: 5s).
    ///
    /// Base configs are loaded first, then this config's values override them.
    /// Later entries in the array override earlier ones.
    ///
    /// **Note:** `npm:` resolution uses `node_modules/` directory walk-up and is
    /// incompatible with Yarn Plug'n'Play (PnP), which has no `node_modules/`.
    /// URL extends fetch on every run (no caching). For reliable CI, prefer `npm:`
    /// for private or critical configs.
    #[serde(default, skip_serializing)]
    pub extends: Vec<String>,

    /// Additional entry point glob patterns.
    #[serde(default)]
    pub entry: Vec<String>,

    /// Glob patterns to ignore from analysis.
    #[serde(default)]
    pub ignore_patterns: Vec<String>,

    /// Custom framework definitions (inline plugin definitions).
    #[serde(default)]
    pub framework: Vec<ExternalPluginDef>,

    /// Workspace overrides.
    #[serde(default)]
    pub workspaces: Option<WorkspaceConfig>,

    /// Dependencies to ignore (always considered used and always considered available).
    ///
    /// Listed dependencies are excluded from both unused dependency and unlisted
    /// dependency detection. Useful for runtime-provided packages like `bun:sqlite`
    /// or implicitly available dependencies.
    #[serde(default)]
    pub ignore_dependencies: Vec<String>,

    /// Export ignore rules.
    #[serde(default)]
    pub ignore_exports: Vec<IgnoreExportRule>,

    /// Rules for suppressing `unresolved-catalog-reference` findings.
    ///
    /// Each rule matches by package name, optionally scoped to a specific
    /// catalog and/or consumer `package.json` glob. Useful for staged catalog
    /// migrations where the catalog edit lands separately from the consumer
    /// edit, and for library-internal placeholder packages whose target
    /// catalog isn't ready yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_catalog_references: Vec<IgnoreCatalogReferenceRule>,

    /// Suppress unused-export findings when the exported symbol is referenced
    /// inside the file that declares it. This mirrors Knip's
    /// `ignoreExportsUsedInFile` option while still reporting exports that have
    /// no references at all.
    #[serde(default)]
    pub ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig,

    /// Class member method/property rules that should never be flagged as
    /// unused. Supports plain member names for global suppression and scoped
    /// objects with `extends` / `implements` constraints for framework-invoked
    /// methods that should only be suppressed on matching classes.
    #[serde(default)]
    pub used_class_members: Vec<UsedClassMemberRule>,

    /// Duplication detection settings.
    #[serde(default)]
    pub duplicates: DuplicatesConfig,

    /// Complexity health metrics settings.
    #[serde(default)]
    pub health: HealthConfig,

    /// Per-issue-type severity rules.
    #[serde(default)]
    pub rules: RulesConfig,

    /// Architecture boundary enforcement configuration.
    #[serde(default)]
    pub boundaries: BoundaryConfig,

    /// Feature flag detection configuration.
    #[serde(default)]
    pub flags: FlagsConfig,

    /// Module resolver configuration (custom conditions, etc.).
    #[serde(default)]
    pub resolve: ResolveConfig,

    /// Production mode: exclude test/dev files, only start/build scripts.
    ///
    /// Accepts the legacy boolean form (`true` applies to all analyses) or a
    /// per-analysis object (`{ "deadCode": false, "health": true, "dupes": false }`).
    #[serde(default)]
    pub production: ProductionConfig,

    /// Paths to external plugin files or directories containing plugin files.
    ///
    /// Supports TOML, JSON, and JSONC formats.
    ///
    /// In addition to these explicit paths, fallow automatically discovers:
    /// - `*.toml`, `*.json`, `*.jsonc` files in `.fallow/plugins/`
    /// - `fallow-plugin-*.{toml,json,jsonc}` files in the project root
    #[serde(default)]
    pub plugins: Vec<String>,

    /// Glob patterns for files that are dynamically loaded at runtime
    /// (plugin directories, locale files, etc.). These files are treated as
    /// always-used and will never be flagged as unused.
    #[serde(default)]
    pub dynamically_loaded: Vec<String>,

    /// Per-file rule overrides matching oxlint's overrides pattern.
    #[serde(default)]
    pub overrides: Vec<ConfigOverride>,

    /// Path to a CODEOWNERS file for `--group-by owner`.
    ///
    /// When unset, fallow auto-probes `CODEOWNERS`, `.github/CODEOWNERS`,
    /// `.gitlab/CODEOWNERS`, and `docs/CODEOWNERS`. Set this to use a
    /// non-standard location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codeowners: Option<String>,

    /// Workspace package name patterns that are public libraries.
    /// Exported API surface from these packages is not flagged as unused.
    #[serde(default)]
    pub public_packages: Vec<String>,

    /// Regression detection baseline embedded in config.
    /// Stores issue counts from a known-good state for CI regression checks.
    /// Populated by `--save-regression-baseline` (no path), read by `--fail-on-regression`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regression: Option<RegressionConfig>,

    /// Audit command baseline paths (one per analysis: dead-code, health, dupes).
    ///
    /// `fallow audit` runs three analyses and each has its own baseline format.
    /// Paths in this section are resolved relative to the project root. CLI flags
    /// (`--dead-code-baseline`, `--health-baseline`, `--dupes-baseline`) override
    /// these values when provided.
    #[serde(default, skip_serializing_if = "AuditConfig::is_empty")]
    pub audit: AuditConfig,

    /// Mark this config as sealed: `extends` paths must be file-relative and
    /// resolve within this config's own directory. `npm:` and `https:` extends
    /// are rejected. Useful for library publishers and monorepo sub-packages
    /// that want to guarantee their config is self-contained and not subject
    /// to ancestor configs being injected via `extends`.
    ///
    /// Discovery is unaffected (first-match-wins already stops the directory
    /// walk at the nearest config). This only constrains `extends`.
    #[serde(default)]
    pub sealed: bool,

    /// Report unused exports in entry files instead of auto-marking them as
    /// used. Catches typos in framework exports (e.g. `meatdata` instead of
    /// `metadata`). The CLI flag `--include-entry-exports` (global) overrides
    /// this when set; otherwise the config value is used.
    #[serde(default)]
    pub include_entry_exports: bool,
}

/// Analysis-specific production-mode selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionAnalysis {
    DeadCode,
    Health,
    Dupes,
}

/// Production-mode defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ProductionConfig {
    /// Legacy/global form: `production = true` or `"production": true`.
    Global(bool),
    /// Per-analysis form.
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

/// Per-analysis production-mode defaults.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct PerAnalysisProductionConfig {
    /// Production mode for dead-code analysis.
    pub dead_code: bool,
    /// Production mode for health analysis.
    pub health: bool,
    /// Production mode for duplication analysis.
    pub dupes: bool,
}

/// Per-analysis baseline paths for the `audit` command.
///
/// Each field points to a baseline file produced by the corresponding
/// subcommand (`fallow dead-code --save-baseline`, `fallow health --save-baseline`,
/// `fallow dupes --save-baseline`). `audit` passes each baseline through to its
/// underlying analysis; baseline-matched issues are excluded from the verdict.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditConfig {
    /// Which findings should make `fallow audit` fail.
    #[serde(default, skip_serializing_if = "AuditGate::is_default")]
    pub gate: AuditGate,

    /// Path to the dead-code baseline (produced by `fallow dead-code --save-baseline`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code_baseline: Option<String>,

    /// Path to the health baseline (produced by `fallow health --save-baseline`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_baseline: Option<String>,

    /// Path to the duplication baseline (produced by `fallow dupes --save-baseline`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes_baseline: Option<String>,
}

impl AuditConfig {
    /// True when all baseline paths are unset.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.gate.is_default()
            && self.dead_code_baseline.is_none()
            && self.health_baseline.is_none()
            && self.dupes_baseline.is_none()
    }
}

/// Gating mode for `fallow audit`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AuditGate {
    /// Only findings introduced by the current changeset affect the verdict.
    #[default]
    NewOnly,
    /// All findings in changed files affect the verdict.
    All,
}

impl AuditGate {
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::NewOnly)
    }
}

/// Regression baseline counts, embedded in the config file.
///
/// When `--fail-on-regression` is used without `--regression-baseline <PATH>`,
/// fallow reads the baseline from this config section.
/// When `--save-regression-baseline` is used without a path argument,
/// fallow writes the baseline into the config file.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegressionConfig {
    /// Dead code issue counts baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<RegressionBaseline>,
}

/// Per-type issue counts for regression comparison.
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
    pub type_only_dependencies: usize,
    #[serde(default)]
    pub test_only_dependencies: usize,
    #[serde(default)]
    pub boundary_violations: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default trait ───────────────────────────────────────────────

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

    // ── JSON deserialization ────────────────────────────────────────

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

    // ── TOML deserialization ────────────────────────────────────────

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

    // ── Serialization roundtrip ─────────────────────────────────────

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
        // $schema has skip_serializing, should not appear in output
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

    // ── RegressionConfig / RegressionBaseline ──────────────────────

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
        // Unset fields default to 0
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

    // ── JSON config with overrides and boundaries ──────────────────

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

    // ── TOML with regression config ────────────────────────────────

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

    // ── TOML with multiple overrides ───────────────────────────────

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

    // ── Default regression config ──────────────────────────────────

    #[test]
    fn regression_config_default_is_none_baseline() {
        let config = RegressionConfig::default();
        assert!(config.baseline.is_none());
    }

    // ── Config with multiple ignore export rules ───────────────────

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

    // ── Public packages ───────────────────────────────────────────

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
