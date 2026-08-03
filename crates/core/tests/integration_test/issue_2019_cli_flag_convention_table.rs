//! Regression tests for issue #2019: the CLI flag-value convention table.
//!
//! #2006 credited packages named as eslint `--format` values. Other CLIs
//! follow the same pattern, where a flag value names an npm package that has
//! no other reference in the project: `mocha --reporter mochawesome`,
//! `node -r dotenv/config`, `jest --testEnvironment jsdom`. The conventions
//! live in `crates/core/data/cli_flag_credits.toml`; a (binary, flag) pair
//! without a row must keep abstaining.

use std::path::Path;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

fn unused_dev_dependencies(results: &fallow_types::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect()
}

fn analyze(root: &Path) -> fallow_types::results::AnalysisResults {
    let config = create_config(root.to_path_buf());
    fallow_core::analyze(&config).expect("analysis should succeed")
}

#[test]
#[cfg_attr(miri, ignore)]
fn issue_2019_convention_table_credits_flag_named_packages() {
    let dir = tempfile::tempdir().expect("temp dir");
    write(
        &dir.path().join("package.json"),
        r#"{
            "name": "flag-convention-repro",
            "private": true,
            "main": "src/index.ts",
            "scripts": {
                "test": "mocha --reporter mochawesome",
                "test:unit": "jest --testEnvironment jsdom",
                "start": "node -r dotenv/config src/index.js"
            },
            "devDependencies": {
                "mocha": "^10.0.0",
                "jest": "^29.0.0",
                "mochawesome": "^7.1.3",
                "jest-environment-jsdom": "^29.0.0",
                "dotenv": "^16.4.5",
                "left-pad": "^1.3.0"
            }
        }"#,
    );
    write(&dir.path().join("src/index.ts"), r"export const value = 1;");
    let results = analyze(dir.path());

    let unused = unused_dev_dependencies(&results);
    for credited in ["mochawesome", "jest-environment-jsdom", "dotenv"] {
        assert!(
            !unused.contains(&credited),
            "{credited} is named by a flag value and must be credited, got {unused:?}"
        );
    }
    assert!(
        unused.contains(&"left-pad"),
        "an unrelated unused devDependency must still be reported, got {unused:?}"
    );
}

/// stylelint `--formatter` and nyc `--reporter` deliberately have no catalogue
/// rows (no naming convention, unstable built-in set), so a package whose name
/// only appears as such a flag value stays unused and no dependency is
/// invented. `pretty` and `lcov` are real npm packages that collide with those
/// values, which is exactly why a generic bare-token rule is refused.
#[test]
#[cfg_attr(miri, ignore)]
fn issue_2019_unlisted_convention_still_abstains() {
    let dir = tempfile::tempdir().expect("temp dir");
    write(
        &dir.path().join("package.json"),
        r#"{
            "name": "flag-convention-control",
            "private": true,
            "main": "src/index.ts",
            "scripts": {
                "lint:css": "stylelint --formatter pretty src/**/*.css",
                "coverage": "nyc --reporter=lcov node src/index.js"
            },
            "devDependencies": {
                "stylelint": "^16.0.0",
                "nyc": "^17.0.0",
                "pretty": "^2.0.0",
                "lcov": "^1.16.0"
            }
        }"#,
    );
    write(&dir.path().join("src/index.ts"), r"export const value = 1;");
    let results = analyze(dir.path());

    let unused = unused_dev_dependencies(&results);
    assert!(
        unused.contains(&"pretty"),
        "an unlisted convention must not credit the bare value, got {unused:?}"
    );
    assert!(
        unused.contains(&"lcov"),
        "an unlisted convention must not credit the bare value, got {unused:?}"
    );

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        unlisted.is_empty(),
        "flag-value credit must never create an unlisted dependency, got {unlisted:?}"
    );
}
