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

/// A config in `build/` is read beside a root config too. Source discovery
/// skips `build/`, so only the filesystem probe finds it.
#[test]
fn a_build_directory_config_is_read_beside_a_root_config() {
    let unused = unused_files(
        r#""webpack": "^5.98.0""#,
        &[
            (
                "webpack.config.js",
                r#"module.exports = { entry: "./src/index.ts" };"#,
            ),
            (
                "build/webpack.prod.js",
                r#"module.exports = { mode: "production", entry: "./src/prod.ts" };"#,
            ),
            ("src/index.ts", "export const index = 1;"),
            ("src/prod.ts", "export const prod = 1;"),
        ],
    );
    assert_used(
        "root config plus build/webpack.prod.js",
        &unused,
        &["src/index.ts", "src/prod.ts"],
    );
}

/// `build/` holds build output, so a `webpack.config.js` there can be compiled
/// or stale. It is not read, and what it names stays reported.
#[test]
fn a_webpack_config_js_in_the_build_directory_is_not_read() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{ "name": "stale-build", "private": true,
             "devDependencies": { "webpack": "^5.98.0", "stale-loader": "^1.0.0" } }"#,
    );
    write(
        &root.join("build/webpack.config.js"),
        r#"module.exports = {
             entry: "./src/old.ts",
             module: { rules: [{ test: /\.ts$/, loader: "stale-loader" }] },
           };"#,
    );
    write(&root.join("src/index.ts"), "export const index = 1;");
    write(&root.join("src/old.ts"), "export const old = 1;");
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        unused.iter().any(|path| path.ends_with("src/old.ts")),
        "a stale build output names no entry, got {unused:?}"
    );
    let unused_dev: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        unused_dev.contains(&"stale-loader"),
        "a loader that only stale output names stays unused, got {unused_dev:?}"
    );
}

/// Analyze a project with `package_json`, an unreferenced `src/orphan.ts` and
/// `files`, and return the unused files and the unused dependency names.
fn unused_files_and_dependencies(
    package_json: &str,
    files: &[(&str, &str)],
) -> (Vec<String>, Vec<String>) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(&root.join("package.json"), package_json);
    write(&root.join("src/orphan.ts"), "export const x = 1;");
    for (path, contents) in files {
        write(&root.join(path), contents);
    }
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    let unused_dependencies = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.clone())
        .chain(
            results
                .unused_dev_dependencies
                .iter()
                .map(|finding| finding.dep.package_name.clone()),
        )
        .collect();
    (unused_files, unused_dependencies)
}

/// Item 3: a bare rollup, rolldown or vite `input` value can name a package or
/// a path. It credits the package, so no `remove-dependency` action is
/// offered, and it keeps the entry pattern, so a path value still credits the
/// file. `build.lib.entry` stays a path.
#[test]
fn a_bare_bundler_input_credits_the_package_and_the_file() {
    for (case, tool, config_name, config) in [
        (
            "rollup",
            "rollup",
            "rollup.config.mjs",
            r#"export default { input: ["my-lib/client", "src/app"] };"#,
        ),
        (
            "rolldown",
            "rolldown",
            "rolldown.config.mjs",
            r#"export default { input: { client: "my-lib/client", app: "src/app" } };"#,
        ),
        (
            "vite",
            "vite",
            "vite.config.mjs",
            r#"export default { build: { rollupOptions: { input: "my-lib/client" }, lib: { entry: "src/app" } } };"#,
        ),
    ] {
        let package_json = format!(
            r#"{{ "name": "bare-input", "private": true, "dependencies": {{ "my-lib": "^1.0.0" }}, "devDependencies": {{ "{tool}": "^1.0.0" }} }}"#
        );
        let (unused, unused_dependencies) = unused_files_and_dependencies(
            &package_json,
            &[
                (config_name, config),
                ("src/app.js", "export const app = 1;"),
            ],
        );
        assert_used(case, &unused, &["src/app.js"]);
        assert!(
            !unused_dependencies.contains(&"my-lib".to_string()),
            "{case}: the package named by input is used, got {unused_dependencies:?}"
        );
    }
}

/// A bare input that names a project file is a path, so it does not credit a
/// declared package that has the same first segment. A package that no code
/// imports stays reported.
#[test]
fn a_bare_input_that_names_a_project_file_credits_no_package() {
    let package_json = r#"{ "name": "bare-input", "private": true, "dependencies": { "lib": "^1.0.0", "my-lib": "^1.0.0" }, "devDependencies": { "rollup": "^4.0.0" } }"#;
    let (unused, unused_dependencies) = unused_files_and_dependencies(
        package_json,
        &[
            (
                "rollup.config.mjs",
                r#"export default { input: ["lib/index", "my-lib/client"] };"#,
            ),
            ("lib/index.js", "export const lib = 1;"),
        ],
    );
    assert_used("local file", &unused, &["lib/index.js"]);
    assert!(
        unused_dependencies.contains(&"lib".to_string()),
        "a local file does not credit the package, got {unused_dependencies:?}"
    );
    assert!(
        !unused_dependencies.contains(&"my-lib".to_string()),
        "a value with no local file credits the package, got {unused_dependencies:?}"
    );
}
