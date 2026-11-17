//! Parser and pairing tests for `fallow-ignore-start` / `fallow-ignore-end`.

use super::*;
use fallow_types::suppress::SuppressionScope;

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

fn problems(parsed: &ParsedSuppressions) -> Vec<(u32, RangeMarkerProblemKind)> {
    parsed
        .range_problems
        .iter()
        .map(|problem| (problem.line, problem.kind))
        .collect()
}

fn ranges(parsed: &ParsedSuppressions) -> Vec<(u32, u32, u32)> {
    parsed
        .suppressions
        .iter()
        .filter_map(|s| s.range_end_line().map(|end| (s.comment_line, s.line, end)))
        .collect()
}

#[test]
fn start_and_end_pair_into_one_range() {
    let source = "// fallow-ignore-start code-duplication -- kept\nconst a = 1;\nconst b = 2;\n// fallow-ignore-end code-duplication\n";
    for parsed in both(source) {
        assert_eq!(ranges(&parsed), vec![(1, 2, 4)]);
        assert!(problems(&parsed).is_empty());
        let range = &parsed.suppressions[0];
        assert_eq!(range.reason.as_deref(), Some("kept"));
        assert_eq!(range.issue_kind_target(), Some(IssueKind::CodeDuplication));
        assert_eq!(range.scope, SuppressionScope::Range { end_line: 4 });
    }
}

#[test]
fn range_never_matches_the_line_predicates() {
    let source = "// fallow-ignore-start code-duplication\nconst a = 1;\n// fallow-ignore-end code-duplication\n";
    let parsed = parse_suppressions_from_source(source);
    let range = &parsed.suppressions[0];
    assert!(range.range_covers_line(2));
    assert!(!range.range_covers_line(3));
    assert!(!range.applies_to_line(2));
    assert!(!fallow_types::suppress::is_suppressed(
        &parsed.suppressions,
        2,
        IssueKind::CodeDuplication
    ));
    assert!(!fallow_types::suppress::is_file_suppressed(
        &parsed.suppressions,
        IssueKind::CodeDuplication
    ));
}

#[test]
fn block_and_html_comment_markers() {
    let block = "/* fallow-ignore-start code-duplication */\nconst a = 1;\n/** fallow-ignore-end code-duplication */\n";
    for parsed in both(block) {
        assert_eq!(ranges(&parsed), vec![(1, 2, 3)]);
    }
    let multi = "/**\n * fallow-ignore-start code-duplication -- kept\n */\nconst a = 1;\n// fallow-ignore-end code-duplication\n";
    for parsed in both(multi) {
        assert_eq!(ranges(&parsed), vec![(2, 4, 5)]);
    }
    let html = "<template>\n  <!-- fallow-ignore-start code-duplication -->\n  <p>a</p>\n  <!-- fallow-ignore-end code-duplication -->\n</template>\n";
    let parsed = parse_suppressions_from_source(html);
    assert_eq!(ranges(&parsed), vec![(2, 3, 4)]);
}

#[test]
fn marker_word_must_end_at_whitespace() {
    let source = "// fallow-ignore-starter code-duplication\nconst a = 1;\n";
    for parsed in both(source) {
        assert!(parsed.range_markers.is_empty());
        assert!(problems(&parsed).is_empty());
        assert!(parsed.suppressions.is_empty());
    }
}

#[test]
fn malformed_pairs_are_problems() {
    let source = "// fallow-ignore-end code-duplication\n// fallow-ignore-start code-duplication\n// fallow-ignore-start code-duplication\nconst a = 1;\n// fallow-ignore-end code-duplication\n// fallow-ignore-start code-duplication\nconst b = 1;\n";
    for parsed in both(source) {
        assert_eq!(ranges(&parsed), vec![(2, 3, 5)]);
        assert_eq!(
            problems(&parsed),
            vec![
                (1, RangeMarkerProblemKind::UnmatchedEnd),
                (3, RangeMarkerProblemKind::NestedStart),
                (6, RangeMarkerProblemKind::UnclosedStart),
            ]
        );
    }
}

#[test]
fn kind_problems_and_mixed_lists() {
    let source = "// fallow-ignore-start\n// fallow-ignore-start unused-export\n// fallow-ignore-start code-duplication unused-exprt\nconst a = 1;\n// fallow-ignore-end code-duplication\n";
    for parsed in both(source) {
        assert_eq!(ranges(&parsed), vec![(3, 4, 5)]);
        let tokens: Vec<(u32, RangeMarkerProblemKind, Option<&str>)> = parsed
            .range_problems
            .iter()
            .map(|p| (p.line, p.kind, p.token.as_deref()))
            .collect();
        assert_eq!(
            tokens,
            vec![
                (1, RangeMarkerProblemKind::MissingKind, None),
                (
                    2,
                    RangeMarkerProblemKind::UnsupportedKind,
                    Some("unused-export")
                ),
                (
                    3,
                    RangeMarkerProblemKind::UnsupportedKind,
                    Some("unused-exprt")
                ),
            ]
        );
        assert!(
            parsed.unknown_kinds.is_empty(),
            "range markers never give unknown-kind stale findings"
        );
    }
}

#[test]
fn scan_keeps_markers_unpaired_with_offsets() {
    let source = "const x = 1;\n  // fallow-ignore-start code-duplication\n";
    let parsed = scan_suppressions_from_source(source);
    assert_eq!(parsed.range_markers.len(), 1);
    let offset = parsed.range_markers[0].offset as usize;
    assert!(source[..offset].ends_with("// fallow-ignore-start "));
    assert!(parsed.range_problems.is_empty());
}

#[test]
fn range_marker_that_shares_its_line_with_code_is_reported() {
    let sources = [
        "/* fallow-ignore-start code-duplication */ const zz = 1;\nconst a = 1;\n// fallow-ignore-end code-duplication\n",
        "const zz = 1; // fallow-ignore-start code-duplication\nconst a = 1;\n// fallow-ignore-end code-duplication\n",
    ];
    for source in sources {
        for parsed in both(source) {
            assert!(ranges(&parsed).is_empty(), "{source:?}");
            assert_eq!(
                problems(&parsed),
                vec![
                    (1, RangeMarkerProblemKind::NotOwnLine),
                    (3, RangeMarkerProblemKind::UnmatchedEnd),
                ],
                "{source:?}"
            );
        }
    }
}

#[test]
fn html_range_marker_that_shares_its_line_with_markup_is_reported() {
    let source = "<template>\n  <!-- fallow-ignore-start code-duplication --> <p>a</p>\n  <!-- fallow-ignore-end code-duplication -->\n</template>\n";
    let parsed = parse_suppressions_from_source(source);
    assert!(ranges(&parsed).is_empty());
    assert_eq!(
        problems(&parsed),
        vec![
            (2, RangeMarkerProblemKind::NotOwnLine),
            (3, RangeMarkerProblemKind::UnmatchedEnd),
        ]
    );
}

#[test]
fn unmatched_end_message_names_the_own_line_rule() {
    assert!(
        RangeMarkerProblemKind::UnmatchedEnd
            .description()
            .contains("own line")
    );
}
