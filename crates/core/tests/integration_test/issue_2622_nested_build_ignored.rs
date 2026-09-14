//! Built-in discovery ignores treat `build/` as generated output at any depth,
//! so a monorepo that keeps per-package build output no longer reports it.

use std::fs;
use std::path::Path;

use super::common::create_config;

#[test]
fn nested_build_output_is_not_analyzed() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(root, "package.json", r#"{ "name": "root" }"#);
    write_file(root, "build/index.js", "export const rootOutput = 1;\n");
    write_file(
        root,
        "projects/app/build/index.js",
        "export const packageOutput = 1;\n",
    );
    write_file(
        root,
        "projects/app/build/server/chunks/entry.js",
        "export const chunk = 1;\n",
    );
    write_file(root, "projects/app/src/main.ts", "export const main = 1;\n");

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| relative(root, &f.file.path))
        .collect();

    assert!(
        unused.iter().all(|path| !has_build_segment(path)),
        "generated build output should not be analyzed: {unused:?}"
    );
    assert_eq!(unused, vec!["projects/app/src/main.ts".to_string()]);
}

/// Excluding a directory removes it from the graph but not from the filesystem,
/// so an import that reaches into it still resolves. Without this the fix would
/// trade false unused-file findings for false unresolved-import findings.
#[test]
fn imports_into_an_excluded_build_directory_still_resolve() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(root, "package.json", r#"{ "name": "root" }"#);
    write_file(
        root,
        "packages/tooling/build/steps.ts",
        "export const compileStep = () => 1;\n",
    );
    write_file(
        root,
        "packages/tooling/src/index.ts",
        "import { compileStep } from '../build/steps';\nconsole.log(compileStep());\n",
    );

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "import into an excluded directory should still resolve: {:?}",
        results.unresolved_imports
    );
}

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write fixture file");
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn has_build_segment(path: &str) -> bool {
    path.split('/').any(|segment| segment == "build")
}
