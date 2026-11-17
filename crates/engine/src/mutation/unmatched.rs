//! The reason why a path of a `Paths` scope selects no source file.

use std::ffi::OsStr;
use std::path::Path;

use fallow_config::{DEFAULT_IGNORE_PATTERNS, IgnorePatternSet};

use super::UnmatchedReason;
use super::layout::Layout;
use crate::graph::is_declaration_file_path;
use crate::module_graph::relative_key_path;

/// The most entries that the check reads under a directory. A larger
/// directory without an ignored file in the first entries gets the
/// `NoSourceFile` reason.
const MAX_WALKED_ENTRIES: usize = 10_000;

/// The prefix and suffix of a built-in ignore glob that excludes a full
/// directory, for example `**/node_modules/**`.
const DIRECTORY_GLOB_PREFIX: &str = "**/";
const DIRECTORY_GLOB_SUFFIX: &str = "/**";

/// Classifies a path that selects no source file. `absolute` is the path
/// joined to the root when it was relative.
///
/// The reason is `Ignored` only when the ignore set excludes the path, or a
/// file under it that would otherwise be a source file. Dependencies and
/// build output under the path (for example `pkg/node_modules`) never give
/// the reason, because they are never source files of the path.
pub(super) fn reason(
    root: &Path,
    absolute: &Path,
    ignore_patterns: Option<&IgnorePatternSet>,
) -> UnmatchedReason {
    if !absolute.exists() {
        return UnmatchedReason::NotFound;
    }
    let Some(ignore_patterns) = ignore_patterns else {
        return UnmatchedReason::NoSourceFile;
    };
    let root = dunce::simplified(root);
    let is_ignored = |path: &Path| {
        let path = dunce::simplified(path);
        let relative = path.strip_prefix(root).unwrap_or(path);
        ignore_patterns.is_match(relative)
    };
    let ignored = if absolute.is_dir() {
        ignore::WalkBuilder::new(absolute)
            .standard_filters(false)
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !entry.file_type().is_some_and(|kind| kind.is_dir())
                    || !is_built_in_ignored_dir(entry.file_name())
            })
            .build()
            .take(MAX_WALKED_ENTRIES)
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_some_and(|kind| !kind.is_dir()))
            .any(|entry| is_source_file(root, entry.path()) && is_ignored(entry.path()))
    } else {
        is_ignored(absolute)
    };
    if ignored {
        UnmatchedReason::Ignored
    } else {
        UnmatchedReason::NoSourceFile
    }
}

/// True when a built-in ignore glob excludes every file under a directory
/// with this name.
fn is_built_in_ignored_dir(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    DEFAULT_IGNORE_PATTERNS.iter().any(|pattern| {
        pattern
            .strip_prefix(DIRECTORY_GLOB_PREFIX)
            .and_then(|rest| rest.strip_suffix(DIRECTORY_GLOB_SUFFIX))
            == Some(name)
    })
}

/// True when the plan selects the file if no ignore glob excludes it: a
/// supported source extension, not a declaration file and not a test path.
fn is_source_file(root: &Path, path: &Path) -> bool {
    let rel = relative_key_path(path, root);
    Layout::from_path(&rel).is_some()
        && !is_declaration_file_path(path)
        && !crate::test_paths::is_test_path_str(root, &rel)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use fallow_config::{DEFAULT_IGNORE_PATTERNS, IgnorePatternSet, compile_path_glob};
    use globset::GlobSetBuilder;

    use super::{UnmatchedReason, reason};

    /// A set of the built-in ignores, with the user globs in front.
    fn ignore_set(user: &[&str]) -> IgnorePatternSet {
        let mut builder = GlobSetBuilder::new();
        for pattern in user.iter().chain(DEFAULT_IGNORE_PATTERNS) {
            builder.add(compile_path_glob(pattern).expect("valid glob"));
        }
        IgnorePatternSet::new(builder.build().expect("glob set"), &[])
    }

    fn project(files: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            fs::write(&path, "export const a = 1;\n").expect("write");
        }
        (dir, root)
    }

    fn classify(root: &Path, path: &str, user: &[&str]) -> UnmatchedReason {
        reason(root, &root.join(path), Some(&ignore_set(user)))
    }

    #[test]
    fn dependencies_under_a_test_only_path_are_not_the_reason() {
        let (_dir, root) = project(&[
            "src/index.ts",
            "pkg/a.test.ts",
            "pkg/node_modules/x/index.js",
        ]);
        assert_eq!(classify(&root, "pkg", &[]), UnmatchedReason::NoSourceFile);
    }

    #[test]
    fn build_output_under_a_test_only_path_is_not_the_reason() {
        let (_dir, root) = project(&["src/index.ts", "pkg/a.test.ts", "pkg/dist/out.js"]);
        assert_eq!(classify(&root, "pkg", &[]), UnmatchedReason::NoSourceFile);
    }

    #[test]
    fn an_ignored_test_or_declaration_file_is_not_the_reason() {
        let (_dir, root) = project(&["pkg/gen/a.test.ts", "pkg/gen/types.d.ts"]);
        assert_eq!(
            classify(&root, "pkg", &["pkg/gen/**"]),
            UnmatchedReason::NoSourceFile
        );
    }

    #[test]
    fn an_ignored_source_file_under_the_path_is_the_reason() {
        let (_dir, root) = project(&[
            "pkg/a.test.ts",
            "pkg/gen/add.ts",
            "pkg/node_modules/x/index.js",
        ]);
        assert_eq!(
            classify(&root, "pkg", &["pkg/gen/**"]),
            UnmatchedReason::Ignored
        );
    }

    #[test]
    fn a_path_inside_a_built_in_ignored_directory_is_ignored() {
        let (_dir, root) = project(&["dist/out.js"]);
        assert_eq!(classify(&root, "dist", &[]), UnmatchedReason::Ignored);
    }
}
