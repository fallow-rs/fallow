//! Regression tests for issue #2757: Module Federation options that reach the
//! plugin call through one level of indirection stayed unread, so the exposed
//! file was reported as unused.
//!
//! Each test builds a webpack project that exposes `src/Button.tsx` beside an
//! unexposed `src/orphan.ts`. The exposed file must be an entry point and the
//! sibling must still report.

use std::path::Path;

use super::common::create_config;

const PACKAGE_JSON: &str = r#"{
    "name": "mf-indirection",
    "private": true,
    "devDependencies": { "webpack": "^5.98.0", "@module-federation/enhanced": "^0.9.0" }
}"#;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Build a project from `files` (path, contents) and return the unused files.
fn unused_files(files: &[(&str, &str)]) -> Vec<String> {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(&root.join("package.json"), PACKAGE_JSON);
    write(
        &root.join("src/Button.tsx"),
        r#"export default (): string => "button";"#,
    );
    write(&root.join("src/orphan.ts"), "export const x = 1;");
    for (path, contents) in files {
        write(&root.join(path), contents);
    }
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn assert_button_is_exposed(shape: &str, files: &[(&str, &str)]) {
    let unused = unused_files(files);
    assert!(
        !unused.iter().any(|path| path.ends_with("src/Button.tsx")),
        "{shape}: the exposed file is an entry point, got {unused:?}"
    );
    assert!(
        unused.iter().any(|path| path.ends_with("src/orphan.ts")),
        "{shape}: an unexposed sibling still reports, got {unused:?}"
    );
}

/// Item 3: an exported top-level `const` resolves like a plain one.
#[test]
fn exported_options_const_is_read() {
    assert_button_is_exposed(
        "export const",
        &[(
            "webpack.config.ts",
            r#"import { ModuleFederationPlugin } from "@module-federation/enhanced";
               export const mfConfig = { name: "app", exposes: { "./Button": "./src/Button.tsx" } };
               export default { plugins: [new ModuleFederationPlugin(mfConfig)] };"#,
        )],
    );
}

/// Item 4: a TypeScript non-null assertion on the options argument.
#[test]
fn non_null_asserted_options_are_read() {
    assert_button_is_exposed(
        "non-null options",
        &[(
            "webpack.config.ts",
            r#"import { ModuleFederationPlugin } from "@module-federation/enhanced";
               const mfConfig = { name: "app", exposes: { "./Button": "./src/Button.tsx" } };
               export default { plugins: [new ModuleFederationPlugin(mfConfig!)] };"#,
        )],
    );
}

/// Item 4: a computed member callee with a string literal property.
#[test]
fn computed_string_member_callee_is_read() {
    assert_button_is_exposed(
        "computed member callee",
        &[(
            "webpack.config.js",
            r#"const container = require("@module-federation/enhanced");
               module.exports = {
                 plugins: [new container["ModuleFederationPlugin"]({
                   name: "app",
                   exposes: { "./Button": "./src/Button.tsx" },
                 })],
               };"#,
        )],
    );
}

/// Item 1: a default import of a relative sibling config.
#[test]
fn options_from_a_default_import_are_read() {
    assert_button_is_exposed(
        "default import",
        &[
            (
                "webpack.config.mjs",
                r#"import { ModuleFederationPlugin } from "@module-federation/enhanced";
                   import mfConfig from "./mf.config.mjs";
                   export default { plugins: [new ModuleFederationPlugin(mfConfig)] };"#,
            ),
            (
                "mf.config.mjs",
                r#"export default { name: "app", exposes: { "./Button": "./src/Button.tsx" } };"#,
            ),
        ],
    );
}

/// Item 1: a named import of an extensionless relative sibling config.
#[test]
fn options_from_a_named_import_are_read() {
    assert_button_is_exposed(
        "named import",
        &[
            (
                "webpack.config.ts",
                r#"import { ModuleFederationPlugin } from "@module-federation/enhanced";
                   import { mfConfig } from "./mf.config";
                   export default { plugins: [new ModuleFederationPlugin(mfConfig)] };"#,
            ),
            (
                "mf.config.ts",
                r#"export const mfConfig = { name: "app", exposes: { "./Button": "./src/Button.tsx" } };"#,
            ),
        ],
    );
}

/// Item 2: an options object that spreads a same-file binding.
#[test]
fn spread_options_are_read() {
    assert_button_is_exposed(
        "spread options",
        &[(
            "webpack.config.js",
            r#"const { ModuleFederationPlugin } = require("@module-federation/enhanced");
               const base = { exposes: { "./Button": "./src/Button.tsx" } };
               const mfConfig = { ...base };
               module.exports = { plugins: [new ModuleFederationPlugin(mfConfig)] };"#,
        )],
    );
}

/// Item 2: `Object.assign({}, base)` over a same-file binding.
#[test]
fn object_assign_options_are_read() {
    assert_button_is_exposed(
        "Object.assign options",
        &[(
            "webpack.config.js",
            r#"const { ModuleFederationPlugin } = require("@module-federation/enhanced");
               const base = { exposes: { "./Button": "./src/Button.tsx" } };
               module.exports = {
                 plugins: [new ModuleFederationPlugin(Object.assign({}, base))],
               };"#,
        )],
    );
}
