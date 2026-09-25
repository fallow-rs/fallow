//! Shared classification of test paths.
//!
//! A test path is a file that holds test code or test support: specs, mocks,
//! fixtures and snapshots. Health hotspots, audit, the human check report,
//! orientation and similar-code evidence all use this one predicate, so a file
//! gets the same verdict on every surface.
//!
//! The match is syntactic and ASCII case-insensitive. A directory segment
//! matches by its full name, and a file-name marker matches only in the last
//! segment. Both `/` and `\` separate segments, so the verdict does not depend
//! on the platform.

use std::path::Path;

/// Directory names that hold test code or test support.
const TEST_DIR_NAMES: &[&str] = &[
    "test",
    "tests",
    "__tests__",
    "__test__",
    "spec",
    "specs",
    "__mocks__",
    "__fixtures__",
    "fixtures",
    "__snapshots__",
    "e2e",
];

/// File-name markers of test files (`app.test.ts`, `login.cy.ts`).
const TEST_FILE_MARKERS: &[&str] = &[
    ".test.",
    ".spec.",
    ".e2e.",
    ".e2e-spec.",
    ".cy.",
    ".fixture.",
];

/// Whether a project-relative path is a test path.
///
/// Pass a path relative to the project root. An absolute path also matches
/// on the directories above the project root.
#[must_use]
pub fn is_test_path(path: &Path) -> bool {
    is_test_path_str(&path.to_string_lossy())
}

/// Whether a project-relative path string is a test path.
///
/// The string form of [`is_test_path`] for callers that hold
/// forward-slash path strings.
#[must_use]
pub fn is_test_path_str(path: &str) -> bool {
    let mut segments = path
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty());
    let Some(file_name) = segments.next_back() else {
        return false;
    };
    has_test_file_marker(file_name) || segments.any(is_test_dir_name)
}

/// Whether one directory segment names a test directory.
#[must_use]
pub fn is_test_dir_name(segment: &str) -> bool {
    TEST_DIR_NAMES
        .iter()
        .any(|name| name.eq_ignore_ascii_case(segment))
}

/// Whether a file name carries a test-file marker.
#[must_use]
pub fn has_test_file_marker(file_name: &str) -> bool {
    TEST_FILE_MARKERS
        .iter()
        .any(|marker| contains_ignore_ascii_case(file_name, marker))
}

/// ASCII case-insensitive substring search without an allocation.
#[must_use]
pub fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
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
                let verdict = if is_test_path(Path::new(path)) {
                    "test"
                } else {
                    "-   "
                };
                format!("{verdict} {path}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(rendered);
    }

    #[test]
    fn path_and_string_forms_agree() {
        for path in ["src/app.test.ts", "tests/unit/widget.ts", "src/app.ts"] {
            assert_eq!(is_test_path(Path::new(path)), is_test_path_str(path));
        }
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
