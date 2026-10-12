//! Line-level detectors over the base and head text of one changed file.
//!
//! The detectors are syntactic. They compare token counts and added lines,
//! and they do not parse the file. A line counts as added when its trimmed
//! text occurs more often in head than in base, so a moved line is not added.

use rustc_hash::FxHashMap;

/// One detector hit on a head line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineHit {
    /// 1-based head line.
    pub line: u32,
    /// The token or value change that supports the hit.
    pub evidence: String,
}

/// Roots of a test-case call: `it(...)`, `test.skip(...)`.
const TEST_CASE_ROOTS: &[&str] = &["it", "test", "specify", "xit", "xtest", "fit"];
/// Roots of a suite call that can carry a skip or focus marker.
const SUITE_ROOTS: &[&str] = &["describe", "suite", "context"];
/// Roots that skip or focus by name alone.
const PREFIXED_ROOTS: &[&str] = &["xit", "xtest", "xdescribe", "fit", "fdescribe"];
/// Members that keep a call a test case (`it.each`, `test.concurrent`).
const TEST_CASE_MEMBERS: &[&str] = &[
    "skip",
    "only",
    "each",
    "todo",
    "concurrent",
    "sequential",
    "fails",
    "failing",
    "fixme",
    "skipIf",
    "runIf",
];
/// Members of `expect` that are an assertion (`expect.soft(x)`).
const EXPECT_ASSERTION_MEMBERS: &[&str] = &["soft", "poll"];

/// Comment tokens that turn off a check. `fallow-ignore` has its own rule.
const SUPPRESSION_TOKENS: &[&str] = &[
    "eslint-disable",
    "oxlint-disable",
    "biome-ignore",
    "deno-lint-ignore",
    "@ts-ignore",
    "@ts-expect-error",
    "@ts-nocheck",
    "istanbul ignore",
    "c8 ignore",
    "v8 ignore",
];
const FALLOW_IGNORE_TOKEN: &str = "fallow-ignore";
/// Suppression tokens of the diff-risk rules. A `fallow-ignore` comment that
/// names only these is the suppression the findings ask for, so it is not an
/// added suppression itself.
const DIFF_RISK_TOKENS: &[&str] = &[
    "weakened-tests",
    "added-suppression",
    "test-only-branch",
    "diverged-clone-sibling",
    "added-dependency",
];

/// Threshold keys where a lower value is less strict.
const MIN_THRESHOLD_KEYS: &[&str] = &[
    "branches",
    "functions",
    "lines",
    "statements",
    "minScore",
    "min-score",
    "minCoverage",
    "min-coverage",
];
/// Threshold keys where a higher value is less strict.
const MAX_THRESHOLD_KEYS: &[&str] = &[
    "maxCrap",
    "max-crap",
    "maxCyclomatic",
    "max-cyclomatic",
    "maxCognitive",
    "max-cognitive",
];

/// Lines of `head` that do not occur in `base`, as `(1-based line, text)`.
///
/// Lines compare by trimmed text as a multiset. When head has more copies of
/// a line than base, the last copies count as added. Blank lines never count.
#[must_use]
pub fn added_lines<'a>(base: &str, head: &'a str) -> Vec<(u32, &'a str)> {
    let mut remaining: FxHashMap<&str, usize> = FxHashMap::default();
    for line in base.lines() {
        *remaining.entry(line.trim()).or_default() += 1;
    }
    let mut head_counts: FxHashMap<&str, usize> = FxHashMap::default();
    for line in head.lines() {
        *head_counts.entry(line.trim()).or_default() += 1;
    }
    // The number of leading copies of each text that base already holds.
    let mut kept: FxHashMap<&str, usize> = head_counts
        .iter()
        .map(|(text, count)| {
            let in_base = remaining.get(text).copied().unwrap_or(0);
            (*text, (*count).min(in_base))
        })
        .collect();
    let mut added = Vec::new();
    for (index, line) in head.lines().enumerate() {
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let keep = kept.entry(text).or_default();
        if *keep > 0 {
            *keep -= 1;
            continue;
        }
        added.push((line_number(index), line));
    }
    added
}

/// Added lines of a test file that add a skip or focus marker.
#[must_use]
pub fn skip_only_hits(base: &str, head: &str) -> Vec<LineHit> {
    let mut hits = Vec::new();
    for (line, text) in added_lines(base, head) {
        for chain in call_chains(code_part(text)) {
            if is_skip_or_focus(&chain) {
                hits.push(LineHit {
                    line,
                    evidence: chain.join("."),
                });
                break;
            }
        }
    }
    hits
}

/// The number of test-case calls in a test file, comments excluded.
#[must_use]
pub fn test_case_count(src: &str) -> usize {
    src.lines()
        .flat_map(|line| call_chains(code_part(line)))
        .filter(|chain| is_test_case(chain))
        .count()
}

/// The number of assertion calls (`expect(`, `assert`, `should`) in a test
/// file, comments excluded.
#[must_use]
pub fn assertion_count(src: &str) -> usize {
    src.lines()
        .map(|line| {
            let code = code_part(line);
            let chains = call_chains(code)
                .into_iter()
                .filter(|chain| is_assertion(chain))
                .count();
            chains + member_should_count(code)
        })
        .sum()
}

/// Added comment lines that add a suppression.
#[must_use]
pub fn added_suppression_hits(base: &str, head: &str) -> Vec<LineHit> {
    added_lines(base, head)
        .into_iter()
        .filter_map(|(line, text)| {
            let comment = comment_part(text)?;
            suppression_token(comment).map(|token| LineHit {
                line,
                evidence: token.to_owned(),
            })
        })
        .collect()
}

/// Added lines of a non-test source file that branch on the test
/// environment.
#[must_use]
pub fn test_only_branch_hits(base: &str, head: &str) -> Vec<LineHit> {
    added_lines(base, head)
        .into_iter()
        .filter_map(|(line, text)| {
            test_environment_token(code_part(text)).map(|token| LineHit {
                line,
                evidence: token.to_owned(),
            })
        })
        .collect()
}

/// Threshold values in a coverage or quality config that the change made
/// less strict. Occurrences of one key pair up in file order.
#[must_use]
pub fn weakened_threshold_hits(base: &str, head: &str) -> Vec<LineHit> {
    let mut hits = Vec::new();
    for (keys, lower_is_weaker) in [(MIN_THRESHOLD_KEYS, true), (MAX_THRESHOLD_KEYS, false)] {
        for key in keys {
            let base_values = numeric_values_for_key(base, key);
            let head_values = numeric_values_for_key(head, key);
            for ((base_value, _), (head_value, head_line)) in
                base_values.iter().zip(head_values.iter())
            {
                let weaker = if lower_is_weaker {
                    head_value < base_value
                } else {
                    head_value > base_value
                };
                if weaker {
                    hits.push(LineHit {
                        line: *head_line,
                        evidence: format!("{key}: {base_value} -> {head_value}"),
                    });
                }
            }
        }
    }
    hits.sort_by_key(|hit| hit.line);
    hits
}

/// Whether `rel_path` names a test-runner, coverage or Fallow config file.
#[must_use]
pub fn is_coverage_config_path(rel_path: &str) -> bool {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    if rel_path.contains("node_modules/") {
        return false;
    }
    const EXACT: &[&str] = &[
        "package.json",
        ".nycrc",
        ".nycrc.json",
        ".nycrc.yml",
        ".nycrc.yaml",
        ".c8rc",
        ".c8rc.json",
        ".fallowrc.json",
        ".fallowrc.jsonc",
        "fallow.toml",
        ".fallow.toml",
    ];
    const PREFIXES: &[&str] = &[
        "vitest.config.",
        "vitest.workspace.",
        "vite.config.",
        "jest.config.",
        "karma.conf.",
        "playwright.config.",
    ];
    EXACT.contains(&name) || PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

fn line_number(index: usize) -> u32 {
    u32::try_from(index + 1).unwrap_or(u32::MAX)
}

/// The code of a line before any comment. A line that starts inside a
/// comment has no code.
fn code_part(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("<!--")
    {
        return "";
    }
    match comment_start(line) {
        Some(start) => &line[..start],
        None => line,
    }
}

/// The comment of a line, from its first comment marker.
fn comment_part(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('*') || trimmed.starts_with('#') {
        return Some(trimmed);
    }
    comment_start(line).map(|start| &line[start..])
}

/// The byte offset of the first `//`, `/*` or `<!--` marker. A `//` right
/// after `:` is a URL scheme, not a comment.
fn comment_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        let pair = (bytes[index], bytes[index + 1]);
        let is_line_comment = pair == (b'/', b'/') && (index == 0 || bytes[index - 1] != b':');
        if is_line_comment || pair == (b'/', b'*') || bytes[index..].starts_with(b"<!--") {
            return Some(index);
        }
        index += 1;
    }
    None
}

const fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// Every member chain in `code` that is called: `it.skip(` gives
/// `["it", "skip"]`. A chain that follows a `.` is a member of another
/// expression, so it is not a chain root.
fn call_chains(code: &str) -> Vec<Vec<&str>> {
    let bytes = code.as_bytes();
    let mut chains = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let starts_ident = is_ident_byte(bytes[index])
            && !bytes[index].is_ascii_digit()
            && (index == 0 || (!is_ident_byte(bytes[index - 1]) && bytes[index - 1] != b'.'));
        if !starts_ident {
            index += 1;
            continue;
        }
        let mut chain = Vec::new();
        let mut cursor = index;
        loop {
            let start = cursor;
            while cursor < bytes.len() && is_ident_byte(bytes[cursor]) {
                cursor += 1;
            }
            chain.push(&code[start..cursor]);
            if cursor + 1 < bytes.len() && bytes[cursor] == b'.' && is_ident_byte(bytes[cursor + 1])
            {
                cursor += 1;
                continue;
            }
            break;
        }
        let next = code[cursor..].trim_start().bytes().next();
        if matches!(next, Some(b'(' | b'`')) {
            chains.push(chain);
        }
        index = cursor.max(index + 1);
    }
    chains
}

fn is_skip_or_focus(chain: &[&str]) -> bool {
    let Some(root) = chain.first() else {
        return false;
    };
    if PREFIXED_ROOTS.contains(root) {
        return true;
    }
    (TEST_CASE_ROOTS.contains(root) || SUITE_ROOTS.contains(root))
        && chain[1..]
            .iter()
            .any(|member| matches!(*member, "skip" | "only"))
}

fn is_test_case(chain: &[&str]) -> bool {
    chain
        .first()
        .is_some_and(|root| TEST_CASE_ROOTS.contains(root))
        && chain[1..]
            .iter()
            .all(|member| TEST_CASE_MEMBERS.contains(member))
}

fn is_assertion(chain: &[&str]) -> bool {
    match chain {
        ["expect"] | ["should" | "assert", ..] => true,
        ["expect", member] => EXPECT_ASSERTION_MEMBERS.contains(member),
        _ => false,
    }
}

/// `x.should.equal(1)` style assertions: `.should` followed by `.` or `(`.
fn member_should_count(code: &str) -> usize {
    code.match_indices(".should")
        .filter(|(start, token)| {
            let after = code.as_bytes().get(start + token.len()).copied();
            matches!(after, Some(b'.' | b'('))
        })
        .count()
}

/// The first suppression token in a comment, or `None`. A `fallow-ignore`
/// marker that names only diff-risk rules is not a suppression here.
fn suppression_token(comment: &str) -> Option<&'static str> {
    let generic = SUPPRESSION_TOKENS
        .iter()
        .filter_map(|token| comment.find(token).map(|at| (at, *token)))
        .min_by_key(|(at, _)| *at);
    let fallow = comment
        .find(FALLOW_IGNORE_TOKEN)
        .filter(|at| !names_only_diff_risk_rules(&comment[*at..]))
        .map(|at| (at, FALLOW_IGNORE_TOKEN));
    [generic, fallow]
        .into_iter()
        .flatten()
        .min_by_key(|(at, _)| *at)
        .map(|(_, token)| token)
}

/// Whether a `fallow-ignore-*` directive lists only diff-risk tokens. A
/// directive without tokens suppresses every rule, so it does not qualify.
fn names_only_diff_risk_rules(directive: &str) -> bool {
    let body = directive
        .split_once("--")
        .map_or(directive, |(tokens, _reason)| tokens);
    let body = body
        .trim_end()
        .trim_end_matches("*/")
        .trim_end_matches("-->");
    let mut words = body.split_whitespace();
    // The first word is the directive itself (`fallow-ignore-next-line`).
    words.next();
    let tokens: Vec<&str> = words
        .flat_map(|word| word.split(','))
        .filter(|token| !token.is_empty())
        .collect();
    !tokens.is_empty() && tokens.iter().all(|token| DIFF_RISK_TOKENS.contains(token))
}

/// The test-environment token in a line of code, or `None`.
fn test_environment_token(code: &str) -> Option<&'static str> {
    // The tokens count only outside string literals: a parser that compares
    // source text with `'import.meta.vitest'` is not a test-only branch.
    let outside = blank_string_literals(code);
    for token in [
        "import.meta.vitest",
        "process.env.VITEST",
        "process.env.JEST_WORKER_ID",
    ] {
        if outside.contains(token) {
            return Some(token);
        }
    }
    let compares_test = ["'test'", "\"test\"", "`test`"]
        .iter()
        .any(|literal| code.contains(literal))
        && (outside.contains("==") || outside.contains("!="));
    (outside.contains("process.env.NODE_ENV") && compares_test).then_some("process.env.NODE_ENV")
}

/// `code` with the content of each string literal on the line replaced by
/// spaces. The quotes stay. A backslash escapes the next character.
fn blank_string_literals(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in code.chars() {
        let Some(open) = quote else {
            if matches!(c, '\'' | '"' | '`') {
                quote = Some(c);
            }
            out.push(c);
            continue;
        };
        if escaped {
            escaped = false;
            out.push(' ');
        } else if c == '\\' {
            escaped = true;
            out.push(' ');
        } else if c == open {
            quote = None;
            out.push(c);
        } else {
            out.push(' ');
        }
    }
    out
}

/// Every numeric value that follows `key` as an object key, with its 1-based
/// line. The key must stand alone: `lines` does not match `maxLines`.
fn numeric_values_for_key(src: &str, key: &str) -> Vec<(f64, u32)> {
    let mut values = Vec::new();
    let mut line = 1u32;
    let mut scanned = 0;
    let mut search_from = 0;
    while let Some(relative) = src[search_from..].find(key) {
        let start = search_from + relative;
        let end = start + key.len();
        search_from = end;
        let before = src[..start].bytes().next_back();
        let after = src.as_bytes().get(end).copied();
        let stands_alone = !before.is_some_and(|byte| is_ident_byte(byte) || byte == b'-')
            && !after.is_some_and(|byte| is_ident_byte(byte) || byte == b'-');
        if !stands_alone {
            continue;
        }
        let rest = src[end..]
            .trim_start_matches(['"', '\''])
            .trim_start_matches([' ', '\t']);
        let Some(rest) = rest.strip_prefix([':', '=']) else {
            continue;
        };
        let rest = rest.trim_start_matches([' ', '\t', '"', '\'']);
        let number: String = rest
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
            .collect();
        let Ok(value) = number.parse::<f64>() else {
            continue;
        };
        line += u32::try_from(src[scanned..start].matches('\n').count()).unwrap_or(0);
        scanned = start;
        values.push((value, line));
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_lines_ignore_moved_lines() {
        let base = "a\nb\nc\n";
        let head = "c\na\nb\nd\n";
        assert_eq!(added_lines(base, head), vec![(4, "d")]);
    }

    #[test]
    fn added_skip_and_only_markers_are_hits_at_their_line() {
        let base = "it('a', () => {});\ntest('b', () => {});\n";
        let head = "it.skip('a', () => {});\ntest.only('b', () => {});\nxit('c', f);\n";
        let hits = skip_only_hits(base, head);
        let evidence: Vec<(u32, &str)> = hits
            .iter()
            .map(|hit| (hit.line, hit.evidence.as_str()))
            .collect();
        assert_eq!(evidence, vec![(1, "it.skip"), (2, "test.only"), (3, "xit")]);
    }

    #[test]
    fn existing_or_commented_skip_markers_are_not_hits() {
        let base = "it.skip('a', () => {});\n";
        let head = "// it.only('x')\nit.skip('a', () => {});\nconst profit = fit2(1);\n";
        assert!(skip_only_hits(base, head).is_empty());
    }

    #[test]
    fn test_case_count_counts_cases_with_modifiers_only() {
        let src = "describe('s', () => {\n  it('a', f);\n  it.skip('b', f);\n  test.each([1])('c', f);\n  test.describe('d', f);\n  submit(1);\n  // it('commented', f);\n});\n";
        assert_eq!(test_case_count(src), 3);
    }

    #[test]
    fn assertion_count_counts_expect_assert_and_should() {
        let src = "expect(a).toBe(1);\nexpect.soft(b).toBe(2);\nassert.equal(c, 3);\nassert(d);\nx.should.equal(4);\nexpect.assertions(2);\n// expect(e)\n";
        assert_eq!(assertion_count(src), 5);
    }

    #[test]
    fn added_suppression_comments_are_hits() {
        let base = "const a = 1;\n// eslint-disable-next-line no-console\nconsole.log(a);\n";
        let head = "// @ts-ignore\nconst a = 1;\n// eslint-disable-next-line no-console\nconsole.log(a);\n/* istanbul ignore next */\nconst s = 'eslint-disable';\n";
        let hits = added_suppression_hits(base, head);
        let evidence: Vec<(u32, &str)> = hits
            .iter()
            .map(|hit| (hit.line, hit.evidence.as_str()))
            .collect();
        assert_eq!(evidence, vec![(1, "@ts-ignore"), (5, "istanbul ignore")]);
    }

    #[test]
    fn fallow_ignore_that_names_only_diff_risk_rules_is_not_an_added_suppression() {
        let head = "// fallow-ignore-next-line weakened-tests\nit.skip('a', f);\n// fallow-ignore-next-line unused-export\nexport const b = 1;\n// fallow-ignore-file\n";
        let hits = added_suppression_hits("", head);
        let lines: Vec<u32> = hits.iter().map(|hit| hit.line).collect();
        assert_eq!(lines, vec![3, 5]);
    }

    #[test]
    fn multibyte_characters_before_a_comment_do_not_panic() {
        let head = "const name = 'café'; // @ts-ignore\nconst ünï = 1;\n";
        let hits = added_suppression_hits("", head);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 1);
    }

    #[test]
    fn added_test_environment_branches_are_hits() {
        let head = "if (process.env.NODE_ENV === 'test') return;\nif (process.env.NODE_ENV === 'production') x();\nconst w = process.env.JEST_WORKER_ID;\nif (process.env.VITEST) y();\nif (import.meta.vitest) z();\n// process.env.VITEST\n";
        let lines: Vec<u32> = test_only_branch_hits("", head)
            .iter()
            .map(|hit| hit.line)
            .collect();
        assert_eq!(lines, vec![1, 3, 4, 5]);
    }

    #[test]
    fn test_environment_tokens_inside_strings_are_not_hits() {
        let head = "return code.slice(a, b) === 'import.meta.vitest'\nconst m = `inside the \\`if (import.meta.vitest)\\` block`;\nconst s = \"process.env.VITEST\";\n";
        assert!(test_only_branch_hits("", head).is_empty());
    }

    #[test]
    fn lowered_min_threshold_and_raised_max_threshold_are_hits() {
        let base = "thresholds: {\n  lines: 90,\n  branches: 80,\n},\nmaxCrap: 30,\n";
        let head = "thresholds: {\n  lines: 70,\n  branches: 85,\n},\nmaxCrap: 45,\n";
        let hits = weakened_threshold_hits(base, head);
        let evidence: Vec<(u32, &str)> = hits
            .iter()
            .map(|hit| (hit.line, hit.evidence.as_str()))
            .collect();
        assert_eq!(
            evidence,
            vec![(2, "lines: 90 -> 70"), (5, "maxCrap: 30 -> 45")]
        );
    }

    #[test]
    fn json_thresholds_are_compared_by_key() {
        let base = r#"{"jest": {"coverageThreshold": {"global": {"statements": 95}}}}"#;
        let head = r#"{"jest": {"coverageThreshold": {"global": {"statements": 50}}}}"#;
        let hits = weakened_threshold_hits(base, head);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].evidence, "statements: 95 -> 50");
    }

    #[test]
    fn coverage_config_paths() {
        assert!(is_coverage_config_path("vitest.config.ts"));
        assert!(is_coverage_config_path("packages/a/jest.config.js"));
        assert!(is_coverage_config_path("package.json"));
        assert!(is_coverage_config_path(".nycrc.json"));
        assert!(is_coverage_config_path(".fallowrc.json"));
        assert!(!is_coverage_config_path("src/lines.ts"));
    }
}
