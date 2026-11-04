//! Unlisted dependencies that a tool config names instead of an import.
//!
//! A tsconfig `compilerOptions.plugins[].name` names a language service
//! plugin. TypeScript loads it by name, so the package must be installed. An
//! Expo app config that sets `userInterfaceStyle` needs `expo-system-ui`.
//! When no manifest that the config can use lists the package, the package is
//! an unlisted dependency. The finding points at the config file.

use std::path::Path;

use fallow_types::results::AnalysisResults;

use super::common::{
    create_config, create_config_with_ignore_deps, create_config_with_ignore_patterns,
};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

fn temp_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical temp dir");
    (dir, root)
}

/// Return `(package, file relative to root, line)` for each unlisted site.
fn unlisted_sites(results: &AnalysisResults, root: &Path) -> Vec<(String, String, u32)> {
    let mut sites: Vec<(String, String, u32)> = results
        .unlisted_dependencies
        .iter()
        .flat_map(|finding| {
            finding.dep.imported_from.iter().map(|site| {
                let file = site
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&site.path)
                    .to_string_lossy()
                    .replace('\\', "/");
                (finding.dep.package_name.clone(), file, site.line)
            })
        })
        .collect();
    sites.sort();
    sites
}

const TSCONFIG_WITH_PLUGINS: &str = r#"{
  "compilerOptions": {
    "strict": true,
    "plugins": [
      { "name": "@acme/ts-plugin" },
      { "name": "./local-plugin.js" },
      { "name": "fs" }
    ]
  }
}
"#;

fn workspace_with_tsconfig(root: &Path, root_deps: &str, app_deps: &str, tsconfig_dir: &str) {
    write(
        &root.join("package.json"),
        &format!(
            r#"{{ "name": "root", "private": true, "workspaces": ["packages/*"], "devDependencies": {{ "typescript": "5.0.0"{root_deps} }} }}"#
        ),
    );
    write(
        &root.join("packages/app/package.json"),
        &format!(
            r#"{{ "name": "app", "private": true, "main": "src/index.ts", "dependencies": {{ {app_deps} }} }}"#
        ),
    );
    write(
        &root.join("packages/app/src/index.ts"),
        "export const app = 1;\n",
    );
    write(
        &root.join(tsconfig_dir).join("tsconfig.json"),
        TSCONFIG_WITH_PLUGINS,
    );
}

#[test]
fn unlisted_tsconfig_plugin_is_reported_at_the_config() {
    let (_dir, root) = temp_root();
    workspace_with_tsconfig(&root, "", "", "packages/app");

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    assert_eq!(
        unlisted_sites(&results, &root),
        vec![(
            "@acme/ts-plugin".to_string(),
            "packages/app/tsconfig.json".to_string(),
            5
        )],
        "only the bare package name is unlisted; a path and a builtin are not"
    );
}

#[test]
fn unlisted_root_tsconfig_plugin_is_reported() {
    let (_dir, root) = temp_root();
    workspace_with_tsconfig(&root, "", "", "");

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    assert_eq!(
        unlisted_sites(&results, &root),
        vec![(
            "@acme/ts-plugin".to_string(),
            "tsconfig.json".to_string(),
            5
        )],
    );
}

#[test]
fn tsconfig_plugin_listed_by_the_root_is_not_unlisted() {
    let (_dir, root) = temp_root();
    workspace_with_tsconfig(&root, r#", "@acme/ts-plugin": "1.0.0""#, "", "packages/app");

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    assert!(unlisted_sites(&results, &root).is_empty());
}

#[test]
fn tsconfig_plugin_listed_by_the_owning_workspace_is_not_unlisted() {
    let (_dir, root) = temp_root();
    workspace_with_tsconfig(&root, "", r#""@acme/ts-plugin": "1.0.0""#, "packages/app");

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    assert!(unlisted_sites(&results, &root).is_empty());
}

#[test]
fn tsconfig_plugin_listed_only_by_a_sibling_is_unlisted() {
    let (_dir, root) = temp_root();
    workspace_with_tsconfig(&root, "", "", "packages/app");
    write(
        &root.join("packages/other/package.json"),
        r#"{ "name": "other", "private": true, "main": "index.ts", "devDependencies": { "@acme/ts-plugin": "1.0.0" } }"#,
    );
    write(
        &root.join("packages/other/index.ts"),
        "export const other = 1;\n",
    );

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    let names: Vec<String> = unlisted_sites(&results, &root)
        .into_iter()
        .map(|(name, file, _)| format!("{name} {file}"))
        .collect();
    assert_eq!(names, vec!["@acme/ts-plugin packages/app/tsconfig.json"]);
}

#[test]
fn ignored_tsconfig_plugin_is_not_unlisted() {
    let (_dir, root) = temp_root();
    workspace_with_tsconfig(&root, "", "", "packages/app");

    let config = create_config_with_ignore_deps(root.clone(), &["@acme/ts-plugin"]);
    let results = fallow_core::analyze(&config).expect("analysis");

    assert!(unlisted_sites(&results, &root).is_empty());
}

fn expo_app(root: &Path, deps: &str, config: &str) {
    write(
        &root.join("package.json"),
        &format!(
            r#"{{ "name": "mobile", "private": true, "main": "App.tsx", "dependencies": {{ "expo": "50.0.0"{deps} }} }}"#
        ),
    );
    write(
        &root.join("App.tsx"),
        "export default function App() { return null; }\n",
    );
    write(&root.join("app.config.ts"), config);
}

const EXPO_TOP_LEVEL_STYLE: &str = r#"import type { ExpoConfig } from "expo/config";

const config: ExpoConfig = {
  name: "mobile",
  slug: "mobile",
  userInterfaceStyle: "automatic",
};

export default config;
"#;

#[test]
fn expo_user_interface_style_requires_system_ui() {
    let (_dir, root) = temp_root();
    expo_app(&root, "", EXPO_TOP_LEVEL_STYLE);

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    assert_eq!(
        unlisted_sites(&results, &root),
        vec![("expo-system-ui".to_string(), "app.config.ts".to_string(), 6)],
    );
}

#[test]
fn expo_platform_user_interface_style_requires_system_ui() {
    let (_dir, root) = temp_root();
    write(
        &root.join("package.json"),
        r#"{ "name": "mobile", "private": true, "main": "App.tsx", "dependencies": { "expo": "50.0.0" } }"#,
    );
    write(
        &root.join("App.tsx"),
        "export default function App() { return null; }\n",
    );
    write(
        &root.join("app.json"),
        r#"{
  "expo": {
    "name": "mobile",
    "android": {
      "userInterfaceStyle": "dark"
    }
  }
}
"#,
    );

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    assert_eq!(
        unlisted_sites(&results, &root),
        vec![("expo-system-ui".to_string(), "app.json".to_string(), 5)],
    );
}

#[test]
fn expo_user_interface_style_with_listed_system_ui_is_clean() {
    let (_dir, root) = temp_root();
    expo_app(
        &root,
        r#", "expo-system-ui": "3.0.0""#,
        EXPO_TOP_LEVEL_STYLE,
    );

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    assert!(unlisted_sites(&results, &root).is_empty());
}

#[test]
fn expo_config_without_user_interface_style_is_clean() {
    let (_dir, root) = temp_root();
    expo_app(
        &root,
        "",
        "export default { name: \"mobile\", slug: \"mobile\" };\n",
    );

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    assert!(unlisted_sites(&results, &root).is_empty());
}

#[test]
fn production_run_skips_a_development_tsconfig_plugin() {
    let (_dir, root) = temp_root();
    workspace_with_tsconfig(&root, "", "", "packages/app");

    let mut config = create_config(root.clone());
    config.production = true;
    let results = fallow_core::analyze(&config).expect("analysis");

    assert!(unlisted_sites(&results, &root).is_empty());
}

#[test]
fn production_run_keeps_the_expo_system_ui_requirement() {
    let (_dir, root) = temp_root();
    expo_app(&root, "", EXPO_TOP_LEVEL_STYLE);

    let mut config = create_config(root.clone());
    config.production = true;
    let results = fallow_core::analyze(&config).expect("analysis");

    assert_eq!(
        unlisted_sites(&results, &root),
        vec![("expo-system-ui".to_string(), "app.config.ts".to_string(), 6)],
    );
}

#[test]
fn expo_ios_only_user_interface_style_needs_no_system_ui() {
    let (_dir, root) = temp_root();
    expo_app(
        &root,
        "",
        "export default { name: \"mobile\", ios: { userInterfaceStyle: \"dark\" } };\n",
    );

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    assert!(unlisted_sites(&results, &root).is_empty());
}

#[test]
fn editor_bundled_tsconfig_plugin_is_not_unlisted() {
    let (_dir, root) = temp_root();
    write(
        &root.join("package.json"),
        r#"{ "name": "app", "private": true, "main": "src/index.ts", "devDependencies": { "typescript": "5.0.0" } }"#,
    );
    write(&root.join("src/index.ts"), "export const app = 1;\n");
    write(
        &root.join("tsconfig.json"),
        r#"{ "compilerOptions": { "plugins": [{ "name": "typescript-svelte-plugin" }] } }"#,
    );

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    assert!(unlisted_sites(&results, &root).is_empty());
}

#[test]
fn tsconfig_plugin_listed_by_a_manifest_without_analyzed_files_is_not_unlisted() {
    let (_dir, root) = temp_root();
    workspace_with_tsconfig(&root, "", "", "packages/app");
    write(
        &root.join("tools/editor/package.json"),
        r#"{ "name": "editor-tools", "private": true, "devDependencies": { "@acme/ts-plugin": "1.0.0" } }"#,
    );
    write(
        &root.join("tools/editor/tsconfig.json"),
        TSCONFIG_WITH_PLUGINS,
    );

    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    let files: Vec<String> = unlisted_sites(&results, &root)
        .into_iter()
        .map(|(_, file, _)| file)
        .collect();
    assert_eq!(files, vec!["packages/app/tsconfig.json"]);
}

#[test]
fn tsconfig_plugin_in_an_ignored_path_is_not_unlisted() {
    let (_dir, root) = temp_root();
    workspace_with_tsconfig(&root, "", "", "packages/app");

    let config = create_config_with_ignore_patterns(root.clone(), &["packages/app/**"]);
    let results = fallow_core::analyze(&config).expect("analysis");

    assert!(unlisted_sites(&results, &root).is_empty());
}
