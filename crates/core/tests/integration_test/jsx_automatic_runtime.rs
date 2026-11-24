//! Integration tests for the dependency credit of the automatic JSX runtime.
//!
//! With the automatic runtime, the compiler adds an import of
//! `react/jsx-runtime` to each file with JSX, so `react` is in use without an
//! import statement. The fixtures (`tests/fixtures/jsx-automatic-runtime/`)
//! cover a Next.js app, a tsconfig `jsx` mode of `react-jsx` and
//! `react-jsxdev`, and the controls where no credit applies: a project
//! without JSX, a `preserve` mode, and a workspace package without JSX.

use super::common::{create_config, fixture_path};

/// The unused dependencies as `(package.json path, package)`, with the path
/// relative to `root`.
fn unused_dependencies(
    results: &fallow_core::results::AnalysisResults,
    root: &std::path::Path,
) -> Vec<(String, String)> {
    results
        .unused_dependencies
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

fn analyze_fixture(name: &str) -> (std::path::PathBuf, Vec<(String, String)>) {
    let root = fixture_path(&format!("jsx-automatic-runtime/{name}"));
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_dependencies(&results, &root);
    (root, unused)
}

fn reports_react(unused: &[(String, String)], manifest: &str) -> bool {
    unused
        .iter()
        .any(|(path, package)| path == manifest && package == "react")
}

#[test]
fn nextjs_app_with_jsx_credits_react() {
    let (_, unused) = analyze_fixture("nextjs");
    assert!(
        !reports_react(&unused, "package.json"),
        "a Next.js file with JSX uses `react`, unused: {unused:?}"
    );
}

#[test]
fn tsconfig_react_jsx_credits_react() {
    let (_, unused) = analyze_fixture("tsconfig-react-jsx");
    assert!(
        !reports_react(&unused, "package.json"),
        "`jsx: react-jsx` with a JSX file uses `react`, unused: {unused:?}"
    );
}

#[test]
fn tsconfig_react_jsxdev_credits_react() {
    let (_, unused) = analyze_fixture("tsconfig-react-jsxdev");
    assert!(
        !reports_react(&unused, "package.json"),
        "`jsx: react-jsxdev` with a JSX file uses `react`, unused: {unused:?}"
    );
}

#[test]
fn tsconfig_react_jsx_without_jsx_files_reports_react() {
    let (_, unused) = analyze_fixture("tsconfig-no-jsx");
    assert!(
        reports_react(&unused, "package.json"),
        "a project without JSX files does not use `react`, unused: {unused:?}"
    );
}

#[test]
fn tsconfig_preserve_reports_react() {
    let (_, unused) = analyze_fixture("tsconfig-preserve");
    assert!(
        reports_react(&unused, "package.json"),
        "`jsx: preserve` adds no runtime import, unused: {unused:?}"
    );
}

#[test]
fn nextjs_credit_stays_in_the_nextjs_workspace() {
    let (_, unused) = analyze_fixture("nextjs-workspace");
    assert!(
        !reports_react(&unused, "apps/web/package.json"),
        "the Next.js workspace uses `react`, unused: {unused:?}"
    );
    assert!(
        reports_react(&unused, "packages/lib/package.json"),
        "a workspace without JSX does not use `react`, unused: {unused:?}"
    );
}
