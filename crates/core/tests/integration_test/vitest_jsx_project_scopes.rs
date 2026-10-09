//! Integration tests for the files that each Vitest project transforms with a
//! config JSX import source.
//!
//! The fixture (`tests/fixtures/vitest-jsx-project-scopes/`) has these
//! projects:
//! - an inline project without `extends`, which inherits the root runtime,
//! - an inline project with `extends: '<path>'`, which uses the runtime of
//!   the named file,
//! - an inline project with `extends: false` that excludes its only test
//!   directory, so its runtime gets no edge,
//! - a nested `vitest.config.ts` and a `vitest.e2e.config.ts` that each set
//!   their own runtime,
//! - a `vitest.e2e.config.ts` that a glob entry names, with its own runtime.
//!
//! The `vitest-jsx-default-react/` fixtures cover a config that sets no
//! import source, where Vite uses the `react` runtime. The
//! `vitest-jsx-vitest4-projects/` fixture covers the Vitest 4 project model.

use super::common::{create_config, fixture_path};

fn unused_file_paths(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn unused_dev_dependency_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect()
}

/// The unused devDependencies as `(package.json path, package)`, with the
/// path relative to `root`.
fn unused_dev_dependencies_by_manifest(
    results: &fallow_core::results::AnalysisResults,
    root: &std::path::Path,
) -> Vec<(String, String)> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|d| {
            let path = d.dep.path.strip_prefix(root).unwrap_or(&d.dep.path);
            (
                path.to_string_lossy().replace('\\', "/"),
                d.dep.package_name.clone(),
            )
        })
        .collect()
}

#[test]
fn project_runtimes_follow_vitest_project_scopes() {
    let root = fixture_path("vitest-jsx-project-scopes");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = unused_file_paths(&results);

    for used in [
        "rt-root/jsx-dev-runtime.ts",
        "rt-base/jsx-dev-runtime.ts",
        "base.config.ts",
        "nested/rt-own/jsx-dev-runtime.ts",
        "e2e/rt-own/jsx-dev-runtime.ts",
        "e2e/vitest.e2e.config.ts",
        "runtime/r1/rt-own/jsx-dev-runtime.ts",
        "runtime/r1/vitest.e2e.config.ts",
    ] {
        assert!(
            !unused_files.iter().any(|path| path.ends_with(used)),
            "`{used}` must be reachable, unused: {unused_files:?}"
        );
    }
    assert!(
        unused_files
            .iter()
            .any(|path| path.ends_with("rt-skip/jsx-dev-runtime.ts")),
        "an excluded project directory must not reach its runtime, unused: {unused_files:?}"
    );
}

#[test]
fn default_react_runtime_credits_react_for_jsx_test_files() {
    let root = fixture_path("vitest-jsx-default-react/with-jsx");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_dev_dependency_names(&results);
    assert!(
        !unused.contains(&"react".to_string()),
        "the default JSX runtime uses `react`, unused: {unused:?}"
    );
}

#[test]
fn default_react_runtime_needs_a_jsx_test_file() {
    let root = fixture_path("vitest-jsx-default-react/no-jsx");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_dev_dependency_names(&results);
    assert!(
        unused.contains(&"react".to_string()),
        "without a JSX test file, `react` stays unused: {unused:?}"
    );
}

#[test]
fn tsconfig_jsx_import_source_replaces_the_default_react_runtime() {
    let root = fixture_path("vitest-jsx-default-react/tsconfig-preact");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_dev_dependency_names(&results);
    assert!(
        unused.contains(&"react".to_string()),
        "a tsconfig `jsxImportSource` replaces `react` for TypeScript files: {unused:?}"
    );
    assert!(
        !unused.contains(&"preact".to_string()),
        "the tsconfig runtime package is used: {unused:?}"
    );
}

#[test]
fn default_react_runtime_credits_only_the_workspace_of_the_test_file() {
    let root = fixture_path("vitest-jsx-default-react/workspaces");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_dev_dependencies_by_manifest(&results, &root);
    assert!(
        !unused.contains(&("packages/a/package.json".to_string(), "react".to_string())),
        "the JSX test file of `a` uses `react`, unused: {unused:?}"
    );
    assert!(
        unused.contains(&("packages/b/package.json".to_string(), "react".to_string())),
        "a JSX test file of `a` must not hide the unused `react` of `b`: {unused:?}"
    );
}

#[test]
fn vite_config_next_to_a_vitest_config_gives_no_runtime_credit() {
    let root = fixture_path("vitest-jsx-default-react/shadowed-vite-config");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_dev_dependency_names(&results);
    assert!(
        unused.contains(&"react".to_string()),
        "Vitest does not load the vite config, so `react` is unused: {unused:?}"
    );
    assert!(
        !unused.contains(&"preact".to_string()),
        "the vitest config runtime package is used: {unused:?}"
    );
}

#[test]
fn vitest4_inline_project_without_extends_does_not_inherit_the_runtime() {
    let root = fixture_path("vitest-jsx-vitest4-projects");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = unused_file_paths(&results);
    assert!(
        unused_files
            .iter()
            .any(|path| path.ends_with("rt/jsx-dev-runtime.ts")),
        "on Vitest 4 the project does not use the root runtime, unused: {unused_files:?}"
    );
}
