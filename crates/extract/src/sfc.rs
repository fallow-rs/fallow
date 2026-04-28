//! Vue/Svelte Single File Component (SFC) script and style extraction.
//!
//! Extracts `<script>` block content from `.vue` and `.svelte` files using regex,
//! handling `lang`, `src`, and `generic` attributes, and filtering HTML comments.
//! Also extracts `<style>` block sources (`@import` / `@use` / `@forward` and
//! `<style src="...">`) so referenced CSS / SCSS files become reachable from the
//! component, preventing false `unused-files` reports on co-located styles.

use std::path::Path;
use std::sync::LazyLock;

use oxc_allocator::Allocator;
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::asset_url::normalize_asset_url;
use crate::parse::compute_import_binding_usage;
use crate::sfc_template::{SfcKind, collect_template_usage_with_bound_targets};
use crate::visitor::ModuleInfoExtractor;
use crate::{ImportInfo, ImportedName, ModuleInfo};
use fallow_types::discover::FileId;
use fallow_types::extract::{FunctionComplexity, byte_offset_to_line_col, compute_line_offsets};
use oxc_span::Span;

/// Regex to extract `<script>` block content from Vue/Svelte SFCs.
/// The attrs pattern handles `>` inside quoted attribute values (e.g., `generic="T extends Foo<Bar>"`).
static SCRIPT_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r#"(?is)<script\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>(?P<body>[\s\S]*?)</script>"#,
    )
    .expect("valid regex")
});

/// Regex to extract the `lang` attribute value from a script tag.
static LANG_ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"lang\s*=\s*["'](\w+)["']"#).expect("valid regex"));

/// Regex to extract the `src` attribute value from a script tag.
/// Requires whitespace (or start of string) before `src` to avoid matching `data-src` etc.
static SRC_ATTR_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?:^|\s)src\s*=\s*["']([^"']+)["']"#).expect("valid regex")
});

/// Regex to detect Vue's bare `setup` attribute.
static SETUP_ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?:^|\s)setup(?:\s|$)").expect("valid regex"));

/// Regex to detect Svelte's `context="module"` attribute.
static CONTEXT_MODULE_ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"context\s*=\s*["']module["']"#).expect("valid regex"));

/// Regex to match HTML comments for filtering script blocks inside comments.
static HTML_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)<!--.*?-->").expect("valid regex"));

/// Regex to extract `<style>` block content from Vue/Svelte SFCs.
/// Mirrors `SCRIPT_BLOCK_RE`: handles `>` inside quoted attribute values and
/// captures the body so `@import` / `@use` / `@forward` directives can be parsed.
static STYLE_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r#"(?is)<style\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>(?P<body>[\s\S]*?)</style>"#,
    )
    .expect("valid regex")
});

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
    /// Whether this script is a Vue `<script setup>` block.
    pub is_setup: bool,
    /// Whether this script is a Svelte module-context block.
    pub is_context_module: bool,
}

/// Extract all `<script>` blocks from a Vue/Svelte SFC source string.
pub fn extract_sfc_scripts(source: &str) -> Vec<SfcScript> {
    // Build HTML comment ranges to filter out <script> blocks inside comments.
    // Using ranges instead of source replacement avoids corrupting script body content
    // (e.g., string literals containing "<!--" would be destroyed by replacement).
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
            let is_setup = SETUP_ATTR_RE.is_match(attrs);
            let is_context_module = CONTEXT_MODULE_ATTR_RE.is_match(attrs);
            SfcScript {
                body,
                is_typescript,
                is_jsx,
                byte_offset,
                src,
                is_setup,
                is_context_module,
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
            let lang = LANG_ATTR_RE
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());
            let src = SRC_ATTR_RE
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());
            SfcStyle { body, lang, src }
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

    for script in &scripts {
        merge_script_into_module(
            kind,
            script,
            &mut combined,
            &mut template_visible_imports,
            &mut template_visible_bound_targets,
            need_complexity,
        );
    }

    for style in &styles {
        merge_style_into_module(style, &mut combined);
    }

    apply_template_usage(
        kind,
        source,
        &template_visible_imports,
        &template_visible_bound_targets,
        &mut combined,
    );
    combined.unused_import_bindings.sort_unstable();
    combined.unused_import_bindings.dedup();
    combined.type_referenced_import_bindings.sort_unstable();
    combined.type_referenced_import_bindings.dedup();
    combined.value_referenced_import_bindings.sort_unstable();
    combined.value_referenced_import_bindings.dedup();

    combined
}

fn sfc_kind(path: &Path) -> SfcKind {
    if path.extension().and_then(|ext| ext.to_str()) == Some("vue") {
        SfcKind::Vue
    } else {
        SfcKind::Svelte
    }
}

fn empty_sfc_module(file_id: FileId, source: &str, content_hash: u64) -> ModuleInfo {
    // For SFC files, use string scanning for suppression comments since script block
    // byte offsets don't correspond to the original file positions.
    let suppressions = crate::suppress::parse_suppressions_from_source(source);

    ModuleInfo {
        file_id,
        exports: Vec::new(),
        imports: Vec::new(),
        re_exports: Vec::new(),
        dynamic_imports: Vec::new(),
        dynamic_import_patterns: Vec::new(),
        require_calls: Vec::new(),
        member_accesses: Vec::new(),
        whole_object_uses: Vec::new(),
        has_cjs_exports: false,
        content_hash,
        suppressions,
        unused_import_bindings: Vec::new(),
        type_referenced_import_bindings: Vec::new(),
        value_referenced_import_bindings: Vec::new(),
        line_offsets: compute_line_offsets(source),
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
    }
}

fn merge_script_into_module(
    kind: SfcKind,
    script: &SfcScript,
    combined: &mut ModuleInfo,
    template_visible_imports: &mut FxHashSet<String>,
    template_visible_bound_targets: &mut FxHashMap<String, String>,
    need_complexity: bool,
) {
    if let Some(src) = &script.src {
        add_script_src_import(combined, src);
    }

    let allocator = Allocator::default();
    let parser_return =
        Parser::new(&allocator, &script.body, source_type_for_script(script)).parse();
    let mut extractor = ModuleInfoExtractor::new();
    extractor.visit_program(&parser_return.program);

    let binding_usage = compute_import_binding_usage(&parser_return.program, &extractor.imports);
    combined
        .unused_import_bindings
        .extend(binding_usage.unused.iter().cloned());
    combined
        .type_referenced_import_bindings
        .extend(binding_usage.type_referenced.iter().cloned());
    combined
        .value_referenced_import_bindings
        .extend(binding_usage.value_referenced.iter().cloned());
    if need_complexity {
        combined.complexity.extend(translate_script_complexity(
            script,
            &parser_return.program,
            &combined.line_offsets,
        ));
    }

    if is_template_visible_script(kind, script) {
        template_visible_imports.extend(
            extractor
                .imports
                .iter()
                .filter(|import| !import.local_name.is_empty())
                .map(|import| import.local_name.clone()),
        );
        template_visible_bound_targets.extend(
            extractor
                .binding_target_names()
                .iter()
                .filter(|(local, _)| !local.starts_with("this."))
                .map(|(local, target)| (local.clone(), target.clone())),
        );
    }

    extractor.merge_into(combined);
}

fn translate_script_complexity(
    script: &SfcScript,
    program: &oxc_ast::ast::Program<'_>,
    sfc_line_offsets: &[u32],
) -> Vec<FunctionComplexity> {
    let script_line_offsets = compute_line_offsets(&script.body);
    let mut complexity = crate::complexity::compute_complexity(program, &script_line_offsets);
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

fn add_script_src_import(module: &mut ModuleInfo, source: &str) {
    // Normalize bare filenames (e.g., `<script src="logic.ts">`) so the
    // resolver treats them as file-relative references, not npm packages.
    module.imports.push(ImportInfo {
        source: normalize_asset_url(source),
        imported_name: ImportedName::SideEffect,
        local_name: String::new(),
        is_type_only: false,
        from_style: false,
        span: Span::default(),
        source_span: Span::default(),
    });
}

/// `lang` attribute values whose body we know how to scan for `@import` /
/// `@use` / `@forward` directives. Plain `<style>` (no `lang`) is treated as
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
    // <style src="./theme.scss"> is symmetric to <script src="...">: seed the
    // referenced file as a side-effect import. The resolver still applies SCSS
    // partial / include-path / node_modules fallbacks because `from_style` is
    // set on the import.
    if let Some(src) = &style.src {
        combined.imports.push(ImportInfo {
            source: normalize_asset_url(src),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: true,
            span: Span::default(),
            source_span: Span::default(),
        });
    }

    let lang = style.lang.as_deref();
    let is_scss = style_lang_is_scss(lang);
    let is_css_like = style_lang_is_css_like(lang);
    if !is_scss && !is_css_like {
        return;
    }

    for spec in crate::css::extract_css_imports(&style.body, is_scss) {
        combined.imports.push(ImportInfo {
            source: spec,
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: true,
            span: Span::default(),
            source_span: Span::default(),
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

fn apply_template_usage(
    kind: SfcKind,
    source: &str,
    template_visible_imports: &FxHashSet<String>,
    template_visible_bound_targets: &FxHashMap<String, String>,
    combined: &mut ModuleInfo,
) {
    if template_visible_imports.is_empty() && template_visible_bound_targets.is_empty() {
        return;
    }

    let template_usage = collect_template_usage_with_bound_targets(
        kind,
        source,
        template_visible_imports,
        template_visible_bound_targets,
    );
    combined
        .unused_import_bindings
        .retain(|binding| !template_usage.used_bindings.contains(binding));
    combined
        .member_accesses
        .extend(template_usage.member_accesses);
    combined
        .whole_object_uses
        .extend(template_usage.whole_object_uses);
}

fn is_template_visible_script(kind: SfcKind, script: &SfcScript) -> bool {
    match kind {
        SfcKind::Vue => script.is_setup,
        SfcKind::Svelte => !script.is_context_module,
    }
}

// SFC tests exercise regex-based HTML string extraction — no unsafe code,
// no Miri-specific value. Oxc parser tests are additionally ~1000x slower.
#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    // ── is_sfc_file ──────────────────────────────────────────────

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

    // ── extract_sfc_scripts: single script block ─────────────────

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

    // ── Multiple script blocks ───────────────────────────────────

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

    // ── <script setup> ───────────────────────────────────────────

    #[test]
    fn script_setup_extracted() {
        let scripts =
            extract_sfc_scripts(r#"<script setup lang="ts">import { ref } from 'vue';</script>"#);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].body.contains("import"));
        assert!(scripts[0].is_typescript);
    }

    // ── <script src="..."> external script ───────────────────────

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

    // ── HTML comment filtering ───────────────────────────────────

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
        // A string in the script body containing <!-- should not cause filtering issues
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

    // ── Generic attributes with > in quoted values ───────────────

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

    // ── lang attribute with single quotes ────────────────────────

    #[test]
    fn lang_single_quoted() {
        let scripts = extract_sfc_scripts("<script lang='ts'>const x = 1;</script>");
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].is_typescript);
    }

    // ── Case-insensitive matching ────────────────────────────────

    #[test]
    fn uppercase_script_tag() {
        let scripts = extract_sfc_scripts(r#"<SCRIPT lang="ts">const x = 1;</SCRIPT>"#);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].is_typescript);
    }

    // ── Edge cases ───────────────────────────────────────────────

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
        // The byte_offset should point to where "code" starts in the source
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

    // ── Full parse tests (Oxc parser ~1000x slower under Miri) ──

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
        // The non-setup block exports `version`
        assert!(
            info.exports
                .iter()
                .any(|e| matches!(&e.name, crate::ExportName::Named(n) if n == "version")),
            "export from <script> block should be extracted"
        );
        // The setup block imports `ref` from 'vue'
        assert!(
            info.imports.iter().any(|i| i.source == "vue"),
            "import from <script setup> block should be extracted"
        );
    }

    // ── lang="tsx" detection ────────────────────────────────────

    #[test]
    fn lang_tsx_detected_as_typescript_jsx() {
        let scripts =
            extract_sfc_scripts(r#"<script lang="tsx">const el = <div>{x}</div>;</script>"#);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].is_typescript, "lang=tsx should be typescript");
        assert!(scripts[0].is_jsx, "lang=tsx should be jsx");
    }

    // ── HTML comment filtering of script blocks ─────────────────

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

    // ── <script src="..."> generates side-effect import ─────────

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

    // ── Additional coverage ─────────────────────────────────────

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
        // Both scripts should have valid byte offsets
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
        // src + lang should both be detected
        let scripts = extract_sfc_scripts(r#"<script src="./logic.ts" lang="tsx"></script>"#);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].src.as_deref(), Some("./logic.ts"));
        assert!(scripts[0].is_typescript);
        assert!(scripts[0].is_jsx);
    }

    // ── extract_sfc_styles (issue #195 Case B) ──

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
        // <style lang="postcss"> body is NOT scanned (custom directives); src is still seeded.
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
}
