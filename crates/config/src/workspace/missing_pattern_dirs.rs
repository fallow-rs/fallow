//! Declared workspace patterns whose directory does not exist.

use std::path::Path;

use super::{DeclaredPattern, collect_declared_patterns};

/// The positive declared patterns that stay inside the root and whose
/// directory part does not exist on disk.
///
/// The directory part is the literal prefix of the pattern
/// ([`fallow_types::workspace::workspace_pattern_literal_dir`]). A pattern
/// without one never qualifies, and a directory that exists but holds no
/// package does not qualify either. The rule stays conservative because a
/// match suppresses the unused-catalog-entries check for the run.
pub(super) fn missing_dir_patterns<'a>(
    root: &'a Path,
    declared: &'a [DeclaredPattern],
) -> impl Iterator<Item = &'a DeclaredPattern> {
    declared.iter().filter(move |declared| {
        !declared.pattern.starts_with('!')
            && !crate::glob_validation::escapes_project_root(&declared.pattern)
            && fallow_types::workspace::workspace_pattern_literal_dir(&declared.pattern)
                .is_some_and(|dir| !root.join(dir).exists())
    })
}

/// Whether a declared workspace pattern names a directory that does not
/// exist, the condition of the `workspace-pattern-dir-missing` diagnostic.
///
/// The catalog analysis reads this to skip the unused-catalog-entries check,
/// because the packages in that directory can consume catalog entries.
/// Discovery and the catalog analysis share one predicate, so the
/// diagnostic and the skip cannot disagree. A root manifest that does not
/// parse gives `false`; discovery reports it.
#[must_use]
pub fn has_missing_workspace_pattern_dirs(root: &Path, extra_patterns: &[String]) -> bool {
    collect_declared_patterns(root, extra_patterns)
        .is_ok_and(|declared| missing_dir_patterns(root, &declared).next().is_some())
}
