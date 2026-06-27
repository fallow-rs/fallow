//! Correctness gate for the persisted graph cache.
//!
//! The persisted graph cache (`crate::cache`, gated on `no_cache == false`)
//! loads a previously-built `ModuleGraph` from `.fallow/graph-cache.bin` and
//! skips the graph build when the file set + fingerprints + graph-affecting
//! options are byte-identical. The non-negotiable invariant is TRANSPARENCY: a
//! cache hit must produce identical analysis results to a cold build. These
//! tests run each fixture cold (clean cache, persists) then warm (loads) and
//! assert the full `AnalysisResults` is identical, plus that a source change
//! correctly misses the cache rather than being stale-served.

use std::path::Path;

use fallow_core::graph_cache::{GraphCacheManifest, GraphCacheMode};
use fallow_types::source_fingerprint::SourceFingerprint;

use super::common::{create_config_with_cache, fixture_path};

/// Recursively copy a fixture tree into `dst` so the graph cache writes into a
/// scratch directory and source mutation does not touch the checked-in fixture.
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create dest dir");
    for entry in std::fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("dir entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).expect("copy file");
        }
    }
}

/// Run the same fixture cold (clean cache) then warm (cache present) and assert
/// the full results are byte-identical. The cold run persists `graph-cache.bin`;
/// the warm run loads it and skips the graph build.
fn assert_cold_warm_identical(fixture: &str) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path(fixture), &root);

    // Cache dir lives OUTSIDE the project root so it is not itself an analyzed
    // source tree; the graph cache writes `graph-cache.bin` here.
    let cache_dir = temp.path().join("cache");

    let config = create_config_with_cache(root, cache_dir.clone());

    // Cold: no cache exists yet, graph is built fresh and persisted.
    let cold = fallow_core::analyze(&config).expect("cold analysis succeeds");
    assert!(
        cache_dir.join("graph-cache.bin").exists(),
        "{fixture}: cold run must persist graph-cache.bin"
    );

    // Warm: graph-cache.bin exists; the graph build is skipped and the cached
    // graph is loaded (with namespace_imported reconstructed).
    let warm = fallow_core::analyze(&config).expect("warm analysis succeeds");

    // Full structural equality: serialize both and compare every issue vec.
    let cold_json = serde_json::to_value(&cold).expect("serialize cold results");
    let warm_json = serde_json::to_value(&warm).expect("serialize warm results");
    assert_eq!(
        cold_json, warm_json,
        "{fixture}: warm (cache hit) results must be byte-identical to cold results"
    );
    assert_eq!(
        cold.total_issues(),
        warm.total_issues(),
        "{fixture}: total issue count must match cold vs warm"
    );
}

#[test]
fn namespace_imports_cold_vs_warm_identical() {
    // Exercises `import * as ns` so the `namespace_imported` reconstruction on
    // cache load is on the path.
    assert_cold_warm_identical("namespace-imports");
}

#[test]
fn barrel_exports_cold_vs_warm_identical() {
    // Exercises re-export chains + reachability + unused exports.
    assert_cold_warm_identical("barrel-exports");
}

#[test]
fn cross_package_members_cold_vs_warm_identical() {
    // Exercises cross-package member crediting (ExportSymbol.members round-trip).
    assert_cold_warm_identical("cross-package-enum-class-members");
}

#[test]
fn basic_project_cold_vs_warm_identical() {
    assert_cold_warm_identical("basic-project");
}

/// A source change must MISS the cache (the manifest no longer matches) rather
/// than being stale-served, and the warm-after-change result must reflect the
/// change. Adds a new unused export to a fixture file and asserts the cached
/// run picks it up.
#[test]
fn source_change_misses_cache_and_reflects_change() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path("barrel-exports"), &root);
    let cache_dir = temp.path().join("cache");

    let config = create_config_with_cache(root.clone(), cache_dir.clone());

    // Cold run: build + persist.
    let before = fallow_core::analyze(&config).expect("cold analysis");
    let unused_before = before.unused_exports.len();

    // Mutate a source file: add a brand-new export that nothing imports. This
    // changes the file's size (and mtime), so its SourceFingerprint changes and
    // the persisted manifest no longer matches the current inputs.
    let target = root.join("src/module-a.ts");
    let original = std::fs::read_to_string(&target).expect("read module-a");
    // Sleep a touch so the mtime is guaranteed to differ on coarse-resolution
    // filesystems even though the size change alone already misses.
    std::thread::sleep(std::time::Duration::from_millis(10));
    std::fs::write(
        &target,
        format!("{original}\nexport const brandNewDeadExport = 42;\n"),
    )
    .expect("write mutated module-a");

    // Re-discover the now-mutated file set and confirm the persisted manifest
    // no longer matches the current inputs (the cache will MISS, not stale-serve).
    let files = fallow_core::discover::discover_files(&config);
    let current = GraphCacheManifest::from_discovered_files(
        &config.root,
        &files,
        GraphCacheMode::new(0, 0, 0),
        |path| {
            std::fs::metadata(path).map_or(SourceFingerprint::new(0, 0), |m| {
                SourceFingerprint::from_metadata(&m)
            })
        },
    );
    let store = fallow_core::graph_cache::GraphCacheStore::load(&cache_dir)
        .expect("persisted graph cache exists after cold run");
    assert!(
        !store.manifest.matches_inputs(&current),
        "a mutated source file must invalidate the persisted graph-cache manifest"
    );

    // The next analyze run must rebuild and reflect the new dead export.
    let after = fallow_core::analyze(&config).expect("analysis after mutation");
    assert_eq!(
        after.unused_exports.len(),
        unused_before + 1,
        "the new dead export must surface (cache must not stale-serve the old graph)"
    );
}

/// A deleted source file must MISS the cache and disappear from the next
/// analysis result, rather than being served from the old persisted graph.
#[test]
fn file_deletion_misses_cache_and_reflects_change() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path("basic-project"), &root);
    let cache_dir = temp.path().join("cache");

    let config = create_config_with_cache(root.clone(), cache_dir.clone());

    // Cold run: build + persist.
    let before = fallow_core::analyze(&config).expect("cold analysis");
    assert!(
        before
            .unused_files
            .iter()
            .any(|issue| issue.file.path.ends_with("src/orphan.ts")),
        "fixture should expose the file that will be deleted"
    );

    let target = root.join("src/orphan.ts");
    std::fs::remove_file(&target).expect("delete unused fixture file");

    let files = fallow_core::discover::discover_files(&config);
    let current = GraphCacheManifest::from_discovered_files(
        &config.root,
        &files,
        GraphCacheMode::new(0, 0, 0),
        |path| {
            std::fs::metadata(path).map_or(SourceFingerprint::new(0, 0), |m| {
                SourceFingerprint::from_metadata(&m)
            })
        },
    );
    let store = fallow_core::graph_cache::GraphCacheStore::load(&cache_dir)
        .expect("persisted graph cache exists after cold run");
    assert!(
        !store.manifest.matches_inputs(&current),
        "a deleted source file must invalidate the persisted graph-cache manifest"
    );

    let after = fallow_core::analyze(&config).expect("analysis after deletion");
    assert!(
        after
            .unused_files
            .iter()
            .all(|issue| !issue.file.path.ends_with("src/orphan.ts")),
        "deleted source file must not survive through a graph-cache hit"
    );
}

/// Resolve a real-world benchmark fixture path. These are gitignored symlinks
/// that may be absent on a fresh checkout, so callers skip when missing.
fn benchmark_fixture_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("benchmarks")
        .join("fixtures")
        .join("real-world")
        .join(name)
}

/// Run a real-world benchmark fixture cold then warm and assert `total_issues`
/// is identical. Skips (does not fail) when the fixture is absent locally
/// (benchmark fixtures are gitignored symlinks). Runs in place against the
/// fixture root, writing the graph cache into an out-of-tree scratch dir so the
/// fixture is never mutated.
fn assert_benchmark_cold_warm_total(name: &str) {
    let fixture = benchmark_fixture_path(name);
    if !fixture.exists() {
        // Benchmark fixtures are gitignored symlinks that may be absent on a
        // fresh checkout: treat as a skip (silent pass) rather than a failure.
        return;
    }

    let cache = tempfile::tempdir().expect("create temp cache dir");
    let config = create_config_with_cache(fixture, cache.path().to_path_buf());

    let cold = fallow_core::analyze(&config).expect("cold benchmark analysis");
    assert!(
        cache.path().join("graph-cache.bin").exists(),
        "{name}: cold run must persist graph-cache.bin"
    );
    let warm = fallow_core::analyze(&config).expect("warm benchmark analysis");

    assert_eq!(
        cold.total_issues(),
        warm.total_issues(),
        "{name}: total_issues must be identical cold vs warm"
    );
}

#[test]
fn benchmark_preact_cold_vs_warm_total_identical() {
    assert_benchmark_cold_warm_total("preact");
}

#[test]
fn benchmark_zod_cold_vs_warm_total_identical() {
    assert_benchmark_cold_warm_total("zod");
}

/// The manifest must hit on identical inputs and miss when a fingerprint or a
/// graph-affecting mode hash changes. This pins `matches_inputs` against the
/// real `from_discovered_files` shape used by the integration path.
#[test]
fn manifest_matches_only_on_identical_inputs() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path("namespace-imports"), &root);

    let config = create_config_with_cache(root, temp.path().join("cache"));
    let files = fallow_core::discover::discover_files(&config);

    let fingerprint_provider = |path: &Path| {
        std::fs::metadata(path).map_or(SourceFingerprint::new(0, 0), |m| {
            SourceFingerprint::from_metadata(&m)
        })
    };

    let manifest_a = GraphCacheManifest::from_discovered_files(
        &config.root,
        &files,
        GraphCacheMode::new(1, 2, 3),
        fingerprint_provider,
    );
    let manifest_same = GraphCacheManifest::from_discovered_files(
        &config.root,
        &files,
        GraphCacheMode::new(1, 2, 3),
        fingerprint_provider,
    );
    let manifest_other_mode = GraphCacheManifest::from_discovered_files(
        &config.root,
        &files,
        GraphCacheMode::new(1, 99, 3),
        fingerprint_provider,
    );

    assert!(manifest_a.matches_inputs(&manifest_same));
    assert!(!manifest_a.matches_inputs(&manifest_other_mode));
}
