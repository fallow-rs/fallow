//! `fallow mutation export`: a saved report in the open
//! `mutation-testing-report-schema` format, version 2. Mutation dashboards and
//! HTML report viewers read this format. It reads files and instruments
//! source text in memory. It never executes code.
//!
//! The format needs the full source of each mutated file. The source comes
//! from the current file, which must have the SHA-256 digest of the report,
//! so the locations always match the source.

use std::collections::BTreeMap;
use std::path::Path;

use fallow_mutation::MutationOptions;
use fallow_output::{MutantStatus, MutationCompletionStatus};
use rustc_hash::FxHashMap;
use serde_json::{Map, Value, json};

use super::tasks::test_file_of;
use crate::mutation_inspect::{
    SavedMutant, SavedReport, report_input_error, report_relative_path, reproduce, sha256_hex,
    without_byte_order_mark,
};

/// The major version of the schema that the export writes.
const SCHEMA_VERSION: &str = "2";
/// The score thresholds of the report colors, in percent.
const THRESHOLD_HIGH: u8 = 80;
const THRESHOLD_LOW: u8 = 60;
/// The `testFiles` key of the tests whose id names no test file.
const UNKNOWN_TEST_FILE: &str = "unknown";

/// Builds the export document of `report` against the files below `root`.
///
/// # Errors
///
/// Returns an invalid-report error for a plan document, a mutant path that
/// leaves the project root, and a mutated file that is missing or changed
/// after the run.
pub fn export(report: &SavedReport, root: &Path) -> Result<Value, fallow_api::ProgrammaticError> {
    if report.kind.as_deref() == Some("mutation-plan")
        || report.mutants.iter().any(|mutant| mutant.status.is_none())
    {
        return Err(report_input_error(
            "the file is a mutation plan, which has no test results; pass a saved `fallow mutation --format json` report",
        ));
    }
    let partial = partial_note(report);
    let static_test_files: Vec<String> = report
        .files
        .iter()
        .flat_map(|file| file.static_test_files.iter().cloned())
        .collect();

    let mut by_file: BTreeMap<&str, Vec<&SavedMutant>> = BTreeMap::new();
    for mutant in &report.mutants {
        report_relative_path(&mutant.file)?;
        by_file
            .entry(mutant.file.as_str())
            .or_default()
            .push(mutant);
    }

    let mut files = Map::new();
    let mut stale = Vec::new();
    let mut test_ids: Vec<&str> = Vec::new();
    for (path, mut mutants) in by_file {
        let source = match current_source(report, root, path) {
            Ok(source) => source,
            Err(cause) => {
                stale.push(format!("{path} ({cause})"));
                continue;
            }
        };
        let replacements = replacements(&source, path);
        let lines = LineStarts::new(&source);
        mutants.sort_by_key(|mutant| (mutant.span, mutant.mutant_id.as_str()));
        let results: Vec<Value> = mutants
            .iter()
            .map(|mutant| mutant_result(&source, &lines, mutant, &replacements, partial.as_deref()))
            .collect();
        for mutant in &mutants {
            test_ids.extend(mutant.covering_tests.iter().map(String::as_str));
            test_ids.extend(mutant.killing_tests.iter().map(String::as_str));
            test_ids.extend(mutant.killed_by.as_deref());
        }
        files.insert(
            path.to_owned(),
            json!({
                "language": language(path),
                "source": source,
                "mutants": results,
            }),
        );
    }
    if !stale.is_empty() {
        return Err(report_input_error(format!(
            "the export needs the source of each mutated file as the run saw it: {}; run `fallow mutation` again, or restore the files",
            stale.join(", ")
        )));
    }

    let mut document = json!({
        "schemaVersion": SCHEMA_VERSION,
        "thresholds": { "high": THRESHOLD_HIGH, "low": THRESHOLD_LOW },
        "files": files,
        "testFiles": test_files(&test_ids, &static_test_files),
        "framework": { "name": "fallow", "version": env!("CARGO_PKG_VERSION") },
    });
    if let Some(completion) = &report.completion {
        document["config"] = json!({
            "completion": {
                "status": completion.status,
                "reasons": completion.reasons,
            }
        });
    }
    Ok(document)
}

/// The note on a survivor or a `no_coverage` mutant of a partial run: such a
/// result is not proof of a test gap.
fn partial_note(report: &SavedReport) -> Option<String> {
    let completion = report.completion.as_ref()?;
    if completion.status != MutationCompletionStatus::Partial {
        return None;
    }
    let reasons = if completion.reasons.is_empty() {
        String::new()
    } else {
        format!(" ({})", completion.reasons.join(", "))
    };
    Some(format!(
        "The run is partial{reasons}, so this result is not conclusive."
    ))
}

/// The current text of `path` without its byte order mark, when the file
/// has the digest of the report. The error is the cause, for the message.
fn current_source(report: &SavedReport, root: &Path, path: &str) -> Result<String, &'static str> {
    let expected = report
        .saved_file(path)
        .ok_or("the report has no digest of the file")?
        .source_sha256
        .as_str();
    let relative = report_relative_path(path).map_err(|_| "the path leaves the project root")?;
    let bytes = std::fs::read(root.join(relative)).map_err(|_| "the file cannot be read")?;
    if sha256_hex(&bytes) != expected {
        return Err("the file changed after the run");
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| "the file is not UTF-8 text")?;
    Ok(without_byte_order_mark(source).to_owned())
}

/// The replacement text of each mutant of one instrumentation of `source`,
/// by mutant id and span.
fn replacements(source: &str, path: &str) -> FxHashMap<(String, (u32, u32)), String> {
    let Some(Ok(instrumented)) =
        fallow_engine::mutation::instrument_source(source, path, &MutationOptions::default())
    else {
        return FxHashMap::default();
    };
    instrumented
        .mutants
        .into_iter()
        .map(|mutant| {
            (
                (mutant.id.as_str().to_owned(), mutant.span),
                mutant.replacement,
            )
        })
        .collect()
}

fn mutant_result(
    source: &str,
    lines: &LineStarts,
    mutant: &SavedMutant,
    replacements: &FxHashMap<(String, (u32, u32)), String>,
    partial: Option<&str>,
) -> Value {
    let status = mutant.status.unwrap_or(MutantStatus::Pending);
    let (schema_status, reason) = schema_status(mutant, status);
    let reason = match (status, partial) {
        (MutantStatus::Survived | MutantStatus::NoCoverage, Some(note)) => Some(match reason {
            Some(reason) => format!("{reason} {note}"),
            None => note.to_owned(),
        }),
        _ => reason,
    };
    let mut result = Map::new();
    result.insert("id".into(), json!(mutant.mutant_id));
    result.insert("mutatorName".into(), json!(mutant.operator));
    result.insert("location".into(), lines.location(source, mutant.span));
    result.insert("status".into(), json!(schema_status));
    if let Some(reason) = reason {
        result.insert("statusReason".into(), json!(reason));
    }
    let replacement = replacements
        .get(&(mutant.mutant_id.clone(), mutant.span))
        .cloned()
        .or_else(|| {
            reproduce(
                source,
                &mutant.file,
                &mutant.mutant_id,
                mutant.span,
                &mutant.operator,
            )
            .map(|reproduced| reproduced.replacement)
        });
    if let Some(replacement) = replacement {
        result.insert("replacement".into(), json!(replacement));
    }
    if !mutant.covering_tests.is_empty() {
        result.insert("coveredBy".into(), json!(mutant.covering_tests));
    }
    let killed_by = if mutant.killing_tests.is_empty() {
        mutant.killed_by.iter().cloned().collect()
    } else {
        mutant.killing_tests.clone()
    };
    if status == MutantStatus::Killed && !killed_by.is_empty() {
        result.insert("killedBy".into(), json!(killed_by));
    }
    if mutant.static_mutant {
        result.insert("static".into(), json!(true));
    }
    if let Some(duration) = mutant.duration_ms {
        result.insert("duration".into(), json!(duration));
    }
    Value::Object(result)
}

/// The schema status of a Fallow status, with the reason for each status that
/// the schema has no exact name for. `flaky`, `pruned` and `equivalent` are
/// `Ignored`, so they never count as a kill or a survivor.
fn schema_status(mutant: &SavedMutant, status: MutantStatus) -> (&'static str, Option<String>) {
    match status {
        MutantStatus::Killed => ("Killed", None),
        MutantStatus::Timeout => ("Timeout", None),
        MutantStatus::Survived => ("Survived", None),
        MutantStatus::NoCoverage => ("NoCoverage", None),
        MutantStatus::RuntimeError => (
            "RuntimeError",
            Some("The test runner failed for a reason outside the tests.".to_owned()),
        ),
        MutantStatus::CompileError => {
            let codes = if mutant.type_error_codes.is_empty() {
                String::new()
            } else {
                let codes: Vec<String> = mutant
                    .type_error_codes
                    .iter()
                    .map(|code| format!("TS{code}"))
                    .collect();
                format!(" ({})", codes.join(", "))
            };
            (
                "CompileError",
                Some(format!(
                    "Each covering test passed, but the mutated file does not type-check{codes}."
                )),
            )
        }
        MutantStatus::Flaky => (
            "Ignored",
            Some("Flaky: a second run changed whether the tests detect the mutant.".to_owned()),
        ),
        MutantStatus::Pruned => (
            "Ignored",
            Some(match &mutant.prune_reason {
                Some(rule) => format!("Pruned: the static rule `{rule}` removed the mutant."),
                None => "Pruned: a static rule removed the mutant.".to_owned(),
            }),
        ),
        MutantStatus::Equivalent => (
            "Ignored",
            Some(
                "Equivalent: a verdict marks the mutant as equivalent to the original code."
                    .to_owned(),
            ),
        ),
        MutantStatus::Pending => (
            "Pending",
            Some(
                "The mutant did not run: the mutant limit or the time budget ended first."
                    .to_owned(),
            ),
        ),
    }
}

/// The start offset of each line, with the line breaks of a browser viewer:
/// `\r\n`, `\r`, `\n`, U+2028 and U+2029.
struct LineStarts(Vec<usize>);

impl LineStarts {
    fn new(source: &str) -> Self {
        let mut starts = vec![0];
        let mut chars = source.char_indices().peekable();
        while let Some((index, character)) = chars.next() {
            let end = match character {
                '\r' if chars.peek().is_some_and(|(_, next)| *next == '\n') => {
                    chars.next();
                    index + 2
                }
                '\r' | '\n' | '\u{2028}' | '\u{2029}' => index + character.len_utf8(),
                _ => continue,
            };
            starts.push(end);
        }
        Self(starts)
    }

    /// The 1-based line and column of each end of `span`. A column counts
    /// UTF-16 code units, the unit of the source string in a browser.
    fn location(&self, source: &str, span: (u32, u32)) -> Value {
        json!({
            "start": self.position(source, span.0 as usize),
            "end": self.position(source, span.1 as usize),
        })
    }

    fn position(&self, source: &str, offset: usize) -> Value {
        let mut offset = offset.min(source.len());
        while !source.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.0.partition_point(|&start| start <= offset);
        let line_start = self.0[line - 1];
        let column = source[line_start..offset].encode_utf16().count() + 1;
        json!({ "line": line, "column": column })
    }
}

fn language(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("ts" | "tsx" | "mts" | "cts") => "typescript",
        Some("vue" | "svelte" | "astro" | "html") => "html",
        _ => "javascript",
    }
}

/// The tests of the report by test file. The name of a test is its id
/// without the file part.
fn test_files(test_ids: &[&str], static_test_files: &[String]) -> Value {
    let mut by_file: BTreeMap<String, BTreeMap<&str, String>> = BTreeMap::new();
    for id in test_ids {
        let file = test_file_of(id, static_test_files);
        let name = file
            .as_deref()
            .and_then(|file| id.split_once(&format!("{file}::")).map(|(_, name)| name))
            .unwrap_or(id)
            .to_owned();
        by_file
            .entry(file.unwrap_or_else(|| UNKNOWN_TEST_FILE.to_owned()))
            .or_default()
            .insert(id, name);
    }
    let files: Map<String, Value> = by_file
        .into_iter()
        .map(|(file, tests)| {
            let tests: Vec<Value> = tests
                .into_iter()
                .map(|(id, name)| json!({ "id": id, "name": name }))
                .collect();
            (file, json!({ "tests": tests }))
        })
        .collect();
    Value::Object(files)
}

#[cfg(test)]
#[path = "mutation_export_tests.rs"]
mod tests;
