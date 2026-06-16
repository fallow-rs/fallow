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
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
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
        injection_tokens: vec![],
        local_type_declarations: vec![],
        public_signature_type_references: vec![],
        namespace_object_aliases: vec![],
        iconify_prefixes: vec![],
        iconify_icon_names: vec![],
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        component_functions: Vec::new(),
        react_props: Vec::new(),
        hook_uses: Vec::new(),
        render_edges: Vec::new(),
    };

    store.insert(std::path::Path::new("test.ts"), cached);
    assert_eq!(store.len(), 1);

    store
        .save(&temp_dir, 0, fallow_extract::cache::DEFAULT_CACHE_MAX_SIZE)
        .unwrap();
    let loaded =
        CacheStore::load(&temp_dir, 0, fallow_extract::cache::DEFAULT_CACHE_MAX_SIZE).unwrap();
    assert_eq!(loaded.len(), 1);

    assert!(loaded.get(std::path::Path::new("test.ts"), 12345).is_some());
    assert!(loaded.get(std::path::Path::new("test.ts"), 99999).is_none());
    assert!(
        loaded
            .get(std::path::Path::new("other.ts"), 12345)
            .is_none()
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn incremental_no_cache_all_misses() {
    let root = fixture_path("basic-project");
    let files = fallow_core::discover::discover_files(&create_config(root));
    let parse_result = fallow_core::extract::parse_all_files(&files, None, false);

    assert_eq!(parse_result.cache_hits, 0);
    assert_eq!(parse_result.cache_misses, parse_result.modules.len());
    assert!(!parse_result.modules.is_empty());
}

#[test]
fn retaining_modules_releases_graph_payload_after_analysis() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"payload-release-fixture","version":"1.0.0"}"#,
    )
    .expect("write package.json");
    std::fs::write(root.join("src/lazy.ts"), "export const lazy = 1;\n")
        .expect("write lazy module");
    std::fs::write(root.join("src/required.ts"), "exports.value = 2;\n")
        .expect("write required module");
    std::fs::write(
        root.join("src/index.ts"),
        r#"
export async function load(flag: boolean) {
  const required = require("./required");
  if (flag) {
    return import("./lazy");
  }
  return required.value;
}
"#,
    )
    .expect("write index module");

    let config = create_config(root.to_path_buf());
    let files = fallow_core::discover::discover_files(&config);
    let parsed = fallow_core::extract::parse_all_files(&files, None, true);
    let parsed_index = parsed
        .modules
        .iter()
        .find(|module| {
            files
                .get(module.file_id.0 as usize)
                .is_some_and(|file| file.path.ends_with("src/index.ts"))
        })
        .expect("parsed index module should exist");
    assert!(!parsed_index.dynamic_imports.is_empty());
    assert!(!parsed_index.require_calls.is_empty());

    let output =
        fallow_core::analyze_retaining_modules(&config, true, false).expect("analysis succeeds");
    let retained_modules = output.modules.expect("retained modules");
    let retained_files = output.files.expect("retained files");
    let retained_index = retained_modules
        .iter()
        .find(|module| {
            retained_files
                .get(module.file_id.0 as usize)
                .is_some_and(|file| file.path.ends_with("src/index.ts"))
        })
        .expect("retained index module should exist");

    assert!(retained_index.dynamic_imports.is_empty());
    assert_eq!(retained_index.dynamic_imports.capacity(), 0);
    assert!(retained_index.require_calls.is_empty());
    assert_eq!(retained_index.require_calls.capacity(), 0);
    assert!(!retained_index.line_offsets.is_empty());
    assert!(!retained_index.complexity.is_empty());
}

#[test]
fn incremental_with_cache_all_hits() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let files = fallow_core::discover::discover_files(&config);

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

    let second = fallow_core::extract::parse_all_files(&files, Some(&cache_store), false);
    assert_eq!(second.cache_hits, first.modules.len());
    assert_eq!(second.cache_misses, 0);
    assert_eq!(second.modules.len(), first.modules.len());
}

#[test]
fn incremental_results_identical() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let files = fallow_core::discover::discover_files(&config);

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

    let second = fallow_core::extract::parse_all_files(&files, Some(&cache_store), false);

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
    let root = fixture_path("basic-project");
    let tmp_cache = tempfile::tempdir().expect("create temp dir");
    let config = create_config_with_cache(root, tmp_cache.path().to_path_buf());

    let first = fallow_core::analyze(&config).expect("first analysis should succeed");

    let second = fallow_core::analyze(&config).expect("second analysis should succeed");

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
    let mut store = fallow_core::cache::CacheStore::new();
    let make_module = || fallow_core::cache::CachedModule {
        content_hash: 1,
        mtime_secs: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: vec![],
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
        injection_tokens: vec![],
        local_type_declarations: vec![],
        public_signature_type_references: vec![],
        namespace_object_aliases: vec![],
        iconify_prefixes: vec![],
        iconify_icon_names: vec![],
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        component_functions: Vec::new(),
        react_props: Vec::new(),
        hook_uses: Vec::new(),
        render_edges: Vec::new(),
    };

    store.insert(std::path::Path::new("/project/existing.ts"), make_module());
    store.insert(std::path::Path::new("/project/deleted.ts"), make_module());
    assert_eq!(store.len(), 2);

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
