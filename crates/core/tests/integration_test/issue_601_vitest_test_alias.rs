//! Issue #601: the Vitest plugin parses `test.alias` so mock / virtual-module
//! consumers are visible.
//!
//! Two false-positive classes are covered:
//! 1. Imports that only resolve through a test-only alias (a virtual module like
//!    `vscode` aliased to a local mock) must not surface as `unresolved-import`
//!    or `unlisted-dependency`.
//! 2. `__mocks__` files aliased to mock a REAL installed package must keep their
//!    exports credited (no `unused-export` / `fallow fix` removal), even when the
//!    production import resolves through `node_modules`.

use super::common::create_config;

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Class 2, the amplitude/wizard shape: a real installed dependency is aliased
/// to a local `__mocks__` file. With `node_modules` present the production import
/// resolves to the real package, so the mock's exports are credited only because
/// the alias target is seeded as a support entry point (mechanism B). They must
/// not be reported unused, and the real dependency must not be reported unused.
#[test]
fn aliased_mock_for_installed_package_credits_exports() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "test-alias-mock",
            "private": true,
            "main": "src/index.ts",
            "dependencies": { "@scope/pkg": "1.0.0" },
            "devDependencies": { "vitest": "2.0.0" }
        }"#,
    );
    write(
        &root.join("tsconfig.json"),
        r#"{ "compilerOptions": { "module": "ESNext", "moduleResolution": "bundler" } }"#,
    );
    write(
        &root.join("vitest.config.ts"),
        r#"
            import { defineConfig } from "vitest/config";
            import { resolve } from "node:path";
            export default defineConfig({
                test: {
                    alias: {
                        "@scope/pkg": resolve(__dirname, "__mocks__/@scope/pkg.ts")
                    }
                }
            });
        "#,
    );
    write(
        &root.join("__mocks__/@scope/pkg.ts"),
        r#"
            export const query = () => "mock-query";
            export const tool = () => "mock-tool";
            export const createSdkMcpServer = () => "mock-server";
        "#,
    );
    write(
        &root.join("src/index.ts"),
        r#"
            import { query } from "@scope/pkg";
            export const run = () => query();
        "#,
    );
    // Stub the real installed package so the production import resolves through
    // node_modules (the case where mechanism B is load-bearing).
    write(
        &root.join("node_modules/@scope/pkg/package.json"),
        r#"{ "name": "@scope/pkg", "version": "1.0.0", "main": "index.js" }"#,
    );
    write(
        &root.join("node_modules/@scope/pkg/index.js"),
        "export const query = () => 'real';\nexport const tool = () => 'real';\nexport const createSdkMcpServer = () => 'real';\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    for name in ["query", "tool", "createSdkMcpServer"] {
        assert!(
            !unused_exports.contains(&name),
            "aliased mock export `{name}` must not be reported unused, found: {unused_exports:?}"
        );
    }

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|p| p.contains("__mocks__/@scope/pkg.ts")),
        "aliased mock file must not be reported unused, found: {unused_files:?}"
    );

    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"@scope/pkg"),
        "aliased real dependency must not be reported unused, found: {unused_deps:?}"
    );
}

/// Class 1: a virtual module (`vscode`, not an npm dependency) aliased to a local
/// mock. The import must resolve through the alias (no `unresolved-import`, no
/// `unlisted-dependency`) and the mock file must stay reachable.
#[test]
fn virtual_module_alias_resolves_and_credits_mock() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "test-alias-virtual",
            "private": true,
            "main": "src/index.ts",
            "devDependencies": { "vitest": "2.0.0" }
        }"#,
    );
    write(
        &root.join("tsconfig.json"),
        r#"{ "compilerOptions": { "module": "ESNext", "moduleResolution": "bundler" } }"#,
    );
    write(
        &root.join("vitest.config.ts"),
        r#"
            import { defineConfig } from "vitest/config";
            export default defineConfig({
                test: { alias: { vscode: "./test/mocks/vscode.ts" } }
            });
        "#,
    );
    write(
        &root.join("test/mocks/vscode.ts"),
        "export const window = { showMessage: () => {} };\n",
    );
    write(
        &root.join("src/index.ts"),
        r#"
            import { window } from "vscode";
            export const run = () => window.showMessage();
        "#,
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved.contains(&"vscode"),
        "virtual-module import resolved via test.alias must not be unresolved, found: {unresolved:?}"
    );

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.contains(&"vscode"),
        "alias-resolved virtual module must not be unlisted, found: {unlisted:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|p| p.contains("test/mocks/vscode.ts")),
        "alias-target mock must not be reported unused, found: {unused_files:?}"
    );
}

/// Production-graph safety: a `test.alias` on a tsconfig-aliased prefix must NOT
/// shadow the real source. `@/api` resolves through tsconfig paths to
/// `src/api.ts` (the standard resolver wins over the plugin path-alias fallback),
/// so `src/api.ts` stays referenced rather than unused.
#[test]
fn test_alias_does_not_shadow_tsconfig_alias() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "test-alias-shadow",
            "private": true,
            "devDependencies": { "vitest": "2.0.0" }
        }"#,
    );
    write(
        &root.join("tsconfig.json"),
        r#"{
            "compilerOptions": {
                "module": "ESNext",
                "moduleResolution": "bundler",
                "baseUrl": ".",
                "paths": { "@/*": ["src/*"] }
            }
        }"#,
    );
    write(
        &root.join("vitest.config.ts"),
        r#"
            import { defineConfig } from "vitest/config";
            export default defineConfig({
                test: { alias: { "@/api": "./test/mocks/api.ts" } }
            });
        "#,
    );
    // A test file is a Vitest entry point. It imports through the tsconfig alias.
    write(
        &root.join("src/feature.test.ts"),
        r#"
            import { realApi } from "@/api";
            it("uses the api", () => { realApi(); });
        "#,
    );
    write(
        &root.join("src/api.ts"),
        "export const realApi = () => 1;\n",
    );
    write(
        &root.join("test/mocks/api.ts"),
        "export const realApi = () => 2;\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files.iter().any(|p| p.ends_with("src/api.ts")),
        "`@/api` must resolve to src/api.ts via tsconfig (not be shadowed by the test mock), \
         so src/api.ts stays referenced; unused_files: {unused_files:?}"
    );

    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    assert!(
        !unused_exports.contains(&"realApi"),
        "src/api.ts `realApi` must be credited via the tsconfig-aliased import, found: {unused_exports:?}"
    );
}
