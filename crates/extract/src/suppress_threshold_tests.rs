//! Parser tests for `fallow-override-threshold-next-line`.

use super::*;
use fallow_types::inline_threshold::InlineThresholdProblemKind;

fn parse_oxc(source: &str) -> ParsedSuppressions {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, source, SourceType::mjs()).parse();
    parse_suppressions(&parser_return.program.comments, source)
}

/// Both parsers, so each test covers the AST path and the line scanner.
fn both(source: &str) -> [ParsedSuppressions; 2] {
    [parse_oxc(source), parse_suppressions_from_source(source)]
}

#[test]
fn line_comment_sets_the_ceilings_of_the_next_line() {
    let source = "// fallow-override-threshold-next-line maxCognitive=40 maxCrap=45 -- legacy parser\nfunction parse() {}\n";
    for parsed in both(source) {
        assert!(parsed.suppressions.is_empty());
        assert!(parsed.inline_thresholds.problems.is_empty());
        let [entry] = parsed.inline_thresholds.entries.as_slice() else {
            panic!("one entry expected: {:?}", parsed.inline_thresholds);
        };
        assert_eq!(entry.comment_line, 1);
        assert_eq!(entry.line, 2);
        assert_eq!(entry.max_cognitive, Some(40));
        assert_eq!(entry.max_crap, Some(45.0));
        assert_eq!(entry.max_cyclomatic, None);
        assert_eq!(entry.reason, "legacy parser");
    }
}

#[test]
fn block_comment_applies_after_the_comment_ends() {
    let source = "/**\n * Parser.\n * fallow-override-threshold-next-line maxCyclomatic=30 -- generated table\n */\nfunction parse() {}\n";
    for parsed in both(source) {
        let [entry] = parsed.inline_thresholds.entries.as_slice() else {
            panic!("one entry expected: {:?}", parsed.inline_thresholds);
        };
        assert_eq!(entry.comment_line, 3);
        assert_eq!(entry.line, 5);
        assert_eq!(entry.max_cyclomatic, Some(30));
    }
}

#[test]
fn prose_line_in_a_block_comment_is_not_a_marker() {
    let source = "/**\n * fallow-override-threshold-next-line is a comment form.\n */\nfunction parse() {}\n";
    for parsed in both(source) {
        assert!(
            parsed.inline_thresholds.is_empty(),
            "{:?}",
            parsed.inline_thresholds
        );
    }
}

#[test]
fn comment_without_reason_is_a_problem() {
    let source = "// fallow-override-threshold-next-line maxCognitive=40\nfunction parse() {}\n";
    for parsed in both(source) {
        assert!(parsed.inline_thresholds.entries.is_empty());
        let [problem] = parsed.inline_thresholds.problems.as_slice() else {
            panic!("one problem expected: {:?}", parsed.inline_thresholds);
        };
        assert_eq!(problem.line, 1);
        assert_eq!(problem.kind, InlineThresholdProblemKind::MissingReason);
    }
}

#[test]
fn colon_syntax_is_an_invalid_key() {
    let source = "// fallow-override-threshold-next-line maxCrap:45 -- why\nfunction parse() {}\n";
    for parsed in both(source) {
        let [problem] = parsed.inline_thresholds.problems.as_slice() else {
            panic!("one problem expected: {:?}", parsed.inline_thresholds);
        };
        assert_eq!(problem.kind, InlineThresholdProblemKind::InvalidKey);
        assert_eq!(problem.token.as_deref(), Some("maxCrap:45"));
    }
}

#[test]
fn marker_word_needs_a_word_boundary() {
    let source =
        "// fallow-override-threshold-next-lines maxCognitive=40 -- why\nfunction parse() {}\n";
    for parsed in both(source) {
        assert!(parsed.inline_thresholds.is_empty());
    }
}

#[test]
fn marker_in_a_template_literal_is_dropped() {
    let source = "const s = `\n// fallow-override-threshold-next-line maxCognitive=40 -- why\n`;\nfunction parse() {}\n";
    let mut parsed = scan_suppressions_from_source(source);
    assert_eq!(parsed.inline_thresholds.entries.len(), 1);
    let start = u32::try_from(source.find('`').unwrap()).unwrap();
    let end = u32::try_from(source.rfind('`').unwrap()).unwrap();
    parsed.drop_markers_in_literals(&[(start, end)]);
    assert!(parsed.inline_thresholds.is_empty());
}
