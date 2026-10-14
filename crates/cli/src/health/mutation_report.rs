//! `fallow health --mutation-report <file>`: the mutation score of a saved
//! `fallow mutation` report next to the CRAP score of each complexity finding.
//!
//! The join runs after the health report is complete, so the mutation score
//! never changes a CRAP score or the health score. A report file whose
//! SHA-256 digest differs from the current source file is stale: its
//! functions get no score and no counts.

use std::path::Path;

use fallow_output::{
    HealthFinding, HealthFindingMutation, HealthGrouping, HealthMutationStaleFile,
    HealthMutationStaleReason, HealthMutationState, HealthMutationSummary, HealthReport,
    HealthSection, MutationCompletionStatus, MutationStatusCounts,
};
use rustc_hash::FxHashMap;
use serde::Deserialize;

use crate::mutation_inspect::{read_report_bytes, report_input_error, sha256_hex};

/// The fields of a saved `fallow mutation` report that the join reads.
/// Unknown fields are ignored, so a report of a later minor version still
/// reads.
#[derive(Debug, Deserialize)]
struct SavedRun {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    completion: Option<SavedCompletion>,
    functions: Vec<SavedFunction>,
    #[serde(default)]
    files: Vec<SavedFile>,
}

#[derive(Debug, Deserialize)]
struct SavedCompletion {
    status: MutationCompletionStatus,
}

#[derive(Debug, Deserialize)]
struct SavedFunction {
    file: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    line: Option<u32>,
    counts: MutationStatusCounts,
}

#[derive(Debug, Deserialize)]
struct SavedFile {
    path: String,
    source_sha256: String,
}

/// A parsed mutation report, ready to join with a health report.
pub struct MutationEvidence {
    completion: Option<MutationCompletionStatus>,
    functions: Vec<SavedFunction>,
    digests: FxHashMap<String, String>,
}

/// Reads and parses a saved `fallow mutation` report.
///
/// # Errors
///
/// Returns an error with exit code 2 when the file cannot be read, is too
/// large, does not parse, or is not a `mutation` report. A plan has no
/// results, so it is refused.
pub fn read_mutation_evidence(
    path: &Path,
) -> Result<MutationEvidence, fallow_api::ProgrammaticError> {
    parse_mutation_evidence(&read_report_bytes(path)?)
}

fn parse_mutation_evidence(
    bytes: &[u8],
) -> Result<MutationEvidence, fallow_api::ProgrammaticError> {
    let run: SavedRun = serde_json::from_slice(bytes).map_err(|error| {
        report_input_error(format!(
            "the file is not a `fallow mutation --format json` report: {error}"
        ))
    })?;
    if let Some(kind) = run.kind.as_deref()
        && kind != "mutation"
    {
        return Err(report_input_error(format!(
            "the file is a `{kind}` document, not a `fallow mutation` run report"
        )));
    }
    Ok(MutationEvidence {
        completion: run.completion.map(|completion| completion.status),
        functions: run.functions,
        digests: run
            .files
            .into_iter()
            .map(|file| (file.path, file.source_sha256))
            .collect(),
    })
}

/// The state of each report file, keyed by root-relative path.
fn file_states(
    evidence: &MutationEvidence,
    root: &Path,
) -> FxHashMap<String, Option<HealthMutationStaleReason>> {
    let mut states = FxHashMap::default();
    for function in &evidence.functions {
        states
            .entry(function.file.clone())
            .or_insert_with(|| stale_reason(evidence, root, &function.file));
    }
    states
}

fn stale_reason(
    evidence: &MutationEvidence,
    root: &Path,
    file: &str,
) -> Option<HealthMutationStaleReason> {
    let Some(expected) = evidence.digests.get(file) else {
        return Some(HealthMutationStaleReason::NoDigest);
    };
    let Ok(bytes) = std::fs::read(root.join(file)) else {
        return Some(HealthMutationStaleReason::FileMissing);
    };
    (sha256_hex(&bytes) != *expected).then_some(HealthMutationStaleReason::SourceChanged)
}

/// The root-relative path of a finding, with forward slashes.
fn finding_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The function results of a report by path, first line and name, and the
/// state of each report file.
struct Join<'a> {
    by_key: FxHashMap<(&'a str, u32, &'a str), Vec<&'a SavedFunction>>,
    states: FxHashMap<String, Option<HealthMutationStaleReason>>,
    root: &'a Path,
}

impl<'a> Join<'a> {
    fn new(evidence: &'a MutationEvidence, root: &'a Path) -> Self {
        let mut by_key: FxHashMap<(&str, u32, &str), Vec<&SavedFunction>> = FxHashMap::default();
        for function in &evidence.functions {
            let (Some(line), Some(name)) = (function.line, function.name.as_deref()) else {
                continue;
            };
            by_key
                .entry((function.file.as_str(), line, name))
                .or_default()
                .push(function);
        }
        Self {
            by_key,
            states: file_states(evidence, root),
            root,
        }
    }

    /// The evidence of one finding. `None` when no function result or more
    /// than one function result matches.
    fn evidence(&self, finding: &HealthFinding) -> Option<HealthFindingMutation> {
        let path = finding_path(&finding.violation.path, self.root);
        let key = (
            path.as_str(),
            finding.violation.line,
            finding.violation.name.as_str(),
        );
        let [function] = self.by_key.get(&key).map(Vec::as_slice)? else {
            return None;
        };
        Some(match self.states.get(&function.file).copied().flatten() {
            Some(reason) => HealthFindingMutation {
                state: HealthMutationState::Stale,
                stale_reason: Some(reason),
                score: None,
                counts: None,
            },
            None => HealthFindingMutation {
                state: HealthMutationState::Fresh,
                stale_reason: None,
                score: function.counts.score(),
                counts: Some(function.counts),
            },
        })
    }

    fn stale_files(&self) -> Vec<HealthMutationStaleFile> {
        let mut files: Vec<HealthMutationStaleFile> = self
            .states
            .iter()
            .filter_map(|(path, reason)| {
                reason.map(|reason| HealthMutationStaleFile {
                    path: path.clone(),
                    reason,
                })
            })
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files
    }
}

/// Adds the mutation evidence to each complexity finding that one function
/// result of the report matches by path, first line and name, also to the
/// findings of each `--group-by` group. Two function results with the same
/// key make the match ambiguous, so the finding gets no evidence. The CRAP
/// scores and the health score do not change.
fn attach_mutation_evidence(
    report: &mut HealthReport,
    groups: Option<&mut HealthGrouping>,
    evidence: &MutationEvidence,
    root: &Path,
) {
    let join = Join::new(evidence, root);
    let (mut fresh, mut stale, mut unmatched) = (0, 0, 0);
    for finding in &mut report.findings {
        finding.violation.mutation = join.evidence(finding);
        match finding
            .violation
            .mutation
            .as_ref()
            .map(|mutation| mutation.state)
        {
            Some(HealthMutationState::Fresh) => fresh += 1,
            Some(HealthMutationState::Stale) => stale += 1,
            None => unmatched += 1,
        }
    }
    for group in groups
        .into_iter()
        .flat_map(|groups| groups.groups.iter_mut())
    {
        for finding in &mut group.findings {
            finding.violation.mutation = join.evidence(finding);
        }
    }

    report.mutation = Some(HealthMutationSummary {
        completion: evidence.completion,
        functions: evidence.functions.len(),
        findings_fresh: fresh,
        findings_stale: stale,
        findings_unmatched: unmatched,
        stale_files: join.stale_files(),
    });
    if let Some(sections) = report.sections.as_mut()
        && !sections.contains(&HealthSection::Mutation)
    {
        sections.push(HealthSection::Mutation);
        sections.sort();
    }
}

/// Adds the mutation evidence to the report and the groups of a health run.
pub fn attach_to_result(result: &mut super::HealthResult, evidence: &MutationEvidence) {
    let root = result.config.root.clone();
    attach_mutation_evidence(
        &mut result.report,
        result.grouping.as_mut(),
        evidence,
        &root,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_or_another_document_is_refused() {
        let error = parse_mutation_evidence(br#"{"kind":"mutation-plan","functions":[]}"#)
            .err()
            .expect("a plan is refused");
        assert_eq!(error.exit_code, 2);
        assert!(parse_mutation_evidence(br#"{"kind":"health","functions":[]}"#).is_err());
        assert!(parse_mutation_evidence(br#"{"kind":"mutation-plan","mutants":[]}"#).is_err());
    }

    #[test]
    fn a_file_without_a_digest_is_stale() {
        let evidence = parse_mutation_evidence(
            br#"{"kind":"mutation","functions":[{"file":"src/a.ts","name":"a","line":1,
            "counts":{"total":1,"killed":1,"timeout":0,"survived":0,"no_coverage":0,
            "runtime_error":0,"flaky":0,"pruned":0,"pending":0,"equivalent":0}}],"files":[]}"#,
        )
        .expect("report");
        let root = tempfile::tempdir().expect("root");
        assert_eq!(
            stale_reason(&evidence, root.path(), "src/a.ts"),
            Some(HealthMutationStaleReason::NoDigest)
        );
    }
}
