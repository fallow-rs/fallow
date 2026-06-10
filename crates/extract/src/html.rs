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
    LazyLock::new(|| crate::static_regex(r"(?s)<!--.*?-->"));

/// Regex to extract `src` attribute from `<script>` tags.
/// Matches both `<script src="...">` and `<script type="module" src="...">`.
/// Uses `(?s)` so `.` matches newlines (multi-line attributes).
static SCRIPT_SRC_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?si)<script\b(?:[^>"']|"[^"]*"|'[^']*')*?\bsrc\s*=\s*["']([^"']+)["']"#)
});

/// Regex to extract `href` attribute from `<link>` tags with `rel="stylesheet"` or
/// `rel="modulepreload"`.
/// Handles attributes in any order (rel before or after href).
static LINK_HREF_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?si)<link\b(?:[^>"']|"[^"]*"|'[^']*')*?\brel\s*=\s*["'](stylesheet|modulepreload)["'](?:[^>"']|"[^"]*"|'[^']*')*?\bhref\s*=\s*["']([^"']+)["']"#,
    )
});

/// Regex for the reverse attribute order: href before rel.
static LINK_HREF_REVERSE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?si)<link\b(?:[^>"']|"[^"]*"|'[^']*')*?\bhref\s*=\s*["']([^"']+)["'](?:[^>"']|"[^"]*"|'[^']*')*?\brel\s*=\s*["'](stylesheet|modulepreload)["']"#,
    )
});

/// Check if a path is an HTML file.
pub(crate) fn is_html_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "html")
}

/// Returns true if an HTML asset reference is a remote URL that should be skipped.
pub(crate) fn is_remote_url(src: &str) -> bool {
    src.starts_with("http://")
        || src.starts_with("https://")
        || src.starts_with("//")
        || src.starts_with("data:")
}

/// Build-time template placeholders that aren't valid import specifiers and
/// never resolve to a real file. Skip them at extraction time so they don't
/// enter the import graph as unresolvable specifiers.
///
/// - `{{ ... }}` covers Handlebars (Ember `index.html`'s `{{rootURL}}`,
///   `{{config.assetsPath}}`), Mustache (Jekyll, Hugo), Jinja2 (Pelican /
///   11ty plugins), and pre-compiled Vue / Angular templates whose
///   interpolation has leaked into a checked-in HTML scaffold.
/// - `###...###` covers ember-cli blueprint scaffold placeholders
///   (`###APPNAME###`, `###DUMMY###`) checked in as addon-fixture templates.
///
/// Neither shape is a legal URL or path character outside template engines,
/// so the skip is generic across frameworks rather than gated on a plugin.
/// Returns `true` for any `src` / `href` value that contains either marker.
pub(crate) fn is_template_placeholder(value: &str) -> bool {
    value.contains("{{") || value.contains("###")
}

/// Extract local (non-remote) asset references from HTML-like markup.
///
/// Returns the raw `src`/`href` strings (trimmed, remote URLs filtered). Shared
/// between the HTML file parser and the JS/TS visitor's tagged template
/// literal override so `` html`<script src="...">` `` in Hono/lit-html/htm
/// layouts emits the same asset edges as a real `.html` file.
pub(crate) fn collect_asset_refs(source: &str) -> Vec<String> {
    let stripped = HTML_COMMENT_RE.replace_all(source, "");
    let mut refs: Vec<String> = Vec::new();

    for cap in SCRIPT_SRC_RE.captures_iter(&stripped) {
        if let Some(m) = cap.get(1) {
            let src = m.as_str().trim();
            if !src.is_empty() && !is_remote_url(src) && !is_template_placeholder(src) {
                refs.push(src.to_string());
            }
        }
    }

    for cap in LINK_HREF_RE.captures_iter(&stripped) {
        if let Some(m) = cap.get(2) {
            let href = m.as_str().trim();
            if !href.is_empty() && !is_remote_url(href) && !is_template_placeholder(href) {
                refs.push(href.to_string());
            }
        }
    }
    for cap in LINK_HREF_REVERSE_RE.captures_iter(&stripped) {
        if let Some(m) = cap.get(1) {
            let href = m.as_str().trim();
            if !href.is_empty() && !is_remote_url(href) && !is_template_placeholder(href) {
                refs.push(href.to_string());
            }
        }
    }

    refs
}

/// Parse an HTML file, extracting script and stylesheet references as imports.
#[cfg(test)]
pub(crate) fn parse_html_to_module(file_id: FileId, source: &str, content_hash: u64) -> ModuleInfo {
    parse_html_to_module_with_complexity(file_id, source, content_hash, false)
}

/// Parse an HTML file and optionally compute Angular template complexity.
pub(crate) fn parse_html_to_module_with_complexity(
    file_id: FileId,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    let parsed_suppressions = crate::suppress::parse_suppressions_from_source(source);

    let mut imports: Vec<ImportInfo> = collect_asset_refs(source)
        .into_iter()
        .map(|raw| ImportInfo {
            source: normalize_asset_url(&raw),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: false,
            span: Span::default(),
            source_span: Span::default(),
        })
        .collect();

    imports.sort_unstable_by(|a, b| a.source.cmp(&b.source));
    imports.dedup_by(|a, b| a.source == b.source);

    let angular::AngularTemplateRefs {
        identifiers,
        member_accesses: template_member_accesses,
        security_sinks,
    } = angular::collect_angular_template_refs(source);
    let mut member_accesses: Vec<MemberAccess> = identifiers
        .into_iter()
        .map(|name| MemberAccess {
            object: ANGULAR_TPL_SENTINEL.to_string(),
            member: name,
        })
        .collect();
    member_accesses.extend(template_member_accesses);

    let complexity = if need_complexity {
        crate::template_complexity::compute_angular_template_complexity(source)
            .into_iter()
            .collect()
    } else {
        Vec::new()
    };

    ModuleInfo {
        file_id,
        exports: Vec::new(),
        imports,
        re_exports: Vec::new(),
        dynamic_imports: Vec::new(),
        dynamic_import_patterns: Vec::new(),
        require_calls: Vec::new(),
        package_path_references: Vec::new(),
        member_accesses,
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
        complexity,
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
        security_sinks,
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn skips_handlebars_placeholder_in_script_src() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script src="{{rootURL}}assets/app.js"></script>
               <script src="{{config.assetsPath}}vendor.js"></script>"#,
            0,
        );
        assert!(
            info.imports.is_empty(),
            "Handlebars-placeholder script srcs should not enter the import graph; got {:?}",
            info.imports
        );
    }

    #[test]
    fn skips_handlebars_placeholder_in_link_href() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<link rel="stylesheet" href="{{rootURL}}assets/app.css">"#,
            0,
        );
        assert!(info.imports.is_empty());
    }

    #[test]
    fn skips_ember_cli_blueprint_placeholder() {
        let info = parse_html_to_module(
            FileId(0),
            r####"<script src="###APPNAME###/app.js"></script>"####,
            0,
        );
        assert!(info.imports.is_empty());
    }

    #[test]
    fn extracts_normal_specifier_alongside_placeholders() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script src="{{rootURL}}assets/app.js"></script>
               <script src="./src/main.ts"></script>"#,
            0,
        );
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./src/main.ts");
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
        assert_eq!(info.imports.len(), 1);
    }

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
