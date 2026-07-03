//! Parsing and extraction engine for fallow codebase intelligence.
//!
//! This crate handles all file parsing: JS/TS via Oxc, Vue/Svelte SFC extraction,
//! Astro frontmatter, MDX import/export extraction, CSS Module class name extraction,
//! HTML asset reference extraction, and incremental caching of parse results.

#![warn(missing_docs)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

mod asset_url;
pub mod astro;
pub mod cache;
pub(crate) mod complexity;
pub mod css;
pub mod css_classes;
pub mod css_in_js;
pub mod css_metrics;
pub mod flags;
pub mod glimmer;
pub mod graphql;
pub mod html;
pub mod iconify;
pub mod inventory;
pub mod mdx;
mod module_info;
mod parse;
pub mod sfc;
pub mod sfc_css;
mod sfc_props;
mod sfc_template;
mod source_map;
pub mod suppress;
/// Tailwind CSS arbitrary-value detection.
pub mod tailwind;
pub(crate) mod template_complexity;
mod template_usage;
/// Visitor utilities for AST extraction.
pub mod visitor;

use std::path::Path;

use rayon::prelude::*;

use cache::CacheStore;
use fallow_types::discover::{DiscoveredFile, FileId};

pub use fallow_types::extract::{
    AngularTemplateMemberAccessFact, AngularThisSpreadFact, ClassHeritageInfo,
    DynamicCustomElementRenderFact, DynamicImportInfo, DynamicImportPattern, ExportInfo,
    ExportName, FactoryCallMemberAccessFact, FactoryFnMemberAccessFact, FactoryReturnExport,
    FluentChainMemberAccessFact, FluentChainNewMemberAccessFact, ImportInfo, ImportedName,
    InstanceExportBindingFact, LocalTypeDeclaration, MemberAccess, MemberInfo, MemberKind,
    ModuleInfo, ParseResult, PlaywrightFixtureAliasFact, PlaywrightFixtureDefinitionFact,
    PlaywrightFixtureTypeFact, PlaywrightFixtureUseFact, PublicSignatureTypeReference,
    ReExportInfo, RequireCallInfo, SemanticFact, VisibilityTag, compute_line_offsets,
};

pub use astro::{
    extract_astro_frontmatter, extract_astro_style_regions, extract_astro_template_regions,
};
pub use css::{
    ThemeScan, ThemeTokenDef, extract_apply_tokens, extract_apply_tokens_located,
    extract_css_module_exports, extract_css_var_reads_located, scan_theme_blocks,
};
pub use css_classes::{
    MarkupClassScan, MarkupClassToken, is_edit_distance_one, is_typo_edit, scan_markup_class_tokens,
};
pub use css_in_js::{
    CssInJsObjectSheets, CssInJsToken, CssInJsTokenDef, CssInJsTokenOrigin, TokenConsumerHit,
    css_in_js_object_sheets, css_in_js_theme_consumers, css_in_js_theme_token_defs,
    css_in_js_token_consumers, css_in_js_token_defs, css_in_js_virtual_stylesheet,
    panda_style_value_consumers, panda_token_call_consumers,
};
pub use css_metrics::{compute_css_analytics, parse_css_color_rgb};
pub use glimmer::{is_glimmer_file, strip_glimmer_templates};
pub use mdx::extract_mdx_statements;
pub use sfc::{
    SourceRegion, extract_sfc_scripts, extract_sfc_styles, extract_sfc_template_regions,
    is_sfc_file,
};
pub use sfc_css::{
    scoped_unused_classes, sfc_preprocessor_virtual_stylesheet, sfc_virtual_stylesheet,
};
pub use tailwind::{TailwindArbitraryUse, scan_tailwind_arbitrary_values};

#[expect(
    clippy::expect_used,
    reason = "static regex patterns are hard-coded analyzer invariants covered by extraction tests"
)]
pub(crate) fn static_regex(pattern: &str) -> regex::Regex {
    regex::Regex::new(pattern).expect("static regex pattern should compile")
}

pub use parse::parse_source_to_module;

/// Leading UTF-8 byte order mark codepoint.
///
/// Windows editors (Notepad, older VS settings, some IDE plugins) emit a UTF-8
/// BOM at the start of source files. fallow's contract is "UTF-8 with or
/// without BOM; line offsets are computed against the post-BOM view; the BOM,
/// if present on input, is preserved on output by `fallow fix`."
const BOM_CHAR: char = '\u{FEFF}';

/// Strip the leading UTF-8 BOM if present.
///
/// Called at every file-read entry point in this crate so the rest of the
/// pipeline (content hash, `compute_line_offsets`, oxc parser, downstream
/// analyses) sees a consistent post-BOM view. Mirrors the
/// `fallow_config` layer (`config_writer.rs::BOM`) so config-shaped sources
/// and source-code-shaped sources are processed symmetrically. See issue #475.
#[must_use]
pub(crate) fn strip_bom(source: &str) -> &str {
    source.strip_prefix(BOM_CHAR).unwrap_or(source)
}

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
    let results: Vec<ParseFileResult> = files
        .par_iter()
        .map(|file| parse_single_file_cached(file, cache, need_complexity))
        .collect();

    let mut modules = Vec::with_capacity(results.len());
    let mut hits = 0usize;
    let mut misses = 0usize;
    let mut parse_cpu_nanos = 0u64;

    for result in results {
        hits += result.cache_hits;
        misses += result.cache_misses;
        parse_cpu_nanos = parse_cpu_nanos.saturating_add(result.parse_cpu_nanos);
        if let Some(module) = result.module {
            modules.push(module);
        }
    }

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
        parse_cpu_ms: parse_cpu_nanos as f64 / 1_000_000.0,
    }
}

struct ParseFileResult {
    module: Option<ModuleInfo>,
    cache_hits: usize,
    cache_misses: usize,
    parse_cpu_nanos: u64,
}

impl ParseFileResult {
    fn cache_hit(module: ModuleInfo) -> Self {
        Self {
            module: Some(module),
            cache_hits: 1,
            cache_misses: 0,
            parse_cpu_nanos: 0,
        }
    }

    fn cache_miss(module: ModuleInfo, parse_cpu_nanos: u64) -> Self {
        Self {
            module: Some(module),
            cache_hits: 0,
            cache_misses: 1,
            parse_cpu_nanos,
        }
    }

    const fn skipped() -> Self {
        Self {
            module: None,
            cache_hits: 0,
            cache_misses: 0,
            parse_cpu_nanos: 0,
        }
    }
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
    need_complexity: bool,
) -> ParseFileResult {
    let cached_by_path = cache.and_then(|store| store.get_by_path_only(&file.path));

    if let Some(cached) = cached_by_path
        && cached.file_size == file.size_bytes
        && let Ok(metadata) = std::fs::metadata(&file.path)
        && metadata.len() == cached.file_size
    {
        let fingerprint =
            fallow_types::source_fingerprint::SourceFingerprint::from_metadata(&metadata);
        if cached.source_fingerprint() == fingerprint
            && fingerprint.has_known_mtime()
            && (!need_complexity || !cached.complexity.is_empty())
        {
            return ParseFileResult::cache_hit(cache::cached_to_module_opts(
                cached,
                file.id,
                need_complexity,
            ));
        }
    }

    let Ok(raw) = std::fs::read_to_string(&file.path) else {
        return ParseFileResult::skipped();
    };
    let source = strip_bom(&raw);
    let content_hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());

    if let Some(cached) = cached_by_path
        && cached.content_hash == content_hash
        && (!need_complexity || !cached.complexity.is_empty())
    {
        return ParseFileResult::cache_hit(cache::cached_to_module_opts(
            cached,
            file.id,
            need_complexity,
        ));
    }

    let parse_start = std::time::Instant::now();
    let module = parse_source_to_module(file.id, &file.path, source, content_hash, need_complexity);
    let parse_cpu_nanos = u64::try_from(parse_start.elapsed().as_nanos()).unwrap_or(u64::MAX);
    ParseFileResult::cache_miss(module, parse_cpu_nanos)
}

/// Parse a single file and extract module information (without complexity).
#[must_use]
pub fn parse_single_file(file: &DiscoveredFile) -> Option<ModuleInfo> {
    let raw = std::fs::read_to_string(&file.path).ok()?;
    let source = strip_bom(&raw);
    let content_hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());
    Some(parse_source_to_module(
        file.id,
        &file.path,
        source,
        content_hash,
        false,
    ))
}

/// Parse from in-memory content (for LSP, includes complexity).
#[must_use]
pub fn parse_from_content(file_id: FileId, path: &Path, content: &str) -> ModuleInfo {
    let content = strip_bom(content);
    let content_hash = xxhash_rust::xxh3::xxh3_64(content.as_bytes());
    parse_source_to_module(file_id, path, content, content_hash, true)
}

#[cfg(all(test, not(miri)))]
mod tests;
