//! HTML file parsing for script, stylesheet, and Angular template references.
//!
//! Extracts `<script src="...">` and `<link rel="stylesheet" href="...">` references
//! from HTML files, creating graph edges so that referenced JS/CSS assets (and their
//! transitive imports) are reachable from the HTML entry point.
//!
//! Also scans for Angular template syntax (`{{ }}`, `[prop]`, `(event)`, `@if`, etc.)
//! and stores referenced identifiers as `MemberAccess` entries with a sentinel object,
//! enabling the analysis phase to credit component class members used in external templates.

use std::path::Path;
use std::sync::LazyLock;

use oxc_span::Span;

use crate::asset_url::normalize_asset_url;
use crate::sfc_template::angular::{self, ANGULAR_TPL_SENTINEL};
use crate::{ImportInfo, ImportedName, MemberAccess, ModuleInfo};
use fallow_types::discover::FileId;

/// Regex to match HTML comments (`<!-- ... -->`) for stripping before extraction.
static HTML_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)<!--.*?-->").expect("valid regex"));

/// Regex to extract `src` attribute from `<script>` tags.
/// Matches both `<script src="...">` and `<script type="module" src="...">`.
/// Uses `(?s)` so `.` matches newlines (multi-line attributes).
static SCRIPT_SRC_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?si)<script\b(?:[^>"']|"[^"]*"|'[^']*')*?\bsrc\s*=\s*["']([^"']+)["']"#)
        .expect("valid regex")
});

/// Regex to extract `href` attribute from `<link>` tags with `rel="stylesheet"` or
/// `rel="modulepreload"`.
/// Handles attributes in any order (rel before or after href).
static LINK_HREF_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r#"(?si)<link\b(?:[^>"']|"[^"]*"|'[^']*')*?\brel\s*=\s*["'](stylesheet|modulepreload)["'](?:[^>"']|"[^"]*"|'[^']*')*?\bhref\s*=\s*["']([^"']+)["']"#,
    )
    .expect("valid regex")
});

/// Regex for the reverse attribute order: href before rel.
static LINK_HREF_REVERSE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r#"(?si)<link\b(?:[^>"']|"[^"]*"|'[^']*')*?\bhref\s*=\s*["']([^"']+)["'](?:[^>"']|"[^"]*"|'[^']*')*?\brel\s*=\s*["'](stylesheet|modulepreload)["']"#,
    )
    .expect("valid regex")
});

/// Check if a path is an HTML file.
// Keep in sync with fallow_core::analyze::predicates::is_html_file (crate boundary prevents sharing)
pub(crate) fn is_html_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "html")
}

/// Returns true if an HTML asset reference is a remote URL that should be skipped.
fn is_remote_url(src: &str) -> bool {
    src.starts_with("http://")
        || src.starts_with("https://")
        || src.starts_with("//")
        || src.starts_with("data:")
}

/// Parse an HTML file, extracting script and stylesheet references as imports.
pub(crate) fn parse_html_to_module(file_id: FileId, source: &str, content_hash: u64) -> ModuleInfo {
    let suppressions = crate::suppress::parse_suppressions_from_source(source);

    // Strip HTML comments before matching to avoid false positives.
    let stripped = HTML_COMMENT_RE.replace_all(source, "");

    let mut imports = Vec::new();

    // Extract <script src="..."> references.
    // Bare filenames (e.g., `src="app.js"`) are normalized to `./app.js` so
    // the resolver doesn't misclassify them as npm packages.
    for cap in SCRIPT_SRC_RE.captures_iter(&stripped) {
        if let Some(m) = cap.get(1) {
            let src = m.as_str().trim();
            if !src.is_empty() && !is_remote_url(src) {
                imports.push(ImportInfo {
                    source: normalize_asset_url(src),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: false,
                    span: Span::default(),
                    source_span: Span::default(),
                });
            }
        }
    }

    // Extract <link rel="stylesheet" href="..."> and <link rel="modulepreload" href="...">.
    // Handle both attribute orders: rel before href, and href before rel.
    // Bare filenames are normalized identically to `<script src>`.
    for cap in LINK_HREF_RE.captures_iter(&stripped) {
        if let Some(m) = cap.get(2) {
            let href = m.as_str().trim();
            if !href.is_empty() && !is_remote_url(href) {
                imports.push(ImportInfo {
                    source: normalize_asset_url(href),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: false,
                    span: Span::default(),
                    source_span: Span::default(),
                });
            }
        }
    }
    for cap in LINK_HREF_REVERSE_RE.captures_iter(&stripped) {
        if let Some(m) = cap.get(1) {
            let href = m.as_str().trim();
            if !href.is_empty() && !is_remote_url(href) {
                imports.push(ImportInfo {
                    source: normalize_asset_url(href),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: false,
                    span: Span::default(),
                    source_span: Span::default(),
                });
            }
        }
    }

    // Deduplicate: the same asset may be referenced by both <script src> and
    // <link rel="modulepreload" href> for the same path.
    imports.sort_unstable_by(|a, b| a.source.cmp(&b.source));
    imports.dedup_by(|a, b| a.source == b.source);

    // Scan for Angular template syntax ({{ }}, [prop], (event), @if, etc.).
    // Referenced identifiers are stored as MemberAccess entries with a sentinel
    // object name so the analysis phase can bridge them to the component class.
    let template_refs = angular::collect_angular_template_refs(source);
    let member_accesses: Vec<MemberAccess> = template_refs
        .into_iter()
        .map(|name| MemberAccess {
            object: ANGULAR_TPL_SENTINEL.to_string(),
            member: name,
        })
        .collect();

    ModuleInfo {
        file_id,
        exports: Vec::new(),
        imports,
        re_exports: Vec::new(),
        dynamic_imports: Vec::new(),
        dynamic_import_patterns: Vec::new(),
        require_calls: Vec::new(),
        member_accesses,
        whole_object_uses: Vec::new(),
        has_cjs_exports: false,
        content_hash,
        suppressions,
        unused_import_bindings: Vec::new(),
        line_offsets: fallow_types::extract::compute_line_offsets(source),
        complexity: Vec::new(),
        flag_uses: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_html_file ─────────────────────────────────────────────

    #[test]
    fn is_html_file_html() {
        assert!(is_html_file(Path::new("index.html")));
    }

    #[test]
    fn is_html_file_nested() {
        assert!(is_html_file(Path::new("pages/about.html")));
    }

    #[test]
    fn is_html_file_rejects_htm() {
        assert!(!is_html_file(Path::new("index.htm")));
    }

    #[test]
    fn is_html_file_rejects_js() {
        assert!(!is_html_file(Path::new("app.js")));
    }

    #[test]
    fn is_html_file_rejects_ts() {
        assert!(!is_html_file(Path::new("app.ts")));
    }

    #[test]
    fn is_html_file_rejects_vue() {
        assert!(!is_html_file(Path::new("App.vue")));
    }

    // ── is_remote_url ────────────────────────────────────────────

    #[test]
    fn remote_url_http() {
        assert!(is_remote_url("http://example.com/script.js"));
    }

    #[test]
    fn remote_url_https() {
        assert!(is_remote_url("https://cdn.example.com/style.css"));
    }

    #[test]
    fn remote_url_protocol_relative() {
        assert!(is_remote_url("//cdn.example.com/lib.js"));
    }

    #[test]
    fn remote_url_data() {
        assert!(is_remote_url("data:text/javascript;base64,abc"));
    }

    #[test]
    fn local_relative_not_remote() {
        assert!(!is_remote_url("./src/entry.js"));
    }

    #[test]
    fn local_root_relative_not_remote() {
        assert!(!is_remote_url("/src/entry.js"));
    }

    // ── parse_html_to_module: script src extraction ──────────────

    #[test]
    fn extracts_module_script_src() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script type="module" src="./src/entry.js"></script>"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./src/entry.js");
    }

    #[test]
    fn extracts_plain_script_src() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script src="./src/polyfills.js"></script>"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./src/polyfills.js");
    }

    #[test]
    fn extracts_multiple_scripts() {
        let info = parse_html_to_module(
            FileId(0),
            r#"
            <script type="module" src="./src/entry.js"></script>
            <script src="./src/polyfills.js"></script>
            "#,
            0,
        );
        assert_eq!(info.imports.len(), 2);
    }

    #[test]
    fn skips_inline_script() {
        let info = parse_html_to_module(FileId(0), r#"<script>console.log("hello");</script>"#, 0);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn skips_remote_script() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script src="https://cdn.example.com/lib.js"></script>"#,
            0,
        );
        assert!(info.imports.is_empty());
    }

    #[test]
    fn skips_protocol_relative_script() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script src="//cdn.example.com/lib.js"></script>"#,
            0,
        );
        assert!(info.imports.is_empty());
    }

    // ── parse_html_to_module: link href extraction ───────────────

    #[test]
    fn extracts_stylesheet_link() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<link rel="stylesheet" href="./src/global.css" />"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./src/global.css");
    }

    #[test]
    fn extracts_modulepreload_link() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<link rel="modulepreload" href="./src/vendor.js" />"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./src/vendor.js");
    }

    #[test]
    fn extracts_link_with_reversed_attrs() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<link href="./src/global.css" rel="stylesheet" />"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./src/global.css");
    }

    // ── Bare asset references normalized to relative paths ──────
    // Regression tests for the same class of bug as #99 (Angular templateUrl).
    // Browsers resolve `src="app.js"` and `href="styles.css"` relative to the
    // HTML file, so emitting these as bare specifiers would misclassify them
    // as unlisted npm packages.

    #[test]
    fn bare_script_src_normalized_to_relative() {
        let info = parse_html_to_module(FileId(0), r#"<script src="app.js"></script>"#, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./app.js");
    }

    #[test]
    fn bare_module_script_src_normalized_to_relative() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script type="module" src="main.ts"></script>"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./main.ts");
    }

    #[test]
    fn bare_stylesheet_link_href_normalized_to_relative() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<link rel="stylesheet" href="styles.css" />"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./styles.css");
    }

    #[test]
    fn bare_link_href_reversed_attrs_normalized_to_relative() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<link href="styles.css" rel="stylesheet" />"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./styles.css");
    }

    #[test]
    fn bare_modulepreload_link_href_normalized_to_relative() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<link rel="modulepreload" href="vendor.js" />"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./vendor.js");
    }

    #[test]
    fn bare_asset_with_subdir_normalized_to_relative() {
        let info = parse_html_to_module(FileId(0), r#"<script src="assets/app.js"></script>"#, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./assets/app.js");
    }

    #[test]
    fn root_absolute_script_src_unchanged() {
        // `/src/main.ts` is a web convention (Vite root-relative) and must
        // stay absolute so the resolver's HTML special case applies.
        let info = parse_html_to_module(FileId(0), r#"<script src="/src/main.ts"></script>"#, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "/src/main.ts");
    }

    #[test]
    fn parent_relative_script_src_unchanged() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script src="../shared/vendor.js"></script>"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "../shared/vendor.js");
    }

    #[test]
    fn skips_preload_link() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<link rel="preload" href="./src/font.woff2" as="font" />"#,
            0,
        );
        assert!(info.imports.is_empty());
    }

    #[test]
    fn skips_icon_link() {
        let info =
            parse_html_to_module(FileId(0), r#"<link rel="icon" href="./favicon.ico" />"#, 0);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn skips_remote_stylesheet() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<link rel="stylesheet" href="https://fonts.googleapis.com/css" />"#,
            0,
        );
        assert!(info.imports.is_empty());
    }

    // ── HTML comment stripping ───────────────────────────────────

    #[test]
    fn skips_commented_out_script() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<!-- <script src="./old.js"></script> -->
            <script src="./new.js"></script>"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./new.js");
    }

    #[test]
    fn skips_commented_out_link() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<!-- <link rel="stylesheet" href="./old.css" /> -->
            <link rel="stylesheet" href="./new.css" />"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./new.css");
    }

    // ── Multi-line attributes ────────────────────────────────────

    #[test]
    fn handles_multiline_script_tag() {
        let info = parse_html_to_module(
            FileId(0),
            "<script\n  type=\"module\"\n  src=\"./src/entry.js\"\n></script>",
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./src/entry.js");
    }

    #[test]
    fn handles_multiline_link_tag() {
        let info = parse_html_to_module(
            FileId(0),
            "<link\n  rel=\"stylesheet\"\n  href=\"./src/global.css\"\n/>",
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./src/global.css");
    }

    // ── Full HTML document ───────────────────────────────────────

    #[test]
    fn full_vite_html() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<!doctype html>
<html>
  <head>
    <link rel="stylesheet" href="./src/global.css" />
    <link rel="icon" href="/favicon.ico" />
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="./src/entry.js"></script>
  </body>
</html>"#,
            0,
        );
        assert_eq!(info.imports.len(), 2);
        let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"./src/global.css"));
        assert!(sources.contains(&"./src/entry.js"));
    }

    // ── Edge cases ───────────────────────────────────────────────

    #[test]
    fn empty_html() {
        let info = parse_html_to_module(FileId(0), "", 0);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn html_with_no_assets() {
        let info = parse_html_to_module(
            FileId(0),
            r"<!doctype html><html><body><h1>Hello</h1></body></html>",
            0,
        );
        assert!(info.imports.is_empty());
    }

    #[test]
    fn single_quoted_attributes() {
        let info = parse_html_to_module(FileId(0), r"<script src='./src/entry.js'></script>", 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./src/entry.js");
    }

    #[test]
    fn all_imports_are_side_effect() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script src="./entry.js"></script>
            <link rel="stylesheet" href="./style.css" />"#,
            0,
        );
        for imp in &info.imports {
            assert!(matches!(imp.imported_name, ImportedName::SideEffect));
            assert!(imp.local_name.is_empty());
            assert!(!imp.is_type_only);
        }
    }

    #[test]
    fn suppression_comments_extracted() {
        let info = parse_html_to_module(
            FileId(0),
            "<!-- fallow-ignore-file -->\n<script src=\"./entry.js\"></script>",
            0,
        );
        // HTML comments use <!-- --> not //, so suppression parsing
        // from source text won't find standard JS-style comments.
        // This is expected — HTML suppression is not supported.
        assert_eq!(info.imports.len(), 1);
    }

    // ── Angular template scanning ──────────────────────────────

    #[test]
    fn angular_template_extracts_member_refs() {
        let info = parse_html_to_module(
            FileId(0),
            "<h1>{{ title() }}</h1>\n\
             <p [class.highlighted]=\"isHighlighted\">{{ greeting() }}</p>\n\
             <button (click)=\"onButtonClick()\">Toggle</button>",
            0,
        );
        let names: rustc_hash::FxHashSet<&str> = info
            .member_accesses
            .iter()
            .filter(|a| a.object == ANGULAR_TPL_SENTINEL)
            .map(|a| a.member.as_str())
            .collect();
        assert!(names.contains("title"), "should contain 'title'");
        assert!(
            names.contains("isHighlighted"),
            "should contain 'isHighlighted'"
        );
        assert!(names.contains("greeting"), "should contain 'greeting'");
        assert!(
            names.contains("onButtonClick"),
            "should contain 'onButtonClick'"
        );
    }

    #[test]
    fn plain_html_no_angular_refs() {
        let info = parse_html_to_module(
            FileId(0),
            "<!doctype html><html><body><h1>Hello</h1></body></html>",
            0,
        );
        assert!(info.member_accesses.is_empty());
    }
}
