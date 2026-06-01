//! Issue #811: the Vite plugin must recover `resolve.alias` when the value is an
//! imported identifier (a shared alias module reused across vite / vitest /
//! storybook) or an array/object assembled with spreads. Otherwise every aliased
//! import surfaces as `unresolved-import`, cascading into `unused-file` /
//! `unused-export` for everything reachable only through those aliases.

use super::common::create_config;

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// The issue's headline shape: `resolve.alias` is set to `sharedAliases`,
/// imported from a sibling `vite.shared.js`. With the entry chain
/// `index.html -> src/main.js -> @/a -> @/b`, the `@` alias must resolve so none
/// of the aliased imports are unresolved and no source file is unused.
#[test]
fn imported_shared_alias_resolves_entry_chain() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "issue-811",
            "private": true,
            "dependencies": { "vite": "5.0.0" }
        }"#,
    );
    // Stub vite so the plugin activates (its enabler dep must be present).
    write(
        &root.join("node_modules/vite/package.json"),
        r#"{ "name": "vite", "version": "5.0.0", "main": "index.js" }"#,
    );
    write(
        &root.join("node_modules/vite/index.js"),
        "export default {};\n",
    );

    write(
        &root.join("index.html"),
        r#"<!doctype html><html><body><script type="module" src="/src/main.js"></script></body></html>"#,
    );
    write(
        &root.join("vite.shared.js"),
        r#"export const sharedAliases = [
            { find: "@", replacement: new URL("./src", import.meta.url).pathname },
        ];"#,
    );
    write(
        &root.join("vite.config.js"),
        r#"
            import { defineConfig } from "vite";
            import { sharedAliases } from "./vite.shared.js";
            export default defineConfig({ resolve: { alias: sharedAliases } });
        "#,
    );
    write(
        &root.join("src/main.js"),
        r#"import { a } from "@/a";
           console.log(a());"#,
    );
    write(
        &root.join("src/a.js"),
        r#"import { b } from "@/b";
           export function a() { return b(); }"#,
    );
    write(
        &root.join("src/b.js"),
        "export function b() { return 1; }\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    for spec in ["@/a", "@/b"] {
        assert!(
            !unresolved.contains(&spec),
            "aliased import `{spec}` must resolve, found unresolved: {unresolved:?}"
        );
    }

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    for file in ["src/a.js", "src/b.js"] {
        assert!(
            !unused_files.iter().any(|p| p.ends_with(file)),
            "`{file}` is reachable through the alias chain, found unused: {unused_files:?}"
        );
    }
}

/// The spread shape: `resolve.alias` is an array built from two imported alias
/// modules plus one inline entry. All three aliases must resolve.
#[test]
fn spread_of_imported_aliases_resolves() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        &root.join("package.json"),
        r#"{
            "name": "issue-811-spread",
            "private": true,
            "dependencies": { "vite": "5.0.0" }
        }"#,
    );
    write(
        &root.join("node_modules/vite/package.json"),
        r#"{ "name": "vite", "version": "5.0.0", "main": "index.js" }"#,
    );
    write(
        &root.join("node_modules/vite/index.js"),
        "export default {};\n",
    );

    write(
        &root.join("index.html"),
        r#"<!doctype html><html><body><script type="module" src="/src/main.js"></script></body></html>"#,
    );
    write(
        &root.join("alias.app.js"),
        r#"export const appAliases = [{ find: "@", replacement: "./src" }];"#,
    );
    write(
        &root.join("alias.lib.js"),
        r#"export const libAliases = [{ find: "~", replacement: "./lib" }];"#,
    );
    write(
        &root.join("vite.config.js"),
        r#"
            import { defineConfig } from "vite";
            import { appAliases } from "./alias.app.js";
            import { libAliases } from "./alias.lib.js";
            export default defineConfig({
                resolve: { alias: [...appAliases, ...libAliases] }
            });
        "#,
    );
    write(
        &root.join("src/main.js"),
        r#"import { a } from "@/a";
           import { helper } from "~/helper";
           console.log(a(), helper());"#,
    );
    write(
        &root.join("src/a.js"),
        "export function a() { return 1; }\n",
    );
    write(
        &root.join("lib/helper.js"),
        "export function helper() { return 2; }\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    for spec in ["@/a", "~/helper"] {
        assert!(
            !unresolved.contains(&spec),
            "spread alias import `{spec}` must resolve, found unresolved: {unresolved:?}"
        );
    }
}
