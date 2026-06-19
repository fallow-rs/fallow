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

/// Parse an Astro file by extracting the frontmatter section, plus any
/// `<script src="...">` references and inline `<script>` import statements
/// in the template body.
pub(crate) fn parse_astro_to_module(
    file_id: FileId,
    source: &str,
    content_hash: u64,
) -> ModuleInfo {
    let parsed_suppressions = crate::suppress::parse_suppressions_from_source(source);
    let line_offsets = fallow_types::extract::compute_line_offsets(source);

    let frontmatter = extract_astro_frontmatter(source);
    let template_offset = frontmatter
        .as_ref()
        .map_or(0, |script| script.byte_offset + script.body.len());
    let template = source.get(template_offset..).unwrap_or("");

    let mut extractor = if let Some(script) = frontmatter.as_ref() {
        let source_type = SourceType::ts();
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &script.body, source_type).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);
        let extraction = ExtractionResult::contiguous(&script.body, script.byte_offset);
        extractor.remap_spans_with(|span| extraction.remap_span(span));
        extractor
    } else {
        ModuleInfoExtractor::new()
    };

    extend_imports_from_template(&mut extractor.imports, template, template_offset);

    let mut info = extractor.into_module_info(file_id, content_hash, parsed_suppressions);
    info.line_offsets = line_offsets;
    info
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
        let info = parse_astro_to_module(FileId(0), "<div>Hello</div>", 42);
        assert!(info.imports.is_empty());
        assert!(info.exports.is_empty());
        assert_eq!(info.content_hash, 42);
        assert_eq!(info.file_id, FileId(0));
    }

    #[test]
    fn parse_astro_to_module_with_imports() {
        let source = "---\nimport { ref } from 'vue';\nconst x = ref(0);\n---\n<div />";
        let info = parse_astro_to_module(FileId(1), source, 99);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "vue");
        assert_eq!(info.file_id, FileId(1));
        assert_eq!(info.content_hash, 99);
    }

    #[test]
    fn parse_astro_to_module_has_line_offsets() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(!info.line_offsets.is_empty());
    }

    #[test]
    fn parse_astro_to_module_has_suppressions() {
        let source = "---\n// fallow-ignore-file\nconst x = 1;\n---\n<div />";
        let info = parse_astro_to_module(FileId(0), source, 0);
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
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./client.ts");
    }

    #[test]
    fn parse_astro_template_script_src_parent_relative() {
        let source = "---\n---\n<script src=\"../scripts/foo.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "../scripts/foo.ts");
    }

    #[test]
    fn parse_astro_template_script_src_bare_normalized() {
        let source = "---\n---\n<script src=\"client.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./client.ts");
    }

    #[test]
    fn parse_astro_template_script_src_skips_remote() {
        let source = "---\n---\n<script src=\"https://cdn.example.com/lib.js\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn parse_astro_template_script_src_multiline_attrs() {
        let source = "---\n---\n<script\n  src=\"./client.ts\"\n></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./client.ts");
    }

    #[test]
    fn parse_astro_template_script_src_with_extra_attrs_is_unprocessed() {
        let source = "---\n---\n<script type=\"module\" src=\"./client.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn parse_astro_template_inline_script_import() {
        let source = "---\n---\n<script>\n  import '../scripts/bar';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
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
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "../utils");
    }

    #[test]
    fn parse_astro_template_inline_script_typescript_syntax() {
        let source = "---\n---\n<script>\n  import { foo } from '../utils';\n  const x: number = foo();\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "../utils");
    }

    #[test]
    fn parse_astro_template_inline_script_with_attributes_is_unprocessed() {
        let source = "---\n---\n<script is:inline>\n  import '../scripts/bar';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn parse_astro_template_type_module_inline_script_is_unprocessed() {
        let source = "---\n---\n<script type=\"module\">\n  import '../scripts/bar';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn parse_astro_template_skips_inline_body_when_src_present() {
        let source = "---\n---\n<script src=\"./client.ts\">import 'should-be-ignored';</script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
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
        let info = parse_astro_to_module(FileId(0), source, 0);
        let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"../scripts/foo.ts"));
        assert!(sources.contains(&"../scripts/bar"));
    }

    #[test]
    fn parse_astro_template_multiple_inline_scripts() {
        let source = "---\n---\n\
                      <script>\n  import '../a';\n</script>\n\
                      <script>\n  import '../b';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"../a"));
        assert!(sources.contains(&"../b"));
    }

    #[test]
    fn parse_astro_template_skips_commented_out_script_src() {
        let source = "---\n---\n<!-- <script src=\"./old.ts\"></script> -->\n<script src=\"./new.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./new.ts");
    }

    #[test]
    fn parse_astro_template_skips_commented_out_inline_script() {
        let source = "---\n---\n<!-- <script>\n  import '../old';\n</script> -->\n<script>\n  import '../new';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
        assert!(sources.contains(&"../new"));
        assert!(!sources.contains(&"../old"));
    }

    #[test]
    fn parse_astro_template_no_frontmatter_with_script() {
        let source = "<html><body><script src=\"./client.ts\"></script></body></html>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./client.ts");
    }

    #[test]
    fn parse_astro_template_empty_inline_script_is_skipped() {
        let source = "---\n---\n<script></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn parse_astro_template_does_not_double_count_frontmatter_imports() {
        let source = "---\nimport Layout from '../Layout.astro';\n---\n<Layout />";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "../Layout.astro");
    }
}
