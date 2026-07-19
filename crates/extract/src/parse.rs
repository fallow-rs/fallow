use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{Comment, Program};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::ExportInfo;
use crate::ModuleInfo;
use crate::astro::{is_astro_file, parse_astro_to_module};
use crate::css::{is_css_file, parse_css_to_module};
use crate::glimmer::{is_glimmer_file, strip_glimmer_templates};
use crate::graphql::{is_graphql_file, parse_graphql_to_module};
use crate::html::{is_html_file, parse_html_to_module_with_complexity};
use crate::mdx::{is_mdx_file, parse_mdx_to_module};
use crate::sfc::{is_sfc_file, parse_sfc_to_module};
use crate::visitor::{ModuleInfoExtractor, RouteLoadHarvestMode};
use fallow_types::discover::FileId;
use fallow_types::extract::{FlagUse, FunctionComplexity, ImportInfo, VisibilityTag};

struct JsxRetryParse {
    extractor: ModuleInfoExtractor,
    semantic_usage: SemanticUsage,
    complexity: Vec<FunctionComplexity>,
    flag_uses: Vec<FlagUse>,
    parsed_suppressions: crate::suppress::ParsedSuppressions,
}

fn source_type_for_path(path: &Path) -> SourceType {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("gts") => SourceType::ts(),
        Some("gjs") => SourceType::mjs(),
        _ => SourceType::from_path(path).unwrap_or_default(),
    }
}

/// Parse source text into a [`ModuleInfo`].
///
/// When `need_complexity` is false the per-function complexity visitor is
/// skipped, saving one full AST walk per file.  The dead-code analysis
/// pipeline never consumes complexity data, so callers that only need
/// imports/exports should pass `false`.
pub fn parse_source_to_module(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    let mut module =
        parse_source_to_module_inner(file_id, path, source, content_hash, need_complexity);
    module.iconify_prefixes = crate::iconify::extract_iconify_prefixes(path, source);
    module.iconify_icon_names = crate::iconify::extract_iconify_icon_names(path, source);
    // Keep this post-parse guard as defense in depth. The extractor is also
    // mode-gated before the AST walk, so incompatible route producer names never
    // enter the shared cached field in the first place.
    if route_load_harvest_mode_for_path(path) == RouteLoadHarvestMode::None {
        module.load_return_keys = Vec::new();
        module.has_unharvestable_load = false;
    }
    module
}

/// Whether a file is a SvelteKit page-load producer:
/// `+page.{ts,server.ts,js,server.js}`. Layout loads (`+layout(.server).{ts,js}`)
/// are out of scope for v1 (cut A). The leading `+` is a SvelteKit-only
/// filename convention, so no ordinary module matches.
fn is_sveltekit_page_load_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    matches!(
        name,
        "+page.ts" | "+page.server.ts" | "+page.js" | "+page.server.js"
    )
}

fn route_load_harvest_mode_for_path(path: &Path) -> RouteLoadHarvestMode {
    if is_sveltekit_page_load_file(path) {
        return RouteLoadHarvestMode::SvelteKitPage;
    }
    if is_conventional_route_loader_file(path) {
        return RouteLoadHarvestMode::ConventionalRoute;
    }
    RouteLoadHarvestMode::None
}

fn is_conventional_route_loader_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('+') {
        return false;
    }
    if !matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("ts" | "tsx" | "js" | "jsx")
    ) {
        return false;
    }
    if matches!(name, "root.ts" | "root.tsx" | "root.js" | "root.jsx")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|part| part.to_str())
            .is_some_and(|part| matches!(part, "app" | "src"))
    {
        return true;
    }
    path_has_route_dir(path, "app") || path_has_route_dir(path, "src")
}

fn path_has_route_dir(path: &Path, app_dir: &str) -> bool {
    let mut previous = None;
    for part in path.components().filter_map(|c| c.as_os_str().to_str()) {
        if previous == Some(app_dir) && part == "routes" {
            return true;
        }
        previous = Some(part);
    }
    false
}

fn parse_source_to_module_inner(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    let source = crate::strip_bom(source);
    if let Some(module) =
        parse_non_js_source_to_module(file_id, path, source, content_hash, need_complexity)
    {
        return module;
    }

    let stripped_glimmer_source = is_glimmer_file(path)
        .then(|| strip_glimmer_templates(source))
        .flatten();
    let parser_source = stripped_glimmer_source.as_deref().unwrap_or(source);
    let source_type = source_type_for_path(path);
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, parser_source, source_type).parse();

    let mut parsed_suppressions =
        crate::suppress::parse_suppressions(&parser_return.program.comments, source);

    let (mut extractor, mut semantic_usage) =
        build_primary_extractor(&parser_return.program, path, source, source_type);

    let line_offsets = fallow_types::extract::compute_line_offsets(source);

    let (mut complexity, mut flag_uses) = compute_primary_complexity_and_flags(
        &parser_return.program,
        parser_source,
        &extractor.inline_template_findings,
        &line_offsets,
        need_complexity,
    );

    apply_jsx_retry_or_jsdoc(
        &JsxRetryOrJsdocInput {
            path,
            parser_source,
            source_type,
            need_complexity,
            line_offsets: &line_offsets,
            comments: &parser_return.program.comments,
            source,
        },
        &mut ParseOutputs {
            extractor: &mut extractor,
            semantic_usage: &mut semantic_usage,
            complexity: &mut complexity,
            flag_uses: &mut flag_uses,
            parsed_suppressions: &mut parsed_suppressions,
        },
    );

    assemble_module_info(ModuleAssemblyInput {
        extractor,
        file_id,
        content_hash,
        parsed_suppressions,
        semantic_usage,
        line_offsets,
        complexity,
        flag_uses,
    })
}

/// Inputs shared by the JSX retry and the fallback JSDoc enrichment pass.
struct JsxRetryOrJsdocInput<'a> {
    path: &'a Path,
    parser_source: &'a str,
    source_type: SourceType,
    need_complexity: bool,
    line_offsets: &'a [u32],
    comments: &'a [Comment],
    source: &'a str,
}

struct ModuleAssemblyInput {
    extractor: ModuleInfoExtractor,
    file_id: FileId,
    content_hash: u64,
    parsed_suppressions: crate::suppress::ParsedSuppressions,
    semantic_usage: SemanticUsage,
    line_offsets: Vec<u32>,
    complexity: Vec<FunctionComplexity>,
    flag_uses: Vec<FlagUse>,
}

/// Build the primary extractor: run the AST walk (JSX-gated), fold in Glimmer
/// template usage, and compute import-binding semantic usage.
fn build_primary_extractor(
    program: &Program<'_>,
    path: &Path,
    source: &str,
    source_type: SourceType,
) -> (ModuleInfoExtractor, SemanticUsage) {
    let mut extractor = ModuleInfoExtractor::new();
    extractor.set_route_load_harvest_mode(route_load_harvest_mode_for_path(path));
    // Gate the React/JSX structural walk on a JSX-capable parse so it is a
    // no-op on non-JSX files (perf: the `audit` hot path on non-React repos
    // must not regress).
    extractor.jsx_capable = source_type.is_jsx();
    extractor.visit_program(program);
    extractor.resolve_pending_local_export_specifiers();

    let template_used_imports =
        collect_glimmer_template_into_extractor(&mut extractor, path, source);
    let semantic_usage =
        compute_semantic_usage(program, &extractor.imports, &template_used_imports);
    (extractor, semantic_usage)
}

/// Compute per-function complexity (with inline-template findings folded in) and
/// feature-flag uses for the primary parse, honoring `need_complexity`.
fn compute_primary_complexity_and_flags(
    program: &Program<'_>,
    parser_source: &str,
    inline_template_findings: &[crate::visitor::InlineTemplateFinding],
    line_offsets: &[u32],
    need_complexity: bool,
) -> (Vec<FunctionComplexity>, Vec<FlagUse>) {
    let mut complexity = if need_complexity {
        crate::complexity::compute_complexity(program, parser_source, line_offsets)
    } else {
        Vec::new()
    };
    if need_complexity {
        append_inline_template_complexity(&mut complexity, inline_template_findings, line_offsets);
    }

    let flag_uses = crate::flags::extract_flags(
        program,
        line_offsets,
        &[],   // built-in patterns only at parse time
        &[],   // built-in prefixes only at parse time
        false, // config object heuristics off at parse time (opt-in via config)
    );
    (complexity, flag_uses)
}

/// Mutable references to the primary-parse outputs a JSX retry replaces wholesale.
struct ParseOutputs<'a> {
    extractor: &'a mut ModuleInfoExtractor,
    semantic_usage: &'a mut SemanticUsage,
    complexity: &'a mut Vec<FunctionComplexity>,
    flag_uses: &'a mut Vec<FlagUse>,
    parsed_suppressions: &'a mut crate::suppress::ParsedSuppressions,
}

/// Run the JSX retry parse: when it improves extraction, overwrite every
/// primary-parse output in place; otherwise apply JSDoc tags to the primary
/// extractor. The retry's own parse already applies JSDoc tags.
fn apply_jsx_retry_or_jsdoc(input: &JsxRetryOrJsdocInput<'_>, outputs: &mut ParseOutputs<'_>) {
    let retry_input = JsxRetryInput {
        path: input.path,
        source: input.source,
        parser_source: input.parser_source,
        source_type: input.source_type,
        total_extracted: outputs.extractor.exports.len()
            + outputs.extractor.imports.len()
            + outputs.extractor.re_exports.len(),
        need_complexity: input.need_complexity,
        line_offsets: input.line_offsets,
    };
    let Some(retry) = parse_with_jsx_retry(&retry_input) else {
        apply_jsdoc_tags_to_extractor(&mut *outputs.extractor, input.comments, input.source);
        return;
    };
    *outputs.extractor = retry.extractor;
    *outputs.semantic_usage = retry.semantic_usage;
    *outputs.complexity = retry.complexity;
    *outputs.flag_uses = retry.flag_uses;
    *outputs.parsed_suppressions = retry.parsed_suppressions;
}

/// Apply JSDoc visibility tags and JSDoc `import()` type references to the
/// extractor's exports/imports for the primary (non-retry) parse.
fn apply_jsdoc_tags_to_extractor(
    extractor: &mut ModuleInfoExtractor,
    comments: &[Comment],
    source: &str,
) {
    apply_jsdoc_visibility_tags(&mut extractor.exports, comments, source);
    extract_jsdoc_import_types(&mut extractor.imports, comments, source);
}

/// Convert the finalized extractor into a `ModuleInfo`, attaching semantic-usage,
/// line-offset, complexity, and flag-use side data.
fn assemble_module_info(input: ModuleAssemblyInput) -> ModuleInfo {
    let ModuleAssemblyInput {
        extractor,
        file_id,
        content_hash,
        parsed_suppressions,
        semantic_usage,
        line_offsets,
        complexity,
        flag_uses,
    } = input;
    let mut info = extractor.into_module_info(file_id, content_hash, parsed_suppressions);
    info.unused_import_bindings = semantic_usage.import_binding_usage.unused;
    info.type_referenced_import_bindings = semantic_usage.import_binding_usage.type_referenced;
    info.value_referenced_import_bindings = semantic_usage.import_binding_usage.value_referenced;
    info.auto_import_candidates = semantic_usage.auto_import_candidates;
    info.line_offsets = line_offsets;
    info.complexity = complexity;
    info.flag_uses = flag_uses;
    info
}

struct JsxRetryInput<'a> {
    path: &'a Path,
    source: &'a str,
    parser_source: &'a str,
    source_type: SourceType,
    total_extracted: usize,
    need_complexity: bool,
    line_offsets: &'a [u32],
}

fn parse_with_jsx_retry(input: &JsxRetryInput<'_>) -> Option<JsxRetryParse> {
    if input.total_extracted != 0 || input.source.len() <= 100 || input.source_type.is_jsx() {
        return None;
    }

    let jsx_type = if input.source_type.is_typescript() {
        SourceType::tsx()
    } else {
        SourceType::jsx()
    };
    let allocator = Allocator::default();
    let retry_return = Parser::new(&allocator, input.parser_source, jsx_type).parse();
    let mut extractor = ModuleInfoExtractor::new();
    extractor.set_route_load_harvest_mode(route_load_harvest_mode_for_path(input.path));
    // The retry re-parses a `.js`/`.ts` file that turned out to contain JSX, so
    // the JSX structural walk applies here too.
    extractor.jsx_capable = true;
    extractor.visit_program(&retry_return.program);
    extractor.resolve_pending_local_export_specifiers();
    let retry_total =
        extractor.exports.len() + extractor.imports.len() + extractor.re_exports.len();
    if retry_total <= input.total_extracted {
        return None;
    }

    let template_used_imports =
        collect_glimmer_template_into_extractor(&mut extractor, input.path, input.source);
    let semantic_usage = compute_semantic_usage(
        &retry_return.program,
        &extractor.imports,
        &template_used_imports,
    );
    let complexity = retry_complexity(
        input.need_complexity,
        &retry_return.program,
        input.parser_source,
        input.line_offsets,
        &extractor,
    );
    let flag_uses =
        crate::flags::extract_flags(&retry_return.program, input.line_offsets, &[], &[], false);
    let parsed_suppressions =
        crate::suppress::parse_suppressions(&retry_return.program.comments, input.source);
    apply_jsdoc_visibility_tags(
        &mut extractor.exports,
        &retry_return.program.comments,
        input.source,
    );
    extract_jsdoc_import_types(
        &mut extractor.imports,
        &retry_return.program.comments,
        input.source,
    );
    Some(JsxRetryParse {
        extractor,
        semantic_usage,
        complexity,
        flag_uses,
        parsed_suppressions,
    })
}

fn retry_complexity(
    need_complexity: bool,
    program: &Program<'_>,
    parser_source: &str,
    line_offsets: &[u32],
    extractor: &ModuleInfoExtractor,
) -> Vec<FunctionComplexity> {
    if !need_complexity {
        return Vec::new();
    }
    let mut complexity =
        crate::complexity::compute_complexity(program, parser_source, line_offsets);
    append_inline_template_complexity(
        &mut complexity,
        &extractor.inline_template_findings,
        line_offsets,
    );
    complexity
}

fn parse_non_js_source_to_module(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> Option<ModuleInfo> {
    if is_sfc_file(path) {
        return Some(parse_sfc_to_module(
            file_id,
            path,
            source,
            content_hash,
            need_complexity,
        ));
    }
    if is_astro_file(path) {
        return Some(parse_astro_to_module(
            file_id,
            source,
            content_hash,
            need_complexity,
        ));
    }
    if is_mdx_file(path) {
        return Some(parse_mdx_to_module(file_id, source, content_hash));
    }
    if is_css_file(path) {
        return Some(parse_css_to_module(file_id, path, source, content_hash));
    }
    if is_graphql_file(path) {
        return Some(parse_graphql_to_module(file_id, source, content_hash));
    }
    if is_html_file(path) {
        return Some(parse_html_to_module_with_complexity(
            file_id,
            source,
            content_hash,
            need_complexity,
        ));
    }
    None
}

/// Scan Glimmer `<template>...</template>` blocks in a `.gts` / `.gjs` file
/// and fold the result directly into `extractor`. Returns the set of import
/// local names that the template body credits, so
/// `compute_import_binding_usage` can skip them when building the unused list.
///
/// Mirrors the Angular inline-template path in
/// `visitor/visit_impl.rs::visit_class`, which pushes
/// `collect_angular_template_refs(...)` results straight onto
/// `self.member_accesses`. The Glimmer scan can't run inside the JS visitor
/// because template bodies are blanked by `strip_glimmer_templates` before
/// the JS parse. The un-stripped source is only available here in
/// `parse.rs`, so this is the earliest point we can fold the result in.
///
/// `extractor.member_accesses` receives every emitted `MemberAccess`
/// (including `this.<member>` chain hops that survive even when there are
/// zero imports; class-member tracking still needs them). Bindings the
/// template credits are returned, not pushed; the caller threads them into
/// `compute_import_binding_usage`'s skip-set so the `unused` vector never
/// names them in the first place. This replaces the previous
/// `apply_glimmer_template_usage` post-construction `info` mutation and
/// the `retain` it performed against `unused_import_bindings`.
fn collect_glimmer_template_into_extractor(
    extractor: &mut ModuleInfoExtractor,
    path: &Path,
    source: &str,
) -> rustc_hash::FxHashSet<String> {
    use rustc_hash::FxHashSet;

    if !is_glimmer_file(path) {
        return FxHashSet::default();
    }
    let template_ranges = crate::glimmer::find_template_ranges(source);
    if template_ranges.is_empty() {
        return FxHashSet::default();
    }

    let imported_bindings: FxHashSet<String> = extractor
        .imports
        .iter()
        .filter(|import| !import.local_name.is_empty())
        .map(|import| import.local_name.clone())
        .collect();

    let usage = crate::sfc_template::glimmer::collect_glimmer_template_usage(
        source,
        &template_ranges,
        &imported_bindings,
    );
    extractor.member_accesses.extend(usage.member_accesses);
    usage.used_bindings
}

/// Synthesise `<template>` complexity findings for inline `@Component({ template: \`...\` })`
/// decorators captured by the visitor pass.
///
/// The template-complexity scanner returns line/col relative to the template
/// body itself; we replace those with the host file's line/col for the
/// matched `@Component`/`@Directive` decorator. Anchoring at the decorator
/// (rather than the literal's opening backtick) gives a useful jump-to-source
/// landing inside the decorator block and lets `// fallow-ignore-next-line
/// complexity` comments placed directly above the decorator suppress the
/// finding through the existing health-side check, with no extra plumbing.
fn append_inline_template_complexity(
    complexity: &mut Vec<fallow_types::extract::FunctionComplexity>,
    findings: &[crate::visitor::InlineTemplateFinding],
    line_offsets: &[u32],
) {
    for finding in findings {
        let Some(mut fc) = crate::template_complexity::compute_angular_template_complexity(
            &finding.template_source,
        ) else {
            continue;
        };
        let (line, col) =
            fallow_types::extract::byte_offset_to_line_col(line_offsets, finding.decorator_start);
        fc.line = line;
        fc.col = col;
        complexity.push(fc);
    }
}

/// Apply JSDoc visibility tags (`@public`, `@internal`, `@alpha`, `@beta`) to exports by
/// matching leading JSDoc comments.
///
/// `Comment.attached_to` points to the `export` keyword byte offset, while
/// `ExportInfo.span` stores the identifier byte offset (e.g., `foo` in
/// `export const foo`). This function bridges the gap: it collects visibility
/// comment attachment offsets with their tag, then for each export finds the
/// nearest preceding attachment point and validates it's part of the same
/// export statement.
fn apply_jsdoc_visibility_tags(exports: &mut [ExportInfo], comments: &[Comment], source: &str) {
    if exports.is_empty() || comments.is_empty() {
        return;
    }

    let mut tag_offsets = collect_jsdoc_tag_offsets(comments, source);
    if tag_offsets.is_empty() {
        return;
    }
    tag_offsets.sort_unstable_by_key(|&(offset, _, _)| offset);

    for export in exports.iter_mut() {
        apply_visibility_tag_to_export(export, &tag_offsets, source);
    }
}

/// Classify a JSDoc comment body into a visibility tag (and optional reason),
/// or `None` when no recognized tag is present.
fn classify_jsdoc_visibility_tag(text: &str) -> Option<(VisibilityTag, Option<String>)> {
    if has_public_tag(text) {
        Some((VisibilityTag::Public, None))
    } else if has_internal_tag(text) {
        Some((VisibilityTag::Internal, None))
    } else if has_alpha_tag(text) {
        Some((VisibilityTag::Alpha, None))
    } else if has_beta_tag(text) {
        Some((VisibilityTag::Beta, None))
    } else {
        let (has_expected_unused, reason) = expected_unused_tag(text);
        has_expected_unused.then_some((VisibilityTag::ExpectedUnused, reason))
    }
}

/// Collect `(attachment_offset, tag, reason)` triples for every JSDoc comment
/// that carries a recognized visibility tag.
fn collect_jsdoc_tag_offsets(
    comments: &[Comment],
    source: &str,
) -> Vec<(u32, VisibilityTag, Option<String>)> {
    let mut tag_offsets: Vec<(u32, VisibilityTag, Option<String>)> = Vec::new();
    for comment in comments {
        if !comment.is_jsdoc() {
            continue;
        }
        let content_span = comment.content_span();
        let start = content_span.start as usize;
        let end = (content_span.end as usize).min(source.len());
        if start >= end {
            continue;
        }
        if let Some((tag, reason)) = classify_jsdoc_visibility_tag(&source[start..end]) {
            tag_offsets.push((comment.attached_to, tag, reason));
        }
    }
    tag_offsets
}

/// Apply the best-matching visibility tag to a single export: an exact
/// attachment-offset hit, else the nearest preceding tag within the same
/// `export` statement prefix.
fn apply_visibility_tag_to_export(
    export: &mut ExportInfo,
    tag_offsets: &[(u32, VisibilityTag, Option<String>)],
    source: &str,
) {
    if export.span.start == 0 && export.span.end == 0 {
        return;
    }

    if let Ok(idx) = tag_offsets.binary_search_by_key(&export.span.start, |&(o, _, _)| o) {
        export.visibility = tag_offsets[idx].1;
        export
            .expected_unused_reason
            .clone_from(&tag_offsets[idx].2);
        return;
    }

    let idx = tag_offsets.partition_point(|&(o, _, _)| o <= export.span.start);
    if idx > 0 {
        let (offset, tag, ref reason) = tag_offsets[idx - 1];
        let offset = offset as usize;
        let export_start = export.span.start as usize;
        if offset < export_start && export_start <= source.len() {
            let between = &source[offset..export_start];
            if between.starts_with("export") && !between.contains(';') && !between.contains('}') {
                export.visibility = tag;
                export.expected_unused_reason.clone_from(reason);
            }
        }
    }
}

/// Check if a JSDoc comment body contains an `@internal` tag.
fn has_internal_tag(comment_text: &str) -> bool {
    for (i, _) in comment_text.match_indices("@internal") {
        let after = i + "@internal".len();
        if after >= comment_text.len() || !is_ident_char(comment_text.as_bytes()[after]) {
            return true;
        }
    }
    false
}

/// Check if a JSDoc comment body contains a `@beta` tag.
fn has_beta_tag(comment_text: &str) -> bool {
    for (i, _) in comment_text.match_indices("@beta") {
        let after = i + "@beta".len();
        if after >= comment_text.len() || !is_ident_char(comment_text.as_bytes()[after]) {
            return true;
        }
    }
    false
}

/// Check if a JSDoc comment body contains an `@alpha` tag.
fn has_alpha_tag(comment_text: &str) -> bool {
    for (i, _) in comment_text.match_indices("@alpha") {
        let after = i + "@alpha".len();
        if after >= comment_text.len() || !is_ident_char(comment_text.as_bytes()[after]) {
            return true;
        }
    }
    false
}

fn split_jsdoc_reason(rest: &str) -> Option<String> {
    for (idx, _) in rest.match_indices("--") {
        let before_ok = idx == 0
            || rest[..idx]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let after_idx = idx + 2;
        let after_ok = after_idx == rest.len()
            || rest[after_idx..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace);
        if before_ok && after_ok {
            let reason = rest[after_idx..].trim();
            return if reason.is_empty() {
                None
            } else {
                Some(reason.to_string())
            };
        }
    }

    None
}

/// Return whether an `@expected-unused` tag is present and its optional reason.
fn expected_unused_tag(comment_text: &str) -> (bool, Option<String>) {
    for (i, _) in comment_text.match_indices("@expected-unused") {
        let after = i + "@expected-unused".len();
        if after >= comment_text.len() || !is_ident_char(comment_text.as_bytes()[after]) {
            return (true, split_jsdoc_reason(&comment_text[after..]));
        }
    }
    (false, None)
}

/// Check if a byte is an identifier-continuation character (alphanumeric or `_`).
const fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Scan JSDoc comments for `import('./path').Member` type expressions and push
/// them onto `imports` as type-only imports.
///
/// JSDoc supports referencing types from other modules via `import()` expressions
/// embedded in tag annotations, e.g.:
///
/// ```js
/// /**
///  * @param foo {import('./types.js').Foo}
///  * @returns {import('./types').Bar}
///  */
/// ```
///
/// Without this scanner, the referenced export (`Foo`, `Bar`) is flagged as
/// unused because no ES `import` statement binds it. The synthesized
/// `ImportInfo` has `is_type_only: true` and an empty `local_name` so it does
/// not interfere with `compute_unused_import_bindings` (which skips imports
/// with empty local names) and does not add a cyclic-dependency edge.
///
/// All JSDoc tag contexts (`@param`, `@returns`, `@type`, `@typedef`,
/// `@callback`, etc.) use the same `{type}` annotation syntax, so scanning
/// type-bearing brace groups covers every call site without treating prose
/// examples as imports.
fn extract_jsdoc_import_types(imports: &mut Vec<ImportInfo>, comments: &[Comment], source: &str) {
    if comments.is_empty() {
        return;
    }

    for comment in comments {
        if !comment.is_jsdoc() {
            continue;
        }
        let content_span = comment.content_span();
        let start = content_span.start as usize;
        let end = (content_span.end as usize).min(source.len());
        if start >= end {
            continue;
        }
        scan_jsdoc_imports_in(&source[start..end], imports);
    }
}

/// Parse a single JSDoc comment body for `import('...').Member` expressions.
///
/// Matches both single and double quoted path literals and extracts the first
/// identifier segment after `)\.` as the imported member name. Nested member
/// access (`import('./x').ns.Foo`) yields `ns` as the imported name, which is
/// correct for fallow's syntactic analysis since the resolver still adds the
/// edge to the target module.
fn scan_jsdoc_imports_in(body: &str, imports: &mut Vec<ImportInfo>) {
    let bytes = body.as_bytes();
    let mut cursor = 0;
    // Brace-nesting stack (byte offsets of currently-open `{`) maintained
    // incrementally as the cursor advances, so each `import(` occurrence reuses
    // the enclosing-brace position instead of rescanning the whole prefix from
    // offset 0. issue #1843 follow-up: turns the per-occurrence O(prefix) rescan
    // in the old `enclosing_jsdoc_brace_start` into a single O(body) forward
    // pass over the comment while staying byte-identical.
    let mut brace_stack: Vec<usize> = Vec::new();
    let mut scanned = 0;
    while let Some(rel) = body[cursor..].find("import(") {
        let import_pos = cursor + rel;
        advance_jsdoc_brace_stack(bytes, &mut brace_stack, &mut scanned, import_pos);
        if !is_inside_jsdoc_type_brace_group(bytes, import_pos, brace_stack.last().copied()) {
            cursor = import_pos + "import(".len();
            continue;
        }
        let open = import_pos + "import(".len();
        match locate_jsdoc_import_path(body, bytes, open) {
            JsdocImportScan::Stop => break,
            JsdocImportScan::Skip(next) => {
                cursor = next;
            }
            JsdocImportScan::Found { path, after_paren } => {
                cursor = resolve_jsdoc_import(body, bytes, after_paren, path, imports);
            }
        }
    }
}

/// Outcome of locating the path literal and closing paren of one JSDoc
/// `import(...)` occurrence.
enum JsdocImportScan<'a> {
    /// Malformed or truncated; abandon the whole scan.
    Stop,
    /// Not a recoverable import here; resume scanning from this cursor.
    Skip(usize),
    /// A non-empty path was parsed; `after_paren` is the cursor past the `)`.
    Found { path: &'a str, after_paren: usize },
}

/// Parse the quoted path literal following `import(` at `open` and locate the
/// closing paren, returning where the caller should resume.
fn locate_jsdoc_import_path<'a>(body: &'a str, bytes: &[u8], open: usize) -> JsdocImportScan<'a> {
    if open >= bytes.len() {
        return JsdocImportScan::Stop;
    }
    let mut i = open;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return JsdocImportScan::Stop;
    }
    let quote = bytes[i];
    if quote != b'\'' && quote != b'"' {
        return JsdocImportScan::Skip(open);
    }
    let path_start = i + 1;
    let Some(rel_close) = body[path_start..].find(quote as char) else {
        return JsdocImportScan::Stop;
    };
    let path_end = path_start + rel_close;
    let path = &body[path_start..path_end];
    if path.is_empty() {
        return JsdocImportScan::Skip(path_end + 1);
    }
    let mut j = path_end + 1;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    if j >= bytes.len() || bytes[j] != b')' {
        return JsdocImportScan::Skip(path_end + 1);
    }
    j += 1;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    JsdocImportScan::Found {
        path,
        after_paren: j,
    }
}

/// Resolve the imported name after the `)` (member access -> `Named`, otherwise
/// `SideEffect`), push the `ImportInfo`, and return the next scan cursor.
fn resolve_jsdoc_import(
    body: &str,
    bytes: &[u8],
    after_paren: usize,
    path: &str,
    imports: &mut Vec<ImportInfo>,
) -> usize {
    let mut j = after_paren;
    if j >= bytes.len() || bytes[j] != b'.' {
        imports.push(jsdoc_type_import(
            path,
            fallow_types::extract::ImportedName::SideEffect,
        ));
        return after_paren;
    }
    j += 1;
    let name_start = j;
    while j < bytes.len() && is_ident_char(bytes[j]) {
        j += 1;
    }
    if name_start == j {
        // No identifier after `.`: leave the cursor at the post-paren position,
        // matching the original `continue` (which never updated `cursor` here).
        return after_paren;
    }
    let member = &body[name_start..j];
    imports.push(jsdoc_type_import(
        path,
        fallow_types::extract::ImportedName::Named(member.to_string()),
    ));
    j
}

/// Build a type-only `ImportInfo` for a JSDoc `import('...')` reference. Spans
/// are defaulted because JSDoc imports carry no real source position.
fn jsdoc_type_import(
    source: &str,
    imported_name: fallow_types::extract::ImportedName,
) -> ImportInfo {
    ImportInfo {
        source: source.to_string(),
        imported_name,
        local_name: String::new(),
        is_type_only: true,
        from_style: false,
        span: oxc_span::Span::default(),
        source_span: oxc_span::Span::default(),
    }
}

/// Returns true when byte index `pos` falls inside a JSDoc type-expression
/// brace group. Prose examples can contain ordinary JavaScript braces, so the
/// enclosing brace must be tied to a JSDoc type tag. `open_brace` is the
/// innermost enclosing `{` offset (or `None` when `pos` is at brace depth zero),
/// supplied by the caller's incrementally-maintained brace stack.
fn is_inside_jsdoc_type_brace_group(body: &[u8], pos: usize, open_brace: Option<usize>) -> bool {
    let Some(open_brace) = open_brace else {
        return false;
    };

    let prefix = line_prefix_before(body, open_brace);
    if jsdoc_line_prefix_has_type_tag(prefix) {
        return true;
    }

    strip_jsdoc_line_prefix(prefix).is_empty()
        && preceding_jsdoc_line_has_type_tag(body, open_brace)
        && has_only_jsdoc_spacing_between(body, open_brace + 1, pos)
}

/// Advance the incrementally-maintained JSDoc brace stack from `*scanned` up to
/// (but not including) `up_to`, pushing the offset of every `{` and popping on
/// every `}`. Afterwards `stack.last()` is the innermost enclosing brace of
/// `up_to`, identical to a fresh scan of `body[..up_to]` but amortized across
/// every `import(` occurrence in the comment instead of rescanning each prefix
/// from offset zero (issue #1843 follow-up).
///
/// `up_to` must not regress (the caller's `import(` cursor only moves forward);
/// a non-advancing call is a no-op.
fn advance_jsdoc_brace_stack(
    body: &[u8],
    stack: &mut Vec<usize>,
    scanned: &mut usize,
    up_to: usize,
) {
    let up_to = up_to.min(body.len());
    while *scanned < up_to {
        match body[*scanned] {
            b'{' => stack.push(*scanned),
            b'}' => {
                stack.pop();
            }
            _ => {}
        }
        *scanned += 1;
    }
}

fn line_prefix_before(body: &[u8], pos: usize) -> &str {
    let start = body[..pos]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |idx| idx + 1);
    std::str::from_utf8(&body[start..pos]).unwrap_or_default()
}

fn strip_jsdoc_line_prefix(prefix: &str) -> &str {
    let trimmed = prefix.trim_start();
    trimmed
        .strip_prefix('*')
        .map_or(trimmed, |rest| rest.trim_start())
}

fn jsdoc_line_prefix_has_type_tag(prefix: &str) -> bool {
    const TYPE_TAGS: [&str; 17] = [
        "@arg",
        "@argument",
        "@augments",
        "@callback",
        "@enum",
        "@extends",
        "@implements",
        "@param",
        "@property",
        "@prop",
        "@return",
        "@returns",
        "@satisfies",
        "@template",
        "@this",
        "@type",
        "@typedef",
    ];

    let prefix = strip_jsdoc_line_prefix(prefix);
    TYPE_TAGS
        .iter()
        .any(|tag| contains_bare_jsdoc_tag(prefix, tag))
}

fn contains_bare_jsdoc_tag(text: &str, tag: &str) -> bool {
    for (idx, _) in text.match_indices(tag) {
        let after = idx + tag.len();
        if after >= text.len() || !is_ident_char(text.as_bytes()[after]) {
            return true;
        }
    }
    false
}

fn preceding_jsdoc_line_has_type_tag(body: &[u8], pos: usize) -> bool {
    let Some(line_end) = body[..pos].iter().rposition(|&b| b == b'\n') else {
        return false;
    };

    let line_start = body[..line_end]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |idx| idx + 1);

    std::str::from_utf8(&body[line_start..line_end]).is_ok_and(jsdoc_line_prefix_has_type_tag)
}

fn has_only_jsdoc_spacing_between(body: &[u8], start: usize, end: usize) -> bool {
    let mut at_line_start = true;
    let mut i = start.min(body.len());
    let end = end.min(body.len());
    while i < end {
        match body[i] {
            b'\n' => {
                at_line_start = true;
                i += 1;
            }
            b'\r' | b'\t' | b' ' => {
                i += 1;
            }
            b'*' if at_line_start => {
                at_line_start = false;
                i += 1;
            }
            _ => return false,
        }
    }
    true
}

/// Check if a JSDoc comment body contains a `@public` or `@api public` tag.
fn has_public_tag(comment_text: &str) -> bool {
    for (i, _) in comment_text.match_indices("@public") {
        let after = i + "@public".len();
        if after >= comment_text.len() || !is_ident_char(comment_text.as_bytes()[after]) {
            return true;
        }
    }
    for (i, _) in comment_text.match_indices("@api") {
        let after = i + "@api".len();
        if after < comment_text.len() && !is_ident_char(comment_text.as_bytes()[after]) {
            let rest = comment_text[after..].trim_start();
            if rest.starts_with("public") {
                let after_public = "public".len();
                if after_public >= rest.len() || !is_ident_char(rest.as_bytes()[after_public]) {
                    return true;
                }
            }
        }
    }
    false
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportBindingUsage {
    pub unused: Vec<String>,
    pub type_referenced: Vec<String>,
    pub value_referenced: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SemanticUsage {
    pub import_binding_usage: ImportBindingUsage,
    pub auto_import_candidates: Vec<String>,
}

pub fn compute_semantic_usage(
    program: &Program<'_>,
    imports: &[ImportInfo],
    template_used: &rustc_hash::FxHashSet<String>,
) -> SemanticUsage {
    use oxc_semantic::SemanticBuilder;
    use rustc_hash::FxHashSet;

    let semantic_ret = SemanticBuilder::new().build(program);
    let semantic = semantic_ret.semantic;
    let scoping = semantic.scoping();
    let root_scope = scoping.root_scope_id();

    let mut unused = Vec::new();
    let mut type_referenced_bindings: FxHashSet<String> = FxHashSet::default();
    let mut value_referenced_bindings: FxHashSet<String> = FxHashSet::default();
    for import in imports {
        if import.local_name.is_empty() {
            continue;
        }
        let name = oxc_str::Ident::from(import.local_name.as_str());
        if let Some(symbol_id) = scoping.get_binding(root_scope, name) {
            let mut has_references = false;
            let mut has_type_references = false;
            let mut has_value_references = false;

            for reference in scoping.get_resolved_references(symbol_id) {
                has_references = true;
                has_type_references |= reference.is_type();
                has_value_references |= reference.is_value();
            }

            if !has_references {
                if !template_used.contains(&import.local_name) {
                    unused.push(import.local_name.clone());
                }
                continue;
            }

            if has_type_references {
                type_referenced_bindings.insert(import.local_name.clone());
            }
            if has_value_references {
                value_referenced_bindings.insert(import.local_name.clone());
            }
        }
    }

    unused.sort_unstable();

    let mut type_referenced_bindings: Vec<String> = type_referenced_bindings.into_iter().collect();
    type_referenced_bindings.sort_unstable();

    let mut value_referenced_bindings: Vec<String> =
        value_referenced_bindings.into_iter().collect();
    value_referenced_bindings.sort_unstable();

    SemanticUsage {
        import_binding_usage: ImportBindingUsage {
            unused,
            type_referenced: type_referenced_bindings,
            value_referenced: value_referenced_bindings,
        },
        auto_import_candidates: compute_auto_import_candidates_from_semantic(scoping),
    }
}

pub fn compute_auto_import_candidates(program: &Program<'_>) -> Vec<String> {
    use oxc_semantic::SemanticBuilder;

    let semantic_ret = SemanticBuilder::new().build(program);
    let semantic = semantic_ret.semantic;
    compute_auto_import_candidates_from_semantic(semantic.scoping())
}

fn compute_auto_import_candidates_from_semantic(scoping: &oxc_semantic::Scoping) -> Vec<String> {
    use rustc_hash::FxHashSet;

    let mut candidates: FxHashSet<String> = FxHashSet::default();
    for (name, reference_ids) in scoping.root_unresolved_references() {
        if reference_ids
            .iter()
            .any(|reference_id| scoping.get_reference(*reference_id).is_value())
        {
            candidates.insert(name.as_str().to_string());
        }
    }

    let mut candidates: Vec<String> = candidates.into_iter().collect();
    candidates.sort_unstable();
    candidates
}

/// Use `oxc_semantic` to summarize how import bindings are referenced in the file.
///
/// An import like `import { foo } from './utils'` where `foo` is never used
/// anywhere in the file should not count as a reference to the `foo` export.
/// This improves unused-export detection precision.
///
/// `template_used` lets framework template scanners (Glimmer `<template>`
/// blocks today; Vue/Svelte SFCs will follow) credit imports referenced only
/// in markup that `oxc_semantic` cannot see. Names in the set are filtered
/// out of the `unused` result before it is built. Pass `&FxHashSet::default()`
/// when no template scan applies.
///
/// Note: `get_resolved_references` counts both value-context and type-context
/// references. A value import used only as a type annotation (`const x: Foo`)
/// will have a type-position reference and will NOT appear in the unused list.
/// This is correct: `import { Foo }` (without `type`) may be needed at runtime.
pub fn compute_import_binding_usage(
    program: &Program<'_>,
    imports: &[ImportInfo],
    template_used: &rustc_hash::FxHashSet<String>,
) -> ImportBindingUsage {
    compute_semantic_usage(program, imports, template_used).import_binding_usage
}

#[cfg(test)]
mod tests {
    use super::{
        advance_jsdoc_brace_stack, has_alpha_tag, has_beta_tag, has_internal_tag, has_public_tag,
        parse_source_to_module, scan_jsdoc_imports_in,
    };
    use fallow_types::discover::FileId;
    use fallow_types::extract::{ImportInfo, ImportedName};
    use std::path::Path;

    #[test]
    fn has_public_tag_matches_bare_tag() {
        assert!(has_public_tag(" * @public"));
    }

    #[test]
    fn has_public_tag_matches_api_public_variant() {
        assert!(has_public_tag(" * @api public"));
    }

    #[test]
    fn has_public_tag_rejects_partial_word() {
        assert!(!has_public_tag(" * @publicly"));
    }

    #[test]
    fn has_public_tag_rejects_at_apipublic() {
        assert!(!has_public_tag(" * @apipublic"));
    }

    #[test]
    fn has_public_tag_rejects_missing_at() {
        assert!(!has_public_tag(" * public"));
    }

    #[test]
    fn has_internal_tag_matches_bare_tag() {
        assert!(has_internal_tag(" * @internal"));
    }

    #[test]
    fn has_internal_tag_rejects_partial_word() {
        assert!(!has_internal_tag(" * @internalizer"));
    }

    #[test]
    fn has_internal_tag_rejects_missing_at() {
        assert!(!has_internal_tag(" * internal"));
    }

    #[test]
    fn has_beta_tag_matches_bare_tag() {
        assert!(has_beta_tag(" * @beta"));
    }

    #[test]
    fn has_beta_tag_rejects_partial_word() {
        assert!(!has_beta_tag(" * @betaware"));
    }

    #[test]
    fn has_beta_tag_rejects_missing_at() {
        assert!(!has_beta_tag(" * beta"));
    }

    #[test]
    fn alpha_tag_standalone() {
        assert!(has_alpha_tag("@alpha"));
    }

    #[test]
    fn alpha_tag_with_text() {
        assert!(has_alpha_tag("@alpha Some description"));
    }

    #[test]
    fn alpha_tag_not_prefix() {
        assert!(!has_alpha_tag("@alphabet"));
    }

    #[test]
    fn has_alpha_tag_rejects_missing_at() {
        assert!(!has_alpha_tag(" * alpha"));
    }

    fn scan(body: &str) -> Vec<ImportInfo> {
        let mut imports = Vec::new();
        scan_jsdoc_imports_in(body, &mut imports);
        imports
    }

    #[test]
    fn scan_jsdoc_single_import_with_member() {
        let imports = scan(" * @param foo {import('./types').Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./types");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Foo".to_string())
        );
        assert!(imports[0].is_type_only);
        assert!(imports[0].local_name.is_empty());
    }

    #[test]
    fn script_auto_import_candidates_capture_zero_import_value_refs() {
        let info = parse_source_to_module(
            FileId(0),
            Path::new("pages/index.ts"),
            r"
                useCounter();
                const price = formatPrice(10);
                const localOnly = () => null;
                localOnly();
                type Local = UseTypeOnly;
            ",
            0,
            false,
        );

        assert!(
            info.auto_import_candidates
                .contains(&"formatPrice".to_string())
        );
        assert!(
            info.auto_import_candidates
                .contains(&"useCounter".to_string())
        );
        assert!(
            !info
                .auto_import_candidates
                .contains(&"UseTypeOnly".to_string())
        );
        assert!(
            !info
                .auto_import_candidates
                .contains(&"localOnly".to_string())
        );
    }

    #[test]
    fn script_auto_import_candidates_skip_explicit_imports() {
        let info = parse_source_to_module(
            FileId(0),
            Path::new("pages/index.ts"),
            "import { useCounter } from '../composables/useCounter';\nuseCounter();\nuseOther();\n",
            0,
            false,
        );

        assert!(
            !info
                .auto_import_candidates
                .contains(&"useCounter".to_string())
        );
        assert!(
            info.auto_import_candidates
                .contains(&"useOther".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_double_quoted_path() {
        let imports = scan(r#" * @type {import("./types").Foo}"#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./types");
    }

    #[test]
    fn scan_jsdoc_multiple_imports_in_same_body() {
        let imports = scan(" * @param a {import('./a').A} @param b {import('./b').B}");
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "./a");
        assert_eq!(imports[1].source, "./b");
    }

    #[test]
    fn scan_jsdoc_union_annotation_captures_both_members() {
        let imports = scan(" * @type {import('./a').A | import('./b').B}");
        assert_eq!(imports.len(), 2);
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("A".to_string())
        );
        assert_eq!(
            imports[1].imported_name,
            ImportedName::Named("B".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_nested_member_uses_first_segment() {
        let imports = scan(" * @type {import('./types').ns.Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("ns".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_parent_relative_path() {
        let imports = scan(" * @type {import('../lib/types.js').Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "../lib/types.js");
    }

    #[test]
    fn scan_jsdoc_bare_package_specifier() {
        let imports = scan(" * @type {import('@scope/pkg').Client}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "@scope/pkg");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Client".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_without_member_is_side_effect() {
        let imports = scan(" * @type {import('./types')}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./types");
        assert_eq!(imports[0].imported_name, ImportedName::SideEffect);
        assert!(imports[0].is_type_only);
    }

    #[test]
    fn scan_jsdoc_empty_path_is_skipped() {
        let imports = scan(" * @type {import('').Foo}");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_truncated_no_closing_quote_does_not_panic() {
        let imports = scan(" * @type {import('./truncated");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_missing_closing_paren_is_skipped() {
        let imports = scan(" * @type {import('./types'.Foo}");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_whitespace_between_paren_and_dot() {
        let imports = scan(" * @type {import('./types') .Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./types");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Foo".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_whitespace_between_paren_and_quote() {
        let imports = scan(" * @type {import( './types').Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./types");
    }

    #[test]
    fn scan_jsdoc_non_quote_after_paren_skipped() {
        let imports = scan(" * @type {import(foo).Bar}");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_ignores_prose_with_import_word() {
        let imports = scan(" * This is an important note about imports.");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_utf8_path_works() {
        let imports = scan(" * @type {import('./héllo').Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./héllo");
    }

    #[test]
    fn scan_jsdoc_empty_body_is_empty() {
        assert!(scan("").is_empty());
    }

    #[test]
    fn scan_jsdoc_no_import_in_body_is_empty() {
        assert!(scan(" * @param foo The foo parameter").is_empty());
    }

    /// Regression: `import('...')` in JSDoc prose (outside any `{...}` brace
    /// group) is documentation/example syntax, not a type annotation. It must
    /// not be reported as a real import. Without this scoping check, files
    /// whose header doc documents which import forms they handle would surface
    /// false-positive unresolved-import findings.
    #[test]
    fn scan_jsdoc_prose_import_outside_braces_is_skipped() {
        // Mirrors the exact shape of an extractor's header doc that lists
        // import forms as bullet-point examples.
        let body = "\n * Handles:\n * - Dynamic imports (await import('./prose')) \n * - Barrel exports (export * from './prose')\n";
        let imports = scan(body);
        assert!(
            imports.is_empty(),
            "prose import() should not be matched; got: {:?}",
            imports
                .iter()
                .map(|i| i.source.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn scan_jsdoc_prose_import_inside_example_object_is_skipped() {
        let body = "\n * @example\n * const loaders = {\n *   admin: () => import('./prose')\n * }";
        let imports = scan(body);
        assert!(
            imports.is_empty(),
            "object-literal example import() should not be matched; got: {:?}",
            imports
                .iter()
                .map(|i| i.source.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn scan_jsdoc_prose_import_inside_inline_braces_is_skipped() {
        let imports = scan(" * Use {import('./prose')} as an example string.");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_bare_example_brace_import_is_skipped() {
        let imports = scan("\n * @example\n * { import('./prose') }\n");
        assert!(imports.is_empty());
    }

    /// A real `{@type ...}` annotation following a prose mention of `import()`
    /// must still be matched. The fix narrows scope without breaking the
    /// intended JSDoc type-annotation behavior.
    #[test]
    fn scan_jsdoc_braced_import_after_prose_is_still_matched() {
        let body = " * Note: dynamic imports like import('./prose') are not types.\n * @type {import('./real').Foo}";
        let imports = scan(body);
        assert_eq!(imports.len(), 1, "got: {imports:?}");
        assert_eq!(imports[0].source, "./real");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Foo".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_multiline_braced_type_tag_is_still_matched() {
        let body = "\n * @returns {\n *   import('./real').Foo\n * }";
        let imports = scan(body);
        assert_eq!(imports.len(), 1, "got: {imports:?}");
        assert_eq!(imports[0].source, "./real");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Foo".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_type_tag_before_brace_line_is_still_matched() {
        let body = "\n * @type\n * { import('./real').Foo }\n";
        let imports = scan(body);
        assert_eq!(imports.len(), 1, "got: {imports:?}");
        assert_eq!(imports[0].source, "./real");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Foo".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_satisfies_type_tag_is_still_matched() {
        let imports = scan(" * @satisfies {import('./real').Foo}");
        assert_eq!(imports.len(), 1, "got: {imports:?}");
        assert_eq!(imports[0].source, "./real");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Foo".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_template_constraint_type_tag_is_still_matched() {
        let imports = scan(" * @template {import('./real').Foo} T");
        assert_eq!(imports.len(), 1, "got: {imports:?}");
        assert_eq!(imports[0].source, "./real");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Foo".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_enum_type_tag_is_still_matched() {
        let imports = scan(" * @enum {import('./real').Foo}");
        assert_eq!(imports.len(), 1, "got: {imports:?}");
        assert_eq!(imports[0].source, "./real");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Foo".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_appends_to_existing_imports() {
        let mut imports = vec![ImportInfo {
            source: "existing".to_string(),
            imported_name: ImportedName::Default,
            local_name: "existing".to_string(),
            is_type_only: false,
            from_style: false,
            span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        }];
        scan_jsdoc_imports_in(" * @type {import('./new').Foo}", &mut imports);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "existing");
        assert_eq!(imports[1].source, "./new");
    }

    #[test]
    fn scan_jsdoc_ident_boundary_stops_at_bracket() {
        let imports = scan(" * @type {import('./t').Abc}");
        assert_eq!(imports.len(), 1);
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Abc".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_empty_member_name_is_skipped() {
        let imports = scan(" * @type {import('./x').}");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_many_imports_incremental_brace_stack_is_identical() {
        // Regression for the issue #1843 follow-up: the enclosing-brace lookup
        // is maintained incrementally across the whole comment rather than
        // rescanning every prefix. A comment packed with many `import(...)` type
        // refs must still extract exactly one import per `{...}` type group, in
        // order, with the same paths and member names as before.
        use std::fmt::Write as _;
        let mut body = String::from("/**\n");
        for i in 0..200 {
            let _ = writeln!(body, " * @param a{i} {{import('./m{i}').T{i}}} description");
        }
        // A prose `import(` outside any type brace group and a nested brace
        // must not add spurious imports or shift the enclosing-brace tracking.
        body.push_str(" * @remarks import('./ignored') appears in prose here\n");
        body.push_str(" * @typedef {{ nested: { deep: import('./deep').D } }} Obj\n");
        body.push_str(" */\n");

        let imports = scan(&body);
        assert_eq!(imports.len(), 201, "got: {imports:?}");
        for (i, import) in imports.iter().take(200).enumerate() {
            assert_eq!(import.source, format!("./m{i}"));
            assert_eq!(import.imported_name, ImportedName::Named(format!("T{i}")));
            assert!(import.is_type_only);
            assert!(import.local_name.is_empty());
        }
        // The nested-brace occurrence still resolves against its enclosing group.
        assert_eq!(imports[200].source, "./deep");
        assert_eq!(
            imports[200].imported_name,
            ImportedName::Named("D".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_brace_stack_matches_offset_zero_rescan() {
        // Cross-checks the incremental brace stack against an independent
        // offset-zero rescan over the full prefix, on inputs where the
        // `import(` cursor skips over intervening braces (issue #1843 follow-up).
        let cases = [
            " * @type {import('./a').A} and {plain} then {import('./b').B}",
            " * @remarks { import('./skip') } @param x {import('./c').C}",
            " * text } stray close { import('./d').D } trailing",
            " * @type {{ a: import('./e').E, b: { c: import('./f').F } }}",
        ];
        for body in cases {
            let bytes = body.as_bytes();
            let mut cursor = 0;
            while let Some(rel) = body[cursor..].find("import(") {
                let import_pos = cursor + rel;
                // Independent offset-zero rescan reproducing the old helper.
                let mut fresh = Vec::new();
                for (idx, &b) in bytes[..import_pos].iter().enumerate() {
                    match b {
                        b'{' => fresh.push(idx),
                        b'}' => {
                            fresh.pop();
                        }
                        _ => {}
                    }
                }
                let mut stack = Vec::new();
                let mut scanned = 0;
                advance_jsdoc_brace_stack(bytes, &mut stack, &mut scanned, import_pos);
                assert_eq!(
                    stack.last().copied(),
                    fresh.last().copied(),
                    "enclosing brace mismatch at {import_pos} in {body:?}"
                );
                cursor = import_pos + "import(".len();
            }
        }
    }
}
