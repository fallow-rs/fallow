//! A pnpm workspace that declares gitignored member directories, as when CI
//! clones upstream repositories into the workspace before the install.
//!
//! Source discovery does not read the files of a gitignored member, so the run
//! cannot prove that the dependencies of its manifest are unused. A dependency
//! alias `workspace:<name>@<range>` resolves to the aliased workspace package,
//! also when a member with the alias name exists.

use std::path::Path;

use super::common::create_config;

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent directory")).expect("directory");
    std::fs::write(path, content).expect("file");
}

/// Make the workspace in a temporary git repository. `clone/` is gitignored and
/// holds a package named `vite` without a `./pack` export. `packages/app`
/// declares `vite` as an alias of the workspace package `@scope/core`.
fn write_workspace(root: &Path) {
    std::fs::create_dir_all(root.join(".git")).expect("git directory");
    write(root, ".gitignore", "clone\n");
    write(root, "package.json", r#"{"name":"root","private":true}"#);
    write(
        root,
        "pnpm-workspace.yaml",
        "packages:\n  - packages/*\n  - clone\n  - clone/packages/*\n",
    );
    write(
        root,
        "packages/app/package.json",
        r#"{"name":"app","main":"./src/index.ts","dependencies":{"vite":"workspace:@scope/core@*","left-pad":"^1.0.0"}}"#,
    );
    write(
        root,
        "packages/app/src/index.ts",
        "import { pack } from 'vite/pack';\nexport const run = pack;\n",
    );
    write(
        root,
        "packages/core/package.json",
        r#"{"name":"@scope/core","exports":{".":"./src/index.ts","./pack":"./src/pack.ts"}}"#,
    );
    write(
        root,
        "packages/core/src/index.ts",
        "export const core = 1;\n",
    );
    write(
        root,
        "packages/core/src/pack.ts",
        "export const pack = 1;\n",
    );
    write(
        root,
        "clone/package.json",
        r#"{"name":"vite","exports":{".":"./index.js"},"devDependencies":{"clone-tool":"^1.0.0"}}"#,
    );
    write(root, "clone/index.js", "export const vite = 1;\n");
    write(
        root,
        "clone/packages/lib/package.json",
        r#"{"name":"clone-lib","dependencies":{"lodash":"^4.0.0"}}"#,
    );
    write(
        root,
        "clone/packages/lib/src/index.js",
        "import lodash from 'lodash';\nexport default lodash;\n",
    );
}

#[test]
fn gitignored_workspace_members_have_no_dependency_findings() {
    let directory = tempfile::tempdir().expect("temporary project directory");
    let root = directory.path();
    write_workspace(root);

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");

    let mut unused: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .chain(
            results
                .unused_dev_dependencies
                .iter()
                .map(|finding| finding.dep.package_name.as_str()),
        )
        .collect();
    unused.sort_unstable();
    assert_eq!(
        unused,
        vec!["left-pad"],
        "a gitignored member has no read sources, so its dependencies are not reported; \
         the dependency of a member that the run reads is still reported"
    );
}

#[test]
fn workspace_alias_resolves_to_the_aliased_package() {
    let directory = tempfile::tempdir().expect("temporary project directory");
    let root = directory.path();
    write_workspace(root);

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "`vite` is an alias of `@scope/core`, so `vite/pack` resolves through its exports; \
         unresolved_imports = {unresolved:?}"
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("packages/core/src/pack.ts")),
        "the alias import reaches packages/core/src/pack.ts; unused_files = {unused_files:?}"
    );
}

#[test]
fn workspace_alias_subpath_without_export_is_unresolved() {
    let directory = tempfile::tempdir().expect("temporary project directory");
    let root = directory.path();
    write_workspace(root);
    write(
        root,
        "packages/app/src/index.ts",
        "import { pack } from 'vite/pack';\nimport { client } from 'vite/client';\nexport const run = [pack, client];\n",
    );

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert_eq!(
        unresolved,
        vec!["vite/client"],
        "pnpm links `vite` to `@scope/core`, which has no `./client` export"
    );
    let unused: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unused.contains(&"vite"),
        "the importer uses the alias; unused_dependencies = {unused:?}"
    );
}
