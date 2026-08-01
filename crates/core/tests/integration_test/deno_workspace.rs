//! Pure-Deno workspace coverage through the public core analysis path.

use std::fs;
use std::path::Path;

use super::common::{create_config, fixture_path};

#[test]
fn mixed_bridge_fixture_keeps_deno_surfaces_reachable() {
    let root = fixture_path("deno-workspace");
    let results = fallow_core::analyze(&create_config(root.clone()))
        .expect("mixed npm/Deno fixture should analyze");
    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| relative(&root, &finding.file.path))
        .collect();

    for reachable in [
        "main.ts",
        "shared.ts",
        "packages/lib/mod.ts",
        "packages/lib/internal.ts",
        "packages/lib/public.ts",
        "standalone_test.ts",
    ] {
        assert!(
            !unused.iter().any(|path| path == reachable),
            "{reachable} should be reachable in mixed Deno fixture; unused: {unused:?}"
        );
    }
    assert!(unused.iter().any(|path| path == "packages/lib/private.ts"));
    assert!(results.unresolved_imports.is_empty());
}

#[test]
fn deno_workspace_exports_imports_and_test_entries_stay_reachable() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(
        root,
        "deno.json",
        r#"{
          "name": "root",
          "workspace": ["./packages/*"],
          "imports": { "lib": "@scope/lib" },
          "exports": "./main.ts"
        }"#,
    );
    write_file(
        root,
        "main.ts",
        "import { value } from 'lib'; console.log(value);\n",
    );
    write_file(
        root,
        "packages/lib/deno.jsonc",
        r#"{
          // Deno package exports are public API entry points.
          "name": "@scope/lib",
          "exports": {
            ".": "./mod.ts",
            "./public": "./public.ts",
          },
        }"#,
    );
    write_file(
        root,
        "packages/lib/mod.ts",
        "export { value } from './internal.ts';\n",
    );
    write_file(
        root,
        "packages/lib/internal.ts",
        "export const value = 1;\n",
    );
    write_file(
        root,
        "packages/lib/public.ts",
        "export const publicValue = 1;\n",
    );
    write_file(
        root,
        "packages/lib/private.ts",
        "export const privateValue = 1;\n",
    );
    write_file(
        root,
        "standalone_test.ts",
        "Deno.test('works', () => {});\n",
    );

    let results = fallow_core::analyze(&create_config(root.to_path_buf()))
        .expect("pure Deno analysis should succeed");
    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| relative(root, &finding.file.path))
        .collect();

    for reachable in [
        "main.ts",
        "packages/lib/mod.ts",
        "packages/lib/internal.ts",
        "packages/lib/public.ts",
        "standalone_test.ts",
    ] {
        assert!(
            !unused.iter().any(|path| path == reachable),
            "{reachable} should be reachable in a Deno workspace; unused: {unused:?}"
        );
    }
    assert!(
        unused.iter().any(|path| path == "packages/lib/private.ts"),
        "unexported orphan should remain unused: {unused:?}"
    );
    assert!(
        results.unresolved_imports.is_empty(),
        "Deno import-map and workspace package imports should resolve: {:?}",
        results.unresolved_imports
    );
}

#[test]
fn malformed_root_deno_config_stops_analysis() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    fs::write(tmp.path().join("deno.jsonc"), "{ imports: [ }")
        .expect("write malformed Deno config");

    let error = fallow_core::analyze(&create_config(tmp.path().to_path_buf()))
        .expect_err("malformed root Deno config must stop analysis");

    assert!(
        error.to_string().contains("root Deno config"),
        "typed config failure should retain Deno context: {error}"
    );
}

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
