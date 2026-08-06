//! Typed client for the batched type-aware semantic protocol.

mod queries;
mod reconcile;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use fallow_types::envelope::{
    TypeAwareAbstentionCounts, TypeAwareInvalidationKind, TypeAwareMeta, TypeAwarePhaseTimings,
    TypeAwareProjectMeta, TypeAwareProjectSource, TypeAwareProjectStatus,
};
use fallow_types::extract::MemberKind;
use fallow_types::output_dead_code::PrivateTypeLeakFinding;
use fallow_types::results::{AnalysisResults, PrivateTypeLeak};
use fallow_types::semantic::{
    ApiSurfaceEntry, ApiSurfaceResult, DEFERRED_PROJECT_CONFIG_HASH, PublicTypeReference,
    SemanticAliasHop, SemanticAnalysisIdentity, SemanticAnalysisMode, SemanticCandidateDecision,
    SemanticCandidateDecisionKind, SemanticCapability, SemanticCompleteness,
    SemanticContractEvidence, SemanticContractRelation, SemanticEditGuard,
    SemanticFrameworkContract, SemanticFrameworkContractEvidence, SemanticFrameworkRelation,
    SemanticGapReason, SemanticImpactConfidence, SemanticImpactPath, SemanticNamespace,
    SemanticOmission, SemanticPrivateTypeLeak, SemanticQuerySummary, SemanticReference,
    SemanticSourceLocation, SemanticSymbol, SemanticSymbolImpact, SemanticSymbolTrace,
    TypeCouplingCycle, TypeCouplingEdge, TypeCouplingFile, TypeCouplingReport, TypeCouplingSummary,
};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::manifest::{
    ANALYSIS_OPERATION as OPERATION, BACKEND_FAMILY, BACKEND_VERSION, QUERY_OPERATIONS,
    SEMANTIC_SCHEMA_VERSION, WIRE_PROTOCOL_VERSION as PROTOCOL_VERSION,
};
use super::transport::{TypeAwareError, TypeAwareOutcome};
use queries::{build_dead_code_queries, query_supports_guarded_fix};
use reconcile::apply_dead_code_response;
#[cfg(test)]
use reconcile::semantic_only_candidate_stays_hidden;

const EVIDENCE_LIMIT: usize = 40;
const MAX_SEMANTIC_QUERIES: usize = 25_000;
const MAX_PRIVATE_LEAK_CANDIDATES: usize = 25_000;

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
        framework_contracts: Vec<SemanticFrameworkContract>,
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
        private_leak_candidates: Vec<PrivateLeakCandidate>,
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

    const fn wire_name(self) -> &'static str {
        match self {
            Self::SymbolUse => "symbol-use",
            Self::SymbolTrace => "symbol-trace",
            Self::ApiSurface => "api-surface",
            Self::SymbolImpact => "symbol-impact",
            Self::TypeCoupling => "type-coupling",
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
    effective_config_hash: String,
    source: TypeAwareProjectSource,
    status: TypeAwareProjectStatus,
    candidate_count: usize,
    confirmed_used_count: usize,
    contract_preserved_count: usize,
    no_static_references_count: usize,
    fix_eligible_count: usize,
    unresolved_count: usize,
    abstained_count: usize,
    reason_code: Option<SemanticGapReason>,
    blocking_diagnostic_count: usize,
    source_file_count: usize,
    program_reused: bool,
    #[serde(default)]
    program_reused_from_previous_snapshot: Option<bool>,
    #[serde(default)]
    snapshot_revision: Option<u64>,
    #[serde(default)]
    invalidation_kind: Option<TypeAwareInvalidationKind>,
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
    private_leak_confirmation: PrivateLeakConfirmation,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct PrivateLeakCandidate {
    id: usize,
    path: String,
    export_name: String,
    type_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateLeakConfirmation {
    requested_candidate_count: usize,
    confirmation_complete: bool,
    confirmed_candidate_ids: Vec<usize>,
}

#[derive(Default)]
struct ApiSurfaceMutation {
    retention: Option<PrivateLeakRetention>,
    upserts: Vec<PrivateLeakUpsert>,
}

struct PrivateLeakRetention {
    requested_candidate_count: usize,
    confirmed_candidate_ids: BTreeSet<usize>,
}

struct PrivateLeakUpsert {
    evidence_path: PathBuf,
    export_name: String,
    type_name: String,
    line: u32,
    col: u32,
    semantic: SemanticPrivateTypeLeak,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceLocation {
    path: PathBuf,
    line: u32,
    col: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticReferenceData {
    path: PathBuf,
    line: u32,
    col: u32,
    role: String,
    source: String,
    #[serde(default)]
    namespace: Option<SemanticNamespace>,
    #[serde(default)]
    via: Vec<SemanticAliasHop>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SymbolUseData {
    symbol: SemanticSymbol,
    selected_project: String,
    owning_projects: Vec<String>,
    total_reference_count: usize,
    contract_relations: Vec<SemanticContractEvidence>,
    framework_contract_relations: Vec<SemanticFrameworkContractEvidence>,
    closed_world_eligible: bool,
    edit_guard: SemanticEditGuard,
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
    confidence: SemanticImpactConfidence,
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
    ApiSurface { unrequested_candidate_count: usize },
    TypeCoupling,
}

#[derive(Debug, Clone, Copy, Default)]
struct LocalCapacity {
    unrequested_symbol_count: usize,
}

struct DeadCodeQueryBatch {
    queries: Vec<SemanticQuery>,
    targets: BTreeMap<usize, QueryTarget>,
    capacity: LocalCapacity,
}

#[derive(Clone, Copy)]
struct DeadCodeReconciliation<'a> {
    request: &'a SemanticRequest,
    targets: &'a BTreeMap<usize, QueryTarget>,
    capacity: LocalCapacity,
}

#[derive(Debug, Default)]
struct CandidateDecisionStats {
    confirmed_used: usize,
    contract_preserved: usize,
    no_static_references: usize,
    fix_eligible: usize,
    unresolved: usize,
    abstained: usize,
    abstention_reasons: TypeAwareAbstentionCounts,
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
    /// Standard metadata consumed by all report surfaces.
    pub type_aware: TypeAwareOutcome,
    /// The type-coupling analysis report.
    pub report: TypeCouplingReport,
}

/// Merge a second semantic capability batch into one run-level metadata block.
/// This preserves the conservative completeness state and keeps every query
/// visible to report, audit, SARIF, MCP, and editor consumers.
pub fn merge_type_aware_meta(base: &mut TypeAwareMeta, overlay: TypeAwareMeta) {
    if overlay.executed {
        base.executed = true;
        base.protocol_version = overlay.protocol_version;
        base.sidecar_version.clone_from(&overlay.sidecar_version);
        base.backend.clone_from(&overlay.backend);
        base.backend_version.clone_from(&overlay.backend_version);
    }
    base.queries.extend(overlay.queries);
    base.queries.sort_by_key(|query| query.query_id);
    base.candidate_decisions.extend(overlay.candidate_decisions);
    base.candidate_decisions.sort_by(|left, right| {
        left.subject
            .path
            .cmp(&right.subject.path)
            .then(left.subject.line.cmp(&right.subject.line))
            .then(left.subject.col.cmp(&right.subject.col))
            .then(left.query_id.cmp(&right.query_id))
    });
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
    base.candidate_count += overlay.candidate_count;
    base.confirmed_used_count += overlay.confirmed_used_count;
    base.contract_preserved_count += overlay.contract_preserved_count;
    base.no_static_references_count += overlay.no_static_references_count;
    base.fix_eligible_count += overlay.fix_eligible_count;
    base.unresolved_count += overlay.unresolved_count;
    base.abstained_count += overlay.abstained_count;
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
    if base.project_config_hash == DEFERRED_PROJECT_CONFIG_HASH
        && overlay.project_config_hash != DEFERRED_PROJECT_CONFIG_HASH
    {
        base.project_config_hash
            .clone_from(&overlay.project_config_hash);
    }
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
pub fn refine_dead_code_results(
    root: &Path,
    results: &mut AnalysisResults,
    projects: &[PathBuf],
    entry_points: &[PathBuf],
    include_symbol_use: bool,
    include_private_type_leaks: bool,
    include_type_coupling: bool,
) -> Result<Option<SemanticDeadCodeOutcome>, TypeAwareError> {
    let outcome = refine_dead_code_results_with_transport(
        root,
        results,
        projects,
        entry_points,
        include_symbol_use,
        include_private_type_leaks,
        include_type_coupling,
        SemanticRequestTransport::OneShot,
    );
    if outcome.is_err() {
        discard_unverified_semantic_candidates(results);
    }
    outcome
}

/// Refine dead-code results through an explicitly owned persistent sidecar.
#[expect(
    clippy::too_many_arguments,
    reason = "semantic refinement keeps requested capability families explicit"
)]
pub fn refine_dead_code_results_in_session(
    session: &mut super::transport::TypeAwareSession,
    changes: Option<&super::transport::TypeAwareFileChanges>,
    root: &Path,
    results: &mut AnalysisResults,
    projects: &[PathBuf],
    entry_points: &[PathBuf],
    include_symbol_use: bool,
    include_private_type_leaks: bool,
    include_type_coupling: bool,
) -> Result<Option<SemanticDeadCodeOutcome>, TypeAwareError> {
    let outcome = refine_dead_code_results_with_transport(
        root,
        results,
        projects,
        entry_points,
        include_symbol_use,
        include_private_type_leaks,
        include_type_coupling,
        SemanticRequestTransport::Session(session, changes),
    );
    if outcome.is_err() {
        discard_unverified_semantic_candidates(results);
    }
    outcome
}

/// Drop findings that only exist pending semantic verification, so a failed
/// type-aware pass never reports unverified candidates.
pub fn discard_unverified_semantic_candidates(results: &mut AnalysisResults) {
    results
        .unused_class_members
        .retain(|finding| !finding.semantic_only_candidate);
}

enum SemanticRequestTransport<'a> {
    OneShot,
    Session(
        &'a mut super::transport::TypeAwareSession,
        Option<&'a super::transport::TypeAwareFileChanges>,
    ),
}

#[expect(
    clippy::too_many_arguments,
    reason = "semantic refinement keeps requested capability families explicit"
)]
fn refine_dead_code_results_with_transport(
    root: &Path,
    results: &mut AnalysisResults,
    projects: &[PathBuf],
    entry_points: &[PathBuf],
    include_symbol_use: bool,
    include_private_type_leaks: bool,
    include_type_coupling: bool,
    transport: SemanticRequestTransport<'_>,
) -> Result<Option<SemanticDeadCodeOutcome>, TypeAwareError> {
    if !include_symbol_use {
        discard_unverified_semantic_candidates(results);
    }
    let canonical_root = dunce::canonicalize(root).map_err(|error| {
        TypeAwareError::from(format!(
            "failed to resolve project root {}: {error}",
            root.display()
        ))
    })?;
    let DeadCodeQueryBatch {
        queries,
        targets,
        capacity,
    } = build_dead_code_queries(
        &canonical_root,
        results,
        entry_points,
        include_symbol_use,
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
        discard_unverified_semantic_candidates(results);
        return Ok(None);
    }
    if queries.is_empty() {
        discard_unverified_semantic_candidates(results);
        return Ok(Some(empty_semantic_outcome(requested_capabilities)));
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
    let response: SemanticResponse = match transport {
        SemanticRequestTransport::OneShot => {
            super::transport::run_semantic_request(&canonical_root, &request)?
        }
        SemanticRequestTransport::Session(session, changes) => {
            session.run_semantic_request(&canonical_root, &request, changes)?
        }
    };
    validate_response(&request, &response)?;
    apply_dead_code_response(
        &canonical_root,
        results,
        DeadCodeReconciliation {
            request: &request,
            targets: &targets,
            capacity,
        },
        response,
        requested_capabilities,
    )
    .map(Some)
}

fn empty_semantic_outcome(mut capabilities: Vec<SemanticCapability>) -> SemanticDeadCodeOutcome {
    capabilities.sort_unstable();
    capabilities.dedup();
    let identity = SemanticAnalysisIdentity {
        mode: SemanticAnalysisMode::TypeAware,
        semantic_schema_version: SEMANTIC_SCHEMA_VERSION,
        capabilities,
        project_config_hash: DEFERRED_PROJECT_CONFIG_HASH.to_string(),
        backend_family: BACKEND_FAMILY.to_string(),
        completeness: SemanticCompleteness::Complete,
    };
    SemanticDeadCodeOutcome {
        type_aware: TypeAwareOutcome {
            meta: TypeAwareMeta {
                identity: Some(identity),
                executed: false,
                protocol_version: PROTOCOL_VERSION,
                sidecar_version: None,
                backend: BACKEND_FAMILY.to_string(),
                backend_version: None,
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
    validate_returned_symbol(&symbol, &data.symbol, result.query_id)?;
    if data.total_alias_hop_count < data.alias_hops.len()
        || result.total_evidence_count < data.checker_evidence_count
        || data.graph_evidence_count < data.alias_hops.len()
    {
        return Err(TypeAwareError::from(
            "type-aware symbol trace totals are smaller than returned evidence".to_string(),
        ));
    }
    let references = decode_symbol_trace_evidence(result)?;
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
            confidence: SemanticImpactConfidence::Unavailable,
            omissions: result.omissions.clone(),
            actions: result.actions.clone(),
        });
    }
    let data: SymbolImpactData = serde_json::from_value(result.data.clone()).map_err(|error| {
        TypeAwareError::from(format!(
            "failed to decode type-aware symbol impact: {error}"
        ))
    })?;
    validate_returned_symbol(&symbol, &data.symbol, result.query_id)?;
    for consumer in &data.direct_consumers {
        validate_semantic_response_path(&consumer.path, result.query_id)?;
    }
    for path in data
        .transitive_affected_files
        .iter()
        .chain(&data.targeted_tests)
    {
        validate_semantic_response_path(&path.path, result.query_id)?;
        for provenance in &path.provenance {
            validate_semantic_response_path(provenance, result.query_id)?;
        }
    }
    if data.confidence == SemanticImpactConfidence::Unavailable {
        return Err(TypeAwareError::from(format!(
            "type-aware query {} returned an unsupported impact confidence",
            result.query_id
        )));
    }
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

fn validate_returned_symbol(
    requested: &SemanticSymbol,
    returned: &SemanticSymbol,
    query_id: usize,
) -> Result<(), TypeAwareError> {
    if requested != returned {
        return Err(TypeAwareError::from(format!(
            "type-aware query {query_id} returned a different exact symbol identity"
        )));
    }
    Ok(())
}

/// Every semantic inspect section resolved for one symbol.
pub struct SemanticInspectOutcome {
    /// Reference trace for the symbol.
    pub trace: SemanticSymbolTrace,
    /// Package API surface and private-leak analysis around the symbol.
    pub api_surface: ApiSurfaceResult,
    /// Impact closure of changing the symbol.
    pub impact: SemanticSymbolImpact,
    /// Standard metadata consumed by all report surfaces.
    pub type_aware: TypeAwareOutcome,
}

/// Resolve every semantic inspect section in one sidecar request and one Program per project.
pub fn inspect_symbol(
    root: &Path,
    projects: &[PathBuf],
    symbol: SemanticSymbol,
    entry_points: &[PathBuf],
) -> Result<SemanticInspectOutcome, TypeAwareError> {
    let canonical_root = dunce::canonicalize(root).map_err(|error| {
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
                private_leak_candidates: Vec::new(),
            },
            SemanticQuery::SymbolImpact {
                id: 2,
                symbol: symbol.clone(),
            },
        ],
    };
    let response: SemanticResponse =
        super::transport::run_semantic_request(&canonical_root, &request)?;
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
    let api_query = request
        .queries
        .iter()
        .find(|query| query.id() == api_result.query_id)
        .ok_or_else(|| {
            TypeAwareError::from("semantic inspect omitted its API surface query".to_string())
        })?;
    let api_surface = decode_api_surface(api_query, api_result)?;
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
            contract_preserved_count: 0,
            no_static_references_count: 0,
            fix_eligible_count: 0,
            unresolved_count: 0,
            abstained_count: 0,
            blocking_diagnostic_count: project.blocking_diagnostic_count,
            source_file_count: project.source_file_count,
            program_reused: Some(project.program_reused),
            program_shared_across_queries: Some(project.program_reused),
            program_reused_from_previous_snapshot: project.program_reused_from_previous_snapshot,
            snapshot_revision: project.snapshot_revision,
            invalidation_kind: project.invalidation_kind,
            reason_code: project.reason_code,
            abstain_reason: None,
        })
        .collect();
    let warnings = response.warnings.clone();
    TypeAwareOutcome {
        meta: TypeAwareMeta {
            identity: Some(identity),
            required_completeness: None,
            queries: response.results.iter().map(query_summary).collect(),
            candidate_decisions: Vec::new(),
            symbol_traces: vec![trace.clone()],
            api_surface: Some(api_surface.clone()),
            symbol_impacts: vec![impact.clone()],
            type_coupling: None,
            executed: true,
            protocol_version: response.protocol_version,
            sidecar_version: Some(response.sidecar_version.clone()),
            backend: response.backend.clone(),
            backend_version: Some(response.backend_version.clone()),
            selected_tsconfigs: response.selected_tsconfigs.clone(),
            candidate_count: 0,
            confirmed_used_count: 0,
            contract_preserved_count: 0,
            no_static_references_count: 0,
            fix_eligible_count: 0,
            unresolved_count: 0,
            abstained_count: 0,
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
) -> Result<SemanticCouplingOutcome, TypeAwareError> {
    let canonical_root = dunce::canonicalize(root).map_err(|error| {
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
            contract_preserved_count: 0,
            no_static_references_count: 0,
            fix_eligible_count: 0,
            unresolved_count: 0,
            abstained_count: 0,
            blocking_diagnostic_count: project.blocking_diagnostic_count,
            source_file_count: project.source_file_count,
            program_reused: Some(project.program_reused),
            program_shared_across_queries: Some(project.program_reused),
            program_reused_from_previous_snapshot: project.program_reused_from_previous_snapshot,
            snapshot_revision: project.snapshot_revision,
            invalidation_kind: project.invalidation_kind,
            reason_code: project.reason_code,
            abstain_reason: None,
        })
        .collect();
    let type_aware = TypeAwareOutcome {
        meta: TypeAwareMeta {
            identity: Some(report.identity.clone()),
            required_completeness: None,
            queries: vec![query],
            candidate_decisions: Vec::new(),
            symbol_traces: Vec::new(),
            api_surface: None,
            symbol_impacts: Vec::new(),
            type_coupling: Some(report.clone()),
            executed: true,
            protocol_version: response.protocol_version,
            sidecar_version: Some(response.sidecar_version),
            backend: response.backend,
            backend_version: Some(response.backend_version),
            selected_tsconfigs: response.selected_tsconfigs,
            candidate_count: 0,
            confirmed_used_count: 0,
            contract_preserved_count: 0,
            no_static_references_count: 0,
            fix_eligible_count: 0,
            unresolved_count: 0,
            abstained_count: 0,
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
    let canonical_root = dunce::canonicalize(root).map_err(|error| {
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
    let response = super::transport::run_semantic_request(&canonical_root, &request)?;
    validate_response(&request, &response)?;
    Ok((request, response))
}

fn semantic_identity(
    response: &SemanticResponse,
    request: &SemanticRequest,
    completeness: SemanticCompleteness,
) -> SemanticAnalysisIdentity {
    let mut capabilities = request
        .queries
        .iter()
        .map(|query| query.operation().capability())
        .collect::<Vec<_>>();
    capabilities.sort_unstable();
    capabilities.dedup();
    SemanticAnalysisIdentity {
        mode: SemanticAnalysisMode::TypeAware,
        semantic_schema_version: SEMANTIC_SCHEMA_VERSION,
        capabilities,
        project_config_hash: response_project_config_hash(&response.projects),
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

fn protocol_path(root: &Path, path: &Path) -> Result<PathBuf, TypeAwareError> {
    let root = dunce::simplified(root);
    let path = dunce::simplified(path);
    let canonical: Option<(PathBuf, PathBuf)>;
    let relative = if path.is_absolute() {
        match path.strip_prefix(root) {
            Ok(relative) => relative,
            Err(_) => {
                // Symlinked prefixes (macOS temp dirs live in /var, a symlink
                // to /private/var) make one side canonical and the other not.
                // Canonicalize both sides before concluding the path escapes
                // the project root.
                canonical = dunce::canonicalize(path)
                    .ok()
                    .zip(dunce::canonicalize(root).ok());
                canonical
                    .as_ref()
                    .and_then(|(canonical_path, canonical_root)| {
                        canonical_path.strip_prefix(canonical_root).ok()
                    })
                    .ok_or_else(|| {
                        TypeAwareError::from(format!(
                            "type-aware path {} is outside project root {}",
                            path.display(),
                            root.display()
                        ))
                    })?
            }
        }
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
        if !QUERY_OPERATIONS.contains(&result.operation.wire_name()) {
            return Err(TypeAwareError::from(format!(
                "type-aware response returned operation {} outside the protocol manifest",
                result.operation.wire_name()
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

fn decode_candidate_decision(
    root: &Path,
    query: &SemanticQuery,
    result: &SemanticQueryResponse,
    fix_supported: bool,
    source_cache: &mut FxHashMap<PathBuf, Vec<u8>>,
) -> Result<SemanticCandidateDecision, TypeAwareError> {
    let SemanticQuery::SymbolUse {
        symbol: requested,
        framework_contracts,
        ..
    } = query
    else {
        return Err(TypeAwareError::from(
            "type-aware symbol-use result was paired with a different query operation".to_string(),
        ));
    };
    if result.status == SemanticCompleteness::Unavailable {
        return Ok(unavailable_candidate_decision(requested.clone(), result));
    }
    let data: SymbolUseData = serde_json::from_value(result.data.clone()).map_err(|error| {
        TypeAwareError::from(format!(
            "failed to decode type-aware symbol-use decision: {error}"
        ))
    })?;
    validate_symbol_use_data(
        SymbolUseValidation {
            root,
            requested,
            requested_framework_contracts: framework_contracts,
            result,
            fix_supported,
        },
        &data,
        source_cache,
    )?;
    let evidence = decode_symbol_use_evidence(result)?;
    let required_contract = data
        .contract_relations
        .iter()
        .find(|contract| !contract.optional)
        .cloned();
    let framework_contract = data.framework_contract_relations.first().cloned();
    let (expected_assertion, decision) = expected_candidate_decision(
        &data,
        required_contract.is_some() || framework_contract.is_some(),
        result.reason_code,
    );
    if result.assertion != expected_assertion {
        return Err(TypeAwareError::from(format!(
            "type-aware query {} assertion {} conflicts with its evidence",
            result.query_id, result.assertion
        )));
    }
    let closed_world_eligible = fix_supported
        && decision == SemanticCandidateDecisionKind::ConfirmedNoStaticReferences
        && data.closed_world_eligible;
    let subject = if fix_supported {
        data.symbol
    } else {
        requested.clone()
    };
    let contract = required_contract.or_else(|| data.contract_relations.first().cloned());
    let explanation = candidate_explanation(CandidateExplanationInput {
        subject: &subject,
        decision,
        fix_eligible: closed_world_eligible,
        owning_projects: &data.owning_projects,
        evidence: &evidence,
        contract: contract.as_ref(),
        framework_contract: framework_contract.as_ref(),
        reason_code: result.reason_code,
        total_evidence_count: result.total_evidence_count,
        truncated: result.truncated,
    });
    Ok(SemanticCandidateDecision {
        query_id: result.query_id,
        subject,
        decision,
        status: result.status,
        owning_projects: data.owning_projects,
        evidence,
        contract,
        framework_contract,
        closed_world_eligible,
        edit_guard: closed_world_eligible.then_some(data.edit_guard),
        reason_code: result.reason_code,
        explanation,
        actions: result.actions.clone(),
        total_evidence_count: result.total_evidence_count,
        truncated: result.truncated,
        omissions: result.omissions.clone(),
    })
}

fn unavailable_candidate_decision(
    subject: SemanticSymbol,
    result: &SemanticQueryResponse,
) -> SemanticCandidateDecision {
    let explanation = candidate_explanation(CandidateExplanationInput {
        subject: &subject,
        decision: SemanticCandidateDecisionKind::RetainedUnresolved,
        fix_eligible: false,
        owning_projects: &[],
        evidence: &[],
        contract: None,
        framework_contract: None,
        reason_code: result.reason_code,
        total_evidence_count: result.total_evidence_count,
        truncated: result.truncated,
    });
    SemanticCandidateDecision {
        query_id: result.query_id,
        subject,
        decision: SemanticCandidateDecisionKind::RetainedUnresolved,
        status: result.status,
        owning_projects: Vec::new(),
        evidence: Vec::new(),
        contract: None,
        framework_contract: None,
        closed_world_eligible: false,
        edit_guard: None,
        reason_code: result.reason_code,
        explanation,
        actions: result.actions.clone(),
        total_evidence_count: result.total_evidence_count,
        truncated: result.truncated,
        omissions: result.omissions.clone(),
    }
}

#[derive(Clone, Copy)]
struct SymbolUseValidation<'a> {
    root: &'a Path,
    requested: &'a SemanticSymbol,
    requested_framework_contracts: &'a [SemanticFrameworkContract],
    result: &'a SemanticQueryResponse,
    fix_supported: bool,
}

fn validate_symbol_use_data(
    validation: SymbolUseValidation<'_>,
    data: &SymbolUseData,
    source_cache: &mut FxHashMap<PathBuf, Vec<u8>>,
) -> Result<(), TypeAwareError> {
    let SymbolUseValidation {
        root,
        requested,
        requested_framework_contracts,
        result,
        fix_supported,
    } = validation;
    if data.total_reference_count != result.total_evidence_count {
        return Err(TypeAwareError::from(format!(
            "type-aware query {} returned inconsistent reference totals",
            result.query_id
        )));
    }
    if data.symbol.namespace != requested.namespace
        || data.symbol.exported_name != requested.exported_name
        || data.owning_projects.is_empty()
        || !data.owning_projects.contains(&data.selected_project)
        || !data
            .owning_projects
            .windows(2)
            .all(|items| items[0] < items[1])
    {
        return Err(TypeAwareError::from(format!(
            "type-aware query {} returned an inconsistent symbol or project identity",
            result.query_id
        )));
    }
    if fix_supported
        && (data.symbol.path != requested.path
            || data.symbol.declaration_kind != requested.declaration_kind
            || data.symbol.local_name != requested.local_name
            || data.symbol.owner != requested.owner
            || data.symbol.line != requested.line
            || data.symbol.col != requested.col)
    {
        return Err(TypeAwareError::from(format!(
            "type-aware query {} moved the exact class-member declaration identity",
            result.query_id
        )));
    }
    if data.closed_world_eligible
        && (result.status != SemanticCompleteness::Complete
            || result.truncated
            || !result.omissions.is_empty()
            || data.total_reference_count != 0
            || !data.contract_relations.is_empty()
            || !data.framework_contract_relations.is_empty())
    {
        return Err(TypeAwareError::from(format!(
            "type-aware query {} claimed unsafe closed-world eligibility",
            result.query_id
        )));
    }
    if data.framework_contract_relations.len() > 1
        || data.framework_contract_relations.iter().any(|evidence| {
            !requested_framework_contracts.iter().any(|contract| {
                contract.framework == evidence.framework
                    && contract.package == evidence.package
                    && contract.relation == evidence.relation
                    && contract.heritage_symbol == evidence.declaration.exported_name
                    && declaration_path_matches_package(
                        &evidence.declaration.path,
                        &contract.package,
                    )
            })
        })
    {
        return Err(TypeAwareError::from(format!(
            "type-aware query {} returned unrequested framework contract evidence",
            result.query_id
        )));
    }
    for contract in &data.contract_relations {
        validate_semantic_response_path(&contract.declaration.path, result.query_id)?;
    }
    for contract in &data.framework_contract_relations {
        validate_semantic_response_path(&contract.declaration.path, result.query_id)?;
    }
    if fix_supported {
        validate_edit_guard(
            root,
            &data.symbol,
            &data.edit_guard,
            result.query_id,
            source_cache,
        )?;
    }
    Ok(())
}

fn declaration_path_matches_package(path: &Path, package: &str) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let marker = format!("/node_modules/{package}/");
    format!("/{normalized}").contains(&marker)
}

fn validate_semantic_response_path(path: &Path, query_id: usize) -> Result<(), TypeAwareError> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::RootDir | Component::Prefix(_)))
    {
        return Err(TypeAwareError::from(format!(
            "type-aware query {query_id} returned a non-project-relative path"
        )));
    }
    Ok(())
}

fn validate_semantic_reference_paths(
    evidence: &SemanticReferenceData,
    query_id: usize,
) -> Result<(), TypeAwareError> {
    if evidence.source != "checker" {
        return Err(TypeAwareError::from(format!(
            "type-aware query {query_id} returned unsupported reference provenance"
        )));
    }
    validate_semantic_response_path(&evidence.path, query_id)?;
    for hop in &evidence.via {
        validate_semantic_response_path(&hop.path, query_id)?;
    }
    Ok(())
}

fn validate_edit_guard(
    root: &Path,
    symbol: &SemanticSymbol,
    guard: &SemanticEditGuard,
    query_id: usize,
    source_cache: &mut FxHashMap<PathBuf, Vec<u8>>,
) -> Result<(), TypeAwareError> {
    let relative = protocol_path(root, &symbol.path)?;
    let path = root.join(relative);
    let bytes = match source_cache.entry(path.clone()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let bytes = std::fs::read(&path).map_err(|error| {
                TypeAwareError::from(format!(
                    "failed to read type-aware declaration for query {query_id}: {error}"
                ))
            })?;
            entry.insert(bytes)
        }
    };
    if guard.start >= guard.end || guard.end > bytes.len() {
        return Err(TypeAwareError::from(format!(
            "type-aware query {query_id} returned an invalid declaration span"
        )));
    }
    let digest = digest_hex(Sha256::digest(&bytes[guard.start..guard.end]));
    if digest != guard.declaration_sha256 {
        return Err(TypeAwareError::from(format!(
            "type-aware query {query_id} returned a stale declaration hash"
        )));
    }
    Ok(())
}

fn decode_symbol_use_evidence(
    result: &SemanticQueryResponse,
) -> Result<Vec<SemanticReference>, TypeAwareError> {
    result
        .evidence
        .iter()
        .map(|value| {
            let evidence: SemanticReferenceData =
                serde_json::from_value(value.clone()).map_err(|error| {
                    TypeAwareError::from(format!(
                        "failed to decode type-aware evidence for query {}: {error}",
                        result.query_id
                    ))
                })?;
            let namespace = evidence.namespace.ok_or_else(|| {
                TypeAwareError::from(format!(
                    "type-aware reference for query {} omitted its namespace",
                    result.query_id
                ))
            })?;
            validate_semantic_reference_paths(&evidence, result.query_id)?;
            Ok(SemanticReference {
                path: evidence.path,
                line: evidence.line,
                col: evidence.col,
                role: evidence.role,
                namespace,
                via: evidence.via,
            })
        })
        .collect()
}

fn decode_symbol_trace_evidence(
    result: &SemanticQueryResponse,
) -> Result<Vec<SemanticReference>, TypeAwareError> {
    let mut references = Vec::new();
    for value in &result.evidence {
        let evidence: SemanticReferenceData =
            serde_json::from_value(value.clone()).map_err(|error| {
                TypeAwareError::from(format!(
                    "failed to decode type-aware trace evidence for query {}: {error}",
                    result.query_id
                ))
            })?;
        validate_semantic_reference_paths(&evidence, result.query_id)?;
        if evidence.role == "declaration" && evidence.namespace.is_none() {
            continue;
        }
        let namespace = evidence.namespace.ok_or_else(|| {
            TypeAwareError::from(format!(
                "type-aware trace reference for query {} omitted its namespace",
                result.query_id
            ))
        })?;
        references.push(SemanticReference {
            path: evidence.path,
            line: evidence.line,
            col: evidence.col,
            role: evidence.role,
            namespace,
            via: evidence.via,
        });
    }
    Ok(references)
}

const fn unresolved_gap(reason: Option<SemanticGapReason>) -> bool {
    matches!(
        reason,
        Some(
            SemanticGapReason::NoProject
                | SemanticGapReason::AmbiguousProject
                | SemanticGapReason::BlockingDiagnostics
                | SemanticGapReason::UnknownSymbol
                | SemanticGapReason::IncompleteProjectCoverage
                | SemanticGapReason::FrameworkContractProvenance
                | SemanticGapReason::Capacity
        )
    )
}

fn expected_candidate_decision(
    data: &SymbolUseData,
    has_required_contract: bool,
    reason_code: Option<SemanticGapReason>,
) -> (&'static str, SemanticCandidateDecisionKind) {
    if data.total_reference_count > 0 {
        return (
            "confirmed-used",
            SemanticCandidateDecisionKind::ConfirmedUsed,
        );
    }
    if has_required_contract {
        return (
            "contract-preserved",
            SemanticCandidateDecisionKind::ContractPreserved,
        );
    }
    if data.closed_world_eligible {
        return (
            "confirmed-no-static-references",
            SemanticCandidateDecisionKind::ConfirmedNoStaticReferences,
        );
    }
    if unresolved_gap(reason_code) {
        return (
            "no-confirmed-use",
            SemanticCandidateDecisionKind::RetainedUnresolved,
        );
    }
    (
        "no-confirmed-use",
        SemanticCandidateDecisionKind::RetainedAbstained,
    )
}

#[derive(Clone, Copy)]
struct CandidateExplanationInput<'a> {
    subject: &'a SemanticSymbol,
    decision: SemanticCandidateDecisionKind,
    fix_eligible: bool,
    owning_projects: &'a [String],
    evidence: &'a [SemanticReference],
    contract: Option<&'a SemanticContractEvidence>,
    framework_contract: Option<&'a SemanticFrameworkContractEvidence>,
    reason_code: Option<SemanticGapReason>,
    total_evidence_count: usize,
    truncated: bool,
}

fn candidate_explanation(input: CandidateExplanationInput<'_>) -> String {
    let CandidateExplanationInput {
        subject,
        decision,
        fix_eligible,
        owning_projects,
        evidence,
        contract,
        framework_contract,
        reason_code,
        total_evidence_count,
        truncated,
    } = input;
    let subject_name = symbol_display_name(subject);
    match decision {
        SemanticCandidateDecisionKind::ConfirmedUsed => evidence.first().map_or_else(
            || {
                format!(
                    "{subject_name} is retained because TypeScript resolved {total_evidence_count} exact static reference(s)."
                )
            },
            |reference| {
                let suffix = evidence_count_suffix(total_evidence_count, truncated);
                format!(
                    "{subject_name} is retained because it is used by a {} reference at {}:{}:{}{suffix}.",
                    reference.role,
                    reference.path.display(),
                    reference.line,
                    reference.col
                )
            },
        ),
        SemanticCandidateDecisionKind::ContractPreserved => {
            framework_contract.map_or_else(
                || contract.map_or_else(
                || {
                    format!(
                        "{subject_name} is retained because validated contract evidence makes deletion unsafe."
                    )
                },
                |contract| {
                    let declaration = symbol_display_name(&contract.declaration);
                    let relation = contract_relation_phrase(contract.relation);
                    format!(
                        "{subject_name} is retained because it {relation} {declaration}, declared at {}:{}:{}.",
                        contract.declaration.path.display(),
                        contract.declaration.line,
                        contract.declaration.col
                    )
                },
            ),
                |contract| {
                    let declaration = symbol_display_name(&contract.declaration);
                    let relation = framework_relation_phrase(contract.relation);
                    format!(
                        "{subject_name} is retained because the {} contract requires it through {relation} {declaration} from {}, declared at {}:{}:{}.",
                        contract.framework,
                        contract.package,
                        contract.declaration.path.display(),
                        contract.declaration.line,
                        contract.declaration.col
                    )
                },
            )
        }
        SemanticCandidateDecisionKind::ConfirmedNoStaticReferences if fix_eligible => {
            format!(
                "{subject_name} has no exact static references or required contracts in {}. A declaration-hash guarded fix is available.",
                project_scope(owning_projects)
            )
        }
        SemanticCandidateDecisionKind::ConfirmedNoStaticReferences => {
            format!(
                "{subject_name} has no exact static references in {}. This declaration kind remains advisory.",
                project_scope(owning_projects)
            )
        }
        SemanticCandidateDecisionKind::RetainedAbstained => {
            format!(
                "{subject_name} is retained because {} makes deletion unsafe in {}.",
                gap_reason_phrase(reason_code),
                project_scope(owning_projects)
            )
        }
        SemanticCandidateDecisionKind::RetainedUnresolved => {
            format!(
                "{subject_name} is retained because {} prevented complete resolution in {}.",
                gap_reason_phrase(reason_code),
                project_scope(owning_projects)
            )
        }
    }
}

fn symbol_display_name(symbol: &SemanticSymbol) -> String {
    symbol.owner.as_ref().map_or_else(
        || symbol.exported_name.clone(),
        |owner| format!("{owner}.{}", symbol.local_name),
    )
}

fn project_scope(projects: &[String]) -> String {
    match projects {
        [] => "the selected TypeScript project scope".to_string(),
        [project] => project.clone(),
        projects => format!("{} owning TypeScript projects", projects.len()),
    }
}

fn evidence_count_suffix(total: usize, truncated: bool) -> String {
    if total <= 1 {
        String::new()
    } else if truncated {
        format!(" (first of {total}, evidence truncated)")
    } else {
        format!(" (first of {total})")
    }
}

const fn contract_relation_phrase(relation: SemanticContractRelation) -> &'static str {
    match relation {
        SemanticContractRelation::InterfaceImplementation => "implements",
        SemanticContractRelation::AbstractImplementation => "implements the abstract member",
        SemanticContractRelation::Override => "overrides",
        SemanticContractRelation::OptionalContract => "matches the optional contract",
    }
}

const fn framework_relation_phrase(relation: SemanticFrameworkRelation) -> &'static str {
    match relation {
        SemanticFrameworkRelation::Extends => "base class",
        SemanticFrameworkRelation::Implements => "interface",
    }
}

const fn gap_reason_phrase(reason: Option<SemanticGapReason>) -> &'static str {
    match reason {
        Some(SemanticGapReason::NoProject) => "no owning TypeScript project",
        Some(SemanticGapReason::AmbiguousProject) => "ambiguous owning projects",
        Some(SemanticGapReason::BlockingDiagnostics) => "blocking structural diagnostics",
        Some(SemanticGapReason::UnknownSymbol) => "an unresolved exact declaration",
        Some(SemanticGapReason::UnknownEntryPoint) => "an unresolved public entry point",
        Some(SemanticGapReason::EvidenceLimit) => "the configured evidence limit",
        Some(SemanticGapReason::DynamicBehavior) => "dynamic runtime behavior",
        Some(SemanticGapReason::VirtualDispatch) => "interface or virtual dispatch",
        Some(SemanticGapReason::DynamicMemberAccess) => "computed or reflective member access",
        Some(SemanticGapReason::DecoratedDeclaration) => "a decorated declaration",
        Some(SemanticGapReason::OptionalContract) => "an optional inherited contract",
        Some(SemanticGapReason::AccessorPair) => "a paired accessor",
        Some(SemanticGapReason::OverloadSet) => "a method overload set",
        Some(SemanticGapReason::AttachedComment) => "an attached source comment",
        Some(SemanticGapReason::AbstractDeclaration) => "an abstract declaration",
        Some(SemanticGapReason::IncompleteProjectCoverage) => "incomplete owning-project coverage",
        Some(SemanticGapReason::FrameworkContractProvenance) => {
            "unverified framework package provenance"
        }
        Some(SemanticGapReason::Capacity) => "the configured semantic capacity",
        Some(SemanticGapReason::UnsupportedSyntax) => "unsupported declaration syntax",
        None => "incomplete semantic evidence",
    }
}

fn record_candidate_decision(
    decision: &SemanticCandidateDecision,
    index: usize,
    confirmed: &mut BTreeSet<usize>,
    stats: &mut CandidateDecisionStats,
) {
    match decision.decision {
        SemanticCandidateDecisionKind::ConfirmedUsed => {
            confirmed.insert(index);
            stats.confirmed_used += 1;
        }
        SemanticCandidateDecisionKind::ContractPreserved => {
            confirmed.insert(index);
            stats.contract_preserved += 1;
        }
        SemanticCandidateDecisionKind::ConfirmedNoStaticReferences => {
            stats.no_static_references += 1;
            stats.fix_eligible += usize::from(decision.closed_world_eligible);
        }
        SemanticCandidateDecisionKind::RetainedUnresolved => {
            stats.unresolved += 1;
            match decision.reason_code {
                Some(SemanticGapReason::NoProject) => stats.abstention_reasons.no_project += 1,
                Some(SemanticGapReason::BlockingDiagnostics) => {
                    stats.abstention_reasons.blocking_diagnostics += 1;
                }
                Some(SemanticGapReason::UnknownSymbol) => {
                    stats.abstention_reasons.unknown_symbol += 1;
                }
                Some(SemanticGapReason::Capacity) => {
                    stats.abstention_reasons.capacity += 1;
                }
                _ => stats.abstention_reasons.unsupported_syntax += 1,
            }
        }
        SemanticCandidateDecisionKind::RetainedAbstained => {
            stats.abstained += 1;
            match decision.reason_code {
                Some(SemanticGapReason::NoProject) => stats.abstention_reasons.no_project += 1,
                Some(SemanticGapReason::BlockingDiagnostics) => {
                    stats.abstention_reasons.blocking_diagnostics += 1;
                }
                Some(SemanticGapReason::UnknownSymbol) => {
                    stats.abstention_reasons.unknown_symbol += 1;
                }
                Some(SemanticGapReason::Capacity) => {
                    stats.abstention_reasons.capacity += 1;
                }
                _ => stats.abstention_reasons.unsupported_syntax += 1,
            }
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

fn add_capacity_gap(
    summary: &mut SemanticQuerySummary,
    surface: &mut ApiSurfaceResult,
    count: usize,
) {
    const ACTION: &str =
        "Narrow the analysis scope so every private-type-leak candidate can be confirmed.";
    let omission = SemanticOmission {
        reason_code: SemanticGapReason::Capacity,
        count,
    };
    add_summary_capacity_gap(summary, count);
    if surface.status == SemanticCompleteness::Complete {
        surface.status = SemanticCompleteness::Partial;
    }
    surface.omissions.push(omission);
    surface.actions.push(ACTION.to_string());
}

fn add_summary_capacity_gap(summary: &mut SemanticQuerySummary, count: usize) {
    const ACTION: &str =
        "Narrow the analysis scope so every type-aware candidate can be confirmed.";
    if summary.status == SemanticCompleteness::Complete {
        summary.status = SemanticCompleteness::Partial;
        summary.reason_code = Some(SemanticGapReason::Capacity);
    }
    summary.truncated = true;
    summary.omissions.push(SemanticOmission {
        reason_code: SemanticGapReason::Capacity,
        count,
    });
    summary.actions.push(ACTION.to_string());
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

#[cfg(test)]
fn apply_api_surface(
    root: &Path,
    results: &mut AnalysisResults,
    query: &SemanticQuery,
    result: &SemanticQueryResponse,
) -> Result<ApiSurfaceResult, TypeAwareError> {
    let (surface, mutation) = plan_api_surface(root, query, result)?;
    apply_api_surface_mutation(results, mutation);
    Ok(surface)
}

fn plan_api_surface(
    root: &Path,
    query: &SemanticQuery,
    result: &SemanticQueryResponse,
) -> Result<(ApiSurfaceResult, ApiSurfaceMutation), TypeAwareError> {
    let Some(data) = decode_api_surface_data(query, result)? else {
        return Ok((
            empty_api_surface_result(result),
            ApiSurfaceMutation::default(),
        ));
    };
    let surface = api_surface_result(result, data.clone());
    let retention = data
        .private_leak_confirmation
        .confirmation_complete
        .then(|| PrivateLeakRetention {
            requested_candidate_count: data.private_leak_confirmation.requested_candidate_count,
            confirmed_candidate_ids: data
                .private_leak_confirmation
                .confirmed_candidate_ids
                .iter()
                .copied()
                .collect(),
        });
    let upserts = data
        .leaks
        .iter()
        .zip(&surface.private_type_leaks)
        .map(|(wire, semantic)| PrivateLeakUpsert {
            evidence_path: root.join(&wire.evidence.path),
            export_name: wire.exposed_symbol.exported_name.clone(),
            type_name: wire.private_declaration.local_name.clone(),
            line: wire.evidence.line,
            col: wire.evidence.col,
            semantic: semantic.clone(),
        })
        .collect();
    Ok((surface, ApiSurfaceMutation { retention, upserts }))
}

fn apply_api_surface_mutation(results: &mut AnalysisResults, mutation: ApiSurfaceMutation) {
    if let Some(retention) = mutation.retention {
        let mut candidate_id = 0_usize;
        results.private_type_leaks.retain(|_| {
            let keep = candidate_id >= retention.requested_candidate_count
                || retention.confirmed_candidate_ids.contains(&candidate_id);
            candidate_id += 1;
            keep
        });
    }
    for upsert in mutation.upserts {
        attach_or_add_private_type_leak(results, upsert);
    }
}

fn decode_api_surface(
    query: &SemanticQuery,
    result: &SemanticQueryResponse,
) -> Result<ApiSurfaceResult, TypeAwareError> {
    let Some(data) = decode_api_surface_data(query, result)? else {
        return Ok(empty_api_surface_result(result));
    };
    Ok(api_surface_result(result, data))
}

fn decode_api_surface_data(
    query: &SemanticQuery,
    result: &SemanticQueryResponse,
) -> Result<Option<ApiSurfaceData>, TypeAwareError> {
    if result.status == SemanticCompleteness::Unavailable {
        return Ok(None);
    }
    let data: ApiSurfaceData = serde_json::from_value(result.data.clone()).map_err(|error| {
        TypeAwareError::from(format!("failed to decode type-aware API surface: {error}"))
    })?;
    validate_api_surface_data(query, result, &data)?;
    Ok(Some(data))
}

fn empty_api_surface_result(result: &SemanticQueryResponse) -> ApiSurfaceResult {
    ApiSurfaceResult {
        assertion: result.assertion.clone(),
        status: result.status,
        entries: Vec::new(),
        private_type_leaks: Vec::new(),
        omissions: result.omissions.clone(),
        actions: result.actions.clone(),
    }
}

fn validate_api_surface_data(
    query: &SemanticQuery,
    result: &SemanticQueryResponse,
    data: &ApiSurfaceData,
) -> Result<(), TypeAwareError> {
    let SemanticQuery::ApiSurface {
        private_leak_candidates,
        ..
    } = query
    else {
        return Err(TypeAwareError::from(
            "type-aware API surface data was paired with a different query operation".to_string(),
        ));
    };
    let confirmed_ids = &data.private_leak_confirmation.confirmed_candidate_ids;
    let ids_are_sorted_unique = confirmed_ids.windows(2).all(|ids| ids[0] < ids[1]);
    let requested_ids = private_leak_candidates
        .iter()
        .map(|candidate| candidate.id)
        .collect::<BTreeSet<_>>();
    let unsafe_confirmation = data.private_leak_confirmation.confirmation_complete
        && (result.status == SemanticCompleteness::Unavailable
            || result
                .omissions
                .iter()
                .any(|omission| omission.reason_code != SemanticGapReason::EvidenceLimit));
    if data.total_export_count < data.exports.len()
        || data.total_entry_count < data.entries.len()
        || data.total_leak_count < data.leaks.len()
        || data.total_public_signature_edge_count < data.public_signature_edges.len()
        || data.private_leak_confirmation.requested_candidate_count != private_leak_candidates.len()
        || !ids_are_sorted_unique
        || confirmed_ids.iter().any(|id| !requested_ids.contains(id))
        || unsafe_confirmation
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

fn attach_or_add_private_type_leak(results: &mut AnalysisResults, upsert: PrivateLeakUpsert) {
    if let Some(existing) = results.private_type_leaks.iter_mut().find(|finding| {
        finding.leak.path == upsert.evidence_path
            && finding.leak.export_name == upsert.export_name
            && finding.leak.type_name == upsert.type_name
    }) {
        existing.leak.semantic = Some(upsert.semantic);
        return;
    }
    results
        .private_type_leaks
        .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
            path: upsert.evidence_path,
            export_name: upsert.export_name,
            type_name: upsert.type_name,
            line: upsert.line,
            col: upsert.col,
            span_start: 0,
            semantic: Some(upsert.semantic),
        }));
}

fn response_project_config_hash(projects: &[SemanticProjectResponse]) -> String {
    let mut hasher = Sha256::new();
    let mut configs = projects
        .iter()
        .map(|project| (&project.config, &project.effective_config_hash))
        .collect::<Vec<_>>();
    configs.sort_unstable();
    for (config, effective_hash) in configs {
        hasher.update(config.as_bytes());
        hasher.update([0]);
        hasher.update(effective_hash.as_bytes());
        hasher.update([0]);
    }
    format!("sha256:{}", digest_hex(hasher.finalize()))
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
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn protocol_path_resolves_symlinked_root_prefixes() {
        // Mirrors macOS temp dirs, where /var is a symlink to /private/var:
        // the sidecar reports paths through the symlink while the canonical
        // project root uses the target (or vice versa). Neither direction may
        // be treated as outside the project root.
        let temp = tempdir().expect("temp dir");
        let real_root = temp.path().join("real");
        fs::create_dir_all(real_root.join("src")).expect("real root");
        fs::write(real_root.join("src/app.ts"), "export const a = 1;\n").expect("file");
        let link_root = temp.path().join("link");
        std::os::unix::fs::symlink(&real_root, &link_root).expect("symlink");

        let via_link = protocol_path(&real_root, &link_root.join("src/app.ts"))
            .expect("symlinked path inside the real root should resolve");
        assert_eq!(via_link, PathBuf::from("src/app.ts"));

        let via_real = protocol_path(&link_root, &real_root.join("src/app.ts"))
            .expect("real path inside the symlinked root should resolve");
        assert_eq!(via_real, PathBuf::from("src/app.ts"));

        let outside = temp.path().join("elsewhere.ts");
        fs::write(&outside, "export const b = 2;\n").expect("outside file");
        let error = protocol_path(&real_root, &outside)
            .expect_err("paths genuinely outside the root must still fail");
        assert!(error.to_string().contains("outside project root"));
    }

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

    fn request_with_all_operations(root: &Path) -> SemanticRequest {
        let symbol = symbol();
        SemanticRequest {
            protocol_version: PROTOCOL_VERSION,
            operation: OPERATION,
            root: root.to_string_lossy().into_owned(),
            projects: vec!["tsconfig.json".to_string()],
            evidence_limit: EVIDENCE_LIMIT,
            queries: vec![
                SemanticQuery::SymbolUse {
                    id: 0,
                    symbol: symbol.clone(),
                    framework_contracts: Vec::new(),
                },
                SemanticQuery::SymbolTrace {
                    id: 1,
                    symbol: symbol.clone(),
                },
                SemanticQuery::SymbolImpact { id: 2, symbol },
                SemanticQuery::ApiSurface {
                    id: 3,
                    entry_points: vec!["src/config.ts".to_string()],
                    include_cycles: true,
                    private_leak_candidates: Vec::new(),
                },
                SemanticQuery::TypeCoupling {
                    id: 4,
                    entry_points: vec!["src/config.ts".to_string()],
                    include_cycles: true,
                },
            ],
        }
    }

    fn complete_result(id: usize, operation: SemanticOperation) -> SemanticQueryResponse {
        SemanticQueryResponse {
            query_id: id,
            operation,
            assertion: "complete".to_string(),
            status: SemanticCompleteness::Complete,
            reason_code: None,
            actions: Vec::new(),
            evidence: vec![json!({"path": "src/config.ts"})],
            total_evidence_count: 1,
            truncated: false,
            omissions: Vec::new(),
            data: json!({}),
        }
    }

    fn complete_response(request: &SemanticRequest) -> SemanticResponse {
        SemanticResponse {
            protocol_version: PROTOCOL_VERSION,
            operation: OPERATION.to_string(),
            sidecar_version: env!("CARGO_PKG_VERSION").to_string(),
            backend: BACKEND_FAMILY.to_string(),
            backend_version: BACKEND_VERSION.to_string(),
            selected_tsconfigs: vec!["tsconfig.json".to_string()],
            projects: Vec::new(),
            results: request
                .queries
                .iter()
                .map(|query| complete_result(query.id(), query.operation()))
                .collect(),
            phase_timings_ms: SemanticPhaseTimings {
                project_setup: 1,
                diagnostics: 2,
                semantic_queries: 3,
            },
            warnings: Vec::new(),
            elapsed_ms: 6,
        }
    }

    fn private_leak(path: PathBuf, export_name: &str, type_name: &str) -> PrivateTypeLeakFinding {
        PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
            path,
            export_name: export_name.to_string(),
            type_name: type_name.to_string(),
            line: 2,
            col: 10,
            span_start: 0,
            semantic: None,
        })
    }

    #[test]
    fn validates_every_semantic_operation_and_response_invariant() {
        let request = request_with_all_operations(Path::new("."));
        let response = complete_response(&request);
        assert!(validate_response(&request, &response).is_ok());
        assert_eq!(
            request
                .queries
                .iter()
                .map(|query| query.operation().capability())
                .collect::<Vec<_>>(),
            vec![
                SemanticCapability::SymbolUse,
                SemanticCapability::SymbolTrace,
                SemanticCapability::SymbolImpact,
                SemanticCapability::ApiSurface,
                SemanticCapability::TypeCoupling,
            ]
        );

        let mut invalid = response.clone();
        invalid.protocol_version += 1;
        assert!(validate_response(&request, &invalid).is_err());

        invalid = response.clone();
        invalid.sidecar_version = "0.0.0".to_string();
        assert!(validate_response(&request, &invalid).is_err());

        invalid = response.clone();
        invalid.backend = "other".to_string();
        assert!(validate_response(&request, &invalid).is_err());

        invalid = response.clone();
        invalid.backend_version = "0".to_string();
        assert!(validate_response(&request, &invalid).is_err());

        invalid = response.clone();
        invalid.results[0].query_id = 99;
        assert!(validate_response(&request, &invalid).is_err());

        invalid = response.clone();
        invalid.results[0].operation = SemanticOperation::ApiSurface;
        assert!(validate_response(&request, &invalid).is_err());

        invalid = response.clone();
        invalid.results.push(invalid.results[0].clone());
        assert!(validate_response(&request, &invalid).is_err());

        invalid = response.clone();
        invalid.results[0].total_evidence_count = 0;
        assert!(validate_response(&request, &invalid).is_err());

        invalid = response.clone();
        invalid.results[0].status = SemanticCompleteness::Partial;
        assert!(validate_response(&request, &invalid).is_err());
        invalid.results[0].reason_code = Some(SemanticGapReason::DynamicBehavior);
        invalid.results[0].actions = vec!["Review dynamic use.".to_string()];
        invalid.results[0].omissions = vec![SemanticOmission {
            reason_code: SemanticGapReason::DynamicBehavior,
            count: 1,
        }];
        assert!(validate_response(&request, &invalid).is_ok());

        invalid = response;
        invalid.results.pop();
        assert!(validate_response(&request, &invalid).is_err());
    }

    #[test]
    fn malformed_late_query_does_not_partially_mutate_dead_code_results() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        let mut results = AnalysisResults::default();
        for name in ["first", "second"] {
            results.unused_exports.push(
                fallow_types::output_dead_code::UnusedExportFinding::with_actions(
                    fallow_types::results::UnusedExport {
                        path: root.join("src/exports.ts"),
                        export_name: name.to_string(),
                        is_type_only: false,
                        line: 1,
                        col: 0,
                        span_start: 0,
                        is_re_export: false,
                    },
                ),
            );
        }
        let batch = build_dead_code_queries(root, &results, &[], true, false, false)
            .expect("semantic query batch");
        let request = SemanticRequest {
            protocol_version: PROTOCOL_VERSION,
            operation: OPERATION,
            root: root.to_string_lossy().into_owned(),
            projects: vec!["tsconfig.json".to_string()],
            evidence_limit: EVIDENCE_LIMIT,
            queries: batch.queries,
        };
        let mut response = complete_response(&request);
        let first_symbol = match &request.queries[0] {
            SemanticQuery::SymbolUse { symbol, .. } => symbol.clone(),
            _ => panic!("expected symbol-use query"),
        };
        response.results[0].assertion = "confirmed-used".to_string();
        response.results[0].evidence = vec![json!({
            "path": "src/consumer.ts",
            "line": 1,
            "col": 0,
            "role": "import",
            "source": "checker",
            "namespace": "value",
            "via": []
        })];
        response.results[0].data = json!({
            "symbol": first_symbol,
            "selected_project": "tsconfig.json",
            "owning_projects": ["tsconfig.json"],
            "total_reference_count": 1,
            "contract_relations": [],
            "framework_contract_relations": [],
            "closed_world_eligible": false,
            "edit_guard": {
                "start": 0,
                "end": 1,
                "declaration_sha256": "unused"
            }
        });
        response.results[1].data = json!({"malformed": true});
        let before = serde_json::to_value(&results).expect("serialize original results");

        let outcome = apply_dead_code_response(
            root,
            &mut results,
            DeadCodeReconciliation {
                request: &request,
                targets: &batch.targets,
                capacity: batch.capacity,
            },
            response,
            vec![SemanticCapability::SymbolUse],
        );

        assert!(outcome.is_err());
        assert_eq!(
            serde_json::to_value(&results).expect("serialize retained results"),
            before
        );
    }

    fn rollback_family_results(root: &Path) -> AnalysisResults {
        let member_path = root.join("src/service.ts");
        let export_path = root.join("src/exports.ts");
        let mut class_member =
            fallow_types::output_dead_code::UnusedClassMemberFinding::with_actions(
                fallow_types::results::UnusedMember {
                    path: member_path,
                    parent_name: "Service".to_string(),
                    member_name: "run".to_string(),
                    kind: MemberKind::ClassMethod,
                    line: 2,
                    col: 2,
                },
            );
        class_member.semantic_only_candidate = true;
        let export = fallow_types::results::UnusedExport {
            path: export_path.clone(),
            export_name: "runtimeValue".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        };
        let type_export = fallow_types::results::UnusedExport {
            path: export_path,
            export_name: "RuntimeType".to_string(),
            is_type_only: true,
            line: 2,
            col: 0,
            span_start: 10,
            is_re_export: false,
        };
        AnalysisResults {
            unused_class_members: vec![class_member],
            unused_exports: vec![
                fallow_types::output_dead_code::UnusedExportFinding::with_actions(export),
            ],
            unused_types: vec![
                fallow_types::output_dead_code::UnusedTypeFinding::with_actions(type_export),
            ],
            private_type_leaks: vec![private_leak(
                root.join("src/api.ts"),
                "PublicResult",
                "Hidden",
            )],
            ..AnalysisResults::default()
        }
    }

    fn malformed_final_capability_response(request: &SemanticRequest) -> SemanticResponse {
        let mut response = complete_response(request);
        for result in &mut response.results[..3] {
            result.assertion = "no-confirmed-use".to_string();
            result.status = SemanticCompleteness::Unavailable;
            result.reason_code = Some(SemanticGapReason::BlockingDiagnostics);
            result.actions = vec!["Repair blocking diagnostics.".to_string()];
            result.omissions = vec![SemanticOmission {
                reason_code: SemanticGapReason::BlockingDiagnostics,
                count: 1,
            }];
        }
        response.results[3].assertion = "no-leak-confirmed".to_string();
        response.results[3].data = json!({
            "exports": [],
            "total_export_count": 0,
            "entries": [],
            "total_entry_count": 0,
            "leaks": [],
            "private_leak_confirmation": {
                "requested_candidate_count": 1,
                "confirmation_complete": true,
                "confirmed_candidate_ids": []
            },
            "total_leak_count": 0,
            "public_signature_edges": [],
            "total_public_signature_edge_count": 0
        });
        response.results[4].data = json!({"malformed": true});
        response
    }

    #[test]
    fn malformed_final_capability_rolls_back_every_dead_code_mutation_family() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        let mut results = rollback_family_results(root);
        let batch = build_dead_code_queries(
            root,
            &results,
            &[root.join("src/index.ts")],
            true,
            true,
            true,
        )
        .expect("mixed semantic query batch");
        let request = SemanticRequest {
            protocol_version: PROTOCOL_VERSION,
            operation: OPERATION,
            root: root.to_string_lossy().into_owned(),
            projects: vec!["tsconfig.json".to_string()],
            evidence_limit: EVIDENCE_LIMIT,
            queries: batch.queries,
        };
        let response = malformed_final_capability_response(&request);
        let before = serde_json::to_value(&results).expect("serialize original results");
        let semantic_only_before = results
            .unused_class_members
            .iter()
            .map(|finding| finding.semantic_only_candidate)
            .collect::<Vec<_>>();

        let outcome = apply_dead_code_response(
            root,
            &mut results,
            DeadCodeReconciliation {
                request: &request,
                targets: &batch.targets,
                capacity: batch.capacity,
            },
            response,
            vec![
                SemanticCapability::SymbolUse,
                SemanticCapability::ApiSurface,
                SemanticCapability::TypeCoupling,
            ],
        );

        assert!(outcome.is_err());
        assert_eq!(
            serde_json::to_value(&results).expect("serialize retained results"),
            before
        );
        assert_eq!(
            results
                .unused_class_members
                .iter()
                .map(|finding| finding.semantic_only_candidate)
                .collect::<Vec<_>>(),
            semantic_only_before
        );
    }

    #[test]
    fn merges_semantic_batches_conservatively() {
        let mut base = TypeAwareMeta {
            identity: Some(SemanticAnalysisIdentity {
                mode: SemanticAnalysisMode::TypeAware,
                semantic_schema_version: SEMANTIC_SCHEMA_VERSION,
                capabilities: vec![SemanticCapability::SymbolUse],
                project_config_hash: "sha256:project".to_string(),
                backend_family: BACKEND_FAMILY.to_string(),
                completeness: SemanticCompleteness::Complete,
            }),
            selected_tsconfigs: vec!["tsconfig.base.json".to_string()],
            warning_count: 1,
            warnings: vec!["base warning".to_string()],
            elapsed_ms: 5,
            phase_timings_ms: TypeAwarePhaseTimings {
                project_setup: 1,
                diagnostics: 2,
                symbol_scan: 3,
            },
            ..TypeAwareMeta::default()
        };
        let overlay = TypeAwareMeta {
            executed: true,
            protocol_version: PROTOCOL_VERSION,
            sidecar_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            backend: BACKEND_FAMILY.to_string(),
            backend_version: Some(BACKEND_VERSION.to_string()),
            identity: Some(SemanticAnalysisIdentity {
                mode: SemanticAnalysisMode::TypeAware,
                semantic_schema_version: SEMANTIC_SCHEMA_VERSION,
                capabilities: vec![
                    SemanticCapability::SymbolUse,
                    SemanticCapability::TypeCoupling,
                ],
                project_config_hash: "sha256:project".to_string(),
                backend_family: BACKEND_FAMILY.to_string(),
                completeness: SemanticCompleteness::Unavailable,
            }),
            selected_tsconfigs: vec![
                "tsconfig.app.json".to_string(),
                "tsconfig.base.json".to_string(),
            ],
            warning_count: 2,
            warnings: vec!["overlay warning".to_string()],
            elapsed_ms: 7,
            phase_timings_ms: TypeAwarePhaseTimings {
                project_setup: 4,
                diagnostics: 5,
                symbol_scan: 6,
            },
            ..TypeAwareMeta::default()
        };

        merge_type_aware_meta(&mut base, overlay);

        let identity = base.identity.expect("merged identity");
        assert_eq!(
            identity.capabilities,
            vec![
                SemanticCapability::SymbolUse,
                SemanticCapability::TypeCoupling
            ]
        );
        assert_eq!(identity.completeness, SemanticCompleteness::Unavailable);
        assert_eq!(
            base.selected_tsconfigs,
            vec![
                "tsconfig.app.json".to_string(),
                "tsconfig.base.json".to_string()
            ]
        );
        assert_eq!(base.warning_count, 3);
        assert_eq!(base.warnings.len(), 2);
        assert_eq!(base.elapsed_ms, 12);
        assert!(base.executed);
        assert_eq!(
            base.sidecar_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(base.backend_version.as_deref(), Some(BACKEND_VERSION));
        assert_eq!(base.phase_timings_ms.project_setup, 5);
        assert_eq!(base.phase_timings_ms.diagnostics, 7);
        assert_eq!(base.phase_timings_ms.symbol_scan, 9);

        let mut missing = None;
        merge_semantic_identity(&mut missing, None);
        assert!(missing.is_none());
        merge_semantic_identity(&mut missing, Some(identity));
        assert!(missing.is_some());
    }

    #[test]
    fn normalizes_semantic_paths_identity_and_impact_helpers() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        fs::write(root.join("tsconfig.json"), "{}").expect("tsconfig");

        assert_eq!(
            protocol_path(root, &root.join("src/config.ts")).expect("relative path"),
            PathBuf::from("src/config.ts")
        );
        assert!(protocol_path(root, Path::new("../outside.ts")).is_err());
        assert!(protocol_path(root, &root.join("../outside.ts")).is_err());
        assert!(validate_semantic_response_path(Path::new("src/config.ts"), 0).is_ok());
        assert!(validate_semantic_response_path(Path::new("../shared/consumer.ts"), 0).is_ok());
        assert!(validate_semantic_response_path(&root.join("outside.ts"), 0).is_err());
        let unsafe_alias = SemanticReferenceData {
            path: PathBuf::from("src/config.ts"),
            line: 1,
            col: 0,
            role: "alias".to_string(),
            source: "checker".to_string(),
            namespace: Some(SemanticNamespace::Value),
            via: vec![SemanticAliasHop {
                path: root.join("outside.ts"),
                from_name: "config".to_string(),
                to_name: "renamedConfig".to_string(),
                relation: "import-alias".to_string(),
            }],
        };
        assert!(validate_semantic_reference_paths(&unsafe_alias, 0).is_err());
        assert_eq!(namespace_name(SemanticNamespace::Value), "value");
        assert_eq!(namespace_name(SemanticNamespace::Type), "type");
        assert_eq!(digest_hex([0, 15, 255]), "000fff");

        let paths = impact_paths(
            vec![
                ImpactPathData {
                    path: PathBuf::from("src/direct.ts"),
                    provenance: Vec::new(),
                },
                ImpactPathData {
                    path: PathBuf::from("src/transitive.ts"),
                    provenance: vec![
                        PathBuf::from("src/config.ts"),
                        PathBuf::from("src/transitive.ts"),
                    ],
                },
            ],
            "affected",
        );
        assert_eq!(paths[0].distance, 1);
        assert_eq!(paths[1].distance, 2);
        assert_eq!(paths[1].relation, "affected");

        let outcome = empty_semantic_outcome(vec![
            SemanticCapability::SymbolUse,
            SemanticCapability::ApiSurface,
            SemanticCapability::SymbolUse,
        ]);
        assert!(!outcome.type_aware.meta.executed);
        assert!(outcome.type_aware.meta.sidecar_version.is_none());
        assert!(outcome.type_aware.meta.backend_version.is_none());
        let identity = outcome
            .type_aware
            .meta
            .identity
            .expect("empty semantic identity");
        assert_eq!(
            identity.capabilities,
            vec![
                SemanticCapability::SymbolUse,
                SemanticCapability::ApiSurface
            ]
        );
        assert_eq!(identity.project_config_hash, DEFERRED_PROJECT_CONFIG_HASH);
        assert!(outcome.type_coupling.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn protocol_path_accepts_windows_verbatim_paths_within_root() {
        let directory = tempdir().expect("temporary directory");
        let root = dunce::canonicalize(directory.path()).expect("canonical project root");
        let source = root.join("src").join("config.ts");
        fs::create_dir_all(source.parent().expect("source directory")).expect("source directory");
        fs::write(&source, "export const config = true;").expect("source fixture");
        let verbatim_source = PathBuf::from(format!(r"\\?\{}", source.display()));

        assert_eq!(
            protocol_path(&root, &verbatim_source).expect("project-relative path"),
            PathBuf::from("src").join("config.ts")
        );
    }

    #[test]
    fn real_semantic_query_replaces_deferred_project_identity() {
        let empty = empty_semantic_outcome(vec![SemanticCapability::SymbolUse]);
        let mut identity = empty.type_aware.meta.identity;
        merge_semantic_identity(
            &mut identity,
            Some(SemanticAnalysisIdentity {
                mode: SemanticAnalysisMode::TypeAware,
                semantic_schema_version: SEMANTIC_SCHEMA_VERSION,
                capabilities: vec![SemanticCapability::TypeCoupling],
                project_config_hash: "sha256:real-project-config".to_string(),
                backend_family: BACKEND_FAMILY.to_string(),
                completeness: SemanticCompleteness::Complete,
            }),
        );

        let merged = identity.expect("merged semantic identity");
        assert_eq!(merged.project_config_hash, "sha256:real-project-config");
        assert_eq!(
            merged.capabilities,
            vec![
                SemanticCapability::SymbolUse,
                SemanticCapability::TypeCoupling
            ]
        );
    }

    #[test]
    fn summarizes_completeness_and_retains_only_unconfirmed_items() {
        let mut complete = complete_result(0, SemanticOperation::SymbolUse);
        let summary = query_summary(&complete);
        assert_eq!(summary.capability, SemanticCapability::SymbolUse);
        assert_eq!(summary.total_evidence_count, 1);

        assert_eq!(
            aggregate_completeness(&[complete.clone()]),
            SemanticCompleteness::Complete
        );
        complete.status = SemanticCompleteness::Partial;
        assert_eq!(
            aggregate_completeness(&[complete.clone()]),
            SemanticCompleteness::Partial
        );
        complete.status = SemanticCompleteness::Unavailable;
        assert_eq!(
            aggregate_completeness(&[complete]),
            SemanticCompleteness::Unavailable
        );

        let mut items = vec!["zero", "one", "two", "three"];
        retain_unconfirmed(&mut items, &BTreeSet::from([0, 2]));
        assert_eq!(items, vec!["one", "three"]);
    }

    #[test]
    fn complete_api_surface_replaces_syntactic_private_leak_guesses() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        let path = root.join("src/api.ts");
        let mut results = AnalysisResults {
            private_type_leaks: vec![
                private_leak(path.clone(), "PublicResult", "Hidden"),
                private_leak(path, "InternalConfig", "ConfigOptions"),
            ],
            ..AnalysisResults::default()
        };
        let result = SemanticQueryResponse {
            query_id: 0,
            operation: SemanticOperation::ApiSurface,
            assertion: "leak-confirmed".to_string(),
            status: SemanticCompleteness::Complete,
            reason_code: None,
            actions: Vec::new(),
            evidence: Vec::new(),
            total_evidence_count: 1,
            truncated: false,
            omissions: Vec::new(),
            data: json!({
                "exports": [],
                "total_export_count": 1,
                "entries": [],
                "total_entry_count": 1,
                "leaks": [{
                    "exposed_symbol": {
                        "path": "src/api.ts",
                        "namespace": "type",
                        "declaration_kind": "interface",
                        "exported_name": "PublicResult",
                        "local_name": "PublicResult",
                        "line": 2,
                        "col": 0,
                        "owner": null
                    },
                    "private_declaration": {
                        "path": "src/model.ts",
                        "namespace": "type",
                        "declaration_kind": "interface",
                        "exported_name": "Hidden",
                        "local_name": "Hidden",
                        "line": 1,
                        "col": 0,
                        "owner": null
                    },
                    "relation": "public-signature-private-type",
                    "evidence": {
                        "path": "src/api.ts",
                        "line": 2,
                        "col": 10
                    }
                }],
                "private_leak_confirmation": {
                    "requested_candidate_count": 2,
                    "confirmation_complete": true,
                    "confirmed_candidate_ids": [0]
                },
                "total_leak_count": 1,
                "public_signature_edges": [],
                "total_public_signature_edge_count": 1
            }),
        };
        let query = SemanticQuery::ApiSurface {
            id: 0,
            entry_points: vec!["src/index.ts".to_string()],
            include_cycles: false,
            private_leak_candidates: vec![
                PrivateLeakCandidate {
                    id: 0,
                    path: "src/api.ts".to_string(),
                    export_name: "PublicResult".to_string(),
                    type_name: "Hidden".to_string(),
                },
                PrivateLeakCandidate {
                    id: 1,
                    path: "src/api.ts".to_string(),
                    export_name: "InternalConfig".to_string(),
                    type_name: "ConfigOptions".to_string(),
                },
            ],
        };

        let api =
            apply_api_surface(root, &mut results, &query, &result).expect("valid API surface");

        assert_eq!(api.private_type_leaks.len(), 1);
        assert_eq!(results.private_type_leaks.len(), 1);
        assert_eq!(
            results.private_type_leaks[0].leak.export_name,
            "PublicResult"
        );
        assert!(results.private_type_leaks[0].leak.semantic.is_some());
    }

    #[test]
    fn evidence_bounded_api_surface_replaces_syntactic_private_leak_guesses() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        let mut results = AnalysisResults::default();
        results.private_type_leaks.push(private_leak(
            root.join("src/api.ts"),
            "InternalConfig",
            "ConfigOptions",
        ));
        let result = SemanticQueryResponse {
            query_id: 0,
            operation: SemanticOperation::ApiSurface,
            assertion: "no-leak-confirmed".to_string(),
            status: SemanticCompleteness::Partial,
            reason_code: Some(SemanticGapReason::EvidenceLimit),
            actions: vec!["Narrow the query.".to_string()],
            evidence: Vec::new(),
            total_evidence_count: 0,
            truncated: true,
            omissions: vec![SemanticOmission {
                reason_code: SemanticGapReason::EvidenceLimit,
                count: 1,
            }],
            data: json!({
                "exports": [],
                "total_export_count": 1,
                "entries": [],
                "total_entry_count": 1,
                "leaks": [],
                "private_leak_confirmation": {
                    "requested_candidate_count": 1,
                    "confirmation_complete": true,
                    "confirmed_candidate_ids": []
                },
                "total_leak_count": 0,
                "public_signature_edges": [],
                "total_public_signature_edge_count": 0
            }),
        };
        let query = SemanticQuery::ApiSurface {
            id: 0,
            entry_points: vec!["src/index.ts".to_string()],
            include_cycles: false,
            private_leak_candidates: vec![PrivateLeakCandidate {
                id: 0,
                path: "src/api.ts".to_string(),
                export_name: "InternalConfig".to_string(),
                type_name: "ConfigOptions".to_string(),
            }],
        };

        apply_api_surface(root, &mut results, &query, &result).expect("valid API surface");

        assert!(results.private_type_leaks.is_empty());
    }

    #[test]
    fn candidate_capacity_retains_unrequested_private_leaks_and_marks_partial() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        let mut results = AnalysisResults::default();
        results.private_type_leaks.push(private_leak(
            root.join("src/api.ts"),
            "First",
            "FirstPrivate",
        ));
        results.private_type_leaks.push(private_leak(
            root.join("src/api.ts"),
            "Second",
            "SecondPrivate",
        ));
        let result = SemanticQueryResponse {
            query_id: 0,
            operation: SemanticOperation::ApiSurface,
            assertion: "no-leak-confirmed".to_string(),
            status: SemanticCompleteness::Complete,
            reason_code: None,
            actions: Vec::new(),
            evidence: Vec::new(),
            total_evidence_count: 0,
            truncated: false,
            omissions: Vec::new(),
            data: json!({
                "exports": [],
                "total_export_count": 0,
                "entries": [],
                "total_entry_count": 0,
                "leaks": [],
                "private_leak_confirmation": {
                    "requested_candidate_count": 1,
                    "confirmation_complete": true,
                    "confirmed_candidate_ids": []
                },
                "total_leak_count": 0,
                "public_signature_edges": [],
                "total_public_signature_edge_count": 0
            }),
        };
        let query = SemanticQuery::ApiSurface {
            id: 0,
            entry_points: vec!["src/index.ts".to_string()],
            include_cycles: false,
            private_leak_candidates: vec![PrivateLeakCandidate {
                id: 0,
                path: "src/api.ts".to_string(),
                export_name: "First".to_string(),
                type_name: "FirstPrivate".to_string(),
            }],
        };

        let mut surface =
            apply_api_surface(root, &mut results, &query, &result).expect("valid API surface");
        let mut summary = query_summary(&result);
        add_capacity_gap(&mut summary, &mut surface, 1);

        assert_eq!(results.private_type_leaks.len(), 1);
        assert_eq!(results.private_type_leaks[0].leak.export_name, "Second");
        assert_eq!(summary.status, SemanticCompleteness::Partial);
        assert_eq!(summary.reason_code, Some(SemanticGapReason::Capacity));
        assert_eq!(surface.status, SemanticCompleteness::Partial);
        assert_eq!(
            surface.omissions[0].reason_code,
            SemanticGapReason::Capacity
        );

        let mut unavailable_summary = SemanticQuerySummary {
            status: SemanticCompleteness::Unavailable,
            reason_code: Some(SemanticGapReason::BlockingDiagnostics),
            ..query_summary(&result)
        };
        let mut unavailable_surface = ApiSurfaceResult {
            status: SemanticCompleteness::Unavailable,
            ..surface
        };
        add_capacity_gap(&mut unavailable_summary, &mut unavailable_surface, 1);
        assert_eq!(
            unavailable_summary.status,
            SemanticCompleteness::Unavailable
        );
        assert_eq!(
            unavailable_summary.reason_code,
            Some(SemanticGapReason::BlockingDiagnostics)
        );
        assert_eq!(
            unavailable_surface.status,
            SemanticCompleteness::Unavailable
        );
    }

    #[test]
    fn semantic_query_capacity_reserves_graph_queries_and_retains_the_tail() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        let mut results = AnalysisResults::default();
        results.unused_types = (0..=MAX_SEMANTIC_QUERIES)
            .map(|index| {
                fallow_types::output_dead_code::UnusedTypeFinding::with_actions(
                    fallow_types::results::UnusedExport {
                        path: root.join("src/types.ts"),
                        export_name: format!("Type{index}"),
                        is_type_only: true,
                        line: 1,
                        col: 0,
                        span_start: 0,
                        is_re_export: false,
                    },
                )
            })
            .collect();

        let batch = build_dead_code_queries(
            root,
            &results,
            &[root.join("src/index.ts")],
            true,
            false,
            true,
        )
        .expect("bounded semantic query batch");

        assert_eq!(batch.queries.len(), MAX_SEMANTIC_QUERIES);
        assert_eq!(batch.capacity.unrequested_symbol_count, 2);
        assert!(matches!(
            batch.queries.last(),
            Some(SemanticQuery::TypeCoupling { .. })
        ));
    }

    #[test]
    fn semantic_queries_preserve_export_namespaces_and_targets() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        let mut results = AnalysisResults::default();
        results.unused_exports.push(
            fallow_types::output_dead_code::UnusedExportFinding::with_actions(
                fallow_types::results::UnusedExport {
                    path: root.join("src/runtime.ts"),
                    export_name: "runtimeValue".to_string(),
                    is_type_only: false,
                    line: 3,
                    col: 7,
                    span_start: 14,
                    is_re_export: false,
                },
            ),
        );
        results.unused_types.push(
            fallow_types::output_dead_code::UnusedTypeFinding::with_actions(
                fallow_types::results::UnusedExport {
                    path: root.join("src/contracts.ts"),
                    export_name: "RuntimeContract".to_string(),
                    is_type_only: true,
                    line: 5,
                    col: 2,
                    span_start: 28,
                    is_re_export: false,
                },
            ),
        );

        let batch = build_dead_code_queries(root, &results, &[], true, false, false)
            .expect("semantic query batch");

        assert_eq!(batch.queries.len(), 2);
        assert!(matches!(
            &batch.queries[0],
            SemanticQuery::SymbolUse {
                id: 0,
                symbol: SemanticSymbol {
                    path,
                    namespace: SemanticNamespace::Value,
                    declaration_kind,
                    exported_name,
                    local_name,
                    owner: None,
                    line: 3,
                    col: 7,
                },
                framework_contracts,
            } if path == Path::new("src/runtime.ts")
                && declaration_kind == "export"
                && exported_name == "runtimeValue"
                && local_name == "runtimeValue"
                && framework_contracts.is_empty()
        ));
        assert!(matches!(
            &batch.queries[1],
            SemanticQuery::SymbolUse {
                id: 1,
                symbol: SemanticSymbol {
                    path,
                    namespace: SemanticNamespace::Type,
                    declaration_kind,
                    exported_name,
                    local_name,
                    owner: None,
                    line: 5,
                    col: 2,
                },
                framework_contracts,
            } if path == Path::new("src/contracts.ts")
                && declaration_kind == "export"
                && exported_name == "RuntimeContract"
                && local_name == "RuntimeContract"
                && framework_contracts.is_empty()
        ));
        assert!(matches!(
            batch.targets.get(&0),
            Some(QueryTarget::UnusedExport(0))
        ));
        assert!(matches!(
            batch.targets.get(&1),
            Some(QueryTarget::UnusedType(0))
        ));
        assert_eq!(batch.capacity.unrequested_symbol_count, 0);
    }

    #[test]
    fn rejects_invalid_private_leak_confirmation_ids_and_counts() {
        let query = SemanticQuery::ApiSurface {
            id: 0,
            entry_points: Vec::new(),
            include_cycles: false,
            private_leak_candidates: vec![
                PrivateLeakCandidate {
                    id: 2,
                    path: "src/api.ts".to_string(),
                    export_name: "first".to_string(),
                    type_name: "First".to_string(),
                },
                PrivateLeakCandidate {
                    id: 7,
                    path: "src/api.ts".to_string(),
                    export_name: "second".to_string(),
                    type_name: "Second".to_string(),
                },
            ],
        };
        let decode = |confirmation| {
            serde_json::from_value::<ApiSurfaceData>(json!({
                "exports": [],
                "total_export_count": 0,
                "entries": [],
                "total_entry_count": 0,
                "leaks": [],
                "private_leak_confirmation": confirmation,
                "total_leak_count": 0,
                "public_signature_edges": [],
                "total_public_signature_edge_count": 0
            }))
            .expect("API surface data")
        };
        let result = complete_result(0, SemanticOperation::ApiSurface);

        assert!(
            validate_api_surface_data(
                &query,
                &result,
                &decode(json!({
                    "requested_candidate_count": 2,
                    "confirmation_complete": true,
                    "confirmed_candidate_ids": [2, 7]
                })),
            )
            .is_ok()
        );
        for confirmation in [
            json!({
                "requested_candidate_count": 1,
                "confirmation_complete": true,
                "confirmed_candidate_ids": [2]
            }),
            json!({
                "requested_candidate_count": 2,
                "confirmation_complete": true,
                "confirmed_candidate_ids": [7, 2]
            }),
            json!({
                "requested_candidate_count": 2,
                "confirmation_complete": true,
                "confirmed_candidate_ids": [2, 2]
            }),
            json!({
                "requested_candidate_count": 2,
                "confirmation_complete": true,
                "confirmed_candidate_ids": [99]
            }),
        ] {
            assert!(validate_api_surface_data(&query, &result, &decode(confirmation)).is_err());
        }
    }

    #[test]
    fn diagnostically_partial_api_surface_retains_syntactic_private_leak_guesses() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        let mut results = AnalysisResults::default();
        results.private_type_leaks.push(private_leak(
            root.join("src/api.ts"),
            "InternalConfig",
            "ConfigOptions",
        ));
        let result = SemanticQueryResponse {
            query_id: 0,
            operation: SemanticOperation::ApiSurface,
            assertion: "no-leak-confirmed".to_string(),
            status: SemanticCompleteness::Partial,
            reason_code: Some(SemanticGapReason::BlockingDiagnostics),
            actions: vec!["Repair diagnostics.".to_string()],
            evidence: Vec::new(),
            total_evidence_count: 0,
            truncated: false,
            omissions: vec![SemanticOmission {
                reason_code: SemanticGapReason::BlockingDiagnostics,
                count: 1,
            }],
            data: json!({
                "exports": [],
                "total_export_count": 0,
                "entries": [],
                "total_entry_count": 0,
                "leaks": [],
                "private_leak_confirmation": {
                    "requested_candidate_count": 1,
                    "confirmation_complete": false,
                    "confirmed_candidate_ids": []
                },
                "total_leak_count": 0,
                "public_signature_edges": [],
                "total_public_signature_edge_count": 0
            }),
        };
        let query = SemanticQuery::ApiSurface {
            id: 0,
            entry_points: vec!["src/index.ts".to_string()],
            include_cycles: false,
            private_leak_candidates: vec![PrivateLeakCandidate {
                id: 0,
                path: "src/api.ts".to_string(),
                export_name: "InternalConfig".to_string(),
                type_name: "ConfigOptions".to_string(),
            }],
        };

        let mut unsafe_result = result.clone();
        unsafe_result.data["private_leak_confirmation"]["confirmation_complete"] =
            serde_json::Value::Bool(true);
        assert!(apply_api_surface(root, &mut results, &query, &unsafe_result).is_err());
        assert_eq!(results.private_type_leaks.len(), 1);

        apply_api_surface(root, &mut results, &query, &result).expect("valid API surface");

        assert_eq!(results.private_type_leaks.len(), 1);
        assert!(results.private_type_leaks[0].leak.semantic.is_none());
    }

    #[test]
    fn unavailable_api_surface_retains_syntactic_private_leaks_without_decoding_data() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        let mut results = AnalysisResults::default();
        results.private_type_leaks.push(private_leak(
            root.join("src/api.ts"),
            "InternalConfig",
            "ConfigOptions",
        ));
        let result = SemanticQueryResponse {
            query_id: 0,
            operation: SemanticOperation::ApiSurface,
            assertion: "no-leak-confirmed".to_string(),
            status: SemanticCompleteness::Unavailable,
            reason_code: Some(SemanticGapReason::BlockingDiagnostics),
            actions: vec!["Repair diagnostics.".to_string()],
            evidence: Vec::new(),
            total_evidence_count: 0,
            truncated: false,
            omissions: vec![SemanticOmission {
                reason_code: SemanticGapReason::BlockingDiagnostics,
                count: 1,
            }],
            data: json!({}),
        };

        let query = SemanticQuery::ApiSurface {
            id: 0,
            entry_points: vec!["src/index.ts".to_string()],
            include_cycles: false,
            private_leak_candidates: vec![PrivateLeakCandidate {
                id: 0,
                path: "src/api.ts".to_string(),
                export_name: "InternalConfig".to_string(),
                type_name: "ConfigOptions".to_string(),
            }],
        };
        let applied = apply_api_surface(root, &mut results, &query, &result)
            .expect("conservative API surface");
        let decoded = decode_api_surface(&query, &result).expect("inspect API surface");

        assert_eq!(results.private_type_leaks.len(), 1);
        assert_eq!(applied.status, SemanticCompleteness::Unavailable);
        assert!(applied.entries.is_empty());
        assert_eq!(decoded.status, SemanticCompleteness::Unavailable);
        assert!(decoded.private_type_leaks.is_empty());
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
        assert_eq!(impact.confidence, SemanticImpactConfidence::Bounded);

        let mut invalid_confidence = result.clone();
        invalid_confidence.data["confidence"] = json!("certain");
        assert!(decode_symbol_impact(&response, &request, &invalid_confidence, symbol()).is_err());

        let mut invalid_path = result;
        invalid_path.data["direct_consumers"][0]["path"] = json!("/outside.ts");
        assert!(decode_symbol_impact(&response, &request, &invalid_path, symbol()).is_err());
    }

    #[test]
    fn candidate_policy_removes_contracts_and_retains_only_fixable_negative_evidence() {
        let decision = |kind, fixable| SemanticCandidateDecision {
            query_id: 0,
            subject: symbol(),
            decision: kind,
            status: SemanticCompleteness::Complete,
            owning_projects: vec!["tsconfig.json".to_string()],
            evidence: Vec::new(),
            contract: None,
            framework_contract: None,
            closed_world_eligible: fixable,
            edit_guard: None,
            reason_code: None,
            explanation: "test decision".to_string(),
            actions: Vec::new(),
            total_evidence_count: 0,
            truncated: false,
            omissions: Vec::new(),
        };
        let mut removed = BTreeSet::new();
        let mut stats = CandidateDecisionStats::default();

        record_candidate_decision(
            &decision(SemanticCandidateDecisionKind::ContractPreserved, false),
            3,
            &mut removed,
            &mut stats,
        );
        record_candidate_decision(
            &decision(
                SemanticCandidateDecisionKind::ConfirmedNoStaticReferences,
                true,
            ),
            4,
            &mut removed,
            &mut stats,
        );

        assert!(removed.contains(&3));
        assert!(!removed.contains(&4));
        assert_eq!(stats.contract_preserved, 1);
        assert_eq!(stats.no_static_references, 1);
        assert_eq!(stats.fix_eligible, 1);
    }

    #[test]
    fn semantic_only_framework_candidates_require_complete_negative_evidence() {
        for decision in [
            SemanticCandidateDecisionKind::ConfirmedUsed,
            SemanticCandidateDecisionKind::ContractPreserved,
            SemanticCandidateDecisionKind::RetainedAbstained,
            SemanticCandidateDecisionKind::RetainedUnresolved,
        ] {
            assert!(semantic_only_candidate_stays_hidden(true, decision));
        }
        assert!(!semantic_only_candidate_stays_hidden(
            true,
            SemanticCandidateDecisionKind::ConfirmedNoStaticReferences,
        ));
        assert!(!semantic_only_candidate_stays_hidden(
            false,
            SemanticCandidateDecisionKind::RetainedUnresolved,
        ));
    }

    #[test]
    fn failed_refinement_cleanup_removes_only_unverified_semantic_candidates() {
        let member = |name: &str, semantic_only_candidate| {
            let mut finding =
                fallow_types::output_dead_code::UnusedClassMemberFinding::with_actions(
                    fallow_types::results::UnusedMember {
                        path: PathBuf::from("src/service.ts"),
                        parent_name: "Service".to_string(),
                        member_name: name.to_string(),
                        kind: MemberKind::ClassMethod,
                        line: 2,
                        col: 2,
                    },
                );
            finding.semantic_only_candidate = semantic_only_candidate;
            finding
        };
        let ordinary = member("ordinary", false);
        let ordinary_before = serde_json::to_value(&ordinary).expect("serialize ordinary finding");
        let mut results = AnalysisResults {
            unused_class_members: vec![ordinary, member("semantic", true)],
            ..AnalysisResults::default()
        };

        discard_unverified_semantic_candidates(&mut results);

        assert_eq!(results.unused_class_members.len(), 1);
        assert_eq!(
            serde_json::to_value(&results.unused_class_members[0])
                .expect("serialize retained ordinary finding"),
            ordinary_before
        );
        assert!(!results.unused_class_members[0].semantic_only_candidate);
    }

    #[test]
    fn guarded_fix_support_excludes_class_properties() {
        let mut method = symbol();
        method.declaration_kind = "class_method".to_string();
        let mut property = method.clone();
        property.declaration_kind = "class_property".to_string();

        assert!(query_supports_guarded_fix(&SemanticQuery::SymbolUse {
            id: 0,
            symbol: method,
            framework_contracts: Vec::new(),
        }));
        assert!(!query_supports_guarded_fix(&SemanticQuery::SymbolUse {
            id: 1,
            symbol: property,
            framework_contracts: Vec::new(),
        }));
    }

    #[test]
    fn candidate_explanations_name_exact_use_contract_and_safe_fix_evidence() {
        let mut method = symbol();
        method.owner = Some("UserRepository".to_string());
        method.exported_name = "save".to_string();
        method.local_name = "save".to_string();
        method.declaration_kind = "class_method".to_string();

        let owning_projects = ["tsconfig.json".to_string()];
        let evidence = [SemanticReference {
            path: PathBuf::from("src/service.ts"),
            line: 12,
            col: 4,
            role: "call".to_string(),
            namespace: SemanticNamespace::Value,
            via: Vec::new(),
        }];
        let used = candidate_explanation(CandidateExplanationInput {
            subject: &method,
            decision: SemanticCandidateDecisionKind::ConfirmedUsed,
            fix_eligible: false,
            owning_projects: &owning_projects,
            evidence: &evidence,
            contract: None,
            framework_contract: None,
            reason_code: None,
            total_evidence_count: 1,
            truncated: false,
        });
        assert_eq!(
            used,
            "UserRepository.save is retained because it is used by a call reference at src/service.ts:12:4."
        );

        let mut interface_member = method.clone();
        interface_member.owner = Some("Repository".to_string());
        let contract = SemanticContractEvidence {
            relation: SemanticContractRelation::InterfaceImplementation,
            declaration: interface_member,
            optional: false,
        };
        let required = candidate_explanation(CandidateExplanationInput {
            subject: &method,
            decision: SemanticCandidateDecisionKind::ContractPreserved,
            fix_eligible: false,
            owning_projects: &owning_projects,
            evidence: &[],
            contract: Some(&contract),
            framework_contract: None,
            reason_code: None,
            total_evidence_count: 0,
            truncated: false,
        });
        assert_eq!(
            required,
            "UserRepository.save is retained because it implements Repository.save, declared at src/config.ts:4:0."
        );

        let fixable = candidate_explanation(CandidateExplanationInput {
            subject: &method,
            decision: SemanticCandidateDecisionKind::ConfirmedNoStaticReferences,
            fix_eligible: true,
            owning_projects: &owning_projects,
            evidence: &[],
            contract: None,
            framework_contract: None,
            reason_code: None,
            total_evidence_count: 0,
            truncated: false,
        });
        assert_eq!(
            fixable,
            "UserRepository.save has no exact static references or required contracts in tsconfig.json. A declaration-hash guarded fix is available."
        );
    }

    #[test]
    fn framework_evidence_must_match_requested_package_declaration() {
        let requested = SemanticFrameworkContract {
            framework: "lit".to_string(),
            package: "lit".to_string(),
            heritage_symbol: "LitElement".to_string(),
            heritage_names: vec!["LitElement".to_string()],
            relation: SemanticFrameworkRelation::Extends,
            members: vec!["render".to_string()],
        };
        let mut declaration = symbol();
        declaration.path = PathBuf::from("src/local-lit.ts");
        declaration.exported_name = "LitElement".to_string();
        declaration.local_name = "LitElement".to_string();
        let data = SymbolUseData {
            symbol: symbol(),
            selected_project: "tsconfig.json".to_string(),
            owning_projects: vec!["tsconfig.json".to_string()],
            total_reference_count: 0,
            contract_relations: Vec::new(),
            framework_contract_relations: vec![SemanticFrameworkContractEvidence {
                framework: "lit".to_string(),
                package: "lit".to_string(),
                relation: SemanticFrameworkRelation::Extends,
                declaration,
            }],
            closed_world_eligible: false,
            edit_guard: SemanticEditGuard {
                start: 0,
                end: 1,
                declaration_sha256: "unused".to_string(),
            },
        };
        let result = SemanticQueryResponse {
            query_id: 0,
            operation: SemanticOperation::SymbolUse,
            assertion: "contract-preserved".to_string(),
            status: SemanticCompleteness::Complete,
            reason_code: None,
            actions: Vec::new(),
            evidence: Vec::new(),
            total_evidence_count: 0,
            truncated: false,
            omissions: Vec::new(),
            data: json!({}),
        };

        let requested_symbol = symbol();
        let requested_contracts = [requested];
        let error = validate_symbol_use_data(
            SymbolUseValidation {
                root: Path::new("."),
                requested: &requested_symbol,
                requested_framework_contracts: &requested_contracts,
                result: &result,
                fix_supported: false,
            },
            &data,
            &mut FxHashMap::default(),
        )
        .expect_err("local same-name declaration must not satisfy a package contract");

        assert!(
            error
                .to_string()
                .contains("unrequested framework contract evidence")
        );
    }
}
