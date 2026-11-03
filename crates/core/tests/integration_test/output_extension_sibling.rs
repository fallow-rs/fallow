//! A TypeScript file can load the build output of a sibling source file by its
//! output name. A bundler builds `src/loader.ts` and `src/app.ts` into one
//! output folder as `loader.cjs` and `app.cjs`, so `require("./app.cjs")` in
//! the loader is correct at run time. The import maps to `src/app.ts`.

use std::fs;
use std::path::Path;

use super::common::create_config;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    fs::write(path, source).expect("write fixture file");
}

fn project() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{"name": "output-extension-sibling", "main": "src/loader.ts"}"#,
    );
    write(
        root,
        "src/loader.ts",
        "require(\"./cache.cjs\");\nrequire(\"./app.mjs\");\nrequire(\"./missing.cjs\");\n",
    );
    write(root, "src/cache.ts", "export const cache = 1;\n");
    write(root, "src/app.tsx", "export const app = 1;\n");
    tmp
}

#[test]
fn output_extension_import_maps_to_the_sibling_typescript_source() {
    let tmp = project();
    let root = tmp.path();
    let results = fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert_eq!(
        unresolved,
        ["./missing.cjs"],
        "only the import without a sibling source stays unresolved"
    );

    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(root)
                .expect("project-relative path")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(
        unused.is_empty(),
        "sibling sources are reachable: {unused:?}"
    );
}
