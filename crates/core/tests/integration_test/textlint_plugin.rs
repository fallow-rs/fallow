//! Integration tests for the textlint plugin.

use super::common::{create_config, fixture_path};

fn unused_file_paths(
    root: &std::path::Path,
    results: &fallow_types::results::AnalysisResults,
) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn unused_dev_dependency_names(results: &fallow_types::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect()
}

fn analyze(fixture: &str) -> (std::path::PathBuf, fallow_types::results::AnalysisResults) {
    let root = fixture_path(fixture);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    (root, results)
}

#[test]
fn textlint_json_config_credits_packages_by_short_name() {
    // textlint loads rules, filter rules and plugins by short name. A script
    // can name another config with `--config`.
    let (_, results) = analyze("textlint-plugin/json");

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in [
        "textlint",
        "textlint-rule-no-todo",
        "textlint-rule-max-comma",
        "@textlint-rule/textlint-rule-no-unmatched-pair",
        "textlint-rule-preset-ja-technical-writing",
        "@textlint-ja/textlint-rule-preset-ai-writing",
        "textlint-filter-rule-comments",
        "@textlint/textlint-plugin-markdown",
        "textlint-plugin-html",
        "textlint-rule-spelling",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by the textlint plugin, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    for dep in ["textlint-rule-write-good", "unused-control"] {
        assert!(
            unused_dev_dependencies.contains(&dep),
            "{dep} is disabled or unreferenced and should still report, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
}

#[test]
fn textlint_js_config_keeps_local_rules_reachable() {
    // A JS config can name local rule modules through computed keys.
    let (root, results) = analyze("textlint-plugin/js");

    let unused_paths = unused_file_paths(&root, &results);
    for path in [
        ".textlintrc.cjs",
        "rules/check-document.cjs",
        "rules/check-heading.js",
        "rules/check-link/index.js",
    ] {
        assert!(
            !unused_paths.contains(&path.to_string()),
            "{path} should be reachable through the textlint config, unused files: {unused_paths:?}"
        );
    }
    assert!(
        unused_paths.contains(&"src/orphan.js".to_string()),
        "ordinary unused files should still report, unused files: {unused_paths:?}"
    );
}

#[test]
fn textlint_package_json_key_credits_packages() {
    let (_, results) = analyze("textlint-plugin/package-json");

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in [
        "textlint",
        "textlint-rule-preset-smarthr",
        "textlint-filter-rule-allowlist",
        "textlint-plugin-latex2e",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by the package.json textlint key, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, unused dev deps: {unused_dev_dependencies:?}"
    );
}

#[test]
fn textlint_yaml_config_credits_enabled_packages() {
    let (_, results) = analyze("textlint-plugin/yaml");

    let unused_dev_dependencies = unused_dev_dependency_names(&results);
    for dep in [
        "textlint",
        "textlint-rule-no-todo",
        "textlint-filter-rule-comments",
        "textlint-plugin-markdown",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited by the YAML textlint config, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
    for dep in ["textlint-rule-write-good", "unused-control"] {
        assert!(
            unused_dev_dependencies.contains(&dep),
            "{dep} should still be reported, unused dev deps: {unused_dev_dependencies:?}"
        );
    }
}
