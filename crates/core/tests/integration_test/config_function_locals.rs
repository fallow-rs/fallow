//! Config files that build their value from local bindings inside a function.
//!
//! A webpack config can export a function that declares `const common = {...}`,
//! spreads it into more configs, and returns an array of them. A Vite config
//! can declare its value in a `defineConfig` callback and return the binding.
//! The static config readers must see the returned value.

use super::common::create_config;

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

fn unused_file_keys(
    results: &fallow_core::results::AnalysisResults,
    root: &std::path::Path,
) -> Vec<String> {
    let mut keys: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            let path = f.file.path.strip_prefix(root).unwrap_or(&f.file.path);
            path.to_string_lossy().replace('\\', "/")
        })
        .collect();
    keys.sort();
    keys
}

const WEBPACK_FUNCTION_MULTI_CONFIG: &str = r#"const path = require("path");

module.exports = (env, argv) => {
  const mode = argv.mode || "development";
  const plugins = [];
  const common = {
    mode,
    module: { rules: [{ test: /\.tsx?$/, use: "ts-loader" }] },
    resolve: {
      alias: { linkedom: path.resolve(__dirname, "src/shims/linkedom.ts") },
      extensions: [".ts", ".js"],
      fallback: { process: false },
    },
    plugins,
    output: { filename: "js/[name].js", path: path.resolve(__dirname, "dist") },
  };
  const bgConfig = {
    ...common,
    entry: { background: path.resolve(__dirname, "./src/bg.ts") },
    target: "webworker",
    resolve: {
      ...common.resolve,
      fallback: { ...common.resolve.fallback, fs: false },
    },
  };
  const webConfig = {
    ...common,
    entry: { web: path.resolve(__dirname, "./src/web.ts") },
    target: "web",
  };
  return [bgConfig, webConfig];
};
"#;

/// The TweetCat shape: an arrow with a block body, local configs that spread
/// a shared local, and a returned array of configs. Webpack builds every
/// config, and the alias key names an installed package, so webpack loads the
/// shim file.
#[test]
fn webpack_function_multi_config_reads_entries_and_aliases_of_each_config() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{
            "name": "repro",
            "private": true,
            "dependencies": { "linkedom": "1.0.0" },
            "devDependencies": { "webpack": "5.0.0", "ts-loader": "9.0.0" }
        }"#,
    );
    write(
        &root.join("node_modules/linkedom/package.json"),
        r#"{ "name": "linkedom", "version": "1.0.0", "main": "index.js" }"#,
    );
    write(
        &root.join("node_modules/linkedom/index.js"),
        "export const parseHTML = () => null;\n",
    );
    write(
        &root.join("webpack.config.js"),
        WEBPACK_FUNCTION_MULTI_CONFIG,
    );
    write(
        &root.join("src/bg.ts"),
        "import { parseHTML } from \"linkedom\";\n\nparseHTML();\n",
    );
    write(&root.join("src/web.ts"), "console.log(\"web\");\n");
    write(
        &root.join("src/shims/linkedom.ts"),
        "export const parseHTML = () => null;\n",
    );
    write(&root.join("src/orphan.ts"), "export const orphan = 1;\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unused_file_keys(&results, root),
        vec!["src/orphan.ts".to_string()],
        "the entries of both configs and the alias shim are used"
    );
}

/// A Vite config that declares its value in a local binding of the
/// `defineConfig` callback and returns the binding.
#[test]
fn vite_callback_local_config_reads_aliases() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{
            "name": "repro",
            "private": true,
            "main": "src/main.ts",
            "devDependencies": { "vite": "5.0.0" }
        }"#,
    );
    write(
        &root.join("vite.config.ts"),
        r#"import path from "node:path";
import { defineConfig } from "vite";

export default defineConfig(({ mode }) => {
  const config = {
    resolve: { alias: { "@lib": path.resolve(__dirname, "src/lib") } },
  };
  return config;
});
"#,
    );
    write(
        &root.join("src/main.ts"),
        "import { util } from \"@lib/util\";\n\nutil();\n",
    );
    write(
        &root.join("src/lib/util.ts"),
        "export const util = () => 1;\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        unused_file_keys(&results, root).is_empty(),
        "the alias resolves the import: {:?}",
        unused_file_keys(&results, root)
    );
    assert!(
        results.unresolved_imports.is_empty(),
        "the alias resolves the import"
    );
}
