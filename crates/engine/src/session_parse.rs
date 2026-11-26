//! Parse dispatch using immutable session configuration and cache identity.
use super::*;
use fallow_types::discover::FileId;

pub(super) fn parse_files_with_config(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    need_complexity: bool,
    cancellation: Option<&AtomicBool>,
) -> ParsedModules {
    let parse_start = Instant::now();
    let cache_max_size_bytes = crate::project_config::resolve_cache_max_size_bytes(config);
    let mut cache_rejection = None;
    let mut parse_cache_bytes_read = 0;
    let mut parse_cache_load_ms = 0.0;
    let mut cache = if config.no_cache {
        None
    } else {
        let load_start = Instant::now();
        let (loaded, bytes_read) = fallow_extract::cache::CacheStore::load_counting_bytes(
            &config.cache_dir,
            &config.root,
            config.cache_config_hash,
            cache_max_size_bytes,
        );
        parse_cache_bytes_read = bytes_read;
        parse_cache_load_ms = load_start.elapsed().as_secs_f64() * 1000.0;
        match loaded {
            Ok(store) => Some(store),
            Err(rejection) => {
                cache_rejection = Some(rejection);
                None
            }
        }
    };
    let read_started_ns = now_ns();
    let parse_result = crate::source::parse_all_files(
        files,
        cache.as_ref(),
        fallow_extract::ParseNeeds {
            complexity: need_complexity,
            call_facts: config.reads_call_facts(),
        },
        cancellation,
        &config.flags.patterns(),
        config.extraction_mode,
    );
    record_source_diagnostics(
        config,
        &parse_result.read_failures,
        &parse_result.parse_degradations,
    );
    let mut modules = parse_result.modules;
    for module in &mut modules {
        module.prepare_analysis_facts();
    }
    let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
    let cache_ms = if token_is_set(cancellation) {
        0.0
    } else {
        update_parse_cache_if_enabled(
            config,
            &mut cache,
            &ParseCacheWrite {
                modules: &modules,
                files,
                need_complexity,
                metadata_hits: &parse_result.metadata_hits,
                fingerprint_of: &|file: &DiscoveredFile| {
                    source_fingerprint(&file.path).for_content_read_at(read_started_ns)
                },
            },
        )
    };
    let metrics = core_backend::ParseMetrics {
        parse_ms,
        cache_ms,
        cache_hits: parse_result.cache_hits,
        cache_misses: parse_result.cache_misses,
        parse_cpu_ms: parse_result.parse_cpu_ms,
        cache_rejection,
        files_read: parse_result.files_read,
        source_bytes_read: parse_result.source_bytes_read,
        parse_cache_bytes_read,
        css_masked_bytes: parse_result.css_masked_bytes,
        parse_cache_load_ms,
    };
    ParsedModules {
        modules,
        metrics,
        read_failures: parse_result.read_failures,
        parse_degradations: parse_result.parse_degradations,
    }
}

fn update_parse_cache_if_enabled(
    config: &ResolvedConfig,
    cache: &mut Option<fallow_extract::cache::CacheStore>,
    write: &ParseCacheWrite<'_>,
) -> f64 {
    let start = Instant::now();
    write_parse_cache(config, cache, write);
    start.elapsed().as_secs_f64() * 1000.0
}

/// Modules to store in the persisted parse cache, with the fingerprint that
/// each file gets in the cache.
pub(super) struct ParseCacheWrite<'a> {
    pub(super) modules: &'a [ModuleInfo],
    pub(super) files: &'a [DiscoveredFile],
    pub(super) need_complexity: bool,
    /// Files that the parse served from cache metadata, sorted by id. Their
    /// entries keep the stored fingerprint, and `fingerprint_of` is not called
    /// for them.
    pub(super) metadata_hits: &'a [FileId],
    pub(super) fingerprint_of: &'a dyn Fn(&DiscoveredFile) -> SourceFingerprint,
}

pub(super) fn write_parse_cache(
    config: &ResolvedConfig,
    cache: &mut Option<fallow_extract::cache::CacheStore>,
    write: &ParseCacheWrite<'_>,
) {
    if config.no_cache {
        return;
    }

    let cache_max_size_bytes = crate::project_config::resolve_cache_max_size_bytes(config);
    let store = cache.get_or_insert_with(|| fallow_extract::cache::CacheStore::new(&config.root));
    if update_parse_cache(store, write, config.reads_call_facts())
        && let Err(error) = store.save(
            config.cache_target(),
            config.cache_config_hash,
            cache_max_size_bytes,
        )
    {
        tracing::warn!("Failed to save cache: {error}");
    }
}

/// Mirror of `fallow_core`'s `update_cache` for session-owned parsing. See
/// [`fallow_extract::cache::CacheStore::record_module`] for the rules that keep
/// the complexity and the call facts of an earlier run.
fn update_parse_cache(
    store: &mut fallow_extract::cache::CacheStore,
    write: &ParseCacheWrite<'_>,
    need_call_facts: bool,
) -> bool {
    let ParseCacheWrite {
        modules,
        files,
        need_complexity,
        metadata_hits,
        fingerprint_of,
    } = *write;
    let mut dirty = false;
    for module in modules {
        if fallow_extract::is_metadata_hit(metadata_hits, module.file_id) {
            continue;
        }
        if let Some(file) = files.get(module.file_id.0 as usize) {
            dirty |= store.record_module(
                &file.path,
                module,
                fingerprint_of(file),
                need_complexity,
                need_call_facts,
            );
        }
    }
    store.retain_paths(files) || dirty
}

fn source_fingerprint(path: &Path) -> SourceFingerprint {
    std::fs::metadata(path).map_or_else(
        |_| SourceFingerprint::new(0, 0),
        |metadata| SourceFingerprint::from_metadata(&metadata),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "export const alpha = 1;\n";
    const STORED: SourceFingerprint = SourceFingerprint::with_ctime(10, 20, 24);

    /// Store the module of a path that does not exist, then write that module
    /// again. A metadata read of the path gives an empty fingerprint, so a
    /// changed entry shows that the writer read the metadata. Returns whether
    /// the write changed the store, and the stored fingerprint.
    fn rewrite_missing_file(metadata_hits: &[FileId]) -> (bool, SourceFingerprint) {
        let root = std::env::temp_dir().join("fallow-session-cache-no-such-project");
        let path = root.join("api.ts");
        let module = fallow_extract::parse_source_to_module(FileId(0), &path, SOURCE, 0, false);
        let mut store = fallow_extract::cache::CacheStore::new(&root);
        store.record_module(&path, &module, STORED, false, false);
        let files = [DiscoveredFile {
            id: FileId(0),
            path: path.clone(),
            size_bytes: STORED.file_size,
        }];
        let read_started_ns = now_ns();
        let changed = update_parse_cache(
            &mut store,
            &ParseCacheWrite {
                modules: std::slice::from_ref(&module),
                files: &files,
                need_complexity: false,
                metadata_hits,
                fingerprint_of: &|file: &DiscoveredFile| {
                    source_fingerprint(&file.path).for_content_read_at(read_started_ns)
                },
            },
            false,
        );
        let stored = store
            .get_by_path_only(&path)
            .expect("entry stays in the store")
            .source_fingerprint();
        (changed, stored)
    }

    #[test]
    #[cfg_attr(miri, ignore = "the metadata read is blocked by Miri isolation")]
    fn the_cache_write_skips_the_stat_of_a_metadata_hit() {
        assert_eq!(rewrite_missing_file(&[FileId(0)]), (false, STORED));
    }

    #[test]
    #[cfg_attr(miri, ignore = "the metadata read is blocked by Miri isolation")]
    fn the_cache_write_reads_the_metadata_of_any_other_module() {
        let (changed, stored) = rewrite_missing_file(&[]);
        assert!(changed);
        assert_eq!(stored, SourceFingerprint::new(0, 0));
    }
}
