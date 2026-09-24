//! `--trace-file` and `--trace-dependency` name the Module Federation source
//! when a Federation config is why a file is an entry point or why a name is
//! provided (issue #2796).
//!
//! Before, a file exposed through `exposes` traced as `is_entry_point: true`
//! with nothing that said why, and a `remotes` alias traced like any package.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, redact_paths, run_fallow_in_root};
use serde_json::{Value, json};
use std::path::Path;
use tempfile::TempDir;

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

/// A root project with a standalone config and a workspace package whose
/// webpack config declares the options inline.
fn federation_project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{
            "name": "host",
            "private": true,
            "main": "src/index.ts",
            "workspaces": ["packages/*"],
            "dependencies": { "react": "^18.0.0" },
            "devDependencies": { "@module-federation/enhanced": "^0.9.0" }
        }"#,
    );
    write(
        root,
        "module-federation.config.ts",
        r"
        export default {
            name: 'host',
            exposes: { './Button': './src/Button.tsx' },
            remotes: { checkout: 'checkout@https://example.test/remoteEntry.js' },
        };
        ",
    );
    write(
        root,
        "src/index.ts",
        "import React from 'react';\nexport const app = React;\nexport const widget = () => import('checkout/Widget');\n",
    );
    write(
        root,
        "src/Button.tsx",
        "export const Button = (): string => 'button';\n",
    );
    write(
        root,
        "packages/cart/package.json",
        r#"{ "name": "cart", "devDependencies": { "webpack": "^5.98.0" } }"#,
    );
    write(
        root,
        "packages/cart/webpack.config.js",
        r"
        const { ModuleFederationPlugin } = require('@module-federation/enhanced/webpack');
        module.exports = {
            plugins: [new ModuleFederationPlugin({
                name: 'cart',
                exposes: { './Cart': './src/Cart.ts' },
                remotes: { checkout: 'checkout@https://example.test/remoteEntry.js' },
            })],
        };
        ",
    );
    write(
        root,
        "packages/cart/src/Cart.ts",
        "export const Cart = 1;\n",
    );
    dir
}

fn trace(root: &Path, flag: &str, target: &str) -> Value {
    parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &[flag, target, "--format", "json", "--quiet", "--no-cache"],
    ))
}

#[test]
fn an_exposed_file_names_the_config_that_exposes_it() {
    let dir = federation_project();
    let root = dir.path();

    let button = trace(root, "--trace-file", "src/Button.tsx");
    assert_eq!(button["is_entry_point"], true, "{button:#}");
    assert_eq!(
        button["sources"],
        json!([{
            "kind": "module-federation",
            "plugin": "module-federation",
            "config": "module-federation.config.ts",
            "key": "exposes",
        }]),
        "{button:#}"
    );

    let cart = trace(root, "--trace-file", "packages/cart/src/Cart.ts");
    assert_eq!(
        cart["sources"],
        json!([{
            "kind": "module-federation",
            "plugin": "webpack",
            "config": "packages/cart/webpack.config.js",
            "key": "exposes",
        }]),
        "{cart:#}"
    );
}

#[test]
fn a_remote_alias_names_every_config_that_declares_it() {
    let dir = federation_project();
    let checkout = trace(dir.path(), "--trace-dependency", "checkout");
    assert_eq!(
        checkout["sources"],
        json!([
            {
                "kind": "module-federation",
                "plugin": "module-federation",
                "config": "module-federation.config.ts",
                "key": "remotes",
            },
            {
                "kind": "module-federation",
                "plugin": "webpack",
                "config": "packages/cart/webpack.config.js",
                "key": "remotes",
            },
        ]),
        "{checkout:#}"
    );
}

#[test]
fn a_trace_without_federation_involvement_is_unchanged() {
    let dir = federation_project();
    let root = dir.path();
    let index = trace(root, "--trace-file", "src/index.ts");
    assert!(index.get("sources").is_none(), "{index:#}");
    let react = trace(root, "--trace-dependency", "react");
    assert!(react.get("sources").is_none(), "{react:#}");
}

#[test]
fn the_human_trace_names_the_federation_source() {
    let dir = federation_project();
    let root = dir.path();
    let file = run_fallow_in_root(
        "dead-code",
        root,
        &["--trace-file", "src/Button.tsx", "--quiet", "--no-cache"],
    );
    let file_text = redact_paths(&format!("{}{}", file.stdout, file.stderr), root);
    assert!(
        file_text.contains("Module Federation `exposes` in module-federation.config.ts"),
        "{file_text}"
    );
    let dependency = run_fallow_in_root(
        "dead-code",
        root,
        &["--trace-dependency", "checkout", "--quiet", "--no-cache"],
    );
    let dependency_text =
        redact_paths(&format!("{}{}", dependency.stdout, dependency.stderr), root);
    assert!(
        dependency_text.contains("Module Federation `remotes` in module-federation.config.ts"),
        "{dependency_text}"
    );
}

/// A remote that only a runtime `registerRemotes` or `loadRemote` call names
/// traces to the source file and the function that name it.
#[test]
fn a_runtime_remote_names_the_file_and_the_call() {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "host", "main": "src/index.ts", "dependencies": { "@module-federation/runtime": "^0.9.0" } }"#,
    );
    write(
        root,
        "src/index.ts",
        r"
        import { registerRemotes, loadRemote } from '@module-federation/runtime';
        registerRemotes([{ name: 'checkout', entry: 'https://example.test/mf.js' }]);
        export const cart = () => loadRemote('cart/Widget');
        export const widget = () => import('checkout/Widget');
        ",
    );
    let checkout = trace(root, "--trace-dependency", "checkout");
    assert_eq!(
        checkout["sources"],
        json!([{
            "kind": "module-federation",
            "plugin": "module-federation",
            "config": "src/index.ts",
            "key": "registerRemotes",
        }]),
        "{checkout:#}"
    );
    let cart = trace(root, "--trace-dependency", "cart");
    assert_eq!(cart["sources"][0]["key"], "loadRemote", "{cart:#}");
}
