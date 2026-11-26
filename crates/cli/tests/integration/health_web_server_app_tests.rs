//! The app that a Playwright `webServer` starts from a nested, non-workspace
//! package is test-side code for the coverage gap rules. See issue #5270.

use std::path::Path;

use tempfile::tempdir;

use super::common::{copy_fixture_tree, fixture_path, parse_json, run_fallow_in_root};

const FIXTURE: &str = "issue-5270-playwright-webserver-app";

/// The untested file paths and the `path:export` names of untested exports.
fn coverage_gaps(root: &Path) -> (Vec<String>, Vec<String>) {
    let output = run_fallow_in_root(
        "health",
        root,
        &["--coverage-gaps", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    let coverage = &json["coverage_gaps"];
    let path_of = |item: &serde_json::Value| {
        item["path"]
            .as_str()
            .expect("finding has a path")
            .replace('\\', "/")
    };
    let files = coverage["files"]
        .as_array()
        .expect("coverage_gaps.files is an array")
        .iter()
        .map(path_of)
        .collect();
    let exports = coverage["exports"]
        .as_array()
        .expect("coverage_gaps.exports is an array")
        .iter()
        .map(|item| {
            let name = item["export_name"].as_str().expect("export has a name");
            format!("{}:{name}", path_of(item))
        })
        .collect();
    (files, exports)
}

fn unused_files(root: &Path) -> Vec<String> {
    let output = run_fallow_in_root("dead-code", root, &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    json["unused_files"]
        .as_array()
        .expect("unused_files is an array")
        .iter()
        .map(|item| {
            item["path"]
                .as_str()
                .expect("finding has a path")
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn web_server_app_files_are_not_coverage_gaps() {
    let root = fixture_path(FIXTURE);
    let (files, exports) = coverage_gaps(&root);

    assert_eq!(
        files,
        ["src/format.ts", "src/index.ts"],
        "only the library files are runtime code"
    );
    assert_eq!(
        exports,
        ["src/format.ts:formatLabel", "src/index.ts:createForm"],
        "the export of the web server app is not a coverage gap"
    );
}

#[test]
fn web_server_app_files_stay_used() {
    let root = fixture_path(FIXTURE);
    let unused = unused_files(&root);
    assert!(
        !unused.iter().any(|path| path.starts_with("app/")),
        "the web server app keeps its files alive, got {unused:?}"
    );
}

#[test]
fn workspace_package_started_by_web_server_stays_runtime_code() {
    let dir = tempdir().expect("create temp dir");
    copy_fixture_tree(&fixture_path(FIXTURE), dir.path());
    let manifest_path = dir.path().join("package.json");
    let manifest = std::fs::read_to_string(&manifest_path).expect("read package.json");
    let manifest = manifest.replacen(
        "\"type\": \"module\",",
        "\"type\": \"module\",\n  \"workspaces\": [\"app\"],",
        1,
    );
    std::fs::write(&manifest_path, manifest).expect("write package.json");

    let (files, _) = coverage_gaps(dir.path());
    assert!(
        files.iter().any(|path| path == "app/src/page.ts"),
        "a workspace package is production code, got {files:?}"
    );
}
