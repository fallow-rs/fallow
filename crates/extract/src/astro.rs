//! Astro component frontmatter extraction.
//!
//! Extracts the TypeScript code between `---` delimiters in `.astro` files,
//! plus `<script src="...">` references and inline `<script>` import
//! statements from the template body. Astro bundles per-component client
//! scripts at build time when the script tag opts into Astro processing, so
//! both processed reference shapes must keep their targets reachable.

use std::path::Path;
use std::sync::LazyLock;

use oxc_allocator::Allocator;
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use rustc_hash::FxHashSet;

use crate::asset_url::normalize_asset_url;
use crate::html::is_remote_url;
use crate::sfc::{SfcScript, SourceRegion};
use crate::source_map::ExtractionResult;
use crate::visitor::ModuleInfoExtractor;
use crate::{ImportInfo, ImportedName, ModuleInfo};
use fallow_types::discover::FileId;

/// Regex to extract Astro frontmatter (content between `---` delimiters at file start).
static ASTRO_FRONTMATTER_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)\A\s*---[ \t]*\r?\n(?P<body>.*?\r?\n)---"));

/// Regex matching `<script>` blocks in the Astro template body. Captures the
/// attribute list and the body so callers can decide whether to follow `src=`
/// or parse the inline body as TypeScript.
static SCRIPT_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?is)<script\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>(?P<body>[\s\S]*?)</script>"#,
    )
});

/// Regex matching opening `<script>` tags in the Astro template body.
static SCRIPT_OPEN_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?is)<script\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>"#)
});

/// Regex matching `<style>` blocks in the Astro template body.
static STYLE_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?is)<style\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>(?P<body>[\s\S]*?)</style>"#,
    )
});

/// Regex detecting and capturing a `src` attribute on a script tag.
static SRC_ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"(?i)(?:^|\s)src\s*=\s*["'](?P<src>[^"']+)["']"#));

/// Regex matching HTML comments for stripping before template scanning.
static HTML_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)<!--.*?-->"));

/// Regex matching JavaScript identifier tokens for template-usage scanning.
static TEMPLATE_IDENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"[A-Za-z_$][A-Za-z0-9_$]*"));

/// Regex matching the root identifier of an opening/closing markup tag
/// (`<Header`, `</Header`, `<ui.Card` -> `ui`). The capture stops at the first
/// non-identifier byte, so a dotted namespace tag yields its root binding.
static TEMPLATE_TAG_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"</?\s*([A-Za-z_$][A-Za-z0-9_$]*)"));

/// Regex matching the `define:vars=` directive prefix. The expression object
/// that follows (`define:vars={{ a, b: c }}`) passes frontmatter values into a
/// scoped `<style>` / `<script>` / element, so the identifiers it references are
/// real prop / import uses.
static DEFINE_VARS_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"define:vars\s*="));

/// Build the disjoint, ascending list of byte ranges to mask when scanning the
/// Astro template: `<script>` / `<style>` block bodies and HTML comments. The
/// three `find_iter` passes are concatenated (not globally sorted, and their
/// ranges overlap when e.g. an HTML comment wraps a `<script>`), then sorted by
/// start and merged into a non-overlapping ascending list so membership can be
/// tested by binary search (`pos_in_masked_ranges`) instead of a linear scan
/// over every range per position. The merged union covers exactly the same byte
/// positions as the raw ranges, so masking decisions are byte-identical.
fn build_masked_ranges(template: &str) -> Vec<(usize, usize)> {
    let mut masked: Vec<(usize, usize)> = Vec::new();
    masked.extend(
        SCRIPT_BLOCK_RE
            .find_iter(template)
            .map(|m| (m.start(), m.end())),
    );
    masked.extend(
        STYLE_BLOCK_RE
            .find_iter(template)
            .map(|m| (m.start(), m.end())),
    );
    masked.extend(
        HTML_COMMENT_RE
            .find_iter(template)
            .map(|m| (m.start(), m.end())),
    );
    masked.sort_unstable_by_key(|&(start, _)| start);
    // Merge overlapping / touching ranges so the result is disjoint and ascending,
    // the invariant `pos_in_masked_ranges`'s binary search relies on.
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(masked.len());
    for (start, end) in masked {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

/// Test whether `pos` falls inside any masked range. `ranges` must be the disjoint
/// ascending list produced by `build_masked_ranges`; membership is a binary search
/// (`partition_point`) rather than a linear `.any()` scan, keeping the per-position
/// cost logarithmic when a template carries many masked regions. Because the list
/// is disjoint and ascending, only the last range with `start <= pos` can contain
/// it, so a single bounds check on that candidate decides membership.
fn pos_in_masked_ranges(ranges: &[(usize, usize)], pos: usize) -> bool {
    let idx = ranges.partition_point(|&(start, _)| start <= pos);
    idx > 0 && pos < ranges[idx - 1].1
}

/// Collect the set of identifier names that appear USED in the Astro template
/// markup, so the frontmatter semantic pass does not mark a template-only-used
/// import (a rendered `<Header/>` component, or a `{fmt(x)}` expression binding)
/// as unused. Only two genuine usage positions are credited: component tag roots
/// (`<Header>`, `<ui.Card>`) and identifiers inside `{ ... }` expression regions
/// (attribute values and text expressions). Raw text content is deliberately NOT
/// scanned, so prose that happens to spell a component name (`references Header`)
/// does not credit the import. `<script>` / `<style>` blocks and HTML comments
/// are masked first so their identifiers (client-script locals, CSS) never
/// credit a frontmatter import. The complement (a frontmatter import referenced
/// in NEITHER the frontmatter script NOR a tag/expression position) is the
/// genuinely-dead case the `unused-import` / `unrendered-component` arms surface.
fn collect_astro_template_used_names(template: &str) -> FxHashSet<String> {
    let mut used = FxHashSet::default();
    if template.is_empty() {
        return used;
    }
    // Byte ranges of `<script>` / `<style>` bodies and HTML comments. Positions
    // inside any of these are skipped rather than mutating the source (mutation
    // would corrupt multibyte UTF-8 inside a masked range).
    let masked = build_masked_ranges(template);
    let is_masked = |pos: usize| pos_in_masked_ranges(&masked, pos);

    // Component tag roots.
    for cap in TEMPLATE_TAG_RE.captures_iter(template) {
        if let Some(root) = cap.get(1)
            && !is_masked(root.start())
        {
            used.insert(root.as_str().to_string());
        }
    }

    // Identifiers inside `{ ... }` expression regions.
    collect_brace_expression_idents(template, &is_masked, &mut used);

    // Identifiers inside `define:vars={ ... }` directive values. These sit in the
    // opening tag of a `<style>` / `<script>` block (masked above), so they are
    // scanned separately over the unmasked template.
    collect_define_vars_idents(template, &mut used);

    used
}

/// Given `open` = the index of a `{` byte in `bytes`, return the index of the
/// matching `}` (exclusive region end), or `bytes.len()` if unterminated. The
/// scan is string-aware so braces inside quoted strings do not shift the depth.
/// All boundary bytes (`{`, `}`, quotes, backslash) are ASCII, so the returned
/// index always lands on a char boundary.
fn brace_body_end(bytes: &[u8], open: usize) -> usize {
    let len = bytes.len();
    let mut depth = 1usize;
    let mut j = open + 1;
    let mut quote: Option<u8> = None;
    while j < len && depth > 0 {
        let c = bytes[j];
        if let Some(q) = quote {
            if c == b'\\' {
                j += 1; // skip the escaped byte
            } else if c == q {
                quote = None;
            }
        } else {
            match c {
                b'"' | b'\'' | b'`' => quote = Some(c),
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
        }
        j += 1;
    }
    // `j` is one past the matching `}` (or `len` if unterminated); the region body
    // excludes the closing brace.
    if depth == 0 { j - 1 } else { len }
}

/// Credit every identifier inside each `define:vars={ ... }` directive value.
/// Astro's `define:vars` injects frontmatter values into a scoped `<style>` /
/// `<script>` (or element) as the expression object, so a prop or import used
/// only there is a genuine use. Because the directive sits inside the opening tag
/// of a `<style>` / `<script>` block that the main scanner masks, this runs over
/// the unmasked template and matches the directive directly. Over-crediting (e.g.
/// an identifier-shaped fragment of a quoted CSS-variable key) only suppresses a
/// finding, never creates one.
fn collect_define_vars_idents(template: &str, used: &mut FxHashSet<String>) {
    let bytes = template.as_bytes();
    for m in DEFINE_VARS_RE.find_iter(template) {
        // Skip whitespace after `define:vars=` to the JSX expression `{`.
        let mut k = m.end();
        while k < bytes.len() && bytes[k].is_ascii_whitespace() {
            k += 1;
        }
        if k >= bytes.len() || bytes[k] != b'{' {
            continue;
        }
        let end = brace_body_end(bytes, k);
        if let Some(region) = template.get(k + 1..end) {
            for token in TEMPLATE_IDENT_RE.find_iter(region) {
                used.insert(token.as_str().to_string());
            }
        }
    }
}

/// Collect every identifier inside each top-level `{ ... }` expression region of
/// the Astro markup. The scan is depth-counted and string-aware so object
/// literals (`{ class: x }`) and strings containing braces do not break region
/// boundaries. Region boundaries (`{`, `}`, and quote bytes) are ASCII, so the
/// `template.get(..)` slice always lands on char boundaries even when the region
/// body contains multibyte text.
fn collect_brace_expression_idents(
    template: &str,
    is_masked: &impl Fn(usize) -> bool,
    used: &mut FxHashSet<String>,
) {
    let bytes = template.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] != b'{' || is_masked(i) {
            i += 1;
            continue;
        }
        let start = i + 1;
        let region_end = brace_body_end(bytes, i);
        if let Some(region) = template.get(start..region_end) {
            for token in TEMPLATE_IDENT_RE.find_iter(region) {
                used.insert(token.as_str().to_string());
            }
        }
        i = region_end.max(start);
    }
}

/// Collect the body text of every top-level `{ ... }` expression region in the
/// Astro markup, skipping `<script>` / `<style>` / HTML-comment ranges. Mirrors
/// `collect_brace_expression_idents`'s scan but returns each region body verbatim
/// (for re-parsing) instead of tokenizing identifiers. See issue #1713.
fn collect_template_expression_regions(template: &str) -> Vec<String> {
    let mut regions = Vec::new();
    if template.is_empty() {
        return regions;
    }
    let masked = build_masked_ranges(template);
    let is_masked = |pos: usize| pos_in_masked_ranges(&masked, pos);

    let bytes = template.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] != b'{' || is_masked(i) {
            i += 1;
            continue;
        }
        let start = i + 1;
        let region_end = brace_body_end(bytes, i);
        if let Some(region) = template.get(start..region_end) {
            let trimmed = region.trim();
            if !trimmed.is_empty() {
                regions.push(trimmed.to_string());
            }
        }
        i = region_end.max(start);
    }
    regions
}

/// Run the member-recording visitor over each Astro template `{ ... }` expression
/// region so `.map()` / `.forEach()` / `for...of` iteration bindings in the
/// template body credit the element-class members the same way the frontmatter
/// does (issue #1713). Each region is parsed as a standalone TSX expression
/// (Astro template expressions contain JSX), visited with a fresh extractor
/// seeded with the frontmatter's array element-type map so
/// `bind_iterable_callback_parameter` can resolve the receiver's element class,
/// and the re-emitted class-qualified member accesses are appended to `info`.
///
/// Over-credit only: the drained accesses are span-less name pairs (`Util.getter`)
/// that the analyze layer resolves through the frontmatter imports; they can only
/// suppress a false `unused-class-member`, never introduce a finding. A region
/// that fails to parse or resolves no element type contributes nothing.
fn extend_template_expression_member_accesses(
    info: &mut ModuleInfo,
    template: &str,
    frontmatter_array_element_types: &rustc_hash::FxHashMap<String, String>,
) {
    if frontmatter_array_element_types.is_empty() {
        return;
    }
    for region in collect_template_expression_regions(template) {
        // Wrap as a parenthesized expression statement so the region parses as a
        // valid TSX program. Spans are irrelevant: only span-less member accesses
        // are drained.
        let wrapped = format!("({region});");
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &wrapped, SourceType::tsx()).parse();
        if parser_return.panicked {
            continue;
        }
        let mut extractor = ModuleInfoExtractor::new();
        extractor.seed_array_binding_element_types(frontmatter_array_element_types);
        extractor.visit_program(&parser_return.program);
        info.member_accesses
            .extend(extractor.take_resolved_iteration_member_accesses());
    }
}

/// Extract frontmatter from an Astro component.
pub fn extract_astro_frontmatter(source: &str) -> Option<SfcScript> {
    ASTRO_FRONTMATTER_RE.captures(source).map(|cap| {
        let body_match = cap.name("body");
        SfcScript {
            body: body_match.map_or("", |m| m.as_str()).to_string(),
            is_typescript: true, // Astro frontmatter is always TS-compatible
            is_jsx: false,
            byte_offset: body_match.map_or(0, |m| m.start()),
            src: None,
            src_span: None,
            is_setup: false,
            is_context_module: false,
            generic_attr: None,
        }
    })
}

/// Extract Astro template markup regions while preserving original byte offsets.
///
/// Frontmatter, `<script>` blocks, `<style>` blocks, and HTML comments are
/// excluded. Inline scripts are deliberately not tokenized as markup; script
/// handling stays with the Astro extraction path that understands processed
/// Astro client scripts.
#[must_use]
pub fn extract_astro_template_regions(source: &str) -> Vec<SourceRegion> {
    let mut ranges = Vec::new();
    if let Some(frontmatter) = ASTRO_FRONTMATTER_RE.find(source) {
        ranges.push((frontmatter.start(), frontmatter.end()));
    }
    ranges.extend(
        SCRIPT_BLOCK_RE
            .find_iter(source)
            .map(|m| (m.start(), m.end())),
    );
    ranges.extend(
        STYLE_BLOCK_RE
            .find_iter(source)
            .map(|m| (m.start(), m.end())),
    );
    ranges.extend(
        HTML_COMMENT_RE
            .find_iter(source)
            .map(|m| (m.start(), m.end())),
    );
    ranges.sort_unstable_by_key(|(start, _)| *start);
    ranges_to_gaps(source, &ranges)
}

/// Extract Astro `<style>` block bodies while preserving original byte offsets.
#[must_use]
pub fn extract_astro_style_regions(source: &str) -> Vec<SourceRegion> {
    let comment_ranges: Vec<(usize, usize)> = HTML_COMMENT_RE
        .find_iter(source)
        .map(|m| (m.start(), m.end()))
        .collect();

    STYLE_BLOCK_RE
        .captures_iter(source)
        .filter(|cap| {
            let start = cap.get(0).map_or(0, |m| m.start());
            !comment_ranges
                .iter()
                .any(|&(cs, ce)| start >= cs && start < ce)
        })
        .filter_map(|cap| {
            let body = cap.name("body")?;
            let text = body.as_str();
            if text.trim().is_empty() {
                return None;
            }
            Some(SourceRegion {
                body: text.to_string(),
                byte_offset: body.start(),
            })
        })
        .collect()
}

fn ranges_to_gaps(source: &str, ranges: &[(usize, usize)]) -> Vec<SourceRegion> {
    let mut regions = Vec::new();
    let mut cursor = 0;
    for &(start, end) in ranges {
        if start > cursor {
            push_region(source, cursor, start, &mut regions);
        }
        cursor = cursor.max(end);
    }
    if cursor < source.len() {
        push_region(source, cursor, source.len(), &mut regions);
    }
    regions
}

fn push_region(source: &str, start: usize, end: usize, regions: &mut Vec<SourceRegion>) {
    let Some(body) = source.get(start..end) else {
        return;
    };
    if body.trim().is_empty() {
        return;
    }
    regions.push(SourceRegion {
        body: body.to_string(),
        byte_offset: start,
    });
}

pub(crate) fn is_astro_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "astro")
}

struct AstroFrontmatterAnalysis {
    extractor: ModuleInfoExtractor,
    semantic_usage: crate::parse::SemanticUsage,
    props_harvest: crate::sfc_props::DefinePropsHarvest,
    complexity: Vec<fallow_types::extract::FunctionComplexity>,
}

/// Parse an Astro file by extracting the frontmatter section, plus any
/// `<script src="...">` references and inline `<script>` import statements
/// in the template body.
pub(crate) fn parse_astro_to_module(
    file_id: FileId,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    let parsed_suppressions = crate::suppress::parse_suppressions_from_source(source);
    let line_offsets = fallow_types::extract::compute_line_offsets(source);

    let frontmatter = extract_astro_frontmatter(source);
    let template_offset = frontmatter
        .as_ref()
        .map_or(0, |script| script.byte_offset + script.body.len());
    let template = source.get(template_offset..).unwrap_or("");

    // Names used in the template markup (rendered components, expression
    // bindings). Passed to the frontmatter semantic pass so a template-only-used
    // import is not falsely classified as an unused binding.
    let template_used = collect_astro_template_used_names(template);

    let frontmatter_offset = frontmatter.as_ref().map_or(0, |script| script.byte_offset);

    let AstroFrontmatterAnalysis {
        mut extractor,
        semantic_usage,
        props_harvest,
        complexity: frontmatter_complexity,
    } = analyze_astro_frontmatter(
        frontmatter.as_ref(),
        &template_used,
        need_complexity,
        &line_offsets,
    );

    extend_imports_from_template(&mut extractor.imports, template, template_offset);

    // Capture the frontmatter's array/reactive-array element-type map before
    // `into_module_info` consumes the extractor, so the template-expression pass
    // below can type a `{utils.map((util) => util.getter)}` callback param to the
    // `const utils: Util[]` element class (issue #1713).
    let frontmatter_array_element_types = extractor.array_binding_element_types().clone();

    let mut info = extractor.into_module_info(file_id, content_hash, parsed_suppressions);

    // Run the member-recording visitor over the Astro template `{...}` expression
    // regions so `.map()` / `.forEach()` / `for...of` iteration-binding member
    // accesses in the TEMPLATE body are credited the same as the frontmatter
    // (issue #1713). Over-credit only: it can only re-emit a class-qualified
    // member access that later suppresses a false `unused-class-member`, never a
    // new finding.
    extend_template_expression_member_accesses(
        &mut info,
        template,
        &frontmatter_array_element_types,
    );
    // Astro-only: thread the frontmatter binding usage onto the module so
    // `referenced_import_bindings` (derived from `imports` minus
    // `unused_import_bindings` in `release_resolution_payload`) reflects ACTUAL
    // use. `auto_import_candidates` is intentionally left empty: Astro has no
    // Nuxt-style convention auto-import, so seeding it would be inert at best.
    info.unused_import_bindings = semantic_usage.import_binding_usage.unused;
    info.type_referenced_import_bindings = semantic_usage.import_binding_usage.type_referenced;
    info.value_referenced_import_bindings = semantic_usage.import_binding_usage.value_referenced;
    apply_astro_props(
        &mut info,
        props_harvest,
        &template_used,
        template,
        frontmatter_offset,
    );
    // Health complexity (only when requested): the frontmatter functions (scored
    // above) plus a synthetic `<template>` entry for the markup `{ ... }`
    // expression / iteration complexity, mirroring the Vue/Svelte synthetic
    // template entry. Folds into the existing complexity aggregate; no new rule.
    info.complexity.extend(frontmatter_complexity);
    if need_complexity {
        info.complexity
            .extend(crate::template_complexity::compute_astro_template_complexity(source));
    }
    info.line_offsets = line_offsets;
    info
}

fn analyze_astro_frontmatter(
    frontmatter: Option<&SfcScript>,
    template_used: &FxHashSet<String>,
    need_complexity: bool,
    line_offsets: &[u32],
) -> AstroFrontmatterAnalysis {
    let Some(script) = frontmatter else {
        return empty_astro_frontmatter_analysis();
    };
    let source_type = SourceType::ts();
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, &script.body, source_type).parse();
    let extractor = extract_astro_frontmatter_module_info(script, &parser_return.program);
    let semantic_usage = crate::parse::compute_semantic_usage(
        &parser_return.program,
        &extractor.imports,
        template_used,
    );
    let props_harvest = crate::sfc_props::harvest_astro_props(&parser_return.program);
    let complexity = if need_complexity {
        compute_astro_frontmatter_complexity(
            &parser_return.program,
            &script.body,
            script.byte_offset,
            line_offsets,
        )
    } else {
        Vec::new()
    };
    AstroFrontmatterAnalysis {
        extractor,
        semantic_usage,
        props_harvest,
        complexity,
    }
}

fn extract_astro_frontmatter_module_info(
    script: &SfcScript,
    program: &oxc_ast::ast::Program<'_>,
) -> ModuleInfoExtractor {
    let mut extractor = ModuleInfoExtractor::new();
    extractor.visit_program(program);
    let extraction = ExtractionResult::contiguous(&script.body, script.byte_offset);
    extractor.remap_spans_with(|span| extraction.remap_span(span));
    extractor
}

fn empty_astro_frontmatter_analysis() -> AstroFrontmatterAnalysis {
    AstroFrontmatterAnalysis {
        extractor: ModuleInfoExtractor::new(),
        semantic_usage: crate::parse::SemanticUsage::default(),
        props_harvest: crate::sfc_props::DefinePropsHarvest::default(),
        complexity: Vec::new(),
    }
}

/// Score the frontmatter's JS functions (cyclomatic/cognitive), remapping each
/// function's line/col from the frontmatter-body coordinate space onto the
/// `.astro` source. Mirrors the Vue/Svelte `sfc::translate_script_complexity`
/// path so a complex `.astro` frontmatter contributes to the health complexity
/// aggregate the same as an SFC `<script>`.
fn compute_astro_frontmatter_complexity(
    program: &oxc_ast::ast::Program<'_>,
    body: &str,
    body_byte_offset: usize,
    source_line_offsets: &[u32],
) -> Vec<fallow_types::extract::FunctionComplexity> {
    let body_line_offsets = fallow_types::extract::compute_line_offsets(body);
    let mut complexity = crate::complexity::compute_complexity(program, body, &body_line_offsets);
    let (body_start_line, body_start_col) = fallow_types::extract::byte_offset_to_line_col(
        source_line_offsets,
        u32::try_from(body_byte_offset).unwrap_or(u32::MAX),
    );
    for function in &mut complexity {
        function.line = body_start_line + function.line.saturating_sub(1);
        if function.line == body_start_line {
            function.col += body_start_col;
        }
    }
    complexity
}

/// Thread the harvested Astro `Props` declaration + `Astro.props` usage onto the
/// module: copy the abstain flags, remap each prop span onto the `.astro` source,
/// and set `used_in_template` from the template-usage scan. A template spread
/// `{...Astro.props}` forwards every prop opaquely, so it sets the whole-file
/// fallthrough abstain (the markup analog of a script-side whole-object use).
fn apply_astro_props(
    info: &mut ModuleInfo,
    harvest: crate::sfc_props::DefinePropsHarvest,
    template_used: &FxHashSet<String>,
    template: &str,
    frontmatter_offset: usize,
) {
    if harvest.has_unharvestable_props {
        info.has_unharvestable_props = true;
    }
    if harvest.has_props_attrs_fallthrough || template.contains("...Astro.props") {
        info.has_props_attrs_fallthrough = true;
    }
    for mut prop in harvest.props {
        prop.span_start = prop
            .span_start
            .saturating_add(u32::try_from(frontmatter_offset).unwrap_or(u32::MAX));
        prop.used_in_template =
            template_used.contains(&prop.local) || template_used.contains(&prop.name);
        info.component_props.push(prop);
    }
}

/// Append imports discovered in the Astro template body: `<script src="...">`
/// references and ESM `import` statements inside inline `<script>` blocks.
fn extend_imports_from_template(
    imports: &mut Vec<ImportInfo>,
    template: &str,
    template_offset: usize,
) {
    if template.is_empty() {
        return;
    }

    let comment_ranges: Vec<(usize, usize)> = HTML_COMMENT_RE
        .find_iter(template)
        .map(|m| (m.start(), m.end()))
        .collect();

    extend_processed_script_src_imports(imports, template, template_offset, &comment_ranges);
    extend_inline_script_imports(imports, template, template_offset, &comment_ranges);
}

/// Whether `pos` falls inside any HTML comment range.
fn pos_in_comment(comment_ranges: &[(usize, usize)], pos: usize) -> bool {
    comment_ranges
        .iter()
        .any(|&(start, end)| pos >= start && pos < end)
}

/// Append `<script src="...">` processed-script references from the template.
fn extend_processed_script_src_imports(
    imports: &mut Vec<ImportInfo>,
    template: &str,
    template_offset: usize,
    comment_ranges: &[(usize, usize)],
) {
    for cap in SCRIPT_OPEN_RE.captures_iter(template) {
        let Some(open) = cap.get(0) else {
            continue;
        };
        if pos_in_comment(comment_ranges, open.start()) {
            continue;
        }
        let attrs = cap.name("attrs").map_or("", |m| m.as_str());
        if let Some((raw, source_span)) = processed_script_src_with_span(attrs, cap.name("attrs")) {
            let tag_span = Span::new(
                (template_offset + open.start()) as u32,
                (template_offset + open.end()) as u32,
            );
            imports.push(ImportInfo {
                source: normalize_asset_url(raw),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: tag_span,
                source_span: Span::new(
                    template_offset as u32 + source_span.start,
                    template_offset as u32 + source_span.end,
                ),
            });
        }
    }
}

/// Append ESM `import` statements found in attribute-free inline `<script>`
/// blocks of the template, remapping spans onto the SFC source.
fn extend_inline_script_imports(
    imports: &mut Vec<ImportInfo>,
    template: &str,
    template_offset: usize,
    comment_ranges: &[(usize, usize)],
) {
    for cap in SCRIPT_BLOCK_RE.captures_iter(template) {
        let Some(open) = cap.get(0) else {
            continue;
        };
        if pos_in_comment(comment_ranges, open.start()) {
            continue;
        }
        let attrs = cap.name("attrs").map_or("", |m| m.as_str());
        if !attrs.trim().is_empty() {
            continue;
        }
        let Some(body_match) = cap.name("body") else {
            continue;
        };
        let body = body_match.as_str();
        if body.trim().is_empty() {
            continue;
        }

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, body, SourceType::ts()).parse();
        let mut inline_extractor = ModuleInfoExtractor::new();
        inline_extractor.visit_program(&parser_return.program);
        let extraction = ExtractionResult::contiguous(body, template_offset + body_match.start());
        inline_extractor.remap_spans_with(|span| extraction.remap_span(span));
        imports.append(&mut inline_extractor.imports);
    }
}

fn processed_script_src_with_span<'a>(
    attrs: &'a str,
    attrs_match: Option<regex::Match<'_>>,
) -> Option<(&'a str, Span)> {
    let cap = SRC_ATTR_RE.captures(attrs)?;
    let src_match = cap.name("src")?;
    let src = src_match.as_str().trim();
    if src.is_empty() || is_remote_url(src) {
        return None;
    }

    let without_src = SRC_ATTR_RE.replace(attrs, "");
    let extra_attrs = without_src.trim();
    let extra_attrs = extra_attrs.strip_suffix('/').unwrap_or(extra_attrs).trim();
    if !extra_attrs.is_empty() {
        return None;
    }

    let attrs_start = attrs_match.map_or(0, |m| m.start());
    Some((
        src,
        Span::new(
            (attrs_start + src_match.start()) as u32,
            (attrs_start + src_match.end()) as u32,
        ),
    ))
}

// Astro tests use regex-based frontmatter extraction.
#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    #[test]
    fn is_astro_file_positive() {
        assert!(is_astro_file(Path::new("Layout.astro")));
    }

    #[test]
    fn is_astro_file_rejects_vue() {
        assert!(!is_astro_file(Path::new("App.vue")));
    }

    #[test]
    fn is_astro_file_rejects_ts() {
        assert!(!is_astro_file(Path::new("utils.ts")));
    }

    #[test]
    fn is_astro_file_rejects_mdx() {
        assert!(!is_astro_file(Path::new("post.mdx")));
    }

    #[test]
    fn extracts_frontmatter_body() {
        let source = "---\nimport Layout from '../layouts/Layout.astro';\nconst title = 'Hi';\n---\n<Layout />";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        let script = script.unwrap();
        assert!(script.body.contains("import Layout"));
        assert!(script.body.contains("const title"));
    }

    #[test]
    fn frontmatter_is_always_typescript() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let script = extract_astro_frontmatter(source).unwrap();
        assert!(script.is_typescript);
    }

    #[test]
    fn frontmatter_is_not_jsx() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let script = extract_astro_frontmatter(source).unwrap();
        assert!(!script.is_jsx);
    }

    #[test]
    fn frontmatter_has_no_src() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let script = extract_astro_frontmatter(source).unwrap();
        assert!(script.src.is_none());
    }

    #[test]
    fn no_frontmatter_returns_none() {
        let source = "<div>No frontmatter here</div>";
        assert!(extract_astro_frontmatter(source).is_none());
    }

    #[test]
    fn no_frontmatter_just_html() {
        let source = "<html><body><h1>Hello</h1></body></html>";
        assert!(extract_astro_frontmatter(source).is_none());
    }

    #[test]
    fn empty_frontmatter() {
        let source = "---\n\n---\n<div />";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        let body = script.unwrap().body;
        assert!(body.trim().is_empty());
    }

    #[test]
    fn only_first_frontmatter_pair() {
        let source = "---\nconst first = true;\n---\n<div />\n---\nconst second = true;\n---\n";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        let body = script.unwrap().body;
        assert!(body.contains("first"));
        assert!(!body.contains("second"));
    }

    #[test]
    fn byte_offset_points_to_body() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let script = extract_astro_frontmatter(source).unwrap();
        let offset = script.byte_offset;
        assert!(source[offset..].starts_with("const x = 1;"));
    }

    #[test]
    fn leading_whitespace_before_frontmatter() {
        let source = "  \n---\nconst x = 1;\n---\n<div />";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        assert!(script.unwrap().body.contains("const x = 1;"));
    }

    #[test]
    fn frontmatter_with_type_annotations() {
        let source = "---\ninterface Props { title: string; }\nconst { title } = Astro.props as Props;\n---\n<h1>{title}</h1>";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        let body = script.unwrap().body;
        assert!(body.contains("interface Props"));
        assert!(body.contains("Astro.props"));
    }

    #[test]
    fn frontmatter_with_multiline_imports() {
        let source = "---\nimport {\n  Component,\n  Fragment\n} from 'react';\n---\n<Component />";
        let script = extract_astro_frontmatter(source).unwrap();
        assert!(script.body.contains("Component"));
        assert!(script.body.contains("Fragment"));
    }

    #[test]
    fn frontmatter_with_crlf_line_endings() {
        let source = "---\r\nexport const x = 1;\r\n---\r\n<div />";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        assert!(script.unwrap().body.contains("export const x = 1;"));
    }

    #[test]
    fn frontmatter_not_at_start_returns_none() {
        let source = "<div />\n---\nconst x = 1;\n---\n";
        assert!(extract_astro_frontmatter(source).is_none());
    }

    #[test]
    fn frontmatter_dashes_in_body_not_confused() {
        let source = "---\nconst x = '---';\nconst y = 2;\n---\n<div />";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        let body = script.unwrap().body;
        assert!(body.contains("const x = '---';"));
    }

    #[test]
    fn parse_astro_to_module_no_frontmatter() {
        let info = parse_astro_to_module(FileId(0), "<div>Hello</div>", 42, false);
        assert!(info.imports.is_empty());
        assert!(info.exports.is_empty());
        assert_eq!(info.content_hash, 42);
        assert_eq!(info.file_id, FileId(0));
    }

    #[test]
    fn parse_astro_to_module_with_imports() {
        let source = "---\nimport { ref } from 'vue';\nconst x = ref(0);\n---\n<div />";
        let info = parse_astro_to_module(FileId(1), source, 99, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "vue");
        assert_eq!(info.file_id, FileId(1));
        assert_eq!(info.content_hash, 99);
    }

    #[test]
    fn parse_astro_to_module_has_line_offsets() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert!(!info.line_offsets.is_empty());
    }

    #[test]
    fn parse_astro_to_module_has_suppressions() {
        let source = "---\n// fallow-ignore-file\nconst x = 1;\n---\n<div />";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert!(!info.suppressions.is_empty());
        assert_eq!(info.suppressions[0].line, 0);
    }

    #[test]
    fn is_astro_file_rejects_svelte() {
        assert!(!is_astro_file(Path::new("Component.svelte")));
    }

    #[test]
    fn is_astro_file_rejects_no_extension() {
        assert!(!is_astro_file(Path::new("Makefile")));
    }

    #[test]
    fn parse_astro_template_script_src_relative() {
        let source = "---\nconst x = 1;\n---\n<script src=\"./client.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./client.ts");
    }

    #[test]
    fn parse_astro_template_script_src_parent_relative() {
        let source = "---\n---\n<script src=\"../scripts/foo.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "../scripts/foo.ts");
    }

    #[test]
    fn parse_astro_template_script_src_bare_normalized() {
        let source = "---\n---\n<script src=\"client.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./client.ts");
    }

    #[test]
    fn parse_astro_template_script_src_skips_remote() {
        let source = "---\n---\n<script src=\"https://cdn.example.com/lib.js\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn parse_astro_template_script_src_multiline_attrs() {
        let source = "---\n---\n<script\n  src=\"./client.ts\"\n></script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./client.ts");
    }

    #[test]
    fn parse_astro_template_script_src_with_extra_attrs_is_unprocessed() {
        let source = "---\n---\n<script type=\"module\" src=\"./client.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn parse_astro_template_inline_script_import() {
        let source = "---\n---\n<script>\n  import '../scripts/bar';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "../scripts/bar");
        assert!(matches!(
            info.imports[0].imported_name,
            crate::ImportedName::SideEffect
        ));
    }

    #[test]
    fn parse_astro_template_inline_script_named_import() {
        let source = "---\n---\n<script>\n  import { foo } from '../utils';\n  foo();\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "../utils");
    }

    #[test]
    fn parse_astro_template_inline_script_typescript_syntax() {
        let source = "---\n---\n<script>\n  import { foo } from '../utils';\n  const x: number = foo();\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "../utils");
    }

    #[test]
    fn parse_astro_template_inline_script_with_attributes_is_unprocessed() {
        let source = "---\n---\n<script is:inline>\n  import '../scripts/bar';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn parse_astro_template_type_module_inline_script_is_unprocessed() {
        let source = "---\n---\n<script type=\"module\">\n  import '../scripts/bar';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn parse_astro_template_skips_inline_body_when_src_present() {
        let source = "---\n---\n<script src=\"./client.ts\">import 'should-be-ignored';</script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./client.ts");
    }

    #[test]
    fn parse_astro_template_combined_src_and_inline() {
        let source = "---\nconst title = \"Hi\";\n---\n\
                      <html><body>\n\
                      <h1>{title}</h1>\n\
                      <script src=\"../scripts/foo.ts\"></script>\n\
                      <script>\n  import '../scripts/bar';\n</script>\n\
                      </body></html>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"../scripts/foo.ts"));
        assert!(sources.contains(&"../scripts/bar"));
    }

    #[test]
    fn parse_astro_template_multiple_inline_scripts() {
        let source = "---\n---\n\
                      <script>\n  import '../a';\n</script>\n\
                      <script>\n  import '../b';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"../a"));
        assert!(sources.contains(&"../b"));
    }

    #[test]
    fn parse_astro_template_skips_commented_out_script_src() {
        let source = "---\n---\n<!-- <script src=\"./old.ts\"></script> -->\n<script src=\"./new.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./new.ts");
    }

    #[test]
    fn parse_astro_template_skips_commented_out_inline_script() {
        let source = "---\n---\n<!-- <script>\n  import '../old';\n</script> -->\n<script>\n  import '../new';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"../new"));
        assert!(!sources.contains(&"../old"));
    }

    #[test]
    fn parse_astro_template_no_frontmatter_with_script() {
        let source = "<html><body><script src=\"./client.ts\"></script></body></html>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./client.ts");
    }

    #[test]
    fn parse_astro_template_empty_inline_script_is_skipped() {
        let source = "---\n---\n<script></script>";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn parse_astro_template_does_not_double_count_frontmatter_imports() {
        let source = "---\nimport Layout from '../Layout.astro';\n---\n<Layout />";
        let info = parse_astro_to_module(FileId(0), source, 0, false);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "../Layout.astro");
    }

    #[test]
    fn astro_template_map_callback_element_type() {
        // Issue #1713: a `.map()` callback param in the TEMPLATE body (not the
        // frontmatter) whose receiver is a frontmatter `Util[]` binding is typed
        // to the `Util` element class, so `util.getter()` credits `Util.getter`.
        let source = "---\n\
            import { Util } from './Util'\n\
            const utils: Util[] = [new Util()]\n\
            ---\n\
            <div>\n\
              {utils.map((util) => <p>{util.getter()}</p>)}\n\
            </div>\n";
        let info = parse_astro_to_module(FileId(0), source, 0, false);

        let has_util_getter = info
            .member_accesses
            .iter()
            .any(|access| access.object == "Util" && access.member == "getter");
        assert!(
            has_util_getter,
            "expected Util.getter from the template .map callback, found: {:?}",
            info.member_accesses
        );
    }

    #[test]
    fn astro_template_map_does_not_credit_unrelated_member() {
        // Control: a member NOT accessed anywhere (frontmatter or template) must
        // not be synthesized as a Util access. Proves the pass is over-credit-only
        // and does not blanket-credit the class.
        let source = "---\n\
            import { Util } from './Util'\n\
            const utils: Util[] = [new Util()]\n\
            ---\n\
            <div>\n\
              {utils.map((util) => <p>{util.getter()}</p>)}\n\
            </div>\n";
        let info = parse_astro_to_module(FileId(0), source, 0, false);

        let has_unused = info
            .member_accesses
            .iter()
            .any(|access| access.object == "Util" && access.member == "unusedMethod");
        assert!(
            !has_unused,
            "Util.unusedMethod is never accessed and must not be credited, found: {:?}",
            info.member_accesses
        );
    }

    #[test]
    fn astro_template_map_without_typed_array_credits_nothing() {
        // Neuter check: no frontmatter typed array => no element type => the
        // template pass contributes no class-qualified access.
        let source = "---\n\
            import { Util } from './Util'\n\
            const utils = getUtils()\n\
            ---\n\
            <div>\n\
              {utils.map((util) => <p>{util.getter()}</p>)}\n\
            </div>\n";
        let info = parse_astro_to_module(FileId(0), source, 0, false);

        assert!(
            !info
                .member_accesses
                .iter()
                .any(|access| access.object == "Util"),
            "an untyped receiver must not credit any Util member, found: {:?}",
            info.member_accesses
        );
    }

    /// Template that is roughly half HTML comments (masked) and half `{expr}`
    /// braces / tags (credited), with an overlapping masked range (a comment that
    /// wraps a `<script>`). The merged binary-search masking must decide every
    /// byte position identically to the old linear `.any()` over the raw,
    /// unsorted, overlapping ranges. Regression for the masking-cost change
    /// (issue #1843 follow-up): behavior must stay byte-identical.
    const MASKED_MIX_TEMPLATE: &str = concat!(
        "<!-- <Widget/> and {Sidebar} -->\n",
        "<Header title={fmt(x)} />\n",
        "<!-- <script>{Buried}</script> -->\n",
        "<script><Hidden/>{Ghost}</script>\n",
        "<style>a { color: {red}; }</style>\n",
        "<Footer>{items.map((i) => i.label)}</Footer>\n",
    );

    #[test]
    fn astro_masked_ranges_match_linear_reference() {
        let template = MASKED_MIX_TEMPLATE;

        // Old masking: a linear scan over the three raw (unsorted, overlapping)
        // `find_iter` passes.
        let mut raw: Vec<(usize, usize)> = Vec::new();
        raw.extend(
            SCRIPT_BLOCK_RE
                .find_iter(template)
                .map(|m| (m.start(), m.end())),
        );
        raw.extend(
            STYLE_BLOCK_RE
                .find_iter(template)
                .map(|m| (m.start(), m.end())),
        );
        raw.extend(
            HTML_COMMENT_RE
                .find_iter(template)
                .map(|m| (m.start(), m.end())),
        );
        let linear = |pos: usize| raw.iter().any(|&(s, e)| pos >= s && pos < e);

        let merged = build_masked_ranges(template);
        // The overlapping comment+script pair must collapse during the merge.
        assert!(
            merged.len() < raw.len(),
            "expected overlapping ranges to merge, raw={raw:?} merged={merged:?}"
        );
        for pos in 0..template.len() {
            assert_eq!(
                pos_in_masked_ranges(&merged, pos),
                linear(pos),
                "masking decision differs at byte {pos}"
            );
        }
    }

    #[test]
    fn astro_template_used_names_masks_comments_credits_braces() {
        let used = collect_astro_template_used_names(MASKED_MIX_TEMPLATE);

        // Credited: tag roots + brace-expression identifiers outside masked regions.
        for name in ["Header", "Footer", "fmt", "x", "items", "map", "i", "label"] {
            assert!(
                used.contains(name),
                "expected {name} to be credited, got {used:?}"
            );
        }

        // Masked: identifiers only reachable inside comments / `<script>` / `<style>`
        // (each would be a tag root or brace ident if the range were not masked).
        for name in ["Widget", "Sidebar", "Buried", "Hidden", "Ghost", "red"] {
            assert!(
                !used.contains(name),
                "expected {name} to stay masked, got {used:?}"
            );
        }
    }
}
