//! Typed client for the batched type-aware semantic protocol.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use fallow_config::TypeAwareRequire;
use fallow_types::envelope::{
    TypeAwareAbstentionCounts, TypeAwareMeta, TypeAwarePhaseTimings, TypeAwareProjectMeta,
    TypeAwareProjectSource, TypeAwareProjectStatus,
};
use fallow_types::extract::MemberKind;
use fallow_types::output_dead_code::PrivateTypeLeakFinding;
use fallow_types::results::{AnalysisResults, PrivateTypeLeak};
use fallow_types::semantic::{
    ApiSurfaceEntry, ApiSurfaceResult, PublicTypeReference, SemanticAliasHop,
    SemanticAnalysisIdentity, SemanticAnalysisMode, SemanticCapability, SemanticCompleteness,
    SemanticGapReason, SemanticImpactPath, SemanticNamespace, SemanticOmission,
    SemanticPrivateTypeLeak, SemanticQuerySummary, SemanticReference, SemanticSourceLocation,
    SemanticSymbol, SemanticSymbolImpact, SemanticSymbolTrace, TypeCouplingCycle, TypeCouplingEdge,
    TypeCouplingFile, TypeCouplingReport, TypeCouplingSummary,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::type_aware::{TypeAwareError, TypeAwareOutcome};

const PROTOCOL_VERSION: u32 = 3;
const OPERATION: &str = "semantic-queries";
const EVIDENCE_LIMIT: usize = 40;
const SEMANTIC_SCHEMA_VERSION: u32 = 1;
const BACKEND_FAMILY: &str = "typescript-go";
const BACKEND_VERSION: &str = "7.0.2";

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct SemanticRequest {
    protocol_version: u32,
    operation: &'static str,
    root: String,
    projects: Vec<String>,
    evidence_limit: usize,
    queries: Vec<SemanticQuery>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
enum SemanticQuery {
    SymbolUse {
        id: usize,
        symbol: SemanticSymbol,
    },
    SymbolTrace {
        id: usize,
        symbol: SemanticSymbol,
    },
    SymbolImpact {
        id: usize,
        symbol: SemanticSymbol,
    },
    ApiSurface {
        id: usize,
        entry_points: Vec<String>,
        include_cycles: bool,
    },
    TypeCoupling {
        id: usize,
        entry_points: Vec<String>,
        include_cycles: bool,
    },
}

impl SemanticQuery {
    const fn id(&self) -> usize {
        match self {
            Self::SymbolUse { id, .. }
            | Self::SymbolTrace { id, .. }
            | Self::SymbolImpact { id, .. }
            | Self::ApiSurface { id, .. }
            | Self::TypeCoupling { id, .. } => *id,
        }
    }

    const fn operation(&self) -> SemanticOperation {
        match self {
            Self::SymbolUse { .. } => SemanticOperation::SymbolUse,
            Self::SymbolTrace { .. } => SemanticOperation::SymbolTrace,
            Self::SymbolImpact { .. } => SemanticOperation::SymbolImpact,
            Self::ApiSurface { .. } => SemanticOperation::ApiSurface,
            Self::TypeCoupling { .. } => SemanticOperation::TypeCoupling,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SemanticOperation {
    SymbolUse,
    SymbolTrace,
    ApiSurface,
    SymbolImpact,
    TypeCoupling,
}

impl SemanticOperation {
    const fn capability(self) -> SemanticCapability {
        match self {
            Self::SymbolUse => SemanticCapability::SymbolUse,
            Self::SymbolTrace => SemanticCapability::SymbolTrace,
            Self::ApiSurface => SemanticCapability::ApiSurface,
            Self::SymbolImpact => SemanticCapability::SymbolImpact,
            Self::TypeCoupling => SemanticCapability::TypeCoupling,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticResponse {
    protocol_version: u32,
    operation: String,
    sidecar_version: String,
    backend: String,
    backend_version: String,
    selected_tsconfigs: Vec<String>,
    projects: Vec<SemanticProjectResponse>,
    results: Vec<SemanticQueryResponse>,
    phase_timings_ms: SemanticPhaseTimings,
    warnings: Vec<String>,
    elapsed_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticProjectResponse {
    config: String,
    source: TypeAwareProjectSource,
    status: TypeAwareProjectStatus,
    reason_code: Option<SemanticGapReason>,
    blocking_diagnostic_count: usize,
    source_file_count: usize,
    program_reused: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticQueryResponse {
    query_id: usize,
    operation: SemanticOperation,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticPhaseTimings {
    project_setup: u64,
    diagnostics: u64,
    semantic_queries: u64,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiSurfaceEntryData {
    exposed: SemanticSymbol,
    origin: SemanticSymbol,
    signature_fingerprint: String,
    referenced_types: Vec<PublicTypeReferenceData>,
    total_referenced_type_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicTypeReferenceData {
    declaration: SemanticSymbol,
    relation: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiLeakData {
    exposed_symbol: SemanticSymbol,
    private_declaration: SemanticSymbol,
    relation: String,
    evidence: EvidenceLocation,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceLocation {
    path: PathBuf,
    line: u32,
    col: u32,
}

#[derive(Debug, Deserialize)]
struct SemanticReferenceData {
    path: PathBuf,
    line: u32,
    col: u32,
    role: String,
    #[serde(default)]
    namespace: Option<SemanticNamespace>,
    #[serde(default)]
    via: Vec<SemanticAliasHop>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SymbolTraceData {
    symbol: SemanticSymbol,
    selected_project: String,
    alias_hops: Vec<serde_json::Value>,
    total_alias_hop_count: usize,
    checker_evidence_count: usize,
    graph_evidence_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SymbolImpactData {
    symbol: SemanticSymbol,
    selected_project: String,
    direct_consumers: Vec<DirectConsumerData>,
    total_direct_consumer_count: usize,
    transitive_affected_files: Vec<ImpactPathData>,
    total_transitive_affected_file_count: usize,
    targeted_tests: Vec<ImpactPathData>,
    total_targeted_test_count: usize,
    confidence: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectConsumerData {
    path: PathBuf,
    namespace: SemanticNamespace,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImpactPathData {
    path: PathBuf,
    provenance: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeCouplingData {
    scope: String,
    direction: String,
    project_size: usize,
    files_analyzed: usize,
    distinct_coupled_files: usize,
    edge_count: usize,
    coupled_file_percentage: Option<f64>,
    p50_distinct_connections: f64,
    p90_distinct_connections: f64,
    p95_public_api_depends_on: f64,
    p95_public_types_used_by: f64,
    high_coupling_percentage: Option<f64>,
    concentration: f64,
    files: Vec<CoupledFileData>,
    total_file_count: usize,
    top_contributors: Vec<CoupledFileData>,
    edges: Vec<TypeCouplingEdgeData>,
    cycles: Vec<Vec<PathBuf>>,
    total_cycle_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoupledFileData {
    path: PathBuf,
    outgoing_label: String,
    outgoing_files: Vec<PathBuf>,
    total_outgoing_file_count: usize,
    incoming_label: String,
    incoming_files: Vec<PathBuf>,
    total_incoming_file_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeCouplingEdgeData {
    source: SemanticSymbol,
    target: SemanticSymbol,
    relation: String,
    evidence: EvidenceLocation,
}

enum QueryTarget {
    ClassMember(usize),
    UnusedExport(usize),
    UnusedType(usize),
    ApiSurface,
    TypeCoupling,
}

/// Result of applying a semantic batch to dead-code findings.
pub struct SemanticDeadCodeOutcome {
    /// Standard metadata consumed by all report surfaces.
    pub type_aware: TypeAwareOutcome,
    /// Advisory coupling result when the caller requested the combined batch.
    pub type_coupling: Option<TypeCouplingReport>,
}

/// Coupling report plus shared metadata for report and integration surfaces.
pub struct SemanticCouplingOutcome {
    pub type_aware: TypeAwareOutcome,
    pub report: TypeCouplingReport,
}

/// Merge a second semantic capability batch into one run-level metadata block.
/// This preserves the conservative completeness state and keeps every query
/// visible to report, audit, SARIF, MCP, and editor consumers.
pub fn merge_type_aware_meta(base: &mut TypeAwareMeta, overlay: TypeAwareMeta) {
    base.queries.extend(overlay.queries);
    base.queries.sort_by_key(|query| query.query_id);
    base.symbol_traces.extend(overlay.symbol_traces);
    base.symbol_impacts.extend(overlay.symbol_impacts);
    if overlay.api_surface.is_some() {
        base.api_surface = overlay.api_surface;
    }
    if overlay.type_coupling.is_some() {
        base.type_coupling = overlay.type_coupling;
    }
    for project in overlay.selected_tsconfigs {
        if !base.selected_tsconfigs.contains(&project) {
            base.selected_tsconfigs.push(project);
        }
    }
    base.selected_tsconfigs.sort();
    base.warning_count += overlay.warning_count;
    base.warnings.extend(overlay.warnings);
    base.elapsed_ms += overlay.elapsed_ms;
    base.phase_timings_ms.project_setup += overlay.phase_timings_ms.project_setup;
    base.phase_timings_ms.diagnostics += overlay.phase_timings_ms.diagnostics;
    base.phase_timings_ms.symbol_scan += overlay.phase_timings_ms.symbol_scan;
    merge_semantic_identity(&mut base.identity, overlay.identity);
}

fn merge_semantic_identity(
    base: &mut Option<SemanticAnalysisIdentity>,
    overlay: Option<SemanticAnalysisIdentity>,
) {
    let Some(overlay) = overlay else {
        return;
    };
    let Some(base) = base.as_mut() else {
        *base = Some(overlay);
        return;
    };
    for capability in overlay.capabilities {
        if !base.capabilities.contains(&capability) {
            base.capabilities.push(capability);
        }
    }
    base.capabilities.sort();
    if semantic_completeness_rank(overlay.completeness)
        > semantic_completeness_rank(base.completeness)
    {
        base.completeness = overlay.completeness;
    }
}

const fn semantic_completeness_rank(completeness: SemanticCompleteness) -> u8 {
    match completeness {
        SemanticCompleteness::Complete => 0,
        SemanticCompleteness::Partial => 1,
        SemanticCompleteness::Unavailable => 2,
    }
}

/// Refine existing unused-symbol findings and enrich private type leaks in one
/// batched semantic pass.
#[expect(
    clippy::too_many_arguments,
    reason = "semantic refinement keeps requested capability families explicit"
)]
pub fn refine_dead_code(
    root: &Path,
    results: &mut AnalysisResults,
    projects: &[PathBuf],
    entry_points: &[PathBuf],
    include_symbol_use: bool,
    include_private_type_leaks: bool,
    include_type_coupling: bool,
    _require: TypeAwareRequire,
) -> Result<Option<SemanticDeadCodeOutcome>, TypeAwareError> {
    let canonical_root = root.canonicalize().map_err(|error| {
        TypeAwareError::from(format!(
            "failed to resolve project root {}: {error}",
            root.display()
        ))
    })?;
    let (queries, targets) = build_dead_code_queries(
        &canonical_root,
        results,
        entry_points,
        include_private_type_leaks,
        include_type_coupling,
    )?;
    let requested_capabilities = [
        include_symbol_use.then_some(SemanticCapability::SymbolUse),
        include_private_type_leaks.then_some(SemanticCapability::ApiSurface),
        include_type_coupling.then_some(SemanticCapability::TypeCoupling),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if requested_capabilities.is_empty() {
        return Ok(None);
    }
    if queries.is_empty() {
        return Ok(Some(empty_semantic_outcome(
            &canonical_root,
            projects,
            requested_capabilities,
        )));
    }
    let request = SemanticRequest {
        protocol_version: PROTOCOL_VERSION,
        operation: OPERATION,
        root: canonical_root.to_string_lossy().into_owned(),
        projects: projects
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        evidence_limit: EVIDENCE_LIMIT,
        queries,
    };
    let response: SemanticResponse =
        crate::type_aware::run_semantic_request(&canonical_root, &request)?;
    validate_response(&request, &response)?;
    apply_dead_code_response(
        &canonical_root,
        results,
        &request,
        &targets,
        response,
        requested_capabilities,
    )
    .map(Some)
}

fn empty_semantic_outcome(
    root: &Path,
    projects: &[PathBuf],
    capabilities: Vec<SemanticCapability>,
) -> SemanticDeadCodeOutcome {
    let configured_projects = identity_project_configs(root, projects);
    let identity = SemanticAnalysisIdentity {
        mode: SemanticAnalysisMode::TypeAware,
        semantic_schema_version: SEMANTIC_SCHEMA_VERSION,
        capabilities,
        project_config_hash: project_config_hash(root, &configured_projects),
        backend_family: BACKEND_FAMILY.to_string(),
        completeness: SemanticCompleteness::Complete,
    };
    SemanticDeadCodeOutcome {
        type_aware: TypeAwareOutcome {
            meta: TypeAwareMeta {
                identity: Some(identity),
                protocol_version: PROTOCOL_VERSION,
                sidecar_version: env!("CARGO_PKG_VERSION").to_string(),
                backend: BACKEND_FAMILY.to_string(),
                backend_version: BACKEND_VERSION.to_string(),
                selected_tsconfigs: Vec::new(),
                ..TypeAwareMeta::default()
            },
            warnings: Vec::new(),
        },
        type_coupling: None,
    }
}

/// Resolve exact checker-backed references for one exported symbol.
pub fn trace_symbol(
    root: &Path,
    projects: &[PathBuf],
    symbol: SemanticSymbol,
    require: TypeAwareRequire,
) -> Result<SemanticSymbolTrace, TypeAwareError> {
    let (request, response) = run_single_query(
        root,
        projects,
        SemanticQuery::SymbolTrace {
            id: 0,
            symbol: symbol.clone(),
        },
    )?;
    let result = response.results.first().ok_or_else(|| {
        TypeAwareError::from("type-aware trace response omitted its query result".to_string())
    })?;
    enforce_requirement(require, result)?;
    decode_symbol_trace(&response, &request, result, symbol)
}

fn decode_symbol_trace(
    response: &SemanticResponse,
    request: &SemanticRequest,
    result: &SemanticQueryResponse,
    symbol: SemanticSymbol,
) -> Result<SemanticSymbolTrace, TypeAwareError> {
    let identity = semantic_identity(response, request, result.status);
    if result.status == SemanticCompleteness::Unavailable {
        return Ok(SemanticSymbolTrace {
            target: symbol,
            identity,
            selected_project: response
                .selected_tsconfigs
                .first()
                .cloned()
                .unwrap_or_else(|| "<unavailable>".to_string()),
            assertion: result.assertion.clone(),
            status: result.status,
            references: Vec::new(),
            total_reference_count: 0,
            checker_evidence_count: 0,
            graph_evidence_count: 0,
            truncated: result.truncated,
            omissions: result.omissions.clone(),
            actions: result.actions.clone(),
        });
    }
    let data: SymbolTraceData = serde_json::from_value(result.data.clone()).map_err(|error| {
        TypeAwareError::from(format!("failed to decode type-aware symbol trace: {error}"))
    })?;
    if data.total_alias_hop_count < data.alias_hops.len()
        || result.total_evidence_count < data.checker_evidence_count
        || data.graph_evidence_count < data.alias_hops.len()
    {
        return Err(TypeAwareError::from(
            "type-aware symbol trace totals are smaller than returned evidence".to_string(),
        ));
    }
    let references = result
        .evidence
        .iter()
        .filter_map(|evidence| {
            serde_json::from_value::<SemanticReferenceData>(evidence.clone()).ok()
        })
        .filter_map(|evidence| {
            evidence.namespace.map(|namespace| SemanticReference {
                path: evidence.path,
                line: evidence.line,
                col: evidence.col,
                role: evidence.role,
                namespace,
                via: evidence.via,
            })
        })
        .collect();
    Ok(SemanticSymbolTrace {
        target: data.symbol,
        identity,
        selected_project: data.selected_project,
        assertion: result.assertion.clone(),
        status: result.status,
        references,
        total_reference_count: data.checker_evidence_count,
        checker_evidence_count: data.checker_evidence_count,
        graph_evidence_count: data.graph_evidence_count,
        truncated: result.truncated,
        omissions: result.omissions.clone(),
        actions: result.actions.clone(),
    })
}

/// Compute exact-symbol consumers and a bounded targeted-test recommendation.
pub fn symbol_impact(
    root: &Path,
    projects: &[PathBuf],
    symbol: SemanticSymbol,
    require: TypeAwareRequire,
) -> Result<SemanticSymbolImpact, TypeAwareError> {
    let (request, response) = run_single_query(
        root,
        projects,
        SemanticQuery::SymbolImpact {
            id: 0,
            symbol: symbol.clone(),
        },
    )?;
    let result = response.results.first().ok_or_else(|| {
        TypeAwareError::from("type-aware impact response omitted its query result".to_string())
    })?;
    enforce_requirement(require, result)?;
    decode_symbol_impact(&response, &request, result, symbol)
}

fn decode_symbol_impact(
    response: &SemanticResponse,
    request: &SemanticRequest,
    result: &SemanticQueryResponse,
    symbol: SemanticSymbol,
) -> Result<SemanticSymbolImpact, TypeAwareError> {
    let identity = semantic_identity(response, request, result.status);
    if result.status == SemanticCompleteness::Unavailable {
        return Ok(SemanticSymbolImpact {
            target: symbol,
            identity,
            selected_project: response
                .selected_tsconfigs
                .first()
                .cloned()
                .unwrap_or_else(|| "<unavailable>".to_string()),
            assertion: result.assertion.clone(),
            status: result.status,
            direct_consumers: Vec::new(),
            total_direct_consumer_count: 0,
            affected_files: Vec::new(),
            total_affected_file_count: 0,
            targeted_tests: Vec::new(),
            total_targeted_test_count: 0,
            confidence: "unavailable".to_string(),
            omissions: result.omissions.clone(),
            actions: result.actions.clone(),
        });
    }
    let data: SymbolImpactData = serde_json::from_value(result.data.clone()).map_err(|error| {
        TypeAwareError::from(format!(
            "failed to decode type-aware symbol impact: {error}"
        ))
    })?;
    if data.total_direct_consumer_count < data.direct_consumers.len()
        || data.total_transitive_affected_file_count < data.transitive_affected_files.len()
        || data.total_targeted_test_count < data.targeted_tests.len()
    {
        return Err(TypeAwareError::from(
            "type-aware symbol impact totals are smaller than returned paths".to_string(),
        ));
    }
    let direct_consumers = data
        .direct_consumers
        .into_iter()
        .map(|consumer| SemanticImpactPath {
            path: consumer.path,
            relation: format!("direct-{}-consumer", namespace_name(consumer.namespace)),
            distance: 1,
            via: Vec::new(),
        })
        .collect();
    let affected_files = impact_paths(data.transitive_affected_files, "transitive-consumer");
    let targeted_tests = impact_paths(data.targeted_tests, "targeted-test");
    Ok(SemanticSymbolImpact {
        target: data.symbol,
        identity,
        selected_project: data.selected_project,
        assertion: result.assertion.clone(),
        status: result.status,
        direct_consumers,
        total_direct_consumer_count: data.total_direct_consumer_count,
        affected_files,
        total_affected_file_count: data.total_transitive_affected_file_count,
        targeted_tests,
        total_targeted_test_count: data.total_targeted_test_count,
        confidence: data.confidence,
        omissions: result.omissions.clone(),
        actions: result.actions.clone(),
    })
}

pub struct SemanticInspectOutcome {
    pub trace: SemanticSymbolTrace,
    pub api_surface: ApiSurfaceResult,
    pub impact: SemanticSymbolImpact,
    pub type_aware: TypeAwareOutcome,
}

/// Resolve every semantic inspect section in one sidecar request and one Program per project.
pub fn inspect_symbol(
    root: &Path,
    projects: &[PathBuf],
    symbol: SemanticSymbol,
    entry_points: &[PathBuf],
) -> Result<SemanticInspectOutcome, TypeAwareError> {
    let canonical_root = root.canonicalize().map_err(|error| {
        TypeAwareError::from(format!(
            "failed to resolve project root {}: {error}",
            root.display()
        ))
    })?;
    let request = SemanticRequest {
        protocol_version: PROTOCOL_VERSION,
        operation: OPERATION,
        root: canonical_root.to_string_lossy().into_owned(),
        projects: projects
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        evidence_limit: EVIDENCE_LIMIT,
        queries: vec![
            SemanticQuery::SymbolTrace {
                id: 0,
                symbol: symbol.clone(),
            },
            SemanticQuery::ApiSurface {
                id: 1,
                entry_points: entry_points
                    .iter()
                    .map(|path| protocol_path(&canonical_root, path))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|path| path.to_string_lossy().replace('\\', "/"))
                    .collect(),
                include_cycles: false,
            },
            SemanticQuery::SymbolImpact {
                id: 2,
                symbol: symbol.clone(),
            },
        ],
    };
    let response: SemanticResponse =
        crate::type_aware::run_semantic_request(&canonical_root, &request)?;
    validate_response(&request, &response)?;
    let trace_result = response
        .results
        .iter()
        .find(|result| result.query_id == 0)
        .ok_or_else(|| TypeAwareError::from("semantic inspect omitted symbol trace".to_string()))?;
    let api_result = response
        .results
        .iter()
        .find(|result| result.query_id == 1)
        .ok_or_else(|| TypeAwareError::from("semantic inspect omitted API surface".to_string()))?;
    let impact_result = response
        .results
        .iter()
        .find(|result| result.query_id == 2)
        .ok_or_else(|| {
            TypeAwareError::from("semantic inspect omitted symbol impact".to_string())
        })?;
    let trace = decode_symbol_trace(&response, &request, trace_result, symbol.clone())?;
    let api_surface = decode_api_surface(api_result)?;
    let impact = decode_symbol_impact(&response, &request, impact_result, symbol)?;
    let type_aware = inspect_type_aware_outcome(&response, &request, &trace, &api_surface, &impact);
    Ok(SemanticInspectOutcome {
        trace,
        api_surface,
        impact,
        type_aware,
    })
}

fn inspect_type_aware_outcome(
    response: &SemanticResponse,
    request: &SemanticRequest,
    trace: &SemanticSymbolTrace,
    api_surface: &ApiSurfaceResult,
    impact: &SemanticSymbolImpact,
) -> TypeAwareOutcome {
    let completeness = aggregate_completeness(&response.results);
    let identity = semantic_identity(response, request, completeness);
    let projects = response
        .projects
        .iter()
        .map(|project| TypeAwareProjectMeta {
            config: project.config.clone(),
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
    let warnings = response.warnings.clone();
    TypeAwareOutcome {
        meta: TypeAwareMeta {
            identity: Some(identity),
            queries: response.results.iter().map(query_summary).collect(),
            symbol_traces: vec![trace.clone()],
            api_surface: Some(api_surface.clone()),
            symbol_impacts: vec![impact.clone()],
            type_coupling: None,
            protocol_version: response.protocol_version,
            sidecar_version: response.sidecar_version.clone(),
            backend: response.backend.clone(),
            backend_version: response.backend_version.clone(),
            selected_tsconfigs: response.selected_tsconfigs.clone(),
            candidate_count: 0,
            confirmed_used_count: 0,
            unresolved_count: 0,
            abstained_count: usize::from(completeness != SemanticCompleteness::Complete),
            abstention_reasons: TypeAwareAbstentionCounts::default(),
            projects,
            warning_count: warnings.len(),
            warnings: warnings.clone(),
            elapsed_ms: response.elapsed_ms,
            phase_timings_ms: TypeAwarePhaseTimings {
                project_setup: response.phase_timings_ms.project_setup,
                diagnostics: response.phase_timings_ms.diagnostics,
                symbol_scan: response.phase_timings_ms.semantic_queries,
            },
        },
        warnings,
    }
}

/// Analyze project-local coupling between package-public TypeScript signatures.
pub fn type_coupling(
    root: &Path,
    projects: &[PathBuf],
    entry_points: &[PathBuf],
    require: TypeAwareRequire,
) -> Result<SemanticCouplingOutcome, TypeAwareError> {
    let canonical_root = root.canonicalize().map_err(|error| {
        TypeAwareError::from(format!(
            "failed to resolve project root {}: {error}",
            root.display()
        ))
    })?;
    let entry_points = entry_points
        .iter()
        .map(|path| protocol_path(&canonical_root, path))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    let (request, response) = run_single_query(
        &canonical_root,
        projects,
        SemanticQuery::TypeCoupling {
            id: 0,
            entry_points,
            include_cycles: true,
        },
    )?;
    let result = response.results.first().ok_or_else(|| {
        TypeAwareError::from("type-aware coupling response omitted its query result".to_string())
    })?;
    enforce_requirement(require, result)?;
    let report = decode_type_coupling(&response, &request, result)?;
    let summary = query_summary(result);
    Ok(build_coupling_outcome(response, report, summary))
}

fn decode_type_coupling(
    response: &SemanticResponse,
    request: &SemanticRequest,
    result: &SemanticQueryResponse,
) -> Result<TypeCouplingReport, TypeAwareError> {
    let identity = semantic_identity(response, request, result.status);
    if result.status == SemanticCompleteness::Unavailable {
        return Ok(TypeCouplingReport {
            identity,
            assertion: result.assertion.clone(),
            status: result.status,
            summary: None,
            files: Vec::new(),
            top_contributors: Vec::new(),
            cycles: Vec::new(),
            omissions: result.omissions.clone(),
            actions: result.actions.clone(),
        });
    }
    let data: TypeCouplingData = serde_json::from_value(result.data.clone()).map_err(|error| {
        TypeAwareError::from(format!("failed to decode type-aware coupling: {error}"))
    })?;
    if data.total_file_count < data.files.len()
        || data.edge_count < data.edges.len()
        || data.total_cycle_count < data.cycles.len()
        || data.files.iter().chain(&data.top_contributors).any(|file| {
            file.total_outgoing_file_count < file.outgoing_files.len()
                || file.total_incoming_file_count < file.incoming_files.len()
                || file.outgoing_label != "public API depends on"
                || file.incoming_label != "public types used by"
        })
    {
        return Err(TypeAwareError::from(
            "type-aware coupling totals are smaller than returned evidence".to_string(),
        ));
    }
    let edges = data
        .edges
        .iter()
        .map(|edge| TypeCouplingEdge {
            source: edge.source.clone(),
            target: edge.target.clone(),
            relation: edge.relation.clone(),
            evidence: SemanticSourceLocation {
                path: edge.evidence.path.clone(),
                line: edge.evidence.line,
                col: edge.evidence.col,
            },
            scope: data.scope.clone(),
        })
        .collect::<Vec<_>>();
    let decode_file = |file: CoupledFileData| TypeCouplingFile {
        edges: edges
            .iter()
            .filter(|edge| edge.source.path == file.path || edge.target.path == file.path)
            .cloned()
            .collect(),
        path: file.path,
        public_api_depends_on: file.total_outgoing_file_count,
        public_api_depends_on_files: file.outgoing_files,
        public_types_used_by: file.total_incoming_file_count,
        public_types_used_by_files: file.incoming_files,
    };
    let files = data.files.into_iter().map(&decode_file).collect();
    let top_contributors = data
        .top_contributors
        .into_iter()
        .map(&decode_file)
        .collect();
    let cycles = data
        .cycles
        .into_iter()
        .map(|files| TypeCouplingCycle { files })
        .collect();
    let summary = data.high_coupling_percentage.and_then(|high_coupling_pct| {
        data.coupled_file_percentage
            .map(|coupled_file_pct| TypeCouplingSummary {
                scope: data.scope,
                direction: data.direction,
                project_size: data.project_size,
                files_analyzed: data.files_analyzed,
                distinct_coupled_files: data.distinct_coupled_files,
                edge_count: data.edge_count,
                coupled_file_pct,
                p50_distinct_connections: data.p50_distinct_connections,
                p90_distinct_connections: data.p90_distinct_connections,
                p95_public_types_used_by: data.p95_public_types_used_by,
                p95_public_api_depends_on: data.p95_public_api_depends_on,
                high_coupling_pct,
                concentration: data.concentration,
                cycle_count: data.total_cycle_count,
            })
    });
    Ok(TypeCouplingReport {
        identity,
        assertion: result.assertion.clone(),
        status: result.status,
        summary,
        files,
        top_contributors,
        cycles,
        omissions: result.omissions.clone(),
        actions: result.actions.clone(),
    })
}

fn build_coupling_outcome(
    response: SemanticResponse,
    report: TypeCouplingReport,
    query: SemanticQuerySummary,
) -> SemanticCouplingOutcome {
    let warning_count = response.warnings.len();
    let warnings = response.warnings.clone();
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
    let type_aware = TypeAwareOutcome {
        meta: TypeAwareMeta {
            identity: Some(report.identity.clone()),
            queries: vec![query],
            symbol_traces: Vec::new(),
            api_surface: None,
            symbol_impacts: Vec::new(),
            type_coupling: Some(report.clone()),
            protocol_version: response.protocol_version,
            sidecar_version: response.sidecar_version,
            backend: response.backend,
            backend_version: response.backend_version,
            selected_tsconfigs: response.selected_tsconfigs,
            candidate_count: 0,
            confirmed_used_count: 0,
            unresolved_count: 0,
            abstained_count: usize::from(report.status != SemanticCompleteness::Complete),
            abstention_reasons: TypeAwareAbstentionCounts::default(),
            projects,
            warning_count,
            warnings: warnings.clone(),
            elapsed_ms: response.elapsed_ms,
            phase_timings_ms: TypeAwarePhaseTimings {
                project_setup: response.phase_timings_ms.project_setup,
                diagnostics: response.phase_timings_ms.diagnostics,
                symbol_scan: response.phase_timings_ms.semantic_queries,
            },
        },
        warnings,
    };
    SemanticCouplingOutcome { type_aware, report }
}

fn run_single_query(
    root: &Path,
    projects: &[PathBuf],
    query: SemanticQuery,
) -> Result<(SemanticRequest, SemanticResponse), TypeAwareError> {
    let canonical_root = root.canonicalize().map_err(|error| {
        TypeAwareError::from(format!(
            "failed to resolve project root {}: {error}",
            root.display()
        ))
    })?;
    let request = SemanticRequest {
        protocol_version: PROTOCOL_VERSION,
        operation: OPERATION,
        root: canonical_root.to_string_lossy().into_owned(),
        projects: projects
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        evidence_limit: EVIDENCE_LIMIT,
        queries: vec![query],
    };
    let response = crate::type_aware::run_semantic_request(&canonical_root, &request)?;
    validate_response(&request, &response)?;
    Ok((request, response))
}

fn enforce_requirement(
    require: TypeAwareRequire,
    result: &SemanticQueryResponse,
) -> Result<(), TypeAwareError> {
    if require == TypeAwareRequire::Complete && result.status != SemanticCompleteness::Complete {
        return Err(TypeAwareError::from(format!(
            "type-aware completeness is required, but {} was {:?}",
            result.assertion, result.status
        )));
    }
    Ok(())
}

fn semantic_identity(
    response: &SemanticResponse,
    request: &SemanticRequest,
    completeness: SemanticCompleteness,
) -> SemanticAnalysisIdentity {
    SemanticAnalysisIdentity {
        mode: SemanticAnalysisMode::TypeAware,
        semantic_schema_version: SEMANTIC_SCHEMA_VERSION,
        capabilities: request
            .queries
            .iter()
            .map(|query| query.operation().capability())
            .collect(),
        project_config_hash: project_config_hash(
            Path::new(&request.root),
            &identity_project_config_strings(Path::new(&request.root), &request.projects),
        ),
        backend_family: response.backend.clone(),
        completeness,
    }
}

fn impact_paths(paths: Vec<ImpactPathData>, relation: &str) -> Vec<SemanticImpactPath> {
    paths
        .into_iter()
        .map(|path| SemanticImpactPath {
            distance: path.provenance.len().max(1),
            via: path.provenance,
            path: path.path,
            relation: relation.to_string(),
        })
        .collect()
}

const fn namespace_name(namespace: SemanticNamespace) -> &'static str {
    match namespace {
        SemanticNamespace::Value => "value",
        SemanticNamespace::Type => "type",
    }
}

fn build_dead_code_queries(
    root: &Path,
    results: &AnalysisResults,
    entry_points: &[PathBuf],
    include_private_type_leaks: bool,
    include_type_coupling: bool,
) -> Result<(Vec<SemanticQuery>, BTreeMap<usize, QueryTarget>), TypeAwareError> {
    let mut queries = Vec::new();
    let mut targets = BTreeMap::new();
    for (index, finding) in results.unused_class_members.iter().enumerate() {
        let id = queries.len();
        queries.push(SemanticQuery::SymbolUse {
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
        targets.insert(id, QueryTarget::ClassMember(index));
    }
    for (index, finding) in results.unused_exports.iter().enumerate() {
        let id = queries.len();
        queries.push(SemanticQuery::SymbolUse {
            id,
            symbol: export_symbol(root, &finding.export, SemanticNamespace::Value)?,
        });
        targets.insert(id, QueryTarget::UnusedExport(index));
    }
    for (index, finding) in results.unused_types.iter().enumerate() {
        let id = queries.len();
        queries.push(SemanticQuery::SymbolUse {
            id,
            symbol: export_symbol(root, &finding.export, SemanticNamespace::Type)?,
        });
        targets.insert(id, QueryTarget::UnusedType(index));
    }
    if include_private_type_leaks && !entry_points.is_empty() {
        let id = queries.len();
        queries.push(SemanticQuery::ApiSurface {
            id,
            entry_points: entry_points
                .iter()
                .map(|path| protocol_path(root, path))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect(),
            include_cycles: false,
        });
        targets.insert(id, QueryTarget::ApiSurface);
    }
    if include_type_coupling {
        let id = queries.len();
        queries.push(SemanticQuery::TypeCoupling {
            id,
            entry_points: entry_points
                .iter()
                .map(|path| protocol_path(root, path))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect(),
            include_cycles: true,
        });
        targets.insert(id, QueryTarget::TypeCoupling);
    }
    Ok((queries, targets))
}

fn export_symbol(
    root: &Path,
    export: &fallow_types::results::UnusedExport,
    namespace: SemanticNamespace,
) -> Result<SemanticSymbol, TypeAwareError> {
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

const fn member_kind_name(kind: MemberKind) -> &'static str {
    match kind {
        MemberKind::ClassMethod => "class_method",
        MemberKind::ClassProperty => "class_property",
        MemberKind::EnumMember => "enum_member",
        MemberKind::NamespaceMember => "namespace_member",
        MemberKind::StoreMember => "store_member",
    }
}

fn protocol_path(root: &Path, path: &Path) -> Result<PathBuf, TypeAwareError> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root).map_err(|_| {
            TypeAwareError::from(format!(
                "type-aware path {} is outside project root {}",
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
        return Err(TypeAwareError::from(format!(
            "type-aware path {} is not project-relative",
            path.display()
        )));
    }
    Ok(relative.to_path_buf())
}

fn validate_response(
    request: &SemanticRequest,
    response: &SemanticResponse,
) -> Result<(), TypeAwareError> {
    if response.protocol_version != PROTOCOL_VERSION || response.operation != OPERATION {
        return Err(TypeAwareError::from(format!(
            "type-aware protocol mismatch: expected {PROTOCOL_VERSION}/{OPERATION}, received {}/{}",
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
    if response.backend != BACKEND_FAMILY {
        return Err(TypeAwareError::from(format!(
            "unsupported type-aware backend {}",
            response.backend
        )));
    }
    if response.backend_version != BACKEND_VERSION {
        return Err(TypeAwareError::from(format!(
            "unsupported type-aware backend version {}; expected {BACKEND_VERSION}",
            response.backend_version
        )));
    }
    let expected = request
        .queries
        .iter()
        .map(|query| (query.id(), query.operation()))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    for result in &response.results {
        let Some(operation) = expected.get(&result.query_id) else {
            return Err(TypeAwareError::from(format!(
                "type-aware response returned unknown query id {}",
                result.query_id
            )));
        };
        if *operation != result.operation || !seen.insert(result.query_id) {
            return Err(TypeAwareError::from(format!(
                "type-aware response returned a duplicate or mismatched query id {}",
                result.query_id
            )));
        }
        if result.total_evidence_count < result.evidence.len() {
            return Err(TypeAwareError::from(format!(
                "type-aware query {} reported fewer total evidence items than it returned",
                result.query_id
            )));
        }
        if result.status != SemanticCompleteness::Complete
            && (result.reason_code.is_none()
                || result.actions.is_empty()
                || result.omissions.is_empty())
        {
            return Err(TypeAwareError::from(format!(
                "type-aware query {} omitted the reason, action, or omission required for an incomplete result",
                result.query_id
            )));
        }
    }
    if seen.len() != expected.len() {
        return Err(TypeAwareError::from(
            "type-aware response did not classify every query".to_string(),
        ));
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "semantic response validation and conservative refinement stay in one transaction"
)]
fn apply_dead_code_response(
    root: &Path,
    results: &mut AnalysisResults,
    request: &SemanticRequest,
    targets: &BTreeMap<usize, QueryTarget>,
    response: SemanticResponse,
    requested_capabilities: Vec<SemanticCapability>,
) -> Result<SemanticDeadCodeOutcome, TypeAwareError> {
    let mut confirmed_class = BTreeSet::new();
    let mut confirmed_exports = BTreeSet::new();
    let mut confirmed_types = BTreeSet::new();
    let mut api_surface = None;
    let mut type_coupling = None;
    let mut query_summaries = Vec::new();
    let mut confirmed_used_count = 0;
    let mut unresolved_count = 0;
    let mut abstained_count = 0;
    let mut abstention_reasons = TypeAwareAbstentionCounts::default();

    for result in &response.results {
        let Some(target) = targets.get(&result.query_id) else {
            continue;
        };
        query_summaries.push(query_summary(result));
        match target {
            QueryTarget::ClassMember(index) => classify_symbol_use(
                result,
                *index,
                &mut confirmed_class,
                &mut confirmed_used_count,
                &mut unresolved_count,
                &mut abstained_count,
                &mut abstention_reasons,
            ),
            QueryTarget::UnusedExport(index) => classify_symbol_use(
                result,
                *index,
                &mut confirmed_exports,
                &mut confirmed_used_count,
                &mut unresolved_count,
                &mut abstained_count,
                &mut abstention_reasons,
            ),
            QueryTarget::UnusedType(index) => classify_symbol_use(
                result,
                *index,
                &mut confirmed_types,
                &mut confirmed_used_count,
                &mut unresolved_count,
                &mut abstained_count,
                &mut abstention_reasons,
            ),
            QueryTarget::ApiSurface => {
                api_surface = Some(apply_api_surface(root, results, result)?);
            }
            QueryTarget::TypeCoupling => {
                type_coupling = Some(decode_type_coupling(&response, request, result)?);
            }
        }
    }

    retain_unconfirmed(&mut results.unused_class_members, &confirmed_class);
    retain_unconfirmed(&mut results.unused_exports, &confirmed_exports);
    retain_unconfirmed(&mut results.unused_types, &confirmed_types);

    let completeness = aggregate_completeness(&response.results);
    let capabilities = requested_capabilities
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let candidate_count = request
        .queries
        .iter()
        .filter(|query| matches!(query, SemanticQuery::SymbolUse { .. }))
        .count();
    let warning_count = response.warnings.len();
    let warnings = response.warnings.clone();
    let identity = SemanticAnalysisIdentity {
        mode: SemanticAnalysisMode::TypeAware,
        semantic_schema_version: SEMANTIC_SCHEMA_VERSION,
        capabilities,
        project_config_hash: project_config_hash(
            root,
            &identity_project_config_strings(root, &request.projects),
        ),
        backend_family: response.backend.clone(),
        completeness,
    };
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
            abstained_count: 0,
            blocking_diagnostic_count: project.blocking_diagnostic_count,
            source_file_count: project.source_file_count,
            program_reused: Some(project.program_reused),
            reason_code: project.reason_code,
            abstain_reason: None,
        })
        .collect();
    let type_aware = TypeAwareOutcome {
        meta: TypeAwareMeta {
            identity: Some(identity),
            queries: query_summaries,
            symbol_traces: Vec::new(),
            api_surface,
            symbol_impacts: Vec::new(),
            type_coupling: type_coupling.clone(),
            protocol_version: response.protocol_version,
            sidecar_version: response.sidecar_version,
            backend: response.backend,
            backend_version: response.backend_version,
            selected_tsconfigs: response.selected_tsconfigs,
            candidate_count,
            confirmed_used_count,
            unresolved_count,
            abstained_count,
            abstention_reasons,
            projects,
            warning_count,
            warnings: warnings.clone(),
            elapsed_ms: response.elapsed_ms,
            phase_timings_ms: TypeAwarePhaseTimings {
                project_setup: response.phase_timings_ms.project_setup,
                diagnostics: response.phase_timings_ms.diagnostics,
                symbol_scan: response.phase_timings_ms.semantic_queries,
            },
        },
        warnings,
    };
    Ok(SemanticDeadCodeOutcome {
        type_aware,
        type_coupling,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "keeps per-category refinement accounting explicit"
)]
fn classify_symbol_use(
    result: &SemanticQueryResponse,
    index: usize,
    confirmed: &mut BTreeSet<usize>,
    confirmed_used_count: &mut usize,
    unresolved_count: &mut usize,
    abstained_count: &mut usize,
    abstention_reasons: &mut TypeAwareAbstentionCounts,
) {
    if result.assertion == "confirmed-used" {
        confirmed.insert(index);
        *confirmed_used_count += 1;
    } else if result.status == SemanticCompleteness::Complete {
        *unresolved_count += 1;
    } else {
        *abstained_count += 1;
        match result.reason_code {
            Some(SemanticGapReason::NoProject) => abstention_reasons.no_project += 1,
            Some(SemanticGapReason::BlockingDiagnostics) => {
                abstention_reasons.blocking_diagnostics += 1;
            }
            _ => abstention_reasons.ambiguous_project += 1,
        }
    }
}

fn retain_unconfirmed<T>(items: &mut Vec<T>, confirmed: &BTreeSet<usize>) {
    let mut index = 0_usize;
    items.retain(|_| {
        let keep = !confirmed.contains(&index);
        index += 1;
        keep
    });
}

fn query_summary(result: &SemanticQueryResponse) -> SemanticQuerySummary {
    SemanticQuerySummary {
        query_id: result.query_id,
        capability: result.operation.capability(),
        assertion: result.assertion.clone(),
        status: result.status,
        reason_code: result.reason_code,
        total_evidence_count: result.total_evidence_count,
        truncated: result.truncated,
        omissions: result.omissions.clone(),
        actions: result.actions.clone(),
    }
}

fn aggregate_completeness(results: &[SemanticQueryResponse]) -> SemanticCompleteness {
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

fn apply_api_surface(
    root: &Path,
    results: &mut AnalysisResults,
    result: &SemanticQueryResponse,
) -> Result<ApiSurfaceResult, TypeAwareError> {
    let data: ApiSurfaceData = serde_json::from_value(result.data.clone()).map_err(|error| {
        TypeAwareError::from(format!("failed to decode type-aware API surface: {error}"))
    })?;
    validate_api_surface_data(&data)?;
    let api_surface = api_surface_result(result, data.clone());
    for (wire, semantic) in data.leaks.iter().zip(&api_surface.private_type_leaks) {
        attach_or_add_private_type_leak(root, results, wire, semantic.clone());
    }
    Ok(api_surface)
}

fn decode_api_surface(result: &SemanticQueryResponse) -> Result<ApiSurfaceResult, TypeAwareError> {
    let data: ApiSurfaceData = serde_json::from_value(result.data.clone()).map_err(|error| {
        TypeAwareError::from(format!("failed to decode type-aware API surface: {error}"))
    })?;
    validate_api_surface_data(&data)?;
    Ok(api_surface_result(result, data))
}

fn validate_api_surface_data(data: &ApiSurfaceData) -> Result<(), TypeAwareError> {
    if data.total_export_count < data.exports.len()
        || data.total_entry_count < data.entries.len()
        || data.total_leak_count < data.leaks.len()
        || data.total_public_signature_edge_count < data.public_signature_edges.len()
        || data
            .entries
            .iter()
            .any(|entry| entry.total_referenced_type_count < entry.referenced_types.len())
    {
        return Err(TypeAwareError::from(
            "type-aware API surface totals are smaller than their returned arrays".to_string(),
        ));
    }
    Ok(())
}

fn api_surface_result(result: &SemanticQueryResponse, data: ApiSurfaceData) -> ApiSurfaceResult {
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
    ApiSurfaceResult {
        assertion: result.assertion.clone(),
        status: result.status,
        entries,
        private_type_leaks: semantic_leaks,
        omissions: result.omissions.clone(),
        actions: result.actions.clone(),
    }
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

fn identity_project_configs(root: &Path, projects: &[PathBuf]) -> Vec<String> {
    let configured = projects
        .iter()
        .map(|project| project.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    identity_project_config_strings(root, &configured)
}

fn identity_project_config_strings(root: &Path, projects: &[String]) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn symbol() -> SemanticSymbol {
        SemanticSymbol {
            path: PathBuf::from("src/config.ts"),
            namespace: SemanticNamespace::Value,
            declaration_kind: "function".to_string(),
            exported_name: "defineConfig".to_string(),
            local_name: "defineConfig".to_string(),
            owner: None,
            line: 4,
            col: 0,
        }
    }

    #[test]
    fn partial_symbol_impact_preserves_valid_consumers_and_targeted_tests() {
        let target = symbol();
        let request = SemanticRequest {
            protocol_version: PROTOCOL_VERSION,
            operation: OPERATION,
            root: ".".to_string(),
            projects: vec!["tsconfig.json".to_string()],
            evidence_limit: EVIDENCE_LIMIT,
            queries: vec![SemanticQuery::SymbolImpact {
                id: 0,
                symbol: target.clone(),
            }],
        };
        let result = SemanticQueryResponse {
            query_id: 0,
            operation: SemanticOperation::SymbolImpact,
            assertion: "consumers-found".to_string(),
            status: SemanticCompleteness::Partial,
            reason_code: Some(SemanticGapReason::DynamicBehavior),
            actions: vec!["Review dynamic consumers.".to_string()],
            evidence: Vec::new(),
            total_evidence_count: 2,
            truncated: false,
            omissions: vec![SemanticOmission {
                reason_code: SemanticGapReason::DynamicBehavior,
                count: 1,
            }],
            data: json!({
                "symbol": target,
                "selected_project": "tsconfig.json",
                "direct_consumers": [{
                    "path": "src/consumer.ts",
                    "namespace": "value"
                }],
                "total_direct_consumer_count": 1,
                "transitive_affected_files": [{
                    "path": "src/transitive.ts",
                    "provenance": ["src/consumer.ts", "src/transitive.ts"]
                }],
                "total_transitive_affected_file_count": 1,
                "targeted_tests": [{
                    "path": "test/config.test.ts",
                    "provenance": ["src/consumer.ts", "test/config.test.ts"]
                }],
                "total_targeted_test_count": 1,
                "confidence": "bounded"
            }),
        };
        let response = SemanticResponse {
            protocol_version: PROTOCOL_VERSION,
            operation: OPERATION.to_string(),
            sidecar_version: env!("CARGO_PKG_VERSION").to_string(),
            backend: BACKEND_FAMILY.to_string(),
            backend_version: BACKEND_VERSION.to_string(),
            selected_tsconfigs: vec!["tsconfig.json".to_string()],
            projects: Vec::new(),
            results: vec![result.clone()],
            phase_timings_ms: SemanticPhaseTimings {
                project_setup: 1,
                diagnostics: 1,
                semantic_queries: 1,
            },
            warnings: Vec::new(),
            elapsed_ms: 3,
        };

        let impact =
            decode_symbol_impact(&response, &request, &result, symbol()).expect("valid impact");

        assert_eq!(impact.status, SemanticCompleteness::Partial);
        assert_eq!(impact.total_direct_consumer_count, 1);
        assert_eq!(
            impact.direct_consumers[0].path,
            PathBuf::from("src/consumer.ts")
        );
        assert_eq!(impact.total_affected_file_count, 1);
        assert_eq!(impact.total_targeted_test_count, 1);
        assert_eq!(
            impact.targeted_tests[0].path,
            PathBuf::from("test/config.test.ts")
        );
        assert_eq!(impact.confidence, "bounded");
        assert!(enforce_requirement(TypeAwareRequire::BestEffort, &result).is_ok());
        assert!(enforce_requirement(TypeAwareRequire::Complete, &result).is_err());
    }
}
