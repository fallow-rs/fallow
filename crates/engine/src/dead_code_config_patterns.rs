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
/// - each `circularDependencies.allow` entry that matched no cycle;
/// - each `ignoreExports` file glob that matched no file, and each
///   `ignoreExports` name that no file of its glob declares;
/// - each `duplicates.ignore` and `health.ignore` glob that matched no file;
/// - each `overrides` rule that cannot change a result of this config;
/// - each `overrides[].files` glob that matched no project file.
///
/// The `ignoreDependencies` entries need `reports_dependencies`. A surface
/// passes `reports_dependencies = false` when its run does not report
/// dependency findings (an issue-type filter without the dependency types, or
/// a file scope), because a dependency entry is then not relevant to what the
/// run shows. A `--production` run checks fewer files and dependency
/// sections, so it reports no redundant entry (issue #3824), no unmatched
/// `circularDependencies.allow` entry (a cycle through a test file is not
/// in that run) and no unmatched ignore list entry (a glob for test files
/// matches no file of that run).
/// A normal run does not report an `ignoreDependencies` entry that a
/// `--production` run could need, because following the note would add a
/// finding to that run.
///
/// An `overrides` rule reports also in a `--production` run, because its cause
/// does not depend on the files of the run. The same is true for an
/// `overrides[].files` glob: its check reads every project file, so the
/// answer does not depend on `--production`, `--changed-since` or
/// `--workspace` (issue #5227). The `reports_dependencies` gate
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
    let ignore_lists = if checks_redundancy {
        unmatched_ignore_list_kinds(config)
    } else {
        Vec::new()
    };
    let finding_patterns = config.ignore_findings.unmatched_patterns();
    let unmatched_cycles = if checks_redundancy {
        config
            .circular_dependencies_allow_effect
            .ineffective(&config.circular_dependencies.allow)
    } else {
        Vec::new()
    };
    let ineffective_overrides = config
        .ineffective_override_rules()
        .into_iter()
        .filter(|entry| reports_dependencies || !entry.dependency_rule);
    let unmatched_override_globs = super::override_globs::unmatched_override_globs(config);
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
        .chain(unmatched_cycles.into_iter().map(|files| {
            WorkspaceDiagnosticKind::CircularDependenciesAllowUnmatched {
                pattern: format!("[{}]", files.join(", ")),
                files: files.clone(),
            }
        }))
        .chain(ignore_lists)
        .chain(ineffective_overrides.map(|entry| {
            WorkspaceDiagnosticKind::OverrideRuleIneffective {
                rule: entry.rule.to_owned(),
                files: entry.files,
                reason: entry.reason.to_owned(),
            }
        }))
        .chain(unmatched_override_globs);
    kinds
        .map(|kind| {
            WorkspaceDiagnostic::new(&config.root, config.root.clone(), kind)
                .into_root_relative(&config.root)
        })
        .collect()
}

/// The `ignoreExports`, `duplicates.ignore` and `health.ignore` entries that
/// matched no file or export of the latest dead-code pass, in that order and
/// in config order per setting (issue #5167).
fn unmatched_ignore_list_kinds(config: &ResolvedConfig) -> Vec<WorkspaceDiagnosticKind> {
    let effect = &config.ignore_list_effect;
    let unmatched_rules: Vec<&str> = effect
        .ignore_export_files
        .ineffective(&config.ignore_export_rules)
        .into_iter()
        .map(|rule| rule.file.as_str())
        .collect();
    let files =
        unmatched_rules.iter().map(
            |pattern| WorkspaceDiagnosticKind::IgnoreExportsFileUnmatched {
                pattern: (*pattern).to_owned(),
            },
        );
    // A rule whose glob matched no file already has a note; a note per name
    // of that rule would repeat it.
    let names = config.ignore_export_names();
    let names = effect
        .ignore_export_names
        .ineffective(&names)
        .into_iter()
        .filter(|name| !unmatched_rules.contains(&name.file))
        .map(|name| WorkspaceDiagnosticKind::IgnoreExportsNameUnmatched {
            pattern: format!("{} in {}", name.export, name.file),
            file: name.file.to_owned(),
            export: name.export.to_owned(),
        });
    let duplicates = effect
        .duplicates_ignore
        .ineffective(&config.duplicates.ignore)
        .into_iter()
        .map(
            |pattern| WorkspaceDiagnosticKind::DuplicatesIgnoreUnmatched {
                pattern: pattern.clone(),
            },
        );
    let health = effect
        .health_ignore
        .ineffective(&config.health.ignore)
        .into_iter()
        .map(|pattern| WorkspaceDiagnosticKind::HealthIgnoreUnmatched {
            pattern: pattern.clone(),
        });
    files.chain(names).chain(duplicates).chain(health).collect()
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
