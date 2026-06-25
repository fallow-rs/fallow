//! Weakening-signal pass (6.F), PROMOTED TO A HEADLINE per v6.
//!
//! A diff-scoped base-vs-head pass over the changed files that flags the
//! AI-era failure modes a green check hides: tests removed or skipped, coverage
//! / thresholds lowered, suppressions added, security checks/steps removed.
//!
//! Always ADVISORY, reviewer-private, NEVER gates, NEVER auto-posted. It rides
//! the brief envelope (exit-0 by construction) as a headline section.
//!
//! Honest scope (ADR-001, syntactic): these are line-shape heuristics over the
//! base-vs-head text of changed files, NOT a semantic test-coverage proof. A
//! signal is an attention pointer ("the diff weakened a guardrail here"), framed
//! so a reviewer decides.

pub use fallow_output::{WeakeningKind, WeakeningSignal};

/// Detect skipped-test additions: an `it.skip` / `xit` / `describe.skip` /
/// `.only` token present in head but not base. `.only` narrows the run to a
/// subset, silently excluding siblings, so it counts as a weakening too.
#[must_use]
pub fn detect_test_weakening(base: &str, head: &str) -> Vec<String> {
    const SKIP_TOKENS: &[&str] = &[
        "it.skip",
        "test.skip",
        "describe.skip",
        "xit(",
        "xdescribe(",
        "it.only",
        "test.only",
        "describe.only",
    ];
    let mut signals = Vec::new();
    for token in SKIP_TOKENS {
        let head_count = count_token(head, token);
        let base_count = count_token(base, token);
        if head_count > base_count {
            signals.push((*token).to_string());
        }
    }
    signals
}

/// Detect net-removed tests: a `it(` / `test(` / `describe(` callsite count that
/// dropped between base and head (a test deleted).
#[must_use]
pub fn detect_removed_tests(base: &str, head: &str) -> Vec<String> {
    const TEST_TOKENS: &[&str] = &["it(", "test(", "describe("];
    let mut signals = Vec::new();
    for token in TEST_TOKENS {
        let base_count = count_token(base, token);
        let head_count = count_token(head, token);
        if base_count > head_count {
            signals.push(format!("{token} removed ({base_count} -> {head_count})"));
        }
    }
    signals
}

/// Detect added suppressions: a `fallow-ignore` / `eslint-disable` / `@ts-ignore`
/// / `@ts-expect-error` count that increased between base and head. Only counts
/// occurrences on a COMMENT line (a line containing `//`, `#`, `/*`, `*`, or
/// `<!--`), since a real suppression directive always lives in a comment. This
/// keeps the token list's own definition (e.g. the string array in this file)
/// from self-flagging, the only false-positive class the real-world smoke hit.
#[must_use]
pub fn detect_added_suppressions(base: &str, head: &str) -> Vec<String> {
    const SUPPRESS_TOKENS: &[&str] = &[
        "fallow-ignore",
        "eslint-disable",
        "@ts-ignore",
        "@ts-expect-error",
        "biome-ignore",
        "prettier-ignore",
    ];
    let mut signals = Vec::new();
    for token in SUPPRESS_TOKENS {
        let base_count = count_token_in_comments(base, token);
        let head_count = count_token_in_comments(head, token);
        if head_count > base_count {
            signals.push(format!("{token} added ({base_count} -> {head_count})"));
        }
    }
    signals
}

/// Detect lowered numeric thresholds: a config key whose numeric value decreased
/// between base and head. Scans common coverage/threshold key names; a key
/// present in both with a strictly smaller head value is a weakening.
#[must_use]
pub fn detect_lowered_thresholds(base: &str, head: &str) -> Vec<String> {
    const THRESHOLD_KEYS: &[&str] = &[
        "branches",
        "functions",
        "lines",
        "statements",
        "minScore",
        "min-score",
        "maxCrap",
        "max-crap",
        "minCoverage",
        "min-coverage",
        "coverageThreshold",
        "threshold",
    ];
    let mut signals = Vec::new();
    for key in THRESHOLD_KEYS {
        let (Some(base_val), Some(head_val)) = (
            first_numeric_for_key(base, key),
            first_numeric_for_key(head, key),
        ) else {
            continue;
        };
        if head_val < base_val {
            signals.push(format!("{key}: {base_val} -> {head_val}"));
        }
    }
    signals
}

/// Detect removed security CI steps: a line invoking a security scanner / audit
/// step that was present in base but is gone in head. Scoped to CI files by the
/// caller; here we count net-removed scanner-invoking lines.
#[must_use]
pub fn detect_removed_security_steps(base: &str, head: &str) -> Vec<String> {
    const SECURITY_TOKENS: &[&str] = &[
        "npm audit",
        "yarn audit",
        "pnpm audit",
        "fallow security",
        "codeql",
        "snyk",
        "trivy",
        "semgrep",
        "gitleaks",
        "dependency-review",
        "osv-scanner",
    ];
    let mut signals = Vec::new();
    for token in SECURITY_TOKENS {
        let base_count = count_token(base, token);
        let head_count = count_token(head, token);
        if base_count > head_count {
            signals.push(format!("{token} step removed"));
        }
    }
    signals
}

/// Whether a path looks like a CI workflow file (where security-step removal is
/// meaningful).
#[must_use]
pub fn is_ci_file(rel_path: &str) -> bool {
    rel_path.contains(".github/workflows/")
        || rel_path.ends_with(".gitlab-ci.yml")
        || rel_path.ends_with(".gitlab-ci.yaml")
}

/// Whether a path looks like a test file (where test removal/skip is meaningful).
#[must_use]
pub fn is_test_file(rel_path: &str) -> bool {
    let lower = rel_path.to_ascii_lowercase();
    lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.contains("__tests__/")
        || lower.contains("/tests/")
        || lower.contains("/test/")
        || lower.contains(".cy.")
}

/// Count occurrences of `token` in `src`.
fn count_token(src: &str, token: &str) -> usize {
    src.matches(token).count()
}

/// Count occurrences of `token` only on lines that look like a comment (a real
/// suppression directive is always commented). Sums per-line matches so multiple
/// suppressions on one line still count.
fn count_token_in_comments(src: &str, token: &str) -> usize {
    src.lines()
        .filter(|line| line_is_comment(line))
        .map(|line| line.matches(token).count())
        .sum()
}

/// Whether a line contains a comment marker (`//`, `#`, `/*`, a leading `*`
/// continuation, or `<!--`). Conservative: any of these makes the line eligible.
fn line_is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    line.contains("//")
        || line.contains("/*")
        || line.contains("<!--")
        || trimmed.starts_with('#')
        || trimmed.starts_with('*')
}

/// Find the first numeric value following `"<key>":` or `<key>:` or `<key> =`
/// anywhere in `src`. Returns the parsed `f64`, or `None` when the key is absent
/// or the value is not numeric. Tolerant of JSON/JSONC/TOML/YAML shapes, both
/// inline (`{ "branches": 90 }`) and per-line. Matches the key at a word
/// boundary (preceded by a quote, whitespace, `{`, or `,`) so `branches` does
/// not match a longer key that merely ends in `branches`.
fn first_numeric_for_key(src: &str, key: &str) -> Option<f64> {
    let mut search_from = 0;
    while let Some(rel) = src[search_from..].find(key) {
        let start = search_from + rel;
        search_from = start + key.len();
        // Word-boundary check on the byte before the key.
        let preceded_ok = start == 0
            || src[..start]
                .chars()
                .next_back()
                .is_some_and(|c| matches!(c, '"' | '\'' | ' ' | '\t' | '{' | ',' | '\n'));
        if !preceded_ok {
            continue;
        }
        let after = src[search_from..]
            .trim_start_matches(['"', '\''])
            .trim_start()
            .trim_start_matches([':', '='])
            .trim_start()
            .trim_start_matches(['"', '\'']);
        let num: String = after
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
            .collect();
        if let Ok(value) = num.parse::<f64>() {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injected_it_skip_is_flagged() {
        let base = "it('works', () => { expect(x).toBe(1); });";
        let head = "it.skip('works', () => { expect(x).toBe(1); });";
        let signals = detect_test_weakening(base, head);
        assert!(
            signals.iter().any(|s| s == "it.skip"),
            "it.skip must be flagged: {signals:?}"
        );
    }

    #[test]
    fn only_narrowing_is_flagged() {
        let base = "describe('a', () => {});";
        let head = "describe.only('a', () => {});";
        let signals = detect_test_weakening(base, head);
        assert!(signals.iter().any(|s| s == "describe.only"));
    }

    #[test]
    fn unchanged_tests_produce_no_signal() {
        let src = "it('a', () => {}); it('b', () => {});";
        assert!(detect_test_weakening(src, src).is_empty());
        assert!(detect_removed_tests(src, src).is_empty());
    }

    #[test]
    fn removed_test_is_flagged() {
        let base = "it('a', () => {}); it('b', () => {});";
        let head = "it('a', () => {});";
        let signals = detect_removed_tests(base, head);
        assert_eq!(signals.len(), 1);
        assert!(signals[0].contains("it("));
    }

    #[test]
    fn added_suppression_is_flagged() {
        let base = "const x = 1;";
        let head = "// eslint-disable-next-line\nconst x = 1;";
        let signals = detect_added_suppressions(base, head);
        assert!(signals.iter().any(|s| s.starts_with("eslint-disable")));
    }

    #[test]
    fn lowered_threshold_is_flagged() {
        let base = r#"{ "branches": 90, "lines": 85 }"#;
        let head = r#"{ "branches": 70, "lines": 85 }"#;
        let signals = detect_lowered_thresholds(base, head);
        assert_eq!(signals, vec!["branches: 90 -> 70".to_string()]);
    }

    #[test]
    fn lowered_min_score_is_flagged() {
        let base = "minScore: 80";
        let head = "minScore: 50";
        let signals = detect_lowered_thresholds(base, head);
        assert_eq!(signals, vec!["minScore: 80 -> 50".to_string()]);
    }

    #[test]
    fn raised_threshold_is_not_flagged() {
        let base = r#"{ "branches": 70 }"#;
        let head = r#"{ "branches": 90 }"#;
        assert!(detect_lowered_thresholds(base, head).is_empty());
    }

    #[test]
    fn removed_security_step_is_flagged() {
        let base = "      - run: npm audit --audit-level=high\n      - run: build";
        let head = "      - run: build";
        let signals = detect_removed_security_steps(base, head);
        assert!(signals.iter().any(|s| s.contains("npm audit")));
    }

    #[test]
    fn file_classifiers() {
        assert!(is_ci_file(".github/workflows/ci.yml"));
        assert!(is_ci_file(".gitlab-ci.yml"));
        assert!(!is_ci_file("src/app.ts"));
        assert!(is_test_file("src/app.test.ts"));
        assert!(is_test_file("__tests__/app.ts"));
        assert!(!is_test_file("src/app.ts"));
    }
}
