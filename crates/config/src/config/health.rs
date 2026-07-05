use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const fn default_max_cyclomatic() -> u16 {
    20
}

const fn default_max_cognitive() -> u16 {
    15
}

/// Savoia and Evans (2007) canonical CRAP threshold: CC=5 untested gives
/// exactly `5^2 + 5 = 30`, marking the boundary where refactoring or test
/// coverage becomes recommended.
const fn default_max_crap() -> f64 {
    30.0
}

const fn default_crap_refactor_band() -> u16 {
    5
}

/// SIG unit-size "very high risk" boundary: functions over 60 lines of code.
/// This is the default line-count threshold above which a function is reported
/// as an oversized "large function".
const fn default_max_unit_size() -> u32 {
    60
}

/// Default for `suggest_inline_suppression`: emit `suppress-line` actions
/// alongside health findings unless a baseline is active or the team has
/// opted out via config.
const fn default_suggest_inline_suppression() -> bool {
    true
}

/// Default bot/service-account author patterns filtered from ownership metrics.
///
/// Matches common CI bot signatures and service-account naming conventions.
/// Users can extend via `health.ownership.botPatterns` in config.
///
/// Note on `[bot]` matching: globset treats `[abc]` as a character class.
/// To match the literal `[bot]` substring (used by GitHub App bots), escape
/// the brackets as `\[bot\]`.
///
/// `*noreply*` is intentionally NOT a default. Most human GitHub contributors
/// commit from `<id>+<handle>@users.noreply.github.com` addresses (GitHub's
/// privacy default). Filtering on `noreply` would silently exclude the
/// majority of real authors. The actual bot accounts already match via the
/// `\[bot\]` literal (e.g., `github-actions[bot]@users.noreply.github.com`).
fn default_bot_patterns() -> Vec<String> {
    vec![
        r"*\[bot\]*".to_string(),
        "dependabot*".to_string(),
        "renovate*".to_string(),
        "github-actions*".to_string(),
        "svc-*".to_string(),
        "*-service-account*".to_string(),
    ]
}

const fn default_email_mode() -> EmailMode {
    EmailMode::Handle
}

/// Privacy mode for author emails emitted in ownership output.
///
/// Defaults to `handle` (local-part only, no domain) so SARIF and JSON
/// artifacts do not leak raw email addresses into CI pipelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EmailMode {
    /// Show the raw email address as it appears in git history.
    /// Use for public repositories where history is already exposed.
    Raw,
    /// Show the local-part only (before the `@`). Mailmap-resolved where possible.
    /// Default. Balances readability and privacy.
    Handle,
    /// Show a stable `xxh3:<16hex>` pseudonym derived from the raw email.
    /// Non-cryptographic; suitable to keep raw emails out of CI artifacts
    /// (SARIF, code-scanning uploads) but not as a security primitive:
    /// a known list of org emails can be brute-forced into a rainbow table.
    /// Use in regulated environments where even local-parts are sensitive.
    Anonymized,
    /// Legacy spelling for [`EmailMode::Anonymized`].
    Hash,
}

/// Configuration for ownership analysis (`fallow health --hotspots --ownership`).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipConfig {
    /// Glob patterns (matched against the author email local-part) that
    /// identify bot or service-account commits to exclude from ownership
    /// signals. Overrides the defaults entirely when set.
    #[serde(default = "default_bot_patterns")]
    pub bot_patterns: Vec<String>,

    /// Privacy mode for emitted author emails. Defaults to `handle`.
    /// Override on the CLI via `--ownership-emails=raw|handle|anonymized`.
    /// The legacy spelling `hash` is still accepted for compatibility.
    #[serde(default = "default_email_mode")]
    pub email_mode: EmailMode,
}

impl Default for OwnershipConfig {
    fn default() -> Self {
        Self {
            bot_patterns: default_bot_patterns(),
            email_mode: default_email_mode(),
        }
    }
}

/// Configuration for complexity health metrics (`fallow health`).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HealthConfig {
    /// Maximum allowed cyclomatic complexity per function (default: 20).
    /// Functions exceeding this threshold are reported.
    #[serde(default = "default_max_cyclomatic")]
    pub max_cyclomatic: u16,

    /// Maximum allowed cognitive complexity per function (default: 15).
    /// Functions exceeding this threshold are reported.
    #[serde(default = "default_max_cognitive")]
    pub max_cognitive: u16,

    /// Maximum allowed CRAP (Change Risk Anti-Patterns) score per function
    /// (default: 30.0). CRAP combines cyclomatic complexity with test
    /// coverage: high complexity plus low coverage produces a high CRAP
    /// score. Functions meeting or exceeding this threshold are reported.
    /// Use `--coverage` with Istanbul data for accurate per-function CRAP;
    /// otherwise fallow estimates coverage from the module graph.
    #[serde(default = "default_max_crap")]
    pub max_crap: f64,

    /// Band below `maxCyclomatic` where CRAP-only findings also receive a
    /// secondary `refactor-function` action (default: 5). Set to `0` to only
    /// suggest refactoring when cyclomatic already meets the configured
    /// threshold.
    #[serde(default = "default_crap_refactor_band")]
    pub crap_refactor_band: u16,

    /// Maximum function length in lines of code before it is reported as an
    /// oversized "large function" (default: 60). Raise it globally, or per file
    /// via `thresholdOverrides[].maxUnitSize`, to relax the bar for generated or
    /// test files (where a `describe()` block spans hundreds of lines) without
    /// disabling complexity checks on those files. This filters the reported
    /// large-functions list only; the descriptive unit-size profile and the
    /// health score still reflect raw sizes (use `health.ignore` to remove a
    /// file from the score entirely).
    #[serde(default = "default_max_unit_size")]
    pub max_unit_size: u32,

    /// Path to Istanbul-format coverage data for accurate per-function CRAP
    /// scores. Relative paths resolve against the project root. The CLI
    /// `--coverage` flag and `FALLOW_COVERAGE` environment variable override
    /// this value.
    #[serde(default)]
    pub coverage: Option<PathBuf>,

    /// Absolute prefix to strip from Istanbul file paths before CRAP matching.
    /// Use when coverage was generated under a different checkout root in CI
    /// or Docker. The CLI `--coverage-root` flag and `FALLOW_COVERAGE_ROOT`
    /// environment variable override this value.
    #[serde(default)]
    pub coverage_root: Option<PathBuf>,

    /// Glob patterns to exclude from complexity analysis.
    #[serde(default)]
    pub ignore: Vec<String>,

    /// Per-file or per-function threshold overrides. These keep exceptional
    /// functions visible as configured numeric ceilings instead of hiding them
    /// behind binary suppressions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threshold_overrides: Vec<HealthThresholdOverride>,

    /// Ownership analysis configuration. Controls bot filtering and email
    /// privacy mode for `--ownership` output.
    #[serde(default)]
    pub ownership: OwnershipConfig,

    /// Whether health JSON output emits `suppress-line` action hints
    /// alongside complexity findings (default: `true`). Set to `false` to
    /// opt out across the project: useful for teams that manage suppressions
    /// exclusively through `// fallow-ignore-*` comments authored by hand or
    /// through the `fallow.suppress` LSP code action, but who do not want
    /// CI-driven `suppress-line` action hints in their JSON output.
    /// `--baseline` activates auto-omission regardless of this setting,
    /// since baseline files are a separate suppression mechanism.
    #[serde(default = "default_suggest_inline_suppression")]
    pub suggest_inline_suppression: bool,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            max_cyclomatic: default_max_cyclomatic(),
            max_cognitive: default_max_cognitive(),
            max_crap: default_max_crap(),
            crap_refactor_band: default_crap_refactor_band(),
            max_unit_size: default_max_unit_size(),
            coverage: None,
            coverage_root: None,
            ignore: vec![],
            threshold_overrides: vec![],
            ownership: OwnershipConfig::default(),
            suggest_inline_suppression: default_suggest_inline_suppression(),
        }
    }
}

/// Per-file or per-function health threshold override.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HealthThresholdOverride {
    /// Project-root-relative file globs this override applies to.
    pub files: Vec<String>,
    /// Exact emitted function names this override applies to. Empty means every
    /// function in matching files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<String>,
    /// Local cyclomatic complexity ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cyclomatic: Option<u16>,
    /// Local cognitive complexity ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cognitive: Option<u16>,
    /// Local CRAP ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_crap: Option<f64>,
    /// Local unit-size ceiling: maximum function length in lines of code before
    /// it is reported as an oversized "large function". Leave `functions` empty
    /// to relax the bar for every function in the matching files (which covers
    /// both the `describe()` wrapper and the individual `it()` blocks in a test
    /// suite).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_unit_size: Option<u32>,
    /// Human-readable rationale for the exception.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl HealthThresholdOverride {
    /// Return true when the override configures at least one local ceiling.
    #[must_use]
    pub const fn has_any_threshold(&self) -> bool {
        self.max_cyclomatic.is_some()
            || self.max_cognitive.is_some()
            || self.max_crap.is_some()
            || self.max_unit_size.is_some()
    }
}

impl HealthConfig {
    /// Validate semantic constraints that serde cannot express.
    #[must_use]
    pub fn threshold_override_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for (index, override_entry) in self.threshold_overrides.iter().enumerate() {
            if override_entry.files.is_empty() {
                errors.push(format!(
                    "health.thresholdOverrides[{index}].files must contain at least one pattern"
                ));
            }
            if !override_entry.has_any_threshold() {
                errors.push(format!(
                    "health.thresholdOverrides[{index}] must set at least one of maxCyclomatic, maxCognitive, maxCrap, or maxUnitSize"
                ));
            }
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_config_defaults() {
        let config = HealthConfig::default();
        assert_eq!(config.max_cyclomatic, 20);
        assert_eq!(config.max_cognitive, 15);
        assert!((config.max_crap - 30.0).abs() < f64::EPSILON);
        assert_eq!(config.crap_refactor_band, 5);
        assert_eq!(config.max_unit_size, 60);
        assert!(config.coverage.is_none());
        assert!(config.coverage_root.is_none());
        assert!(config.ignore.is_empty());
        assert!(config.threshold_overrides.is_empty());
    }

    #[test]
    fn health_config_json_all_fields() {
        let json = r#"{
            "maxCyclomatic": 30,
            "maxCognitive": 25,
            "maxCrap": 50.0,
            "crapRefactorBand": 3,
            "coverage": "coverage/coverage-final.json",
            "coverageRoot": "/ci/workspace",
            "ignore": ["**/generated/**", "vendor/**"],
            "thresholdOverrides": [{
                "files": ["components/auth/src/index.ts"],
                "functions": ["createAuthModule"],
                "maxCognitive": 25,
                "reason": "linear module assembly; agreed 2026-06"
            }]
        }"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_cyclomatic, 30);
        assert_eq!(config.max_cognitive, 25);
        assert!((config.max_crap - 50.0).abs() < f64::EPSILON);
        assert_eq!(config.crap_refactor_band, 3);
        assert_eq!(
            config.coverage,
            Some(PathBuf::from("coverage/coverage-final.json"))
        );
        assert_eq!(config.coverage_root, Some(PathBuf::from("/ci/workspace")));
        assert_eq!(config.ignore, vec!["**/generated/**", "vendor/**"]);
        assert_eq!(config.threshold_overrides.len(), 1);
        assert_eq!(
            config.threshold_overrides[0].files,
            vec!["components/auth/src/index.ts"]
        );
        assert_eq!(
            config.threshold_overrides[0].functions,
            vec!["createAuthModule"]
        );
        assert_eq!(config.threshold_overrides[0].max_cognitive, Some(25));
    }

    #[test]
    fn health_config_json_partial_uses_defaults() {
        let json = r#"{"maxCyclomatic": 10}"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_cyclomatic, 10);
        assert_eq!(config.max_cognitive, 15); // default
        assert!((config.max_crap - 30.0).abs() < f64::EPSILON); // default
        assert_eq!(config.crap_refactor_band, 5); // default
        assert!(config.ignore.is_empty()); // default
        assert!(config.threshold_overrides.is_empty()); // default
    }

    #[test]
    fn health_config_json_only_max_crap() {
        let json = r#"{"maxCrap": 15.5}"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert!((config.max_crap - 15.5).abs() < f64::EPSILON);
        assert_eq!(config.max_cyclomatic, 20); // default
        assert_eq!(config.max_cognitive, 15); // default
        assert_eq!(config.crap_refactor_band, 5); // default
    }

    #[test]
    fn health_config_json_empty_object_uses_all_defaults() {
        let config: HealthConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.max_cyclomatic, 20);
        assert_eq!(config.max_cognitive, 15);
        assert_eq!(config.crap_refactor_band, 5);
        assert!(config.ignore.is_empty());
        assert!(config.threshold_overrides.is_empty());
    }

    #[test]
    fn health_config_json_only_ignore() {
        let json = r#"{"ignore": ["test/**"]}"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_cyclomatic, 20); // default
        assert_eq!(config.max_cognitive, 15); // default
        assert_eq!(config.ignore, vec!["test/**"]);
        assert!(config.threshold_overrides.is_empty());
    }

    #[test]
    fn health_config_toml_all_fields() {
        let toml_str = r#"
maxCyclomatic = 25
maxCognitive = 20
ignore = ["generated/**", "vendor/**"]

[[thresholdOverrides]]
files = ["src/auth.ts"]
maxCognitive = 25
"#;
        let config: HealthConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.max_cyclomatic, 25);
        assert_eq!(config.max_cognitive, 20);
        assert_eq!(config.ignore, vec!["generated/**", "vendor/**"]);
        assert_eq!(config.threshold_overrides.len(), 1);
        assert_eq!(config.threshold_overrides[0].max_cognitive, Some(25));
    }

    #[test]
    fn health_config_toml_defaults() {
        let config: HealthConfig = toml::from_str("").unwrap();
        assert_eq!(config.max_cyclomatic, 20);
        assert_eq!(config.max_cognitive, 15);
        assert!(config.ignore.is_empty());
        assert!(config.threshold_overrides.is_empty());
    }

    #[test]
    fn health_config_json_roundtrip() {
        let config = HealthConfig {
            max_cyclomatic: 50,
            max_cognitive: 40,
            max_crap: 75.0,
            crap_refactor_band: 4,
            max_unit_size: 120,
            ignore: vec!["test/**".to_string()],
            threshold_overrides: vec![HealthThresholdOverride {
                files: vec!["src/auth.ts".to_string()],
                functions: Vec::new(),
                max_cyclomatic: Some(30),
                max_cognitive: None,
                max_crap: Some(45.0),
                max_unit_size: None,
                reason: Some("framework assembly".to_string()),
            }],
            coverage: None,
            coverage_root: None,
            ownership: OwnershipConfig::default(),
            suggest_inline_suppression: false,
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: HealthConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.max_cyclomatic, 50);
        assert_eq!(restored.max_cognitive, 40);
        assert!((restored.max_crap - 75.0).abs() < f64::EPSILON);
        assert_eq!(restored.crap_refactor_band, 4);
        assert_eq!(restored.max_unit_size, 120);
        assert_eq!(restored.ignore, vec!["test/**"]);
        assert_eq!(restored.threshold_overrides.len(), 1);
        assert_eq!(restored.threshold_overrides[0].max_cyclomatic, Some(30));
        assert_eq!(restored.threshold_overrides[0].max_crap, Some(45.0));
        assert!(!restored.suggest_inline_suppression);
    }

    #[test]
    fn health_config_threshold_override_omitted_functions_matches_all() {
        let json = r#"{
            "thresholdOverrides": [{
                "files": ["src/auth.ts"],
                "maxCognitive": 25
            }]
        }"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        let override_entry = &config.threshold_overrides[0];
        assert!(override_entry.functions.is_empty());
        assert_eq!(override_entry.max_cognitive, Some(25));
        assert!(config.threshold_override_errors().is_empty());
    }

    #[test]
    fn health_config_threshold_override_validation_requires_files() {
        let json = r#"{
            "thresholdOverrides": [{
                "files": [],
                "maxCognitive": 25
            }]
        }"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.threshold_override_errors(),
            vec!["health.thresholdOverrides[0].files must contain at least one pattern"]
        );
    }

    #[test]
    fn health_config_threshold_override_validation_requires_threshold() {
        let json = r#"{
            "thresholdOverrides": [{
                "files": ["src/auth.ts"],
                "reason": "temporary"
            }]
        }"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.threshold_override_errors(),
            vec![
                "health.thresholdOverrides[0] must set at least one of maxCyclomatic, maxCognitive, maxCrap, or maxUnitSize"
            ]
        );
    }

    #[test]
    fn health_config_threshold_override_max_unit_size_only_is_valid() {
        let json = r#"{
            "thresholdOverrides": [{
                "files": ["**/*.test.*"],
                "maxUnitSize": 500
            }]
        }"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        let override_entry = &config.threshold_overrides[0];
        assert_eq!(override_entry.max_unit_size, Some(500));
        assert!(override_entry.max_cyclomatic.is_none());
        assert!(override_entry.has_any_threshold());
        assert!(config.threshold_override_errors().is_empty());
    }

    #[test]
    fn health_config_json_only_max_unit_size() {
        let json = r#"{"maxUnitSize": 100}"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_unit_size, 100);
        assert_eq!(config.max_cyclomatic, 20); // default
        assert!(config.threshold_overrides.is_empty());
    }

    #[test]
    fn health_config_threshold_override_rejects_unknown_keys() {
        let err = serde_json::from_str::<HealthConfig>(
            r#"{"thresholdOverrides":[{"files":["src/auth.ts"],"maxCogntive":25}]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("maxCogntive"));
    }

    #[test]
    fn health_config_suggest_inline_suppression_default_true() {
        let config = HealthConfig::default();
        assert!(config.suggest_inline_suppression);
    }

    #[test]
    fn health_config_suggest_inline_suppression_explicit_false() {
        let json = r#"{"suggestInlineSuppression": false}"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert!(!config.suggest_inline_suppression);
    }

    #[test]
    fn health_config_suggest_inline_suppression_omitted_uses_default() {
        let config: HealthConfig = serde_json::from_str("{}").unwrap();
        assert!(config.suggest_inline_suppression);
    }

    #[test]
    fn health_config_zero_thresholds() {
        let json = r#"{"maxCyclomatic": 0, "maxCognitive": 0}"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_cyclomatic, 0);
        assert_eq!(config.max_cognitive, 0);
    }

    #[test]
    fn health_config_large_thresholds() {
        let json = r#"{"maxCyclomatic": 65535, "maxCognitive": 65535}"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_cyclomatic, u16::MAX);
        assert_eq!(config.max_cognitive, u16::MAX);
    }

    #[test]
    fn ownership_config_default_has_bot_patterns() {
        let cfg = OwnershipConfig::default();
        assert!(cfg.bot_patterns.iter().any(|p| p == r"*\[bot\]*"));
        assert!(cfg.bot_patterns.iter().any(|p| p == "dependabot*"));
        assert!(cfg.bot_patterns.iter().any(|p| p == "github-actions*"));
        assert!(
            !cfg.bot_patterns.iter().any(|p| p == "*noreply*"),
            "*noreply* must not be a default bot pattern (filters real human \
             contributors using GitHub's privacy default email)"
        );
        assert_eq!(cfg.email_mode, EmailMode::Handle);
    }

    #[test]
    fn ownership_config_default_via_health() {
        let cfg = HealthConfig::default();
        assert_eq!(cfg.ownership.email_mode, EmailMode::Handle);
        assert!(!cfg.ownership.bot_patterns.is_empty());
    }

    #[test]
    fn ownership_config_json_overrides_defaults() {
        let json = r#"{
            "ownership": {
                "botPatterns": ["custom-bot*"],
                "emailMode": "raw"
            }
        }"#;
        let config: HealthConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.ownership.bot_patterns, vec!["custom-bot*"]);
        assert_eq!(config.ownership.email_mode, EmailMode::Raw);
    }

    #[test]
    fn ownership_config_email_mode_kebab_case() {
        for (mode, repr) in [
            (EmailMode::Raw, "\"raw\""),
            (EmailMode::Handle, "\"handle\""),
            (EmailMode::Anonymized, "\"anonymized\""),
            (EmailMode::Hash, "\"hash\""),
        ] {
            let s = serde_json::to_string(&mode).unwrap();
            assert_eq!(s, repr);
            let back: EmailMode = serde_json::from_str(repr).unwrap();
            assert_eq!(back, mode);
        }
    }

    #[test]
    fn ownership_config_email_mode_accepts_legacy_hash_alias() {
        let back: EmailMode = serde_json::from_str("\"hash\"").unwrap();
        assert_eq!(back, EmailMode::Hash);
    }
}
