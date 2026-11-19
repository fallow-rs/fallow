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

use oxc_allocator::Allocator;
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};

use crate::asset_url::normalize_asset_url;
use crate::sfc_template::angular;
use crate::source_map::{ExtractionResult, ScriptSection};
use crate::suppress_literals::program_literal_spans;
use crate::visitor::ModuleInfoExtractor;
use crate::{
    AngularTemplateMemberAccessFact, DynamicImportInfo, DynamicImportPattern, ImportInfo,
    ImportedName, MemberAccess, ModuleInfo, SemanticFact,
};
use fallow_types::discover::FileId;

/// Regex to match HTML comments (`<!-- ... -->`) for stripping before extraction.
static HTML_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)<!--.*?-->"));

/// Regex to match a complete `<script>` element: the opening tag (group 1), the
/// body, and the closing tag (group 2). The body is raw text, so markup inside
/// it (for example a JS string that builds `<link>` or `<script src>` tags) is
/// not part of the document. The browser ends the body at the first
/// `</script`, so the lazy `.*?` matches the same boundary.
static SCRIPT_ELEMENT_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?si)(<script\b(?:[^>"']|"[^"]*"|'[^']*')*>).*?(</script\s*>)"#)
});

/// A whole `<script>` element, with its attributes and its body.
static SCRIPT_PARTS_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?si)<script\b((?:[^>"']|"[^"]*"|'[^']*')*)>(.*?)</script\s*>"#)
});

/// A `type` attribute whose value is data, not script: JSON, an import map,
/// or a template.
static DATA_TYPE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?i)\btype\s*=\s*["']?[^"'\s>]*(json|importmap|template|html|text/plain|shader)"#,
    )
});

/// A `src` attribute.
static SRC_ATTRIBUTE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?i)\bsrc\s*="));

/// `true` when the page has an inline script with a body. A module script can
/// import any module, and a classic script can call `import()`. Extraction
/// records the imports of such a script, but not the calls of the imported
/// functions.
fn has_inline_script(source: &str) -> bool {
    let without_comments = HTML_COMMENT_RE.replace_all(source, "");
    SCRIPT_PARTS_RE
        .captures_iter(&without_comments)
        .any(|parts| {
            let attributes = parts.get(1).map_or("", |m| m.as_str());
            let body = parts.get(2).map_or("", |m| m.as_str());
            !DATA_TYPE_RE.is_match(attributes)
                && !SRC_ATTRIBUTE_RE.is_match(attributes)
                && !body.trim().is_empty()
        })
}

/// The value of a `type` attribute, in double quotes, single quotes or bare.
static TYPE_ATTRIBUTE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?i)(?:^|\s)type\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#)
});

/// `type` values of a classic script. The browser runs the body as JavaScript.
const CLASSIC_SCRIPT_TYPES: &[&str] = &[
    "",
    "text/javascript",
    "application/javascript",
    "text/ecmascript",
    "application/ecmascript",
];

/// The `type` value of a module script.
const MODULE_SCRIPT_TYPE: &str = "module";

/// The parse mode of an inline script body. A script with a `src` attribute
/// does not run its body, and a script of another type holds data.
fn inline_script_source_type(attributes: &str) -> Option<SourceType> {
    if SRC_ATTRIBUTE_RE.is_match(attributes) {
        return None;
    }
    let kind = TYPE_ATTRIBUTE_RE
        .captures(attributes)
        .and_then(|cap| cap.get(1).or_else(|| cap.get(2)).or_else(|| cap.get(3)))
        .map_or("", |value| value.as_str().trim());
    if kind.eq_ignore_ascii_case(MODULE_SCRIPT_TYPE) {
        return Some(SourceType::mjs());
    }
    CLASSIC_SCRIPT_TYPES
        .iter()
        .any(|classic| kind.eq_ignore_ascii_case(classic))
        .then(SourceType::script)
}

/// The inline `<script>` blocks of a page that the browser runs, with the
/// parse mode of each body. A `<script>` tag inside an HTML comment does not
/// run, and a body with only whitespace has nothing to parse.
pub fn page_script_sections(source: &str) -> Vec<ScriptSection> {
    let mut scanned = source.to_owned();
    for comment in HTML_COMMENT_RE.find_iter(source) {
        blank(&mut scanned, comment.range());
    }
    SCRIPT_PARTS_RE
        .captures_iter(&scanned)
        .filter_map(|parts| {
            let (attributes, body) = (parts.get(1)?, parts.get(2)?);
            let source_type = inline_script_source_type(attributes.as_str())?;
            let body_text = &source[body.range()];
            if body_text.trim().is_empty() {
                return None;
            }
            Some(ScriptSection {
                extraction: ExtractionResult::contiguous(body_text, body.start()),
                source_type,
            })
        })
        .collect()
}

/// The imports of the inline scripts of a page, with spans in the page, the
/// uses of the imported bindings (`m.default` in
/// `import("./a.js").then((m) => m.default)`), and the spans of the string
/// and template literals when the caller asks for them.
#[derive(Default)]
struct InlineScriptFacts {
    imports: Vec<ImportInfo>,
    dynamic_imports: Vec<DynamicImportInfo>,
    dynamic_import_patterns: Vec<DynamicImportPattern>,
    member_accesses: Vec<MemberAccess>,
    whole_object_uses: Vec<String>,
    semantic_facts: Vec<SemanticFact>,
    literal_spans: Vec<(u32, u32)>,
}

/// Parse each inline script body of the page. A module script can import
/// modules, and a classic or module script can call `import()`. The browser
/// loads these modules, so they are part of the graph of the page.
///
/// With `collect_literal_spans`, the same parse also gives the spans of the
/// string and template literals, so a suppression marker inside a literal
/// has no effect.
fn collect_inline_script_facts(source: &str, collect_literal_spans: bool) -> InlineScriptFacts {
    let mut found = InlineScriptFacts::default();
    for section in page_script_sections(source) {
        let body_text = section.extraction.body.as_str();
        // The whole-file nesting scan reads the page as script, so quotes in
        // the HTML text can hide the brackets of a script body. Each body
        // therefore gets its own bounded parse. A too-deep body adds no facts
        // and no literal spans, so its markers keep their effect.
        let Some((info, literal_spans)) = crate::parse_bounded(body_text, || {
            let allocator = Allocator::default();
            let parsed = Parser::new(&allocator, body_text, section.source_type).parse();
            let literal_spans = if collect_literal_spans {
                program_literal_spans(&parsed.program, &section.extraction)
            } else {
                Vec::new()
            };
            let mut extractor = ModuleInfoExtractor::new();
            extractor.visit_program(&parsed.program);
            extractor.remap_spans_with(|span| section.extraction.remap_span(span));
            let info = extractor.into_module_info(
                FileId(0),
                0,
                crate::suppress::ParsedSuppressions::default(),
            );
            (info, literal_spans)
        }) else {
            continue;
        };
        found.literal_spans.extend(literal_spans);
        // A classic script with an `import` declaration is a syntax error in
        // the browser, so only a module script gives static imports.
        // A server template can put a placeholder such as `{{ STATIC_URL }}`
        // in a specifier. The `src` and `href` values skip these, so the
        // script specifiers skip them too.
        if section.source_type.is_module() {
            found.imports.extend(
                info.imports
                    .into_iter()
                    .filter(|import| !is_template_placeholder(&import.source)),
            );
        }
        found.dynamic_imports.extend(
            info.dynamic_imports
                .into_iter()
                .filter(|import| !is_template_placeholder(&import.source)),
        );
        found.dynamic_import_patterns.extend(
            info.dynamic_import_patterns
                .into_iter()
                .filter(|pattern| !is_template_placeholder(&pattern.prefix)),
        );
        found
            .member_accesses
            .extend(info.member_accesses.iter().cloned());
        found
            .whole_object_uses
            .extend(info.whole_object_uses.iter().cloned());
        found
            .semantic_facts
            .extend(info.semantic_facts.iter().cloned());
    }
    found
}

/// The inline `<script>` blocks of an Astro component that hold script, not
/// data. Astro bundles each such block as a module.
pub fn inline_script_sections(source: &str) -> Vec<ScriptSection> {
    SCRIPT_PARTS_RE
        .captures_iter(source)
        .filter_map(|parts| {
            let attributes = parts.get(1).map_or("", |m| m.as_str());
            let body = parts.get(2)?;
            let is_script = !DATA_TYPE_RE.is_match(attributes)
                && !SRC_ATTRIBUTE_RE.is_match(attributes)
                && !body.as_str().trim().is_empty();
            is_script.then(|| ScriptSection {
                extraction: ExtractionResult::contiguous(body.as_str(), body.start()),
                source_type: SourceType::mjs(),
            })
        })
        .collect()
}

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
pub fn is_html_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "html")
}

/// Returns true if an HTML asset reference is a remote URL that should be skipped.
pub fn is_remote_url(src: &str) -> bool {
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
/// - `${...}` covers JS template-literal interpolation and JSP / EL
///   expressions (`${contextPath}/app.js`) in server-rendered templates.
///
/// None of these shapes is a legal URL or path outside template engines, so
/// the skip is generic across frameworks rather than gated on a plugin.
/// Returns `true` for any `src` / `href` value that contains one of the markers.
pub fn is_template_placeholder(value: &str) -> bool {
    value.contains("{{") || value.contains("###") || value.contains("${")
}

/// Extract local (non-remote) asset references from HTML-like markup.
///
/// Returns the raw `src`/`href` strings (trimmed, remote URLs filtered). Shared
/// between the HTML file parser and the JS/TS visitor's tagged template
/// literal override so `` html`<script src="...">` `` in Hono/lit-html/htm
/// layouts emits the same asset edges as a real `.html` file.
pub fn collect_asset_refs(source: &str) -> Vec<String> {
    collect_asset_ref_spans(source)
        .into_iter()
        .map(|asset| asset.value)
        .collect()
}

/// One asset reference and the byte range of its quoted value in the source.
pub struct AssetRef {
    pub value: String,
    /// Starts at the opening quote and ends after the closing quote.
    pub quoted: std::ops::Range<usize>,
}

/// [`collect_asset_refs`] with the location of each value. Comments and inline
/// script bodies become blanks of the same byte length, so an offset in the
/// scanned text is an offset in `source`.
pub fn collect_asset_ref_spans(source: &str) -> Vec<AssetRef> {
    let mut scanned = source.to_owned();
    for comment in HTML_COMMENT_RE.find_iter(source) {
        blank(&mut scanned, comment.range());
    }
    // Keep the opening tag so `<script src="...">` still matches, but drop the
    // inline body: markup inside it is script text, not document markup.
    let bodies: Vec<std::ops::Range<usize>> = SCRIPT_ELEMENT_RE
        .captures_iter(&scanned)
        .filter_map(|element| Some(element.get(1)?.end()..element.get(2)?.start()))
        .collect();
    for body in bodies {
        blank(&mut scanned, body);
    }

    let mut refs: Vec<AssetRef> = Vec::new();
    let mut push = |value: Option<regex::Match<'_>>| {
        let Some(value) = value else {
            return;
        };
        let trimmed = value.as_str().trim();
        if !trimmed.is_empty() && !is_remote_url(trimmed) && !is_template_placeholder(trimmed) {
            refs.push(AssetRef {
                value: trimmed.to_string(),
                quoted: value.start() - 1..value.end() + 1,
            });
        }
    };
    for cap in SCRIPT_SRC_RE.captures_iter(&scanned) {
        push(cap.get(1));
    }
    for cap in LINK_HREF_RE.captures_iter(&scanned) {
        push(cap.get(2));
    }
    for cap in LINK_HREF_REVERSE_RE.captures_iter(&scanned) {
        push(cap.get(1));
    }
    refs
}

/// Replace a byte range with spaces and keep its line breaks, so the byte
/// offsets and the line numbers of the text after it do not change.
fn blank(text: &mut String, range: std::ops::Range<usize>) {
    let replacement: String = text[range.clone()]
        .bytes()
        .map(|byte| if byte == b'\n' { '\n' } else { ' ' })
        .collect();
    text.replace_range(range, &replacement);
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
pub fn collect_custom_element_tags(source: &str) -> Vec<String> {
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
fn parse_html_to_module(file_id: FileId, source: &str, content_hash: u64) -> ModuleInfo {
    parse_html_to_module_with_complexity(file_id, source, content_hash, false)
}

/// Computed building blocks for an HTML [`ModuleInfo`], gathered before the
/// struct is assembled.
struct HtmlModuleParts {
    imports: Vec<ImportInfo>,
    /// The facts of the inline scripts. Their imports are in `imports`.
    inline_scripts: InlineScriptFacts,
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
fn collect_html_module_parts(
    source: &str,
    need_complexity: bool,
    collect_literal_spans: bool,
) -> HtmlModuleParts {
    let mut imports: Vec<ImportInfo> = collect_asset_refs(source)
        .into_iter()
        .map(|raw| ImportInfo {
            source: normalize_asset_url(&raw),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: Span::default(),
            source_span: Span::default(),
        })
        .collect();

    imports.sort_unstable_by(|a, b| a.source.cmp(&b.source));
    imports.dedup_by(|a, b| a.source == b.source);
    let mut inline_scripts = collect_inline_script_facts(source, collect_literal_spans);
    imports.append(&mut inline_scripts.imports);

    let angular::AngularTemplateRefs {
        identifiers,
        member_accesses: template_member_accesses,
        security_sinks,
    } = angular::collect_angular_template_refs(source);
    let identifiers: Vec<String> = identifiers.into_iter().collect();
    let mut semantic_facts: Vec<SemanticFact> = identifiers
        .iter()
        .cloned()
        .map(|member| {
            SemanticFact::AngularTemplateMemberAccess(AngularTemplateMemberAccessFact { member })
        })
        .collect();
    semantic_facts.append(&mut inline_scripts.semantic_facts);
    let mut member_accesses = template_member_accesses;
    member_accesses.append(&mut inline_scripts.member_accesses);

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
        inline_scripts,
        member_accesses,
        semantic_facts,
        security_sinks,
        angular_used_selectors,
        has_dynamic_component_render,
        complexity,
    }
}

/// Parse an HTML file and optionally compute Angular template complexity.
pub fn parse_html_to_module_with_complexity(
    file_id: FileId,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    // One parse of each inline script gives both the graph facts and the
    // literal spans that drop the suppression markers inside strings.
    let mut parsed_suppressions = crate::suppress::scan_suppressions_from_source(source);
    let has_markers = parsed_suppressions.has_markers();
    let parts = collect_html_module_parts(source, need_complexity, has_markers);
    parsed_suppressions.drop_markers_in_literals(&parts.inline_scripts.literal_spans);
    parsed_suppressions.pair_ranges();
    html_module_info(file_id, content_hash, source, parsed_suppressions, parts)
}

/// Assemble the `ModuleInfo` for an HTML file from its computed parts. The
/// JS-level fields hold only the imports of the inline scripts and the uses
/// of their bindings, because a page has no exports.
fn html_module_info(
    file_id: FileId,
    content_hash: u64,
    source: &str,
    parsed_suppressions: crate::suppress::ParsedSuppressions,
    parts: HtmlModuleParts,
) -> ModuleInfo {
    let HtmlModuleParts {
        imports,
        inline_scripts,
        member_accesses,
        semantic_facts,
        security_sinks,
        angular_used_selectors,
        has_dynamic_component_render,
        complexity,
    } = parts;

    let contracts = crate::sfc_template::component_contracts::collect(
        source,
        &imports,
        fallow_types::extract::ComponentFramework::Angular,
        0,
        &[],
        &[],
    );
    let function_contracts = has_inline_script(source).then(|| {
        Box::new(fallow_types::extract::FunctionContractFacts {
            template_callers: true,
            ..Default::default()
        })
    });
    ModuleInfo {
        component_contracts: Some(Box::new(contracts)),
        function_contracts,
        imports,
        dynamic_imports: inline_scripts.dynamic_imports,
        dynamic_import_patterns: inline_scripts.dynamic_import_patterns,
        whole_object_uses: inline_scripts.whole_object_uses.into(),
        member_accesses: member_accesses.into(),
        semantic_facts: semantic_facts.into(),
        content_hash,
        suppressions: parsed_suppressions.suppressions,
        unknown_suppression_kinds: parsed_suppressions.unknown_kinds,
        range_marker_problems: parsed_suppressions.range_problems,
        line_offsets: fallow_types::extract::compute_line_offsets(source),
        complexity,
        security_sinks,
        // Custom-element tags rendered in a standalone `.html` document (an app
        // shell, demo, or dev page) feed the Lit `unrendered-component` arm's
        // project-wide rendered-tag union, so an element rendered only from HTML
        // (e.g. a root `<my-app>` in `index.html`) is not falsely flagged.
        used_custom_element_tags: collect_custom_element_tags(source),
        angular_used_selectors,
        has_dynamic_component_render,
        ..ModuleInfo::empty(file_id)
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
    fn an_inline_script_marks_template_callers() {
        for source in [
            "<script type=\"module\">import { f } from './f.js'; f(2);</script>",
            "<SCRIPT type=module>\n  go();\n</SCRIPT>",
            "<script>import('./f.js').then((m) => m.f(2));</script>",
            "<script type=\"text/javascript\">go();</script>",
        ] {
            assert!(has_inline_script(source), "{source}");
        }
        for source in [
            "<script type=\"module\" src=\"./main.js\"></script>",
            "<!-- <script type=\"module\">go();</script> -->",
            "<script type=\"module\">  </script>",
            "<script type=\"importmap\">{\"imports\":{}}</script>",
            "<script type=\"application/ld+json\">{}</script>",
            "<script type=\"text/x-template\"><p></p></script>",
        ] {
            assert!(!has_inline_script(source), "{source}");
        }
    }

    #[test]
    fn inline_module_script_imports_enter_the_graph() {
        let source = "<!doctype html>\n<script type=\"module\">\n  import { x } from 'foo'\n  import './side.js'\n  console.log(x)\n</script>\n";
        let info = parse_html_to_module(FileId(0), source, 0);
        let foo = info
            .imports
            .iter()
            .find(|import| import.source == "foo")
            .expect("the inline script import of `foo`");
        assert!(matches!(&foo.imported_name, ImportedName::Named(name) if name == "x"));
        assert_eq!(
            &source[foo.source_span.start as usize..foo.source_span.end as usize],
            "'foo'"
        );
        assert!(
            info.imports
                .iter()
                .any(|import| import.source == "./side.js")
        );
    }

    #[test]
    fn inline_classic_script_dynamic_import_enters_the_graph() {
        let source = "<script>import('./lazy.js').then((m) => m.run());</script>";
        let info = parse_html_to_module(FileId(0), source, 0);
        let lazy = info
            .dynamic_imports
            .iter()
            .find(|import| import.source == "./lazy.js")
            .expect("the dynamic import of the classic script");
        assert_eq!(
            &source[lazy.span.start as usize..lazy.span.end as usize],
            "import('./lazy.js')"
        );
    }

    #[test]
    fn inline_script_records_the_uses_of_imported_bindings() {
        let source = "<script type=\"module\">\n  import('./dynamic.js').then((m) => m.default)\n  import * as ns from './ns.js'\n  ns.a\n</script>";
        let info = parse_html_to_module(FileId(0), source, 0);
        for (object, member) in [("m", "default"), ("ns", "a")] {
            assert!(
                info.member_accesses
                    .iter()
                    .any(|access| access.object == object && access.member == member),
                "{object}.{member}: {:?}",
                info.member_accesses
            );
        }
    }

    #[test]
    fn inline_script_template_placeholders_add_no_imports() {
        let source = "<script type=\"module\">\n  import '{{ STATIC_URL }}app.js'\n  import('{{ STATIC_URL }}lazy.js')\n  import(`${base}/x.js`)\n  import './kept.js'\n</script>";
        let info = parse_html_to_module(FileId(0), source, 0);
        let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"./kept.js"), "{sources:?}");
        assert!(
            sources.iter().all(|s| !is_template_placeholder(s)),
            "{sources:?}"
        );
        assert!(
            info.dynamic_imports
                .iter()
                .all(|import| !is_template_placeholder(&import.source)),
            "{:?}",
            info.dynamic_imports
        );
        assert!(
            info.dynamic_import_patterns
                .iter()
                .all(|pattern| !is_template_placeholder(&pattern.prefix)),
            "{:?}",
            info.dynamic_import_patterns
        );
    }

    #[test]
    fn inactive_inline_scripts_add_no_imports() {
        for source in [
            "<!-- <script type=\"module\">import 'foo'</script> -->",
            "<script type=\"importmap\">{\"imports\":{\"foo\":\"./foo.js\"}}</script>",
            "<script type=\"text/x-template\">import 'foo'</script>",
            "<script type=\"module\" src=\"./main.js\">import 'foo'</script>",
            "<script>import 'foo'</script>",
        ] {
            let info = parse_html_to_module(FileId(0), source, 0);
            assert!(
                info.imports.iter().all(|import| import.source != "foo"),
                "{source}: {:?}",
                info.imports
            );
        }
    }

    #[test]
    fn skips_inline_script() {
        let info = parse_html_to_module(FileId(0), r#"<script>console.log("hello");</script>"#, 0);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn skips_script_tag_string_inside_inline_script_body() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script>const markup = '<script src="./widget.js"></' + 'script>';</script>"#,
            0,
        );
        assert!(
            info.imports.is_empty(),
            "markup inside an inline script body is not an asset reference; got {:?}",
            info.imports
        );
    }

    #[test]
    fn skips_link_tag_string_inside_inline_script_body() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script type="module">
                 const css = `<link rel="stylesheet" href="./themes/dark.css">`;
               </script>"#,
            0,
        );
        assert!(
            info.imports.is_empty(),
            "markup inside an inline script body is not an asset reference; got {:?}",
            info.imports
        );
    }

    #[test]
    fn extracts_script_src_next_to_inline_script() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<script>window.flags = { debug: true };</script>
               <script src="./a.js"></script>
               <script type="module" src="./b.js">/* ignored body */</script>"#,
            0,
        );
        let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
        assert_eq!(sources, vec!["./a.js", "./b.js"]);
    }

    #[test]
    fn skips_js_interpolation_placeholder_in_link_href() {
        let info = parse_html_to_module(
            FileId(0),
            r#"<link href="${base}/x.css" rel="stylesheet">
               <script src="${contextPath}/app.js"></script>"#,
            0,
        );
        assert!(
            info.imports.is_empty(),
            "`${{...}}` interpolation placeholders should not enter the import graph; got {:?}",
            info.imports
        );
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
