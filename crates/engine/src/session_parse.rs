//! Parse dispatch using immutable session configuration and cache identity.
use super::*;

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
