//! Unicode oracle for the `fallow fix` export edit.
//!
//! The oracle writes source files with astral characters, combining marks,
//! Unicode whitespace as indentation, an optional UTF-8 byte order mark and
//! CRLF line endings. It applies the fix for one unused export and checks
//! that the fix removes the `export ` keyword of the target line, keeps the
//! byte order mark and the line endings, and changes no other byte.
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
/// so the fix declines a line that starts with it.
const INDENT: &[&str] = &[
    " ", "\t", "\u{a0}", "\u{3000}", "\u{2028}", "\u{2029}", "\u{1680}", "\u{2007}", "\u{202f}",
    "\u{205f}", "\u{feff}",
];

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
];

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

/// Declarations as `(before name, after name)`. `$text` is text.
const DECLARATIONS: &[(&str, &str)] = &[
    ("const ", " = \"$text\";"),
    ("function ", "() { return \"$text\"; }"),
    ("class ", " { m = \"$text\"; }"),
    ("type ", " = \"$text\";"),
    ("interface ", " { \"$text\": 1 }"),
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
    bom: bool,
    line_ending: &'static str,
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
        any::<bool>(),
        prop::sample::select(&["\n", "\r\n"][..]),
    )
        .prop_map(
            |(before, after, indent, start, rest, (keyword, tail), text, bom, line_ending)| {
                let name = format!("{start}{rest}");
                let tail = tail.replace("$text", &text);
                let target = before.len();
                let mut lines = before;
                lines.push(format!("{indent}export {keyword}{name}{tail}"));
                lines.extend(after);
                lines.push(String::new());
                Case {
                    lines,
                    target,
                    indent,
                    name,
                    bom,
                    line_ending,
                }
            },
        )
}

fn file_text(case: &Case, lines: &[String]) -> String {
    let bom = if case.bom { "\u{feff}" } else { "" };
    format!("{bom}{}", lines.join(case.line_ending))
}

/// A setup step failed, so the case cannot run.
fn setup_error(error: impl std::fmt::Display) -> TestCaseError {
    TestCaseError::fail(format!("test setup failed: {error}"))
}

fn check(case: &Case) -> Result<(), TestCaseError> {
    let dir = tempfile::tempdir().map_err(setup_error)?;
    let root = dir.path();
    let path = root.join("module.ts");
    let original = file_text(case, &case.lines);
    std::fs::write(&path, &original).map_err(setup_error)?;

    let export = fallow_types::results::UnusedExport {
        path: path.clone(),
        export_name: case.name.clone(),
        is_type_only: false,
        line: u32::try_from(case.target + 1).unwrap_or(u32::MAX),
        col: 0,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    };
    let mut exports_by_file: FxHashMap<PathBuf, Vec<&fallow_types::results::UnusedExport>> =
        FxHashMap::default();
    exports_by_file.insert(path.clone(), vec![&export]);
    // The analysis hashes the text after the byte order mark.
    let analysed = original.strip_prefix('\u{feff}').unwrap_or(&original);
    let mut hashes = CapturedHashes::default();
    hashes.insert(
        path.clone(),
        xxhash_rust::xxh3::xxh3_64(analysed.as_bytes()),
    );
    let mut plan = FixPlan::for_root(root).map_err(setup_error)?;
    let mut fixes = Vec::new();
    apply_export_fixes(&mut ExportFixInput {
        root,
        exports_by_file: &exports_by_file,
        hashes: &hashes,
        unresolved_import_files: &FxHashSet::default(),
        caveats_by_file: &FxHashMap::default(),
        plan: &mut plan,
        output: OutputFormat::Json,
        dry_run: false,
        fixes: &mut fixes,
    });
    let outcome = plan.commit();
    prop_assert!(outcome.failed.is_empty(), "the fix plan failed to commit");

    let actual = std::fs::read_to_string(&path).map_err(setup_error)?;
    if fixes.is_empty() {
        prop_assert!(
            !case.indent.chars().all(char::is_whitespace),
            "no fix for {:?}",
            case.lines[case.target]
        );
        prop_assert_eq!(actual, original, "a declined fix changed the file");
        return Ok(());
    }
    let mut expected_lines = case.lines.clone();
    expected_lines[case.target] = expected_lines[case.target].replacen("export ", "", 1);
    prop_assert_eq!(actual, file_text(case, &expected_lines));
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn export_fix_changes_only_the_export_keyword(case in case()) {
        check(&case)?;
    }
}
