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

use std::path::Path;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Reproduce the reporter's shape: `paths` live in a `tsconfig.app.json` and
/// the analyzed package.json declares no `typescript` dependency.
fn create_project(root: &Path) {
    write(
        &root.join("package.json"),
        r#"{ "name": "acme-app", "private": true, "type": "module" }"#,
    );
    write(
        &root.join("tsconfig.app.json"),
        r#"{
            "compilerOptions": { "paths": { "@acme/*": ["./src/*"] } },
            "include": ["src/**/*.ts"]
        }"#,
    );
    write(
        &root.join("src/internal/common/request-context.ts"),
        r"export interface RequestContext { id: string; }
           export interface RequestOptions { retries: number; }",
    );
    write(
        &root.join("src/internal/feature-a/clients/base-client.ts"),
        r#"import { RequestContext } from "@acme/internal/common/request-context";
           export class BaseClient { ctx: RequestContext | null = null; }"#,
    );
    write(
        &root.join("src/internal/feature-b/clients/lookup-client.ts"),
        r#"import { BaseClient } from "@acme/internal/feature-a/clients/base-client";
           import { missingHelper } from "@acme/internal/files/missing-helper";
           export class LookupClient { client = new BaseClient(); helper = missingHelper; }"#,
    );
    write(
        &root.join("src/main.ts"),
        r#"import { LookupClient } from "@acme/internal/feature-b/clients/lookup-client";
           const c = new LookupClient();
           console.log(c);"#,
    );
}

#[test]
fn issue_1911_paths_alias_not_reported_as_unlisted_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    create_project(dir.path());

    let config = create_config(dir.path().to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // 1. The alias prefix must NOT be classified as an npm package.
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.contains(&"@acme/internal"),
        "aliased prefix @acme/internal should not be an unlisted dependency, got {unlisted:?}"
    );

    // 2. The genuinely-broken alias target must reclassify as unresolved-import.
    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.contains(&"@acme/internal/files/missing-helper"),
        "broken alias target should surface as unresolved-import, got {unresolved:?}"
    );

    // 3. Valid alias imports resolve internally (never surface as unresolved).
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
