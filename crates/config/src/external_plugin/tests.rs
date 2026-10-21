use super::*;
use crate::ScopedUsedClassMemberRule;

#[test]
fn deserialize_minimal_plugin() {
    let toml_str = r#"
name = "my-plugin"
enablers = ["my-pkg"]
"#;
    let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
    assert_eq!(plugin.name, "my-plugin");
    assert_eq!(plugin.enablers, vec!["my-pkg"]);
    assert!(plugin.entry_points.is_empty());
    assert!(plugin.always_used.is_empty());
    assert!(plugin.config_patterns.is_empty());
    assert!(plugin.tooling_dependencies.is_empty());
    assert!(plugin.used_exports.is_empty());
    assert!(plugin.used_class_members.is_empty());
}

#[test]
fn deserialize_plugin_with_used_class_members_json() {
    let json_str = r#"{
        "name": "ag-grid",
        "enablers": ["ag-grid-angular"],
        "usedClassMembers": ["agInit", "refresh"]
    }"#;
    let plugin: ExternalPluginDef = serde_json::from_str(json_str).unwrap();
    assert_eq!(plugin.name, "ag-grid");
    assert_eq!(
        plugin.used_class_members,
        vec![
            UsedClassMemberRule::from("agInit"),
            UsedClassMemberRule::from("refresh"),
        ]
    );
}

#[test]
fn deserialize_plugin_with_scoped_used_class_members_json() {
    let json_str = r#"{
        "name": "ag-grid",
        "enablers": ["ag-grid-angular"],
        "usedClassMembers": [
            "agInit",
            { "implements": "ICellRendererAngularComp", "members": ["refresh"] },
            { "extends": "BaseCommand", "members": ["execute"] }
        ]
    }"#;
    let plugin: ExternalPluginDef = serde_json::from_str(json_str).unwrap();
    assert_eq!(
        plugin.used_class_members,
        vec![
            UsedClassMemberRule::from("agInit"),
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
fn deserialize_plugin_with_used_class_members_toml() {
    let toml_str = r#"
name = "ag-grid"
enablers = ["ag-grid-angular"]
usedClassMembers = ["agInit", "refresh"]
"#;
    let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
    assert_eq!(
        plugin.used_class_members,
        vec![
            UsedClassMemberRule::from("agInit"),
            UsedClassMemberRule::from("refresh"),
        ]
    );
}

#[test]
fn deserialize_plugin_with_scoped_used_class_members_toml() {
    let toml_str = r#"
name = "ag-grid"
enablers = ["ag-grid-angular"]
usedClassMembers = [
  { implements = "ICellRendererAngularComp", members = ["refresh"] },
  { extends = "BaseCommand", members = ["execute"] }
]
"#;
    let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
    assert_eq!(
        plugin.used_class_members,
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
fn deserialize_plugin_rejects_unconstrained_scoped_used_class_members() {
    let result = serde_json::from_str::<ExternalPluginDef>(
        r#"{
            "name": "ag-grid",
            "enablers": ["ag-grid-angular"],
            "usedClassMembers": [{ "members": ["refresh"] }]
        }"#,
    );
    assert!(
        result.is_err(),
        "unconstrained scoped rule should be rejected"
    );
}

#[test]
fn deserialize_full_plugin() {
    let toml_str = r#"
name = "my-framework"
enablers = ["my-framework", "@my-framework/core"]
entryPoints = ["src/routes/**/*.{ts,tsx}", "src/middleware.ts"]
configPatterns = ["my-framework.config.{ts,js,mjs}"]
alwaysUsed = ["src/setup.ts", "public/**/*"]
toolingDependencies = ["my-framework-cli"]

[[usedExports]]
pattern = "src/routes/**/*.{ts,tsx}"
exports = ["default", "loader", "action"]

[[usedExports]]
pattern = "src/middleware.ts"
exports = ["default"]
"#;
    let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
    assert_eq!(plugin.name, "my-framework");
    assert_eq!(plugin.enablers.len(), 2);
    assert_eq!(plugin.entry_points.len(), 2);
    assert_eq!(
        plugin.config_patterns,
        vec!["my-framework.config.{ts,js,mjs}"]
    );
    assert_eq!(plugin.always_used.len(), 2);
    assert_eq!(plugin.tooling_dependencies, vec!["my-framework-cli"]);
    assert_eq!(plugin.used_exports.len(), 2);
    assert_eq!(plugin.used_exports[0].pattern, "src/routes/**/*.{ts,tsx}");
    assert_eq!(
        plugin.used_exports[0].exports,
        vec!["default", "loader", "action"]
    );
}

#[test]
fn deserialize_json_plugin() {
    let json_str = r#"{
        "name": "my-json-plugin",
        "enablers": ["my-pkg"],
        "entryPoints": ["src/**/*.ts"],
        "configPatterns": ["my-plugin.config.js"],
        "alwaysUsed": ["src/setup.ts"],
        "toolingDependencies": ["my-cli"],
        "usedExports": [
            { "pattern": "src/**/*.ts", "exports": ["default"] }
        ]
    }"#;
    let plugin: ExternalPluginDef = serde_json::from_str(json_str).unwrap();
    assert_eq!(plugin.name, "my-json-plugin");
    assert_eq!(plugin.enablers, vec!["my-pkg"]);
    assert_eq!(plugin.entry_points, vec!["src/**/*.ts"]);
    assert_eq!(plugin.config_patterns, vec!["my-plugin.config.js"]);
    assert_eq!(plugin.always_used, vec!["src/setup.ts"]);
    assert_eq!(plugin.tooling_dependencies, vec!["my-cli"]);
    assert_eq!(plugin.used_exports.len(), 1);
    assert_eq!(plugin.used_exports[0].exports, vec!["default"]);
}

#[test]
fn manifest_field_paths_and_templates_are_parsed_once_and_round_trip() {
    let source = r#"{
        "name": "manifest-plugin",
        "manifestEntries": [{
            "manifests": "**/manifest.json",
            "when": { "plugin.browser": true },
            "entries": [{
                "path": "${plugin.entry}/index.{ts,tsx}",
                "when": { "type": "plugin" }
            }]
        }]
    }"#;

    let plugin: ExternalPluginDef = serde_json::from_str(source).unwrap();
    let rule = &plugin.manifest_entries[0];
    let condition_path = rule.when.keys().next().unwrap();
    assert_eq!(
        condition_path.segments(),
        &[
            ManifestFieldSegment::Key("plugin".to_string()),
            ManifestFieldSegment::Key("browser".to_string()),
        ]
    );
    assert_eq!(
        rule.entries[0].path.as_str(),
        "${plugin.entry}/index.{ts,tsx}"
    );
    assert_eq!(
        rule.entries[0].path.parts(),
        &[
            ManifestPathPart::Field("plugin.entry".parse().unwrap()),
            ManifestPathPart::Literal("/index.{ts,tsx}".to_string()),
        ]
    );

    let serialized = serde_json::to_value(&plugin).unwrap();
    assert_eq!(
        serialized["manifestEntries"][0]["when"]["plugin.browser"],
        true
    );
    assert_eq!(
        serialized["manifestEntries"][0]["entries"][0]["path"],
        "${plugin.entry}/index.{ts,tsx}"
    );
}

#[test]
fn invalid_manifest_field_paths_and_templates_are_rejected() {
    for source in [
        r#"{
            "name": "bad-condition",
            "manifestEntries": [{
                "manifests": "**/manifest.json",
                "when": { "plugin..browser": true },
                "entries": [{ "path": "index.ts" }]
            }]
        }"#,
        r#"{
            "name": "bad-template-field",
            "manifestEntries": [{
                "manifests": "**/manifest.json",
                "entries": [{ "path": "${plugin..entry}/index.ts" }]
            }]
        }"#,
        r#"{
            "name": "bad-entry-condition",
            "manifestEntries": [{
                "manifests": "**/manifest.json",
                "entries": [{
                    "path": "index.ts",
                    "when": { "plugin..browser": true }
                }]
            }]
        }"#,
        r#"{
            "name": "unterminated-template",
            "manifestEntries": [{
                "manifests": "**/manifest.json",
                "entries": [{ "path": "${plugin.entry/index.ts" }]
            }]
        }"#,
    ] {
        assert!(serde_json::from_str::<ExternalPluginDef>(source).is_err());
    }
}

#[test]
fn manifest_field_paths_support_only_explicit_array_traversal() {
    let path: ManifestFieldPath = "content_scripts[*].js[*]".parse().unwrap();
    assert_eq!(
        path.segments(),
        &[
            ManifestFieldSegment::Key("content_scripts".to_string()),
            ManifestFieldSegment::Each,
            ManifestFieldSegment::Key("js".to_string()),
            ManifestFieldSegment::Each,
        ]
    );

    for invalid in [
        "content_scripts[0].js",
        "content_scripts[].js",
        "content_scripts[foo].js",
        "content_scripts[*x].js",
        "[*].js",
    ] {
        assert!(invalid.parse::<ManifestFieldPath>().is_err(), "{invalid}");
    }
}

#[test]
fn manifest_conditions_distinguish_scalar_equality_from_presence() {
    let source = r#"{
        "name": "condition-plugin",
        "manifestEntries": [{
            "manifests": "**/manifest.json",
            "when": {
                "main": { "exists": true },
                "enabled": false,
                "mode": "worker",
                "version": 3,
                "metadata": null
            },
            "entries": [{ "path": "index.ts" }]
        }]
    }"#;
    let plugin: ExternalPluginDef = serde_json::from_str(source).unwrap();
    let when = &plugin.manifest_entries[0].when;

    assert_eq!(
        when.get(&"main".parse().unwrap()),
        Some(&ManifestCondition::Exists(ManifestExistsPredicate {
            exists: true,
        }))
    );
    assert_eq!(
        when.get(&"enabled".parse().unwrap()),
        Some(&ManifestCondition::Equals(serde_json::Value::Bool(false)))
    );
    assert_eq!(
        when.get(&"metadata".parse().unwrap()),
        Some(&ManifestCondition::Equals(serde_json::Value::Null))
    );

    let serialized = serde_json::to_value(plugin).unwrap();
    assert_eq!(
        serialized["manifestEntries"][0]["when"]["main"],
        serde_json::json!({ "exists": true })
    );
}

#[test]
fn manifest_conditions_preserve_non_operator_json_equality_values() {
    let expected = [
        serde_json::json!({ "exists": "not-an-operator" }),
        serde_json::json!({ "exists": true, "extra": true }),
        serde_json::json!({ "other": true }),
        serde_json::json!(["worker"]),
    ];
    for condition in expected {
        let source = serde_json::json!({
            "name": "equality-condition",
            "manifestEntries": [{
                "manifests": "**/manifest.json",
                "when": { "field": condition.clone() },
                "entries": [{ "path": "index.ts" }]
            }]
        });
        let plugin: ExternalPluginDef = serde_json::from_value(source).unwrap();
        assert_eq!(
            plugin.manifest_entries[0]
                .when
                .get(&"field".parse().unwrap()),
            Some(&ManifestCondition::Equals(condition))
        );
    }
}

#[test]
fn manifest_exists_conditions_deserialize_from_toml() {
    let source = r#"
name = "condition-plugin"

[[manifestEntries]]
manifests = "**/manifest.json"

[manifestEntries.when.main]
exists = true

[[manifestEntries.entries]]
path = "index.ts"
"#;
    let plugin: ExternalPluginDef = toml::from_str(source).unwrap();
    assert_eq!(
        plugin.manifest_entries[0]
            .when
            .get(&"main".parse().unwrap()),
        Some(&ManifestCondition::Exists(ManifestExistsPredicate {
            exists: true,
        }))
    );
}

#[test]
fn deserialize_jsonc_plugin() {
    let jsonc_str = r#"{
        "name": "my-jsonc-plugin",
        "enablers": ["my-pkg"],
        /* Block comment */
        "entryPoints": ["src/**/*.ts"]
    }"#;
    let plugin: ExternalPluginDef = crate::jsonc::parse_to_value(jsonc_str).unwrap();
    assert_eq!(plugin.name, "my-jsonc-plugin");
    assert_eq!(plugin.enablers, vec!["my-pkg"]);
    assert_eq!(plugin.entry_points, vec!["src/**/*.ts"]);
}

#[test]
fn deserialize_json_with_schema_field() {
    let json_str = r#"{
        "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/plugin-schema.json",
        "name": "schema-plugin",
        "enablers": ["my-pkg"]
    }"#;
    let plugin: ExternalPluginDef = serde_json::from_str(json_str).unwrap();
    assert_eq!(plugin.name, "schema-plugin");
    assert_eq!(plugin.enablers, vec!["my-pkg"]);
}

#[test]
fn plugin_json_schema_generation() {
    let schema = ExternalPluginDef::json_schema();
    assert!(schema.is_object());
    let obj = schema.as_object().unwrap();
    assert!(obj.contains_key("properties"));
}

#[test]
fn discover_plugins_from_fallow_plugins_dir() {
    let dir = std::env::temp_dir().join(format!("fallow-test-ext-plugins-{}", std::process::id()));
    let plugins_dir = dir.join(".fallow").join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);

    std::fs::write(
        plugins_dir.join("my-plugin.toml"),
        r#"
name = "my-plugin"
enablers = ["my-pkg"]
entryPoints = ["src/**/*.ts"]
"#,
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &[]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "my-plugin");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discover_json_plugins_from_fallow_plugins_dir() {
    let dir = std::env::temp_dir().join(format!(
        "fallow-test-ext-json-plugins-{}",
        std::process::id()
    ));
    let plugins_dir = dir.join(".fallow").join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);

    std::fs::write(
        plugins_dir.join("my-plugin.json"),
        r#"{"name": "json-plugin", "enablers": ["json-pkg"]}"#,
    )
    .unwrap();

    std::fs::write(
        plugins_dir.join("my-plugin.jsonc"),
        r#"{
            "name": "jsonc-plugin",
            "enablers": ["jsonc-pkg"]
        }"#,
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &[]);
    assert_eq!(plugins.len(), 2);
    assert_eq!(plugins[0].name, "json-plugin");
    assert_eq!(plugins[1].name, "jsonc-plugin");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discover_fallow_plugin_files_in_root() {
    let dir = std::env::temp_dir().join(format!("fallow-test-root-plugins-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);

    std::fs::write(
        dir.join("fallow-plugin-custom.toml"),
        r#"
name = "custom"
enablers = ["custom-pkg"]
"#,
    )
    .unwrap();

    std::fs::write(dir.join("some-other-file.toml"), r#"name = "ignored""#).unwrap();

    let plugins = discover_external_plugins(&dir, &[]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "custom");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discover_fallow_plugin_json_files_in_root() {
    let dir = std::env::temp_dir().join(format!(
        "fallow-test-root-json-plugins-{}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);

    std::fs::write(
        dir.join("fallow-plugin-custom.json"),
        r#"{"name": "json-root", "enablers": ["json-pkg"]}"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("fallow-plugin-custom2.jsonc"),
        r#"{
            "name": "jsonc-root",
            "enablers": ["jsonc-pkg"]
        }"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("fallow-plugin-bad.yaml"),
        "name: ignored\nenablers:\n  - pkg\n",
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &[]);
    assert_eq!(plugins.len(), 2);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discover_mixed_formats_in_dir() {
    let dir =
        std::env::temp_dir().join(format!("fallow-test-mixed-plugins-{}", std::process::id()));
    let plugins_dir = dir.join(".fallow").join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);

    std::fs::write(
        plugins_dir.join("a-plugin.toml"),
        r#"
name = "toml-plugin"
enablers = ["toml-pkg"]
"#,
    )
    .unwrap();

    std::fs::write(
        plugins_dir.join("b-plugin.json"),
        r#"{"name": "json-plugin", "enablers": ["json-pkg"]}"#,
    )
    .unwrap();

    std::fs::write(
        plugins_dir.join("c-plugin.jsonc"),
        r#"{
            "name": "jsonc-plugin",
            "enablers": ["jsonc-pkg"]
        }"#,
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &[]);
    assert_eq!(plugins.len(), 3);
    assert_eq!(plugins[0].name, "toml-plugin");
    assert_eq!(plugins[1].name, "json-plugin");
    assert_eq!(plugins[2].name, "jsonc-plugin");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deduplicates_by_name() {
    let dir =
        std::env::temp_dir().join(format!("fallow-test-dedup-plugins-{}", std::process::id()));
    let plugins_dir = dir.join(".fallow").join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);

    std::fs::write(
        plugins_dir.join("my-plugin.toml"),
        r#"
name = "my-plugin"
enablers = ["pkg-a"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("fallow-plugin-my-plugin.toml"),
        r#"
name = "my-plugin"
enablers = ["pkg-b"]
"#,
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &[]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].enablers, vec!["pkg-a"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_plugin_paths_take_priority() {
    let dir = std::env::temp_dir().join(format!("fallow-test-config-paths-{}", std::process::id()));
    let custom_dir = dir.join("custom-plugins");
    let _ = std::fs::create_dir_all(&custom_dir);

    std::fs::write(
        custom_dir.join("explicit.toml"),
        r#"
name = "explicit"
enablers = ["explicit-pkg"]
"#,
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &["custom-plugins".to_string()]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "explicit");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_plugin_path_to_single_file() {
    let dir = std::env::temp_dir().join(format!("fallow-test-single-file-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);

    std::fs::write(
        dir.join("my-plugin.toml"),
        r#"
name = "single-file"
enablers = ["single-pkg"]
"#,
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &["my-plugin.toml".to_string()]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "single-file");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_plugin_path_to_single_json_file() {
    let dir = std::env::temp_dir().join(format!(
        "fallow-test-single-json-file-{}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);

    std::fs::write(
        dir.join("my-plugin.json"),
        r#"{"name": "json-single", "enablers": ["json-pkg"]}"#,
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &["my-plugin.json".to_string()]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "json-single");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn configured_plugin_diagnostics_report_missing_file() {
    let dir = tempfile::tempdir().expect("temp root");

    let diagnostics =
        diagnose_configured_external_plugins(dir.path(), &["missing-plugin.json".to_string()]);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].kind, ConfiguredPluginDiagnosticKind::Missing);
    assert_eq!(diagnostics[0].path, dir.path().join("missing-plugin.json"));
}

#[test]
fn configured_plugin_diagnostics_report_malformed_file() {
    let dir = tempfile::tempdir().expect("temp root");
    std::fs::write(dir.path().join("broken.json"), "{").expect("write plugin");

    let diagnostics =
        diagnose_configured_external_plugins(dir.path(), &["broken.json".to_string()]);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        ConfiguredPluginDiagnosticKind::InvalidDefinition
    );
}

#[test]
fn configured_plugin_diagnostics_report_directory_without_plugin_files() {
    let dir = tempfile::tempdir().expect("temp root");
    let plugins_dir = dir.path().join("plugins");
    std::fs::create_dir(&plugins_dir).expect("create plugin dir");
    std::fs::write(plugins_dir.join("plugin.yaml"), "name: ignored")
        .expect("write unsupported file");

    let diagnostics = diagnose_configured_external_plugins(dir.path(), &["plugins".to_string()]);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        ConfiguredPluginDiagnosticKind::NoPluginFiles
    );
}

#[test]
fn skips_invalid_toml() {
    let dir =
        std::env::temp_dir().join(format!("fallow-test-invalid-plugin-{}", std::process::id()));
    let plugins_dir = dir.join(".fallow").join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);

    std::fs::write(plugins_dir.join("bad.toml"), r#"enablers = ["pkg"]"#).unwrap();

    std::fs::write(
        plugins_dir.join("good.toml"),
        r#"
name = "good"
enablers = ["good-pkg"]
"#,
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &[]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "good");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn skips_invalid_json() {
    let dir = std::env::temp_dir().join(format!(
        "fallow-test-invalid-json-plugin-{}",
        std::process::id()
    ));
    let plugins_dir = dir.join(".fallow").join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);

    std::fs::write(plugins_dir.join("bad.json"), r#"{"enablers": ["pkg"]}"#).unwrap();

    std::fs::write(
        plugins_dir.join("good.json"),
        r#"{"name": "good-json", "enablers": ["good-pkg"]}"#,
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &[]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "good-json");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prefix_enablers() {
    let toml_str = r#"
name = "scoped"
enablers = ["@myorg/"]
"#;
    let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
    assert_eq!(plugin.enablers, vec!["@myorg/"]);
}

#[test]
fn skips_empty_name() {
    let dir = std::env::temp_dir().join(format!("fallow-test-empty-name-{}", std::process::id()));
    let plugins_dir = dir.join(".fallow").join("plugins");
    let _ = std::fs::create_dir_all(&plugins_dir);

    std::fs::write(
        plugins_dir.join("empty.toml"),
        r#"
name = ""
enablers = ["pkg"]
"#,
    )
    .unwrap();

    let plugins = discover_external_plugins(&dir, &[]);
    assert!(plugins.is_empty(), "empty-name plugin should be skipped");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rejects_paths_outside_root() {
    let dir = std::env::temp_dir().join(format!("fallow-test-path-escape-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);

    let plugins = discover_external_plugins(&dir, &["../../../etc".to_string()]);
    assert!(plugins.is_empty(), "paths outside root should be rejected");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plugin_format_detection() {
    assert!(matches!(
        PluginFormat::from_path(Path::new("plugin.toml")),
        Some(PluginFormat::Toml)
    ));
    assert!(matches!(
        PluginFormat::from_path(Path::new("plugin.json")),
        Some(PluginFormat::Json)
    ));
    assert!(matches!(
        PluginFormat::from_path(Path::new("plugin.jsonc")),
        Some(PluginFormat::Jsonc)
    ));
    assert!(PluginFormat::from_path(Path::new("plugin.yaml")).is_none());
    assert!(PluginFormat::from_path(Path::new("plugin")).is_none());
}

#[test]
fn is_plugin_file_checks_extensions() {
    assert!(is_plugin_file(Path::new("plugin.toml")));
    assert!(is_plugin_file(Path::new("plugin.json")));
    assert!(is_plugin_file(Path::new("plugin.jsonc")));
    assert!(!is_plugin_file(Path::new("plugin.yaml")));
    assert!(!is_plugin_file(Path::new("plugin.txt")));
    assert!(!is_plugin_file(Path::new("plugin")));
}

#[test]
fn detection_deserialize_dependency() {
    let json = r#"{"type": "dependency", "package": "next"}"#;
    let detection: PluginDetection = serde_json::from_str(json).unwrap();
    assert!(matches!(detection, PluginDetection::Dependency { package } if package == "next"));
}

#[test]
fn detection_deserialize_file_exists() {
    let json = r#"{"type": "fileExists", "pattern": "tsconfig.json"}"#;
    let detection: PluginDetection = serde_json::from_str(json).unwrap();
    assert!(
        matches!(detection, PluginDetection::FileExists { pattern } if pattern == "tsconfig.json")
    );
}

#[test]
fn detection_deserialize_all() {
    let json = r#"{"type": "all", "conditions": [{"type": "dependency", "package": "a"}, {"type": "dependency", "package": "b"}]}"#;
    let detection: PluginDetection = serde_json::from_str(json).unwrap();
    assert!(matches!(detection, PluginDetection::All { conditions } if conditions.len() == 2));
}

#[test]
fn detection_deserialize_any() {
    let json = r#"{"type": "any", "conditions": [{"type": "dependency", "package": "a"}]}"#;
    let detection: PluginDetection = serde_json::from_str(json).unwrap();
    assert!(matches!(detection, PluginDetection::Any { conditions } if conditions.len() == 1));
}

#[test]
fn plugin_with_detection_field() {
    let json = r#"{
        "name": "my-plugin",
        "detection": {"type": "dependency", "package": "my-pkg"},
        "entryPoints": ["src/**/*.ts"]
    }"#;
    let plugin: ExternalPluginDef = serde_json::from_str(json).unwrap();
    assert_eq!(plugin.name, "my-plugin");
    assert!(plugin.detection.is_some());
    assert!(plugin.enablers.is_empty());
    assert_eq!(plugin.entry_points, vec!["src/**/*.ts"]);
}

#[test]
fn plugin_without_detection_uses_enablers() {
    let json = r#"{
        "name": "my-plugin",
        "enablers": ["my-pkg"]
    }"#;
    let plugin: ExternalPluginDef = serde_json::from_str(json).unwrap();
    assert!(plugin.detection.is_none());
    assert_eq!(plugin.enablers, vec!["my-pkg"]);
}

#[test]
fn detection_nested_all_with_any() {
    let json = r#"{
        "type": "all",
        "conditions": [
            {"type": "dependency", "package": "react"},
            {"type": "any", "conditions": [
                {"type": "fileExists", "pattern": "next.config.js"},
                {"type": "fileExists", "pattern": "next.config.mjs"}
            ]}
        ]
    }"#;
    let detection: PluginDetection = serde_json::from_str(json).unwrap();
    match detection {
        PluginDetection::All { conditions } => {
            assert_eq!(conditions.len(), 2);
            assert!(matches!(
                &conditions[0],
                PluginDetection::Dependency { package } if package == "react"
            ));
            match &conditions[1] {
                PluginDetection::Any { conditions: inner } => {
                    assert_eq!(inner.len(), 2);
                }
                other => panic!("expected Any, got: {other:?}"),
            }
        }
        other => panic!("expected All, got: {other:?}"),
    }
}

#[test]
fn detection_empty_all_conditions() {
    let json = r#"{"type": "all", "conditions": []}"#;
    let detection: PluginDetection = serde_json::from_str(json).unwrap();
    assert!(matches!(
        detection,
        PluginDetection::All { conditions } if conditions.is_empty()
    ));
}

#[test]
fn detection_empty_any_conditions() {
    let json = r#"{"type": "any", "conditions": []}"#;
    let detection: PluginDetection = serde_json::from_str(json).unwrap();
    assert!(matches!(
        detection,
        PluginDetection::Any { conditions } if conditions.is_empty()
    ));
}

#[test]
fn detection_toml_dependency() {
    let toml_str = r#"
name = "my-plugin"

[detection]
type = "dependency"
package = "next"
"#;
    let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
    assert!(plugin.detection.is_some());
    assert!(matches!(
        plugin.detection.unwrap(),
        PluginDetection::Dependency { package } if package == "next"
    ));
}

#[test]
fn detection_toml_file_exists() {
    let toml_str = r#"
name = "my-plugin"

[detection]
type = "fileExists"
pattern = "next.config.js"
"#;
    let plugin: ExternalPluginDef = toml::from_str(toml_str).unwrap();
    assert!(matches!(
        plugin.detection.unwrap(),
        PluginDetection::FileExists { pattern } if pattern == "next.config.js"
    ));
}

#[test]
fn plugin_all_fields_json() {
    let json = r#"{
        "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/plugin-schema.json",
        "name": "full-plugin",
        "detection": {"type": "dependency", "package": "my-pkg"},
        "enablers": ["fallback-enabler"],
        "entryPoints": ["src/entry.ts"],
        "configPatterns": ["config.js"],
        "alwaysUsed": ["src/polyfills.ts"],
        "toolingDependencies": ["my-cli"],
        "usedExports": [{"pattern": "src/**", "exports": ["default", "setup"]}]
    }"#;
    let plugin: ExternalPluginDef = serde_json::from_str(json).unwrap();
    assert_eq!(plugin.name, "full-plugin");
    assert!(plugin.detection.is_some());
    assert_eq!(plugin.enablers, vec!["fallback-enabler"]);
    assert_eq!(plugin.entry_points, vec!["src/entry.ts"]);
    assert_eq!(plugin.config_patterns, vec!["config.js"]);
    assert_eq!(plugin.always_used, vec!["src/polyfills.ts"]);
    assert_eq!(plugin.tooling_dependencies, vec!["my-cli"]);
    assert_eq!(plugin.used_exports.len(), 1);
    assert_eq!(plugin.used_exports[0].pattern, "src/**");
    assert_eq!(plugin.used_exports[0].exports, vec!["default", "setup"]);
}

#[test]
fn plugin_with_special_chars_in_name() {
    let json = r#"{"name": "@scope/my-plugin-v2.0", "enablers": ["pkg"]}"#;
    let plugin: ExternalPluginDef = serde_json::from_str(json).unwrap();
    assert_eq!(plugin.name, "@scope/my-plugin-v2.0");
}

#[test]
fn parse_plugin_toml_format() {
    let content = r#"
name = "test-plugin"
enablers = ["test-pkg"]
entryPoints = ["src/**/*.ts"]
"#;
    let result = parse_plugin(content, &PluginFormat::Toml, Path::new("test.toml"));
    assert!(result.is_some());
    let plugin = result.unwrap();
    assert_eq!(plugin.name, "test-plugin");
}

#[test]
fn parse_plugin_json_format() {
    let content = r#"{"name": "json-test", "enablers": ["pkg"]}"#;
    let result = parse_plugin(content, &PluginFormat::Json, Path::new("test.json"));
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "json-test");
}

#[test]
fn parse_plugin_jsonc_format() {
    let content = r#"{
        "name": "jsonc-test",
        "enablers": ["pkg"]
    }"#;
    let result = parse_plugin(content, &PluginFormat::Jsonc, Path::new("test.jsonc"));
    assert!(result.is_some());
    assert_eq!(result.unwrap().name, "jsonc-test");
}

#[test]
fn parse_plugin_invalid_toml_returns_none() {
    let content = "not valid toml [[[";
    let result = parse_plugin(content, &PluginFormat::Toml, Path::new("bad.toml"));
    assert!(result.is_none());
}

#[test]
fn parse_plugin_invalid_json_returns_none() {
    let content = "{ not valid json }";
    let result = parse_plugin(content, &PluginFormat::Json, Path::new("bad.json"));
    assert!(result.is_none());
}

#[test]
fn parse_plugin_invalid_jsonc_returns_none() {
    let content = r#"{"enablers": ["pkg"]}"#;
    let result = parse_plugin(content, &PluginFormat::Jsonc, Path::new("bad.jsonc"));
    assert!(result.is_none());
}
