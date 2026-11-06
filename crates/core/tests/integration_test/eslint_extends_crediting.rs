//! Credit for packages and files that an ESLint `extends` entry names.
//!
//! - An `extends` entry with a subpath (`eslint-config-shared/back`) loads a
//!   file of the package `eslint-config-shared`. The package gets the credit,
//!   not the full specifier.
//! - When that package is a workspace package linked into `node_modules`, the
//!   file that ESLint loads is used.
//! - A workspace flat config (`eslint.config.mjs`) with an `import` must not
//!   hide the root `.eslintrc.json` from config discovery.

use std::path::Path;

use super::common::create_config;

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent dir")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

fn unused_dev_dependencies(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect()
}

#[cfg(unix)]
fn unused_file_paths(results: &fallow_core::results::AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn write_subpath_extends_project(root: &Path) {
    write(
        root,
        "package.json",
        r#"{"name":"root","private":true,"workspaces":["apps/*","packages/*"],"devDependencies":{"eslint":"8.57.1"}}"#,
    );
    write(
        root,
        "apps/api/package.json",
        r#"{"name":"api","main":"index.js","devDependencies":{"eslint-config-shared":"1.0.0"}}"#,
    );
    write(root, "apps/api/index.js", "module.exports = 1;\n");
    write(
        root,
        "apps/api/.eslintrc.cjs",
        "module.exports = { root: true, extends: ['eslint-config-shared/back/typescript'] };\n",
    );
    write(
        root,
        "apps/web/package.json",
        r#"{"name":"web","main":"index.js","devDependencies":{"eslint-config-shared":"1.0.0"}}"#,
    );
    write(root, "apps/web/index.js", "module.exports = 1;\n");
    write(
        root,
        "apps/web/.eslintrc.json",
        r#"{"root":true,"extends":["shared/front"]}"#,
    );
    write(
        root,
        "packages/eslint-config-shared/package.json",
        r#"{"name":"eslint-config-shared","private":true,"main":"index.js"}"#,
    );
    write(
        root,
        "packages/eslint-config-shared/back/typescript.js",
        "module.exports = { rules: {} };\n",
    );
    write(
        root,
        "packages/eslint-config-shared/front/index.js",
        "module.exports = { rules: {} };\n",
    );
}

#[test]
fn extends_subpath_credits_the_config_package() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_subpath_extends_project(root);

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_dev_dependencies(&results);
    assert!(
        !unused.iter().any(|name| name == "eslint-config-shared"),
        "an extends entry with a subpath must credit its package: {unused:?}"
    );
}

#[cfg(unix)]
#[test]
fn extends_subpath_credits_the_files_of_a_linked_workspace_config_package() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_subpath_extends_project(root);
    std::fs::create_dir_all(root.join("node_modules")).expect("create node_modules");
    std::os::unix::fs::symlink(
        "../packages/eslint-config-shared",
        root.join("node_modules/eslint-config-shared"),
    )
    .expect("link workspace package");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_file_paths(&results, root);
    assert!(
        !unused
            .iter()
            .any(|path| path.starts_with("packages/eslint-config-shared/")),
        "files that an extends entry loads must be used: {unused:?}"
    );
}

#[test]
fn workspace_flat_config_with_import_keeps_root_eslintrc_credit() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"root","private":true,"workspaces":["apps/*"],"devDependencies":{"eslint":"9.26.0","eslint-config-prettier":"8.8.0"}}"#,
    );
    write(
        root,
        ".eslintrc.json",
        r#"{"root":true,"extends":["prettier"]}"#,
    );
    write(
        root,
        "apps/web/package.json",
        r#"{"name":"web","private":true,"main":"src/index.ts","devDependencies":{"@eslint/js":"9.26.0"}}"#,
    );
    write(root, "apps/web/src/index.ts", "export const value = 1;\n");
    write(
        root,
        "apps/web/eslint.config.mjs",
        "import js from '@eslint/js';\nexport default [js.configs.recommended];\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_dev_dependencies(&results);
    assert!(
        !unused.iter().any(|name| name == "eslint-config-prettier"),
        "the root .eslintrc.json extends must keep its credit: {unused:?}"
    );
    assert!(
        !unused.iter().any(|name| name == "@eslint/js"),
        "the workspace flat config import must keep its credit: {unused:?}"
    );
}
