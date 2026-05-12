use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

const fn default_true() -> bool {
    true
}

const fn default_min_tokens() -> usize {
    50
}

const fn default_min_lines() -> usize {
    5
}

const fn default_min_occurrences() -> usize {
    2
}

/// Reject `< 2` at deserialize time. A single occurrence isn't a duplicate;
/// silently clamping would poison reproducibility across config / env / CLI
/// override sources.
fn deserialize_min_occurrences<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let value = usize::deserialize(deserializer)?;
    if value < 2 {
        return Err(serde::de::Error::custom(format!(
            "minOccurrences must be at least 2 (got {value}); a single occurrence isn't a duplicate"
        )));
    }
    Ok(value)
}

const fn default_min_corpus_size_for_shingle_filter() -> usize {
    1024
}

const fn default_min_corpus_size_for_token_cache() -> usize {
    5_000
}

/// Configuration for code duplication detection.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesConfig {
    /// Whether duplication detection is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Detection mode: strict, mild, weak, or semantic.
    #[serde(default)]
    pub mode: DetectionMode,

    /// Minimum number of tokens for a clone.
    #[serde(default = "default_min_tokens")]
    pub min_tokens: usize,

    /// Minimum number of lines for a clone.
    #[serde(default = "default_min_lines")]
    pub min_lines: usize,

    /// Minimum number of occurrences (instances of the same clone) before a
    /// group is reported. Defaults to 2 (every duplicated pair is reported).
    /// Raise this to focus on widespread copy-paste worth refactoring and skip
    /// context-sensitive pairs.
    #[serde(
        default = "default_min_occurrences",
        deserialize_with = "deserialize_min_occurrences"
    )]
    #[schemars(range(min = 2))]
    pub min_occurrences: usize,

    /// Maximum allowed duplication percentage (0 = no limit).
    #[serde(default)]
    pub threshold: f64,

    /// Additional ignore patterns for duplication analysis.
    #[serde(default)]
    pub ignore: Vec<String>,

    /// Merge built-in generated-framework ignore patterns with `ignore`.
    ///
    /// Set to `false` to use only the user-provided `ignore` list.
    #[serde(default = "default_true")]
    pub ignore_defaults: bool,

    /// Only report cross-directory duplicates.
    #[serde(default)]
    pub skip_local: bool,

    /// Enable cross-language clone detection by stripping type annotations.
    ///
    /// When enabled, TypeScript type annotations (parameter types, return types,
    /// generics, interfaces, type aliases) are stripped from the token stream,
    /// allowing detection of clones between `.ts` and `.js` files.
    #[serde(default)]
    pub cross_language: bool,

    /// Exclude ES `import` declarations from clone detection.
    ///
    /// When enabled, all `import` statements (value imports, type imports, and
    /// side-effect imports) are stripped from the token stream before clone
    /// detection. This reduces noise from sorted import blocks that naturally
    /// look similar across files. Only affects ES `import` declarations;
    /// CommonJS `require()` calls are not filtered.
    #[serde(default)]
    pub ignore_imports: bool,

    /// Fine-grained normalization overrides on top of the detection mode.
    #[serde(default)]
    pub normalization: NormalizationConfig,

    /// Minimum tokenized file count before focused duplicate analysis prefilters
    /// unchanged files with k-token shingles.
    #[serde(default = "default_min_corpus_size_for_shingle_filter")]
    pub min_corpus_size_for_shingle_filter: usize,

    /// Minimum source file count before the persistent duplication token cache
    /// activates. Below this threshold the cache load/save overhead exceeds the
    /// tokenize savings, so the cache stays disabled even when not running with
    /// `--no-cache`.
    #[serde(default = "default_min_corpus_size_for_token_cache")]
    pub min_corpus_size_for_token_cache: usize,
}

impl Default for DuplicatesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: DetectionMode::default(),
            min_tokens: default_min_tokens(),
            min_lines: default_min_lines(),
            min_occurrences: default_min_occurrences(),
            threshold: 0.0,
            ignore: vec![],
            ignore_defaults: true,
            skip_local: false,
            cross_language: false,
            ignore_imports: false,
            normalization: NormalizationConfig::default(),
            min_corpus_size_for_shingle_filter: default_min_corpus_size_for_shingle_filter(),
            min_corpus_size_for_token_cache: default_min_corpus_size_for_token_cache(),
        }
    }
}

/// Fine-grained normalization overrides.
///
/// Each option, when set to `Some(true)`, forces that normalization regardless of
/// the detection mode. When set to `Some(false)`, it forces preservation. When
/// `None`, the detection mode's default behavior applies.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NormalizationConfig {
    /// Blind all identifiers (variable names, function names, etc.) to the same hash.
    /// Default in `semantic` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_identifiers: Option<bool>,

    /// Blind string literal values to the same hash.
    /// Default in `weak` and `semantic` modes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_string_values: Option<bool>,

    /// Blind numeric literal values to the same hash.
    /// Default in `semantic` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_numeric_values: Option<bool>,
}

/// Resolved normalization flags: mode defaults merged with user overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedNormalization {
    pub ignore_identifiers: bool,
    pub ignore_string_values: bool,
    pub ignore_numeric_values: bool,
}

impl ResolvedNormalization {
    /// Resolve normalization from a detection mode and optional overrides.
    #[must_use]
    pub fn resolve(mode: DetectionMode, overrides: &NormalizationConfig) -> Self {
        let (default_ids, default_strings, default_numbers) = match mode {
            DetectionMode::Strict | DetectionMode::Mild => (false, false, false),
            DetectionMode::Weak => (false, true, false),
            DetectionMode::Semantic => (true, true, true),
        };

        Self {
            ignore_identifiers: overrides.ignore_identifiers.unwrap_or(default_ids),
            ignore_string_values: overrides.ignore_string_values.unwrap_or(default_strings),
            ignore_numeric_values: overrides.ignore_numeric_values.unwrap_or(default_numbers),
        }
    }
}

/// Detection mode controlling how aggressively tokens are normalized.
///
/// Since fallow uses AST-based tokenization (not lexer-based), whitespace and
/// comments are inherently absent from the token stream. The `Strict` and `Mild`
/// modes are currently equivalent. `Weak` mode additionally blinds string
/// literals. `Semantic` mode blinds all identifiers and literal values for
/// Type-2 (renamed variable) clone detection.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DetectionMode {
    /// All tokens preserved including identifier names and literal values (Type-1 only).
    Strict,
    /// Default mode -- equivalent to strict for AST-based tokenization.
    #[default]
    Mild,
    /// Blind string literal values (structure-preserving).
    Weak,
    /// Blind all identifiers and literal values for structural (Type-2) detection.
    Semantic,
}

impl std::fmt::Display for DetectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Strict => write!(f, "strict"),
            Self::Mild => write!(f, "mild"),
            Self::Weak => write!(f, "weak"),
            Self::Semantic => write!(f, "semantic"),
        }
    }
}

impl std::str::FromStr for DetectionMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "strict" => Ok(Self::Strict),
            "mild" => Ok(Self::Mild),
            "weak" => Ok(Self::Weak),
            "semantic" => Ok(Self::Semantic),
            other => Err(format!("unknown detection mode: '{other}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DuplicatesConfig defaults ────────────────────────────────────

    #[test]
    fn duplicates_config_defaults() {
        let config = DuplicatesConfig::default();
        assert!(config.enabled);
        assert_eq!(config.mode, DetectionMode::Mild);
        assert_eq!(config.min_tokens, 50);
        assert_eq!(config.min_lines, 5);
        assert_eq!(config.min_occurrences, 2);
        assert!((config.threshold - 0.0).abs() < f64::EPSILON);
        assert!(config.ignore.is_empty());
        assert!(config.ignore_defaults);
        assert!(!config.skip_local);
        assert!(!config.cross_language);
        assert!(!config.ignore_imports);
        assert_eq!(config.min_corpus_size_for_shingle_filter, 1024);
        assert_eq!(config.min_corpus_size_for_token_cache, 5_000);
    }

    // ── DetectionMode FromStr ────────────────────────────────────────

    #[test]
    fn detection_mode_from_str_all_variants() {
        assert_eq!(
            "strict".parse::<DetectionMode>().unwrap(),
            DetectionMode::Strict
        );
        assert_eq!(
            "mild".parse::<DetectionMode>().unwrap(),
            DetectionMode::Mild
        );
        assert_eq!(
            "weak".parse::<DetectionMode>().unwrap(),
            DetectionMode::Weak
        );
        assert_eq!(
            "semantic".parse::<DetectionMode>().unwrap(),
            DetectionMode::Semantic
        );
    }

    #[test]
    fn detection_mode_from_str_case_insensitive() {
        assert_eq!(
            "STRICT".parse::<DetectionMode>().unwrap(),
            DetectionMode::Strict
        );
        assert_eq!(
            "Weak".parse::<DetectionMode>().unwrap(),
            DetectionMode::Weak
        );
        assert_eq!(
            "SEMANTIC".parse::<DetectionMode>().unwrap(),
            DetectionMode::Semantic
        );
    }

    #[test]
    fn detection_mode_from_str_unknown() {
        let err = "foobar".parse::<DetectionMode>().unwrap_err();
        assert!(err.contains("unknown detection mode"));
        assert!(err.contains("foobar"));
    }

    // ── DetectionMode Display ────────────────────────────────────────

    #[test]
    fn detection_mode_display() {
        assert_eq!(DetectionMode::Strict.to_string(), "strict");
        assert_eq!(DetectionMode::Mild.to_string(), "mild");
        assert_eq!(DetectionMode::Weak.to_string(), "weak");
        assert_eq!(DetectionMode::Semantic.to_string(), "semantic");
    }

    // ── ResolvedNormalization::resolve ────────────────────────────────

    #[test]
    fn resolve_strict_mode_all_false() {
        let resolved =
            ResolvedNormalization::resolve(DetectionMode::Strict, &NormalizationConfig::default());
        assert!(!resolved.ignore_identifiers);
        assert!(!resolved.ignore_string_values);
        assert!(!resolved.ignore_numeric_values);
    }

    #[test]
    fn resolve_mild_mode_all_false() {
        let resolved =
            ResolvedNormalization::resolve(DetectionMode::Mild, &NormalizationConfig::default());
        assert!(!resolved.ignore_identifiers);
        assert!(!resolved.ignore_string_values);
        assert!(!resolved.ignore_numeric_values);
    }

    #[test]
    fn resolve_weak_mode_only_strings_true() {
        let resolved =
            ResolvedNormalization::resolve(DetectionMode::Weak, &NormalizationConfig::default());
        assert!(!resolved.ignore_identifiers);
        assert!(resolved.ignore_string_values);
        assert!(!resolved.ignore_numeric_values);
    }

    #[test]
    fn resolve_semantic_mode_all_true() {
        let resolved = ResolvedNormalization::resolve(
            DetectionMode::Semantic,
            &NormalizationConfig::default(),
        );
        assert!(resolved.ignore_identifiers);
        assert!(resolved.ignore_string_values);
        assert!(resolved.ignore_numeric_values);
    }

    #[test]
    fn resolve_override_forces_true() {
        // Strict mode defaults to all false, but override forces ignore_identifiers to true
        let overrides = NormalizationConfig {
            ignore_identifiers: Some(true),
            ignore_string_values: None,
            ignore_numeric_values: None,
        };
        let resolved = ResolvedNormalization::resolve(DetectionMode::Strict, &overrides);
        assert!(resolved.ignore_identifiers);
        assert!(!resolved.ignore_string_values);
        assert!(!resolved.ignore_numeric_values);
    }

    #[test]
    fn resolve_override_forces_false() {
        // Semantic mode defaults to all true, but override forces ignore_identifiers to false
        let overrides = NormalizationConfig {
            ignore_identifiers: Some(false),
            ignore_string_values: Some(false),
            ignore_numeric_values: None,
        };
        let resolved = ResolvedNormalization::resolve(DetectionMode::Semantic, &overrides);
        assert!(!resolved.ignore_identifiers);
        assert!(!resolved.ignore_string_values);
        assert!(resolved.ignore_numeric_values); // not overridden
    }

    #[test]
    fn resolve_all_overrides_on_weak() {
        let overrides = NormalizationConfig {
            ignore_identifiers: Some(true),
            ignore_string_values: Some(false), // override weak default (true -> false)
            ignore_numeric_values: Some(true),
        };
        let resolved = ResolvedNormalization::resolve(DetectionMode::Weak, &overrides);
        assert!(resolved.ignore_identifiers);
        assert!(!resolved.ignore_string_values); // overridden from true to false
        assert!(resolved.ignore_numeric_values);
    }

    // ── DuplicatesConfig deserialization ──────────────────────────────

    #[test]
    fn duplicates_config_json_all_fields() {
        let json = r#"{
            "enabled": false,
            "mode": "semantic",
            "minTokens": 100,
            "minLines": 10,
            "minOccurrences": 3,
            "threshold": 5.0,
            "ignore": ["**/vendor/**"],
            "ignoreDefaults": false,
            "skipLocal": true,
            "crossLanguage": true,
            "ignoreImports": true
        }"#;
        let config: DuplicatesConfig = serde_json::from_str(json).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.mode, DetectionMode::Semantic);
        assert_eq!(config.min_tokens, 100);
        assert_eq!(config.min_lines, 10);
        assert_eq!(config.min_occurrences, 3);
        assert!((config.threshold - 5.0).abs() < f64::EPSILON);
        assert_eq!(config.ignore, vec!["**/vendor/**"]);
        assert!(!config.ignore_defaults);
        assert!(config.skip_local);
        assert!(config.cross_language);
        assert!(config.ignore_imports);
    }

    #[test]
    fn duplicates_config_json_partial_uses_defaults() {
        let json = r#"{"mode": "weak"}"#;
        let config: DuplicatesConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled); // default
        assert_eq!(config.mode, DetectionMode::Weak);
        assert_eq!(config.min_tokens, 50); // default
        assert_eq!(config.min_lines, 5); // default
        assert!(config.ignore_defaults);
    }

    #[test]
    fn duplicates_config_json_ignore_defaults_merges_by_default() {
        let json = r#"{"ignore": ["**/foo/**"]}"#;
        let config: DuplicatesConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.ignore, vec!["**/foo/**"]);
        assert!(config.ignore_defaults);
    }

    #[test]
    fn normalization_config_json_overrides() {
        let json = r#"{
            "ignoreIdentifiers": true,
            "ignoreStringValues": false
        }"#;
        let config: NormalizationConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.ignore_identifiers, Some(true));
        assert_eq!(config.ignore_string_values, Some(false));
        assert_eq!(config.ignore_numeric_values, None);
    }

    // ── TOML deserialization ────────────────────────────────────────

    #[test]
    fn duplicates_config_toml_all_fields() {
        let toml_str = r#"
enabled = false
mode = "weak"
minTokens = 75
minLines = 8
minOccurrences = 3
threshold = 3.0
ignore = ["vendor/**"]
skipLocal = true
crossLanguage = true
ignoreImports = true

[normalization]
ignoreIdentifiers = true
ignoreStringValues = true
ignoreNumericValues = false
"#;
        let config: DuplicatesConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.mode, DetectionMode::Weak);
        assert_eq!(config.min_tokens, 75);
        assert_eq!(config.min_lines, 8);
        assert_eq!(config.min_occurrences, 3);
        assert!((config.threshold - 3.0).abs() < f64::EPSILON);
        assert_eq!(config.ignore, vec!["vendor/**"]);
        assert!(config.skip_local);
        assert!(config.cross_language);
        assert!(config.ignore_imports);
        assert_eq!(config.normalization.ignore_identifiers, Some(true));
        assert_eq!(config.normalization.ignore_string_values, Some(true));
        assert_eq!(config.normalization.ignore_numeric_values, Some(false));
    }

    #[test]
    fn duplicates_config_toml_defaults() {
        let toml_str = "";
        let config: DuplicatesConfig = toml::from_str(toml_str).unwrap();
        assert!(config.enabled);
        assert_eq!(config.mode, DetectionMode::Mild);
        assert_eq!(config.min_tokens, 50);
        assert_eq!(config.min_lines, 5);
    }

    // ── NormalizationConfig edge cases ──────────────────────────────

    #[test]
    fn normalization_config_default_all_none() {
        let config = NormalizationConfig::default();
        assert!(config.ignore_identifiers.is_none());
        assert!(config.ignore_string_values.is_none());
        assert!(config.ignore_numeric_values.is_none());
    }

    #[test]
    fn normalization_config_empty_json_object() {
        let config: NormalizationConfig = serde_json::from_str("{}").unwrap();
        assert!(config.ignore_identifiers.is_none());
        assert!(config.ignore_string_values.is_none());
        assert!(config.ignore_numeric_values.is_none());
    }

    // ── DetectionMode default ───────────────────────────────────────

    #[test]
    fn detection_mode_default_is_mild() {
        assert_eq!(DetectionMode::default(), DetectionMode::Mild);
    }

    // ── ResolvedNormalization equality ───────────────────────────────

    #[test]
    fn resolved_normalization_equality() {
        let a = ResolvedNormalization {
            ignore_identifiers: true,
            ignore_string_values: false,
            ignore_numeric_values: true,
        };
        let b = ResolvedNormalization {
            ignore_identifiers: true,
            ignore_string_values: false,
            ignore_numeric_values: true,
        };
        assert_eq!(a, b);

        let c = ResolvedNormalization {
            ignore_identifiers: false,
            ignore_string_values: false,
            ignore_numeric_values: true,
        };
        assert_ne!(a, c);
    }

    // ── Detection mode JSON deserialization ──────────────────────────

    #[test]
    fn detection_mode_json_deserialization() {
        let strict: DetectionMode = serde_json::from_str(r#""strict""#).unwrap();
        assert_eq!(strict, DetectionMode::Strict);

        let mild: DetectionMode = serde_json::from_str(r#""mild""#).unwrap();
        assert_eq!(mild, DetectionMode::Mild);

        let weak: DetectionMode = serde_json::from_str(r#""weak""#).unwrap();
        assert_eq!(weak, DetectionMode::Weak);

        let semantic: DetectionMode = serde_json::from_str(r#""semantic""#).unwrap();
        assert_eq!(semantic, DetectionMode::Semantic);
    }

    #[test]
    fn detection_mode_invalid_json() {
        let result: Result<DetectionMode, _> = serde_json::from_str(r#""aggressive""#);
        assert!(result.is_err());
    }

    // ── Serialize roundtrip ─────────────────────────────────────────

    #[test]
    fn duplicates_config_json_roundtrip() {
        let config = DuplicatesConfig {
            enabled: false,
            mode: DetectionMode::Semantic,
            min_tokens: 100,
            min_lines: 10,
            min_occurrences: 4,
            threshold: 5.5,
            ignore: vec!["test/**".to_string()],
            ignore_defaults: false,
            skip_local: true,
            cross_language: true,
            ignore_imports: true,
            normalization: NormalizationConfig {
                ignore_identifiers: Some(true),
                ignore_string_values: None,
                ignore_numeric_values: Some(false),
            },
            min_corpus_size_for_shingle_filter: 2048,
            min_corpus_size_for_token_cache: 8_000,
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: DuplicatesConfig = serde_json::from_str(&json).unwrap();
        assert!(!restored.enabled);
        assert_eq!(restored.mode, DetectionMode::Semantic);
        assert_eq!(restored.min_tokens, 100);
        assert_eq!(restored.min_lines, 10);
        assert_eq!(restored.min_occurrences, 4);
        assert!((restored.threshold - 5.5).abs() < f64::EPSILON);
        assert!(!restored.ignore_defaults);
        assert!(restored.skip_local);
        assert!(restored.cross_language);
        assert_eq!(restored.min_corpus_size_for_shingle_filter, 2048);
        assert_eq!(restored.min_corpus_size_for_token_cache, 8_000);
        assert!(restored.ignore_imports);
        assert_eq!(restored.normalization.ignore_identifiers, Some(true));
        assert!(restored.normalization.ignore_string_values.is_none());
        assert_eq!(restored.normalization.ignore_numeric_values, Some(false));
    }

    // ── NormalizationConfig skip_serializing_if ─────────────────────

    #[test]
    fn normalization_none_fields_not_serialized() {
        let config = NormalizationConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            !json.contains("ignoreIdentifiers"),
            "None fields should be skipped"
        );
        assert!(
            !json.contains("ignoreStringValues"),
            "None fields should be skipped"
        );
        assert!(
            !json.contains("ignoreNumericValues"),
            "None fields should be skipped"
        );
    }

    #[test]
    fn normalization_some_fields_serialized() {
        let config = NormalizationConfig {
            ignore_identifiers: Some(true),
            ignore_string_values: None,
            ignore_numeric_values: Some(false),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("ignoreIdentifiers"));
        assert!(!json.contains("ignoreStringValues"));
        assert!(json.contains("ignoreNumericValues"));
    }

    // ── minOccurrences validation ───────────────────────────────────

    #[test]
    fn min_occurrences_accepts_two_or_more() {
        let json = r#"{"minOccurrences": 2}"#;
        let config: DuplicatesConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.min_occurrences, 2);

        let json = r#"{"minOccurrences": 5}"#;
        let config: DuplicatesConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.min_occurrences, 5);
    }

    #[test]
    fn min_occurrences_rejects_one() {
        let json = r#"{"minOccurrences": 1}"#;
        let err = serde_json::from_str::<DuplicatesConfig>(json).unwrap_err();
        assert!(err.to_string().contains("at least 2"));
    }

    #[test]
    fn min_occurrences_rejects_zero() {
        let json = r#"{"minOccurrences": 0}"#;
        let err = serde_json::from_str::<DuplicatesConfig>(json).unwrap_err();
        assert!(err.to_string().contains("at least 2"));
    }

    #[test]
    fn min_occurrences_rejects_one_in_toml() {
        let toml_str = "minOccurrences = 1";
        let err = toml::from_str::<DuplicatesConfig>(toml_str).unwrap_err();
        assert!(err.to_string().contains("at least 2"));
    }
}
