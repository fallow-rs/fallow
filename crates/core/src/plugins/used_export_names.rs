//! A path lookup over the plugin used-export rules.

use std::collections::BTreeSet;

use super::{AggregatedPluginResult, CompiledPathRule};

/// The plugin used-export rules of one plugin run, compiled for path lookups.
///
/// A framework plugin marks some exports of a file as used by the framework,
/// for example `revalidate` in a Next.js page. The unused-export analysis
/// reads the same rules with the same path matcher.
#[derive(Debug, Clone)]
pub struct UsedExportNames {
    rules: Vec<(CompiledPathRule, Vec<String>)>,
}

impl UsedExportNames {
    /// The export names that a plugin marks as used in the file at the
    /// project-root-relative `rel_path`, with forward slashes. The name `*`
    /// means each export of the file.
    #[must_use]
    pub fn names_for(&self, rel_path: &str) -> BTreeSet<&str> {
        self.rules
            .iter()
            .filter(|(path, _)| path.matches(rel_path))
            .flat_map(|(_, exports)| exports.iter().map(String::as_str))
            .collect()
    }
}

impl AggregatedPluginResult {
    /// Compiles the used-export rules for path lookups. A rule with an
    /// invalid glob matches no file, as in the unused-export analysis.
    #[must_use]
    pub fn used_export_names(&self) -> UsedExportNames {
        UsedExportNames {
            rules: self
                .used_exports
                .iter()
                .filter_map(|rule| {
                    let path = CompiledPathRule::for_used_export_rule(
                        &rule.rule.path,
                        "used_exports pattern",
                    )?;
                    Some((path, rule.rule.exports.clone()))
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{PluginUsedExportRule, UsedExportRule};
    use super::*;

    fn result(rules: Vec<PluginUsedExportRule>) -> AggregatedPluginResult {
        AggregatedPluginResult {
            used_exports: rules,
            ..AggregatedPluginResult::default()
        }
    }

    #[test]
    fn names_for_a_path_join_each_matching_rule() {
        let names = result(vec![
            PluginUsedExportRule::new(
                "nextjs",
                UsedExportRule::new("app/**/page.{ts,tsx}", ["default", "revalidate"]),
            ),
            PluginUsedExportRule::new("nextjs", UsedExportRule::new("app/**/*.tsx", ["dynamic"])),
            PluginUsedExportRule::new(
                "tanstack-router",
                UsedExportRule::new("src/routes/**/*.tsx", ["Route"]),
            ),
        ])
        .used_export_names();

        assert_eq!(
            names.names_for("app/blog/page.tsx"),
            BTreeSet::from(["default", "dynamic", "revalidate"])
        );
        assert_eq!(
            names.names_for("src/routes/index.tsx"),
            BTreeSet::from(["Route"])
        );
        assert!(names.names_for("src/lib/math.ts").is_empty());
    }

    #[test]
    fn an_excluded_glob_and_an_invalid_glob_match_no_file() {
        let names = result(vec![
            PluginUsedExportRule::new(
                "tanstack-router",
                UsedExportRule::new("src/routes/**/*.tsx", ["Route"])
                    .with_excluded_globs(["src/routes/**/-*.tsx"]),
            ),
            PluginUsedExportRule::new("broken", UsedExportRule::new("src/[", ["config"])),
        ])
        .used_export_names();

        assert_eq!(
            names.names_for("src/routes/posts.tsx"),
            BTreeSet::from(["Route"])
        );
        assert!(names.names_for("src/routes/-helper.tsx").is_empty());
        assert!(names.names_for("src/[").is_empty());
    }
}
