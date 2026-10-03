//! An orphan module declaration file is reported as an unused file.
//!
//! A `.d.ts`, `.d.mts` or `.d.cts` file is a module when it has a top-level
//! import or export and no `declare global` or `declare module` block. Such a
//! file adds nothing to the global scope, so it only matters when something
//! points to it. When no same-stem JS or TS sibling, package.json types field,
//! triple-slash reference, reachable import or covering tsconfig keeps it, the
//! file is no longer seeded as an entry point: it is reported, and its own
//! imports no longer keep other modules reachable. Ambient (script-style)
//! declaration files stay entry points.

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "orphan-module-declaration-files";

fn unused_file_paths(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    let root = fixture_path(FIXTURE);
    results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn orphan_module_declaration_file_is_reported_and_stops_seeding_its_imports() {
    let config = create_config(fixture_path(FIXTURE));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let paths = unused_file_paths(&results);

    for reported in [
        "legacy/orphan.d.ts",
        "legacy/orphan-helper.ts",
        "covered/excluded/excluded.d.ts",
    ] {
        assert!(
            paths.iter().any(|p| p == reported),
            "{reported} must be reported as an unused file, got {paths:?}"
        );
    }
}

#[test]
fn declaration_files_with_a_consumer_or_global_effect_stay_entry_points() {
    let config = create_config(fixture_path(FIXTURE));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let paths = unused_file_paths(&results);

    for kept in [
        // Script-style declaration file and the module its type import names.
        "legacy/ambient.d.ts",
        "legacy/ambient-target.ts",
        // `declare global` and `declare module` blocks keep the file and its imports.
        "legacy/augment.d.ts",
        "legacy/augment-target.ts",
        "legacy/virtual.d.ts",
        "legacy/virtual-target.ts",
        // A same-stem JS sibling.
        "legacy/sibling.d.ts",
        // package.json `types`, `exports` types condition and `typesVersions`.
        "types/public.d.ts",
        "types/exported.d.ts",
        "types/versioned/extra.d.ts",
        // A triple-slash reference.
        "types/referenced.d.ts",
        // A tsconfig `include`, and a tsconfig without `include` or `files`.
        "covered/covered.d.ts",
        "nested/inner.d.ts",
    ] {
        assert!(
            !paths.iter().any(|p| p == kept),
            "{kept} must not be reported as an unused file, got {paths:?}"
        );
    }
}

fn copy_tree(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).expect("create dest dir");
    for entry in std::fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("dir entry");
        let to = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).expect("copy file");
        }
    }
}

#[test]
fn a_tsconfig_change_misses_the_graph_cache() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path(FIXTURE), &root);
    let config = super::common::create_config_with_cache(root.clone(), temp.path().join("cache"));

    let cold = fallow_core::analyze(&config).expect("cold analysis succeeds");
    assert!(
        cold.unused_files
            .iter()
            .any(|f| f.file.path.ends_with("legacy/orphan.d.ts"))
    );

    // Only tsconfig.json changes, which is not a discovered source file. The
    // orphan is now covered, so the warm run must not serve the cached graph.
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"include": ["src", "covered", "legacy"], "exclude": ["covered/excluded"]}"#,
    )
    .expect("rewrite tsconfig");
    let warm = fallow_core::analyze(&config).expect("warm analysis succeeds");
    let warm_paths: Vec<_> = warm.unused_files.iter().map(|f| &f.file.path).collect();
    assert!(
        !warm_paths
            .iter()
            .any(|p| p.ends_with("legacy/orphan.d.ts") || p.ends_with("legacy/orphan-helper.ts")),
        "a covered declaration file and its import must not be reported, got {warm_paths:?}"
    );
}
