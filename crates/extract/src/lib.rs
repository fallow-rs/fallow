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
    AngularTemplateMemberAccessFact, AngularThisSpreadFact, ClassHeritageInfo, DynamicImportInfo,
    DynamicImportPattern, ExportInfo, ExportName, FactoryCallMemberAccessFact,
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
    ThemeScan, ThemeTokenDef, extract_apply_tokens, extract_css_module_exports, scan_theme_blocks,
};
pub use css_classes::{
    MarkupClassScan, MarkupClassToken, is_edit_distance_one, is_typo_edit, scan_markup_class_tokens,
};
pub use css_metrics::compute_css_analytics;
pub use glimmer::{is_glimmer_file, strip_glimmer_templates};
pub use mdx::extract_mdx_statements;
pub use sfc::{
    SourceRegion, extract_sfc_scripts, extract_sfc_styles, extract_sfc_template_regions,
    is_sfc_file,
};
pub use sfc_css::{scoped_unused_classes, sfc_virtual_stylesheet};
pub use sfc_template::angular::{ANGULAR_THIS_SPREAD_SENTINEL, ANGULAR_TPL_SENTINEL};
pub use tailwind::{TailwindArbitraryUse, scan_tailwind_arbitrary_values};

#[expect(
    clippy::expect_used,
    reason = "static regex patterns are hard-coded analyzer invariants covered by extraction tests"
)]
pub(crate) fn static_regex(pattern: &str) -> regex::Regex {
    regex::Regex::new(pattern).expect("static regex pattern should compile")
}

/// Legacy member-access object prefix for exported-instance bindings.
///
/// New extraction writes [`SemanticFact::InstanceExportBinding`]. The prefix
/// remains available so analysis can decode older cache entries that used
/// `MemberAccess.object` as a string protocol.
pub const INSTANCE_EXPORT_SENTINEL: &str = "__fallow_instance_export__:";

/// Legacy member-access object prefix for typed Playwright fixture definitions.
///
/// New extraction writes [`SemanticFact::PlaywrightFixtureDefinition`]. The
/// prefix remains available so analysis can decode older cache entries that
/// used `MemberAccess.object` as a string protocol.
pub const PLAYWRIGHT_FIXTURE_DEF_SENTINEL: &str = "__fallow_playwright_fixture_def__:";

/// Legacy member-access object prefix for Playwright fixture wrapper aliases.
///
/// New extraction writes [`SemanticFact::PlaywrightFixtureAlias`]. The prefix
/// remains available so analysis can decode older cache entries that used
/// `MemberAccess.object` as a string protocol.
pub const PLAYWRIGHT_FIXTURE_ALIAS_SENTINEL: &str = "__fallow_playwright_fixture_alias__:";

/// Legacy member-access object prefix for Playwright fixture member uses.
///
/// New extraction writes [`SemanticFact::PlaywrightFixtureUse`]. The prefix
/// remains available so analysis can decode older cache entries that used
/// `MemberAccess.object` as a string protocol.
pub const PLAYWRIGHT_FIXTURE_USE_SENTINEL: &str = "__fallow_playwright_fixture_use__:";

/// Legacy member-access object prefix for exported Playwright fixture type aliases.
///
/// New extraction writes [`SemanticFact::PlaywrightFixtureType`]. The prefix
/// remains available so analysis can decode older cache entries that used
/// `MemberAccess.object` as a string protocol.
pub const PLAYWRIGHT_FIXTURE_TYPE_SENTINEL: &str = "__fallow_playwright_fixture_type__:";

/// Legacy member-access object prefix for static-factory call returns.
///
/// New extraction writes [`SemanticFact::FactoryCallMemberAccess`]. The prefix
/// remains available so analysis can decode older cache entries that used
/// `MemberAccess.object` as a string protocol. See issue #346.
pub const FACTORY_CALL_SENTINEL: &str = "__fallow_factory_call__:";

/// Legacy member-access object prefix for fluent-builder chain credit.
///
/// New extraction writes [`SemanticFact::FluentChainMemberAccess`]. The prefix
/// remains available so analysis can decode older cache entries that used
/// `MemberAccess.object` as a string protocol. See issue #387.
pub const FLUENT_CHAIN_SENTINEL: &str = "__fallow_fluent_chain__:";

/// Legacy member-access object prefix for fluent chains rooted at a `new`
/// expression.
///
/// New extraction writes [`SemanticFact::FluentChainNewMemberAccess`]. The
/// prefix remains available so analysis can decode older cache entries that
/// used `MemberAccess.object` as a string protocol. See issue #605.
pub const FLUENT_CHAIN_NEW_SENTINEL: &str = "__fallow_fluent_chain_new__:";

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
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    let cache_hits = AtomicUsize::new(0);
    let cache_misses = AtomicUsize::new(0);
    let parse_cpu_nanos = AtomicU64::new(0);

    let modules: Vec<ModuleInfo> = files
        .par_iter()
        .filter_map(|file| {
            parse_single_file_cached(
                file,
                cache,
                &cache_hits,
                &cache_misses,
                &parse_cpu_nanos,
                need_complexity,
            )
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
        parse_cpu_ms: parse_cpu_nanos.load(Ordering::Relaxed) as f64 / 1_000_000.0,
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
    cache_hits: &std::sync::atomic::AtomicUsize,
    cache_misses: &std::sync::atomic::AtomicUsize,
    parse_cpu_nanos: &std::sync::atomic::AtomicU64,
    need_complexity: bool,
) -> Option<ModuleInfo> {
    use std::sync::atomic::Ordering;

    if let Some(store) = cache
        && let Ok(metadata) = std::fs::metadata(&file.path)
    {
        let fingerprint =
            fallow_types::source_fingerprint::SourceFingerprint::from_metadata(&metadata);
        if let Some(cached) = store.get_by_metadata(&file.path, fingerprint)
            && (!need_complexity || !cached.complexity.is_empty())
        {
            cache_hits.fetch_add(1, Ordering::Relaxed);
            return Some(cache::cached_to_module_opts(
                cached,
                file.id,
                need_complexity,
            ));
        }
    }

    let raw = std::fs::read_to_string(&file.path).ok()?;
    let source = strip_bom(&raw);
    let content_hash = xxhash_rust::xxh3::xxh3_64(source.as_bytes());

    if let Some(store) = cache
        && let Some(cached) = store.get(&file.path, content_hash)
        && (!need_complexity || !cached.complexity.is_empty())
    {
        cache_hits.fetch_add(1, Ordering::Relaxed);
        return Some(cache::cached_to_module_opts(
            cached,
            file.id,
            need_complexity,
        ));
    }
    cache_misses.fetch_add(1, Ordering::Relaxed);

    let parse_start = std::time::Instant::now();
    let module = parse_source_to_module(file.id, &file.path, source, content_hash, need_complexity);
    parse_cpu_nanos.fetch_add(
        u64::try_from(parse_start.elapsed().as_nanos()).unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
    Some(module)
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
