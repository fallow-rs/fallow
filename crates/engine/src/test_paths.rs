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
//! The match is mostly syntactic and ASCII case-insensitive. A directory
//! segment matches by its full name, and a file-name marker matches only in
//! the last segment. Both `/` and `\` separate segments, so the verdict does
//! not depend on the platform.
//!
//! Two rules also read the file system below the project root, because the
//! path alone is not sufficient:
//!
//! - A `.cy.` file is a Cypress spec only when it is a script file below a
//!   `cypress` directory, or below a directory that holds a Cypress config or
//!   a `cypress` directory. `.cy.` is also the Welsh language code, so
//!   `src/i18n/strings.cy.ts` in a project without Cypress is not a test.
//! - A `spec` or `specs` directory holds tests only at a test root: the project
//!   root, a package root (a directory with a `package.json`), or a directory
//!   with a `src` or `lib` directory next to the `spec` directory. A
//!   `src/spec/` module is production code.

use std::path::{Path, PathBuf};

/// Directory names that hold test code at every depth.
const TEST_CODE_DIR_NAMES: &[&str] = &["test", "tests", "__tests__", "__test__", "e2e"];

/// Directory names that hold test code only at a test root.
const TEST_ROOT_DIR_NAMES: &[&str] = &["spec", "specs"];

/// Directory names that hold test support: mocks, fixtures and snapshots.
const TEST_SUPPORT_DIR_NAMES: &[&str] = &["__mocks__", "__fixtures__", "fixtures", "__snapshots__"];

/// File-name markers of test code (`app.test.ts`, `app.spec.ts`).
const TEST_CODE_FILE_MARKERS: &[&str] = &[".test.", ".spec.", ".e2e.", ".e2e-spec."];

/// File-name markers of test support (`user.fixture.ts`).
const TEST_SUPPORT_FILE_MARKERS: &[&str] = &[".fixture."];

/// File-name marker of a Cypress spec (`login.cy.ts`).
const CYPRESS_FILE_MARKER: &str = ".cy.";

/// Script extensions that a Cypress spec can have.
const CYPRESS_SPEC_EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts"];

/// The directory name of a Cypress suite.
const CYPRESS_DIR_NAME: &str = "cypress";

/// File names of a Cypress config.
const CYPRESS_CONFIG_FILES: &[&str] = &[
    "cypress.config.ts",
    "cypress.config.js",
    "cypress.config.mjs",
    "cypress.config.cjs",
    "cypress.config.mts",
    "cypress.config.cts",
    "cypress.json",
];

/// The manifest file that marks a package root.
const PACKAGE_MANIFEST: &str = "package.json";

/// Source directory names. A `spec` directory next to one of them is a test root.
const SOURCE_DIR_NAMES: &[&str] = &["src", "lib"];

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
/// `root` is the project root. Pass `relative` relative to `root`. An
/// absolute path also matches on the directories above the project root.
#[must_use]
pub fn is_test_path(root: &Path, relative: &Path) -> bool {
    is_test_path_str(root, &relative.to_string_lossy())
}

/// Whether a project-relative path string is test code or test support.
///
/// The string form of [`is_test_path`] for callers that hold
/// forward-slash path strings.
#[must_use]
pub fn is_test_path_str(root: &Path, relative: &str) -> bool {
    classify(root, relative).is_some()
}

/// Whether a project-relative path is test code. Test support (mocks,
/// fixtures and snapshots) does not match, except below a test directory.
///
/// `root` is the project root. Pass `relative` relative to `root`. An
/// absolute path also matches on the directories above the project root.
#[must_use]
pub fn is_test_code_path(root: &Path, relative: &Path) -> bool {
    is_test_code_path_str(root, &relative.to_string_lossy())
}

/// Whether a project-relative path string is test code.
///
/// The string form of [`is_test_code_path`] for callers that hold
/// forward-slash path strings.
#[must_use]
pub fn is_test_code_path_str(root: &Path, relative: &str) -> bool {
    classify(root, relative) == Some(TestPathKind::Code)
}

/// The role of a path, or `None` for a path that is not a test path. A
/// test-code match wins over a test-support match.
fn classify(root: &Path, path: &str) -> Option<TestPathKind> {
    let mut segments: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .collect();
    let file_name = segments.pop()?;
    let dirs = segments;
    let has_marker = |markers: &[&str]| {
        markers
            .iter()
            .any(|marker| contains_ignore_ascii_case(file_name, marker))
    };
    if has_marker(TEST_CODE_FILE_MARKERS) || is_cypress_spec(root, &dirs, file_name) {
        return Some(TestPathKind::Code);
    }
    let mut kind = has_marker(TEST_SUPPORT_FILE_MARKERS).then_some(TestPathKind::Support);
    for (depth, segment) in dirs.iter().enumerate() {
        if is_one_of(segment, TEST_CODE_DIR_NAMES)
            || (is_one_of(segment, TEST_ROOT_DIR_NAMES) && is_test_root(root, &dirs[..depth]))
        {
            return Some(TestPathKind::Code);
        }
        if is_one_of(segment, TEST_SUPPORT_DIR_NAMES) {
            kind = Some(TestPathKind::Support);
        }
    }
    kind
}

/// Whether `file_name` below `dirs` is a Cypress spec: a script file with the
/// `.cy.` marker below a `cypress` directory, or below a directory that holds
/// a Cypress config or a `cypress` directory.
fn is_cypress_spec(root: &Path, dirs: &[&str], file_name: &str) -> bool {
    if !contains_ignore_ascii_case(file_name, CYPRESS_FILE_MARKER) {
        return false;
    }
    let is_script = Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| is_one_of(extension, CYPRESS_SPEC_EXTENSIONS));
    if !is_script {
        return false;
    }
    if dirs
        .iter()
        .any(|segment| segment.eq_ignore_ascii_case(CYPRESS_DIR_NAME))
    {
        return true;
    }
    (0..=dirs.len()).any(|depth| holds_cypress_setup(&join_below(root, &dirs[..depth])))
}

/// Whether `dir` holds a Cypress config or a `cypress` directory.
fn holds_cypress_setup(dir: &Path) -> bool {
    dir.join(CYPRESS_DIR_NAME).is_dir()
        || CYPRESS_CONFIG_FILES
            .iter()
            .any(|name| dir.join(name).is_file())
}

/// Whether the directory `dirs` below `root` is a test root: the project
/// root, a package root, or a directory that holds a source directory.
fn is_test_root(root: &Path, dirs: &[&str]) -> bool {
    if dirs.is_empty() {
        return true;
    }
    let dir = join_below(root, dirs);
    dir.join(PACKAGE_MANIFEST).is_file()
        || SOURCE_DIR_NAMES.iter().any(|name| dir.join(name).is_dir())
}

/// The directory `dirs` below `root`.
fn join_below(root: &Path, dirs: &[&str]) -> PathBuf {
    let mut dir = root.to_path_buf();
    dir.extend(dirs);
    dir
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

    /// A project root that does not exist, so no rule finds file-system evidence.
    const NO_ROOT: &str = "/fallow-test-paths-missing-root";

    fn root() -> &'static Path {
        Path::new(NO_ROOT)
    }

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
                let verdict = match (is_test_code_path(root(), path), is_test_path(root(), path)) {
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
            assert_eq!(
                is_test_path(root(), Path::new(path)),
                is_test_path_str(root(), path)
            );
            assert_eq!(
                is_test_code_path(root(), Path::new(path)),
                is_test_code_path_str(root(), path)
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
            assert!(is_test_path_str(root(), path), "{path} is a test path");
            assert!(
                !is_test_code_path_str(root(), path),
                "{path} is not test code"
            );
        }
    }

    #[test]
    fn test_code_wins_over_test_support() {
        assert!(is_test_code_path_str(root(), "test/fixtures/user.ts"));
        assert!(is_test_code_path_str(
            root(),
            "src/__fixtures__/user.test.ts"
        ));
        assert!(is_test_code_path_str(root(), "src/__mocks__/tests/api.ts"));
    }

    #[test]
    fn backslash_separates_segments_on_every_platform() {
        assert!(is_test_path_str(root(), r"src\__tests__\widget.ts"));
        assert!(is_test_path_str(root(), r"packages\core\tests\widget.ts"));
        assert!(!is_test_path_str(root(), r"src\components\widget.ts"));
    }

    #[test]
    fn marker_in_a_directory_name_does_not_match() {
        assert!(!is_test_path_str(root(), "src/foo.test.d/widget.ts"));
        assert!(is_test_path_str(root(), "src/foo.test.d/widget.test.ts"));
    }

    #[test]
    fn empty_and_root_only_paths_are_not_tests() {
        assert!(!is_test_path_str(root(), ""));
        assert!(!is_test_path_str(root(), "/"));
        assert!(!is_test_path_str(root(), "tests/"));
    }

    #[test]
    fn welsh_locale_file_is_not_test_code() {
        assert!(!is_test_path_str(root(), "src/i18n/strings.cy.ts"));
    }

    #[test]
    fn spec_directory_below_source_is_not_test_code() {
        assert!(!is_test_path_str(root(), "src/spec/schema.ts"));
    }

    /// Create `relative` below `root`: a directory when it ends with `/`,
    /// else an empty file.
    fn touch(root: &Path, relative: &str) {
        let path = root.join(relative);
        if relative.ends_with('/') {
            std::fs::create_dir_all(path).unwrap();
            return;
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn cypress_spec_needs_cypress_evidence() {
        assert!(is_test_code_path_str(root(), "cypress/e2e/login.cy.ts"));
        assert!(!is_test_path_str(root(), "src/components/Button.cy.tsx"));

        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "cypress.config.ts");
        assert!(is_test_code_path_str(
            dir.path(),
            "src/components/Button.cy.tsx"
        ));
        assert!(!is_test_path_str(dir.path(), "src/i18n/strings.cy.json"));

        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "packages/web/cypress/");
        assert!(is_test_code_path_str(
            dir.path(),
            "packages/web/src/Button.cy.jsx"
        ));
        assert!(!is_test_path_str(
            dir.path(),
            "packages/api/src/strings.cy.ts"
        ));
    }

    #[test]
    fn spec_directory_counts_only_at_a_test_root() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "packages/core/package.json");
        assert!(is_test_code_path_str(dir.path(), "spec/widget.ts"));
        assert!(is_test_code_path_str(dir.path(), "specs/widget.ts"));
        assert!(is_test_code_path_str(
            dir.path(),
            "packages/core/spec/widget.ts"
        ));
        assert!(!is_test_path_str(
            dir.path(),
            "packages/core/src/spec/schema.ts"
        ));
        assert!(!is_test_path_str(
            dir.path(),
            "packages/other/spec/schema.ts"
        ));

        touch(dir.path(), "examples/basic/src/");
        assert!(is_test_code_path_str(
            dir.path(),
            "examples/basic/spec/widget.ts"
        ));
    }

    #[test]
    fn substring_search_ignores_ascii_case() {
        assert!(contains_ignore_ascii_case("App.TEST.ts", ".test."));
        assert!(!contains_ignore_ascii_case("ab", "abc"));
        assert!(contains_ignore_ascii_case("anything", ""));
    }
}
