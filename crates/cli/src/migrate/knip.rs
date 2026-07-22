use serde_json::Value;

use super::knip_fields::{
    migrate_exclude, migrate_ignore_deps, migrate_ignore_exports_used_in_file, migrate_include,
    migrate_rules, migrate_simple_field, warn_plugin_keys, warn_unmappable_fields,
};
#[cfg(test)]
use super::knip_tables::KNIP_RULE_MAP;
use super::{MigrationWarning, string_or_array};

type JsonMap = serde_json::Map<String, Value>;

pub(super) fn migrate_knip(
    knip: &Value,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let Some(obj) = knip.as_object() else {
        warnings.push(MigrationWarning {
            source: "knip",
            field: "(root)".to_string(),
            message: "expected an object, got something else".to_string(),
            suggestion: None,
        });
        return;
    };

    migrate_simple_field(obj, "entry", "entry", config);

    migrate_simple_field(obj, "ignore", "ignorePatterns", config);

    if let Some(ignore_deps_val) = obj.get("ignoreDependencies") {
        migrate_ignore_deps(ignore_deps_val, config, warnings);
    }

    if let Some(value) = obj.get("ignoreExportsUsedInFile") {
        migrate_ignore_exports_used_in_file(value, config, warnings);
    }

    if let Some(rules_val) = obj.get("rules") {
        migrate_rules(rules_val, config, warnings);
    }

    if let Some(exclude_val) = obj.get("exclude") {
        let excluded = string_or_array(exclude_val);
        if !excluded.is_empty() {
            migrate_exclude(&excluded, config, warnings);
        }
    }

    if let Some(include_val) = obj.get("include") {
        let included = string_or_array(include_val);
        if !included.is_empty() {
            migrate_include(&included, config, warnings);
        }
    }

    warn_unmappable_fields(obj, warnings);

    warn_plugin_keys(obj, warnings);

    if let Some(workspaces_val) = obj.get("workspaces")
        && workspaces_val.is_object()
    {
        warnings.push(MigrationWarning {
            source: "knip",
            field: "workspaces".to_string(),
            message: "per-workspace plugin overrides have limited support in fallow".to_string(),
            suggestion: Some(
                "fallow auto-discovers workspace packages; use --workspace flag to scope output"
                    .to_string(),
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_config() -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::new()
    }

    #[test]
    fn migrate_minimal_knip_json() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"entry": ["src/index.ts"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!(["src/index.ts"])
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_with_rules() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{"rules": {"files": "warn", "exports": "off", "dependencies": "error"}}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "warn");
        assert_eq!(rules.get("unused-exports").unwrap(), "off");
        assert_eq!(rules.get("unused-dependencies").unwrap(), "error");
    }

    #[test]
    fn migrate_knip_with_exclude() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"exclude": ["files", "types"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "off");
        assert_eq!(rules.get("unused-types").unwrap(), "off");
    }

    #[test]
    fn migrate_knip_with_include() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"include": ["files", "exports"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert!(
            !rules.contains_key("unused-files"),
            "included type 'unused-files' should not be in rules"
        );
        assert!(
            !rules.contains_key("unused-exports"),
            "included type 'unused-exports' should not be in rules"
        );
        assert_eq!(rules.get("unused-dependencies").unwrap(), "off");
        assert_eq!(rules.get("unused-dev-dependencies").unwrap(), "off");
        assert_eq!(rules.get("unused-types").unwrap(), "off");
        assert_eq!(rules.get("unused-enum-members").unwrap(), "off");
        assert_eq!(rules.get("unused-class-members").unwrap(), "off");
        assert_eq!(rules.get("unlisted-dependencies").unwrap(), "off");
        assert_eq!(rules.get("unresolved-imports").unwrap(), "off");
        assert_eq!(rules.get("duplicate-exports").unwrap(), "off");
    }

    #[test]
    fn migrate_knip_with_ignore_patterns() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignore": ["src/generated/**", "**/*.test.ts"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignorePatterns").unwrap(),
            &serde_json::json!(["src/generated/**", "**/*.test.ts"])
        );
    }

    #[test]
    fn migrate_knip_with_ignore_dependencies() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignoreDependencies": ["@org/lib", "lodash"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &serde_json::json!(["@org/lib", "lodash"])
        );
    }

    #[test]
    fn migrate_knip_ignore_exports_used_in_file_bool() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignoreExportsUsedInFile": true}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreExportsUsedInFile").unwrap(),
            &serde_json::json!(true)
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_ignore_exports_used_in_file_kind_form() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{"ignoreExportsUsedInFile": {"type": true, "interface": true}}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreExportsUsedInFile").unwrap(),
            &serde_json::json!({"type": true, "interface": true})
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_regex_ignore_deps_skipped() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignoreDependencies": ["/^@org/", "lodash"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &serde_json::json!(["lodash"])
        );
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "ignoreDependencies");
    }

    #[test]
    fn migrate_knip_unmappable_fields_generate_warnings() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"project": ["src/**"], "paths": {"@/*": ["src/*"]}}"#)
                .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 2);
        let fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
        assert!(fields.contains(&"project"));
        assert!(fields.contains(&"paths"));
    }

    #[test]
    fn migrate_knip_plugin_keys_generate_warnings() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{"entry": ["src/index.ts"], "eslint": {"entry": ["eslint.config.js"]}}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "eslint");
        assert!(warnings[0].message.contains("auto-detected"));
    }

    #[test]
    fn migrate_knip_entry_string() {
        let knip: serde_json::Value = serde_json::from_str(r#"{"entry": "src/index.ts"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!(["src/index.ts"])
        );
    }

    #[test]
    fn migrate_knip_exclude_unmappable_warns() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"exclude": ["optionalPeerDependencies"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].field.contains("optionalPeerDependencies"));
    }

    #[test]
    fn migrate_knip_rules_unmappable_warns() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"rules": {"binaries": "warn", "files": "error"}}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "error");
        assert!(!rules.contains_key("binaries"));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].field.contains("binaries"));
    }

    #[test]
    fn migrate_knip_non_object_root_warns() {
        let knip: serde_json::Value = serde_json::json!("not an object");
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "(root)");
        assert!(warnings[0].message.contains("expected an object"));
        assert!(config.is_empty());
    }

    #[test]
    fn migrate_knip_workspaces_object_warns() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"workspaces": {"packages/*": {"entry": ["src/index.ts"]}}}"#)
                .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "workspaces");
        assert!(
            warnings[0]
                .message
                .contains("per-workspace plugin overrides")
        );
        assert!(warnings[0].suggestion.is_some());
    }

    #[test]
    fn migrate_knip_workspaces_non_object_no_warning() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"workspaces": ["packages/*"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!warnings.iter().any(|w| w.field == "workspaces"));
    }

    #[test]
    fn migrate_knip_all_regex_ignore_deps_no_output() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignoreDependencies": ["/^@org/", "/^lodash/"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("ignoreDependencies"));
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn migrate_knip_ignore_deps_single_string() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignoreDependencies": "lodash"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &serde_json::json!(["lodash"])
        );
    }

    #[test]
    fn migrate_knip_rules_non_string_severity_ignored() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"rules": {"files": 123, "exports": "warn"}}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert!(!rules.contains_key("unused-files"));
        assert_eq!(rules.get("unused-exports").unwrap(), "warn");
    }

    #[test]
    fn migrate_knip_rules_non_object_ignored() {
        let knip: serde_json::Value = serde_json::from_str(r#"{"rules": "invalid"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("rules"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_include_unmappable_warns() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"include": ["files", "binaries"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let include_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.field.starts_with("include."))
            .collect();
        assert_eq!(include_warnings.len(), 1);
        assert!(include_warnings[0].field.contains("binaries"));
    }

    #[test]
    fn migrate_knip_rules_then_include_rules_take_precedence() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{"rules": {"dependencies": "warn"}, "include": ["files", "dependencies"]}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-dependencies").unwrap(), "warn");
        assert_eq!(rules.get("unused-exports").unwrap(), "off");
        assert!(
            !rules.contains_key("unused-files"),
            "included type 'unused-files' should not be in rules"
        );
    }

    #[test]
    fn migrate_knip_multiple_unmappable_fields_with_suggestions() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{"ignoreFiles": ["x.ts"], "ignoreMembers": ["id"], "ignoreUnresolved": ["y"]}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 3);
        for w in &warnings {
            assert!(
                w.suggestion.is_some(),
                "warning for `{}` should have a suggestion",
                w.field
            );
        }
    }

    #[test]
    fn migrate_knip_multiple_plugin_keys_warn() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{"eslint": {"entry": ["a.js"]}, "jest": {"entry": ["b.js"]}, "vitest": true}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.message.contains("auto-detected"))
                .count(),
            3
        );
    }

    #[test]
    fn migrate_knip_all_rule_mappings() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{"rules": {
                "files": "error",
                "dependencies": "warn",
                "devDependencies": "off",
                "exports": "error",
                "types": "warn",
                "enumMembers": "error",
                "classMembers": "warn",
                "unlisted": "error",
                "unresolved": "warn",
                "duplicates": "off"
            }}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "error");
        assert_eq!(rules.get("unused-dependencies").unwrap(), "warn");
        assert_eq!(rules.get("unused-dev-dependencies").unwrap(), "off");
        assert_eq!(rules.get("unused-exports").unwrap(), "error");
        assert_eq!(rules.get("unused-types").unwrap(), "warn");
        assert_eq!(rules.get("unused-enum-members").unwrap(), "error");
        assert_eq!(rules.get("unused-class-members").unwrap(), "warn");
        assert_eq!(rules.get("unlisted-dependencies").unwrap(), "error");
        assert_eq!(rules.get("unresolved-imports").unwrap(), "warn");
        assert_eq!(rules.get("duplicate-exports").unwrap(), "off");
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_exclude_all_mappable_types() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{"exclude": ["files", "dependencies", "devDependencies", "exports",
                "types", "enumMembers", "classMembers", "unlisted", "unresolved", "duplicates"]}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        for (_, fallow_name) in KNIP_RULE_MAP {
            assert_eq!(
                rules.get(*fallow_name).unwrap(),
                "off",
                "{fallow_name} should be off"
            );
        }
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_empty_entry_array() {
        let knip: serde_json::Value = serde_json::from_str(r#"{"entry": []}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("entry"));
    }

    #[test]
    fn migrate_knip_empty_ignore_array() {
        let knip: serde_json::Value = serde_json::from_str(r#"{"ignore": []}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("ignorePatterns"));
    }

    #[test]
    fn migrate_knip_unmappable_without_suggestion() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{"ignoreBinaries": ["tsc"], "treatConfigHintsAsErrors": true}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 2);
        assert_eq!(
            warnings.iter().filter(|w| w.suggestion.is_none()).count(),
            2
        );
    }

    #[test]
    fn migrate_knip_rules_unknown_key_warns() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"rules": {"completelyUnknownRule": "warn"}}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("rules"));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "rules.completelyUnknownRule");
        assert!(warnings[0].message.contains("unknown knip issue type"));
        assert!(
            warnings[0]
                .suggestion
                .as_deref()
                .unwrap_or("")
                .contains("docs.fallow.tools/migration/from-knip")
        );
    }

    #[test]
    fn migrate_knip_exclude_unknown_key_warns() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"exclude": ["totallyMadeUp"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "exclude.totallyMadeUp");
        assert!(warnings[0].message.contains("unknown knip issue type"));
        assert!(warnings[0].suggestion.is_some());
    }

    #[test]
    fn migrate_knip_include_unknown_key_warns() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"include": ["files", "madeUp"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let unknown_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.message.contains("unknown knip issue type"))
            .collect();
        assert_eq!(unknown_warnings.len(), 1);
        assert_eq!(unknown_warnings[0].field, "include.madeUp");
    }

    #[test]
    fn migrate_knip_documented_unmappable_keeps_existing_message() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"rules": {"binaries": "warn"}}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "rules.binaries");
        assert!(warnings[0].message.contains("no fallow equivalent"));
        assert!(warnings[0].suggestion.is_none());
    }

    #[test]
    fn migrate_knip_complex_full_config() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{
                "entry": ["src/index.ts", "src/worker.ts"],
                "ignore": ["**/*.generated.*"],
                "ignoreDependencies": ["/^@internal/", "lodash", "react"],
                "rules": {"files": "warn", "exports": "error"},
                "exclude": ["types"],
                "project": ["src/**"],
                "eslint": {"entry": ["eslint.config.js"]},
                "workspaces": {"packages/*": {"entry": ["index.ts"]}}
            }"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!(["src/index.ts", "src/worker.ts"])
        );
        assert_eq!(
            config.get("ignorePatterns").unwrap(),
            &serde_json::json!(["**/*.generated.*"])
        );
        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &serde_json::json!(["lodash", "react"])
        );

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "warn");
        assert_eq!(rules.get("unused-exports").unwrap(), "error");
        assert_eq!(rules.get("unused-types").unwrap(), "off");

        let warning_fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
        assert!(warning_fields.contains(&"ignoreDependencies"));
        assert!(warning_fields.contains(&"project"));
        assert!(warning_fields.contains(&"eslint"));
        assert!(warning_fields.contains(&"workspaces"));
    }

    #[test]
    fn migrate_knip_empty_object_no_config() {
        let knip: serde_json::Value = serde_json::from_str(r"{}").unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(config.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_exclude_overrides_rules_for_same_type() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"rules": {"files": "error"}, "exclude": ["files"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "off");
    }

    #[test]
    fn migrate_knip_ignore_deps_non_value_ignored() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignoreDependencies": 42}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("ignoreDependencies"));
    }

    #[test]
    fn migrate_knip_ignore_single_string() {
        let knip: serde_json::Value = serde_json::from_str(r#"{"ignore": "dist/**"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignorePatterns").unwrap(),
            &serde_json::json!(["dist/**"])
        );
    }

    #[test]
    fn migrate_knip_all_warnings_have_knip_source() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{
                "project": ["src/**"],
                "ignoreDependencies": ["/^@scope/"],
                "rules": {"binaries": "warn"},
                "exclude": ["optionalPeerDependencies"],
                "eslint": {},
                "workspaces": {"pkg": {}}
            }"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!warnings.is_empty());
        for w in &warnings {
            assert_eq!(
                w.source, "knip",
                "warning for `{}` should have source \"knip\"",
                w.field
            );
        }
    }

    #[test]
    fn migrate_knip_empty_include_array() {
        let knip: serde_json::Value = serde_json::from_str(r#"{"include": []}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("rules"));
    }

    #[test]
    fn migrate_knip_empty_exclude_array() {
        let knip: serde_json::Value = serde_json::from_str(r#"{"exclude": []}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("rules"));
    }

    #[test]
    fn migrate_knip_ignore_deps_mixed_types_in_array() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignoreDependencies": ["lodash", 42, true, "react"]}"#)
                .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &serde_json::json!(["lodash", "react"])
        );
    }

    #[test]
    fn migrate_knip_exclude_single_string() {
        let knip: serde_json::Value = serde_json::from_str(r#"{"exclude": "files"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "off");
    }

    #[test]
    fn migrate_knip_include_single_string() {
        let knip: serde_json::Value = serde_json::from_str(r#"{"include": "files"}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert!(!rules.contains_key("unused-files"));
        assert_eq!(rules.get("unused-exports").unwrap(), "off");
        assert_eq!(rules.get("unused-dependencies").unwrap(), "off");
    }
}
