//! Issue #601: the Vitest plugin parses `test.alias` so virtual-module and mock aliases stay visible.

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

/// Virtual module alias to a local mock must resolve cleanly.
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

/// `test.alias` must not shadow tsconfig-based source resolution.
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

/// Follow-up surface 1: `test.alias` embedded in `vite.config.ts` (the common
/// `defineConfig({ test: {...}, resolve: { alias } })` shape). The Vitest plugin
/// never sees vite.config.ts; the Vite plugin extracts the test-block aliases.
#[test]
fn vite_config_embedded_test_alias_resolves_virtual_module() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "vite-embedded-test-alias",
            "private": true,
            "main": "src/index.ts",
            "devDependencies": { "vite": "5.0.0", "vitest": "2.0.0" }
        }"#,
    );
    write(
        &root.join("tsconfig.json"),
        r#"{ "compilerOptions": { "module": "ESNext", "moduleResolution": "bundler" } }"#,
    );
    write(
        &root.join("vite.config.ts"),
        r#"
            import { defineConfig } from "vite";
            export default defineConfig({
                test: { alias: { vscode: "./test/mock/vscode.ts" } }
            });
        "#,
    );
    write(
        &root.join("test/mock/vscode.ts"),
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
        "test.alias in vite.config must resolve the virtual module, found: {unresolved:?}"
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
}

/// Follow-up surface 2 + misfire fix: a project-level `resolve.alias` mock in a
/// `vitest.config.ts` (the vite `test-alias-from-vite` workspaces-browser shape),
/// AND a top-level `resolve.alias` directory mapping `@app` -> path.resolve(src)
/// that must NOT be mistaken for a package-to-package alias (the misfire fix),
/// even though `src/` exists only because we write it here. The import `@app/api`
/// must resolve to `src/api.ts` so it stays referenced.
#[test]
fn vitest_config_resolve_alias_directory_and_project_mock() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "vitest-resolve-alias",
            "private": true,
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
                resolve: { alias: { "@app": resolve(__dirname, "src") } },
                test: {
                    projects: [
                        {
                            test: { name: "browser" },
                            resolve: { alias: { vscode: "./test/mock/vscode.ts" } }
                        }
                    ]
                }
            });
        "#,
    );
    write(
        &root.join("src/feature.test.ts"),
        r#"
            import { realApi } from "@app/api";
            import { window } from "vscode";
            it("uses aliases", () => { realApi(); window; });
        "#,
    );
    write(
        &root.join("src/api.ts"),
        "export const realApi = () => 1;\n",
    );
    write(
        &root.join("test/mock/vscode.ts"),
        "export const window = {};\n",
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
        "`@app`->resolve(src) directory alias must resolve `@app/api` to src/api.ts \
         (NOT be misclassified as package-to-package), so src/api.ts stays referenced; \
         unused_files: {unused_files:?}"
    );

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved.contains(&"@app/api"),
        "`@app/api` must resolve via the directory alias, found: {unresolved:?}"
    );
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.contains(&"vscode"),
        "project-level resolve.alias must resolve the mock (not unlisted), found: {unlisted:?}"
    );
}

/// Follow-up surface 3: `vitest.workspace.ts` array file (`defineWorkspace([...])`).
/// `find_config_object` returns None for an array default export, so the
/// workspace-array traversal must extract each element's aliases.
#[test]
fn vitest_workspace_array_file_aliases_resolve() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "vitest-workspace-array",
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
        &root.join("vitest.workspace.ts"),
        r#"
            import { defineWorkspace } from "vitest/config";
            export default defineWorkspace([
                { test: { alias: { vscode: "./test/mock/vscode.ts" } } }
            ]);
        "#,
    );
    write(
        &root.join("test/mock/vscode.ts"),
        "export const window = {};\n",
    );
    write(
        &root.join("src/index.ts"),
        r#"
            import { window } from "vscode";
            export const run = () => window;
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
        "vitest.workspace array-file alias must resolve the virtual module, found: {unresolved:?}"
    );
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.contains(&"vscode"),
        "workspace-array alias-resolved module must not be unlisted, found: {unlisted:?}"
    );
}

/// Issue #3963: a `resolve.alias` entry swaps a relative import for a stub
/// file. The resolver resolves `./clock.js` to the original file, so the stub
/// has no graph consumer. With `includeEntryExports`, the exports of the stub
/// must stay credited. Exports of the original file stay reportable.
#[test]
fn relative_alias_replacement_exports_are_used_with_include_entry_exports() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "repro",
            "private": true,
            "type": "module",
            "main": "src/index.ts",
            "scripts": { "test": "vitest" },
            "devDependencies": { "vitest": "4.0.0" }
        }"#,
    );
    write(
        &root.join("node_modules/vitest/package.json"),
        r#"{ "name": "vitest", "version": "4.0.0", "bin": { "vitest": "vitest.mjs" } }"#,
    );
    write(
        &root.join("vitest.config.ts"),
        r#"
            export default {
                resolve: {
                    alias: [{ find: "./clock.js", replacement: new URL("./test/stubs/clock.ts", import.meta.url).pathname }],
                },
            };
        "#,
    );
    write(
        &root.join("src/clock.ts"),
        "export function now(): number { return Date.now(); }\nexport function unusedClock(): number { return 1; }\n",
    );
    write(
        &root.join("src/index.ts"),
        "import { now } from \"./clock.js\";\n\nexport function stamp(): number {\n    return now();\n}\n",
    );
    write(
        &root.join("test/stubs/clock.ts"),
        "export function now(): number { return 0; }\n",
    );
    write(
        &root.join("test/index.spec.ts"),
        "import { stamp } from \"../src/index.js\";\n\nif (stamp() !== 0) {\n    throw new Error(\"stamp\");\n}\n",
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_exports
        .iter()
        .map(|e| {
            let path = e.export.path.strip_prefix(root).unwrap_or(&e.export.path);
            format!(
                "{}:{}",
                path.to_string_lossy().replace('\\', "/"),
                e.export.export_name
            )
        })
        .collect();

    assert_eq!(
        unused,
        vec!["src/clock.ts:unusedClock".to_string()],
        "only the unused export of the original file stays reportable"
    );
}

fn unused_export_keys(
    results: &fallow_core::results::AnalysisResults,
    root: &std::path::Path,
) -> Vec<String> {
    let mut keys: Vec<String> = results
        .unused_exports
        .iter()
        .map(|e| {
            let path = e.export.path.strip_prefix(root).unwrap_or(&e.export.path);
            format!(
                "{}:{}",
                path.to_string_lossy().replace('\\', "/"),
                e.export.export_name
            )
        })
        .collect();
    keys.sort();
    keys
}

/// A bare alias key names an installed package. The resolver resolves the
/// import to the real package, so the mock never gets a graph consumer. With
/// `includeEntryExports`, the exports of the mock must stay credited.
#[test]
fn installed_package_alias_mock_exports_are_used_with_include_entry_exports() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "repro",
            "private": true,
            "type": "module",
            "main": "src/index.ts",
            "dependencies": { "pkg": "1.0.0" },
            "devDependencies": { "vitest": "4.0.0" }
        }"#,
    );
    write(
        &root.join("node_modules/pkg/package.json"),
        r#"{ "name": "pkg", "version": "1.0.0", "main": "index.js" }"#,
    );
    write(
        &root.join("node_modules/pkg/index.js"),
        "export const query = () => 'real';\n",
    );
    write(
        &root.join("vitest.config.ts"),
        r#"
            export default {
                test: { alias: { pkg: new URL("./mocks/pkg.ts", import.meta.url).pathname } },
            };
        "#,
    );
    write(
        &root.join("mocks/pkg.ts"),
        "export const query = () => 'mock';\nexport const tool = () => 'mock';\n",
    );
    write(
        &root.join("src/index.ts"),
        "import { query } from \"pkg\";\n\nexport const run = () => query();\n",
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unused_export_keys(&results, root),
        vec!["src/index.ts:run".to_string()],
        "the mock replaces the installed package, so its exports are used"
    );
}

/// A bare alias key names a package that is not installed. The resolver
/// routes the import to the mock, so an unused export of the mock stays
/// reportable.
#[test]
fn missing_package_alias_mock_keeps_precise_export_tracking() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "repro",
            "private": true,
            "type": "module",
            "main": "src/index.ts",
            "devDependencies": { "vitest": "4.0.0" }
        }"#,
    );
    write(
        &root.join("vitest.config.ts"),
        r#"
            export default {
                test: { alias: { vscode: new URL("./mocks/vscode.ts", import.meta.url).pathname } },
            };
        "#,
    );
    write(
        &root.join("mocks/vscode.ts"),
        "export const window = {};\nexport const unusedMock = 1;\n",
    );
    write(
        &root.join("src/index.ts"),
        "import { window } from \"vscode\";\n\nexport const run = () => window;\n",
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unused_export_keys(&results, root),
        vec![
            "mocks/vscode.ts:unusedMock".to_string(),
            "src/index.ts:run".to_string()
        ],
    );
}

/// A top-level `resolve.alias` of a Vite mode config, such as
/// `vite.config.prod.ts`, swaps a relative import for a stub at build time.
/// Only the Vite plugin reads a mode config. The resolver resolves the relative
/// import to the original file, so the stub has no graph consumer.
#[test]
fn vite_mode_config_relative_alias_replacement_exports_are_used() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "repro",
            "private": true,
            "type": "module",
            "scripts": { "build": "vite build --config vite.config.prod.ts" },
            "devDependencies": { "vite": "7.0.0" }
        }"#,
    );
    write(
        &root.join("node_modules/vite/package.json"),
        r#"{ "name": "vite", "version": "7.0.0", "bin": { "vite": "bin/vite.js" } }"#,
    );
    write(
        &root.join("index.html"),
        "<script type=\"module\" src=\"/src/main.ts\"></script>\n",
    );
    write(
        &root.join("vite.config.prod.ts"),
        r#"
            export default {
                resolve: {
                    alias: [{ find: "./clock.js", replacement: new URL("./src/stubs/clock.ts", import.meta.url).pathname }],
                },
            };
        "#,
    );
    write(
        &root.join("src/clock.ts"),
        "export function now(): number { return Date.now(); }\nexport function unusedClock(): number { return 1; }\n",
    );
    write(
        &root.join("src/main.ts"),
        "import { now } from \"./clock.js\";\n\nconsole.log(now());\n",
    );
    write(
        &root.join("src/stubs/clock.ts"),
        "export function now(): number { return 0; }\n",
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unused_export_keys(&results, root),
        vec!["src/clock.ts:unusedClock".to_string()],
        "only the unused export of the original file stays reportable"
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(
        unused_files.is_empty(),
        "the stub replaces the original file at build time: {unused_files:?}"
    );
}
