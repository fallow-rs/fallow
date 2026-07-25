use globset::{Glob, GlobSet, GlobSetBuilder};

/// Compiled project-relative patterns for hiding source-owned findings.
///
/// Positive patterns select paths to hide. Negated patterns (prefixed with
/// `!`) select report exceptions. When every pattern is negated, all paths
/// outside those exceptions are hidden, matching Knip's `ignore` semantics.
#[derive(Debug, Clone, Default)]
pub struct FindingIgnoreMatcher {
    hidden: GlobSet,
    reported: GlobSet,
    has_patterns: bool,
}

impl FindingIgnoreMatcher {
    #[expect(
        clippy::expect_used,
        reason = "ignoreFindings patterns are validated before config resolution"
    )]
    pub(crate) fn compile(patterns: &[String]) -> Self {
        if patterns.is_empty() {
            return Self::default();
        }

        let (hidden, reported) = Self::build_sets(patterns)
            .expect("ignoreFindings pattern sets were validated before config resolution");

        Self {
            hidden,
            reported,
            has_patterns: true,
        }
    }

    pub(super) fn validate_compilation(patterns: &[String]) -> Result<(), globset::Error> {
        Self::build_sets(patterns).map(|_| ())
    }

    fn build_sets(patterns: &[String]) -> Result<(GlobSet, GlobSet), globset::Error> {
        let mut hidden = GlobSetBuilder::new();
        let mut reported = GlobSetBuilder::new();

        for pattern in patterns {
            let (builder, pattern) = if let Some(pattern) = pattern.strip_prefix('!') {
                (&mut reported, pattern)
            } else {
                (&mut hidden, pattern.as_str())
            };
            let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
            builder.add(Glob::new(pattern)?);
        }

        Ok((hidden.build()?, reported.build()?))
    }

    /// Whether no finding-ignore patterns were configured.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        !self.has_patterns
    }

    /// Whether a normalized project-root-relative source path is hidden.
    #[must_use]
    pub fn is_ignored(&self, path: &str) -> bool {
        self.has_patterns
            && (self.hidden.is_empty() || self.hidden.is_match(path))
            && !self.reported.is_match(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher(patterns: &[&str]) -> FindingIgnoreMatcher {
        FindingIgnoreMatcher::compile(
            &patterns
                .iter()
                .map(|pattern| (*pattern).to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn empty_matcher_ignores_nothing() {
        let matcher = matcher(&[]);

        assert!(matcher.is_empty());
        assert!(!matcher.is_ignored("src/app.ts"));
    }

    #[test]
    fn positive_patterns_hide_matching_paths() {
        let matcher = matcher(&["**/*.test.ts"]);

        assert!(matcher.is_ignored("src/app.test.ts"));
        assert!(!matcher.is_ignored("src/app.ts"));
    }

    #[test]
    fn negated_patterns_keep_matching_paths_reported() {
        let matcher = matcher(&["**/*.ts", "!src/public/**"]);

        assert!(matcher.is_ignored("src/private/app.ts"));
        assert!(!matcher.is_ignored("src/public/app.ts"));
    }

    #[test]
    fn negated_only_patterns_report_only_matching_paths() {
        let matcher = matcher(&["!src/public/**"]);

        assert!(matcher.is_ignored("src/private/app.ts"));
        assert!(!matcher.is_ignored("src/public/app.ts"));
    }

    #[test]
    fn leading_dot_slash_is_normalized_after_negation() {
        let matcher = matcher(&["!./src/public/**"]);

        assert!(matcher.is_ignored("src/private/app.ts"));
        assert!(!matcher.is_ignored("src/public/app.ts"));
    }

    #[test]
    fn pattern_order_does_not_change_set_semantics() {
        let first = matcher(&["**/*.ts", "!src/public/**"]);
        let second = matcher(&["!src/public/**", "**/*.ts"]);

        for path in ["src/private/app.ts", "src/public/app.ts", "README.md"] {
            assert_eq!(first.is_ignored(path), second.is_ignored(path));
        }
    }

    #[test]
    fn matches_dotfiles_and_forward_slash_paths() {
        let matcher = matcher(&["**/*.test.ts", ".storybook/**"]);

        assert!(matcher.is_ignored("packages/ui/src/button.test.ts"));
        assert!(matcher.is_ignored(".storybook/preview.ts"));
    }
}
