//! The `call-evidence` request of the `constant-parameter` rule.
//!
//! The static rule selects the candidates and hands them over in
//! [`ConstantParameterCandidates`]. The companion answers two questions and
//! returns evidence only:
//!
//! - For a parameter with a default: can an argument of a call be
//!   `undefined`?
//! - For a method of an exported class: which calls does the checker see, and
//!   can each argument be `undefined`? Or why can a call be invisible?
//!
//! The verdicts are computed here, with the rules of the static rule.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fallow_types::envelope::TypeAwareCallEvidenceMeta;
use fallow_types::results::{
    AnalysisResults, ConstantParameter, ConstantParameterCallSite, ConstantParameterCandidates,
    ConstantParameterEvidence, ConstantParameterFinding, ConstantParameterReason,
    MAX_INSPECTED_CALL_SITES, MethodClassCandidate, UnusedDefaultCandidate,
    constant_parameter_explanation, sort_constant_parameters,
};
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

use super::super::manifest::{CALL_EVIDENCE_OPERATION, WIRE_PROTOCOL_VERSION, selected_backend};
use super::super::transport::{TypeAwareError, run_semantic_request};
use super::{SemanticDeadCodeOutcome, empty_semantic_outcome};

/// The fewest counted calls for a finding, the same value as the static rule.
const MIN_CALL_SITES: usize = 2;
/// The most project files that a request names. A larger project skips the
/// method candidates, because the companion must read each file.
const MAX_PROJECT_FILES: usize = 100_000;
/// The most argument positions of all default candidates of one request.
const MAX_CANDIDATE_ARGUMENTS: usize = 200_000;
/// The most default candidates of one request.
const MAX_DEFAULT_CANDIDATES: usize = 50_000;
/// The most class candidates of one request.
const MAX_CLASS_CANDIDATES: usize = 20_000;

#[derive(Serialize)]
struct Request {
    protocol_version: u32,
    operation: &'static str,
    root: String,
    defaults: Vec<WireDefault>,
    classes: Vec<WireClass>,
    files: Vec<String>,
}

#[derive(Serialize)]
struct WireDefault {
    id: usize,
    arguments: Vec<WireArgument>,
}

#[derive(Serialize)]
struct WireArgument {
    path: String,
    call_start: u32,
    index: u32,
}

#[derive(Serialize)]
struct WireClass {
    id: usize,
    path: String,
    class: String,
    methods: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum DefaultOutcome {
    Defined,
    MaybeUndefined,
    Unchecked,
}

#[derive(Deserialize)]
struct DefaultResult {
    id: usize,
    outcome: DefaultOutcome,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum MethodOutcome {
    Complete,
    Abstained,
}

#[derive(Deserialize)]
struct MethodParam {
    index: u32,
    name: Option<String>,
    pattern: bool,
    rest: bool,
    optional: bool,
    has_default: bool,
    written: bool,
    decorated: bool,
    condition_reads: u32,
    line: u32,
    col: u32,
}

#[derive(Deserialize)]
struct CallArgument {
    maybe_undefined: bool,
    #[serde(default)]
    pass_through: Option<u32>,
}

#[derive(Deserialize)]
struct MethodCall {
    path: String,
    line: u32,
    col: u32,
    arg_count: u32,
    #[serde(default)]
    spread_from: Option<u32>,
    args: Vec<CallArgument>,
}

#[derive(Deserialize)]
struct MethodResult {
    class_id: usize,
    method: String,
    outcome: MethodOutcome,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    line: u32,
    #[serde(default)]
    params: Vec<MethodParam>,
    #[serde(default)]
    calls: Vec<MethodCall>,
}

#[derive(Deserialize)]
struct Response {
    protocol_version: u32,
    operation: String,
    sidecar_version: String,
    backend: String,
    backend_version: String,
    defaults: Vec<DefaultResult>,
    methods: Vec<MethodResult>,
    elapsed_ms: u64,
}

/// What the companion reported, for `_meta.type_aware`.
struct CallEvidenceRun {
    pub meta: TypeAwareCallEvidenceMeta,
    pub sidecar_version: String,
    pub backend_version: String,
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// The candidates that fit the request bounds, with paths below `root`.
struct Bounded {
    defaults: Vec<UnusedDefaultCandidate>,
    classes: Vec<MethodClassCandidate>,
    files: Vec<String>,
}

fn bounded(root: &Path, candidates: ConstantParameterCandidates) -> Bounded {
    let ConstantParameterCandidates {
        mut defaults,
        mut classes,
        files,
    } = candidates;
    defaults.retain(|candidate| {
        candidate
            .arguments
            .iter()
            .all(|argument| argument.path.starts_with(root))
    });
    defaults.truncate(MAX_DEFAULT_CANDIDATES);
    let mut argument_count = 0;
    defaults.retain(|candidate| {
        argument_count += candidate.arguments.len();
        argument_count <= MAX_CANDIDATE_ARGUMENTS
    });
    let files: Vec<String> = files
        .iter()
        .filter_map(|file| relative(root, file))
        .collect();
    if files.len() > MAX_PROJECT_FILES {
        classes.clear();
    }
    classes.retain(|class| class.path.starts_with(root));
    classes.truncate(MAX_CLASS_CANDIDATES);
    Bounded {
        defaults,
        classes,
        files,
    }
}

fn request(root: &Path, bounded: &Bounded) -> Request {
    Request {
        protocol_version: WIRE_PROTOCOL_VERSION,
        operation: CALL_EVIDENCE_OPERATION,
        root: root.to_string_lossy().into_owned(),
        defaults: bounded
            .defaults
            .iter()
            .enumerate()
            .map(|(id, candidate)| WireDefault {
                id,
                arguments: candidate
                    .arguments
                    .iter()
                    .filter_map(|argument| {
                        Some(WireArgument {
                            path: relative(root, &argument.path)?,
                            call_start: argument.call_start,
                            index: argument.index,
                        })
                    })
                    .collect(),
            })
            .collect(),
        classes: bounded
            .classes
            .iter()
            .enumerate()
            .filter_map(|(id, class)| {
                Some(WireClass {
                    id,
                    path: relative(root, &class.path)?,
                    class: class.class_name.clone(),
                    methods: class.methods.clone(),
                })
            })
            .collect(),
        files: bounded.files.clone(),
    }
}

/// The findings that a validated response proves, and the abstention
/// reasons in `meta`.
fn proved_findings(
    root: &Path,
    bounded: &Bounded,
    response: &Response,
    meta: &mut TypeAwareCallEvidenceMeta,
) -> Vec<ConstantParameter> {
    let mut findings = Vec::new();
    for result in &response.defaults {
        match result.outcome {
            DefaultOutcome::Defined => findings.push(bounded.defaults[result.id].finding.clone()),
            DefaultOutcome::MaybeUndefined => {}
            DefaultOutcome::Unchecked => {
                count(&mut meta.abstention_reasons, result.reason.as_deref());
            }
        }
    }
    for result in &response.methods {
        match result.outcome {
            MethodOutcome::Complete => {
                findings.extend(method_findings(
                    root,
                    &bounded.classes[result.class_id],
                    result,
                ));
            }
            MethodOutcome::Abstained => {
                count(&mut meta.abstention_reasons, result.reason.as_deref());
            }
        }
    }
    findings
}

/// Ask the companion for the evidence of `candidates`, and add each finding
/// that the evidence proves to `results`. Returns `None` when no candidate
/// needs evidence, so the companion does not start.
///
/// # Errors
///
/// Returns an error when the companion fails, or when its response does not
/// match the request.
fn refine_constant_parameters(
    root: &Path,
    results: &mut AnalysisResults,
    candidates: ConstantParameterCandidates,
) -> Result<Option<CallEvidenceRun>, TypeAwareError> {
    if candidates.is_empty() {
        return Ok(None);
    }
    let bounded = bounded(root, candidates);
    if bounded.defaults.is_empty() && bounded.classes.is_empty() {
        return Ok(None);
    }
    let request = request(root, &bounded);
    let response: Response = run_semantic_request(root, &request)?;
    validate(&request, &response)?;
    let mut meta = TypeAwareCallEvidenceMeta {
        default_candidates: bounded.defaults.len(),
        method_candidates: request
            .classes
            .iter()
            .map(|class| class.methods.len())
            .sum(),
        elapsed_ms: response.elapsed_ms,
        ..TypeAwareCallEvidenceMeta::default()
    };
    let findings = proved_findings(root, &bounded, &response, &mut meta);
    meta.findings = findings.len();
    results.constant_parameters.extend(
        findings
            .into_iter()
            .map(ConstantParameterFinding::with_actions),
    );
    sort_constant_parameters(&mut results.constant_parameters);
    Ok(Some(CallEvidenceRun {
        meta,
        sidecar_version: response.sidecar_version,
        backend_version: response.backend_version,
    }))
}

/// Run the call-evidence request for `candidates` after the semantic
/// queries, and record it in the metadata of `outcome`.
///
/// # Errors
///
/// Returns an error when the call-evidence request fails.
pub(super) fn add_call_evidence(
    root: &Path,
    results: &mut AnalysisResults,
    candidates: ConstantParameterCandidates,
    outcome: Option<SemanticDeadCodeOutcome>,
) -> Result<Option<SemanticDeadCodeOutcome>, TypeAwareError> {
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let Some(run) = refine_constant_parameters(&canonical_root, results, candidates)? else {
        return Ok(outcome);
    };
    let mut outcome = match outcome {
        Some(outcome) => outcome,
        None => empty_semantic_outcome(
            Vec::new(),
            selected_backend().map_err(TypeAwareError::from)?.family,
        ),
    };
    let meta = &mut outcome.type_aware.meta;
    meta.executed = true;
    meta.sidecar_version.get_or_insert(run.sidecar_version);
    meta.backend_version.get_or_insert(run.backend_version);
    meta.elapsed_ms += run.meta.elapsed_ms;
    meta.call_evidence = Some(run.meta);
    Ok(Some(outcome))
}

/// Drop the candidates of a run that cannot send the one-shot request. A
/// semantic session does not accept it, so an editor session reports only
/// the findings that the call facts prove.
pub(super) fn drop_candidates(results: &mut AnalysisResults) {
    results.constant_parameter_candidates = ConstantParameterCandidates::default();
}

fn count(reasons: &mut BTreeMap<String, usize>, reason: Option<&str>) {
    *reasons
        .entry(reason.unwrap_or("unknown").to_string())
        .or_default() += 1;
}

/// What one call passes at one parameter position.
enum Seen {
    Omitted,
    Unknown,
    Value { maybe_undefined: bool },
}

fn seen(call: &MethodCall, index: u32) -> Seen {
    if call.spread_from.is_some_and(|spread| index >= spread) {
        return Seen::Unknown;
    }
    if index >= call.arg_count {
        return Seen::Omitted;
    }
    usize::try_from(index)
        .ok()
        .and_then(|at| call.args.get(at))
        .map_or(Seen::Unknown, |argument| Seen::Value {
            maybe_undefined: argument.maybe_undefined,
        })
}

/// The verdict for one method parameter, with the rules of the static rule:
/// `never-passed` when each call omits an optional or defaulted parameter and
/// no self-call passes it on, `unused-default` when each call passes a value
/// that cannot be `undefined`. A method does not get `identical-argument`.
fn method_verdict(
    param: &MethodParam,
    counted: &[&MethodCall],
    self_calls: bool,
) -> Option<ConstantParameterReason> {
    let seen: Vec<Seen> = counted.iter().map(|call| seen(call, param.index)).collect();
    if seen.iter().all(|seen| matches!(seen, Seen::Omitted)) {
        return ((param.optional || param.has_default) && !self_calls)
            .then_some(ConstantParameterReason::NeverPassed);
    }
    let defined = seen.iter().all(|seen| {
        matches!(
            seen,
            Seen::Value {
                maybe_undefined: false
            }
        )
    });
    (param.has_default && defined).then_some(ConstantParameterReason::UnusedDefault)
}

fn method_findings(
    root: &Path,
    class: &MethodClassCandidate,
    result: &MethodResult,
) -> Vec<ConstantParameter> {
    let suppressed: FxHashSet<u32> = class.suppressed_lines.iter().copied().collect();
    if suppressed.contains(&result.line) {
        return Vec::new();
    }
    let mut findings = Vec::new();
    for param in &result.params {
        let Some(name) = param.name.as_deref() else {
            continue;
        };
        if param.pattern || param.rest || param.written || param.decorated {
            continue;
        }
        let counted: Vec<&MethodCall> = result
            .calls
            .iter()
            .filter(|call| {
                usize::try_from(param.index)
                    .ok()
                    .and_then(|at| call.args.get(at))
                    .and_then(|argument| argument.pass_through)
                    != Some(param.index)
            })
            .collect();
        if counted.len() < MIN_CALL_SITES {
            continue;
        }
        let self_calls = counted.len() < result.calls.len();
        let Some(reason) = method_verdict(param, &counted, self_calls) else {
            continue;
        };
        if suppressed.contains(&param.line) {
            continue;
        }
        let subject = MethodSubject {
            param,
            name,
            reason,
        };
        findings.push(method_finding(root, class, result, &subject, &counted));
    }
    findings
}

/// One parameter of a method and its verdict.
struct MethodSubject<'a> {
    param: &'a MethodParam,
    name: &'a str,
    reason: ConstantParameterReason,
}

fn method_finding(
    root: &Path,
    class: &MethodClassCandidate,
    result: &MethodResult,
    subject: &MethodSubject<'_>,
    counted: &[&MethodCall],
) -> ConstantParameter {
    let MethodSubject {
        param,
        name,
        reason,
    } = *subject;
    let mut sites: Vec<ConstantParameterCallSite> = counted
        .iter()
        .map(|call| ConstantParameterCallSite {
            path: root.join(&call.path),
            line: call.line,
            col: call.col,
        })
        .collect();
    sites.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
    });
    sites.dedup_by(|a, b| a.path == b.path && a.line == b.line && a.col == b.col);
    sites.truncate(MAX_INSPECTED_CALL_SITES);
    let evidence = Some(ConstantParameterEvidence::TypeAware);
    ConstantParameter {
        path: PathBuf::from(&class.path),
        function_name: format!("{}.{}", class.class_name, result.method),
        parameter_name: name.to_string(),
        parameter_index: param.index,
        option_key: None,
        line: param.line,
        col: param.col,
        reason,
        evidence,
        value: None,
        optional: param.optional,
        has_default: param.has_default,
        condition_reads: param.condition_reads,
        call_site_count: u32::try_from(counted.len()).unwrap_or(u32::MAX),
        inspected_call_sites: sites,
        explanation: constant_parameter_explanation(reason, false, evidence),
    }
}

fn validate(request: &Request, response: &Response) -> Result<(), TypeAwareError> {
    if response.protocol_version != WIRE_PROTOCOL_VERSION
        || response.operation != CALL_EVIDENCE_OPERATION
    {
        return Err(mismatch(&format!(
            "protocol {} operation `{}`",
            response.protocol_version, response.operation
        )));
    }
    if response.sidecar_version != env!("CARGO_PKG_VERSION") {
        return Err(TypeAwareError::from(format!(
            "type-aware companion version {} does not match Fallow {}. Install fallow-type-aware@{}",
            response.sidecar_version,
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_VERSION"),
        )));
    }
    selected_backend()
        .and_then(|backend| backend.check_reported(&response.backend, &response.backend_version))
        .map_err(TypeAwareError::from)?;
    let defaults: Vec<usize> = response.defaults.iter().map(|result| result.id).collect();
    let expected: Vec<usize> = request.defaults.iter().map(|default| default.id).collect();
    if defaults != expected {
        return Err(mismatch("default results that do not match the request"));
    }
    let methods: Vec<(usize, &str)> = response
        .methods
        .iter()
        .map(|result| (result.class_id, result.method.as_str()))
        .collect();
    let expected: Vec<(usize, &str)> = request
        .classes
        .iter()
        .flat_map(|class| {
            class
                .methods
                .iter()
                .map(move |method| (class.id, method.as_str()))
        })
        .collect();
    if methods != expected {
        return Err(mismatch("method results that do not match the request"));
    }
    for result in &response.methods {
        let complete = matches!(result.outcome, MethodOutcome::Complete);
        if complete == result.reason.is_some() {
            return Err(mismatch(&format!(
                "an invalid result for method `{}`",
                result.method
            )));
        }
        let paths_ok = result.calls.iter().all(|call| {
            !call.path.starts_with('/') && !call.path.split('/').any(|part| part == "..")
        });
        if !paths_ok {
            return Err(mismatch("a call path outside the project root"));
        }
    }
    Ok(())
}

fn mismatch(detail: &str) -> TypeAwareError {
    TypeAwareError::from(format!(
        "the type-aware companion returned {detail} for the call-evidence request"
    ))
}

#[cfg(test)]
#[path = "call_evidence_tests.rs"]
mod tests;
