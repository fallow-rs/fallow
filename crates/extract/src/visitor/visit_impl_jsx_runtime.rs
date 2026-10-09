//! The per-file JSX pragma `@jsxImportSource <source>`.
//!
//! With the automatic JSX runtime, the compiler adds an import of
//! `<source>/jsx-runtime` to a file that has this pragma. No import
//! statement shows that edge, so the extractor records it from the comment.

use oxc_ast::ast::Program;
use oxc_span::{GetSpan, Span};

use fallow_types::extract::{ImportInfo, ImportedName};

use super::super::ModuleInfoExtractor;

/// The runtime module that the automatic JSX transform imports from the
/// pragma source in a production build.
const JSX_RUNTIME_SUBPATH: &str = "jsx-runtime";

/// The bindings that the automatic JSX transform imports from
/// `<source>/jsx-runtime`.
const JSX_RUNTIME_BINDINGS: [&str; 3] = ["jsx", "jsxs", "Fragment"];

/// The JSX pragma keywords that this module reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JsxPragma {
    ImportSource,
    Runtime,
}

/// A JSX pragma value and its byte offset in the source text.
struct PragmaValue<'s> {
    value: &'s str,
    start: u32,
}

impl ModuleInfoExtractor {
    /// Record the imports that a `@jsxImportSource` pragma adds.
    ///
    /// The pragma rules follow the Oxc and Babel JSX transforms, with one
    /// stricter rule from TypeScript:
    /// - only comments before the first statement hold a pragma, so a
    ///   `@jsxImportSource` line in a later doc comment adds no edge;
    /// - the last value wins;
    /// - `@` must be at the start of the comment or follow whitespace or `*`;
    /// - `@jsxRuntime classic` selects the classic runtime, which imports
    ///   nothing;
    /// - a file without a JSX element or fragment imports nothing.
    ///
    /// Call this after the walk, because the walk sets `has_jsx`.
    ///
    /// A file with JSX and neither pragma sets `jsx_runtime_from_config`,
    /// because a bundler or test config can then supply the source.
    ///
    /// The import is named, with no local binding, so the graph credits the
    /// runtime bindings without a local usage check. The dev runtime
    /// (`<source>/jsx-dev-runtime`) is not recorded: a build can omit it, and
    /// a missing relative dev runtime would be a false unresolved import.
    pub(super) fn record_jsx_import_source_pragma(&mut self, program: &Program<'_>) {
        if !self.has_jsx {
            return;
        }
        let source_text = program.source_text;
        let leading_end = program
            .body
            .first()
            .map_or(program.span.end, |statement| statement.span().start);
        let mut import_source: Option<(PragmaValue<'_>, Span)> = None;
        let mut classic_runtime = false;
        for comment in program
            .comments
            .iter()
            .take_while(|comment| comment.span.end <= leading_end)
        {
            let content_span = comment.content_span();
            let content = content_span.source_text(source_text);
            for (pragma, value) in jsx_pragmas(content, content_span.start) {
                match pragma {
                    JsxPragma::ImportSource => import_source = Some((value, comment.span)),
                    JsxPragma::Runtime => classic_runtime = value.value == "classic",
                }
            }
        }
        if classic_runtime {
            return;
        }
        let Some((value, comment_span)) = import_source else {
            // Without a file pragma, the bundler or test config selects the
            // runtime source. The resolver adds that edge from a plugin rule.
            self.jsx_runtime_from_config = true;
            return;
        };
        let source = jsx_runtime_specifier(value.value);
        let value_end = value.start + u32::try_from(value.value.len()).unwrap_or(0);
        let source_span = Span::new(value.start, value_end);
        for binding in JSX_RUNTIME_BINDINGS {
            self.imports.push(ImportInfo {
                source: source.clone(),
                imported_name: ImportedName::Named(binding.to_string()),
                local_name: String::new(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: comment_span,
                source_span,
            });
        }
    }
}

/// The specifier that the automatic JSX transform imports for a pragma
/// source: `<source>/jsx-runtime`, with one separator.
fn jsx_runtime_specifier(import_source: &str) -> String {
    let base = import_source.trim_end_matches('/');
    if base.is_empty() {
        // `@jsxImportSource /` names the file-system root. Keep it as-is.
        return format!("/{JSX_RUNTIME_SUBPATH}");
    }
    format!("{base}/{JSX_RUNTIME_SUBPATH}")
}

/// The `@jsxImportSource` and `@jsxRuntime` pragmas in one comment, in source
/// order. `content_start` is the byte offset of `content` in the source text.
fn jsx_pragmas(content: &str, content_start: u32) -> Vec<(JsxPragma, PragmaValue<'_>)> {
    let mut pragmas = Vec::new();
    let bytes = content.as_bytes();
    let mut index = 0;
    while let Some(found) = content[index..].find("@jsx") {
        let at = index + found;
        index = at + 1;
        if at > 0 && !matches!(bytes[at - 1], b' ' | b'\t' | b'\r' | b'\n' | b'*') {
            continue;
        }
        let rest = &content[at + "@jsx".len()..];
        let Some(keyword_end) = rest.find([' ', '\t']) else {
            break;
        };
        let pragma = match &rest[..keyword_end] {
            "ImportSource" => JsxPragma::ImportSource,
            "Runtime" => JsxPragma::Runtime,
            _ => continue,
        };
        let after_keyword = &rest[keyword_end..];
        let value_text = after_keyword.trim_start_matches([' ', '\t']);
        let value_len = value_text
            .find(|c: char| c.is_ascii_whitespace() || c == '\u{0B}')
            .unwrap_or(value_text.len());
        let value = &value_text[..value_len];
        if value.is_empty() {
            continue;
        }
        let value_offset = content.len() - value_text.len();
        index = value_offset + value_len;
        pragmas.push((
            pragma,
            PragmaValue {
                value,
                start: content_start + u32::try_from(value_offset).unwrap_or(0),
            },
        ));
    }
    pragmas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(content: &str) -> Vec<&str> {
        jsx_pragmas(content, 0)
            .into_iter()
            .filter(|(pragma, _)| *pragma == JsxPragma::ImportSource)
            .map(|(_, value)| value.value)
            .collect()
    }

    #[test]
    fn reads_import_source_values() {
        assert_eq!(values("* @jsxImportSource ../../jsx "), vec!["../../jsx"]);
        assert_eq!(values("* @jsxImportSource . *"), vec!["."]);
        assert_eq!(values(" @jsxImportSource\tpreact"), vec!["preact"]);
        assert_eq!(
            values("*\n * @jsx h\n * @jsxImportSource @emotion/react\n "),
            vec!["@emotion/react"]
        );
    }

    #[test]
    fn ignores_inline_mentions_and_unknown_pragmas() {
        assert!(values("see `@jsxImportSource foo` in docs").is_empty());
        assert!(values("mail@jsxImportSource foo").is_empty());
        assert!(values("@jsxImportSourceX foo").is_empty());
        assert!(values("@jsxImportSource").is_empty());
        assert!(values("@jsxImportSource   ").is_empty());
    }

    #[test]
    fn value_offset_points_into_the_source_text() {
        let content = "* @jsxImportSource ./jsx ";
        let pragmas = jsx_pragmas(content, 10);
        let start = pragmas[0].1.start as usize - 10;
        assert_eq!(&content[start..start + "./jsx".len()], "./jsx");
    }

    #[test]
    fn runtime_specifier_has_one_separator() {
        assert_eq!(jsx_runtime_specifier("preact"), "preact/jsx-runtime");
        assert_eq!(jsx_runtime_specifier("hono/jsx"), "hono/jsx/jsx-runtime");
        assert_eq!(jsx_runtime_specifier("../"), "../jsx-runtime");
        assert_eq!(jsx_runtime_specifier("./"), "./jsx-runtime");
        assert_eq!(jsx_runtime_specifier("."), "./jsx-runtime");
    }
}
