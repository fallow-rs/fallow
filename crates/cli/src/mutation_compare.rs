//! `fallow mutation --compare <report>`: the mutants whose result differs
//! from a saved report. A mutant id stays the same until its enclosing
//! function changes, so two runs over the same files compare by id. The
//! main use is a test edit: a mutant that a test killed before and that
//! survives now shows that the edit weakened the tests.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use fallow_api::ProgrammaticError;
use fallow_output::{
    MutantResult, MutantStatus, MutationCompare, MutationCompareChange, MutationCompareMutant,
    MutationCompletionStatus, MutationReport,
};

use crate::mutation_inspect::{
    SavedMutant, SavedReport, parse_saved_report, read_report_bytes, report_input_error, sha256_hex,
};

/// The exit code of invalid input.
const INPUT_EXIT_CODE: u8 = 2;

/// The error code of a compare with a report that is not complete.
const INCOMPLETE_CODE: &str = "FALLOW_MUTATION_COMPARE_INCOMPLETE";

/// The facts of the saved report that a compare reads.
#[derive(Debug)]
pub struct Baseline {
    report_sha256: String,
    mutants: Vec<SavedMutant>,
}

fn incomplete_error(message: impl Into<String>) -> ProgrammaticError {
    ProgrammaticError::new(message, INPUT_EXIT_CODE)
        .with_code(INCOMPLETE_CODE)
        .with_context("mutation.compare")
}

/// Reads the saved report of `--compare`.
///
/// # Errors
///
/// Returns an exit-2 error for a report that does not parse, a plan, and a
/// report without `completion.status: complete`. The mutants of a partial
/// report have no final status, so a compare with it proves nothing.
pub fn load(path: &Path) -> Result<Baseline, ProgrammaticError> {
    let bytes = read_report_bytes(path)?;
    let report = parse_saved_report(&bytes)?;
    if report.kind.as_deref() == Some("mutation-plan") {
        return Err(report_input_error(
            "the file is a mutation plan, which has no test results; pass a saved `fallow mutation --format json` report",
        )
        .with_context("mutation.compare"));
    }
    guard_complete(&report)?;
    Ok(Baseline {
        report_sha256: sha256_hex(&bytes),
        mutants: report.mutants,
    })
}

fn guard_complete(report: &SavedReport) -> Result<(), ProgrammaticError> {
    let status = report
        .completion
        .as_ref()
        .map(|completion| completion.status);
    if status == Some(MutationCompletionStatus::Complete) {
        return Ok(());
    }
    Err(incomplete_error(
        "the saved report is not complete (`completion.status` is not `complete`), so its mutants have no final result",
    )
    .with_help("compare with the report of a complete run of `fallow mutation`"))
}

/// Refuses a run that is not complete. The report of the run is still
/// written, so the caller can see why.
pub fn guard_run_complete(report: &MutationReport) -> Option<ProgrammaticError> {
    (report.completion.status != MutationCompletionStatus::Complete).then(|| {
        incomplete_error(
            "this run is not complete (`completion.status` is `partial`), so the compare proves nothing",
        )
        .with_help("see `completion.reasons` of the report, fix the cause, and run the compare again")
    })
}

const fn is_detected(status: MutantStatus) -> bool {
    matches!(status, MutantStatus::Killed | MutantStatus::Timeout)
}

const fn is_undetected(status: MutantStatus) -> bool {
    matches!(status, MutantStatus::Survived | MutantStatus::NoCoverage)
}

/// The change of a mutant in both runs, or `None` when the compare shows no
/// change. A `flaky` result in either run makes the mutant inconclusive.
fn change(previous: MutantStatus, current: MutantStatus) -> Option<MutationCompareChange> {
    if previous == MutantStatus::Flaky || current == MutantStatus::Flaky {
        return Some(MutationCompareChange::Inconclusive);
    }
    (is_detected(previous) && is_undetected(current)).then_some(MutationCompareChange::Regressed)
}

/// The tests that killed `mutant` in the saved report, sorted.
fn killing_tests(mutant: &SavedMutant) -> Vec<String> {
    let mut tests = if mutant.killing_tests.is_empty() {
        mutant.killed_by.iter().cloned().collect()
    } else {
        mutant.killing_tests.clone()
    };
    tests.sort();
    tests.dedup();
    tests
}

impl Baseline {
    /// The compare of `results`, the mutants of the current run, with the
    /// saved report.
    pub fn compare(&self, results: &[MutantResult]) -> MutationCompare {
        let saved: BTreeMap<&str, &SavedMutant> = self
            .mutants
            .iter()
            .map(|mutant| (mutant.mutant_id.as_str(), mutant))
            .collect();
        let mut compared = 0;
        let mut mutants = Vec::new();
        for result in results {
            let location = &result.location;
            let previous = saved.get(location.mutant_id.as_str());
            if previous.is_some() {
                compared += 1;
            }
            let previous_status = previous.and_then(|mutant| mutant.status);
            let entry = match previous {
                None => Some(MutationCompareChange::Added),
                Some(_) => previous_status.and_then(|status| change(status, result.status)),
            };
            if let Some(change) = entry {
                mutants.push(MutationCompareMutant {
                    mutant_id: location.mutant_id.clone(),
                    file: location.file.clone(),
                    line: location.line,
                    change,
                    previous_status,
                    status: Some(result.status),
                    previous_killing_tests: previous.map_or_else(Vec::new, |m| killing_tests(m)),
                });
            }
        }
        let current: rustc_hash::FxHashSet<&str> = results
            .iter()
            .map(|result| result.location.mutant_id.as_str())
            .collect();
        for mutant in saved
            .values()
            .filter(|m| !current.contains(m.mutant_id.as_str()))
        {
            mutants.push(MutationCompareMutant {
                mutant_id: mutant.mutant_id.clone(),
                file: mutant.file.clone(),
                line: mutant.line,
                change: MutationCompareChange::Removed,
                previous_status: mutant.status,
                status: None,
                previous_killing_tests: killing_tests(mutant),
            });
        }
        mutants.sort_by(|left, right| {
            (&left.file, left.line, &left.mutant_id).cmp(&(
                &right.file,
                right.line,
                &right.mutant_id,
            ))
        });
        let count = |change: MutationCompareChange| {
            mutants
                .iter()
                .filter(|mutant| mutant.change == change)
                .count()
        };
        MutationCompare {
            report_sha256: self.report_sha256.clone(),
            compared,
            regressed: count(MutationCompareChange::Regressed),
            inconclusive: count(MutationCompareChange::Inconclusive),
            added: count(MutationCompareChange::Added),
            removed: count(MutationCompareChange::Removed),
            mutants,
        }
    }
}

const fn status_name(status: MutantStatus) -> &'static str {
    match status {
        MutantStatus::Killed => "killed",
        MutantStatus::Timeout => "timeout",
        MutantStatus::Survived => "survived",
        MutantStatus::NoCoverage => "no_coverage",
        MutantStatus::RuntimeError => "runtime_error",
        MutantStatus::Flaky => "flaky",
        MutantStatus::Pruned => "pruned",
        MutantStatus::Pending => "pending",
        MutantStatus::Equivalent => "equivalent",
        MutantStatus::CompileError => "compile_error",
    }
}

fn status_text(status: Option<MutantStatus>) -> &'static str {
    status.map_or("absent", status_name)
}

/// The human summary of a compare, after the run report. Each regressed
/// mutant names the test that killed it in the saved report.
pub fn render(compare: &MutationCompare) -> String {
    let mut text = String::from("\nCompare with the saved report\n");
    let _ = writeln!(text, "  {:>4}  mutants in both runs", compare.compared);
    let rows = [
        (
            "regressed: detected before, not detected now",
            compare.regressed,
        ),
        (
            "inconclusive: flaky in one of the runs",
            compare.inconclusive,
        ),
        ("added: only in this run", compare.added),
        ("removed: only in the saved report", compare.removed),
    ];
    for (label, count) in rows.into_iter().filter(|(_, count)| *count > 0) {
        let _ = writeln!(text, "  {count:>4}  {label}");
    }
    let regressed = compare
        .mutants
        .iter()
        .filter(|mutant| mutant.change == MutationCompareChange::Regressed);
    for mutant in regressed {
        let killers = if mutant.previous_killing_tests.is_empty() {
            "no named test".to_owned()
        } else {
            mutant.previous_killing_tests.join(", ")
        };
        let _ = writeln!(
            text,
            "  {}:{}  {}  {} -> {}, killed before by {killers}",
            mutant.file,
            mutant.line,
            mutant.mutant_id,
            status_text(mutant.previous_status),
            status_text(mutant.status),
        );
    }
    if compare.regressed == 0 {
        text.push_str("No mutant lost its detection.\n");
    }
    text
}

#[cfg(test)]
#[path = "mutation_compare_tests.rs"]
mod tests;
