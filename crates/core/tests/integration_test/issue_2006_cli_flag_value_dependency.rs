//! Regression tests for issue #2006: `eslint-formatter-gha` reported as an
//! unused devDependency while it is used via `npx eslint --format gha` in a
//! GitHub Actions workflow.
//!
//! CI `run:` commands are already scanned and their binaries credited, but flag
//! values were discarded, so a package named only as a flag argument had no
//! reference anywhere. eslint expands `--format gha` to `eslint-formatter-gha`
//! through a documented shorthand.

use std::path::Path;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

fn create_project(root: &Path, run_command: &str) {
    write(
        &root.join("package.json"),
        r#"{
            "name": "eslint-formatter-repro",
            "private": true,
            "main": "src/index.ts",
            "devDependencies": {
                "eslint": "^9.0.0",
                "eslint-formatter-gha": "^2.0.1",
                "left-pad": "^1.3.0"
            }
        }"#,
    );
    write(&root.join("src/index.ts"), r"export const value = 1;");
    write(
        &root.join(".github/workflows/ci.yml"),
        &format!(
            r"name: CI
on: [push]
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - run: {run_command}
"
        ),
    );
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
fn issue_2006_eslint_formatter_flag_value_credits_the_package() {
    for command in [
        "npx eslint --format gha",
        "npx eslint --format=gha",
        "npx eslint -f gha",
    ] {
        let dir = tempfile::tempdir().expect("temp dir");
        create_project(dir.path(), command);
        let results = analyze(dir.path());

        let unused = unused_dev_dependencies(&results);
        assert!(
            !unused.contains(&"eslint-formatter-gha"),
            "`{command}` should credit the formatter package, got {unused:?}"
        );
        assert!(
            unused.contains(&"left-pad"),
            "an unrelated unused devDependency must still be reported for `{command}`, got {unused:?}"
        );
    }
}

/// Built-in formatters ship inside eslint, so they must not synthesize a package
/// name, and a synthesized name must never become an unlisted dependency.
#[test]
#[cfg_attr(miri, ignore)]
fn issue_2006_builtin_formatter_credits_nothing_and_invents_no_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    create_project(dir.path(), "npx eslint --format stylish");
    let results = analyze(dir.path());

    let unused = unused_dev_dependencies(&results);
    assert!(
        unused.contains(&"eslint-formatter-gha"),
        "a formatter that no command names stays unused, got {unused:?}"
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
