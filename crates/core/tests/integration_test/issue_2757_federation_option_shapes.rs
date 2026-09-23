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

/// Item 1: a CommonJS `require` of a relative sibling config.
#[test]
fn options_from_a_relative_require_are_read() {
    assert_button_is_exposed(
        "relative require",
        &[
            (
                "webpack.config.js",
                r#"const { ModuleFederationPlugin } = require("@module-federation/enhanced");
                   const mfConfig = require("./mf.config");
                   module.exports = { plugins: [new ModuleFederationPlugin(mfConfig)] };"#,
            ),
            (
                "mf.config.js",
                r#"module.exports = { name: "app", exposes: { "./Button": "./src/Button.tsx" } };"#,
            ),
        ],
    );
}

/// A webpack config under `config/` resolves `exposes` and scopes `remotes`
/// against the package root, as webpack does without `context`.
#[test]
fn a_config_under_the_config_directory_anchors_to_the_package_root() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(&root.join("package.json"), PACKAGE_JSON);
    write(
        &root.join("config/webpack.client.js"),
        r#"const { ModuleFederationPlugin } = require("@module-federation/enhanced");
           module.exports = {
             entry: "./src/index.ts",
             plugins: [new ModuleFederationPlugin({
               name: "app",
               exposes: { "./Button": "./src/Button.tsx" },
               remotes: { checkout: "checkout@https://example.test/remoteEntry.js" },
             })],
           };"#,
    );
    write(&root.join("src/index.ts"), r#"import "checkout/Cart";"#);
    write(
        &root.join("src/Button.tsx"),
        r#"export default (): string => "button";"#,
    );
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused.iter().any(|path| path.ends_with("src/Button.tsx")),
        "the exposed file is an entry point, got {unused:?}"
    );
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.contains(&"checkout"),
        "the remote alias covers the package, got {unlisted:?}"
    );
}

/// Item 7: an inline call that is not a known wrapper keeps the credit from
/// the object literal passed to it, the same as the bound form.
#[test]
fn an_unrecognized_wrapper_call_keeps_the_inner_literal_credit() {
    for (shape, config) in [
        (
            "inline wrapper",
            r#"const { ModuleFederationPlugin } = require("@module-federation/enhanced");
               const federationConfig = (options) => options;
               module.exports = {
                 plugins: [new ModuleFederationPlugin(federationConfig({ name: "app", exposes: { "./Button": "./src/Button.tsx" } }))],
               };"#,
        ),
        (
            "bound wrapper",
            r#"const { ModuleFederationPlugin } = require("@module-federation/enhanced");
               const federationConfig = (options) => options;
               const mf = federationConfig({ name: "app", exposes: { "./Button": "./src/Button.tsx" } });
               module.exports = { plugins: [new ModuleFederationPlugin(mf)] };"#,
        ),
        (
            "inline identity wrapper",
            r#"const { ModuleFederationPlugin, createModuleFederationConfig } = require("@module-federation/enhanced");
               module.exports = {
                 plugins: [new ModuleFederationPlugin(createModuleFederationConfig({ name: "app", exposes: { "./Button": "./src/Button.tsx" } }))],
               };"#,
        ),
    ] {
        assert_button_is_exposed(shape, &[("webpack.config.js", config)]);
    }
}

/// Item 6: a standalone `module-federation.config.*` that declares a Federation
/// key credits the build plugin, which no config file imports. The runtime
/// package is not credited from a config file.
#[test]
fn a_standalone_config_credits_the_build_plugin() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{
            "name": "mf-standalone",
            "private": true,
            "main": "src/index.ts",
            "devDependencies": {
                "@module-federation/enhanced": "^0.9.0",
                "@module-federation/runtime": "^0.9.0"
            }
        }"#,
    );
    write(
        &root.join("module-federation.config.js"),
        r#"module.exports = { name: "app", exposes: { "./Button": "./src/Button.tsx" } };"#,
    );
    write(&root.join("src/index.ts"), "export const x = 1;");
    write(
        &root.join("src/Button.tsx"),
        r#"export default (): string => "button";"#,
    );
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unused.contains(&"@module-federation/enhanced"),
        "the build plugin is credited, got {unused:?}"
    );
    assert!(
        unused.contains(&"@module-federation/runtime"),
        "an unused runtime still reports, got {unused:?}"
    );
}

/// Item 5: an `exposes` target in a sibling workspace is inside the project,
/// so it is an entry point. A target outside the project matches no file.
#[test]
fn an_exposes_target_in_a_sibling_workspace_is_credited() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{ "name": "mf-monorepo", "private": true, "workspaces": ["packages/*"] }"#,
    );
    write(&root.join("packages/app/package.json"), PACKAGE_JSON);
    write(
        &root.join("packages/app/webpack.config.js"),
        r#"const { ModuleFederationPlugin } = require("@module-federation/enhanced");
           module.exports = {
             entry: "./src/index.ts",
             plugins: [new ModuleFederationPlugin({
               name: "app",
               exposes: {
                 "./Thing": "../shared/src/Thing.tsx",
                 "./Outside": "../../../outside/Thing.tsx",
               },
             })],
           };"#,
    );
    write(&root.join("packages/app/src/index.ts"), "console.log(1);");
    write(
        &root.join("packages/shared/package.json"),
        r#"{ "name": "shared", "private": true, "main": "src/index.ts" }"#,
    );
    write(
        &root.join("packages/shared/src/index.ts"),
        "export const x = 1;",
    );
    write(
        &root.join("packages/shared/src/Thing.tsx"),
        "export const Thing = 1;",
    );
    write(
        &root.join("packages/shared/src/orphan.ts"),
        "export const y = 1;",
    );
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused
            .iter()
            .any(|path| path.ends_with("packages/shared/src/Thing.tsx")),
        "the exposed sibling-workspace file is an entry point, got {unused:?}"
    );
    assert!(
        unused
            .iter()
            .any(|path| path.ends_with("packages/shared/src/orphan.ts")),
        "an unexposed file still reports, got {unused:?}"
    );
}

/// The sibling-workspace resolution belongs to the Federation reader only. A
/// Storybook `stories` pattern such as `../src/**` is relative to its config
/// directory, so it must not credit a file one directory above the workspace.
#[test]
fn a_storybook_parent_pattern_does_not_climb_out_of_its_workspace() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{ "name": "sb-monorepo", "private": true, "workspaces": ["packages/*"] }"#,
    );
    write(
        &root.join("packages/ui/package.json"),
        r#"{ "name": "ui", "private": true, "main": "src/index.ts", "devDependencies": { "storybook": "^8.0.0", "@storybook/react": "^8.0.0" } }"#,
    );
    write(
        &root.join("packages/ui/.storybook/main.ts"),
        r#"export default { stories: ["../src/**/*.docs.tsx"] };"#,
    );
    write(
        &root.join("packages/ui/src/index.ts"),
        "export const x = 1;",
    );
    write(
        &root.join("packages/src/stray.docs.tsx"),
        "export const stray = 1;",
    );
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        unused
            .iter()
            .any(|path| path.ends_with("packages/src/stray.docs.tsx")),
        "a file outside the workspace is not a story, got {unused:?}"
    );
}
