//! Negated workspace patterns, such as `!apps/api` in `pnpm-workspace.yaml`.

use std::path::Path;

/// The compiled `!` patterns of one workspace declaration.
pub(super) struct NegatedPatterns(Vec<globset::GlobMatcher>);

impl NegatedPatterns {
    /// Compile each pattern that starts with `!`. Other patterns are skipped.
    pub(super) fn new<'a>(patterns: impl IntoIterator<Item = &'a str>) -> Self {
        Self(
            patterns
                .into_iter()
                .filter_map(|p| p.strip_prefix('!'))
                .filter_map(|p| crate::glob_validation::compile_path_glob(p).ok())
                .map(|g| g.compile_matcher())
                .collect(),
        )
    }

    /// Whether a negated pattern excludes `dir`. The patterns are relative to
    /// `root`.
    pub(super) fn excludes(&self, root: &Path, dir: &Path) -> bool {
        if self.0.is_empty() {
            return false;
        }
        let relative = dir.strip_prefix(root).unwrap_or(dir);
        let relative_str = relative.to_string_lossy().replace('\\', "/");
        self.0.iter().any(|m| m.is_match(relative_str.as_str()))
    }
}
