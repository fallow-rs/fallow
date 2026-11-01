//! The config entries that had no effect in a dead-code pass, as
//! `workspace_diagnostics[]` entries.

use fallow_config::{
    ResolvedConfig, RulesConfig, Severity, WorkspaceDiagnostic, WorkspaceDiagnosticKind,
};

/// `workspace_diagnostics[]` entries for the config entries that had no
/// effect in the latest dead-code pass over `config`.
///
/// In config order per setting:
///
/// - each `ignoreFindings` pattern that matched no finding;
/// - each `ignoreDependencies` glob that matched no declared dependency, and
///   each `ignoreDependencies` entry that hid no dependency finding;
/// - each `entry` pattern that matched only files that are entry points
///   anyway;
/// - each `ignorePatterns` glob that excluded no file the run would otherwise
///   analyze;
/// - each `overrides` rule that cannot change a result of this config.
///
/// The `ignoreDependencies` entries need `reports_dependencies`. A surface
/// passes `reports_dependencies = false` when its run does not report
/// dependency findings (an issue-type filter without the dependency types, or
/// a file scope), because a dependency entry is then not relevant to what the
/// run shows. A `--production` run checks fewer files and dependency
/// sections, so it reports no redundant entry (issue #3824).
///
/// An `overrides` rule reports also in a `--production` run, because its cause
/// does not depend on the files of the run. The `reports_dependencies` gate
/// removes the dependency rules (issue #4065). Read `config` after CLI flags
/// changed its rules, because a flag can start a detector.
///
/// The CLI, the programmatic API and the MCP typed path all build their
/// envelope from this one function, and the human note reads the same
/// entries, so every output states the same patterns.
#[must_use]
pub fn config_pattern_diagnostics(
    config: &ResolvedConfig,
    reports_dependencies: bool,
) -> Vec<WorkspaceDiagnostic> {
    let dependencies_reported = reports_dependencies && dependency_rules_on(&config.rules);
    let dependency_globs = if dependencies_reported {
        config.ignore_dependencies.unmatched_globs()
    } else {
        Vec::new()
    };
    let checks_redundancy = !config.production;
    let redundant_dependencies = if dependencies_reported && checks_redundancy {
        config.ignore_dependencies.redundant_entries()
    } else {
        Vec::new()
    };
    let (redundant_entries, redundant_ignores) = if checks_redundancy {
        (
            config
                .entry_pattern_effect
                .ineffective(&config.entry_patterns),
            config.ignore_patterns.redundant_user_patterns(),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let finding_patterns = config.ignore_findings.unmatched_patterns();
    let ineffective_overrides = config
        .ineffective_override_rules()
        .into_iter()
        .filter(|entry| reports_dependencies || !entry.dependency_rule);
    let kinds = finding_patterns
        .into_iter()
        .map(
            |pattern| WorkspaceDiagnosticKind::IgnoreFindingsPatternUnmatched {
                pattern: pattern.to_owned(),
            },
        )
        .chain(dependency_globs.into_iter().map(|pattern| {
            WorkspaceDiagnosticKind::IgnoreDependenciesGlobUnmatched {
                pattern: pattern.to_owned(),
            }
        }))
        .chain(redundant_entries.into_iter().map(|pattern| {
            WorkspaceDiagnosticKind::EntryPatternRedundant {
                pattern: pattern.to_owned(),
            }
        }))
        .chain(redundant_dependencies.into_iter().map(|pattern| {
            WorkspaceDiagnosticKind::IgnoreDependenciesEntryRedundant {
                pattern: pattern.to_owned(),
            }
        }))
        .chain(redundant_ignores.into_iter().map(|pattern| {
            WorkspaceDiagnosticKind::IgnorePatternsEntryRedundant {
                pattern: pattern.to_owned(),
            }
        }))
        .chain(ineffective_overrides.map(|entry| {
            WorkspaceDiagnosticKind::OverrideRuleIneffective {
                rule: entry.rule.to_owned(),
                files: entry.files,
                reason: entry.reason.to_owned(),
            }
        }));
    kinds
        .map(|kind| {
            WorkspaceDiagnostic::new(&config.root, config.root.clone(), kind)
                .into_root_relative(&config.root)
        })
        .collect()
}

/// Whether at least one rule that `ignoreDependencies` controls is on. With
/// every such rule off, the run reports no dependency finding at all.
fn dependency_rules_on(rules: &RulesConfig) -> bool {
    [
        rules.unused_dependencies,
        rules.unused_dev_dependencies,
        rules.unused_optional_dependencies,
        rules.unlisted_dependencies,
        rules.type_only_dependencies,
        rules.test_only_dependencies,
        rules.dev_dependencies_in_production,
    ]
    .into_iter()
    .any(|severity| severity != Severity::Off)
}
