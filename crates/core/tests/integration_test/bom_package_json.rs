//! A `package.json` that starts with a UTF-8 byte order mark is a valid
//! manifest for npm and Node.js, so every dependency check reads it.

use std::fs;
use std::path::Path;

use fallow_config::{FallowConfig, OutputFormat};

const BOM: &str = "\u{FEFF}";

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

fn analyze(root: &Path) -> fallow_types::results::AnalysisResults {
    let config = FallowConfig::default().resolve(
        root.to_path_buf(),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn unused_dependency_names(results: &fallow_types::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect()
}

#[test]
fn workspace_manifest_with_a_bom_reports_its_unused_dependency() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
    );
    write(
        root,
        "packages/app/package.json",
        &format!(
            r#"{BOM}{{"name":"app","main":"src/index.js","dependencies":{{"left-pad":"^1.0.0"}}}}"#
        ),
    );
    write(root, "packages/app/src/index.js", "export const a = 1;\n");

    assert_eq!(unused_dependency_names(&analyze(root)), ["left-pad"]);
}

#[test]
fn manifest_config_key_with_a_bom_credits_the_tool() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        &format!(
            r#"{BOM}{{"name":"root","main":"index.js","devDependencies":{{"prettier":"^3.0.0"}},"prettier":{{"semi":false}}}}"#
        ),
    );
    write(root, "index.js", "export const a = 1;\n");

    let results = analyze(root);
    assert!(
        !results
            .unused_dev_dependencies
            .iter()
            .any(|dep| dep.dep.package_name == "prettier"),
        "the `prettier` key configures prettier: {:?}",
        results.unused_dev_dependencies
    );
}

#[test]
fn root_manifest_with_a_bom_reports_its_unused_npm_override() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        &format!(
            r#"{BOM}{{"name":"root","main":"index.js","dependencies":{{"react":"^18.0.0"}},"overrides":{{"axios":"^1.6.0"}}}}"#
        ),
    );
    write(root, "index.js", "import 'react';\n");

    let results = analyze(root);
    let unused: Vec<&str> = results
        .unused_dependency_overrides
        .iter()
        .map(|finding| finding.entry.target_package.as_str())
        .collect();
    assert_eq!(unused, ["axios"]);
}
