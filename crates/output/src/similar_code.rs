//! Independent JSON contracts for opt-in semantic similar-code discovery.

use std::fmt;

use crate::root_envelopes::{RootEnvelopeMode, serialize_named_json_output};
use fallow_types::envelope::{ElapsedMs, ToolVersion};
use serde::{Deserialize, Serialize};

/// Current raw similar-code envelope schema version.
pub const SIMILAR_CODE_SCHEMA_VERSION: u32 = 1;
/// Current similar-code inspect envelope schema version.
pub const SIMILAR_CODE_INSPECT_SCHEMA_VERSION: u32 = 1;
/// Current similar-code review envelope schema version.
pub const SIMILAR_CODE_REVIEW_SCHEMA_VERSION: u32 = 1;
/// Current local-provider status envelope schema version.
pub const SIMILAR_CODE_STATUS_SCHEMA_VERSION: u32 = 1;
/// Current vector-cache clear envelope schema version.
pub const SIMILAR_CODE_CACHE_CLEAR_SCHEMA_VERSION: u32 = 1;

/// Version singleton for raw similar-code output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SimilarCodeSchemaVersion {
    /// Initial independent contract.
    #[serde(rename = "1")]
    V1,
}

/// Version singleton for a similar-code inspect packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SimilarCodeInspectSchemaVersion {
    /// Initial independent contract.
    #[serde(rename = "1")]
    V1,
}

/// Version singleton for reviewed similar-code output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SimilarCodeReviewSchemaVersion {
    /// Initial independent contract.
    #[serde(rename = "1")]
    V1,
}

/// Version singleton for local-provider status output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SimilarCodeStatusSchemaVersion {
    /// Initial independent status contract.
    #[serde(rename = "1")]
    V1,
}

/// Version singleton for vector-cache clear output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SimilarCodeCacheClearSchemaVersion {
    /// Initial independent cache-mutation contract.
    #[serde(rename = "1")]
    V1,
}

/// Machine-readable readiness of the exact local companion and pinned model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeStatusOutput {
    /// Independent status schema version.
    pub schema_version: SimilarCodeStatusSchemaVersion,
    /// Fallow version producing the status envelope.
    pub version: ToolVersion,
    /// Companion protocol version.
    pub protocol_version: u32,
    /// Embedding calculation semantics implemented by the companion.
    pub embedding_semantics_version: u32,
    /// Exact companion package version.
    pub companion_version: String,
    /// Whether every pinned model artifact is ready and verified.
    pub model_ready: bool,
    /// Immutable model identifier.
    pub model_id: String,
    /// Immutable model revision.
    pub model_revision: String,
    /// Embedding width.
    pub dimensions: u32,
    /// Maximum tokenizer length.
    pub max_tokens: u32,
    /// Model license identifier.
    pub license: String,
    /// Local model cache directory.
    pub cache_dir: String,
    /// Expected download size for all pinned artifacts.
    pub download_bytes: u64,
    /// Whether source analysis stays local and offline.
    pub analysis_offline: bool,
    /// Whether all installed artifacts passed integrity validation.
    pub integrity_verified: bool,
    /// Actionable readiness problem when the model is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// Whether this setup invocation downloaded new bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloaded: Option<bool>,
}

/// Result of explicitly clearing the derived project-namespaced vector cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeCacheClearOutput {
    /// Independent cache-clear schema version.
    pub schema_version: SimilarCodeCacheClearSchemaVersion,
    /// Fallow version producing the cache-clear envelope.
    pub version: ToolVersion,
    /// Whether an existing vector cache was removed.
    pub removed: bool,
    /// Whether model artifacts were removed. Version 1 always emits false.
    pub model_removed: bool,
}

/// Version singleton for the separate human or agent verdict document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SimilarCodeVerdictSchemaVersion {
    /// Initial independent verdict contract.
    #[serde(rename = "1")]
    V1,
}

/// Immutable local provider provenance for one generation run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeProviderProvenance {
    /// Provider family. Version 1 accepts only the official local companion.
    pub provider: SimilarCodeProvider,
    /// Exact companion package version.
    pub companion_version: String,
    /// Companion protocol version negotiated for this run.
    pub protocol_version: u32,
    /// Whether source content left the local machine. Version 1 requires false.
    pub source_left_machine: bool,
}

/// Provider families admitted by the version 1 public contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeProvider {
    /// Exact-version official companion executed locally.
    OfficialLocalCompanion,
}

/// Immutable model artifact provenance.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeModelProvenance {
    /// Stable model identifier.
    pub model_id: String,
    /// Immutable model revision.
    pub revision: String,
    /// SHA-256 digest of the exact model artifact bytes.
    pub artifact_sha256: String,
    /// SPDX license identifier or reviewed license label.
    pub license: String,
    /// Embedding vector dimensions.
    pub dimensions: u32,
}

/// Parameters that materially affect generated embeddings and scores.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeGenerationParameters {
    /// Numeric representation used for model inference.
    pub dtype: String,
    /// Pooling strategy applied to model output.
    pub pooling: String,
    /// Whether vectors were normalized before comparison.
    pub normalized: bool,
    /// Maximum inference batch size used by the run.
    pub batch_size: u32,
    /// Maximum tokenizer length before deterministic truncation.
    pub max_tokens: u32,
    /// Digest over the complete effective generation parameter set.
    pub parameter_sha256: String,
}

/// Effective endpoint scope used for corpus admission and pair retention.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeScopeProvenance {
    /// Whether file, changed-file, diff, or workspace scoping was active.
    pub active: bool,
    /// Sorted project-root-relative paths satisfying every active predicate.
    pub paths: Vec<String>,
}

/// Complete provenance needed to reproduce candidate generation.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeGeneration {
    /// Version of extraction and normalization semantics used for both IDs.
    pub extraction_semantics_version: u32,
    /// Version of the calculation that produces model embeddings.
    pub embedding_semantics_version: u32,
    /// Local provider provenance.
    pub provider: SimilarCodeProviderProvenance,
    /// Immutable model provenance.
    pub model: SimilarCodeModelProvenance,
    /// Effective generation parameters.
    pub parameters: SimilarCodeGenerationParameters,
    /// Materialized endpoint scope needed to reproduce scoped discovery.
    pub scope: SimilarCodeScopeProvenance,
    /// Minimum cosine similarity admitted into the candidate set.
    pub threshold: f64,
    /// Minimum source line count admitted into function extraction.
    pub min_lines: u64,
}

/// Exact named location of one candidate function.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeLocation {
    /// Project-root-relative, forward-slash path.
    pub path: String,
    /// Extracted function or method name.
    pub name: String,
    /// One-based inclusive start line.
    pub start_line: u32,
    /// One-based inclusive start column.
    pub start_column: u32,
    /// One-based inclusive end line.
    pub end_line: u32,
    /// One-based inclusive end column.
    pub end_column: u32,
    /// SHA-256 digest of the exact extracted function source.
    pub source_sha256: String,
}

/// Stable, coarse interpretation of a candidate score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeSimilarityBand {
    /// Candidate is close to the configured lower threshold.
    Moderate,
    /// Candidate has a strong semantic similarity score.
    High,
    /// Candidate has an exceptionally high semantic similarity score.
    VeryHigh,
}

/// Verification state of a raw semantic candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeVerificationStatus {
    /// Candidate generation is discovery only and has not verified behavior.
    Unverified,
}

/// Explicit availability state for one optional enrichment source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeEnrichmentState {
    /// Evidence is included in the inspect packet.
    Available,
    /// Evidence was requested but could not be obtained.
    Unavailable,
    /// Evidence was not requested for this run.
    NotRequested,
}

/// Availability of every supported source-grounded enrichment.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeEnrichmentAvailability {
    /// Import or workspace relationship evidence.
    pub graph_relationship: SimilarCodeEnrichmentState,
    /// Entry-point reachability evidence.
    pub entry_point_reachability: SimilarCodeEnrichmentState,
    /// Direct caller evidence.
    pub callers: SimilarCodeEnrichmentState,
    /// Direct callee evidence.
    pub callees: SimilarCodeEnrichmentState,
    /// Ownership evidence.
    pub ownership: SimilarCodeEnrichmentState,
    /// Churn evidence.
    pub churn: SimilarCodeEnrichmentState,
    /// Test relationship evidence.
    pub tests: SimilarCodeEnrichmentState,
    /// Deterministic clone coverage evidence.
    pub deterministic_clone_coverage: SimilarCodeEnrichmentState,
    /// Runtime evidence.
    pub runtime: SimilarCodeEnrichmentState,
}

/// Read-only follow-up exposed for a candidate.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeAction {
    /// Stable action identifier.
    pub action: SimilarCodeActionType,
    /// Human-readable description of the read-only operation.
    pub description: String,
    /// Explicit mutation guarantee. Version 1 requires this to be true.
    pub read_only: bool,
}

/// Read-only actions supported by the candidate workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeActionType {
    /// Build a bounded source-grounded inspect packet.
    Inspect,
    /// Join this candidate with an external verdict document.
    Review,
}

/// One unverified semantic similar-code candidate.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeCandidate {
    /// Snapshot-stable opaque candidate identity.
    pub candidate_id: String,
    /// Content-stable key used for safe line-movement rebinding.
    pub review_key: String,
    /// First location in deterministic pair order.
    pub left: SimilarCodeLocation,
    /// Second location in deterministic pair order.
    pub right: SimilarCodeLocation,
    /// Cosine similarity reported by the pinned provider and model.
    pub similarity: f64,
    /// Stable score band for consumers that do not need the raw score.
    pub similarity_band: SimilarCodeSimilarityBand,
    /// Raw candidates are always explicitly unverified.
    pub verification_status: SimilarCodeVerificationStatus,
    /// Availability of optional deterministic and runtime context.
    pub enrichment: SimilarCodeEnrichmentAvailability,
    /// Read-only inspect and review affordances.
    pub actions: Vec<SimilarCodeAction>,
}

/// Bounded phase names in the similar-code pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodePhase {
    /// Discover eligible source files.
    Discovery,
    /// Extract and normalize supported functions.
    Extraction,
    /// Load or populate the local vector cache.
    Cache,
    /// Generate embeddings with the official local companion.
    Embedding,
    /// Validate provider output before using it.
    Validation,
    /// Compare vectors and select bounded candidates.
    Comparison,
    /// Build optional deterministic context.
    Enrichment,
}

/// Completion state for one generation phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodePhaseStatus {
    /// Phase completed its admitted scope.
    Complete,
    /// Phase returned bounded partial results.
    Partial,
    /// Phase was intentionally not run.
    Skipped,
    /// Phase reached its configured timeout.
    TimedOut,
}

/// Accounting for one bounded generation phase.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodePhaseCompletion {
    /// Pipeline phase.
    pub phase: SimilarCodePhase,
    /// Whether this phase completed, skipped, or returned partial data.
    pub status: SimilarCodePhaseStatus,
    /// Number of admitted inputs processed by this phase.
    pub processed: u64,
    /// Total admitted inputs known to this phase, when available.
    pub total: Option<u64>,
    /// Stable explanation when the phase did not complete.
    pub reason: Option<String>,
}

/// Effective resource limits for a similar-code run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeLimits {
    /// Maximum source files admitted.
    pub max_files: u64,
    /// Maximum extracted functions admitted.
    pub max_functions: u64,
    /// Maximum aggregate normalized source bytes admitted.
    pub max_source_bytes: u64,
    /// Maximum normalized bytes admitted for one function.
    pub max_function_bytes: u64,
    /// Maximum embedding batch size.
    pub max_batch_size: u64,
    /// Maximum vector bytes retained for comparison.
    pub max_vector_bytes: u64,
    /// Maximum pair comparisons performed.
    pub max_comparisons: u64,
    /// Maximum candidates returned.
    pub max_candidates: u64,
    /// Maximum returned neighbors per function.
    pub max_neighbors_per_function: u64,
    /// End-to-end timeout in milliseconds.
    pub timeout_ms: u64,
}

/// Stable reasons why admitted work was skipped or truncated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeSkipReason {
    /// Function was shorter than the configured minimum source-line count.
    BelowMinimumLines,
    /// Syntax form is outside the supported extraction contract.
    UnsupportedFunction,
    /// Generated source was excluded.
    GeneratedSource,
    /// One function exceeded the per-function byte limit.
    FunctionTooLarge,
    /// File or function admission limit was reached.
    InputLimit,
    /// Aggregate source byte limit was reached.
    SourceBytesLimit,
    /// Vector memory limit was reached.
    VectorMemoryLimit,
    /// Pair comparison limit was reached.
    ComparisonLimit,
    /// Candidate result limit was reached.
    CandidateLimit,
    /// Per-function neighbor limit was reached.
    NeighborLimit,
    /// Provider or overall timeout was reached.
    Timeout,
    /// Local provider failed after returning a usable subset of vectors.
    ProviderFailure,
    /// Provider tokenization truncated an otherwise admitted source fragment.
    TokenTruncation,
    /// Optional enrichment source was unavailable.
    EnrichmentUnavailable,
}

/// Count of skipped work for a stable reason.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeSkip {
    /// Phase that skipped the work.
    pub phase: SimilarCodePhase,
    /// Stable skip reason.
    pub reason: SimilarCodeSkipReason,
    /// Number of inputs skipped for this phase and reason.
    pub count: u64,
}

/// Vector cache outcome for a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeCacheStatus {
    /// Cache use was disabled.
    Disabled,
    /// Every requested vector was found in the cache.
    Hit,
    /// No requested vector was found in the cache.
    Miss,
    /// The run used both cached and newly generated vectors.
    Mixed,
}

/// Privacy-safe cache accounting. Source fragments are never represented.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeCacheSummary {
    /// Aggregate cache outcome.
    pub status: SimilarCodeCacheStatus,
    /// Valid vector cache hits.
    pub hits: u64,
    /// Vector cache misses.
    pub misses: u64,
    /// Newly written vector cache entries.
    pub writes: u64,
    /// Corrupt or incompatible entries ignored safely.
    pub invalid_entries: u64,
}

/// Overall trustworthiness of an emitted result set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeCompletionStatus {
    /// Every admitted generation phase completed.
    Complete,
    /// One or more limits, skips, or timeouts made the result partial.
    Partial,
}

/// Typed completion, limit, skip, and cache accounting.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeCompletion {
    /// Overall completion status. Only `complete` makes an empty set conclusive.
    pub status: SimilarCodeCompletionStatus,
    /// Per-phase completion in pipeline order.
    pub phases: Vec<SimilarCodePhaseCompletion>,
    /// Effective run limits.
    pub limits: SimilarCodeLimits,
    /// Aggregated skips in phase and reason order.
    pub skips: Vec<SimilarCodeSkip>,
    /// Privacy-safe vector cache accounting.
    pub cache: SimilarCodeCacheSummary,
    /// Aggregate model inference wall time reported by the local provider.
    pub provider_inference_ms: u64,
}

/// Non-severity diagnostic domain for similar-code generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeDiagnosticDomain {
    /// Source discovery or workspace interpretation.
    Workspace,
    /// Function extraction or normalization.
    Extraction,
    /// Local provider execution or protocol validation.
    Provider,
    /// Local vector cache handling.
    Cache,
    /// Optional source-grounded enrichment.
    Enrichment,
    /// Verdict matching and review-key rebinding.
    Review,
}

/// Actionable diagnostic without a severity or gate implication.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeDiagnostic {
    /// Stable diagnostic domain.
    pub domain: SimilarCodeDiagnosticDomain,
    /// Stable machine-readable code.
    pub code: String,
    /// Bounded human-readable explanation.
    pub message: String,
    /// Optional project-root-relative path.
    pub path: Option<String>,
}

/// Raw `fallow similar-code --format json` output.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeOutput {
    /// Independent envelope schema version.
    pub schema_version: SimilarCodeSchemaVersion,
    /// Fallow version that produced this output.
    pub version: ToolVersion,
    /// End-to-end elapsed milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Immutable provider, model, and parameter provenance.
    pub generation: SimilarCodeGeneration,
    /// Deterministically ordered unverified candidates.
    pub candidates: Vec<SimilarCodeCandidate>,
    /// Typed completion and boundedness accounting.
    pub completion: SimilarCodeCompletion,
    /// Non-severity diagnostics in deterministic order.
    pub diagnostics: Vec<SimilarCodeDiagnostic>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimilarCodeOutputWire {
    schema_version: SimilarCodeSchemaVersion,
    version: String,
    elapsed_ms: u64,
    generation: SimilarCodeGeneration,
    candidates: Vec<SimilarCodeCandidate>,
    completion: SimilarCodeCompletion,
    diagnostics: Vec<SimilarCodeDiagnostic>,
}

impl<'de> Deserialize<'de> for SimilarCodeOutput {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let wire = SimilarCodeOutputWire::deserialize(deserializer)?;
        Ok(Self {
            schema_version: wire.schema_version,
            version: ToolVersion(wire.version),
            elapsed_ms: ElapsedMs(wire.elapsed_ms),
            generation: wire.generation,
            candidates: wire.candidates,
            completion: wire.completion,
            diagnostics: wire.diagnostics,
        })
    }
}

/// Bounded handoff for inspecting one immutable discovery candidate without
/// rerunning global retrieval or ranking.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SimilarCodeCandidateSnapshot {
    /// Schema version of the discovery envelope that produced the candidate.
    pub schema_version: SimilarCodeSchemaVersion,
    /// Immutable provider, model, parameter, and scope provenance.
    pub generation: SimilarCodeGeneration,
    /// The exact unverified candidate selected from discovery.
    pub candidate: SimilarCodeCandidate,
    /// Original discovery completeness and limit accounting.
    pub completion: SimilarCodeCompletion,
    /// Original non-severity discovery diagnostics.
    pub diagnostics: Vec<SimilarCodeDiagnostic>,
}

/// One named graph reference used in an inspect packet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeNamedReference {
    /// Project-root-relative, forward-slash path.
    pub path: String,
    /// Referenced symbol name.
    pub name: String,
    /// One-based source line.
    pub line: u32,
}

/// Conservative syntactic side-effect hint for an inspected function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeSideEffectHint {
    /// No syntactic side-effect signal was found.
    PureLooking,
    /// The function contains a syntactic side-effect signal.
    MayHaveSideEffects,
    /// The available evidence is insufficient to classify the function.
    Unknown,
}

/// Bounded evidence for one side of an inspect packet.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeSideEvidence {
    /// Bounded source window included only in inspect output, never raw output or cache.
    pub source_window: Option<String>,
    /// Declared parameter count when extraction supplied it.
    pub parameter_count: Option<u32>,
    /// Whether the inspected function is declared async.
    pub is_async: Option<bool>,
    /// Whether the inspected function is a generator.
    pub is_generator: Option<bool>,
    /// Whether the inspected function contains an await expression.
    pub has_await: Option<bool>,
    /// Whether the inspected function contains a throw expression.
    pub has_throw: Option<bool>,
    /// Conservative syntactic side-effect classification.
    pub side_effect_hint: Option<SimilarCodeSideEffectHint>,
    /// Whether the function is reachable from a configured entry point.
    pub entry_point_reachable: Option<bool>,
    /// Bounded, deterministically ordered direct callers.
    pub callers: Vec<SimilarCodeNamedReference>,
    /// Bounded, deterministically ordered direct callees.
    pub callees: Vec<SimilarCodeNamedReference>,
    /// Bounded, deterministically ordered ownership labels.
    pub owners: Vec<String>,
    /// Recent commit count in the configured churn window.
    pub churn_commits: Option<u64>,
    /// Bounded, root-relative related test paths.
    pub tests: Vec<String>,
    /// Fraction covered by deterministic clone groups, from zero through one.
    pub deterministic_clone_coverage: Option<f64>,
    /// Runtime observation count when compatible runtime evidence is present.
    pub runtime_observations: Option<u64>,
}

/// Bounded source-grounded packet for one immutable candidate.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeInspectPacket {
    /// Candidate identity this packet describes.
    pub candidate_id: String,
    /// Content-stable review key this packet describes.
    pub review_key: String,
    /// Availability of every optional evidence source.
    pub availability: SimilarCodeEnrichmentAvailability,
    /// Graph relationship label, when relationship evidence is available.
    pub graph_relationship: Option<String>,
    /// Evidence for the candidate's first location.
    pub left: SimilarCodeSideEvidence,
    /// Evidence for the candidate's second location.
    pub right: SimilarCodeSideEvidence,
}

/// `fallow similar-code inspect --format json` output.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeInspectOutput {
    /// Independent inspect envelope schema version.
    pub schema_version: SimilarCodeInspectSchemaVersion,
    /// Fallow version that produced this output.
    pub version: ToolVersion,
    /// End-to-end elapsed milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Generation provenance copied from the candidate document.
    pub generation: SimilarCodeGeneration,
    /// Immutable raw candidate being inspected.
    pub candidate: SimilarCodeCandidate,
    /// Bounded source-grounded inspect packet.
    pub packet: SimilarCodeInspectPacket,
    /// Typed completion and boundedness accounting.
    pub completion: SimilarCodeCompletion,
    /// Non-severity diagnostics in deterministic order.
    pub diagnostics: Vec<SimilarCodeDiagnostic>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimilarCodeInspectOutputWire {
    schema_version: SimilarCodeInspectSchemaVersion,
    version: String,
    elapsed_ms: u64,
    generation: SimilarCodeGeneration,
    candidate: SimilarCodeCandidate,
    packet: SimilarCodeInspectPacket,
    completion: SimilarCodeCompletion,
    diagnostics: Vec<SimilarCodeDiagnostic>,
}

impl<'de> Deserialize<'de> for SimilarCodeInspectOutput {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let wire = SimilarCodeInspectOutputWire::deserialize(deserializer)?;
        Ok(Self {
            schema_version: wire.schema_version,
            version: ToolVersion(wire.version),
            elapsed_ms: ElapsedMs(wire.elapsed_ms),
            generation: wire.generation,
            candidate: wire.candidate,
            packet: wire.packet,
            completion: wire.completion,
            diagnostics: wire.diagnostics,
        })
    }
}

/// Separate verdict input for one immutable candidate.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SimilarCodeVerdict {
    /// Snapshot identity from the raw candidate.
    pub candidate_id: String,
    /// Content-stable identity from the raw candidate.
    pub review_key: String,
    /// Whether the pair is useful enough to review. Null means undecided.
    pub candidate_worthy: Option<bool>,
    /// Whether the two functions behave equivalently. Null means undecided.
    pub behaviorally_equivalent: Option<bool>,
    /// Whether consolidation is safe. Null means undecided.
    pub refactor_safe: Option<bool>,
    /// Domain interpretation independent of the three judgments.
    pub outcome: SimilarCodeDomainOutcome,
    /// Bounded explanation grounded in the inspected sources.
    pub rationale: String,
}

impl SimilarCodeVerdict {
    /// Validate the implication chain without collapsing the three judgments.
    ///
    /// # Errors
    ///
    /// Returns an error when a positive stronger judgment contradicts a
    /// negative or unknown prerequisite.
    pub fn validate(&self) -> Result<(), SimilarCodeVerdictValidationError> {
        if self.refactor_safe == Some(true) && self.behaviorally_equivalent != Some(true) {
            return Err(SimilarCodeVerdictValidationError::RefactorSafetyRequiresEquivalence);
        }
        if self.behaviorally_equivalent == Some(true) && self.candidate_worthy != Some(true) {
            return Err(SimilarCodeVerdictValidationError::EquivalenceRequiresCandidate);
        }
        Ok(())
    }
}

/// Validation failures for the independent verdict judgments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimilarCodeVerdictValidationError {
    /// Refactor safety cannot be positive without behavioral equivalence.
    RefactorSafetyRequiresEquivalence,
    /// Behavioral equivalence cannot be positive for a rejected candidate.
    EquivalenceRequiresCandidate,
}

impl fmt::Display for SimilarCodeVerdictValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RefactorSafetyRequiresEquivalence => {
                formatter.write_str("refactor_safe=true requires behaviorally_equivalent=true")
            }
            Self::EquivalenceRequiresCandidate => {
                formatter.write_str("behaviorally_equivalent=true requires candidate_worthy=true")
            }
        }
    }
}

impl std::error::Error for SimilarCodeVerdictValidationError {}

/// Domain outcome assigned by review, never by candidate generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeDomainOutcome {
    /// Functions implement the same responsibility.
    SameResponsibility,
    /// Functions are related but intentionally serve distinct responsibilities.
    RelatedButDistinct,
    /// Duplication is understood and intentional.
    IntentionalDuplication,
    /// Pair is not meaningfully related.
    Unrelated,
    /// Available evidence does not support a verdict.
    NeedsHumanReview,
}

/// Versioned external verdict document consumed by review.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SimilarCodeVerdictInput {
    /// Independent verdict document schema version.
    pub schema_version: SimilarCodeVerdictSchemaVersion,
    /// Verdicts in deterministic candidate order.
    pub verdicts: Vec<SimilarCodeVerdict>,
}

/// How review matched an external verdict to the current candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SimilarCodeVerdictMatch {
    /// Verdict matched the exact snapshot candidate identity.
    CandidateId,
    /// Verdict was rebound unambiguously through both content digests.
    ReviewKey,
    /// No verdict was supplied for this candidate.
    Unverified,
    /// Review-key rebinding was ambiguous and therefore refused.
    AmbiguousReviewKey,
}

/// One candidate joined with its separate verdict, if safely matched.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeReviewedCandidate {
    /// Immutable raw candidate, unchanged by review.
    pub candidate: SimilarCodeCandidate,
    /// Safely matched external verdict, absent when still unverified.
    pub verdict: Option<SimilarCodeVerdict>,
    /// Match or abstention path used by review.
    pub verdict_match: SimilarCodeVerdictMatch,
    /// Domain outcome. Unverified or ambiguous entries use `needs-human-review`.
    pub outcome: SimilarCodeDomainOutcome,
}

/// Digests that make the review join reproducible without exposing source.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeReviewProvenance {
    /// SHA-256 digest of the exact candidate JSON input bytes.
    pub candidates_sha256: String,
    /// SHA-256 digest of the exact verdict JSON input bytes.
    pub verdicts_sha256: String,
}

/// `fallow similar-code review --format json` output.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SimilarCodeReviewOutput {
    /// Independent review envelope schema version.
    pub schema_version: SimilarCodeReviewSchemaVersion,
    /// Fallow version that produced this output.
    pub version: ToolVersion,
    /// End-to-end elapsed milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Immutable generation provenance copied from the candidate document.
    pub generation: SimilarCodeGeneration,
    /// Input document provenance for this deterministic join.
    pub review: SimilarCodeReviewProvenance,
    /// Raw candidates joined with verdicts in candidate order.
    pub candidates: Vec<SimilarCodeReviewedCandidate>,
    /// Typed completion and boundedness accounting.
    pub completion: SimilarCodeCompletion,
    /// Non-severity diagnostics in deterministic order.
    pub diagnostics: Vec<SimilarCodeDiagnostic>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimilarCodeReviewOutputWire {
    schema_version: SimilarCodeReviewSchemaVersion,
    version: String,
    elapsed_ms: u64,
    generation: SimilarCodeGeneration,
    review: SimilarCodeReviewProvenance,
    candidates: Vec<SimilarCodeReviewedCandidate>,
    completion: SimilarCodeCompletion,
    diagnostics: Vec<SimilarCodeDiagnostic>,
}

impl<'de> Deserialize<'de> for SimilarCodeReviewOutput {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let wire = SimilarCodeReviewOutputWire::deserialize(deserializer)?;
        Ok(Self {
            schema_version: wire.schema_version,
            version: ToolVersion(wire.version),
            elapsed_ms: ElapsedMs(wire.elapsed_ms),
            generation: wire.generation,
            review: wire.review,
            candidates: wire.candidates,
            completion: wire.completion,
            diagnostics: wire.diagnostics,
        })
    }
}

/// Serialize raw similar-code output with its root discriminator.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_similar_code_json_output(
    output: SimilarCodeOutput,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "similar-code", mode)
}

/// Serialize a similar-code inspect packet with its root discriminator.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_similar_code_inspect_json_output(
    output: SimilarCodeInspectOutput,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "similar-code-inspect", mode)
}

/// Serialize reviewed similar-code output with its root discriminator.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_similar_code_review_json_output(
    output: SimilarCodeReviewOutput,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "similar-code-review", mode)
}

/// Serialize local-provider status with its root discriminator.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_similar_code_status_json_output(
    output: SimilarCodeStatusOutput,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "similar-code-status", mode)
}

/// Serialize a vector-cache clear result with its root discriminator.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_similar_code_cache_clear_json_output(
    output: SimilarCodeCacheClearOutput,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "similar-code-cache-clear", mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_axes_remain_independent_but_enforce_implication() {
        let verdict = SimilarCodeVerdict {
            candidate_id: "sc_123".to_string(),
            review_key: "scr_456".to_string(),
            candidate_worthy: Some(true),
            behaviorally_equivalent: Some(false),
            refactor_safe: Some(true),
            outcome: SimilarCodeDomainOutcome::RelatedButDistinct,
            rationale: "Same domain, different behavior.".to_string(),
        };

        assert_eq!(
            verdict.validate(),
            Err(SimilarCodeVerdictValidationError::RefactorSafetyRequiresEquivalence)
        );
    }

    #[test]
    fn raw_serializer_adds_only_the_similar_code_kind() {
        let output = raw_output();

        let value = serialize_similar_code_json_output(output, RootEnvelopeMode::Tagged)
            .expect("similar-code output should serialize");

        assert_eq!(value["kind"], "similar-code");
        assert_eq!(value["schema_version"], "1");
        assert!(value.get("severity").is_none());
        assert!(value.get("gate").is_none());
        assert!(value.get("fixes").is_none());
    }

    #[test]
    fn raw_output_round_trips_for_review_input() {
        let value = serde_json::to_value(raw_output()).expect("raw output should serialize");
        let decoded: SimilarCodeOutput =
            serde_json::from_value(value).expect("raw output should deserialize");

        assert_eq!(decoded.schema_version, SimilarCodeSchemaVersion::V1);
        assert_eq!(decoded.version.0, "3.9.0");
        assert_eq!(
            decoded.completion.status,
            SimilarCodeCompletionStatus::Complete
        );
    }

    #[test]
    fn verdict_input_rejects_unknown_fields() {
        let value = serde_json::json!({
            "schema_version": "1",
            "verdicts": [],
            "unexpected": true
        });

        assert!(serde_json::from_value::<SimilarCodeVerdictInput>(value).is_err());
    }

    #[test]
    fn review_preserves_null_judgments() {
        let verdict = SimilarCodeVerdict {
            candidate_id: "sc_123".to_string(),
            review_key: "scr_456".to_string(),
            candidate_worthy: None,
            behaviorally_equivalent: None,
            refactor_safe: None,
            outcome: SimilarCodeDomainOutcome::NeedsHumanReview,
            rationale: "Insufficient evidence.".to_string(),
        };

        let value = serde_json::to_value(verdict).expect("verdict should serialize");
        assert!(value["candidate_worthy"].is_null());
        assert!(value["behaviorally_equivalent"].is_null());
        assert!(value["refactor_safe"].is_null());
    }

    fn generation() -> SimilarCodeGeneration {
        SimilarCodeGeneration {
            extraction_semantics_version: 1,
            embedding_semantics_version: 1,
            provider: SimilarCodeProviderProvenance {
                provider: SimilarCodeProvider::OfficialLocalCompanion,
                companion_version: "3.9.0".to_string(),
                protocol_version: 2,
                source_left_machine: false,
            },
            model: SimilarCodeModelProvenance {
                model_id: "example/model".to_string(),
                revision: "immutable-revision".to_string(),
                artifact_sha256: "abc".to_string(),
                license: "Apache-2.0".to_string(),
                dimensions: 384,
            },
            parameters: SimilarCodeGenerationParameters {
                dtype: "fp32".to_string(),
                pooling: "mean".to_string(),
                normalized: true,
                batch_size: 8,
                max_tokens: 1024,
                parameter_sha256: "def".to_string(),
            },
            scope: SimilarCodeScopeProvenance {
                active: true,
                paths: vec!["src/a.ts".to_string()],
            },
            threshold: 0.8,
            min_lines: 3,
        }
    }

    fn raw_output() -> SimilarCodeOutput {
        SimilarCodeOutput {
            schema_version: SimilarCodeSchemaVersion::V1,
            version: ToolVersion("3.9.0".to_string()),
            elapsed_ms: ElapsedMs(4),
            generation: generation(),
            candidates: Vec::new(),
            completion: completion(),
            diagnostics: Vec::new(),
        }
    }

    fn completion() -> SimilarCodeCompletion {
        SimilarCodeCompletion {
            status: SimilarCodeCompletionStatus::Complete,
            phases: Vec::new(),
            limits: SimilarCodeLimits {
                max_files: 10,
                max_functions: 100,
                max_source_bytes: 1_000_000,
                max_function_bytes: 10_000,
                max_batch_size: 8,
                max_vector_bytes: 1_000_000,
                max_comparisons: 1_000,
                max_candidates: 10,
                max_neighbors_per_function: 3,
                timeout_ms: 30_000,
            },
            skips: Vec::new(),
            cache: SimilarCodeCacheSummary {
                status: SimilarCodeCacheStatus::Miss,
                hits: 0,
                misses: 0,
                writes: 0,
                invalid_entries: 0,
            },
            provider_inference_ms: 0,
        }
    }
}
