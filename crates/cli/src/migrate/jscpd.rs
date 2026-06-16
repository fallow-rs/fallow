use super::{MigrationWarning, string_or_array};

/// jscpd fields that cannot be mapped and generate warnings.
const JSCPD_UNMAPPABLE_FIELDS: &[(&str, &str, Option<&str>)] = &[
    ("maxLines", "No maximum line count limit in fallow", None),
    ("maxSize", "No maximum file size limit in fallow", None),
    (
        "ignorePattern",
        "Content-based ignore patterns are not supported",
        Some("use inline suppression: // fallow-ignore-next-line code-duplication"),
    ),
    (
        "reporters",
        "Reporters are not configurable in fallow",
        Some("use --format flag instead (human/json/sarif/compact)"),
    ),
    (
        "output",
        "fallow writes to stdout",
        Some("redirect output with shell: fallow dupes > report.json"),
    ),
    (
        "blame",
        "Git blame integration is not supported in fallow",
        None,
    ),
    ("absolute", "fallow always shows relative paths", None),
    (
        "noSymlinks",
        "Symlink handling is not configurable in fallow",
        None,
    ),
    (
        "ignoreCase",
        "Case-insensitive matching is not supported in fallow",
        None,
    ),
    ("format", "fallow auto-detects JS/TS files", None),
    (
        "formatsExts",
        "Custom file extensions are not configurable in fallow",
        None,
    ),
    ("store", "Store backend is not configurable in fallow", None),
    (
        "tokensToSkip",
        "Token skipping is not configurable in fallow",
        None,
    ),
    (
        "exitCode",
        "Exit codes are not configurable in fallow",
        Some("use the rules system to control which issues cause CI failure"),
    ),
    (
        "pattern",
        "Pattern filtering is not supported in fallow",
        None,
    ),
    (
        "path",
        "Source path configuration is not supported",
        Some("run fallow from the project root directory"),
    ),
];

pub(super) fn migrate_jscpd(
    jscpd: &serde_json::Value,
    config: &mut serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
) {
    let Some(obj) = jscpd.as_object() else {
        warnings.push(MigrationWarning {
            source: "jscpd",
            field: "(root)".to_string(),
            message: "expected an object, got something else".to_string(),
            suggestion: None,
        });
        return;
    };

    let dupes = duplicate_options_from_jscpd(obj, warnings);
    if !dupes.is_empty() {
        config.insert("duplicates".to_string(), serde_json::Value::Object(dupes));
    }

    push_unmappable_jscpd_warnings(obj, warnings);
}

fn duplicate_options_from_jscpd(
    obj: &serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut dupes = serde_json::Map::new();
    insert_jscpd_numeric_options(obj, &mut dupes);
    insert_jscpd_mode(obj, &mut dupes, warnings);
    insert_jscpd_skip_local(obj, &mut dupes);
    insert_jscpd_ignore(obj, &mut dupes);
    dupes
}

fn insert_jscpd_numeric_options(
    obj: &serde_json::Map<String, serde_json::Value>,
    dupes: &mut serde_json::Map<String, serde_json::Value>,
) {
    if let Some(min_tokens) = obj.get("minTokens").and_then(serde_json::Value::as_u64) {
        dupes.insert(
            "minTokens".to_string(),
            serde_json::Value::Number(min_tokens.into()),
        );
    }

    if let Some(min_lines) = obj.get("minLines").and_then(serde_json::Value::as_u64) {
        dupes.insert(
            "minLines".to_string(),
            serde_json::Value::Number(min_lines.into()),
        );
    }

    if let Some(threshold) = obj.get("threshold").and_then(serde_json::Value::as_f64)
        && let Some(n) = serde_json::Number::from_f64(threshold)
    {
        dupes.insert("threshold".to_string(), serde_json::Value::Number(n));
    }
}

fn insert_jscpd_mode(
    obj: &serde_json::Map<String, serde_json::Value>,
    dupes: &mut serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
) {
    if let Some(mode_str) = obj.get("mode").and_then(|v| v.as_str()) {
        let fallow_mode = match mode_str {
            "strict" => Some("strict"),
            "mild" => Some("mild"),
            "weak" => {
                warnings.push(MigrationWarning {
                    source: "jscpd",
                    field: "mode".to_string(),
                    message: "jscpd's \"weak\" mode may differ semantically from fallow's \"weak\" \
                              mode. jscpd uses lexer-based tokens while fallow uses AST-based tokens."
                        .to_string(),
                    suggestion: Some(
                        "test with both \"weak\" and \"mild\" to find the best match".to_string(),
                    ),
                });
                Some("weak")
            }
            other => {
                warnings.push(MigrationWarning {
                    source: "jscpd",
                    field: "mode".to_string(),
                    message: format!("unknown mode `{other}`, defaulting to \"mild\""),
                    suggestion: None,
                });
                None
            }
        };
        if let Some(mode) = fallow_mode {
            dupes.insert(
                "mode".to_string(),
                serde_json::Value::String(mode.to_string()),
            );
        }
    }
}

fn insert_jscpd_skip_local(
    obj: &serde_json::Map<String, serde_json::Value>,
    dupes: &mut serde_json::Map<String, serde_json::Value>,
) {
    if let Some(skip_local) = obj.get("skipLocal").and_then(serde_json::Value::as_bool) {
        dupes.insert("skipLocal".to_string(), serde_json::Value::Bool(skip_local));
    }
}

fn insert_jscpd_ignore(
    obj: &serde_json::Map<String, serde_json::Value>,
    dupes: &mut serde_json::Map<String, serde_json::Value>,
) {
    if let Some(ignore_val) = obj.get("ignore") {
        let ignores = string_or_array(ignore_val);
        if !ignores.is_empty() {
            dupes.insert(
                "ignore".to_string(),
                serde_json::Value::Array(
                    ignores.into_iter().map(serde_json::Value::String).collect(),
                ),
            );
        }
    }
}

fn push_unmappable_jscpd_warnings(
    obj: &serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
) {
    for (field, message, suggestion) in JSCPD_UNMAPPABLE_FIELDS {
        if obj.contains_key(*field) {
            warnings.push(MigrationWarning {
                source: "jscpd",
                field: (*field).to_string(),
                message: (*message).to_string(),
                suggestion: suggestion.map(std::string::ToString::to_string),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_config() -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::new()
    }

    #[test]
    fn migrate_jscpd_basic() {
        let jscpd: serde_json::Value =
            serde_json::from_str(r#"{"minTokens": 100, "minLines": 10, "threshold": 5.0}"#)
                .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("minTokens").unwrap(), 100);
        assert_eq!(dupes.get("minLines").unwrap(), 10);
        assert_eq!(dupes.get("threshold").unwrap(), 5.0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_jscpd_mode_weak_warns() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"mode": "weak"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("mode").unwrap(), "weak");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("differ semantically"));
    }

    #[test]
    fn migrate_jscpd_skip_local() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"skipLocal": true}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("skipLocal").unwrap(), true);
    }

    #[test]
    fn migrate_jscpd_ignore_patterns() {
        let jscpd: serde_json::Value =
            serde_json::from_str(r#"{"ignore": ["**/*.test.ts", "dist/**"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(
            dupes.get("ignore").unwrap(),
            &serde_json::json!(["**/*.test.ts", "dist/**"])
        );
    }

    #[test]
    fn migrate_jscpd_unmappable_fields_generate_warnings() {
        let jscpd: serde_json::Value = serde_json::from_str(
            r#"{"minTokens": 50, "maxLines": 1000, "reporters": ["console"], "blame": true}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 3);
        let fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
        assert!(fields.contains(&"maxLines"));
        assert!(fields.contains(&"reporters"));
        assert!(fields.contains(&"blame"));
    }

    #[test]
    fn migrate_jscpd_non_object_root_warns() {
        let jscpd: serde_json::Value = serde_json::json!("not an object");
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "(root)");
        assert!(warnings[0].message.contains("expected an object"));
        assert!(config.is_empty());
    }

    #[test]
    fn migrate_jscpd_mode_strict() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"mode": "strict"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("mode").unwrap(), "strict");
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_jscpd_mode_mild() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"mode": "mild"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("mode").unwrap(), "mild");
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_jscpd_mode_unknown() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"mode": "experimental"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates");
        if let Some(dupes) = dupes {
            assert!(dupes.get("mode").is_none());
        }
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("unknown mode"));
        assert!(warnings[0].message.contains("experimental"));
    }

    #[test]
    fn migrate_jscpd_skip_local_false() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"skipLocal": false}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("skipLocal").unwrap(), false);
    }

    #[test]
    fn migrate_jscpd_ignore_single_string() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"ignore": "dist/**"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(
            dupes.get("ignore").unwrap(),
            &serde_json::json!(["dist/**"])
        );
    }

    #[test]
    fn migrate_jscpd_empty_ignore_array() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"ignore": []}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        if let Some(dupes) = config.get("duplicates") {
            assert!(dupes.get("ignore").is_none());
        }
    }

    #[test]
    fn migrate_jscpd_threshold_integer() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"threshold": 10}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("threshold").unwrap(), 10.0);
    }

    #[test]
    fn migrate_jscpd_empty_object() {
        let jscpd: serde_json::Value = serde_json::from_str(r"{}").unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert!(!config.contains_key("duplicates"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_jscpd_all_unmappable_fields() {
        let jscpd: serde_json::Value = serde_json::from_str(
            r#"{
                "minTokens": 50,
                "maxLines": 1000,
                "maxSize": "100kb",
                "ignorePattern": ["foo"],
                "reporters": ["console"],
                "output": "./reports",
                "blame": true,
                "absolute": true,
                "noSymlinks": true,
                "ignoreCase": true,
                "format": ["javascript"],
                "formatsExts": {"js": ["mjs"]},
                "store": "redis",
                "tokensToSkip": ["if"],
                "exitCode": 1,
                "pattern": "*.ts",
                "path": ["src/"]
            }"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert_eq!(warnings.len(), JSCPD_UNMAPPABLE_FIELDS.len());

        let warning_fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
        for expected in [
            "maxLines",
            "maxSize",
            "ignorePattern",
            "reporters",
            "output",
            "blame",
            "absolute",
            "noSymlinks",
            "ignoreCase",
            "format",
            "formatsExts",
            "store",
            "tokensToSkip",
            "exitCode",
            "pattern",
            "path",
        ] {
            assert!(
                warning_fields.contains(&expected),
                "missing warning for `{expected}`"
            );
        }

        for w in &warnings {
            assert_eq!(w.source, "jscpd");
        }

        let by_field = |f: &str| warnings.iter().find(|w| w.field == f).unwrap();
        assert_eq!(
            by_field("maxLines").message,
            "No maximum line count limit in fallow"
        );
        assert_eq!(
            by_field("reporters").message,
            "Reporters are not configurable in fallow"
        );
    }

    #[test]
    fn migrate_jscpd_unmappable_with_suggestions() {
        let jscpd: serde_json::Value = serde_json::from_str(
            r#"{"ignorePattern": ["foo"], "reporters": ["console"], "output": "out", "exitCode": 1, "path": ["src"]}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 5);

        let by_field = |f: &str| warnings.iter().find(|w| w.field == f).unwrap();

        let w = by_field("ignorePattern");
        assert_eq!(
            w.suggestion.as_deref().unwrap(),
            "use inline suppression: // fallow-ignore-next-line code-duplication"
        );

        let w = by_field("reporters");
        assert_eq!(
            w.suggestion.as_deref().unwrap(),
            "use --format flag instead (human/json/sarif/compact)"
        );

        let w = by_field("output");
        assert_eq!(
            w.suggestion.as_deref().unwrap(),
            "redirect output with shell: fallow dupes > report.json"
        );

        let w = by_field("exitCode");
        assert_eq!(
            w.suggestion.as_deref().unwrap(),
            "use the rules system to control which issues cause CI failure"
        );

        let w = by_field("path");
        assert_eq!(
            w.suggestion.as_deref().unwrap(),
            "run fallow from the project root directory"
        );
    }

    #[test]
    fn migrate_jscpd_unmappable_without_suggestions() {
        let jscpd: serde_json::Value = serde_json::from_str(
            r#"{"maxLines": 1000, "maxSize": "50kb", "blame": true, "absolute": true, "noSymlinks": false, "ignoreCase": true, "format": ["js"], "formatsExts": {}, "store": "redis", "tokensToSkip": ["x"], "pattern": "*.ts"}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let expected_count = JSCPD_UNMAPPABLE_FIELDS
            .iter()
            .filter(|f| f.2.is_none())
            .count();
        assert_eq!(warnings.len(), expected_count);

        let by_field = |f: &str| warnings.iter().find(|w| w.field == f).unwrap();

        assert!(by_field("maxLines").suggestion.is_none());
        assert_eq!(
            by_field("maxLines").message,
            "No maximum line count limit in fallow"
        );

        assert!(by_field("maxSize").suggestion.is_none());
        assert_eq!(
            by_field("maxSize").message,
            "No maximum file size limit in fallow"
        );

        assert!(by_field("blame").suggestion.is_none());
        assert_eq!(
            by_field("blame").message,
            "Git blame integration is not supported in fallow"
        );

        assert!(by_field("absolute").suggestion.is_none());
        assert!(by_field("noSymlinks").suggestion.is_none());
        assert!(by_field("ignoreCase").suggestion.is_none());
        assert!(by_field("format").suggestion.is_none());
        assert!(by_field("formatsExts").suggestion.is_none());
        assert!(by_field("store").suggestion.is_none());
        assert!(by_field("tokensToSkip").suggestion.is_none());
        assert!(by_field("pattern").suggestion.is_none());
    }

    #[test]
    fn migrate_jscpd_complex_full_config() {
        let jscpd: serde_json::Value = serde_json::from_str(
            r#"{
                "minTokens": 75,
                "minLines": 8,
                "threshold": 3.5,
                "mode": "weak",
                "skipLocal": true,
                "ignore": ["**/vendor/**", "dist/**"],
                "maxLines": 5000,
                "reporters": ["json"],
                "blame": false
            }"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("minTokens").unwrap(), 75);
        assert_eq!(dupes.get("minLines").unwrap(), 8);
        assert_eq!(dupes.get("threshold").unwrap(), 3.5);
        assert_eq!(dupes.get("mode").unwrap(), "weak");
        assert_eq!(dupes.get("skipLocal").unwrap(), true);
        assert_eq!(
            dupes.get("ignore").unwrap(),
            &serde_json::json!(["**/vendor/**", "dist/**"])
        );

        assert_eq!(warnings.len(), 4);
        let warning_fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
        assert!(warning_fields.contains(&"mode"));
        assert!(warning_fields.contains(&"maxLines"));
        assert!(warning_fields.contains(&"reporters"));
        assert!(warning_fields.contains(&"blame"));
    }

    #[test]
    fn migrate_jscpd_non_numeric_min_tokens_ignored() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"minTokens": "fifty"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert!(!config.contains_key("duplicates"));
    }

    #[test]
    fn migrate_jscpd_non_numeric_min_lines_ignored() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"minLines": "ten"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert!(!config.contains_key("duplicates"));
    }

    #[test]
    fn migrate_jscpd_mode_non_string_ignored() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"mode": 42}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert!(!config.contains_key("duplicates"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_jscpd_threshold_non_numeric_ignored() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"threshold": "high"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert!(!config.contains_key("duplicates"));
    }

    #[test]
    fn migrate_jscpd_skip_local_non_bool_ignored() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"skipLocal": "yes"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert!(!config.contains_key("duplicates"));
    }

    #[test]
    fn migrate_jscpd_min_tokens_float_ignored() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"minTokens": 50.5}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert!(!config.contains_key("duplicates"));
    }

    #[test]
    fn migrate_jscpd_threshold_zero() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"threshold": 0}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("threshold").unwrap(), 0.0);
    }

    #[test]
    fn migrate_jscpd_ignore_mixed_types() {
        let jscpd: serde_json::Value =
            serde_json::from_str(r#"{"ignore": ["dist/**", 42, true]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(
            dupes.get("ignore").unwrap(),
            &serde_json::json!(["dist/**"])
        );
    }

    #[test]
    fn migrate_jscpd_ignore_non_value() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"ignore": 42}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert!(!config.contains_key("duplicates"));
    }

    #[test]
    fn migrate_jscpd_all_warnings_have_jscpd_source() {
        let jscpd: serde_json::Value =
            serde_json::from_str(r#"{"mode": "unknown_mode", "maxLines": 100, "blame": true}"#)
                .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert!(!warnings.is_empty());
        for w in &warnings {
            assert_eq!(
                w.source, "jscpd",
                "warning for `{}` should have source \"jscpd\"",
                w.field
            );
        }
    }

    #[test]
    fn migrate_jscpd_only_unmappable_fields_no_duplicates_key() {
        let jscpd: serde_json::Value =
            serde_json::from_str(r#"{"maxLines": 1000, "blame": true, "reporters": ["json"]}"#)
                .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert!(!config.contains_key("duplicates"));
        assert_eq!(warnings.len(), 3);
    }

    #[test]
    fn migrate_jscpd_min_lines_zero() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"minLines": 0}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("minLines").unwrap(), 0);
    }

    #[test]
    fn migrate_jscpd_large_min_tokens() {
        let jscpd: serde_json::Value = serde_json::from_str(r#"{"minTokens": 999999}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        let dupes = config.get("duplicates").unwrap().as_object().unwrap();
        assert_eq!(dupes.get("minTokens").unwrap(), 999_999);
    }

    #[test]
    fn migrate_jscpd_null_root() {
        let jscpd: serde_json::Value = serde_json::json!(null);
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert!(config.is_empty());
    }

    #[test]
    fn migrate_jscpd_array_root() {
        let jscpd: serde_json::Value = serde_json::json!([1, 2, 3]);
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_jscpd(&jscpd, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "(root)");
        assert!(config.is_empty());
    }
}
