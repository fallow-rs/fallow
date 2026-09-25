//! Regression tests for issue #2876: three Module Federation shapes that the
//! readers did not cover.
//!
//! - A bare package named as an `exposes` target credited the package in every
//!   workspace, not only in the package that owns the config.
//! - A Federation plugin call in a helper module that a bundler config imports
//!   was not read, so its `shared`, `remotes` and `exposes` gave no credit.
//! - A runtime call in the `<script>` block of a `.vue` or `.svelte` file was
//!   not read, and `init({ remotes })` registered nothing.

use std::path::Path;

use fallow_config::WorkspaceDiagnosticKind;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

struct Outcome {
    /// `package.json path:package` of each unused dependency.
    unused: Vec<String>,
    /// `importing file:package` of each unlisted dependency.
    unlisted: Vec<String>,
    /// Root-relative paths of the unused files.
    unused_files: Vec<String>,
    /// `(file name, key, reason)` of each `plugin-config-unreadable` entry.
    unreadable: Vec<(String, String, String)>,
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Build a project from `files` (path, contents) and return its findings.
fn analyze(files: &[(&str, &str)]) -> Outcome {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonical temp dir");
    for (path, contents) in files {
        write(&root.join(path), contents);
    }
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = results
        .unused_dependencies
        .iter()
        .map(|finding| {
            format!(
                "{}:{}",
                relative(&root, &finding.dep.path),
                finding.dep.package_name
            )
        })
        .collect();
    let unlisted = results
        .unlisted_dependencies
        .iter()
        .flat_map(|finding| {
            finding
                .dep
                .imported_from
                .iter()
                .map(|site| {
                    format!(
                        "{}:{}",
                        relative(&root, &site.path),
                        finding.dep.package_name
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let unused_files = results
        .unused_files
        .iter()
        .map(|finding| relative(&root, &finding.file.path))
        .collect();
    let unreadable = fallow_config::workspace_diagnostics_for(&config.root)
        .into_iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            WorkspaceDiagnosticKind::PluginConfigUnreadable { key, reason, .. } => Some((
                diagnostic
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                key,
                reason,
            )),
            _ => None,
        })
        .collect();
    Outcome {
        unused,
        unlisted,
        unused_files,
        unreadable,
    }
}

const ROOT_PACKAGE: &str = r#"{ "name": "mono", "private": true, "workspaces": ["packages/*"] }"#;

/// A bare package that `exposes` names credits the package that owns the
/// config. A sibling workspace that declares the same package and never uses
/// it still reports it.
#[test]
fn a_bare_exposes_target_credits_only_the_package_that_owns_the_config() {
    let package = |name: &str| {
        format!(
            r#"{{
                "name": "{name}",
                "main": "src/index.ts",
                "dependencies": {{ "shared-utils": "^1.0.0" }},
                "devDependencies": {{ "webpack": "^5.98.0" }}
            }}"#
        )
    };
    let host = package("host");
    let other = package("other");
    let outcome = analyze(&[
        ("package.json", ROOT_PACKAGE),
        ("packages/host/package.json", &host),
        ("packages/host/src/index.ts", "export const app = 1;\n"),
        (
            "packages/host/webpack.config.js",
            r"
            module.exports = {
                plugins: [
                    new ModuleFederationPlugin({ name: 'host', exposes: { './utils': 'shared-utils' } }),
                ],
            };
            ",
        ),
        ("packages/other/package.json", &other),
        ("packages/other/src/index.ts", "export const app = 1;\n"),
    ]);
    assert!(
        !outcome
            .unused
            .contains(&"packages/host/package.json:shared-utils".to_string()),
        "the host exposes shared-utils, got {:?}",
        outcome.unused
    );
    assert!(
        outcome
            .unused
            .contains(&"packages/other/package.json:shared-utils".to_string()),
        "a sibling that does not expose shared-utils still reports it, got {:?}",
        outcome.unused
    );
}

/// The `federated-css-react-ssr` shape: `config/webpack.client.js` requires
/// `./module-federation`, which exports the plugin instances. The helper is
/// read as if its calls sat in the config that imports it.
#[test]
fn a_plugin_call_in_an_imported_helper_module_is_read() {
    let outcome = analyze(&[
        (
            "package.json",
            r#"{
                "name": "shell",
                "main": "src/index.js",
                "scripts": { "build": "webpack --config config/webpack.client.js" },
                "dependencies": { "react": "^18.0.0", "styled-components": "^6.0.0" },
                "devDependencies": { "webpack": "^5.98.0", "@module-federation/enhanced": "^0.9.0" }
            }"#,
        ),
        (
            "src/index.js",
            "import React from 'react';\nexport const widget = () => import('checkout/Widget');\n",
        ),
        ("src/Button.js", "export const Button = () => null;\n"),
        (
            "config/module-federation.js",
            r"
            const { ModuleFederationPlugin } = require('@module-federation/enhanced/webpack');
            module.exports = {
                client: new ModuleFederationPlugin({
                    name: 'shell',
                    remotes: { checkout: 'checkout@http://localhost:3001/remoteEntry.js' },
                    exposes: { './Button': './src/Button' },
                    shared: [{ react: { singleton: true }, 'styled-components': { singleton: true } }],
                }),
            };
            ",
        ),
        (
            "config/webpack.client.js",
            r"
            const moduleFederationPlugin = require('./module-federation');
            module.exports = {
                entry: './src/index.js',
                plugins: [moduleFederationPlugin.client],
            };
            ",
        ),
    ]);
    assert!(
        !outcome
            .unused
            .iter()
            .any(|finding| finding.ends_with(":styled-components")),
        "the helper shares styled-components, got {:?}",
        outcome.unused
    );
    assert!(
        !outcome
            .unlisted
            .iter()
            .any(|finding| finding.ends_with(":checkout")),
        "the helper declares the checkout remote, got {:?}",
        outcome.unlisted
    );
    assert!(
        !outcome.unused_files.contains(&"src/Button.js".to_string()),
        "the helper exposes src/Button.js, got {:?}",
        outcome.unused_files
    );
}

/// A module that no bundler config imports is not read, even when it holds a
/// Federation plugin call.
#[test]
fn a_helper_module_that_no_config_imports_is_not_read() {
    let outcome = analyze(&[
        (
            "package.json",
            r#"{
                "name": "shell",
                "main": "src/index.js",
                "dependencies": { "styled-components": "^6.0.0" },
                "devDependencies": { "webpack": "^5.98.0" }
            }"#,
        ),
        ("src/index.js", "export const app = 1;\n"),
        (
            "tools/federation.js",
            r"
            module.exports = new ModuleFederationPlugin({
                name: 'shell',
                shared: { 'styled-components': { singleton: true } },
            });
            ",
        ),
        (
            "webpack.config.js",
            "module.exports = { entry: './src/index.js' };\n",
        ),
    ]);
    assert!(
        outcome
            .unused
            .iter()
            .any(|finding| finding.ends_with(":styled-components")),
        "a module that no config imports gives no credit, got {:?}",
        outcome.unused
    );
}

fn runtime_package(name: &str) -> String {
    format!(
        r#"{{ "name": "{name}", "main": "src/index.ts", "dependencies": {{ "@module-federation/runtime": "^0.9.0", "@module-federation/enhanced": "^0.9.0" }} }}"#
    )
}

/// A runtime call in a Vue or Svelte script block registers its remote, and
/// `init({ remotes })` registers remotes like `registerRemotes`.
#[test]
fn runtime_calls_in_sfc_script_blocks_provide_their_remotes() {
    let host = runtime_package("host");
    let outcome = analyze(&[
        ("package.json", ROOT_PACKAGE),
        ("packages/host/package.json", &host),
        (
            "packages/host/src/index.ts",
            r"
            import App from './App.vue';
            import Shell from './Shell.svelte';
            export { App, Shell };
            export const widget = () => import('checkout/Widget');
            export const cart = () => import('cart/Summary');
            ",
        ),
        (
            "packages/host/src/App.vue",
            r#"
<template><div /></template>
<script setup lang="ts">
import { loadRemote } from '@module-federation/enhanced/runtime';
loadRemote('checkout/Widget');
</script>
"#,
        ),
        (
            "packages/host/src/Shell.svelte",
            r"
<script>
  import { init } from '@module-federation/runtime';
  init({ name: 'host', remotes: [{ name: 'cart', entry: 'https://example.test/cart.js' }] });
</script>
<main />
",
        ),
    ]);
    assert!(
        !outcome
            .unlisted
            .iter()
            .any(|finding| finding.ends_with(":checkout") || finding.ends_with(":cart")),
        "the SFC calls provide checkout and cart, got {:?}",
        outcome.unlisted
    );
    assert!(
        outcome.unreadable.is_empty(),
        "got {:?}",
        outcome.unreadable
    );
}

/// A runtime call with a dynamic argument in an SFC records the advisory on
/// the SFC file.
#[test]
fn a_dynamic_runtime_call_in_an_sfc_is_recorded() {
    let host = runtime_package("host");
    let outcome = analyze(&[
        ("package.json", ROOT_PACKAGE),
        ("packages/host/package.json", &host),
        (
            "packages/host/src/index.ts",
            "import App from './App.vue';\nexport { App };\n",
        ),
        (
            "packages/host/src/App.vue",
            r"
<script setup>
import { init } from '@module-federation/runtime';
init(options);
</script>
",
        ),
    ]);
    assert_eq!(
        outcome.unreadable,
        vec![(
            "App.vue".to_string(),
            "init".to_string(),
            "dynamic-argument".to_string()
        )]
    );
}
