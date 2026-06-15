//! CSS/SCSS file parsing and CSS Module class name extraction.
//!
//! Handles `@import`, `@use`, `@forward`, `@plugin`, `@apply`, `@tailwind` directives,
//! and extracts class names as named exports from `.module.css`/`.module.scss` files.

use std::path::Path;
use std::sync::LazyLock;

use oxc_span::Span;

use crate::{ExportInfo, ExportName, ImportInfo, ImportedName, ModuleInfo, VisibilityTag};
use fallow_types::discover::FileId;

/// Regex to extract CSS @import sources.
/// Matches: @import "path"; @import 'path'; @import url("path"); @import url('path'); @import url(path);
static CSS_IMPORT_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"@import\s+(?:url\(\s*(?:["']([^"']+)["']|([^)]+))\s*\)|["']([^"']+)["'])"#,
    )
});

/// Regex to extract SCSS @use and @forward sources.
/// Matches: @use "path"; @use 'path'; @forward "path"; @forward 'path';
static SCSS_USE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"@(?:use|forward)\s+["']([^"']+)["']"#));

/// Regex to extract Tailwind CSS @plugin sources.
/// Matches: @plugin "package"; @plugin 'package'; @plugin "./local-plugin.js";
static CSS_PLUGIN_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"@plugin\s+["']([^"']+)["']"#));

/// Regex to extract @apply class references.
/// Matches: @apply class1 class2 class3;
static CSS_APPLY_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"@apply\s+[^;}\n]+"));

/// Regex to extract @tailwind directives.
/// Matches: @tailwind base; @tailwind components; @tailwind utilities;
static CSS_TAILWIND_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"@tailwind\s+\w+"));

/// Regex to match CSS block comments (`/* ... */`) for stripping before extraction.
static CSS_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)/\*.*?\*/"));

/// Regex to match SCSS single-line comments (`// ...`) for stripping before extraction.
static SCSS_LINE_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"//[^\n]*"));

/// Regex to extract CSS class names from selectors.
/// Matches `.className` in selectors. Applied after stripping comments, strings, and URLs.
static CSS_CLASS_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"\.([a-zA-Z_][a-zA-Z0-9_-]*)"));

/// Regex to strip quoted strings and `url(...)` content from CSS before class extraction.
/// Prevents false positives from `content: ".foo"` and `url(./path/file.ext)`.
static CSS_NON_SELECTOR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"(?s)"[^"]*"|'[^']*'|url\([^)]*\)"#));

/// Regex to strip the prelude of `@layer` and `@import` at-rules before
/// CSS-Modules class extraction. Matches the `@keyword` plus everything up to
/// (but not including) the next `;` or `{`, so block bodies are preserved.
///
/// Narrow allowlist by design (issue #540): only at-rules whose preludes
/// legitimately carry dot-separated identifiers without selector semantics are
/// stripped. `@layer foo.bar` (CSS Cascading & Inheritance L5) lists layer
/// names; `@import url("x.css") layer(theme.button)` carries a parenthesised
/// layer reference. `@scope (.foo) to (.bar)` keeps its existing behavior
/// because the prelude IS a selector list and `.foo` / `.bar` are real class
/// references that the user may want to surface as exports.
static CSS_AT_RULE_PRELUDE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"@(?:layer|import)\b[^;{]*"));

pub(crate) fn is_css_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "css" || ext == "scss")
}

/// A CSS import source with both the literal source and fallow's resolver-normalized form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssImportSource {
    /// The import source exactly as it appeared in `@import` / `@use` / `@forward` / `@plugin`.
    pub raw: String,
    /// The source normalized for fallow's resolver (`variables` -> `./variables` in SCSS).
    pub normalized: String,
    /// Whether this source came from Tailwind CSS `@plugin`.
    pub is_plugin: bool,
    /// Span of the source specifier in the original CSS/SCSS input.
    pub span: Span,
}

fn is_css_module_file(path: &Path) -> bool {
    is_css_file(path)
        && path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| stem.ends_with(".module"))
}

/// Returns true if a CSS import source is a remote URL or data URI that should be skipped.
fn is_css_url_import(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://") || source.starts_with("data:")
}

/// Normalize a CSS/SCSS import path to use `./` prefix for relative paths.
/// Bare file names such as `reset.css` stay relative for CSS ergonomics, while
/// package subpaths such as `tailwindcss/theme.css` stay bare so bundler-style
/// package CSS imports resolve through `node_modules`.
///
/// When `is_scss` is true, extensionless specifiers that are not SCSS built-in
/// modules (`sass:*`) are treated as relative imports (SCSS partial convention).
/// This handles `@use 'variables'` resolving to `./_variables.scss`.
///
/// Scoped npm packages (`@scope/pkg`) are always kept bare, even when they have
/// CSS extensions (e.g., `@fontsource/monaspace-neon/400.css`). Bundlers like
/// Vite resolve these from node_modules, not as relative paths.
fn normalize_css_import_path(path: String, is_scss: bool) -> String {
    if path.starts_with('.') || path.starts_with('/') || path.contains("://") {
        return path;
    }
    if path.starts_with('@') && path.contains('/') {
        return path;
    }
    let path_ref = std::path::Path::new(&path);
    if !is_scss
        && path.contains('/')
        && path_ref
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(is_style_extension)
    {
        return path;
    }
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str());
    match ext {
        Some(e) if is_style_extension(e) => format!("./{path}"),
        _ => {
            if is_scss && !path.contains(':') {
                format!("./{path}")
            } else {
                path
            }
        }
    }
}

fn is_style_extension(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("css")
        || ext.eq_ignore_ascii_case("scss")
        || ext.eq_ignore_ascii_case("sass")
        || ext.eq_ignore_ascii_case("less")
}

/// Strip comments from CSS/SCSS source to avoid matching directives inside comments.
#[cfg(test)]
fn strip_css_comments(source: &str, is_scss: bool) -> String {
    let stripped = CSS_COMMENT_RE.replace_all(source, "");
    if is_scss {
        SCSS_LINE_COMMENT_RE.replace_all(&stripped, "").into_owned()
    } else {
        stripped.into_owned()
    }
}

fn mask_css_comments(source: &str, is_scss: bool) -> String {
    let mut masked = mask_with_whitespace(source, &CSS_COMMENT_RE);
    if is_scss {
        masked = mask_with_whitespace(&masked, &SCSS_LINE_COMMENT_RE);
    }
    masked
}

/// Normalize a Tailwind CSS `@plugin` target.
///
/// Unlike SCSS `@use`, extensionless targets such as `daisyui` are package
/// specifiers, not local partials. Keep bare specifiers bare and only preserve
/// explicit relative/root-relative paths.
fn normalize_css_plugin_path(path: String) -> String {
    path
}

/// Extract `@import` / `@use` / `@forward` / `@plugin` source paths from a CSS/SCSS string.
///
/// Returns both the raw source and the normalized source. URL imports
/// (`http://`, `https://`, `data:`) are skipped. Use [`extract_css_imports`]
/// when only the normalized form is needed.
#[must_use]
pub fn extract_css_import_sources(source: &str, is_scss: bool) -> Vec<CssImportSource> {
    let stripped = mask_css_comments(source, is_scss);
    let mut out = Vec::new();

    for cap in CSS_IMPORT_RE.captures_iter(&stripped) {
        let raw = cap.get(1).or_else(|| cap.get(2)).or_else(|| cap.get(3));
        if let Some(m) = raw {
            let (src, span) = trimmed_match_with_span(m);
            if !src.is_empty() && !is_css_url_import(&src) {
                out.push(CssImportSource {
                    normalized: normalize_css_import_path(src.clone(), is_scss),
                    raw: src,
                    is_plugin: false,
                    span,
                });
            }
        }
    }

    if is_scss {
        for cap in SCSS_USE_RE.captures_iter(&stripped) {
            if let Some(m) = cap.get(1) {
                let (raw, span) = trimmed_match_with_span(m);
                out.push(CssImportSource {
                    normalized: normalize_css_import_path(raw.clone(), true),
                    raw,
                    is_plugin: false,
                    span,
                });
            }
        }
    }

    for cap in CSS_PLUGIN_RE.captures_iter(&stripped) {
        if let Some(m) = cap.get(1) {
            let (raw, span) = trimmed_match_with_span(m);
            if !raw.is_empty() && !is_css_url_import(&raw) {
                out.push(CssImportSource {
                    normalized: normalize_css_plugin_path(raw.clone()),
                    raw,
                    is_plugin: true,
                    span,
                });
            }
        }
    }

    out
}

fn trimmed_match_with_span(m: regex::Match<'_>) -> (String, Span) {
    let raw = m.as_str();
    let trimmed_start = raw.len() - raw.trim_start().len();
    let trimmed_end = raw.trim_end().len();
    let start = m.start() + trimmed_start;
    let end = m.start() + trimmed_end;
    (raw.trim().to_string(), Span::new(start as u32, end as u32))
}

/// Extract normalized `@import` / `@use` / `@forward` / `@plugin` source paths from a CSS/SCSS string.
///
/// Returns specifiers normalized via `normalize_css_import_path`. URL imports
/// (`http://`, `https://`, `data:`) are skipped. Used by callers that only need
/// entry/dependency source paths; callers that need import kind information
/// should use [`extract_css_import_sources`].
#[must_use]
pub fn extract_css_imports(source: &str, is_scss: bool) -> Vec<String> {
    extract_css_import_sources(source, is_scss)
        .into_iter()
        .map(|source| source.normalized)
        .collect()
}

/// Mask every regex match in `src` with ASCII spaces (`0x20`) of equal byte
/// length, so byte offsets in the returned string correspond 1:1 to byte
/// offsets in the original.
///
/// Used to neutralise CSS comments, quoted strings, `url(...)`, and at-rule
/// preludes before scanning for `.class` selectors, while preserving the
/// original-source positions that callers need to populate `ExportInfo.span`
/// (issue #549). The `regex` crate guarantees match boundaries respect UTF-8
/// char boundaries, so the masked buffer is always valid UTF-8.
fn mask_with_whitespace(src: &str, re: &regex::Regex) -> String {
    let mut out = String::with_capacity(src.len());
    let mut cursor = 0;
    for m in re.find_iter(src) {
        out.push_str(&src[cursor..m.start()]);
        for _ in m.start()..m.end() {
            out.push(' ');
        }
        cursor = m.end();
    }
    out.push_str(&src[cursor..]);
    out
}

/// Extract class names from a CSS module file as named exports.
///
/// Each emitted [`ExportInfo`] carries a [`Span`] pointing at the bare class
/// name in the ORIGINAL `source` (no leading dot), so downstream
/// `compute_line_offsets` resolves the real declaration line and column
/// instead of falling back to line:1 col:0 (issue #549).
pub fn extract_css_module_exports(source: &str, is_scss: bool) -> Vec<ExportInfo> {
    let mut masked = mask_with_whitespace(source, &CSS_COMMENT_RE);
    if is_scss {
        masked = mask_with_whitespace(&masked, &SCSS_LINE_COMMENT_RE);
    }
    masked = mask_with_whitespace(&masked, &CSS_NON_SELECTOR_RE);
    masked = mask_with_whitespace(&masked, &CSS_AT_RULE_PRELUDE_RE);

    let mut seen = rustc_hash::FxHashSet::default();
    let mut exports = Vec::new();
    for cap in CSS_CLASS_RE.captures_iter(&masked) {
        if let Some(m) = cap.get(1) {
            let class_name = m.as_str().to_string();
            if seen.insert(class_name.clone()) {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "CSS files exceeding u32::MAX bytes are not a realistic input"
                )]
                let span = Span::new(m.start() as u32, m.end() as u32);
                exports.push(ExportInfo {
                    name: ExportName::Named(class_name),
                    local_name: None,
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span,
                    members: Vec::new(),
                    is_side_effect_used: false,
                    super_class: None,
                });
            }
        }
    }
    exports
}

/// Parse a CSS/SCSS file, extracting @import, @use, @forward, @plugin, @apply, and @tailwind directives.
pub(crate) fn parse_css_to_module(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
) -> ModuleInfo {
    let parsed_suppressions = crate::suppress::parse_suppressions_from_source(source);
    let is_scss = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "scss");

    let stripped = mask_css_comments(source, is_scss);

    let mut imports = Vec::new();

    for source in extract_css_import_sources(source, is_scss) {
        imports.push(ImportInfo {
            source: source.normalized,
            imported_name: if source.is_plugin {
                ImportedName::Default
            } else {
                ImportedName::SideEffect
            },
            local_name: String::new(),
            is_type_only: false,
            from_style: false,
            span: source.span,
            source_span: source.span,
        });
    }

    let has_apply = CSS_APPLY_RE.is_match(&stripped);
    let has_tailwind = CSS_TAILWIND_RE.is_match(&stripped);
    if has_apply || has_tailwind {
        imports.push(ImportInfo {
            source: "tailwindcss".to_string(),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: false,
            span: Span::default(),
            source_span: Span::default(),
        });
    }

    let exports = if is_css_module_file(path) {
        extract_css_module_exports(source, is_scss)
    } else {
        Vec::new()
    };

    ModuleInfo {
        file_id,
        exports,
        imports,
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
        suppressions: parsed_suppressions.suppressions,
        unknown_suppression_kinds: parsed_suppressions.unknown_kinds,
        unused_import_bindings: Vec::new(),
        type_referenced_import_bindings: Vec::new(),
        value_referenced_import_bindings: Vec::new(),
        line_offsets: fallow_types::extract::compute_line_offsets(source),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to collect export names as strings from `extract_css_module_exports`.
    fn export_names(source: &str) -> Vec<String> {
        extract_css_module_exports(source, false)
            .into_iter()
            .filter_map(|e| match e.name {
                ExportName::Named(n) => Some(n),
                ExportName::Default => None,
            })
            .collect()
    }

    #[test]
    fn is_css_file_css() {
        assert!(is_css_file(Path::new("styles.css")));
    }

    #[test]
    fn is_css_file_scss() {
        assert!(is_css_file(Path::new("styles.scss")));
    }

    #[test]
    fn is_css_file_rejects_js() {
        assert!(!is_css_file(Path::new("app.js")));
    }

    #[test]
    fn is_css_file_rejects_ts() {
        assert!(!is_css_file(Path::new("app.ts")));
    }

    #[test]
    fn is_css_file_rejects_less() {
        assert!(!is_css_file(Path::new("styles.less")));
    }

    #[test]
    fn is_css_file_rejects_no_extension() {
        assert!(!is_css_file(Path::new("Makefile")));
    }

    #[test]
    fn is_css_module_file_module_css() {
        assert!(is_css_module_file(Path::new("Component.module.css")));
    }

    #[test]
    fn is_css_module_file_module_scss() {
        assert!(is_css_module_file(Path::new("Component.module.scss")));
    }

    #[test]
    fn is_css_module_file_rejects_plain_css() {
        assert!(!is_css_module_file(Path::new("styles.css")));
    }

    #[test]
    fn is_css_module_file_rejects_plain_scss() {
        assert!(!is_css_module_file(Path::new("styles.scss")));
    }

    #[test]
    fn is_css_module_file_rejects_module_js() {
        assert!(!is_css_module_file(Path::new("utils.module.js")));
    }

    #[test]
    fn extracts_single_class() {
        let names = export_names(".foo { color: red; }");
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn extracts_multiple_classes() {
        let names = export_names(".foo { } .bar { }");
        assert_eq!(names, vec!["foo", "bar"]);
    }

    #[test]
    fn extracts_nested_classes() {
        let names = export_names(".foo .bar { color: red; }");
        assert!(names.contains(&"foo".to_string()));
        assert!(names.contains(&"bar".to_string()));
    }

    #[test]
    fn extracts_hyphenated_class() {
        let names = export_names(".my-class { }");
        assert_eq!(names, vec!["my-class"]);
    }

    #[test]
    fn extracts_camel_case_class() {
        let names = export_names(".myClass { }");
        assert_eq!(names, vec!["myClass"]);
    }

    #[test]
    fn extracts_underscore_class() {
        let names = export_names("._hidden { } .__wrapper { }");
        assert!(names.contains(&"_hidden".to_string()));
        assert!(names.contains(&"__wrapper".to_string()));
    }

    #[test]
    fn pseudo_selector_hover() {
        let names = export_names(".foo:hover { color: blue; }");
        assert_eq!(names, vec!["foo"]);
    }

    #[test]
    fn pseudo_selector_focus() {
        let names = export_names(".input:focus { outline: none; }");
        assert_eq!(names, vec!["input"]);
    }

    #[test]
    fn pseudo_element_before() {
        let names = export_names(".icon::before { content: ''; }");
        assert_eq!(names, vec!["icon"]);
    }

    #[test]
    fn combined_pseudo_selectors() {
        let names = export_names(".btn:hover, .btn:active, .btn:focus { }");
        assert_eq!(names, vec!["btn"]);
    }

    #[test]
    fn classes_inside_media_query() {
        let names = export_names(
            "@media (max-width: 768px) { .mobile-nav { display: block; } .desktop-nav { display: none; } }",
        );
        assert!(names.contains(&"mobile-nav".to_string()));
        assert!(names.contains(&"desktop-nav".to_string()));
    }

    #[test]
    fn classes_inside_multi_line_media_query() {
        let names =
            export_names("@media\n  screen and (min-width: 600px)\n{\n  .real { color: red; }\n}");
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn at_layer_statement_does_not_export() {
        let names = export_names("@layer foo.bar;");
        assert!(names.is_empty(), "got {names:?}");
        let names = export_names("@layer foo.bar, foo.baz;");
        assert!(names.is_empty(), "got {names:?}");
    }

    #[test]
    fn at_layer_block_keeps_body_classes() {
        let names = export_names("@layer foo.bar { .root { color: red; } }");
        assert_eq!(names, vec!["root"]);
    }

    #[test]
    fn at_layer_multiline_prelude_keeps_body_classes() {
        let names = export_names("@layer\n  foo.bar\n{ .root { color: red; } }");
        assert_eq!(names, vec!["root"]);
    }

    #[test]
    fn at_layer_with_nested_media_keeps_body() {
        let names =
            export_names("@layer foo.bar { @media (max-width: 768px) { .real { color: red; } } }");
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn at_import_with_layer_attribute_does_not_export() {
        let names = export_names(r#"@import url("x.css") layer(theme.button);"#);
        assert!(names.is_empty(), "got {names:?}");
    }

    #[test]
    fn class_then_at_layer_does_not_leak_prelude() {
        let names =
            export_names(".outer { color: blue; } @layer foo.bar { .inner { color: red; } }");
        assert_eq!(names, vec!["outer", "inner"]);
    }

    #[test]
    fn at_scope_keeps_selector_list_classes() {
        let names = export_names("@scope (.parent) to (.child) { .title { color: red; } }");
        assert!(names.contains(&"parent".to_string()), "got {names:?}");
        assert!(names.contains(&"child".to_string()), "got {names:?}");
        assert!(names.contains(&"title".to_string()), "got {names:?}");
    }

    #[test]
    fn at_keyframes_numeric_step_is_not_class() {
        let names = export_names(
            "@keyframes slide { 0% { transform: scale(.5); } 100% { transform: scale(1); } }",
        );
        assert!(names.is_empty(), "got {names:?}");
    }

    #[test]
    fn at_webkit_keyframes_keeps_body_classes() {
        let names = export_names("@-webkit-keyframes slide { 0% { } 100% { } } .real { }");
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn deduplicates_repeated_class() {
        let names = export_names(".btn { color: red; } .btn { font-size: 14px; }");
        assert_eq!(names.iter().filter(|n| *n == "btn").count(), 1);
    }

    #[test]
    fn empty_source() {
        let names = export_names("");
        assert!(names.is_empty());
    }

    #[test]
    fn no_classes() {
        let names = export_names("body { margin: 0; } * { box-sizing: border-box; }");
        assert!(names.is_empty());
    }

    #[test]
    fn ignores_classes_in_block_comments() {
        let names = export_names("/* .fake { } */ .real { }");
        assert!(!names.contains(&"fake".to_string()));
        assert!(names.contains(&"real".to_string()));
    }

    #[test]
    fn ignores_classes_in_scss_line_comments() {
        let exports = extract_css_module_exports("// .fake\n.real { }", true);
        let names: Vec<_> = exports
            .iter()
            .filter_map(|e| match &e.name {
                ExportName::Named(n) => Some(n.as_str()),
                ExportName::Default => None,
            })
            .collect();
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn ignores_classes_in_strings() {
        let names = export_names(r#".real { content: ".fake"; }"#);
        assert!(names.contains(&"real".to_string()));
        assert!(!names.contains(&"fake".to_string()));
    }

    #[test]
    fn ignores_classes_in_url() {
        let names = export_names(".real { background: url(./images/hero.png); }");
        assert!(names.contains(&"real".to_string()));
        assert!(!names.contains(&"png".to_string()));
    }

    #[test]
    fn strip_css_block_comment() {
        let result = strip_css_comments("/* removed */ .kept { }", false);
        assert!(!result.contains("removed"));
        assert!(result.contains(".kept"));
    }

    #[test]
    fn strip_scss_line_comment() {
        let result = strip_css_comments("// removed\n.kept { }", true);
        assert!(!result.contains("removed"));
        assert!(result.contains(".kept"));
    }

    #[test]
    fn strip_scss_preserves_css_outside_comments() {
        let source = "// line comment\n/* block comment */\n.visible { color: red; }";
        let result = strip_css_comments(source, true);
        assert!(result.contains(".visible"));
    }

    #[test]
    fn url_import_http() {
        assert!(is_css_url_import("http://example.com/style.css"));
    }

    #[test]
    fn url_import_https() {
        assert!(is_css_url_import("https://fonts.googleapis.com/css"));
    }

    #[test]
    fn url_import_data() {
        assert!(is_css_url_import("data:text/css;base64,abc"));
    }

    #[test]
    fn url_import_local_not_skipped() {
        assert!(!is_css_url_import("./local.css"));
    }

    #[test]
    fn url_import_bare_specifier_not_skipped() {
        assert!(!is_css_url_import("tailwindcss"));
    }

    #[test]
    fn normalize_relative_dot_path_unchanged() {
        assert_eq!(
            normalize_css_import_path("./reset.css".to_string(), false),
            "./reset.css"
        );
    }

    #[test]
    fn normalize_parent_relative_path_unchanged() {
        assert_eq!(
            normalize_css_import_path("../shared.scss".to_string(), false),
            "../shared.scss"
        );
    }

    #[test]
    fn normalize_absolute_path_unchanged() {
        assert_eq!(
            normalize_css_import_path("/styles/main.css".to_string(), false),
            "/styles/main.css"
        );
    }

    #[test]
    fn normalize_url_unchanged() {
        assert_eq!(
            normalize_css_import_path("https://example.com/style.css".to_string(), false),
            "https://example.com/style.css"
        );
    }

    #[test]
    fn normalize_bare_css_gets_dot_slash() {
        assert_eq!(
            normalize_css_import_path("app.css".to_string(), false),
            "./app.css"
        );
    }

    #[test]
    fn normalize_css_package_subpath_stays_bare() {
        assert_eq!(
            normalize_css_import_path("tailwindcss/theme.css".to_string(), false),
            "tailwindcss/theme.css"
        );
    }

    #[test]
    fn normalize_css_package_subpath_with_dotted_name_stays_bare() {
        assert_eq!(
            normalize_css_import_path("highlight.js/styles/github.css".to_string(), false),
            "highlight.js/styles/github.css"
        );
    }

    #[test]
    fn normalize_bare_scss_gets_dot_slash() {
        assert_eq!(
            normalize_css_import_path("vars.scss".to_string(), false),
            "./vars.scss"
        );
    }

    #[test]
    fn normalize_bare_sass_gets_dot_slash() {
        assert_eq!(
            normalize_css_import_path("main.sass".to_string(), false),
            "./main.sass"
        );
    }

    #[test]
    fn normalize_bare_less_gets_dot_slash() {
        assert_eq!(
            normalize_css_import_path("theme.less".to_string(), false),
            "./theme.less"
        );
    }

    #[test]
    fn normalize_bare_js_extension_stays_bare() {
        assert_eq!(
            normalize_css_import_path("module.js".to_string(), false),
            "module.js"
        );
    }

    #[test]
    fn normalize_scss_bare_partial_gets_dot_slash() {
        assert_eq!(
            normalize_css_import_path("variables".to_string(), true),
            "./variables"
        );
    }

    #[test]
    fn normalize_scss_bare_partial_with_subdir_gets_dot_slash() {
        assert_eq!(
            normalize_css_import_path("base/reset".to_string(), true),
            "./base/reset"
        );
    }

    #[test]
    fn normalize_scss_builtin_stays_bare() {
        assert_eq!(
            normalize_css_import_path("sass:math".to_string(), true),
            "sass:math"
        );
    }

    #[test]
    fn normalize_scss_relative_path_unchanged() {
        assert_eq!(
            normalize_css_import_path("../styles/variables".to_string(), true),
            "../styles/variables"
        );
    }

    #[test]
    fn normalize_css_bare_extensionless_stays_bare() {
        assert_eq!(
            normalize_css_import_path("tailwindcss".to_string(), false),
            "tailwindcss"
        );
    }

    #[test]
    fn normalize_scoped_package_with_css_extension_stays_bare() {
        assert_eq!(
            normalize_css_import_path("@fontsource/monaspace-neon/400.css".to_string(), false),
            "@fontsource/monaspace-neon/400.css"
        );
    }

    #[test]
    fn normalize_scoped_package_with_scss_extension_stays_bare() {
        assert_eq!(
            normalize_css_import_path("@company/design-system/tokens.scss".to_string(), true),
            "@company/design-system/tokens.scss"
        );
    }

    #[test]
    fn normalize_scoped_package_without_extension_stays_bare() {
        assert_eq!(
            normalize_css_import_path("@fallow/design-system/styles".to_string(), false),
            "@fallow/design-system/styles"
        );
    }

    #[test]
    fn normalize_scoped_package_extensionless_scss_stays_bare() {
        assert_eq!(
            normalize_css_import_path("@company/tokens".to_string(), true),
            "@company/tokens"
        );
    }

    #[test]
    fn normalize_path_alias_with_css_extension_stays_bare() {
        assert_eq!(
            normalize_css_import_path("@/components/Button.css".to_string(), false),
            "@/components/Button.css"
        );
    }

    #[test]
    fn normalize_path_alias_extensionless_stays_bare() {
        assert_eq!(
            normalize_css_import_path("@/styles/variables".to_string(), false),
            "@/styles/variables"
        );
    }

    #[test]
    fn strip_css_no_comments() {
        let source = ".foo { color: red; }";
        assert_eq!(strip_css_comments(source, false), source);
    }

    #[test]
    fn strip_css_multiple_block_comments() {
        let source = "/* comment-one */ .foo { } /* comment-two */ .bar { }";
        let result = strip_css_comments(source, false);
        assert!(!result.contains("comment-one"));
        assert!(!result.contains("comment-two"));
        assert!(result.contains(".foo"));
        assert!(result.contains(".bar"));
    }

    #[test]
    fn strip_scss_does_not_affect_non_scss() {
        let source = "// this stays\n.foo { }";
        let result = strip_css_comments(source, false);
        assert!(result.contains("// this stays"));
    }

    #[test]
    fn css_module_parses_suppressions() {
        let info = parse_css_to_module(
            fallow_types::discover::FileId(0),
            Path::new("Component.module.css"),
            "/* fallow-ignore-file */\n.btn { color: red; }",
            0,
        );
        assert!(!info.suppressions.is_empty());
        assert_eq!(info.suppressions[0].line, 0);
    }

    #[test]
    fn extracts_class_starting_with_underscore() {
        let names = export_names("._private { } .__dunder { }");
        assert!(names.contains(&"_private".to_string()));
        assert!(names.contains(&"__dunder".to_string()));
    }

    #[test]
    fn ignores_id_selectors() {
        let names = export_names("#myId { color: red; }");
        assert!(!names.contains(&"myId".to_string()));
    }

    #[test]
    fn ignores_element_selectors() {
        let names = export_names("div { color: red; } span { }");
        assert!(names.is_empty());
    }

    #[test]
    fn extract_css_imports_at_import_quoted() {
        let imports = extract_css_imports(r#"@import "./reset.css";"#, false);
        assert_eq!(imports, vec!["./reset.css"]);
    }

    #[test]
    fn extract_css_imports_package_subpath_stays_bare() {
        let imports =
            extract_css_imports(r#"@import "tailwindcss/theme.css" layer(theme);"#, false);
        assert_eq!(imports, vec!["tailwindcss/theme.css"]);
    }

    #[test]
    fn extract_css_imports_at_import_url() {
        let imports = extract_css_imports(r#"@import url("./reset.css");"#, false);
        assert_eq!(imports, vec!["./reset.css"]);
    }

    #[test]
    fn extract_css_imports_skips_remote_urls() {
        let imports =
            extract_css_imports(r#"@import "https://fonts.example.com/font.css";"#, false);
        assert!(imports.is_empty());
    }

    #[test]
    fn extract_css_imports_scss_use_normalizes_partial() {
        let imports = extract_css_imports(r#"@use "variables";"#, true);
        assert_eq!(imports, vec!["./variables"]);
    }

    #[test]
    fn extract_css_imports_scss_forward_normalizes_partial() {
        let imports = extract_css_imports(r#"@forward "tokens";"#, true);
        assert_eq!(imports, vec!["./tokens"]);
    }

    #[test]
    fn extract_css_imports_skips_comments() {
        let imports = extract_css_imports(
            r#"/* @import "./hidden.scss"; */
@use "real";"#,
            true,
        );
        assert_eq!(imports, vec!["./real"]);
    }

    #[test]
    fn extract_css_imports_at_plugin_keeps_package_bare() {
        let imports = extract_css_imports(r#"@plugin "daisyui";"#, true);
        assert_eq!(imports, vec!["daisyui"]);
    }

    #[test]
    fn extract_css_imports_at_plugin_tracks_relative_file() {
        let imports = extract_css_imports(r#"@plugin "./tailwind-plugin.js";"#, false);
        assert_eq!(imports, vec!["./tailwind-plugin.js"]);
    }

    #[test]
    fn extract_css_imports_scss_at_import_kept_relative() {
        let imports = extract_css_imports(r"@import 'Foo';", true);
        assert_eq!(imports, vec!["./Foo"]);
    }

    #[test]
    fn extract_css_imports_additional_data_string_body() {
        let body = r#"@use "./src/styles/global.scss";"#;
        let imports = extract_css_imports(body, true);
        assert_eq!(imports, vec!["./src/styles/global.scss"]);
    }

    #[test]
    fn mask_with_whitespace_preserves_byte_length() {
        let src = "/* hello */ .foo { }";
        let masked = mask_with_whitespace(src, &CSS_COMMENT_RE);
        assert_eq!(masked.len(), src.len());
        assert!(masked.is_char_boundary(src.len()));
    }

    #[test]
    fn mask_with_whitespace_preserves_offsets_around_multibyte() {
        let src = "/* \u{2713} */ .foo { }";
        let foo_offset = src.find(".foo").expect("`.foo` present");
        let masked = mask_with_whitespace(src, &CSS_COMMENT_RE);
        assert_eq!(masked.len(), src.len());
        assert_eq!(masked.find(".foo"), Some(foo_offset));
    }

    /// Resolve a span's start to (line, col) using the same primitives the
    /// downstream pipeline uses in `crates/core/src/analyze/unused_exports.rs`.
    fn span_line_col(source: &str, start: u32) -> (u32, u32) {
        let offsets = fallow_types::extract::compute_line_offsets(source);
        fallow_types::extract::byte_offset_to_line_col(&offsets, start)
    }

    #[test]
    fn span_points_at_real_class_declaration_line() {
        let source = "\n\n\n\n.foo { color: red; }\n";
        let exports = extract_css_module_exports(source, false);
        assert_eq!(exports.len(), 1);
        let span = exports[0].span;
        let (line, col) = span_line_col(source, span.start);
        assert_eq!(line, 5, "`.foo` on line 5 must produce line 5, not line 1");
        assert_eq!(
            col, 1,
            "column points at `f` in `.foo` (post-dot identifier)"
        );
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            "foo",
            "span range must slice to the class identifier in the original source"
        );
    }

    #[test]
    fn span_survives_multibyte_comment_prefix() {
        let source = "/* \u{2713} */\n.foo { }";
        let exports = extract_css_module_exports(source, false);
        assert_eq!(exports.len(), 1);
        let span = exports[0].span;
        assert!(
            source.is_char_boundary(span.start as usize),
            "span.start must lie on a UTF-8 char boundary"
        );
        assert_eq!(&source[span.start as usize..span.end as usize], "foo");
    }

    #[test]
    fn span_skips_at_layer_prelude_dot_segments() {
        let source = "@layer foo.bar { }\n.root { }\n";
        let exports = extract_css_module_exports(source, false);
        let names: Vec<_> = exports
            .iter()
            .filter_map(|e| match &e.name {
                ExportName::Named(n) => Some(n.as_str()),
                ExportName::Default => None,
            })
            .collect();
        assert_eq!(names, vec!["root"], "@layer sub-segments must not export");
        let span = exports[0].span;
        let (line, _col) = span_line_col(source, span.start);
        assert_eq!(line, 2, "`.root` lives on line 2 of the original source");
        assert_eq!(&source[span.start as usize..span.end as usize], "root");
    }

    #[test]
    fn span_skips_classes_in_strings() {
        let source = ".real { content: \".fake\"; }\n.also-real { }\n";
        let exports = extract_css_module_exports(source, false);
        let names: Vec<_> = exports
            .iter()
            .filter_map(|e| match &e.name {
                ExportName::Named(n) => Some(n.as_str()),
                ExportName::Default => None,
            })
            .collect();
        assert_eq!(names, vec!["real", "also-real"]);
        for export in &exports {
            let span = export.span;
            let slice = &source[span.start as usize..span.end as usize];
            match &export.name {
                ExportName::Named(n) => assert_eq!(slice, n.as_str()),
                ExportName::Default => unreachable!("CSS modules emit only named exports"),
            }
        }
    }

    #[test]
    fn span_deduplicates_to_first_occurrence() {
        let source = ".btn { color: red; }\n.btn { color: blue; }\n";
        let exports = extract_css_module_exports(source, false);
        assert_eq!(exports.len(), 1);
        let (line, _col) = span_line_col(source, exports[0].span.start);
        assert_eq!(
            line, 1,
            "first occurrence wins for deduplicated class names"
        );
    }

    #[test]
    fn span_inside_media_query() {
        let source =
            "@media (max-width: 768px) {\n  .mobile { display: block; }\n  .desktop { }\n}\n";
        let exports = extract_css_module_exports(source, false);
        let by_name: rustc_hash::FxHashMap<&str, oxc_span::Span> = exports
            .iter()
            .filter_map(|e| match &e.name {
                ExportName::Named(n) => Some((n.as_str(), e.span)),
                ExportName::Default => None,
            })
            .collect();
        let mobile_line = span_line_col(source, by_name["mobile"].start).0;
        let desktop_line = span_line_col(source, by_name["desktop"].start).0;
        assert_eq!(mobile_line, 2);
        assert_eq!(desktop_line, 3);
    }

    #[test]
    fn at_layer_only_module_emits_no_exports() {
        let exports = extract_css_module_exports("@layer foo.bar, foo.baz;\n", false);
        assert!(exports.is_empty());
    }

    #[test]
    fn parse_css_to_module_resolves_real_line_offsets() {
        let source = "\n\n\n\n.foo { color: red; }\n";
        let info = parse_css_to_module(
            fallow_types::discover::FileId(0),
            Path::new("Component.module.css"),
            source,
            0,
        );
        assert_eq!(info.exports.len(), 1);
        let (line, _col) = fallow_types::extract::byte_offset_to_line_col(
            &info.line_offsets,
            info.exports[0].span.start,
        );
        assert_eq!(line, 5, "downstream line must equal the source line");
    }
}
