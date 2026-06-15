//! Vue/Svelte Single File Component (SFC) script and style extraction.
//!
//! Extracts `<script>` block content from `.vue` and `.svelte` files using regex,
//! handling `lang`, `src` metadata, and `generic` attributes, and filtering
//! HTML comments. Vue external script references are emitted as graph edges;
//! Svelte markup-level script `src` references are treated as runtime HTML.
//! Also extracts `<style>` block sources (`@import` / `@use` / `@forward` /
//! `@plugin` and `<style src="...">`) so referenced CSS / SCSS files become
//! reachable from the component, preventing false `unused-files` reports on
//! co-located styles.

use std::path::Path;
use std::sync::LazyLock;

use oxc_allocator::Allocator;
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::asset_url::normalize_asset_url;
use crate::parse::{
    compute_auto_import_candidates, compute_import_binding_usage, compute_semantic_usage,
};
use crate::sfc_template::{SfcKind, collect_template_usage_with_bound_targets};
use crate::source_map::ExtractionResult;
use crate::visitor::ModuleInfoExtractor;
use crate::{ImportInfo, ImportedName, ModuleInfo};
use fallow_types::discover::FileId;
use fallow_types::extract::{FunctionComplexity, byte_offset_to_line_col, compute_line_offsets};
use oxc_span::Span;

/// Regex to extract `<script>` block content from Vue/Svelte SFCs.
/// The attrs pattern handles `>` inside quoted attribute values (e.g., `generic="T extends Foo<Bar>"`).
static SCRIPT_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?is)<script\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>(?P<body>[\s\S]*?)</script>"#,
    )
});

/// Regex to extract the `lang` attribute value from a script tag.
static LANG_ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"lang\s*=\s*["'](\w+)["']"#));

/// Regex to extract the `src` attribute value from a script tag.
/// Requires whitespace (or start of string) before `src` to avoid matching `data-src` etc.
static SRC_ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"(?:^|\s)src\s*=\s*["']([^"']+)["']"#));

/// Regex to detect Vue's bare `setup` attribute.
static SETUP_ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?:^|\s)setup(?:\s|$)"));

/// Regex to detect Svelte's `context="module"` attribute.
static CONTEXT_MODULE_ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"context\s*=\s*["']module["']"#));

/// Regex to extract Vue's `generic="..."` attribute value (script-setup
/// generics). Matches the contents between the quotes and stops at the
/// closing quote, mirroring `LANG_ATTR_RE`.
static VUE_GENERIC_ATTR_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?:^|\s)generic\s*=\s*"([^"]*)"|(?:^|\s)generic\s*=\s*'([^']*)'"#)
});

/// Regex to extract Svelte's `generics="..."` attribute value (Svelte 4
/// generic script attribute, repurposed by some Svelte 5 code).
static SVELTE_GENERICS_ATTR_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?:^|\s)generics\s*=\s*"([^"]*)"|(?:^|\s)generics\s*=\s*'([^']*)'"#)
});

/// Regex to match HTML comments for filtering script blocks inside comments.
static HTML_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)<!--.*?-->"));

/// Regex to detect a whole-object prop/attr spread in a Vue template:
/// `v-bind="$attrs"`, `v-bind="$props"`, or `v-bind="props"` (with single or
/// double quotes). A bound prop may be consumed indirectly, so the
/// `unused-component-prop` detector abstains on the whole file when this matches.
static PROPS_ATTRS_SPREAD_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"v-bind\s*=\s*["'](?:\$attrs|\$props|props)["']"#));

/// FP-1 (unused-load-data-key): a SvelteKit route component passing the whole
/// `data` prop opaquely in MARKUP, where a child reads arbitrary keys the
/// detector cannot see. Matches `data={data}` (whole-prop pass to a child) and
/// `{...data}` (Svelte template spread). The script-side `const x = {...data}` /
/// `fn(data)` / `const X = data` forms are captured by the JS visitor instead.
/// Only a whole-`data` pass forces the abstain; `data.x` member access stays a
/// credited consumer.
static SVELTE_TEMPLATE_DATA_WHOLE_USE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?:=\s*\{\s*data\s*\}|\{\s*\.\.\.\s*data\s*\})"));

/// Matches an emit-style call in template markup: a callee identifier (or
/// `$emit`) followed by `(` and its first argument. Group 1 is the callee name
/// (filtered against the harvested emit binding / `$emit` by the caller), groups
/// 2 and 3 are a string-literal first arg (single- or double-quoted: the event
/// name, credited as used), and group 4 is the first non-space character of a
/// NON-literal first arg (a dynamic emit, whose event name is unknowable, forcing
/// a whole-file abstain). Event names allow kebab and namespaced forms
/// (`update:modelValue`, `my-event`). The Rust `regex` crate has no
/// backreferences, so the two quote styles are separate alternatives.
static TEMPLATE_EMIT_CALL_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"([\w$]+)\s*\(\s*(?:'([\w:-]*)'|"([\w:-]*)"|(\S))"#));

/// Regex to extract `<style>` block content from Vue/Svelte SFCs.
/// Mirrors `SCRIPT_BLOCK_RE`: handles `>` inside quoted attribute values and
/// captures the body so `@import` / `@use` / `@forward` directives can be parsed.
static STYLE_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?is)<style\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>(?P<body>[\s\S]*?)</style>"#,
    )
});

/// Static asset references in SFC markup: `<img src="./logo.png">`,
/// `<source src="...">`, `<video poster="...">`, etc.
///
/// Scoped to genuine asset elements (`img` / `source` / `video` / `audio` /
/// `track` / `embed`) so a custom component's `src` PROP (`<MyImage src="./x">`)
/// is never misread as an asset edge. ONLY plain relative literals (`./` or
/// `../`) are captured: dynamic bindings (`:src`, `v-bind:src`, `bind:src`,
/// `src={...}`, `data-src`), alias-prefixed (`@/`), root-relative (`/foo`),
/// remote, interpolated (`{{ }}` / `{ }`), and query/hash-suffixed values are
/// all skipped (the value class excludes `{`, `?`, `#`, whitespace, and angle
/// brackets, and the alternation anchors on a leading `./` or `../`). A
/// captured ref becomes a `SideEffect` import; an existing asset resolves to
/// `ExternalFile` (no finding) and a genuinely-missing one surfaces as
/// `unresolved-import` on the trusted resolver path.
static TEMPLATE_ASSET_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?si)<(?:img|source|video|audio|track|embed)\b(?:[^>"']|"[^"]*"|'[^']*')*?\s(?:src|poster)\s*=\s*(?:"((?:\./|\.\./)[^"<>{}?#\s]*)"|'((?:\./|\.\./)[^'<>{}?#\s]*)')"#,
    )
});

/// Mask `<script>` / `<style>` blocks and HTML comments to equal-length spaces
/// so a markup-region scan (asset refs) sees only the template, while byte
/// offsets still map 1:1 into the original source for line/col reporting.
fn mask_non_markup_regions(source: &str) -> String {
    let mut masked = source.to_string();
    for re in [&*SCRIPT_BLOCK_RE, &*STYLE_BLOCK_RE, &*HTML_COMMENT_RE] {
        masked = re
            .replace_all(&masked, |caps: &regex::Captures<'_>| {
                " ".repeat(caps[0].len())
            })
            .into_owned();
    }
    masked
}

/// Collect static relative asset references from SFC markup as
/// `(normalized_specifier, value_span)` pairs. See [`TEMPLATE_ASSET_RE`].
fn collect_template_asset_refs(source: &str) -> Vec<(String, Span)> {
    let masked = mask_non_markup_regions(source);
    let mut refs = Vec::new();
    for caps in TEMPLATE_ASSET_RE.captures_iter(&masked) {
        let Some(value) = caps.get(1).or_else(|| caps.get(2)) else {
            continue;
        };
        let raw = value.as_str();
        if raw.is_empty() {
            continue;
        }
        refs.push((
            normalize_asset_url(raw),
            Span::new(value.start() as u32, value.end() as u32),
        ));
    }
    refs
}

/// An extracted `<script>` block from a Vue or Svelte SFC.
pub struct SfcScript {
    /// The script body text.
    pub body: String,
    /// Whether the script uses TypeScript (`lang="ts"` or `lang="tsx"`).
    pub is_typescript: bool,
    /// Whether the script uses JSX syntax (`lang="tsx"` or `lang="jsx"`).
    pub is_jsx: bool,
    /// Byte offset of the script body within the full SFC source.
    pub byte_offset: usize,
    /// External script source path from `src` attribute.
    pub src: Option<String>,
    /// Span of the `src` attribute value in the full SFC source.
    pub src_span: Option<Span>,
    /// Whether this script is a Vue `<script setup>` block.
    pub is_setup: bool,
    /// Whether this script is a Svelte module-context block.
    pub is_context_module: bool,
    /// Type-parameter list from a `generic="..."` (Vue) or `generics="..."`
    /// (Svelte) attribute on the script tag. Holds the bare constraint, no
    /// surrounding angle brackets, e.g. `T extends Test<boolean>`.
    pub generic_attr: Option<String>,
}

/// Extract all `<script>` blocks from a Vue/Svelte SFC source string.
pub fn extract_sfc_scripts(source: &str) -> Vec<SfcScript> {
    let comment_ranges: Vec<(usize, usize)> = HTML_COMMENT_RE
        .find_iter(source)
        .map(|m| (m.start(), m.end()))
        .collect();

    SCRIPT_BLOCK_RE
        .captures_iter(source)
        .filter(|cap| {
            let start = cap.get(0).map_or(0, |m| m.start());
            !comment_ranges
                .iter()
                .any(|&(cs, ce)| start >= cs && start < ce)
        })
        .map(|cap| {
            let attrs = cap.name("attrs").map_or("", |m| m.as_str());
            let body_match = cap.name("body");
            let byte_offset = body_match.map_or(0, |m| m.start());
            let body = body_match.map_or("", |m| m.as_str()).to_string();
            let lang = LANG_ATTR_RE
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str());
            let is_typescript = matches!(lang, Some("ts" | "tsx"));
            let is_jsx = matches!(lang, Some("tsx" | "jsx"));
            let src = SRC_ATTR_RE
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());
            let attrs_start = cap.name("attrs").map_or(0, |m| m.start());
            let src_span = SRC_ATTR_RE.captures(attrs).and_then(|c| c.get(1)).map(|m| {
                Span::new(
                    (attrs_start + m.start()) as u32,
                    (attrs_start + m.end()) as u32,
                )
            });
            let is_setup = SETUP_ATTR_RE.is_match(attrs);
            let is_context_module = CONTEXT_MODULE_ATTR_RE.is_match(attrs);
            let generic_attr = VUE_GENERIC_ATTR_RE
                .captures(attrs)
                .or_else(|| SVELTE_GENERICS_ATTR_RE.captures(attrs))
                .and_then(|cap| cap.get(1).or_else(|| cap.get(2)))
                .map(|m| m.as_str().to_string())
                .filter(|value| !value.trim().is_empty());
            SfcScript {
                body,
                is_typescript,
                is_jsx,
                byte_offset,
                src,
                src_span,
                is_setup,
                is_context_module,
                generic_attr,
            }
        })
        .collect()
}

/// An extracted `<style>` block from a Vue or Svelte SFC.
pub struct SfcStyle {
    /// The style body text (CSS / SCSS / Sass / Less / Stylus / PostCSS source).
    pub body: String,
    /// The `lang` attribute value (`scss`, `sass`, `less`, `stylus`, `postcss`, ...).
    /// `None` for plain `<style>` (CSS).
    pub lang: Option<String>,
    /// External style source path from the `src` attribute (`<style src="./theme.scss">`).
    pub src: Option<String>,
    /// Span of the `src` attribute value in the full SFC source.
    pub src_span: Option<Span>,
    /// Byte offset of the style body within the full SFC source.
    pub byte_offset: usize,
}

/// Extract all `<style>` blocks from a Vue/Svelte SFC source string.
///
/// Mirrors [`extract_sfc_scripts`]: filters blocks inside HTML comments and
/// captures the `lang` and `src` attributes so the caller can route the body to
/// the right preprocessor's import scanner (currently only CSS / SCSS / Sass) or
/// seed the `src` reference as a side-effect import.
pub fn extract_sfc_styles(source: &str) -> Vec<SfcStyle> {
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
        .map(|cap| {
            let attrs = cap.name("attrs").map_or("", |m| m.as_str());
            let body = cap.name("body").map_or("", |m| m.as_str()).to_string();
            let byte_offset = cap.name("body").map_or(0, |m| m.start());
            let lang = LANG_ATTR_RE
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());
            let src = SRC_ATTR_RE
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());
            let attrs_start = cap.name("attrs").map_or(0, |m| m.start());
            let src_span = SRC_ATTR_RE.captures(attrs).and_then(|c| c.get(1)).map(|m| {
                Span::new(
                    (attrs_start + m.start()) as u32,
                    (attrs_start + m.end()) as u32,
                )
            });
            SfcStyle {
                body,
                lang,
                src,
                src_span,
                byte_offset,
            }
        })
        .collect()
}

/// Check if a file path is a Vue or Svelte SFC (`.vue` or `.svelte`).
#[must_use]
pub fn is_sfc_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "vue" || ext == "svelte")
}

/// Parse an SFC file by extracting and combining all `<script>` and `<style>` blocks.
pub(crate) fn parse_sfc_to_module(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    let scripts = extract_sfc_scripts(source);
    let styles = extract_sfc_styles(source);
    let kind = sfc_kind(path);
    let mut combined = empty_sfc_module(file_id, source, content_hash);
    let mut template_visible_imports: FxHashSet<String> = FxHashSet::default();
    let mut template_visible_bound_targets: FxHashMap<String, String> = FxHashMap::default();
    let mut props_return_binding: Option<String> = None;
    let mut emit_return_binding: Option<String> = None;

    for script in &scripts {
        merge_script_into_module(&mut SfcScriptMergeInput {
            kind,
            script,
            combined: &mut combined,
            template_visible_imports: &mut template_visible_imports,
            template_visible_bound_targets: &mut template_visible_bound_targets,
            props_return_binding: &mut props_return_binding,
            emit_return_binding: &mut emit_return_binding,
            need_complexity,
        });
    }

    for style in &styles {
        merge_style_into_module(style, &mut combined);
    }

    // Whole-object prop/attr spread in the template (`v-bind="$attrs"`,
    // `v-bind="$props"`, `v-bind="props"`) can consume a prop indirectly, so the
    // `unused-component-prop` detector must abstain on the whole file.
    if kind == SfcKind::Vue
        && !combined.component_props.is_empty()
        && PROPS_ATTRS_SPREAD_RE.is_match(source)
    {
        combined.has_props_attrs_fallthrough = true;
    }

    apply_template_usage(
        kind,
        source,
        &template_visible_imports,
        &template_visible_bound_targets,
        props_return_binding.as_deref(),
        kind == SfcKind::Svelte && is_sveltekit_route_data_component(path),
        &mut combined,
    );

    // Credit `<emit_binding>('event')` / `$emit('event')` calls in the template
    // (`@click="emit('close')"`), which the script-only emit usage walk cannot
    // see. A dynamic template emit (`$emit(someVar)`) abstains the whole file.
    if kind == SfcKind::Vue && !combined.component_emits.is_empty() {
        apply_template_emit_usage(source, emit_return_binding.as_deref(), &mut combined);
    }

    // Static relative asset references in markup (`<img src="./logo.png">`)
    // become SideEffect imports so a genuinely-missing asset surfaces as
    // `unresolved-import` (existing assets resolve to `ExternalFile`, no finding).
    for (specifier, span) in collect_template_asset_refs(source) {
        combined.imports.push(ImportInfo {
            source: specifier,
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: false,
            span,
            source_span: span,
        });
    }

    combined.unused_import_bindings.sort_unstable();
    combined.unused_import_bindings.dedup();
    combined.type_referenced_import_bindings.sort_unstable();
    combined.type_referenced_import_bindings.dedup();
    combined.value_referenced_import_bindings.sort_unstable();
    combined.value_referenced_import_bindings.dedup();
    combined.auto_import_candidates.sort_unstable();
    combined.auto_import_candidates.dedup();

    combined
}

fn sfc_kind(path: &Path) -> SfcKind {
    if path.extension().and_then(|ext| ext.to_str()) == Some("vue") {
        SfcKind::Vue
    } else {
        SfcKind::Svelte
    }
}

/// SvelteKit route components receive a `data` prop populated by the route's
/// `load()` return object. This predicate gates the `data`-as-template-root
/// credit (unused-load-data-key Primitive B) to exactly those files. It matches
/// `+page.svelte` / `+layout.svelte` AND their layout-reset variants
/// (`+page@.svelte`, `+page@named.svelte`, `+page@(group).svelte`, and the
/// `+layout@...` forms), all of which still receive the `data` prop. `+error.svelte`
/// is excluded (it receives `$page.error`, not the `load()` `data` prop), and a
/// non-route file like `+pageHelper.svelte` is excluded by the grammar (the part
/// after `+page` must be empty or start with `@`). The leading `+` is a
/// SvelteKit-only filename convention, so no ordinary `.svelte` component matches.
fn is_sveltekit_route_data_component(path: &Path) -> bool {
    let Some(stem) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".svelte"))
    else {
        return false;
    };
    ["+page", "+layout"].iter().any(|prefix| {
        stem.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('@'))
    })
}

fn empty_sfc_module(file_id: FileId, source: &str, content_hash: u64) -> ModuleInfo {
    let parsed = crate::suppress::parse_suppressions_from_source(source);

    ModuleInfo {
        file_id,
        exports: Vec::new(),
        imports: Vec::new(),
        re_exports: Vec::new(),
        dynamic_imports: Vec::new(),
        dynamic_import_patterns: Vec::new(),
        require_calls: Vec::new(),
        package_path_references: Vec::new(),
        member_accesses: Vec::new(),
        whole_object_uses: Vec::new(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        content_hash,
        suppressions: parsed.suppressions,
        unknown_suppression_kinds: parsed.unknown_kinds,
        unused_import_bindings: Vec::new(),
        type_referenced_import_bindings: Vec::new(),
        value_referenced_import_bindings: Vec::new(),
        line_offsets: compute_line_offsets(source),
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
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
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: Vec::new(),
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    }
}

struct SfcScriptMergeInput<'a> {
    kind: SfcKind,
    script: &'a SfcScript,
    combined: &'a mut ModuleInfo,
    template_visible_imports: &'a mut FxHashSet<String>,
    template_visible_bound_targets: &'a mut FxHashMap<String, String>,
    props_return_binding: &'a mut Option<String>,
    emit_return_binding: &'a mut Option<String>,
    need_complexity: bool,
}

fn merge_script_into_module(input: &mut SfcScriptMergeInput<'_>) {
    if input.kind == SfcKind::Vue
        && let Some(src) = &input.script.src
    {
        add_script_src_import(input.combined, src, input.script.src_span);
    }

    let allocator = Allocator::default();
    let parser_return = Parser::new(
        &allocator,
        &input.script.body,
        source_type_for_script(input.script),
    )
    .parse();
    let mut extractor = ModuleInfoExtractor::new();
    extractor.visit_program(&parser_return.program);
    let extraction = ExtractionResult::contiguous(&input.script.body, input.script.byte_offset);
    extractor.remap_spans_with(|span| extraction.remap_span(span));
    extractor.resolve_typed_destructure_bindings();

    let augmented_body = build_generic_attr_probe_source(input.script);
    let empty_template_used = rustc_hash::FxHashSet::default();
    let (binding_usage, auto_import_candidates) = if let Some(augmented) = augmented_body.as_deref()
    {
        let augmented_return =
            Parser::new(&allocator, augmented, source_type_for_script(input.script)).parse();
        (
            compute_import_binding_usage(
                &augmented_return.program,
                &extractor.imports,
                &empty_template_used,
            ),
            compute_auto_import_candidates(&parser_return.program),
        )
    } else {
        let semantic_usage = compute_semantic_usage(
            &parser_return.program,
            &extractor.imports,
            &empty_template_used,
        );
        (
            semantic_usage.import_binding_usage,
            semantic_usage.auto_import_candidates,
        )
    };
    input
        .combined
        .unused_import_bindings
        .extend(binding_usage.unused.iter().cloned());
    input
        .combined
        .type_referenced_import_bindings
        .extend(binding_usage.type_referenced.iter().cloned());
    input
        .combined
        .value_referenced_import_bindings
        .extend(binding_usage.value_referenced.iter().cloned());
    input
        .combined
        .auto_import_candidates
        .extend(auto_import_candidates);
    if input.need_complexity {
        input
            .combined
            .complexity
            .extend(translate_script_complexity(
                input.script,
                &parser_return.program,
                &input.combined.line_offsets,
            ));
    }

    // Vue `<script setup>` `defineProps` harvesting for `unused-component-prop`.
    // Spans returned by the harvest are relative to the script body; remap onto
    // the SFC source via the script byte offset.
    if input.kind == SfcKind::Vue && input.script.is_setup {
        let harvest = crate::sfc_props::harvest_define_props(&parser_return.program);
        if harvest.has_unharvestable_props {
            input.combined.has_unharvestable_props = true;
        }
        if harvest.has_props_attrs_fallthrough {
            input.combined.has_props_attrs_fallthrough = true;
        }
        if harvest.has_define_expose {
            input.combined.has_define_expose = true;
        }
        if harvest.has_define_model {
            input.combined.has_define_model = true;
        }
        if let Some(binding) = harvest.props_return_binding {
            *input.props_return_binding = Some(binding);
        }
        for mut prop in harvest.props {
            prop.span_start += input.script.byte_offset as u32;
            input.combined.component_props.push(prop);
        }

        // `defineEmits` harvesting for `unused-component-emit`. Same span remap.
        // `defineModel` creates implicit `update:x` emits, so a file with
        // `defineModel` must abstain emits too (reuse the props-side flag).
        let emit_harvest = crate::sfc_props::harvest_define_emits(&parser_return.program);
        if emit_harvest.has_unharvestable_emits {
            input.combined.has_unharvestable_emits = true;
        }
        if emit_harvest.has_dynamic_emit {
            input.combined.has_dynamic_emit = true;
        }
        if emit_harvest.has_emit_whole_object_use {
            input.combined.has_emit_whole_object_use = true;
        }
        if let Some(binding) = emit_harvest.emit_binding {
            *input.emit_return_binding = Some(binding);
        }
        for mut emit in emit_harvest.emits {
            emit.span_start += input.script.byte_offset as u32;
            input.combined.component_emits.push(emit);
        }
    }

    if is_template_visible_script(input.kind, input.script) {
        input.template_visible_imports.extend(
            extractor
                .imports
                .iter()
                .filter(|import| !import.local_name.is_empty())
                .map(|import| import.local_name.clone()),
        );
        input.template_visible_bound_targets.extend(
            extractor
                .binding_target_names()
                .iter()
                .filter(|(local, _)| !local.starts_with("this."))
                .map(|(local, target)| (local.clone(), target.clone())),
        );
    }

    extractor.merge_into(input.combined);
}

fn translate_script_complexity(
    script: &SfcScript,
    program: &oxc_ast::ast::Program<'_>,
    sfc_line_offsets: &[u32],
) -> Vec<FunctionComplexity> {
    let script_line_offsets = compute_line_offsets(&script.body);
    let mut complexity =
        crate::complexity::compute_complexity(program, &script.body, &script_line_offsets);
    let (body_start_line, body_start_col) =
        byte_offset_to_line_col(sfc_line_offsets, script.byte_offset as u32);

    for function in &mut complexity {
        function.line = body_start_line + function.line.saturating_sub(1);
        if function.line == body_start_line {
            function.col += body_start_col;
        }
    }

    complexity
}

fn add_script_src_import(module: &mut ModuleInfo, source: &str, source_span: Option<Span>) {
    let span = source_span.unwrap_or_default();
    module.imports.push(ImportInfo {
        source: normalize_asset_url(source),
        imported_name: ImportedName::SideEffect,
        local_name: String::new(),
        is_type_only: false,
        from_style: false,
        span,
        source_span: span,
    });
}

/// `lang` attribute values whose body we know how to scan for `@import` /
/// `@use` / `@forward` / `@plugin` directives. Plain `<style>` (no `lang`) is treated as
/// CSS. `less`, `stylus`, and `postcss` bodies are NOT scanned because their
/// import syntax differs (`@import (reference)` modifiers, etc.); their
/// `<style src="...">` references are still seeded.
fn style_lang_is_scss(lang: Option<&str>) -> bool {
    matches!(lang, Some("scss" | "sass"))
}

fn style_lang_is_css_like(lang: Option<&str>) -> bool {
    lang.is_none() || matches!(lang, Some("css"))
}

fn merge_style_into_module(style: &SfcStyle, combined: &mut ModuleInfo) {
    if let Some(src) = &style.src {
        let span = style.src_span.unwrap_or_default();
        combined.imports.push(ImportInfo {
            source: normalize_asset_url(src),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: true,
            span,
            source_span: span,
        });
    }

    let lang = style.lang.as_deref();
    let is_scss = style_lang_is_scss(lang);
    let is_css_like = style_lang_is_css_like(lang);
    if !is_scss && !is_css_like {
        return;
    }

    for source in crate::css::extract_css_import_sources(&style.body, is_scss) {
        let source_span = Span::new(
            style.byte_offset as u32 + source.span.start,
            style.byte_offset as u32 + source.span.end,
        );
        combined.imports.push(ImportInfo {
            source: source.normalized,
            imported_name: if source.is_plugin {
                ImportedName::Default
            } else {
                ImportedName::SideEffect
            },
            local_name: String::new(),
            is_type_only: false,
            from_style: true,
            span: source_span,
            source_span,
        });
    }
}

fn source_type_for_script(script: &SfcScript) -> SourceType {
    match (script.is_typescript, script.is_jsx) {
        (true, true) => SourceType::tsx(),
        (true, false) => SourceType::ts(),
        (false, true) => SourceType::jsx(),
        (false, false) => SourceType::mjs(),
    }
}

/// Build an augmented script body that pins the `generic="..."` constraint as
/// a synthetic local type alias. The alias is unexported and uses a sentinel
/// name so it can't collide with user code. Returns `None` when there is no
/// generic attribute to pin (the common case), so callers fall back to the
/// raw body without paying for a second parse.
fn build_generic_attr_probe_source(script: &SfcScript) -> Option<String> {
    let constraint = script.generic_attr.as_deref()?.trim();
    if constraint.is_empty() {
        return None;
    }
    Some(format!(
        "{}\n;type __FALLOW_GENERIC_ATTR_PROBE<{}> = unknown;\n",
        script.body, constraint,
    ))
}

fn apply_template_usage(
    kind: SfcKind,
    source: &str,
    template_visible_imports: &FxHashSet<String>,
    template_visible_bound_targets: &FxHashMap<String, String>,
    props_return_binding: Option<&str>,
    credit_load_data: bool,
    combined: &mut ModuleInfo,
) {
    // Props are NOT imports, so the template scanner does not credit them by
    // default. Thread the harvested prop names (and the `defineProps` return
    // binding, so `props.<name>` template member accesses are emitted) in as an
    // additional credited set alongside `template_visible_imports`. Crediting a
    // prop name against an import is inert (no import binding shares the name),
    // so the unused-import retain is unaffected.
    let mut credited: FxHashSet<String> = template_visible_imports.clone();
    // unused-load-data-key Primitive B: a SvelteKit route component
    // (`+page.svelte` / `+layout.svelte`) receives a `data` prop populated by
    // the route's `load()` return object. The prop is template-visible
    // (`{data.x}`, `{#each data.items as i}`) but is neither an import nor a
    // tracked binding, so the member-access scanner gates it out by default.
    // Credit `data` as a recognized root so its template member accesses
    // (`data.<key>`) are emitted for the cross-file load-data-key join. Gated to
    // route components only (`credit_load_data`): a non-route component's
    // `let { data } = $props()` is a different, parent-passed `data`, so
    // crediting it as load data would be semantically wrong. Inert for every
    // other detector unless a tracked export/instance is named `data` (mirrors
    // Primitive A); the load-data-key join is the only consumer of `data.<key>`.
    if credit_load_data {
        credited.insert("data".to_string());
        // FP-1: a route component spreading / passing the whole `data` prop in
        // markup consumes arbitrary keys opaquely; force the detector to abstain
        // on this route. A false abstain only suppresses findings, never creates
        // one, so matching the full source (script + template) is safe.
        if SVELTE_TEMPLATE_DATA_WHOLE_USE_RE.is_match(source) {
            combined.has_load_data_whole_use = true;
        }
    }
    if !combined.component_props.is_empty() {
        for prop in &combined.component_props {
            // Credit both the declared name (Vue exposes props by name in the
            // template) and the destructure local (a renamed prop is used via it).
            credited.insert(prop.name.clone());
            credited.insert(prop.local.clone());
        }
        // Vue's implicit `$props` whole-props object is always available in a
        // template; credit `$props.<name>` member accesses too.
        credited.insert("$props".to_string());
        if let Some(binding) = props_return_binding {
            credited.insert(binding.to_string());
        }
    }

    // unused-load-data-key Primitive B: a route `data` prop is usually typed as
    // SvelteKit's generated `PageData` / `LayoutData` (`export let data:
    // PageData`). That typed binding (`data -> PageData`) would otherwise make the
    // template scanner remap `{data.x}` / `<Child foo={data.x} />` member accesses
    // onto the generated type (`PageData.x`), dropping the `data`-keyed access the
    // cross-file load-data join needs. Drop `data` from the bound-targets for the
    // template scan so its template member accesses stay keyed on `data`. The
    // generated `$types` aliases carry no real members for any member detector, so
    // losing the type-keyed template credit is inert; script-side `data` reads are
    // unaffected (their resolution already ran during `merge_into`).
    let template_usage = if credit_load_data && template_visible_bound_targets.contains_key("data")
    {
        let mut filtered = template_visible_bound_targets.clone();
        filtered.remove("data");
        collect_template_usage_with_bound_targets(kind, source, &credited, &filtered)
    } else {
        collect_template_usage_with_bound_targets(
            kind,
            source,
            &credited,
            template_visible_bound_targets,
        )
    };

    // A template reference credits `used_in_template`: either a bare prop name in
    // `used_bindings` (destructured prop form, or template uses the bare name) OR
    // a `<props>.<name>` / `$props.<name>` member access (the
    // `const props = defineProps()` form and Vue's implicit `$props`).
    if !combined.component_props.is_empty() {
        let member_used: FxHashSet<&str> = template_usage
            .member_accesses
            .iter()
            .filter(|access| {
                access.object == "$props"
                    || props_return_binding.is_some_and(|binding| access.object == binding)
            })
            .map(|access| access.member.as_str())
            .collect();
        for prop in &mut combined.component_props {
            if template_usage.used_bindings.contains(&prop.name)
                || template_usage.used_bindings.contains(&prop.local)
                || member_used.contains(prop.name.as_str())
            {
                prop.used_in_template = true;
            }
        }
    }

    // A custom-named `defineProps` return spread as a whole object in the template
    // (`const myProps = defineProps(); <Child v-bind="myProps" />`) consumes every
    // prop opaquely; the literal `props`/`$props`/`$attrs` regex misses a custom
    // name. The scanner records a bare `v-bind="myProps"` value as a used binding
    // (not a whole-object use), so a bare reference to the return binding in either
    // set means abstain on the whole file.
    if let Some(binding) = props_return_binding
        && (template_usage.used_bindings.contains(binding)
            || template_usage
                .whole_object_uses
                .iter()
                .any(|used| used == binding))
    {
        combined.has_props_attrs_fallthrough = true;
    }

    combined
        .unused_import_bindings
        .retain(|binding| !template_usage.used_bindings.contains(binding));
    combined
        .member_accesses
        .extend(template_usage.member_accesses);
    combined
        .whole_object_uses
        .extend(template_usage.whole_object_uses);
    combined
        .security_sinks
        .extend(template_usage.security_sinks);
    if !template_usage.unresolved_tag_names.is_empty() {
        let mut names: Vec<String> = template_usage.unresolved_tag_names.into_iter().collect();
        names.sort_unstable();
        combined.auto_import_candidates.extend(names);
        combined.auto_import_candidates.dedup();
    }
}

/// Credit emit events fired from the `<template>` (`@click="emit('close')"`,
/// `@click="$emit('remove')"`, `:close="{ onClick: () => emit('close') }"`),
/// which the script-only emit usage walk in `harvest_define_emits` cannot see.
///
/// Scans the template-only region (scripts/styles/comments masked) for
/// [`TEMPLATE_EMIT_CALL_RE`]: a call whose callee is the harvested emit binding
/// (`emit` / `emits` / whatever it was bound to) or the implicit `$emit` (always
/// available in a Vue template regardless of `<script setup>` binding). A
/// string-literal first arg credits the matching `ComponentEmit` as used; a
/// non-literal first arg (a variable / template-literal) is a dynamic template
/// emit whose event is unknowable, so the whole file abstains (`has_dynamic_emit`)
/// to preserve the zero-FP doctrine.
///
/// Over-crediting is the safe direction (it only suppresses a finding), so a
/// liberal raw-source scan is intentional here. The scan is byte-safe: the regex
/// runs over the `&str` template and only reads captured-group text, never
/// slicing at arbitrary byte offsets.
fn apply_template_emit_usage(
    source: &str,
    emit_return_binding: Option<&str>,
    combined: &mut ModuleInfo,
) {
    let masked = mask_non_markup_regions(source);
    let mut used: FxHashSet<String> = FxHashSet::default();
    let mut dynamic = false;

    for caps in TEMPLATE_EMIT_CALL_RE.captures_iter(&masked) {
        let Some(callee) = caps.get(1) else {
            continue;
        };
        let callee = callee.as_str();
        let is_emit_call =
            callee == "$emit" || emit_return_binding.is_some_and(|binding| callee == binding);
        if !is_emit_call {
            continue;
        }
        if let Some(event) = caps.get(2).or_else(|| caps.get(3)) {
            // String-literal first arg (single- or double-quoted): the event
            // name. Credit it as used.
            used.insert(event.as_str().to_string());
        } else if caps.get(4).is_some() {
            // Non-literal first arg (`$emit(someVar)`, `emit(\`x\`)`): the event
            // cannot be known. Abstain on the whole file.
            dynamic = true;
        }
    }

    if dynamic {
        combined.has_dynamic_emit = true;
    }
    if !used.is_empty() {
        for emit in &mut combined.component_emits {
            if used.contains(&emit.name) {
                emit.used = true;
            }
        }
    }
}

fn is_template_visible_script(kind: SfcKind, script: &SfcScript) -> bool {
    match kind {
        SfcKind::Vue => script.is_setup,
        SfcKind::Svelte => !script.is_context_module,
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    #[test]
    fn is_sfc_file_vue() {
        assert!(is_sfc_file(Path::new("App.vue")));
    }

    #[test]
    fn is_sfc_file_svelte() {
        assert!(is_sfc_file(Path::new("Counter.svelte")));
    }

    #[test]
    fn is_sfc_file_rejects_ts() {
        assert!(!is_sfc_file(Path::new("utils.ts")));
    }

    #[test]
    fn is_sfc_file_rejects_jsx() {
        assert!(!is_sfc_file(Path::new("App.jsx")));
    }

    #[test]
    fn is_sfc_file_rejects_astro() {
        assert!(!is_sfc_file(Path::new("Layout.astro")));
    }

    #[test]
    fn single_plain_script() {
        let scripts = extract_sfc_scripts("<script>const x = 1;</script>");
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].body, "const x = 1;");
        assert!(!scripts[0].is_typescript);
        assert!(!scripts[0].is_jsx);
        assert!(scripts[0].src.is_none());
    }

    #[test]
    fn single_ts_script() {
        let scripts = extract_sfc_scripts(r#"<script lang="ts">const x: number = 1;</script>"#);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].is_typescript);
        assert!(!scripts[0].is_jsx);
    }

    #[test]
    fn single_tsx_script() {
        let scripts = extract_sfc_scripts(r#"<script lang="tsx">const el = <div />;</script>"#);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].is_typescript);
        assert!(scripts[0].is_jsx);
    }

    #[test]
    fn single_jsx_script() {
        let scripts = extract_sfc_scripts(r#"<script lang="jsx">const el = <div />;</script>"#);
        assert_eq!(scripts.len(), 1);
        assert!(!scripts[0].is_typescript);
        assert!(scripts[0].is_jsx);
    }

    #[test]
    fn two_script_blocks() {
        let source = r#"
<script lang="ts">
export default {};
</script>
<script setup lang="ts">
const count = 0;
</script>
"#;
        let scripts = extract_sfc_scripts(source);
        assert_eq!(scripts.len(), 2);
        assert!(scripts[0].body.contains("export default"));
        assert!(scripts[1].body.contains("count"));
    }

    #[test]
    fn script_setup_extracted() {
        let scripts =
            extract_sfc_scripts(r#"<script setup lang="ts">import { ref } from 'vue';</script>"#);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].body.contains("import"));
        assert!(scripts[0].is_typescript);
    }

    #[test]
    fn script_src_detected() {
        let scripts = extract_sfc_scripts(r#"<script src="./component.ts" lang="ts"></script>"#);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].src.as_deref(), Some("./component.ts"));
    }

    #[test]
    fn data_src_not_treated_as_src() {
        let scripts =
            extract_sfc_scripts(r#"<script lang="ts" data-src="./nope.ts">const x = 1;</script>"#);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].src.is_none());
    }

    #[test]
    fn script_inside_html_comment_filtered() {
        let source = r#"
<!-- <script lang="ts">import { bad } from 'bad';</script> -->
<script lang="ts">import { good } from 'good';</script>
"#;
        let scripts = extract_sfc_scripts(source);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].body.contains("good"));
    }

    #[test]
    fn spanning_comment_filters_script() {
        let source = r#"
<!-- disabled:
<script lang="ts">import { bad } from 'bad';</script>
-->
<script lang="ts">const ok = true;</script>
"#;
        let scripts = extract_sfc_scripts(source);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].body.contains("ok"));
    }

    #[test]
    fn string_containing_comment_markers_not_corrupted() {
        let source = r#"
<script setup lang="ts">
const marker = "<!-- not a comment -->";
import { ref } from 'vue';
</script>
"#;
        let scripts = extract_sfc_scripts(source);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].body.contains("import"));
    }

    #[test]
    fn generic_attr_with_angle_bracket() {
        let source =
            r#"<script setup lang="ts" generic="T extends Foo<Bar>">const x = 1;</script>"#;
        let scripts = extract_sfc_scripts(source);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].body, "const x = 1;");
    }

    #[test]
    fn nested_generic_attr() {
        let source = r#"<script setup lang="ts" generic="T extends Map<string, Set<number>>">const x = 1;</script>"#;
        let scripts = extract_sfc_scripts(source);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].body, "const x = 1;");
    }

    #[test]
    fn lang_single_quoted() {
        let scripts = extract_sfc_scripts("<script lang='ts'>const x = 1;</script>");
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].is_typescript);
    }

    #[test]
    fn uppercase_script_tag() {
        let scripts = extract_sfc_scripts(r#"<SCRIPT lang="ts">const x = 1;</SCRIPT>"#);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].is_typescript);
    }

    #[test]
    fn no_script_block() {
        let scripts = extract_sfc_scripts("<template><div>Hello</div></template>");
        assert!(scripts.is_empty());
    }

    #[test]
    fn empty_script_body() {
        let scripts = extract_sfc_scripts(r#"<script lang="ts"></script>"#);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].body.is_empty());
    }

    #[test]
    fn whitespace_only_script() {
        let scripts = extract_sfc_scripts("<script lang=\"ts\">\n  \n</script>");
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].body.trim().is_empty());
    }

    #[test]
    fn byte_offset_is_set() {
        let source = r#"<template><div/></template><script lang="ts">code</script>"#;
        let scripts = extract_sfc_scripts(source);
        assert_eq!(scripts.len(), 1);
        let offset = scripts[0].byte_offset;
        assert_eq!(&source[offset..offset + 4], "code");
    }

    #[test]
    fn script_with_extra_attributes() {
        let scripts = extract_sfc_scripts(
            r#"<script lang="ts" id="app" type="module" data-custom="val">const x = 1;</script>"#,
        );
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].is_typescript);
        assert!(scripts[0].src.is_none());
    }

    #[test]
    fn multiple_script_blocks_exports_combined() {
        let source = r#"
<script lang="ts">
export const version = '1.0';
</script>
<script setup lang="ts">
import { ref } from 'vue';
const count = ref(0);
</script>
"#;
        let info = parse_sfc_to_module(FileId(0), Path::new("Dual.vue"), source, 0, false);
        assert!(
            info.exports
                .iter()
                .any(|e| matches!(&e.name, crate::ExportName::Named(n) if n == "version")),
            "export from <script> block should be extracted"
        );
        assert!(
            info.imports.iter().any(|i| i.source == "vue"),
            "import from <script setup> block should be extracted"
        );
    }

    #[test]
    fn lang_tsx_detected_as_typescript_jsx() {
        let scripts =
            extract_sfc_scripts(r#"<script lang="tsx">const el = <div>{x}</div>;</script>"#);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].is_typescript, "lang=tsx should be typescript");
        assert!(scripts[0].is_jsx, "lang=tsx should be jsx");
    }

    #[test]
    fn multiline_html_comment_filters_all_script_blocks_inside() {
        let source = r#"
<!--
  This whole section is disabled:
  <script lang="ts">import { bad1 } from 'bad1';</script>
  <script lang="ts">import { bad2 } from 'bad2';</script>
-->
<script lang="ts">import { good } from 'good';</script>
"#;
        let scripts = extract_sfc_scripts(source);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].body.contains("good"));
    }

    #[test]
    fn script_src_generates_side_effect_import() {
        let info = parse_sfc_to_module(
            FileId(0),
            Path::new("External.vue"),
            r#"<script src="./external-logic.ts" lang="ts"></script>"#,
            0,
            false,
        );
        assert!(
            info.imports
                .iter()
                .any(|i| i.source == "./external-logic.ts"
                    && matches!(i.imported_name, ImportedName::SideEffect)),
            "script src should generate a side-effect import"
        );
    }

    #[test]
    fn parse_sfc_no_script_returns_empty_module() {
        let info = parse_sfc_to_module(
            FileId(0),
            Path::new("Empty.vue"),
            "<template><div>Hello</div></template>",
            42,
            false,
        );
        assert!(info.imports.is_empty());
        assert!(info.exports.is_empty());
        assert_eq!(info.content_hash, 42);
        assert_eq!(info.file_id, FileId(0));
    }

    #[test]
    fn parse_sfc_has_line_offsets() {
        let info = parse_sfc_to_module(
            FileId(0),
            Path::new("LineOffsets.vue"),
            r#"<script lang="ts">const x = 1;</script>"#,
            0,
            false,
        );
        assert!(!info.line_offsets.is_empty());
    }

    #[test]
    fn parse_sfc_has_suppressions() {
        let info = parse_sfc_to_module(
            FileId(0),
            Path::new("Suppressions.vue"),
            r#"<script lang="ts">
// fallow-ignore-file
export const foo = 1;
</script>"#,
            0,
            false,
        );
        assert!(!info.suppressions.is_empty());
    }

    #[test]
    fn source_type_jsx_detection() {
        let scripts = extract_sfc_scripts(r#"<script lang="jsx">const el = <div />;</script>"#);
        assert_eq!(scripts.len(), 1);
        assert!(!scripts[0].is_typescript);
        assert!(scripts[0].is_jsx);
    }

    #[test]
    fn source_type_plain_js_detection() {
        let scripts = extract_sfc_scripts("<script>const x = 1;</script>");
        assert_eq!(scripts.len(), 1);
        assert!(!scripts[0].is_typescript);
        assert!(!scripts[0].is_jsx);
    }

    #[test]
    fn is_sfc_file_rejects_no_extension() {
        assert!(!is_sfc_file(Path::new("Makefile")));
    }

    #[test]
    fn is_sfc_file_rejects_mdx() {
        assert!(!is_sfc_file(Path::new("post.mdx")));
    }

    #[test]
    fn is_sfc_file_rejects_css() {
        assert!(!is_sfc_file(Path::new("styles.css")));
    }

    #[test]
    fn multiple_script_blocks_both_have_offsets() {
        let source = r#"<script lang="ts">const a = 1;</script>
<script setup lang="ts">const b = 2;</script>"#;
        let scripts = extract_sfc_scripts(source);
        assert_eq!(scripts.len(), 2);
        let offset0 = scripts[0].byte_offset;
        let offset1 = scripts[1].byte_offset;
        assert_eq!(
            &source[offset0..offset0 + "const a = 1;".len()],
            "const a = 1;"
        );
        assert_eq!(
            &source[offset1..offset1 + "const b = 2;".len()],
            "const b = 2;"
        );
    }

    #[test]
    fn script_with_src_and_lang() {
        let scripts = extract_sfc_scripts(r#"<script src="./logic.ts" lang="tsx"></script>"#);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].src.as_deref(), Some("./logic.ts"));
        assert!(scripts[0].is_typescript);
        assert!(scripts[0].is_jsx);
    }

    #[test]
    fn extract_style_block_lang_scss() {
        let source = r#"<template/><style lang="scss">@import 'Foo';</style>"#;
        let styles = extract_sfc_styles(source);
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].lang.as_deref(), Some("scss"));
        assert!(styles[0].body.contains("@import"));
        assert!(styles[0].src.is_none());
    }

    #[test]
    fn extract_style_block_with_src() {
        let source = r#"<style src="./theme.scss" lang="scss"></style>"#;
        let styles = extract_sfc_styles(source);
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].src.as_deref(), Some("./theme.scss"));
        assert_eq!(styles[0].lang.as_deref(), Some("scss"));
    }

    #[test]
    fn extract_style_block_plain_no_lang() {
        let source = r"<style>.foo { color: red; }</style>";
        let styles = extract_sfc_styles(source);
        assert_eq!(styles.len(), 1);
        assert!(styles[0].lang.is_none());
    }

    #[test]
    fn extract_multiple_style_blocks() {
        let source = r#"<style lang="scss">@import 'a';</style>
<style scoped lang="scss">@import 'b';</style>"#;
        let styles = extract_sfc_styles(source);
        assert_eq!(styles.len(), 2);
    }

    #[test]
    fn style_block_inside_html_comment_filtered() {
        let source = r#"<!-- <style lang="scss">@import 'bad';</style> -->
<style lang="scss">@import 'good';</style>"#;
        let styles = extract_sfc_styles(source);
        assert_eq!(styles.len(), 1);
        assert!(styles[0].body.contains("good"));
    }

    #[test]
    fn parse_sfc_extracts_style_imports_with_from_style_flag() {
        let info = parse_sfc_to_module(
            FileId(0),
            Path::new("Foo.vue"),
            r#"<template/><style lang="scss">@import 'Foo';</style>"#,
            0,
            false,
        );
        let style_import = info
            .imports
            .iter()
            .find(|i| i.source == "./Foo")
            .expect("scss @import 'Foo' should be normalized to ./Foo");
        assert!(
            style_import.from_style,
            "imports from <style> blocks must carry from_style=true so the resolver \
             enables SCSS partial fallback for the SFC importer"
        );
        assert!(matches!(
            style_import.imported_name,
            ImportedName::SideEffect
        ));
    }

    #[test]
    fn parse_sfc_extracts_style_plugin_as_default_import() {
        let info = parse_sfc_to_module(
            FileId(0),
            Path::new("Foo.vue"),
            r#"<template/><style>@plugin "./tailwind-plugin.js";</style>"#,
            0,
            false,
        );
        let plugin_import = info
            .imports
            .iter()
            .find(|i| i.source == "./tailwind-plugin.js")
            .expect("style @plugin should create an import");
        assert!(plugin_import.from_style);
        assert!(matches!(plugin_import.imported_name, ImportedName::Default));
    }

    #[test]
    fn parse_sfc_extracts_style_src_with_from_style_flag() {
        let info = parse_sfc_to_module(
            FileId(0),
            Path::new("Bar.vue"),
            r#"<style src="./Bar.scss" lang="scss"></style>"#,
            0,
            false,
        );
        let style_src = info
            .imports
            .iter()
            .find(|i| i.source == "./Bar.scss")
            .expect("<style src=\"./Bar.scss\"> should produce a side-effect import");
        assert!(style_src.from_style);
    }

    #[test]
    fn parse_sfc_skips_unsupported_style_lang_body_but_keeps_src() {
        let info = parse_sfc_to_module(
            FileId(0),
            Path::new("Baz.vue"),
            r#"<style lang="postcss" src="./Baz.pcss">@custom-rule "skipped";</style>"#,
            0,
            false,
        );
        assert!(
            info.imports.iter().any(|i| i.source == "./Baz.pcss"),
            "src reference should still be seeded for unsupported lang"
        );
        assert!(
            !info.imports.iter().any(|i| i.source.contains("skipped")),
            "postcss body should not be scanned for @import directives"
        );
    }

    fn asset_refs(source: &str) -> Vec<String> {
        super::collect_template_asset_refs(source)
            .into_iter()
            .map(|(s, _)| s)
            .collect()
    }

    #[test]
    fn captures_static_relative_template_asset_refs() {
        assert_eq!(
            asset_refs(r#"<template><img src="./logo.png" /></template>"#),
            vec!["./logo.png".to_string()]
        );
        assert_eq!(
            asset_refs(r#"<source src="../media/clip.mp4">"#),
            vec!["../media/clip.mp4".to_string()]
        );
        assert_eq!(
            asset_refs(r#"<video poster="./thumb.jpg"></video>"#),
            vec!["./thumb.jpg".to_string()]
        );
    }

    #[test]
    fn skips_dynamic_alias_root_remote_and_query_asset_refs() {
        // Dynamic bindings (Vue `:src`, `v-bind:src`, Svelte `bind:src` / `src={}`).
        assert!(asset_refs(r#"<img :src="logo" />"#).is_empty());
        assert!(asset_refs(r#"<img v-bind:src="logo" />"#).is_empty());
        assert!(asset_refs(r#"<img bind:src="logo" />"#).is_empty());
        assert!(asset_refs(r"<img src={logo} />").is_empty());
        assert!(asset_refs(r#"<img data-src="./x.png" />"#).is_empty());
        // Alias-prefixed, root-relative, remote, bare: not plain relative literals.
        assert!(asset_refs(r#"<img src="@/assets/x.png" />"#).is_empty());
        assert!(asset_refs(r#"<img src="/logo.png" />"#).is_empty());
        assert!(asset_refs(r#"<img src="https://cdn/x.png" />"#).is_empty());
        // Query / hash suffix abstains (the resolver cannot verify them).
        assert!(asset_refs(r#"<img src="./x.png?inline" />"#).is_empty());
        // Interpolated value abstains.
        assert!(asset_refs(r#"<img src="{{ logo }}" />"#).is_empty());
    }

    #[test]
    fn skips_custom_component_src_prop() {
        // A custom component's `src` PROP must never be read as an asset edge.
        assert!(asset_refs(r#"<MyImage src="./x.png" />"#).is_empty());
        assert!(asset_refs(r#"<AppIcon src="../icons/y.svg" />"#).is_empty());
    }

    #[test]
    fn skips_asset_refs_inside_script_style_and_comments() {
        // Masked regions must not contribute asset refs.
        assert!(asset_refs(r#"<script>const x = "<img src='./a.png'>"</script>"#).is_empty());
        assert!(asset_refs(r#"<style>/* <img src="./b.png"> */ .x{}</style>"#).is_empty());
        assert!(asset_refs(r#"<!-- <img src="./c.png" /> -->"#).is_empty());
    }

    #[test]
    fn parse_sfc_emits_template_asset_as_side_effect_import() {
        let info = parse_sfc_to_module(
            FileId(0),
            Path::new("Hero.vue"),
            r#"<template><img src="./hero.png" /></template><script>let x=1</script>"#,
            0,
            false,
        );
        assert!(
            info.imports.iter().any(|i| i.source == "./hero.png"
                && matches!(i.imported_name, ImportedName::SideEffect)
                && !i.from_style),
            "template <img src> should seed a SideEffect import: {:?}",
            info.imports
        );
    }
}
