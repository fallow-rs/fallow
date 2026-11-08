//! Unicode oracle for the infrastructure file parsers.
//!
//! The Dockerfile, Procfile and `fly.toml` parsers cut lines at byte
//! offsets. The oracle gives them lines with astral characters, combining
//! marks and Unicode whitespace around and inside the instruction keywords.
//! Each parser must return without a panic, and each command that the input
//! plants must give its file reference.
//!
//! The case count and the seed come from `PROPTEST_CASES` and
//! `PROPTEST_RNG_SEED`. Without `PROPTEST_RNG_SEED` the seed is fixed, so
//! each pull request run checks the same cases.

use proptest::prelude::*;
use proptest::test_runner::RngSeed;

use super::*;

const DEFAULT_SEED: u64 = 0x0001_f600;

/// Characters that are more than one UTF-8 byte, more than one UTF-16 unit,
/// or a combining mark, plus Unicode whitespace that `str::trim` removes.
const NOISE: &[&str] = &[
    "a",
    "Z",
    "0",
    " ",
    "\t",
    "\u{e9}",
    "e\u{301}",
    "\u{3042}",
    "\u{65e5}",
    "\u{1f600}",
    "\u{1d49c}",
    "\u{10400}",
    "\u{a0}",
    "\u{3000}",
    "\u{feff}",
    "\u{2028}",
    "\u{2029}",
    "\u{1680}",
    "\u{202f}",
    "\u{200d}",
    "\"",
    "[",
    "]",
    ",",
    "=",
    ":",
    "-",
];

/// Instruction keywords, whole and cut, in mixed case.
const KEYWORDS: &[&str] = &[
    "RUN ",
    "CMD ",
    "ENTRYPOINT ",
    "run ",
    "Cmd ",
    "R",
    "RU",
    "RUN",
    "C",
    "CM",
    "CMD",
    "ENTRY",
    "ENTRYPOIN",
    "ENTRYPOINT",
    "",
];

/// Leading whitespace that `str::trim` removes.
const INDENT: &[&str] = &[" ", "\t", "\u{a0}", "\u{3000}", "\u{2028}", "\u{1680}"];

/// Letters for a planted file name, ASCII and non-ASCII.
const NAME_CHARS: &[&str] = &["a", "b", "z", "\u{e9}", "\u{65e5}", "\u{1d49c}", "\u{3042}"];

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

fn pick(set: &'static [&'static str]) -> impl Strategy<Value = &'static str> {
    prop::sample::select(set)
}

fn joined(set: &'static [&'static str], len: std::ops::Range<usize>) -> BoxedStrategy<String> {
    prop::collection::vec(pick(set), len)
        .prop_map(|parts| parts.concat())
        .boxed()
}

/// A line that may start with a keyword or a cut keyword, followed by
/// non-ASCII text. A multi-byte character can sit at each byte offset that
/// the parser compares.
fn noise_line() -> BoxedStrategy<String> {
    (joined(INDENT, 0..3), pick(KEYWORDS), joined(NOISE, 0..12))
        .prop_map(|(indent, keyword, rest)| format!("{indent}{keyword}{rest}"))
        .boxed()
}

/// A file name such as `scripts/日a.ts`. The name has no space, quote or
/// backslash, so each parser reads it as one argument.
fn planted_file() -> BoxedStrategy<String> {
    joined(NAME_CHARS, 1..5)
        .prop_map(|name| format!("scripts/{name}.ts"))
        .boxed()
}

/// One input line plus the file reference that the line plants, if any.
#[derive(Debug, Clone)]
struct Line {
    text: String,
    planted: Option<String>,
}

fn dockerfile_line() -> BoxedStrategy<Line> {
    prop_oneof![
        3 => noise_line().prop_map(|text| Line { text, planted: None }),
        1 => (joined(INDENT, 0..3), prop::sample::select(&["RUN", "CMD", "ENTRYPOINT"][..]), planted_file())
            .prop_map(|(indent, keyword, file)| Line {
                text: format!("{indent}{keyword} node {file}"),
                planted: Some(file),
            }),
        1 => (prop::sample::select(&["CMD", "ENTRYPOINT"][..]), planted_file())
            .prop_map(|(keyword, file)| Line {
                text: format!("{keyword} [\"node\", \"{file}\"]"),
                planted: Some(file),
            }),
    ]
    .boxed()
}

fn procfile_line() -> BoxedStrategy<Line> {
    prop_oneof![
        3 => noise_line().prop_map(|text| Line { text, planted: None }),
        1 => (joined(NAME_CHARS, 1..4), joined(INDENT, 0..3), planted_file())
            .prop_map(|(process, indent, file)| Line {
                text: format!("{indent}{process}: node {file}"),
                planted: Some(file),
            }),
    ]
    .boxed()
}

fn fly_toml_line() -> BoxedStrategy<Line> {
    prop_oneof![
        3 => noise_line().prop_map(|text| Line { text, planted: None }),
        1 => (prop::sample::select(&["release_command", "cmd"][..]), joined(INDENT, 0..3), planted_file())
            .prop_map(|(key, indent, file)| Line {
                text: format!("{indent}{key} = \"node {file}\""),
                planted: Some(file),
            }),
    ]
    .boxed()
}

/// The planted references. No generated line ends with a backslash, so no
/// line joins the next one, and each planted command stays a command.
fn expected_refs(lines: &[Line]) -> Vec<&str> {
    lines
        .iter()
        .filter_map(|line| line.planted.as_deref())
        .collect()
}

fn content(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn check(lines: &[Line], refs: &[String]) -> Result<(), TestCaseError> {
    for expected in expected_refs(lines) {
        prop_assert!(
            refs.iter().any(|found| found == expected),
            "the parser lost the planted reference {expected:?}; found {refs:?}"
        );
    }
    Ok(())
}

fn dockerfile_refs(content: &str) -> Vec<String> {
    extract_dockerfile_file_refs(content, CommandRefContext::NONE)
}

fn procfile_refs(content: &str) -> Vec<String> {
    extract_procfile_file_refs(content, CommandRefContext::NONE)
}

fn fly_toml_refs(content: &str) -> Vec<String> {
    extract_fly_toml_file_refs(content, CommandRefContext::NONE)
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn dockerfile_parser_keeps_planted_refs(lines in prop::collection::vec(dockerfile_line(), 1..8)) {
        check(&lines, &dockerfile_refs(&content(&lines)))?;
    }

    #[test]
    fn procfile_parser_keeps_planted_refs(lines in prop::collection::vec(procfile_line(), 1..8)) {
        check(&lines, &procfile_refs(&content(&lines)))?;
    }

    #[test]
    fn fly_toml_parser_keeps_planted_refs(lines in prop::collection::vec(fly_toml_line(), 1..8)) {
        check(&lines, &fly_toml_refs(&content(&lines)))?;
    }
}

/// Issue #2896: a multi-byte character across the length of `RUN ` split a
/// UTF-8 boundary.
#[test]
fn dockerfile_keyword_cut_inside_a_character() {
    for line in [
        "RU\u{3042}",
        "CM\u{e9}",
        "ENTRYPOIN\u{1f600}",
        "\u{3042}\u{3044}",
    ] {
        assert!(dockerfile_refs(line).is_empty(), "{line:?}");
    }
}
