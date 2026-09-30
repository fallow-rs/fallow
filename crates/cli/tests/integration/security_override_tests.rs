//! Per-path `overrides[].rules` for the security rules (issue #2985): an `off`
//! override drops the security candidates in matching files, an `error`
//! override makes `fallow security` exit 1, and files outside the override
//! glob keep the top-level severity.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap/expect to keep fixture setup concise"
)]

use crate::common::fallow_bin;
use std::path::Path;
use std::process::{Command, Stdio};

const SINK_SOURCE: &str = "export function run(code) {\n  return eval(code);\n}\n";

/// Write a project with one `eval` sink in `src/generated/client.js` and one in
/// `src/handwritten.js`, plus a `.fallowrc.json` with `overrides`.
fn write_project(root: &Path, overrides: &str) {
    std::fs::create_dir_all(root.join("src/generated")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "security-override-fixture", "private": true, "type": "module", "main": "src/index.js" }"#,
    )
    .expect("package");
    std::fs::write(root.join("src/generated/client.js"), SINK_SOURCE).expect("client");
    std::fs::write(root.join("src/handwritten.js"), SINK_SOURCE).expect("handwritten");
    std::fs::write(
        root.join("src/index.js"),
        "import { run } from \"./generated/client.js\";\nimport { run as runLocal } from \"./handwritten.js\";\nexport const result = run(\"1 + 1\") + runLocal(\"2\");\n",
    )
    .expect("index");
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(r#"{{ "overrides": {overrides} }}"#),
    )
    .expect("config");
}

/// Run `fallow security --format json` and return `(exit_code, sink paths)`.
fn run_security(root: &Path) -> (i32, Vec<String>) {
    let out = Command::new(fallow_bin())
        .args(["security", "--format", "json", "--quiet", "--no-cache"])
        .arg("--root")
        .arg(root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run fallow security");
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("security output should be valid JSON");
    let mut paths: Vec<String> = json["security_findings"]
        .as_array()
        .expect("security_findings array")
        .iter()
        .filter(|finding| finding["kind"] == "tainted-sink")
        .map(|finding| finding["path"].as_str().expect("path").replace('\\', "/"))
        .collect();
    paths.sort();
    (out.status.code().unwrap_or(-1), paths)
}

#[test]
fn security_override_off_drops_findings_in_matching_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_project(
        dir.path(),
        r#"[{ "files": ["src/generated/**"], "rules": { "security-sink": "off" } }]"#,
    );

    let (code, paths) = run_security(dir.path());

    assert_eq!(code, 0);
    assert_eq!(paths, vec!["src/handwritten.js".to_owned()]);
}

#[test]
fn security_override_error_fails_the_run_while_top_level_is_warn() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_project(
        dir.path(),
        r#"[{ "files": ["src/generated/**"], "rules": { "security-sink": "error" } }]"#,
    );

    let (code, paths) = run_security(dir.path());

    assert_eq!(code, 1);
    assert_eq!(
        paths,
        vec![
            "src/generated/client.js".to_owned(),
            "src/handwritten.js".to_owned()
        ]
    );
}

#[test]
fn security_override_does_not_change_files_outside_its_glob() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_project(
        dir.path(),
        r#"[{ "files": ["src/vendor/**"], "rules": { "security-sink": "error" } },
            { "files": ["src/legacy/**"], "rules": { "security-sink": "off" } }]"#,
    );

    let (code, paths) = run_security(dir.path());

    assert_eq!(code, 0);
    assert_eq!(
        paths,
        vec![
            "src/generated/client.js".to_owned(),
            "src/handwritten.js".to_owned()
        ]
    );
}
