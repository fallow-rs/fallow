//! Regression tests for issue #2753: bundler config readers missed entries.
//!
//! A webpack config under `config/` was not read, rspack and rsbuild ignored
//! the base directory option, and an entry that names a directory or an
//! extensionless file matched no file.

use std::path::Path;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// Build a project from `files` (path, contents), add an unreferenced
/// `src/orphan.ts`, and return the unused files.
fn unused_files(dev_dependencies: &str, files: &[(&str, &str)]) -> Vec<String> {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        &format!(
            r#"{{ "name": "bundler-entries", "private": true, "devDependencies": {{ {dev_dependencies} }} }}"#
        ),
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

fn assert_used(case: &str, unused: &[String], used: &[&str]) {
    for path in used {
        assert!(
            !unused.iter().any(|unused| unused.ends_with(path)),
            "{case}: {path} is used, got {unused:?}"
        );
    }
    assert!(
        unused.iter().any(|path| path.ends_with("src/orphan.ts")),
        "{case}: an unreferenced file still reports, got {unused:?}"
    );
}

/// Item 2: `config/webpack.client.js` is a webpack config. The config file and
/// its entry are used.
#[test]
fn webpack_config_under_config_directory_is_read() {
    let unused = unused_files(
        r#""webpack": "^5.98.0""#,
        &[
            (
                "config/webpack.client.js",
                r#"module.exports = { entry: "./src/client.ts" };"#,
            ),
            ("src/client.ts", "export const client = 1;"),
        ],
    );
    assert_used(
        "config/webpack.client.js",
        &unused,
        &["config/webpack.client.js", "src/client.ts"],
    );
}

/// Item 4: rspack resolves a relative entry against `context`.
#[test]
fn rspack_entry_resolves_against_context() {
    let unused = unused_files(
        r#""@rspack/core": "^1.0.0""#,
        &[
            (
                "rspack.config.js",
                r#"const path = require("path");
                   module.exports = { context: path.resolve(__dirname, "src"), entry: "./app.ts" };"#,
            ),
            ("src/app.ts", "export const app = 1;"),
        ],
    );
    assert_used("rspack context", &unused, &["src/app.ts"]);
}

/// Item 4: rsbuild resolves a relative entry against `root`.
#[test]
fn rsbuild_entry_resolves_against_root() {
    let unused = unused_files(
        r#""@rsbuild/core": "^1.0.0""#,
        &[
            (
                "rsbuild.config.ts",
                r#"import path from "node:path";
                   import { defineConfig } from "@rsbuild/core";
                   export default defineConfig({
                     root: path.resolve(__dirname, "app"),
                     source: { entry: { index: "./main.ts" } },
                   });"#,
            ),
            ("app/main.ts", "export const main = 1;"),
        ],
    );
    assert_used("rsbuild root", &unused, &["app/main.ts"]);
}

/// A directory entry resolves to the directory index, as webpack does.
#[test]
fn webpack_directory_entry_resolves_to_its_index() {
    let unused = unused_files(
        r#""webpack": "^5.98.0""#,
        &[
            (
                "webpack.config.js",
                r#"module.exports = { entry: "./lib" };"#,
            ),
            ("lib/index.ts", "export const lib = 1;"),
        ],
    );
    assert_used("directory entry", &unused, &["lib/index.ts"]);
}

/// An extensionless file entry resolves with the source extensions, as webpack
/// does before it tries the directory index.
#[test]
fn webpack_extensionless_entry_resolves_to_the_file() {
    let unused = unused_files(
        r#""webpack": "^5.98.0""#,
        &[
            (
                "webpack.config.js",
                r#"module.exports = { entry: "./src/app" };"#,
            ),
            ("src/app.ts", "export const app = 1;"),
        ],
    );
    assert_used("extensionless entry", &unused, &["src/app.ts"]);
}

/// An extensionless entry and a directory entry resolve against `context` too,
/// for webpack and for rspack.
#[test]
fn extensionless_entries_resolve_against_context() {
    for (dependency, config_file) in [
        (r#""webpack": "^5.98.0""#, "webpack.config.js"),
        (r#""@rspack/core": "^1.0.0""#, "rspack.config.js"),
    ] {
        let unused = unused_files(
            dependency,
            &[
                (
                    config_file,
                    r#"const path = require("path");
                       module.exports = {
                         context: path.resolve(__dirname, "app"),
                         entry: { a: "./main", b: "./widgets" },
                       };"#,
                ),
                ("app/main.ts", "export const main = 1;"),
                ("app/widgets/index.ts", "export const widgets = 1;"),
            ],
        );
        assert_used(
            config_file,
            &unused,
            &["app/main.ts", "app/widgets/index.ts"],
        );
    }
}

/// The rsbuild default entry shape, `./src/index`, resolves against `root`.
#[test]
fn rsbuild_default_entry_resolves_against_root() {
    let unused = unused_files(
        r#""@rsbuild/core": "^1.0.0""#,
        &[
            (
                "rsbuild.config.ts",
                r#"import path from "node:path";
                   import { defineConfig } from "@rsbuild/core";
                   export default defineConfig({
                     root: path.resolve(__dirname, "app"),
                     source: { entry: { index: "./src/index" } },
                   });"#,
            ),
            ("app/src/index.ts", "export const index = 1;"),
        ],
    );
    assert_used("rsbuild root default entry", &unused, &["app/src/index.ts"]);
}

/// `build/` and `webpack/` hold webpack configs too.
#[test]
fn webpack_configs_under_build_and_webpack_directories_are_read() {
    for directory in ["build", "webpack"] {
        let config_file = format!("{directory}/webpack.prod.js");
        let unused = unused_files(
            r#""webpack": "^5.98.0""#,
            &[
                (
                    &config_file,
                    r#"module.exports = { mode: "production", entry: "./src/client.ts" };"#,
                ),
                ("src/client.ts", "export const client = 1;"),
            ],
        );
        assert_used(&config_file, &unused, &[&config_file, "src/client.ts"]);
    }
}

/// A helper module beside the configs is not a config, so it stays reportable
/// when nothing imports it.
#[test]
fn webpack_helper_modules_in_a_config_directory_stay_reportable() {
    let unused = unused_files(
        r#""webpack": "^5.98.0""#,
        &[
            (
                "config/webpack.client.js",
                r#"const { merge } = require("webpack-merge");
                   const common = require("./webpack.common.js");
                   module.exports = merge(common, { mode: "development" });"#,
            ),
            (
                "config/webpack.common.js",
                r#"module.exports = { entry: "./src/client.ts" };"#,
            ),
            (
                "config/webpack.paths.js",
                r#"const path = require("path");
                   module.exports = { src: path.resolve(__dirname, "../src") };"#,
            ),
            (
                "config/webpack.parts.js",
                r"exports.devServer = () => ({ devServer: { hot: true } });",
            ),
            ("src/client.ts", "export const client = 1;"),
        ],
    );
    assert_used(
        "config helpers",
        &unused,
        &[
            "config/webpack.client.js",
            "config/webpack.common.js",
            "src/client.ts",
        ],
    );
    for helper in ["config/webpack.paths.js", "config/webpack.parts.js"] {
        assert!(
            unused.iter().any(|path| path.ends_with(helper)),
            "{helper} is a helper that nothing imports, got {unused:?}"
        );
    }
}
