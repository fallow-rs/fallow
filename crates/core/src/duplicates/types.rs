pub use fallow_config::{DetectionMode, DuplicatesConfig};
pub use fallow_types::duplicates::{
    CloneFamily, CloneGroup, CloneInstance, DefaultIgnoreSkipCount, DefaultIgnoreSkips,
    DuplicationReport, DuplicationStats, MirroredDirectory, RefactoringKind, RefactoringSuggestion,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = DuplicatesConfig::default();
        assert!(config.enabled);
        assert_eq!(config.mode, DetectionMode::Mild);
        assert_eq!(config.min_tokens, 50);
        assert_eq!(config.min_lines, 5);
        assert!((config.threshold - 0.0).abs() < f64::EPSILON);
        assert!(config.ignore.is_empty());
        assert!(config.ignore_defaults);
        assert!(!config.skip_local);
        assert!(!config.cross_language);
        assert!(config.normalization.ignore_identifiers.is_none());
        assert!(config.normalization.ignore_string_values.is_none());
        assert!(config.normalization.ignore_numeric_values.is_none());
    }

    #[test]
    fn detection_mode_display() {
        assert_eq!(DetectionMode::Strict.to_string(), "strict");
        assert_eq!(DetectionMode::Mild.to_string(), "mild");
        assert_eq!(DetectionMode::Weak.to_string(), "weak");
        assert_eq!(DetectionMode::Semantic.to_string(), "semantic");
    }

    #[test]
    fn detection_mode_from_str() {
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
        assert!("unknown".parse::<DetectionMode>().is_err());
    }

    #[test]
    fn detection_mode_default_is_mild() {
        assert_eq!(DetectionMode::default(), DetectionMode::Mild);
    }

    #[test]
    fn config_deserialize_toml() {
        let toml_str = r#"
enabled = true
mode = "semantic"
minTokens = 30
minLines = 3
threshold = 5.0
skipLocal = true
ignore = ["**/*.generated.ts"]
"#;
        let config: DuplicatesConfig = toml::from_str(toml_str).unwrap();
        assert!(config.enabled);
        assert_eq!(config.mode, DetectionMode::Semantic);
        assert_eq!(config.min_tokens, 30);
        assert_eq!(config.min_lines, 3);
        assert!((config.threshold - 5.0).abs() < f64::EPSILON);
        assert!(config.skip_local);
        assert_eq!(config.ignore, vec!["**/*.generated.ts"]);
    }

    #[test]
    fn config_deserialize_defaults() {
        let toml_str = "";
        let config: DuplicatesConfig = toml::from_str(toml_str).unwrap();
        assert!(config.enabled);
        assert_eq!(config.mode, DetectionMode::Mild);
        assert_eq!(config.min_tokens, 50);
        assert_eq!(config.min_lines, 5);
    }

    #[test]
    fn config_deserialize_cross_language() {
        let toml_str = r"crossLanguage = true";
        let config: DuplicatesConfig = toml::from_str(toml_str).unwrap();
        assert!(config.cross_language);
    }

    #[test]
    fn config_deserialize_normalization_overrides() {
        let toml_str = r"
[normalization]
ignoreIdentifiers = true
ignoreStringValues = false
";
        let config: DuplicatesConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.normalization.ignore_identifiers, Some(true));
        assert_eq!(config.normalization.ignore_string_values, Some(false));
        assert!(config.normalization.ignore_numeric_values.is_none());
    }

    #[test]
    fn config_deserialize_json_normalization() {
        let json_str = r#"{
            "crossLanguage": true,
            "normalization": {
                "ignoreIdentifiers": true,
                "ignoreNumericValues": true
            }
        }"#;
        let config: DuplicatesConfig = serde_json::from_str(json_str).unwrap();
        assert!(config.cross_language);
        assert_eq!(config.normalization.ignore_identifiers, Some(true));
        assert_eq!(config.normalization.ignore_numeric_values, Some(true));
        assert!(config.normalization.ignore_string_values.is_none());
    }
}
