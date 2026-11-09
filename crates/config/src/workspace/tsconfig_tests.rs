use super::*;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().expect("parent directory")).expect("create parent");
    std::fs::write(path, content).expect("write file");
}

fn map(root: &Path, entry: &str, source_extensions: &[&str]) -> TsconfigOutputResolution {
    TsconfigOutputMap::from_project(root).resolve_source_for_entry(entry, source_extensions)
}

#[test]
fn maps_generated_entry_with_inherited_paths_relative_to_their_config_file() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("configs/tsconfig.base.json"),
        r#"{
            "compilerOptions": {
                "rootDir": "../source",
                "outDir": "../distribution"
            }
        }"#,
    );
    write(
        &root.join("tsconfig.build.json"),
        r#"{"extends":"./configs/tsconfig.base.json"}"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    assert_eq!(
        map(root, "./distribution/index.js", &["ts", "tsx", "js"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
    );
}

#[test]
fn linked_files_follow_extends_and_references_transitively() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path();
    write(
        &root.join("tsconfig.json"),
        r#"{
          // comments are allowed
          "extends": ["./configs/base.json"],
          "references": [{ "path": "./packages/app" }, { "path": "./missing" }]
        }"#,
    );
    write(
        &root.join("configs/base.json"),
        r#"{"extends":"./shared.json"}"#,
    );
    write(&root.join("configs/shared.json"), "{}");
    write(
        &root.join("packages/app/tsconfig.json"),
        r#"{"extends":"../../tsconfig.json"}"#,
    );

    let mut linked = tsconfig_linked_files(&root.join("tsconfig.json"));
    linked.sort();
    let mut expected = vec![
        root.join("configs/base.json"),
        root.join("configs/shared.json"),
        root.join("packages/app/tsconfig.json"),
    ];
    expected.sort();
    assert_eq!(linked, expected);
}

#[test]
fn no_emit_configs_do_not_map_public_outputs() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("configs/tsconfig.base.json"),
        r#"{
            "compilerOptions": {
                "rootDir": "../source",
                "outDir": "../distribution"
            }
        }"#,
    );
    write(
        &root.join("tsconfig.test.json"),
        r#"{
            "extends": "./configs/tsconfig.base.json",
            "compilerOptions": {"noEmit": true}
        }"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::ConfiguredWithoutSource,
        "a noEmit config maps no output to a source file"
    );
    write(
        &root.join("tsconfig.test.json"),
        r#"{"extends":"./configs/tsconfig.base.json","compilerOptions":{"noEmit":false}}"#,
    );
    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
    );
}

/// A Nest build can declare `rootDir: "."` while the scripts run
/// `dist/<path>.js` from `src/<path>.ts`. The config claims the output but
/// names no file, which differs from two candidate files.
#[test]
fn emitting_config_without_a_source_file_is_reported_apart_from_ambiguity() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("tsconfig.build.json"),
        r#"{"compilerOptions":{"rootDir":".","outDir":"./dist"}}"#,
    );
    write(
        &root.join("src/database/setup-db.ts"),
        "export const value = 1;\n",
    );

    assert_eq!(
        map(root, "./dist/database/setup-db.js", &["ts"]),
        TsconfigOutputResolution::ConfiguredWithoutSource
    );
}

#[test]
fn ambiguous_source_candidates_fail_closed() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("tsconfig.build.json"),
        r#"{
            "compilerOptions": {
                "rootDir": "./source",
                "outDir": "./distribution"
            }
        }"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");
    write(&root.join("source/index.js"), "export const value = 1;\n");

    assert_eq!(
        map(root, "./distribution/index.js", &["ts", "js"]),
        TsconfigOutputResolution::ConfiguredButUnresolved
    );
    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
    );
}

#[test]
fn maps_dotted_basenames_and_keeps_module_output_families_separate() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("tsconfig.build.json"),
        r#"{
            "compilerOptions": {
                "rootDir": "./source",
                "outDir": "./distribution"
            }
        }"#,
    );
    write(
        &root.join("source/feature.test.ts"),
        "export const value = 1;\n",
    );
    write(
        &root.join("source/feature.native.mts"),
        "export const native = 1;\n",
    );

    assert_eq!(
        map(root, "./distribution/feature.test.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/feature.test.ts"))
    );
    assert_eq!(
        map(root, "./distribution/feature.test.mjs", &["ts"]),
        TsconfigOutputResolution::ConfiguredWithoutSource,
        "an MJS output must not resolve to an unrelated TS source with the same basename"
    );
    assert_eq!(
        map(root, "./distribution/feature.native.mjs", &["mts"]),
        TsconfigOutputResolution::Resolved(root.join("source/feature.native.mts"))
    );
}

#[test]
fn resolves_installed_package_extends_and_config_dir_options_statically() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("node_modules/@scope/config/package.json"),
        r#"{"exports":"./shared.json"}"#,
    );
    write(
        &root.join("node_modules/@scope/config/shared.json"),
        r#"{
            "compilerOptions": {
                "outDir": "${configDir}/distribution"
            }
        }"#,
    );
    write(
        &root.join("tsconfig.json"),
        r#"{"extends":"@scope/config"}"#,
    );
    write(
        &root.join("tsconfig.build.json"),
        r#"{
            "extends":"./tsconfig.json",
            "compilerOptions":{"rootDir":"./source"}
        }"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
    );
}

#[test]
fn bare_extends_uses_package_lookup_and_dotted_local_stems_append_json() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("configs/tsconfig.base.json"),
        r#"{
            "compilerOptions": {
                "rootDir": "../source",
                "outDir": "../local-output"
            }
        }"#,
    );
    write(
        &root.join("node_modules/shared-config/package.json"),
        r#"{"exports":"./shared.json"}"#,
    );
    write(
        &root.join("node_modules/shared-config/shared.json"),
        r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
    );
    write(
        &root.join("shared-config"),
        r#"{"compilerOptions":{"outDir":"./wrong-output"}}"#,
    );
    write(
        &root.join("tsconfig.local.json"),
        r#"{"extends":"./configs/tsconfig.base"}"#,
    );
    write(
        &root.join("tsconfig.package.json"),
        r#"{
            "extends":"shared-config",
            "compilerOptions":{"rootDir":"./source"}
        }"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    let map = TsconfigOutputMap::from_project(root);
    assert_eq!(
        map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
        "bare package names must resolve through node_modules even with a same-named local file"
    );
    assert_eq!(
        map.resolve_source_for_entry("./local-output/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
        "dotted local config stems should resolve by appending .json"
    );
}

#[test]
fn package_config_fields_and_unexported_subpaths_resolve_json_files() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    let package_root = root.join("node_modules/shared-config");
    for file in [
        "valid.json",
        "dotted.base.json",
        "double.json.json",
        "extensionless",
        "invalid.jsonc",
        "invalid.ts",
    ] {
        write(
            &package_root.join(file),
            r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
        );
    }
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    for use_field in [true, false] {
        for (target, resolves) in [
            ("valid.json", true),
            ("valid", true),
            ("dotted.base", true),
            ("double.json", true),
            ("extensionless", false),
            ("invalid.jsonc", false),
            ("invalid.ts", false),
        ] {
            let (manifest, extends) = if use_field {
                (
                    serde_json::json!({"tsconfig": target}),
                    "shared-config".to_string(),
                )
            } else {
                (serde_json::json!({}), format!("shared-config/{target}"))
            };
            write(&package_root.join("package.json"), &manifest.to_string());
            write(
                &root.join("tsconfig.json"),
                &serde_json::json!({
                    "extends": extends,
                    "compilerOptions": {"rootDir": "./source"}
                })
                .to_string(),
            );
            let expected = if resolves {
                TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
            } else {
                TsconfigOutputResolution::Unconfigured
            };
            assert_eq!(
                map(root, "./distribution/index.js", &["ts"]),
                expected,
                "package config target {target}, tsconfig field present={use_field}"
            );
        }
    }
}

#[test]
fn package_exports_precede_conventional_tsconfig_and_conditions_fail_closed() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("node_modules/shared-config/package.json"),
        r#"{"exports":{".":"./build.json"}}"#,
    );
    write(
        &root.join("node_modules/shared-config/build.json"),
        r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
    );
    write(
        &root.join("node_modules/shared-config/tsconfig.json"),
        r#"{"compilerOptions":{"outDir":"../../other"}}"#,
    );
    write(
        &root.join("node_modules/conditional-config/package.json"),
        r#"{"exports":{".":{"types":"./types.json"}},"tsconfig":"./fallback.json"}"#,
    );
    write(
        &root.join("node_modules/conditional-config/types.json"),
        r#"{"compilerOptions":{"outDir":"../../conditional-output"}}"#,
    );
    write(
        &root.join("node_modules/conditional-config/fallback.json"),
        r#"{"compilerOptions":{"outDir":"../../conditional-output"}}"#,
    );
    write(
        &root.join("tsconfig.build.json"),
        r#"{
            "extends":"shared-config",
            "compilerOptions":{"rootDir":"./source"}
        }"#,
    );
    write(
        &root.join("tsconfig.conditional.json"),
        r#"{
            "extends":"conditional-config",
            "compilerOptions":{"rootDir":"./source"}
        }"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    let map = TsconfigOutputMap::from_project(root);
    assert_eq!(
        map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
        "the simple root export should be selected before conventional tsconfig.json"
    );
    assert_eq!(
        map.resolve_source_for_entry("./other/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured,
        "a simple root export must take precedence over the conventional config"
    );
    assert_eq!(
        map.resolve_source_for_entry("./conditional-output/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured,
        "unsupported conditional package exports must not fall through to package tsconfig"
    );
}

#[test]
fn package_export_targets_require_explicit_config_extensions() {
    for (extends, export_key) in [
        ("shared-config", None),
        ("shared-config", Some(".")),
        ("shared-config/build", Some("./build")),
    ] {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        let package_root = root.join("node_modules/shared-config");
        for file in [
            "build.json",
            "build",
            "build.jsonc",
            "build.ts",
            "inferred.json",
            "double.json.json",
        ] {
            write(
                &package_root.join(file),
                r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
            );
        }
        write(
            &root.join("tsconfig.json"),
            &format!(r#"{{"extends":"{extends}","compilerOptions":{{"rootDir":"./source"}}}}"#),
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        for (target, expected) in [
            ("./build", TsconfigOutputResolution::Unconfigured),
            ("./build.jsonc", TsconfigOutputResolution::Unconfigured),
            ("./build.ts", TsconfigOutputResolution::Unconfigured),
            ("./inferred", TsconfigOutputResolution::Unconfigured),
            ("./double.json", TsconfigOutputResolution::Unconfigured),
            (
                "./build.json",
                TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            ),
        ] {
            let exports = export_key.map_or_else(
                || serde_json::json!(target),
                |key| serde_json::json!({key: target}),
            );
            write(
                &package_root.join("package.json"),
                &serde_json::json!({"exports": exports}).to_string(),
            );
            assert_eq!(
                map(root, "./distribution/index.js", &["ts"]),
                expected,
                "package exports must resolve exactly: {extends} -> {target}"
            );
        }
    }
}

#[test]
fn package_subpaths_resolve_through_simple_exports_without_bypassing_restrictions() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("node_modules/shared-config/package.json"),
        r#"{"exports":{".":"./root.json","./build":"./build.json"}}"#,
    );
    write(
        &root.join("node_modules/shared-config/root.json"),
        r#"{"compilerOptions":{"outDir":"../../wrong-output"}}"#,
    );
    write(
        &root.join("node_modules/shared-config/build.json"),
        r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
    );
    write(
        &root.join("node_modules/shared-config/private.json"),
        r#"{"compilerOptions":{"outDir":"../../private-output"}}"#,
    );
    write(
        &root.join("node_modules/restricted-config/package.json"),
        r#"{"exports":"./root.json"}"#,
    );
    write(
        &root.join("node_modules/restricted-config/root.json"),
        r#"{"compilerOptions":{"outDir":"../../wrong-output"}}"#,
    );
    write(
        &root.join("node_modules/restricted-config/private.json"),
        r#"{"compilerOptions":{"outDir":"../../private-output"}}"#,
    );
    write(
        &root.join("tsconfig.build.json"),
        r#"{
            "extends":"shared-config/build",
            "compilerOptions":{"rootDir":"./source"}
        }"#,
    );
    write(
        &root.join("tsconfig.restricted.json"),
        r#"{
            "extends":"restricted-config/private",
            "compilerOptions":{"rootDir":"./source"}
        }"#,
    );
    write(
        &root.join("tsconfig.private.json"),
        r#"{"extends":"shared-config/private","compilerOptions":{"rootDir":"./source"}}"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    let map = TsconfigOutputMap::from_project(root);
    assert_eq!(
        map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
        "the exact exported subpath should map through the configured build output"
    );
    assert_eq!(
        map.resolve_source_for_entry("./wrong-output/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured,
        "a subpath must not use the package root config"
    );
    assert_eq!(
        map.resolve_source_for_entry("./private-output/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured,
        "an unexported package subpath must not be loaded from disk"
    );
}

#[test]
fn package_subpaths_resolve_through_exports_patterns() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("node_modules/@scope/config/package.json"),
        r#"{"exports":{"./base.json":"./base.json","./*":"./*","./presets/*.json":"./generated/*.json"}}"#,
    );
    write(
        &root.join("node_modules/@scope/config/modern/build.json"),
        r#"{"compilerOptions":{"outDir":"../../../../distribution"}}"#,
    );
    write(
        &root.join("node_modules/@scope/config/generated/strict.json"),
        r#"{"compilerOptions":{"outDir":"../../../../preset-output"}}"#,
    );
    write(
        &root.join("tsconfig.build.json"),
        r#"{"extends":"@scope/config/modern/build.json","compilerOptions":{"rootDir":"./source"}}"#,
    );
    write(
        &root.join("tsconfig.preset.json"),
        r#"{"extends":"@scope/config/presets/strict.json","compilerOptions":{"rootDir":"./source"}}"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    let map = TsconfigOutputMap::from_project(root);
    assert_eq!(
        map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
        "a `./*` pattern exports the subpath"
    );
    assert_eq!(
        map.resolve_source_for_entry("./preset-output/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
        "the pattern with the longest prefix wins"
    );
}

#[test]
fn missing_local_extends_does_not_use_child_options() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("tsconfig.build.json"),
        r#"{
            "extends":"./missing-config.json",
            "compilerOptions": {
                "rootDir":"./source",
                "outDir":"./distribution"
            }
        }"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured
    );
    write(&root.join("missing-config.json"), "{}");
    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
    );
}

#[test]
fn emit_declaration_only_maps_javascript_output_to_source() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("tsconfig.json"),
        r#"{"compilerOptions":{"rootDir":"./src","outDir":"./dist","emitDeclarationOnly":true,"declaration":true}}"#,
    );
    write(&root.join("src/cli.ts"), "export const main = () => 1;\n");

    for output in ["./dist/cli.js", "./dist/cli.d.ts"] {
        assert_eq!(
            map(root, output, &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("src/cli.ts")),
            "{output} maps to source"
        );
    }
    assert_eq!(
        map(root, "./dist/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured,
        "JavaScript output without a source file stays open for other fallbacks"
    );
    assert_eq!(
        map(root, "./dist/index.d.ts", &["ts"]),
        TsconfigOutputResolution::ConfiguredWithoutSource
    );
}

#[test]
fn array_extends_applies_parents_in_order_before_own_options() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("configs/first.json"),
        r#"{"compilerOptions":{"rootDir":"../wrong","outDir":"../distribution"}}"#,
    );
    write(
        &root.join("configs/second.json"),
        r#"{"compilerOptions":{"rootDir":"../source"}}"#,
    );
    write(
        &root.join("tsconfig.build.json"),
        r#"{"extends":["./configs/first.json","./configs/second.json"]}"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
    );
}

#[test]
fn output_path_without_root_dir_maps_from_the_config_folder() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(&root.join("tsconfig.json"), r#"{"compilerOptions":{}}"#);
    write(
        &root.join("tsconfig.build.json"),
        r#"{"extends":["./tsconfig.json"],"compilerOptions":{"outDir":"dist"}}"#,
    );
    write(
        &root.join("nodes/Http/transport.ts"),
        "export const t = 1;\n",
    );

    let output_map = TsconfigOutputMap::from_project(root);
    assert_eq!(
        output_map
            .resolve_source_for_output_path(&root.join("dist/nodes/Http/transport.js"), &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("nodes/Http/transport.ts"))
    );
    assert_eq!(
        output_map.resolve_source_for_output_path(&root.join("dist/missing.js"), &["ts"]),
        TsconfigOutputResolution::Unconfigured,
        "an inferred rootDir does not claim the output"
    );
    assert_eq!(
        map(root, "./dist/nodes/Http/transport.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured,
        "package entries keep the explicit rootDir rule"
    );
}

#[test]
fn explicit_root_dir_wins_over_the_config_folder_default() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("tsconfig.json"),
        r#"{"compilerOptions":{"rootDir":"src","outDir":"dist"}}"#,
    );
    write(
        &root.join("tsconfig.other.json"),
        r#"{"compilerOptions":{"outDir":"dist"}}"#,
    );
    write(&root.join("src/util.ts"), "export const a = 1;\n");
    write(&root.join("util.ts"), "export const b = 1;\n");
    write(&root.join("only-root.ts"), "export const c = 1;\n");

    let output_map = TsconfigOutputMap::from_project(root);
    assert_eq!(
        output_map.resolve_source_for_output_path(&root.join("dist/util.js"), &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("src/util.ts"))
    );
    assert_eq!(
        output_map.resolve_source_for_output_path(&root.join("dist/only-root.js"), &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("only-root.ts")),
        "the config folder default still applies when the explicit rootDir has no source"
    );
}

#[test]
fn invalid_configs_fail_closed() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(&root.join("source/index.ts"), "export const value = 1;\n");
    write(
        &root.join("tsconfig.build.json"),
        "{ this is not valid JSONC",
    );

    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured
    );
    write(
        &root.join("tsconfig.build.json"),
        r#"{"compilerOptions":{"rootDir":"./source","outDir":"./distribution"}}"#,
    );
    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
    );

    let missing = tempfile::tempdir().expect("empty temporary project");
    assert_eq!(
        map(missing.path(), "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured
    );
}

/// Run `map` on a separate thread and fail when it does not finish in time.
fn map_within(
    root: &Path,
    entry: &'static str,
    limit: std::time::Duration,
) -> TsconfigOutputResolution {
    let root = root.to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(map(&root, entry, &["ts"]));
    });
    receiver
        .recv_timeout(limit)
        .expect("the extends walk must finish within the time limit")
}

#[test]
fn wide_array_extends_diamond_reads_each_config_once() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write_extends_diamond(root, 8, 13, 1, 0);

    assert_eq!(
        map_within(
            root,
            "./distribution/index.js",
            std::time::Duration::from_secs(10)
        ),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
        "the last extends entry of each level must win"
    );
}

#[test]
fn repeated_array_extends_entries_are_followed_once() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write_extends_diamond(root, 8, 12, 400, 0);

    assert_eq!(
        map_within(
            root,
            "./distribution/index.js",
            std::time::Duration::from_secs(5)
        ),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
    );
}

#[test]
fn extends_walk_over_the_entry_limit_fails_closed() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("configs/base.json"),
        r#"{"compilerOptions":{"rootDir":"../source","outDir":"../distribution"}}"#,
    );
    // Each specifier is a different string for the same config.
    let parents = (0..=MAX_EXTENDS_ENTRIES)
        .map(|index| format!(r#""./configs/folder-{index}/../base.json""#))
        .collect::<Vec<_>>()
        .join(",");
    write(
        &root.join("tsconfig.build.json"),
        &format!(r#"{{"extends":[{parents}]}}"#),
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured
    );
}

#[test]
fn root_config_over_a_limit_does_not_disable_other_root_configs() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("configs/base.json"),
        r#"{"compilerOptions":{"rootDir":"../code","outDir":"../output"}}"#,
    );
    write(
        &root.join("configs/shared.json"),
        r#"{"extends":"./base.json"}"#,
    );
    write(&root.join("configs/leaf.json"), "{}");
    // `tsconfig.a.json` sorts before `tsconfig.json`. Its own entries use
    // the whole entry budget, so the walk goes over the limit inside the
    // shared config.
    let parents = (0..MAX_EXTENDS_ENTRIES - 1)
        .map(|index| format!(r#""./configs/folder-{index}/../leaf.json""#))
        .chain(std::iter::once(r#""./configs/shared.json""#.to_owned()))
        .collect::<Vec<_>>()
        .join(",");
    write(
        &root.join("tsconfig.a.json"),
        &format!(r#"{{"extends":[{parents}]}}"#),
    );
    write(
        &root.join("tsconfig.json"),
        r#"{"extends":"./configs/shared.json"}"#,
    );
    write(&root.join("code/index.ts"), "export const value = 1;\n");

    let output_map = TsconfigOutputMap::from_project(root);
    assert_eq!(
        output_map.configs.len(),
        1,
        "only the root config over the limit fails closed"
    );
    assert_eq!(
        output_map.resolve_source_for_entry("./output/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("code/index.ts"))
    );
}

#[test]
fn repeated_extends_entry_keeps_its_last_place() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("configs/first.json"),
        r#"{"compilerOptions":{"rootDir":"../source","outDir":"../distribution"}}"#,
    );
    write(
        &root.join("configs/second.json"),
        r#"{"compilerOptions":{"rootDir":"../wrong"}}"#,
    );
    write(
        &root.join("tsconfig.build.json"),
        r#"{"extends":["./configs/first.json","./configs/second.json","./configs/first.json"]}"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
    );
}

#[test]
fn many_root_configs_over_one_extends_graph_share_the_walk() {
    const EXTRA_ROOTS: usize = 40;
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path().to_path_buf();
    write_extends_diamond(&root, 8, 12, 10, EXTRA_ROOTS);

    let (sender, receiver) = std::sync::mpsc::channel();
    let walk_root = root.clone();
    std::thread::spawn(move || {
        let _ = sender.send(TsconfigOutputMap::from_project(&walk_root));
    });
    let output_map = receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the extends walk must finish within the time limit");
    assert_eq!(
        output_map.configs.len(),
        EXTRA_ROOTS + 1,
        "every root config must stay within the shared walk limits"
    );
    assert_eq!(
        output_map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
    );
}

/// Write a project where `tsconfig.build.json` and `extra_roots` other
/// root configs extend a diamond of `levels` levels of `width` configs.
/// Every config of a level extends every config of the next level, each
/// entry `duplicates` times. Only the last config of the last level sets
/// the correct `rootDir`.
fn write_extends_diamond(
    root: &Path,
    width: usize,
    levels: usize,
    duplicates: usize,
    extra_roots: usize,
) {
    for level in 0..levels {
        for index in 0..width {
            let content = if level + 1 == levels {
                let root_dir = if index + 1 == width {
                    "../source"
                } else {
                    "../wrong"
                };
                format!(
                    r#"{{"compilerOptions":{{"rootDir":"{root_dir}","outDir":"../distribution"}}}}"#
                )
            } else {
                let parents = (0..duplicates)
                    .flat_map(|_| 0..width)
                    .map(|parent| format!(r#""./level-{}-{parent}.json""#, level + 1))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"extends":[{parents}]}}"#)
            };
            write(
                &root.join(format!("configs/level-{level}-{index}.json")),
                &content,
            );
        }
    }
    let top = (0..width)
        .map(|index| format!(r#""./configs/level-0-{index}.json""#))
        .collect::<Vec<_>>()
        .join(",");
    let top = format!(r#"{{"extends":[{top}]}}"#);
    write(&root.join("tsconfig.build.json"), &top);
    for extra in 0..extra_roots {
        write(&root.join(format!("tsconfig.extra-{extra}.json")), &top);
    }
    write(&root.join("source/index.ts"), "export const value = 1;\n");
}

#[test]
fn extends_walk_over_the_read_limit_fails_closed() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    let parents = (0..=MAX_EXTENDS_READS)
        .map(|index| {
            write(
                &root.join(format!("configs/base-{index}.json")),
                r#"{"compilerOptions":{"rootDir":"../source","outDir":"../distribution"}}"#,
            );
            format!(r#""./configs/base-{index}.json""#)
        })
        .collect::<Vec<_>>()
        .join(",");
    write(
        &root.join("tsconfig.build.json"),
        &format!(r#"{{"extends":[{parents}]}}"#),
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured
    );
}

#[test]
fn shared_parent_in_two_extends_branches_is_not_a_cycle() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory.path();
    write(
        &root.join("configs/shared.json"),
        r#"{"compilerOptions":{"rootDir":"../source","outDir":"../distribution"}}"#,
    );
    write(
        &root.join("configs/first.json"),
        r#"{"extends":"./shared.json","compilerOptions":{"rootDir":"../wrong"}}"#,
    );
    write(
        &root.join("configs/second.json"),
        r#"{"extends":"./shared.json"}"#,
    );
    write(
        &root.join("tsconfig.build.json"),
        r#"{"extends":["./configs/first.json","./configs/second.json"]}"#,
    );
    write(&root.join("source/index.ts"), "export const value = 1;\n");

    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
        "the second branch re-applies the shared parent over the first branch"
    );

    write(
        &root.join("configs/shared.json"),
        r#"{"extends":"../tsconfig.build.json"}"#,
    );
    assert_eq!(
        map(root, "./distribution/index.js", &["ts"]),
        TsconfigOutputResolution::Unconfigured,
        "a real cycle still fails closed"
    );
}
