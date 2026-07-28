use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    usage: Option<Arc<PatternUsage>>,
}

/// Per-pattern hit state for the shared matcher.
///
/// The matcher is cloned along with the resolved config and consulted from
/// several pipeline stages, so the state lives behind an `Arc` and every clone
/// records into the same run.
#[derive(Debug)]
struct PatternUsage {
    patterns: Vec<String>,
    /// Original pattern index for each glob in `hidden`, in build order.
    hidden_origins: Vec<usize>,
    /// Original pattern index for each glob in `reported`, in build order.
    reported_origins: Vec<usize>,
    matched: Vec<AtomicBool>,
    consulted: AtomicBool,
}

impl PatternUsage {
    fn record(&self, origins: &[usize], set_indices: &[usize]) {
        for &index in set_indices {
            if let Some(&origin) = origins.get(index)
                && let Some(flag) = self.matched.get(origin)
            {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }
}

/// Compiled glob sets plus the mapping back to the configured pattern order.
struct CompiledSets {
    hidden: GlobSet,
    reported: GlobSet,
    hidden_origins: Vec<usize>,
    reported_origins: Vec<usize>,
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

        let sets = Self::build_sets(patterns)
            .expect("ignoreFindings pattern sets were validated before config resolution");

        Self {
            hidden: sets.hidden,
            reported: sets.reported,
            usage: Some(Arc::new(PatternUsage {
                patterns: patterns.to_vec(),
                hidden_origins: sets.hidden_origins,
                reported_origins: sets.reported_origins,
                matched: patterns.iter().map(|_| AtomicBool::new(false)).collect(),
                consulted: AtomicBool::new(false),
            })),
        }
    }

    pub(super) fn validate_compilation(patterns: &[String]) -> Result<(), globset::Error> {
        Self::build_sets(patterns).map(|_| ())
    }

    fn build_sets(patterns: &[String]) -> Result<CompiledSets, globset::Error> {
        let mut hidden = GlobSetBuilder::new();
        let mut reported = GlobSetBuilder::new();
        let mut hidden_origins = Vec::new();
        let mut reported_origins = Vec::new();

        for (index, pattern) in patterns.iter().enumerate() {
            let (builder, origins, pattern) = if let Some(pattern) = pattern.strip_prefix('!') {
                (&mut reported, &mut reported_origins, pattern)
            } else {
                (&mut hidden, &mut hidden_origins, pattern.as_str())
            };
            let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
            builder.add(Glob::new(pattern)?);
            origins.push(index);
        }

        Ok(CompiledSets {
            hidden: hidden.build()?,
            reported: reported.build()?,
            hidden_origins,
            reported_origins,
        })
    }

    /// Whether no finding-ignore patterns were configured.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.usage.is_none()
    }

    /// Whether a normalized project-root-relative source path is hidden.
    #[must_use]
    pub fn is_ignored(&self, path: &str) -> bool {
        let Some(usage) = self.usage.as_deref() else {
            return false;
        };
        usage.consulted.store(true, Ordering::Relaxed);

        // Both sets are matched unconditionally so a negated pattern is not
        // reported as a no-op merely because the positive set short-circuited.
        let mut indices = Vec::new();
        self.hidden.matches_into(path, &mut indices);
        let hidden_hit = !indices.is_empty();
        usage.record(&usage.hidden_origins, &indices);

        self.reported.matches_into(path, &mut indices);
        let reported_hit = !indices.is_empty();
        usage.record(&usage.reported_origins, &indices);

        (self.hidden.is_empty() || hidden_hit) && !reported_hit
    }

    /// Configured patterns that matched no candidate finding path, in config
    /// order.
    ///
    /// Empty when nothing was configured or when the matcher was never
    /// consulted, because a run without candidate paths says nothing about
    /// whether a pattern is a typo.
    #[must_use]
    pub fn unmatched_patterns(&self) -> Vec<&str> {
        let Some(usage) = self.usage.as_deref() else {
            return Vec::new();
        };
        if !usage.consulted.load(Ordering::Relaxed) {
            return Vec::new();
        }
        usage
            .patterns
            .iter()
            .zip(&usage.matched)
            .filter(|(_, matched)| !matched.load(Ordering::Relaxed))
            .map(|(pattern, _)| pattern.as_str())
            .collect()
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
    fn empty_configuration_reports_no_unmatched_patterns() {
        let matcher = matcher(&[]);

        assert!(!matcher.is_ignored("src/app.ts"));
        assert!(matcher.unmatched_patterns().is_empty());
    }

    #[test]
    fn matching_pattern_is_not_reported_as_unmatched() {
        let matcher = matcher(&["**/*.test.ts"]);

        assert!(matcher.is_ignored("src/app.test.ts"));
        assert!(matcher.unmatched_patterns().is_empty());
    }

    #[test]
    fn pattern_matching_nothing_is_reported() {
        let matcher = matcher(&["**/*.test.ts", "src/legcy/**"]);

        assert!(matcher.is_ignored("src/app.test.ts"));
        assert_eq!(matcher.unmatched_patterns(), vec!["src/legcy/**"]);
    }

    #[test]
    fn unconsulted_matcher_reports_no_unmatched_patterns() {
        let matcher = matcher(&["src/legcy/**"]);

        assert!(matcher.unmatched_patterns().is_empty());
    }

    #[test]
    fn negated_pattern_matching_nothing_is_reported() {
        let matcher = matcher(&["**/*.ts", "!src/public/**", "!src/publik/**"]);

        assert!(matcher.is_ignored("src/private/app.ts"));
        assert!(!matcher.is_ignored("src/public/app.ts"));
        assert_eq!(matcher.unmatched_patterns(), vec!["!src/publik/**"]);
    }

    #[test]
    fn usage_state_is_shared_across_clones() {
        let matcher = matcher(&["**/*.test.ts"]);
        let clone = matcher.clone();

        assert!(clone.is_ignored("src/app.test.ts"));
        assert!(matcher.unmatched_patterns().is_empty());
    }

    #[test]
    fn matches_dotfiles_and_forward_slash_paths() {
        let matcher = matcher(&["**/*.test.ts", ".storybook/**"]);

        assert!(matcher.is_ignored("packages/ui/src/button.test.ts"));
        assert!(matcher.is_ignored(".storybook/preview.ts"));
    }
}
