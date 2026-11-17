//! The reason why a path of a `Paths` scope selects no source file.

use std::path::Path;

use fallow_config::IgnorePatternSet;

use super::UnmatchedReason;

/// The most entries that the check reads under a directory. A larger
/// directory without an ignored file in the first entries gets the
/// `NoSourceFile` reason.
const MAX_WALKED_ENTRIES: usize = 10_000;

/// Classifies a path that selects no source file. `absolute` is the path
/// joined to the root when it was relative.
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
            .build()
            .take(MAX_WALKED_ENTRIES)
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_some_and(|kind| !kind.is_dir()))
            .any(|entry| is_ignored(entry.path()))
    } else {
        is_ignored(absolute)
    };
    if ignored {
        UnmatchedReason::Ignored
    } else {
        UnmatchedReason::NoSourceFile
    }
}
