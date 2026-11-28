//! Negated workspace patterns, such as `!apps/api` in `pnpm-workspace.yaml`.

use std::path::Path;

/// The compiled `!` patterns of one workspace declaration.
pub(super) struct NegatedPatterns(Vec<crate::PathMatcher>);

impl NegatedPatterns {
    /// Compile each pattern that starts with `!`. Other patterns are skipped,
    /// and so is a pattern whose matcher does not compile.
    pub(super) fn new<'a>(patterns: impl IntoIterator<Item = &'a str>) -> Self {
        Self(
            patterns
                .into_iter()
                .filter_map(|p| p.strip_prefix('!'))
                .filter_map(|p| crate::glob_validation::compile_path_glob(p).ok())
                .filter_map(|g| crate::glob_validation::try_compile_matcher(&g).ok())
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

#[cfg(test)]
mod tests {
    use super::NegatedPatterns;

    /// A glob that parses, but whose regex nests too deep to compile.
    fn deeply_nested_glob() -> String {
        format!("{}b{}", "a{".repeat(130), "}".repeat(130))
    }

    #[test]
    fn a_negated_pattern_that_does_not_compile_excludes_nothing() {
        let negated = format!("!{}", deeply_nested_glob());
        let patterns = NegatedPatterns::new([negated.as_str(), "packages/*"]);
        let root = std::path::Path::new("/p");
        assert!(!patterns.excludes(root, &root.join("packages/a")));
    }

    #[test]
    fn a_public_package_glob_that_does_not_compile_selects_nothing() {
        assert!(!crate::workspace_is_public("ab", &[deeply_nested_glob()]));
    }
}
