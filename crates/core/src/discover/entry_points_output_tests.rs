//! Tests of the output directory mapping of a package entry.

use super::*;

/// The output mapping without a discovered file set: a `lib/` file on
/// disk counts as present.
fn try_legacy_output_to_source_path(base: &Path, entry: &str) -> Option<PathBuf> {
    fallow_graph::resolve::output_entry_to_source_path(base, entry, SOURCE_EXTENSIONS, |_| true)
}

#[test]
fn maps_dist_to_src_with_ts_extension() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("utils.ts"), "export const u = 1;").unwrap();

    let result = try_legacy_output_to_source_path(dir.path(), "./dist/utils.js");
    assert!(result.is_some());
    assert!(
        result
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
            .contains("src/utils.ts")
    );
}

#[test]
fn returns_none_when_no_source_file_exists() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let result = try_legacy_output_to_source_path(dir.path(), "./dist/missing.js");
    assert!(result.is_none());
}

#[test]
fn ignores_non_output_directories() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("foo.ts"), "export const f = 1;").unwrap();

    let result = try_legacy_output_to_source_path(dir.path(), "./scripts/foo.js");
    assert!(result.is_none());
}

#[test]
fn maps_lib_entry_to_source_only_when_lib_target_is_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("foo.ts"), "export const f = 1;").unwrap();

    assert_eq!(
        try_legacy_output_to_source_path(dir.path(), "./lib/foo.js"),
        Some(src.join("foo.ts")),
        "a missing lib/ build output maps to the same-stem source file"
    );

    let lib = dir.path().join("lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("foo.js"), "exports.f = 1;").unwrap();
    assert_eq!(
        try_legacy_output_to_source_path(dir.path(), "./lib/foo.js"),
        None,
        "a lib/ file on disk stays the entry"
    );
}

#[test]
fn lib_entry_without_extension_counts_as_present() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/index.ts"), "export {};").unwrap();
    std::fs::create_dir_all(dir.path().join("lib")).unwrap();
    std::fs::write(dir.path().join("lib/index.js"), "module.exports = {};").unwrap();

    assert_eq!(
        try_legacy_output_to_source_path(dir.path(), "./lib/index"),
        None
    );
    assert_eq!(try_legacy_output_to_source_path(dir.path(), "./lib"), None);
}

#[test]
fn maps_missing_lib_entry_with_nested_prefix() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let modules_src = dir.path().join("modules/src");
    std::fs::create_dir_all(&modules_src).unwrap();
    std::fs::write(modules_src.join("x.ts"), "export const x = 1;").unwrap();

    assert_eq!(
        try_legacy_output_to_source_path(dir.path(), "./modules/lib/x.mjs"),
        Some(modules_src.join("x.ts"))
    );
}

#[test]
fn bare_missing_lib_entry_does_not_map_to_source_directory() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::create_dir_all(dir.path().join("src")).unwrap();

    assert_eq!(try_legacy_output_to_source_path(dir.path(), "./lib"), None);
}

#[test]
fn lib_is_not_an_index_fallback_output_dir() {
    assert!(!is_entry_in_output_dir("./lib/foo.js"));
}

#[test]
fn maps_nested_output_path_preserving_prefix() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let modules_src = dir.path().join("modules").join("src");
    std::fs::create_dir_all(&modules_src).unwrap();
    std::fs::write(modules_src.join("helper.ts"), "export const h = 1;").unwrap();

    let result = try_legacy_output_to_source_path(dir.path(), "./modules/dist/helper.js");
    assert!(result.is_some());
    assert!(
        result
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
            .contains("modules/src/helper.ts")
    );
}
