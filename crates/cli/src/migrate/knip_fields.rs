use serde_json::{Map, Value};

use super::knip_tables::{
    KNIP_PLUGIN_KEYS, KNIP_RULE_MAP, KNIP_UNMAPPABLE_FIELDS, KNIP_UNMAPPABLE_ISSUE_TYPES,
};
use super::{MigrationWarning, string_or_array};

type JsonMap = Map<String, Value>;

/// Docs URL surfaced as a suggestion when a knip rule key is completely
/// unknown to fallow (typo, future knip rule, or an issue type the migrator
/// has not yet catalogued). Users follow it to either fix the typo or report
/// the missing mapping.
pub(super) const MIGRATION_DOCS_URL: &str = "https://docs.fallow.tools/migration/from-knip";

/// Emit a `MigrationWarning` for one rule-key-equivalent input that the
/// migrator did not translate. Used by `migrate_rules`, `migrate_exclude`, and
/// `migrate_include` so all three share the same documented-unmappable vs
/// completely-unknown ladder. The diagnostic vocabulary mirrors the table
/// name `KNIP_UNMAPPABLE_ISSUE_TYPES`: knip refers to these as issue types,
/// not rule keys, so we use the same word in both branches.
fn warn_unmapped_rule_key(context: &str, key: &str, warnings: &mut Vec<MigrationWarning>) {
    if KNIP_UNMAPPABLE_ISSUE_TYPES.contains(&key) {
        warnings.push(MigrationWarning {
            source: "knip",
            field: format!("{context}.{key}"),
            message: format!("issue type `{key}` has no fallow equivalent"),
            suggestion: None,
        });
    } else {
        warnings.push(MigrationWarning {
            source: "knip",
            field: format!("{context}.{key}"),
            message: format!("unknown knip issue type `{key}`; not migrated"),
            suggestion: Some(format!(
                "check for a typo or report the missing mapping at {MIGRATION_DOCS_URL}"
            )),
        });
    }
}

/// Migrate a string-or-array field from knip to a fallow config field.
pub(super) fn migrate_simple_field(
    obj: &JsonMap,
    src_key: &str,
    dst_key: &str,
    config: &mut JsonMap,
) {
    if let Some(val) = obj.get(src_key) {
        let entries = string_or_array(val);
        if !entries.is_empty() {
            config.insert(
                dst_key.to_string(),
                Value::Array(entries.into_iter().map(Value::String).collect()),
            );
        }
    }
}

/// Migrate knip `rules` to fallow `rules`, warning about unmappable rule names.
pub(super) fn migrate_rules(
    rules_val: &Value,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let Some(rules_obj) = rules_val.as_object() else {
        return;
    };

    let mut fallow_rules = Map::new();
    for (knip_name, fallow_name) in KNIP_RULE_MAP {
        if let Some(severity_val) = rules_obj.get(*knip_name)
            && let Some(severity_str) = severity_val.as_str()
        {
            fallow_rules.insert(
                (*fallow_name).to_string(),
                Value::String(severity_str.to_string()),
            );
        }
    }

    for key in rules_obj.keys() {
        if KNIP_RULE_MAP.iter().any(|(k, _)| k == key) {
            continue;
        }
        warn_unmapped_rule_key("rules", key, warnings);
    }

    if !fallow_rules.is_empty() {
        config.insert("rules".to_string(), Value::Object(fallow_rules));
    }
}

/// Migrate knip `exclude` — set excluded issue types to `"off"` in fallow rules.
pub(super) fn migrate_exclude(
    excluded: &[String],
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let rules = config
        .entry("rules".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(rules_obj) = rules.as_object_mut() else {
        return;
    };

    for knip_name in excluded {
        if let Some((_, fallow_name)) = KNIP_RULE_MAP.iter().find(|(k, _)| k == knip_name) {
            rules_obj.insert((*fallow_name).to_string(), Value::String("off".to_string()));
        } else {
            warn_unmapped_rule_key("exclude", knip_name, warnings);
        }
    }
}

/// Migrate knip `include` — set non-included issue types to `"off"` in fallow rules.
pub(super) fn migrate_include(
    included: &[String],
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let rules = config
        .entry("rules".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(rules_obj) = rules.as_object_mut() else {
        return;
    };

    for (knip_name, fallow_name) in KNIP_RULE_MAP {
        if !included.iter().any(|i| i == knip_name) {
            rules_obj
                .entry((*fallow_name).to_string())
                .or_insert_with(|| Value::String("off".to_string()));
        }
    }
    for name in included {
        if KNIP_RULE_MAP.iter().any(|(k, _)| k == name) {
            continue;
        }
        warn_unmapped_rule_key("include", name, warnings);
    }
}

/// Migrate knip `ignoreDependencies` — filter out regex patterns with warnings.
pub(super) fn migrate_ignore_deps(
    ignore_deps_val: &Value,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let deps = string_or_array(ignore_deps_val);
    let non_regex: Vec<String> = deps
        .into_iter()
        .filter(|d| {
            if d.starts_with('/') && d.ends_with('/') {
                warnings.push(MigrationWarning {
                    source: "knip",
                    field: "ignoreDependencies".to_string(),
                    message: format!("regex pattern `{d}` skipped (fallow uses exact strings)"),
                    suggestion: Some("add each dependency name explicitly".to_string()),
                });
                false
            } else {
                true
            }
        })
        .collect();
    if !non_regex.is_empty() {
        config.insert(
            "ignoreDependencies".to_string(),
            Value::Array(non_regex.into_iter().map(Value::String).collect()),
        );
    }
}

/// Migrate knip `ignoreExportsUsedInFile` to fallow.
pub(super) fn migrate_ignore_exports_used_in_file(
    value: &Value,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    if let Some(enabled) = value.as_bool() {
        config.insert("ignoreExportsUsedInFile".to_string(), Value::Bool(enabled));
        return;
    }

    let Some(obj) = value.as_object() else {
        warnings.push(MigrationWarning {
            source: "knip",
            field: "ignoreExportsUsedInFile".to_string(),
            message: "expected a boolean or object".to_string(),
            suggestion: Some("use true or {\"type\": true, \"interface\": true}".to_string()),
        });
        return;
    };

    let mut migrated = Map::new();
    for key in ["type", "interface"] {
        if let Some(enabled) = obj.get(key).and_then(Value::as_bool) {
            migrated.insert(key.to_string(), Value::Bool(enabled));
        }
    }

    if !migrated.is_empty() {
        config.insert(
            "ignoreExportsUsedInFile".to_string(),
            Value::Object(migrated),
        );
    }
}

/// Warn about knip fields that have no fallow equivalent.
pub(super) fn warn_unmappable_fields(obj: &JsonMap, warnings: &mut Vec<MigrationWarning>) {
    for (field, message, suggestion) in KNIP_UNMAPPABLE_FIELDS {
        if obj.contains_key(*field) {
            warnings.push(MigrationWarning {
                source: "knip",
                field: (*field).to_string(),
                message: (*message).to_string(),
                suggestion: suggestion.map(std::string::ToString::to_string),
            });
        }
    }
}

/// Warn about knip plugin-specific config keys that are auto-detected in fallow.
pub(super) fn warn_plugin_keys(obj: &JsonMap, warnings: &mut Vec<MigrationWarning>) {
    for key in obj.keys() {
        if KNIP_PLUGIN_KEYS.contains(&key.as_str()) {
            warnings.push(MigrationWarning {
                source: "knip",
                field: key.clone(),
                message: format!(
                    "plugin config `{key}` is auto-detected by fallow's built-in plugins"
                ),
                suggestion: Some(
                    "remove this section; fallow detects framework config automatically"
                        .to_string(),
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_config() -> JsonMap {
        Map::new()
    }

    #[test]
    fn simple_field_present_array() {
        let obj: JsonMap =
            serde_json::from_str(r#"{"entry": ["src/index.ts", "src/main.ts"]}"#).unwrap();
        let mut config = empty_config();
        migrate_simple_field(&obj, "entry", "entry", &mut config);

        assert_eq!(
            config.get("entry").unwrap(),
            &json!(["src/index.ts", "src/main.ts"])
        );
    }

    #[test]
    fn simple_field_present_string() {
        let obj: JsonMap = serde_json::from_str(r#"{"entry": "src/index.ts"}"#).unwrap();
        let mut config = empty_config();
        migrate_simple_field(&obj, "entry", "entry", &mut config);

        assert_eq!(config.get("entry").unwrap(), &json!(["src/index.ts"]));
    }

    #[test]
    fn simple_field_absent() {
        let obj: JsonMap = serde_json::from_str(r#"{"other": "value"}"#).unwrap();
        let mut config = empty_config();
        migrate_simple_field(&obj, "entry", "entry", &mut config);

        assert!(!config.contains_key("entry"));
    }

    #[test]
    fn simple_field_renames_key() {
        let obj: JsonMap = serde_json::from_str(r#"{"ignore": ["**/*.test.ts"]}"#).unwrap();
        let mut config = empty_config();
        migrate_simple_field(&obj, "ignore", "ignorePatterns", &mut config);

        assert!(!config.contains_key("ignore"));
        assert_eq!(
            config.get("ignorePatterns").unwrap(),
            &json!(["**/*.test.ts"])
        );
    }

    #[test]
    fn simple_field_non_string_non_array_skipped() {
        let obj: JsonMap = serde_json::from_str(r#"{"entry": 42}"#).unwrap();
        let mut config = empty_config();
        migrate_simple_field(&obj, "entry", "entry", &mut config);

        assert!(!config.contains_key("entry"));
    }

    #[test]
    fn simple_field_empty_array_skipped() {
        let obj: JsonMap = serde_json::from_str(r#"{"entry": []}"#).unwrap();
        let mut config = empty_config();
        migrate_simple_field(&obj, "entry", "entry", &mut config);

        assert!(!config.contains_key("entry"));
    }

    #[test]
    fn rules_known_mapping() {
        let rules_val = json!({"files": "error", "exports": "warn"});
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_rules(&rules_val, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "error");
        assert_eq!(rules.get("unused-exports").unwrap(), "warn");
        assert!(warnings.is_empty());
    }

    #[test]
    fn rules_unknown_unmappable_generates_warning() {
        let rules_val = json!({"binaries": "warn"});
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_rules(&rules_val, &mut config, &mut warnings);

        assert!(!config.contains_key("rules"));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "rules.binaries");
        assert!(warnings[0].message.contains("no fallow equivalent"));
    }

    #[test]
    fn rules_unknown_key_warns_with_docs_suggestion() {
        let rules_val = json!({"totallyUnknown": "error"});
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_rules(&rules_val, &mut config, &mut warnings);

        assert!(!config.contains_key("rules"));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "rules.totallyUnknown");
        assert!(warnings[0].message.contains("unknown knip issue type"));
        let suggestion = warnings[0].suggestion.as_deref().unwrap_or("");
        assert!(
            suggestion.contains("docs.fallow.tools/migration/from-knip"),
            "expected docs URL in suggestion, got: {suggestion}"
        );
    }

    #[test]
    fn rules_empty_object() {
        let rules_val = json!({});
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_rules(&rules_val, &mut config, &mut warnings);

        assert!(!config.contains_key("rules"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn rules_non_object_is_noop() {
        let rules_val = json!("not-an-object");
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_rules(&rules_val, &mut config, &mut warnings);

        assert!(!config.contains_key("rules"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn exclude_single_known_type() {
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_exclude(&["files".to_string()], &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "off");
        assert!(warnings.is_empty());
    }

    #[test]
    fn exclude_multiple_types() {
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_exclude(
            &[
                "files".to_string(),
                "types".to_string(),
                "duplicates".to_string(),
            ],
            &mut config,
            &mut warnings,
        );

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "off");
        assert_eq!(rules.get("unused-types").unwrap(), "off");
        assert_eq!(rules.get("duplicate-exports").unwrap(), "off");
        assert!(warnings.is_empty());
    }

    #[test]
    fn exclude_unmappable_type_warns() {
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_exclude(
            &["optionalPeerDependencies".to_string()],
            &mut config,
            &mut warnings,
        );

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].field.contains("optionalPeerDependencies"));
    }

    #[test]
    fn exclude_empty_slice() {
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_exclude(&[], &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert!(rules.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn include_known_types_sets_others_to_off() {
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_include(
            &["files".to_string(), "exports".to_string()],
            &mut config,
            &mut warnings,
        );

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert!(!rules.contains_key("unused-files"));
        assert!(!rules.contains_key("unused-exports"));
        assert_eq!(rules.get("unused-dependencies").unwrap(), "off");
        assert_eq!(rules.get("unused-types").unwrap(), "off");
        assert!(warnings.is_empty());
    }

    #[test]
    fn include_unmappable_type_warns() {
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_include(
            &["files".to_string(), "binaries".to_string()],
            &mut config,
            &mut warnings,
        );

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "include.binaries");
    }

    #[test]
    fn include_respects_existing_rules() {
        let mut config = empty_config();
        let mut rules = Map::new();
        rules.insert(
            "unused-dependencies".to_string(),
            Value::String("warn".to_string()),
        );
        config.insert("rules".to_string(), Value::Object(rules));

        let mut warnings = Vec::new();
        migrate_include(&["files".to_string()], &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-dependencies").unwrap(), "warn");
    }

    #[test]
    fn ignore_deps_plain_strings() {
        let val = json!(["lodash", "react"]);
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_ignore_deps(&val, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &json!(["lodash", "react"])
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn ignore_deps_regex_filtered_with_warning() {
        let val = json!(["/^@scope/", "lodash"]);
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_ignore_deps(&val, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &json!(["lodash"])
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("regex pattern"));
    }

    #[test]
    fn ignore_deps_all_regex_no_config_key() {
        let val = json!(["/^@a/", "/^@b/"]);
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_ignore_deps(&val, &mut config, &mut warnings);

        assert!(!config.contains_key("ignoreDependencies"));
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn ignore_deps_single_string() {
        let val = json!("lodash");
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_ignore_deps(&val, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &json!(["lodash"])
        );
    }

    #[test]
    fn ignore_deps_non_string_value_skipped() {
        let val = json!(42);
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_ignore_deps(&val, &mut config, &mut warnings);

        assert!(!config.contains_key("ignoreDependencies"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn warn_unmappable_fields_detects_known_fields() {
        let obj: JsonMap =
            serde_json::from_str(r#"{"project": ["src/**"], "compilers": {}}"#).unwrap();
        let mut warnings = Vec::new();
        warn_unmappable_fields(&obj, &mut warnings);

        assert_eq!(warnings.len(), 2);
        let fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
        assert!(fields.contains(&"project"));
        assert!(fields.contains(&"compilers"));
    }

    #[test]
    fn warn_unmappable_fields_empty_object_no_warnings() {
        let obj = Map::new();
        let mut warnings = Vec::new();
        warn_unmappable_fields(&obj, &mut warnings);

        assert!(warnings.is_empty());
    }

    #[test]
    fn warn_unmappable_fields_unrelated_keys_no_warnings() {
        let obj: JsonMap = serde_json::from_str(r#"{"entry": ["x"], "rules": {}}"#).unwrap();
        let mut warnings = Vec::new();
        warn_unmappable_fields(&obj, &mut warnings);

        assert!(warnings.is_empty());
    }

    #[test]
    fn warn_unmappable_fields_suggestion_presence() {
        let obj: JsonMap =
            serde_json::from_str(r#"{"ignoreFiles": ["x.ts"], "project": ["src"]}"#).unwrap();
        let mut warnings = Vec::new();
        warn_unmappable_fields(&obj, &mut warnings);

        let ignore_files_warning = warnings.iter().find(|w| w.field == "ignoreFiles").unwrap();
        assert!(ignore_files_warning.suggestion.is_some());

        let project_warning = warnings.iter().find(|w| w.field == "project").unwrap();
        assert!(project_warning.suggestion.is_none());
    }

    fn concrete_suppression_kind(suggestion: &str) -> Option<&str> {
        ["fallow-ignore-next-line ", "fallow-ignore-file "]
            .into_iter()
            .find_map(|directive| suggestion.split_once(directive).map(|(_, tail)| tail))
            .and_then(|tail| tail.split_whitespace().next())
            .filter(|kind| !kind.starts_with('['))
    }

    #[test]
    fn suppression_suggestion_guard_understands_file_directives() {
        assert_eq!(
            concrete_suppression_kind("Use // fallow-ignore-file unused-export"),
            Some("unused-export")
        );
        assert_eq!(
            concrete_suppression_kind("Use // fallow-ignore-file [issue-type]"),
            None
        );
    }

    #[test]
    fn suppression_suggestions_use_recognized_issue_kinds() {
        use fallow_types::suppress::{IssueKind, parse_suppression_target};

        for (_, _, suggestion) in KNIP_UNMAPPABLE_FIELDS {
            let Some(suggestion) = suggestion else {
                continue;
            };
            let Some(issue_kind) = concrete_suppression_kind(suggestion) else {
                continue;
            };

            assert!(
                parse_suppression_target(issue_kind).is_some(),
                "suppression suggestion uses unrecognized issue kind {issue_kind:?}: {suggestion}"
            );
        }

        let unresolved_suggestion = KNIP_UNMAPPABLE_FIELDS
            .iter()
            .find(|(field, _, _)| *field == "ignoreUnresolved")
            .and_then(|(_, _, suggestion)| *suggestion)
            .expect("ignoreUnresolved should include a suppression suggestion");
        let unresolved_kind = unresolved_suggestion
            .split_whitespace()
            .next_back()
            .and_then(parse_suppression_target)
            .and_then(|target| target.issue_kind());

        assert_eq!(unresolved_kind, Some(IssueKind::UnresolvedImport));
    }

    #[test]
    fn warn_plugin_keys_detects_plugins() {
        let obj: JsonMap =
            serde_json::from_str(r#"{"eslint": {"entry": ["a.js"]}, "jest": true}"#).unwrap();
        let mut warnings = Vec::new();
        warn_plugin_keys(&obj, &mut warnings);

        assert_eq!(warnings.len(), 2);
        let fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
        assert!(fields.contains(&"eslint"));
        assert!(fields.contains(&"jest"));
        for w in &warnings {
            assert!(w.suggestion.is_some());
        }
    }

    #[test]
    fn warn_plugin_keys_empty_object_no_warnings() {
        let obj = Map::new();
        let mut warnings = Vec::new();
        warn_plugin_keys(&obj, &mut warnings);

        assert!(warnings.is_empty());
    }

    #[test]
    fn warn_plugin_keys_non_plugin_keys_no_warnings() {
        let obj: JsonMap =
            serde_json::from_str(r#"{"entry": ["x"], "ignore": ["y"], "rules": {}}"#).unwrap();
        let mut warnings = Vec::new();
        warn_plugin_keys(&obj, &mut warnings);

        assert!(warnings.is_empty());
    }
}
