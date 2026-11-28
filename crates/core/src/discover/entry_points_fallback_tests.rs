//! Tests of the source index fallback of a package entry.

use super::*;

#[test]
fn detects_dist_entry_in_output_dir() {
    assert!(is_entry_in_output_dir("./dist/esm2022/index.js"));
    assert!(is_entry_in_output_dir("dist/index.js"));
    assert!(is_entry_in_output_dir("./build/index.js"));
    assert!(is_entry_in_output_dir("./out/main.js"));
    assert!(is_entry_in_output_dir("./esm/index.js"));
    assert!(is_entry_in_output_dir("./cjs/index.js"));
}

#[test]
fn rejects_non_output_entry_paths() {
    assert!(!is_entry_in_output_dir("./src/index.ts"));
    assert!(!is_entry_in_output_dir("src/main.ts"));
    assert!(!is_entry_in_output_dir("./index.js"));
    assert!(!is_entry_in_output_dir(""));
}

#[test]
fn root_index_entries_are_recognized_for_source_fallback() {
    assert!(is_package_root_index_entry("./index.js"));
    assert!(is_package_root_index_entry("index.cjs"));
    assert!(is_package_root_index_entry("./index.d.ts"));
    assert!(!is_package_root_index_entry("./src/index.js"));
    assert!(!is_package_root_index_entry("./main.js"));
    assert!(!is_package_root_index_entry(""));
}

#[test]
fn rejects_substring_match_for_output_dir() {
    assert!(!is_entry_in_output_dir("./distro/index.js"));
    assert!(!is_entry_in_output_dir("./build-scripts/run.js"));
}

#[test]
fn finds_src_index_ts() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let index_path = src.join("index.ts");
    std::fs::write(&index_path, "export const a = 1;").unwrap();

    let result = try_source_index_fallback(dir.path(), dir.path());
    assert_eq!(result.as_deref(), Some(index_path.as_path()));
}

#[test]
fn finds_src_index_tsx_when_ts_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let index_path = src.join("index.tsx");
    std::fs::write(&index_path, "export default 1;").unwrap();

    let result = try_source_index_fallback(dir.path(), dir.path());
    assert_eq!(result.as_deref(), Some(index_path.as_path()));
}

#[test]
fn prefers_src_index_over_root_index() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let src_index = src.join("index.ts");
    std::fs::write(&src_index, "export const a = 1;").unwrap();
    let root_index = dir.path().join("index.ts");
    std::fs::write(&root_index, "export const b = 2;").unwrap();

    let result = try_source_index_fallback(dir.path(), dir.path());
    assert_eq!(result.as_deref(), Some(src_index.as_path()));
}

#[test]
fn falls_back_to_src_main() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let main_path = src.join("main.ts");
    std::fs::write(&main_path, "export const a = 1;").unwrap();

    let result = try_source_index_fallback(dir.path(), dir.path());
    assert_eq!(result.as_deref(), Some(main_path.as_path()));
}

#[test]
fn falls_back_to_root_index_when_no_src() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let index_path = dir.path().join("index.js");
    std::fs::write(&index_path, "module.exports = {};").unwrap();

    let result = try_source_index_fallback(dir.path(), dir.path());
    assert_eq!(result.as_deref(), Some(index_path.as_path()));
}

#[test]
fn returns_none_when_nothing_matches() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let result = try_source_index_fallback(dir.path(), dir.path());
    assert!(result.is_none());
}

#[test]
fn resolve_entry_path_falls_back_to_src_index_for_dist_entry() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let canonical = dunce::canonicalize(dir.path()).unwrap();

    let dist_dir = canonical.join("dist").join("esm2022");
    std::fs::create_dir_all(&dist_dir).unwrap();
    std::fs::write(dist_dir.join("index.js"), "export const x = 1;").unwrap();

    let src = canonical.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let src_index = src.join("index.ts");
    std::fs::write(&src_index, "export const x = 1;").unwrap();

    let result = resolve_entry_path(
        &canonical,
        "./dist/esm2022/index.js",
        &canonical,
        EntryPointSource::PackageJsonMain,
    );
    assert!(result.is_some());
    let entry = result.unwrap();
    assert_eq!(entry.path, src_index);
}

#[test]
fn resolve_entry_path_uses_direct_src_mirror_when_available() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let canonical = dunce::canonicalize(dir.path()).unwrap();

    let src_mirror = canonical.join("src").join("esm2022");
    std::fs::create_dir_all(&src_mirror).unwrap();
    let mirror_index = src_mirror.join("index.ts");
    std::fs::write(&mirror_index, "export const x = 1;").unwrap();

    let src_index = canonical.join("src").join("index.ts");
    std::fs::write(&src_index, "export const y = 2;").unwrap();

    let result = resolve_entry_path(
        &canonical,
        "./dist/esm2022/index.js",
        &canonical,
        EntryPointSource::PackageJsonMain,
    );
    assert_eq!(result.map(|e| e.path), Some(mirror_index));
}

/// A tsconfig with `rootDir: "."` claims `dist/` but names no file. A
/// Nest build emits `dist/<path>.js` from `src/<path>.ts`.
#[test]
fn configured_output_without_source_maps_to_an_analyzed_src_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let canonical = dunce::canonicalize(dir.path()).unwrap();
    std::fs::write(
        canonical.join("tsconfig.build.json"),
        r#"{"compilerOptions":{"rootDir":".","outDir":"./dist"}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(canonical.join("src/scripts")).unwrap();
    let script = canonical.join("src/scripts/setup.ts");
    std::fs::write(&script, "export {};").unwrap();
    let entry = "dist/scripts/setup.js";

    let analyzed: &(dyn Fn(&Path) -> bool + Sync) = &|_| true;
    let result = resolve_entry_path_with_discovered(
        &canonical,
        entry,
        &canonical,
        EntryPointSource::package_json_script("build"),
        analyzed,
    );
    assert_eq!(result.map(|e| e.path), Some(script));

    let not_analyzed: &(dyn Fn(&Path) -> bool + Sync) = &|_| false;
    let result = resolve_entry_path_with_discovered(
        &canonical,
        entry,
        &canonical,
        EntryPointSource::package_json_script("build"),
        not_analyzed,
    );
    assert_eq!(
        result.map(|e| e.path),
        None,
        "a source file outside the analyzed files must not become an entry"
    );
}

#[test]
fn resolve_entry_path_falls_back_to_src_index_for_missing_root_index() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let canonical = dunce::canonicalize(dir.path()).unwrap();

    let src = canonical.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let src_index = src.join("index.ts");
    std::fs::write(&src_index, "export const x = 1;").unwrap();

    let result = resolve_entry_path(
        &canonical,
        "./index.js",
        &canonical,
        EntryPointSource::PackageJsonMain,
    );
    assert_eq!(result.map(|entry| entry.path), Some(src_index));
}
