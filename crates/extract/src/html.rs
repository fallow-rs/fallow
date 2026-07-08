//! HTML file parsing for script, stylesheet, and Angular template references.
//!
//! Extracts `<script src="...">` and `<link rel="stylesheet" href="...">` references
//! from HTML files, creating graph edges so that referenced JS/CSS assets (and their
//! transitive imports) are reachable from the HTML entry point.
//!
//! Also scans for Angular template syntax (`{{ }}`, `[prop]`, `(event)`, `@if`, etc.)
//! and stores referenced identifiers as typed semantic facts.

use std::path::Path;
use std::sync::LazyLock;

use oxc_span::Span;

use crate::asset_url::normalize_asset_url;
use crate::sfc_template::angular;
use crate::{
    AngularTemplateMemberAccessFact, ImportInfo, ImportedName, MemberAccess, ModuleInfo,
    SemanticFact,
};
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

/// Regex matching an opening or closing custom-element tag. The HTML spec
/// requires a custom-element name to contain a hyphen, so `[a-z][a-z0-9]*-...`
/// captures `<x-foo>` / `<my-element>` while native tags (`div`, `span`) never
/// match. The capture stops before attributes / `>` / `/`.
static CUSTOM_ELEMENT_TAG_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| crate::static_regex(r"</?\s*([a-z][a-z0-9]*-[a-z0-9-]*)"));

/// Collect the custom-element tag names rendered in an `html` template snippet
/// (`<x-foo>` / `</x-foo>` -> `x-foo`). HTML comments are stripped first so a
/// commented-out `<!-- <x-foo> -->` does not credit the element. Deduped; native
/// HTML tags are excluded by the hyphen requirement. Feeds the Lit
/// `unrendered-component` arm's project-wide rendered-tag union.
pub(crate) fn collect_custom_element_tags(source: &str) -> Vec<String> {
    let stripped = HTML_COMMENT_RE.replace_all(source, "");
    let mut tags: Vec<String> = Vec::new();
    for cap in CUSTOM_ELEMENT_TAG_RE.captures_iter(&stripped) {
        if let Some(m) = cap.get(1) {
            let tag = m.as_str();
            if !tags.iter().any(|t| t == tag) {
                tags.push(tag.to_string());
            }
        }
    }
    tags
}

/// Parse an HTML file, extracting script and stylesheet references as imports.
#[cfg(test)]
pub(crate) fn parse_html_to_module(file_id: FileId, source: &str, content_hash: u64) -> ModuleInfo {
    parse_html_to_module_with_complexity(file_id, source, content_hash, false)
}

/// Computed building blocks for an HTML [`ModuleInfo`], gathered before the
/// (irreducible) struct literal is assembled.
struct HtmlModuleParts {
    imports: Vec<ImportInfo>,
    member_accesses: Vec<MemberAccess>,
    semantic_facts: Vec<SemanticFact>,
    security_sinks: Vec<fallow_types::extract::SinkSite>,
    angular_used_selectors: Vec<String>,
    has_dynamic_component_render: bool,
    complexity: Vec<fallow_types::extract::FunctionComplexity>,
}

/// Collect the asset-reference imports, Angular template member accesses /
/// security sinks / used selectors, and (optionally) template complexity for an
/// HTML source.
fn collect_html_module_parts(source: &str, need_complexity: bool) -> HtmlModuleParts {
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
    let identifiers: Vec<String> = identifiers.into_iter().collect();
    let semantic_facts: Vec<SemanticFact> = identifiers
        .iter()
        .cloned()
        .map(|member| {
            SemanticFact::AngularTemplateMemberAccess(AngularTemplateMemberAccessFact { member })
        })
        .collect();
    let member_accesses = template_member_accesses;

    // Angular external template (`templateUrl`): harvest the custom element
    // selector tags rendered here so the Angular `unrendered-component` detector
    // unions them into the project-wide used-selector set, and flag the
    // `*ngComponentOutlet` dynamic-render escape hatch (project-wide abstain).
    let angular_used_selectors = angular::collect_angular_used_selectors(source);
    let has_dynamic_component_render = source.contains("ngComponentOutlet");

    let complexity = if need_complexity {
        crate::template_complexity::compute_angular_template_complexity(source)
            .into_iter()
            .collect()
    } else {
        Vec::new()
    };

    HtmlModuleParts {
        imports,
        member_accesses,
        semantic_facts,
        security_sinks,
        angular_used_selectors,
        has_dynamic_component_render,
        complexity,
    }
}

/// Parse an HTML file and optionally compute Angular template complexity.
pub(crate) fn parse_html_to_module_with_complexity(
    file_id: FileId,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    let parsed_suppressions = crate::suppress::parse_suppressions_from_source(source);
    let parts = collect_html_module_parts(source, need_complexity);
    html_module_info(file_id, content_hash, source, parsed_suppressions, parts)
}

/// Assemble the `ModuleInfo` for an HTML file from its computed parts; all
/// JS-level fields stay empty since HTML carries no module structure. Pure
/// plumbing struct literal.
fn html_module_info(
    file_id: FileId,
    content_hash: u64,
    source: &str,
    parsed_suppressions: crate::suppress::ParsedSuppressions,
    parts: HtmlModuleParts,
) -> ModuleInfo {
    let HtmlModuleParts {
        imports,
        member_accesses,
        semantic_facts,
        security_sinks,
        angular_used_selectors,
        has_dynamic_component_render,
        complexity,
    } = parts;

    ModuleInfo {
        file_id,
        exports: Vec::new(),
        imports,
        re_exports: Vec::new(),
        dynamic_imports: Vec::new(),
        dynamic_import_patterns: Vec::new(),
        require_calls: Vec::new(),
        package_path_references: Box::default(),
        member_accesses,
        semantic_facts: semantic_facts.into(),
        whole_object_uses: Box::default(),
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
        exported_factory_returns: Box::default(),
        type_member_types: Box::default(),
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks,
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        inline_server_action_exports: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: Vec::new(),
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        angular_inputs: Vec::new(),
        angular_outputs: Vec::new(),
        angular_component_selectors: Vec::new(),
        registered_custom_elements: Vec::new(),
        // Custom-element tags rendered in a standalone `.html` document (an app
        // shell, demo, or dev page) feed the Lit `unrendered-component` arm's
        // project-wide rendered-tag union, so an element rendered only from HTML
        // (e.g. a root `<my-app>` in `index.html`) is not falsely flagged.
        used_custom_element_tags: collect_custom_element_tags(source),
        angular_used_selectors,
        angular_entry_component_refs: Vec::new(),
        has_dynamic_component_render,
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
        component_functions: Vec::new(),
        react_props: Vec::new(),
        hook_uses: Vec::new(),
        render_edges: Vec::new(),
        svelte_dispatched_events: Vec::new(),
        svelte_listened_events: Vec::new(),
        has_dynamic_dispatch: false,
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
        let fact_names: rustc_hash::FxHashSet<&str> = info
            .semantic_facts
            .iter()
            .filter_map(|fact| {
                if let SemanticFact::AngularTemplateMemberAccess(access) = fact {
                    Some(access.member.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(fact_names.contains("title"), "should contain 'title'");
        assert!(
            fact_names.contains("isHighlighted"),
            "should contain 'isHighlighted'"
        );
        assert!(fact_names.contains("greeting"), "should contain 'greeting'");
        assert!(
            fact_names.contains("onButtonClick"),
            "should contain 'onButtonClick'"
        );
        assert!(
            info.member_accesses.is_empty(),
            "Angular template refs should emit typed facts instead of member accesses: {:?}",
            info.member_accesses
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
