//! Unicode oracle for extracted spans.
//!
//! The oracle parses modules with astral characters, combining marks and
//! Unicode whitespace in identifiers, strings, comments and indentation.
//! Each span that extraction reports must fall on a character boundary of
//! the source, so a consumer can slice the source at it. The slice of each
//! named export is the name, and the slice of each import source is the
//! quoted specifier. The byte line and column of each export must agree
//! with a scan of the source for `\n`.
//!
//! The case count and the seed come from `PROPTEST_CASES` and
//! `PROPTEST_RNG_SEED`. Without `PROPTEST_RNG_SEED` the seed is fixed, so
//! each pull request run checks the same cases.

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::{ExportName, ModuleInfo, byte_offset_to_line_col};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

use crate::parse_from_content;

const DEFAULT_SEED: u64 = 0x0001_f600;

/// JavaScript whitespace and line terminators, as indentation.
const INDENT: &[&str] = &[
    " ", "\t", "\u{a0}", "\u{3000}", "\u{2028}", "\u{2029}", "\u{1680}", "\u{2007}", "\u{202f}",
    "\u{205f}", "\u{feff}", "\u{b}", "\u{c}",
];

/// Identifier start characters: ASCII, two-byte, three-byte and astral.
const NAME_START: &[&str] = &[
    "a",
    "Z",
    "_",
    "$",
    "\u{e9}",
    "\u{65e5}",
    "\u{1d49c}",
    "\u{10400}",
];

/// Identifier characters after the first one, with combining marks and
/// the zero-width joiners that `ID_Continue` allows.
const NAME_REST: &[&str] = &[
    "a",
    "0",
    "_",
    "\u{e9}",
    "\u{65e5}",
    "\u{1d49c}",
    "\u{10400}",
    "\u{301}",
    "\u{200c}",
    "\u{200d}",
];

/// Text for strings, comments and module specifiers.
const TEXT: &[&str] = &[
    "a",
    " ",
    "\u{e9}",
    "e\u{301}",
    "\u{65e5}",
    "\u{1f600}",
    "\u{1f469}\u{200d}\u{1f4bb}",
    "\u{1d49c}",
    "\u{a0}",
    "\u{3000}",
    "\u{feff}",
    "\u{2028}",
];

/// The statement shapes, as templates. `{n}` is a unique identifier, `{t}`
/// is text, and `{s}` is a module specifier.
const STATEMENTS: &[&str] = &[
    "export const {n} = \"{t}\";",
    "export function {n}() { return '{t}'; }",
    "export class {n} { m = \"{t}\"; }",
    "export type {n} = \"{t}\";",
    "export interface {n} { \"{t}\": 1 }",
    "export enum {n} { A = \"{t}\" }",
    "const {n} = 1; export { {n} };",
    "import { {n} } from \"{s}\"; console.log({n});",
    "import * as {n} from \"{s}\"; console.log({n});",
    "export { {n} } from \"{s}\";",
    "const {n} = require(\"{s}\");",
    "const {n} = import(\"{s}\");",
    // A line comment would end at U+2028, so the text goes in a block
    // comment.
    "/* {t} */",
];

fn config() -> ProptestConfig {
    let mut config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    if matches!(config.rng_seed, RngSeed::Random) {
        config.rng_seed = RngSeed::Fixed(DEFAULT_SEED);
    }
    config
}

fn joined(set: &'static [&'static str], len: std::ops::Range<usize>) -> BoxedStrategy<String> {
    prop::collection::vec(prop::sample::select(set), len)
        .prop_map(|parts| parts.concat())
        .boxed()
}

/// One statement of a generated module.
#[derive(Debug, Clone)]
struct Statement {
    text: String,
    /// The declared identifier, unique in the module.
    name: String,
    /// The module specifier, if the statement has one.
    specifier: Option<String>,
}

fn statement(index: usize) -> impl Strategy<Value = Statement> {
    (
        joined(INDENT, 0..3),
        prop::sample::select(NAME_START),
        joined(NAME_REST, 0..4),
        prop::sample::select(STATEMENTS),
        joined(TEXT, 0..6),
    )
        .prop_map(move |(indent, start, rest, template, text)| {
            // The index keeps each name unique and never a keyword.
            let name = format!("{start}{rest}{index}");
            let specifier = template
                .contains("{s}")
                .then(|| format!("./{}{index}", text.replace(['"', '\\'], "")));
            let body = template
                .replace("{n}", &name)
                .replace("{t}", &text)
                .replace("{s}", specifier.as_deref().unwrap_or(""));
            Statement {
                text: format!("{indent}{body}"),
                name,
                specifier,
            }
        })
}

fn module() -> impl Strategy<Value = Vec<Statement>> {
    (1usize..7).prop_flat_map(|len| (0..len).map(statement).collect::<Vec<_>>())
}

/// Each span that extraction reports, by name.
fn spans(info: &ModuleInfo) -> Vec<(&'static str, u32, u32)> {
    let mut spans = Vec::new();
    for export in info.exports.iter() {
        spans.push(("export", export.span.start, export.span.end));
        for member in &export.members {
            spans.push(("member", member.span.start, member.span.end));
        }
    }
    for import in &info.imports {
        spans.push(("import", import.span.start, import.span.end));
        spans.push((
            "import source",
            import.source_span.start,
            import.source_span.end,
        ));
    }
    for re_export in &info.re_exports {
        spans.push(("re-export", re_export.span.start, re_export.span.end));
        spans.push((
            "re-export statement",
            re_export.statement_span.start,
            re_export.statement_span.end,
        ));
        spans.push((
            "re-export source",
            re_export.source_span.start,
            re_export.source_span.end,
        ));
    }
    for dynamic in &info.dynamic_imports {
        spans.push(("dynamic import", dynamic.span.start, dynamic.span.end));
    }
    for require in &info.require_calls {
        spans.push(("require", require.span.start, require.span.end));
        spans.push((
            "require source",
            require.source_span.start,
            require.source_span.end,
        ));
    }
    spans
}

/// The 1-based line and the 0-based byte column of `offset`, by a scan of
/// the text before it for `\n`.
fn line_col_by_scan(source: &str, offset: usize) -> (u32, u32) {
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let col = before
        .rfind('\n')
        .map_or(offset, |newline| offset - newline - 1);
    (
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(col).unwrap_or(u32::MAX),
    )
}

fn check(statements: &[Statement]) -> Result<(), TestCaseError> {
    let source = statements
        .iter()
        .map(|statement| statement.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let info = parse_from_content(FileId(0), Path::new("module.ts"), &source);
    // Extraction strips a leading byte order mark, so each span and line
    // offset is relative to the text after it (issue #475).
    let source = source.strip_prefix('\u{feff}').unwrap_or(&source);
    prop_assert_eq!(
        info.parse_error_count,
        0,
        "the module does not parse:\n{}",
        source
    );

    for (kind, start, end) in spans(&info) {
        let (start, end) = (start as usize, end as usize);
        prop_assert!(
            start <= end && source.get(start..end).is_some(),
            "the {} span {}..{} is not on a character boundary of:\n{}",
            kind,
            start,
            end,
            source
        );
    }

    for export in info.exports.iter() {
        let ExportName::Named(name) = &export.name else {
            continue;
        };
        let slice = &source[export.span.start as usize..export.span.end as usize];
        prop_assert_eq!(slice, name.as_str(), "the span of an export is its name");
        let (line, col) = byte_offset_to_line_col(&info.line_offsets, export.span.start);
        prop_assert_eq!(
            (line, col),
            line_col_by_scan(source, export.span.start as usize)
        );
    }

    for import in &info.imports {
        let slice = &source[import.source_span.start as usize..import.source_span.end as usize];
        let unquoted = slice
            .strip_prefix(['"', '\''])
            .and_then(|rest| rest.strip_suffix(['"', '\'']));
        prop_assert_eq!(
            unquoted,
            Some(import.source.as_str()),
            "the source span of an import is the quoted specifier"
        );
    }

    for statement in statements {
        if let Some(specifier) = &statement.specifier {
            let found = info
                .imports
                .iter()
                .any(|import| &import.source == specifier)
                || info.re_exports.iter().any(|re| &re.source == specifier)
                || info
                    .require_calls
                    .iter()
                    .any(|call| &call.source == specifier)
                || info
                    .dynamic_imports
                    .iter()
                    .any(|call| &call.source == specifier);
            prop_assert!(found, "no import of {:?} in:\n{}", specifier, source);
        }
        if statement.text.contains("export ") && !statement.text.contains("export { ") {
            let declared = info.exports.iter().any(
                |export| matches!(&export.name, ExportName::Named(name) if *name == statement.name),
            );
            prop_assert!(declared, "no export {:?} in:\n{}", statement.name, source);
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn spans_fall_on_character_boundaries(statements in module()) {
        // Known bug: see the ignored test `two_leading_byte_order_marks`.
        prop_assume!(!statements[0].text.starts_with("\u{feff}\u{feff}"));
        check(&statements)?;
    }
}

/// Known bug on main: `parse_from_content` strips a leading U+FEFF, and the
/// parser strips one more. A second U+FEFF is JavaScript whitespace, so the
/// spans of `\u{feff}\u{feff}export const a0 = 1;` are 3 bytes off: the
/// span of `a0` holds `st`.
#[test]
#[ignore = "known bug: extraction strips two leading U+FEFF"]
fn two_leading_byte_order_marks() {
    let statements = [Statement {
        text: "\u{feff}\u{feff}export const a0 = 1;".to_string(),
        name: "a0".to_string(),
        specifier: None,
    }];
    check(&statements).expect("each span counts the second U+FEFF");
}
