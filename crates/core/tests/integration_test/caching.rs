use std::path::PathBuf;

use super::common::{create_config, create_config_with_cache, fixture_path};

#[test]
fn source_read_failure_preserves_sparse_file_ids() {
    use fallow_types::discover::{DiscoveredFile, FileId};

    let project = tempfile::tempdir().expect("create project");
    let files = ["a.ts", "b.ts", "c.ts"].map(|name| project.path().join(name));
    for (index, path) in files.iter().enumerate() {
        std::fs::write(path, format!("export const value{index} = {index};\n"))
            .expect("write source");
    }
    let discovered: Vec<DiscoveredFile> = files
        .iter()
        .enumerate()
        .map(|(index, path)| DiscoveredFile {
            id: FileId(u32::try_from(index).expect("test index fits u32")),
            path: path.clone(),
            size_bytes: std::fs::metadata(path).expect("source metadata").len(),
        })
        .collect();
    std::fs::remove_file(&files[1]).expect("remove middle source after discovery");

    let result = fallow_core::extract::parse_all_files(&discovered, None, false);

    assert_eq!(
        result
            .modules
            .iter()
            .map(|module| module.file_id)
            .collect::<Vec<_>>(),
        vec![FileId(0), FileId(2)]
    );
    assert_eq!(result.read_failures.len(), 1);
    assert_eq!(result.read_failures[0].file_id, FileId(1));
    assert_eq!(result.read_failures[0].path, files[1]);
    assert!(!result.read_failures[0].error.is_empty());
}

#[cfg(unix)]
#[test]
fn warm_metadata_cache_reports_source_that_becomes_unreadable() {
    use std::os::unix::fs::PermissionsExt;

    use fallow_core::cache::{CacheStore, module_to_cached};
    use fallow_types::discover::{DiscoveredFile, FileId};
    use fallow_types::source_fingerprint::SourceFingerprint;

    let project = tempfile::tempdir().expect("create project");
    let path = project.path().join("cached.ts");
    std::fs::write(&path, "export const cached = 1;\n").expect("write source");
    let metadata = std::fs::metadata(&path).expect("source metadata");
    let discovered = [DiscoveredFile {
        id: FileId(0),
        path: path.clone(),
        size_bytes: metadata.len(),
    }];

    let cold = fallow_core::extract::parse_all_files(&discovered, None, false);
    let mut cache = CacheStore::new(std::path::Path::new(""));
    cache.insert(
        &path,
        module_to_cached(
            cold.modules.first().expect("cold parse produces module"),
            SourceFingerprint::from_metadata(&metadata),
            false,
        ),
    );

    let original_mode = metadata.permissions().mode();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0))
        .expect("make source unreadable");
    let warm = fallow_core::extract::parse_all_files(&discovered, Some(&cache), false);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(original_mode))
        .expect("restore source permissions");

    assert!(
        warm.modules.is_empty(),
        "stale cached module must not be used"
    );
    assert_eq!(warm.cache_hits, 0);
    assert_eq!(warm.read_failures.len(), 1);
    assert_eq!(warm.read_failures[0].file_id, FileId(0));
    assert_eq!(warm.read_failures[0].path, path);
    assert!(!warm.read_failures[0].error.is_empty());
}

/// The metadata fast path must not serve a cached module for a file that was
/// rewritten to the same length with its modification time restored.
///
/// `(mtime, size)` is writer-controlled, so this shape is reachable through
/// `touch -r`, a codemod that puts the timestamp back, and a `git checkout` of
/// an equal-length revision. Serving the cached parse here means every
/// downstream verdict describes the previous content.
#[test]
fn warm_metadata_cache_misses_an_equal_length_rewrite_with_a_restored_mtime() {
    use fallow_core::cache::{CacheStore, module_to_cached};
    use fallow_types::discover::{DiscoveredFile, FileId};
    use fallow_types::source_fingerprint::SourceFingerprint;

    let project = tempfile::tempdir().expect("create project");
    let path = project.path().join("api.ts");
    let before = "export const alpha = 1;\n";
    let after = "export const bravo = 1;\n";
    assert_eq!(before.len(), after.len());
    std::fs::write(&path, before).expect("write source");

    let metadata = std::fs::metadata(&path).expect("source metadata");
    let modified = metadata.modified().expect("source mtime");
    let accessed = metadata.accessed().unwrap_or(modified);
    let discovered = [DiscoveredFile {
        id: FileId(0),
        path: path.clone(),
        size_bytes: metadata.len(),
    }];

    let cold = fallow_core::extract::parse_all_files(&discovered, None, false);
    let mut cache = CacheStore::new(std::path::Path::new(""));
    cache.insert(
        &path,
        module_to_cached(
            cold.modules.first().expect("cold parse produces module"),
            SourceFingerprint::from_metadata(&metadata),
            false,
        ),
    );

    std::fs::write(&path, after).expect("rewrite source");
    let handle = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open source for timestamp restore");
    handle
        .set_times(
            std::fs::FileTimes::new()
                .set_accessed(accessed)
                .set_modified(modified),
        )
        .expect("restore source timestamps");
    let rewritten = std::fs::metadata(&path).expect("source metadata after rewrite");
    assert_eq!(rewritten.len(), metadata.len());
    assert_eq!(rewritten.modified().expect("mtime after rewrite"), modified);

    let warm = fallow_core::extract::parse_all_files(&discovered, Some(&cache), false);

    let exports: Vec<String> = warm
        .modules
        .first()
        .expect("warm parse produces module")
        .exports
        .iter()
        .map(|export| export.name.to_string())
        .collect();
    assert_eq!(
        exports,
        vec!["bravo".to_string()],
        "the warm parse must describe the file on disk, not the cached one"
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "roundtrip fixture enumerates cache fields"
)]
fn cache_roundtrip() {
    use fallow_core::cache::CacheStore;

    let temp = tempfile::tempdir().expect("create temp dir");
    let temp_dir = temp.path().join("cache");

    let mut store = CacheStore::new(std::path::Path::new(""));
    assert!(store.is_empty());

    let cached = fallow_core::cache::CachedModule {
        content_hash: 12345,
        mtime_ns: 0,
        ctime_ns: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: Box::default(),
        member_accesses: vec![],
        semantic_facts: None,
        whole_object_uses: Box::default(),
        dynamic_import_patterns: vec![],
        parse_error_count: 0,
        parse_panicked: false,
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        complexity_extracted: false,
        flag_uses: vec![],
        flag_registry_facts: None,
        class_heritage: vec![],
        exported_factory_returns: None,
        exported_factory_return_object_shapes: None,
        type_member_types: None,
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
        inline_server_action_exports: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        angular_inputs: Vec::new(),
        angular_outputs: Vec::new(),
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
        svelte_dispatched_events: Vec::new(),
        svelte_listened_events: Vec::new(),
        angular_component_selectors: Vec::new(),
        registered_custom_elements: Vec::new(),
        used_custom_element_tags: Vec::new(),
        angular_used_selectors: Vec::new(),
        angular_entry_component_refs: Vec::new(),
        has_dynamic_component_render: false,
        has_dynamic_dispatch: false,
    };

    store.insert(std::path::Path::new("test.ts"), cached);
    assert_eq!(store.len(), 1);

    store
        .save(&temp_dir, 0, fallow_extract::cache::DEFAULT_CACHE_MAX_SIZE)
        .unwrap();
    let loaded = CacheStore::load(
        &temp_dir,
        std::path::Path::new(""),
        0,
        fallow_extract::cache::DEFAULT_CACHE_MAX_SIZE,
    )
    .unwrap();
    assert_eq!(loaded.len(), 1);

    assert!(loaded.get(std::path::Path::new("test.ts"), 12345).is_some());
    assert!(loaded.get(std::path::Path::new("test.ts"), 99999).is_none());
    assert!(
        loaded
            .get(std::path::Path::new("other.ts"), 12345)
            .is_none()
    );
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
    let mut cache_store = fallow_core::cache::CacheStore::new(std::path::Path::new(""));
    for module in &first.modules {
        if let Some(file) = files.get(module.file_id.0 as usize) {
            cache_store.insert(
                &file.path,
                fallow_core::cache::module_to_cached(
                    module,
                    fallow_types::source_fingerprint::SourceFingerprint::new(0, 0),
                    false,
                ),
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
    let mut cache_store = fallow_core::cache::CacheStore::new(std::path::Path::new(""));
    for module in &first.modules {
        if let Some(file) = files.get(module.file_id.0 as usize) {
            cache_store.insert(
                &file.path,
                fallow_core::cache::module_to_cached(
                    module,
                    fallow_types::source_fingerprint::SourceFingerprint::new(0, 0),
                    false,
                ),
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
#[expect(
    clippy::too_many_lines,
    reason = "prune fixture enumerates every cache field"
)]
fn incremental_cache_prune_stale_entries() {
    let mut store = fallow_core::cache::CacheStore::new(std::path::Path::new(""));
    let make_module = || fallow_core::cache::CachedModule {
        content_hash: 1,
        mtime_ns: 0,
        ctime_ns: 0,
        file_size: 0,
        last_access_secs: 0,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        require_calls: vec![],
        package_path_references: Box::default(),
        member_accesses: vec![],
        semantic_facts: None,
        whole_object_uses: Box::default(),
        dynamic_import_patterns: vec![],
        parse_error_count: 0,
        parse_panicked: false,
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        line_offsets: vec![],
        complexity: vec![],
        complexity_extracted: false,
        flag_uses: vec![],
        flag_registry_facts: None,
        class_heritage: vec![],
        exported_factory_returns: None,
        exported_factory_return_object_shapes: None,
        type_member_types: None,
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
        inline_server_action_exports: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        angular_inputs: Vec::new(),
        angular_outputs: Vec::new(),
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
        svelte_dispatched_events: Vec::new(),
        svelte_listened_events: Vec::new(),
        angular_component_selectors: Vec::new(),
        registered_custom_elements: Vec::new(),
        used_custom_element_tags: Vec::new(),
        angular_used_selectors: Vec::new(),
        angular_entry_component_refs: Vec::new(),
        has_dynamic_component_render: false,
        has_dynamic_dispatch: false,
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

/// A second complexity-consuming run on an unchanged tree must not reparse.
///
/// `dead-code` parses with `need_complexity == false`, so its cache entries
/// carry no complexity. The entry that `health` then writes has to be stored:
/// while the write-back skipped every entry whose content hash still matched,
/// the complexity a `health` run computed was thrown away every time, and each
/// later `health` run paid a full cold parse forever.
#[test]
#[expect(
    deprecated,
    reason = "fallow_core is the internal surface that exposes parse-cache counters"
)]
fn health_after_dead_code_stops_reparsing_once_complexity_is_cached() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_fixture_tree(&fixture_path("basic-project"), &root);
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let _ = fallow_core::analyze_retaining_modules(&config, false, true)
        .expect("dead-code style run succeeds");
    let first_health = fallow_core::analyze_retaining_modules(&config, true, true)
        .expect("first complexity run succeeds");
    let second_health = fallow_core::analyze_retaining_modules(&config, true, true)
        .expect("second complexity run succeeds");

    assert!(
        cache_misses(&first_health) > 0,
        "the first complexity run has no cached complexity to reuse"
    );
    assert_eq!(
        cache_misses(&second_health),
        0,
        "the complexity the first run computed must be cached for the next one"
    );
}

/// A metadata-only touch followed by a complexity-blind run must not strip the
/// complexity a previous run stored.
///
/// The touch makes the metadata fingerprint stale, which sends the write-back
/// down the refresh branch. That branch rewrote the whole entry from the
/// current run's module, and on a `dead-code` run that module's complexity is
/// empty by design, so one touch plus one `dead-code` run degraded a rich cache
/// permanently.
#[test]
#[expect(
    deprecated,
    reason = "fallow_core is the internal surface that exposes parse-cache counters"
)]
fn a_complexity_blind_run_does_not_strip_cached_complexity() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    copy_fixture_tree(&fixture_path("basic-project"), &root);
    let config = create_config_with_cache(root.clone(), temp.path().join("cache"));

    let _ = fallow_core::analyze_retaining_modules(&config, true, true)
        .expect("complexity run succeeds");

    touch_every_source(&root);
    let _ = fallow_core::analyze_retaining_modules(&config, false, true)
        .expect("dead-code style run succeeds");

    let health = fallow_core::analyze_retaining_modules(&config, true, true)
        .expect("complexity run after touch succeeds");
    assert_eq!(
        cache_misses(&health),
        0,
        "a touch plus a complexity-blind run must not force a cold complexity parse"
    );
}

/// A cache entry for a file the current run did not discover must survive when
/// the file is still on disk.
///
/// Discovery scope is per-command: `--production` drops test and story files,
/// `--root` narrows to a subtree. Evicting whatever the current scope did not
/// walk meant one narrow run threw away every entry outside its scope and the
/// next full run reparsed them from cold.
#[test]
fn retain_paths_keeps_entries_for_files_that_still_exist() {
    let project = tempfile::tempdir().expect("create project");
    let discovered_path = project.path().join("discovered.ts");
    let undiscovered_path = project.path().join("out-of-scope.ts");
    let deleted_path = project.path().join("deleted.ts");
    for path in [&discovered_path, &undiscovered_path, &deleted_path] {
        std::fs::write(path, "export const value = 1;\n").expect("write source");
    }
    std::fs::remove_file(&deleted_path).expect("remove source after caching it");

    let mut store = fallow_core::cache::CacheStore::new(std::path::Path::new(""));
    for path in [&discovered_path, &undiscovered_path, &deleted_path] {
        let module = fallow_extract::parse_from_content(
            fallow_types::discover::FileId(0),
            path,
            "export const value = 1;\n",
        );
        store.insert(
            path,
            fallow_core::cache::module_to_cached(
                &module,
                fallow_types::source_fingerprint::SourceFingerprint::new(0, 0),
                false,
            ),
        );
    }

    let files = vec![fallow_core::discover::DiscoveredFile {
        id: fallow_core::discover::FileId(0),
        path: discovered_path.clone(),
        size_bytes: 1,
    }];
    store.retain_paths(&files);

    assert!(
        store.get_by_path_only(&discovered_path).is_some(),
        "a discovered file keeps its entry"
    );
    assert!(
        store.get_by_path_only(&undiscovered_path).is_some(),
        "a file outside this run's scope is not gone, so its entry must survive"
    );
    assert!(
        store.get_by_path_only(&deleted_path).is_none(),
        "a deleted file must lose its entry"
    );
}

fn cache_misses(output: &fallow_core::AnalysisOutput) -> usize {
    output
        .timings
        .as_ref()
        .expect("retained trace timings")
        .cache_misses
}

/// Bump every source file's modification time without changing its content.
fn touch_every_source(root: &std::path::Path) {
    let now = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
    for entry in walk_files(root) {
        let handle = std::fs::OpenOptions::new()
            .write(true)
            .open(&entry)
            .expect("open source for touch");
        handle
            .set_times(
                std::fs::FileTimes::new()
                    .set_accessed(now)
                    .set_modified(now),
            )
            .expect("touch source");
    }
}

fn walk_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if entry.file_type().expect("file type").is_dir() {
            found.extend(walk_files(&path));
        } else {
            found.push(path);
        }
    }
    found
}

fn copy_fixture_tree(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).expect("create dest dir");
    for entry in std::fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("dir entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_fixture_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).expect("copy file");
        }
    }
}
