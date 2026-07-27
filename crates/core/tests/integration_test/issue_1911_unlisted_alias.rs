//! Regression tests for issue #1911: a TypeScript `paths` alias
//! (`@acme/*` -> `./src/*`) misreported as an unlisted npm dependency
//! (`@acme/internal`) when neither the TypeScript plugin (no declared
//! `typescript` dependency) nor the per-file nearest-`tsconfig.json` chain
//! (paths live in a sibling `tsconfig.app.json`) registers the alias.
//!
//! The fix activates the TypeScript plugin on the presence of a
//! `tsconfig.json` / `tsconfig.*.json` config file, so its `compilerOptions.paths`
//! are registered project-wide, the valid alias imports resolve internally, and
//! only a genuinely-broken alias target surfaces as `unresolved-import`.
//!
//! Also covers issue #1942, the same misreport reached from the other side: the
//! alias target exists but `ignorePatterns` kept it out of the file index, so
//! the bare-looking specifier fell through to npm-package classification. The
//! resolver now keeps the concrete target for alias matches that land inside the
//! analyzed root, while targets outside it stay npm packages so workspace
//! install symlinks keep their dependency credit (#1008).

use std::path::Path;

use super::common::{create_config, create_config_with_ignore_patterns};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Reproduce the reporter's shape: `paths` live in a `tsconfig.app.json` and
/// the analyzed package.json declares no `typescript` dependency.
fn create_project(root: &Path, project_dir: &str) {
    let project_root = root.join(project_dir);
    write(
        &root.join("package.json"),
        r#"{ "name": "acme-app", "private": true, "type": "module" }"#,
    );
    write(
        &project_root.join("tsconfig.app.json"),
        r#"{
            "compilerOptions": { "paths": { "@acme/*": ["./src/*"] } },
            "include": ["src/**/*.ts"]
        }"#,
    );
    write(
        &project_root.join("src/internal/common/request-context.ts"),
        r"export interface RequestContext { id: string; }
           export interface RequestOptions { retries: number; }",
    );
    write(
        &project_root.join("src/internal/feature-a/clients/base-client.ts"),
        r#"import { RequestContext } from "@acme/internal/common/request-context";
           export class BaseClient { ctx: RequestContext | null = null; }"#,
    );
    write(
        &project_root.join("src/internal/feature-b/clients/lookup-client.ts"),
        r#"import { BaseClient } from "@acme/internal/feature-a/clients/base-client";
           import { missingHelper } from "@acme/internal/files/missing-helper";
           export class LookupClient { client = new BaseClient(); helper = missingHelper; }"#,
    );
    write(
        &project_root.join("src/main.ts"),
        r#"import { LookupClient } from "@acme/internal/feature-b/clients/lookup-client";
           const c = new LookupClient();
           console.log(c);"#,
    );
}

fn assert_alias_resolution(results: &fallow_types::results::AnalysisResults) {
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.contains(&"@acme/internal"),
        "aliased prefix @acme/internal should not be an unlisted dependency, got {unlisted:?}"
    );

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.contains(&"@acme/internal/files/missing-helper"),
        "broken alias target should surface as unresolved-import, got {unresolved:?}"
    );

    for specifier in [
        "@acme/internal/common/request-context",
        "@acme/internal/feature-a/clients/base-client",
        "@acme/internal/feature-b/clients/lookup-client",
    ] {
        assert!(
            !unresolved.contains(&specifier),
            "valid alias import {specifier} should resolve, got {unresolved:?}"
        );
    }
}

#[test]
fn issue_1911_paths_alias_not_reported_as_unlisted_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    create_project(dir.path(), "");

    let config = create_config(dir.path().to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_alias_resolution(&results);
}

#[test]
fn issue_1911_nested_paths_alias_activates_in_production_without_typescript_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    create_project(dir.path(), "apps/web");

    let mut config = create_config(dir.path().to_path_buf());
    config.production = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_alias_resolution(&results);
}

#[test]
fn issue_1942_ignored_paths_alias_not_reported_as_unlisted_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{ "name": "fallow-ignored-alias-repro", "private": true }"#,
    );
    write(
        &root.join("tsconfig.json"),
        r#"{
            "compilerOptions": {
                "paths": { "@synthetic/*": ["./sparta/*"] }
            },
            "include": ["src/**/*.ts", "sparta/**/*.ts"]
        }"#,
    );
    // The second import is the positive control: a specifier that matches no
    // alias and is installed nowhere must still be reported, so a regression
    // that suppresses every unlisted dependency cannot make this test pass.
    write(
        &root.join("src/entry.ts"),
        r#"import { localValue } from "@synthetic/api/local-module";
           import { helper } from "definitely-not-installed";
           console.log(localValue, helper);"#,
    );
    write(
        &root.join("sparta/api/local-module.ts"),
        r#"export const localValue = "resolved inside the workspace";"#,
    );

    let config =
        create_config_with_ignore_patterns(root.to_path_buf(), &["sparta/api/local-module.ts"]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.contains(&"@synthetic/api"),
        "ignored local alias target should not be an unlisted dependency, got {unlisted:?}"
    );
    assert!(
        unlisted.contains(&"definitely-not-installed"),
        "a genuinely unlisted dependency must still be reported, got {unlisted:?}"
    );

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved.contains(&"@synthetic/api/local-module"),
        "ignored local alias target should still resolve, got {unresolved:?}"
    );
}

/// The alias guard added for #1942 must not swallow npm-package accounting for
/// a workspace dependency whose install symlink resolves outside the analyzed
/// root, which is the case `package_usage_name_for_external_bare_specifier`
/// exists to serve (#1008). The specifier shape alone is ambiguous: `@acme/ui`
/// pattern-matches the `@acme/*` alias key, so only the resolved target's
/// location distinguishes the two.
#[test]
#[cfg_attr(miri, ignore)]
fn issue_1942_guard_still_credits_workspace_package_resolved_outside_root() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = dir.path();
    let app = repo.join("apps/web");

    write(
        &repo.join("packages/ui/package.json"),
        r#"{ "name": "@acme/ui", "version": "1.0.0", "main": "src/index.ts" }"#,
    );
    write(
        &repo.join("packages/ui/src/index.ts"),
        r#"export const Button = "button";"#,
    );

    write(
        &app.join("package.json"),
        r#"{
            "name": "web",
            "private": true,
            "dependencies": { "@acme/ui": "workspace:*" }
        }"#,
    );
    // A `paths` key whose prefix matches the workspace scope, which is what makes
    // the specifier indistinguishable from an aliased project file by shape.
    write(
        &app.join("tsconfig.json"),
        r#"{
            "compilerOptions": { "paths": { "@acme/*": ["../../packages/*/src"] } },
            "include": ["src/**/*.ts"]
        }"#,
    );
    write(
        &app.join("src/main.ts"),
        r#"import { Button } from "@acme/ui";
           console.log(Button);"#,
    );

    let link = app.join("node_modules/@acme/ui");
    std::fs::create_dir_all(link.parent().expect("link parent")).expect("create node_modules dir");
    #[cfg(unix)]
    std::os::unix::fs::symlink(repo.join("packages/ui"), &link).expect("symlink workspace package");
    #[cfg(windows)]
    if std::os::windows::fs::symlink_dir(repo.join("packages/ui"), &link).is_err() {
        // Windows needs developer mode or elevation for symlinks; without the
        // link there is nothing to regress, so skip rather than fail spuriously.
        return;
    }

    let config = create_config(app);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unused.contains(&"@acme/ui"),
        "imported workspace package must stay credited as used, got {unused:?}"
    );
}
