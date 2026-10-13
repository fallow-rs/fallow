//! `fallow mutation inspect`: one mutant of a saved report, against the
//! current file. It reads files and instruments one source text in memory. It
//! never executes code.
//!
//! A mutation report holds no source text (decision 5 of the mutation
//! engine). The code of a mutant comes from the current file: the file must
//! have the SHA-256 digest of the report, and the instrumentation of the
//! current text must give the same mutant id. Otherwise the packet is stale
//! and has no code, so a changed file never gives a wrong answer.

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use fallow_mutation::{MutationOptions, SourceKind, instrument};
use fallow_output::{
    MutantLocation, MutantStatus, MutationInspectCode, MutationInspectOutput,
    MutationInspectSchemaVersion, MutationInspectStatus, MutationStaleReason,
};
use fallow_types::envelope::ToolVersion;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The largest saved report that inspect reads.
const MAX_REPORT_BYTES: u64 = 64 * 1024 * 1024;

/// The fields of a saved `fallow mutation` or `fallow mutation --plan`
/// document that inspect reads. Unknown fields are ignored, so a report of a
/// later minor version still reads.
#[derive(Debug, Deserialize)]
pub struct SavedReport {
    #[serde(default)]
    kind: Option<String>,
    mutants: Vec<SavedMutant>,
    #[serde(default)]
    files: Vec<SavedFile>,
}

#[derive(Debug, Deserialize)]
struct SavedMutant {
    mutant_id: String,
    file: String,
    line: u32,
    column: u32,
    span: (u32, u32),
    operator: String,
    #[serde(default)]
    function: Option<String>,
    #[serde(default)]
    function_line: Option<u32>,
    #[serde(default)]
    rank: Option<u32>,
    #[serde(default)]
    status: Option<MutantStatus>,
    #[serde(default)]
    covering_tests: Vec<String>,
    #[serde(default)]
    killed_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SavedFile {
    path: String,
    source_sha256: String,
    #[serde(default)]
    static_test_files: Vec<String>,
}

fn input_error(message: impl Into<String>) -> fallow_api::ProgrammaticError {
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
    let file = std::fs::File::open(path)
        .map_err(|error| input_error(format!("failed to read {}: {error}", path.display())))?;
    let mut bytes = Vec::new();
    file.take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| input_error(format!("failed to read {}: {error}", path.display())))?;
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err(input_error("the mutation report is larger than 64 MiB"));
    }
    parse_saved_report(&bytes)
}

fn parse_saved_report(bytes: &[u8]) -> Result<SavedReport, fallow_api::ProgrammaticError> {
    let report: SavedReport = serde_json::from_slice(bytes).map_err(|error| {
        input_error(format!(
            "the file is not a `fallow mutation --format json` report: {error}"
        ))
    })?;
    if let Some(kind) = report.kind.as_deref()
        && kind != "mutation"
        && kind != "mutation-plan"
    {
        return Err(input_error(format!(
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

/// Instruments `source` and finds the mutant `mutant_id` at `span`.
///
/// The mutant id can depend on the other mutants of the file when two
/// mutants have the same inputs, so a second pass keeps only the mutants that
/// overlap `span`.
pub fn reproduce(
    source: &str,
    file: &str,
    mutant_id: &str,
    span: (u32, u32),
) -> Option<Reproduced> {
    let kind = SourceKind::from_path(file)?;
    let passes = [
        MutationOptions::default(),
        MutationOptions {
            ranges: Some(vec![span]),
            ..MutationOptions::default()
        },
    ];
    passes.iter().find_map(|options| {
        let instrumented = instrument(source, file, kind, options).ok()?;
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
    let relative = Path::new(&mutant.file);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(input_error(format!(
            "the report path `{}` is not inside the project root",
            mutant.file
        )));
    }
    let saved_file = report.files.iter().find(|file| file.path == mutant.file);
    let report_sha256 = saved_file.map(|file| file.source_sha256.clone());
    let current = std::fs::read(root.join(relative)).ok();
    let current_sha256 = current.as_deref().map(sha256_hex);
    let code = || -> Result<MutationInspectCode, MutationStaleReason> {
        let bytes = current.as_deref().ok_or(MutationStaleReason::FileMissing)?;
        let expected = report_sha256
            .as_deref()
            .ok_or(MutationStaleReason::NoDigest)?;
        if current_sha256.as_deref() != Some(expected) {
            return Err(MutationStaleReason::SourceChanged);
        }
        let source =
            std::str::from_utf8(bytes).map_err(|_| MutationStaleReason::MutantNotReproduced)?;
        let reproduced = reproduce(source, &mutant.file, mutant_id, mutant.span)
            .ok_or(MutationStaleReason::MutantNotReproduced)?;
        let (original_lines, mutated_lines) =
            mutated_lines(source, mutant.span, &reproduced.replacement);
        Ok(MutationInspectCode {
            original: reproduced.original,
            replacement: reproduced.replacement,
            original_lines,
            mutated_lines,
        })
    };
    let (status, stale_reason, code) = match code() {
        Ok(code) => (MutationInspectStatus::Fresh, None, Some(code)),
        Err(reason) => (MutationInspectStatus::Stale, Some(reason), None),
    };
    let mut covering_tests = mutant.covering_tests.clone();
    covering_tests.sort();
    Ok(MutationInspectOutput {
        schema_version: MutationInspectSchemaVersion::V1,
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
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests {
    use super::*;

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
