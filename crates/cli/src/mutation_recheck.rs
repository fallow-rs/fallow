//! `fallow mutation --recheck`: rerun mutants of a saved report against the
//! current tests. The selection comes from the report: the ids of
//! `--mutant`, else each `survived` and `no_coverage` mutant. Each selected
//! file must still have the digest of the report, and the run allows a
//! dirty worktree only for test files, so the result proves a change of the
//! tests only.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fallow_api::ProgrammaticError;
use fallow_output::{
    MutantStatus, MutationRecheck, MutationRecheckMutant, MutationRecheckOutcome, MutationReport,
};

use crate::mutation_inspect::{
    SavedMutant, SavedReport, parse_saved_report, read_report_bytes, report_input_error,
    report_relative_path, sha256_hex,
};

/// The exit code of invalid input.
const INPUT_EXIT_CODE: u8 = 2;

/// One selected mutant with its status in the earlier report.
#[derive(Debug)]
struct Selected {
    mutant_id: String,
    file: String,
    line: u32,
    previous_status: MutantStatus,
}

/// The mutants of a recheck and the facts of the earlier report.
#[derive(Debug)]
pub struct Selection {
    /// The project root of the report paths.
    root: PathBuf,
    report_sha256: String,
    mutants: Vec<Selected>,
}

impl Selection {
    /// The ids of the selected mutants, for the run options.
    pub fn mutant_ids(&self) -> Vec<String> {
        self.mutants
            .iter()
            .map(|mutant| mutant.mutant_id.clone())
            .collect()
    }

    /// The absolute paths of the selected files, for the run scope.
    pub fn files(&self) -> Vec<PathBuf> {
        let files: BTreeSet<&str> = self
            .mutants
            .iter()
            .map(|mutant| mutant.file.as_str())
            .collect();
        files.into_iter().map(|file| self.root.join(file)).collect()
    }

    /// The recheck facts of `report`, the run of the selected mutants.
    pub fn section(&self, report: &MutationReport) -> MutationRecheck {
        let fails_on_original = !report.baseline_failures.is_empty();
        let mut mutants: Vec<MutationRecheckMutant> = self
            .mutants
            .iter()
            .filter_map(|selected| {
                let result = report
                    .mutants
                    .iter()
                    .find(|result| result.location.mutant_id == selected.mutant_id)?;
                Some(MutationRecheckMutant {
                    mutant_id: selected.mutant_id.clone(),
                    file: selected.file.clone(),
                    line: selected.line,
                    previous_status: selected.previous_status,
                    status: result.status,
                    outcome: outcome(result.status, fails_on_original),
                })
            })
            .collect();
        mutants.sort_by(|left, right| {
            (&left.file, left.line, &left.mutant_id).cmp(&(
                &right.file,
                right.line,
                &right.mutant_id,
            ))
        });
        let count = |outcome: MutationRecheckOutcome| {
            mutants
                .iter()
                .filter(|mutant| mutant.outcome == outcome)
                .count()
        };
        MutationRecheck {
            report_sha256: self.report_sha256.clone(),
            killed: count(MutationRecheckOutcome::Killed),
            survived: count(MutationRecheckOutcome::Survived),
            no_coverage: count(MutationRecheckOutcome::NoCoverage),
            tests_fail_on_original: count(MutationRecheckOutcome::TestsFailOnOriginal),
            unsettled: count(MutationRecheckOutcome::Unsettled),
            mutants,
        }
    }
}

/// The outcome of one mutant. A failing test on the original code proves no
/// survivor, so a mutant without a kill then has that outcome.
const fn outcome(status: MutantStatus, fails_on_original: bool) -> MutationRecheckOutcome {
    match status {
        MutantStatus::Killed | MutantStatus::Timeout => MutationRecheckOutcome::Killed,
        _ if fails_on_original => MutationRecheckOutcome::TestsFailOnOriginal,
        MutantStatus::Survived => MutationRecheckOutcome::Survived,
        MutantStatus::NoCoverage => MutationRecheckOutcome::NoCoverage,
        _ => MutationRecheckOutcome::Unsettled,
    }
}

fn is_open(mutant: &SavedMutant) -> bool {
    matches!(
        mutant.status,
        Some(MutantStatus::Survived | MutantStatus::NoCoverage)
    )
}

fn recheck_error(code: &str, message: impl Into<String>) -> ProgrammaticError {
    ProgrammaticError::new(message, INPUT_EXIT_CODE)
        .with_code(code)
        .with_context("mutation.recheck")
}

/// Selects the mutants of a recheck from the saved report at `path`.
///
/// # Errors
///
/// Returns an exit-2 error for a report that does not parse or is a plan,
/// an id that the report does not have, a selection without a mutant, and a
/// selected file that changed after the report.
pub fn select(path: &Path, root: &Path, ids: &[String]) -> Result<Selection, ProgrammaticError> {
    let bytes = read_report_bytes(path)?;
    let report = parse_saved_report(&bytes)?;
    if report.kind.as_deref() == Some("mutation-plan") {
        return Err(report_input_error(
            "the file is a mutation plan, which has no test results; pass a saved `fallow mutation --format json` report",
        ));
    }
    let chosen = chosen_mutants(&report, ids)?;
    let mut files: Vec<&str> = chosen.iter().map(|mutant| mutant.file.as_str()).collect();
    files.sort_unstable();
    files.dedup();
    for file in files {
        guard_unchanged(&report, root, file)?;
    }
    Ok(Selection {
        root: root.to_path_buf(),
        report_sha256: sha256_hex(&bytes),
        mutants: chosen
            .into_iter()
            .map(|mutant| Selected {
                mutant_id: mutant.mutant_id.clone(),
                file: mutant.file.clone(),
                line: mutant.line,
                previous_status: mutant.status.unwrap_or(MutantStatus::Survived),
            })
            .collect(),
    })
}

fn chosen_mutants<'a>(
    report: &'a SavedReport,
    ids: &[String],
) -> Result<Vec<&'a SavedMutant>, ProgrammaticError> {
    if ids.is_empty() {
        let open: Vec<&SavedMutant> = report
            .mutants
            .iter()
            .filter(|mutant| is_open(mutant))
            .collect();
        if open.is_empty() {
            return Err(recheck_error(
                "FALLOW_MUTATION_NOTHING_TO_RECHECK",
                "the report has no open task: no mutant survived and each mutant has coverage",
            ));
        }
        return Ok(open);
    }
    let wanted: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    let chosen: Vec<&SavedMutant> = report
        .mutants
        .iter()
        .filter(|mutant| wanted.contains(mutant.mutant_id.as_str()))
        .collect();
    let found: BTreeSet<&str> = chosen
        .iter()
        .map(|mutant| mutant.mutant_id.as_str())
        .collect();
    let missing: Vec<&str> = wanted.difference(&found).copied().collect();
    if !missing.is_empty() {
        return Err(recheck_error(
            "FALLOW_MUTATION_UNKNOWN_MUTANT",
            format!("the report has no mutant `{}`", missing.join("`, `")),
        )
        .with_help(
            "pass a mutant_id of the report, or --report <file> with the report of that id",
        ));
    }
    Ok(chosen)
}

/// Refuses a file whose current digest differs from the report. A changed
/// source changes the mutants, so their earlier results do not apply.
fn guard_unchanged(report: &SavedReport, root: &Path, file: &str) -> Result<(), ProgrammaticError> {
    let relative = report_relative_path(file)?;
    let expected = report
        .saved_file(file)
        .map(|saved| saved.source_sha256.as_str());
    let current = std::fs::read(root.join(relative))
        .ok()
        .map(|bytes| sha256_hex(&bytes));
    if expected.is_some() && current.as_deref() == expected {
        return Ok(());
    }
    Err(recheck_error(
        "FALLOW_MUTATION_SOURCE_CHANGED",
        format!("`{file}` changed after the report, so its mutants changed too"),
    )
    .with_help(
        "a recheck proves a change of the tests only; run `fallow mutation` again for new results",
    ))
}

/// The human summary of a recheck, after the run report.
pub fn render(recheck: &MutationRecheck) -> String {
    let mut text = String::from("\nRecheck of the earlier report\n");
    let rows = [
        ("killed", recheck.killed),
        ("still survived", recheck.survived),
        ("no coverage", recheck.no_coverage),
        (
            "tests fail on the original code",
            recheck.tests_fail_on_original,
        ),
        ("no final result", recheck.unsettled),
    ];
    for (label, count) in rows.into_iter().filter(|(_, count)| *count > 0) {
        let _ = writeln!(text, "  {count:>4}  {label}");
    }
    if recheck.tests_fail_on_original > 0 {
        text.push_str(
            "A test of this run fails on the original code, so the run proves no survivor. Fix the failing test, then recheck again.\n",
        );
    } else if recheck.survived + recheck.no_coverage > 0 {
        text.push_str("Run `fallow mutation tasks` for the mutants that are still open.\n");
    } else if recheck.unsettled == 0 {
        text.push_str("Each rechecked mutant is killed.\n");
    }
    text
}

#[cfg(test)]
#[path = "mutation_recheck_tests.rs"]
mod tests;
