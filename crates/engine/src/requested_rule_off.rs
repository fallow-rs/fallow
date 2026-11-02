//! The requested checks that did not run because their rule is off, as
//! `workspace_diagnostics[]` entries (issue #4067).

use fallow_config::{
    ResolvedConfig, RulesConfig, Severity, WorkspaceDiagnostic, WorkspaceDiagnosticKind,
};
use fallow_types::issue_meta::DEAD_CODE_FILTER_FLAGS;

type RuleSeverity = fn(&RulesConfig) -> Severity;
type RequestedRule = (&'static str, RuleSeverity);

const CYCLE_RULES: &[RequestedRule] = &[
    ("circular-dependencies", |r| r.circular_dependencies),
    ("re-export-cycle", |r| r.re_export_cycle),
    ("package-cycle", |r| r.package_cycle),
];

/// The rules that an explicit selector asks for, by config key. This table is
/// the one source of the note.
///
/// A rule is not in this table when one of these is true:
///
/// - The selector turns the rule on: `--private-type-leaks`,
///   `--absent-component-props` and `--deprecated-exports-in-use`.
/// - `off` is the documented way to narrow the selector:
///   `type-only-dependencies`, `test-only-dependencies` and
///   `dev-dependencies-in-production` under `--unused-deps`.
/// - The rule is opt-in and is not the subject of the selector:
///   `require-suppression-reason` under `--stale-suppressions`.
const REQUESTED_RULES: &[(&str, &[RequestedRule])] = &[
    ("--unused-files", &[("unused-files", |r| r.unused_files)]),
    (
        "--unused-exports",
        &[("unused-exports", |r| r.unused_exports)],
    ),
    (
        "--unused-deps",
        &[
            ("unused-dependencies", |r| r.unused_dependencies),
            ("unused-dev-dependencies", |r| r.unused_dev_dependencies),
            ("unused-optional-dependencies", |r| {
                r.unused_optional_dependencies
            }),
        ],
    ),
    ("--unused-types", &[("unused-types", |r| r.unused_types)]),
    (
        "--unused-enum-members",
        &[("unused-enum-members", |r| r.unused_enum_members)],
    ),
    (
        "--unused-class-members",
        &[("unused-class-members", |r| r.unused_class_members)],
    ),
    (
        "--unused-store-members",
        &[("unused-store-members", |r| r.unused_store_members)],
    ),
    (
        "--unprovided-injects",
        &[("unprovided-injects", |r| r.unprovided_injects)],
    ),
    (
        "--unrendered-components",
        &[("unrendered-components", |r| r.unrendered_components)],
    ),
    (
        "--unused-component-props",
        &[("unused-component-props", |r| r.unused_component_props)],
    ),
    (
        "--unused-component-emits",
        &[("unused-component-emits", |r| r.unused_component_emits)],
    ),
    (
        "--unused-component-inputs",
        &[("unused-component-inputs", |r| r.unused_component_inputs)],
    ),
    (
        "--unused-component-outputs",
        &[("unused-component-outputs", |r| r.unused_component_outputs)],
    ),
    (
        "--unused-svelte-events",
        &[("unused-svelte-events", |r| r.unused_svelte_events)],
    ),
    (
        "--unused-server-actions",
        &[("unused-server-actions", |r| r.unused_server_actions)],
    ),
    (
        "--unused-load-data-keys",
        &[("unused-load-data-keys", |r| r.unused_load_data_keys)],
    ),
    (
        "--unused-effect-layers",
        &[("unused-effect-layers", |r| r.unused_effect_layers)],
    ),
    (
        "--unused-tagged-errors",
        &[("unused-tagged-errors", |r| r.unused_tagged_errors)],
    ),
    (
        "--unproduced-catch-tags",
        &[("unproduced-catch-tags", |r| r.unproduced_catch_tags)],
    ),
    (
        "--unresolved-imports",
        &[("unresolved-imports", |r| r.unresolved_imports)],
    ),
    (
        "--unlisted-deps",
        &[("unlisted-dependencies", |r| r.unlisted_dependencies)],
    ),
    (
        "--duplicate-exports",
        &[("duplicate-exports", |r| r.duplicate_exports)],
    ),
    (
        "--circular-deps",
        &[("circular-dependencies", |r| r.circular_dependencies)],
    ),
    (
        "--re-export-cycles",
        &[("re-export-cycle", |r| r.re_export_cycle)],
    ),
    (
        "--package-cycles",
        &[("package-cycle", |r| r.package_cycle)],
    ),
    (
        "--boundary-violations",
        &[("boundary-violation", |r| r.boundary_violation)],
    ),
    (
        "--policy-violations",
        &[("policy-violation", |r| r.policy_violation)],
    ),
    (
        "--stale-suppressions",
        &[("stale-suppressions", |r| r.stale_suppressions)],
    ),
    (
        "--unused-catalog-entries",
        &[("unused-catalog-entries", |r| r.unused_catalog_entries)],
    ),
    (
        "--empty-catalog-groups",
        &[("empty-catalog-groups", |r| r.empty_catalog_groups)],
    ),
    (
        "--unresolved-catalog-references",
        &[("unresolved-catalog-references", |r| {
            r.unresolved_catalog_references
        })],
    ),
    (
        "--unused-dependency-overrides",
        &[("unused-dependency-overrides", |r| {
            r.unused_dependency_overrides
        })],
    ),
    (
        "--misconfigured-dependency-overrides",
        &[("misconfigured-dependency-overrides", |r| {
            r.misconfigured_dependency_overrides
        })],
    ),
    (ARCHITECTURE_CYCLES, CYCLE_RULES),
    (
        ARCHITECTURE_BOUNDARIES,
        &[("boundary-violation", |r| r.boundary_violation)],
    ),
    (
        ARCHITECTURE_POLICY,
        &[("policy-violation", |r| r.policy_violation)],
    ),
];

/// The selector of the dependency rules, which a `--file` run never checks.
const UNUSED_DEPS_FLAG: &str = "--unused-deps";

const ARCHITECTURE_CYCLES: &str = "--cycles";
const ARCHITECTURE_BOUNDARIES: &str = "--boundaries";
const ARCHITECTURE_POLICY: &str = "--policy";

/// The rules that production mode forces off in the resolved config. The
/// config does not say `off`, and setting the rule on changes nothing in
/// production mode, so a note about these keys is false.
const PRODUCTION_FORCED_OFF: [&str; 2] =
    ["unused-dev-dependencies", "unused-optional-dependencies"];

/// The selector flags of `fallow architecture` that a caller set. An empty list
/// means that the caller asked for no specific check.
#[must_use]
pub fn architecture_requested_flags(
    cycles: bool,
    boundaries: bool,
    policy: bool,
) -> Vec<&'static str> {
    [
        (ARCHITECTURE_CYCLES, cycles),
        (ARCHITECTURE_BOUNDARIES, boundaries),
        (ARCHITECTURE_POLICY, policy),
    ]
    .into_iter()
    .filter_map(|(flag, set)| set.then_some(flag))
    .collect()
}

/// The dead-code filter flags for which `is_active` answers true, in registry
/// order. The CLI and the typed API share this walk, so a new flag in the
/// registry reaches both.
#[must_use]
pub fn explicit_filter_flags(is_active: impl Fn(&str) -> bool) -> Vec<&'static str> {
    DEAD_CODE_FILTER_FLAGS
        .iter()
        .copied()
        .filter(|flag| is_active(flag))
        .collect()
}

/// One `requested-rule-off` entry for each rule that `requested_flags` asks for
/// and `config` has at `off`, deduplicated by config key.
///
/// Pass `reports_dependencies = false` when the run drops dependency findings,
/// as a `--file` run does. A rule at `error` would then still check nothing,
/// so the `--unused-deps` entries would give a false remedy.
///
/// Read `config` after CLI flags changed its rules: an opt-in flag starts its
/// detector. The session stores nothing, so a long-lived process cannot keep an
/// entry of an earlier pass.
#[must_use]
pub fn requested_rule_off_diagnostics(
    config: &ResolvedConfig,
    requested_flags: &[&str],
    reports_dependencies: bool,
) -> Vec<WorkspaceDiagnostic> {
    let mut seen: Vec<&str> = Vec::new();
    let mut diagnostics = Vec::new();
    for requested in requested_flags {
        let Some((flag, rules)) = REQUESTED_RULES.iter().find(|(flag, _)| flag == requested) else {
            continue;
        };
        if !reports_dependencies && *flag == UNUSED_DEPS_FLAG {
            continue;
        }
        for (rule, severity) in *rules {
            if severity(&config.rules) != Severity::Off
                || (config.production && PRODUCTION_FORCED_OFF.contains(rule))
                || seen.contains(rule)
            {
                continue;
            }
            seen.push(rule);
            diagnostics.push(
                WorkspaceDiagnostic::new(
                    &config.root,
                    config.root.clone(),
                    WorkspaceDiagnosticKind::RequestedRuleOff {
                        rule: (*rule).to_owned(),
                        flag: (*flag).to_owned(),
                    },
                )
                .into_root_relative(&config.root),
            );
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use fallow_types::issue_meta::ISSUE_KIND_META;

    use super::*;

    /// Each `(flag, config key)` of the registry that fires no note, with the
    /// reason. A new row must join the table above or this list.
    const EXCLUDED_ROWS: &[(&str, &str, &str)] = &[
        (
            "--private-type-leaks",
            "private-type-leaks",
            "the flag turns the rule on",
        ),
        (
            "--deprecated-exports-in-use",
            "deprecated-exports-in-use",
            "the flag turns the rule on",
        ),
        (
            "--absent-component-props",
            "absent-component-props",
            "the flag turns the rule on",
        ),
        (
            "--unused-deps",
            "type-only-dependencies",
            "off is the documented way to narrow the flag",
        ),
        (
            "--unused-deps",
            "test-only-dependencies",
            "off is the documented way to narrow the flag",
        ),
        (
            "--unused-deps",
            "dev-dependencies-in-production",
            "off is the documented way to narrow the flag",
        ),
        (
            "--stale-suppressions",
            "require-suppression-reason",
            "an opt-in rule that defaults to off, not the subject of the flag",
        ),
    ];

    fn table_has(flag: &str, key: &str) -> bool {
        REQUESTED_RULES.iter().any(|(table_flag, rules)| {
            *table_flag == flag && rules.iter().any(|(rule, _)| *rule == key)
        })
    }

    #[test]
    fn every_filter_flag_row_is_in_the_table_or_a_named_exclusion() {
        for row in ISSUE_KIND_META {
            let (Some(flag), Some(key)) = (row.filter_flag, row.config_key) else {
                continue;
            };
            let excluded = EXCLUDED_ROWS
                .iter()
                .any(|(excluded_flag, rule, _)| *excluded_flag == flag && *rule == key);
            assert!(
                table_has(flag, key) != excluded,
                "{flag} / {key} must be in exactly one of the table and the exclusions"
            );
        }
    }

    #[test]
    fn every_table_key_and_exclusion_is_a_registry_row_or_an_architecture_selector() {
        for (flag, rules) in REQUESTED_RULES {
            for (key, _) in *rules {
                let registered = ISSUE_KIND_META
                    .iter()
                    .any(|row| row.config_key == Some(key));
                assert!(registered, "{flag} names unknown rule {key}");
            }
        }
        for (flag, key, _) in EXCLUDED_ROWS {
            assert!(
                ISSUE_KIND_META
                    .iter()
                    .any(|row| row.filter_flag == Some(flag) && row.config_key == Some(key)),
                "stale exclusion {flag} / {key}"
            );
        }
    }

    fn all_off(production: bool) -> ResolvedConfig {
        let mut config = fallow_config::FallowConfig::default().resolve(
            std::path::PathBuf::from("/project"),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        config.production = production;
        let off = serde_json::Map::from_iter(
            ISSUE_KIND_META
                .iter()
                .filter_map(|row| row.config_key)
                .map(|key| (key.to_owned(), serde_json::json!("off"))),
        );
        config.rules =
            serde_json::from_value(serde_json::Value::Object(off)).expect("rules deserialize");
        config
    }

    fn rules_of(diagnostics: &[WorkspaceDiagnostic]) -> Vec<(&str, &str)> {
        diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.kind.requested_rule_off())
            .collect()
    }

    /// With every rule off, each registered flag fires exactly its table rows,
    /// and the function reads the severity by the row's key.
    #[test]
    fn each_filter_flag_fires_the_rules_of_its_table_row() {
        let config = all_off(false);
        for flag in DEAD_CODE_FILTER_FLAGS.iter().copied() {
            let expected: Vec<(&str, &str)> = ISSUE_KIND_META
                .iter()
                .filter(|row| row.filter_flag == Some(flag))
                .filter_map(|row| row.config_key)
                .filter(|key| {
                    !EXCLUDED_ROWS
                        .iter()
                        .any(|(excluded, rule, _)| *excluded == flag && rule == key)
                })
                .fold(Vec::new(), |mut keys, key| {
                    if !keys.iter().any(|(known, _)| *known == key) {
                        keys.push((key, flag));
                    }
                    keys
                });
            assert_eq!(
                rules_of(&requested_rule_off_diagnostics(&config, &[flag], true)),
                expected,
                "{flag}"
            );
        }
    }

    #[test]
    fn a_rule_that_is_on_fires_no_entry() {
        let mut config = all_off(false);
        config.rules.unused_files = Severity::Warn;
        assert!(requested_rule_off_diagnostics(&config, &["--unused-files"], true).is_empty());
        assert_eq!(
            requested_rule_off_diagnostics(&config, &["--unused-exports"], true).len(),
            1,
            "positive control"
        );
        config.rules = RulesConfig::default();
        for flag in DEAD_CODE_FILTER_FLAGS.iter().copied() {
            let entries = requested_rule_off_diagnostics(&config, &[flag], true);
            assert!(rules_of(&entries).is_empty(), "{flag}: {entries:?}");
        }
    }

    #[test]
    fn production_silences_only_the_rules_it_forces_off() {
        assert_eq!(
            rules_of(&requested_rule_off_diagnostics(
                &all_off(true),
                &["--unused-deps"],
                true
            )),
            vec![("unused-dependencies", "--unused-deps")]
        );
        assert_eq!(
            rules_of(&requested_rule_off_diagnostics(
                &all_off(false),
                &["--unused-deps"],
                true
            )),
            vec![
                ("unused-dependencies", "--unused-deps"),
                ("unused-dev-dependencies", "--unused-deps"),
                ("unused-optional-dependencies", "--unused-deps"),
            ]
        );
    }

    #[test]
    fn a_run_without_dependency_findings_skips_only_the_dependency_rules() {
        assert_eq!(
            rules_of(&requested_rule_off_diagnostics(
                &all_off(false),
                &["--unused-deps", "--unused-files"],
                false,
            )),
            vec![("unused-files", "--unused-files")]
        );
    }

    #[test]
    fn an_empty_selection_and_an_unknown_flag_fire_nothing() {
        let config = all_off(false);
        assert!(requested_rule_off_diagnostics(&config, &[], true).is_empty());
        assert!(requested_rule_off_diagnostics(&config, &["--not-a-flag"], true).is_empty());
        assert_eq!(
            requested_rule_off_diagnostics(&config, &["--not-a-flag", "--unused-files"], true)
                .len(),
            1,
            "positive control"
        );
    }

    #[test]
    fn architecture_selectors_map_to_their_rules_and_dedupe_by_key() {
        let config = all_off(false);
        assert_eq!(
            rules_of(&requested_rule_off_diagnostics(
                &config,
                &architecture_requested_flags(true, true, true),
                true,
            )),
            vec![
                ("circular-dependencies", "--cycles"),
                ("re-export-cycle", "--cycles"),
                ("package-cycle", "--cycles"),
                ("boundary-violation", "--boundaries"),
                ("policy-violation", "--policy"),
            ]
        );
        assert!(architecture_requested_flags(false, false, false).is_empty());
        assert_eq!(
            rules_of(&requested_rule_off_diagnostics(
                &config,
                &["--cycles", "--circular-deps"],
                true,
            ))
            .len(),
            3,
            "one entry per config key"
        );
    }

    #[test]
    fn the_stale_suppressions_flag_ignores_the_opt_in_reason_rule() {
        let mut config = all_off(false);
        config.rules.require_suppression_reason = Severity::Error;
        assert_eq!(
            rules_of(&requested_rule_off_diagnostics(
                &config,
                &["--stale-suppressions"],
                true
            )),
            vec![("stale-suppressions", "--stale-suppressions")]
        );
    }
}
