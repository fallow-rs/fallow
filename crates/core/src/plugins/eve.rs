//! eve agent framework plugin.
//!
//! eve builds an agent from the modules under `agent/`. The path of a module
//! selects its slot (`agent.ts`, `tools/`, `hooks/`, `channels/`,
//! `subagents/<id>/` and more), and eve reads the default export of each slot
//! module. Modules under a `lib/` directory are import-only helpers, and
//! files under `sandbox/workspace/` are seeded into the sandbox, so neither is
//! an entry point. `eve eval` loads each `evals/**/*.eval.*` file and the
//! `evals/evals.config.*` defaults file.

use super::{PathRule, Plugin, UsedExportRule};

/// Only the exact `eve` package activates the plugin. The entry patterns
/// match nothing in a project without the `agent/` and `evals/` layout.
const ENABLERS: &[&str] = &["eve"];

const ENTRY_PATTERNS: &[&str] = &[
    "agent/**/*.{ts,tsx,js,jsx,mts,mjs}",
    "evals/**/*.eval.{ts,js,mts,mjs}",
    "evals/evals.config.{ts,js,mts,mjs}",
];

/// Directories under `agent/` that hold no slot modules.
const NON_SLOT_GLOBS: &[&str] = &["agent/**/lib/**", "agent/**/sandbox/workspace/**"];

const DEFAULT_EXPORTS: &[&str] = &["default"];

const TOOLING_DEPENDENCIES: &[&str] = &["eve"];

pub struct EvePlugin;

impl Plugin for EvePlugin {
    fn name(&self) -> &'static str {
        "eve"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn script_enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn entry_pattern_rules(&self) -> Vec<PathRule> {
        ENTRY_PATTERNS
            .iter()
            .map(|pattern| {
                PathRule::new(*pattern).with_excluded_globs(NON_SLOT_GLOBS.iter().copied())
            })
            .collect()
    }

    fn used_export_rules(&self) -> Vec<UsedExportRule> {
        ENTRY_PATTERNS
            .iter()
            .map(|pattern| {
                UsedExportRule::new(*pattern, DEFAULT_EXPORTS.iter().copied())
                    .with_excluded_globs(NON_SLOT_GLOBS.iter().copied())
            })
            .collect()
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fallow_config::EntryPointRole;

    use super::*;

    #[test]
    fn activates_only_from_the_exact_eve_package() {
        let plugin = EvePlugin;
        let root = Path::new("/project");

        assert!(plugin.is_enabled_with_deps(&["eve".to_string()], root));
        assert!(!plugin.is_enabled_with_deps(&["eve-utils".to_string()], root));
        assert!(!plugin.is_enabled_with_deps(&["@eve/core".to_string()], root));
    }

    #[test]
    fn activates_from_the_eve_script_binary() {
        let plugin = EvePlugin;
        let scripts = rustc_hash::FxHashSet::from_iter(["eve".to_string()]);

        assert!(plugin.is_enabled_with_scripts(&scripts, Path::new("/project")));
    }

    #[test]
    fn every_entry_and_used_export_rule_skips_non_slot_directories() {
        let plugin = EvePlugin;

        for rule in plugin.entry_pattern_rules() {
            assert_eq!(rule.exclude_globs, NON_SLOT_GLOBS);
        }
        let export_rules = plugin.used_export_rules();
        assert_eq!(export_rules.len(), ENTRY_PATTERNS.len());
        for rule in export_rules {
            assert_eq!(rule.exports, DEFAULT_EXPORTS);
            assert_eq!(rule.path.exclude_globs, NON_SLOT_GLOBS);
        }
    }

    #[test]
    fn agent_modules_are_runtime_entry_points() {
        assert_eq!(EvePlugin.entry_point_role(), EntryPointRole::Runtime);
    }
}
