//! Webpack `resolve.alias` replacement files that the resolver never reaches.
//!
//! Fallow applies a plugin alias only when the normal resolution of an import
//! fails. A relative key and a bare key for an installed package resolve
//! without the alias, so the replacement file has no consumer in the graph.

use super::common::create_config;

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
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

fn write_project(root: &std::path::Path, alias: &str) {
    write(
        &root.join("package.json"),
        r#"{
            "name": "repro",
            "private": true,
            "main": "src/index.js",
            "dependencies": { "pkg": "1.0.0" },
            "devDependencies": { "webpack": "5.0.0" }
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
        &root.join("webpack.config.js"),
        &format!(
            r#"const path = require("path");
module.exports = {{
  entry: "./src/index.js",
  resolve: {{ alias: {{ {alias} }} }},
}};
"#
        ),
    );
}

/// A relative key resolves to the original file in fallow. Webpack joins a
/// relative import with the importer directory relative to the package root,
/// so `./clock.js` in `src/index.js` matches the key `./src/clock.js`, and
/// webpack loads the replacement file instead.
#[test]
fn relative_key_replacement_exports_are_used_with_include_entry_exports() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_project(
        root,
        r#""./src/clock.js": path.resolve(__dirname, "mocks/clock.js")"#,
    );
    write(
        &root.join("src/clock.js"),
        "export const now = () => 1;\nexport const unusedReal = 2;\n",
    );
    write(
        &root.join("mocks/clock.js"),
        "export const now = () => 0;\nexport const unusedReal = 0;\n",
    );
    write(
        &root.join("src/index.js"),
        "import { now } from \"./clock.js\";\n\nexport const run = () => now();\n",
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        unused_file_keys(&results, root).is_empty(),
        "webpack loads the replacement file: {:?}",
        unused_file_keys(&results, root)
    );
    assert_eq!(
        unused_export_keys(&results, root),
        vec![
            "src/clock.js:unusedReal".to_string(),
            "src/index.js:run".to_string(),
        ],
        "only the unused export of the original file and the entry stay reportable"
    );
}

/// A bare key names an installed package. The import resolves to the real
/// package, so the exports of the replacement file must stay credited.
#[test]
fn installed_package_key_replacement_exports_are_used_with_include_entry_exports() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_project(root, r#"pkg: path.resolve(__dirname, "mocks/pkg.js")"#);
    write(
        &root.join("mocks/pkg.js"),
        "export const query = () => 'mock';\nexport const tool = () => 'mock';\n",
    );
    write(
        &root.join("src/index.js"),
        "import { query } from \"pkg\";\n\nexport const run = () => query();\n",
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        unused_file_keys(&results, root).is_empty(),
        "webpack loads the replacement file: {:?}",
        unused_file_keys(&results, root)
    );
    assert_eq!(
        unused_export_keys(&results, root),
        vec!["src/index.js:run".to_string()],
        "the replacement file stands in for the installed package"
    );
}

/// A bare key for a package that is not installed routes the import to the
/// replacement file, so an unused export there stays reportable.
#[test]
fn missing_package_key_keeps_precise_export_tracking() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_project(
        root,
        r#"absent: path.resolve(__dirname, "mocks/absent.js")"#,
    );
    write(
        &root.join("mocks/absent.js"),
        "export const used = 1;\nexport const unusedMock = 2;\n",
    );
    write(
        &root.join("src/index.js"),
        "import { used } from \"absent\";\n\nexport const run = () => used;\n",
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unused_export_keys(&results, root),
        vec![
            "mocks/absent.js:unusedMock".to_string(),
            "src/index.js:run".to_string(),
        ],
        "the resolver routes the import to the replacement file"
    );
}

/// A path-alias key, such as `@lib`, routes the import to the replacement
/// file, so an unused export there stays reportable.
#[test]
fn path_alias_key_keeps_precise_export_tracking() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_project(
        root,
        r#""@lib": path.resolve(__dirname, "src/lib/index.js")"#,
    );
    write(
        &root.join("src/lib/index.js"),
        "export const used = 1;\nexport const unusedLib = 2;\n",
    );
    write(
        &root.join("src/index.js"),
        "import { used } from \"@lib\";\n\nexport const run = () => used;\n",
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unused_export_keys(&results, root),
        vec![
            "src/index.js:run".to_string(),
            "src/lib/index.js:unusedLib".to_string(),
        ],
        "the resolver routes the import to the replacement file"
    );
}

/// A trailing `$` marks an exact-match key. `pkg$` still names the installed
/// package, so webpack loads the replacement file.
#[test]
fn exact_match_installed_package_key_seeds_the_replacement() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write_project(root, r#"pkg$: path.resolve(__dirname, "mocks/pkg.js")"#);
    write(
        &root.join("mocks/pkg.js"),
        "export const query = () => 'mock';\nexport const tool = () => 'mock';\n",
    );
    write(
        &root.join("src/index.js"),
        "import { query } from \"pkg\";\n\nexport const run = () => query();\n",
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        unused_file_keys(&results, root).is_empty(),
        "webpack loads the replacement file: {:?}",
        unused_file_keys(&results, root)
    );
    assert_eq!(
        unused_export_keys(&results, root),
        vec!["src/index.js:run".to_string()],
        "the replacement file stands in for the installed package"
    );
}
