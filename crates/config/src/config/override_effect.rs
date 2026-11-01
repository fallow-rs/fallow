//! The `overrides` rules that cannot change a result (issue #4065).
//!
//! Fallow reads only the top-level severity of a project-level rule. The
//! detector of a base-gated rule runs only when the top-level rule is on.
//! The engine lists its project-level findings in `effective_severity.rs`,
//! and a test there fails when that list and [`PROJECT_LEVEL_RULES`] differ.

use fallow_types::workspace::{OVERRIDE_REASON_BASE_RULE_OFF, OVERRIDE_REASON_PROJECT_LEVEL};

use super::ResolvedConfig;
use super::rules::{PartialRulesConfig, RulesConfig, Severity};

type OverrideSeverity = fn(&PartialRulesConfig) -> Option<Severity>;
type TopLevelSeverity = fn(&RulesConfig) -> Severity;

/// Each row is the canonical rule name, whether `ignoreDependencies` or
/// `--unused-deps` concerns the rule, and the override field of the rule.
const PROJECT_LEVEL_RULES: [(&str, bool, OverrideSeverity); 10] = [
    ("unused-dependencies", true, |r| r.unused_dependencies),
    ("unused-dev-dependencies", true, |r| {
        r.unused_dev_dependencies
    }),
    ("unused-optional-dependencies", true, |r| {
        r.unused_optional_dependencies
    }),
    ("unlisted-dependencies", true, |r| r.unlisted_dependencies),
    ("type-only-dependencies", true, |r| r.type_only_dependencies),
    ("test-only-dependencies", true, |r| r.test_only_dependencies),
    ("dev-dependencies-in-production", true, |r| {
        r.dev_dependencies_in_production
    }),
    ("duplicate-exports", false, |r| r.duplicate_exports),
    ("re-export-cycle", false, |r| r.re_export_cycle),
    ("unused-catalog-entries", false, |r| {
        r.unused_catalog_entries
    }),
];

/// The rules for which a run confirms that an override does not start the
/// detector when the top-level rule is off. A rule joins this list only after
/// a fixture gives a finding with the rule on and none with the override.
/// The list excludes security rules, because `fallow security` starts its
/// detector by itself.
const BASE_GATED_RULES: [(&str, OverrideSeverity, TopLevelSeverity); 9] = [
    ("unused-files", |r| r.unused_files, |r| r.unused_files),
    ("unused-exports", |r| r.unused_exports, |r| r.unused_exports),
    ("unused-types", |r| r.unused_types, |r| r.unused_types),
    (
        "private-type-leaks",
        |r| r.private_type_leaks,
        |r| r.private_type_leaks,
    ),
    (
        "unused-enum-members",
        |r| r.unused_enum_members,
        |r| r.unused_enum_members,
    ),
    (
        "unused-class-members",
        |r| r.unused_class_members,
        |r| r.unused_class_members,
    ),
    (
        "unresolved-imports",
        |r| r.unresolved_imports,
        |r| r.unresolved_imports,
    ),
    (
        "circular-dependencies",
        |r| r.circular_dependencies,
        |r| r.circular_dependencies,
    ),
    (
        "deprecated-exports-in-use",
        |r| r.deprecated_exports_in_use,
        |r| r.deprecated_exports_in_use,
    ),
];

/// One `overrides` rule that cannot change the result of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IneffectiveOverrideRule {
    /// The canonical rule name, also when the config uses an alias.
    pub rule: &'static str,
    /// The `overrides[].files` globs, as written in the config.
    pub files: Vec<String>,
    /// The `override-rule-ineffective` reason token.
    pub reason: &'static str,
    /// Whether the rule is a dependency rule, so that a run without
    /// dependency findings has no use for the entry.
    pub dependency_rule: bool,
}

/// The canonical names of the project-level rules that `rules` sets.
#[must_use]
pub fn project_level_override_rules(rules: &PartialRulesConfig) -> Vec<&'static str> {
    PROJECT_LEVEL_RULES
        .iter()
        .filter(|(_, _, severity)| severity(rules).is_some())
        .map(|(name, _, _)| *name)
        .collect()
}

/// Whether the key `key` of an `overrides[].rules` map sets `rule`, directly
/// or through an alias.
pub(super) fn key_sets_rule(key: &str, rule: &str) -> bool {
    let map = serde_json::json!({ key: "off" });
    let Ok(rules) = serde_json::from_value::<PartialRulesConfig>(map) else {
        return false;
    };
    let in_project_level = PROJECT_LEVEL_RULES
        .iter()
        .any(|(name, _, severity)| *name == rule && severity(&rules).is_some());
    in_project_level
        || BASE_GATED_RULES
            .iter()
            .any(|(name, severity, _)| *name == rule && severity(&rules).is_some())
}

impl ResolvedConfig {
    /// The `overrides` rules that cannot change a result of this config, in
    /// config order. Equal `(rule, files, reason)` entries appear once,
    /// because a merged config has no user-visible index to tell them apart.
    ///
    /// Read this after CLI flags changed `rules`: a flag such as
    /// `--private-type-leaks` starts the detector, and the override then works.
    #[must_use]
    pub fn ineffective_override_rules(&self) -> Vec<IneffectiveOverrideRule> {
        let mut entries: Vec<IneffectiveOverrideRule> = Vec::new();
        for entry in &self.overrides {
            let project_level = PROJECT_LEVEL_RULES
                .iter()
                .filter(|(_, _, severity)| severity(&entry.rules).is_some())
                .map(|(name, dependency_rule, _)| IneffectiveOverrideRule {
                    rule: name,
                    files: entry.files.clone(),
                    reason: OVERRIDE_REASON_PROJECT_LEVEL,
                    dependency_rule: *dependency_rule,
                });
            let base_off = BASE_GATED_RULES
                .iter()
                .filter(|(_, severity, top_level)| {
                    top_level(&self.rules) == Severity::Off
                        && severity(&entry.rules).is_some_and(|value| value != Severity::Off)
                })
                .map(|(name, _, _)| IneffectiveOverrideRule {
                    rule: name,
                    files: entry.files.clone(),
                    reason: OVERRIDE_REASON_BASE_RULE_OFF,
                    dependency_rule: false,
                });
            for candidate in project_level.chain(base_off) {
                if !entries.contains(&candidate) {
                    entries.push(candidate);
                }
            }
        }
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_alias_key_sets_the_canonical_rule() {
        assert!(key_sets_rule(
            "dev-dependency-in-production",
            "dev-dependencies-in-production"
        ));
        assert!(key_sets_rule(
            "dev-dependencies-in-production",
            "dev-dependencies-in-production"
        ));
        assert!(key_sets_rule("unused-file", "unused-files"));
        assert!(!key_sets_rule("unused-files", "unused-exports"));
        assert!(!key_sets_rule("not-a-rule", "unused-files"));
    }

    #[test]
    fn the_two_tables_share_no_rule() {
        for (name, _, _) in PROJECT_LEVEL_RULES {
            assert!(
                !BASE_GATED_RULES.iter().any(|(base, _, _)| *base == name),
                "{name} would get two reasons"
            );
        }
    }
}
