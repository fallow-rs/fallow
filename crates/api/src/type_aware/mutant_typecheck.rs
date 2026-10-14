//! The `mutant-typecheck` request of `fallow mutation --typecheck`.
//!
//! The companion applies each mutant to an in-memory copy of its file and
//! compares the error diagnostics of the file with the original file. It
//! never writes a project file.

use std::path::Path;

use fallow_output::MutationTypecheckSkipReason;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

use super::manifest::{MUTANT_TYPECHECK_OPERATION, WIRE_PROTOCOL_VERSION};
use super::transport::{TypeAwareError, run_semantic_request};

/// One mutant to type-check: the byte span in the original file and the
/// replacement text.
#[derive(Debug, Clone, Serialize)]
pub struct TypecheckMutant {
    /// The mutant id.
    pub id: String,
    /// The byte offset of the span start.
    pub start: u32,
    /// The byte offset of the span end.
    pub end: u32,
    /// The text that replaces the span.
    pub replacement: String,
}

/// The mutants of one file.
#[derive(Debug, Clone, Serialize)]
pub struct TypecheckFile {
    /// The root-relative path, with forward slashes.
    pub path: String,
    /// The SHA-256 digest of the file at plan time. The companion skips the
    /// file when the digest no longer matches.
    pub source_sha256: String,
    /// The mutants of the file.
    pub mutants: Vec<TypecheckMutant>,
}

/// The result of one mutant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypecheckOutcome {
    /// The mutated file has no new error.
    Compiles,
    /// The mutated file has new errors, with these diagnostic codes.
    CompileError(Vec<u32>),
    /// The checker did not check the file.
    Unchecked(MutationTypecheckSkipReason),
}

#[derive(Serialize)]
struct Request<'a> {
    protocol_version: u32,
    operation: &'static str,
    root: String,
    files: &'a [TypecheckFile],
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WireOutcome {
    Compiles,
    CompileError,
    Unchecked,
}

#[derive(Deserialize)]
struct WireResult {
    id: String,
    outcome: WireOutcome,
    #[serde(default)]
    codes: Vec<u32>,
    #[serde(default)]
    reason: Option<MutationTypecheckSkipReason>,
}

#[derive(Deserialize)]
struct Response {
    protocol_version: u32,
    operation: String,
    results: Vec<WireResult>,
}

/// The diagnostic codes that the report keeps for one mutant.
const MAX_CODES: usize = 8;

/// Finds the companion, without starting it.
///
/// # Errors
///
/// Returns the remediation text when no trusted companion exists.
pub fn check_typecheck_companion(root: &Path) -> Result<(), String> {
    super::discover_companion(root)
}

/// Type-checks `files` through the companion. The result has one entry for
/// each requested mutant, in request order.
///
/// # Errors
///
/// Returns an error when the companion fails, or when its response does not
/// match the request.
pub fn typecheck_mutants(
    root: &Path,
    files: &[TypecheckFile],
) -> Result<Vec<(String, TypecheckOutcome)>, TypeAwareError> {
    if files.iter().all(|file| file.mutants.is_empty()) {
        return Ok(Vec::new());
    }
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let request = Request {
        protocol_version: WIRE_PROTOCOL_VERSION,
        operation: MUTANT_TYPECHECK_OPERATION,
        root: root.to_string_lossy().into_owned(),
        files,
    };
    let response: Response = run_semantic_request(&root, &request)?;
    validate(files, response)
}

fn validate(
    files: &[TypecheckFile],
    response: Response,
) -> Result<Vec<(String, TypecheckOutcome)>, TypeAwareError> {
    if response.protocol_version != WIRE_PROTOCOL_VERSION
        || response.operation != MUTANT_TYPECHECK_OPERATION
    {
        return Err(TypeAwareError::from(format!(
            "the type-aware companion answered protocol {} operation `{}`, expected protocol {WIRE_PROTOCOL_VERSION} operation `{MUTANT_TYPECHECK_OPERATION}`",
            response.protocol_version, response.operation
        )));
    }
    let expected: Vec<&str> = files
        .iter()
        .flat_map(|file| file.mutants.iter().map(|mutant| mutant.id.as_str()))
        .collect();
    let mut seen = FxHashSet::default();
    let mut results = Vec::with_capacity(response.results.len());
    for result in response.results {
        if !seen.insert(result.id.clone()) {
            return Err(mismatch(&format!("a duplicate mutant `{}`", result.id)));
        }
        let outcome = match (result.outcome, result.reason) {
            (WireOutcome::Compiles, None) if result.codes.is_empty() => TypecheckOutcome::Compiles,
            (WireOutcome::CompileError, None) if !result.codes.is_empty() => {
                let mut codes = result.codes;
                codes.sort_unstable();
                codes.dedup();
                codes.truncate(MAX_CODES);
                TypecheckOutcome::CompileError(codes)
            }
            (WireOutcome::Unchecked, Some(reason)) if result.codes.is_empty() => {
                TypecheckOutcome::Unchecked(reason)
            }
            _ => return Err(mismatch(&format!("an invalid result for `{}`", result.id))),
        };
        results.push((result.id, outcome));
    }
    let answered: Vec<&str> = results.iter().map(|(id, _)| id.as_str()).collect();
    if answered != expected {
        return Err(mismatch("results that do not match the requested mutants"));
    }
    Ok(results)
}

fn mismatch(detail: &str) -> TypeAwareError {
    TypeAwareError::from(format!(
        "the type-aware companion returned {detail} for the mutant type check"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<TypecheckFile> {
        vec![TypecheckFile {
            path: "src/a.ts".to_owned(),
            source_sha256: "0".repeat(64),
            mutants: ["m1", "m2", "m3"]
                .into_iter()
                .map(|id| TypecheckMutant {
                    id: id.to_owned(),
                    start: 0,
                    end: 1,
                    replacement: "x".to_owned(),
                })
                .collect(),
        }]
    }

    fn response(results: &serde_json::Value) -> Response {
        serde_json::from_value(serde_json::json!({
            "protocol_version": WIRE_PROTOCOL_VERSION,
            "operation": MUTANT_TYPECHECK_OPERATION,
            "results": results.clone(),
        }))
        .expect("a response")
    }

    #[test]
    fn a_valid_response_maps_each_outcome_in_request_order() {
        let results = validate(
            &files(),
            response(&serde_json::json!([
                { "id": "m1", "outcome": "compiles" },
                { "id": "m2", "outcome": "compile-error", "codes": [2345, 2322, 2322] },
                { "id": "m3", "outcome": "unchecked", "reason": "no-project" },
            ])),
        )
        .expect("a valid response");
        assert_eq!(
            results,
            vec![
                ("m1".to_owned(), TypecheckOutcome::Compiles),
                (
                    "m2".to_owned(),
                    TypecheckOutcome::CompileError(vec![2322, 2345])
                ),
                (
                    "m3".to_owned(),
                    TypecheckOutcome::Unchecked(MutationTypecheckSkipReason::NoProject)
                ),
            ]
        );
    }

    #[test]
    fn a_response_that_does_not_match_the_request_is_an_error() {
        let missing = response(&serde_json::json!([{ "id": "m1", "outcome": "compiles" }]));
        assert!(validate(&files(), missing).is_err());
        let unknown_reason = serde_json::from_value::<Response>(serde_json::json!({
            "protocol_version": WIRE_PROTOCOL_VERSION,
            "operation": MUTANT_TYPECHECK_OPERATION,
            "results": [{ "id": "m1", "outcome": "unchecked", "reason": "other" }],
        }));
        assert!(unknown_reason.is_err());
        let no_codes = response(&serde_json::json!([
            { "id": "m1", "outcome": "compile-error" },
            { "id": "m2", "outcome": "compiles" },
            { "id": "m3", "outcome": "compiles" },
        ]));
        assert!(validate(&files(), no_codes).is_err());
        let duplicate = response(&serde_json::json!([
            { "id": "m1", "outcome": "compiles" },
            { "id": "m1", "outcome": "compiles" },
            { "id": "m3", "outcome": "compiles" },
        ]));
        assert!(validate(&files(), duplicate).is_err());
    }
}
