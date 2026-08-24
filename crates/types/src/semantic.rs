//! Shared contracts for opt-in TypeScript semantic analysis.
//!
//! These types describe project-wide evidence that complements Fallow's
//! syntactic graph. They do not model TypeScript compiler diagnostics or
//! generic typed lint findings.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::serde_path;

/// Semantic capabilities that can share one TypeScript Program session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SemanticCapability {
    /// Confirm exact project-wide symbol use for an existing finding.
    SymbolUse,
    /// Explain exact declarations, references, aliases, and re-exports.
    SymbolTrace,
    /// Describe package-public signatures and private type leaks.
    ApiSurface,
    /// Find exact symbol consumers, affected files, and targeted tests.
    SymbolImpact,
    /// Measure project-local public-signature coupling.
    TypeCoupling,
}

/// Effective policy applied when semantic evidence is incomplete.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SemanticCompletenessRequirement {
    /// Keep incomplete semantic evidence advisory.
    #[default]
    BestEffort,
    /// Make incomplete semantic evidence fail the command.
    Complete,
}

/// Whether the semantic backend answered every requested query safely.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SemanticCompleteness {
    /// Every requested query completed without omissions.
    Complete,
    /// Some evidence is valid, but bounded or unsupported relations remain.
    Partial,
    /// No safe semantic assertion could be made.
    #[default]
    Unavailable,
}

/// Analysis mode stored with baselines, snapshots, audit sides, and impact data.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SemanticAnalysisMode {
    /// Normal Fallow analysis with no TypeScript semantic backend.
    #[default]
    Syntactic,
    /// Opt-in analysis with one or more semantic capabilities.
    TypeAware,
}

/// Compatibility identity for comparing two analysis results.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticAnalysisIdentity {
    /// Syntactic or type-aware analysis mode.
    pub mode: SemanticAnalysisMode,
    /// Version of the semantic result schema, independent of tool versions.
    pub semantic_schema_version: u32,
    /// Sorted capability set requested for the analysis.
    pub capabilities: Vec<SemanticCapability>,
    /// Hash of normalized project ownership and compiler configuration.
    pub project_config_hash: String,
    /// Backend family, such as `typescript-go`.
    pub backend_family: String,
    /// Completeness of the resulting semantic analysis.
    pub completeness: SemanticCompleteness,
}

/// Project-config identity used only when no semantic query was needed.
///
/// A clean run has no checker evidence whose compatibility depends on a
/// concrete TypeScript project. Stored comparisons therefore treat this value
/// as deferred until a later run has an actual semantic candidate.
pub const DEFERRED_PROJECT_CONFIG_HASH: &str = "deferred:no-semantic-queries";

impl Default for SemanticAnalysisIdentity {
    fn default() -> Self {
        Self {
            mode: SemanticAnalysisMode::Syntactic,
            semantic_schema_version: 1,
            capabilities: Vec::new(),
            project_config_hash: String::new(),
            backend_family: String::new(),
            completeness: SemanticCompleteness::Complete,
        }
    }
}

impl SemanticAnalysisIdentity {
    /// Identity used by legacy and current analyses that did not request the
    /// optional TypeScript semantic backend.
    #[must_use]
    pub fn syntactic() -> Self {
        Self::default()
    }

    /// Name the compatibility fields that differ between two stored results.
    /// Tool, package, protocol, and exact backend versions are provenance and
    /// therefore intentionally excluded.
    #[must_use]
    pub fn incompatible_fields(&self, other: &Self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if self.mode != other.mode {
            fields.push("mode");
        }
        if self.semantic_schema_version != other.semantic_schema_version {
            fields.push("semantic_schema_version");
        }
        if self.capabilities != other.capabilities {
            fields.push("capabilities");
        }
        let project_hash_deferred = self.project_config_hash == DEFERRED_PROJECT_CONFIG_HASH
            || other.project_config_hash == DEFERRED_PROJECT_CONFIG_HASH;
        if !project_hash_deferred && self.project_config_hash != other.project_config_hash {
            fields.push("project_config_hash");
        }
        if self.backend_family != other.backend_family {
            fields.push("backend_family");
        }
        if self.completeness != other.completeness {
            fields.push("completeness");
        }
        fields
    }
}

/// Value or type namespace for one exact declaration or reference.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SemanticNamespace {
    /// Runtime value namespace.
    #[default]
    Value,
    /// Type-only namespace.
    Type,
}

/// Stable identity for a declaration sent to or returned by the backend.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticSymbol {
    /// Project-root-relative declaration path.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Value or type namespace.
    pub namespace: SemanticNamespace,
    /// Stable declaration kind, such as `function` or `class-method`.
    pub declaration_kind: String,
    /// Name exposed to consumers.
    pub exported_name: String,
    /// Local declaration name.
    pub local_name: String,
    /// Optional owning class or namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// One-based declaration line.
    pub line: u32,
    /// Zero-based UTF-8 byte column.
    pub col: u32,
}

/// One project-root-relative source location used as semantic evidence.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticSourceLocation {
    /// Project-root-relative source path.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// One-based source line.
    pub line: u32,
    /// Zero-based UTF-8 byte column.
    pub col: u32,
}

/// Stable reason why semantic evidence is partial or unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SemanticGapReason {
    /// No selected TypeScript project owns the target.
    NoProject,
    /// More than one owning project produced incompatible declaration evidence.
    AmbiguousProject,
    /// Structural diagnostics make the selected project unsafe to query.
    BlockingDiagnostics,
    /// Named exports from a Svelte virtual module are unavailable without a
    /// framework-aware TypeScript-Go host.
    SvelteVirtualModuleExports,
    /// The exact declaration could not be resolved.
    UnknownSymbol,
    /// A requested package-public entry point could not be resolved.
    UnknownEntryPoint,
    /// Evidence was truncated at the configured limit.
    EvidenceLimit,
    /// Dynamic runtime behavior is outside checker-visible semantics.
    DynamicBehavior,
    /// Interface, inherited, or virtual dispatch may reach an implementation
    /// without referencing that concrete method symbol.
    VirtualDispatch,
    /// A computed or reflective member access can address the declaration.
    DynamicMemberAccess,
    /// A decorator can consume the declaration outside normal references.
    DecoratedDeclaration,
    /// An optional interface or inherited contract makes deletion unsafe.
    OptionalContract,
    /// A getter or setter has a paired accessor that must be changed atomically.
    AccessorPair,
    /// The declaration participates in a method overload set.
    OverloadSet,
    /// Source comments are attached to the declaration and must be reviewed.
    AttachedComment,
    /// The candidate itself declares an abstract contract.
    AbstractDeclaration,
    /// Not every project that owns the declaration completed the query.
    IncompleteProjectCoverage,
    /// An external framework declaration could not be attributed to an exact
    /// package.
    FrameworkContractProvenance,
    /// A configured request or response capacity was reached.
    Capacity,
    /// Checker references exist, but every referenced file is unreachable
    /// from the analyzed entry points and therefore cannot refute dead code.
    UnreachableEvidence,
    /// Checker evidence consists only of re-export declarations, which do not
    /// prove that any reachable consumer reads the exposed binding.
    NonCreditingEvidence,
    /// The requested syntax or declaration kind is unsupported.
    UnsupportedSyntax,
}

/// Counted omission attached to a partial or unavailable result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticOmission {
    /// Stable omission reason.
    pub reason_code: SemanticGapReason,
    /// Number of omitted items or relations.
    pub count: usize,
}

/// Conservative outcome for one existing Fallow dead-code candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SemanticCandidateDecisionKind {
    /// The checker resolved at least one exact static reference.
    ConfirmedUsed,
    /// Removing the declaration would change inherited behavior or an implemented contract.
    ContractPreserved,
    /// Complete closed-world analysis found no checker-resolved references.
    ConfirmedNoStaticReferences,
    /// Analysis deliberately declined a closed-world assertion.
    RetainedAbstained,
    /// The exact declaration or owning project could not be resolved.
    RetainedUnresolved,
}

/// How a class member participates in an inherited or implemented relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SemanticContractRelation {
    /// A required member declared by an implemented interface.
    InterfaceImplementation,
    /// An abstract base member implemented by the candidate.
    AbstractImplementation,
    /// A concrete inherited member overridden by the candidate.
    Override,
    /// An optional inherited or interface member that still blocks auto-fix.
    OptionalContract,
}

/// Exact declaration evidence for an inherited or implemented class-member relation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticContractEvidence {
    /// Contract relation that makes deletion unsafe.
    pub relation: SemanticContractRelation,
    /// Exact interface or base-class declaration.
    pub declaration: SemanticSymbol,
    /// Whether the source contract marks this member optional.
    pub optional: bool,
}

/// Heritage relation used by a framework-owned class-member contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SemanticFrameworkRelation {
    /// The class extends a framework base class.
    Extends,
    /// The class implements a framework interface.
    Implements,
}

/// Exact framework contract supplied by a detected Fallow plugin.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticFrameworkContract {
    /// Fallow plugin that supplied the contract.
    pub framework: String,
    /// Package that must own the resolved heritage declaration.
    pub package: String,
    /// Exported base class or interface name.
    pub heritage_symbol: String,
    /// Syntactic spellings accepted only for surfacing a latent candidate.
    pub heritage_names: Vec<String>,
    /// Extends or implements relation.
    pub relation: SemanticFrameworkRelation,
    /// Framework-dispatched members covered by this contract.
    pub members: Vec<String>,
}

/// Checker-validated evidence that a framework contract preserves a member.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticFrameworkContractEvidence {
    /// Fallow plugin that supplied the contract.
    pub framework: String,
    /// Exact package that owns the heritage declaration.
    pub package: String,
    /// Extends or implements relation.
    pub relation: SemanticFrameworkRelation,
    /// Exact framework base class or interface declaration.
    pub declaration: SemanticSymbol,
}

/// Exact source span and content hash used to guard semantic source edits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticEditGuard {
    /// Zero-based UTF-8 byte offset where the declaration starts.
    pub start: usize,
    /// Exclusive zero-based UTF-8 byte offset where the declaration ends.
    pub end: usize,
    /// Lowercase SHA-256 digest of the exact declaration text.
    pub declaration_sha256: String,
}

/// Bounded, inspectable decision record for one semantic dead-code candidate.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticCandidateDecision {
    /// Query identifier used to correlate low-level query metadata.
    pub query_id: usize,
    /// Exact candidate declaration.
    pub subject: SemanticSymbol,
    /// Conservative Fallow-owned decision.
    pub decision: SemanticCandidateDecisionKind,
    /// Completeness of the supporting semantic evidence.
    pub status: SemanticCompleteness,
    /// Every selected TypeScript project that owns the declaration.
    pub owning_projects: Vec<String>,
    /// Bounded checker-resolved reference or uncertainty evidence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<SemanticReference>,
    /// Inherited contract evidence, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<SemanticContractEvidence>,
    /// Framework-owned contract evidence, distinct from TypeScript contracts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework_contract: Option<SemanticFrameworkContractEvidence>,
    /// Whether this exact decision may enable a guarded class-member fix.
    pub closed_world_eligible: bool,
    /// Exact declaration guard required before a source edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_guard: Option<SemanticEditGuard>,
    /// Primary reason when the candidate remains unresolved or abstained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<SemanticGapReason>,
    /// Concise explanation suitable for dry-run and agent output.
    pub explanation: String,
    /// Plain next actions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
    /// Evidence count before bounding.
    pub total_evidence_count: usize,
    /// Whether evidence was truncated.
    pub truncated: bool,
    /// Counted omissions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omissions: Vec<SemanticOmission>,
}

/// Compact per-query status embedded in run metadata.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticQuerySummary {
    /// Stable query identifier within this analysis run.
    pub query_id: usize,
    /// Capability that answered the query.
    pub capability: SemanticCapability,
    /// Operation-specific assertion, never a generic compiler verdict.
    pub assertion: String,
    /// Completeness of this query.
    pub status: SemanticCompleteness,
    /// Stable primary gap reason when partial or unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<SemanticGapReason>,
    /// Evidence count before bounding.
    pub total_evidence_count: usize,
    /// Whether evidence or payload arrays were truncated.
    pub truncated: bool,
    /// Counted omissions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omissions: Vec<SemanticOmission>,
    /// Plain next actions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

/// Located semantic reference evidence.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticReference {
    /// Project-root-relative reference path.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// One-based source line.
    pub line: u32,
    /// Zero-based UTF-8 byte column.
    pub col: u32,
    /// Reference role, such as `read`, `type`, `alias`, or `re-export`.
    pub role: String,
    /// Value or type namespace used at this location.
    pub namespace: SemanticNamespace,
    /// Alias and re-export hops between the reference and declaration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<SemanticAliasHop>,
}

/// One alias or re-export hop in semantic provenance.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticAliasHop {
    /// Project-root-relative hop path.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name before this hop.
    pub from_name: String,
    /// Name exposed after this hop.
    pub to_name: String,
    /// Relation, such as `import-alias` or `re-export`.
    pub relation: String,
}

/// Typed semantic trace attached to an existing syntactic trace.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticSymbolTrace {
    /// Exact target declaration.
    pub target: SemanticSymbol,
    /// Semantic mode, capabilities, project selection, and completeness.
    pub identity: SemanticAnalysisIdentity,
    /// TypeScript project selected for this symbol.
    pub selected_project: String,
    /// Concrete assertion, such as `references-found`.
    pub assertion: String,
    /// Completeness of the trace.
    pub status: SemanticCompleteness,
    /// Bounded reference evidence.
    pub references: Vec<SemanticReference>,
    /// Count before evidence bounding.
    pub total_reference_count: usize,
    /// Exact reference locations found by the TypeScript checker.
    pub checker_evidence_count: usize,
    /// Alias and re-export hops derived from the semantic graph.
    pub graph_evidence_count: usize,
    /// Whether reference evidence was truncated.
    pub truncated: bool,
    /// Counted omissions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omissions: Vec<SemanticOmission>,
    /// Plain next actions for a user or automation consumer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

/// One project-local type referenced by a public signature.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PublicTypeReference {
    /// Referenced declaration.
    pub declaration: SemanticSymbol,
    /// Signature relation, such as return type or generic constraint.
    pub relation: String,
}

/// One package-public API entry described by the semantic backend.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ApiSurfaceEntry {
    /// Symbol exposed through a package entry point.
    pub exposed: SemanticSymbol,
    /// Canonical origin after aliases and re-exports.
    pub origin: SemanticSymbol,
    /// Stable normalized signature fingerprint.
    pub signature_fingerprint: String,
    /// Project-local types referenced by the signature.
    pub referenced_types: Vec<PublicTypeReference>,
}

/// Exact semantic evidence for a private type leak.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticPrivateTypeLeak {
    /// Public symbol whose signature exposes the type.
    pub exposed: SemanticSymbol,
    /// Project-local declaration that is not package-public.
    pub private_declaration: SemanticSymbol,
    /// Signature relation through which the type is exposed.
    pub relation: String,
    /// Stable TypeScript diagnostic code used as supporting evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_code: Option<u32>,
}

/// Package API surface result shared by inspect and private-leak analysis.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ApiSurfaceResult {
    /// Concrete assertion, such as `leak-confirmed`.
    pub assertion: String,
    /// Completeness of package-public traversal.
    pub status: SemanticCompleteness,
    /// Public API entries.
    pub entries: Vec<ApiSurfaceEntry>,
    /// Confirmed private type leaks.
    pub private_type_leaks: Vec<SemanticPrivateTypeLeak>,
    /// Counted omissions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omissions: Vec<SemanticOmission>,
    /// Plain next actions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

/// One production file or test reached by exact-symbol impact analysis.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticImpactPath {
    /// Project-root-relative affected path.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Relation to the target, such as `direct-value-consumer`.
    pub relation: String,
    /// Shortest graph distance from the target.
    pub distance: usize,
    /// Located provenance path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub via: Vec<PathBuf>,
}

/// Confidence of exact-symbol impact analysis after known dynamic gaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SemanticImpactConfidence {
    /// All reported static paths are complete within the selected project
    /// scope.
    High,
    /// Static paths are useful, but virtual dispatch or dynamic behavior
    /// bounds completeness.
    Bounded,
    /// Impact analysis could not run.
    Unavailable,
}

impl std::fmt::Display for SemanticImpactConfidence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::High => "high",
            Self::Bounded => "bounded",
            Self::Unavailable => "unavailable",
        })
    }
}

/// Exact-symbol impact and targeted-test recommendation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SemanticSymbolImpact {
    /// Exact target declaration.
    pub target: SemanticSymbol,
    /// Semantic mode, capabilities, project selection, and completeness.
    pub identity: SemanticAnalysisIdentity,
    /// TypeScript project selected for this symbol.
    pub selected_project: String,
    /// Concrete assertion, such as `consumers-found`.
    pub assertion: String,
    /// Completeness of impact analysis.
    pub status: SemanticCompleteness,
    /// Files that reference the exact symbol directly.
    pub direct_consumers: Vec<SemanticImpactPath>,
    /// Direct consumer count before evidence bounding.
    pub total_direct_consumer_count: usize,
    /// Transitively affected production files.
    pub affected_files: Vec<SemanticImpactPath>,
    /// Transitive affected-file count before evidence bounding.
    pub total_affected_file_count: usize,
    /// Directly relevant test entry points.
    pub targeted_tests: Vec<SemanticImpactPath>,
    /// Targeted-test count before evidence bounding.
    pub total_targeted_test_count: usize,
    /// Confidence after accounting for dynamic behavior.
    pub confidence: SemanticImpactConfidence,
    /// Counted omissions, including dynamic behavior.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omissions: Vec<SemanticOmission>,
    /// Plain next actions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

/// One project-local public-signature type edge.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeCouplingEdge {
    /// Public API declaration that owns the signature.
    pub source: SemanticSymbol,
    /// Project-local type used by that signature.
    pub target: SemanticSymbol,
    /// Signature relation.
    pub relation: String,
    /// Source location where the public signature references the target type.
    pub evidence: SemanticSourceLocation,
    /// Scope, such as `module-export` or `package-public`.
    pub scope: String,
}

/// Per-file project-local public-signature coupling.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeCouplingFile {
    /// Project-root-relative file path.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Distinct files this file's public API depends on.
    pub public_api_depends_on: usize,
    /// Located project files this file's public API depends on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub public_api_depends_on_files: Vec<PathBuf>,
    /// Distinct files whose public types use this file.
    pub public_types_used_by: usize,
    /// Located project files whose public types use this file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub public_types_used_by_files: Vec<PathBuf>,
    /// Located public-signature edges.
    pub edges: Vec<TypeCouplingEdge>,
}

/// One project-local cycle through public-signature type dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeCouplingCycle {
    /// Ordered project-root-relative files, ending at the start file.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub files: Vec<PathBuf>,
}

/// Project summary for advisory type coupling.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeCouplingSummary {
    /// Measurement boundary, currently project-local public signatures.
    pub scope: String,
    /// Edge direction, currently directed.
    pub direction: String,
    /// Distinct project files in the selected TypeScript projects.
    pub project_size: usize,
    /// Distinct project files included in the denominator.
    pub files_analyzed: usize,
    /// Files participating in at least one project-local type edge.
    pub distinct_coupled_files: usize,
    /// Project-local public-signature edge count before evidence bounding.
    pub edge_count: usize,
    /// Percentage of analyzed files participating in a type edge.
    pub coupled_file_pct: f64,
    /// Median distinct-file type connections.
    pub p50_distinct_connections: f64,
    /// P90 distinct-file type connections.
    pub p90_distinct_connections: f64,
    /// P95 incoming distinct-file type coupling.
    pub p95_public_types_used_by: f64,
    /// P95 outgoing distinct-file type coupling.
    pub p95_public_api_depends_on: f64,
    /// Percentage of files above the adaptive high-coupling threshold.
    pub high_coupling_pct: f64,
    /// Share of edge endpoints represented by the top contributors.
    pub concentration: f64,
    /// Number of project-local public-signature cycles.
    pub cycle_count: usize,
}

/// Advisory project-local public-signature coupling report.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeCouplingReport {
    /// Semantic mode, capability, project selection, and completeness.
    pub identity: SemanticAnalysisIdentity,
    /// Concrete assertion, such as `coupling-found`.
    pub assertion: String,
    /// Completeness of coupling traversal.
    pub status: SemanticCompleteness,
    /// Project summary. Absent when analysis is unavailable, never a fake zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<TypeCouplingSummary>,
    /// Per-file coupling details.
    pub files: Vec<TypeCouplingFile>,
    /// Highest-degree files contributing to project coupling.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_contributors: Vec<TypeCouplingFile>,
    /// Located project-local type cycles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cycles: Vec<TypeCouplingCycle>,
    /// Counted omissions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omissions: Vec<SemanticOmission>,
    /// Plain next actions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_identity_reports_each_compatibility_dimension() {
        let syntactic = SemanticAnalysisIdentity::syntactic();
        assert!(syntactic.incompatible_fields(&syntactic).is_empty());

        let type_aware = SemanticAnalysisIdentity {
            mode: SemanticAnalysisMode::TypeAware,
            semantic_schema_version: 2,
            capabilities: vec![SemanticCapability::SymbolUse],
            project_config_hash: "sha256:project".to_string(),
            backend_family: "typescript-go".to_string(),
            completeness: SemanticCompleteness::Partial,
        };
        assert_eq!(
            syntactic.incompatible_fields(&type_aware),
            vec![
                "mode",
                "semantic_schema_version",
                "capabilities",
                "project_config_hash",
                "backend_family",
                "completeness",
            ]
        );

        let mut deferred = type_aware.clone();
        deferred.project_config_hash = DEFERRED_PROJECT_CONFIG_HASH.to_string();
        assert!(
            deferred
                .incompatible_fields(&type_aware)
                .iter()
                .all(|field| *field != "project_config_hash")
        );
    }
}
