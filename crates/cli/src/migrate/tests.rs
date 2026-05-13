use super::jscpd::migrate_jscpd;
use super::jsonc::{generate_jsonc, indent_json_value};
use super::knip::migrate_knip;
use super::toml_gen::generate_toml;
use super::{
    MigrationResult, MigrationWarning, load_json_or_jsonc, migrate_auto_detect, migrate_from_file,
    string_or_array, strip_trailing_commas,
};

fn empty_config() -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::new()
}

// -- Combined migration tests --------------------------------------------

#[test]
fn migrate_both_knip_and_jscpd() {
    let knip: serde_json::Value =
        serde_json::from_str(r#"{"entry": ["src/index.ts"], "ignore": ["dist/**"]}"#).unwrap();
    let jscpd: serde_json::Value =
        serde_json::from_str(r#"{"minTokens": 100, "skipLocal": true}"#).unwrap();
    let mut config_map = empty_config();
    let mut warnings = Vec::new();
    migrate_knip(&knip, &mut config_map, &mut warnings);
    migrate_jscpd(&jscpd, &mut config_map, &mut warnings);

    assert_eq!(
        config_map.get("entry").unwrap(),
        &serde_json::json!(["src/index.ts"])
    );
    assert_eq!(
        config_map.get("ignorePatterns").unwrap(),
        &serde_json::json!(["dist/**"])
    );
    let dupes = config_map.get("duplicates").unwrap().as_object().unwrap();
    assert_eq!(dupes.get("minTokens").unwrap(), 100);
    assert_eq!(dupes.get("skipLocal").unwrap(), true);
    assert!(warnings.is_empty());
}

// -- Output format tests -------------------------------------------------

#[test]
fn jsonc_output_has_schema() {
    let result = MigrationResult {
        config: serde_json::json!({"entry": ["src/index.ts"]}),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_jsonc(&result);
    assert!(output.contains("$schema"));
    assert!(output.contains("fallow-rs/fallow"));
}

#[test]
fn jsonc_output_has_source_comment() {
    let result = MigrationResult {
        config: serde_json::json!({"entry": ["src/index.ts"]}),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_jsonc(&result);
    assert!(output.contains("// Migrated from knip.json"));
}

#[test]
fn toml_output_has_source_comment() {
    let result = MigrationResult {
        config: serde_json::json!({"entry": ["src/index.ts"]}),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(output.contains("# Migrated from knip.json"));
}

#[test]
fn toml_output_rules_section() {
    let result = MigrationResult {
        config: serde_json::json!({
            "rules": {
                "unused-files": "error",
                "unused-exports": "warn"
            }
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(output.contains("[rules]"));
    assert!(output.contains("unused-files = \"error\""));
    assert!(output.contains("unused-exports = \"warn\""));
}

#[test]
fn toml_output_duplicates_section() {
    let result = MigrationResult {
        config: serde_json::json!({
            "duplicates": {
                "minTokens": 100,
                "skipLocal": true
            }
        }),
        warnings: vec![],
        sources: vec![".jscpd.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(output.contains("[duplicates]"));
    assert!(output.contains("minTokens = 100"));
    assert!(output.contains("skipLocal = true"));
}

// -- Deserialization roundtrip tests --------------------------------------

#[test]
fn toml_output_deserializes_as_valid_config() {
    let result = MigrationResult {
        config: serde_json::json!({
            "entry": ["src/index.ts"],
            "ignorePatterns": ["dist/**"],
            "ignoreDependencies": ["lodash"],
            "rules": {
                "unused-files": "error",
                "unused-exports": "warn"
            },
            "duplicates": {
                "minTokens": 100,
                "skipLocal": true
            }
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_toml(&result);
    let config: fallow_config::FallowConfig = toml::from_str(&output).unwrap();
    assert_eq!(config.entry, vec!["src/index.ts"]);
    assert_eq!(config.ignore_patterns, vec!["dist/**"]);
    assert_eq!(config.ignore_dependencies, vec!["lodash"]);
}

#[test]
fn jsonc_output_deserializes_as_valid_config() {
    let result = MigrationResult {
        config: serde_json::json!({
            "entry": ["src/index.ts"],
            "ignoreDependencies": ["lodash"],
            "rules": {
                "unused-files": "warn"
            }
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_jsonc(&result);
    let config: fallow_config::FallowConfig =
        jsonc_parser::parse_to_serde_value(&output, &jsonc_parser::ParseOptions::default())
            .unwrap();
    assert_eq!(config.entry, vec!["src/index.ts"]);
    assert_eq!(config.ignore_dependencies, vec!["lodash"]);
}

// -- JSONC comment stripping test ----------------------------------------

#[test]
fn jsonc_comments_stripped() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-jsonc");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("knip.jsonc");
    std::fs::write(
        &path,
        r#"{
            // Entry points
            "entry": ["src/index.ts"],
            /* Block comment */
            "ignore": ["dist/**"]
        }"#,
    )
    .unwrap();

    let value = load_json_or_jsonc(&path).unwrap();
    assert_eq!(value["entry"], serde_json::json!(["src/index.ts"]));
    assert_eq!(value["ignore"], serde_json::json!(["dist/**"]));

    let _ = std::fs::remove_dir_all(&tmpdir);
}

// -- Package.json embedded config detection ------------------------------

#[test]
fn auto_detect_package_json_knip() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-pkg-knip");
    let _ = std::fs::create_dir_all(&tmpdir);
    let pkg_path = tmpdir.join("package.json");
    std::fs::write(
        &pkg_path,
        r#"{"name": "test", "knip": {"entry": ["src/main.ts"]}}"#,
    )
    .unwrap();

    let result = migrate_auto_detect(&tmpdir).unwrap();
    assert!(!result.sources.is_empty());
    assert!(result.sources[0].contains("package.json"));

    let config_obj = result.config.as_object().unwrap();
    assert_eq!(
        config_obj.get("entry").unwrap(),
        &serde_json::json!(["src/main.ts"])
    );

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn auto_detect_package_json_jscpd() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-pkg-jscpd");
    let _ = std::fs::create_dir_all(&tmpdir);
    let pkg_path = tmpdir.join("package.json");
    std::fs::write(&pkg_path, r#"{"name": "test", "jscpd": {"minTokens": 75}}"#).unwrap();

    let result = migrate_auto_detect(&tmpdir).unwrap();
    assert!(!result.sources.is_empty());
    assert!(result.sources[0].contains("package.json"));

    let config_obj = result.config.as_object().unwrap();
    let dupes = config_obj.get("duplicates").unwrap().as_object().unwrap();
    assert_eq!(dupes.get("minTokens").unwrap(), 75);

    let _ = std::fs::remove_dir_all(&tmpdir);
}

// -- MigrationWarning Display tests --------------------------------------

#[test]
fn warning_display_without_suggestion() {
    let w = MigrationWarning {
        source: "knip",
        field: "project".to_string(),
        message: "Fallow auto-discovers project files".to_string(),
        suggestion: None,
    };
    let display = format!("{w}");
    assert_eq!(
        display,
        "[knip] `project`: Fallow auto-discovers project files"
    );
}

#[test]
fn warning_display_with_suggestion() {
    let w = MigrationWarning {
        source: "jscpd",
        field: "ignorePattern".to_string(),
        message: "Content-based ignore patterns are not supported".to_string(),
        suggestion: Some("use inline suppression".to_string()),
    };
    let display = format!("{w}");
    assert!(display.contains("[jscpd] `ignorePattern`"));
    assert!(display.contains("(suggestion: use inline suppression)"));
}

// -- string_or_array tests -----------------------------------------------

#[test]
fn string_or_array_with_string_value() {
    let val = serde_json::json!("single");
    assert_eq!(string_or_array(&val), vec!["single"]);
}

#[test]
fn string_or_array_with_array_value() {
    let val = serde_json::json!(["a", "b", "c"]);
    assert_eq!(string_or_array(&val), vec!["a", "b", "c"]);
}

#[test]
fn string_or_array_with_non_string_non_array() {
    let val = serde_json::json!(42);
    assert!(string_or_array(&val).is_empty());
}

#[test]
fn string_or_array_with_mixed_array_filters_non_strings() {
    let val = serde_json::json!(["valid", 123, "also-valid", null]);
    assert_eq!(string_or_array(&val), vec!["valid", "also-valid"]);
}

// -- load_json_or_jsonc error handling -----------------------------------

#[test]
fn load_json_or_jsonc_file_not_found() {
    let path = std::path::PathBuf::from("/nonexistent/path/to/config.json");
    let err = load_json_or_jsonc(&path).unwrap_err();
    assert!(err.contains("failed to read"));
    assert!(err.contains("/nonexistent/path/to/config.json"));
}

#[test]
fn load_json_or_jsonc_invalid_json_and_invalid_jsonc() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-invalid-json");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("bad.json");
    std::fs::write(&path, "not { valid json at all !!!").unwrap();

    let err = load_json_or_jsonc(&path).unwrap_err();
    assert!(err.contains("failed to parse"));

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn load_json_or_jsonc_rejects_malformed_leading_comma() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-jsonc-leading-comma");
    let _ = std::fs::remove_dir_all(&tmpdir);
    std::fs::create_dir_all(&tmpdir).unwrap();
    let path = tmpdir.join("knip.jsonc");
    std::fs::write(&path, "{,}").unwrap();

    let err = load_json_or_jsonc(&path).unwrap_err();
    assert!(err.contains("failed to parse"));

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn load_json_or_jsonc_accepts_trailing_commas() {
    // Reproduces issue #276: knip.jsonc with trailing commas was rejected
    // as `trailing comma at line 6 column 3`.
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-jsonc-trailing");
    let _ = std::fs::remove_dir_all(&tmpdir);
    std::fs::create_dir_all(&tmpdir).unwrap();
    let path = tmpdir.join("knip.jsonc");
    std::fs::write(
        &path,
        r#"{
  "entry": [
    "src/index.ts",
    "src/main.ts",
  ],
  "ignore": [
    "dist/**",
  ],
}"#,
    )
    .unwrap();

    let value = load_json_or_jsonc(&path).expect("trailing commas should parse");
    assert_eq!(
        value,
        serde_json::json!({
            "entry": ["src/index.ts", "src/main.ts"],
            "ignore": ["dist/**"],
        })
    );

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn load_json_or_jsonc_handles_comments_and_trailing_commas_together() {
    // knip.jsonc users routinely combine line comments with trailing
    // commas — both must be accepted in the same file.
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-jsonc-mixed");
    let _ = std::fs::remove_dir_all(&tmpdir);
    std::fs::create_dir_all(&tmpdir).unwrap();
    let path = tmpdir.join("knip.jsonc");
    std::fs::write(
        &path,
        r#"{
  // Entry points
  "entry": ["src/index.ts",],
  /* block comment */
  "ignore": ["dist/**",],
}"#,
    )
    .unwrap();

    let value = load_json_or_jsonc(&path).expect("comments + trailing commas should parse");
    assert_eq!(
        value.get("entry").unwrap(),
        &serde_json::json!(["src/index.ts"])
    );
    assert_eq!(
        value.get("ignore").unwrap(),
        &serde_json::json!(["dist/**"])
    );

    let _ = std::fs::remove_dir_all(&tmpdir);
}

// -- strip_trailing_commas helper ----------------------------------------

#[test]
fn strip_trailing_commas_drops_simple_object_comma() {
    assert_eq!(strip_trailing_commas(r#"{"a":1,}"#), r#"{"a":1}"#);
}

#[test]
fn strip_trailing_commas_drops_simple_array_comma() {
    assert_eq!(strip_trailing_commas(r"[1,2,3,]"), r"[1,2,3]");
}

#[test]
fn strip_trailing_commas_preserves_malformed_leading_comma() {
    assert_eq!(strip_trailing_commas(r"{,}"), r"{,}");
    assert_eq!(strip_trailing_commas(r"[, ]"), r"[, ]");
}

#[test]
fn strip_trailing_commas_preserves_separator_commas() {
    assert_eq!(
        strip_trailing_commas(r#"{"a":1,"b":2}"#),
        r#"{"a":1,"b":2}"#,
    );
}

#[test]
fn strip_trailing_commas_ignores_commas_inside_strings() {
    // Trailing comma inside a string literal must be preserved verbatim.
    let input = r#"{"msg":"hello, world,"}"#;
    assert_eq!(strip_trailing_commas(input), input);
}

#[test]
fn strip_trailing_commas_handles_escaped_quote_in_string() {
    // Escaped quotes inside the string must not flip in_string state, so
    // the comma inside `"hi,\","` stays put while the trailing comma
    // before `}` is stripped.
    let input = r#"{"msg":"he said \"hi,\",","n":1,}"#;
    let expected = r#"{"msg":"he said \"hi,\",","n":1}"#;
    assert_eq!(strip_trailing_commas(input), expected);
}

#[test]
fn strip_trailing_commas_handles_whitespace_before_brace() {
    // Pretty-printed input puts the comma on its own line before the
    // closing brace — still must be stripped.
    let input = "{\n  \"a\": 1,\n}";
    let expected = "{\n  \"a\": 1\n}";
    assert_eq!(strip_trailing_commas(input), expected);
}

// -- migrate_from_file tests ---------------------------------------------

#[test]
fn migrate_from_file_nonexistent_path() {
    let path = std::path::PathBuf::from("/tmp/does-not-exist-at-all.json");
    let _ = std::fs::remove_file(&path); // ensure it doesn't exist
    match migrate_from_file(&path) {
        Err(err) => assert!(err.contains("config file not found")),
        Ok(_) => panic!("expected error for nonexistent path"),
    }
}

#[test]
fn migrate_from_file_knip_ts_rejected() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-knip-ts");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("knip.ts");
    std::fs::write(&path, "export default {};").unwrap();

    match migrate_from_file(&path) {
        Err(err) => {
            assert!(err.contains("TypeScript config files"));
            assert!(err.contains("knip.ts"));
        }
        Ok(_) => panic!("expected error for .ts knip config"),
    }

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_knip_json() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-from-knip");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("knip.json");
    std::fs::write(&path, r#"{"entry": ["src/main.ts"]}"#).unwrap();

    let result = migrate_from_file(&path).unwrap();
    assert_eq!(result.sources.len(), 1);
    assert!(result.sources[0].contains("knip.json"));
    let config_obj = result.config.as_object().unwrap();
    assert_eq!(
        config_obj.get("entry").unwrap(),
        &serde_json::json!(["src/main.ts"])
    );

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_jscpd_json() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-from-jscpd");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join(".jscpd.json");
    std::fs::write(&path, r#"{"minTokens": 50, "mode": "strict"}"#).unwrap();

    let result = migrate_from_file(&path).unwrap();
    assert_eq!(result.sources.len(), 1);
    let config_obj = result.config.as_object().unwrap();
    let dupes = config_obj.get("duplicates").unwrap().as_object().unwrap();
    assert_eq!(dupes.get("minTokens").unwrap(), 50);
    assert_eq!(dupes.get("mode").unwrap(), "strict");

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_package_json_with_both_knip_and_jscpd() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-pkg-both");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("package.json");
    std::fs::write(
        &path,
        r#"{
            "name": "test",
            "knip": {"entry": ["src/app.ts"], "ignore": ["generated/**"]},
            "jscpd": {"minTokens": 80, "skipLocal": true}
        }"#,
    )
    .unwrap();

    let result = migrate_from_file(&path).unwrap();
    // Should have 2 sources: knip key + jscpd key
    assert_eq!(result.sources.len(), 2);
    assert!(result.sources[0].contains("knip"));
    assert!(result.sources[1].contains("jscpd"));

    let config_obj = result.config.as_object().unwrap();
    assert!(config_obj.contains_key("entry"));
    assert!(config_obj.contains_key("ignorePatterns"));
    assert!(config_obj.contains_key("duplicates"));

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_package_json_without_knip_or_jscpd() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-pkg-empty");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("package.json");
    std::fs::write(&path, r#"{"name": "test", "version": "1.0.0"}"#).unwrap();

    match migrate_from_file(&path) {
        Err(err) => assert!(err.contains("no knip or jscpd configuration found")),
        Ok(_) => panic!("expected error for package.json without knip/jscpd"),
    }

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_package_json_with_only_jscpd() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-pkg-jscpd-only");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("package.json");
    std::fs::write(
        &path,
        r#"{"name": "test", "jscpd": {"threshold": 5, "minTokens": 50}}"#,
    )
    .unwrap();

    let result = migrate_from_file(&path).unwrap();
    assert_eq!(result.sources.len(), 1);
    assert!(result.sources[0].contains("jscpd"));
    assert!(
        result.config.get("duplicates").is_some(),
        "should have duplicates key from jscpd migration"
    );

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_unrecognized_file_detected_as_knip() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-detect-knip");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("custom-config.json");
    // Has knip-like fields: "entry" and "ignore"
    std::fs::write(
        &path,
        r#"{"entry": ["src/index.ts"], "ignore": ["dist/**"]}"#,
    )
    .unwrap();

    let result = migrate_from_file(&path).unwrap();
    assert_eq!(result.sources.len(), 1);
    let config_obj = result.config.as_object().unwrap();
    assert!(config_obj.contains_key("entry"));

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_unrecognized_file_detected_as_jscpd() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-detect-jscpd");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("custom-dupes.json");
    // Has jscpd-like fields
    std::fs::write(&path, r#"{"minTokens": 100, "threshold": 5.0}"#).unwrap();

    let result = migrate_from_file(&path).unwrap();
    assert_eq!(result.sources.len(), 1);
    let config_obj = result.config.as_object().unwrap();
    assert!(config_obj.contains_key("duplicates"));

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_unrecognized_file_unknown_format() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-detect-unknown");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("random.json");
    // No knip-like or jscpd-like fields
    std::fs::write(&path, r#"{"foo": "bar", "baz": 123}"#).unwrap();

    match migrate_from_file(&path) {
        Err(err) => assert!(err.contains("could not determine config format")),
        Ok(_) => panic!("expected error for unrecognized config format"),
    }

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_knip_heuristic_via_rules_field() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-detect-rules");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("my-config.json");
    std::fs::write(&path, r#"{"rules": {"files": "warn"}}"#).unwrap();

    let result = migrate_from_file(&path).unwrap();
    assert_eq!(result.sources.len(), 1);
    let config_obj = result.config.as_object().unwrap();
    let rules = config_obj.get("rules").unwrap().as_object().unwrap();
    assert_eq!(rules.get("unused-files").unwrap(), "warn");

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_knip_heuristic_via_ignore_dependencies() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-detect-ignoredeps");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("my-config.json");
    std::fs::write(&path, r#"{"ignoreDependencies": ["lodash"]}"#).unwrap();

    let result = migrate_from_file(&path).unwrap();
    assert_eq!(result.sources.len(), 1);
    let config_obj = result.config.as_object().unwrap();
    assert_eq!(
        config_obj.get("ignoreDependencies").unwrap(),
        &serde_json::json!(["lodash"])
    );

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_knip_heuristic_via_ignore_exports_used_in_file() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-detect-ignoreexportsusedinfile");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("my-config.json");
    std::fs::write(&path, r#"{"ignoreExportsUsedInFile": true}"#).unwrap();

    let result = migrate_from_file(&path).unwrap();
    assert_eq!(result.sources.len(), 1);
    let config_obj = result.config.as_object().unwrap();
    assert_eq!(
        config_obj.get("ignoreExportsUsedInFile").unwrap(),
        &serde_json::json!(true)
    );

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn migrate_from_file_jscpd_heuristic_via_mode() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-detect-mode");
    let _ = std::fs::create_dir_all(&tmpdir);
    let path = tmpdir.join("duplication.json");
    std::fs::write(&path, r#"{"mode": "mild"}"#).unwrap();

    let result = migrate_from_file(&path).unwrap();
    assert_eq!(result.sources.len(), 1);
    let config_obj = result.config.as_object().unwrap();
    let dupes = config_obj.get("duplicates").unwrap().as_object().unwrap();
    assert_eq!(dupes.get("mode").unwrap(), "mild");

    let _ = std::fs::remove_dir_all(&tmpdir);
}

// -- auto_detect tests ---------------------------------------------------

#[test]
fn auto_detect_no_configs_found() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-auto-empty");
    let _ = std::fs::create_dir_all(&tmpdir);
    // No config files at all — but also no package.json
    // Remove any stale files
    for name in &[
        "knip.json",
        "knip.jsonc",
        ".knip.json",
        ".knip.jsonc",
        "knip.ts",
        "knip.config.ts",
        ".jscpd.json",
        "package.json",
    ] {
        let _ = std::fs::remove_file(tmpdir.join(name));
    }

    let result = migrate_auto_detect(&tmpdir).unwrap();
    assert!(result.sources.is_empty());

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn auto_detect_knip_ts_skipped_with_warning() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-auto-knip-ts");
    let _ = std::fs::create_dir_all(&tmpdir);
    // Remove all possible config files to isolate test
    for name in &[
        "knip.json",
        "knip.jsonc",
        ".knip.json",
        ".knip.jsonc",
        ".jscpd.json",
        "package.json",
    ] {
        let _ = std::fs::remove_file(tmpdir.join(name));
    }
    let path = tmpdir.join("knip.ts");
    std::fs::write(&path, "export default {};").unwrap();

    let result = migrate_auto_detect(&tmpdir).unwrap();
    // knip.ts is skipped (not added to sources), but warning is generated
    assert!(result.sources.is_empty());
    assert!(!result.warnings.is_empty());
    assert!(
        result.warnings[0]
            .message
            .contains("TypeScript config files")
    );

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn auto_detect_knip_json_takes_precedence_over_knip_jsonc() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-auto-precedence");
    let _ = std::fs::create_dir_all(&tmpdir);
    // Remove stale files
    for name in &[".knip.json", ".knip.jsonc", ".jscpd.json", "package.json"] {
        let _ = std::fs::remove_file(tmpdir.join(name));
    }

    std::fs::write(tmpdir.join("knip.json"), r#"{"entry": ["from-knip-json"]}"#).unwrap();
    std::fs::write(
        tmpdir.join("knip.jsonc"),
        r#"{"entry": ["from-knip-jsonc"]}"#,
    )
    .unwrap();

    let result = migrate_auto_detect(&tmpdir).unwrap();
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.sources[0], "knip.json");
    let config_obj = result.config.as_object().unwrap();
    assert_eq!(
        config_obj.get("entry").unwrap(),
        &serde_json::json!(["from-knip-json"])
    );

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn auto_detect_standalone_knip_prevents_package_json_knip() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-auto-standalone-over-pkg");
    let _ = std::fs::create_dir_all(&tmpdir);
    // Remove stale files
    for name in &[
        "knip.jsonc",
        ".knip.json",
        ".knip.jsonc",
        "knip.ts",
        "knip.config.ts",
        ".jscpd.json",
    ] {
        let _ = std::fs::remove_file(tmpdir.join(name));
    }

    std::fs::write(tmpdir.join("knip.json"), r#"{"entry": ["standalone"]}"#).unwrap();
    std::fs::write(
        tmpdir.join("package.json"),
        r#"{"name": "test", "knip": {"entry": ["from-pkg"]}}"#,
    )
    .unwrap();

    let result = migrate_auto_detect(&tmpdir).unwrap();
    // Should use standalone knip.json, not package.json knip key
    assert_eq!(result.sources.len(), 1);
    assert_eq!(result.sources[0], "knip.json");
    let config_obj = result.config.as_object().unwrap();
    assert_eq!(
        config_obj.get("entry").unwrap(),
        &serde_json::json!(["standalone"])
    );

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn auto_detect_standalone_jscpd_prevents_package_json_jscpd() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-auto-jscpd-standalone");
    let _ = std::fs::create_dir_all(&tmpdir);
    for name in &[
        "knip.json",
        "knip.jsonc",
        ".knip.json",
        ".knip.jsonc",
        "knip.ts",
        "knip.config.ts",
    ] {
        let _ = std::fs::remove_file(tmpdir.join(name));
    }

    std::fs::write(tmpdir.join(".jscpd.json"), r#"{"minTokens": 200}"#).unwrap();
    std::fs::write(
        tmpdir.join("package.json"),
        r#"{"name": "test", "jscpd": {"minTokens": 50}}"#,
    )
    .unwrap();

    let result = migrate_auto_detect(&tmpdir).unwrap();
    // Standalone .jscpd.json used; package.json jscpd key skipped
    let jscpd_source = result
        .sources
        .iter()
        .find(|s| s.contains("jscpd"))
        .expect("should have jscpd source");
    assert_eq!(jscpd_source, ".jscpd.json");
    let config_obj = result.config.as_object().unwrap();
    let dupes = config_obj.get("duplicates").unwrap().as_object().unwrap();
    assert_eq!(dupes.get("minTokens").unwrap(), 200);

    let _ = std::fs::remove_dir_all(&tmpdir);
}

#[test]
fn auto_detect_package_json_with_both_knip_and_jscpd() {
    let tmpdir = std::env::temp_dir().join("fallow-test-migrate-auto-pkg-both");
    let _ = std::fs::create_dir_all(&tmpdir);
    for name in &[
        "knip.json",
        "knip.jsonc",
        ".knip.json",
        ".knip.jsonc",
        "knip.ts",
        "knip.config.ts",
        ".jscpd.json",
    ] {
        let _ = std::fs::remove_file(tmpdir.join(name));
    }

    std::fs::write(
        tmpdir.join("package.json"),
        r#"{"name": "test", "knip": {"entry": ["src/app.ts"]}, "jscpd": {"minTokens": 60}}"#,
    )
    .unwrap();

    let result = migrate_auto_detect(&tmpdir).unwrap();
    assert_eq!(result.sources.len(), 2);
    assert!(result.sources[0].contains("knip"));
    assert!(result.sources[1].contains("jscpd"));

    let _ = std::fs::remove_dir_all(&tmpdir);
}

// -- generate_jsonc key ordering test ------------------------------------

#[test]
fn jsonc_output_keys_ordered_correctly() {
    let result = MigrationResult {
        config: serde_json::json!({
            "duplicates": {"minTokens": 50},
            "rules": {"unused-files": "warn"},
            "entry": ["src/index.ts"],
            "ignoreDependencies": ["lodash"],
            "ignoreExportsUsedInFile": true,
            "ignorePatterns": ["dist/**"]
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string(), ".jscpd.json".to_string()],
    };
    let output = generate_jsonc(&result);
    // entry should come before ignorePatterns, which should come before rules, which should come before duplicates
    let entry_pos = output.find("\"entry\"").unwrap();
    let ignore_pos = output.find("\"ignorePatterns\"").unwrap();
    let ignore_deps_pos = output.find("\"ignoreDependencies\"").unwrap();
    let ignore_exports_used_in_file_pos = output.find("\"ignoreExportsUsedInFile\"").unwrap();
    let rules_pos = output.find("\"rules\"").unwrap();
    let dupes_pos = output.find("\"duplicates\"").unwrap();
    assert!(entry_pos < ignore_pos);
    assert!(ignore_pos < ignore_deps_pos);
    assert!(ignore_deps_pos < ignore_exports_used_in_file_pos);
    assert!(ignore_exports_used_in_file_pos < rules_pos);
    assert!(rules_pos < dupes_pos);
}

#[test]
fn jsonc_output_with_multiple_sources() {
    let result = MigrationResult {
        config: serde_json::json!({"entry": ["src/index.ts"]}),
        warnings: vec![],
        sources: vec!["knip.json".to_string(), ".jscpd.json".to_string()],
    };
    let output = generate_jsonc(&result);
    assert!(output.contains("// Migrated from knip.json, .jscpd.json"));
}

#[test]
fn toml_output_with_multiple_sources() {
    let result = MigrationResult {
        config: serde_json::json!({"entry": ["src/index.ts"]}),
        warnings: vec![],
        sources: vec!["knip.json".to_string(), ".jscpd.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(output.contains("# Migrated from knip.json, .jscpd.json"));
}

// -- indent_json_value tests ---------------------------------------------

#[test]
fn indent_json_value_single_line_unchanged() {
    let result = indent_json_value("42", 4);
    assert_eq!(result, "42");
}

#[test]
fn indent_json_value_multiline_indents_continuation_lines() {
    let json = "{\n  \"a\": 1\n}";
    let result = indent_json_value(json, 2);
    assert_eq!(result, "{\n    \"a\": 1\n  }");
}

// -- generate_toml duplicates string/array values test -------------------

#[test]
fn toml_output_duplicates_string_and_array_values() {
    let result = MigrationResult {
        config: serde_json::json!({
            "duplicates": {
                "mode": "strict",
                "ignore": ["dist/**", "node_modules/**"],
                "threshold": 5.5
            }
        }),
        warnings: vec![],
        sources: vec![".jscpd.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(output.contains("[duplicates]"));
    assert!(output.contains("mode = \"strict\""));
    assert!(output.contains("ignore = [\"dist/**\", \"node_modules/**\"]"));
    assert!(output.contains("threshold = 5.5"));
}

#[test]
fn toml_output_empty_rules_omits_section() {
    let result = MigrationResult {
        config: serde_json::json!({
            "rules": {}
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(!output.contains("[rules]"));
}

#[test]
fn toml_output_empty_duplicates_omits_section() {
    let result = MigrationResult {
        config: serde_json::json!({
            "duplicates": {}
        }),
        warnings: vec![],
        sources: vec![".jscpd.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(!output.contains("[duplicates]"));
}

// -- Full TOML roundtrip with all field types ----------------------------

#[test]
fn toml_full_roundtrip_with_duplicates() {
    let result = MigrationResult {
        config: serde_json::json!({
            "entry": ["src/index.ts"],
            "ignorePatterns": ["dist/**"],
            "ignoreDependencies": ["lodash"],
            "rules": {
                "unused-files": "error",
                "unused-exports": "warn",
                "unused-types": "off"
            },
            "duplicates": {
                "minTokens": 75,
                "minLines": 5,
                "threshold": 10.0,
                "skipLocal": true,
                "mode": "mild",
                "ignore": ["**/*.test.ts"]
            }
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string(), ".jscpd.json".to_string()],
    };
    let output = generate_toml(&result);
    let config: fallow_config::FallowConfig = toml::from_str(&output).unwrap();
    assert_eq!(config.entry, vec!["src/index.ts"]);
    assert_eq!(config.ignore_patterns, vec!["dist/**"]);
    assert_eq!(config.ignore_dependencies, vec!["lodash"]);
    assert_eq!(config.duplicates.min_tokens, 75);
    assert_eq!(config.duplicates.min_lines, 5);
    assert!(config.duplicates.skip_local);
}

// -- Full JSONC roundtrip with all field types ---------------------------

#[test]
fn jsonc_full_roundtrip_with_all_fields() {
    let result = MigrationResult {
        config: serde_json::json!({
            "entry": ["src/main.ts", "src/worker.ts"],
            "ignorePatterns": ["build/**"],
            "ignoreDependencies": ["react", "lodash"],
            "rules": {
                "unused-files": "error",
                "unused-exports": "off",
                "unused-types": "warn"
            },
            "duplicates": {
                "minTokens": 120,
                "skipLocal": false
            }
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_jsonc(&result);
    let config: fallow_config::FallowConfig =
        jsonc_parser::parse_to_serde_value(&output, &jsonc_parser::ParseOptions::default())
            .unwrap();
    assert_eq!(config.entry, vec!["src/main.ts", "src/worker.ts"]);
    assert_eq!(config.ignore_patterns, vec!["build/**"]);
    assert_eq!(config.ignore_dependencies, vec!["react", "lodash"]);
}

// -- indent_json_value edge cases ----------------------------------------

#[test]
fn indent_json_value_empty_string() {
    let result = indent_json_value("", 4);
    assert_eq!(result, "");
}

#[test]
fn indent_json_value_deeply_nested_object() {
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "a": {
            "b": {
                "c": 1
            }
        }
    }))
    .unwrap();
    let result = indent_json_value(&json, 2);
    // First line stays as-is, continuation lines get 2-space indent
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines[0], "{");
    assert!(lines[1].starts_with("  ")); // continuation indented
    assert!(result.contains("\"c\": 1"));
}

#[test]
fn indent_json_value_array() {
    let json = "[\n  1,\n  2,\n  3\n]";
    let result = indent_json_value(json, 4);
    assert_eq!(result, "[\n      1,\n      2,\n      3\n    ]");
}

// -- generate_jsonc edge cases -------------------------------------------

#[test]
fn jsonc_output_empty_config() {
    let result = MigrationResult {
        config: serde_json::json!({}),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_jsonc(&result);
    assert!(output.contains("$schema"));
    assert!(output.contains("// Migrated from knip.json"));
    // No config keys should appear (only $schema and comment)
    assert!(!output.contains("\"entry\""));
    assert!(!output.contains("\"rules\""));
    assert!(!output.contains("\"ignorePatterns\""));
    assert!(!output.contains("\"duplicates\""));
    // Output should start with { and end with }
    assert!(output.starts_with('{'));
    assert!(output.trim_end().ends_with('}'));
}

#[test]
fn jsonc_output_only_rules() {
    let result = MigrationResult {
        config: serde_json::json!({
            "rules": {
                "unused-files": "error",
                "unused-exports": "off"
            }
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_jsonc(&result);
    // Should contain rules but no entry/ignorePatterns/duplicates
    assert!(output.contains("\"rules\""));
    assert!(!output.contains("\"entry\""));
    assert!(!output.contains("\"ignorePatterns\""));
    assert!(!output.contains("\"duplicates\""));
    // Should be valid JSONC that round-trips
    let parsed: serde_json::Value =
        jsonc_parser::parse_to_serde_value(&output, &jsonc_parser::ParseOptions::default())
            .unwrap();
    let rules = parsed.get("rules").unwrap().as_object().unwrap();
    assert_eq!(rules.get("unused-files").unwrap(), "error");
    assert_eq!(rules.get("unused-exports").unwrap(), "off");
}

// -- generate_toml edge cases --------------------------------------------

#[test]
fn toml_output_empty_config() {
    let result = MigrationResult {
        config: serde_json::json!({}),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(output.contains("# Migrated from knip.json"));
    // No sections should appear
    assert!(!output.contains("[rules]"));
    assert!(!output.contains("[duplicates]"));
    assert!(!output.contains("entry"));
    assert!(!output.contains("ignorePatterns"));
}

#[test]
fn toml_output_only_ignore_patterns() {
    let result = MigrationResult {
        config: serde_json::json!({
            "ignorePatterns": ["dist/**", "build/**"]
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(output.contains("ignorePatterns = [\"dist/**\", \"build/**\"]"));
    assert!(!output.contains("[rules]"));
    assert!(!output.contains("[duplicates]"));
    // Should parse as valid TOML
    let config: fallow_config::FallowConfig = toml::from_str(&output).unwrap();
    assert_eq!(config.ignore_patterns, vec!["dist/**", "build/**"]);
}

#[test]
fn toml_output_only_ignore_dependencies() {
    let result = MigrationResult {
        config: serde_json::json!({
            "ignoreDependencies": ["lodash", "react"]
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(output.contains("ignoreDependencies = [\"lodash\", \"react\"]"));
    assert!(!output.contains("[rules]"));
    assert!(!output.contains("[duplicates]"));
    let config: fallow_config::FallowConfig = toml::from_str(&output).unwrap();
    assert_eq!(config.ignore_dependencies, vec!["lodash", "react"]);
}

#[test]
fn toml_output_ignore_exports_used_in_file_bool() {
    let result = MigrationResult {
        config: serde_json::json!({
            "ignoreExportsUsedInFile": true
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(output.contains("ignoreExportsUsedInFile = true"));
    let config: fallow_config::FallowConfig = toml::from_str(&output).unwrap();
    assert!(config.ignore_exports_used_in_file.suppresses(false));
    assert!(config.ignore_exports_used_in_file.suppresses(true));
}

#[test]
fn toml_output_ignore_exports_used_in_file_kind_form() {
    let result = MigrationResult {
        config: serde_json::json!({
            "ignoreExportsUsedInFile": {"type": true, "interface": false}
        }),
        warnings: vec![],
        sources: vec!["knip.json".to_string()],
    };
    let output = generate_toml(&result);
    assert!(output.contains("ignoreExportsUsedInFile = { type = true, interface = false }"));
    let config: fallow_config::FallowConfig = toml::from_str(&output).unwrap();
    assert!(!config.ignore_exports_used_in_file.suppresses(false));
    assert!(config.ignore_exports_used_in_file.suppresses(true));
}

// -- string_or_array additional edge cases -------------------------------

#[test]
fn string_or_array_with_empty_array() {
    let val = serde_json::json!([]);
    assert!(string_or_array(&val).is_empty());
}

#[test]
fn string_or_array_with_null() {
    let val = serde_json::json!(null);
    assert!(string_or_array(&val).is_empty());
}

#[test]
fn string_or_array_with_bool() {
    let val = serde_json::json!(true);
    assert!(string_or_array(&val).is_empty());
}

#[test]
fn string_or_array_with_object() {
    let val = serde_json::json!({"key": "value"});
    assert!(string_or_array(&val).is_empty());
}
