//! Helpers for Glimmer component files (`.gts` / `.gjs`).

use std::ops::Range;
use std::path::Path;

const OPEN_TEMPLATE: &str = "<template";
const CLOSE_TEMPLATE: &str = "</template>";

#[derive(Clone, Copy, Debug)]
struct TemplateSpan {
    start: usize,
    opening_end: usize,
    close_start: usize,
    close_end: usize,
    has_closing_tag: bool,
}

impl TemplateSpan {
    fn range(self) -> Range<usize> {
        self.start..self.close_end
    }
}

/// Return `true` for Glimmer source files.
#[must_use]
pub fn is_glimmer_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext == "gts" || ext == "gjs")
}

/// Locate `<template>...</template>` block byte ranges. The returned ranges
/// span from the opening `<` of `<template` through the closing `>` of
/// `</template>`. An unclosed `<template` consumes from its start to the end
/// of the source. The byte offsets are stable and the same offsets used by
/// [`strip_glimmer_templates`] so callers (e.g. the
/// `crates/extract/src/sfc_template/glimmer.rs` scanner) can correlate the
/// two passes. The scanner walks each range over the UN-stripped source to
/// recover template-only import/member references that the stripped JS
/// parse pass cannot see.
#[must_use]
pub(crate) fn find_template_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let n = source.len();
    let mut cursor = 0;

    while let Some(span) = next_template_span(source, cursor) {
        ranges.push(span.range());
        if span.close_end >= n {
            break;
        }
        cursor = span.close_end;
    }

    ranges
}

/// Strip Glimmer `<template>` blocks while preserving byte offsets and line
/// numbers for the JavaScript/TypeScript parser.
///
/// Two replacement strategies are used based on the surrounding context, so
/// that the post-strip source is always syntactically valid TS/JS regardless
/// of how many templates appear in the file:
///
/// - Class-body context (`<template>` directly inside a class body): blank
///   all bytes to spaces, leaving an empty class body, which is valid
///   TS/JS. This matches the original behavior.
///
/// - Expression context (`<template>` follows `=`, `,`, `(`, `?`, or `:`):
///   replace with a byte-length-preserving parenthesised template literal
///   `` (`...`) ``. The opening tag's bytes become `` (` `` plus spaces;
///   the closing tag's bytes become spaces plus `` `) ``. Content bytes
///   that would interfere with the inert string (`` ` ``, `$`, `\`) are
///   escaped to spaces. This keeps the surrounding statement
///   (e.g. `const x = <template>...</template>;`) syntactically valid so
///   oxc can extract imports and exports from the rest of the file.
///
/// Without the expression-context strategy, a file containing both a
/// module-level template expression and a class-body template would yield
/// `const x = ;` after blanking, which is a syntax error that prevents oxc
/// from recovering any imports.
#[must_use]
pub fn strip_glimmer_templates(source: &str) -> Option<String> {
    let mut bytes = source.as_bytes().to_vec();
    let mut changed = false;
    let n = source.len();
    let mut cursor = 0;

    while let Some(span) = next_template_span(source, cursor) {
        rewrite_template_span(source.as_bytes(), &mut bytes, span);
        changed = true;
        cursor = span.close_end;
        if cursor >= n {
            break;
        }
    }

    if changed {
        String::from_utf8(bytes).ok()
    } else {
        None
    }
}

fn next_template_span(source: &str, cursor: usize) -> Option<TemplateSpan> {
    let n = source.len();
    let relative_start = source[cursor..].find(OPEN_TEMPLATE)?;
    let start = cursor + relative_start;
    let after_template_word = start + OPEN_TEMPLATE.len();
    let opening_end = source[after_template_word..]
        .find('>')
        .map_or(n, |relative| after_template_word + relative + 1);

    match source[opening_end..].find(CLOSE_TEMPLATE) {
        Some(relative_close) => {
            let close_start = opening_end + relative_close;
            Some(TemplateSpan {
                start,
                opening_end,
                close_start,
                close_end: close_start + CLOSE_TEMPLATE.len(),
                has_closing_tag: true,
            })
        }
        None => Some(TemplateSpan {
            start,
            opening_end,
            close_start: n,
            close_end: n,
            has_closing_tag: false,
        }),
    }
}

fn rewrite_template_span(source: &[u8], bytes: &mut [u8], span: TemplateSpan) {
    if can_rewrite_as_expression(source, span) {
        rewrite_expression_template(bytes, span);
    } else {
        blank_template_span(bytes, span);
    }
}

fn can_rewrite_as_expression(source: &[u8], span: TemplateSpan) -> bool {
    let opening_len = span.opening_end - span.start;
    let closing_len = span.close_end - span.close_start;

    span.has_closing_tag
        && opening_len >= 2
        && closing_len >= 2
        && is_expression_position(source, span.start)
}

fn rewrite_expression_template(bytes: &mut [u8], span: TemplateSpan) {
    bytes[span.start] = b'(';
    bytes[span.start + 1] = b'`';
    blank_non_newline(&mut bytes[span.start + 2..span.opening_end]);
    escape_template_literal_bytes(&mut bytes[span.opening_end..span.close_start]);
    blank_non_newline(&mut bytes[span.close_start..span.close_end - 2]);
    bytes[span.close_end - 2] = b'`';
    bytes[span.close_end - 1] = b')';
}

fn blank_template_span(bytes: &mut [u8], span: TemplateSpan) {
    blank_non_newline(&mut bytes[span.start..span.close_end]);
}

fn blank_non_newline(bytes: &mut [u8]) {
    for byte in bytes {
        if !matches!(*byte, b'\n' | b'\r') {
            *byte = b' ';
        }
    }
}

fn escape_template_literal_bytes(bytes: &mut [u8]) {
    for byte in bytes {
        if matches!(*byte, b'`' | b'$' | b'\\') {
            *byte = b' ';
        }
    }
}

/// Return `true` when the byte at `pos` opens a template in JS/TS expression
/// position. Heuristic: walk back over whitespace and check the previous
/// non-whitespace byte against a small set of expression-only delimiters,
/// then fall back to checking whether the byte ends an expression-prefix
/// keyword (`default`, `return`, `throw`, `yield`, `await`, `new`).
///
/// Covered shapes:
/// - assignment / declaration initializer: `const x = <template>...`
/// - argument: `foo(<template>...)`, `decorator(<template>...)`
/// - sequence expression: `(a, <template>...)`
/// - ternary: `cond ? <template>... : <template>...`
/// - standalone default export: `export default <template>...`
/// - return / throw / yield / await / new: `return <template>...`
///
/// Class-body templates land on `{`, `;`, or `}` (after a prior method
/// body), none of which appear in the set, so they fall through to the
/// blank-out branch. Identifier-byte lookback collects the FULL identifier
/// ending at `pos` and only matches against the keyword set, so a user
/// binding like `mydefault` or `$return` falls through correctly.
fn is_expression_position(bytes: &[u8], pos: usize) -> bool {
    let mut idx = pos;
    while idx > 0 && matches!(bytes[idx - 1], b' ' | b'\t' | b'\n' | b'\r') {
        idx -= 1;
    }
    if idx == 0 {
        return false;
    }
    let prev = bytes[idx - 1];
    if matches!(prev, b'=' | b',' | b'(' | b'?' | b':') {
        return true;
    }
    if !is_identifier_byte(prev) {
        return false;
    }
    let ident = prev_identifier(bytes, idx);
    matches!(
        ident,
        b"default" | b"return" | b"throw" | b"yield" | b"await" | b"new"
    )
}

/// Return `true` for bytes that can appear inside an ASCII JS/TS identifier.
/// Includes `$` because it is a legal identifier byte in JS; widening the
/// walk-back boundary to `$` means user identifiers like `$default` collect
/// the full `$default` rather than the suffix `default`, avoiding a false
/// positive against the keyword set.
fn is_identifier_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// Collect the identifier ending at `end` by walking back while the bytes
/// are identifier bytes. Returns the identifier slice, which may be empty
/// if `bytes[end - 1]` is not an identifier byte.
fn prev_identifier(bytes: &[u8], end: usize) -> &[u8] {
    let mut start = end;
    while start > 0 && is_identifier_byte(bytes[start - 1]) {
        start -= 1;
    }
    &bytes[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_template_ranges_captures_all_blocks() {
        let source = "a<template>b\nc</template>d<template>e</template>f";
        let ranges = find_template_ranges(source);
        assert_eq!(ranges.len(), 2);
        assert_eq!(&source[ranges[0].clone()], "<template>b\nc</template>");
        assert_eq!(&source[ranges[1].clone()], "<template>e</template>");
    }

    #[test]
    fn find_template_ranges_handles_unclosed_block() {
        let source = "<template>nope";
        let ranges = find_template_ranges(source);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 0..source.len());
    }

    #[test]
    fn find_template_ranges_returns_empty_when_absent() {
        assert!(find_template_ranges("export const x = 1;").is_empty());
    }

    #[test]
    fn strips_template_blocks_and_preserves_newlines() {
        let source =
            "import x from './x';\n<template>\n  <x />\n</template>\nexport const y = x;\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(stripped.contains("import x from './x';"));
        assert!(stripped.contains("export const y = x;"));
        assert!(!stripped.contains("<template>"));
        assert_eq!(stripped.len(), source.len());
        assert_eq!(stripped.lines().count(), source.lines().count());
    }

    #[test]
    fn strips_class_body_template_to_empty_class() {
        let source = "import Component from '@glimmer/component';\nexport default class X extends Component {\n  <template>billing</template>\n}\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(stripped.contains("class X extends Component {"));
        assert!(!stripped.contains("<template>"));
        assert!(!stripped.contains('('));
        assert_eq!(stripped.len(), source.len());
    }

    #[test]
    fn replaces_module_level_template_expression_with_parenthesised_literal() {
        let source = "const x = <template>foo</template>;\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(stripped.contains("const x = (`"));
        assert!(stripped.contains("`);"));
        assert!(!stripped.contains("<template>"));
        assert_eq!(stripped.len(), source.len());
    }

    #[test]
    fn handles_multi_template_module_and_class_in_same_file() {
        let source = "import C from '@glimmer/component';\nconst W = <template>\n  one\n</template>;\nexport default class X extends C {\n  <template>\n    two\n  </template>\n}\n";
        let stripped = strip_glimmer_templates(source).expect("templates should be stripped");

        assert!(stripped.contains("const W = (`"));
        assert!(stripped.contains("`);"));
        assert!(stripped.contains("class X extends C {"));
        assert!(!stripped.contains("<template>"));
        assert!(!stripped.contains("</template>"));
        assert_eq!(stripped.len(), source.len());
        assert_eq!(stripped.lines().count(), source.lines().count());
    }

    #[test]
    fn escapes_backtick_dollar_backslash_inside_expression_template() {
        let source = "const x = <template>a`b${c}d\\e</template>;\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(!stripped.contains('`') || stripped.matches('`').count() == 2);
        assert!(!stripped.contains("${"));
        assert!(!stripped.contains('\\'));
        assert!(stripped.contains("a b "));
        assert!(stripped.contains("d e"));
        assert_eq!(stripped.len(), source.len());
    }

    #[test]
    fn unclosed_template_blanks_to_eof_without_expression_form() {
        let source = "const x = <template>oops\nexport const y = 1;\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(!stripped.contains("<template>"));
        assert_eq!(stripped.len(), source.len());
    }

    #[test]
    fn handles_template_after_typed_initializer() {
        let source = "const x: TOC<{}> = <template>hi</template>;\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(stripped.contains("const x: TOC<{}> = (`"));
        assert!(stripped.contains("`);"));
        assert_eq!(stripped.len(), source.len());
    }

    #[test]
    fn handles_template_in_decorator_call() {
        let source = "@Some(<template>x</template>)\nclass Foo {}\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(stripped.contains("@Some((`"));
        assert!(stripped.contains("`))"));
        assert_eq!(stripped.len(), source.len());
    }

    /// Regression for issue #379: `export default <template>...</template>`
    /// (no const wrapper) is the canonical template-only-component shape.
    /// Without keyword lookback, the previous non-whitespace byte `t` from
    /// `default` falls through to blank-out, leaving `export default ;`
    /// which is a TypeScript syntax error.
    #[test]
    fn handles_template_after_export_default() {
        let source =
            "import Icon from './icon';\nexport default <template>\n  <Icon />\n</template>\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(stripped.contains("import Icon from './icon';"));
        assert!(stripped.contains("export default (`"));
        assert!(stripped.contains("`)"));
        assert!(!stripped.contains("<template>"));
        assert_eq!(stripped.len(), source.len());
    }

    #[test]
    fn handles_template_after_return_keyword() {
        let source = "function build() {\n  return <template>hi</template>;\n}\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(stripped.contains("return (`"));
        assert!(stripped.contains("`);"));
        assert_eq!(stripped.len(), source.len());
    }

    #[test]
    fn handles_template_after_throw_keyword() {
        let source = "function fail() {\n  throw <template>x</template>;\n}\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(stripped.contains("throw (`"));
        assert!(stripped.contains("`);"));
        assert_eq!(stripped.len(), source.len());
    }

    /// User identifiers that happen to end with a keyword suffix must NOT
    /// trigger expression-form stripping. The walk-back collects the FULL
    /// identifier (`mydefault`), which does not match the keyword set, so
    /// the stripper falls through to blank-out.
    #[test]
    fn identifier_ending_in_keyword_suffix_falls_through_to_blank() {
        let source = "mydefault <template>x</template>\n";
        let stripped = strip_glimmer_templates(source).expect("template should be stripped");

        assert!(!stripped.contains("(`"));
        assert!(!stripped.contains("<template>"));
        assert_eq!(stripped.len(), source.len());
    }
}
