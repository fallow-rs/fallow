//! `list --entry-points` and `--trace-file` name the package.json script that
//! makes a file an entry point (issue #4175).
//!
//! The `source` string of a script entry point stays `package.json script`.
//! The script key is in the separate `script` field.

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

/// A project with a `main` entry, two scripts that run the same file, and a
/// script that runs a second file.
fn script_project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{
            "name": "scripts-app",
            "private": true,
            "main": "src/index.js",
            "scripts": {
                "seed": "node scripts/seed.js",
                "build": "node scripts/build.js",
                "prebuild": "node scripts/build.js --check"
            }
        }"#,
    );
    write(root, "src/index.js", "export const app = 1;\n");
    write(root, "scripts/build.js", "export const build = 1;\n");
    write(root, "scripts/seed.js", "export const seed = 1;\n");
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

#[test]
fn list_entry_points_json_names_the_script() {
    let dir = script_project();
    let entries = entry_points_json(dir.path());
    assert_eq!(
        entries,
        vec![
            json!({ "path": "scripts/build.js", "source": "package.json script", "script": "build" }),
            json!({ "path": "scripts/seed.js", "source": "package.json script", "script": "seed" }),
            json!({ "path": "src/index.js", "source": "package.json main" }),
        ]
    );
}

#[test]
fn list_entry_points_human_names_the_script() {
    let dir = script_project();
    let output = run_fallow_in_root(
        "list",
        dir.path(),
        &["--entry-points", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "list failed: {}", output.stderr);
    assert!(
        output
            .stdout
            .contains("scripts/build.js (package.json script `build`)"),
        "stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("src/index.js (package.json main)\n"),
        "stdout: {}",
        output.stdout
    );
}

#[test]
fn trace_file_names_the_script() {
    let dir = script_project();
    let root = dir.path();
    let json = parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--trace-file",
            "scripts/build.js",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ));
    assert_eq!(json["is_entry_point"], json!(true));
    assert_eq!(json["entry_script"], json!("build"));

    let main = parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--trace-file",
            "src/index.js",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ));
    assert_eq!(main["is_entry_point"], json!(true));
    assert!(main.get("entry_script").is_none(), "trace: {main}");

    let human = run_fallow_in_root(
        "dead-code",
        root,
        &["--trace-file", "scripts/build.js", "--quiet", "--no-cache"],
    );
    let text = format!("{}{}", human.stdout, human.stderr);
    assert!(
        text.contains("(entry point, package.json script `build`)"),
        "trace: {text}"
    );
}
