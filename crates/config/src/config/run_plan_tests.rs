use super::*;

use AnalysisRequest::{Default as D, Excluded as X, Named as N};
use AnalysisSelection::{Run, Skipped};
use ConfigKey::{AuditDeadCode, AuditDupes, AuditHealth, DuplicatesEnabled};
use PlannedAnalysis::{DeadCode, Duplication, Health};
use RunCommand::{Audit, Combined};
use SkipReason::{Config, NotRequested};

const ANALYSES: [PlannedAnalysis; 3] = [DeadCode, Duplication, Health];
const COMMANDS: [RunCommand; 2] = [Combined, Audit];
const REQUESTS: [AnalysisRequest; 3] = [N, D, X];
const KEY_VALUES: [Option<bool>; 3] = [None, Some(true), Some(false)];

/// The `audit.*` key column of a rule row.
#[derive(Clone, Copy)]
enum Key {
    /// Every value.
    Any,
    /// The key is not set.
    Unset,
    /// The key is `true`.
    On,
    /// The key is `false`.
    Off,
}

impl Key {
    const fn matches(self, value: Option<bool>) -> bool {
        matches!(
            (self, value),
            (Self::Any, _)
                | (Self::Unset, None)
                | (Self::On, Some(true))
                | (Self::Off, Some(false))
        )
    }
}

/// One row of the rule for one analysis. `None` in a column matches every
/// value. `key` is the `audit.*` key of the analysis itself.
struct Row {
    analysis: Option<PlannedAnalysis>,
    command: Option<RunCommand>,
    request: AnalysisRequest,
    enabled: Option<bool>,
    key: Key,
    expected: AnalysisSelection,
}

const fn row(
    analysis: Option<PlannedAnalysis>,
    command: Option<RunCommand>,
    request: AnalysisRequest,
    enabled: Option<bool>,
    key: Key,
    expected: AnalysisSelection,
) -> Row {
    Row {
        analysis,
        command,
        request,
        enabled,
        key,
        expected,
    }
}

/// The rule, as literal rows. Each combination of inputs matches one row.
const RULE: [Row; 16] = [
    row(None, None, N, None, Key::Any, Run),
    row(None, None, X, None, Key::Any, Skipped(NotRequested)),
    row(Some(DeadCode), Some(Combined), D, None, Key::Any, Run),
    row(Some(Health), Some(Combined), D, None, Key::Any, Run),
    row(
        Some(Duplication),
        Some(Combined),
        D,
        Some(true),
        Key::Any,
        Run,
    ),
    row(
        Some(Duplication),
        Some(Combined),
        D,
        Some(false),
        Key::Any,
        Skipped(Config(DuplicatesEnabled)),
    ),
    row(Some(DeadCode), Some(Audit), D, None, Key::Unset, Run),
    row(Some(DeadCode), Some(Audit), D, None, Key::On, Run),
    row(
        Some(DeadCode),
        Some(Audit),
        D,
        None,
        Key::Off,
        Skipped(Config(AuditDeadCode)),
    ),
    row(Some(Health), Some(Audit), D, None, Key::Unset, Run),
    row(Some(Health), Some(Audit), D, None, Key::On, Run),
    row(
        Some(Health),
        Some(Audit),
        D,
        None,
        Key::Off,
        Skipped(Config(AuditHealth)),
    ),
    row(
        Some(Duplication),
        Some(Audit),
        D,
        None,
        Key::Off,
        Skipped(Config(AuditDupes)),
    ),
    // `audit.dupes: true` wins over `duplicates.enabled: false`.
    row(Some(Duplication), Some(Audit), D, None, Key::On, Run),
    row(
        Some(Duplication),
        Some(Audit),
        D,
        Some(true),
        Key::Unset,
        Run,
    ),
    row(
        Some(Duplication),
        Some(Audit),
        D,
        Some(false),
        Key::Unset,
        Skipped(Config(DuplicatesEnabled)),
    ),
];

fn duplicates(enabled: bool) -> DuplicatesConfig {
    DuplicatesConfig {
        enabled,
        ..DuplicatesConfig::default()
    }
}

fn audit(dead_code: Option<bool>, health: Option<bool>, dupes: Option<bool>) -> AuditConfig {
    AuditConfig {
        dead_code,
        health,
        dupes,
        ..AuditConfig::default()
    }
}

fn plan(enabled: bool, audit_config: &AuditConfig, request: RunRequest) -> RunPlan {
    plan_run(
        PlanSections {
            duplicates: &duplicates(enabled),
            audit: audit_config,
        },
        request,
    )
}

const fn own_key(analysis: PlannedAnalysis, config: &AuditConfig) -> Option<bool> {
    match analysis {
        PlannedAnalysis::DeadCode => config.dead_code,
        PlannedAnalysis::Duplication => config.dupes,
        PlannedAnalysis::Health => config.health,
    }
}

const fn request_of(request: RunRequest, analysis: PlannedAnalysis) -> AnalysisRequest {
    match analysis {
        PlannedAnalysis::DeadCode => request.dead_code,
        PlannedAnalysis::Duplication => request.duplication,
        PlannedAnalysis::Health => request.health,
    }
}

const fn decision_of(plan: RunPlan, analysis: PlannedAnalysis) -> AnalysisSelection {
    match analysis {
        PlannedAnalysis::DeadCode => plan.dead_code,
        PlannedAnalysis::Duplication => plan.duplication,
        PlannedAnalysis::Health => plan.health,
    }
}

fn matches_column<T: PartialEq + Copy>(column: Option<T>, value: T) -> bool {
    column.is_none_or(|expected| expected == value)
}

/// Every command, request triple, `duplicates.enabled` value and `audit.*`
/// triple. Each analysis of each plan equals the one rule row that matches.
#[test]
fn plan_run_follows_the_rule_table_for_every_input() {
    let mut used = [false; RULE.len()];
    for command in COMMANDS {
        for dead_code in REQUESTS {
            for duplication in REQUESTS {
                for health in REQUESTS {
                    let request = RunRequest {
                        command,
                        dead_code,
                        duplication,
                        health,
                    };
                    check_every_config(request, &mut used);
                }
            }
        }
    }
    assert!(used.iter().all(|row| *row), "unused rule rows: {used:?}");
}

fn check_every_config(request: RunRequest, used: &mut [bool; RULE.len()]) {
    for enabled in [true, false] {
        for dead_code in KEY_VALUES {
            for health in KEY_VALUES {
                for dupes in KEY_VALUES {
                    let config = audit(dead_code, health, dupes);
                    let plan = plan(enabled, &config, request);
                    for analysis in ANALYSES {
                        let matching: Vec<usize> = RULE
                            .iter()
                            .enumerate()
                            .filter(|(_, row)| {
                                matches_column(row.analysis, analysis)
                                    && matches_column(row.command, request.command)
                                    && row.request == request_of(request, analysis)
                                    && matches_column(row.enabled, enabled)
                                    && row.key.matches(own_key(analysis, &config))
                            })
                            .map(|(index, _)| index)
                            .collect();
                        let [index] = matching[..] else {
                            panic!(
                                "{analysis:?} {request:?} {enabled} {config:?}: rows {matching:?}"
                            );
                        };
                        used[index] = true;
                        assert_eq!(
                            decision_of(plan, analysis),
                            RULE[index].expected,
                            "{analysis:?} {request:?} enabled {enabled} {config:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn audit_dupes_true_runs_duplication_with_duplicates_enabled_false() {
    let request = RunRequest::from_options(Audit, None, None, None);
    let plan = plan(false, &audit(None, None, Some(true)), request);
    assert_eq!(plan.duplication, Run);
}

#[test]
fn combined_ignores_the_audit_keys() {
    let request = RunRequest::from_lists(Combined, &[], &[]);
    let config = audit(Some(false), Some(false), Some(false));
    let plan = plan(true, &config, request);
    assert_eq!(
        plan,
        RunPlan {
            dead_code: Run,
            duplication: Run,
            health: Run,
        }
    );
    assert!(plan.runs_any());
}

#[test]
fn reads_config_only_when_a_default_request_meets_a_key() {
    assert!(RunRequest::from_lists(Combined, &[], &[]).reads_config());
    assert!(!RunRequest::from_lists(Combined, &[], &[Duplication]).reads_config());
    assert!(!RunRequest::from_lists(Combined, &[Duplication, Health], &[]).reads_config());
    assert!(RunRequest::from_lists(Audit, &[], &[Duplication]).reads_config());
    assert!(!RunRequest::from_lists(Audit, &[DeadCode], &[]).reads_config());
    assert!(!RunRequest::all_named(Audit).reads_config());
    assert!(!RunRequest::all_named(Combined).reads_config());
}

#[test]
fn from_lists_names_only_and_excludes_skip() {
    let only = RunRequest::from_lists(Combined, &[Duplication, Health], &[]);
    assert_eq!((only.dead_code, only.duplication, only.health), (X, N, N));

    let skip = RunRequest::from_lists(Audit, &[], &[DeadCode]);
    assert_eq!((skip.dead_code, skip.duplication, skip.health), (X, D, D));
    assert_eq!(skip.command, Audit);

    let none = RunRequest::from_lists(Combined, &[], &[]);
    assert_eq!((none.dead_code, none.duplication, none.health), (D, D, D));
    assert!(ANALYSES.iter().all(|analysis| none.requested(*analysis)));
    assert!(!skip.requested(DeadCode));
}

#[test]
fn from_booleans_names_a_lone_analysis_only() {
    let alone = RunRequest::from_booleans(Combined, false, true, false);
    assert_eq!(
        (alone.dead_code, alone.duplication, alone.health),
        (X, N, X)
    );

    let pair = RunRequest::from_booleans(Combined, true, true, false);
    assert_eq!((pair.dead_code, pair.duplication, pair.health), (D, D, X));

    let all = RunRequest::from_booleans(Combined, true, true, true);
    assert_eq!((all.dead_code, all.duplication, all.health), (D, D, D));

    let lone_dead_code = RunRequest::from_booleans(Combined, true, false, false);
    assert_eq!(
        (
            lone_dead_code.dead_code,
            lone_dead_code.duplication,
            lone_dead_code.health
        ),
        (N, X, X)
    );
}

#[test]
fn from_options_maps_each_option() {
    assert_eq!(AnalysisRequest::from_option(Some(true)), N);
    assert_eq!(AnalysisRequest::from_option(Some(false)), X);
    assert_eq!(AnalysisRequest::from_option(None), D);
    let request = RunRequest::from_options(Audit, Some(true), None, Some(false));
    assert_eq!(
        (request.dead_code, request.duplication, request.health),
        (N, D, X)
    );
    assert_eq!(
        RunRequest::all_named(Audit),
        RunRequest::from_options(Audit, Some(true), Some(true), Some(true))
    );
}

#[test]
fn selection_reports_the_key_that_turned_it_off() {
    assert!(Skipped(Config(AuditDupes)).off_by(AuditDupes));
    assert!(Skipped(Config(DuplicatesEnabled)).off_by(DuplicatesEnabled));
    assert!(!Skipped(Config(AuditDupes)).off_by(DuplicatesEnabled));
    assert!(!Skipped(NotRequested).off_by(DuplicatesEnabled));
    assert!(!Run.off_by(DuplicatesEnabled));
}

#[test]
fn runs_by_default_is_the_combined_default_plan() {
    for enabled in [true, false] {
        let config = duplicates(enabled);
        let plan = plan_run(
            PlanSections {
                duplicates: &config,
                audit: &AuditConfig::default(),
            },
            RunRequest::from_lists(Combined, &[], &[]),
        );
        assert_eq!(config.runs_by_default(), plan.duplication.runs());
        assert_eq!(config.runs_by_default(), enabled);
    }
}

#[test]
fn plan_sections_point_at_the_config_sections() {
    let config = FallowConfig {
        duplicates: duplicates(false),
        audit: audit(Some(false), None, None),
        ..FallowConfig::default()
    };
    let sections = config.plan_sections();
    assert!(!sections.duplicates.enabled);
    assert_eq!(sections.audit.dead_code, Some(false));
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_request() -> impl Strategy<Value = AnalysisRequest> {
        prop_oneof![Just(N), Just(D), Just(X)]
    }

    fn arb_command() -> impl Strategy<Value = RunCommand> {
        prop_oneof![Just(Combined), Just(Audit)]
    }

    fn arb_key() -> impl Strategy<Value = Option<bool>> {
        prop_oneof![Just(None), Just(Some(true)), Just(Some(false))]
    }

    fn arb_config() -> impl Strategy<Value = (bool, AuditConfig)> {
        (any::<bool>(), arb_key(), arb_key(), arb_key()).prop_map(
            |(enabled, dead_code, health, dupes)| (enabled, audit(dead_code, health, dupes)),
        )
    }

    proptest! {
        /// A request that does not read the config gives one plan under
        /// every config.
        #[test]
        fn a_request_that_reads_no_config_ignores_the_config(
            command in arb_command(),
            dead_code in arb_request(),
            duplication in arb_request(),
            health in arb_request(),
            first in arb_config(),
            second in arb_config(),
        ) {
            let request = RunRequest { command, dead_code, duplication, health };
            prop_assume!(!request.reads_config());
            prop_assert_eq!(
                plan(first.0, &first.1, request),
                plan(second.0, &second.1, request)
            );
        }
    }
}
