//! Regression tests for issue #2952: a workspace dependency that a package
//! imports only through a package.json `imports` alias (`#lib/*` ->
//! `@repro/lib/*`) was reported as an unused dependency. The alias resolved
//! through the pnpm install symlink to the workspace source file, so the import
//! edge existed, but the dependency on the target workspace package was not
//! credited.

use std::path::Path;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

fn link_workspace_package(root: &Path, consumer: &str, scope: &str, name: &str, target: &str) {
    let scope_dir = root.join(consumer).join("node_modules").join(scope);
    std::fs::create_dir_all(&scope_dir).expect("create node_modules scope");
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join(target), scope_dir.join(name)).expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(root.join(target), scope_dir.join(name)).expect("symlink");
}

/// Reproduce the reporter's pnpm workspace, including the install symlink.
fn create_project(root: &Path, app_source: &str) {
    write(
        &root.join("package.json"),
        r#"{ "name": "root", "private": true }"#,
    );
    write(
        &root.join("pnpm-workspace.yaml"),
        "packages: [\"packages/*\"]\n",
    );
    write(
        &root.join("packages/lib/package.json"),
        r#"{ "name": "@repro/lib", "type": "module", "exports": { "./*": "./src/*.ts" } }"#,
    );
    write(
        &root.join("packages/lib/src/hello.ts"),
        "export const hello = () => \"hello\";\n",
    );
    write(
        &root.join("packages/app/package.json"),
        r##"{
            "name": "@repro/app",
            "type": "module",
            "private": true,
            "imports": { "#lib/*": "@repro/lib/*" },
            "dependencies": { "@repro/lib": "workspace:*" },
            "exports": { ".": "./src/index.ts" }
        }"##,
    );
    write(&root.join("packages/app/src/index.ts"), app_source);
    link_workspace_package(root, "packages/app", "@repro", "lib", "packages/lib");
}

fn unused_dependency_names(results: &fallow_types::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect()
}

#[test]
fn package_imports_alias_credits_target_workspace_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical temp dir");
    create_project(
        &root,
        "import { hello } from \"#lib/hello\";\nconsole.log(hello());\n",
    );

    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");

    let unused = unused_dependency_names(&results);
    assert!(
        !unused.contains(&"@repro/lib"),
        "@repro/lib is used through the #lib/* imports alias, got unused: {unused:?}"
    );
    assert!(
        results.unresolved_imports.is_empty(),
        "the #lib/hello alias should resolve, got: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|finding| &finding.import.specifier)
            .collect::<Vec<_>>()
    );
    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();
    assert!(
        !unused_exports.contains(&"hello"),
        "hello is imported through the alias, got unused exports: {unused_exports:?}"
    );
}

#[test]
fn package_imports_alias_without_import_keeps_dependency_unused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical temp dir");
    create_project(&root, "console.log(\"no workspace import\");\n");

    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");

    let unused = unused_dependency_names(&results);
    assert!(
        unused.contains(&"@repro/lib"),
        "an imports alias that no file uses must not credit @repro/lib, got unused: {unused:?}"
    );
}
