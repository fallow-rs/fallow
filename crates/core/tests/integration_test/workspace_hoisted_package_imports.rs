//! Dependency accounting for workspace packages in an npm-hoisted layout.
//!
//! npm and yarn link every workspace package into the root `node_modules`, so
//! a package can import a sibling that it does not declare. The import then
//! resolves through the root install symlink to the sibling source file. A
//! direct `@repro/lib/...` import and a package `imports` alias that targets
//! `@repro/lib` must give the same unlisted-dependency result.

use std::path::Path;

use fallow_types::results::AnalysisResults;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Link a workspace package into the root `node_modules`, as npm does.
fn link_hoisted_package(root: &Path, name: &str, target: &str) {
    let scope_dir = root.join("node_modules/@repro");
    std::fs::create_dir_all(&scope_dir).expect("create node_modules scope");
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join(target), scope_dir.join(name)).expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(root.join(target), scope_dir.join(name)).expect("symlink");
}

struct AppManifest<'a> {
    imports: &'a str,
    dependencies: &'a str,
}

fn create_project(root: &Path, app: &AppManifest<'_>, app_source: &str, lib_source: &str) {
    write(
        &root.join("package.json"),
        r#"{ "name": "root", "private": true, "workspaces": ["packages/*"] }"#,
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
        &root.join("packages/lib/src/bye.ts"),
        "export const bye = () => \"bye\";\n",
    );
    write(&root.join("packages/lib/src/index.ts"), lib_source);
    write(
        &root.join("packages/app/package.json"),
        &format!(
            r#"{{
                "name": "@repro/app",
                "type": "module",
                "private": true,
                "imports": {},
                "dependencies": {},
                "exports": {{ ".": "./src/index.ts" }}
            }}"#,
            app.imports, app.dependencies
        ),
    );
    write(
        &root.join("packages/app/src/local.ts"),
        "export const bye = () => \"local\";\n",
    );
    write(&root.join("packages/app/src/index.ts"), app_source);
    link_hoisted_package(root, "lib", "packages/lib");
    link_hoisted_package(root, "app", "packages/app");
}

fn analyze(app: &AppManifest<'_>, app_source: &str, lib_source: &str) -> AnalysisResults {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical temp dir");
    create_project(&root, app, app_source, lib_source);
    fallow_core::analyze(&create_config(root)).expect("analysis should succeed")
}

/// Return `(package, file, line)` for each unlisted-dependency import site.
fn unlisted_sites(results: &AnalysisResults) -> Vec<(String, String, u32)> {
    let mut sites: Vec<(String, String, u32)> = results
        .unlisted_dependencies
        .iter()
        .flat_map(|finding| {
            finding.dep.imported_from.iter().map(|site| {
                let file = site
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (finding.dep.package_name.clone(), file, site.line)
            })
        })
        .collect();
    sites.sort();
    sites
}

fn unused_dependency_names(results: &AnalysisResults) -> Vec<&str> {
    results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect()
}

const LIB_ALIAS: &str = r##"{ "#lib/*": "@repro/lib/*" }"##;
const NO_DEPS: &str = "{}";
const DECLARED_LIB: &str = r#"{ "@repro/lib": "*" }"#;
const PLAIN_LIB: &str = "export const lib = 1;\n";

#[test]
fn direct_import_of_undeclared_hoisted_workspace_package_is_unlisted() {
    let results = analyze(
        &AppManifest {
            imports: NO_DEPS,
            dependencies: NO_DEPS,
        },
        "import { bye } from \"@repro/lib/bye\";\nconsole.log(bye());\n",
        PLAIN_LIB,
    );

    assert_eq!(
        unlisted_sites(&results),
        vec![("@repro/lib".to_string(), "index.ts".to_string(), 1)],
        "a direct import of an undeclared workspace package must be unlisted"
    );
}

#[test]
fn direct_and_alias_imports_of_undeclared_workspace_package_agree() {
    let alias_only = analyze(
        &AppManifest {
            imports: LIB_ALIAS,
            dependencies: NO_DEPS,
        },
        "import { hello } from \"#lib/hello\";\nconsole.log(hello());\n",
        PLAIN_LIB,
    );
    let direct_only = analyze(
        &AppManifest {
            imports: LIB_ALIAS,
            dependencies: NO_DEPS,
        },
        "import { bye } from \"@repro/lib/bye\";\nconsole.log(bye());\n",
        PLAIN_LIB,
    );

    assert_eq!(unlisted_sites(&alias_only), unlisted_sites(&direct_only));
    assert_eq!(
        unlisted_sites(&direct_only),
        vec![("@repro/lib".to_string(), "index.ts".to_string(), 1)]
    );
}

#[test]
fn declared_hoisted_workspace_dependency_stays_silent() {
    let results = analyze(
        &AppManifest {
            imports: LIB_ALIAS,
            dependencies: DECLARED_LIB,
        },
        "import { hello } from \"#lib/hello\";\nimport { bye } from \"@repro/lib/bye\";\nconsole.log(hello(), bye());\n",
        PLAIN_LIB,
    );

    assert!(
        unlisted_sites(&results).is_empty(),
        "a declared workspace dependency must not be unlisted, got {:?}",
        unlisted_sites(&results)
    );
    assert!(
        !unused_dependency_names(&results).contains(&"@repro/lib"),
        "a declared and imported workspace dependency must not be unused"
    );
}

#[test]
fn workspace_package_self_import_stays_silent() {
    let results = analyze(
        &AppManifest {
            imports: NO_DEPS,
            dependencies: NO_DEPS,
        },
        "console.log(\"app\");\n",
        "import { bye } from \"@repro/lib/bye\";\nexport const lib = bye();\n",
    );

    assert!(
        unlisted_sites(&results).is_empty(),
        "a package that imports itself by name must not be unlisted, got {:?}",
        unlisted_sites(&results)
    );
}

#[test]
fn root_package_import_of_workspace_package_stays_silent() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical temp dir");
    create_project(
        &root,
        &AppManifest {
            imports: NO_DEPS,
            dependencies: NO_DEPS,
        },
        "console.log(\"app\");\n",
        PLAIN_LIB,
    );
    write(
        &root.join("test/lib.test.js"),
        "import { bye } from \"@repro/lib/bye\";\nconsole.log(bye());\n",
    );

    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");

    assert!(
        unlisted_sites(&results).is_empty(),
        "the root package links every workspace package through its `workspaces` field, got {:?}",
        unlisted_sites(&results)
    );
}

/// Node.js takes the first valid target of an `imports` fallback array, so the
/// local `./src/local.ts` target wins and `@repro/lib` is not used.
const LOCAL_FIRST_FALLBACK: &str = r##"{ "#bye": ["./src/local.ts", "@repro/lib/bye"] }"##;

#[test]
fn fallback_array_with_local_first_target_does_not_credit_workspace_package() {
    let results = analyze(
        &AppManifest {
            imports: LOCAL_FIRST_FALLBACK,
            dependencies: NO_DEPS,
        },
        "import { bye } from \"#bye\";\nconsole.log(bye());\n",
        PLAIN_LIB,
    );

    assert!(
        unlisted_sites(&results).is_empty(),
        "#bye resolves to the local file, so @repro/lib is not used, got {:?}",
        unlisted_sites(&results)
    );
}

#[test]
fn fallback_array_with_local_first_target_leaves_declared_workspace_package_unused() {
    let results = analyze(
        &AppManifest {
            imports: LOCAL_FIRST_FALLBACK,
            dependencies: DECLARED_LIB,
        },
        "import { bye } from \"#bye\";\nconsole.log(bye());\n",
        PLAIN_LIB,
    );

    assert!(
        unused_dependency_names(&results).contains(&"@repro/lib"),
        "#bye resolves to the local file, so @repro/lib must stay unused, got {:?}",
        unused_dependency_names(&results)
    );
}
