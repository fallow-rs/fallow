use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::{
    AstKind,
    ast::{Comment, Program},
};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};

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
use fallow_types::extract::{
    FlagPatterns, FunctionComplexity, ImportInfo, ImportedName, VisibilityTag,
};

use crate::flags::ExtractedFlags;

struct JsxRetryParse {
    extractor: ModuleInfoExtractor,
    semantic_usage: SemanticUsage,
    complexity: Vec<FunctionComplexity>,
    flags: ExtractedFlags,
    parsed_suppressions: crate::suppress::ParsedSuppressions,
    degradation: ParseDegradation,
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
///
/// Flag detection uses the built-in patterns only; see
/// [`parse_source_to_module_with_flags`].
pub fn parse_source_to_module(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    parse_source_to_module_with_flags(
        file_id,
        path,
        source,
        content_hash,
        need_complexity,
        &FlagPatterns::default(),
    )
}

/// Parse source text into a [`ModuleInfo`], with the user flag patterns
/// applied on top of the built-in ones.
pub fn parse_source_to_module_with_flags(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
    flag_patterns: &FlagPatterns,
) -> ModuleInfo {
    let mut module = parse_source_to_module_inner(
        file_id,
        path,
        source,
        content_hash,
        need_complexity,
        flag_patterns,
    );
    if is_glimmer_file(path) {
        for range in crate::glimmer::find_template_ranges(source) {
            let (start, end) = (range.start, range.end);
            let mut callers = crate::sfc_template::component_contracts::collect(
                &source[start..end],
                &module.imports,
                fallow_types::extract::ComponentFramework::Ember,
                start as u32,
                module
                    .component_contracts
                    .as_deref()
                    .map_or(&[], |facts| facts.spread_bindings.as_slice()),
                module
                    .component_contracts
                    .as_deref()
                    .map_or(&[], |facts| facts.aliases.as_slice()),
            );
            let (reads, incomplete, dynamic) =
                crate::sfc_template::glimmer::collect_argument_reads(&source[start..end]);
            if dynamic {
                callers
                    .incomplete_frameworks
                    .push(fallow_types::extract::ComponentFramework::Ember);
            }
            if let Some(facts) = &mut module.component_contracts {
                let owners: Vec<_> = facts
                    .template_owners
                    .iter()
                    .filter(|owner| owner.start <= start as u32 && end as u32 <= owner.end)
                    .collect();
                if let [owner] = owners.as_slice() {
                    for declaration in &mut facts.declarations {
                        if declaration.component_span == owner.component_span
                            && declaration.framework
                                == fallow_types::extract::ComponentFramework::Ember
                        {
                            declaration.is_used |= reads.contains(&declaration.name);
                            declaration.incomplete |= incomplete;
                        }
                    }
                }
            }
            crate::component_contracts::merge(&mut module.component_contracts, callers, 0, None);
        }
    }
    module.iconify_prefixes = crate::iconify::extract_iconify_prefixes(path, source);
    module.iconify_icon_names = crate::iconify::extract_iconify_icon_names(path, source);
    let federation_facts =
        crate::federation_runtime::extract_federation_runtime_facts(path, source);
    if !federation_facts.is_empty() {
        module.semantic_facts = module
            .semantic_facts
            .iter()
            .cloned()
            .chain(federation_facts)
            .collect();
    }
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
    flag_patterns: &FlagPatterns,
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
    let mut degradation = ParseDegradation::from_parser(&parser_return);

    let mut parsed_suppressions =
        crate::suppress::parse_suppressions(&parser_return.program.comments, source);

    let (mut extractor, mut semantic_usage) =
        build_primary_extractor(&parser_return.program, path, source, source_type);

    let line_offsets = fallow_types::extract::compute_line_offsets(source);

    let (mut complexity, mut flags) = compute_primary_complexity_and_flags(
        &parser_return.program,
        parser_source,
        &extractor.inline_template_findings,
        &line_offsets,
        need_complexity,
        flag_patterns,
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
            export_statements: &crate::jsdoc_attach::export_statement_spans(&parser_return.program),
            flag_patterns,
        },
        &mut ParseOutputs {
            extractor: &mut extractor,
            semantic_usage: &mut semantic_usage,
            complexity: &mut complexity,
            flags: &mut flags,
            parsed_suppressions: &mut parsed_suppressions,
            degradation: &mut degradation,
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
        flags,
        degradation,
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
    export_statements: &'a [oxc_span::Span],
    flag_patterns: &'a FlagPatterns,
}

struct ModuleAssemblyInput {
    extractor: ModuleInfoExtractor,
    file_id: FileId,
    content_hash: u64,
    parsed_suppressions: crate::suppress::ParsedSuppressions,
    semantic_usage: SemanticUsage,
    line_offsets: Vec<u32>,
    complexity: Vec<FunctionComplexity>,
    flags: ExtractedFlags,
    degradation: ParseDegradation,
}

/// How much of the file the parser actually understood.
///
/// oxc reports recoverable errors for valid-but-newer syntax as well as for
/// genuinely broken sources, so this is reporting data only: extraction keeps
/// every symbol it found, and no finding is ever withheld because of it. What
/// it buys is honesty about the case where a file failed to parse and its
/// imports therefore never credited anything, which otherwise surfaces as a
/// confident `unused-file` finding on the file that was in fact imported.
#[derive(Debug, Clone, Copy, Default)]
struct ParseDegradation {
    error_count: u32,
    panicked: bool,
}

impl ParseDegradation {
    fn from_parser(parser_return: &oxc_parser::ParserReturn<'_>) -> Self {
        Self {
            error_count: u32::try_from(parser_return.diagnostics.len()).unwrap_or(u32::MAX),
            panicked: parser_return.fatal_error,
        }
    }
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
        compute_semantic_usage_for_extractor(program, &mut extractor, &template_used_imports);
    extractor.resolve_vitest_mock_operations(&semantic_usage.mock_api_reference_spans);
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
    flag_patterns: &FlagPatterns,
) -> (Vec<FunctionComplexity>, ExtractedFlags) {
    let mut complexity = if need_complexity {
        crate::complexity::compute_complexity(program, parser_source, line_offsets)
    } else {
        Vec::new()
    };
    if need_complexity {
        append_inline_template_complexity(&mut complexity, inline_template_findings, line_offsets);
    }

    let flags = crate::flags::extract_flags(program, line_offsets, flag_patterns);
    (complexity, flags)
}

/// Mutable references to the primary-parse outputs a JSX retry replaces wholesale.
struct ParseOutputs<'a> {
    extractor: &'a mut ModuleInfoExtractor,
    semantic_usage: &'a mut SemanticUsage,
    complexity: &'a mut Vec<FunctionComplexity>,
    flags: &'a mut ExtractedFlags,
    parsed_suppressions: &'a mut crate::suppress::ParsedSuppressions,
    degradation: &'a mut ParseDegradation,
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
        flag_patterns: input.flag_patterns,
    };
    let Some(retry) = parse_with_jsx_retry(&retry_input) else {
        apply_jsdoc_tags_to_extractor(
            &mut *outputs.extractor,
            input.comments,
            input.source,
            input.export_statements,
        );
        return;
    };
    *outputs.extractor = retry.extractor;
    *outputs.semantic_usage = retry.semantic_usage;
    *outputs.complexity = retry.complexity;
    *outputs.flags = retry.flags;
    *outputs.parsed_suppressions = retry.parsed_suppressions;
    // The retry parse replaced every primary output, so the primary parse's
    // diagnostics describe a tree nothing downstream can see any more.
    *outputs.degradation = retry.degradation;
}

/// Apply JSDoc visibility tags and JSDoc `import()` type references to the
/// extractor's exports/imports for the primary (non-retry) parse.
fn apply_jsdoc_tags_to_extractor(
    extractor: &mut ModuleInfoExtractor,
    comments: &[Comment],
    source: &str,
    statements: &[oxc_span::Span],
) {
    apply_jsdoc_visibility_tags(&mut extractor.exports, comments, source, statements);
    crate::jsdoc_deprecated::apply_jsdoc_deprecated_tags(
        &mut extractor.exports,
        comments,
        source,
        statements,
    );
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
        flags,
        degradation,
    } = input;
    let mut info = extractor.into_module_info(file_id, content_hash, parsed_suppressions);
    let mut contracts = semantic_usage.component_contracts;
    if degradation.error_count > 0 || degradation.panicked {
        for declaration in &mut contracts.declarations {
            declaration.incomplete = true;
        }
        for invocation in &mut contracts.invocations {
            invocation.unknown_props = true;
        }
    }
    if !contracts.aliases.is_empty()
        || !contracts.exports.is_empty()
        || !contracts.declarations.is_empty()
        || !contracts.invocations.is_empty()
        || !contracts.escapes.is_empty()
        || !contracts.incomplete_frameworks.is_empty()
        || !contracts.spread_bindings.is_empty()
    {
        info.component_contracts = Some(Box::new(contracts));
    }
    info.parse_error_count = degradation.error_count;
    info.parse_panicked = degradation.panicked;
    info.unused_import_bindings = semantic_usage.import_binding_usage.unused;
    info.type_referenced_import_bindings = semantic_usage.import_binding_usage.type_referenced;
    info.value_referenced_import_bindings = semantic_usage.import_binding_usage.value_referenced;
    info.auto_import_candidates
        .extend(semantic_usage.auto_import_candidates);
    info.auto_import_candidates.sort_unstable();
    info.auto_import_candidates.dedup();
    append_declaration_merge_facts(
        &mut info.semantic_facts,
        semantic_usage.declaration_merges,
        0,
    );
    info.line_offsets = line_offsets;
    info.complexity = complexity;
    info.flag_uses = flags.flag_uses;
    info.flag_registry_facts = flags.registry_facts;
    info
}

pub fn append_declaration_merge_facts(
    facts: &mut std::sync::Arc<[fallow_types::extract::SemanticFact]>,
    mut groups: Vec<fallow_types::extract::DeclarationMergeFact>,
    byte_offset: u32,
) {
    if groups.is_empty() {
        return;
    }
    if byte_offset != 0 {
        for group in &mut groups {
            for (start, end) in &mut group.export_spans {
                *start += byte_offset;
                *end += byte_offset;
            }
        }
    }
    let mut merged = std::mem::take(facts).to_vec();
    merged.extend(
        groups
            .into_iter()
            .map(fallow_types::extract::SemanticFact::DeclarationMerge),
    );
    *facts = merged.into();
}

struct JsxRetryInput<'a> {
    path: &'a Path,
    source: &'a str,
    parser_source: &'a str,
    source_type: SourceType,
    total_extracted: usize,
    need_complexity: bool,
    line_offsets: &'a [u32],
    flag_patterns: &'a FlagPatterns,
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
    let degradation = ParseDegradation::from_parser(&retry_return);
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
    let semantic_usage = compute_semantic_usage_for_extractor(
        &retry_return.program,
        &mut extractor,
        &template_used_imports,
    );
    extractor.resolve_vitest_mock_operations(&semantic_usage.mock_api_reference_spans);
    let complexity = retry_complexity(
        input.need_complexity,
        &retry_return.program,
        input.parser_source,
        input.line_offsets,
        &extractor,
    );
    let flags = crate::flags::extract_flags(
        &retry_return.program,
        input.line_offsets,
        input.flag_patterns,
    );
    let parsed_suppressions =
        crate::suppress::parse_suppressions(&retry_return.program.comments, input.source);
    let export_statements = crate::jsdoc_attach::export_statement_spans(&retry_return.program);
    apply_jsdoc_visibility_tags(
        &mut extractor.exports,
        &retry_return.program.comments,
        input.source,
        &export_statements,
    );
    crate::jsdoc_deprecated::apply_jsdoc_deprecated_tags(
        &mut extractor.exports,
        &retry_return.program.comments,
        input.source,
        &export_statements,
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
        flags,
        parsed_suppressions,
        degradation,
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
/// A tag belongs to an export when it attaches to the export itself or to
/// the start of the export statement that holds it (see
/// [`crate::jsdoc_attach`]). A tag on one statement never reaches a later
/// statement, also in a file without semicolons.
fn apply_jsdoc_visibility_tags(
    exports: &mut [ExportInfo],
    comments: &[Comment],
    source: &str,
    statements: &[oxc_span::Span],
) {
    if exports.is_empty() || comments.is_empty() {
        return;
    }

    let mut tag_offsets = collect_jsdoc_tag_offsets(comments, source);
    if tag_offsets.is_empty() {
        return;
    }
    // Stable: comments stay in source order within one attachment offset.
    tag_offsets.sort_by_key(|&(offset, _, _)| offset);

    for export in exports.iter_mut() {
        apply_visibility_tag_to_export(export, &tag_offsets, statements);
    }
}

/// Classify a JSDoc comment body into a visibility tag (and optional reason),
/// or `None` when no recognized tag is present.
fn classify_jsdoc_visibility_tag(text: &str) -> Option<(VisibilityTag, Option<String>)> {
    if has_public_tag(text) {
        Some((VisibilityTag::Public, None))
    } else if bare_jsdoc_tag_end(text, "@internal").is_some() {
        Some((VisibilityTag::Internal, None))
    } else if bare_jsdoc_tag_end(text, "@alpha").is_some() {
        Some((VisibilityTag::Alpha, None))
    } else if bare_jsdoc_tag_end(text, "@beta").is_some() {
        Some((VisibilityTag::Beta, None))
    } else {
        bare_jsdoc_tag_end(text, "@expected-unused").map(|after| {
            (
                VisibilityTag::ExpectedUnused,
                split_jsdoc_reason(&text[after..]),
            )
        })
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

/// Apply the visibility tag that belongs to a single export.
fn apply_visibility_tag_to_export(
    export: &mut ExportInfo,
    tag_offsets: &[(u32, VisibilityTag, Option<String>)],
    statements: &[oxc_span::Span],
) {
    if export.span.start == 0 && export.span.end == 0 {
        return;
    }
    let found = crate::jsdoc_attach::tag_index_for_export(
        tag_offsets,
        |&(offset, _, _)| offset,
        export.span.start,
        statements,
    );
    if let Some(idx) = found {
        export.visibility = tag_offsets[idx].1;
        export
            .expected_unused_reason
            .clone_from(&tag_offsets[idx].2);
    }
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

/// Check if a byte is an identifier-continuation character (alphanumeric or `_`).
const fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Scan JSDoc comments for `import('./path').Member` type expressions and
/// `@import` tags, and push them onto `imports` as type-only imports.
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

    let mut namespaces = Vec::new();
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
        let body = &source[start..end];
        scan_jsdoc_imports_in(body, imports);
        scan_jsdoc_import_tags_in(body, imports, &mut namespaces);
    }
    if !namespaces.is_empty() {
        push_jsdoc_namespace_members(imports, &namespaces, comments, source);
    }
}

/// A namespace binding from a JSDoc `@import * as ns from './mod'` tag.
struct JsdocNamespaceImport {
    local: String,
    source: String,
}

/// Push a type-only import for each `<ns>.<Member>` reference in the JSDoc
/// type expressions of the file, plus a side-effect import that keeps the
/// target module reachable when no member is used.
///
/// A namespace import credits every export of the target module. The binding
/// of a JSDoc `@import` tag exists only in JSDoc types, so the members that
/// the file reads are known, and only those members get credit.
fn push_jsdoc_namespace_members(
    imports: &mut Vec<ImportInfo>,
    namespaces: &[JsdocNamespaceImport],
    comments: &[Comment],
    source: &str,
) {
    use fallow_types::extract::ImportedName;

    let mut members: Vec<(usize, &str)> = Vec::new();
    for comment in comments.iter().filter(|comment| comment.is_jsdoc()) {
        let content_span = comment.content_span();
        let start = content_span.start as usize;
        let end = (content_span.end as usize).min(source.len());
        if start >= end {
            continue;
        }
        scan_jsdoc_namespace_members_in(&source[start..end], namespaces, &mut members);
    }
    for namespace in namespaces {
        imports.push(jsdoc_type_import(
            &namespace.source,
            ImportedName::SideEffect,
        ));
    }
    for (index, member) in members {
        imports.push(jsdoc_type_import(
            &namespaces[index].source,
            ImportedName::Named(member.to_string()),
        ));
    }
}

/// Collect each `<ns>.<Member>` reference inside a JSDoc type brace group of
/// one comment body. `members` holds `(namespace index, member)` pairs
/// without duplicates.
fn scan_jsdoc_namespace_members_in<'a>(
    body: &'a str,
    namespaces: &[JsdocNamespaceImport],
    members: &mut Vec<(usize, &'a str)>,
) {
    let bytes = body.as_bytes();
    let mut brace_stack: Vec<usize> = Vec::new();
    let mut scanned = 0;
    let mut pos = 0;
    while pos < bytes.len() {
        let starts_ident =
            (bytes[pos].is_ascii_alphabetic() || bytes[pos] == b'_' || bytes[pos] == b'$')
                && (pos == 0
                    || !(is_ident_char(bytes[pos - 1]) || matches!(bytes[pos - 1], b'$' | b'.')));
        if !starts_ident {
            pos += 1;
            continue;
        }
        let Some((ident, after)) = take_js_identifier(&body[pos..]) else {
            pos += 1;
            continue;
        };
        let ident_pos = pos;
        pos += ident.len();
        let Some(index) = namespaces
            .iter()
            .position(|namespace| namespace.local == ident)
        else {
            continue;
        };
        let Some((member, _)) = after.strip_prefix('.').and_then(take_js_identifier) else {
            continue;
        };
        advance_jsdoc_brace_stack(bytes, &mut brace_stack, &mut scanned, ident_pos);
        if !is_inside_jsdoc_type_brace_group(bytes, ident_pos, brace_stack.last().copied()) {
            continue;
        }
        let entry = (index, member);
        if !members.contains(&entry) {
            members.push(entry);
        }
        pos += 1 + member.len();
    }
}

const JSDOC_IMPORT_TAG: &str = "@import";

/// Parse a single JSDoc comment body for TypeScript `@import` tags and push
/// each imported binding as a type-only import.
///
/// ```js
/// /** @import { Foo, Bar as Baz } from './types' */
/// /** @import * as ns from './ns' */
/// /** @import Def from './def' */
/// ```
///
/// The tag must start a JSDoc line, and its clause can continue on the next
/// lines until the module specifier or the next tag. A tag that does not parse
/// as an import clause adds no import. A namespace binding goes to
/// `namespaces` and not to `imports`, because the caller credits only the
/// members that the file reads through it.
fn scan_jsdoc_import_tags_in(
    body: &str,
    imports: &mut Vec<ImportInfo>,
    namespaces: &mut Vec<JsdocNamespaceImport>,
) {
    let bytes = body.as_bytes();
    let mut cursor = 0;
    while let Some(rel) = body[cursor..].find(JSDOC_IMPORT_TAG) {
        let tag_pos = cursor + rel;
        cursor = tag_pos + JSDOC_IMPORT_TAG.len();
        let starts_line = strip_jsdoc_line_prefix(line_prefix_before(bytes, tag_pos)).is_empty();
        let ends_tag = bytes
            .get(cursor)
            .is_none_or(|&b| b.is_ascii_whitespace() || b == b'{' || b == b'*');
        if !starts_line || !ends_tag {
            continue;
        }
        let clause = jsdoc_import_tag_clause(&body[cursor..]);
        let Some(parsed) = parse_jsdoc_import_clause(&clause) else {
            continue;
        };
        if let Some(local) = parsed.namespace {
            namespaces.push(JsdocNamespaceImport {
                local: local.to_string(),
                source: parsed.source.to_string(),
            });
        } else if parsed.names.is_empty() {
            imports.push(jsdoc_type_import(
                parsed.source,
                fallow_types::extract::ImportedName::SideEffect,
            ));
        }
        for name in parsed.names {
            imports.push(jsdoc_type_import(parsed.source, name));
        }
    }
}

/// Join the text after an `@import` tag into one line. Continuation lines lose
/// their JSDoc `*` prefix, and the clause stops before the next tag.
fn jsdoc_import_tag_clause(rest: &str) -> String {
    let mut lines = rest.split('\n');
    let mut clause = lines.next().unwrap_or_default().to_string();
    for line in lines {
        let line = strip_jsdoc_line_prefix(line);
        if line.starts_with('@') {
            break;
        }
        clause.push(' ');
        clause.push_str(line);
    }
    clause
}

/// The bindings and the module specifier of one JSDoc `@import` clause.
struct JsdocImportClause<'a> {
    /// Default and named bindings.
    names: Vec<fallow_types::extract::ImportedName>,
    /// The local name of a `* as ns` binding.
    namespace: Option<&'a str>,
    source: &'a str,
}

/// Parse `<bindings> from '<specifier>'`. Returns `None` when the clause is
/// not a valid import clause.
fn parse_jsdoc_import_clause(clause: &str) -> Option<JsdocImportClause<'_>> {
    use fallow_types::extract::ImportedName;

    let mut names = Vec::new();
    let mut namespace = None;
    let mut rest = clause.trim_start();
    if let Some((ident, after)) = take_js_identifier(rest)
        && ident != "from"
    {
        names.push(ImportedName::Default);
        rest = after.trim_start();
        match rest.strip_prefix(',') {
            Some(after_comma) => rest = after_comma.trim_start(),
            None => {
                return parse_jsdoc_import_from(rest).map(|source| JsdocImportClause {
                    names,
                    namespace,
                    source,
                });
            }
        }
    }
    if let Some(after_star) = rest.strip_prefix('*') {
        let after_as = after_star.trim_start().strip_prefix("as")?;
        let (local, after_ns) = take_js_identifier(after_as.trim_start())?;
        namespace = Some(local);
        rest = after_ns;
    } else if let Some(after_brace) = rest.strip_prefix('{') {
        let close = after_brace.find('}')?;
        for specifier in after_brace[..close].split(',') {
            if let Some(name) = jsdoc_import_specifier_name(specifier) {
                names.push(name);
            }
        }
        rest = &after_brace[close + 1..];
    } else if names.is_empty() {
        return None;
    }
    parse_jsdoc_import_from(rest.trim_start()).map(|source| JsdocImportClause {
        names,
        namespace,
        source,
    })
}

/// Read the imported name of one `{ ... }` entry: `A`, `A as B`, `type A`,
/// `'a-b' as B` or `default as B`.
fn jsdoc_import_specifier_name(specifier: &str) -> Option<fallow_types::extract::ImportedName> {
    use fallow_types::extract::ImportedName;

    let specifier = specifier.trim();
    let specifier = specifier
        .strip_prefix("type")
        .filter(|after| after.starts_with(char::is_whitespace))
        .map_or(specifier, str::trim_start);
    let name = match specifier.as_bytes().first()? {
        quote @ (b'\'' | b'"') => {
            let inner = &specifier[1..];
            &inner[..inner.find(*quote as char)?]
        }
        _ => take_js_identifier(specifier)?.0,
    };
    if name == "default" {
        return Some(ImportedName::Default);
    }
    Some(ImportedName::Named(name.to_string()))
}

/// Parse `from '<specifier>'` and return the non-empty specifier.
fn parse_jsdoc_import_from(rest: &str) -> Option<&str> {
    let (keyword, after) = take_js_identifier(rest)?;
    if keyword != "from" {
        return None;
    }
    let after = after.trim_start();
    let quote = after.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let inner = &after[1..];
    let source = &inner[..inner.find(quote)?];
    (!source.is_empty()).then_some(source)
}

/// Split a leading JavaScript identifier (ASCII letters, digits, `_`, `$`)
/// from `text`.
fn take_js_identifier(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    if !bytes
        .first()
        .is_some_and(|&b| b.is_ascii_alphabetic() || b == b'_' || b == b'$')
    {
        return None;
    }
    let end = bytes
        .iter()
        .position(|&b| !(is_ident_char(b) || b == b'$'))
        .unwrap_or(bytes.len());
    Some(text.split_at(end))
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

/// Build a type-only `ImportInfo` for a JSDoc `import('...')` reference or
/// `@import` tag. Spans
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
        is_type_only_star: false,
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
        .any(|tag| bare_jsdoc_tag_end(prefix, tag).is_some())
}

/// Return the byte offset just after the first `tag` in `text` that is not
/// followed by an identifier character, so `@alpha` does not match `@alphabet`.
fn bare_jsdoc_tag_end(text: &str, tag: &str) -> Option<usize> {
    text.match_indices(tag)
        .map(|(idx, _)| idx + tag.len())
        .find(|&after| after >= text.len() || !is_ident_char(text.as_bytes()[after]))
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
    if bare_jsdoc_tag_end(comment_text, "@public").is_some() {
        return true;
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

/// Reference spans proving module-mock API provenance (issue #2068 / #2082).
///
/// `mock_bindings` holds spans of references that resolve to a mock-API value
/// binding: a named `vi` import from `vitest` (any local alias), a named
/// `jest` import from `@jest/globals` (any local alias), or the unresolved
/// `jest` global that the Jest test environment injects. `vitest_namespaces`
/// holds spans of references to a `import * as ns from "vitest"` binding, so
/// `ns.vi.mock(...)` can be proven through the namespace identifier.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MockApiReferenceSpans {
    pub(crate) mock_bindings: rustc_hash::FxHashSet<Span>,
    pub(crate) vitest_namespaces: rustc_hash::FxHashSet<Span>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SemanticUsage {
    pub import_binding_usage: ImportBindingUsage,
    pub auto_import_candidates: Vec<String>,
    pub declaration_merges: Vec<fallow_types::extract::DeclarationMergeFact>,
    pub(crate) mock_api_reference_spans: MockApiReferenceSpans,
    pub(crate) module_binding_reference_spans: rustc_hash::FxHashSet<Span>,
    pub(crate) imported_call_reference_spans: rustc_hash::FxHashSet<Span>,
    /// Non-destructured `require()` bindings nothing in the file references.
    /// Moved into `import_binding_usage.unused` by
    /// [`compute_semantic_usage_for_extractor`], which is the layer that knows
    /// which of them the exported form declares.
    pub(crate) unreferenced_import_equals_bindings: Vec<String>,
    pub(crate) component_contracts: fallow_types::extract::ComponentContractFacts,
}

pub fn compute_semantic_usage_for_extractor(
    program: &Program<'_>,
    extractor: &mut ModuleInfoExtractor,
    template_used: &rustc_hash::FxHashSet<String>,
) -> SemanticUsage {
    let computed_enum_key_spans = extractor.computed_enum_key_reference_spans();
    let imported_call_candidates = extractor.imported_call_reference_candidates();
    let require_namespace_bindings = extractor.require_namespace_bindings();
    let mut semantic_usage = compute_semantic_usage_with_candidates(
        program,
        &extractor.imports,
        &extractor.exports,
        &require_namespace_bindings,
        template_used,
        SemanticReferenceCandidates {
            module_bindings: &computed_enum_key_spans,
            imported_calls: &imported_call_candidates,
        },
    );
    extractor.resolve_computed_enum_key_uses(&semantic_usage.module_binding_reference_spans);
    extractor.resolve_imported_call_sites(&semantic_usage.imported_call_reference_spans);
    report_unreferenced_import_equals_bindings(
        &mut semantic_usage,
        &extractor.exported_import_equals_names,
    );
    semantic_usage
}

/// Move every unreferenced non-destructured `require()` binding into the
/// unused import-binding list, except exported import-equals declarations.
///
/// TypeScript elides an import-equals binding nothing references, exactly as it
/// elides an unreferenced `import * as X from './x'`, so such a binding must
/// not credit the target's exports; leaving it out deleted every unused-export
/// and unused-type row on the target (issue #2365). The edge itself stays, so
/// the target is still a reachable file, which is what the namespace-import
/// twin does.
///
/// `export import X = require('./x')` is exempt: the binding is the file's
/// public API and has no local reference by construction, so it keeps the
/// whole-object credit issue #2373 gives the `import * as X; export { X }`
/// twin.
fn report_unreferenced_import_equals_bindings(
    semantic_usage: &mut SemanticUsage,
    exported_import_equals_names: &[String],
) {
    let unreferenced = std::mem::take(&mut semantic_usage.unreferenced_import_equals_bindings);
    if unreferenced.is_empty() {
        return;
    }
    let unused = &mut semantic_usage.import_binding_usage.unused;
    unused.extend(unreferenced.into_iter().filter(|name| {
        !exported_import_equals_names
            .iter()
            .any(|exported| exported == name)
    }));
    // One name, one row: the same binding name reaches this list twice when a
    // file declares it both at root and inside a namespace body, and the graph
    // reads membership rather than a count.
    unused.sort_unstable();
    unused.dedup();
}

#[derive(Clone, Copy)]
struct SemanticReferenceCandidates<'a> {
    module_bindings: &'a rustc_hash::FxHashSet<Span>,
    imported_calls: &'a rustc_hash::FxHashSet<Span>,
}

fn compute_semantic_usage_with_candidates(
    program: &Program<'_>,
    imports: &[ImportInfo],
    exports: &[ExportInfo],
    require_namespace_bindings: &[String],
    template_used: &rustc_hash::FxHashSet<String>,
    candidates: SemanticReferenceCandidates<'_>,
) -> SemanticUsage {
    use oxc_semantic::SemanticBuilder;
    use rustc_hash::FxHashSet;

    let semantic_ret = SemanticBuilder::new().with_build_nodes(true).build(program);
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
        if let Some((has_references, has_type_references, has_value_references)) =
            binding_reference_usage(scoping, &import.local_name)
        {
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

    let import_equals = classify_import_equals_bindings(
        scoping,
        require_namespace_bindings,
        template_used,
        &mut type_referenced_bindings,
        &mut value_referenced_bindings,
    );

    unused.sort_unstable();

    let mut type_referenced_bindings: Vec<String> = type_referenced_bindings.into_iter().collect();
    type_referenced_bindings.sort_unstable();

    let mut value_referenced_bindings: Vec<String> =
        value_referenced_bindings.into_iter().collect();
    value_referenced_bindings.sort_unstable();
    let mock_api_reference_spans = compute_mock_api_reference_spans(&semantic, imports, root_scope);
    let declaration_merges = declaration_merge_facts(&semantic);
    let mut module_binding_reference_spans = FxHashSet::default();
    if !candidates.module_bindings.is_empty() {
        for symbol_id in scoping.symbol_ids() {
            if scoping.symbol_scope_id(symbol_id) != root_scope {
                continue;
            }
            module_binding_reference_spans.extend(
                scoping
                    .get_resolved_references(symbol_id)
                    .filter_map(|reference| {
                        let AstKind::IdentifierReference(identifier) =
                            semantic.nodes().kind(reference.node_id())
                        else {
                            return None;
                        };
                        candidates
                            .module_bindings
                            .contains(&identifier.span)
                            .then_some(identifier.span)
                    }),
            );
        }
    }

    SemanticUsage {
        import_binding_usage: ImportBindingUsage {
            unused,
            type_referenced: type_referenced_bindings,
            value_referenced: value_referenced_bindings,
        },
        auto_import_candidates: compute_auto_import_candidates_from_semantic(scoping),
        declaration_merges,
        mock_api_reference_spans,
        module_binding_reference_spans,
        imported_call_reference_spans: imported_call_reference_spans(
            &semantic,
            imports,
            candidates.imported_calls,
        ),
        unreferenced_import_equals_bindings: import_equals.unreferenced,
        component_contracts: crate::component_contracts::collect(&semantic, imports, exports),
    }
}

/// Admit only value references to one actual runtime ESM import declaration.
fn imported_call_reference_spans(
    semantic: &oxc_semantic::Semantic<'_>,
    imports: &[ImportInfo],
    candidates: &rustc_hash::FxHashSet<Span>,
) -> rustc_hash::FxHashSet<Span> {
    let mut spans = rustc_hash::FxHashSet::default();
    if candidates.is_empty() {
        return spans;
    }
    let scoping = semantic.scoping();
    for import in imports {
        if import.is_type_only || import.local_name.is_empty() {
            continue;
        }
        let Some(symbol) = scoping.get_binding(
            scoping.root_scope_id(),
            oxc_str::Ident::from(import.local_name.as_str()),
        ) else {
            continue;
        };
        let mut declarations = scoping.symbol_declarations(symbol);
        let Some(declaration) = declarations.next() else {
            continue;
        };
        if declarations.next().is_some()
            || !matches!(
                semantic.nodes().kind(declaration),
                AstKind::ImportSpecifier(_)
                    | AstKind::ImportDefaultSpecifier(_)
                    | AstKind::ImportNamespaceSpecifier(_)
            )
        {
            continue;
        }
        spans.extend(
            scoping
                .get_resolved_references(symbol)
                .filter_map(|reference| {
                    if !reference.is_value() {
                        return None;
                    }
                    let AstKind::IdentifierReference(identifier) =
                        semantic.nodes().kind(reference.node_id())
                    else {
                        return None;
                    };
                    candidates
                        .contains(&identifier.span)
                        .then_some(identifier.span)
                }),
        );
    }
    spans
}

/// Verdicts [`classify_import_equals_bindings`] reaches per binding name.
#[derive(Default)]
struct ImportEqualsClassification {
    /// Names with no resolved reference anywhere in the file.
    unreferenced: Vec<String>,
}

/// Aggregate references for every binding with `local_name`, including
/// namespace and ambient-module scopes. Module graph binding lists are
/// name-keyed, so duplicate spellings fail closed: any live binding keeps the
/// shared edge classified instead of declaring it unused.
fn binding_reference_usage(
    scoping: &oxc_semantic::Scoping,
    local_name: &str,
) -> Option<(bool, bool, bool)> {
    let mut found_binding = false;
    let mut has_references = false;
    let mut has_type_references = false;
    let mut has_value_references = false;
    for symbol_id in scoping
        .symbol_ids()
        .filter(|symbol_id| scoping.symbol_name(*symbol_id) == local_name)
    {
        found_binding = true;
        for reference in scoping.get_resolved_references(symbol_id) {
            has_references = true;
            has_type_references |= reference.is_type();
            has_value_references |= reference.is_value();
        }
    }
    if found_binding
        && let Some(reference_ids) = scoping.root_unresolved_references().get(local_name)
    {
        for reference_id in reference_ids {
            let reference = scoping.get_reference(*reference_id);
            has_references = true;
            has_type_references |= reference.is_type();
            has_value_references |= reference.is_value();
        }
    }
    found_binding.then_some((has_references, has_type_references, has_value_references))
}

/// Classify non-destructured `require()` bindings for type and value usage and
/// report which names nothing in the file references.
///
/// The binding lives in both the type and the value namespace, the same way
/// `import * as X from './y'` does, but the require path records it outside
/// `imports`, so the caller's `imports` loop never sees it. Without a
/// type-space entry, `X.SomeType` in an annotation leaves the target's type
/// exports uncredited (issue #2365).
///
/// A name with no resolved reference is returned as unreferenced, the same
/// verdict the `imports` loop reaches for an unreferenced `import * as X`: the
/// declaration is erased by TypeScript, so it must not buy the target a
/// whole-object credit. A name used only by a framework template is referenced,
/// matching the `template_used` skip the `imports` loop applies.
///
fn classify_import_equals_bindings(
    scoping: &oxc_semantic::Scoping,
    import_equals_bindings: &[String],
    template_used: &rustc_hash::FxHashSet<String>,
    type_referenced_bindings: &mut rustc_hash::FxHashSet<String>,
    value_referenced_bindings: &mut rustc_hash::FxHashSet<String>,
) -> ImportEqualsClassification {
    if import_equals_bindings.is_empty() {
        return ImportEqualsClassification::default();
    }

    let mut classification = ImportEqualsClassification::default();
    for local_name in import_equals_bindings {
        if local_name.is_empty() {
            continue;
        }
        let Some((has_references, has_type_references, has_value_references)) =
            binding_reference_usage(scoping, local_name)
        else {
            continue;
        };
        if !has_references {
            if !template_used.contains(local_name) {
                classification.unreferenced.push(local_name.clone());
            }
            continue;
        }
        if has_type_references {
            type_referenced_bindings.insert(local_name.clone());
        }
        if has_value_references {
            value_referenced_bindings.insert(local_name.clone());
        }
    }
    classification
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MergeDeclarationKind {
    Interface,
    Class,
    Function,
    Enum,
    Namespace,
}

fn declaration_merge_facts(
    semantic: &oxc_semantic::Semantic<'_>,
) -> Vec<fallow_types::extract::DeclarationMergeFact> {
    use fallow_types::extract::DeclarationMergeFact;

    let scoping = semantic.scoping();
    let mut groups = Vec::new();
    for symbol_id in scoping.symbol_ids() {
        let declarations: Vec<_> = scoping
            .symbol_declarations(symbol_id)
            .filter_map(|node_id| merge_declaration(semantic.nodes().kind(node_id)))
            .collect();
        if declarations.len() < 2 {
            continue;
        }
        let mut selected = Vec::new();
        for (index, (kind, span)) in declarations.iter().enumerate() {
            // Self-compatible kinds (interface, enum, namespace) would otherwise
            // always select themselves, grouping declarations that cannot merge
            // with each other (`interface Foo` plus `enum Foo`).
            if declarations
                .iter()
                .enumerate()
                .any(|(other_index, (other, _))| {
                    other_index != index && compatible_merge(*kind, *other)
                })
            {
                selected.push((span.start, span.end));
            }
        }
        selected.sort_unstable();
        selected.dedup();
        if selected.len() > 1 {
            groups.push(DeclarationMergeFact {
                export_spans: selected,
            });
        }
    }
    groups.sort_unstable_by_key(|group| group.export_spans[0]);
    groups
}

fn merge_declaration(kind: AstKind<'_>) -> Option<(MergeDeclarationKind, Span)> {
    match kind {
        AstKind::TSInterfaceDeclaration(declaration) => {
            Some((MergeDeclarationKind::Interface, declaration.id.span))
        }
        AstKind::Class(declaration) => declaration
            .id
            .as_ref()
            .map(|id| (MergeDeclarationKind::Class, id.span)),
        AstKind::Function(declaration) => declaration
            .id
            .as_ref()
            .map(|id| (MergeDeclarationKind::Function, id.span)),
        AstKind::TSEnumDeclaration(declaration) if !declaration.r#const => {
            Some((MergeDeclarationKind::Enum, declaration.id.span))
        }
        AstKind::TSNamespaceDeclaration(declaration) => {
            Some((MergeDeclarationKind::Namespace, declaration.id.span))
        }
        _ => None,
    }
}

const fn compatible_merge(left: MergeDeclarationKind, right: MergeDeclarationKind) -> bool {
    use MergeDeclarationKind::{Class, Enum, Function, Interface, Namespace};

    matches!(
        (left, right),
        (Interface, Interface | Class | Namespace)
            | (Class, Interface | Namespace)
            | (Function, Namespace)
            | (Enum, Enum | Namespace)
            | (Namespace, Interface | Class | Function | Enum | Namespace)
    )
}

fn compute_mock_api_reference_spans(
    semantic: &oxc_semantic::Semantic<'_>,
    imports: &[ImportInfo],
    root_scope: oxc_semantic::ScopeId,
) -> MockApiReferenceSpans {
    let scoping = semantic.scoping();
    let mut spans = MockApiReferenceSpans::default();

    let collect_binding_spans = |local_name: &str, out: &mut rustc_hash::FxHashSet<Span>| {
        let Some(symbol_id) = scoping.get_binding(root_scope, oxc_str::Ident::from(local_name))
        else {
            return;
        };
        out.extend(
            scoping
                .get_resolved_references(symbol_id)
                .filter_map(|reference| {
                    let AstKind::IdentifierReference(identifier) =
                        semantic.nodes().kind(reference.node_id())
                    else {
                        return None;
                    };
                    Some(identifier.span)
                }),
        );
    };

    for import in imports {
        if import.is_type_only || import.local_name.is_empty() {
            continue;
        }
        let is_vi_binding = import.source == "vitest"
            && matches!(&import.imported_name, ImportedName::Named(name) if name == "vi");
        let is_jest_binding = import.source == "@jest/globals"
            && matches!(&import.imported_name, ImportedName::Named(name) if name == "jest");
        let is_vitest_namespace =
            import.source == "vitest" && matches!(&import.imported_name, ImportedName::Namespace);

        if is_vi_binding || is_jest_binding {
            collect_binding_spans(&import.local_name, &mut spans.mock_bindings);
        } else if is_vitest_namespace {
            collect_binding_spans(&import.local_name, &mut spans.vitest_namespaces);
        }
    }

    // The Jest test environment injects `jest` as a global, so unresolved
    // value references named `jest` count as mock-API provenance. Masking only
    // ever applies to files the plugin layer classified as test entry points,
    // which grounds this in the existing Jest test-root detection. Unresolved
    // `vi` stays unproven on purpose (unchanged from #2068): Vitest exposes
    // `vi` as a global only under `globals: true`, and without reading that
    // config the safe direction is to abstain.
    for (name, reference_ids) in scoping.root_unresolved_references() {
        if name.as_str() != "jest" {
            continue;
        }
        spans
            .mock_bindings
            .extend(reference_ids.iter().filter_map(|reference_id| {
                let reference = scoping.get_reference(*reference_id);
                if !reference.is_value() {
                    return None;
                }
                let AstKind::IdentifierReference(identifier) =
                    semantic.nodes().kind(reference.node_id())
                else {
                    return None;
                };
                Some(identifier.span)
            }));
    }

    spans
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
///
/// `import_equals_bindings` carries the `import X = require('./x')` locals the
/// extractor collected for the same program. They live outside `imports`, so
/// without them the require-derived lane would be absent on this path and such
/// a binding would keep crediting its target on a script the caller re-parses
/// (a Vue `generic="..."` block). Pass `&[]` when the program has none.
pub fn compute_import_binding_usage(
    program: &Program<'_>,
    imports: &[ImportInfo],
    import_equals_bindings: &[String],
    template_used: &rustc_hash::FxHashSet<String>,
) -> ImportBindingUsage {
    let mut semantic_usage = compute_semantic_usage_with_candidates(
        program,
        imports,
        &[],
        import_equals_bindings,
        template_used,
        SemanticReferenceCandidates {
            module_bindings: &rustc_hash::FxHashSet::default(),
            imported_calls: &rustc_hash::FxHashSet::default(),
        },
    );
    // The exported form is exempt, exactly as it is on the extractor path, but
    // `export import X = require('./x')` is not a `<script setup>` spelling: no
    // name is exempted here.
    report_unreferenced_import_equals_bindings(&mut semantic_usage, &[]);
    semantic_usage.import_binding_usage
}

#[cfg(test)]
mod tests {
    use super::{
        advance_jsdoc_brace_stack, classify_jsdoc_visibility_tag, parse_source_to_module,
        scan_jsdoc_import_tags_in, scan_jsdoc_imports_in,
    };
    use fallow_types::discover::FileId;
    use fallow_types::extract::{ImportInfo, ImportedName, VisibilityTag};
    use std::path::Path;

    #[test]
    fn classify_jsdoc_visibility_tag_requires_a_bare_tag() {
        let cases = [
            (" * @public", Some(VisibilityTag::Public)),
            (" * @api public", Some(VisibilityTag::Public)),
            (" * @publicly", None),
            (" * @apipublic", None),
            (" * public", None),
            (" * @internal", Some(VisibilityTag::Internal)),
            (" * @internal-only", Some(VisibilityTag::Internal)),
            (" * @internalizer", None),
            (" * @internalFoo", None),
            (" * internal", None),
            (" * @beta", Some(VisibilityTag::Beta)),
            (" * @betaware", None),
            (" * @beta_x", None),
            (" * beta", None),
            ("@alpha", Some(VisibilityTag::Alpha)),
            ("@alpha Some description", Some(VisibilityTag::Alpha)),
            ("@alphabet", None),
            (" * alpha", None),
            (" * @expected-unused", Some(VisibilityTag::ExpectedUnused)),
            (" * @expected-unusedX", None),
        ];
        for (text, expected) in cases {
            let actual = classify_jsdoc_visibility_tag(text).map(|(tag, _)| tag);
            assert_eq!(actual, expected, "{text:?}");
        }
    }

    #[test]
    fn classify_jsdoc_visibility_tag_keeps_the_expected_unused_reason() {
        assert_eq!(
            classify_jsdoc_visibility_tag(" * @expected-unused -- kept for the plugin API"),
            Some((
                VisibilityTag::ExpectedUnused,
                Some("kept for the plugin API".to_string())
            ))
        );
        assert_eq!(
            classify_jsdoc_visibility_tag(" * @expected-unused"),
            Some((VisibilityTag::ExpectedUnused, None))
        );
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

    fn scan_tags(body: &str) -> Vec<(String, ImportedName)> {
        let mut imports = Vec::new();
        let mut namespaces = Vec::new();
        scan_jsdoc_import_tags_in(body, &mut imports, &mut namespaces);
        assert!(namespaces.is_empty());
        assert!(imports.iter().all(|import| import.is_type_only));
        assert!(imports.iter().all(|import| import.local_name.is_empty()));
        imports
            .into_iter()
            .map(|import| (import.source, import.imported_name))
            .collect()
    }

    fn named(source: &str, name: &str) -> (String, ImportedName) {
        (source.to_string(), ImportedName::Named(name.to_string()))
    }

    #[test]
    fn scan_jsdoc_import_tag_named_bindings() {
        assert_eq!(
            scan_tags(" @import { A, B as C, type D, 'e-f' as E } from '../types/foo' "),
            vec![
                named("../types/foo", "A"),
                named("../types/foo", "B"),
                named("../types/foo", "D"),
                named("../types/foo", "e-f"),
            ]
        );
    }

    #[test]
    fn scan_jsdoc_import_tag_namespace_default_and_mixed() {
        assert_eq!(
            scan_tags(" @import Def from './def' "),
            vec![("./def".to_string(), ImportedName::Default)]
        );
        assert_eq!(
            scan_tags(" @import Def, { $A, default as B } from './mixed' "),
            vec![
                ("./mixed".to_string(), ImportedName::Default),
                named("./mixed", "$A"),
                ("./mixed".to_string(), ImportedName::Default),
            ]
        );
        assert_eq!(
            scan_tags(" @import {} from './empty' "),
            vec![("./empty".to_string(), ImportedName::SideEffect)]
        );
    }

    fn jsdoc_tag_imports(source: &str) -> Vec<(String, ImportedName)> {
        let info = parse_source_to_module(FileId(0), Path::new("src/index.js"), source, 0, false);
        info.imports
            .into_iter()
            .filter(|import| import.is_type_only && import.local_name.is_empty())
            .map(|import| (import.source, import.imported_name))
            .collect()
    }

    #[test]
    fn jsdoc_namespace_import_tag_credits_only_the_members_in_types() {
        let source = r#"/** @import * as ns from "./ns" */
/** @import * as unread from './unread' */
/**
 * Read ns.Prose in a sentence: no credit.
 * @param {ns.Shape | ns.Line} a
 * @returns {ns.Shape}
 */
export function draw(a) { return a; }
/** @type {Array<ns.Point>} */
export const points = [];
"#;
        assert_eq!(
            jsdoc_tag_imports(source),
            vec![
                ("./ns".to_string(), ImportedName::SideEffect),
                ("./unread".to_string(), ImportedName::SideEffect),
                named("./ns", "Shape"),
                named("./ns", "Line"),
                named("./ns", "Point"),
            ]
        );
    }

    #[test]
    fn jsdoc_namespace_import_tag_keeps_default_binding() {
        assert_eq!(
            jsdoc_tag_imports(
                "/** @import Def, * as ns from './mod' */
/** @type {ns.A} */
export const a = 1;
"
            ),
            vec![
                ("./mod".to_string(), ImportedName::Default),
                ("./mod".to_string(), ImportedName::SideEffect),
                named("./mod", "A"),
            ]
        );
    }

    #[test]
    fn scan_jsdoc_import_tag_spans_lines_and_tags() {
        let body = "\n * @import {\n *   A,\n *   B,\n * } from './multi'\n * @import { C } from './next'\n * @param {A} a\n ";
        assert_eq!(
            scan_tags(body),
            vec![
                named("./multi", "A"),
                named("./multi", "B"),
                named("./next", "C"),
            ]
        );
    }

    #[test]
    fn scan_jsdoc_import_tag_ignores_non_tags_and_bad_clauses() {
        for body in [
            " Use @import { A } from './prose' in a sentence ",
            " @imports { A } from './plural' ",
            " @import { A } './missing-from' ",
            " @import { A } from '' ",
            " @import { A from './unclosed' ",
            " @import { A }\n * @param {A} from './next-tag'",
            " @import { A } from './truncated",
            " @import",
            " @import * from './no-alias' ",
        ] {
            assert!(scan_tags(body).is_empty(), "{body:?}");
        }
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
            is_type_only_star: false,
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
