#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Production mode leaves out a file that is an entry point only through a
//! user `framework[]` entry with the `support` role (issue #4068).

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};
use tempfile::TempDir;

const TOOLS_FRAMEWORK: &str = r#"{
  "name": "repo-tools",
  "enablers": ["left-pad"],
  "entryPoints": ["tools/*.js"],
  "entryPointRole": "support"
}"#;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// The issue repro: `src/index.js` is the runtime root, `tools/report.js` is a
/// support entry. `onlyTools` and `src/tool-helper.js` have tool readers only.
fn support_tools_fixture(config: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{
  "name": "repro",
  "private": true,
  "type": "module",
  "dependencies": { "chalk": "5.0.0" },
  "devDependencies": { "left-pad": "1.3.0" }
}"#,
    );
    write(
        root,
        "src/index.js",
        "import { used } from './lib.js';\nconsole.log(used);\n",
    );
    write(
        root,
        "src/lib.js",
        "export const used = 1;\nexport const onlyTools = 2;\n",
    );
    write(root, "src/tool-helper.js", "export const pad = 4;\n");
    write(
        root,
        "tools/report.js",
        "import leftPad from 'left-pad';\nimport chalk from 'chalk';\nimport { onlyTools } from '../src/lib.js';\nimport { pad } from '../src/tool-helper.js';\nexport const stray = 1;\nconsole.log(chalk, leftPad(String(onlyTools), pad));\n",
    );
    write(root, ".fallowrc.json", config);
    dir
}

fn default_config() -> String {
    format!(r#"{{ "entry": ["src/index.js"], "framework": [{TOOLS_FRAMEWORK}] }}"#)
}

fn dead_code(root: &Path, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = run_fallow_in_root("dead-code", root, &args);
    assert!(
        output.code == 0 || output.code == 1,
        "dead-code should not crash. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    parse_json(&output)
}

fn unused_export_names(json: &serde_json::Value) -> Vec<String> {
    json["unused_exports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|export| {
            format!(
                "{}:{}",
                export["path"].as_str().unwrap(),
                export["export_name"].as_str().unwrap()
            )
        })
        .collect()
}

fn unused_file_paths(json: &serde_json::Value) -> Vec<String> {
    json["unused_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap().to_string())
        .collect()
}

fn unused_dependency_names(json: &serde_json::Value) -> Vec<String> {
    json["unused_dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|dep| dep["package_name"].as_str().unwrap().to_string())
        .collect()
}

/// Every path that any finding of the report names, as a flat string list.
fn all_reported_paths(json: &serde_json::Value) -> Vec<String> {
    fn collect(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, inner) in map {
                    if key == "path"
                        && let Some(path) = inner.as_str()
                    {
                        out.push(path.to_string());
                    }
                    collect(inner, out);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    collect(item, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for key in [
        "unused_files",
        "unused_exports",
        "unused_types",
        "unresolved_imports",
        "unlisted_dependencies",
        "duplicate_exports",
        "circular_dependencies",
        "stale_suppressions",
    ] {
        if let Some(value) = json.get(key) {
            collect(value, &mut out);
        }
    }
    out
}

#[test]
fn normal_run_keeps_crediting_support_tool_imports() {
    let dir = support_tools_fixture(&default_config());
    let json = dead_code(dir.path(), &[]);

    let exports = unused_export_names(&json);
    assert!(
        !exports.iter().any(|e| e.ends_with(":onlyTools")),
        "a normal run credits the tool import: {exports:?}"
    );
    let files = unused_file_paths(&json);
    assert!(
        !files.iter().any(|f| f.ends_with("tool-helper.js")),
        "a normal run keeps the tool helper alive: {files:?}"
    );
}

#[test]
fn production_reports_exports_and_files_that_only_support_tools_use() {
    let dir = support_tools_fixture(&default_config());
    let json = dead_code(dir.path(), &["--production"]);

    let exports = unused_export_names(&json);
    assert!(
        exports.contains(&"src/lib.js:onlyTools".to_string()),
        "production must not credit an import of a support tool: {exports:?}"
    );
    assert!(
        !exports.iter().any(|e| e.ends_with(":used")),
        "the runtime import still credits `used`: {exports:?}"
    );
    let files = unused_file_paths(&json);
    assert!(
        files.contains(&"src/tool-helper.js".to_string()),
        "a file that only a support tool imports is unused in production: {files:?}"
    );
    let deps = unused_dependency_names(&json);
    assert!(
        deps.contains(&"chalk".to_string()),
        "a dependency that only a support tool imports is unused in production: {deps:?}"
    );
}

#[test]
fn production_reports_no_finding_for_the_support_tool_itself() {
    let dir = support_tools_fixture(&default_config());
    let json = dead_code(dir.path(), &["--production"]);

    let paths = all_reported_paths(&json);
    assert!(
        !paths.iter().any(|p| p.starts_with("tools/")),
        "the support tool takes no part in a production run: {paths:?}"
    );
    let summary = &json["entry_points"];
    let total = summary["total"].as_u64().unwrap_or_default();
    assert_eq!(
        total, 1,
        "only the runtime root is an entry point in production: {summary}"
    );
}

#[test]
fn a_support_file_that_also_matches_config_entry_stays_a_runtime_root() {
    let config = format!(
        r#"{{ "entry": ["src/index.js", "tools/report.js"], "framework": [{TOOLS_FRAMEWORK}] }}"#
    );
    let dir = support_tools_fixture(&config);
    let json = dead_code(dir.path(), &["--production"]);

    let exports = unused_export_names(&json);
    assert!(
        !exports.iter().any(|e| e.ends_with(":onlyTools")),
        "a config `entry` match keeps the tool a runtime root: {exports:?}"
    );
    let files = unused_file_paths(&json);
    assert!(
        !files.iter().any(|f| f.ends_with("tool-helper.js")),
        "a config `entry` match keeps the tool imports alive: {files:?}"
    );
}

#[test]
fn a_support_file_that_also_matches_a_runtime_framework_entry_stays_a_runtime_root() {
    let config = format!(
        r#"{{ "entry": ["src/index.js"], "framework": [{TOOLS_FRAMEWORK}, {{ "name": "repo-server", "enablers": ["left-pad"], "entryPoints": ["tools/report.js"], "entryPointRole": "runtime" }}] }}"#
    );
    let dir = support_tools_fixture(&config);
    let json = dead_code(dir.path(), &["--production"]);

    let exports = unused_export_names(&json);
    assert!(
        !exports.iter().any(|e| e.ends_with(":onlyTools")),
        "a runtime role match keeps the tool a runtime root: {exports:?}"
    );
}

#[test]
fn a_support_file_that_runtime_code_imports_stays_a_normal_module() {
    let dir = support_tools_fixture(&default_config());
    write(
        dir.path(),
        "src/index.js",
        "import { used } from './lib.js';\nimport { stray } from '../tools/report.js';\nconsole.log(used, stray);\n",
    );
    let json = dead_code(dir.path(), &["--production"]);

    let exports = unused_export_names(&json);
    assert!(
        !exports.iter().any(|e| e.ends_with(":onlyTools")),
        "a tool file that runtime code loads keeps crediting its imports: {exports:?}"
    );
    let deps = unused_dependency_names(&json);
    assert!(
        !deps.contains(&"chalk".to_string()),
        "a tool file that runtime code loads keeps its dependencies used: {deps:?}"
    );
}

#[test]
fn a_user_plugin_without_an_explicit_role_keeps_its_entries_in_production() {
    // `entryPointRole` defaults to `support`, but only an explicit
    // `support` role opts in. A plugin that lists app entries without a
    // role keeps them in production.
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "app", "private": true, "type": "module", "devDependencies": { "x": "1.0.0" } }"#,
    );
    write(root, "src/index.js", "console.log('app');\n");
    write(
        root,
        "src/pages/home.js",
        "import { lib } from '../lib.js';\nconsole.log(lib);\n",
    );
    write(root, "src/lib.js", "export const lib = 1;\n");
    write(
        root,
        ".fallowrc.json",
        r#"{ "framework": [{ "name": "app", "enablers": ["x"], "entryPoints": ["src/pages/*.js"] }] }"#,
    );

    let json = dead_code(root, &["--production"]);
    let files = unused_file_paths(&json);
    assert!(
        !files.iter().any(|f| f.ends_with("lib.js")),
        "a plugin without an explicit role keeps its entries: {files:?}"
    );
    let exports = unused_export_names(&json);
    assert!(
        !exports.iter().any(|e| e.ends_with(":lib")),
        "a plugin without an explicit role keeps crediting imports: {exports:?}"
    );
}

#[test]
fn a_user_plugin_with_a_built_in_name_keeps_its_support_entries() {
    // Entry rules carry only the plugin name, so a user plugin named like a
    // built-in shares its rules. Production mode keeps such entries.
    let config = TOOLS_FRAMEWORK.replace("repo-tools", "vitest");
    let dir = support_tools_fixture(&format!(
        r#"{{ "entry": ["src/index.js"], "framework": [{config}] }}"#
    ));
    let json = dead_code(dir.path(), &["--production"]);

    let exports = unused_export_names(&json);
    assert!(
        !exports.iter().any(|e| e.ends_with(":onlyTools")),
        "a built-in plugin name keeps the support entry: {exports:?}"
    );
}

#[test]
fn a_support_file_credits_no_iconify_package_in_production() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{
  "name": "icons",
  "private": true,
  "dependencies": { "astro-icon": "1.1.0", "@iconify-json/jam": "1.2.0" }
}"#,
    );
    write(root, "src/index.js", "console.log('app');\n");
    write(
        root,
        "tools/preview.astro",
        "---\nimport { Icon } from 'astro-icon/components';\n---\n<Icon name=\"jam:github\" />\n",
    );
    write(
        root,
        ".fallowrc.json",
        r#"{ "entry": ["src/index.js"], "framework": [{ "name": "repo-tools", "enablers": ["astro-icon"], "entryPoints": ["tools/*.astro"], "entryPointRole": "support" }] }"#,
    );

    let normal = unused_dependency_names(&dead_code(root, &[]));
    assert!(
        !normal.contains(&"@iconify-json/jam".to_string()),
        "a normal run credits the icon set of the tool file: {normal:?}"
    );
    let production = unused_dependency_names(&dead_code(root, &["--production"]));
    assert!(
        production.contains(&"@iconify-json/jam".to_string()),
        "a production run gives no credit from the tool file: {production:?}"
    );
}

#[test]
fn production_keeps_built_in_plugin_support_entries() {
    let dir = support_tools_fixture(&default_config());
    let root = dir.path();
    // Danger is a built-in plugin whose dangerfile is a support entry.
    write(
        root,
        "package.json",
        r#"{
  "name": "repro",
  "private": true,
  "type": "module",
  "dependencies": { "chalk": "5.0.0" },
  "devDependencies": { "danger": "12.0.0", "left-pad": "1.3.0" }
}"#,
    );
    write(
        root,
        "src/danger-rules.js",
        "export const dangerOnly = 1;\n",
    );
    write(
        root,
        "dangerfile.js",
        "import { dangerOnly } from './src/danger-rules.js';\nconsole.log(dangerOnly);\n",
    );

    for extra in [&[][..], &["--production"][..]] {
        let json = dead_code(root, extra);
        let exports = unused_export_names(&json);
        assert!(
            !exports.iter().any(|e| e.ends_with(":dangerOnly")),
            "a built-in support entry keeps crediting its imports ({extra:?}): {exports:?}"
        );
        let files = unused_file_paths(&json);
        assert!(
            !files.iter().any(|f| f.ends_with("danger-rules.js")),
            "a built-in support entry keeps its imports alive ({extra:?}): {files:?}"
        );
    }
}

#[test]
fn per_analysis_production_config_leaves_out_support_tools() {
    let config = format!(
        r#"{{ "entry": ["src/index.js"], "production": {{ "deadCode": true }}, "framework": [{TOOLS_FRAMEWORK}] }}"#
    );
    let dir = support_tools_fixture(&config);
    let json = dead_code(dir.path(), &[]);

    let exports = unused_export_names(&json);
    assert!(
        exports.contains(&"src/lib.js:onlyTools".to_string()),
        "`production.deadCode` must not credit a support tool import: {exports:?}"
    );
}

#[test]
fn a_warm_cache_from_the_other_mode_gives_the_cold_result() {
    let dir = support_tools_fixture(&default_config());
    let root = dir.path();
    let cold_normal = dead_code(root, &["--no-cache"]);
    let cold_production = dead_code(root, &["--production", "--no-cache"]);

    // Each mode runs on top of a cache that the other mode wrote last.
    for _ in 0..2 {
        let warm_normal = dead_code(root, &[]);
        assert_eq!(
            unused_export_names(&warm_normal),
            unused_export_names(&cold_normal)
        );
        assert_eq!(
            unused_file_paths(&warm_normal),
            unused_file_paths(&cold_normal)
        );
        let warm_production = dead_code(root, &["--production"]);
        assert_eq!(
            unused_export_names(&warm_production),
            unused_export_names(&cold_production)
        );
        assert_eq!(
            unused_file_paths(&warm_production),
            unused_file_paths(&cold_production)
        );
        assert_eq!(
            unused_dependency_names(&warm_production),
            unused_dependency_names(&cold_production)
        );
    }
}

/// `supportEntry` declares the tool scripts without a `framework[]` entry.
fn support_entry_config() -> String {
    r#"{ "entry": ["src/index.js"], "supportEntry": ["tools/*.js"] }"#.to_string()
}

#[test]
fn support_entry_keeps_tool_imports_alive_in_a_normal_run() {
    let dir = support_tools_fixture(&support_entry_config());
    let json = dead_code(dir.path(), &[]);

    let exports = unused_export_names(&json);
    assert!(
        !exports.iter().any(|e| e.ends_with(":onlyTools")),
        "a normal run credits the import of a support entry: {exports:?}"
    );
    let files = unused_file_paths(&json);
    assert!(
        !files
            .iter()
            .any(|f| f.ends_with("tool-helper.js") || f.ends_with("report.js")),
        "a normal run keeps the support entry and its imports: {files:?}"
    );
    assert!(
        !unused_dependency_names(&json).contains(&"chalk".to_string()),
        "a normal run credits the dependency that a support entry imports"
    );
}

#[test]
fn support_entry_takes_no_part_in_a_production_run() {
    let dir = support_tools_fixture(&support_entry_config());
    let json = dead_code(dir.path(), &["--production"]);

    let exports = unused_export_names(&json);
    assert!(
        exports.contains(&"src/lib.js:onlyTools".to_string()),
        "production must not credit an import of a support entry: {exports:?}"
    );
    let files = unused_file_paths(&json);
    assert!(
        files.contains(&"src/tool-helper.js".to_string()),
        "a file that only a support entry imports is unused in production: {files:?}"
    );
    assert!(
        unused_dependency_names(&json).contains(&"chalk".to_string()),
        "a dependency that only a support entry imports is unused in production"
    );
    let paths = all_reported_paths(&json);
    assert!(
        !paths.iter().any(|p| p.ends_with("tools/report.js")),
        "production reports no finding for the support entry itself: {paths:?}"
    );
}

#[test]
fn a_file_in_entry_and_support_entry_stays_a_production_entry() {
    let dir = support_tools_fixture(
        r#"{ "entry": ["src/index.js", "tools/*.js"], "supportEntry": ["tools/*.js"] }"#,
    );
    let json = dead_code(dir.path(), &["--production"]);
    let exports = unused_export_names(&json);
    assert!(
        !exports.iter().any(|e| e.ends_with(":onlyTools")),
        "`entry` wins over `supportEntry`: {exports:?}"
    );
}

#[test]
fn production_config_for_dead_code_excludes_support_entries() {
    let dir = support_tools_fixture(
        r#"{ "entry": ["src/index.js"], "supportEntry": ["tools/*.js"],
             "production": { "deadCode": true } }"#,
    );
    let json = dead_code(dir.path(), &[]);
    assert!(
        unused_export_names(&json).contains(&"src/lib.js:onlyTools".to_string()),
        "per-analysis production mode also excludes support entries"
    );
}
