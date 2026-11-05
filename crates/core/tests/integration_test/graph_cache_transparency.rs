//! Correctness gate for the persisted graph cache.
//!
//! The persisted graph cache (`crate::cache`, gated on `no_cache == false`)
//! loads a previously-built `ModuleGraph` from `.fallow/graph-cache.bin` and
//! skips the graph build when the file set + fingerprints + graph-affecting
//! options are byte-identical. The non-negotiable invariant is TRANSPARENCY: a
//! cache hit must produce identical analysis results to a cold build. These
//! tests run each fixture cold (clean cache, persists) then warm (loads) and
//! assert the full `AnalysisResults` is identical.
//!
//! The edit cases (source edits, renames, options, manifests, install state)
//! live in the cache model tests of `fallow-engine`
//! (`crates/engine/tests/integration/cache_model`). Those tests compare each
//! warm run with a cache-free run over generated edit sequences, and check
//! that the graph cache key covers each path that import resolution reads.

use fallow_core::graph_cache::{GraphCacheManifest, GraphCacheMode};
use fallow_types::discover::FileId;

use super::common::{copy_fixture_tree, create_config_with_cache, fixture_path};

/// Run the same fixture cold (clean cache) then warm (cache present) and assert
/// the full results are byte-identical. The cold run persists `graph-cache.bin`;
/// the warm run loads it and skips the graph build.
fn assert_cold_warm_identical(fixture: &str) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_fixture_tree(&fixture_path(fixture), &root);

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

fn current_manifest_with_cached_mode(
    config: &fallow_config::ResolvedConfig,
    store: &fallow_core::graph_cache::GraphCacheStore,
) -> GraphCacheManifest {
    let files = fallow_core::discover::discover_files(config);
    let hashes = content_hashes_for(&files);
    GraphCacheManifest::from_discovered_files(&config.root, &files, store.manifest.mode, |file| {
        hashes.get(file.id.0 as usize).copied().unwrap_or_default()
    })
}

/// Content hash per `FileId`, mirroring what the analysis path feeds the
/// manifest. Parsing without a cache is what the production builder gets from
/// the parse stage it runs anyway.
fn content_hashes_for(files: &[fallow_types::discover::DiscoveredFile]) -> Vec<u64> {
    let parsed = fallow_core::extract::parse_all_files(files, None, false);
    let mut hashes = vec![0u64; files.len()];
    for module in &parsed.modules {
        if let Some(slot) = hashes.get_mut(module.file_id.0 as usize) {
            *slot = module.content_hash;
        }
    }
    hashes
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
fn effective_export_resolution_cold_vs_warm_identical() {
    assert_cold_warm_identical("effective-export-explicit-shadow");
    assert_cold_warm_identical("effective-export-ambiguous-star");
    assert_cold_warm_identical("effective-export-ambiguous-star-partial");
    assert_cold_warm_identical("effective-export-ambiguous-star-nested");
    assert_cold_warm_identical("effective-export-convergent-star");
    assert_cold_warm_identical("declaration-merge");
    assert_cold_warm_identical("renamed-star-surface");
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

#[test]
#[expect(
    deprecated,
    reason = "trace timings are still the internal contract for this cache performance gate"
)]
fn warm_graph_cache_hit_skips_import_resolution() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_fixture_tree(&fixture_path("barrel-exports"), &root);
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

/// Force the resolver-payload reuse path: the manifest still passes
/// `classify_resolution_mismatch` while the persisted graph is unusable, so the
/// graph is rebuilt from the cached `ResolvedProject` alone. Every import field
/// the graph reads must survive that mirror, so the rebuilt output has to match
/// the cold output exactly.
#[expect(
    deprecated,
    reason = "trace timings are still the internal contract for this cache performance gate"
)]
fn assert_resolver_cache_hit_matches_cold(fixture: &str) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_fixture_tree(&fixture_path(fixture), &root);
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
        store
            .manifest
            .classify_resolution_mismatch(&current)
            .is_none(),
        "stable file keys and content hashes should still allow resolver reuse"
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

#[test]
fn resolver_cache_hit_rebuilds_graph_when_file_ids_shift() {
    assert_resolver_cache_hit_matches_cold("barrel-exports");
}

#[test]
fn resolver_cache_hit_preserves_ambient_type_only_star_lane() {
    // Issue #2375: the type-only-star flag rides on `ImportInfo`, so a graph
    // rebuilt from the cached resolver payload alone must still credit the
    // ambient `export type *` target in the type lane only.
    assert_resolver_cache_hit_matches_cold("issue-2375-ambient-type-star");
}

/// Resolve a real-world benchmark fixture path. These are gitignored symlinks
/// that may be absent on a fresh checkout, so callers skip when missing.
fn benchmark_fixture_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
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

/// The manifest must hit on identical inputs and miss when file content or a
/// graph-affecting mode hash changes. This pins `matches_inputs` against the
/// real `from_discovered_files` shape used by the integration path.
#[test]
fn manifest_matches_only_on_identical_inputs() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_fixture_tree(&fixture_path("namespace-imports"), &root);

    let config = create_config_with_cache(root, temp.path().join("cache"));
    let files = fallow_core::discover::discover_files(&config);

    let hashes = content_hashes_for(&files);
    let content_hash_provider = |file: &fallow_types::discover::DiscoveredFile| {
        hashes.get(file.id.0 as usize).copied().unwrap_or_default()
    };

    let manifest_a = GraphCacheManifest::from_discovered_files(
        &config.root,
        &files,
        GraphCacheMode::new(1, 2, 3),
        content_hash_provider,
    );
    let manifest_same = GraphCacheManifest::from_discovered_files(
        &config.root,
        &files,
        GraphCacheMode::new(1, 2, 3),
        content_hash_provider,
    );
    let manifest_other_mode = GraphCacheManifest::from_discovered_files(
        &config.root,
        &files,
        GraphCacheMode::new(1, 99, 3),
        content_hash_provider,
    );

    assert!(manifest_a.matches_inputs(&manifest_same));
    assert!(!manifest_a.matches_inputs(&manifest_other_mode));
}

#[test]
fn a_graph_cache_from_another_root_never_reuses_original_checkout_paths() {
    let temp = tempfile::tempdir().expect("temp root");
    let original = temp.path().join("original");
    let relocated = temp.path().join("relocated");
    let cache = temp.path().join("cache");
    std::fs::create_dir_all(original.join("src")).unwrap();
    std::fs::write(
        original.join("package.json"),
        r#"{"name":"relocation","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        original.join("src/index.ts"),
        "import { used } from './lib'; console.log(used);\n",
    )
    .unwrap();
    std::fs::write(
        original.join("src/lib.ts"),
        "export const used = 1; export const unused = 2;\n",
    )
    .unwrap();
    let original_config = create_config_with_cache(original.clone(), cache.clone());
    fallow_core::analyze(&original_config).expect("prime original cache");
    copy_fixture_tree(&original, &relocated);
    let mut relocated_config = create_config_with_cache(relocated.clone(), cache);
    let warm = fallow_core::analyze(&relocated_config).expect("relocated warm analysis");
    relocated_config.no_cache = true;
    let cold = fallow_core::analyze(&relocated_config).expect("relocated uncached analysis");
    assert_eq!(format!("{warm:#?}"), format!("{cold:#?}"));
    assert!(
        warm.unused_exports
            .iter()
            .all(|finding| finding.export.path.starts_with(&relocated))
    );
}
