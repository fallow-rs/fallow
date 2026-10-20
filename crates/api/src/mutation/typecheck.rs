//! The type check of survived mutants (`fallow mutation --typecheck`).
//!
//! The check runs after the tests and reads only the survivors. A survivor
//! whose mutated file does not type-check becomes `compile_error`, which the
//! score does not count. A survivor that the checker cannot check keeps the
//! status `survived`.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use fallow_engine::mutation::{MutationPlan, is_component_file};
use fallow_output::{
    MutantResult, MutantStatus, MutationRunIssue, MutationTypecheckOutput, MutationTypecheckSkip,
    MutationTypecheckSkipReason,
};
use rustc_hash::FxHashMap;

use crate::type_aware::{TypecheckFile, TypecheckMutant, TypecheckOutcome};

/// The survivors of one run, split by what the checker can read.
pub struct SurvivorRequest {
    /// The request files: the survivors in files that the checker can read,
    /// grouped by file in plan order.
    pub files: Vec<TypecheckFile>,
    /// The outcomes of the survivors in component files. The checker cannot
    /// read a Vue, Svelte or Astro file, so these survivors are not sent.
    pub unsupported: Vec<(String, TypecheckOutcome)>,
}

impl SurvivorRequest {
    /// The number of survivors, sent or not.
    pub fn survivors(&self) -> usize {
        self.files
            .iter()
            .map(|file| file.mutants.len())
            .sum::<usize>()
            + self.unsupported.len()
    }
}

/// Splits the survivors of `results` into the request files and the
/// component survivors. `results` is in plan order.
pub fn survivor_request(plan: &MutationPlan, results: &[MutantResult]) -> SurvivorRequest {
    let digests: FxHashMap<&str, &str> = plan
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.source_sha256.as_str()))
        .collect();
    let mut request = SurvivorRequest {
        files: Vec::new(),
        unsupported: Vec::new(),
    };
    let mut index_of: FxHashMap<&str, usize> = FxHashMap::default();
    for (planned, result) in plan.mutants.iter().zip(results) {
        if result.status != MutantStatus::Survived {
            continue;
        }
        let Some(digest) = digests.get(planned.file.as_str()) else {
            continue;
        };
        if is_component_file(&planned.file) {
            request.unsupported.push((
                planned.mutant.id.as_str().to_owned(),
                TypecheckOutcome::Unchecked(MutationTypecheckSkipReason::UnsupportedFile),
            ));
            continue;
        }
        let mutant = TypecheckMutant {
            id: planned.mutant.id.as_str().to_owned(),
            start: planned.mutant.span.0,
            end: planned.mutant.span.1,
            replacement: planned.mutant.replacement.clone(),
        };
        if let Some(&index) = index_of.get(planned.file.as_str()) {
            request.files[index].mutants.push(mutant);
            continue;
        }
        index_of.insert(planned.file.as_str(), request.files.len());
        request.files.push(TypecheckFile {
            path: planned.file.clone(),
            source_sha256: (*digest).to_owned(),
            mutants: vec![mutant],
        });
    }
    request
}

/// Applies the checker outcomes to `results` and returns the summary. Only a
/// `survived` mutant can change: a compile error sets `compile_error` and
/// the diagnostic codes.
pub fn apply_outcomes(
    results: &mut [MutantResult],
    outcomes: Vec<(String, TypecheckOutcome)>,
) -> MutationTypecheckOutput {
    let mut by_id: FxHashMap<String, TypecheckOutcome> = outcomes.into_iter().collect();
    let mut output = MutationTypecheckOutput {
        completed: true,
        survivors: by_id.len(),
        checked: 0,
        compile_errors: 0,
        skipped: Vec::new(),
    };
    let mut skipped = BTreeMap::new();
    for result in results.iter_mut() {
        if result.status != MutantStatus::Survived {
            continue;
        }
        let Some(outcome) = by_id.remove(&result.location.mutant_id) else {
            continue;
        };
        match outcome {
            TypecheckOutcome::Compiles => output.checked += 1,
            TypecheckOutcome::CompileError(codes) => {
                output.checked += 1;
                output.compile_errors += 1;
                result.status = MutantStatus::CompileError;
                result.type_error_codes = codes;
            }
            TypecheckOutcome::Unchecked(reason) => *skipped.entry(reason).or_insert(0) += 1,
        }
    }
    output.skipped = skipped
        .into_iter()
        .map(|(reason, count)| MutationTypecheckSkip { reason, count })
        .collect();
    output
}

/// The summary and the issue of a type check that failed. The survivors keep
/// the status `survived`. The `unsupported` component survivors did not
/// depend on the companion, so the summary still counts them.
pub fn failed_output(
    survivors: usize,
    unsupported: usize,
    message: &str,
) -> (MutationTypecheckOutput, MutationRunIssue) {
    let skipped = if unsupported == 0 {
        Vec::new()
    } else {
        vec![MutationTypecheckSkip {
            reason: MutationTypecheckSkipReason::UnsupportedFile,
            count: unsupported,
        }]
    };
    (
        MutationTypecheckOutput {
            completed: false,
            survivors,
            checked: 0,
            compile_errors: 0,
            skipped,
        },
        MutationRunIssue {
            code: "typecheck-failed".to_owned(),
            message: format!(
                "the type check of the survivors failed, so the survivors keep the status survived: {message}"
            ),
            mutant_id: None,
            stderr_tail: None,
        },
    )
}

/// The result of the survivor type check of one run.
pub struct TypecheckStep {
    /// The summary, `None` when the run did not check the survivors.
    pub output: Option<MutationTypecheckOutput>,
    /// The issue of a failed check.
    pub issue: Option<MutationRunIssue>,
    /// The wall time of the check.
    pub elapsed: Duration,
}

/// Type-checks the survivors of `results` when `enabled`, and applies the
/// outcomes. After a failed check, `results` is unchanged and the step has
/// an issue.
pub fn typecheck_survivors(
    enabled: bool,
    root: &Path,
    plan: &MutationPlan,
    results: &mut [MutantResult],
) -> TypecheckStep {
    let started = Instant::now();
    if !enabled {
        return TypecheckStep {
            output: None,
            issue: None,
            elapsed: Duration::ZERO,
        };
    }
    let request = survivor_request(plan, results);
    let survivors = request.survivors();
    let (output, issue) = match crate::type_aware::typecheck_mutants(root, &request.files) {
        Ok(mut outcomes) => {
            outcomes.extend(request.unsupported);
            (apply_outcomes(results, outcomes), None)
        }
        Err(error) => {
            let (output, issue) =
                failed_output(survivors, request.unsupported.len(), &error.to_string());
            (output, Some(issue))
        }
    };
    TypecheckStep {
        output: Some(output),
        issue,
        elapsed: started.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use fallow_engine::mutation::{
        Mutant, MutantDisposition, MutantId, MutationPlanCounts, Operator, PlannedFile,
        PlannedMutant,
    };
    use fallow_output::{MutantLocation, MutationSummary};

    use super::*;

    fn plan_with(paths: &[&str]) -> MutationPlan {
        MutationPlan {
            files: paths
                .iter()
                .map(|path| PlannedFile {
                    path: (*path).to_owned(),
                    source_sha256: "ab".repeat(32),
                    instrumented_code: String::new(),
                    static_test_files: Vec::new(),
                })
                .collect(),
            mutants: paths
                .iter()
                .enumerate()
                .map(|(index, path)| PlannedMutant {
                    mutant: Mutant {
                        id: MutantId(format!("m{index}")),
                        operator: Operator::StringLiteral,
                        span: (0, 1),
                        line: 1,
                        column: 0,
                        original: "\"a\"".to_owned(),
                        replacement: "\"\"".to_owned(),
                        enclosing_function: None,
                    },
                    file: (*path).to_owned(),
                    rank: None,
                    disposition: MutantDisposition::Planned,
                    function: None,
                    test_reach: None,
                })
                .collect(),
            test_files: Vec::new(),
            reach_files: Vec::new(),
            test_file_reach: BTreeMap::new(),
            support_files: Vec::new(),
            counts: MutationPlanCounts::default(),
            no_test_files: false,
            framework_config_checked: false,
            skipped_files: Vec::new(),
            unmatched_paths: Vec::new(),
        }
    }

    fn result(id: &str, status: MutantStatus) -> MutantResult {
        MutantResult {
            location: MutantLocation {
                mutant_id: id.to_owned(),
                file: "src/a.ts".to_owned(),
                line: 1,
                column: 1,
                span: (0, 1),
                operator: "string-literal".to_owned(),
                function: None,
                function_line: None,
                rank: None,
            },
            status,
            prune_reason: None,
            killed_by: None,
            covering_tests: vec!["src/a.test.ts > a".to_owned()],
            static_mutant: false,
            duration_ms: None,
            test_reach: None,
            retried: false,
            attempts: Vec::new(),
            cached: false,
            type_error_codes: Vec::new(),
        }
    }

    #[test]
    fn a_survivor_that_does_not_type_check_leaves_the_score() {
        let mut results = vec![
            result("killed", MutantStatus::Killed),
            result("compiles", MutantStatus::Survived),
            result("broken", MutantStatus::Survived),
            result("loose", MutantStatus::Survived),
        ];
        assert_eq!(MutationSummary::from_results(&results).score, Some(0.25));
        let output = apply_outcomes(
            &mut results,
            vec![
                ("compiles".to_owned(), TypecheckOutcome::Compiles),
                (
                    "broken".to_owned(),
                    TypecheckOutcome::CompileError(vec![2322]),
                ),
                (
                    "loose".to_owned(),
                    TypecheckOutcome::Unchecked(MutationTypecheckSkipReason::NoProject),
                ),
            ],
        );
        assert_eq!(
            output,
            MutationTypecheckOutput {
                completed: true,
                survivors: 3,
                checked: 2,
                compile_errors: 1,
                skipped: vec![MutationTypecheckSkip {
                    reason: MutationTypecheckSkipReason::NoProject,
                    count: 1,
                }],
            }
        );
        assert_eq!(results[2].status, MutantStatus::CompileError);
        assert_eq!(results[2].type_error_codes, vec![2322]);
        assert_eq!(results[1].status, MutantStatus::Survived);
        assert_eq!(results[3].status, MutantStatus::Survived);
        let summary = MutationSummary::from_results(&results);
        assert_eq!(summary.counts.compile_error, 1);
        assert_eq!(summary.score, Some(1.0 / 3.0));
        assert_eq!(summary.coverage, Some(1.0));
    }

    #[test]
    fn an_outcome_for_a_mutant_that_did_not_survive_changes_nothing() {
        let mut results = vec![result("killed", MutantStatus::Killed)];
        apply_outcomes(
            &mut results,
            vec![(
                "killed".to_owned(),
                TypecheckOutcome::CompileError(vec![2322]),
            )],
        );
        assert_eq!(results[0].status, MutantStatus::Killed);
        assert!(results[0].type_error_codes.is_empty());
    }

    #[test]
    fn a_component_survivor_is_not_sent_and_gets_its_own_reason() {
        let plan = plan_with(&[
            "src/a.ts",
            "src/store.svelte.ts",
            "src/Button.vue",
            "src/Card.svelte",
            "src/Page.astro",
        ]);
        let results: Vec<MutantResult> = plan
            .mutants
            .iter()
            .map(|planned| {
                let mut survivor = result(planned.mutant.id.as_str(), MutantStatus::Survived);
                survivor.location.file.clone_from(&planned.file);
                survivor
            })
            .collect();
        let request = survivor_request(&plan, &results);
        let sent: Vec<&str> = request
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        assert_eq!(sent, vec!["src/a.ts", "src/store.svelte.ts"]);
        let mut results = results;
        let mut outcomes = request.unsupported;
        for planned in &plan.mutants[..2] {
            outcomes.push((
                planned.mutant.id.as_str().to_owned(),
                TypecheckOutcome::Compiles,
            ));
        }
        let output = apply_outcomes(&mut results, outcomes);
        assert_eq!(output.survivors, 5);
        assert_eq!(output.checked, 2);
        assert_eq!(
            output.skipped,
            vec![MutationTypecheckSkip {
                reason: MutationTypecheckSkipReason::UnsupportedFile,
                count: 3,
            }]
        );
    }

    #[test]
    fn only_component_survivors_complete_without_the_companion() {
        let plan = plan_with(&["src/Button.vue"]);
        let mut results = vec![result("m0", MutantStatus::Survived)];
        results[0].location.file = "src/Button.vue".to_owned();
        let step = typecheck_survivors(true, Path::new("/nonexistent"), &plan, &mut results);
        assert!(step.issue.is_none());
        let output = step.output.expect("a summary");
        assert!(output.completed);
        assert_eq!(output.survivors, 1);
        assert_eq!(output.checked, 0);
        assert_eq!(
            output.skipped,
            vec![MutationTypecheckSkip {
                reason: MutationTypecheckSkipReason::UnsupportedFile,
                count: 1,
            }]
        );
        assert_eq!(results[0].status, MutantStatus::Survived);
    }

    #[test]
    fn a_failed_check_reports_an_issue() {
        let (output, issue) = failed_output(3, 1, "the companion exited with code 1");
        assert!(!output.completed);
        assert_eq!(output.survivors, 3);
        assert_eq!(
            output.skipped,
            vec![MutationTypecheckSkip {
                reason: MutationTypecheckSkipReason::UnsupportedFile,
                count: 1,
            }]
        );
        assert_eq!(issue.code, "typecheck-failed");
        assert!(issue.message.ends_with("the companion exited with code 1"));
    }
}
