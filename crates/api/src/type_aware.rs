//! Programmatic adapter for the optional TypeScript semantic companion.
//!
//! The adapter only refines existing Fallow findings and emits project-wide
//! provenance. It deliberately does not surface compiler diagnostics or
//! generic typed lint findings.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use fallow_engine::session::AnalysisSession;
use fallow_types::envelope::{
    TypeAwareAbstentionCounts, TypeAwareMeta, TypeAwarePhaseTimings, TypeAwareProjectMeta,
    TypeAwareProjectSource, TypeAwareProjectStatus,
};
use fallow_types::extract::MemberKind;
use fallow_types::output_dead_code::PrivateTypeLeakFinding;
use fallow_types::results::{AnalysisResults, PrivateTypeLeak};
use fallow_types::semantic::{
    ApiSurfaceEntry, ApiSurfaceResult, PublicTypeReference, SemanticAnalysisIdentity,
    SemanticAnalysisMode, SemanticCapability, SemanticCompleteness, SemanticGapReason,
    SemanticNamespace, SemanticOmission, SemanticPrivateTypeLeak, SemanticQuerySummary,
    SemanticSymbol,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{DeadCodeFilters, ProgrammaticError, TypeAwareOptions};

const PROTOCOL_VERSION: u32 = 3;
const OPERATION: &str = "semantic-queries";
const EVIDENCE_LIMIT: usize = 40;
const BACKEND_FAMILY: &str = "typescript-go";
const BACKEND_VERSION: &str = "7.0.2";
const SIDECAR_TIMEOUT: Duration = Duration::from_mins(2);
const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 16 * 1024;

#[derive(Serialize)]
struct Request {
    protocol_version: u32,
    operation: &'static str,
    root: String,
    projects: Vec<String>,
    evidence_limit: usize,
    queries: Vec<Query>,
    #[serde(skip)]
    requested_capabilities: Vec<SemanticCapability>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
enum Query {
    SymbolUse {
        id: usize,
        symbol: SemanticSymbol,
    },
    ApiSurface {
        id: usize,
        entry_points: Vec<String>,
        include_cycles: bool,
    },
}

impl Query {
    const fn id(&self) -> usize {
        match self {
            Self::SymbolUse { id, .. } | Self::ApiSurface { id, .. } => *id,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    protocol_version: u32,
    operation: String,
    sidecar_version: String,
    backend: String,
    backend_version: String,
    selected_tsconfigs: Vec<String>,
    projects: Vec<ProjectResponse>,
    results: Vec<QueryResponse>,
    phase_timings_ms: PhaseTimings,
    warnings: Vec<String>,
    elapsed_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectResponse {
    config: String,
    source: TypeAwareProjectSource,
    status: TypeAwareProjectStatus,
    reason_code: Option<SemanticGapReason>,
    blocking_diagnostic_count: usize,
    source_file_count: usize,
    program_reused: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryResponse {
    query_id: usize,
    operation: Operation,
    assertion: String,
    status: SemanticCompleteness,
    reason_code: Option<SemanticGapReason>,
    actions: Vec<String>,
    evidence: Vec<serde_json::Value>,
    total_evidence_count: usize,
    truncated: bool,
    omissions: Vec<SemanticOmission>,
    data: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiSurfaceData {
    exports: Vec<SemanticSymbol>,
    total_export_count: usize,
    entries: Vec<ApiSurfaceEntryData>,
    total_entry_count: usize,
    leaks: Vec<ApiLeakData>,
    total_leak_count: usize,
    public_signature_edges: Vec<serde_json::Value>,
    total_public_signature_edge_count: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiSurfaceEntryData {
    exposed: SemanticSymbol,
    origin: SemanticSymbol,
    signature_fingerprint: String,
    referenced_types: Vec<PublicTypeReferenceData>,
    total_referenced_type_count: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicTypeReferenceData {
    declaration: SemanticSymbol,
    relation: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiLeakData {
    exposed_symbol: SemanticSymbol,
    private_declaration: SemanticSymbol,
    relation: String,
    evidence: EvidenceLocation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceLocation {
    path: PathBuf,
    line: u32,
    col: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Operation {
    SymbolUse,
    ApiSurface,
}

impl Operation {
    const fn capability(self) -> SemanticCapability {
        match self {
            Self::SymbolUse => SemanticCapability::SymbolUse,
            Self::ApiSurface => SemanticCapability::ApiSurface,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhaseTimings {
    project_setup: u64,
    diagnostics: u64,
    semantic_queries: u64,
}

enum Target {
    ClassMember(usize),
    UnusedExport(usize),
    UnusedType(usize),
    ApiSurface,
}

/// Refine a typed programmatic dead-code run through the same v3 protocol.
pub fn refine_dead_code(
    options: &TypeAwareOptions,
    filters: &DeadCodeFilters,
    session: &AnalysisSession,
    results: &mut AnalysisResults,
) -> Result<Option<TypeAwareMeta>, ProgrammaticError> {
    if !options.enabled {
        return Ok(None);
    }
    let root = session.root().canonicalize().map_err(|error| {
        semantic_error(format!(
            "failed to resolve project root {}: {error}",
            session.root().display()
        ))
    })?;
    let include_all = !filters.any_active();
    let include_symbol_use = include_all
        || filters.unused_exports
        || filters.unused_types
        || filters.unused_class_members;
    let include_api_surface = include_all || filters.private_type_leaks;
    let entry_points = fallow_engine::list_inventory::collect_entry_points(
        session.config(),
        session.files(),
        session.workspaces(),
        None,
    )
    .into_iter()
    .map(|entry| protocol_path(&root, &entry.path))
    .collect::<Result<Vec<_>, _>>()?;
    let (queries, targets) = build_queries(&root, results, &entry_points, include_api_surface)?;
    let requested_capabilities = [
        include_symbol_use.then_some(SemanticCapability::SymbolUse),
        include_api_surface.then_some(SemanticCapability::ApiSurface),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let request = Request {
        protocol_version: PROTOCOL_VERSION,
        operation: OPERATION,
        root: root.to_string_lossy().into_owned(),
        projects: options
            .projects
            .iter()
            .map(|project| project.to_string_lossy().into_owned())
            .collect(),
        evidence_limit: EVIDENCE_LIMIT,
        queries,
        requested_capabilities,
    };
    if request.queries.is_empty() {
        return Ok(Some(empty_semantic_meta(&request)));
    }
    let response = run_request(&root, &request)?;
    validate_response(&request, &response)?;
    if options.require == fallow_config::TypeAwareRequire::Complete
        && aggregate_completeness(&response.results) != SemanticCompleteness::Complete
    {
        return Err(semantic_error(
            "type-aware completeness is required, but the semantic result was incomplete",
        ));
    }
    apply_response(results, &request, &targets, response)
}

fn empty_semantic_meta(request: &Request) -> TypeAwareMeta {
    TypeAwareMeta {
        identity: Some(SemanticAnalysisIdentity {
            mode: SemanticAnalysisMode::TypeAware,
            semantic_schema_version: 1,
            capabilities: request.requested_capabilities.clone(),
            project_config_hash: project_config_hash(
                Path::new(&request.root),
                &identity_project_configs(Path::new(&request.root), &request.projects),
            ),
            backend_family: BACKEND_FAMILY.to_string(),
            completeness: SemanticCompleteness::Complete,
        }),
        protocol_version: PROTOCOL_VERSION,
        sidecar_version: env!("CARGO_PKG_VERSION").to_string(),
        backend: BACKEND_FAMILY.to_string(),
        backend_version: BACKEND_VERSION.to_string(),
        ..TypeAwareMeta::default()
    }
}

fn build_queries(
    root: &Path,
    results: &AnalysisResults,
    entry_points: &[PathBuf],
    include_api_surface: bool,
) -> Result<(Vec<Query>, BTreeMap<usize, Target>), ProgrammaticError> {
    let mut queries = Vec::new();
    let mut targets = BTreeMap::new();
    for (index, finding) in results.unused_class_members.iter().enumerate() {
        let id = queries.len();
        queries.push(Query::SymbolUse {
            id,
            symbol: SemanticSymbol {
                path: protocol_path(root, &finding.member.path)?,
                namespace: SemanticNamespace::Value,
                declaration_kind: member_kind_name(finding.member.kind).to_string(),
                exported_name: finding.member.member_name.clone(),
                local_name: finding.member.member_name.clone(),
                owner: Some(finding.member.parent_name.clone()),
                line: finding.member.line,
                col: finding.member.col,
            },
        });
        targets.insert(id, Target::ClassMember(index));
    }
    for (index, finding) in results.unused_exports.iter().enumerate() {
        let id = queries.len();
        queries.push(Query::SymbolUse {
            id,
            symbol: export_symbol(root, &finding.export, SemanticNamespace::Value)?,
        });
        targets.insert(id, Target::UnusedExport(index));
    }
    for (index, finding) in results.unused_types.iter().enumerate() {
        let id = queries.len();
        queries.push(Query::SymbolUse {
            id,
            symbol: export_symbol(root, &finding.export, SemanticNamespace::Type)?,
        });
        targets.insert(id, Target::UnusedType(index));
    }
    if include_api_surface && !entry_points.is_empty() {
        let id = queries.len();
        queries.push(Query::ApiSurface {
            id,
            entry_points: entry_points
                .iter()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect(),
            include_cycles: false,
        });
        targets.insert(id, Target::ApiSurface);
    }
    Ok((queries, targets))
}

#[expect(
    clippy::too_many_lines,
    reason = "semantic response validation and conservative result refinement stay together"
)]
fn apply_response(
    results: &mut AnalysisResults,
    request: &Request,
    targets: &BTreeMap<usize, Target>,
    response: Response,
) -> Result<Option<TypeAwareMeta>, ProgrammaticError> {
    let mut class_members = BTreeSet::new();
    let mut exports = BTreeSet::new();
    let mut types = BTreeSet::new();
    let mut confirmed_used_count = 0;
    let mut unresolved_count = 0;
    let mut abstained_count = 0;
    let mut abstention_reasons = TypeAwareAbstentionCounts::default();
    let mut api_surface = None;
    for result in &response.results {
        let Some(target) = targets.get(&result.query_id) else {
            return Err(semantic_error(format!(
                "semantic response returned unknown query id {}",
                result.query_id
            )));
        };
        if result.assertion == "confirmed-used" {
            match target {
                Target::ClassMember(index) => {
                    class_members.insert(*index);
                }
                Target::UnusedExport(index) => {
                    exports.insert(*index);
                }
                Target::UnusedType(index) => {
                    types.insert(*index);
                }
                Target::ApiSurface => {
                    api_surface = Some(apply_api_surface(
                        Path::new(&request.root),
                        results,
                        result,
                    )?);
                }
            }
            confirmed_used_count += 1;
        } else if !matches!(target, Target::ApiSurface) {
            if result.status == SemanticCompleteness::Complete {
                unresolved_count += 1;
            } else {
                abstained_count += 1;
                match result.reason_code {
                    Some(SemanticGapReason::NoProject) => abstention_reasons.no_project += 1,
                    Some(SemanticGapReason::BlockingDiagnostics) => {
                        abstention_reasons.blocking_diagnostics += 1;
                    }
                    _ => abstention_reasons.ambiguous_project += 1,
                }
            }
        }
    }
    retain_unconfirmed(&mut results.unused_class_members, &class_members);
    retain_unconfirmed(&mut results.unused_exports, &exports);
    retain_unconfirmed(&mut results.unused_types, &types);

    let completeness = aggregate_completeness(&response.results);
    let mut capabilities = request.requested_capabilities.clone();
    capabilities.sort();
    capabilities.dedup();
    let identity = SemanticAnalysisIdentity {
        mode: SemanticAnalysisMode::TypeAware,
        semantic_schema_version: 1,
        capabilities,
        project_config_hash: project_config_hash(
            Path::new(&request.root),
            &identity_project_configs(Path::new(&request.root), &request.projects),
        ),
        backend_family: response.backend.clone(),
        completeness,
    };
    let query_summaries = response
        .results
        .iter()
        .map(|result| SemanticQuerySummary {
            query_id: result.query_id,
            capability: result.operation.capability(),
            assertion: result.assertion.clone(),
            status: result.status,
            reason_code: result.reason_code,
            total_evidence_count: result.total_evidence_count,
            truncated: result.truncated,
            omissions: result.omissions.clone(),
            actions: result.actions.clone(),
        })
        .collect();
    let projects = response
        .projects
        .into_iter()
        .map(|project| TypeAwareProjectMeta {
            config: project.config,
            source: project.source,
            status: project.status,
            candidate_count: 0,
            confirmed_used_count: 0,
            unresolved_count: 0,
            abstained_count: usize::from(project.reason_code.is_some()),
            blocking_diagnostic_count: project.blocking_diagnostic_count,
            source_file_count: project.source_file_count,
            program_reused: Some(project.program_reused),
            reason_code: project.reason_code,
            abstain_reason: None,
        })
        .collect();
    let warning_count = response.warnings.len();
    Ok(Some(TypeAwareMeta {
        identity: Some(identity),
        queries: query_summaries,
        symbol_traces: Vec::new(),
        api_surface,
        symbol_impacts: Vec::new(),
        type_coupling: None,
        protocol_version: response.protocol_version,
        sidecar_version: response.sidecar_version,
        backend: response.backend,
        backend_version: response.backend_version,
        selected_tsconfigs: response.selected_tsconfigs,
        candidate_count: request
            .queries
            .iter()
            .filter(|query| matches!(query, Query::SymbolUse { .. }))
            .count(),
        confirmed_used_count,
        unresolved_count,
        abstained_count,
        abstention_reasons,
        projects,
        warning_count,
        warnings: response.warnings,
        elapsed_ms: response.elapsed_ms,
        phase_timings_ms: TypeAwarePhaseTimings {
            project_setup: response.phase_timings_ms.project_setup,
            diagnostics: response.phase_timings_ms.diagnostics,
            symbol_scan: response.phase_timings_ms.semantic_queries,
        },
    }))
}

fn apply_api_surface(
    root: &Path,
    results: &mut AnalysisResults,
    result: &QueryResponse,
) -> Result<ApiSurfaceResult, ProgrammaticError> {
    let data: ApiSurfaceData = serde_json::from_value(result.data.clone()).map_err(|error| {
        semantic_error(format!("failed to decode type-aware API surface: {error}"))
    })?;
    if data.total_export_count < data.exports.len()
        || data.total_entry_count < data.entries.len()
        || data.total_leak_count < data.leaks.len()
        || data.total_public_signature_edge_count < data.public_signature_edges.len()
        || data
            .entries
            .iter()
            .any(|entry| entry.total_referenced_type_count < entry.referenced_types.len())
    {
        return Err(semantic_error(
            "type-aware API surface totals are smaller than their returned arrays",
        ));
    }
    let semantic_leaks = data
        .leaks
        .iter()
        .map(|leak| SemanticPrivateTypeLeak {
            exposed: leak.exposed_symbol.clone(),
            private_declaration: leak.private_declaration.clone(),
            relation: leak.relation.clone(),
            diagnostic_code: None,
        })
        .collect::<Vec<_>>();
    for (wire, semantic) in data.leaks.iter().zip(&semantic_leaks) {
        attach_or_add_private_type_leak(root, results, wire, semantic.clone());
    }
    let entries = data
        .entries
        .into_iter()
        .map(|entry| ApiSurfaceEntry {
            exposed: entry.exposed,
            origin: entry.origin,
            signature_fingerprint: entry.signature_fingerprint,
            referenced_types: entry
                .referenced_types
                .into_iter()
                .map(|reference| PublicTypeReference {
                    declaration: reference.declaration,
                    relation: reference.relation,
                })
                .collect(),
        })
        .collect();
    Ok(ApiSurfaceResult {
        assertion: result.assertion.clone(),
        status: result.status,
        entries,
        private_type_leaks: semantic_leaks,
        omissions: result.omissions.clone(),
        actions: result.actions.clone(),
    })
}

fn attach_or_add_private_type_leak(
    root: &Path,
    results: &mut AnalysisResults,
    wire: &ApiLeakData,
    semantic: SemanticPrivateTypeLeak,
) {
    let evidence_path = root.join(&wire.evidence.path);
    let type_name = wire.private_declaration.local_name.clone();
    if let Some(existing) = results.private_type_leaks.iter_mut().find(|finding| {
        finding.leak.path == evidence_path
            && finding.leak.export_name == wire.exposed_symbol.exported_name
            && finding.leak.type_name == type_name
    }) {
        existing.leak.semantic = Some(semantic);
        return;
    }
    results
        .private_type_leaks
        .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
            path: evidence_path,
            export_name: wire.exposed_symbol.exported_name.clone(),
            type_name,
            line: wire.evidence.line,
            col: wire.evidence.col,
            span_start: 0,
            semantic: Some(semantic),
        }));
}

fn run_request(root: &Path, request: &Request) -> Result<Response, ProgrammaticError> {
    let sidecar = discover_sidecar()?;
    let install_dir = sidecar.parent().ok_or_else(|| {
        semantic_error(format!(
            "type-aware companion {} has no trusted install directory",
            sidecar.display()
        ))
    })?;
    let mut command = Command::new(&sidecar);
    command
        .current_dir(install_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    restrict_environment(&mut command, root);
    let mut child = command.spawn().map_err(|error| {
        semantic_error(format!("failed to start {}: {error}", sidecar.display()))
    })?;
    let request_bytes = serde_json::to_vec(request)
        .map_err(|error| semantic_error(format!("failed to encode semantic request: {error}")))?;
    if request_bytes.len() > MAX_REQUEST_BYTES {
        terminate_and_wait(&mut child);
        return Err(semantic_error("semantic request exceeded the 8 MB limit"));
    }
    let Some(mut stdin) = child.stdin.take() else {
        terminate_and_wait(&mut child);
        return Err(semantic_error("semantic companion stdin was unavailable"));
    };
    if let Err(error) = stdin.write_all(&request_bytes) {
        drop(stdin);
        terminate_and_wait(&mut child);
        return Err(semantic_error(format!(
            "failed to write semantic request: {error}"
        )));
    }
    drop(stdin);
    let Some(stdout) = child.stdout.take() else {
        terminate_and_wait(&mut child);
        return Err(semantic_error("semantic companion stdout was unavailable"));
    };
    let Some(stderr) = child.stderr.take() else {
        drop(stdout);
        terminate_and_wait(&mut child);
        return Err(semantic_error("semantic companion stderr was unavailable"));
    };
    let stdout_reader = std::thread::spawn(move || read_bounded(stdout, MAX_RESPONSE_BYTES));
    let stderr_reader = std::thread::spawn(move || read_bounded(stderr, MAX_STDERR_BYTES));
    let deadline = Instant::now() + SIDECAR_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                terminate_child_tree(&mut child);
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(semantic_error(
                    "semantic companion timed out after 120 seconds",
                ));
            }
            Err(error) => {
                terminate_child_tree(&mut child);
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(semantic_error(format!(
                    "failed to wait for semantic companion: {error}"
                )));
            }
        }
    };
    let (stdout, stdout_truncated) = stdout_reader
        .join()
        .map_err(|_| semantic_error("semantic stdout reader panicked"))?
        .map_err(|error| semantic_error(format!("failed to read semantic stdout: {error}")))?;
    let (stderr, _) = stderr_reader
        .join()
        .map_err(|_| semantic_error("semantic stderr reader panicked"))?
        .map_err(|error| semantic_error(format!("failed to read semantic stderr: {error}")))?;
    if stdout_truncated {
        return Err(semantic_error("semantic response exceeded the 32 MB limit"));
    }
    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        return Err(semantic_error(format!(
            "semantic companion exited with {}: {}",
            status,
            stderr.chars().take(4096).collect::<String>().trim()
        )));
    }
    serde_json::from_slice(&stdout)
        .map_err(|error| semantic_error(format!("invalid semantic response: {error}")))
}

fn read_bounded(reader: impl Read, limit: usize) -> Result<(Vec<u8>, bool), std::io::Error> {
    let mut bytes = Vec::new();
    reader.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > limit;
    if truncated {
        bytes.truncate(limit);
    }
    Ok((bytes, truncated))
}

#[cfg_attr(
    unix,
    expect(
        unsafe_code,
        reason = "POSIX process-group termination requires libc::kill"
    )
)]
fn terminate_child_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        unsafe fn kill_group(pid: u32) {
            // SAFETY: the child was spawned into a dedicated process group.
            let _ = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
        }
        // SAFETY: `child.id()` is the leader of the dedicated process group.
        unsafe { kill_group(child.id()) };
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

fn terminate_and_wait(child: &mut std::process::Child) {
    terminate_child_tree(child);
    let _ = child.wait();
}

fn restrict_environment(command: &mut Command, root: &Path) {
    const ALLOWED: &[&str] = &[
        "HOME",
        "PATHEXT",
        "COMSPEC",
        "SYSTEMROOT",
        "TEMP",
        "TMP",
        "TMPDIR",
        "USERPROFILE",
        "WINDIR",
    ];
    let values = ALLOWED
        .iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (*key, value)))
        .collect::<Vec<_>>();
    let path = std::env::var_os("PATH").and_then(|value| sanitize_search_path(root, &value));
    command.env_clear().envs(values);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    command.env("NoDefaultCurrentDirectoryInExePath", "1");
}

fn sanitize_search_path(root: &Path, value: &OsStr) -> Option<OsString> {
    let mut safe = Vec::new();
    for entry in std::env::split_paths(value) {
        if !entry.is_absolute() {
            continue;
        }
        let Ok(canonical) = entry.canonicalize() else {
            continue;
        };
        if canonical.starts_with(root) || safe.contains(&canonical) {
            continue;
        }
        safe.push(canonical);
    }
    std::env::join_paths(safe).ok()
}

fn discover_sidecar() -> Result<PathBuf, ProgrammaticError> {
    if let Some(value) = std::env::var_os("FALLOW_TYPE_AWARE_BIN") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return path.canonicalize().map_err(|error| {
                semantic_error(format!("failed to resolve {}: {error}", path.display()))
            });
        }
        return Err(semantic_error(format!(
            "FALLOW_TYPE_AWARE_BIN points to {}, but no file exists there",
            path.display()
        )));
    }
    let executable = std::env::current_exe()
        .map_err(|error| semantic_error(format!("failed to locate current executable: {error}")))?;
    let sibling = executable
        .parent()
        .map(|parent| {
            parent.join(if cfg!(windows) {
                "fallow-type-aware.cmd"
            } else {
                "fallow-type-aware"
            })
        })
        .filter(|path| path.is_file());
    sibling.ok_or_else(|| {
        semantic_error(format!(
            "type-aware companion is unavailable. Install fallow-type-aware@{} or set FALLOW_TYPE_AWARE_BIN to its trusted absolute path",
            env!("CARGO_PKG_VERSION")
        ))
    })
}

fn validate_response(request: &Request, response: &Response) -> Result<(), ProgrammaticError> {
    if response.protocol_version != PROTOCOL_VERSION || response.operation != OPERATION {
        return Err(semantic_error("type-aware protocol mismatch"));
    }
    if response.sidecar_version != env!("CARGO_PKG_VERSION") {
        return Err(semantic_error(format!(
            "type-aware companion version {} does not match Fallow {}",
            response.sidecar_version,
            env!("CARGO_PKG_VERSION")
        )));
    }
    if response.backend != BACKEND_FAMILY {
        return Err(semantic_error(format!(
            "unsupported type-aware backend {}",
            response.backend
        )));
    }
    if response.backend_version != BACKEND_VERSION {
        return Err(semantic_error(format!(
            "unsupported type-aware backend version {}; expected {BACKEND_VERSION}",
            response.backend_version
        )));
    }
    let expected = request
        .queries
        .iter()
        .map(|query| {
            let operation = match query {
                Query::SymbolUse { .. } => Operation::SymbolUse,
                Query::ApiSurface { .. } => Operation::ApiSurface,
            };
            (query.id(), operation)
        })
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    for result in &response.results {
        let Some(operation) = expected.get(&result.query_id) else {
            return Err(semantic_error(
                "semantic response returned an unknown query",
            ));
        };
        if *operation != result.operation || !seen.insert(result.query_id) {
            return Err(semantic_error(
                "semantic response returned a duplicate or mismatched query",
            ));
        }
        if result.total_evidence_count < result.evidence.len() {
            return Err(semantic_error("semantic evidence totals are inconsistent"));
        }
        if result.status != SemanticCompleteness::Complete
            && (result.reason_code.is_none()
                || result.actions.is_empty()
                || result.omissions.is_empty())
        {
            return Err(semantic_error(
                "incomplete semantic result omitted its reason and next action",
            ));
        }
    }
    if seen.len() != expected.len() {
        return Err(semantic_error(
            "semantic response did not classify every query",
        ));
    }
    Ok(())
}

fn export_symbol(
    root: &Path,
    export: &fallow_types::results::UnusedExport,
    namespace: SemanticNamespace,
) -> Result<SemanticSymbol, ProgrammaticError> {
    Ok(SemanticSymbol {
        path: protocol_path(root, &export.path)?,
        namespace,
        declaration_kind: "export".to_string(),
        exported_name: export.export_name.clone(),
        local_name: export.export_name.clone(),
        owner: None,
        line: export.line,
        col: export.col,
    })
}

fn protocol_path(root: &Path, path: &Path) -> Result<PathBuf, ProgrammaticError> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root).map_err(|_| {
            semantic_error(format!(
                "semantic path {} is outside project root {}",
                path.display(),
                root.display()
            ))
        })?
    } else {
        path
    };
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(semantic_error(format!(
            "semantic path {} is not project-relative",
            relative.display()
        )));
    }
    Ok(relative.to_path_buf())
}

fn retain_unconfirmed<T>(items: &mut Vec<T>, confirmed: &BTreeSet<usize>) {
    let mut index = 0;
    items.retain(|_| {
        let keep = !confirmed.contains(&index);
        index += 1;
        keep
    });
}

fn aggregate_completeness(results: &[QueryResponse]) -> SemanticCompleteness {
    if results
        .iter()
        .any(|result| result.status == SemanticCompleteness::Unavailable)
    {
        SemanticCompleteness::Unavailable
    } else if results
        .iter()
        .any(|result| result.status == SemanticCompleteness::Partial)
    {
        SemanticCompleteness::Partial
    } else {
        SemanticCompleteness::Complete
    }
}

fn project_config_hash(root: &Path, configs: &[String]) -> String {
    let mut hasher = Sha256::new();
    let mut configs = configs.to_vec();
    configs.sort();
    for config in configs {
        hasher.update(config.as_bytes());
        let path = Path::new(&config);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        if !config.starts_with('<')
            && let Ok(bytes) = std::fs::read(path)
        {
            hasher.update(bytes);
        }
    }
    format!("sha256:{}", digest_hex(hasher.finalize()))
}

fn identity_project_configs(root: &Path, projects: &[String]) -> Vec<String> {
    if projects.is_empty() {
        return vec![if root.join("tsconfig.json").is_file() {
            "tsconfig.json".to_string()
        } else {
            "<auto>".to_string()
        }];
    }
    let mut normalized = projects
        .iter()
        .map(|project| {
            let path = Path::new(project);
            let path = if path.is_absolute() {
                path.strip_prefix(root).unwrap_or(path)
            } else {
                path
            };
            path.to_string_lossy().replace('\\', "/")
        })
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn digest_hex(bytes: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;

    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

const fn member_kind_name(kind: MemberKind) -> &'static str {
    match kind {
        MemberKind::ClassMethod => "class_method",
        MemberKind::ClassProperty => "class_property",
        MemberKind::EnumMember => "enum_member",
        MemberKind::NamespaceMember => "namespace_member",
        MemberKind::StoreMember => "store_member",
    }
}

fn semantic_error(message: impl Into<String>) -> ProgrammaticError {
    ProgrammaticError::new(message.into(), 2)
        .with_code("FALLOW_TYPE_AWARE_FAILED")
        .with_context("analysis.typeAware")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn symbol() -> SemanticSymbol {
        SemanticSymbol {
            path: PathBuf::from("src/index.ts"),
            namespace: SemanticNamespace::Value,
            declaration_kind: "function".to_string(),
            exported_name: "run".to_string(),
            local_name: "run".to_string(),
            owner: None,
            line: 1,
            col: 0,
        }
    }

    fn request() -> Request {
        Request {
            protocol_version: PROTOCOL_VERSION,
            operation: OPERATION,
            root: ".".to_string(),
            projects: vec!["tsconfig.json".to_string()],
            evidence_limit: EVIDENCE_LIMIT,
            queries: vec![
                Query::SymbolUse {
                    id: 0,
                    symbol: symbol(),
                },
                Query::ApiSurface {
                    id: 1,
                    entry_points: vec!["src/index.ts".to_string()],
                    include_cycles: true,
                },
            ],
            requested_capabilities: vec![
                SemanticCapability::SymbolUse,
                SemanticCapability::ApiSurface,
            ],
        }
    }

    fn query_response(id: usize, operation: Operation) -> QueryResponse {
        QueryResponse {
            query_id: id,
            operation,
            assertion: "complete".to_string(),
            status: SemanticCompleteness::Complete,
            reason_code: None,
            actions: Vec::new(),
            evidence: vec![json!({"path": "src/index.ts"})],
            total_evidence_count: 1,
            truncated: false,
            omissions: Vec::new(),
            data: json!({}),
        }
    }

    fn response() -> Response {
        Response {
            protocol_version: PROTOCOL_VERSION,
            operation: OPERATION.to_string(),
            sidecar_version: env!("CARGO_PKG_VERSION").to_string(),
            backend: BACKEND_FAMILY.to_string(),
            backend_version: BACKEND_VERSION.to_string(),
            selected_tsconfigs: vec!["tsconfig.json".to_string()],
            projects: Vec::new(),
            results: vec![
                query_response(0, Operation::SymbolUse),
                query_response(1, Operation::ApiSurface),
            ],
            phase_timings_ms: PhaseTimings {
                project_setup: 1,
                diagnostics: 2,
                semantic_queries: 3,
            },
            warnings: Vec::new(),
            elapsed_ms: 6,
        }
    }

    #[test]
    fn validates_protocol_contract_and_incomplete_evidence_requirements() {
        let request = request();
        let mut response = response();
        assert!(validate_response(&request, &response).is_ok());

        response.protocol_version += 1;
        assert!(validate_response(&request, &response).is_err());
        response.protocol_version = PROTOCOL_VERSION;

        response.sidecar_version = "0.0.0".to_string();
        assert!(validate_response(&request, &response).is_err());
        response.sidecar_version = env!("CARGO_PKG_VERSION").to_string();

        response.backend = "other".to_string();
        assert!(validate_response(&request, &response).is_err());
        response.backend = BACKEND_FAMILY.to_string();

        response.backend_version = "0".to_string();
        assert!(validate_response(&request, &response).is_err());
        response.backend_version = BACKEND_VERSION.to_string();

        response.results.pop();
        assert!(validate_response(&request, &response).is_err());
        response
            .results
            .push(query_response(1, Operation::ApiSurface));

        response
            .results
            .push(query_response(1, Operation::ApiSurface));
        assert!(validate_response(&request, &response).is_err());
        response.results.pop();

        response.results[0].operation = Operation::ApiSurface;
        assert!(validate_response(&request, &response).is_err());
        response.results[0].operation = Operation::SymbolUse;

        response.results[0].query_id = 99;
        assert!(validate_response(&request, &response).is_err());
        response.results[0].query_id = 0;

        response.results[0].total_evidence_count = 0;
        assert!(validate_response(&request, &response).is_err());
        response.results[0].total_evidence_count = 1;

        response.results[0].status = SemanticCompleteness::Partial;
        assert!(validate_response(&request, &response).is_err());
        response.results[0].reason_code = Some(SemanticGapReason::DynamicBehavior);
        response.results[0].actions = vec!["Review dynamic use.".to_string()];
        response.results[0].omissions = vec![SemanticOmission {
            reason_code: SemanticGapReason::DynamicBehavior,
            count: 1,
        }];
        assert!(validate_response(&request, &response).is_ok());
    }

    #[test]
    fn normalizes_paths_projects_hashes_and_member_kinds() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        fs::create_dir(root.join("src")).expect("source directory");
        fs::write(root.join("tsconfig.json"), "{}").expect("tsconfig");

        assert_eq!(
            protocol_path(root, &root.join("src/index.ts")).expect("root-relative path"),
            PathBuf::from("src/index.ts")
        );
        assert!(protocol_path(root, Path::new("../outside.ts")).is_err());
        assert!(protocol_path(root, &root.join("../outside.ts")).is_err());

        assert_eq!(
            identity_project_configs(root, &[]),
            vec!["tsconfig.json".to_string()]
        );
        assert_eq!(
            identity_project_configs(
                root,
                &[
                    "configs/tsconfig.json".to_string(),
                    "configs/tsconfig.json".to_string(),
                    root.join("tsconfig.json").to_string_lossy().into_owned(),
                ],
            ),
            vec![
                "configs/tsconfig.json".to_string(),
                "tsconfig.json".to_string()
            ]
        );
        assert_eq!(digest_hex([0, 15, 255]), "000fff");
        assert_eq!(
            project_config_hash(root, &["tsconfig.json".to_string()]),
            project_config_hash(root, &["tsconfig.json".to_string()])
        );

        assert_eq!(member_kind_name(MemberKind::ClassMethod), "class_method");
        assert_eq!(
            member_kind_name(MemberKind::ClassProperty),
            "class_property"
        );
        assert_eq!(member_kind_name(MemberKind::EnumMember), "enum_member");
        assert_eq!(
            member_kind_name(MemberKind::NamespaceMember),
            "namespace_member"
        );
        assert_eq!(member_kind_name(MemberKind::StoreMember), "store_member");
    }

    #[test]
    fn classifies_queries_completeness_and_confirmed_items() {
        let request = request();
        assert_eq!(request.queries[0].id(), 0);
        assert_eq!(request.queries[1].id(), 1);
        assert_eq!(
            Operation::SymbolUse.capability(),
            SemanticCapability::SymbolUse
        );
        assert_eq!(
            Operation::ApiSurface.capability(),
            SemanticCapability::ApiSurface
        );

        let complete = query_response(0, Operation::SymbolUse);
        let mut partial = query_response(1, Operation::ApiSurface);
        partial.status = SemanticCompleteness::Partial;
        let mut unavailable = query_response(2, Operation::ApiSurface);
        unavailable.status = SemanticCompleteness::Unavailable;
        assert_eq!(
            aggregate_completeness(&[complete]),
            SemanticCompleteness::Complete
        );
        assert_eq!(
            aggregate_completeness(&[partial]),
            SemanticCompleteness::Partial
        );
        assert_eq!(
            aggregate_completeness(&[unavailable]),
            SemanticCompleteness::Unavailable
        );

        let mut items = vec!["zero", "one", "two", "three"];
        retain_unconfirmed(&mut items, &BTreeSet::from([1, 3]));
        assert_eq!(items, vec!["zero", "two"]);
    }

    #[test]
    fn bounded_reader_reports_overflow_without_losing_prefix() {
        let (bytes, overflowed) = read_bounded(&b"abcdef"[..], 3).expect("bounded read succeeds");
        assert_eq!(bytes, b"abc");
        assert!(overflowed);

        let (bytes, overflowed) = read_bounded(&b"ok"[..], 3).expect("short bounded read succeeds");
        assert_eq!(bytes, b"ok");
        assert!(!overflowed);
    }
}
