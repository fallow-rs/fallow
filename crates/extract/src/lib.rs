//! Parsing and extraction engine for the fallow codebase analyzer.
//!
//! This crate handles all file parsing: JS/TS via Oxc, Vue/Svelte SFC extraction,
//! Astro frontmatter, MDX import/export extraction, CSS Module class name extraction,
//! HTML asset reference extraction, and incremental caching of parse results.

#![warn(missing_docs)]

mod asset_url;
pub mod astro;
pub mod cache;
pub(crate) mod complexity;
pub mod css;
pub mod flags;
pub mod html;
pub mod mdx;
mod parse;
pub mod sfc;
mod sfc_template;
pub mod suppress;
mod template_usage;
pub mod visitor;

use std::path::Path;

use rayon::prelude::*;

use cache::CacheStore;
use fallow_types::discover::{DiscoveredFile, FileId};

// Re-export all extract types from fallow-types
pub use fallow_types::extract::{
    DynamicImportInfo, DynamicImportPattern, ExportInfo, ExportName, ImportInfo, ImportedName,
    MemberAccess, MemberInfo, MemberKind, ModuleInfo, ParseResult, ReExportInfo, RequireCallInfo,
    compute_line_offsets,
};

// Re-export extraction functions for internal use and fuzzing
pub use astro::extract_astro_frontmatter;
pub use css::extract_css_module_exports;
pub use mdx::extract_mdx_statements;
pub use sfc::{extract_sfc_scripts, is_sfc_file};
pub use sfc_template::angular::ANGULAR_TPL_SENTINEL;

use parse::parse_source_to_module;

/// Parse all files in parallel, extracting imports and exports.
/// Uses the cache to skip reparsing files whose content hasn't changed.
///
/// When `need_complexity` is true, per-function cyclomatic/cognitive complexity
/// metrics are computed during parsing (needed by the `health` command).
/// Pass `false` for dead-code analysis where complexity data is unused.
pub fn parse_all_files(
    files: &[DiscoveredFile],
    cache: Option<&CacheStore>,
    need_complexity: bool,
) -> ParseResult {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let cache_hits = AtomicUsize::new(0);
    let cache_misses = AtomicUsize::new(0);

    let modules: Vec<ModuleInfo> = files
        .par_iter()
        .filter_map(|file| {
            parse_single_file_cached(file, cache, &cache_hits, &cache_misses, need_complexity)
        })
        .collect();

    let hits = cache_hits.load(Ordering::Relaxed);
    let misses = cache_misses.load(Ordering::Relaxed);
    if hits > 0 || misses > 0 {
        tracing::info!(
            cache_hits = hits,
            cache_misses = misses,
            "incremental cache stats"
        );
    }

    ParseResult {
        modules,
        cache_hits: hits,
        cache_misses: misses,
    }
}

/// Extract mtime (seconds since epoch) from file metadata.
/// Returns 0 if mtime cannot be determined (pre-epoch, unsupported OS, etc.).
fn mtime_secs(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// Parse a single file, consulting the cache first.
///
/// Cache validation strategy (fast path -> slow path):
/// 1. `stat()` the file to get mtime + size (single syscall, no file read)
/// 2. If mtime+size match the cached entry -> cache hit, return immediately
/// 3. If mtime+size differ -> read file, compute content hash
/// 4. If content hash matches cached entry -> cache hit (file was `touch`ed but unchanged)
/// 5. Otherwise -> cache miss, full parse
fn parse_single_file_cached(
    file: &DiscoveredFile,
    cache: Option<&CacheStore>,
    cache_hits: &std::sync::atomic::AtomicUsize,
    cache_misses: &std::sync::atomic::AtomicUsize,
    need_complexity: bool,
) -> Option<ModuleInfo> {
    use std::sync::atomic::Ordering;

    // Fast path: check mtime+size before reading file content.
    // A single stat() syscall is ~100x cheaper than read()+hash().
    if let Some(store) = cache
        && let Ok(metadata) = std::fs::metadata(&file.path)
    {
        let mt = mtime_secs(&metadata);
        let sz = metadata.len();
        if let Some(cached) = store.get_by_metadata(&file.path, mt, sz) {
            // When complexity is requested but the cached entry lacks it
            // (populated by a prior `check` run), skip the cache and re-parse.
            if !need_complexity || !cached.complexity.is_empty() {
                cache_hits.fetch_add(1, Ordering::Relaxed);
                return Some(cache::cached_to_module(cached, file.id));
            }
        }
    }

    // Slow path: read file content and compute content hash.
    let source = std::fs::read_to_string(&file.path).ok()?;
    let content_hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());

    // Check cache by content hash (handles touch/save-without-change)
    if let Some(store) = cache
        && let Some(cached) = store.get(&file.path, content_hash)
        && (!need_complexity || !cached.complexity.is_empty())
    {
        cache_hits.fetch_add(1, Ordering::Relaxed);
        return Some(cache::cached_to_module(cached, file.id));
    }
    cache_misses.fetch_add(1, Ordering::Relaxed);

    // Cache miss — do a full parse
    Some(parse_source_to_module(
        file.id,
        &file.path,
        &source,
        content_hash,
        need_complexity,
    ))
}

/// Parse a single file and extract module information (without complexity).
#[must_use]
pub fn parse_single_file(file: &DiscoveredFile) -> Option<ModuleInfo> {
    let source = std::fs::read_to_string(&file.path).ok()?;
    let content_hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
    Some(parse_source_to_module(
        file.id,
        &file.path,
        &source,
        content_hash,
        false,
    ))
}

/// Parse from in-memory content (for LSP, includes complexity).
#[must_use]
pub fn parse_from_content(file_id: FileId, path: &Path, content: &str) -> ModuleInfo {
    let content_hash = xxhash_rust::xxh3::xxh3_64(content.as_bytes());
    parse_source_to_module(file_id, path, content, content_hash, true)
}

// Parser integration tests invoke Oxc under Miri which is ~1000x slower.
// Unit tests in individual modules (visitor, suppress, sfc, css, etc.) still run.
#[cfg(all(test, not(miri)))]
mod tests;
