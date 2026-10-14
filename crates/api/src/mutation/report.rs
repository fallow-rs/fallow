//! Builds the public mutation envelopes from the plan and the schedule.

use std::collections::BTreeMap;

use fallow_engine::mutation::{MutantDisposition, MutationPlan, PlannedMutant, PruneReason};
use fallow_output::{
    MutantLocation, MutantResult, MutantStatus, MutationBaselineFailure, MutationCompletion,
    MutationFunctionResult, MutationIncompleteReason, MutationPlanCountsOutput,
    MutationPlanDisposition, MutationPlanFile, MutationPlanMutant, MutationRunIssue,
    MutationSkippedFile, MutationStatusCounts, MutationSummary,
};
use fallow_types::envelope::ToolVersion;
use rustc_hash::FxHashSet;

use super::scheduler::ScheduleOutcome;

/// The kebab-case name of a serde enum value.
fn kebab<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub fn tool_version() -> ToolVersion {
    ToolVersion(env!("CARGO_PKG_VERSION").to_owned())
}

fn location(mutant: &PlannedMutant) -> MutantLocation {
    MutantLocation {
        mutant_id: mutant.mutant.id.as_str().to_owned(),
        file: mutant.file.clone(),
        line: mutant.mutant.line,
        column: mutant.mutant.column + 1,
        span: mutant.mutant.span,
        operator: mutant.mutant.operator.as_str().to_owned(),
        function: mutant
            .function
            .as_ref()
            .and_then(|function| function.name.clone()),
        function_line: mutant.function.as_ref().map(|function| function.line),
        rank: mutant.rank,
    }
}

fn prune_reason(mutant: &PlannedMutant) -> Option<String> {
    match mutant.disposition {
        MutantDisposition::Pruned(reason) => Some(kebab::<PruneReason>(&reason)),
        _ => None,
    }
}

/// The skipped files of a plan.
pub fn skipped_files(plan: &MutationPlan) -> Vec<MutationSkippedFile> {
    plan.skipped_files
        .iter()
        .map(|file| MutationSkippedFile {
            path: file.path.clone(),
            reason: kebab(&file.reason),
        })
        .collect()
}

/// The plan mutants as public plan entries.
pub fn plan_mutants(plan: &MutationPlan) -> Vec<MutationPlanMutant> {
    plan.mutants
        .iter()
        .map(|mutant| MutationPlanMutant {
            location: location(mutant),
            disposition: match mutant.disposition {
                MutantDisposition::Planned => MutationPlanDisposition::Planned,
                MutantDisposition::Pending => MutationPlanDisposition::Pending,
                MutantDisposition::NoCoverage => MutationPlanDisposition::NoCoverage,
                MutantDisposition::Pruned(_) => MutationPlanDisposition::Pruned,
            },
            prune_reason: prune_reason(mutant),
        })
        .collect()
}

/// The plan totals.
pub const fn plan_counts(plan: &MutationPlan) -> MutationPlanCountsOutput {
    MutationPlanCountsOutput {
        total: plan.counts.total,
        planned: plan.counts.planned,
        pending: plan.counts.pending,
        no_coverage: plan.counts.no_coverage,
        pruned_dead_code: plan.counts.pruned.dead_code,
        pruned_generated: plan.counts.pruned.generated,
        pruned_side_effect_sink: plan.counts.pruned.side_effect_sink,
        pruned_framework_config: plan.counts.pruned.framework_config,
    }
}

/// The plan files, with no source text.
pub fn plan_files(plan: &MutationPlan) -> Vec<MutationPlanFile> {
    plan.files
        .iter()
        .map(|file| MutationPlanFile {
            path: file.path.clone(),
            source_sha256: file.source_sha256.clone(),
            static_test_files: file.static_test_files.clone(),
        })
        .collect()
}

/// The result of each plan mutant: the plan disposition, or the scheduler
/// outcome of a planned mutant. A planned mutant with no outcome is
/// `pending`. A pending mutant that the dry run never covered is
/// `no_coverage`. A statically `no_coverage` mutant with an outcome is a static
/// map miss, and the outcome of the dry-run map wins.
pub fn mutant_results(plan: &MutationPlan, outcome: Option<&ScheduleOutcome>) -> Vec<MutantResult> {
    plan.mutants
        .iter()
        .map(|mutant| {
            let base = MutantResult {
                location: location(mutant),
                status: MutantStatus::Pending,
                prune_reason: prune_reason(mutant),
                killed_by: None,
                covering_tests: Vec::new(),
                static_mutant: false,
                duration_ms: None,
            };
            let scheduled =
                outcome.and_then(|outcome| outcome.results.get(mutant.mutant.id.as_str()));
            match (mutant.disposition, scheduled) {
                (MutantDisposition::Pruned(_), _) => MutantResult {
                    status: MutantStatus::Pruned,
                    ..base
                },
                (MutantDisposition::Planned | MutantDisposition::NoCoverage, Some(result)) => {
                    MutantResult {
                        status: result.status,
                        killed_by: result.killed_by.clone(),
                        covering_tests: result.covering_tests.clone(),
                        static_mutant: result.static_mutant,
                        duration_ms: result.duration_ms,
                        ..base
                    }
                }
                (MutantDisposition::NoCoverage, None) => MutantResult {
                    status: MutantStatus::NoCoverage,
                    ..base
                },
                (MutantDisposition::Pending, Some(result))
                    if result.status == MutantStatus::NoCoverage =>
                {
                    MutantResult {
                        status: MutantStatus::NoCoverage,
                        static_mutant: result.static_mutant,
                        ..base
                    }
                }
                (MutantDisposition::Planned | MutantDisposition::Pending, _) => base,
            }
        })
        .collect()
}

/// The totals of `results`.
pub fn summary(results: &[MutantResult]) -> MutationSummary {
    MutationSummary::from_results(results)
}

/// A function group: the file, then the first line and span of the function.
/// Code outside each function has no line and no span, so it sorts first.
type FunctionKey = (String, Option<u32>, Option<(u32, u32)>);

/// The results per enclosing function, by file, then line. Code outside each
/// function comes first in its file.
pub fn function_results(
    plan: &MutationPlan,
    results: &[MutantResult],
) -> Vec<MutationFunctionResult> {
    let mut groups: BTreeMap<FunctionKey, MutationFunctionResult> = BTreeMap::new();
    for (mutant, result) in plan.mutants.iter().zip(results) {
        let function = mutant.function.as_ref();
        let key = (
            mutant.file.clone(),
            function.map(|function| function.line),
            function.map(|function| function.span),
        );
        let group = groups.entry(key).or_insert_with(|| MutationFunctionResult {
            file: mutant.file.clone(),
            name: function.and_then(|function| function.name.clone()),
            line: function.map(|function| function.line),
            end_line: function.map(|function| function.end_line),
            in_diff: function.is_some_and(|function| function.in_diff),
            cyclomatic: function.and_then(|function| function.cyclomatic),
            counts: MutationStatusCounts::default(),
            score: None,
            survived: Vec::new(),
        });
        group.counts.add(result.status);
        if result.status == MutantStatus::Survived {
            group.survived.push(result.location.mutant_id.clone());
        }
    }
    groups
        .into_values()
        .map(|mut group| {
            group.score = group.counts.score();
            group
        })
        .collect()
}

/// The facts that decide the completion of a run.
pub struct CompletionFacts<'a> {
    pub plan: &'a MutationPlan,
    pub results: &'a [MutantResult],
    pub baseline_failures: &'a [MutationBaselineFailure],
    pub worktree_changed: bool,
    /// The end-of-run worktree check failed.
    pub worktree_unchecked: bool,
    /// The time budget ended before each survivor and timeout got its retry.
    pub unretried: bool,
    /// The `path_map` keys that the runner did not load in the dry run.
    pub unloaded_paths: &'a [String],
}

/// The completion: `partial` when a planned mutant has no final status or a
/// fact makes the survivor list inconclusive.
pub fn completion(facts: &CompletionFacts<'_>) -> MutationCompletion {
    let mut reasons = Vec::new();
    if facts
        .results
        .iter()
        .any(|result| result.status == MutantStatus::RuntimeError)
    {
        reasons.push(MutationIncompleteReason::RuntimeError);
    }
    if facts.plan.counts.pending > 0 {
        reasons.push(MutationIncompleteReason::MutantLimit);
    }
    let budget_pending = facts
        .plan
        .mutants
        .iter()
        .zip(facts.results)
        .any(|(mutant, result)| {
            mutant.disposition == MutantDisposition::Planned
                && result.status == MutantStatus::Pending
        });
    if budget_pending || facts.unretried {
        reasons.push(MutationIncompleteReason::TimeBudget);
    }
    if !facts.plan.skipped_files.is_empty() {
        reasons.push(MutationIncompleteReason::SkippedFiles);
    }
    if has_unloaded_reached_file(facts.plan, facts.unloaded_paths) {
        reasons.push(MutationIncompleteReason::UnloadedFiles);
    }
    if !facts.baseline_failures.is_empty() {
        reasons.push(MutationIncompleteReason::BaselineFailure);
    }
    if facts.worktree_changed {
        reasons.push(MutationIncompleteReason::WorktreeChanged);
    }
    if facts.worktree_unchecked {
        reasons.push(MutationIncompleteReason::WorktreeUnchecked);
    }
    MutationCompletion::from_reasons(reasons)
}

/// True when the runner did not load a file that has a planned mutant and a
/// static test file. The static map says that a dry-run test loads such a
/// file, so its `no_coverage` mutants are a load fault (`protocol.md`
/// section 6), not a fact. A file that no static test reaches stays a fact.
fn has_unloaded_reached_file(plan: &MutationPlan, unloaded_paths: &[String]) -> bool {
    if unloaded_paths.is_empty() {
        return false;
    }
    let unloaded: FxHashSet<&str> = unloaded_paths.iter().map(String::as_str).collect();
    plan.files.iter().any(|file| {
        unloaded.contains(file.path.as_str())
            && !file.static_test_files.is_empty()
            && plan.mutants.iter().any(|mutant| {
                mutant.file == file.path && mutant.disposition == MutantDisposition::Planned
            })
    })
}

/// The public baseline failures of a schedule.
pub fn baseline_failures(outcome: Option<&ScheduleOutcome>) -> Vec<MutationBaselineFailure> {
    outcome
        .map(|outcome| {
            outcome
                .baseline_failures
                .iter()
                .map(|failure| MutationBaselineFailure {
                    file: failure.file.clone(),
                    test_id: failure.test_id.clone(),
                    message: failure.message.clone(),
                    mutant_ids: failure.mutant_ids.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The public issues of a schedule.
pub fn issues(outcome: Option<&ScheduleOutcome>) -> Vec<MutationRunIssue> {
    outcome
        .map(|outcome| {
            outcome
                .issues
                .iter()
                .map(|issue| MutationRunIssue {
                    code: issue.code.to_owned(),
                    message: issue.message.clone(),
                    mutant_id: issue.mutant_id.clone(),
                    stderr_tail: issue.stderr_tail.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::scheduler::{MutantOutcome, ScheduleTimings};
    use super::*;
    use fallow_engine::mutation::{
        FunctionFacts, Mutant, MutantId, MutationPlanCounts, Operator, PlannedFile, PruneCounts,
        SkipReason, SkippedFile,
    };
    use rustc_hash::FxHashMap;

    fn function(name: &str, line: u32) -> FunctionFacts {
        FunctionFacts {
            name: Some(name.to_owned()),
            span: (line * 100, line * 100 + 50),
            line,
            end_line: line + 3,
            cyclomatic: Some(2),
            in_diff: true,
        }
    }

    fn planned(
        id: &str,
        rank: Option<u32>,
        disposition: MutantDisposition,
        function: Option<FunctionFacts>,
    ) -> PlannedMutant {
        PlannedMutant {
            mutant: Mutant {
                id: MutantId(id.to_owned()),
                operator: Operator::ArithmeticOperator,
                span: (10, 15),
                line: 2,
                column: 9,
                original: "a + b".to_owned(),
                replacement: "a - b".to_owned(),
                enclosing_function: None,
            },
            file: "src/a.ts".to_owned(),
            rank,
            disposition,
            function,
        }
    }

    fn plan(mutants: Vec<PlannedMutant>, pending: usize) -> MutationPlan {
        MutationPlan {
            files: vec![PlannedFile {
                path: "src/a.ts".to_owned(),
                source_sha256: "ab".repeat(32),
                instrumented_code: "SECRET SOURCE".to_owned(),
                static_test_files: vec!["test/a.test.ts".to_owned()],
            }],
            counts: MutationPlanCounts {
                total: mutants.len(),
                pending,
                pruned: PruneCounts::default(),
                ..MutationPlanCounts::default()
            },
            mutants,
            test_files: vec!["test/a.test.ts".to_owned()],
            no_test_files: false,
            framework_config_checked: false,
            skipped_files: Vec::new(),
            unmatched_paths: Vec::new(),
        }
    }

    fn outcome(results: &[(&str, MutantStatus)]) -> ScheduleOutcome {
        ScheduleOutcome {
            ready: None,
            results: results
                .iter()
                .map(|(id, status)| {
                    (
                        (*id).to_owned(),
                        MutantOutcome {
                            status: *status,
                            killed_by: (*status == MutantStatus::Killed)
                                .then(|| "test/a.test.ts::t1".to_owned()),
                            covering_tests: vec!["test/a.test.ts::t1".to_owned()],
                            static_mutant: false,
                            duration_ms: Some(4),
                        },
                    )
                })
                .collect::<FxHashMap<_, _>>(),
            baseline_failures: Vec::new(),
            unloaded_paths: Vec::new(),
            issues: Vec::new(),
            unretried: Vec::new(),
            timings: ScheduleTimings::default(),
            workers: 1,
        }
    }

    #[test]
    fn a_pending_mutant_that_the_dry_run_never_hit_is_no_coverage() {
        let plan = plan(
            vec![
                planned("m1", Some(1), MutantDisposition::Planned, None),
                planned("m2", Some(2), MutantDisposition::Pending, None),
                planned("m3", Some(3), MutantDisposition::Pending, None),
            ],
            2,
        );
        let mut outcome = outcome(&[
            ("m1", MutantStatus::Killed),
            ("m3", MutantStatus::NoCoverage),
        ]);
        if let Some(result) = outcome.results.get_mut("m3") {
            result.covering_tests.clear();
        }
        let results = mutant_results(&plan, Some(&outcome));
        let statuses: Vec<MutantStatus> = results.iter().map(|result| result.status).collect();
        assert_eq!(
            statuses,
            vec![
                MutantStatus::Killed,
                MutantStatus::Pending,
                MutantStatus::NoCoverage,
            ]
        );
        let summary = summary(&results);
        assert_eq!(summary.coverage, Some(2.0 / 3.0));
    }

    #[test]
    fn plan_dispositions_and_schedule_outcomes_merge_into_one_status_each() {
        let plan = plan(
            vec![
                planned(
                    "m1",
                    Some(1),
                    MutantDisposition::Planned,
                    Some(function("add", 1)),
                ),
                planned(
                    "m2",
                    Some(2),
                    MutantDisposition::Planned,
                    Some(function("add", 1)),
                ),
                planned("m3", Some(3), MutantDisposition::Planned, None),
                planned("m4", Some(4), MutantDisposition::Pending, None),
                planned(
                    "m5",
                    None,
                    MutantDisposition::NoCoverage,
                    Some(function("sub", 9)),
                ),
                planned(
                    "m6",
                    None,
                    MutantDisposition::Pruned(PruneReason::SideEffectSink),
                    Some(function("sub", 9)),
                ),
            ],
            1,
        );
        let outcome = outcome(&[("m1", MutantStatus::Killed), ("m2", MutantStatus::Survived)]);
        let results = mutant_results(&plan, Some(&outcome));
        let statuses: Vec<MutantStatus> = results.iter().map(|result| result.status).collect();
        assert_eq!(
            statuses,
            vec![
                MutantStatus::Killed,
                MutantStatus::Survived,
                MutantStatus::Pending,
                MutantStatus::Pending,
                MutantStatus::NoCoverage,
                MutantStatus::Pruned,
            ]
        );
        assert_eq!(results[0].killed_by.as_deref(), Some("test/a.test.ts::t1"));
        assert_eq!(results[0].location.column, 10, "the column is 1-based");
        assert_eq!(results[5].prune_reason.as_deref(), Some("side-effect-sink"));

        let summary = summary(&results);
        assert_eq!(summary.counts.total, 6);
        assert_eq!(
            summary.score,
            Some(0.5),
            "killed / (killed + survived); no_coverage stays out"
        );
        assert_eq!(
            summary.coverage,
            Some(0.8),
            "covered / (covered + no_coverage); the pending mutants count as covered"
        );

        let completion = completion(&CompletionFacts {
            plan: &plan,
            results: &results,
            baseline_failures: &[],
            worktree_changed: false,
            worktree_unchecked: false,
            unretried: false,
            unloaded_paths: &[],
        });
        assert_eq!(
            completion.reasons,
            vec![
                MutationIncompleteReason::MutantLimit,
                MutationIncompleteReason::TimeBudget
            ],
            "m3 is planned with no outcome, m4 is after the limit"
        );
    }

    #[test]
    fn functions_roll_up_by_file_and_span_with_their_survivors() {
        let plan = plan(
            vec![
                planned(
                    "m1",
                    Some(1),
                    MutantDisposition::Planned,
                    Some(function("add", 1)),
                ),
                planned(
                    "m2",
                    Some(2),
                    MutantDisposition::Planned,
                    Some(function("add", 1)),
                ),
                planned("m3", Some(3), MutantDisposition::Planned, None),
                planned(
                    "m4",
                    Some(4),
                    MutantDisposition::Planned,
                    Some(function("sub", 9)),
                ),
            ],
            0,
        );
        let outcome = outcome(&[
            ("m1", MutantStatus::Killed),
            ("m2", MutantStatus::Survived),
            ("m3", MutantStatus::Timeout),
            ("m4", MutantStatus::Survived),
        ]);
        let results = mutant_results(&plan, Some(&outcome));
        let functions = function_results(&plan, &results);
        type Shape<'a> = (
            Option<&'a str>,
            Option<u32>,
            usize,
            Option<f64>,
            Vec<&'a str>,
        );
        let shape: Vec<Shape<'_>> = functions
            .iter()
            .map(|function| {
                (
                    function.name.as_deref(),
                    function.line,
                    function.counts.total,
                    function.score,
                    function.survived.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                (None, None, 1, Some(1.0), vec![]),
                (Some("add"), Some(1), 2, Some(0.5), vec!["m2"]),
                (Some("sub"), Some(9), 1, Some(0.0), vec!["m4"]),
            ]
        );
        assert!(functions[1].in_diff);
        assert_eq!(functions[1].end_line, Some(4));
        assert_eq!(
            completion(&CompletionFacts {
                plan: &plan,
                results: &results,
                baseline_failures: &[],
                worktree_changed: false,
                worktree_unchecked: false,
                unretried: false,
                unloaded_paths: &[],
            })
            .reasons,
            Vec::<MutationIncompleteReason>::new()
        );
    }

    #[test]
    fn runtime_errors_skipped_files_baseline_failures_and_worktree_changes_make_a_run_partial() {
        let mut plan = plan(
            vec![planned("m1", Some(1), MutantDisposition::Planned, None)],
            0,
        );
        plan.skipped_files.push(SkippedFile {
            path: "src/big.ts".to_owned(),
            reason: SkipReason::NotAnalyzed,
        });
        let outcome = outcome(&[("m1", MutantStatus::RuntimeError)]);
        let results = mutant_results(&plan, Some(&outcome));
        let failure = MutationBaselineFailure {
            file: Some("test/a.test.ts".to_owned()),
            test_id: None,
            message: Some("boom".to_owned()),
            mutant_ids: Vec::new(),
        };
        let completion = completion(&CompletionFacts {
            plan: &plan,
            results: &results,
            baseline_failures: &[failure],
            worktree_changed: true,
            worktree_unchecked: false,
            unretried: false,
            unloaded_paths: &[],
        });
        assert_eq!(
            completion.reasons,
            vec![
                MutationIncompleteReason::RuntimeError,
                MutationIncompleteReason::SkippedFiles,
                MutationIncompleteReason::BaselineFailure,
                MutationIncompleteReason::WorktreeChanged,
            ]
        );
        assert_eq!(skipped_files(&plan)[0].reason, "not-analyzed");
    }

    #[test]
    fn an_unretried_mutant_and_an_unchecked_worktree_make_a_run_partial() {
        let plan = plan(
            vec![planned("m1", Some(1), MutantDisposition::Planned, None)],
            0,
        );
        let outcome = outcome(&[("m1", MutantStatus::Survived)]);
        let results = mutant_results(&plan, Some(&outcome));
        let completion = completion(&CompletionFacts {
            plan: &plan,
            results: &results,
            baseline_failures: &[],
            worktree_changed: false,
            worktree_unchecked: true,
            unretried: true,
            unloaded_paths: &[],
        });
        assert_eq!(
            completion.reasons,
            vec![
                MutationIncompleteReason::TimeBudget,
                MutationIncompleteReason::WorktreeUnchecked,
            ]
        );
    }

    #[test]
    fn an_unloaded_file_that_a_dry_run_test_reaches_makes_a_run_partial() {
        let mut plan = plan(
            vec![planned("m1", Some(1), MutantDisposition::Planned, None)],
            0,
        );
        plan.files.push(PlannedFile {
            path: "src/b.ts".to_owned(),
            source_sha256: "cd".repeat(32),
            instrumented_code: String::new(),
            static_test_files: Vec::new(),
        });
        let results = mutant_results(&plan, Some(&outcome(&[("m1", MutantStatus::NoCoverage)])));
        let reasons = |unloaded: &[String]| {
            completion(&CompletionFacts {
                plan: &plan,
                results: &results,
                baseline_failures: &[],
                worktree_changed: false,
                worktree_unchecked: false,
                unretried: false,
                unloaded_paths: unloaded,
            })
            .reasons
        };
        assert_eq!(
            reasons(&["src/a.ts".to_owned()]),
            vec![MutationIncompleteReason::UnloadedFiles],
            "a static test reaches src/a.ts, so the runner had to load it"
        );
        assert_eq!(
            reasons(&["src/b.ts".to_owned()]),
            Vec::new(),
            "no static test reaches src/b.ts, so it is a fact, not a fault"
        );
    }

    #[test]
    fn a_static_map_miss_takes_the_schedule_outcome() {
        let plan = plan(
            vec![
                planned("m1", None, MutantDisposition::NoCoverage, None),
                planned("m2", None, MutantDisposition::NoCoverage, None),
            ],
            0,
        );
        let results = mutant_results(&plan, Some(&outcome(&[("m1", MutantStatus::Killed)])));
        assert_eq!(results[0].status, MutantStatus::Killed);
        assert_eq!(results[0].killed_by.as_deref(), Some("test/a.test.ts::t1"));
        assert_eq!(results[1].status, MutantStatus::NoCoverage);
    }

    #[test]
    fn plan_output_parts_carry_no_source_text() {
        let plan = plan(
            vec![planned(
                "m1",
                Some(1),
                MutantDisposition::Pruned(PruneReason::FrameworkConfig),
                None,
            )],
            0,
        );
        let files = serde_json::to_string(&plan_files(&plan)).expect("json");
        let mutants = serde_json::to_string(&plan_mutants(&plan)).expect("json");
        for text in [&files, &mutants] {
            assert!(!text.contains("SECRET SOURCE"));
            assert!(!text.contains("a + b"));
            assert!(!text.contains("a - b"));
        }
        assert!(mutants.contains(r#""disposition":"pruned""#), "{mutants}");
        assert!(
            mutants.contains(r#""prune_reason":"framework-config""#),
            "{mutants}"
        );
        assert!(files.contains("test/a.test.ts"), "{files}");
    }
}
