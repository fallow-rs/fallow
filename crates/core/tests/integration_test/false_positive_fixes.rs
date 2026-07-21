use super::common::{create_config, fixture_path};

#[test]
fn eslint_relative_extends_config_is_not_reported_unused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::create_dir_all(root.join("config")).expect("config dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "eslint-chain",
            "private": true,
            "devDependencies": {
                "eslint": "8.57.0",
                "@typescript-eslint/parser": "7.0.0",
                "eslint-config-prettier": "9.1.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "compilerOptions": {
                "target": "ES2022",
                "module": "ES2022",
                "moduleResolution": "bundler",
                "strict": true,
                "skipLibCheck": true
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join(".eslintrc.json"),
        r#"{ "root": true, "extends": ["./config/eslintrc.base.js"] }"#,
    )
    .expect("eslint root config");
    std::fs::write(
        root.join("config/eslintrc.base.js"),
        r#"module.exports = {
            extends: ["prettier"],
            overrides: [
                { files: ["*.ts"], parser: "@typescript-eslint/parser", rules: {} }
            ]
        };"#,
    )
    .expect("eslint base config");
    std::fs::write(root.join("src/index.ts"), "export const hello = 'world';")
        .expect("source file");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "config/eslintrc.base.js"),
        "ESLint base config reached through relative extends should be used, got: {unused_files:?}"
    );

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dev_dependencies.contains(&"@typescript-eslint/parser"),
        "override parser should be credited through the ESLint config chain: {unused_dev_dependencies:?}"
    );
    assert!(
        !unused_dev_dependencies.contains(&"eslint-config-prettier"),
        "extends package should be credited through the ESLint config chain: {unused_dev_dependencies:?}"
    );
}

#[test]
fn type_only_bidirectional_import_not_reported_as_cycle() {
    let root = fixture_path("type-only-cycle");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.circular_dependencies.is_empty(),
        "type-only bidirectional imports should not be reported as circular dependencies, got: {:?}",
        results
            .circular_dependencies
            .iter()
            .map(|cd| &cd.cycle.files)
            .collect::<Vec<_>>()
    );
}

#[test]
fn type_only_cycle_still_detects_unused_exports() {
    let root = fixture_path("type-only-cycle");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert!(
        !unused_file_names.contains(&"user.ts".to_string()),
        "user.ts should not be unused, got: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"post.ts".to_string()),
        "post.ts should not be unused, got: {unused_file_names:?}"
    );
}

#[test]
fn unrelated_route_files_not_flagged_as_duplicate_exports() {
    let root = fixture_path("route-duplicate-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let route_dupes: Vec<&str> = results
        .duplicate_exports
        .iter()
        .filter(|d| d.export.export_name == "Area" || d.export.export_name == "handler")
        .map(|d| d.export.export_name.as_str())
        .collect();
    assert!(
        route_dupes.is_empty(),
        "route files with separate importers should not be flagged as duplicates, got: {route_dupes:?}"
    );
}

#[test]
fn shared_util_duplicates_with_common_importer_still_flagged() {
    let root = fixture_path("route-duplicate-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let format_date_dupe = results
        .duplicate_exports
        .iter()
        .find(|d| d.export.export_name == "formatDate");
    assert!(
        format_date_dupe.is_some(),
        "formatDate in shared files with common importer should be flagged, got dupes: {:?}",
        results
            .duplicate_exports
            .iter()
            .map(|d| &d.export.export_name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn broken_tsconfig_extends_does_not_poison_sibling_resolution() {
    let root = fixture_path("tsconfig-broken-extends");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "broken sibling tsconfig should not poison resolution for files covered \
         by a valid sibling; got unresolved imports: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|u| (u.import.path.display().to_string(), &u.import.specifier))
            .collect::<Vec<_>>()
    );
}

#[test]
fn broken_tsconfig_path_alias_is_not_misclassified_as_unlisted_dependency() {
    let root = fixture_path("tsconfig-broken-path-alias").join("app");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();

    assert!(
        !unlisted_names.contains(&"@gen/foo"),
        "@gen/foo is a declared tsconfig path alias and should not be treated as an unlisted dependency: {unlisted_names:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"@gen/foo"),
        "@gen/foo points outside the analysis root and should remain unresolved when the tsconfig chain is broken: {unresolved_specifiers:?}"
    );
}

#[test]
fn unresolved_tsconfig_path_alias_is_not_misclassified_as_unlisted_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "alias-unlisted-regression",
            "private": true,
            "main": "src/index.ts",
            "dependencies": {
                "react": "18.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@app/*": ["src/app/*"]
                }
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { missing } from "@app/missing";
import { external } from "@scope/pkg";
import React from "react";

console.log(missing, external, React);
"#,
    )
    .expect("index");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();

    assert!(
        !unlisted_names.contains(&"@app/missing"),
        "declared tsconfig path aliases should not be treated as unlisted dependencies: {unlisted_names:?}"
    );
    assert!(
        !unlisted_names.contains(&"react"),
        "real listed packages should still receive dependency credit: {unlisted_names:?}"
    );
    assert!(
        unlisted_names.contains(&"@scope/pkg"),
        "real unlisted scoped packages should still be reported in a tsconfig path project: {unlisted_names:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"@app/missing"),
        "missing local alias targets should stay unresolved instead of becoming package deps: {unresolved_specifiers:?}"
    );
}

#[test]
fn bun_bare_runtime_module_is_not_unlisted() {
    let root = fixture_path("issue-642-bun-builtin");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"bun"),
        "bare `bun` should resolve to a platform builtin: {unresolved_specifiers:?}"
    );

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted_names.contains(&"bun"),
        "bare `bun` value and type imports must not surface as unlisted dependencies: {unlisted_names:?}"
    );
}

#[test]
fn bun_test_discovery_uses_default_patterns_scoped_by_test_root() {
    let root = fixture_path("issue-951-bun-test-discovery");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    for used in [
        "test/unit/example.test.ts",
        "test/unit/smoke_spec.ts",
        "test/unit/helpers/helper.ts",
        "test/setup.ts",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == used),
            "{used} should be reachable through Bun test discovery or preload: {unused_files:?}"
        );
    }

    for still_unused in [
        "test/outside/outside.test.ts",
        "test/unit/helpers/orphan.ts",
    ] {
        assert!(
            unused_files.iter().any(|path| path == still_unused),
            "{still_unused} should remain reportable: {unused_files:?}"
        );
    }
}

#[test]
fn wildcard_tsconfig_paths_do_not_misclassify_bare_imports() {
    let root = fixture_path("issue-327-wildcard-paths-node-builtins");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"node:url"),
        "`node:url` should resolve to a platform builtin even when tsconfig wildcard paths are configured: {unresolved_specifiers:?}"
    );
    assert!(
        !unresolved_specifiers.contains(&"fs"),
        "bare `fs` should resolve to a platform builtin even when tsconfig wildcard paths are configured: {unresolved_specifiers:?}"
    );
    assert!(
        !unresolved_specifiers.contains(&"bun:sqlite"),
        "`bun:sqlite` should resolve to a platform builtin even when tsconfig wildcard paths are configured: {unresolved_specifiers:?}"
    );
    assert!(
        !unresolved_specifiers.contains(&"cloudflare:sockets"),
        "`cloudflare:sockets` should resolve to a platform builtin even when tsconfig wildcard paths are configured: {unresolved_specifiers:?}"
    );
    assert!(
        !unresolved_specifiers.contains(&"doesnotexist"),
        "bare package typos should remain dependency findings, not unresolved imports: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.is_empty(),
        "no imports should be unresolved in the wildcard-paths fixture: {unresolved_specifiers:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files.iter().any(|path| path == "src/helpers.ts"),
        "`./src/helpers.ts` must stay reachable through the `*` -> `./src/*` rewrite after the fix: {unused_files:?}"
    );

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted_names.contains(&"node:url"),
        "platform builtins must not surface as unlisted dependencies: {unlisted_names:?}"
    );
    assert!(
        !unlisted_names.contains(&"fs"),
        "platform builtins must not surface as unlisted dependencies: {unlisted_names:?}"
    );
    assert!(
        !unlisted_names.contains(&"bun:sqlite"),
        "platform builtins must not surface as unlisted dependencies: {unlisted_names:?}"
    );
    assert!(
        !unlisted_names.contains(&"cloudflare:sockets"),
        "platform builtins must not surface as unlisted dependencies: {unlisted_names:?}"
    );
    assert!(
        unlisted_names.contains(&"doesnotexist"),
        "bare package typos should still surface as unlisted dependencies: {unlisted_names:?}"
    );
}

#[test]
fn missing_react_native_extends_keeps_local_tsconfig_path_aliases() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/components")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "react-native-missing-extends",
            "private": true,
            "main": "src/index.ts",
            "dependencies": {
                "react-native": "0.80.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./node_modules/@react-native/typescript-config/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@/*": ["src/*"]
                }
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { Button } from "@/components/Button";
export const app = Button;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("src/components/Button.web.ts"),
        "export const Button = 'button';\n",
    )
    .expect("button");
    std::fs::write(
        root.join("src/components/Button.ts"),
        "export const Button = 'generic';\n",
    )
    .expect("generic button");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"@/components/Button"),
        "local tsconfig paths should survive a missing React Native base config: {unresolved_specifiers:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "src/components/Button.web.ts"),
        "React Native platform alias target should stay reachable: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path.ends_with("src/components/Button.ts")),
        "React Native platform target should win over the generic target: {unused_files:?}"
    );
}

#[test]
fn missing_react_native_extends_resolves_explicit_js_alias_to_platform_source() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/components")).expect("components dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "react-native-explicit-js-alias",
            "private": true,
            "main": "src/index.ts",
            "dependencies": {
                "react-native": "0.80.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./node_modules/@react-native/typescript-config/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@/*": ["src/*"]
                }
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { Button } from "@/components/Button.js";
export const app = Button;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("src/components/Button.ios.ts"),
        "export const Button = 'ios';\n",
    )
    .expect("ios button");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "explicit .js aliases should probe React Native platform source files: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("src/components/Button.ios.ts")),
        "platform source target should stay reachable: {unused_files:?}"
    );
}

#[test]
fn missing_expo_extends_keeps_local_tsconfig_path_aliases() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("app/screens")).expect("app dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "expo-missing-extends",
            "private": true,
            "main": "app/index.tsx",
            "dependencies": {
                "expo": "53.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./node_modules/expo/tsconfig.base",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@app/*": ["app/*"]
                }
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("app/index.tsx"),
        r#"import { Screen } from "@app/screens/Screen";
export const app = Screen;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("app/screens/Screen.native.tsx"),
        "export const Screen = 'native';\n",
    )
    .expect("native screen");
    std::fs::write(
        root.join("app/screens/Screen.tsx"),
        "export const Screen = 'generic';\n",
    )
    .expect("generic screen");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "Expo projects should get the same broken-extends path alias fallback as React Native: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "app/screens/Screen.native.tsx"),
        "Expo platform alias target should stay reachable: {unused_files:?}"
    );
}

#[test]
fn tsconfig_alias_fallback_does_not_probe_platform_files_without_mobile_plugins() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/components")).expect("components dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "web-missing-extends",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@/*": ["src/*"]
                }
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { Button } from "@/components/Button";
export const app = Button;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("src/components/Button.web.ts"),
        "export const Button = 'web';\n",
    )
    .expect("web button");
    std::fs::write(
        root.join("src/components/Button.ts"),
        "export const Button = 'generic';\n",
    )
    .expect("generic button");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "plain TS projects should still resolve the alias: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "src/components/Button.ts"),
        "generic target should stay reachable without mobile plugins: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path.ends_with("src/components/Button.web.ts")),
        "platform-specific files should not shadow generic files without React Native or Expo: {unused_files:?}"
    );
}

#[test]
fn missing_extends_keeps_local_tsconfig_paths_without_base_url() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/features/nested")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-local-paths",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r##"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "paths": {
                    "$env": ["src/env.ts"],
                    "@features/*": ["src/features/*"],
                    "#theme": ["missing/theme.ts", "src/theme.ts"],
                    "$api": ["src/api.js"],
                    "*": ["src/catchall/*"],
                },
            },
        }"##,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r##"import { env } from "$env";
import { feature } from "@features/home";
import { nested } from "@features/nested";
import { theme } from "#theme";
import { api } from "$api";
import { shared } from "shared";

export const app = [env, feature, nested, theme, api, shared].join("-");
"##,
    )
    .expect("index");
    std::fs::write(root.join("src/env.ts"), "export const env = 'env';\n").expect("env");
    std::fs::write(root.join("src/api.ts"), "export const api = 'api';\n").expect("api");
    std::fs::write(
        root.join("src/features/home.ts"),
        "export const feature = 'home';\n",
    )
    .expect("home");
    std::fs::write(
        root.join("src/features/nested/index.ts"),
        "export const nested = 'nested';\n",
    )
    .expect("nested index");
    std::fs::create_dir_all(root.join("src/catchall")).expect("catchall dir");
    std::fs::write(
        root.join("src/catchall/shared.ts"),
        "export const shared = 'shared';\n",
    )
    .expect("shared");
    std::fs::write(root.join("src/theme.ts"), "export const theme = 'theme';\n").expect("theme");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "exact aliases, wildcard aliases, fallback targets, and directory indexes should resolve: {:?}",
        results.unresolved_imports
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    for expected_used in [
        "src/env.ts",
        "src/features/home.ts",
        "src/features/nested/index.ts",
        "src/theme.ts",
        "src/api.ts",
        "src/catchall/shared.ts",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == expected_used),
            "{expected_used} should stay reachable through local tsconfig paths: {unused_files:?}"
        );
    }
}

#[test]
fn missing_extends_resolves_aliases_for_all_import_edge_shapes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/lib")).expect("lib dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-alias-edge-shapes",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@lib/*": ["src/lib/*"]
                }
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { staticValue } from "@lib/static";
export { namedValue } from "@lib/named";
export * from "@lib/star";

const { requiredValue } = require("@lib/required");
void import("@lib/dynamic");

export const app = [staticValue, requiredValue].join("-");
"#,
    )
    .expect("index");
    for (name, source) in [
        ("static.ts", "export const staticValue = 'static';\n"),
        ("named.ts", "export const namedValue = 'named';\n"),
        ("star.ts", "export const starValue = 'star';\n"),
        ("required.ts", "export const requiredValue = 'required';\n"),
        ("dynamic.ts", "export const dynamicValue = 'dynamic';\n"),
    ] {
        std::fs::write(root.join("src/lib").join(name), source).expect("lib file");
    }

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "all alias edge shapes should resolve under a broken extends chain: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    for expected_used in [
        "src/lib/static.ts",
        "src/lib/named.ts",
        "src/lib/star.ts",
        "src/lib/required.ts",
        "src/lib/dynamic.ts",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == expected_used),
            "{expected_used} should stay reachable through alias fallback: {unused_files:?}"
        );
    }
}

#[test]
fn missing_extends_prefers_local_alias_over_node_modules_fallback() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/shared")).expect("shared src dir");
    std::fs::create_dir_all(root.join("node_modules/shared")).expect("shared package dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-alias-over-package",
            "private": true,
            "main": "src/index.ts",
            "dependencies": {
                "shared": "1.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "shared/*": ["src/shared/*"]
                }
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { value } from "shared/value";
export const app = value;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("src/shared/value.ts"),
        "export const value = 'local';\n",
    )
    .expect("local shared value");
    std::fs::write(
        root.join("node_modules/shared/value.js"),
        "exports.value = 'package';\n",
    )
    .expect("package shared value");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "local tsconfig alias should resolve before resolver-less node_modules fallback: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("src/shared/value.ts")),
        "local alias target should stay reachable: {unused_files:?}"
    );
}

#[test]
fn missing_extends_keeps_local_base_url_without_paths() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/utils")).expect("utils dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-base-url",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "baseUrl": "src"
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { date } from "utils/date";
export const app = date;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("src/utils/date.ts"),
        "export const date = 'date';\n",
    )
    .expect("date");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted_names.contains(&"utils"),
        "local baseUrl imports should not become unlisted dependencies: {unlisted_names:?}"
    );
    assert!(
        results.unresolved_imports.is_empty(),
        "local baseUrl imports should resolve under a broken extends chain: {:?}",
        results.unresolved_imports
    );
}

#[test]
fn missing_extends_prefers_specific_tsconfig_path_aliases() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/components")).expect("components dir");
    std::fs::create_dir_all(root.join("src/wild/components")).expect("wild dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-specific-paths",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@/*": ["src/wild/*"],
                    "@/components/*": ["src/components/*"]
                }
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { Button } from "@/components/Button";
export const app = Button;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("src/components/Button.ts"),
        "export const Button = 'specific';\n",
    )
    .expect("specific");
    std::fs::write(
        root.join("src/wild/components/Button.ts"),
        "export const Button = 'wild';\n",
    )
    .expect("wild");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "src/components/Button.ts"),
        "specific alias target should be reachable: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path.ends_with("src/wild/components/Button.ts")),
        "broad alias target should not shadow the specific alias: {unused_files:?}"
    );
}

#[test]
fn missing_extends_keeps_parent_tsconfig_path_aliases() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("apps/mobile/src")).expect("app dir");
    std::fs::create_dir_all(root.join("shared")).expect("shared dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-parent-paths",
            "private": true,
            "main": "apps/mobile/src/App.tsx",
            "dependencies": {
                "react-native": "0.80.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.base.json"),
        r#"{
            "extends": "./node_modules/@react-native/typescript-config/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@shared/*": ["shared/*"]
                }
            }
        }"#,
    )
    .expect("base tsconfig");
    std::fs::write(
        root.join("apps/mobile/tsconfig.json"),
        r#"{
            "extends": "../../tsconfig.base.json"
        }"#,
    )
    .expect("app tsconfig");
    std::fs::write(
        root.join("apps/mobile/src/App.tsx"),
        r#"import { theme } from "@shared/theme";
export const app = theme;
"#,
    )
    .expect("app");
    std::fs::write(
        root.join("shared/theme.ts"),
        "export const theme = 'theme';\n",
    )
    .expect("theme");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "parent tsconfig path aliases should resolve under a broken extends chain: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files.iter().any(|path| path == "shared/theme.ts"),
        "parent alias target should stay reachable: {unused_files:?}"
    );
}

#[test]
fn missing_extends_keeps_referenced_tsconfig_path_aliases() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/utils")).expect("utils dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-referenced-paths",
            "private": true,
            "main": "src/main.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "files": [],
            "references": [
                { "path": "./tsconfig.app.json" },
                { "path": "./tsconfig.spec.json" }
            ]
        }"#,
    )
    .expect("solution tsconfig");
    std::fs::write(
        root.join("tsconfig.app.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@/*": ["src/*"]
                }
            },
            "include": ["src/**/*.ts"]
        }"#,
    )
    .expect("app tsconfig");
    std::fs::write(
        root.join("tsconfig.spec.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@/*": ["spec/*"]
                }
            },
            "include": ["src/**/*.spec.ts"]
        }"#,
    )
    .expect("spec tsconfig");
    std::fs::write(
        root.join("src/main.ts"),
        r#"import { message } from "@/utils/message";
export const app = message;
"#,
    )
    .expect("main");
    std::fs::write(
        root.join("src/utils/message.ts"),
        "export const message = 'hello';\n",
    )
    .expect("message");
    std::fs::create_dir_all(root.join("spec/utils")).expect("spec utils dir");
    std::fs::write(
        root.join("spec/utils/message.ts"),
        "export const message = 'spec';\n",
    )
    .expect("spec message");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "referenced tsconfig path aliases should survive a broken extends chain: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "src/utils/message.ts"),
        "app reference alias target should stay reachable: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path.ends_with("spec/utils/message.ts")),
        "non-matching referenced tsconfig should not shadow the app alias: {unused_files:?}"
    );
}

// Since issue #1911 the TypeScript plugin activates on the presence of a
// tsconfig (no `typescript` dependency required) and registers the
// `compilerOptions.paths` of EVERY discovered tsconfig project-wide (the
// documented liberal union; it does not model TypeScript's per-file `paths`
// override semantics). A broken `extends` chain still does not break the child's
// own path resolution, and the base config's `@base/*` is additionally credited.
#[test]
fn missing_extends_keeps_child_path_alias_resolution() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/child")).expect("child dir");
    std::fs::create_dir_all(root.join("src/base")).expect("base dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-child-paths",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.base.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@base/*": ["src/base/*"]
                }
            }
        }"#,
    )
    .expect("base tsconfig");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./tsconfig.base.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@child/*": ["src/child/*"]
                }
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { child } from "@child/value";
import { base } from "@base/value";

export const app = child + base;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("src/child/value.ts"),
        "export const child = 'child';\n",
    )
    .expect("child value");
    std::fs::write(
        root.join("src/base/value.ts"),
        "export const base = 'base';\n",
    )
    .expect("base value");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"@child/value"),
        "child paths should resolve: {unresolved_specifiers:?}"
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("src/child/value.ts")),
        "child alias target should stay reachable: {unused_files:?}"
    );
    // With the TypeScript plugin now active on tsconfig presence (issue #1911),
    // the base config's `@base/*` alias is registered project-wide, so
    // `@base/value` resolves and its target stays reachable. This is the
    // false-negative direction (an alias TypeScript would treat as overridden
    // stays reachable), the accepted trade-off for eliminating the #1911
    // unlisted-dependency false positive.
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("src/base/value.ts")),
        "base alias target resolves via the plugin's project-wide tsconfig paths: {unused_files:?}"
    );
}

#[test]
fn missing_extends_keeps_inherited_root_dirs_resolution() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/features")).expect("src dir");
    std::fs::create_dir_all(root.join("generated/features")).expect("generated dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-inherited-root-dirs",
            "private": true,
            "main": "src/features/view.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.base.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "rootDirs": ["src", "generated"]
            }
        }"#,
    )
    .expect("base tsconfig");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./tsconfig.base.json",
            "compilerOptions": {
                "strict": true
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/features/view.ts"),
        r#"import { generated } from "./view.generated";
export const view = generated;
"#,
    )
    .expect("view");
    std::fs::write(
        root.join("generated/features/view.generated.ts"),
        "export const generated = 'generated';\n",
    )
    .expect("generated");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "inherited rootDirs should resolve under a broken extends chain: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("generated/features/view.generated.ts")),
        "inherited rootDirs target should stay reachable: {unused_files:?}"
    );
}

#[test]
fn missing_package_extends_keeps_inherited_path_aliases() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("node_modules/@repo/tsconfig"))
        .expect("package tsconfig dir");
    std::fs::create_dir_all(root.join("shared")).expect("shared dir");
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-package-extends",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("node_modules/@repo/tsconfig/base.json"),
        r#"{
            "extends": "./missing.json",
            "compilerOptions": {
                "baseUrl": "../../..",
                "paths": {
                    "@shared/*": ["shared/*"]
                }
            }
        }"#,
    )
    .expect("package base tsconfig");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "@repo/tsconfig/base"
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { value } from "@shared/value";
export const app = value;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("shared/value.ts"),
        "export const value = 'shared';\n",
    )
    .expect("shared value");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "package-style tsconfig extends should be followed for local fallback: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files.iter().any(|path| path == "shared/value.ts"),
        "package-extended alias target should stay reachable: {unused_files:?}"
    );
}

#[test]
fn missing_extends_array_keeps_local_path_aliases() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/lib")).expect("lib dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-array",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.paths.json"),
        r#"{
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@lib/*": ["src/lib/*"]
                }
            }
        }"#,
    )
    .expect("paths tsconfig");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": [
                "./tsconfig.paths.json",
                "./node_modules/@scope/missing/tsconfig.json"
            ]
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { value } from "@lib/value";
export const app = value;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("src/lib/value.ts"),
        "export const value = 'lib';\n",
    )
    .expect("lib value");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "extends arrays should preserve local path aliases when another base is missing: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files.iter().any(|path| path == "src/lib/value.ts"),
        "extends-array alias target should stay reachable: {unused_files:?}"
    );
}

#[test]
fn missing_extends_resolves_path_alias_package_directory_targets() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("packages/pkg/src")).expect("pkg dir");
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-package-dir-target",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@pkg": ["packages/pkg"]
                }
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/index.ts"),
        r#"import { pkg } from "@pkg";
export const app = pkg;
"#,
    )
    .expect("index");
    std::fs::write(
        root.join("packages/pkg/package.json"),
        r#"{
            "name": "@local/pkg",
            "module": "src/index.ts"
        }"#,
    )
    .expect("pkg package json");
    std::fs::write(
        root.join("packages/pkg/src/index.ts"),
        "export const pkg = 'pkg';\n",
    )
    .expect("pkg index");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "path alias directory package targets should resolve: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("packages/pkg/src/index.ts")),
        "package directory alias target should stay reachable: {unused_files:?}"
    );
}

#[test]
fn missing_extends_keeps_local_root_dirs_resolution() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/features")).expect("src dir");
    std::fs::create_dir_all(root.join("generated/features")).expect("generated dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-extends-root-dirs",
            "private": true,
            "main": "src/features/view.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "extends": "./node_modules/@scope/missing/tsconfig.json",
            "compilerOptions": {
                "rootDirs": ["src", "generated"]
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/features/view.ts"),
        r#"import { generated } from "./view.generated";
export const view = generated;
"#,
    )
    .expect("view");
    std::fs::write(
        root.join("generated/features/view.generated.ts"),
        "export const generated = 'generated';\n",
    )
    .expect("generated");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "rootDirs relative imports should resolve under a broken extends chain: {:?}",
        results.unresolved_imports
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("generated/features/view.generated.ts")),
        "rootDirs target should stay reachable: {unused_files:?}"
    );
}

#[test]
fn glimmer_typescript_imports_use_tsconfig_path_aliases() {
    let root = fixture_path("glimmer-path-aliases");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_paths: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert!(
        !unused_file_paths
            .iter()
            .any(|path| path.ends_with("app/services/my-service.ts")),
        "multi-template .gts imports should resolve tsconfig path aliases and keep my-service.ts reachable: \
         {unused_file_paths:?}"
    );
    assert!(
        unused_file_paths
            .iter()
            .any(|path| path.ends_with("app/services/unused-service.ts")),
        "the fixture should still report genuinely unused services: {unused_file_paths:?}"
    );
    assert!(
        results.unresolved_imports.is_empty(),
        ".gts tsconfig path alias imports should not be unresolved: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|import| &import.import.specifier)
            .collect::<Vec<_>>()
    );
}

#[test]
fn interface_member_usage_does_not_flag_implementer_members() {
    let root = fixture_path("interface-member-usage");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect();

    assert!(
        !unused_members.contains(&"FixedSizeScrollStrategy.attached".to_string()),
        "attached should be credited through interface-typed access: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"FixedSizeScrollStrategy.attach".to_string()),
        "attach should be credited through interface-typed access: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"FixedSizeScrollStrategy.detach".to_string()),
        "detach should be credited through interface-typed access: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"FixedSizeScrollStrategy.unusedHelper".to_string()),
        "unrelated members should still be reported: {unused_members:?}"
    );
}

#[test]
fn prisma_config_ts_is_recognized_as_entry_point() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("prisma")).expect("prisma dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "prisma-config-entry",
            "private": true,
            "devDependencies": {
                "prisma": "6.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("prisma/schema.prisma"),
        "generator client { provider = \"prisma-client-js\" }\n",
    )
    .expect("schema.prisma");
    std::fs::write(
        root.join("prisma.config.ts"),
        r#"import { defineConfig } from "prisma/config";

export default defineConfig({
    schema: "prisma/schema.prisma",
});
"#,
    )
    .expect("prisma.config.ts");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused.iter().any(|p| p.ends_with("prisma.config.ts")),
        "prisma.config.ts is the Prisma 6.x config-file location and should not be reported \
         as unused. Got: {unused:?}"
    );
}

#[test]
fn prisma_dot_config_schema_folder_credits_configured_generators_only() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join(".config")).expect(".config dir");
    std::fs::create_dir_all(root.join("db/schema/nested")).expect("schema dir");
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "prisma-dot-config-schema-folder",
            "private": true,
            "dependencies": {
                "@prisma/client": "6.0.0"
            },
            "devDependencies": {
                "prisma": "6.0.0",
                "prisma-json-types-generator": "3.0.0",
                "prisma-erd-generator": "2.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join(".config/prisma.ts"),
        r#"export default {
    schema: "../db/schema",
};
"#,
    )
    .expect("prisma config");
    std::fs::write(
        root.join("db/schema/generator.prisma"),
        r#"generator client {
  provider = "prisma-client-js"
}

generator json {
  provider = "prisma-json-types-generator"
}
"#,
    )
    .expect("generator schema");
    std::fs::write(
        root.join("db/schema/nested/model.prisma"),
        "model User {\n  id Int @id\n}\n",
    )
    .expect("nested model schema");
    std::fs::write(
        root.join("db/other.prisma"),
        r#"generator erd {
  provider = "prisma-erd-generator"
}
"#,
    )
    .expect("unconfigured schema");
    std::fs::write(
        root.join("src/index.ts"),
        "import { PrismaClient } from '@prisma/client';\nexport const db = new PrismaClient();\n",
    )
    .expect("entry");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler"},"include":["src/**/*"]}"#,
    )
    .expect("tsconfig");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev: Vec<String> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect();
    assert!(
        !unused_dev.contains(&"prisma-json-types-generator".to_owned()),
        "generator provider from schema configured by .config/prisma.ts should be credited. \
         unused_dev={unused_dev:?}"
    );
    assert!(
        unused_dev.contains(&"prisma-erd-generator".to_owned()),
        "generator provider outside the configured schema folder should not be credited. \
         unused_dev={unused_dev:?}"
    );
}

#[test]
fn prisma_custom_generator_provider_is_credited() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("prisma")).expect("prisma dir");
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "prisma-custom-gen",
            "private": true,
            "dependencies": {
                "@prisma/client": "6.0.0"
            },
            "devDependencies": {
                "prisma": "6.0.0",
                "prisma-json-types-generator": "3.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("prisma/schema.prisma"),
        r#"generator client {
  provider = "prisma-client-js"
}

generator json {
  provider = "prisma-json-types-generator"
}

datasource db {
  provider = "postgresql"
  url      = env("DATABASE_URL")
}

model User {
  id Int @id
}
"#,
    )
    .expect("schema.prisma");
    std::fs::write(
        root.join("src/index.ts"),
        "import { PrismaClient } from '@prisma/client';\nexport const db = new PrismaClient();\n",
    )
    .expect("entry");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler"},"include":["src/**/*"]}"#,
    )
    .expect("tsconfig");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev: Vec<String> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect();
    let unused_prod: Vec<String> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect();
    assert!(
        !unused_dev.contains(&"prisma-json-types-generator".to_owned())
            && !unused_prod.contains(&"prisma-json-types-generator".to_owned()),
        "prisma-json-types-generator is referenced as a generator provider in \
         prisma/schema.prisma and should be credited. unused_dev={unused_dev:?} \
         unused_prod={unused_prod:?}"
    );
}

#[test]
fn prisma_multifile_schema_credits_generator_provider() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("prisma/schema")).expect("prisma/schema dir");
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "prisma-multifile",
            "private": true,
            "dependencies": {
                "@prisma/client": "6.0.0"
            },
            "devDependencies": {
                "prisma": "6.0.0",
                "prisma-erd-generator": "2.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("prisma/schema/generators.prisma"),
        r#"generator client {
  provider = "prisma-client-js"
}

generator erd {
  provider = "prisma-erd-generator"
}
"#,
    )
    .expect("generators.prisma");
    std::fs::write(
        root.join("prisma/schema/models.prisma"),
        "model User {\n  id Int @id\n}\n",
    )
    .expect("models.prisma");
    std::fs::write(
        root.join("src/index.ts"),
        "import { PrismaClient } from '@prisma/client';\nexport const db = new PrismaClient();\n",
    )
    .expect("entry");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler"},"include":["src/**/*"]}"#,
    )
    .expect("tsconfig");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev: Vec<String> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect();
    assert!(
        !unused_dev.contains(&"prisma-erd-generator".to_owned()),
        "prisma-erd-generator referenced from prisma/schema/generators.prisma should be \
         credited under the multi-file schema layout. unused_dev={unused_dev:?}"
    );
}

#[test]
fn prisma_root_schema_credits_generator_provider() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "prisma-root-schema",
            "private": true,
            "dependencies": {
                "@prisma/client": "6.0.0"
            },
            "devDependencies": {
                "prisma": "6.0.0",
                "prisma-json-types-generator": "3.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("schema.prisma"),
        r#"generator client {
  provider = "prisma-client-js"
}

generator json {
  provider = "prisma-json-types-generator"
}

model User {
  id Int @id
}
"#,
    )
    .expect("schema.prisma");
    std::fs::write(
        root.join("src/index.ts"),
        "import { PrismaClient } from '@prisma/client';\nexport const db = new PrismaClient();\n",
    )
    .expect("entry");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler"},"include":["src/**/*"]}"#,
    )
    .expect("tsconfig");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev: Vec<String> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect();
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_dev.contains(&"prisma-json-types-generator".to_owned()),
        "prisma-json-types-generator referenced from root schema.prisma should be credited. \
         unused_dev={unused_dev:?}"
    );
    assert!(
        !unused_files.iter().any(|p| p.ends_with("schema.prisma")),
        "root schema.prisma is a Prisma default schema location and should not be reported as \
         unused. unused_files={unused_files:?}"
    );
}

#[test]
fn prisma_block_commented_generator_provider_is_not_credited() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("prisma")).expect("prisma dir");
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "prisma-block-commented-generator",
            "private": true,
            "dependencies": {
                "@prisma/client": "6.0.0"
            },
            "devDependencies": {
                "prisma": "6.0.0",
                "prisma-erd-generator": "2.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("prisma/schema.prisma"),
        r#"generator client {
  provider = "prisma-client-js"
}

/*
generator erd {
  provider = "prisma-erd-generator"
}
*/

model User {
  id Int @id
}
"#,
    )
    .expect("schema.prisma");
    std::fs::write(
        root.join("src/index.ts"),
        "import { PrismaClient } from '@prisma/client';\nexport const db = new PrismaClient();\n",
    )
    .expect("entry");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler"},"include":["src/**/*"]}"#,
    )
    .expect("tsconfig");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev: Vec<String> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect();
    assert!(
        unused_dev.contains(&"prisma-erd-generator".to_owned()),
        "prisma-erd-generator only appears inside a Prisma block comment and should remain \
         reportable as unused. unused_dev={unused_dev:?}"
    );
}

#[test]
fn node_module_register_loader_is_credited_as_used_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::create_dir_all(root.join("resources/loaders")).expect("loaders dir");

    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "register-loader-fixture",
            "private": true,
            "devDependencies": {
                "@swc-node/register": "1.11.1"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler"},"include":["src/**/*","resources/**/*"]}"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("resources/loaders/ts.js"),
        "import { register } from 'node:module';\n\
         import { pathToFileURL } from 'node:url';\n\
         register('@swc-node/register/esm', pathToFileURL('./'));\n",
    )
    .expect("loader file");
    std::fs::write(root.join("src/index.ts"), "export const hello = 'world';\n").expect("entry");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dev.contains(&"@swc-node/register"),
        "@swc-node/register is loaded via `register('@swc-node/register/esm', ...)` and should \
         not be flagged as unused. unused_dev={unused_dev:?}"
    );
}

#[test]
fn node_module_register_new_url_loader_hooks_are_used_exports() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/hooks")).expect("hooks dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "register-new-url-loader-fixture",
            "private": true,
            "main": "src/register.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler"},"include":["src/**/*"]}"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/register.ts"),
        "import { register } from 'node:module';\n\
         const loaderUrl = new URL('./hooks/json-loader.ts', import.meta.url);\n\
         register(loaderUrl);\n",
    )
    .expect("register file");
    std::fs::write(
        root.join("src/hooks/json-loader.ts"),
        "export async function load(url: string, context: unknown, nextLoad: Function) {\n\
             return nextLoad(url, context);\n\
         }\n\
         export function helperOnlyForTests() { return true; }\n",
    )
    .expect("loader file");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .export
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&export.export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export.export_name.clone(),
            )
        })
        .collect();

    assert!(
        !unused_exports
            .iter()
            .any(|(file, export)| file == "src/hooks/json-loader.ts" && export == "load"),
        "Node calls registered loader hook exports reflectively: {unused_exports:?}"
    );
    assert!(
        unused_exports.iter().any(|(file, export)| {
            file == "src/hooks/json-loader.ts" && export == "helperOnlyForTests"
        }),
        "non-hook exports should still be reported: {unused_exports:?}"
    );
}

#[test]
fn node_module_register_legacy_loader_hook_exports_are_used() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src/hooks")).expect("hooks dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "register-legacy-loader-fixture",
            "private": true,
            "main": "src/register.ts"
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"esnext","moduleResolution":"bundler"},"include":["src/**/*"]}"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join("src/register.ts"),
        "import { register } from 'node:module';\n\
         register('./hooks/legacy-loader.ts', import.meta.url);\n",
    )
    .expect("register file");
    std::fs::write(
        root.join("src/hooks/legacy-loader.ts"),
        "export function getFormat(url, context, defaultGetFormat) {\n\
             return defaultGetFormat(url, context);\n\
         }\n\
         export function getSource(url, context, defaultGetSource) {\n\
             return defaultGetSource(url, context);\n\
         }\n\
         export function transformSource(source, context, defaultTransformSource) {\n\
             return defaultTransformSource(source, context);\n\
         }\n\
         export function helperOnlyForTests() { return true; }\n",
    )
    .expect("loader file");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .export
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&export.export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export.export_name.clone(),
            )
        })
        .collect();

    for legacy in ["getFormat", "getSource", "transformSource"] {
        assert!(
            !unused_exports
                .iter()
                .any(|(file, export)| file == "src/hooks/legacy-loader.ts" && export == legacy),
            "{legacy} legacy hook should not be reported as unused-export: {unused_exports:?}"
        );
    }
    assert!(
        unused_exports.iter().any(|(file, export)| {
            file == "src/hooks/legacy-loader.ts" && export == "helperOnlyForTests"
        }),
        "non-hook exports should still be reported: {unused_exports:?}"
    );
}

#[test]
fn dangerfile_ts_is_recognized_as_tooling_entry() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "danger-fixture",
            "private": true,
            "main": "src/index.ts",
            "devDependencies": {
                "danger": "13.0.0",
                "unused-tool": "1.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(root.join("src/index.ts"), "export const app = true;\n").expect("source file");
    std::fs::write(
        root.join("dangerfile.ts"),
        "import { danger } from 'danger';\n\
         export default function check() {\n\
             return danger.github.pr;\n\
         }\n",
    )
    .expect("dangerfile");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results);
    assert!(
        !unused_files.contains(&"dangerfile.ts".to_owned()),
        "Dangerfile should be treated as a tooling entry. unused_files={unused_files:?}"
    );

    let unused_dev = unused_dev_dependencies(&results);
    assert!(
        !unused_dev.contains(&"danger".to_owned()),
        "Danger should be credited as the active tooling package. unused_dev={unused_dev:?}"
    );
    assert!(
        unused_dev.contains(&"unused-tool".to_owned()),
        "unrelated dev dependencies should remain reportable. unused_dev={unused_dev:?}"
    );
}

#[test]
fn stryker_config_mjs_credits_runner_checker_and_plugin_packages() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "stryker-fixture",
            "private": true,
            "main": "src/index.ts",
            "devDependencies": {
                "@stryker-mutator/core": "8.0.0",
                "@stryker-mutator/mocha-runner": "8.0.0",
                "@stryker-mutator/typescript-checker": "8.0.0",
                "@stryker-mutator/html-reporter": "8.0.0",
                "custom-stryker-plugin": "1.0.0",
                "unused-tool": "1.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(root.join("src/index.ts"), "export const app = true;\n").expect("source file");
    std::fs::write(
        root.join("stryker.conf.mjs"),
        "import '@stryker-mutator/html-reporter';\n\
         export default {\n\
             testRunner: 'mocha',\n\
             checkers: ['typescript'],\n\
             plugins: ['custom-stryker-plugin', 'dashboard']\n\
         };\n",
    )
    .expect("stryker config");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results);
    assert!(
        !unused_files.contains(&"stryker.conf.mjs".to_owned()),
        "Stryker config should be treated as a tooling entry. unused_files={unused_files:?}"
    );

    let unused_dev = unused_dev_dependencies(&results);
    for dep in [
        "@stryker-mutator/core",
        "@stryker-mutator/mocha-runner",
        "@stryker-mutator/typescript-checker",
        "@stryker-mutator/html-reporter",
        "custom-stryker-plugin",
    ] {
        assert!(
            !unused_dev.contains(&dep.to_owned()),
            "{dep} should be credited from Stryker config. unused_dev={unused_dev:?}"
        );
    }
    assert!(
        unused_dev.contains(&"unused-tool".to_owned()),
        "unrelated dev dependencies should remain reportable. unused_dev={unused_dev:?}"
    );
}

#[test]
fn stryker_json_config_credits_known_runner_without_js_parsing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "stryker-json-fixture",
            "private": true,
            "main": "src/index.ts",
            "devDependencies": {
                "@stryker-mutator/core": "8.0.0",
                "@stryker-mutator/jest-runner": "8.0.0",
                "unused-tool": "1.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(root.join("src/index.ts"), "export const app = true;\n").expect("source file");
    std::fs::write(
        root.join("stryker.config.json"),
        r#"{
            "testRunner": "jest",
            "plugins": ["dashboard"]
        }"#,
    )
    .expect("stryker json config");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results);
    assert!(
        !unused_files.contains(&"stryker.config.json".to_owned()),
        "Stryker JSON config should be treated as a tooling entry. unused_files={unused_files:?}"
    );

    let unused_dev = unused_dev_dependencies(&results);
    assert!(
        !unused_dev.contains(&"@stryker-mutator/core".to_owned()),
        "Stryker core should be credited as tooling. unused_dev={unused_dev:?}"
    );
    assert!(
        !unused_dev.contains(&"@stryker-mutator/jest-runner".to_owned()),
        "known JSON testRunner should credit the runner package. unused_dev={unused_dev:?}"
    );
    assert!(
        unused_dev.contains(&"unused-tool".to_owned()),
        "unrelated dev dependencies should remain reportable. unused_dev={unused_dev:?}"
    );
}

#[test]
fn stryker_workspace_config_is_recognized_from_root_dev_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("packages/app/src")).expect("workspace src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "stryker-workspace-fixture",
            "private": true,
            "workspaces": ["packages/*"],
            "devDependencies": {
                "@stryker-mutator/core": "8.0.0",
                "@stryker-mutator/jest-runner": "8.0.0",
                "unused-tool": "1.0.0"
            }
        }"#,
    )
    .expect("root package json");
    std::fs::write(
        root.join("packages/app/package.json"),
        r#"{
            "name": "@example/app",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("workspace package json");
    std::fs::write(
        root.join("packages/app/src/index.ts"),
        "export const app = true;\n",
    )
    .expect("source file");
    std::fs::write(
        root.join("packages/app/stryker.conf.mjs"),
        "export default { testRunner: 'jest' };\n",
    )
    .expect("workspace stryker config");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results);
    assert!(
        !unused_files.contains(&"packages/app/stryker.conf.mjs".to_owned()),
        "workspace Stryker config should be treated as a tooling entry. unused_files={unused_files:?}"
    );

    let unused_dev = unused_dev_dependencies(&results);
    assert!(
        !unused_dev.contains(&"@stryker-mutator/core".to_owned()),
        "root Stryker core should activate tooling in a workspace-shaped project. unused_dev={unused_dev:?}"
    );
    assert!(
        !unused_dev.contains(&"@stryker-mutator/jest-runner".to_owned()),
        "nested Stryker config should credit known runner packages. unused_dev={unused_dev:?}"
    );
    assert!(
        unused_dev.contains(&"unused-tool".to_owned()),
        "unrelated dev dependencies should remain reportable. unused_dev={unused_dev:?}"
    );
}

#[test]
fn wuchale_workspace_config_is_recognized_from_root_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("packages/app/src")).expect("workspace src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "wuchale-workspace-fixture",
            "private": true,
            "workspaces": ["packages/*"],
            "devDependencies": {
                "wuchale": "0.9.0",
                "@wuchale/vite-plugin": "0.9.0",
                "@wuchale/svelte": "0.9.0",
                "unused-tool": "1.0.0"
            }
        }"#,
    )
    .expect("root package json");
    std::fs::write(
        root.join("packages/app/package.json"),
        r#"{
            "name": "@example/app",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("workspace package json");
    std::fs::write(
        root.join("packages/app/src/index.ts"),
        "export const app = true;\n",
    )
    .expect("source file");
    std::fs::write(
        root.join("packages/app/wuchale.config.js"),
        "import { defineConfig } from 'wuchale';\n\
         import { adapter as svelte } from '@wuchale/svelte';\n\
         export default defineConfig({ adapters: { main: svelte() } });\n",
    )
    .expect("wuchale config");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results);
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("packages/app/wuchale.config.js")),
        "workspace Wuchale config should be treated as a tooling entry. unused_files={unused_files:?}"
    );

    let unused_dev = unused_dev_dependencies(&results);
    for dep in ["wuchale", "@wuchale/vite-plugin", "@wuchale/svelte"] {
        assert!(
            !unused_dev.contains(&dep.to_owned()),
            "{dep} should be credited from Wuchale tooling/config. unused_dev={unused_dev:?}"
        );
    }
    assert!(
        unused_dev.contains(&"unused-tool".to_owned()),
        "unrelated dev dependencies should remain reportable. unused_dev={unused_dev:?}"
    );
}

#[test]
fn wuchale_config_file_activates_plugin_without_enabler_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "wuchale-config-activated-fixture",
            "private": true,
            "main": "src/index.ts",
            "devDependencies": {
                "@wuchale/svelte": "0.9.0",
                "unused-tool": "1.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(root.join("src/index.ts"), "export const app = true;\n").expect("source file");
    std::fs::write(
        root.join("wuchale.config.js"),
        "import { adapter as svelte } from '@wuchale/svelte';\n\
         export default { adapters: { main: svelte() } };\n",
    )
    .expect("wuchale config");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results);
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("wuchale.config.js")),
        "documented Wuchale config should activate the plugin and stay reachable. unused_files={unused_files:?}"
    );

    let unused_dev = unused_dev_dependencies(&results);
    assert!(
        !unused_dev.contains(&"@wuchale/svelte".to_owned()),
        "adapter imported from config should be credited when activation comes from the config file. unused_dev={unused_dev:?}"
    );
    assert!(
        unused_dev.contains(&"unused-tool".to_owned()),
        "unrelated dev dependencies should remain reportable. unused_dev={unused_dev:?}"
    );
}

#[test]
fn wuchale_vite_config_file_keeps_custom_js_config_reachable() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("packages/app/config")).expect("config dir");
    std::fs::create_dir_all(root.join("packages/app/src")).expect("source dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "wuchale-vite-fixture",
            "private": true,
            "workspaces": ["packages/*"],
            "devDependencies": {
                "wuchale": "0.9.0",
                "@wuchale/vite-plugin": "0.9.0",
                "@wuchale/svelte": "0.9.0",
                "unused-tool": "1.0.0"
            }
        }"#,
    )
    .expect("root package json");
    std::fs::write(
        root.join("packages/app/package.json"),
        r#"{
            "name": "@example/app",
            "private": true,
            "main": "src/index.ts"
        }"#,
    )
    .expect("workspace package json");
    std::fs::write(
        root.join("packages/app/src/index.ts"),
        "export const app = true;\n",
    )
    .expect("source file");
    std::fs::write(
        root.join("packages/app/vite.config.ts"),
        "import { defineConfig } from 'vite';\n\
         import { wuchale } from '@wuchale/vite-plugin';\n\
         export default defineConfig({\n\
             plugins: [wuchale({ configFile: './config/custom-wuchale.config.js' })]\n\
         });\n",
    )
    .expect("vite config");
    std::fs::write(
        root.join("packages/app/config/custom-wuchale.config.js"),
        "import { defineConfig } from 'wuchale';\n\
         import { adapter as svelte } from '@wuchale/svelte';\n\
         export default defineConfig({ adapters: { main: svelte() } });\n",
    )
    .expect("custom wuchale config");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results);
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("packages/app/config/custom-wuchale.config.js")),
        "static Vite configFile should keep the custom Wuchale config reachable. unused_files={unused_files:?}"
    );

    let unused_dev = unused_dev_dependencies(&results);
    for dep in ["wuchale", "@wuchale/vite-plugin", "@wuchale/svelte"] {
        assert!(
            !unused_dev.contains(&dep.to_owned()),
            "{dep} should be credited from Wuchale Vite config wiring. unused_dev={unused_dev:?}"
        );
    }
    assert!(
        unused_dev.contains(&"unused-tool".to_owned()),
        "unrelated dev dependencies should remain reportable. unused_dev={unused_dev:?}"
    );
}

#[test]
fn wuchale_unsupported_ts_config_remains_reportable() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "wuchale-ts-config-fixture",
            "private": true,
            "main": "src/index.ts",
            "devDependencies": {
                "wuchale": "0.9.0",
                "@wuchale/svelte": "0.9.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(root.join("src/index.ts"), "export const app = true;\n").expect("source file");
    std::fs::write(
        root.join("wuchale.config.ts"),
        "import { adapter as svelte } from '@wuchale/svelte';\n\
         export default { adapters: { main: svelte() } };\n",
    )
    .expect("unsupported config");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results);
    assert!(
        unused_files
            .iter()
            .any(|path| path.ends_with("wuchale.config.ts")),
        "unsupported Wuchale TypeScript config should not be hidden by the plugin. unused_files={unused_files:?}"
    );
}

fn unused_file_paths(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn unused_dev_dependencies(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.clone())
        .collect()
}
