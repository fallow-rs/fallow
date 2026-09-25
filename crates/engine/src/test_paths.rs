//! Shared classification of test paths.
//!
//! Two sets of paths exist:
//!
//! - Test code holds the tests: specs, test directories and end-to-end suites.
//! - Test support helps the tests: mocks, fixtures and snapshots.
//!
//! [`is_test_code_path`] matches test code only. Use it when the question is
//! "does a test exist" or "did a test change": audit test adjacency, audit
//! test-weakening signals and similar-code related tests.
//!
//! [`is_test_path`] matches test code and test support. Use it when the
//! question is "is this production code": health hotspots, the human check
//! split, orientation and the audit branching split.
//!
//! The match is syntactic and ASCII case-insensitive. A directory segment
//! matches by its full name, and a file-name marker matches only in the last
//! segment. Both `/` and `\` separate segments, so the verdict does not depend
//! on the platform.

use std::path::Path;

/// Directory names that hold test code.
const TEST_CODE_DIR_NAMES: &[&str] = &[
    "test",
    "tests",
    "__tests__",
    "__test__",
    "spec",
    "specs",
    "e2e",
];

/// Directory names that hold test support: mocks, fixtures and snapshots.
const TEST_SUPPORT_DIR_NAMES: &[&str] = &["__mocks__", "__fixtures__", "fixtures", "__snapshots__"];

/// File-name markers of test code (`app.test.ts`, `login.cy.ts`).
const TEST_CODE_FILE_MARKERS: &[&str] = &[".test.", ".spec.", ".e2e.", ".e2e-spec.", ".cy."];

/// File-name markers of test support (`user.fixture.ts`).
const TEST_SUPPORT_FILE_MARKERS: &[&str] = &[".fixture."];

/// The role of a test path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestPathKind {
    /// The path holds tests.
    Code,
    /// The path helps tests: a mock, a fixture or a snapshot.
    Support,
}

/// Whether a project-relative path is test code or test support.
///
/// Pass a path relative to the project root. An absolute path also matches
/// on the directories above the project root.
#[must_use]
pub fn is_test_path(path: &Path) -> bool {
    is_test_path_str(&path.to_string_lossy())
}

/// Whether a project-relative path string is test code or test support.
///
/// The string form of [`is_test_path`] for callers that hold
/// forward-slash path strings.
#[must_use]
pub fn is_test_path_str(path: &str) -> bool {
    classify(path).is_some()
}

/// Whether a project-relative path is test code. Test support (mocks,
/// fixtures and snapshots) does not match, except below a test directory.
///
/// Pass a path relative to the project root. An absolute path also matches
/// on the directories above the project root.
#[must_use]
pub fn is_test_code_path(path: &Path) -> bool {
    is_test_code_path_str(&path.to_string_lossy())
}

/// Whether a project-relative path string is test code.
///
/// The string form of [`is_test_code_path`] for callers that hold
/// forward-slash path strings.
#[must_use]
pub fn is_test_code_path_str(path: &str) -> bool {
    classify(path) == Some(TestPathKind::Code)
}

/// The role of a path, or `None` for a path that is not a test path. A
/// test-code match wins over a test-support match.
fn classify(path: &str) -> Option<TestPathKind> {
    let mut segments = path
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty());
    let file_name = segments.next_back()?;
    let has_marker = |markers: &[&str]| {
        markers
            .iter()
            .any(|marker| contains_ignore_ascii_case(file_name, marker))
    };
    let mut kind = has_marker(TEST_CODE_FILE_MARKERS)
        .then_some(TestPathKind::Code)
        .or_else(|| has_marker(TEST_SUPPORT_FILE_MARKERS).then_some(TestPathKind::Support));
    for segment in segments {
        if kind == Some(TestPathKind::Code) {
            break;
        }
        if is_one_of(segment, TEST_CODE_DIR_NAMES) {
            kind = Some(TestPathKind::Code);
        } else if is_one_of(segment, TEST_SUPPORT_DIR_NAMES) {
            kind = Some(TestPathKind::Support);
        }
    }
    kind
}

/// Whether a directory segment equals one of `names`, ignoring ASCII case.
fn is_one_of(segment: &str, names: &[&str]) -> bool {
    names.iter().any(|name| name.eq_ignore_ascii_case(segment))
}

/// ASCII case-insensitive substring search without an allocation.
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_verdicts_over_shared_corpus() {
        let corpus = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/test-path-corpus.txt"
        ));
        let rendered = corpus
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|path| {
                let path = Path::new(path);
                let verdict = match (is_test_code_path(path), is_test_path(path)) {
                    (true, _) => "code",
                    (false, true) => "supp",
                    (false, false) => "-   ",
                };
                format!("{verdict} {}", path.display())
            })
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(rendered);
    }

    #[test]
    fn path_and_string_forms_agree() {
        for path in [
            "src/app.test.ts",
            "tests/unit/widget.ts",
            "src/__mocks__/api.ts",
            "src/app.ts",
        ] {
            assert_eq!(is_test_path(Path::new(path)), is_test_path_str(path));
            assert_eq!(
                is_test_code_path(Path::new(path)),
                is_test_code_path_str(path)
            );
        }
    }

    #[test]
    fn test_support_is_a_test_path_but_not_test_code() {
        for path in [
            "src/__mocks__/api.ts",
            "src/__fixtures__/user.ts",
            "fixtures/user.ts",
            "src/__snapshots__/api.ts.snap",
            "src/user.fixture.ts",
        ] {
            assert!(is_test_path_str(path), "{path} is a test path");
            assert!(!is_test_code_path_str(path), "{path} is not test code");
        }
    }

    #[test]
    fn test_code_wins_over_test_support() {
        assert!(is_test_code_path_str("test/fixtures/user.ts"));
        assert!(is_test_code_path_str("src/__fixtures__/user.test.ts"));
        assert!(is_test_code_path_str("src/__mocks__/tests/api.ts"));
    }

    #[test]
    fn backslash_separates_segments_on_every_platform() {
        assert!(is_test_path_str(r"src\__tests__\widget.ts"));
        assert!(is_test_path_str(r"packages\core\tests\widget.ts"));
        assert!(!is_test_path_str(r"src\components\widget.ts"));
    }

    #[test]
    fn marker_in_a_directory_name_does_not_match() {
        assert!(!is_test_path_str("src/foo.test.d/widget.ts"));
        assert!(is_test_path_str("src/foo.test.d/widget.test.ts"));
    }

    #[test]
    fn empty_and_root_only_paths_are_not_tests() {
        assert!(!is_test_path_str(""));
        assert!(!is_test_path_str("/"));
        assert!(!is_test_path_str("tests/"));
    }

    #[test]
    fn substring_search_ignores_ascii_case() {
        assert!(contains_ignore_ascii_case("App.TEST.ts", ".test."));
        assert!(!contains_ignore_ascii_case("ab", "abc"));
        assert!(contains_ignore_ascii_case("anything", ""));
    }
}
