//! A workspace package can depend on a local folder with a `file:` or `link:`
//! spec. pnpm installs a `file:` folder as a copy in its store, so the import
//! does not resolve to the project source through a symlink. The local folder
//! is a workspace, the same as for a `file:` spec in the root manifest.

use std::fs;
use std::path::Path;

use super::common::create_config;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    fs::write(path, source).expect("write fixture file");
}

#[test]
fn local_path_dependency_of_a_workspace_package_is_a_workspace() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{"name": "nested-local-path-root", "private": true, "workspaces": ["apps/*"]}"#,
    );
    write(
        root,
        "apps/app/package.json",
        r#"{
  "name": "@sample/app",
  "main": "src/index.ts",
  "dependencies": { "@sample/native-text": "file:./modules/native-text" }
}"#,
    );
    write(
        root,
        "apps/app/src/index.ts",
        "import { NativeText } from \"@sample/native-text\";\nexport const view = NativeText;\n",
    );
    write(
        root,
        "apps/app/modules/native-text/package.json",
        r#"{"name": "@sample/native-text", "main": "./index.ts"}"#,
    );
    write(
        root,
        "apps/app/modules/native-text/index.ts",
        "export { NativeText } from \"./src/NativeText\";\n",
    );
    write(
        root,
        "apps/app/modules/native-text/src/NativeText.ts",
        "export const NativeText = \"text\";\n",
    );

    let results = fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis");
    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(root)
                .expect("project-relative path")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(
        unused.is_empty(),
        "the local dependency folder is reachable: {unused:?}"
    );
}

/// The local package has its own manifest, so an import of a dependency that
/// only the parent package lists is unlisted in the local package.
#[test]
fn local_path_dependency_checks_imports_against_its_own_manifest() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{"name": "nested-local-path-root", "private": true, "workspaces": ["apps/*"]}"#,
    );
    write(
        root,
        "apps/app/package.json",
        r#"{
  "name": "@sample/app",
  "main": "src/index.ts",
  "dependencies": { "@sample/helper": "file:./modules/helper", "shared-lib": "*" }
}"#,
    );
    write(
        root,
        "apps/app/src/index.ts",
        "import { helper } from \"@sample/helper\";\nexport const value = helper;\n",
    );
    write(
        root,
        "apps/app/modules/helper/package.json",
        r#"{"name": "@sample/helper", "main": "./index.ts"}"#,
    );
    write(
        root,
        "apps/app/modules/helper/index.ts",
        "import shared from \"shared-lib\";\nexport const helper = shared;\n",
    );

    let results = fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis");
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert_eq!(unlisted, ["shared-lib"]);
    let unused: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert_eq!(unused, ["shared-lib"]);
}
