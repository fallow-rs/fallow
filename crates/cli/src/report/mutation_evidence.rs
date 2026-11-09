//! The mutation evidence of one `fallow report` call: the saved report of
//! `fallow mutation`, the error envelope of a failed run, or the reason that
//! the CI job did not run mutation testing.
//!
//! A CI job writes the report after it ran project code, so this module reads
//! the file as untrusted input. An input error exits 2. A mutation report that
//! does not parse becomes the unreadable state, so the analysis output still
//! renders. A survived mutant becomes a finding only when its file still has
//! the SHA-256 digest of the report, so a line number never points into a
//! changed file.

use std::path::Path;

use fallow_api::ProgrammaticError;
use fallow_output::{
    CiIssue, CodeClimateIssue, CodeClimateIssueInput, CodeClimateSeverity, GateOutcome, GateStatus,
    MutantChange, MutantStatus, MutationCiLocation, MutationCiState, MutationCiView,
    MutationCompletion, MutationIncompleteReason, MutationNotRunReason, MutationScopeKind,
    build_codeclimate_issue, codeclimate_fingerprint_hash, issues_from_codeclimate_issues,
    survived_description,
};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::cli_args::MutationNotRunArg;
use crate::mutation_inspect::{
    SavedMutant, SavedReport, read_report_bytes, report_input_error, report_relative_path,
    sha256_hex,
};

/// The maximum number of survived mutants that one render shows as findings.
pub const MUTATION_MAX_INLINE: usize = 20;

/// The rule of a survived mutant finding.
pub const MUTATION_SURVIVED_RULE: &str = "fallow/mutation-survived";

/// The only gate that a mutation report can add to the analysis results.
const MIN_SCORE_GATE: &str = "mutation-min-score";

/// The CodeClimate category of a survivor: a code change that no test
/// detects is a risk of an undetected bug.
const SURVIVED_CATEGORY: &str = "Bug Risk";

/// The kinds of the other `fallow mutation` documents, which are not a report
/// of a run.
const REFUSED_KINDS: [&str; 3] = ["mutation-plan", "mutation-tasks", "mutation-inspect"];

/// The maximum number of characters of an untrusted value in a message.
const VALUE_MAX_CHARS: usize = 80;

/// The maximum number of characters of the reason of an unreadable report.
const REASON_MAX_CHARS: usize = 500;

/// Each reason of a partial run, to map a wire name to its value.
const INCOMPLETE_REASONS: [MutationIncompleteReason; 11] = [
    MutationIncompleteReason::RuntimeError,
    MutationIncompleteReason::MutantLimit,
    MutationIncompleteReason::SampledOut,
    MutationIncompleteReason::TimeBudget,
    MutationIncompleteReason::SkippedFiles,
    MutationIncompleteReason::UnloadedFiles,
    MutationIncompleteReason::TestFilesNotDiscovered,
    MutationIncompleteReason::BaselineFailure,
    MutationIncompleteReason::WorktreeChanged,
    MutationIncompleteReason::TypecheckFailed,
    MutationIncompleteReason::WorktreeUnchecked,
];

/// The stdout document of `fallow mutation --format json` on exit 2.
#[derive(Deserialize)]
struct ErrorEnvelope {
    message: String,
    exit_code: i32,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    help: Option<String>,
}

/// One survived mutant whose file has the digest of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurvivorFinding {
    pub mutant_id: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub operator: String,
    pub function: Option<String>,
    pub change: Option<MutantChange>,
}

impl SurvivorFinding {
    fn from_saved(mutant: &SavedMutant) -> Self {
        Self {
            mutant_id: mutant.mutant_id.clone(),
            file: mutant.file.clone(),
            line: mutant.line,
            column: mutant.column,
            operator: mutant.operator.clone(),
            function: mutant.function.clone(),
            change: mutant.change.clone(),
        }
    }

    fn sort_key(&self) -> (&str, u32, u32, &str) {
        (&self.file, self.line, self.column, &self.mutant_id)
    }

    fn codeclimate_issue(&self) -> CodeClimateIssue {
        let description = survived_description(
            &self.operator,
            self.change.as_ref(),
            self.function.as_deref(),
            &self.mutant_id,
        );
        let fingerprint = codeclimate_fingerprint_hash(&[
            MUTATION_SURVIVED_RULE,
            &self.file,
            &self.operator,
            &self.mutant_id,
        ]);
        build_codeclimate_issue(CodeClimateIssueInput {
            check_name: MUTATION_SURVIVED_RULE,
            description: &description,
            severity: CodeClimateSeverity::Minor,
            category: SURVIVED_CATEGORY,
            path: &self.file,
            begin_line: Some(self.line),
            fingerprint: &fingerprint,
        })
    }
}

/// The mutation input of one render: the view for the section and the
/// decision row, and the survivors for the findings.
#[derive(Debug, Clone)]
pub struct MutationEvidence {
    view: MutationCiView,
    survivors: Vec<SurvivorFinding>,
}

impl MutationEvidence {
    /// Reads the file of `--mutation-report`. `root` is the `--root` of the
    /// report call, which must be the root of the mutation run.
    ///
    /// # Errors
    ///
    /// Returns an exit-2 `FALLOW_MUTATION_REPORT_INVALID` error when the file
    /// cannot be read, is larger than 64 MiB, is another document, is a
    /// `--lean` document, or has a mutant path outside the root. A mutation
    /// report that does not parse is not an error: it gives the unreadable
    /// state and a warning on stderr.
    pub fn load(path: &Path, root: &Path) -> Result<Self, ProgrammaticError> {
        let evidence = Self::parse(&read_report_bytes(path)?, root)?;
        if let MutationCiState::Unreadable { reason } = &evidence.view.state {
            eprintln!("warning: FALLOW_MUTATION_REPORT_INVALID: {reason}");
        }
        Ok(evidence)
    }

    fn parse(bytes: &[u8], root: &Path) -> Result<Self, ProgrammaticError> {
        let value: Value = match serde_json::from_slice(bytes) {
            Ok(value) => value,
            Err(error) => {
                return Ok(Self::unreadable(&format!(
                    "the file is not valid JSON: {error}"
                )));
            }
        };
        if is_error_envelope(&value) {
            return Self::from_error_envelope(value);
        }
        check_document(&value)?;
        let report: SavedReport = match serde_json::from_value(value) {
            Ok(report) => report,
            Err(error) => return Ok(Self::unreadable(&error.to_string())),
        };
        for mutant in &report.mutants {
            report_relative_path(&mutant.file)?;
        }
        Ok(Self::from_report(&report, root).unwrap_or_else(|reason| Self::unreadable(&reason)))
    }

    fn from_error_envelope(value: Value) -> Result<Self, ProgrammaticError> {
        let envelope: ErrorEnvelope = serde_json::from_value(value).map_err(|error| {
            report_input_error(format!(
                "the file is not a valid error envelope of `fallow mutation`: {}",
                one_line(&error.to_string(), REASON_MAX_CHARS)
            ))
        })?;
        Ok(Self::with_state(MutationCiState::Failed {
            exit_code: envelope.exit_code,
            code: envelope.code.unwrap_or_default(),
            message: envelope.message,
            help: envelope.help,
        }))
    }

    fn from_report(report: &SavedReport, root: &Path) -> Result<Self, String> {
        let summary = report
            .summary
            .as_ref()
            .ok_or_else(|| "the report has no `summary`".to_owned())?;
        let min_score_gate = min_score_gate(report.gate_outcomes.as_ref())?;
        let mut digests = FileDigests::new(report, root);
        let mut survivors = Vec::new();
        let mut no_coverage = Vec::new();
        let mut stale_hidden = 0;
        for mutant in &report.mutants {
            match mutant.status {
                Some(MutantStatus::Survived) if digests.is_fresh(&mutant.file) => {
                    survivors.push(SurvivorFinding::from_saved(mutant));
                }
                Some(MutantStatus::Survived) => stale_hidden += 1,
                Some(MutantStatus::NoCoverage) => no_coverage.push(MutationCiLocation {
                    file: mutant.file.clone(),
                    line: mutant.line,
                    function: mutant.function.clone(),
                }),
                _ => {}
            }
        }
        survivors.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        survivors.dedup();
        no_coverage.sort();
        no_coverage.dedup();
        Ok(Self {
            view: MutationCiView {
                scope: report
                    .scope
                    .as_ref()
                    .and_then(|scope| scope_kind(&scope.kind)),
                counts: summary.counts,
                score: summary.score,
                completion: report.completion.as_ref().map(|completion| {
                    let mut reasons: Vec<MutationIncompleteReason> = completion
                        .reasons
                        .iter()
                        .filter_map(|name| incomplete_reason(name))
                        .collect();
                    reasons.sort_unstable();
                    reasons.dedup();
                    MutationCompletion {
                        status: completion.status,
                        reasons,
                    }
                }),
                min_score_gate,
                no_coverage,
                stale_hidden,
                elapsed_ms: report.elapsed_ms,
                cache_hits: report.cache.as_ref().map(|cache| cache.hits),
                ..MutationCiView::new(MutationCiState::Ran)
            },
            survivors,
        })
    }

    /// The evidence of a job that did not run mutation testing, or whose run
    /// a signal stopped before it wrote a report.
    #[must_use]
    pub fn not_run(reason: MutationNotRunArg) -> Self {
        Self::with_state(match reason {
            MutationNotRunArg::EventRefused => {
                MutationCiState::NotRun(MutationNotRunReason::EventRefused)
            }
            MutationNotRunArg::ForkRefused => {
                MutationCiState::NotRun(MutationNotRunReason::ForkRefused)
            }
            MutationNotRunArg::NoChangeScope => {
                MutationCiState::NotRun(MutationNotRunReason::NoChangeScope)
            }
            MutationNotRunArg::Interrupted => MutationCiState::Interrupted,
        })
    }

    fn with_state(state: MutationCiState) -> Self {
        Self {
            view: MutationCiView::new(state),
            survivors: Vec::new(),
        }
    }

    fn unreadable(reason: &str) -> Self {
        Self::with_state(MutationCiState::Unreadable {
            reason: one_line(reason, REASON_MAX_CHARS),
        })
    }

    /// Sets the `--root` that the inspect command of the section names, as
    /// the user wrote it. `None` names no root.
    #[must_use]
    pub fn with_root_flag(mut self, root: Option<&Path>) -> Self {
        self.view.root_flag = root.map(|root| root.to_string_lossy().into_owned());
        self
    }

    /// The view that the section, the decision row and the run notice
    /// render.
    #[must_use]
    pub const fn view(&self) -> &MutationCiView {
        &self.view
    }

    /// The survivor findings on the lines of the shared diff, in (file, line,
    /// column, mutant id) order and capped at [`MUTATION_MAX_INLINE`]. The
    /// view counts the survivors over the cap as `inline_overflow`.
    ///
    /// Paths are root-relative. The caller adds the report path prefix, as
    /// for the analysis issues.
    pub fn codeclimate_issues(&mut self) -> Vec<CodeClimateIssue> {
        self.filtered_issues(super::ci::diff_filter::filter_issues_from_env)
    }

    fn filtered_issues(
        &mut self,
        diff_filter: impl FnOnce(Vec<CiIssue>) -> Vec<CiIssue>,
    ) -> Vec<CodeClimateIssue> {
        let issues = self.survivor_issues();
        let kept: FxHashSet<String> = diff_filter(issues_from_codeclimate_issues(&issues))
            .into_iter()
            .map(|issue| issue.fingerprint)
            .collect();
        let total = issues.len();
        let on_diff: Vec<CodeClimateIssue> = issues
            .into_iter()
            .filter(|issue| kept.contains(&issue.fingerprint))
            .collect();
        self.view.off_diff_hidden = total - on_diff.len();
        self.cap_inline(on_diff)
    }

    fn survivor_issues(&self) -> Vec<CodeClimateIssue> {
        self.survivors
            .iter()
            .map(SurvivorFinding::codeclimate_issue)
            .collect()
    }

    fn cap_inline(&mut self, mut issues: Vec<CodeClimateIssue>) -> Vec<CodeClimateIssue> {
        self.view.inline_overflow = issues.len().saturating_sub(MUTATION_MAX_INLINE);
        issues.truncate(MUTATION_MAX_INLINE);
        issues
    }

    /// Adds the `mutation-min-score` entry of the report to the
    /// `gate_outcomes` of the analysis envelope, so the status note, the gate
    /// rows and the gate lines of each surface name it. The map is created
    /// when it is absent. An entry with the same name is replaced, with a
    /// note on stderr. No other gate of the report reaches the envelope.
    pub fn merge_gate_outcomes(&self, envelope: &mut Value) {
        let Some(entry) = self
            .view
            .min_score_gate
            .as_ref()
            .and_then(|gate| serde_json::to_value(gate).ok())
        else {
            return;
        };
        let Some(object) = envelope.as_object_mut() else {
            return;
        };
        let gates = object
            .entry("gate_outcomes")
            .or_insert_with(|| Value::Object(Map::new()));
        if !gates.is_object() {
            *gates = Value::Object(Map::new());
        }
        let Some(gates) = gates.as_object_mut() else {
            return;
        };
        if gates.insert(MIN_SCORE_GATE.to_owned(), entry).is_some() {
            eprintln!(
                "note: the `{MIN_SCORE_GATE}` gate of the mutation report replaces the entry of the same name in the analysis results"
            );
        }
    }
}

/// The current digest check of each file of a report. One file is read and
/// hashed at most once.
struct FileDigests<'a> {
    report: &'a SavedReport,
    root: &'a Path,
    fresh: FxHashMap<&'a str, bool>,
}

impl<'a> FileDigests<'a> {
    fn new(report: &'a SavedReport, root: &'a Path) -> Self {
        Self {
            report,
            root,
            fresh: FxHashMap::default(),
        }
    }

    /// True when the file under the root has the digest of the report. A file
    /// that is missing, or that the report has no digest for, is not fresh.
    fn is_fresh(&mut self, file: &'a str) -> bool {
        let (report, root) = (self.report, self.root);
        *self.fresh.entry(file).or_insert_with(|| {
            let Some(expected) = report.saved_file(file).map(|saved| &saved.source_sha256) else {
                return false;
            };
            std::fs::read(root.join(file))
                .is_ok_and(|bytes| sha256_hex(&bytes) == expected.as_str())
        })
    }
}

fn is_error_envelope(value: &Value) -> bool {
    value.get("kind").is_none() && value.get("error") == Some(&Value::Bool(true))
}

/// Refuses each document that is not the full report of a run.
fn check_document(value: &Value) -> Result<(), ProgrammaticError> {
    match value.get("kind").and_then(Value::as_str) {
        Some("mutation") => {}
        Some(kind) if REFUSED_KINDS.contains(&kind) => {
            return Err(report_input_error(format!(
                "the file is a `{kind}` document; --mutation-report reads the report of `fallow mutation --format json`"
            )));
        }
        Some(kind) => {
            return Err(report_input_error(format!(
                "the file is a `{}` document, not a mutation report or a mutation error envelope",
                one_line(kind, VALUE_MAX_CHARS)
            )));
        }
        None => {
            return Err(report_input_error(
                "the file is not a mutation report or a mutation error envelope",
            ));
        }
    }
    if value.get("profile").and_then(Value::as_str) == Some(fallow_output::LEAN_PROFILE) {
        return Err(report_input_error(
            "the mutation report is a --lean document; pass the full report of `fallow mutation --format json`",
        ));
    }
    Ok(())
}

/// The `mutation-min-score` entry of the report. A gate entry that does not
/// parse makes the report unreadable, so a configured gate never disappears
/// without a trace.
fn min_score_gate(gates: Option<&Map<String, Value>>) -> Result<Option<GateOutcome>, String> {
    let Some(entry) = gates.and_then(|gates| gates.get(MIN_SCORE_GATE)) else {
        return Ok(None);
    };
    let invalid = || format!("the `{MIN_SCORE_GATE}` gate entry is not a valid gate outcome");
    let status = match entry.get("status").and_then(Value::as_str) {
        Some("pass") => GateStatus::Pass,
        Some("warn") => GateStatus::Warn,
        Some("fail") => GateStatus::Fail,
        Some("skipped") => GateStatus::Skipped,
        _ => return Err(invalid()),
    };
    let enforced = entry
        .get("enforced")
        .and_then(Value::as_bool)
        .ok_or_else(invalid)?;
    let number = |key: &str| match entry.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_f64().map(Some).ok_or_else(invalid),
    };
    Ok(Some(GateOutcome {
        observed: number("observed")?,
        threshold: number("threshold")?,
        ..GateOutcome::new(status, enforced)
    }))
}

/// The scope kind of its wire name. A name that this build does not know
/// gives no scope line.
fn scope_kind(name: &str) -> Option<MutationScopeKind> {
    match name {
        "changed" => Some(MutationScopeKind::Changed),
        "all" => Some(MutationScopeKind::All),
        "paths" => Some(MutationScopeKind::Paths),
        _ => None,
    }
}

/// The reason of a partial run of its wire name. A name that this build does
/// not know is dropped.
fn incomplete_reason(name: &str) -> Option<MutationIncompleteReason> {
    INCOMPLETE_REASONS
        .into_iter()
        .find(|reason| serde_json::to_value(reason).is_ok_and(|value| value.as_str() == Some(name)))
}

/// `text` as one line with no control character, cut at `max_chars`. An
/// untrusted value goes to stderr, and a line break there could start a CI
/// workflow command.
fn one_line(text: &str, max_chars: usize) -> String {
    let flat: String = text
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let mut cut: String = flat.chars().take(max_chars).collect();
    cut.push_str("...");
    cut
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests {
    use super::*;
    use fallow_output::{
        CodeClimateSeverity, GateOutcome, GateStatus, MutationCiLocation, MutationCompletion,
        MutationCompletionStatus, MutationIncompleteReason, MutationNotRunReason,
        MutationScopeKind, MutationStatusCounts,
    };
    use serde_json::json;

    const SOURCE: &str = "export const isAdult = (age) => age >= 18;\n";
    const OTHER: &str = "export const isChild = (age) => age < 18;\n";

    fn sha(text: &str) -> String {
        crate::mutation_inspect::sha256_hex(text.as_bytes())
    }

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/age.ts"), SOURCE).unwrap();
        std::fs::write(dir.path().join("src/other.ts"), OTHER).unwrap();
        dir
    }

    fn mutant(id: &str, file: &str, line: u32, column: u32, status: &str) -> Value {
        json!({
            "mutant_id": id, "file": file, "line": line, "column": column,
            "span": [32, 34], "operator": "relational-operator", "function": "isAdult",
            "change": { "from": ">=", "to": ">" }, "status": status
        })
    }

    fn counts() -> Value {
        serde_json::to_value(MutationStatusCounts {
            total: 18,
            killed: 10,
            survived: 6,
            no_coverage: 2,
            ..MutationStatusCounts::default()
        })
        .unwrap()
    }

    fn report(mutants: &[Value]) -> Value {
        json!({
            "kind": "mutation", "schema_version": "2", "elapsed_ms": 252_000,
            "completion": { "status": "partial", "reasons": ["time-budget", "later-reason"] },
            "scope": { "kind": "changed", "base_ref": "origin/main" },
            "cache": { "hits": 3, "stored": 0, "not_keyed": [] },
            "summary": { "counts": counts(), "score": 0.625, "coverage": 0.9 },
            "gate_outcomes": { "mutation-min-score": {
                "status": "fail", "enforced": true, "observed": 62.5, "threshold": 80.0
            } },
            "mutants": mutants,
            "files": [
                { "path": "src/age.ts", "source_sha256": sha(SOURCE) },
                { "path": "src/other.ts", "source_sha256": sha(OTHER) }
            ]
        })
    }

    fn parse(value: &Value, root: &Path) -> Result<MutationEvidence, ProgrammaticError> {
        MutationEvidence::parse(value.to_string().as_bytes(), root)
    }

    fn input_error(value: &Value) -> ProgrammaticError {
        let dir = project();
        parse(value, dir.path()).unwrap_err()
    }

    fn assert_input_error(error: &ProgrammaticError, needle: &str) {
        assert_eq!(error.exit_code, 2);
        assert_eq!(
            error.code.as_deref(),
            Some("FALLOW_MUTATION_REPORT_INVALID")
        );
        assert!(error.message.contains(needle), "{}", error.message);
    }

    fn unreadable_reason(evidence: &MutationEvidence) -> &str {
        match &evidence.view.state {
            MutationCiState::Unreadable { reason } => reason,
            other => panic!("expected the unreadable state, got {other:?}"),
        }
    }

    #[test]
    fn a_report_gives_the_view_the_survivors_and_the_gate() {
        let dir = project();
        let evidence = parse(
            &report(&[
                mutant("m2", "src/age.ts", 1, 37, "survived"),
                mutant("m1", "src/age.ts", 1, 33, "survived"),
                mutant("m3", "src/other.ts", 1, 30, "no_coverage"),
                mutant("m4", "src/other.ts", 1, 31, "killed"),
            ]),
            dir.path(),
        )
        .unwrap();

        let view = &evidence.view;
        assert_eq!(view.state, MutationCiState::Ran);
        assert_eq!(view.scope, Some(MutationScopeKind::Changed));
        assert_eq!(view.counts.survived, 6);
        assert_eq!(view.score, Some(0.625));
        assert_eq!(
            view.completion,
            Some(MutationCompletion {
                status: MutationCompletionStatus::Partial,
                reasons: vec![MutationIncompleteReason::TimeBudget],
            })
        );
        assert_eq!(
            view.min_score_gate,
            Some(GateOutcome::measured(GateStatus::Fail, true, 62.5, 80.0))
        );
        assert_eq!(
            view.no_coverage,
            vec![MutationCiLocation {
                file: "src/other.ts".to_owned(),
                line: 1,
                function: Some("isAdult".to_owned()),
            }]
        );
        assert_eq!(view.elapsed_ms, Some(252_000));
        assert_eq!(view.cache_hits, Some(3));
        assert_eq!(view.stale_hidden, 0);
        let ids: Vec<&str> = evidence
            .survivors
            .iter()
            .map(|survivor| survivor.mutant_id.as_str())
            .collect();
        assert_eq!(ids, ["m1", "m2"]);
    }

    #[test]
    fn an_error_envelope_gives_the_failed_state() {
        let dir = project();
        let evidence = parse(
            &json!({
                "error": true, "message": "no test runner", "exit_code": 2,
                "code": "FALLOW_MUTATION_NO_RUNNER", "help": "install vitest"
            }),
            dir.path(),
        )
        .unwrap();
        assert_eq!(
            evidence.view.state,
            MutationCiState::Failed {
                exit_code: 2,
                code: "FALLOW_MUTATION_NO_RUNNER".to_owned(),
                message: "no test runner".to_owned(),
                help: Some("install vitest".to_owned()),
            }
        );
        assert!(evidence.survivors.is_empty());
    }

    #[test]
    fn plans_tasks_inspect_lean_and_other_documents_exit_2() {
        for kind in ["mutation-plan", "mutation-tasks", "mutation-inspect"] {
            let error = input_error(&json!({ "kind": kind, "mutants": [] }));
            assert_input_error(&error, kind);
        }
        let mut lean = report(&[]);
        lean["profile"] = json!("lean");
        assert_input_error(&input_error(&lean), "--lean");
        assert_input_error(&input_error(&json!({ "kind": "health" })), "health");
        assert_input_error(&input_error(&json!([1, 2])), "not a mutation report");
    }

    #[test]
    fn an_absolute_or_parent_path_exits_2() {
        for file in ["/etc/passwd", "../outside.ts", "src/../../outside.ts"] {
            let error = input_error(&report(&[mutant("m1", file, 1, 1, "killed")]));
            assert_input_error(&error, "not inside the project root");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_drive_or_root_path_exits_2() {
        // Neither path is absolute on Windows, but each one leaves the root.
        for file in ["C:outside.ts", "\\outside.ts"] {
            let error = input_error(&report(&[mutant("m1", file, 1, 1, "killed")]));
            assert_input_error(&error, "not inside the project root");
        }
    }

    #[test]
    fn the_survivors_that_the_diff_filter_removes_are_counted() {
        let dir = project();
        let mutants = vec![
            mutant("a1", "src/age.ts", 1, 1, "survived"),
            mutant("o1", "src/other.ts", 1, 1, "survived"),
            mutant("o2", "src/other.ts", 2, 1, "survived"),
        ];
        let mut evidence = parse(&report(&mutants), dir.path()).unwrap();
        let on_age = |issues: Vec<CiIssue>| -> Vec<CiIssue> {
            issues
                .into_iter()
                .filter(|issue| issue.path == "src/age.ts")
                .collect()
        };

        assert_eq!(evidence.filtered_issues(on_age).len(), 1);
        assert_eq!(evidence.view.off_diff_hidden, 2);
        assert_eq!(evidence.filtered_issues(|issues| issues).len(), 3);
        assert_eq!(evidence.view.off_diff_hidden, 0);
    }

    #[test]
    fn a_missing_or_oversized_file_exits_2() {
        let dir = project();
        let missing = MutationEvidence::load(&dir.path().join("none.json"), dir.path());
        assert_input_error(&missing.unwrap_err(), "failed to read");

        let large = dir.path().join("large.json");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        let error = MutationEvidence::load(&large, dir.path()).unwrap_err();
        assert_input_error(&error, "larger than 64 MiB");
    }

    #[test]
    fn a_mutation_report_that_does_not_parse_is_unreadable() {
        let dir = project();
        let mut bad_type = report(&[mutant("m1", "src/age.ts", 1, 1, "survived")]);
        bad_type["mutants"][0]["line"] = json!("ten");
        let evidence = parse(&bad_type, dir.path()).unwrap();
        assert!(unreadable_reason(&evidence).contains("invalid type"));

        let mut no_summary = report(&[]);
        no_summary.as_object_mut().unwrap().remove("summary");
        let evidence = parse(&no_summary, dir.path()).unwrap();
        assert!(unreadable_reason(&evidence).contains("summary"));

        let mut no_mutants = report(&[]);
        no_mutants.as_object_mut().unwrap().remove("mutants");
        let evidence = parse(&no_mutants, dir.path()).unwrap();
        assert!(unreadable_reason(&evidence).contains("mutants"));

        let mut bad_gate = report(&[]);
        bad_gate["gate_outcomes"]["mutation-min-score"]["status"] = json!("maybe");
        let evidence = parse(&bad_gate, dir.path()).unwrap();
        assert!(unreadable_reason(&evidence).contains("mutation-min-score"));

        let evidence = MutationEvidence::parse(b"{\"kind\":\"mutation\",", dir.path()).unwrap();
        assert!(unreadable_reason(&evidence).contains("not valid JSON"));
    }

    #[test]
    fn an_unreadable_reason_has_no_line_break() {
        let dir = project();
        let mut forged = report(&[]);
        forged["scope"] = json!(["\n::error::forged"]);
        forged["completion"]["status"] = json!("\n::error::forged");
        let evidence = parse(&forged, dir.path()).unwrap();
        let reason = unreadable_reason(&evidence);
        assert!(!reason.contains('\n'), "{reason}");
        assert!(!reason.contains('\r'), "{reason}");
    }

    #[test]
    fn a_changed_or_missing_file_hides_its_survivors_as_stale() {
        let dir = project();
        let mut value = report(&[
            mutant("m1", "src/age.ts", 1, 33, "survived"),
            mutant("m2", "src/age.ts", 1, 37, "survived"),
            mutant("m3", "src/other.ts", 1, 30, "survived"),
            mutant("m4", "src/gone.ts", 1, 30, "survived"),
            mutant("m5", "src/undigested.ts", 1, 30, "survived"),
        ]);
        value["files"][0]["source_sha256"] = json!(sha("an older text"));
        value["files"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "path": "src/gone.ts", "source_sha256": sha(SOURCE) }));
        std::fs::write(dir.path().join("src/undigested.ts"), SOURCE).unwrap();

        let evidence = parse(&value, dir.path()).unwrap();

        let ids: Vec<&str> = evidence
            .survivors
            .iter()
            .map(|survivor| survivor.mutant_id.as_str())
            .collect();
        assert_eq!(ids, ["m3"]);
        assert_eq!(evidence.view.stale_hidden, 4);
    }

    #[test]
    fn a_survivor_issue_has_the_rule_the_severity_and_a_line_free_fingerprint() {
        let dir = project();
        let at = |line: u32, id: &str| {
            let evidence = parse(
                &report(&[mutant(id, "src/age.ts", line, 33, "survived")]),
                dir.path(),
            )
            .unwrap();
            evidence.survivor_issues().remove(0)
        };
        let issue = at(14, "9f3c2a1b");
        assert_eq!(issue.check_name, MUTATION_SURVIVED_RULE);
        assert_eq!(issue.severity, CodeClimateSeverity::Minor);
        assert_eq!(issue.location.path, "src/age.ts");
        assert_eq!(issue.location.lines.begin, 14);
        assert_eq!(
            issue.description,
            "Changing >= to > in isAdult made no test fail. Mutant 9f3c2a1b."
        );
        assert_eq!(
            issue.fingerprint,
            fallow_output::codeclimate_fingerprint_hash(&[
                MUTATION_SURVIVED_RULE,
                "src/age.ts",
                "relational-operator",
                "9f3c2a1b",
            ])
        );
        assert_eq!(at(20, "9f3c2a1b").fingerprint, issue.fingerprint);
        assert_ne!(at(14, "0a1b2c3d").fingerprint, issue.fingerprint);
    }

    #[test]
    fn the_inline_cap_keeps_20_survivors_and_counts_the_overflow() {
        let dir = project();
        let mutants: Vec<Value> = (1..=25)
            .map(|line| mutant(&format!("m{line:02}"), "src/age.ts", line, 1, "survived"))
            .collect();
        let mut evidence = parse(&report(&mutants), dir.path()).unwrap();
        let issues = evidence.survivor_issues();
        let kept = evidence.cap_inline(issues);
        assert_eq!(kept.len(), MUTATION_MAX_INLINE);
        assert_eq!(evidence.view.inline_overflow, 5);
        assert_eq!(kept[0].location.lines.begin, 1);
        assert_eq!(kept[19].location.lines.begin, 20);
    }

    #[test]
    fn the_cap_counts_only_the_survivors_on_the_diff() {
        let dir = project();
        let mut mutants: Vec<Value> = (1..=22)
            .map(|line| mutant(&format!("a{line:02}"), "src/age.ts", line, 1, "survived"))
            .collect();
        mutants.extend(
            (1..=5).map(|line| mutant(&format!("o{line}"), "src/other.ts", line, 1, "survived")),
        );
        let mut evidence = parse(&report(&mutants), dir.path()).unwrap();
        let on_age = |issues: Vec<CiIssue>| -> Vec<CiIssue> {
            issues
                .into_iter()
                .filter(|issue| issue.path == "src/age.ts")
                .collect()
        };

        let issues = evidence.filtered_issues(on_age);
        assert_eq!(issues.len(), MUTATION_MAX_INLINE);
        assert!(
            issues
                .iter()
                .all(|issue| issue.location.path == "src/age.ts")
        );
        assert_eq!(evidence.view.inline_overflow, 2);

        assert_eq!(evidence.filtered_issues(on_age).len(), MUTATION_MAX_INLINE);
        assert_eq!(evidence.view.inline_overflow, 2);
    }

    #[test]
    fn input_in_reverse_order_gives_the_same_output() {
        let dir = project();
        let mutants = vec![
            mutant("b", "src/age.ts", 2, 5, "survived"),
            mutant("a", "src/age.ts", 2, 5, "survived"),
            mutant("c", "src/age.ts", 1, 9, "survived"),
            mutant("d", "src/other.ts", 1, 1, "survived"),
            mutant("e", "src/other.ts", 3, 1, "no_coverage"),
            mutant("f", "src/age.ts", 4, 1, "no_coverage"),
        ];
        let reversed: Vec<Value> = mutants.iter().rev().cloned().collect();
        let forward = parse(&report(&mutants), dir.path()).unwrap();
        let backward = parse(&report(&reversed), dir.path()).unwrap();

        let ids: Vec<&str> = forward
            .survivors
            .iter()
            .map(|survivor| survivor.mutant_id.as_str())
            .collect();
        assert_eq!(ids, ["c", "a", "b", "d"]);
        assert_eq!(forward.survivors, backward.survivors);
        assert_eq!(forward.view, backward.view);
        let fingerprints = |evidence: &MutationEvidence| -> Vec<String> {
            evidence
                .survivor_issues()
                .into_iter()
                .map(|issue| issue.fingerprint)
                .collect()
        };
        assert_eq!(fingerprints(&forward), fingerprints(&backward));
    }

    #[test]
    fn not_run_maps_each_reason_to_its_state() {
        let cases = [
            (
                MutationNotRunArg::EventRefused,
                MutationCiState::NotRun(MutationNotRunReason::EventRefused),
            ),
            (
                MutationNotRunArg::ForkRefused,
                MutationCiState::NotRun(MutationNotRunReason::ForkRefused),
            ),
            (
                MutationNotRunArg::NoChangeScope,
                MutationCiState::NotRun(MutationNotRunReason::NoChangeScope),
            ),
            (MutationNotRunArg::Interrupted, MutationCiState::Interrupted),
        ];
        for (reason, state) in cases {
            let evidence = MutationEvidence::not_run(reason);
            assert_eq!(evidence.view.state, state);
            assert!(evidence.survivors.is_empty());
            let mut envelope = json!({ "kind": "dead-code" });
            evidence.merge_gate_outcomes(&mut envelope);
            assert_eq!(envelope, json!({ "kind": "dead-code" }));
        }
    }

    #[test]
    fn the_root_flag_is_the_root_that_the_caller_passed() {
        let evidence = MutationEvidence::not_run(MutationNotRunArg::EventRefused)
            .with_root_flag(Some(Path::new("packages/app")));
        assert_eq!(evidence.view.root_flag.as_deref(), Some("packages/app"));
        let evidence =
            MutationEvidence::not_run(MutationNotRunArg::EventRefused).with_root_flag(None);
        assert_eq!(evidence.view.root_flag, None);
    }

    #[test]
    fn merge_creates_the_gate_map_or_replaces_the_entry() {
        let dir = project();
        let evidence = parse(&report(&[]), dir.path()).unwrap();
        let entry =
            json!({ "status": "fail", "enforced": true, "observed": 62.5, "threshold": 80.0 });

        let mut without = json!({ "kind": "dead-code" });
        evidence.merge_gate_outcomes(&mut without);
        assert_eq!(
            without["gate_outcomes"],
            json!({ "mutation-min-score": entry })
        );

        let mut with = json!({ "kind": "health", "gate_outcomes": {
            "health-score": { "status": "pass", "enforced": true },
            "mutation-min-score": { "status": "pass", "enforced": false }
        } });
        evidence.merge_gate_outcomes(&mut with);
        assert_eq!(with["gate_outcomes"]["mutation-min-score"], entry);
        assert_eq!(
            with["gate_outcomes"]["health-score"],
            json!({ "status": "pass", "enforced": true })
        );
    }

    #[test]
    fn merge_takes_only_the_gate_that_mutation_testing_owns() {
        let dir = project();
        let mut value = report(&[]);
        value["gate_outcomes"]["dead-code"] = json!({ "status": "pass", "enforced": true });
        let evidence = parse(&value, dir.path()).unwrap();
        let mut envelope = json!({ "kind": "dead-code", "gate_outcomes": {
            "dead-code": { "status": "fail", "enforced": true }
        } });
        evidence.merge_gate_outcomes(&mut envelope);
        assert_eq!(
            envelope["gate_outcomes"]["dead-code"],
            json!({ "status": "fail", "enforced": true })
        );

        let mut no_gate = report(&[]);
        no_gate.as_object_mut().unwrap().remove("gate_outcomes");
        let evidence = parse(&no_gate, dir.path()).unwrap();
        assert_eq!(evidence.view.min_score_gate, None);
        let mut envelope = json!({ "kind": "dead-code" });
        evidence.merge_gate_outcomes(&mut envelope);
        assert_eq!(envelope, json!({ "kind": "dead-code" }));
    }

    #[test]
    fn the_reason_list_has_each_incomplete_reason() {
        const fn listed(reason: MutationIncompleteReason) -> MutationIncompleteReason {
            match reason {
                MutationIncompleteReason::RuntimeError
                | MutationIncompleteReason::MutantLimit
                | MutationIncompleteReason::SampledOut
                | MutationIncompleteReason::TimeBudget
                | MutationIncompleteReason::SkippedFiles
                | MutationIncompleteReason::UnloadedFiles
                | MutationIncompleteReason::TestFilesNotDiscovered
                | MutationIncompleteReason::BaselineFailure
                | MutationIncompleteReason::WorktreeChanged
                | MutationIncompleteReason::TypecheckFailed
                | MutationIncompleteReason::WorktreeUnchecked => reason,
            }
        }
        assert_eq!(INCOMPLETE_REASONS.len(), 11);
        for reason in INCOMPLETE_REASONS {
            assert_eq!(listed(reason), reason);
        }
    }

    #[test]
    fn the_survivor_rule_is_registered() {
        let rule = crate::explain::rule_by_id(MUTATION_SURVIVED_RULE).unwrap();
        assert_eq!(rule.category, "Tests");
    }
}
