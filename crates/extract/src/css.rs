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
    regex::Regex::new(r#"@import\s+(?:url\(\s*(?:["']([^"']+)["']|([^)]+))\s*\)|["']([^"']+)["'])"#)
        .expect("valid regex")
});

/// Regex to extract SCSS @use and @forward sources.
/// Matches: @use "path"; @use 'path'; @forward "path"; @forward 'path';
static SCSS_USE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"@(?:use|forward)\s+["']([^"']+)["']"#).expect("valid regex")
});

/// Regex to extract Tailwind CSS @plugin sources.
/// Matches: @plugin "package"; @plugin 'package'; @plugin "./local-plugin.js";
static CSS_PLUGIN_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"@plugin\s+["']([^"']+)["']"#).expect("valid regex"));

/// Regex to extract @apply class references.
/// Matches: @apply class1 class2 class3;
static CSS_APPLY_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"@apply\s+[^;}\n]+").expect("valid regex"));

/// Regex to extract @tailwind directives.
/// Matches: @tailwind base; @tailwind components; @tailwind utilities;
static CSS_TAILWIND_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"@tailwind\s+\w+").expect("valid regex"));

/// Regex to match CSS block comments (`/* ... */`) for stripping before extraction.
static CSS_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)/\*.*?\*/").expect("valid regex"));

/// Regex to match SCSS single-line comments (`// ...`) for stripping before extraction.
static SCSS_LINE_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"//[^\n]*").expect("valid regex"));

/// Regex to extract CSS class names from selectors.
/// Matches `.className` in selectors. Applied after stripping comments, strings, and URLs.
static CSS_CLASS_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\.([a-zA-Z_][a-zA-Z0-9_-]*)").expect("valid regex"));

/// Regex to strip quoted strings and `url(...)` content from CSS before class extraction.
/// Prevents false positives from `content: ".foo"` and `url(./path/file.ext)`.
static CSS_NON_SELECTOR_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?s)"[^"]*"|'[^']*'|url\([^)]*\)"#).expect("valid regex")
});

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
    // Scoped npm packages (`@scope/...`) are always bare specifiers resolved
    // from node_modules, regardless of file extension.
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
    // Bare filenames with CSS/SCSS extensions are relative file imports.
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str());
    match ext {
        Some(e) if is_style_extension(e) => format!("./{path}"),
        _ => {
            // In SCSS, extensionless bare specifiers like `@use 'variables'` are
            // local partials, not npm packages. SCSS built-in modules (`sass:math`,
            // `sass:color`) use a colon prefix and should stay bare.
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
fn strip_css_comments(source: &str, is_scss: bool) -> String {
    let stripped = CSS_COMMENT_RE.replace_all(source, "");
    if is_scss {
        SCSS_LINE_COMMENT_RE.replace_all(&stripped, "").into_owned()
    } else {
        stripped.into_owned()
    }
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
    let stripped = strip_css_comments(source, is_scss);
    let mut out = Vec::new();

    for cap in CSS_IMPORT_RE.captures_iter(&stripped) {
        let raw = cap
            .get(1)
            .or_else(|| cap.get(2))
            .or_else(|| cap.get(3))
            .map(|m| m.as_str().trim().to_string());
        if let Some(src) = raw
            && !src.is_empty()
            && !is_css_url_import(&src)
        {
            out.push(CssImportSource {
                normalized: normalize_css_import_path(src.clone(), is_scss),
                raw: src,
                is_plugin: false,
            });
        }
    }

    if is_scss {
        for cap in SCSS_USE_RE.captures_iter(&stripped) {
            if let Some(m) = cap.get(1) {
                let raw = m.as_str().to_string();
                out.push(CssImportSource {
                    normalized: normalize_css_import_path(raw.clone(), true),
                    raw,
                    is_plugin: false,
                });
            }
        }
    }

    for cap in CSS_PLUGIN_RE.captures_iter(&stripped) {
        if let Some(m) = cap.get(1) {
            let raw = m.as_str().trim().to_string();
            if !raw.is_empty() && !is_css_url_import(&raw) {
                out.push(CssImportSource {
                    normalized: normalize_css_plugin_path(raw.clone()),
                    raw,
                    is_plugin: true,
                });
            }
        }
    }

    out
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

/// Extract class names from a CSS module file as named exports.
pub fn extract_css_module_exports(source: &str) -> Vec<ExportInfo> {
    let cleaned = CSS_NON_SELECTOR_RE.replace_all(source, "");
    let mut seen = rustc_hash::FxHashSet::default();
    let mut exports = Vec::new();
    for cap in CSS_CLASS_RE.captures_iter(&cleaned) {
        if let Some(m) = cap.get(1) {
            let class_name = m.as_str().to_string();
            if seen.insert(class_name.clone()) {
                exports.push(ExportInfo {
                    name: ExportName::Named(class_name),
                    local_name: None,
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: Span::default(),
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
    let suppressions = crate::suppress::parse_suppressions_from_source(source);
    let is_scss = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "scss");

    // Strip comments before matching to avoid false positives from commented-out code.
    let stripped = strip_css_comments(source, is_scss);

    let mut imports = Vec::new();

    // Extract @import statements
    for cap in CSS_IMPORT_RE.captures_iter(&stripped) {
        let source_path = cap
            .get(1)
            .or_else(|| cap.get(2))
            .or_else(|| cap.get(3))
            .map(|m| m.as_str().trim().to_string());
        if let Some(src) = source_path
            && !src.is_empty()
            && !is_css_url_import(&src)
        {
            // CSS/SCSS @import resolves relative paths without ./ prefix,
            // so normalize to ./ to avoid bare-specifier misclassification
            let src = normalize_css_import_path(src, is_scss);
            imports.push(ImportInfo {
                source: src,
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: Span::default(),
                source_span: Span::default(),
            });
        }
    }

    // Extract SCSS @use/@forward statements
    if is_scss {
        for cap in SCSS_USE_RE.captures_iter(&stripped) {
            if let Some(m) = cap.get(1) {
                imports.push(ImportInfo {
                    source: normalize_css_import_path(m.as_str().to_string(), true),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::default(),
                    source_span: Span::default(),
                });
            }
        }
    }

    // Extract Tailwind CSS @plugin directives. These can reference npm packages
    // (e.g. `@plugin "daisyui"`) or explicit local plugin files.
    for cap in CSS_PLUGIN_RE.captures_iter(&stripped) {
        if let Some(m) = cap.get(1) {
            let source = m.as_str().trim().to_string();
            if !source.is_empty() && !is_css_url_import(&source) {
                imports.push(ImportInfo {
                    source: normalize_css_plugin_path(source),
                    imported_name: ImportedName::Default,
                    local_name: String::new(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::default(),
                    source_span: Span::default(),
                });
            }
        }
    }

    // If @apply or @tailwind directives exist, create a synthetic import to tailwindcss
    // to mark the dependency as used
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

    // For CSS module files, extract class names as named exports
    let exports = if is_css_module_file(path) {
        extract_css_module_exports(&stripped)
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
        member_accesses: Vec::new(),
        whole_object_uses: Vec::new(),
        has_cjs_exports: false,
        content_hash,
        suppressions,
        unused_import_bindings: Vec::new(),
        type_referenced_import_bindings: Vec::new(),
        value_referenced_import_bindings: Vec::new(),
        line_offsets: fallow_types::extract::compute_line_offsets(source),
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to collect export names as strings from `extract_css_module_exports`.
    fn export_names(source: &str) -> Vec<String> {
        extract_css_module_exports(source)
            .into_iter()
            .filter_map(|e| match e.name {
                ExportName::Named(n) => Some(n),
                ExportName::Default => None,
            })
            .collect()
    }

    // ── is_css_file ──────────────────────────────────────────────

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

    // ── is_css_module_file ───────────────────────────────────────

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

    // ── extract_css_module_exports: basic class extraction ───────

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

    // ── Pseudo-selectors ─────────────────────────────────────────

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
        // "btn" should be deduplicated
        assert_eq!(names, vec!["btn"]);
    }

    // ── Media queries ────────────────────────────────────────────

    #[test]
    fn classes_inside_media_query() {
        let names = export_names(
            "@media (max-width: 768px) { .mobile-nav { display: block; } .desktop-nav { display: none; } }",
        );
        assert!(names.contains(&"mobile-nav".to_string()));
        assert!(names.contains(&"desktop-nav".to_string()));
    }

    // ── Deduplication ────────────────────────────────────────────

    #[test]
    fn deduplicates_repeated_class() {
        let names = export_names(".btn { color: red; } .btn { font-size: 14px; }");
        assert_eq!(names.iter().filter(|n| *n == "btn").count(), 1);
    }

    // ── Edge cases ───────────────────────────────────────────────

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
        // Note: extract_css_module_exports itself does NOT strip comments;
        // comments are stripped in parse_css_to_module before calling it.
        // But CSS_NON_SELECTOR_RE strips quoted strings. Testing the
        // strip_css_comments + extract pipeline via the stripped source:
        let stripped = strip_css_comments("/* .fake { } */ .real { }", false);
        let names = export_names(&stripped);
        assert!(!names.contains(&"fake".to_string()));
        assert!(names.contains(&"real".to_string()));
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
        // "png" from "hero.png" should not be extracted
        assert!(!names.contains(&"png".to_string()));
    }

    // ── strip_css_comments ───────────────────────────────────────

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

    // ── is_css_url_import ────────────────────────────────────────

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

    // ── normalize_css_import_path ─────────────────────────────────

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

    // ── SCSS partial normalization ───────────────────────────────

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
        // In CSS context (not SCSS), extensionless imports are npm packages
        assert_eq!(
            normalize_css_import_path("tailwindcss".to_string(), false),
            "tailwindcss"
        );
    }

    // ── Scoped npm packages stay bare ───────────────────────────

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
        // Path aliases like `@/components/Button.css` (configured via tsconfig paths
        // or Vite alias) share the `@` prefix with scoped packages. They must stay
        // bare so the resolver's path-alias path can handle them; prepending `./`
        // would break resolution.
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

    // ── strip_css_comments edge cases ─────────────────────────────

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
        // When is_scss=false, line comments should NOT be stripped
        let source = "// this stays\n.foo { }";
        let result = strip_css_comments(source, false);
        assert!(result.contains("// this stays"));
    }

    // ── parse_css_to_module: suppression integration ──────────────

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

    // ── CSS class name edge cases ─────────────────────────────────

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

    // ── extract_css_imports (issue #195: vite additionalData / SFC styles) ──

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
        // Bare specifier in SCSS context is normalized to ./
        assert_eq!(imports, vec!["./Foo"]);
    }

    #[test]
    fn extract_css_imports_additional_data_string_body() {
        // Mimics what Vite's css.preprocessorOptions.scss.additionalData ships
        let body = r#"@use "./src/styles/global.scss";"#;
        let imports = extract_css_imports(body, true);
        assert_eq!(imports, vec!["./src/styles/global.scss"]);
    }
}
