#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `production.ignore` leaves paths out of discovery in production mode only
//! (issue #4622). The default run keeps the paths.

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};
use tempfile::TempDir;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// `src/index.js` is the entry point. `scripts/seed.js` and `src/orphan.js`
/// have no importers, so each is an unused file.
fn fixture(production: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "repro", "private": true, "type": "module" }"#,
    );
    write(root, "src/index.js", "console.log(1);\n");
    write(root, "src/orphan.js", "export const orphan = 1;\n");
    write(root, "scripts/seed.js", "export const seed = 1;\n");
    write(
        root,
        ".fallowrc.json",
        &format!(r#"{{ "entry": ["src/index.js"], "production": {production} }}"#),
    );
    dir
}

fn unused_files(root: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = run_fallow_in_root("dead-code", root, &args);
    assert!(
        output.code == 0 || output.code == 1,
        "dead-code should not crash. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let mut files: Vec<String> = json["unused_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap().replace('\\', "/"))
        .collect();
    files.sort();
    files
}

const OBJECT_WITH_IGNORE: &str = r#"{ "deadCode": true, "ignore": ["scripts/**"] }"#;

#[test]
fn production_ignore_drops_paths_in_production_mode() {
    let dir = fixture(OBJECT_WITH_IGNORE);
    assert_eq!(unused_files(dir.path(), &[]), vec!["src/orphan.js"]);
}

#[test]
fn production_ignore_applies_with_the_production_flag() {
    let dir = fixture(r#"{ "ignore": ["scripts/**"] }"#);
    assert_eq!(
        unused_files(dir.path(), &["--production"]),
        vec!["src/orphan.js"]
    );
}

#[test]
fn production_ignore_keeps_paths_in_the_default_run() {
    let dir = fixture(r#"{ "ignore": ["scripts/**"] }"#);
    assert_eq!(
        unused_files(dir.path(), &[]),
        vec!["scripts/seed.js", "src/orphan.js"]
    );
    let dir = fixture(OBJECT_WITH_IGNORE);
    assert_eq!(
        unused_files(dir.path(), &["--no-production"]),
        vec!["scripts/seed.js", "src/orphan.js"]
    );
}

#[test]
fn boolean_production_form_is_unchanged() {
    let dir = fixture("true");
    assert_eq!(
        unused_files(dir.path(), &[]),
        vec!["scripts/seed.js", "src/orphan.js"]
    );
}

fn health_files_analyzed(root: &Path) -> u64 {
    let output = run_fallow_in_root("health", root, &["--format", "json", "--quiet"]);
    assert!(
        output.code == 0 || output.code == 1,
        "health should not crash. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    parse_json(&output)["summary"]["files_analyzed"]
        .as_u64()
        .unwrap()
}

#[test]
fn production_ignore_follows_the_per_analysis_scope() {
    let dir = fixture(r#"{ "deadCode": true, "health": false, "ignore": ["scripts/**"] }"#);
    assert_eq!(
        health_files_analyzed(dir.path()),
        3,
        "health is not in production mode, so it keeps scripts/"
    );
    let dir = fixture(r#"{ "health": true, "ignore": ["scripts/**"] }"#);
    assert_eq!(
        health_files_analyzed(dir.path()),
        2,
        "health in production mode drops scripts/"
    );
}
