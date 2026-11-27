//! A path lookup over the test entry patterns that a test runner reads.

use fallow_config::EntryPointRole;

use super::{AggregatedPluginResult, PathRule, PathRuleIndex};

/// The test entry patterns of one plugin run, compiled for path lookups.
///
/// The Vitest plugin adds `**/__tests__/**/*` as a test entry pattern, so
/// that a helper module below `__tests__` is not reported as unused. The
/// Vitest default include does not select such a file, so this lookup leaves
/// that pattern out. Each other pattern of a test plugin stays: a configured
/// Vitest include, a Jest or tap default, or a file that a `node --test`
/// script names.
#[derive(Debug, Clone)]
pub struct TestIncludes {
    rules: PathRuleIndex<()>,
}

impl TestIncludes {
    /// True when a test pattern selects the project-root-relative
    /// `rel_path`, with forward slashes.
    #[must_use]
    pub fn selects(&self, rel_path: &str) -> bool {
        self.rules.any_match(rel_path)
    }
}

impl AggregatedPluginResult {
    /// Compiles the test entry patterns for path lookups. A pattern with an
    /// invalid glob selects no file.
    #[must_use]
    pub fn test_includes(&self) -> TestIncludes {
        let rules = self
            .entry_patterns
            .iter()
            .filter(|(rule, plugin)| {
                self.entry_point_roles.get(plugin) == Some(&EntryPointRole::Test)
                    && !is_vitest_tests_dir_convention(rule, plugin)
            })
            .map(|(rule, _)| (rule, ()));
        TestIncludes {
            rules: PathRuleIndex::build(rules, "test entry pattern"),
        }
    }
}

/// True for the Vitest `__tests__` pattern, also with a workspace prefix.
fn is_vitest_tests_dir_convention(rule: &PathRule, plugin: &str) -> bool {
    let convention = super::vitest::VITEST_TESTS_DIR_CONVENTION;
    plugin == "vitest"
        && rule
            .pattern
            .strip_suffix(convention)
            .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn includes(rules: &[(&str, &str)], test_plugins: &[&str]) -> TestIncludes {
        let result = AggregatedPluginResult {
            entry_patterns: rules
                .iter()
                .map(|(pattern, plugin)| (PathRule::new(*pattern), (*plugin).to_owned()))
                .collect(),
            entry_point_roles: test_plugins
                .iter()
                .map(|plugin| ((*plugin).to_owned(), EntryPointRole::Test))
                .collect(),
            ..AggregatedPluginResult::default()
        };
        result.test_includes()
    }

    #[test]
    fn the_vitest_tests_dir_convention_selects_no_file() {
        let convention = super::super::vitest::VITEST_TESTS_DIR_CONVENTION;
        let prefixed = format!("packages/a/{convention}");
        let lookup = includes(
            &[(convention, "vitest"), (prefixed.as_str(), "vitest")],
            &["vitest"],
        );
        assert!(!lookup.selects("src/__tests__/helper.ts"));
        assert!(!lookup.selects("packages/a/src/__tests__/helper.ts"));
    }

    #[test]
    fn a_test_plugin_pattern_selects_and_a_support_pattern_does_not() {
        let lookup = includes(
            &[
                ("src/**/__tests__/**/*.ts", "vitest"),
                ("**/__tests__/**/*.{ts,tsx,js,jsx}", "jest"),
                ("scripts/**/*.ts", "storybook"),
            ],
            &["vitest", "jest"],
        );
        assert!(lookup.selects("src/a/__tests__/check.ts"));
        assert!(lookup.selects("lib/__tests__/check.js"));
        assert!(!lookup.selects("scripts/build.ts"));
    }
}
