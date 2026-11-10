//! `fallow mutation inspect`: one mutant of a saved report, against the
//! current file. It reads files and instruments one source text in memory. It
//! never executes code.
//!
//! A mutation report holds no source text. The code of a mutant comes from the current file: the file must
//! have the SHA-256 digest of the report, and the instrumentation of the
//! current text must give the same mutant id. Otherwise the packet is stale
//! and has no code, so a changed file never gives a wrong answer.

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use fallow_mutation::MutationOptions;
use fallow_output::{
    MutantChange, MutantLocation, MutantStatus, MutationInspectCode, MutationInspectOutput,
    MutationInspectSchemaVersion, MutationInspectStatus, MutationStaleReason,
};
use fallow_types::envelope::ToolVersion;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The largest saved report that inspect reads.
const MAX_REPORT_BYTES: u64 = 64 * 1024 * 1024;

/// The fields of a saved `fallow mutation` or `fallow mutation --plan`
/// document that inspect and the CI render read. Unknown fields are ignored,
/// so a report of a later minor version still reads.
#[derive(Debug, Deserialize)]
pub struct SavedReport {
    #[serde(default)]
    pub kind: Option<String>,
    pub mutants: Vec<SavedMutant>,
    #[serde(default)]
    pub files: Vec<SavedFile>,
    #[serde(default)]
    pub completion: Option<SavedCompletion>,
    /// The totals. A plan has no summary.
    #[serde(default)]
    pub summary: Option<SavedSummary>,
    #[serde(default)]
    pub scope: Option<SavedScope>,
    /// The gate verdicts, keyed by gate name. Absent when no minimum applies.
    #[serde(default)]
    pub gate_outcomes: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
    /// The result cache facts. Absent with `--no-cache`.
    #[serde(default)]
    pub cache: Option<SavedCache>,
}

#[derive(Debug, Deserialize)]
pub struct SavedCompletion {
    pub status: fallow_output::MutationCompletionStatus,
    /// The wire names of the reasons of a partial run. A name that this
    /// build does not know stays a string, so a later report still reads.
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SavedSummary {
    pub counts: fallow_output::MutationStatusCounts,
    /// The score as a fraction from 0 to 1. `null` when no mutant counted.
    #[serde(default)]
    pub score: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct SavedScope {
    /// The wire name of the scope kind, for example `changed`.
    pub kind: String,
}

#[derive(Debug, Deserialize)]
pub struct SavedCache {
    /// The mutants whose status came from the cache.
    pub hits: usize,
}

#[derive(Debug, Deserialize)]
pub struct SavedMutant {
    pub mutant_id: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub span: (u32, u32),
    pub operator: String,
    #[serde(default)]
    pub function: Option<String>,
    #[serde(default)]
    function_line: Option<u32>,
    #[serde(default)]
    rank: Option<u32>,
    #[serde(default)]
    pub change: Option<MutantChange>,
    #[serde(default)]
    pub status: Option<MutantStatus>,
    #[serde(default)]
    pub covering_tests: Vec<String>,
    #[serde(default)]
    pub killed_by: Option<String>,
    #[serde(default)]
    pub killing_tests: Vec<String>,
    #[serde(default)]
    kill_matrix_complete: bool,
    #[serde(default)]
    pub equivalence_hint: Option<fallow_output::MutantEquivalenceHint>,
    /// The witness reference of a `--record-args values` run.
    #[serde(default)]
    pub witness: Option<SavedWitness>,
    #[serde(default)]
    pub prune_reason: Option<String>,
    #[serde(default)]
    pub static_mutant: bool,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub type_error_codes: Vec<u32>,
}

/// The witness reference of a saved mutant. It holds no value.
#[derive(Debug, Clone, Deserialize)]
pub struct SavedWitness {
    pub verdict: fallow_output::MutationReplayVerdict,
    #[serde(default)]
    pub input_id: Option<String>,
    #[serde(default)]
    pub inputs_checked: u32,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SavedFile {
    pub path: String,
    pub source_sha256: String,
    #[serde(default)]
    pub static_test_files: Vec<String>,
}

/// An invalid-report error: exit code 2, `FALLOW_MUTATION_REPORT_INVALID`.
pub fn report_input_error(message: impl Into<String>) -> fallow_api::ProgrammaticError {
    fallow_api::ProgrammaticError::new(message, 2)
        .with_code("FALLOW_MUTATION_REPORT_INVALID")
        .with_context("mutation.report")
}

/// Reads and parses a saved report, bounded to 64 MiB.
///
/// # Errors
///
/// Returns an error when the file cannot be read, is too large, is not a
/// mutation report or plan, or does not parse.
pub fn read_saved_report(path: &Path) -> Result<SavedReport, fallow_api::ProgrammaticError> {
    parse_saved_report(&read_report_bytes(path)?)
}

/// Reads the bytes of a saved report, bounded to 64 MiB.
///
/// # Errors
///
/// Returns an error when the file cannot be read or is too large.
pub fn read_report_bytes(path: &Path) -> Result<Vec<u8>, fallow_api::ProgrammaticError> {
    let file = std::fs::File::open(path).map_err(|error| {
        report_input_error(format!("failed to read {}: {error}", path.display()))
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            report_input_error(format!("failed to read {}: {error}", path.display()))
        })?;
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err(report_input_error(
            "the mutation report is larger than 64 MiB",
        ));
    }
    Ok(bytes)
}

/// Parses the bytes of a saved report.
///
/// # Errors
///
/// Returns an error when the bytes are not a mutation report or plan.
pub fn parse_saved_report(bytes: &[u8]) -> Result<SavedReport, fallow_api::ProgrammaticError> {
    let report: SavedReport = serde_json::from_slice(bytes).map_err(|error| {
        report_input_error(format!(
            "the file is not a `fallow mutation --format json` report: {error}"
        ))
    })?;
    if let Some(kind) = report.kind.as_deref()
        && kind != "mutation"
        && kind != "mutation-plan"
    {
        return Err(report_input_error(format!(
            "the file is a `{kind}` document, not a mutation report or plan"
        )));
    }
    Ok(report)
}

/// The SHA-256 digest of `bytes`, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// The code of one mutant, reproduced from a source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reproduced {
    pub original: String,
    pub replacement: String,
}

/// Instruments `source` and finds the mutant `mutant_id` of `operator` at
/// `span`.
///
/// The instrumentation selects only `operator`, so an opt-in operator that
/// the default set leaves out is reproduced too. A mutant id hashes its
/// operator, so the other operators of the run cannot change it, and with one
/// operator no other candidate can take its place as a text duplicate. An
/// unknown name selects every operator. The mutant id can depend on the
/// other mutants of the file when two mutants have the same inputs, so a
/// second pass keeps only the mutants that overlap `span`.
pub fn reproduce(
    source: &str,
    file: &str,
    mutant_id: &str,
    span: (u32, u32),
    operator: &str,
) -> Option<Reproduced> {
    let operators = fallow_mutation::Operator::from_name(operator).map_or_else(
        || fallow_mutation::Operator::ALL.to_vec(),
        |operator| vec![operator],
    );
    let passes = [
        MutationOptions {
            operators: operators.clone(),
            ranges: None,
            record_exports: false,
        },
        MutationOptions {
            operators,
            ranges: Some(vec![span]),
            record_exports: false,
        },
    ];
    passes.iter().find_map(|options| {
        let instrumented =
            fallow_engine::mutation::instrument_source(source, file, options)?.ok()?;
        instrumented
            .mutants
            .into_iter()
            .find(|mutant| mutant.id.as_str() == mutant_id && mutant.span == span)
            .map(|mutant| Reproduced {
                original: mutant.original,
                replacement: mutant.replacement,
            })
    })
}

/// The text of the source lines around `span`: the part before the span on
/// its first line, and the part after the span on its last line.
pub fn line_context(source: &str, span: (u32, u32)) -> (&str, &str) {
    let (start, end) = (span.0 as usize, span.1 as usize);
    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[end..]
        .find('\n')
        .map_or(source.len(), |index| end + index);
    (
        &source[line_start..start],
        source[end..line_end].trim_end_matches('\r'),
    )
}

/// The whole source lines that contain `span`, before and after the
/// replacement.
pub fn mutated_lines(source: &str, span: (u32, u32), replacement: &str) -> (String, String) {
    let (before, after) = line_context(source, span);
    let original = &source[span.0 as usize..span.1 as usize];
    (
        format!("{before}{original}{after}"),
        format!("{before}{replacement}{after}"),
    )
}

impl SavedReport {
    /// The file entry of `path`, with its digest and static test files.
    pub fn saved_file(&self, path: &str) -> Option<&SavedFile> {
        self.files.iter().find(|file| file.path == path)
    }
}

/// The report path of a mutant as a relative path below the project root.
///
/// # Errors
///
/// Returns an invalid-report error for an absolute path or a path with `..`.
pub fn report_relative_path(file: &str) -> Result<&Path, fallow_api::ProgrammaticError> {
    let relative = Path::new(file);
    if relative.is_absolute()
        || relative.components().any(|component| {
            // `C:foo` and `\\foo` are not absolute on Windows, but they leave
            // the root through a drive prefix or the drive root.
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
            )
        })
    {
        return Err(report_input_error(format!(
            "the report path `{file}` is not inside the project root"
        )));
    }
    Ok(relative)
}

/// The source without its leading UTF-8 byte order mark. The run strips the
/// mark before it instruments a file, so the report spans count from the end
/// of the mark.
pub fn without_byte_order_mark(source: &str) -> &str {
    source.strip_prefix('\u{feff}').unwrap_or(source)
}

/// The code of `mutant` from the current file bytes. The file must have the
/// digest of the report, and its instrumentation must give the mutant.
pub fn mutant_code(
    current: Option<&[u8]>,
    current_sha256: Option<&str>,
    report_sha256: Option<&str>,
    mutant: &SavedMutant,
) -> Result<MutationInspectCode, MutationStaleReason> {
    let bytes = current.ok_or(MutationStaleReason::FileMissing)?;
    let expected = report_sha256.ok_or(MutationStaleReason::NoDigest)?;
    if current_sha256 != Some(expected) {
        return Err(MutationStaleReason::SourceChanged);
    }
    let source =
        std::str::from_utf8(bytes).map_err(|_| MutationStaleReason::MutantNotReproduced)?;
    let source = without_byte_order_mark(source);
    let reproduced = reproduce(
        source,
        &mutant.file,
        &mutant.mutant_id,
        mutant.span,
        &mutant.operator,
    )
    .ok_or(MutationStaleReason::MutantNotReproduced)?;
    let (original_lines, mutated_lines) =
        mutated_lines(source, mutant.span, &reproduced.replacement);
    Ok(MutationInspectCode {
        original: reproduced.original,
        replacement: reproduced.replacement,
        original_lines,
        mutated_lines,
    })
}

fn location(mutant: &SavedMutant) -> MutantLocation {
    MutantLocation {
        mutant_id: mutant.mutant_id.clone(),
        file: mutant.file.clone(),
        line: mutant.line,
        column: mutant.column,
        span: mutant.span,
        operator: mutant.operator.clone(),
        function: mutant.function.clone(),
        function_line: mutant.function_line,
        rank: mutant.rank,
        change: mutant.change.clone(),
    }
}

/// Builds the inspect packet of `mutant_id` against the files below `root`.
///
/// # Errors
///
/// Returns an error when the report has no mutant with this id, or when its
/// file path leaves the project root.
pub fn inspect(
    report: &SavedReport,
    mutant_id: &str,
    root: &Path,
) -> Result<MutationInspectOutput, fallow_api::ProgrammaticError> {
    let mutant = report
        .mutants
        .iter()
        .find(|mutant| mutant.mutant_id == mutant_id)
        .ok_or_else(|| {
            fallow_api::ProgrammaticError::new(format!("the report has no mutant `{mutant_id}`"), 2)
                .with_code("FALLOW_MUTATION_UNKNOWN_MUTANT")
                .with_context("mutation.mutantId")
                .with_help(
                    "without --report, inspect reads the report of the last run for this project, and each run replaces it; pass --report <file> to read another saved report",
                )
        })?;
    let relative = report_relative_path(&mutant.file)?;
    let saved_file = report.saved_file(&mutant.file);
    let report_sha256 = saved_file.map(|file| file.source_sha256.clone());
    let current = std::fs::read(root.join(relative)).ok();
    let current_sha256 = current.as_deref().map(sha256_hex);
    let code = || {
        mutant_code(
            current.as_deref(),
            current_sha256.as_deref(),
            report_sha256.as_deref(),
            mutant,
        )
    };
    let (status, stale_reason, code) = match code() {
        Ok(code) => (MutationInspectStatus::Fresh, None, Some(code)),
        Err(reason) => (MutationInspectStatus::Stale, Some(reason), None),
    };
    let mut covering_tests = mutant.covering_tests.clone();
    covering_tests.sort();
    let mut killing_tests = mutant.killing_tests.clone();
    killing_tests.sort();
    Ok(MutationInspectOutput {
        schema_version: MutationInspectSchemaVersion::V2,
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_owned()),
        status,
        stale_reason,
        location: location(mutant),
        report_status: mutant.status,
        report_sha256,
        current_sha256,
        code,
        covering_tests,
        static_test_files: saved_file
            .map(|file| file.static_test_files.clone())
            .unwrap_or_default(),
        killed_by: mutant.killed_by.clone(),
        killing_tests,
        kill_matrix_complete: mutant.kill_matrix_complete,
        replay: None,
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests {
    use super::*;
    use fallow_mutation::{SourceKind, instrument};

    const SOURCE: &str = "export function add(a: number, b: number) {\n  return a + b;\n}\n";

    fn arithmetic_mutant(source: &str) -> fallow_mutation::Mutant {
        let instrumented = instrument(
            source,
            "src/math.ts",
            SourceKind::Ts,
            &MutationOptions::default(),
        )
        .unwrap();
        instrumented
            .mutants
            .into_iter()
            .find(|mutant| mutant.original == "+" && mutant.replacement == "-")
            .unwrap()
    }

    fn report_json(id: &str, span: (u32, u32), sha: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "kind": "mutation",
            "schema_version": "1",
            "mutants": [{
                "mutant_id": id, "file": "src/math.ts", "line": 2, "column": 10,
                "span": [span.0, span.1], "operator": "arithmetic-operator",
                "function": "add", "function_line": 1, "rank": 1, "status": "survived",
                "covering_tests": ["test/math.test.ts::b", "test/math.test.ts::a"]
            }],
            "files": [{ "path": "src/math.ts", "source_sha256": sha,
                        "static_test_files": ["test/math.test.ts"] }]
        }))
        .unwrap()
    }

    fn project(source: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/math.ts"), source).unwrap();
        dir
    }

    #[test]
    fn a_fresh_file_gives_the_original_and_the_mutated_line() {
        let mutant = arithmetic_mutant(SOURCE);
        let dir = project(SOURCE);
        let report = parse_saved_report(&report_json(
            mutant.id.as_str(),
            mutant.span,
            &sha256_hex(SOURCE.as_bytes()),
        ))
        .unwrap();

        let packet = inspect(&report, mutant.id.as_str(), dir.path()).unwrap();

        assert_eq!(packet.status, MutationInspectStatus::Fresh);
        assert_eq!(packet.stale_reason, None);
        let code = packet.code.unwrap();
        assert_eq!(code.original, "+");
        assert_eq!(code.replacement, "-");
        assert_eq!(code.original_lines, "  return a + b;");
        assert_eq!(code.mutated_lines, "  return a - b;");
        assert_eq!(packet.report_status, Some(MutantStatus::Survived));
        assert_eq!(
            packet.covering_tests,
            vec![
                "test/math.test.ts::a".to_owned(),
                "test/math.test.ts::b".to_owned()
            ]
        );
        assert_eq!(
            packet.static_test_files,
            vec!["test/math.test.ts".to_owned()]
        );
    }

    #[test]
    fn saved_inspect_preserves_all_killers_and_defaults_legacy_evidence() {
        let mutant = arithmetic_mutant(SOURCE);
        let dir = project(SOURCE);
        let bytes = report_json(
            mutant.id.as_str(),
            mutant.span,
            &sha256_hex(SOURCE.as_bytes()),
        );
        let legacy = parse_saved_report(&bytes).unwrap();
        let old = inspect(&legacy, mutant.id.as_str(), dir.path()).unwrap();
        assert_eq!(old.status, MutationInspectStatus::Fresh);
        assert_eq!(old.killing_tests, Vec::<String>::new());
        assert!(!old.kill_matrix_complete);

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["schema_version"] = serde_json::json!("2");
        value["mutants"][0]["status"] = serde_json::json!("killed");
        value["mutants"][0]["killed_by"] = serde_json::json!("test/math.test.ts::b");
        value["mutants"][0]["killing_tests"] =
            serde_json::json!(["test/math.test.ts::b", "test/math.test.ts::a"]);
        value["mutants"][0]["kill_matrix_complete"] = serde_json::json!(true);
        let report = parse_saved_report(&serde_json::to_vec(&value).unwrap()).unwrap();
        let mut packet = inspect(&report, mutant.id.as_str(), dir.path()).unwrap();
        assert_eq!(
            packet.killing_tests,
            ["test/math.test.ts::a", "test/math.test.ts::b"]
        );
        assert!(packet.kill_matrix_complete);
        assert_eq!(packet.killed_by.as_deref(), Some("test/math.test.ts::b"));
        let text = crate::mutation_human::render_inspect(&packet);
        assert!(text.contains("Killing tests (complete matrix)"), "{text}");
        assert!(
            text.contains("  test/math.test.ts::a\n  test/math.test.ts::b"),
            "{text}"
        );
        packet.kill_matrix_complete = false;
        let partial = crate::mutation_human::render_inspect(&packet);
        assert!(
            partial.contains("Observed killing tests (incomplete matrix)"),
            "{partial}"
        );
        packet.kill_matrix_complete = true;
        packet.killing_tests.clear();
        packet.killed_by = None;
        packet.report_status = Some(MutantStatus::Survived);
        let survived = crate::mutation_human::render_inspect(&packet);
        assert!(
            survived.contains("No test killed this mutant."),
            "{survived}"
        );
    }

    #[test]
    fn a_changed_or_missing_file_is_stale_and_shows_no_code() {
        let mutant = arithmetic_mutant(SOURCE);
        let report = parse_saved_report(&report_json(
            mutant.id.as_str(),
            mutant.span,
            &sha256_hex(SOURCE.as_bytes()),
        ))
        .unwrap();

        let changed = project("export function add(a: number, b: number) {\n  return a * b;\n}\n");
        let packet = inspect(&report, mutant.id.as_str(), changed.path()).unwrap();
        assert_eq!(packet.status, MutationInspectStatus::Stale);
        assert_eq!(
            packet.stale_reason,
            Some(MutationStaleReason::SourceChanged)
        );
        assert!(packet.code.is_none());
        assert_ne!(packet.current_sha256, packet.report_sha256);

        let empty = tempfile::tempdir().unwrap();
        let packet = inspect(&report, mutant.id.as_str(), empty.path()).unwrap();
        assert_eq!(packet.stale_reason, Some(MutationStaleReason::FileMissing));
        assert_eq!(packet.current_sha256, None);
    }

    #[test]
    fn an_unknown_mutant_id_with_the_same_digest_is_not_reproduced() {
        let mutant = arithmetic_mutant(SOURCE);
        let dir = project(SOURCE);
        let report = parse_saved_report(&report_json(
            "0000000000000000",
            mutant.span,
            &sha256_hex(SOURCE.as_bytes()),
        ))
        .unwrap();
        let packet = inspect(&report, "0000000000000000", dir.path()).unwrap();
        assert_eq!(
            packet.stale_reason,
            Some(MutationStaleReason::MutantNotReproduced)
        );
    }

    #[test]
    fn a_mutant_in_a_vue_script_block_is_reproduced() {
        let source = "<template><p>{{ total }}</p></template>\n<script setup lang=\"ts\">\nconst total = 1 + 2\n</script>\n";
        let mutant = fallow_engine::mutation::instrument_source(
            source,
            "src/Total.vue",
            &MutationOptions::default(),
        )
        .unwrap()
        .unwrap()
        .mutants
        .into_iter()
        .find(|mutant| mutant.original == "+")
        .unwrap();
        let reproduced = reproduce(
            source,
            "src/Total.vue",
            mutant.id.as_str(),
            mutant.span,
            "arithmetic-operator",
        )
        .unwrap();
        assert_eq!(reproduced.replacement, "-");
        assert_eq!(mutant.line, 3);
    }

    #[test]
    fn an_opt_in_mutant_is_reproduced() {
        let source = "export function save(x: number) {\n  write(x);\n}\n";
        let options = MutationOptions {
            operators: vec![fallow_mutation::Operator::BlockStatement],
            ..MutationOptions::default()
        };
        let mutant = fallow_engine::mutation::instrument_source(source, "src/save.ts", &options)
            .unwrap()
            .unwrap()
            .mutants
            .into_iter()
            .next()
            .unwrap();
        let reproduced = reproduce(
            source,
            "src/save.ts",
            mutant.id.as_str(),
            mutant.span,
            "block-statement",
        )
        .unwrap();
        assert_eq!(reproduced.replacement, "");
        assert_eq!(reproduced.original, "\n  write(x);\n");
    }

    #[test]
    fn inspect_rejects_other_documents_unknown_ids_and_escaping_paths() {
        let error = parse_saved_report(br#"{"kind":"health","mutants":[]}"#).unwrap_err();
        assert!(
            error.message.contains("`health` document"),
            "{}",
            error.message
        );

        let report = parse_saved_report(&report_json("m1", (0, 1), "ab")).unwrap();
        let error = inspect(&report, "m2", Path::new("/")).unwrap_err();
        assert_eq!(
            error.code.as_deref(),
            Some("FALLOW_MUTATION_UNKNOWN_MUTANT")
        );

        let escaping = parse_saved_report(
            br#"{"mutants":[{"mutant_id":"m1","file":"../etc/passwd","line":1,"column":1,"span":[0,1],"operator":"x"}]}"#,
        )
        .unwrap();
        let error = inspect(&escaping, "m1", Path::new("/tmp")).unwrap_err();
        assert!(
            error.message.contains("not inside the project root"),
            "{}",
            error.message
        );
    }
}
