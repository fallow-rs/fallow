use serde_json::{Map, Value};

use super::knip::migrate_knip;
use super::knip_fields::error_rules_with_warn_default;

/// The fallow rules that a knip issue type maps to and that default to `warn`.
const WARN_DEFAULT_RULES: [&str; 4] = [
    "unused-dev-dependencies",
    "unused-namespace-members",
    "unlisted-binaries",
    "duplicate-export-aliases",
];

fn migrated_rules(knip: &str) -> Map<String, Value> {
    let knip: Value = serde_json::from_str(knip).unwrap();
    let mut config = Map::new();
    let mut warnings = Vec::new();
    migrate_knip(&knip, &mut config, &mut warnings);
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    config
        .get("rules")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

#[test]
fn include_dependencies_keeps_dev_dependencies_on() {
    let rules = migrated_rules(r#"{"include": ["files", "dependencies"]}"#);

    assert_eq!(rules.get("unused-dev-dependencies").unwrap(), "error");
    assert!(!rules.contains_key("unused-dependencies"));
    assert_eq!(rules.get("unused-exports").unwrap(), "off");
}

#[test]
fn include_without_dependencies_turns_dev_dependencies_off() {
    let rules = migrated_rules(r#"{"include": ["files"]}"#);

    assert_eq!(rules.get("unused-dev-dependencies").unwrap(), "off");
    assert_eq!(rules.get("unused-dependencies").unwrap(), "off");
}

#[test]
fn exclude_dependencies_turns_dev_dependencies_off() {
    let rules = migrated_rules(r#"{"exclude": ["dependencies"]}"#);

    assert_eq!(rules.get("unused-dependencies").unwrap(), "off");
    assert_eq!(rules.get("unused-dev-dependencies").unwrap(), "off");
}

#[test]
fn config_without_rules_keeps_error_severity() {
    let rules = migrated_rules(r#"{"entry": ["index.ts"]}"#);

    for rule in WARN_DEFAULT_RULES {
        assert_eq!(rules.get(rule).unwrap(), "error", "{rule}");
    }
    assert_eq!(rules.len(), WARN_DEFAULT_RULES.len(), "{rules:?}");
}

#[test]
fn explicit_severity_of_the_source_config_wins() {
    let rules = migrated_rules(
        r#"{"rules": {"devDependencies": "warn", "binaries": "off", "files": "warn"}}"#,
    );

    assert_eq!(rules.get("unused-dev-dependencies").unwrap(), "warn");
    assert_eq!(rules.get("unlisted-binaries").unwrap(), "off");
    assert_eq!(rules.get("unused-files").unwrap(), "warn");
    assert_eq!(rules.get("unused-namespace-members").unwrap(), "error");
    assert_eq!(rules.get("duplicate-export-aliases").unwrap(), "error");
}

#[test]
fn excluded_issue_types_stay_off() {
    let rules = migrated_rules(r#"{"exclude": ["devDependencies", "duplicates"]}"#);

    assert_eq!(rules.get("unused-dev-dependencies").unwrap(), "off");
    assert_eq!(rules.get("duplicate-export-aliases").unwrap(), "off");
    assert_eq!(rules.get("unlisted-binaries").unwrap(), "error");
}

#[test]
fn non_object_config_gets_no_rules() {
    let mut config = Map::new();
    let mut warnings = Vec::new();
    migrate_knip(&Value::from(42), &mut config, &mut warnings);

    assert!(!config.contains_key("rules"));
}

#[test]
fn note_lists_the_error_rules_that_default_to_warn() {
    let config = serde_json::json!({"rules": {
        "unused-files": "error",
        "unused-dev-dependencies": "error",
        "unlisted-binaries": "warn",
        "duplicate-export-aliases": "error",
    }});

    assert_eq!(
        error_rules_with_warn_default(&config),
        ["unused-dev-dependencies", "duplicate-export-aliases"]
    );
    assert!(error_rules_with_warn_default(&serde_json::json!({})).is_empty());
}

#[test]
fn migrated_rules_parse_as_fallow_config() {
    let rules = migrated_rules(r#"{"entry": ["index.ts"]}"#);
    let parsed: fallow_config::RulesConfig = serde_json::from_value(Value::Object(rules)).unwrap();

    assert_eq!(
        parsed.unused_dev_dependencies,
        fallow_config::Severity::Error
    );
    assert_eq!(
        parsed.unused_optional_dependencies,
        fallow_config::Severity::Warn
    );
}
