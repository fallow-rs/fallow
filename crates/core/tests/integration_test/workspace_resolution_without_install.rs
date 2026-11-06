//! Module resolution across workspaces of a monorepo whose dependencies are
//! not installed and whose packages are not built.

use std::path::Path;

use super::common::create_config;

fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dir");
        std::fs::write(path, content).expect("write file");
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unresolved_specifiers(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.clone())
        .collect()
}

fn unused_dependency_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.clone())
        .collect()
}

/// A tsconfig `paths` entry that maps a package name to its `@types` package
/// in one workspace must not make the imports of that package unresolved,
/// in that workspace or in another one.
#[test]
fn types_package_path_alias_of_one_workspace_keeps_package_imports_resolved() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_files(
        root,
        &[
            ("package.json", r#"{"name":"root","private":true}"#),
            ("pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n"),
            (
                "packages/plain/package.json",
                r#"{"name":"plain","private":true,"main":"src/index.ts","dependencies":{"react":"19.0.0"}}"#,
            ),
            (
                "packages/plain/tsconfig.json",
                r#"{"compilerOptions":{"jsx":"preserve"}}"#,
            ),
            (
                "packages/plain/src/index.ts",
                "import { useState } from 'react';\nexport const plain = useState;\n",
            ),
            (
                "packages/typed/package.json",
                r#"{"name":"typed","private":true,"main":"src/index.ts","dependencies":{"react":"19.0.0"}}"#,
            ),
            (
                "packages/typed/tsconfig.json",
                r#"{"compilerOptions":{"paths":{"react":["./node_modules/@types/react"]}}}"#,
            ),
            (
                "packages/typed/src/index.ts",
                "import { useState } from 'react';\nexport const typed = useState;\n",
            ),
        ],
    );

    let results = fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis");

    assert!(
        unresolved_specifiers(&results).is_empty(),
        "react imports must resolve as a package: {:?}",
        unresolved_specifiers(&results)
    );
    assert!(
        !unused_dependency_names(&results).contains(&"react".to_string()),
        "react is imported in both workspaces: {:?}",
        unused_dependency_names(&results)
    );
}

fn unlisted_dependency_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.clone())
        .collect()
}

/// A tsconfig `paths` entry onto another package in one workspace does not
/// apply to the files of another workspace. Their imports keep the package
/// name that they declare.
#[test]
fn package_path_alias_of_one_workspace_does_not_redirect_other_workspaces() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_files(
        root,
        &[
            ("package.json", r#"{"name":"root","private":true}"#),
            ("pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n"),
            (
                "packages/plain/package.json",
                r#"{"name":"plain","private":true,"main":"src/index.ts","dependencies":{"lodash":"4.0.0"}}"#,
            ),
            (
                "packages/plain/src/index.ts",
                "import { map } from 'lodash';\nexport const plain = map;\n",
            ),
            (
                "packages/aliased/package.json",
                r#"{"name":"aliased","private":true,"main":"src/index.ts","dependencies":{"lodash-es":"4.0.0"}}"#,
            ),
            (
                "packages/aliased/tsconfig.json",
                r#"{"compilerOptions":{"paths":{"lodash":["./node_modules/lodash-es"]}}}"#,
            ),
            (
                "packages/aliased/src/index.ts",
                "import { map } from 'lodash';\nexport const aliased = map;\n",
            ),
        ],
    );

    let results = fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis");

    assert!(
        unresolved_specifiers(&results).is_empty(),
        "{:?}",
        unresolved_specifiers(&results)
    );
    assert!(
        unused_dependency_names(&results).is_empty(),
        "each workspace uses its declared package: {:?}",
        unused_dependency_names(&results)
    );
    assert!(
        unlisted_dependency_names(&results).is_empty(),
        "{:?}",
        unlisted_dependency_names(&results)
    );
}

/// An alias into an installed `node_modules` directory whose package is
/// missing is a broken alias, and the import stays unresolved.
#[test]
fn project_alias_onto_missing_package_of_an_install_stays_unresolved() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_files(
        root,
        &[
            (
                "package.json",
                r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
            ),
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"paths":{"lodash":["./node_modules/lodash-es"]}}}"#,
            ),
            ("node_modules/other/package.json", r#"{"name":"other"}"#),
            (
                "packages/app/package.json",
                r#"{"name":"app","private":true,"main":"src/index.ts","dependencies":{"lodash":"4.0.0"}}"#,
            ),
            ("packages/app/tsconfig.json", r#"{"compilerOptions":{}}"#),
            (
                "packages/app/src/index.ts",
                "import { map } from 'lodash';\nexport const app = map;\n",
            ),
        ],
    );

    let results = fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis");

    assert_eq!(unresolved_specifiers(&results), vec!["lodash".to_string()]);
}

/// A package `bin` script imports the JavaScript output of a tsconfig with
/// `emitDeclarationOnly: true`. Another tool emits that JavaScript into
/// `outDir`, so the import maps back to the source file under `rootDir`.
#[test]
fn emit_declaration_only_output_import_maps_to_source() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_files(
        root,
        &[
            ("package.json", r#"{"name":"root","private":true}"#),
            ("pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n"),
            (
                "packages/cli/package.json",
                r#"{"name":"cli","private":true,"type":"module","bin":{"cli":"bin/cli.js"}}"#,
            ),
            (
                "packages/cli/tsconfig.json",
                r#"{"compilerOptions":{"rootDir":"./src","outDir":"./dist","emitDeclarationOnly":true,"declaration":true}}"#,
            ),
            (
                "packages/cli/bin/cli.js",
                "import { main } from '../dist/cli.js';\nmain();\n",
            ),
            ("packages/cli/src/cli.ts", "export const main = () => 1;\n"),
        ],
    );

    let results = fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| relative(root, &file.file.path))
        .collect();
    assert!(
        !unused_files.contains(&"packages/cli/src/cli.ts".to_string()),
        "the bin script reaches the source entry: {unused_files:?}"
    );
    assert!(
        unresolved_specifiers(&results).is_empty(),
        "the output import maps to source: {:?}",
        unresolved_specifiers(&results)
    );
}

/// An `exports` subpath lists `types` first with a declaration file that is
/// not built, then a `source` branch. With the `source` condition, an import
/// of that subpath from another workspace resolves to the source file.
#[test]
fn unbuilt_types_branch_falls_through_to_source_condition() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_files(
        root,
        &[
            (
                "package.json",
                r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
            ),
            (
                "packages/app/package.json",
                r#"{"name":"@scope/app","exports":{".":{"source":"./src/index.ts","default":"./dist/index.js"}},"dependencies":{"@scope/panel":"workspace:*"}}"#,
            ),
            (
                "packages/app/src/index.ts",
                "import { a } from '@scope/panel/admin';\nexport const b = a;\n",
            ),
            (
                "packages/panel/package.json",
                r#"{"name":"@scope/panel","exports":{"./admin":{"types":"./dist/admin/src/index.d.ts","source":"./admin/src/index.ts","import":"./dist/admin/index.mjs","require":"./dist/admin/index.js","default":"./dist/admin/index.js"},"./package.json":"./package.json"}}"#,
            ),
            ("packages/panel/admin/src/index.ts", "export const a = 1;\n"),
        ],
    );

    let mut config = create_config(root.to_path_buf());
    config.resolve.conditions = vec!["source".to_string()];
    let results = fallow_core::analyze(&config).expect("analysis");

    assert!(
        unresolved_specifiers(&results).is_empty(),
        "the subpath resolves through the source branch: {:?}",
        unresolved_specifiers(&results)
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| relative(root, &file.file.path))
        .collect();
    assert!(
        !unused_files.contains(&"packages/panel/admin/src/index.ts".to_string()),
        "the import reaches the source file: {unused_files:?}"
    );
    assert!(
        !unused_dependency_names(&results).contains(&"@scope/panel".to_string()),
        "the workspace dependency is imported: {:?}",
        unused_dependency_names(&results)
    );
}
