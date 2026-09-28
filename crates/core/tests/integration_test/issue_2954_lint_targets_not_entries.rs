//! Issue #2954: formatter and linter targets are not entry points.
//!
//! `oxfmt --check "**/*.ts"` or `eslint src/x.ts` reads the files but does not
//! execute them. The tool stays a used dependency, but its file arguments must
//! not make the files reachable. `node scripts/x.ts` still creates an entry.

use super::common::{create_config, fixture_path};

fn unused_file_paths(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn unused_dev_dependency_names(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect()
}

fn is_reported(paths: &[String], suffix: &str) -> bool {
    paths.iter().any(|p| p.ends_with(suffix))
}

#[test]
fn package_json_formatter_and_linter_targets_do_not_seed_entries() {
    let root = fixture_path("issue-2954-script-lint-targets");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    for dead in ["src/dead.ts", "src/dead-lint.ts", "src/dead-prettier.ts"] {
        assert!(
            is_reported(&paths, dead),
            "{dead} is only a formatter or linter target and must stay unused. Got: {paths:?}"
        );
    }
    assert!(
        !is_reported(&paths, "scripts/tool.ts"),
        "scripts/tool.ts runs through `node` and must stay an entry. Got: {paths:?}"
    );
    for loaded in ["tools/fmt.js", "tools/prettier-plugin.mjs"] {
        assert!(
            !is_reported(&paths, loaded),
            "{loaded} is loaded through a formatter or plugin flag and must stay reachable. \
             Got: {paths:?}"
        );
    }
    assert!(
        !is_reported(&paths, "src/index.ts"),
        "src/index.ts is the package main. Got: {paths:?}"
    );

    let unused_dev = unused_dev_dependency_names(&results);
    for tool in ["oxfmt", "eslint", "oxlint", "prettier"] {
        assert!(
            !unused_dev.iter().any(|name| name == tool),
            "{tool} runs in a script and must stay a used dependency. Got: {unused_dev:?}"
        );
    }
}

#[test]
fn ci_formatter_and_linter_targets_do_not_seed_entries() {
    let root = fixture_path("issue-2954-ci-lint-targets");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    for dead in ["src/dead.ts", "src/dead-lint.ts"] {
        assert!(
            is_reported(&paths, dead),
            "{dead} is only a formatter or linter target in CI and must stay unused. \
             Got: {paths:?}"
        );
    }
    assert!(
        !is_reported(&paths, "scripts/ci-tool.ts"),
        "scripts/ci-tool.ts runs through `node` in CI and must stay an entry. Got: {paths:?}"
    );

    let unused_dev = unused_dev_dependency_names(&results);
    for tool in ["oxfmt", "eslint"] {
        assert!(
            !unused_dev.iter().any(|name| name == tool),
            "{tool} runs in CI and must stay a used dependency. Got: {unused_dev:?}"
        );
    }
}
