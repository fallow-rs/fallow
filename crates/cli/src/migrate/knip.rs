use serde_json::Value;

use super::knip_fields::{
    migrate_exclude, migrate_ignore, migrate_ignore_binaries, migrate_ignore_deps,
    migrate_ignore_exports_used_in_file, migrate_include, migrate_rules, warn_unhandled_root_keys,
    warn_unmappable_fields,
};
use super::knip_scoped::{
    KnipScope, WorkspaceMigration, config_marks_production_entry, migrate_entry,
    migrate_ignore_issues, migrate_workspaces,
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

    let root = KnipScope::root();
    let production_split = config_marks_production_entry(obj);
    if let Some(value) = obj.get("entry") {
        migrate_entry(value, &root, production_split, config, warnings);
    }

    if let Some(value) = obj.get("ignore") {
        migrate_ignore(value, &root, config, warnings);
    }

    if let Some(ignore_deps_val) = obj.get("ignoreDependencies") {
        migrate_ignore_deps(ignore_deps_val, "ignoreDependencies", config, warnings);
    }

    if let Some(value) = obj.get("ignoreBinaries") {
        migrate_ignore_binaries(value, "ignoreBinaries", config, warnings);
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

    // `ignoreIssues` reads the migrated `rules`, so it runs after them.
    if let Some(value) = obj.get("ignoreIssues") {
        migrate_ignore_issues(value, &root, config, warnings);
    }

    let include_entry_exports = migrate_include_entry_exports(obj, config, warnings);

    warn_unmappable_fields(obj, warnings);

    warn_unhandled_root_keys(obj, warnings);

    if let Some(workspaces) = obj.get("workspaces") {
        let migration = WorkspaceMigration {
            root_include_entry_exports: include_entry_exports,
            production_split,
        };
        migrate_workspaces(workspaces, &migration, config, warnings);
    }
}

/// Migrate the root `includeEntryExports` flag and return its effective value.
fn migrate_include_entry_exports(
    obj: &JsonMap,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) -> bool {
    let Some(value) = obj.get("includeEntryExports") else {
        return false;
    };
    match value.as_bool() {
        Some(true) => {
            config.insert("includeEntryExports".to_string(), Value::Bool(true));
            true
        }
        Some(false) => false,
        None => {
            warnings.push(MigrationWarning {
                source: "knip",
                field: "includeEntryExports".to_string(),
                message: "expected a boolean; not migrated".to_string(),
                suggestion: None,
            });
            false
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
    fn migrate_knip_ignore_preserves_positive_and_negated_patterns() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignore": ["src/generated/**", "!src/generated/keep.ts"]}"#)
                .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreFindings").unwrap(),
            &serde_json::json!(["src/generated/**", "!src/generated/keep.ts"])
        );
        assert!(!config.contains_key("ignorePatterns"));
        assert!(warnings.is_empty());
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
    fn migrate_knip_regex_ignore_deps_converted() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignoreDependencies": ["/^@org/", "@acme/.+", "lodash"]}"#)
                .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &serde_json::json!(["@org*", "@acme/*", "lodash"])
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_regex_ignore_deps_skipped() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignoreDependencies": ["^(react|vue)$", "lodash"]}"#).unwrap();
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
            serde_json::from_str(r#"{"rules": {"catalog": "warn", "files": "error"}}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "error");
        assert!(!rules.contains_key("catalog"));

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].field.contains("catalog"));
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

    fn warning_fields(warnings: &[MigrationWarning]) -> Vec<&str> {
        warnings
            .iter()
            .map(|warning| warning.field.as_str())
            .collect()
    }

    #[test]
    fn migrate_knip_workspace_entry_is_rebased_on_the_workspace_path() {
        let knip = serde_json::json!({
            "entry": ["src/root.ts"],
            "workspaces": {
                ".": {"entry": [".github/scripts/report.cjs"]},
                "apps/server": {"entry": ["src/bin.ts", "./scripts/*.ts"]},
                "packages/*": {"entry": "test/fixtures/*.ts"},
                "./infra/cache/": {"entry": ["alchemy.run.ts"]}
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!([
                "src/root.ts",
                ".github/scripts/report.cjs",
                "apps/server/src/bin.ts",
                "apps/server/scripts/*.ts",
                "packages/*/test/fixtures/*.ts",
                "infra/cache/alchemy.run.ts"
            ])
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn migrate_knip_entry_drops_the_production_marker() {
        let knip = serde_json::json!({"entry": ["src/main.ts!", "src/cli.ts!"]});
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!(["src/main.ts", "src/cli.ts"])
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn migrate_knip_negated_entry_warns_instead_of_writing_a_dead_glob() {
        let knip = serde_json::json!({
            "entry": ["src/**/*.ts", "!src/**/*.test.ts"],
            "workspaces": {"apps/web": {"entry": ["!src/legacy.ts"]}}
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!(["src/**/*.ts"])
        );
        assert_eq!(
            warning_fields(&warnings),
            ["entry", "workspaces.apps/web.entry"]
        );
        assert!(warnings[0].message.contains("!src/**/*.test.ts"));
    }

    #[test]
    fn migrate_knip_workspace_ignore_becomes_rebased_ignore_findings() {
        let knip = serde_json::json!({
            "ignore": ["dist/**"],
            "workspaces": {
                "scripts": {"ignore": ["npm-launcher/**", "!npm-launcher/keep.ts"]},
                "packages/*": {"ignore": "src/generated/**"}
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreFindings").unwrap(),
            &serde_json::json!([
                "dist/**",
                "scripts/npm-launcher/**",
                "!scripts/npm-launcher/keep.ts",
                "packages/*/src/generated/**"
            ])
        );
        assert!(!config.contains_key("ignorePatterns"));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn migrate_knip_workspace_ignore_dependencies_keep_their_workspace_scope() {
        let knip = serde_json::json!({
            "ignoreDependencies": ["lodash"],
            "workspaces": {
                ".": {"ignoreDependencies": ["root-only"]},
                "apps/server": {"ignoreDependencies": ["ffi-rs", "lodash", "/^@org/"]},
                "apps/web": {"ignoreDependencies": ["^(a|b)$"]}
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &serde_json::json!(["lodash", "root-only"]),
            "the root workspace uses the global list"
        );
        assert_eq!(
            config.get("workspaces").unwrap(),
            &serde_json::json!({"settings": {
                "apps/server": {"ignoreDependencies": ["ffi-rs", "lodash", "@org*"]}
            }})
        );
        assert_eq!(
            warning_fields(&warnings),
            ["workspaces.apps/web.ignoreDependencies"]
        );
        assert!(warnings[0].message.contains("^(a|b)$"));
    }

    #[test]
    fn migrate_knip_overlapping_workspace_keys_keep_the_most_specific_value() {
        let knip = serde_json::json!({
            "workspaces": {
                "packages/*": {"includeEntryExports": true, "ignoreDependencies": ["a"]},
                "packages/core": {"entry": ["src/main.ts"]},
                "packages/util": {"includeEntryExports": true}
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("workspaces").unwrap(),
            &serde_json::json!({"settings": {
                "packages/*": {"ignoreDependencies": ["a"], "includeEntryExports": true},
                "packages/core": {"includeEntryExports": false},
                "packages/util": {"includeEntryExports": true}
            }}),
            "a more specific section without the key keeps the root value"
        );
        assert_eq!(
            warning_fields(&warnings),
            [
                "workspaces.packages/*.ignoreDependencies",
                "workspaces.packages/*.ignoreDependencies"
            ],
            "fallow merges the lists of overlapping keys"
        );
        assert!(warnings[0].message.contains("packages/core"));
    }

    #[test]
    fn migrate_knip_ignore_issues_map_to_overrides_and_ignore_exports() {
        let knip = serde_json::json!({
            "rules": {"types": "off"},
            "workspaces": {
                "apps/server": {
                    "ignoreIssues": {"**/*.graphql.ts": ["exports", "types", "namespaceMembers"]}
                },
                "apps/web": {
                    "ignoreIssues": {"src/components/ui/*.tsx": ["exports", "duplicates"]}
                },
                "packages/*": {
                    "ignoreIssues": {"src/_generated/**": ["files", "classMembers", "dependencies"]}
                }
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("overrides").unwrap(),
            &serde_json::json!([
                {
                    "files": ["apps/server/**/*.graphql.ts"],
                    "rules": {
                        "unused-exports": "off",
                        "unused-types": "off",
                        "unused-namespace-members": "off"
                    }
                },
                {
                    "files": ["packages/*/src/_generated/**"],
                    "rules": {"unused-files": "off", "unused-class-members": "off"}
                }
            ])
        );
        assert_eq!(
            config.get("ignoreExports").unwrap(),
            &serde_json::json!([
                {"file": "apps/web/src/components/ui/*.tsx", "exports": ["*"]}
            ])
        );
        assert_eq!(
            warning_fields(&warnings),
            ["workspaces.packages/*.ignoreIssues.src/_generated/**.dependencies"]
        );
    }

    #[test]
    fn migrate_knip_namespace_issue_types_follow_their_base_type() {
        let knip = serde_json::json!({
            "rules": {"types": "off", "nsTypes": "off", "nsExports": "off", "namespaceMembers": "off"},
            "exclude": ["nsExports"],
            "ignoreIssues": {
                "src/ui/*.tsx": ["exports", "nsExports", "namespaceMembers"],
                "src/api/*.ts": ["nsExports"]
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("overrides").unwrap(),
            &serde_json::json!([
                {
                    "files": ["src/ui/*.tsx"],
                    "rules": {"unused-exports": "off", "unused-namespace-members": "off"}
                }
            ])
        );
        assert_eq!(
            config
                .get("rules")
                .and_then(|rules| rules.get("unused-namespace-members")),
            Some(&serde_json::json!("off"))
        );
        assert_eq!(
            warning_fields(&warnings),
            [
                "rules.nsExports",
                "exclude.nsExports",
                "ignoreIssues.src/api/*.ts.nsExports"
            ]
        );
        assert!(
            warnings[0].message.contains("`unused-exports`"),
            "{:?}",
            warnings[0]
        );
        assert!(
            warnings[2].message.contains("`unused-exports`"),
            "{:?}",
            warnings[2]
        );
    }

    #[test]
    fn migrate_knip_namespace_issue_types_accept_defaults_and_global_off() {
        let knip = serde_json::json!({
            "rules": {"nsExports": "error", "types": "off"},
            "ignoreIssues": {"src/api/*.ts": ["nsTypes"]}
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn migrate_knip_namespace_members_map_to_unused_namespace_members() {
        let knip = serde_json::json!({
            "rules": {"namespaceMembers": "warn"},
            "include": ["exports", "namespaceMembers"],
            "exclude": ["namespaceMembers"]
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            config
                .get("rules")
                .and_then(|rules| rules.get("unused-namespace-members")),
            Some(&serde_json::json!("off"))
        );
    }

    #[test]
    fn migrate_ignore_issues_duplicates_without_exports_turns_alias_rule_off() {
        let knip = serde_json::json!({
            "ignoreIssues": {"src/ui/*.tsx": ["duplicates"]}
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("ignoreExports"));
        assert_eq!(
            config.get("overrides").unwrap(),
            &serde_json::json!([
                {
                    "files": ["src/ui/*.tsx"],
                    "rules": {"duplicate-export-aliases": "off"}
                }
            ])
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_include_entry_exports_keeps_its_workspace_scope() {
        let knip = serde_json::json!({
            "workspaces": {
                "packages/*": {"includeEntryExports": true},
                "apps/web": {"includeEntryExports": false}
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("includeEntryExports"));
        assert_eq!(
            config.get("workspaces").unwrap(),
            &serde_json::json!({"settings": {"packages/*": {"includeEntryExports": true}}}),
            "a value equal to the root value adds nothing"
        );
        assert!(warnings.is_empty(), "{warnings:?}");

        let knip = serde_json::json!({
            "includeEntryExports": true,
            "workspaces": {".": {"includeEntryExports": false}}
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);
        assert_eq!(
            config.get("includeEntryExports").unwrap(),
            &serde_json::json!(true)
        );
        assert_eq!(
            warning_fields(&warnings),
            [r#"workspaces["."].includeEntryExports"#]
        );
    }

    #[test]
    fn migrate_knip_unmarked_entries_become_support_entries_when_the_config_marks_one() {
        let knip = serde_json::json!({
            "entry": ["scripts/build.ts"],
            "workspaces": {
                "apps/server": {"entry": ["src/bin.ts!", "scripts/cli.ts"]},
                "apps/web": {"entry": "test/fixtures/**/*.mjs"}
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!(["apps/server/src/bin.ts"])
        );
        assert_eq!(
            config.get("supportEntry").unwrap(),
            &serde_json::json!([
                "scripts/build.ts",
                "apps/server/scripts/cli.ts",
                "apps/web/test/fixtures/**/*.mjs"
            ])
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn migrate_knip_entries_stay_entries_when_no_entry_is_marked() {
        let knip = serde_json::json!({
            "entry": ["src/main.ts"],
            "workspaces": {"apps/web": {"entry": ["scripts/a.ts"]}}
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!(["src/main.ts", "apps/web/scripts/a.ts"])
        );
        assert!(!config.contains_key("supportEntry"));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn migrate_knip_workspace_unmapped_fields_warn_at_the_exact_field() {
        let knip = serde_json::json!({
            "workspaces": {
                ".": {
                    "project": ["*.ts"],
                    "vitest": {"entry": [".github/**/*.test.cjs"]},
                    "ignoreUnresolved": ["virtual:x"],
                    "unknownKey": true
                }
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(config.is_empty(), "{config:?}");
        assert_eq!(
            warning_fields(&warnings),
            [
                "workspaces[\".\"].project",
                "workspaces[\".\"].vitest",
                "workspaces[\".\"].ignoreUnresolved",
                "workspaces[\".\"].unknownKey"
            ]
        );
        assert!(warnings[1].message.contains("auto-detected"));
    }

    #[test]
    fn migrate_knip_mixed_production_entries_split_without_a_warning() {
        let knip = serde_json::json!({
            "entry": ["src/prod.ts!", "src/dev.ts"],
            "workspaces": {
                "apps/server": {"entry": ["src/bin.ts!", "scripts/*.ts"]},
                "apps/docs": {"entry": ["scripts/build.ts"]}
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!(["src/prod.ts", "apps/server/src/bin.ts"])
        );
        assert_eq!(
            config.get("supportEntry").unwrap(),
            &serde_json::json!([
                "src/dev.ts",
                "apps/server/scripts/*.ts",
                "apps/docs/scripts/build.ts"
            ])
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn migrate_knip_workspace_keys_and_patterns_are_normalized() {
        let knip = serde_json::json!({
            "workspaces": {
                "apps\\win": {"entry": ["src\\main.ts", "/src/abs.ts"]},
                "/apps/rooted": {"entry": ["index.ts"]},
                "!apps/neg": {"entry": ["n.ts"]},
                "packages/**": {"entry": ["**/*.ts"], "ignore": ["!**/keep.ts"]}
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!([
                "apps/win/src/main.ts",
                "apps/win/src/abs.ts",
                "apps/rooted/index.ts",
                "packages/**/*.ts"
            ])
        );
        assert_eq!(
            config.get("ignoreFindings").unwrap(),
            &serde_json::json!(["!packages/**/keep.ts"])
        );
        assert_eq!(warning_fields(&warnings), ["workspaces.!apps/neg"]);
    }

    #[test]
    fn migrate_knip_workspace_traversal_patterns_are_not_written() {
        let knip = serde_json::json!({
            "workspaces": {
                "apps/web": {"entry": ["../shared/index.ts", "src/main.ts"]},
                "../outside": {"entry": ["index.ts"]}
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("entry").unwrap(),
            &serde_json::json!(["apps/web/src/main.ts"])
        );
        assert_eq!(
            warning_fields(&warnings),
            ["workspaces.apps/web.entry", "workspaces.../outside"]
        );
    }

    #[test]
    fn migrate_knip_workspaces_non_object_warns() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"workspaces": ["packages/*"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "workspaces");
        assert!(warnings[0].message.contains("expected an object"));
    }

    #[test]
    fn migrate_knip_workspace_member_non_object_warns_at_member() {
        let knip = serde_json::json!({"workspaces": {"packages/*": true}});
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "workspaces.packages/*");
        assert!(warnings[0].message.contains("expected an object"));
    }

    #[test]
    fn migrate_knip_all_regex_ignore_deps_no_output() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"ignoreDependencies": ["^(a|b)$", "/^lodash/i"]}"#).unwrap();
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
            serde_json::from_str(r#"{"include": ["files", "catalog"]}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let include_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.field.starts_with("include."))
            .collect();
        assert_eq!(include_warnings.len(), 1);
        assert!(include_warnings[0].field.contains("catalog"));
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
        assert_eq!(rules.get("duplicate-export-aliases").unwrap(), "off");
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_exclude_all_mappable_types() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{"exclude": ["files", "dependencies", "devDependencies", "exports",
                "types", "enumMembers", "classMembers", "namespaceMembers", "unlisted", "binaries",
                "unresolved", "duplicates", "cycles"]}"#,
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

        assert!(!config.contains_key("ignoreFindings"));
    }

    #[test]
    fn migrate_knip_unmappable_without_suggestion() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"tags": ["-lintignore"], "treatConfigHintsAsErrors": true}"#)
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
                .contains("fallow.tools/docs/migration/from-knip/")
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
    fn migrate_ignore_binaries_and_binaries_rule() {
        let knip = serde_json::json!({
            "ignoreBinaries": ["eas", "/^xcrun/"],
            "rules": {"binaries": "off"},
            "workspaces": {"apps/mobile": {"ignoreBinaries": ["plutil", "eas"]}}
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreBinaries").unwrap(),
            &serde_json::json!(["eas", "xcrun*", "plutil"])
        );
        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unlisted-binaries").unwrap(), "off");
        assert_eq!(
            warning_fields(&warnings),
            ["workspaces.apps/mobile.ignoreBinaries"]
        );
        assert!(warnings[0].message.contains("global ignoreBinaries list"));
    }

    #[test]
    fn migrate_knip_documented_unmappable_keeps_existing_message() {
        let knip: serde_json::Value =
            serde_json::from_str(r#"{"rules": {"catalog": "warn"}}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "rules.catalog");
        assert!(warnings[0].message.contains("no fallow equivalent"));
        assert!(warnings[0].suggestion.is_none());
    }

    #[test]
    fn migrate_knip_complex_full_config() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{
                "entry": ["src/index.ts", "src/worker.ts"],
                "ignore": ["**/*.generated.*"],
                "ignoreDependencies": ["/^@internal/", "^(a|b)$", "lodash", "react"],
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
            &serde_json::json!(["src/index.ts", "src/worker.ts", "packages/*/index.ts"])
        );
        assert_eq!(
            config.get("ignoreFindings").unwrap(),
            &serde_json::json!(["**/*.generated.*"])
        );
        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &serde_json::json!(["@internal*", "lodash", "react"])
        );

        let rules = config.get("rules").unwrap().as_object().unwrap();
        assert_eq!(rules.get("unused-files").unwrap(), "warn");
        assert_eq!(rules.get("unused-exports").unwrap(), "error");
        assert_eq!(rules.get("unused-types").unwrap(), "off");

        let warning_fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
        assert!(warning_fields.contains(&"ignoreDependencies"));
        assert!(warning_fields.contains(&"project"));
        assert!(warning_fields.contains(&"eslint"));
        assert!(
            !warning_fields
                .iter()
                .any(|field| field.starts_with("workspaces"))
        );
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
            config.get("ignoreFindings").unwrap(),
            &serde_json::json!(["dist/**"])
        );
    }

    #[test]
    fn migrate_knip_negated_only_ignore_is_preserved() {
        let knip = serde_json::json!({"ignore": ["!src/keep.ts"]});
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config["ignoreFindings"],
            serde_json::json!(["!src/keep.ts"])
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_knip_mixed_ignore_warns_for_each_invalid_entry() {
        let knip = serde_json::json!({"ignore": ["src/**", 7, null, "!src/keep.ts"]});
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config["ignoreFindings"],
            serde_json::json!(["src/**", "!src/keep.ts"])
        );
        let fields: Vec<&str> = warnings
            .iter()
            .map(|warning| warning.field.as_str())
            .collect();
        assert_eq!(fields, ["ignore[1]", "ignore[2]"]);
    }

    #[test]
    fn migrate_knip_invalid_ignore_shape_warns_without_output() {
        let knip = serde_json::json!({"ignore": {"pattern": "src/**"}});
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert!(!config.contains_key("ignoreFindings"));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "ignore");
        assert!(warnings[0].message.contains("string or array of strings"));
    }

    #[test]
    fn migrate_knip_all_warnings_have_knip_source() {
        let knip: serde_json::Value = serde_json::from_str(
            r#"{
                "project": ["src/**"],
                "ignoreDependencies": ["^(a|b)$"],
                "rules": {"catalog": "warn"},
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

    fn warning_for<'a>(warnings: &'a [MigrationWarning], field: &str) -> &'a MigrationWarning {
        let matching: Vec<&MigrationWarning> =
            warnings.iter().filter(|w| w.field == field).collect();
        assert_eq!(matching.len(), 1, "one warning for {field}: {warnings:?}");
        matching[0]
    }

    #[test]
    fn migrate_knip_production_ignore_entries_lose_the_marker_with_a_warning() {
        let knip = serde_json::json!({
            "ignoreDependencies": ["left-pad!", "lodash", "/^@org/!"],
            "ignoreBinaries": ["tsx!"],
            "workspaces": {"apps/web": {"ignoreDependencies": ["react!"]}}
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        assert_eq!(
            config.get("ignoreDependencies").unwrap(),
            &serde_json::json!(["left-pad", "lodash", "@org*"])
        );
        assert_eq!(
            config.get("ignoreBinaries").unwrap(),
            &serde_json::json!(["tsx"])
        );
        assert_eq!(
            config["workspaces"]["settings"]["apps/web"]["ignoreDependencies"],
            serde_json::json!(["react"])
        );
        let messages: Vec<(&str, &str)> = warnings
            .iter()
            .map(|w| (w.field.as_str(), w.message.as_str()))
            .collect();
        assert_eq!(
            messages,
            [
                (
                    "ignoreDependencies",
                    "`left-pad!` applies only in production mode; fallow has no production-only ignoreDependencies, so `left-pad` is now ignored in every mode"
                ),
                (
                    "ignoreDependencies",
                    "`/^@org/!` applies only in production mode; fallow has no production-only ignoreDependencies, so `@org*` is now ignored in every mode"
                ),
                (
                    "ignoreBinaries",
                    "`tsx!` applies only in production mode; fallow has no production-only ignoreBinaries, so `tsx` is now ignored in every mode"
                ),
                (
                    "workspaces.apps/web.ignoreDependencies",
                    "`react!` applies only in production mode; fallow has no production-only ignoreDependencies, so `react` is now ignored in every mode"
                ),
            ]
        );
        assert_eq!(
            warnings[0].suggestion.as_deref(),
            Some("remove `left-pad` if the default mode must report it")
        );
    }

    #[test]
    fn migrate_knip_project_production_exclusions_are_named_in_the_warning() {
        let knip = serde_json::json!({
            "project": ["**/*.js!", "!scripts/**!", "!tests/**!", "!legacy/**"],
            "workspaces": {"apps/web": {"project": ["src/**", "!e2e/**!"]}}
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let root = warning_for(&warnings, "project");
        assert_eq!(
            root.message,
            "production exclusions `!scripts/**!`, `!tests/**!` not migrated; fallow has no production-only exclusion, so `--production` reports unused files and exports in these paths"
        );
        assert_eq!(
            root.suggestion.as_deref(),
            Some(
                "to hide these findings in every mode, add an overrides entry with files [\"scripts/**\", \"tests/**\"] and rules unused-files and unused-exports set to off"
            )
        );
        let workspace = warning_for(&warnings, "workspaces.apps/web.project");
        assert_eq!(
            workspace.message,
            "production exclusions `!e2e/**!` not migrated; fallow has no production-only exclusion, so `--production` reports unused files and exports in these paths"
        );
        assert_eq!(
            workspace.suggestion.as_deref(),
            Some(
                "to hide these findings in every mode, add an overrides entry with files [\"apps/web/e2e/**\"] and rules unused-files and unused-exports set to off"
            )
        );

        // A `project` without a production exclusion keeps the general warning.
        let mut plain_warnings = Vec::new();
        migrate_knip(
            &serde_json::json!({"project": ["src/**", "!legacy/**"]}),
            &mut empty_config(),
            &mut plain_warnings,
        );
        assert_eq!(
            warning_for(&plain_warnings, "project").message,
            "Fallow auto-discovers project files"
        );
    }

    #[test]
    fn migrate_knip_plugin_configs_with_names_fallow_skips_are_named() {
        let knip = serde_json::json!({
            "playwright": {"config": ["playwright.e2e.config.ts", "e2e/pw.ts", "./suites/smoke.ts"]},
            "vitest": {"config": ["vitest.config.ts"]},
            "jest": {"config": ["jest.config.{js,ts}", "package.json"]},
            "next": {"config": "config/next.custom.mjs"},
            "workspaces": {
                "apps/web": {"playwright": {"config": "playwright.config.{js,ts,mjs}"}},
                "apps/api": {"playwright": {"config": "custom.ts"}}
            }
        });
        let mut config = empty_config();
        let mut warnings = Vec::new();
        migrate_knip(&knip, &mut config, &mut warnings);

        let playwright = warning_for(&warnings, "playwright");
        assert_eq!(
            playwright.message,
            "plugin config `playwright` names config files that fallow does not find by file name: `e2e/pw.ts`, `./suites/smoke.ts`"
        );
        assert_eq!(
            playwright.suggestion.as_deref(),
            Some(
                "pass each of these configs to `playwright` with `--config` in a package.json script, or add entry patterns for the test files, setup files and reporters that each config names"
            )
        );
        assert!(
            warning_for(&warnings, "vitest")
                .message
                .contains("auto-detected")
        );
        // Fallow reads the inline config in package.json without a config file.
        assert!(
            warning_for(&warnings, "jest")
                .message
                .contains("auto-detected")
        );
        // `next` is the knip key of the fallow `nextjs` plugin.
        assert_eq!(
            warning_for(&warnings, "next").message,
            "plugin config `next` names config files that fallow does not find by file name: `config/next.custom.mjs`"
        );
        assert!(
            warning_for(&warnings, "workspaces.apps/web.playwright")
                .message
                .contains("auto-detected")
        );
        assert_eq!(
            warning_for(&warnings, "workspaces.apps/api.playwright").message,
            "plugin config `playwright` names config files that fallow does not find by file name: `custom.ts`"
        );
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
