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

use fallow_config::{FallowConfig, OutputFormat};
use fallow_core::graph_cache::{GraphCacheManifest, GraphCacheMode};
use fallow_types::discover::FileId;
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

fn create_custom_config_with_cache(
    root: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    customize: impl FnOnce(&mut FallowConfig),
) -> fallow_config::ResolvedConfig {
    let mut raw = FallowConfig::default();
    customize(&mut raw);
    let mut config = raw.resolve(root, OutputFormat::Human, 4, false, true, None);
    config.cache_dir = cache_dir;
    config
}

fn current_manifest_with_cached_mode(
    config: &fallow_config::ResolvedConfig,
    store: &fallow_core::graph_cache::GraphCacheStore,
) -> GraphCacheManifest {
    let files = fallow_core::discover::discover_files(config);
    GraphCacheManifest::from_discovered_files(&config.root, &files, store.manifest.mode, |path| {
        std::fs::metadata(path).map_or(SourceFingerprint::new(0, 0), |m| {
            SourceFingerprint::from_metadata(&m)
        })
    })
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
    // changes the file's size, so its SourceFingerprint changes and the
    // persisted manifest no longer matches the current inputs.
    let target = root.join("src/module-a.ts");
    let original = std::fs::read_to_string(&target).expect("read module-a");
    std::fs::write(
        &target,
        format!("{original}\nexport const brandNewDeadExport = 42;\n"),
    )
    .expect("write mutated module-a");

    // Re-discover the now-mutated file set and confirm the persisted manifest
    // no longer matches the current inputs (the cache will MISS, not stale-serve).
    let store = fallow_core::graph_cache::GraphCacheStore::load(&cache_dir)
        .expect("persisted graph cache exists after cold run");
    let current = current_manifest_with_cached_mode(&config, &store);
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

    let store = fallow_core::graph_cache::GraphCacheStore::load(&cache_dir)
        .expect("persisted graph cache exists after cold run");
    let current = current_manifest_with_cached_mode(&config, &store);
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

/// A rename must MISS the cache and the next result must use the new path,
/// even when the file contents are byte-identical.
#[test]
fn file_rename_misses_cache_and_reflects_new_path() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path("basic-project"), &root);
    let cache_dir = temp.path().join("cache");

    let config = create_config_with_cache(root.clone(), cache_dir.clone());

    let before = fallow_core::analyze(&config).expect("cold analysis");
    assert!(
        before
            .unused_files
            .iter()
            .any(|issue| issue.file.path.ends_with("src/orphan.ts")),
        "fixture should expose the file that will be renamed"
    );

    std::fs::rename(
        root.join("src/orphan.ts"),
        root.join("src/renamed-orphan.ts"),
    )
    .expect("rename unused fixture file");

    let store = fallow_core::graph_cache::GraphCacheStore::load(&cache_dir)
        .expect("persisted graph cache exists after cold run");
    let current = current_manifest_with_cached_mode(&config, &store);
    assert!(
        !store.manifest.matches_inputs(&current),
        "a renamed source file must invalidate the persisted graph-cache manifest"
    );

    let after = fallow_core::analyze(&config).expect("analysis after rename");
    assert!(
        after
            .unused_files
            .iter()
            .all(|issue| !issue.file.path.ends_with("src/orphan.ts")),
        "old source path must not survive through a graph-cache hit"
    );
    assert!(
        after
            .unused_files
            .iter()
            .any(|issue| issue.file.path.ends_with("src/renamed-orphan.ts")),
        "renamed source path must be analyzed after cache invalidation"
    );
}

/// Production mode intentionally stays out of `resolver_options_hash`; it must
/// invalidate through the discovered file set. A stale non-production graph
/// would keep test-only imports alive and hide `testHelper`.
#[test]
#[expect(
    deprecated,
    reason = "trace timings are still the internal contract for this cache invalidation gate"
)]
fn production_mode_change_misses_cache_and_reflects_file_set() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path("production-mode"), &root);
    let cache_dir = temp.path().join("cache");

    let non_production = create_config_with_cache(root.clone(), cache_dir.clone());
    fallow_core::analyze(&non_production).expect("cold non-production analysis");

    let production = create_custom_config_with_cache(root, cache_dir.clone(), |config| {
        config.production = true.into();
    });
    let store = fallow_core::graph_cache::GraphCacheStore::load(&cache_dir)
        .expect("persisted graph cache exists after cold run");
    let current = current_manifest_with_cached_mode(&production, &store);
    assert!(
        !store.manifest.matches_inputs(&current),
        "production mode must invalidate via the discovered file set"
    );

    let after = fallow_core::analyze_with_trace(&production).expect("production analysis");
    let timings = after.timings.expect("trace timings retained");
    assert!(
        timings.resolve_imports_ms > f64::EPSILON,
        "production mode change must miss the graph cache, got {}ms",
        timings.resolve_imports_ms
    );
    let unused_export_names: Vec<&str> = after
        .results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"testHelper"),
        "production analysis must reflect excluded test files, got {unused_export_names:?}"
    );
}

/// Ignore patterns intentionally stay out of `resolver_options_hash`; they must
/// invalidate through the discovered file set. A stale graph would keep the
/// ignored orphan file in the result.
#[test]
#[expect(
    deprecated,
    reason = "trace timings are still the internal contract for this cache invalidation gate"
)]
fn ignore_pattern_change_misses_cache_and_reflects_file_set() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path("basic-project"), &root);
    let cache_dir = temp.path().join("cache");

    let without_ignore = create_config_with_cache(root.clone(), cache_dir.clone());
    let before = fallow_core::analyze(&without_ignore).expect("cold analysis");
    assert!(
        before
            .unused_files
            .iter()
            .any(|issue| issue.file.path.ends_with("src/orphan.ts")),
        "fixture should expose the ignored file before ignorePatterns change"
    );

    let with_ignore = create_custom_config_with_cache(root, cache_dir.clone(), |config| {
        config.ignore_patterns = vec!["src/orphan.ts".to_string()];
    });
    let store = fallow_core::graph_cache::GraphCacheStore::load(&cache_dir)
        .expect("persisted graph cache exists after cold run");
    let current = current_manifest_with_cached_mode(&with_ignore, &store);
    assert!(
        !store.manifest.matches_inputs(&current),
        "ignorePatterns change must invalidate via the discovered file set"
    );

    let after = fallow_core::analyze_with_trace(&with_ignore).expect("ignored analysis");
    let timings = after.timings.expect("trace timings retained");
    assert!(
        timings.resolve_imports_ms > f64::EPSILON,
        "ignorePatterns change must miss the graph cache, got {}ms",
        timings.resolve_imports_ms
    );
    assert!(
        after
            .results
            .unused_files
            .iter()
            .all(|issue| !issue.file.path.ends_with("src/orphan.ts")),
        "ignored source file must not survive through a graph-cache hit"
    );
}

/// Resolve conditions are graph-affecting resolver options, so they must
/// invalidate through `GraphCacheMode` even when the discovered file set is
/// unchanged.
#[test]
#[expect(
    deprecated,
    reason = "trace timings are still the internal contract for this cache invalidation gate"
)]
fn resolve_condition_change_misses_cache_and_matches_cold_output() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path("barrel-exports"), &root);
    let cache_dir = temp.path().join("cache");

    let base = create_config_with_cache(root.clone(), cache_dir.clone());
    fallow_core::analyze(&base).expect("cold base analysis");

    let mut changed = create_config_with_cache(root.clone(), cache_dir);
    changed.resolve.conditions.push("react-server".to_string());

    let after = fallow_core::analyze_with_trace(&changed).expect("changed condition analysis");
    let timings = after.timings.expect("trace timings retained");
    assert!(
        timings.resolve_imports_ms > f64::EPSILON,
        "resolve condition changes must miss the graph cache, got {}ms",
        timings.resolve_imports_ms
    );

    let mut cold_changed = create_config_with_cache(root, temp.path().join("cold-cache"));
    cold_changed
        .resolve
        .conditions
        .push("react-server".to_string());
    cold_changed.no_cache = true;
    let cold = fallow_core::analyze(&cold_changed).expect("cold changed condition analysis");
    let cold_json = serde_json::to_value(&cold).expect("serialize cold changed results");
    let after_json =
        serde_json::to_value(&after.results).expect("serialize cache-miss changed results");
    assert_eq!(
        cold_json, after_json,
        "a resolve-condition cache miss must match a cold analysis with the same config"
    );
}

#[test]
#[expect(
    deprecated,
    reason = "trace timings are still the internal contract for this cache performance gate"
)]
fn warm_graph_cache_hit_skips_import_resolution() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path("barrel-exports"), &root);
    let cache_dir = temp.path().join("cache");

    let config = create_config_with_cache(root, cache_dir);

    let cold = fallow_core::analyze_with_trace(&config).expect("cold analysis");
    let warm = fallow_core::analyze_with_trace(&config).expect("warm analysis");

    let cold_json = serde_json::to_value(&cold.results).expect("serialize cold results");
    let warm_json = serde_json::to_value(&warm.results).expect("serialize warm results");
    assert_eq!(
        cold_json, warm_json,
        "warm graph-cache hit must preserve analysis output"
    );

    let warm_timings = warm.timings.expect("trace timings retained");
    assert!(
        warm_timings.resolve_imports_ms.abs() <= f64::EPSILON,
        "warm graph-cache hit must skip import resolution, got {}ms",
        warm_timings.resolve_imports_ms
    );
}

#[test]
#[expect(
    deprecated,
    reason = "trace timings are still the internal contract for this cache performance gate"
)]
fn resolver_cache_hit_rebuilds_graph_when_file_ids_shift() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_tree(&fixture_path("barrel-exports"), &root);
    let cache_dir = temp.path().join("cache");

    let config = create_config_with_cache(root, cache_dir.clone());

    let cold = fallow_core::analyze_with_trace(&config).expect("cold analysis");
    let mut store = fallow_core::graph_cache::GraphCacheStore::load(&cache_dir)
        .expect("persisted graph cache exists after cold run");
    let current = current_manifest_with_cached_mode(&config, &store);

    for file in &mut store.manifest.files {
        file.file_id = FileId(file.file_id.0 + 10_000);
    }
    store.graph.modules.clear();
    store.graph.package_usage.clear();
    store.graph.type_only_package_usage.clear();
    store.graph.entry_points.clear();
    store.graph.runtime_entry_points.clear();
    store.graph.test_entry_points.clear();
    store.graph.reverse_deps.clear();

    assert!(
        !store.manifest.matches_inputs(&current),
        "shifted FileIds must not trust the persisted graph"
    );
    assert!(
        store.manifest.matches_resolution_inputs(&current),
        "stable file keys and fingerprints should still allow resolver reuse"
    );
    store.save(&cache_dir);

    let warm = fallow_core::analyze_with_trace(&config).expect("resolver-cache analysis");
    let cold_json = serde_json::to_value(&cold.results).expect("serialize cold results");
    let warm_json = serde_json::to_value(&warm.results).expect("serialize warm results");
    assert_eq!(
        cold_json, warm_json,
        "resolver-cache hit must rebuild the graph and preserve analysis output"
    );

    let warm_timings = warm.timings.expect("trace timings retained");
    assert!(
        warm_timings.resolve_imports_ms.abs() <= f64::EPSILON,
        "resolver-cache hit must skip import resolution, got {}ms",
        warm_timings.resolve_imports_ms
    );
    assert!(
        warm_timings.build_graph_ms > f64::EPSILON,
        "resolver-cache hit must rebuild the graph, got {}ms",
        warm_timings.build_graph_ms
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
