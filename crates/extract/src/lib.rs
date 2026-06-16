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
    ClassHeritageInfo, DynamicImportInfo, DynamicImportPattern, ExportInfo, ExportName, ImportInfo,
    ImportedName, LocalTypeDeclaration, MemberAccess, MemberInfo, MemberKind, ModuleInfo,
    ParseResult, PublicSignatureTypeReference, ReExportInfo, RequireCallInfo, VisibilityTag,
    compute_line_offsets,
};

pub use astro::extract_astro_frontmatter;
pub use css::{
    ThemeScan, ThemeTokenDef, extract_apply_tokens, extract_css_module_exports, scan_theme_blocks,
};
pub use css_classes::{
    MarkupClassScan, MarkupClassToken, is_edit_distance_one, is_typo_edit, scan_markup_class_tokens,
};
pub use css_metrics::compute_css_analytics;
pub use glimmer::{is_glimmer_file, strip_glimmer_templates};
pub use mdx::extract_mdx_statements;
pub use sfc::{extract_sfc_scripts, extract_sfc_styles, is_sfc_file};
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

/// Synthetic member-access object used to carry exported-instance bindings.
///
/// `MemberAccess { object: format!("{INSTANCE_EXPORT_SENTINEL}{export_name}"), member: target }`
/// means the exported value named `export_name` is an instance of the local
/// class/interface symbol named `target`.
pub const INSTANCE_EXPORT_SENTINEL: &str = "__fallow_instance_export__:";

/// Synthetic member-access object prefix for typed Playwright fixtures.
///
/// `MemberAccess { object: format!("{PLAYWRIGHT_FIXTURE_DEF_SENTINEL}{test}:{fixture}"), member: type_name }`
/// means the exported Playwright test object named `test` provides a fixture
/// named `fixture` whose declared type is `type_name`.
pub const PLAYWRIGHT_FIXTURE_DEF_SENTINEL: &str = "__fallow_playwright_fixture_def__:";

/// Synthetic member-access object prefix for Playwright fixture wrapper aliases.
///
/// `MemberAccess { object: format!("{PLAYWRIGHT_FIXTURE_ALIAS_SENTINEL}{alias}:"), member: base }`
/// means the exported Playwright test object named `alias` inherits fixture
/// definitions from the exported test object named `base`.
pub const PLAYWRIGHT_FIXTURE_ALIAS_SENTINEL: &str = "__fallow_playwright_fixture_alias__:";

/// Synthetic member-access object prefix for Playwright fixture member uses.
///
/// `MemberAccess { object: format!("{PLAYWRIGHT_FIXTURE_USE_SENTINEL}{test}:{fixture}"), member }`
/// means a callback passed to the Playwright test object named `test`
/// destructures `fixture` and accesses `fixture.member`.
pub const PLAYWRIGHT_FIXTURE_USE_SENTINEL: &str = "__fallow_playwright_fixture_use__:";

/// Synthetic member-access object prefix for exported Playwright fixture type aliases.
///
/// `MemberAccess { object: format!("{PLAYWRIGHT_FIXTURE_TYPE_SENTINEL}{alias}:{fixture_path}"), member: type_name }`
/// means a local type alias named `alias` contains a nested fixture path whose
/// declared type is `type_name`. The analyze layer uses this when a Playwright
/// fixture generic imports an object type alias from another module.
pub const PLAYWRIGHT_FIXTURE_TYPE_SENTINEL: &str = "__fallow_playwright_fixture_type__:";

/// Synthetic member-access object prefix for static-factory call returns.
///
/// `MemberAccess { object: format!("{FACTORY_CALL_SENTINEL}{callee}:{method}"), member }`
/// means a local binding was assigned from `<callee>.<method>()` and a member
/// is accessed on the result. The analyze layer resolves `callee` through the
/// consumer module's imports to a class export and credits `member` on the
/// class when the matching method carries `is_instance_returning_static`.
/// See issue #346.
pub const FACTORY_CALL_SENTINEL: &str = "__fallow_factory_call__:";

/// Synthetic member-access object prefix for fluent-builder chain credit.
///
/// `MemberAccess { object: format!("{FLUENT_CHAIN_SENTINEL}{callee}:{root_method}:{chain}"), member }`
/// means a fluent chain `<callee>.<root_method>().<...chain>.<member>` was
/// observed. `chain` is a comma-separated list of method names (empty when
/// `member` is the first chained call after `root_method`). The analyze layer
/// resolves `callee` to a class export, validates `root_method` has
/// `is_instance_returning_static`, walks each `chain` segment requiring
/// `is_self_returning` on the class, and credits `member` on the class
/// when the chain remains on the class type. See issue #387.
pub const FLUENT_CHAIN_SENTINEL: &str = "__fallow_fluent_chain__:";

/// Synthetic member-access object prefix for fluent chains rooted at a `new`
/// expression.
///
/// `MemberAccess { object: format!("{FLUENT_CHAIN_NEW_SENTINEL}{class}:{chain}"), member }`
/// means a chain `new <class>(...).<...chain>.<member>` was observed. Unlike
/// `FLUENT_CHAIN_SENTINEL`, there is no root method: a constructor always
/// returns an instance of `class`, so no `is_instance_returning_static` check
/// applies. `chain` is a comma-separated list of the intermediate method names
/// between the constructor and `member` (it always contains at least the first
/// method, which must be `is_self_returning` to reach `member`). The analyze
/// layer resolves `class` to a class export, requires every `chain` segment to
/// be `is_self_returning` on the class, and credits `member` on the class.
/// The first method directly off the constructor is credited separately via
/// the `static_member_object_name` `NewExpression` arm. See issue #605.
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
        let mt = mtime_secs(&metadata);
        let sz = metadata.len();
        if let Some(cached) = store.get_by_metadata(&file.path, mt, sz)
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

/// Extract mtime (seconds since epoch) from file metadata.
/// Returns 0 if mtime cannot be determined (pre-epoch, unsupported OS, etc.).
fn mtime_secs(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
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
