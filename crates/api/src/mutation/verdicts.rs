//! Joins a mutation report with a separate verdict document.
//!
//! A verdict marks one mutant as equivalent to the original code. This
//! replaces an inline suppression comment: the report stays the evidence, and the verdict is a separate,
//! reviewable judgment. The join key is the `review_key` of the verdict, which
//! is the `mutant_id` of the report. The id hashes the enclosing function
//! text, so a verdict stays valid until that function changes.

use std::collections::BTreeSet;

use fallow_output::{
    MutantStatus, MutationReport, MutationStatusCounts, MutationSummary, MutationVerdictInput,
    MutationVerdictJoin,
};
use rustc_hash::FxHashSet;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::ProgrammaticError;

/// The largest verdict document that the join reads.
pub const MAX_VERDICT_INPUT_BYTES: usize = 16 * 1024 * 1024;

fn verdict_error(message: impl Into<String>) -> ProgrammaticError {
    ProgrammaticError::new(message, 2)
        .with_code("FALLOW_MUTATION_VERDICTS_INVALID")
        .with_context("mutation.verdicts")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// The effect of one verdict on a mutant with `status`.
enum Effect {
    Applied,
    Contradicted,
    Deferred,
}

const fn effect(status: MutantStatus) -> Effect {
    match status {
        MutantStatus::Survived
        | MutantStatus::NoCoverage
        | MutantStatus::Flaky
        | MutantStatus::Equivalent => Effect::Applied,
        MutantStatus::Killed | MutantStatus::Timeout | MutantStatus::CompileError => {
            Effect::Contradicted
        }
        MutantStatus::Pending | MutantStatus::RuntimeError | MutantStatus::Pruned => {
            Effect::Deferred
        }
    }
}

/// Moves one mutant from `from` to `equivalent` in the function group that
/// holds it.
fn move_in_function(report: &mut MutationReport, index: usize, from: MutantStatus) {
    let location = &report.mutants[index].location;
    let key = (
        location.file.as_str(),
        location.function.as_deref(),
        location.function_line,
    );
    let group = report.functions.iter_mut().find(|group| {
        (group.file.as_str(), group.name.as_deref(), group.line) == key
            && count_of(&group.counts, from) > 0
    });
    let Some(group) = group else {
        return;
    };
    group.counts.remove(from);
    group.counts.add(MutantStatus::Equivalent);
    group.survived.retain(|id| *id != location.mutant_id);
    group.score = group.counts.score();
}

const fn count_of(counts: &MutationStatusCounts, status: MutantStatus) -> usize {
    match status {
        MutantStatus::Killed => counts.killed,
        MutantStatus::Timeout => counts.timeout,
        MutantStatus::Survived => counts.survived,
        MutantStatus::NoCoverage => counts.no_coverage,
        MutantStatus::RuntimeError => counts.runtime_error,
        MutantStatus::Flaky => counts.flaky,
        MutantStatus::Pruned => counts.pruned,
        MutantStatus::Pending => counts.pending,
        MutantStatus::Equivalent => counts.equivalent,
        MutantStatus::CompileError => counts.compile_error,
    }
}

/// Parses a verdict document and checks its keys.
fn parse_verdicts(verdict_json: &[u8]) -> Result<MutationVerdictInput, ProgrammaticError> {
    if verdict_json.len() > MAX_VERDICT_INPUT_BYTES {
        return Err(verdict_error(
            "the mutation verdict document is larger than 16 MiB",
        ));
    }
    let input: MutationVerdictInput = serde_json::from_slice(verdict_json)
        .map_err(|error| verdict_error(format!("invalid mutation verdict document: {error}")))?;
    let mut keys = FxHashSet::default();
    for verdict in &input.verdicts {
        if verdict.review_key.trim().is_empty() {
            return Err(verdict_error("a mutation verdict has an empty review_key"));
        }
        if !keys.insert(verdict.review_key.as_str()) {
            return Err(verdict_error(format!(
                "the review_key `{}` occurs more than one time",
                verdict.review_key
            )));
        }
    }
    Ok(input)
}

/// Checks a verdict document without a report.
///
/// A caller runs this check before a mutation run, so an invalid document
/// stops the command before any test runs.
///
/// # Errors
///
/// Returns the same errors as [`apply_mutation_verdicts`].
pub fn validate_mutation_verdicts(verdict_json: &[u8]) -> Result<(), ProgrammaticError> {
    parse_verdicts(verdict_json).map(|_| ())
}

/// Applies a verdict document to `report`.
///
/// Each verdict for a `survived`, `no_coverage` or `flaky` mutant sets the
/// status `equivalent`, which does not count in the score. A verdict for a
/// killed or timed-out mutant is contradicted by a test, so the status stays.
/// A verdict for a mutant with no tested result (`pending`, `runtime_error`,
/// `pruned`) is deferred. A verdict that matches no mutant is unmatched; it
/// is not an error, because one verdict document serves many scoped runs.
/// The summary and the function results are recomputed.
///
/// # Errors
///
/// Returns a fail-closed error for a document over 16 MiB, a malformed
/// document, an empty `review_key`, or a `review_key` that occurs twice.
pub fn apply_mutation_verdicts(
    report: &mut MutationReport,
    verdict_json: &[u8],
) -> Result<(), ProgrammaticError> {
    let input = parse_verdicts(verdict_json)?;
    let keys: FxHashSet<&str> = input
        .verdicts
        .iter()
        .map(|verdict| verdict.review_key.as_str())
        .collect();

    let mut applied = BTreeSet::new();
    let mut contradicted = BTreeSet::new();
    let mut deferred = BTreeSet::new();
    let mut matched = FxHashSet::default();
    for index in 0..report.mutants.len() {
        let id = report.mutants[index].location.mutant_id.clone();
        if !keys.contains(id.as_str()) {
            continue;
        }
        matched.insert(id.clone());
        let status = report.mutants[index].status;
        match effect(status) {
            Effect::Applied => {
                if status != MutantStatus::Equivalent {
                    report.mutants[index].status = MutantStatus::Equivalent;
                    move_in_function(report, index, status);
                }
                applied.insert(id);
            }
            Effect::Contradicted => {
                contradicted.insert(id);
            }
            Effect::Deferred => {
                deferred.insert(id);
            }
        }
    }
    let unmatched: BTreeSet<String> = input
        .verdicts
        .iter()
        .filter(|verdict| !matched.contains(&verdict.review_key))
        .map(|verdict| verdict.review_key.clone())
        .collect();

    report.summary = MutationSummary::from_results(&report.mutants);
    report.verdicts = Some(MutationVerdictJoin {
        verdicts_sha256: sha256_hex(verdict_json),
        applied: applied.into_iter().collect(),
        contradicted: contradicted.into_iter().collect(),
        deferred: deferred.into_iter().collect(),
        unmatched: unmatched.into_iter().collect(),
    });
    Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests {
    use fallow_output::{
        MutantLocation, MutantResult, MutationCompletion, MutationFunctionResult,
        MutationSchemaVersion, MutationScopeKind, MutationScopeOutput, MutationSummary,
        MutationTimings, MutationWorktreeCheck,
    };
    use fallow_types::envelope::{ElapsedMs, ToolVersion};

    use super::*;

    fn mutant(id: &str, status: MutantStatus) -> MutantResult {
        MutantResult {
            location: MutantLocation {
                mutant_id: id.to_owned(),
                file: "src/a.ts".to_owned(),
                line: 2,
                column: 3,
                span: (10, 15),
                operator: "arithmetic-operator".to_owned(),
                function: Some("add".to_owned()),
                function_line: Some(1),
                rank: Some(1),
            },
            status,
            prune_reason: None,
            killed_by: None,
            covering_tests: match status {
                MutantStatus::NoCoverage | MutantStatus::Pending | MutantStatus::Pruned => {
                    Vec::new()
                }
                _ => vec!["test/a.test.ts::t1".to_owned()],
            },
            static_mutant: false,
            duration_ms: None,
            test_reach: None,
            retried: false,
            attempts: Vec::new(),
            cached: false,
            type_error_codes: Vec::new(),
        }
    }

    fn report(mutants: Vec<MutantResult>) -> MutationReport {
        let mut counts = MutationStatusCounts::default();
        let mut survived = Vec::new();
        for result in &mutants {
            counts.add(result.status);
            if result.status == MutantStatus::Survived {
                survived.push(result.location.mutant_id.clone());
            }
        }
        MutationReport {
            schema_version: MutationSchemaVersion::V1,
            version: ToolVersion("0.0.0".to_owned()),
            elapsed_ms: ElapsedMs(0),
            completion: MutationCompletion::from_reasons(Vec::new()),
            scope: MutationScopeOutput {
                kind: MutationScopeKind::All,
                base_ref: None,
            },
            runner: None,
            cache: None,
            summary: MutationSummary::from_results(&mutants),
            functions: vec![MutationFunctionResult {
                file: "src/a.ts".to_owned(),
                name: Some("add".to_owned()),
                line: Some(1),
                end_line: Some(4),
                in_diff: true,
                cyclomatic: Some(1),
                counts,
                score: counts.score(),
                survived,
            }],
            mutants,
            no_test_files: false,
            baseline_failures: Vec::new(),
            skipped_files: Vec::new(),
            unmatched_paths: Vec::new(),
            unloaded_paths: Vec::new(),
            worktree: MutationWorktreeCheck {
                checked: true,
                allow_dirty: false,
                dirty_at_start: Vec::new(),
                changed_paths: Vec::new(),
            },
            errors: Vec::new(),
            timings: MutationTimings::default(),
            files: Vec::new(),
            verdicts: None,
            typecheck: None,
        }
    }

    fn document(keys: &[&str]) -> Vec<u8> {
        let verdicts: Vec<serde_json::Value> = keys
            .iter()
            .map(|key| serde_json::json!({ "review_key": key, "verdict": "equivalent" }))
            .collect();
        serde_json::to_vec(&serde_json::json!({ "schema_version": "1", "verdicts": verdicts }))
            .unwrap()
    }

    #[test]
    fn equivalent_verdicts_leave_the_score_and_the_survivor_list() {
        let mut report = report(vec![
            mutant("m1", MutantStatus::Killed),
            mutant("m2", MutantStatus::Survived),
            mutant("m3", MutantStatus::Survived),
            mutant("m4", MutantStatus::NoCoverage),
        ]);
        assert_eq!(report.summary.score, Some(1.0 / 3.0));
        assert_eq!(report.summary.coverage, Some(0.75));

        apply_mutation_verdicts(&mut report, &document(&["m2", "m4"])).unwrap();

        let statuses: Vec<MutantStatus> = report.mutants.iter().map(|m| m.status).collect();
        assert_eq!(
            statuses,
            vec![
                MutantStatus::Killed,
                MutantStatus::Equivalent,
                MutantStatus::Survived,
                MutantStatus::Equivalent
            ]
        );
        assert_eq!(report.summary.counts.equivalent, 2);
        assert_eq!(report.summary.counts.survived, 1);
        assert_eq!(report.summary.counts.no_coverage, 0);
        assert_eq!(report.summary.counts.total, 4);
        assert_eq!(report.summary.score, Some(0.5));
        assert_eq!(report.summary.coverage, Some(1.0));
        let function = &report.functions[0];
        assert_eq!(function.counts.equivalent, 2);
        assert_eq!(function.counts.no_coverage, 0);
        assert_eq!(function.survived, vec!["m3".to_owned()]);
        assert_eq!(function.score, Some(0.5));
        let join = report.verdicts.unwrap();
        assert_eq!(join.applied, vec!["m2".to_owned(), "m4".to_owned()]);
        assert!(join.unmatched.is_empty());
        assert_eq!(join.verdicts_sha256.len(), 64);
    }

    #[test]
    fn a_kill_contradicts_a_verdict_and_unknown_keys_stay_unmatched() {
        let mut report = report(vec![
            mutant("m1", MutantStatus::Killed),
            mutant("m2", MutantStatus::Pending),
            mutant("m3", MutantStatus::Survived),
        ]);

        apply_mutation_verdicts(&mut report, &document(&["gone", "m1", "m2"])).unwrap();

        assert_eq!(report.mutants[0].status, MutantStatus::Killed);
        assert_eq!(report.mutants[1].status, MutantStatus::Pending);
        assert_eq!(report.mutants[2].status, MutantStatus::Survived);
        assert_eq!(report.summary.score, Some(0.5));
        let join = report.verdicts.unwrap();
        assert!(join.applied.is_empty());
        assert_eq!(join.contradicted, vec!["m1".to_owned()]);
        assert_eq!(join.deferred, vec!["m2".to_owned()]);
        assert_eq!(join.unmatched, vec!["gone".to_owned()]);
    }

    #[test]
    fn malformed_and_duplicate_verdicts_fail_closed() {
        let cases: [(&[u8], &str); 4] = [
            (b"{", "invalid mutation verdict document"),
            (
                br#"{"schema_version":"1","verdicts":[{"review_key":"m1","verdict":"survived"}]}"#,
                "invalid mutation verdict document",
            ),
            (
                br#"{"schema_version":"1","verdicts":[{"review_key":" ","verdict":"equivalent"}]}"#,
                "empty review_key",
            ),
            (&document(&["m1", "m1"]), "more than one time"),
        ];
        for (json, needle) in cases {
            let checked = validate_mutation_verdicts(json).unwrap_err();
            assert_eq!(checked.exit_code, 2);
            assert!(checked.message.contains(needle), "{}", checked.message);
            let mut report = report(vec![mutant("m1", MutantStatus::Survived)]);
            let error = apply_mutation_verdicts(&mut report, json).unwrap_err();
            assert_eq!(
                error.code.as_deref(),
                Some("FALLOW_MUTATION_VERDICTS_INVALID")
            );
            assert_eq!(error.exit_code, 2);
            assert!(error.message.contains(needle), "{}", error.message);
            assert_eq!(report.mutants[0].status, MutantStatus::Survived);
            assert!(report.verdicts.is_none());
        }
        assert!(validate_mutation_verdicts(&document(&["m1", "m2"])).is_ok());
    }
}
