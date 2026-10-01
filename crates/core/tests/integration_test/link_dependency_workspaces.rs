//! A root `package.json` that lists local packages as `link:` or `file:`
//! dependencies, with no `workspaces` field, no `pnpm-workspace.yaml` and no
//! tsconfig `paths`. Older yarn monorepos such as Kibana use this layout, with
//! packages three to five directories below the root.

use std::fs;
use std::path::Path;

use super::common::{create_config, create_config_with_ignore_patterns};

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

fn unused_dependency_keys(
    root: &Path,
    results: &fallow_core::results::AnalysisResults,
) -> Vec<String> {
    let mut keys: Vec<String> = results
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
    keys.sort_unstable();
    keys
}

#[test]
fn link_targets_that_source_discovery_skips_do_not_become_workspaces() {
    // A yalc copy lives in a hidden directory, and a build output or an
    // ignored vendor directory is not walked. A workspace there gives a
    // workspace root without source files, so each import of the package
    // becomes an unresolved import.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_file(
        root,
        "package.json",
        r#"{
          "name": "app",
          "private": true,
          "main": "index.ts",
          "dependencies": {
            "ylib": "file:.yalc/ylib",
            "blib": "link:build/blib",
            "dlib": "link:dist/dlib",
            "vlib": "link:vendor/deep/vlib"
          }
        }"#,
    );
    for (dir, name) in [
        (".yalc/ylib", "ylib"),
        ("build/blib", "blib"),
        ("dist/dlib", "dlib"),
        ("vendor/deep/vlib", "vlib"),
    ] {
        write_file(
            root,
            &format!("{dir}/package.json"),
            &format!(r#"{{"name": "{name}", "main": "index.js"}}"#),
        );
        write_file(
            root,
            &format!("{dir}/index.js"),
            "export const value = 1;\n",
        );
    }
    write_file(
        root,
        "index.ts",
        "import { value as y } from 'ylib';\n\
         import { value as b } from 'blib';\n\
         import { value as d } from 'dlib';\n\
         import { value as v } from 'vlib';\n\
         export const total = y + b + d + v;\n",
    );

    let config = create_config_with_ignore_patterns(root.to_path_buf(), &["vendor/**"]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert!(unresolved.is_empty(), "{unresolved:?}");
    assert!(
        unused_dependency_keys(root, &results).is_empty(),
        "{:?}",
        results.unused_dependencies
    );
}

#[test]
fn root_link_entry_for_a_glob_declared_workspace_is_still_an_unused_dependency() {
    // The `workspaces` glob declares `packages/a`. The root `link:` entry is
    // a second declaration, so an entry that no code uses is reported, as
    // before.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_file(
        root,
        "package.json",
        r#"{
          "name": "repo",
          "private": true,
          "workspaces": ["packages/*"],
          "dependencies": {
            "a": "link:packages/a",
            "tool": "link:tools/deep/tool"
          }
        }"#,
    );
    write_file(
        root,
        "packages/a/package.json",
        r#"{"name": "a", "main": "index.ts"}"#,
    );
    write_file(root, "packages/a/index.ts", "export const a = 1;\n");
    write_file(
        root,
        "tools/deep/tool/package.json",
        r#"{"name": "tool", "main": "index.ts"}"#,
    );
    write_file(root, "tools/deep/tool/index.ts", "export const tool = 1;\n");

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");

    // `tool` has no other declaration, so its root entry stays exempt.
    assert_eq!(unused_dependency_keys(root, &results), ["a@package.json"]);
}
