//! Unicode oracle for the remove-export quick fix.
//!
//! Analysis results carry byte columns, and the LSP wire contract uses
//! UTF-16 code units. The oracle gives the quick fix source lines with
//! astral characters, combining marks and Unicode whitespace as
//! indentation. It applies each edit the way an LSP client does, by UTF-16
//! units, and checks these facts:
//!
//! - each edit position falls on a character boundary, never inside a
//!   surrogate pair;
//! - the edit removes the `export ` keyword of the target line, and changes
//!   no other text;
//! - the diagnostic range covers the export name, in UTF-16 units.
//!
//! The case count and the seed come from `PROPTEST_CASES` and
//! `PROPTEST_RNG_SEED`. Without `PROPTEST_RNG_SEED` the seed is fixed, so
//! each pull request run checks the same cases.

use proptest::prelude::*;
use proptest::test_runner::RngSeed;

use super::*;

const DEFAULT_SEED: u64 = 0x0001_f600;

/// Leading whitespace. Each item except U+FEFF is whitespace for both
/// JavaScript and `str::trim_start`. U+FEFF is JavaScript whitespace only,
/// so the quick fix declines a line that starts with it.
const INDENT: &[&str] = &[
    " ", "\t", "\u{a0}", "\u{3000}", "\u{2028}", "\u{2029}", "\u{1680}", "\u{2007}", "\u{202f}",
    "\u{205f}", "\u{feff}",
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

/// Identifier characters after the first one. Combining marks are absent
/// because of the known bug in the ignored test `combining_mark_in_name`.
const NAME_REST: &[&str] = &[
    "a",
    "0",
    "_",
    "\u{e9}",
    "\u{65e5}",
    "\u{1d49c}",
    "\u{10400}",
];

/// Text for string literals and other lines: astral characters, combining
/// marks, joiners and Unicode whitespace.
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

/// Declarations that the quick fix can strip `export ` from, as
/// `(before name, after name)`.
const DECLARATIONS: &[(&str, &str)] = &[
    ("const ", " = \"$text\";"),
    ("let ", " = '$text';"),
    ("function ", "() { return \"$text\"; }"),
    ("async function ", "() {}"),
    ("class ", " { m = \"$text\"; }"),
    ("type ", " = \"$text\";"),
    ("interface ", " { \"$text\": 1 }"),
    ("enum ", " { A = \"$text\" }"),
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

/// A source file with one exported declaration on line `target`.
#[derive(Debug, Clone)]
struct Case {
    lines: Vec<String>,
    target: usize,
    indent: String,
    name: String,
    /// Byte column of the name on the target line.
    name_col: usize,
}

fn case() -> impl Strategy<Value = Case> {
    let other_line = joined(TEXT, 0..8).prop_map(|text| format!("// {text}"));
    (
        prop::collection::vec(other_line.clone(), 0..3),
        prop::collection::vec(other_line, 0..3),
        joined(INDENT, 0..4),
        prop::sample::select(NAME_START),
        joined(NAME_REST, 0..4),
        prop::sample::select(DECLARATIONS),
        joined(TEXT, 0..6),
    )
        .prop_map(
            |(before, after, indent, start, rest, (keyword, tail), text)| {
                let name = format!("{start}{rest}");
                let tail = tail.replace("$text", &text);
                let target = before.len();
                let name_col = indent.len() + "export ".len() + keyword.len();
                let mut lines = before;
                lines.push(format!("{indent}export {keyword}{name}{tail}"));
                lines.extend(after);
                Case {
                    lines,
                    target,
                    indent,
                    name,
                    name_col,
                }
            },
        )
}

/// Convert a UTF-16 column on `line` to a byte offset. `None` when the
/// column is past the line end or inside a surrogate pair.
fn utf16_to_byte(line: &str, column: u32) -> Option<usize> {
    let mut units = 0;
    for (byte, ch) in line.char_indices() {
        if units == column {
            return Some(byte);
        }
        units += u32::try_from(ch.len_utf16()).ok()?;
        if units > column {
            return None;
        }
    }
    (units == column).then_some(line.len())
}

fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

/// Apply `edits` to `lines` the way an LSP client does.
fn apply(lines: &[String], edits: &[TextEdit]) -> Result<Vec<String>, TestCaseError> {
    let mut result = lines.to_vec();
    for edit in edits {
        let Range { start, end } = edit.range;
        prop_assert_eq!(start.line, end.line, "the edit spans more than one line");
        let line = &mut result[start.line as usize];
        let from = utf16_to_byte(line, start.character);
        let to = utf16_to_byte(line, end.character);
        prop_assert!(
            from.is_some() && to.is_some(),
            "the edit range {}..{} is not on a character boundary of {:?}",
            start.character,
            end.character,
            line
        );
        line.replace_range(from.unwrap_or(0)..to.unwrap_or(0), &edit.new_text);
    }
    Ok(result)
}

fn finding(path: &Path, case: &Case) -> fallow_api::editor_results::UnusedExportFinding {
    fallow_api::editor_results::UnusedExportFinding::with_actions(
        fallow_api::editor_results::UnusedExport {
            path: path.to_path_buf(),
            export_name: case.name.clone(),
            is_type_only: false,
            line: u32::try_from(case.target + 1).unwrap_or(u32::MAX),
            col: u32::try_from(case.name_col).unwrap_or(u32::MAX),
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        },
    )
}

/// A setup step failed, so the case cannot run.
fn setup_error(error: impl std::fmt::Display) -> TestCaseError {
    TestCaseError::fail(format!("test setup failed: {error}"))
}

fn check(case: &Case) -> Result<(), TestCaseError> {
    check_file(case, "")
}

/// Check `case` with `disk_prefix` before the text of the file on disk.
/// An editor hides a UTF-8 byte order mark, so the document lines and the
/// analysis columns do not contain it, but the file on disk does.
fn check_file(case: &Case, disk_prefix: &str) -> Result<(), TestCaseError> {
    let dir = tempfile::tempdir().map_err(setup_error)?;
    let path = dir.path().join("module.ts");
    std::fs::write(&path, format!("{disk_prefix}{}", case.lines.join("\n")))
        .map_err(setup_error)?;
    let uri = Uri::from_file_path(&path).expect("an absolute path");

    let mut results = AnalysisResults::default();
    results.unused_exports.push(finding(&path, case));
    let line_refs: Vec<&str> = case.lines.iter().map(String::as_str).collect();
    let target = u32::try_from(case.target).unwrap_or(u32::MAX);
    let cursor = Range {
        start: Position {
            line: target,
            character: 0,
        },
        end: Position {
            line: target,
            character: 0,
        },
    };
    let actions = build_remove_export_actions(RemoveExportActionInput::new(
        &results, &path, &uri, &cursor, &line_refs,
    ));

    if !case.indent.chars().all(char::is_whitespace) {
        prop_assert!(
            actions.is_empty(),
            "a quick fix for a line that `str::trim_start` does not trim"
        );
        return Ok(());
    }
    prop_assert!(
        !actions.is_empty(),
        "no quick fix for {:?}",
        case.lines[case.target]
    );
    prop_assert_eq!(actions.len(), 1);
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        return Err(TestCaseError::fail("the quick fix is a command"));
    };

    let edits = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .and_then(|changes| changes.get(&uri))
        .ok_or_else(|| TestCaseError::fail("the quick fix has no edit for the file"))?;
    let edited = apply(&case.lines, edits)?;
    let mut expected = case.lines.clone();
    expected[case.target] = expected[case.target].replacen("export ", "", 1);
    prop_assert_eq!(edited, expected);

    let diagnostic = &action.diagnostics.as_ref().expect("a diagnostic")[0];
    let line = &case.lines[case.target];
    let name_start = utf16_len(&line[..case.name_col]);
    prop_assert_eq!(diagnostic.range.start.character, name_start);
    prop_assert_eq!(
        diagnostic.range.end.character,
        name_start + utf16_len(&case.name)
    );
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn remove_export_edit_changes_only_the_export_keyword(case in case()) {
        check(&case)?;
    }
}

/// U+00A0 is two UTF-8 bytes and one UTF-16 unit, so a byte count gives
/// the edit range `2..9` instead of `1..8`.
#[test]
fn nbsp_indentation() {
    let case = Case {
        lines: vec!["\u{a0}export const helper = 1;".to_string()],
        target: 0,
        indent: "\u{a0}".to_string(),
        name: "helper".to_string(),
        name_col: "\u{a0}export const ".len(),
    };
    check(&case).expect("the edit removes only `export `");
}

/// The analysis computes byte columns after a UTF-8 byte order mark, and
/// the position mapper reads the file without the mark. For
/// `\u{feff}export const helper = 1;` on disk, the diagnostic starts at
/// UTF-16 column 13.
#[test]
fn byte_order_mark_on_disk() {
    let case = Case {
        lines: vec!["export const helper = 1;".to_string()],
        target: 0,
        indent: String::new(),
        name: "helper".to_string(),
        name_col: "export const ".len(),
    };
    check_file(&case, "\u{feff}").expect("the diagnostic covers the name");
}

/// Known bug on main: `\u{301}` is a combining mark, which `ID_Continue`
/// allows in a name, but the quick fix reads the name only up to it. For
/// `export const cafe\u{301} = 1;` the quick fix offers no action.
#[test]
#[ignore = "known bug: no quick fix for a name with a combining mark"]
fn combining_mark_in_name() {
    let case = Case {
        lines: vec!["export const cafe\u{301} = 1;".to_string()],
        target: 0,
        indent: String::new(),
        name: "cafe\u{301}".to_string(),
        name_col: "export const ".len(),
    };
    check(&case).expect("the quick fix removes only `export `");
}
