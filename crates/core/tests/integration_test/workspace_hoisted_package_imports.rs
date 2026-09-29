//! Dependency accounting for workspace packages in an npm-hoisted layout.
//!
//! npm, yarn classic and bun link every workspace package into the root
//! `node_modules`, so a package can import a sibling that it does not declare.
//! The import then resolves through the root install symlink to the sibling
//! source file. A direct `@repro/lib/...` import and a package `imports` alias
//! that targets `@repro/lib` must give the same unlisted-dependency result.
//!
//! A root file can import a workspace package without a declaration only when
//! the install links the package into the root `node_modules`. After an
//! install, the link on disk decides. Without an install, the package manager
//! settings predict the link: pnpm links only with a hoisting setting, yarn
//! berry only with the `node-modules` linker, and bun only with the hoisted
//! linker.

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

/// Build the hoisted project, add a root test file that imports `@repro/lib`,
/// apply the package manager layout, and return the unlisted import sites.
fn root_import_sites(layout: impl FnOnce(&Path)) -> Vec<(String, String, u32)> {
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
    layout(&root);
    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");
    unlisted_sites(&results)
}

fn root_lib_unlisted() -> Vec<(String, String, u32)> {
    vec![("@repro/lib".to_string(), "lib.test.js".to_string(), 1)]
}

/// Replace the root manifest with a pnpm root: no `workspaces` field, and the
/// packages come from `pnpm-workspace.yaml`.
fn pnpm_root(root: &Path, workspace_yaml_extra: &str) {
    write(
        &root.join("package.json"),
        r#"{ "name": "root", "private": true }"#,
    );
    write(
        &root.join("pnpm-workspace.yaml"),
        &format!("packages:\n  - \"packages/*\"\n{workspace_yaml_extra}"),
    );
    write(&root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n");
}

/// Remove the root install links, as pnpm without hoisting and yarn PnP do.
fn remove_root_links(root: &Path) {
    std::fs::remove_dir_all(root.join("node_modules")).expect("remove root node_modules");
}

#[test]
fn npm_root_import_of_workspace_package_stays_silent() {
    let sites = root_import_sites(|root| {
        write(&root.join("package-lock.json"), "{}");
    });
    assert!(
        sites.is_empty(),
        "npm links every workspace package into the root node_modules, got {sites:?}"
    );
}

#[test]
fn yarn_classic_root_import_of_workspace_package_stays_silent() {
    let sites = root_import_sites(|root| {
        write(&root.join("yarn.lock"), "# yarn lockfile v1\n");
    });
    assert!(
        sites.is_empty(),
        "yarn classic links every workspace package into the root node_modules, got {sites:?}"
    );
}

#[test]
fn bun_root_import_of_workspace_package_stays_silent() {
    let sites = root_import_sites(|root| {
        write(&root.join("bun.lock"), "{}");
    });
    assert!(
        sites.is_empty(),
        "bun links every workspace package into the root node_modules, got {sites:?}"
    );
}

#[test]
fn yarn_berry_node_modules_root_import_of_workspace_package_stays_silent() {
    let sites = root_import_sites(|root| {
        write(&root.join("yarn.lock"), "__metadata:\n  version: 8\n");
        write(&root.join(".yarnrc.yml"), "nodeLinker: node-modules\n");
    });
    assert!(
        sites.is_empty(),
        "the yarn node-modules linker hoists workspace packages to the root, got {sites:?}"
    );
}

#[test]
fn yarn_berry_pnp_root_import_of_undeclared_workspace_package_is_unlisted() {
    let sites = root_import_sites(|root| {
        write(&root.join("yarn.lock"), "__metadata:\n  version: 8\n");
        write(&root.join(".yarnrc.yml"), "enableGlobalCache: true\n");
        write(&root.join(".pnp.cjs"), "module.exports = {};\n");
        remove_root_links(root);
    });
    assert_eq!(sites, root_lib_unlisted());
}

#[test]
fn yarn_berry_pnp_root_import_stays_unlisted_with_root_link() {
    // A stale root link from an earlier node-modules install does not make the
    // package available to a PnP runtime.
    let sites = root_import_sites(|root| {
        write(&root.join("yarn.lock"), "__metadata:\n  version: 8\n");
        write(&root.join(".yarnrc.yml"), "nodeLinker: pnp\n");
    });
    assert_eq!(sites, root_lib_unlisted());
}

#[test]
fn yarn_berry_pnpm_linker_root_import_of_undeclared_workspace_package_is_unlisted() {
    let sites = root_import_sites(|root| {
        write(&root.join("yarn.lock"), "__metadata:\n  version: 8\n");
        write(&root.join(".yarnrc.yml"), "nodeLinker: pnpm\n");
        remove_root_links(root);
    });
    assert_eq!(sites, root_lib_unlisted());
}

#[test]
fn pnpm_root_import_of_undeclared_workspace_package_is_unlisted() {
    let sites = root_import_sites(|root| {
        pnpm_root(root, "");
        remove_root_links(root);
    });
    assert_eq!(sites, root_lib_unlisted());
}

#[test]
fn pnpm_root_with_workspaces_field_stays_unlisted() {
    // pnpm ignores the `workspaces` field, so it does not link the packages
    // into the root `node_modules`.
    let sites = root_import_sites(|root| {
        pnpm_root(root, "");
        write(
            &root.join("package.json"),
            r#"{ "name": "root", "private": true, "workspaces": ["packages/*"] }"#,
        );
        remove_root_links(root);
    });
    assert_eq!(sites, root_lib_unlisted());
}

#[test]
fn pnpm_shamefully_hoist_root_import_of_workspace_package_stays_silent() {
    let sites = root_import_sites(|root| {
        pnpm_root(root, "");
        write(&root.join(".npmrc"), "shamefully-hoist=true\n");
    });
    assert!(
        sites.is_empty(),
        "shamefully-hoist links workspace packages into the root, got {sites:?}"
    );
}

/// Keep the root `node_modules` of an install, but remove the workspace links
/// from it. This is the layout of an install that links no workspace package
/// into the root, for example pnpm `node-linker=hoisted` or the bun isolated
/// linker.
fn remove_root_workspace_links(root: &Path, marker_dir: &str) {
    std::fs::remove_dir_all(root.join("node_modules/@repro")).expect("remove root links");
    std::fs::create_dir_all(root.join("node_modules").join(marker_dir))
        .expect("create install marker dir");
}

#[test]
fn pnpm_hoisted_node_linker_root_import_stays_unlisted() {
    // pnpm puts the workspace links of `node-linker=hoisted` in the node_modules
    // of each package, not in the root node_modules.
    let installed = root_import_sites(|root| {
        pnpm_root(root, "");
        write(&root.join(".npmrc"), "node-linker = hoisted\n");
        remove_root_workspace_links(root, ".pnpm");
    });
    assert_eq!(installed, root_lib_unlisted());

    let not_installed = root_import_sites(|root| {
        pnpm_root(root, "");
        write(&root.join(".npmrc"), "node-linker = hoisted\n");
        remove_root_links(root);
    });
    assert_eq!(not_installed, root_lib_unlisted());
}

#[test]
fn pnpm_root_link_from_the_install_decides_over_the_settings() {
    // pnpm 9 hoists `*eslint*` and `*prettier*` packages by default, so a real
    // install links `@repro/eslint-config` into the root with no setting.
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
    pnpm_root(&root, "");
    write(
        &root.join("package.json"),
        r#"{ "name": "root", "private": true, "packageManager": "pnpm@9.6.0" }"#,
    );
    write(
        &root.join("packages/eslint-config/package.json"),
        r#"{ "name": "@repro/eslint-config", "type": "module", "exports": { ".": "./index.js" } }"#,
    );
    write(
        &root.join("packages/eslint-config/index.js"),
        "export default [];\n",
    );
    remove_root_workspace_links(&root, ".pnpm");
    link_hoisted_package(&root, "eslint-config", "packages/eslint-config");
    write(
        &root.join("scripts/run.js"),
        "import config from \"@repro/eslint-config\";\nimport { bye } from \"@repro/lib/bye\";\nconsole.log(config, bye());\n",
    );

    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");
    assert_eq!(
        unlisted_sites(&results),
        vec![("@repro/lib".to_string(), "run.js".to_string(), 2)],
        "only the package without a root link is unlisted"
    );
}

#[test]
fn pnpm_11_npmrc_hoist_setting_without_root_link_stays_unlisted() {
    // pnpm 11 reads its hoisting settings only from `pnpm-workspace.yaml`.
    let installed = root_import_sites(|root| {
        pnpm_root(root, "");
        write(
            &root.join("package.json"),
            r#"{ "name": "root", "private": true, "packageManager": "pnpm@11.25.0" }"#,
        );
        write(&root.join(".npmrc"), "shamefully-hoist=true\n");
        remove_root_workspace_links(root, ".pnpm");
    });
    assert_eq!(installed, root_lib_unlisted());

    let not_installed = root_import_sites(|root| {
        pnpm_root(root, "");
        write(
            &root.join("package.json"),
            r#"{ "name": "root", "private": true, "packageManager": "pnpm@11.25.0" }"#,
        );
        write(&root.join(".npmrc"), "shamefully-hoist=true\n");
        remove_root_links(root);
    });
    assert_eq!(not_installed, root_lib_unlisted());
}

#[test]
fn bun_isolated_linker_root_import_stays_unlisted() {
    // A fresh bun workspace install uses the isolated linker: the root
    // node_modules has only `.bun`, and `bun.lock` has `configVersion: 1`.
    let bun_lock =
        "{\n  \"lockfileVersion\": 1,\n  \"configVersion\": 1,\n  \"workspaces\": {},\n}\n";
    let installed = root_import_sites(|root| {
        write(&root.join("bun.lock"), bun_lock);
        remove_root_workspace_links(root, ".bun");
    });
    assert_eq!(installed, root_lib_unlisted());

    let not_installed = root_import_sites(|root| {
        write(&root.join("bun.lock"), bun_lock);
        remove_root_links(root);
    });
    assert_eq!(not_installed, root_lib_unlisted());
}

#[test]
fn pnpm_workspace_yaml_hoist_setting_root_import_stays_silent() {
    let sites = root_import_sites(|root| pnpm_root(root, "shamefullyHoist: true\n"));
    assert!(
        sites.is_empty(),
        "pnpm-workspace.yaml shamefullyHoist links workspace packages into the root, got {sites:?}"
    );
}

#[test]
fn pnpm_public_hoist_pattern_applies_to_matching_packages_only() {
    let matching = root_import_sites(|root| {
        pnpm_root(root, "");
        write(&root.join(".npmrc"), "public-hoist-pattern[]=@repro/*\n");
    });
    assert!(
        matching.is_empty(),
        "a matching public-hoist-pattern links the package into the root, got {matching:?}"
    );

    let other = root_import_sites(|root| {
        pnpm_root(root, "");
        write(&root.join(".npmrc"), "public-hoist-pattern[]=*eslint*\n");
        remove_root_links(root);
    });
    assert_eq!(other, root_lib_unlisted());
}

#[test]
fn pnpm_hoist_workspace_packages_false_keeps_root_import_unlisted() {
    let sites = root_import_sites(|root| {
        pnpm_root(root, "hoistWorkspacePackages: false\n");
        write(&root.join(".npmrc"), "shamefully-hoist=true\n");
        remove_root_links(root);
    });
    assert_eq!(sites, root_lib_unlisted());
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
