use std::path::PathBuf;

use super::common::{create_config, create_config_with_cache, fixture_path};

#[test]
fn cache_roundtrip() {
    use fallow_core::cache::CacheStore;
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("fallow-test-cache-{unique}"));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let mut store = CacheStore::new();
    assert!(store.is_empty());

    let cached = fallow_core::cache::CachedModule {
        content_hash: 12345,
        mtime_secs: 0,
        file_size: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: vec![],
        public_signature_type_references: vec![],
        namespace_object_aliases: vec![],
    };

    store.insert(std::path::Path::new("test.ts"), cached);
    assert_eq!(store.len(), 1);

    // Save and reload
    store.save(&temp_dir).unwrap();
    let loaded = CacheStore::load(&temp_dir).unwrap();
    assert_eq!(loaded.len(), 1);

    // Correct hash -> hit
    assert!(loaded.get(std::path::Path::new("test.ts"), 12345).is_some());
    // Wrong hash -> miss
    assert!(loaded.get(std::path::Path::new("test.ts"), 99999).is_none());
    // Unknown file -> miss
    assert!(
        loaded
            .get(std::path::Path::new("other.ts"), 12345)
            .is_none()
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

// ── Incremental analysis (Phase A) tests ──────────────────────────

#[test]
fn incremental_no_cache_all_misses() {
    // First run without any existing cache: all files should be cache misses
    let root = fixture_path("basic-project");
    let files = fallow_core::discover::discover_files(&create_config(root));
    let parse_result = fallow_core::extract::parse_all_files(&files, None, false);

    assert_eq!(parse_result.cache_hits, 0);
    assert_eq!(parse_result.cache_misses, parse_result.modules.len());
    assert!(!parse_result.modules.is_empty());
}

#[test]
fn incremental_with_cache_all_hits() {
    // Build a cache from the first parse, then parse again — should be all hits
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let files = fallow_core::discover::discover_files(&config);

    // First parse: build cache
    let first = fallow_core::extract::parse_all_files(&files, None, false);
    let mut cache_store = fallow_core::cache::CacheStore::new();
    for module in &first.modules {
        if let Some(file) = files.get(module.file_id.0 as usize) {
            cache_store.insert(
                &file.path,
                fallow_core::cache::module_to_cached(module, 0, 0),
            );
        }
    }

    // Second parse: should hit cache for every file
    let second = fallow_core::extract::parse_all_files(&files, Some(&cache_store), false);
    assert_eq!(second.cache_hits, first.modules.len());
    assert_eq!(second.cache_misses, 0);
    assert_eq!(second.modules.len(), first.modules.len());
}

#[test]
fn incremental_results_identical() {
    // Results from a cached run should be identical to a fresh run
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let files = fallow_core::discover::discover_files(&config);

    // First parse
    let first = fallow_core::extract::parse_all_files(&files, None, false);
    let mut cache_store = fallow_core::cache::CacheStore::new();
    for module in &first.modules {
        if let Some(file) = files.get(module.file_id.0 as usize) {
            cache_store.insert(
                &file.path,
                fallow_core::cache::module_to_cached(module, 0, 0),
            );
        }
    }

    // Second parse (from cache)
    let second = fallow_core::extract::parse_all_files(&files, Some(&cache_store), false);

    // Verify all module data matches
    assert_eq!(first.modules.len(), second.modules.len());
    for (a, b) in first.modules.iter().zip(second.modules.iter()) {
        assert_eq!(a.file_id, b.file_id);
        assert_eq!(a.content_hash, b.content_hash);
        assert_eq!(a.exports.len(), b.exports.len());
        assert_eq!(a.imports.len(), b.imports.len());
        assert_eq!(a.re_exports.len(), b.re_exports.len());
        assert_eq!(a.dynamic_imports.len(), b.dynamic_imports.len());
        assert_eq!(a.has_cjs_exports, b.has_cjs_exports);
        assert_eq!(a.suppressions.len(), b.suppressions.len());
    }
}

#[test]
fn incremental_full_pipeline_results_match() {
    // Full pipeline results should be identical whether using cache or not
    let root = fixture_path("basic-project");
    let tmp_cache = tempfile::tempdir().expect("create temp dir");
    let config = create_config_with_cache(root, tmp_cache.path().to_path_buf());

    // First run: populates cache
    let first = fallow_core::analyze(&config).expect("first analysis should succeed");

    // Second run: uses cache
    let second = fallow_core::analyze(&config).expect("second analysis should succeed");

    // Results should be identical
    assert_eq!(first.unused_files.len(), second.unused_files.len());
    assert_eq!(first.unused_exports.len(), second.unused_exports.len());
    assert_eq!(first.unused_types.len(), second.unused_types.len());
    assert_eq!(
        first.unresolved_imports.len(),
        second.unresolved_imports.len()
    );
}

#[test]
fn incremental_cache_prune_stale_entries() {
    // Cache entries for deleted files should be pruned
    let mut store = fallow_core::cache::CacheStore::new();
    let make_module = || fallow_core::cache::CachedModule {
        content_hash: 1,
        mtime_secs: 0,
        file_size: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: vec![],
        public_signature_type_references: vec![],
        namespace_object_aliases: vec![],
    };

    store.insert(std::path::Path::new("/project/existing.ts"), make_module());
    store.insert(std::path::Path::new("/project/deleted.ts"), make_module());
    assert_eq!(store.len(), 2);

    // Only "existing.ts" is in the current file set
    let files = vec![fallow_core::discover::DiscoveredFile {
        id: fallow_core::discover::FileId(0),
        path: PathBuf::from("/project/existing.ts"),
        size_bytes: 100,
    }];
    store.retain_paths(&files);

    assert_eq!(store.len(), 1);
    assert!(
        store
            .get_by_path_only(std::path::Path::new("/project/existing.ts"))
            .is_some()
    );
    assert!(
        store
            .get_by_path_only(std::path::Path::new("/project/deleted.ts"))
            .is_none()
    );
}
