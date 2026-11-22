#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow plugin-check` evaluates an external plugin in each package that
//! runs it: the analysis activates a plugin with the dependencies of one
//! workspace and resolves its manifest globs from that workspace root.

use std::fs;
use std::path::Path;

use crate::common::{parse_json, run_fallow_raw};

const ROUTES_PLUGIN: &str = r#"{"name":"routes","enablers":["route-kit"],"manifestEntries":[{"manifests":"routes.json","entries":[{"path":"${entry}"}]}]}"#;

fn plugin_check(root: &Path) -> serde_json::Value {
    let output = run_fallow_raw(&[
        "plugin-check",
        "--root",
        root.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    parse_json(&output)
}

/// A root with `packages/*` workspaces and an app whose routes file names a
/// page.
fn monorepo(root: &Path, app_manifest: &str) {
    write(
        root,
        "package.json",
        r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
    );
    write(root, "packages/app/package.json", app_manifest);
    write(
        root,
        "packages/app/routes.json",
        r#"{"entry":"src/page.ts"}"#,
    );
    write(root, "packages/app/src/page.ts", "export const page = 1;\n");
    write(root, "fallow-plugin-routes.jsonc", ROUTES_PLUGIN);
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn plugin_check_reads_the_workspace_that_enables_the_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
    );
    write(
        root,
        "packages/app/package.json",
        r#"{"name":"app","dependencies":{"route-kit":"1.0.0"}}"#,
    );
    write(
        root,
        "packages/app/routes.json",
        r#"{"entry":"src/page.ts"}"#,
    );
    write(root, "packages/app/src/page.ts", "export const page = 1;\n");
    write(root, "packages/other/package.json", r#"{"name":"other"}"#);
    write(
        root,
        "fallow-plugin-routes.jsonc",
        r#"{"name":"routes","enablers":["route-kit"],"manifestEntries":[{"manifests":"routes.json","entries":[{"path":"${entry}"}]}]}"#,
    );

    let output = run_fallow_raw(&[
        "plugin-check",
        "--root",
        root.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let plugin = &json["plugins"][0];
    assert_eq!(plugin["active"], true, "{json}");
    assert_eq!(
        plugin["active_in"],
        serde_json::json!(["packages/app"]),
        "{json}"
    );
    let rule = &plugin["manifest_rules"][0];
    assert_eq!(rule["package"], "packages/app", "{json}");
    assert_eq!(
        rule["manifests_matched"],
        serde_json::json!(["packages/app/routes.json"]),
        "{json}"
    );
    assert_eq!(
        rule["matched"][0]["seeded"][0]["path"], "packages/app/src/page.ts",
        "{json}"
    );
    assert_eq!(
        rule["matched"][0]["seeded"][0]["path_exists"], true,
        "{json}"
    );
    assert_eq!(rule["warnings"], serde_json::json!([]), "{json}");
}

#[test]
fn plugin_check_counts_a_root_tool_that_a_workspace_script_calls() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    monorepo(
        root,
        r#"{"name":"app","scripts":{"build":"route-kit build"}}"#,
    );
    write(
        root,
        "package.json",
        r#"{"name":"root","private":true,"workspaces":["packages/*"],"devDependencies":{"route-kit":"1.0.0"}}"#,
    );
    write(
        root,
        "node_modules/route-kit/package.json",
        r#"{"name":"route-kit","version":"1.0.0","bin":{"route-kit":"cli.js"}}"#,
    );

    let json = plugin_check(root);
    assert_eq!(
        json["plugins"][0]["active_in"],
        serde_json::json!([".", "packages/app"]),
        "{json}"
    );
}

#[test]
fn plugin_check_reads_a_workspace_that_only_the_config_declares() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    monorepo(
        root,
        r#"{"name":"app","dependencies":{"route-kit":"1.0.0"}}"#,
    );
    write(root, "package.json", r#"{"name":"root","private":true}"#);
    write(
        root,
        ".fallowrc.json",
        r#"{"workspaces":{"patterns":["packages/*"]}}"#,
    );

    let json = plugin_check(root);
    assert_eq!(
        json["plugins"][0]["active_in"],
        serde_json::json!(["packages/app"]),
        "{json}"
    );
}

#[test]
fn plugin_check_skips_a_root_without_a_package_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "routes.json", r#"{"entry":"page.ts"}"#);
    write(
        root,
        "fallow-plugin-routes.jsonc",
        r#"{"name":"routes","detection":{"type":"fileExists","pattern":"routes.json"}}"#,
    );

    let json = plugin_check(root);
    assert_eq!(json["plugins"][0]["active"], false, "{json}");
}
