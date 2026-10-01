//! A root `package.json` that lists local packages as `link:` or `file:`
//! dependencies, with no `workspaces` field, no `pnpm-workspace.yaml` and no
//! tsconfig `paths`. Older yarn monorepos such as Kibana use this layout, with
//! packages three to five directories below the root.

use std::fs;
use std::path::Path;

use super::common::create_config;

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

/// Write the Kibana-depth layout. `@kbn/app` lists `@kbn/foo` with a relative
/// `link:` spec, the way a yarn-era package declares a sibling.
fn write_kibana_depth_project(root: &Path) {
    write_file(
        root,
        "package.json",
        r#"{
          "name": "kibana",
          "private": true,
          "dependencies": {
            "@kbn/foo": "link:src/platform/packages/shared/kbn-foo",
            "@kbn/app": "link:x-pack/solutions/search/plugins/app",
            "left-pad": "1.3.0"
          }
        }"#,
    );
    write_file(
        root,
        "src/platform/packages/shared/kbn-foo/package.json",
        r#"{"name": "@kbn/foo", "private": true}"#,
    );
    write_file(
        root,
        "src/platform/packages/shared/kbn-foo/index.ts",
        "export { used } from './src/used';\n",
    );
    write_file(
        root,
        "src/platform/packages/shared/kbn-foo/src/used.ts",
        "export const used = () => 1;\n",
    );
    write_file(
        root,
        "src/platform/packages/shared/kbn-foo/src/deep.ts",
        "export const deep = 2;\nexport const deepUnused = 3;\n",
    );
    write_file(
        root,
        "x-pack/solutions/search/plugins/app/package.json",
        r#"{
          "name": "@kbn/app",
          "private": true,
          "dependencies": {
            "@kbn/foo": "link:../../../../../src/platform/packages/shared/kbn-foo"
          }
        }"#,
    );
    write_file(
        root,
        "x-pack/solutions/search/plugins/app/index.ts",
        "import { used } from '@kbn/foo';\n\
         import { deep } from '@kbn/foo/src/deep';\n\
         export const run = () => used() + deep;\n",
    );
}

#[test]
fn deep_link_dependency_targets_resolve_as_workspace_sources() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_kibana_depth_project(root);

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| relative(root, &finding.file.path))
        .collect();
    assert!(
        unused_files.is_empty(),
        "linked package sources must be reachable: {unused_files:?}"
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.clone())
        .collect();
    assert_eq!(unused_exports, ["deepUnused"]);

    assert!(
        results.unresolved_imports.is_empty(),
        "{:?}",
        results.unresolved_imports
    );
    assert!(
        results.unlisted_dependencies.is_empty(),
        "{:?}",
        results.unlisted_dependencies
    );
}

#[test]
fn root_link_entries_for_linked_workspaces_are_not_unused_dependencies() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_kibana_depth_project(root);

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_dependencies
        .iter()
        .map(|finding| {
            format!(
                "{}@{}",
                finding.dep.package_name,
                relative(root, &finding.dep.path)
            )
        })
        .collect();
    // The root `link:` entry is how this layout declares `@kbn/app`. No code
    // imports it, but removing the entry also removes the package from the
    // project, so it is not an unused dependency. `left-pad` is the control.
    assert_eq!(unused, ["left-pad@package.json"]);
}
