//! `list --entry-points` names the package.json field that declares an entry
//! point (issue #4831).
//!
//! The root package, a workspace package and a nested package of a project
//! without workspaces each report the real field: `main`, `module`,
//! `source`, `browser`, `bin` or `exports`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow_in_root};
use serde_json::{Value, json};
use std::path::Path;
use tempfile::TempDir;

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

/// A project whose root, workspace and nested packages each declare entry
/// points in a different package.json field.
fn field_project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{
            "name": "fields-app",
            "private": true,
            "workspaces": ["packages/*"],
            "main": "src/main.js",
            "module": "src/module.js",
            "source": "src/source.js",
            "browser": "src/browser.js",
            "bin": { "fields": "src/cli.js" },
            "exports": { ".": "./src/index.js" }
        }"#,
    );
    for file in [
        "src/main.js",
        "src/module.js",
        "src/source.js",
        "src/browser.js",
        "src/cli.js",
        "src/index.js",
    ] {
        write(root, file, "export const value = 1;\n");
    }
    write(
        root,
        "packages/lib/package.json",
        r#"{ "name": "lib", "module": "./module.js", "exports": "./entry.js" }"#,
    );
    write(root, "packages/lib/module.js", "export const lib = 1;\n");
    write(root, "packages/lib/entry.js", "export const entry = 1;\n");
    dir
}

/// A project without workspaces. Fallow then reads the package.json files
/// in directories such as `tools/` as nested packages.
fn nested_project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "nested-app", "private": true, "main": "src/index.js" }"#,
    );
    write(root, "src/index.js", "export const app = 1;\n");
    write(
        root,
        "tools/helper/package.json",
        r#"{ "name": "helper", "private": true, "main": "./main.js", "bin": "./bin.js" }"#,
    );
    write(root, "tools/helper/main.js", "export const helper = 1;\n");
    write(root, "tools/helper/bin.js", "export const bin = 1;\n");
    dir
}

fn entry_points_json(root: &Path) -> Vec<Value> {
    let output = run_fallow_in_root(
        "list",
        root,
        &[
            "--entry-points",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(output.code, 0, "list failed: {}", output.stderr);
    let mut json = parse_json(&output);
    let Value::Array(entries) = json["entry_points"].take() else {
        panic!("entry_points is not an array: {json}");
    };
    entries
}

fn source_of<'a>(entries: &'a [Value], path: &str) -> &'a str {
    entries
        .iter()
        .find(|entry| entry["path"] == json!(path))
        .and_then(|entry| entry["source"].as_str())
        .unwrap_or_else(|| panic!("no entry point {path} in {entries:?}"))
}

#[test]
fn list_entry_points_json_names_the_package_json_field() {
    let dir = field_project();
    let entries = entry_points_json(dir.path());
    for (path, source) in [
        ("src/main.js", "package.json main"),
        ("src/module.js", "package.json module"),
        ("src/source.js", "package.json source"),
        ("src/browser.js", "package.json browser"),
        ("src/cli.js", "package.json bin"),
        ("src/index.js", "package.json exports"),
        ("packages/lib/module.js", "package.json module"),
        ("packages/lib/entry.js", "package.json exports"),
    ] {
        assert_eq!(source_of(&entries, path), source, "entry point {path}");
    }
}

#[test]
fn list_entry_points_json_names_the_field_of_a_nested_package() {
    let dir = nested_project();
    let entries = entry_points_json(dir.path());
    for (path, source) in [
        ("src/index.js", "package.json main"),
        ("tools/helper/main.js", "package.json main"),
        ("tools/helper/bin.js", "package.json bin"),
    ] {
        assert_eq!(source_of(&entries, path), source, "entry point {path}");
    }
}

#[test]
fn list_entry_points_human_names_the_package_json_field() {
    let dir = field_project();
    let output = run_fallow_in_root(
        "list",
        dir.path(),
        &["--entry-points", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "list failed: {}", output.stderr);
    for line in [
        "src/cli.js (package.json bin)\n",
        "src/index.js (package.json exports)\n",
        "src/module.js (package.json module)\n",
    ] {
        assert!(output.stdout.contains(line), "stdout: {}", output.stdout);
    }
}

/// A project whose packages declare wildcard targets in fields other than
/// `exports`: the root and workspace packages in `main`, the nested package
/// in `bin`.
fn wildcard_field_project(workspaces: bool) -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    let workspaces_field = if workspaces {
        r#""workspaces": ["packages/*"],"#
    } else {
        ""
    };
    write(
        root,
        "package.json",
        &format!(
            r#"{{ "name": "wildcard-app", "private": true, {workspaces_field} "main": "./src/*.ts" }}"#
        ),
    );
    write(root, "src/a.ts", "export const a = 1;\n");
    write(root, "src/b.ts", "export const b = 1;\n");
    let package_dir = if workspaces { "packages" } else { "tools" };
    write(
        root,
        &format!("{package_dir}/lib/package.json"),
        r#"{ "name": "lib", "private": true, "main": "./src/*.ts", "bin": { "lib": "./bin/*.js" } }"#,
    );
    write(
        root,
        &format!("{package_dir}/lib/src/c.ts"),
        "export const c = 1;\n",
    );
    write(
        root,
        &format!("{package_dir}/lib/bin/run.js"),
        "export const run = 1;\n",
    );
    dir
}

#[test]
fn list_entry_points_json_names_the_field_of_a_wildcard_target() {
    for (workspaces, package_dir) in [(true, "packages"), (false, "tools")] {
        let dir = wildcard_field_project(workspaces);
        let entries = entry_points_json(dir.path());
        for (path, source) in [
            ("src/a.ts".to_owned(), "package.json main"),
            ("src/b.ts".to_owned(), "package.json main"),
            (format!("{package_dir}/lib/src/c.ts"), "package.json main"),
            (format!("{package_dir}/lib/bin/run.js"), "package.json bin"),
        ] {
            assert_eq!(source_of(&entries, &path), source, "entry point {path}");
        }
    }
}
