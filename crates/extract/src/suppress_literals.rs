//! Suppression markers outside the string literals of inline scripts.
//!
//! Vue, Svelte, Astro, MDX and HTML files get their suppressions from a line
//! scan, because the file is not one JavaScript program. The scan does not
//! know string context. These helpers parse the inline scripts only when the
//! scan found a marker, and drop each marker inside a string or template
//! literal.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{Program, StringLiteral, TemplateElement};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};

use crate::source_map::{ExtractionResult, ScriptSection, embedded_script_sections};
use crate::suppress::{ParsedSuppressions, scan_suppressions_from_source};

/// The inline scripts of a file, by file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptHost {
    /// A Vue or Svelte SFC: its `<script>` blocks.
    Sfc,
    /// An Astro component: its frontmatter and its `<script>` blocks.
    Astro,
    /// An MDX document: its `import` and `export` statements.
    Mdx,
    /// An HTML page: its inline `<script>` blocks.
    Html,
}

/// Parse the suppressions of a file of any type. A marker inside a string or
/// template literal of the file's scripts has no effect.
///
/// A JavaScript or TypeScript file is one script. A file type without
/// scripts, such as CSS, keeps every marker of the scan.
#[must_use]
pub fn parse_suppressions_for_file(path: &Path, source: &str) -> ParsedSuppressions {
    parse_suppressions_outside_literals(source, || script_sections_for_path(path, source))
}

/// Parse the suppressions of a file whose scripts `host` describes.
pub fn parse_host_suppressions(host: ScriptHost, source: &str) -> ParsedSuppressions {
    parse_suppressions_outside_literals(source, || host_sections(host, source))
}

/// Parse the suppressions of a Vue or Svelte SFC.
pub fn parse_sfc_suppressions(source: &str) -> ParsedSuppressions {
    parse_host_suppressions(ScriptHost::Sfc, source)
}

fn parse_suppressions_outside_literals(
    source: &str,
    sections: impl FnOnce() -> Vec<ScriptSection>,
) -> ParsedSuppressions {
    let mut parsed = scan_suppressions_from_source(source);
    if parsed.has_markers() {
        parsed.drop_markers_in_literals(&literal_spans(&sections()));
    }
    parsed.pair_ranges();
    parsed
}

fn script_sections_for_path(path: &Path, source: &str) -> Vec<ScriptSection> {
    if crate::astro::is_astro_file(path) {
        return host_sections(ScriptHost::Astro, source);
    }
    if let Some(sections) = embedded_script_sections(path, source) {
        return sections;
    }
    if crate::html::is_html_file(path) {
        return host_sections(ScriptHost::Html, source);
    }
    SourceType::from_path(path)
        .ok()
        .map(|source_type| ScriptSection {
            extraction: ExtractionResult::contiguous(source, 0),
            source_type,
        })
        .into_iter()
        .collect()
}

fn host_sections(host: ScriptHost, source: &str) -> Vec<ScriptSection> {
    match host {
        ScriptHost::Sfc => crate::sfc::extract_sfc_scripts(source)
            .iter()
            .filter(|script| script.src.is_none())
            .map(sfc_section)
            .collect(),
        ScriptHost::Astro => crate::astro::extract_astro_frontmatter(source)
            .iter()
            .map(sfc_section)
            .chain(astro_script_sections(source))
            .collect(),
        ScriptHost::Mdx => vec![ScriptSection {
            extraction: crate::mdx::extract_mdx_statements_mapped(source),
            source_type: SourceType::tsx(),
        }],
        ScriptHost::Html => crate::html::page_script_sections(source),
    }
}

/// Astro compiles its `<script>` blocks as TypeScript, so a typed
/// declaration must not stop the parse. A plain JavaScript block parses the
/// same way as TypeScript.
fn astro_script_sections(source: &str) -> impl Iterator<Item = ScriptSection> {
    crate::html::inline_script_sections(source)
        .into_iter()
        .map(|section| ScriptSection {
            source_type: SourceType::ts(),
            ..section
        })
}

fn sfc_section(script: &crate::sfc::SfcScript) -> ScriptSection {
    ScriptSection {
        extraction: ExtractionResult::contiguous(&script.body, script.byte_offset),
        source_type: crate::sfc::source_type_for_script(script),
    }
}

/// The spans of the string and template literals of `sections`, as byte
/// offsets in the file. A section that is too deep to parse safely gives no
/// spans, so its markers keep their effect.
fn literal_spans(sections: &[ScriptSection]) -> Vec<(u32, u32)> {
    let mut spans = Vec::new();
    for section in sections {
        let body = section.extraction.body.as_str();
        let found =
            crate::parse_bounded(body, || section_literal_spans(section)).unwrap_or_default();
        spans.extend(found);
    }
    spans
}

fn section_literal_spans(section: &ScriptSection) -> Vec<(u32, u32)> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &section.extraction.body, section.source_type).parse();
    program_literal_spans(&parsed.program, &section.extraction)
}

/// The spans of the string and template literals of `program`, as byte
/// offsets in the file that `extraction` maps to.
pub fn program_literal_spans(
    program: &Program<'_>,
    extraction: &ExtractionResult,
) -> Vec<(u32, u32)> {
    let mut collector = LiteralSpans {
        extraction,
        spans: Vec::new(),
    };
    collector.visit_program(program);
    collector.spans
}

struct LiteralSpans<'s> {
    extraction: &'s ExtractionResult,
    spans: Vec<(u32, u32)>,
}

impl LiteralSpans<'_> {
    fn push(&mut self, span: Span) {
        let span = self.extraction.remap_span(span);
        self.spans.push((span.start, span.end));
    }
}

impl<'a> Visit<'a> for LiteralSpans<'_> {
    fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
        self.push(literal.span);
    }

    fn visit_template_element(&mut self, element: &TemplateElement<'a>) {
        self.push(element.span);
    }
}
