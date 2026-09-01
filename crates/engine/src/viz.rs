//! Typed data contract and builder for `fallow viz`.
//!
//! The CLI runs one project analysis (dead code + duplication + complexity)
//! through [`crate::session::AnalysisSession`] and hands the retained
//! artifacts to [`build_viz_data`]. The resulting [`VizData`] is embedded as
//! JSON in the self-contained interactive HTML the `viz` command writes.
//!
//! The contract is engine-owned so the graph internals never leak past the
//! engine boundary: everything the frontend needs is resolved to file
//! indices, relative paths, and plain counts here.

use std::path::Path;

use rustc_hash::FxHashMap;
use serde::Serialize;
use serde_json::Value;

use fallow_config::{ResolvedConfig, WorkspaceInfo};
use fallow_output::HealthReport;
use fallow_types::discover::DiscoveredFile;
use fallow_types::duplicates::{CloneInstance, DuplicationReport};
use fallow_types::extract::{FunctionComplexity, ModuleInfo};
use fallow_types::results::{AnalysisResults, FeatureFlag, SecurityFinding};

use crate::module_graph::RetainedModuleGraph;

/// A file counts as a complexity hotspot at or above this cyclomatic score.
const HOTSPOT_CYCLOMATIC_FLOOR: u16 = 10;
/// Maximum bytes of clone-fragment preview shipped per clone group. The
/// budget is measured in bytes, not characters; truncation only ever cuts
/// at a line boundary, so multi-byte source cannot be sliced mid-character.
const CLONE_PREVIEW_MAX_BYTES: usize = 2000;
/// Maximum lines of clone-fragment preview shipped per clone group. The
/// preview grows to its content in the panel (no inner scroll), so this can
/// be generous; big blocks still truncate, keeping the leading context.
const CLONE_PREVIEW_MAX_LINES: usize = 32;
/// Source lines of context included on each side of the duplicated block
/// in a clone preview. A fixed window is universal: clones are frequently
/// not functions (interface fields, object literals, type aliases), so no
/// enclosing-scope detection is attempted.
const CLONE_PREVIEW_CONTEXT: usize = 4;
/// Maximum clone groups serialized into the payload. Far above any
/// legitimate report; a guardrail against multi-MB HTML on monorepos.
/// Groups keep the detector's report order, so the cap keeps the first N.
const MAX_CLONE_GROUPS: usize = 500;
/// Maximum specialized finding records serialized per analysis family.
const MAX_ANALYSIS_FINDINGS: usize = 1000;
/// Maximum located security blind-spot samples beyond the aggregate rows.
const MAX_SECURITY_BLIND_SPOT_SAMPLES: usize = 100;
/// Maximum file-health rows serialized into the browser payload.
const MAX_HEALTH_FILES: usize = 2000;
/// Edge flag bit: every import of this edge is type-only.
const EDGE_FLAG_TYPE_ONLY: u32 = 1;
/// Reason reported by every family that needs runtime evidence viz was not
/// given. Viz takes no runtime coverage input, so the Health and Security
/// lenses say so instead of presenting a static-only answer as the whole one.
const NO_RUNTIME_COVERAGE_REASON: &str = "No runtime coverage input was provided";

/// Everything [`build_viz_data`] needs from one project analysis run.
pub struct VizBuildInput<'a> {
    /// Dead-code analysis results (unused files/exports, cycles, boundaries).
    pub results: &'a AnalysisResults,
    /// Retained module graph for edges, entry points, and export counts.
    pub graph: &'a RetainedModuleGraph,
    /// Parsed modules with complexity data, when retained.
    pub modules: Option<&'a [ModuleInfo]>,
    /// Discovered source files, in `FileId` order.
    pub files: &'a [DiscoveredFile],
    /// Duplication report from the same session.
    pub duplication: &'a DuplicationReport,
    /// Discovered monorepo workspaces.
    pub workspaces: &'a [WorkspaceInfo],
    /// Resolved config (project root + boundary zones).
    pub config: &'a ResolvedConfig,
    /// Feature flag records derived from the same parsed session.
    pub feature_flags: &'a [FeatureFlag],
    /// Whether to project the HTML-only lens detail payloads.
    pub include_analysis_details: bool,
}

/// Serialized payload embedded in the viz HTML.
#[derive(Serialize)]
pub struct VizData {
    /// Project display name (root directory basename).
    pub root: String,
    /// One entry per analyzed source file, indexed by position.
    pub files: Vec<VizFile>,
    /// Import edges as `[from, to, flags]` file-index pairs.
    /// `flags` bit 0 marks an edge whose imports are all type-only.
    pub edges: Vec<[u32; 3]>,
    /// Project-wide totals for the header stat boxes.
    pub summary: VizSummary,
    /// Discovered workspaces; `VizFile.workspace` indexes into this.
    pub workspaces: Vec<VizWorkspace>,
    /// Boundary zones; `VizFile.zone` and violations index into this.
    pub zones: Vec<VizZone>,
    /// Circular-dependency cycles as file-index lists.
    pub cycles: Vec<Vec<u32>>,
    /// Clone groups; `VizFile.clone_groups` indexes into this.
    pub clones: Vec<VizCloneGroup>,
    /// Boundary violations resolved to file indices.
    pub violations: Vec<VizViolation>,
    /// Architecture findings that do not fit the legacy graph overlays alone.
    pub architecture: VizFindingAnalysis,
    /// Dependency and public-API findings, excluding unused dependencies.
    pub dependencies: VizFindingAnalysis,
    /// Real health scoring and hotspot data from the shared analysis session.
    pub health: VizHealthData,
    /// Static security candidates and explicit blind spots.
    pub security: VizSecurityData,
    /// Framework-specific findings and detector diagnostics.
    pub frameworks: VizFrameworkData,
    /// CSS and design-system findings from health analysis.
    pub styling: VizStylingData,
    /// Detected feature flag use sites.
    pub feature_flags: VizFindingAnalysis,
}

/// Honest availability state for one Viz analysis family.
#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VizAvailabilityState {
    /// The analysis ran and its count is the whole answer.
    Complete,
    /// The analysis is switched off by configuration.
    Disabled,
    /// The analysis has nothing to say about this project.
    NotApplicable,
    /// The analysis could not run, so no count can be claimed.
    Unavailable,
}

/// Count contract and availability for one analysis family.
///
/// A count is meaningful only when `state` is
/// [`VizAvailabilityState::Complete`]. Every other state carries a count of
/// zero that the frontend must render as missing data rather than as zero
/// findings.
#[derive(Serialize)]
pub struct VizAvailability {
    /// Whether the count below can be read as a result.
    pub state: VizAvailabilityState,
    /// Number of items in `unit`, valid only in the `Complete` state.
    pub count: usize,
    /// What `count` counts, such as `findings` or `files`.
    pub unit: &'static str,
    /// Why the analysis is not complete, for every non-complete state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Total before payload truncation, when the payload carries fewer items
    /// than `count`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<usize>,
}

impl VizAvailability {
    const fn complete(count: usize, unit: &'static str, truncated: Option<usize>) -> Self {
        Self {
            state: VizAvailabilityState::Complete,
            count,
            unit,
            reason: None,
            truncated,
        }
    }

    fn unavailable(unit: &'static str, reason: impl Into<String>) -> Self {
        Self {
            state: VizAvailabilityState::Unavailable,
            count: 0,
            unit,
            reason: Some(reason.into()),
            truncated: None,
        }
    }

    fn disabled(unit: &'static str, reason: impl Into<String>) -> Self {
        Self {
            state: VizAvailabilityState::Disabled,
            count: 0,
            unit,
            reason: Some(reason.into()),
            truncated: None,
        }
    }
}

/// Stable presentation record shared by finding-oriented Viz families.
#[derive(Serialize)]
pub struct VizFinding {
    kind: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    files: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    severity: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    facts: Vec<VizFindingFact>,
    actions: Vec<VizFindingAction>,
}

/// One stable scalar fact from an analyzer-specific record.
#[derive(Serialize)]
pub struct VizFindingFact {
    label: String,
    value: String,
}

/// One stable action projected from an analyzer-specific record.
#[derive(Serialize)]
pub struct VizFindingAction {
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    auto_fixable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// One finding-oriented analysis family.
#[derive(Serialize)]
pub struct VizFindingAnalysis {
    /// Whether this family ran, and how many findings it stands behind.
    pub availability: VizAvailability,
    /// Total finding count before the payload was capped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub findings_truncated: Option<usize>,
    /// The findings carried in the payload.
    pub findings: Vec<VizFinding>,
}

/// Framework findings plus detector capability metadata.
#[derive(Serialize)]
pub struct VizFrameworkData {
    /// Whether framework analysis ran, and how many findings it produced.
    pub availability: VizAvailability,
    /// Whether the per-detector capability list is trustworthy. A detector
    /// can be unavailable while findings from other detectors are complete.
    pub detector_availability: VizAvailability,
    /// Total finding count before the payload was capped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub findings_truncated: Option<usize>,
    /// The findings carried in the payload.
    pub findings: Vec<VizFinding>,
    /// Frameworks detected in the project.
    pub detected_frameworks: Vec<String>,
    /// Per-detector status, so a silent detector is distinguishable from a
    /// detector that ran and found nothing.
    pub detectors: Vec<VizFrameworkDetector>,
}

/// Status of one framework-specific detector.
#[derive(Serialize)]
pub struct VizFrameworkDetector {
    id: String,
    framework: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// Availability of the individual Health signal families.
#[derive(Serialize)]
pub struct VizHealthCapabilities {
    /// Cyclomatic and cognitive complexity findings.
    pub complexity: VizAvailability,
    /// Per-file maintainability index scores.
    pub maintainability: VizAvailability,
    /// CRAP risk scores, which need coverage to be meaningful.
    pub crap: VizAvailability,
    /// Istanbul coverage ingestion.
    pub coverage: VizAvailability,
    /// Runtime execution evidence, which needs a runtime coverage input.
    pub runtime: VizAvailability,
    /// Git churn, which needs a history walk viz does not perform.
    pub churn: VizAvailability,
    /// Churn-weighted complexity hotspots, gated on churn.
    pub hotspots: VizAvailability,
    /// Ownership attribution, which needs a history walk viz does not perform.
    pub ownership: VizAvailability,
}

/// Real file-health metrics.
#[derive(Serialize)]
pub struct VizHealthFile {
    file: u32,
    path: String,
    maintainability_index: f64,
    crap_max: f64,
    complexity_density: f64,
    fan_in: usize,
    fan_out: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    hotspot_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commits: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ownership: Option<Value>,
}

/// Health lens payload populated after the shared health runner completes.
#[derive(Serialize)]
pub struct VizHealthData {
    /// Whether the health runner completed, and how many files it scored.
    pub availability: VizAvailability,
    /// Per-signal availability, so the lens can dim what did not run.
    pub capabilities: VizHealthCapabilities,
    /// Whether the run reused the viz session's parse instead of reparsing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_parse: Option<bool>,
    /// Overall health score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Letter grade derived from `score`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    /// Mean maintainability index across scored files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_maintainability: Option<f64>,
    /// Per-file metrics carried in the payload.
    pub files: Vec<VizHealthFile>,
    /// Total scored-file count before the payload was capped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_truncated: Option<usize>,
    /// Total finding count before the payload was capped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub findings_truncated: Option<usize>,
    /// The health findings carried in the payload.
    pub findings: Vec<VizFinding>,
}

/// Styling findings plus the project-level CSS analytics and score.
#[derive(Serialize)]
pub struct VizStylingData {
    /// Whether styling analysis ran, and how many findings it produced.
    pub availability: VizAvailability,
    /// Total finding count before the payload was capped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub findings_truncated: Option<usize>,
    /// The styling findings carried in the payload.
    pub findings: Vec<VizFinding>,
    /// Project-level styling score.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Letter grade derived from `score`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    /// How much evidence the score rests on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    /// Project-level CSS analytics, rendered as-is by the lens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Value>,
}

/// One hop in a static security trace.
#[derive(Serialize)]
pub struct VizSecurityTraceHop {
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<u32>,
    path: String,
    line: u32,
    col: u32,
    role: String,
}

/// One endpoint in a typed Security taint flow.
#[derive(Serialize)]
pub struct VizSecurityEndpoint {
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<u32>,
    path: String,
    line: u32,
    col: u32,
}

/// Typed source-to-sink flow summary.
#[derive(Serialize)]
pub struct VizSecurityTaintFlow {
    source: VizSecurityEndpoint,
    sink: VizSecurityEndpoint,
    intra_module: bool,
    cross_module_hops: u32,
}

/// Static security candidate. The record deliberately contains no
/// exploitability verdict.
#[derive(Serialize)]
pub struct VizSecurityCandidate {
    id: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwe: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<u32>,
    path: String,
    line: u32,
    col: u32,
    evidence: String,
    severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    taint_confidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sink: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url_shape: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network_destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reachable_from_entry: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reachable_from_untrusted_source: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blast_radius: Option<u32>,
    crosses_boundary: bool,
    client_server_boundary: bool,
    cross_module_boundary: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    architecture_zone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dead_code: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    taint_flow: Option<VizSecurityTaintFlow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    observed_controls: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    control_verification_prompt: Option<String>,
    trace: Vec<VizSecurityTraceHop>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    taint_trace: Vec<VizSecurityTraceHop>,
    actions: Value,
}

/// Explicitly counted security blind spot.
#[derive(Serialize)]
pub struct VizSecurityBlindSpot {
    kind: String,
    count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// Security lens payload. Runtime evidence is a separate capability from the
/// always-local static candidate pass.
#[derive(Serialize)]
pub struct VizSecurityData {
    /// Whether the static candidate pass ran, and how many candidates it
    /// surfaced. Candidates are unverified, never vulnerability verdicts.
    pub availability: VizAvailability,
    /// Runtime evidence availability. Without a runtime coverage input this
    /// stays `Unavailable`, never a complete count of zero.
    pub runtime_availability: VizAvailability,
    /// The static candidates carried in the payload.
    pub candidates: Vec<VizSecurityCandidate>,
    /// How many places the static pass could not see into.
    pub blind_spot_count: usize,
    /// Total blind-spot count before the payload was capped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blind_spots_truncated: Option<usize>,
    /// The blind spots carried in the payload.
    pub blind_spots: Vec<VizSecurityBlindSpot>,
}

/// One analyzed source file.
#[derive(Serialize)]
pub struct VizFile {
    /// Root-relative path with forward slashes.
    pub path: String,
    /// File size in bytes (treemap area).
    pub size: u64,
    /// Dead-code status classification.
    pub status: VizFileStatus,
    /// Number of exports declared by the file.
    pub export_count: u16,
    /// Number of exports (values + types) reported unused.
    pub unused_export_count: u16,
    /// Whether the file is an entry point.
    pub is_entry: bool,
    /// Number of files importing this file.
    pub importer_count: u16,
    /// Number of files this file imports.
    pub import_count: u16,
    /// Index into `VizData.workspaces`, if the file belongs to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<u16>,
    /// Index into `VizData.zones`, if the file matches a boundary zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<u16>,
    /// Names of unused exports (for actionable tooltips).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unused_exports: Vec<String>,
    /// Number of functions parsed in the file.
    pub fn_count: u16,
    /// Highest cyclomatic complexity of any function in the file.
    pub max_cyclomatic: u16,
    /// Highest cognitive complexity of any function in the file.
    pub max_cognitive: u16,
    /// Total React hook calls across the file's functions.
    pub react_hooks: u16,
    /// Deepest JSX nesting across the file's functions.
    pub jsx_depth: u16,
    /// Every function in the file, sorted hardest-first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<VizFunction>,
    /// Duplicated lines in this file across all clone groups.
    pub dup_lines: u32,
    /// Indices into `VizData.clones` this file participates in.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clone_groups: Vec<u32>,
    /// Whether the file participates in any circular dependency.
    pub in_cycle: bool,
}

/// Dead-code status of a file, ordered by severity in the frontend.
#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VizFileStatus {
    /// No findings.
    Clean,
    /// Live file with one or more unused exports.
    HasUnusedExports,
    /// Entire file is unreachable.
    Unused,
    /// Configured or detected entry point.
    EntryPoint,
}

/// One function inside a file, with its complexity metrics.
#[derive(Serialize)]
pub struct VizFunction {
    /// Function name, or `<anonymous>`.
    name: String,
    /// 1-based start line.
    line: u32,
    /// McCabe cyclomatic complexity.
    cyclomatic: u16,
    /// SonarSource cognitive complexity.
    cognitive: u16,
    /// Body line count.
    lines: u32,
    /// React hook calls made directly in the body.
    hooks: u16,
    /// Deepest JSX nesting in the body.
    jsx_depth: u16,
    /// Props destructured from the first parameter.
    props: u16,
}

/// Project-wide totals for the header stat boxes.
#[derive(Serialize)]
pub struct VizSummary {
    /// Total analyzed files.
    pub total_files: usize,
    /// Total bytes across analyzed files.
    pub total_size: u64,
    /// Total import edges.
    pub total_edges: usize,
    /// Fully unused files.
    pub unused_files: usize,
    /// Unused exports (values + types).
    pub unused_exports: usize,
    /// Unused exported types.
    pub unused_types: usize,
    /// Unused dependencies (prod + dev + optional).
    pub unused_deps: usize,
    /// Imports that resolve to nothing.
    pub unresolved_imports: usize,
    /// Circular dependency cycles.
    pub circular_deps: usize,
    /// Clone groups detected.
    pub clone_groups: usize,
    /// Total duplicated lines across clone groups.
    pub duplicated_lines: usize,
    /// Boundary violations.
    pub boundary_violations: usize,
    /// Files at or above the complexity hotspot floor.
    pub hotspot_files: usize,
    /// Kept clone groups dropped by the `MAX_CLONE_GROUPS` payload cap.
    /// Present only when the clone payload was truncated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clone_groups_truncated: Option<u32>,
}

/// One discovered workspace.
#[derive(Serialize)]
pub struct VizWorkspace {
    /// Package name.
    name: String,
    /// Root-relative workspace root.
    root: String,
}

/// One configured boundary zone.
#[derive(Serialize)]
pub struct VizZone {
    /// Zone name from the boundaries config.
    name: String,
    /// Number of files classified into this zone.
    files: u32,
}

/// One clone group resolved to file indices.
#[derive(Serialize)]
pub struct VizCloneGroup {
    /// Lines per duplicated block.
    lines: usize,
    /// Tokens per duplicated block.
    tokens: usize,
    /// Where the duplicated block appears.
    instances: Vec<VizCloneInstance>,
    /// Source preview: a context window around the duplicated block, the
    /// copied lines flanked by up to `CLONE_PREVIEW_CONTEXT` surrounding
    /// source lines on each side.
    preview: String,
    /// 0-based index, among the lines of `preview`, of the first copied
    /// line. Lines before it are dimmed context.
    highlight_start: u32,
    /// Number of copied lines present in `preview`. The frontend highlights
    /// `preview` lines `[highlight_start, highlight_start + highlight_lines)`
    /// and dims the rest.
    highlight_lines: u32,
}

/// One location of a duplicated block.
#[derive(Serialize)]
pub struct VizCloneInstance {
    /// File index into `VizData.files`.
    file: u32,
    /// 1-based start line.
    start_line: u32,
    /// 1-based end line.
    end_line: u32,
}

/// One boundary violation resolved to file indices.
#[derive(Serialize)]
pub struct VizViolation {
    /// Importing file index.
    from: u32,
    /// Imported file index.
    to: u32,
    /// Index into `VizData.zones` for the importing file's zone.
    from_zone: u16,
    /// Index into `VizData.zones` for the imported file's zone.
    to_zone: u16,
    /// 1-based line of the offending import.
    line: u32,
    /// Raw import specifier.
    specifier: String,
}

/// Build the viz payload from one project analysis run.
#[must_use]
pub fn build_viz_data(input: &VizBuildInput<'_>) -> VizData {
    let root = &input.config.root;
    let index = FileIndex::new(input.files);
    let workspaces = build_workspaces(input.workspaces, root);
    let (zones, zone_by_file) = classify_zones(input, &index);
    let (clones, clone_groups_by_file, dup_lines_by_file, clone_groups_truncated) =
        build_clones(input.duplication, &index, MAX_CLONE_GROUPS);
    let cycles = build_cycles(input.results, &index);
    let violations = build_violations(input.results, &zones, &index);
    let (architecture, dependencies, security, frameworks, feature_flags) =
        if input.include_analysis_details {
            (
                build_architecture(input.results, &index, root),
                build_dependencies(input.results, &index, root),
                build_security(input.results, &index, root),
                build_frameworks(input.results, &index, root),
                build_feature_flags(input.feature_flags, &index, root),
            )
        } else {
            skipped_analysis_details()
        };

    let files = build_files(
        input,
        &index,
        &FilePropertyMaps {
            zone_by_file: &zone_by_file,
            clone_groups_by_file: &clone_groups_by_file,
            dup_lines_by_file: &dup_lines_by_file,
            cycles: &cycles,
        },
    );

    let summary = build_summary(
        input,
        &files,
        &clones,
        &cycles,
        &violations,
        clone_groups_truncated,
    );

    VizData {
        root: display_root(root),
        files,
        edges: build_edges(input.graph, &index),
        summary,
        workspaces,
        zones,
        cycles,
        clones,
        violations,
        architecture,
        dependencies,
        health: VizHealthData {
            availability: VizAvailability::unavailable("files", "Health analysis did not complete"),
            capabilities: unavailable_health_capabilities("Health analysis did not complete"),
            shared_parse: None,
            score: None,
            grade: None,
            average_maintainability: None,
            files: Vec::new(),
            files_truncated: None,
            findings_truncated: None,
            findings: Vec::new(),
        },
        security,
        frameworks,
        styling: VizStylingData {
            availability: VizAvailability::unavailable(
                "findings",
                "Styling analysis did not complete",
            ),
            findings_truncated: None,
            findings: Vec::new(),
            score: None,
            grade: None,
            confidence: None,
            summary: None,
        },
        feature_flags,
    }
}

fn skipped_analysis_details() -> (
    VizFindingAnalysis,
    VizFindingAnalysis,
    VizSecurityData,
    VizFrameworkData,
    VizFindingAnalysis,
) {
    let skipped = |unit| VizFindingAnalysis {
        availability: VizAvailability::disabled(unit, "Not needed for this Viz output format"),
        findings_truncated: None,
        findings: Vec::new(),
    };
    (
        skipped("violations"),
        skipped("findings"),
        VizSecurityData {
            availability: VizAvailability::disabled(
                "candidates",
                "Not needed for this Viz output format",
            ),
            runtime_availability: VizAvailability::disabled(
                "observations",
                "Not needed for this Viz output format",
            ),
            candidates: Vec::new(),
            blind_spot_count: 0,
            blind_spots_truncated: None,
            blind_spots: Vec::new(),
        },
        VizFrameworkData {
            availability: VizAvailability::disabled(
                "findings",
                "Not needed for this Viz output format",
            ),
            detector_availability: VizAvailability::disabled(
                "detectors",
                "Not needed for this Viz output format",
            ),
            findings_truncated: None,
            findings: Vec::new(),
            detected_frameworks: Vec::new(),
            detectors: Vec::new(),
        },
        skipped("flags"),
    )
}

/// Maps absolute paths to dense viz file indices in `FileId` order.
struct FileIndex<'a> {
    ordered: Vec<&'a DiscoveredFile>,
    by_path: FxHashMap<&'a Path, u32>,
    by_file_id: FxHashMap<u32, u32>,
}

impl<'a> FileIndex<'a> {
    fn new(files: &'a [DiscoveredFile]) -> Self {
        let mut ordered: Vec<&DiscoveredFile> = files.iter().collect();
        ordered.sort_by_key(|f| f.id.0);
        let mut by_path = FxHashMap::default();
        let mut by_file_id = FxHashMap::default();
        for (i, f) in ordered.iter().enumerate() {
            let idx = clamp_u32(i);
            by_path.insert(f.path.as_path(), idx);
            by_file_id.insert(f.id.0, idx);
        }
        Self {
            ordered,
            by_path,
            by_file_id,
        }
    }

    fn index_of_path(&self, path: &Path) -> Option<u32> {
        self.by_path.get(path).copied()
    }

    fn index_of_file_id(&self, file_id: u32) -> Option<u32> {
        self.by_file_id.get(&file_id).copied()
    }
}

fn analysis_from_records(
    mut findings: Vec<VizFinding>,
    total_findings: usize,
    primary_count: usize,
    unit: &'static str,
) -> VizFindingAnalysis {
    let findings_truncated = (total_findings > MAX_ANALYSIS_FINDINGS)
        .then_some(total_findings.saturating_sub(MAX_ANALYSIS_FINDINGS));
    let primary_truncated = (primary_count > MAX_ANALYSIS_FINDINGS)
        .then_some(primary_count.saturating_sub(MAX_ANALYSIS_FINDINGS));
    findings.truncate(MAX_ANALYSIS_FINDINGS);
    VizFindingAnalysis {
        availability: VizAvailability::complete(primary_count, unit, primary_truncated),
        findings_truncated,
        findings,
    }
}

fn push_findings<T: Serialize>(
    out: &mut Vec<VizFinding>,
    kind: &str,
    title: &str,
    values: &[T],
    root: &Path,
    index: &FileIndex<'_>,
) {
    let remaining = MAX_ANALYSIS_FINDINGS.saturating_sub(out.len());
    out.extend(values.iter().take(remaining).filter_map(|value| {
        serde_json::to_value(value).ok().map(|detail| {
            finding_from_value(kind, title, detail, root, &|path| index.index_of_path(path))
        })
    }));
}

fn finding_from_value(
    kind: &str,
    title: &str,
    mut detail: Value,
    root: &Path,
    resolve_file: &dyn Fn(&Path) -> Option<u32>,
) -> VizFinding {
    let raw_path = find_string_key(&detail, &["path", "from_path", "consumer_path", "file"])
        .map(str::to_owned);
    let mut raw_paths = Vec::new();
    collect_path_values(&detail, &mut raw_paths);
    let mut paths = Vec::new();
    let mut files = Vec::new();
    for raw in raw_paths {
        let raw_path = Path::new(&raw);
        let absolute_path = if raw_path.is_absolute() {
            raw_path.to_path_buf()
        } else {
            root.join(raw_path)
        };
        let display = relative_path(&absolute_path, root);
        if !paths.contains(&display) {
            paths.push(display);
        }
        if let Some(file) = resolve_file(&absolute_path)
            && !files.contains(&file)
        {
            files.push(file);
        }
    }
    let absolute = raw_path.as_deref().map(Path::new).map(|path| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        }
    });
    let file = absolute.as_deref().and_then(resolve_file);
    let path = absolute.as_deref().map(|path| relative_path(path, root));
    let line = find_u64_key(&detail, &["line", "start_line"])
        .map(|value| u32::try_from(value).unwrap_or(u32::MAX));
    relativize_value_paths(&mut detail, root);
    let description =
        find_string_key(&detail, &["message", "evidence", "reason"]).map(str::to_owned);
    let severity = find_string_key(&detail, &["severity"]).map(str::to_owned);
    let facts = finding_facts(&detail);
    let actions = finding_actions(&detail);
    VizFinding {
        kind: kind.to_string(),
        title: title.to_string(),
        file,
        path,
        line,
        files,
        paths,
        description,
        severity,
        facts,
        actions,
    }
}

fn finding_facts(detail: &Value) -> Vec<VizFindingFact> {
    const EXCLUDED: &[&str] = &[
        "path",
        "from_path",
        "to_path",
        "consumer_path",
        "file",
        "files",
        "paths",
        "line",
        "start_line",
        "message",
        "evidence",
        "reason",
        "severity",
        "actions",
    ];
    let Some(fields) = detail.as_object() else {
        return Vec::new();
    };
    fields
        .iter()
        .filter(|(label, _)| !EXCLUDED.contains(&label.as_str()))
        .filter_map(|(label, value)| {
            scalar_fact_value(value).map(|value| VizFindingFact {
                label: label.clone(),
                value,
            })
        })
        .take(12)
        .collect()
}

fn scalar_fact_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Array(values) if values.iter().all(Value::is_string) => Some(
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn finding_actions(detail: &Value) -> Vec<VizFindingAction> {
    let mut actions = Vec::new();
    if let Some(record) = detail.as_object() {
        for key in ["verify_command", "trace_command", "command"] {
            if let Some(command) = record.get(key).and_then(Value::as_str) {
                actions.push(VizFindingAction {
                    label: "Verify".to_string(),
                    kind: None,
                    auto_fixable: false,
                    command: Some(command.to_string()),
                    comment: None,
                    config_key: None,
                    value: None,
                    description: None,
                });
            }
        }
        if let Some(value) = record.get("actions") {
            append_projected_actions(&mut actions, value);
        }
    }
    actions
}

fn append_projected_actions(actions: &mut Vec<VizFindingAction>, value: &Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                if let Some(action) = projected_action(value) {
                    actions.push(action);
                }
            }
        }
        Value::Object(values) => {
            for (label, value) in values {
                if let Some(command) = value.as_str() {
                    actions.push(VizFindingAction {
                        label: label.clone(),
                        kind: Some(label.clone()),
                        auto_fixable: false,
                        command: Some(command.to_string()),
                        comment: None,
                        config_key: None,
                        value: None,
                        description: None,
                    });
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn projected_action(value: &Value) -> Option<VizFindingAction> {
    let action = value.as_object()?;
    let kind = ["kind", "type"]
        .iter()
        .find_map(|key| action.get(*key).and_then(Value::as_str))
        .map(str::to_owned);
    let label = ["label", "title"]
        .iter()
        .find_map(|key| action.get(*key).and_then(Value::as_str))
        .map(str::to_owned)
        .or_else(|| kind.clone())
        .unwrap_or_else(|| "Review".to_string());
    let command = action
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let comment = action
        .get("comment")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let auto_fixable = action
        .get("auto_fixable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let config_key = action
        .get("config_key")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let projected_value = action.get("value").cloned();
    let description = ["description", "note"]
        .iter()
        .find_map(|key| action.get(*key).and_then(Value::as_str))
        .map(str::to_owned);
    (command.is_some()
        || comment.is_some()
        || description.is_some()
        || config_key.is_some()
        || projected_value.is_some())
    .then_some(VizFindingAction {
        label,
        kind,
        auto_fixable,
        command,
        comment,
        config_key,
        value: projected_value,
        description,
    })
}

fn collect_path_values(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if is_path_key(key)
                    && let Some(path) = value.as_str()
                {
                    out.push(path.to_string());
                }
                if is_path_collection_key(key)
                    && let Some(values) = value.as_array()
                {
                    out.extend(values.iter().filter_map(Value::as_str).map(str::to_string));
                }
                collect_path_values(value, out);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_path_values(value, out);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn find_string_key<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key).and_then(Value::as_str) {
                    return Some(value);
                }
            }
            map.values().find_map(|value| find_string_key(value, keys))
        }
        Value::Array(values) => values.iter().find_map(|value| find_string_key(value, keys)),
        _ => None,
    }
}

fn find_u64_key(value: &Value, keys: &[&str]) -> Option<u64> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key).and_then(Value::as_u64) {
                    return Some(value);
                }
            }
            map.values().find_map(|value| find_u64_key(value, keys))
        }
        Value::Array(values) => values.iter().find_map(|value| find_u64_key(value, keys)),
        _ => None,
    }
}

fn relativize_value_paths(value: &mut Value, root: &Path) {
    relativize_keyed_paths(value, root, false);
}

fn relativize_keyed_paths(value: &mut Value, root: &Path, is_path: bool) {
    match value {
        Value::String(text) if is_path => {
            let path = Path::new(text);
            if path.is_absolute() {
                *text = relative_path(path, root);
            }
        }
        Value::Array(values) => {
            for value in values {
                relativize_keyed_paths(value, root, is_path);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                let is_path = is_path_key(key) || is_path_collection_key(key);
                relativize_keyed_paths(value, root, is_path);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn is_path_key(key: &str) -> bool {
    matches!(
        key,
        "path"
            | "file"
            | "from_path"
            | "to_path"
            | "consumer_path"
            | "source_path"
            | "definition_path"
            | "template_path"
            | "inherited_from"
            | "reachable_via"
            | "new_path"
            | "old_path"
            | "cycle_path"
            | "docs_path"
            | "meta_docs_path"
            | "full_report_path"
    )
}

fn is_path_collection_key(key: &str) -> bool {
    matches!(
        key,
        "files"
            | "paths"
            | "conflicting_paths"
            | "used_in_workspaces"
            | "hardcoded_consumers"
            | "hot_paths"
    )
}

fn serialized_label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_string())
}

fn build_architecture(
    results: &AnalysisResults,
    index: &FileIndex<'_>,
    root: &Path,
) -> VizFindingAnalysis {
    let mut findings = Vec::new();
    push_findings(
        &mut findings,
        "boundary-violation",
        "Forbidden import",
        &results.boundary_violations,
        root,
        index,
    );
    push_findings(
        &mut findings,
        "boundary-coverage",
        "File outside architecture zones",
        &results.boundary_coverage_violations,
        root,
        index,
    );
    push_findings(
        &mut findings,
        "boundary-call",
        "Forbidden call",
        &results.boundary_call_violations,
        root,
        index,
    );
    push_findings(
        &mut findings,
        "policy-violation",
        "Policy violation",
        &results.policy_violations,
        root,
        index,
    );
    push_findings(
        &mut findings,
        "circular-dependency",
        "Import cycle",
        &results.circular_dependencies,
        root,
        index,
    );
    push_findings(
        &mut findings,
        "re-export-cycle",
        "Re-export cycle",
        &results.re_export_cycles,
        root,
        index,
    );
    let violation_count = results.boundary_violations.len()
        + results.boundary_coverage_violations.len()
        + results.boundary_call_violations.len()
        + results.policy_violations.len();
    let total_findings =
        violation_count + results.circular_dependencies.len() + results.re_export_cycles.len();
    analysis_from_records(findings, total_findings, violation_count, "violations")
}

fn build_dependencies(
    results: &AnalysisResults,
    index: &FileIndex<'_>,
    root: &Path,
) -> VizFindingAnalysis {
    let mut findings = Vec::new();
    let mut count = 0;
    macro_rules! add {
        ($field:ident, $kind:literal, $title:literal) => {
            count += results.$field.len();
            push_findings(&mut findings, $kind, $title, &results.$field, root, index);
        };
    }
    add!(unresolved_imports, "unresolved-import", "Unresolved import");
    add!(
        unlisted_dependencies,
        "unlisted-dependency",
        "Unlisted dependency"
    );
    add!(
        type_only_dependencies,
        "type-only-dependency",
        "Type-only dependency"
    );
    add!(
        test_only_dependencies,
        "test-only-dependency",
        "Test-only dependency"
    );
    add!(
        dev_dependencies_in_production,
        "dev-dependency-in-production",
        "Development dependency used in production"
    );
    add!(
        duplicate_exports,
        "duplicate-export",
        "Duplicate public export"
    );
    add!(
        private_type_leaks,
        "private-type-leak",
        "Private type leaked by public API"
    );
    add!(
        unused_catalog_entries,
        "unused-catalog-entry",
        "Unused catalog entry"
    );
    add!(
        empty_catalog_groups,
        "empty-catalog-group",
        "Empty catalog group"
    );
    add!(
        unresolved_catalog_references,
        "unresolved-catalog-reference",
        "Unresolved catalog reference"
    );
    add!(
        unused_dependency_overrides,
        "unused-dependency-override",
        "Unused dependency override"
    );
    add!(
        misconfigured_dependency_overrides,
        "misconfigured-dependency-override",
        "Misconfigured dependency override"
    );
    analysis_from_records(findings, count, count, "findings")
}

fn build_frameworks(
    results: &AnalysisResults,
    index: &FileIndex<'_>,
    root: &Path,
) -> VizFrameworkData {
    let mut findings = Vec::new();
    let mut count = 0;
    macro_rules! add {
        ($field:ident, $kind:literal, $title:literal) => {
            count += results.$field.len();
            push_findings(&mut findings, $kind, $title, &results.$field, root, index);
        };
    }
    add!(
        invalid_client_exports,
        "invalid-client-export",
        "Invalid client export"
    );
    add!(
        mixed_client_server_barrels,
        "mixed-client-server-barrel",
        "Mixed client/server barrel"
    );
    add!(
        misplaced_directives,
        "misplaced-directive",
        "Misplaced framework directive"
    );
    add!(
        unprovided_injects,
        "unprovided-inject",
        "Injected value is never provided"
    );
    add!(
        unrendered_components,
        "unrendered-component",
        "Component is never rendered"
    );
    add!(route_collisions, "route-collision", "Route collision");
    add!(
        dynamic_segment_name_conflicts,
        "dynamic-segment-conflict",
        "Dynamic segment conflict"
    );
    add!(
        unused_component_props,
        "unused-component-prop",
        "Unused component prop"
    );
    add!(
        unused_component_emits,
        "unused-component-emit",
        "Unused component event"
    );
    add!(
        unused_component_inputs,
        "unused-component-input",
        "Unused component input"
    );
    add!(
        unused_component_outputs,
        "unused-component-output",
        "Unused component output"
    );
    add!(
        unused_svelte_events,
        "unused-svelte-event",
        "Unused Svelte event"
    );
    add!(
        unused_server_actions,
        "unused-server-action",
        "Unused server action"
    );
    add!(
        unused_load_data_keys,
        "unused-load-data-key",
        "Unused load-data key"
    );
    add!(prop_drilling_chains, "prop-drilling", "Prop-drilling chain");
    add!(thin_wrappers, "thin-wrapper", "Thin component wrapper");
    add!(
        duplicate_prop_shapes,
        "duplicate-prop-shape",
        "Duplicate prop shape"
    );
    let mut analysis = analysis_from_records(findings, count, count, "findings");
    if results.unused_load_data_keys_global_abstain {
        analysis.availability.reason = Some(
            "Load-data-key analysis abstained because whole-object page data usage was detected"
                .to_string(),
        );
    }
    VizFrameworkData {
        availability: analysis.availability,
        detector_availability: VizAvailability::unavailable(
            "detectors",
            "Framework detector coverage did not complete",
        ),
        findings_truncated: analysis.findings_truncated,
        findings: analysis.findings,
        detected_frameworks: Vec::new(),
        detectors: Vec::new(),
    }
}

fn build_feature_flags(
    flags: &[FeatureFlag],
    index: &FileIndex<'_>,
    root: &Path,
) -> VizFindingAnalysis {
    let mut findings = Vec::new();
    push_findings(
        &mut findings,
        "feature-flag",
        "Feature flag use",
        flags,
        root,
        index,
    );
    analysis_from_records(findings, flags.len(), flags.len(), "flags")
}

fn build_security(
    results: &AnalysisResults,
    index: &FileIndex<'_>,
    root: &Path,
) -> VizSecurityData {
    let total = results.security_findings.len();
    let truncated =
        (total > MAX_ANALYSIS_FINDINGS).then_some(total.saturating_sub(MAX_ANALYSIS_FINDINGS));
    let mut sorted_findings: Vec<&SecurityFinding> = results.security_findings.iter().collect();
    sorted_findings.sort_by_key(|finding| {
        let severity = serialized_label(&crate::security::derive_security_severity(finding));
        let priority = match severity.as_str() {
            "high" => 0,
            "medium" => 1,
            _ => 2,
        };
        (priority, relative_path(&finding.path, root), finding.line)
    });
    let candidates = sorted_findings
        .into_iter()
        .take(MAX_ANALYSIS_FINDINGS)
        .map(|finding| build_security_candidate(finding, index, root))
        .collect();

    let mut blind_spots = Vec::new();
    if results.security_unresolved_edge_files > 0 {
        blind_spots.push(VizSecurityBlindSpot {
            kind: "unresolved-dynamic-imports".to_string(),
            count: results.security_unresolved_edge_files,
            path: None,
            file: None,
            line: None,
            reason: Some("Dynamic imports prevent complete client/server reachability".to_string()),
        });
    }
    if results.security_unresolved_callee_sites > 0 {
        blind_spots.push(VizSecurityBlindSpot {
            kind: "unresolved-callee-sites".to_string(),
            count: results.security_unresolved_callee_sites,
            path: None,
            file: None,
            line: None,
            reason: Some(
                "Dynamic or computed callees could not be matched to the sink catalogue"
                    .to_string(),
            ),
        });
    }
    let diagnostic_count = results.security_unresolved_callee_diagnostics.len();
    for diagnostic in results
        .security_unresolved_callee_diagnostics
        .iter()
        .take(MAX_SECURITY_BLIND_SPOT_SAMPLES)
    {
        blind_spots.push(VizSecurityBlindSpot {
            kind: "unresolved-callee-sample".to_string(),
            count: 1,
            path: Some(relative_path(&diagnostic.path, root)),
            file: index.index_of_path(&diagnostic.path),
            line: Some(diagnostic.line),
            reason: Some(serialized_label(&diagnostic.reason)),
        });
    }

    VizSecurityData {
        availability: VizAvailability::complete(total, "candidates", truncated),
        runtime_availability: VizAvailability::unavailable(
            "observations",
            NO_RUNTIME_COVERAGE_REASON,
        ),
        candidates,
        blind_spot_count: results.security_unresolved_edge_files
            + results.security_unresolved_callee_sites,
        blind_spots_truncated: (diagnostic_count > MAX_SECURITY_BLIND_SPOT_SAMPLES)
            .then_some(diagnostic_count.saturating_sub(MAX_SECURITY_BLIND_SPOT_SAMPLES)),
        blind_spots,
    }
}

fn build_security_candidate(
    finding: &SecurityFinding,
    index: &FileIndex<'_>,
    root: &Path,
) -> VizSecurityCandidate {
    let kind = serialized_label(&finding.kind);
    let path = relative_path(&finding.path, root);
    let severity = serialized_label(&crate::security::derive_security_severity(finding));
    let id = crate::security::security_finding_id(finding, Path::new(&path));
    let reachability = finding.reachability.as_ref();
    let architecture_zone = finding
        .candidate
        .boundary
        .architecture_zone
        .as_ref()
        .map(|zone| format!("{} -> {}", zone.from, zone.to));
    let dead_code = serialize_relative(finding.dead_code.as_ref(), root);
    let runtime = serialize_relative(finding.runtime.as_ref(), root);
    let taint_flow = security_taint_flow(finding, index, root);
    let observed_controls = security_controls(finding, root);
    let actions = serialize_relative_value(&finding.actions, root)
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let trace = security_trace(finding, index, root);
    let taint_trace = finding
        .reachability
        .as_ref()
        .map_or_else(Vec::new, |reachability| {
            trace_hops(&reachability.untrusted_source_trace, index, root)
        });
    VizSecurityCandidate {
        id,
        kind,
        category: finding.category.clone(),
        cwe: finding.cwe,
        file: index.index_of_path(&finding.path),
        path,
        line: finding.line,
        col: finding.col,
        evidence: finding.evidence.clone(),
        severity,
        taint_confidence: reachability
            .and_then(|reachability| reachability.taint_confidence.as_ref())
            .map(serialized_label),
        source_kind: finding.candidate.source_kind.clone(),
        sink: finding.candidate.sink.callee.clone(),
        url_shape: finding
            .candidate
            .sink
            .url_shape
            .as_ref()
            .map(serialized_label),
        network_destination: finding
            .candidate
            .network
            .as_ref()
            .and_then(|network| network.destination.clone()),
        reachable_from_entry: reachability.map(|value| value.reachable_from_entry),
        reachable_from_untrusted_source: reachability
            .map(|value| value.reachable_from_untrusted_source),
        blast_radius: reachability.map(|value| value.blast_radius),
        crosses_boundary: reachability.is_some_and(|value| value.crosses_boundary)
            || finding.candidate.boundary.client_server
            || finding.candidate.boundary.cross_module
            || architecture_zone.is_some(),
        client_server_boundary: finding.candidate.boundary.client_server,
        cross_module_boundary: finding.candidate.boundary.cross_module,
        architecture_zone,
        dead_code,
        runtime,
        taint_flow,
        observed_controls,
        control_verification_prompt: finding
            .attack_surface
            .as_ref()
            .map(|surface| surface.defensive_boundary.verification_prompt.clone()),
        trace,
        taint_trace,
        actions,
    }
}

fn serialize_relative<T: Serialize>(value: Option<&T>, root: &Path) -> Option<Value> {
    value.and_then(|value| serialize_relative_value(value, root))
}

fn serialize_relative_value<T: Serialize>(value: &T, root: &Path) -> Option<Value> {
    let mut serialized = serde_json::to_value(value).ok()?;
    relativize_value_paths(&mut serialized, root);
    Some(serialized)
}

fn security_controls(finding: &SecurityFinding, root: &Path) -> Vec<Value> {
    finding
        .attack_surface
        .as_ref()
        .map_or_else(Vec::new, |surface| {
            surface
                .defensive_boundary
                .controls
                .iter()
                .filter_map(|control| serialize_relative_value(control, root))
                .collect()
        })
}

fn security_trace(
    finding: &SecurityFinding,
    index: &FileIndex<'_>,
    root: &Path,
) -> Vec<VizSecurityTraceHop> {
    trace_hops(&finding.trace, index, root)
}

fn trace_hops(
    hops: &[fallow_types::results::TraceHop],
    index: &FileIndex<'_>,
    root: &Path,
) -> Vec<VizSecurityTraceHop> {
    hops.iter()
        .map(|hop| VizSecurityTraceHop {
            file: index.index_of_path(&hop.path),
            path: relative_path(&hop.path, root),
            line: hop.line,
            col: hop.col,
            role: serialized_label(&hop.role),
        })
        .collect()
}

fn security_taint_flow(
    finding: &SecurityFinding,
    index: &FileIndex<'_>,
    root: &Path,
) -> Option<VizSecurityTaintFlow> {
    let flow = finding.taint_flow.as_ref()?;
    let endpoint = |value: &fallow_types::results::TaintEndpoint| VizSecurityEndpoint {
        file: index.index_of_path(&value.path),
        path: relative_path(&value.path, root),
        line: value.line,
        col: value.col,
    };
    Some(VizSecurityTaintFlow {
        source: endpoint(&flow.source),
        sink: endpoint(&flow.sink),
        intra_module: flow.path.intra_module,
        cross_module_hops: flow.path.cross_module_hops,
    })
}

/// Populate Health, Framework diagnostics, and Styling from the health runner
/// that consumed the same session artifacts.
pub fn apply_health_report(data: &mut VizData, report: &HealthReport, root: &Path) {
    let by_path: FxHashMap<String, u32> = data
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.clone(), clamp_u32(index)))
        .collect();
    apply_health_data(data, report, root, &by_path);
    apply_framework_data(data, report);
    apply_styling_data(data, report, root, &by_path);
}

fn apply_health_data(
    data: &mut VizData,
    report: &HealthReport,
    root: &Path,
    by_path: &FxHashMap<String, u32>,
) {
    let resolve = |path: &Path| by_path.get(&relative_path(path, root)).copied();
    let hotspot_by_path: FxHashMap<String, &fallow_output::HotspotFinding> = report
        .hotspots
        .iter()
        .map(|hotspot| (relative_path(&hotspot.path, root), hotspot))
        .collect();
    let files = health_files(report, root, by_path, &hotspot_by_path);
    let (findings, total_findings) = health_findings(report, root, &resolve);
    let concern_count = health_concern_count(report, root, by_path);
    data.health = VizHealthData {
        availability: VizAvailability::complete(concern_count, "files", None),
        capabilities: health_capabilities(report),
        shared_parse: data.health.shared_parse,
        score: report.health_score.as_ref().map(|score| score.score),
        grade: report
            .health_score
            .as_ref()
            .map(|score| score.grade.to_string()),
        average_maintainability: report.summary.average_maintainability,
        files_truncated: report
            .file_scores
            .len()
            .checked_sub(MAX_HEALTH_FILES)
            .filter(|count| *count > 0),
        findings_truncated: total_findings
            .checked_sub(findings.len())
            .filter(|count| *count > 0),
        files,
        findings,
    };
}

fn health_concern_count(
    report: &HealthReport,
    root: &Path,
    by_path: &FxHashMap<String, u32>,
) -> usize {
    let mut files = rustc_hash::FxHashSet::default();
    let mut add = |path: &Path| {
        if let Some(file) = by_path.get(&relative_path(path, root)) {
            files.insert(*file);
        }
    };
    for finding in &report.findings {
        add(&finding.path);
    }
    for hotspot in &report.hotspots {
        add(&hotspot.path);
    }
    if let Some(gaps) = &report.coverage_gaps {
        for finding in &gaps.files {
            add(&finding.file.path);
        }
        for finding in &gaps.exports {
            add(&finding.export.path);
        }
    }
    files.len()
}

fn health_files(
    report: &HealthReport,
    root: &Path,
    by_path: &FxHashMap<String, u32>,
    hotspots: &FxHashMap<String, &fallow_output::HotspotFinding>,
) -> Vec<VizHealthFile> {
    report
        .file_scores
        .iter()
        .take(MAX_HEALTH_FILES)
        .filter_map(|score| {
            let path = relative_path(&score.path, root);
            let file = by_path.get(&path).copied()?;
            let hotspot = hotspots.get(&path).copied();
            Some(VizHealthFile {
                file,
                path,
                maintainability_index: score.maintainability_index,
                crap_max: score.crap_max,
                complexity_density: score.complexity_density,
                fan_in: score.fan_in,
                fan_out: score.fan_out,
                hotspot_score: hotspot.map(|entry| entry.score),
                commits: hotspot.map(|entry| entry.commits),
                ownership: hotspot
                    .and_then(|entry| serialize_relative(entry.ownership.as_ref(), root)),
            })
        })
        .collect()
}

fn health_findings(
    report: &HealthReport,
    root: &Path,
    resolve: &dyn Fn(&Path) -> Option<u32>,
) -> (Vec<VizFinding>, usize) {
    let coverage_count = report
        .coverage_gaps
        .as_ref()
        .map_or(0, |gaps| gaps.files.len() + gaps.exports.len());
    let total = report.findings.len() + report.hotspots.len() + coverage_count;
    let mut findings = Vec::with_capacity(total.min(MAX_ANALYSIS_FINDINGS));
    append_findings(
        &mut findings,
        &report.findings,
        "health-finding",
        "Health threshold exceeded",
        root,
        resolve,
    );
    append_findings(
        &mut findings,
        &report.hotspots,
        "git-hotspot",
        "Complex and frequently changed file",
        root,
        resolve,
    );
    if let Some(gaps) = &report.coverage_gaps {
        append_findings(
            &mut findings,
            &gaps.files,
            "coverage-gap-file",
            "File has no test path",
            root,
            resolve,
        );
        append_findings(
            &mut findings,
            &gaps.exports,
            "coverage-gap-export",
            "Export has no test path",
            root,
            resolve,
        );
    }
    (findings, total)
}

fn append_findings<T: Serialize>(
    out: &mut Vec<VizFinding>,
    values: &[T],
    kind: &str,
    title: &str,
    root: &Path,
    resolve: &dyn Fn(&Path) -> Option<u32>,
) {
    let remaining = MAX_ANALYSIS_FINDINGS.saturating_sub(out.len());
    out.extend(values.iter().take(remaining).filter_map(|value| {
        serde_json::to_value(value)
            .ok()
            .map(|detail| finding_from_value(kind, title, detail, root, resolve))
    }));
}

fn health_capabilities(report: &HealthReport) -> VizHealthCapabilities {
    let file_count = report.file_scores.len();
    let coverage = report.coverage_gaps.as_ref().map_or_else(
        || VizAvailability::unavailable("gaps", "Coverage gap analysis did not produce a result"),
        |gaps| VizAvailability::complete(gaps.files.len() + gaps.exports.len(), "gaps", None),
    );
    let runtime = report.runtime_coverage.as_ref().map_or_else(
        || VizAvailability::unavailable("observations", NO_RUNTIME_COVERAGE_REASON),
        |runtime| {
            VizAvailability::complete(runtime.summary.functions_tracked, "observations", None)
        },
    );
    VizHealthCapabilities {
        complexity: VizAvailability::complete(report.findings.len(), "findings", None),
        maintainability: VizAvailability::complete(file_count, "files", None),
        crap: VizAvailability::complete(file_count, "files", None),
        coverage,
        runtime,
        churn: VizAvailability::disabled("files", "Git history is not loaded by Viz"),
        hotspots: VizAvailability::disabled("files", "Git history is not loaded by Viz"),
        ownership: VizAvailability::disabled("files", "Git history is not loaded by Viz"),
    }
}

fn unavailable_health_capabilities(reason: &str) -> VizHealthCapabilities {
    VizHealthCapabilities {
        complexity: VizAvailability::unavailable("findings", reason),
        maintainability: VizAvailability::unavailable("files", reason),
        crap: VizAvailability::unavailable("files", reason),
        coverage: VizAvailability::unavailable("gaps", reason),
        runtime: VizAvailability::unavailable("observations", NO_RUNTIME_COVERAGE_REASON),
        churn: VizAvailability::unavailable("files", reason),
        hotspots: VizAvailability::unavailable("files", reason),
        ownership: VizAvailability::unavailable("files", reason),
    }
}

fn apply_framework_data(data: &mut VizData, report: &HealthReport) {
    let count = data.frameworks.availability.count;
    let Some(diagnostics) = &report.framework_health else {
        if count == 0 {
            data.frameworks.detector_availability = VizAvailability {
                state: VizAvailabilityState::NotApplicable,
                count: 0,
                unit: "detectors",
                reason: Some("No supported framework was detected".to_string()),
                truncated: None,
            };
            data.frameworks.availability.state = VizAvailabilityState::NotApplicable;
            data.frameworks.availability.reason =
                Some("No supported framework was detected".to_string());
        } else {
            data.frameworks.detector_availability = VizAvailability::unavailable(
                "detectors",
                "Framework detector metadata was not produced",
            );
        }
        return;
    };
    data.frameworks
        .detected_frameworks
        .clone_from(&diagnostics.detected_frameworks);
    data.frameworks.detectors = diagnostics
        .detectors
        .iter()
        .map(|detector| VizFrameworkDetector {
            id: detector.id.clone(),
            framework: detector.framework.clone(),
            status: serialized_label(&detector.status),
            reason: detector.reason.clone(),
        })
        .collect();
    data.frameworks.detector_availability =
        VizAvailability::complete(diagnostics.detectors.len(), "detectors", None);
    if diagnostics.detected_frameworks.is_empty() && count == 0 {
        data.frameworks.availability.state = VizAvailabilityState::NotApplicable;
        data.frameworks.availability.reason =
            Some("No supported framework was detected".to_string());
        data.frameworks.detector_availability.state = VizAvailabilityState::NotApplicable;
        data.frameworks.detector_availability.reason =
            Some("No supported framework was detected".to_string());
    }
}

fn apply_styling_data(
    data: &mut VizData,
    report: &HealthReport,
    root: &Path,
    by_path: &FxHashMap<String, u32>,
) {
    let resolve = |path: &Path| by_path.get(&relative_path(path, root)).copied();
    let count = report.styling_findings.len();
    let mut findings = Vec::with_capacity(count.min(MAX_ANALYSIS_FINDINGS));
    append_findings(
        &mut findings,
        &report.styling_findings,
        "styling-finding",
        "Styling health finding",
        root,
        &resolve,
    );
    let truncated = count.checked_sub(findings.len()).filter(|value| *value > 0);
    let styling = report.styling_health.as_ref();
    data.styling = VizStylingData {
        availability: report.css_analytics.as_ref().map_or_else(
            || VizAvailability {
                state: VizAvailabilityState::NotApplicable,
                count: 0,
                unit: "findings",
                reason: Some("No supported CSS or component styling was detected".to_string()),
                truncated: None,
            },
            |_| VizAvailability::complete(count, "findings", truncated),
        ),
        findings_truncated: truncated,
        findings,
        score: styling.map(|health| health.score),
        grade: styling.map(|health| health.grade.to_string()),
        confidence: styling.map(|health| serialized_label(&health.confidence)),
        summary: report
            .css_analytics
            .as_ref()
            .and_then(|analytics| serde_json::to_value(&analytics.summary).ok()),
    };
}

/// Per-file lookup maps threaded into [`build_files`].
struct FilePropertyMaps<'a> {
    zone_by_file: &'a FxHashMap<u32, u16>,
    clone_groups_by_file: &'a FxHashMap<u32, Vec<u32>>,
    dup_lines_by_file: &'a FxHashMap<u32, u32>,
    cycles: &'a [Vec<u32>],
}

fn display_root(root: &Path) -> String {
    root.file_name().map_or_else(
        || root.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn relative_path(path: &Path, root: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(root) {
        return relative.to_string_lossy().replace('\\', "/");
    }
    if path.is_absolute() {
        let name = path
            .file_name()
            .map_or_else(|| "path".into(), |name| name.to_string_lossy());
        return format!("<external>/{name}");
    }
    path.to_string_lossy().replace('\\', "/")
}

fn build_workspaces(workspaces: &[WorkspaceInfo], root: &Path) -> Vec<VizWorkspace> {
    workspaces
        .iter()
        .map(|ws| VizWorkspace {
            name: ws.name.clone(),
            root: relative_path(&ws.root, root),
        })
        .collect()
}

fn workspace_index_for(path: &Path, workspaces: &[WorkspaceInfo]) -> Option<u16> {
    let mut best: Option<(usize, usize)> = None;
    for (i, ws) in workspaces.iter().enumerate() {
        if path.starts_with(&ws.root) {
            let depth = ws.root.components().count();
            if best.is_none_or(|(_, d)| depth > d) {
                best = Some((i, depth));
            }
        }
    }
    best.map(|(i, _)| clamp_u16(i))
}

fn classify_zones(
    input: &VizBuildInput<'_>,
    index: &FileIndex<'_>,
) -> (Vec<VizZone>, FxHashMap<u32, u16>) {
    let boundaries = &input.config.boundaries;
    let mut zones: Vec<VizZone> = boundaries
        .zones
        .iter()
        .map(|z| VizZone {
            name: z.name.clone(),
            files: 0,
        })
        .collect();
    let name_to_index: FxHashMap<&str, u16> = boundaries
        .zones
        .iter()
        .enumerate()
        .map(|(i, z)| (z.name.as_str(), clamp_u16(i)))
        .collect();

    let mut zone_by_file = FxHashMap::default();
    if zones.is_empty() {
        return (zones, zone_by_file);
    }

    for (i, file) in index.ordered.iter().enumerate() {
        let rel = relative_path(&file.path, &input.config.root);
        if let Some(zone_name) = boundaries.classify_zone(&rel)
            && let Some(&zone_idx) = name_to_index.get(zone_name)
        {
            zone_by_file.insert(clamp_u32(i), zone_idx);
            zones[zone_idx as usize].files += 1;
        }
    }

    (zones, zone_by_file)
}

/// Clone payload maps: kept groups, per-file group ids, per-file duplicated
/// lines, and how many kept-groups the payload cap dropped.
type CloneMaps = (
    Vec<VizCloneGroup>,
    FxHashMap<u32, Vec<u32>>,
    FxHashMap<u32, u32>,
    u32,
);

fn build_clones(
    duplication: &DuplicationReport,
    index: &FileIndex<'_>,
    max_groups: usize,
) -> CloneMaps {
    let mut clones = Vec::new();
    let mut groups_by_file: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    let mut dup_lines_by_file: FxHashMap<u32, u32> = FxHashMap::default();
    let mut truncated: usize = 0;

    for group in &duplication.clone_groups {
        let instances: Vec<VizCloneInstance> = group
            .instances
            .iter()
            .filter_map(|inst| {
                index
                    .index_of_path(&inst.file)
                    .map(|file| VizCloneInstance {
                        file,
                        start_line: clamp_u32(inst.start_line),
                        end_line: clamp_u32(inst.end_line),
                    })
            })
            .collect();
        if instances.len() < 2 {
            continue;
        }
        if clones.len() >= max_groups {
            truncated += 1;
            continue;
        }

        let group_idx = clamp_u32(clones.len());
        for inst in &instances {
            let entry = groups_by_file.entry(inst.file).or_default();
            if entry.last() != Some(&group_idx) {
                entry.push(group_idx);
            }
            *dup_lines_by_file.entry(inst.file).or_default() +=
                inst.end_line.saturating_sub(inst.start_line) + 1;
        }

        let (preview, highlight_start, highlight_lines) = group
            .instances
            .first()
            .map(build_clone_preview)
            .unwrap_or_default();

        clones.push(VizCloneGroup {
            lines: group.line_count,
            tokens: group.token_count,
            instances,
            preview,
            highlight_start,
            highlight_lines,
        });
    }

    (
        clones,
        groups_by_file,
        dup_lines_by_file,
        clamp_u32(truncated),
    )
}

fn truncate_preview(fragment: &str) -> String {
    let mut out = String::new();
    for (i, line) in fragment.lines().enumerate() {
        if i >= CLONE_PREVIEW_MAX_LINES || out.len() + line.len() > CLONE_PREVIEW_MAX_BYTES {
            out.push('\u{2026}');
            break;
        }
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// Build the representative clone preview: a context window around the
/// duplicated block, with the highlight range located within it. Returns
/// `(preview, highlight_start, highlight_lines)` where `highlight_start`
/// is the 0-based index of the first copied line among the preview lines
/// and `highlight_lines` is the copied line count present in `preview`.
///
/// Falls back to the bare fragment with the whole block highlighted on
/// any read failure, empty source, or an out-of-range line span. Never
/// panics.
fn build_clone_preview(inst: &CloneInstance) -> (String, u32, u32) {
    let Ok(source) = std::fs::read_to_string(&inst.file) else {
        return fragment_fallback(&inst.fragment);
    };
    let lines: Vec<&str> = source.lines().collect();
    let total = lines.len();
    if total == 0 || inst.start_line == 0 || inst.start_line > total {
        return fragment_fallback(&inst.fragment);
    }

    // Block bounds as a 0-based `[block_start, block_end)` range, clamped
    // to the file and guaranteed to hold at least one line.
    let block_start = inst.start_line - 1;
    let block_end = inst.end_line.min(total).max(inst.start_line);
    let mut block_lines = block_end - block_start;
    let mut before = block_start.min(CLONE_PREVIEW_CONTEXT);
    let mut after = (total - block_end).min(CLONE_PREVIEW_CONTEXT);

    // Line cap: when the block plus its context fits, trim context
    // symmetrically to fit. When the block alone fills the cap, keep the
    // leading context (so the highlight always reads against some dimmed
    // lines) and truncate the block's tail, always keeping >= 1 block line.
    if before + block_lines + after > CLONE_PREVIEW_MAX_LINES {
        if before + block_lines >= CLONE_PREVIEW_MAX_LINES {
            after = 0;
            block_lines = CLONE_PREVIEW_MAX_LINES.saturating_sub(before).max(1);
        } else {
            trim_context(
                &mut before,
                &mut after,
                CLONE_PREVIEW_MAX_LINES - block_lines,
            );
        }
    }

    enforce_byte_cap(
        &lines,
        block_start,
        &mut before,
        &mut after,
        &mut block_lines,
    );

    let win_start = block_start - before;
    let win_end = win_start + before + block_lines + after;
    let preview = lines[win_start..win_end].join("\n");
    (preview, clamp_u32(before), clamp_u32(block_lines))
}

/// Fallback preview: the bare fragment, capped, with the whole block
/// highlighted (nothing dimmed).
fn fragment_fallback(fragment: &str) -> (String, u32, u32) {
    let preview = truncate_preview(fragment);
    let highlight_lines = if preview.is_empty() {
        0
    } else {
        preview.lines().count()
    };
    (preview, 0, clamp_u32(highlight_lines))
}

/// Reduce `before`/`after` so their sum fits `budget`, dropping from the
/// larger side first (ties favor keeping `after`) so the two flanks stay
/// balanced. Deterministic.
fn trim_context(before: &mut usize, after: &mut usize, budget: usize) {
    while *before + *after > budget {
        if *before >= *after {
            *before -= 1;
        } else {
            *after -= 1;
        }
    }
}

/// Trim the preview window to `CLONE_PREVIEW_MAX_BYTES`, dropping context
/// lines (larger side first) before ever cutting into the highlighted
/// block. If the block alone still overflows, its tail lines are dropped,
/// but at least one line is always kept.
fn enforce_byte_cap(
    lines: &[&str],
    block_start: usize,
    before: &mut usize,
    after: &mut usize,
    block_lines: &mut usize,
) {
    let window_bytes = |before: usize, after: usize, block_lines: usize| -> usize {
        let start = block_start - before;
        let end = start + before + block_lines + after;
        let separators = (end - start).saturating_sub(1);
        lines[start..end].iter().map(|l| l.len()).sum::<usize>() + separators
    };
    while window_bytes(*before, *after, *block_lines) > CLONE_PREVIEW_MAX_BYTES {
        if *before + *after > 0 {
            if *before >= *after {
                *before -= 1;
            } else {
                *after -= 1;
            }
        } else if *block_lines > 1 {
            *block_lines -= 1;
        } else {
            break;
        }
    }
}

fn build_cycles(results: &AnalysisResults, index: &FileIndex<'_>) -> Vec<Vec<u32>> {
    results
        .circular_dependencies
        .iter()
        .filter_map(|cd| {
            let ids: Vec<u32> = cd
                .cycle
                .files
                .iter()
                .filter_map(|p| index.index_of_path(p))
                .collect();
            (ids.len() == cd.cycle.files.len()).then_some(ids)
        })
        .collect()
}

fn build_violations(
    results: &AnalysisResults,
    zones: &[VizZone],
    index: &FileIndex<'_>,
) -> Vec<VizViolation> {
    let name_to_index: FxHashMap<&str, u16> = zones
        .iter()
        .enumerate()
        .map(|(i, z)| (z.name.as_str(), clamp_u16(i)))
        .collect();

    results
        .boundary_violations
        .iter()
        .filter_map(|finding| {
            let v = &finding.violation;
            let from = index.index_of_path(&v.from_path)?;
            let to = index.index_of_path(&v.to_path)?;
            let from_zone = *name_to_index.get(v.from_zone.as_str())?;
            let to_zone = *name_to_index.get(v.to_zone.as_str())?;
            Some(VizViolation {
                from,
                to,
                from_zone,
                to_zone,
                line: v.line,
                specifier: v.import_specifier.clone(),
            })
        })
        .collect()
}

fn build_edges(graph: &RetainedModuleGraph, index: &FileIndex<'_>) -> Vec<[u32; 3]> {
    let graph = graph.as_graph();
    let mut edges = Vec::with_capacity(graph.edge_count());
    for node in &graph.modules {
        let Some(source) = index.index_of_file_id(node.file_id.0) else {
            continue;
        };
        for (target_id, all_type_only, _span) in graph.outgoing_edge_summaries(node.file_id) {
            let Some(target) = index.index_of_file_id(target_id.0) else {
                continue;
            };
            let flags = if all_type_only {
                EDGE_FLAG_TYPE_ONLY
            } else {
                0
            };
            edges.push([source, target, flags]);
        }
    }
    edges
}

/// Complexity aggregates for one file, folded from its parsed functions.
#[derive(Default)]
struct ComplexityRollup {
    fn_count: u16,
    max_cyclomatic: u16,
    max_cognitive: u16,
    react_hooks: u16,
    jsx_depth: u16,
    functions: Vec<VizFunction>,
}

fn rollup_complexity(functions: &[FunctionComplexity]) -> ComplexityRollup {
    let mut rollup = ComplexityRollup {
        fn_count: clamp_u16(functions.len()),
        ..ComplexityRollup::default()
    };
    for f in functions {
        rollup.max_cyclomatic = rollup.max_cyclomatic.max(f.cyclomatic);
        rollup.max_cognitive = rollup.max_cognitive.max(f.cognitive);
        rollup.react_hooks = rollup.react_hooks.saturating_add(f.react_hook_count);
        rollup.jsx_depth = rollup.jsx_depth.max(f.react_jsx_max_depth);
    }

    // Named functions only, hardest-first: the panel lists these and folds the
    // (often many) anonymous arrow/callback functions into a single count via
    // `fn_count`. Placeholder names for unnamed functions are `<arrow>` /
    // `<anonymous>`, so a leading `<` marks the ones to fold away.
    let mut named: Vec<&FunctionComplexity> = functions
        .iter()
        .filter(|f| !f.name.starts_with('<'))
        .collect();
    named.sort_by(|a, b| {
        b.cyclomatic
            .cmp(&a.cyclomatic)
            .then(b.cognitive.cmp(&a.cognitive))
    });
    rollup.functions = named
        .into_iter()
        .map(|f| VizFunction {
            name: f.name.clone(),
            line: f.line,
            cyclomatic: f.cyclomatic,
            cognitive: f.cognitive,
            lines: f.line_count,
            hooks: f.react_hook_count,
            jsx_depth: f.react_jsx_max_depth,
            props: f.react_prop_count,
        })
        .collect();
    rollup
}

fn build_files(
    input: &VizBuildInput<'_>,
    index: &FileIndex<'_>,
    maps: &FilePropertyMaps<'_>,
) -> Vec<VizFile> {
    let graph = input.graph.as_graph();
    let unused_file_paths: rustc_hash::FxHashSet<&Path> = input
        .results
        .unused_files
        .iter()
        .map(|f| f.file.path.as_path())
        .collect();

    let mut unused_exports_by_file: FxHashMap<&Path, Vec<String>> = FxHashMap::default();
    for export in &input.results.unused_exports {
        unused_exports_by_file
            .entry(export.export.path.as_path())
            .or_default()
            .push(export.export.export_name.clone());
    }
    for export in &input.results.unused_types {
        unused_exports_by_file
            .entry(export.export.path.as_path())
            .or_default()
            .push(export.export.export_name.clone());
    }

    let mut complexity_by_file_id: FxHashMap<u32, ComplexityRollup> = FxHashMap::default();
    if let Some(modules) = input.modules {
        for module in modules {
            if !module.complexity.is_empty() {
                complexity_by_file_id
                    .insert(module.file_id.0, rollup_complexity(&module.complexity));
            }
        }
    }

    let mut in_cycle = vec![false; index.ordered.len()];
    for cycle in maps.cycles {
        for &idx in cycle {
            if let Some(slot) = in_cycle.get_mut(idx as usize) {
                *slot = true;
            }
        }
    }

    index
        .ordered
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let viz_idx = clamp_u32(i);
            let node_idx = file.id.0 as usize;
            let node = graph.modules.get(node_idx);
            let is_entry = node.is_some_and(|n| n.is_entry_point());
            let export_count = node.map_or(0, |n| clamp_u16(n.exports.len()));
            let import_count = clamp_u16(graph.edges_for(file.id).len());
            let importer_count = clamp_u16(input.graph.direct_importer_count(file.id));

            let unused_export_names = unused_exports_by_file
                .remove(file.path.as_path())
                .unwrap_or_default();
            let unused_export_count = clamp_u16(unused_export_names.len());

            let status = if unused_file_paths.contains(file.path.as_path()) {
                VizFileStatus::Unused
            } else if unused_export_count > 0 {
                VizFileStatus::HasUnusedExports
            } else if is_entry {
                VizFileStatus::EntryPoint
            } else {
                VizFileStatus::Clean
            };

            let complexity = complexity_by_file_id.remove(&file.id.0).unwrap_or_default();

            VizFile {
                path: relative_path(&file.path, &input.config.root),
                size: file.size_bytes,
                status,
                export_count,
                unused_export_count,
                is_entry,
                importer_count,
                import_count,
                workspace: workspace_index_for(&file.path, input.workspaces),
                zone: maps.zone_by_file.get(&viz_idx).copied(),
                unused_exports: unused_export_names,
                fn_count: complexity.fn_count,
                max_cyclomatic: complexity.max_cyclomatic,
                max_cognitive: complexity.max_cognitive,
                react_hooks: complexity.react_hooks,
                jsx_depth: complexity.jsx_depth,
                functions: complexity.functions,
                dup_lines: maps.dup_lines_by_file.get(&viz_idx).copied().unwrap_or(0),
                clone_groups: maps
                    .clone_groups_by_file
                    .get(&viz_idx)
                    .cloned()
                    .unwrap_or_default(),
                in_cycle: in_cycle[i],
            }
        })
        .collect()
}

fn build_summary(
    input: &VizBuildInput<'_>,
    files: &[VizFile],
    clones: &[VizCloneGroup],
    cycles: &[Vec<u32>],
    violations: &[VizViolation],
    clone_groups_truncated: u32,
) -> VizSummary {
    let results = input.results;
    VizSummary {
        total_files: files.len(),
        total_size: files.iter().map(|f| f.size).sum(),
        total_edges: input.graph.edge_count(),
        unused_files: results.unused_files.len(),
        unused_exports: results.unused_exports.len() + results.unused_types.len(),
        unused_types: results.unused_types.len(),
        unused_deps: results.unused_dependencies.len()
            + results.unused_dev_dependencies.len()
            + results.unused_optional_dependencies.len(),
        unresolved_imports: results.unresolved_imports.len(),
        circular_deps: cycles.len(),
        clone_groups: clones.len(),
        duplicated_lines: clones.iter().map(|c| c.lines * c.instances.len()).sum(),
        boundary_violations: violations.len(),
        hotspot_files: files
            .iter()
            .filter(|f| f.max_cyclomatic >= HOTSPOT_CYCLOMATIC_FLOOR)
            .count(),
        clone_groups_truncated: (clone_groups_truncated > 0).then_some(clone_groups_truncated),
    }
}

fn clamp_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn clamp_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_config::{BoundaryConfig, BoundaryZone, FallowConfig};
    use fallow_graph::graph::ModuleGraph;
    use fallow_graph::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
    use fallow_types::duplicates::{CloneGroup, CloneInstance};
    use fallow_types::extract::{ImportInfo, ImportedName};
    use fallow_types::output_dead_code::{BoundaryViolationFinding, CircularDependencyFinding};
    use fallow_types::output_format::OutputFormat;
    use fallow_types::results::{BoundaryViolation, CircularDependency};

    use super::*;
    use crate::discover::{EntryPoint, EntryPointSource, FileId};

    /// Owned fixture parts backing one [`VizBuildInput`].
    struct Fixture {
        config: ResolvedConfig,
        files: Vec<DiscoveredFile>,
        results: AnalysisResults,
        graph: crate::module_graph::RetainedModuleGraph,
        duplication: DuplicationReport,
        workspaces: Vec<WorkspaceInfo>,
    }

    impl Fixture {
        fn input(&self) -> VizBuildInput<'_> {
            VizBuildInput {
                results: &self.results,
                graph: &self.graph,
                modules: None,
                files: &self.files,
                duplication: &self.duplication,
                workspaces: &self.workspaces,
                config: &self.config,
                feature_flags: &[],
                include_analysis_details: true,
            }
        }
    }

    fn project_root() -> PathBuf {
        PathBuf::from("/viz-project")
    }

    fn discovered(id: u32, path: PathBuf, size_bytes: u64) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(id),
            path,
            size_bytes,
        }
    }

    fn import_of(target: FileId, specifier: &str) -> ResolvedImport {
        ResolvedImport {
            info: ImportInfo {
                source: specifier.to_owned(),
                imported_name: ImportedName::Named("value".to_owned()),
                local_name: "value".to_owned(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: oxc_span::Span::new(0, 0),
                source_span: oxc_span::Span::new(0, 0),
            },
            target: ResolveResult::InternalModule(target),
        }
    }

    fn zone(name: &str, pattern: &str) -> BoundaryZone {
        BoundaryZone {
            name: name.to_owned(),
            patterns: vec![pattern.to_owned()],
            auto_discover: Vec::new(),
            root: None,
        }
    }

    fn resolved_config(root: &Path) -> ResolvedConfig {
        let config = FallowConfig {
            boundaries: BoundaryConfig {
                zones: vec![zone("app", "src/**"), zone("shared", "lib/**")],
                ..BoundaryConfig::default()
            },
            ..FallowConfig::default()
        };
        config.resolve(root.to_path_buf(), OutputFormat::Json, 1, false, true, None)
    }

    fn cycle_finding(files: Vec<PathBuf>) -> CircularDependencyFinding {
        let length = files.len();
        CircularDependencyFinding::with_actions(CircularDependency {
            files,
            length,
            line: 1,
            col: 0,
            edges: Vec::new(),
            is_cross_package: false,
        })
    }

    fn violation_finding(from_path: PathBuf, to_path: PathBuf) -> BoundaryViolationFinding {
        BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path,
            to_path,
            from_zone: "app".to_owned(),
            to_zone: "shared".to_owned(),
            import_specifier: "../lib/c".to_owned(),
            line: 2,
            col: 0,
        })
    }

    fn clone_instance(file: PathBuf, start_line: usize, end_line: usize) -> CloneInstance {
        CloneInstance {
            file,
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            fragment: "const shared = 1;\nconst repeated = 2;\nconst block = 3;".to_owned(),
        }
    }

    fn clone_group(instances: Vec<CloneInstance>) -> CloneGroup {
        CloneGroup {
            instances,
            token_count: 12,
            line_count: 3,
            similarity: None,
        }
    }

    /// Synthetic project: 3 files, one import edge a to b, one resolvable
    /// cycle (a, b) plus one unresolvable, one clone group over (a, c) plus a
    /// dropped and a same-file group, one resolvable boundary violation a to
    /// c plus one unresolvable, two zones, one workspace over `lib/`.
    fn fixture_with(extra_graph_file: bool) -> Fixture {
        let root = project_root();
        let a = root.join("src/a.ts");
        let b = root.join("src/b.ts");
        let c = root.join("lib/c.ts");
        let missing = root.join("src/missing.ts");

        let files = vec![
            discovered(0, a.clone(), 100),
            discovered(1, b.clone(), 50),
            discovered(2, c.clone(), 25),
        ];

        let mut graph_files = files.clone();
        let mut imports = vec![import_of(FileId(1), "./b")];
        if extra_graph_file {
            graph_files.push(discovered(3, root.join("src/d.ts"), 10));
            imports.push(import_of(FileId(3), "./d"));
        }
        let resolved = vec![ResolvedModule {
            file_id: FileId(0),
            path: a.clone(),
            resolved_imports: imports,
            ..ResolvedModule::default()
        }];
        let entry_points = vec![EntryPoint {
            path: a.clone(),
            source: EntryPointSource::PackageJsonMain,
        }];
        let graph = crate::module_graph::RetainedModuleGraph::from(ModuleGraph::build(
            &resolved,
            &entry_points,
            &graph_files,
        ));

        let results = AnalysisResults {
            circular_dependencies: vec![
                cycle_finding(vec![a.clone(), b]),
                cycle_finding(vec![a.clone(), missing.clone()]),
            ],
            boundary_violations: vec![
                violation_finding(a.clone(), c.clone()),
                violation_finding(a.clone(), missing),
            ],
            ..AnalysisResults::default()
        };

        let duplication = DuplicationReport {
            clone_groups: vec![
                clone_group(vec![
                    clone_instance(a.clone(), 1, 3),
                    clone_instance(c, 10, 12),
                ]),
                clone_group(vec![
                    clone_instance(a.clone(), 20, 22),
                    clone_instance(root.join("outside.ts"), 1, 3),
                ]),
                clone_group(vec![
                    clone_instance(a.clone(), 30, 32),
                    clone_instance(a, 40, 42),
                ]),
            ],
            ..DuplicationReport::default()
        };

        let workspaces = vec![WorkspaceInfo {
            root: root.join("lib"),
            name: "shared-lib".to_owned(),
            is_internal_dependency: false,
        }];

        Fixture {
            config: resolved_config(&root),
            files,
            results,
            graph,
            duplication,
            workspaces,
        }
    }

    fn fixture() -> Fixture {
        fixture_with(false)
    }

    #[test]
    fn files_and_edges_use_stable_indices() {
        let fx = fixture();
        let data = build_viz_data(&fx.input());

        let paths: Vec<&str> = data.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["src/a.ts", "src/b.ts", "lib/c.ts"]);
        assert_eq!(data.edges, vec![[0, 1, 0]]);
        assert!(data.files[0].is_entry);
        assert!(matches!(data.files[0].status, VizFileStatus::EntryPoint));
        assert!(matches!(data.files[1].status, VizFileStatus::Clean));
        assert_eq!(data.files[0].import_count, 1);
        assert_eq!(data.files[1].importer_count, 1);
        assert_eq!(data.files[0].workspace, None);
        assert_eq!(data.files[2].workspace, Some(0));
        assert_eq!(data.workspaces.len(), 1);
        assert_eq!(data.workspaces[0].root, "lib");
    }

    #[test]
    fn edges_to_files_missing_from_input_are_dropped() {
        let fx = fixture_with(true);
        let data = build_viz_data(&fx.input());

        // The graph carries a to b AND a to d, but d is not in `input.files`,
        // so build_edges drops the second edge instead of emitting a
        // dangling index.
        assert_eq!(fx.graph.edge_count(), 2);
        assert_eq!(data.edges, vec![[0, 1, 0]]);
    }

    #[test]
    fn clone_groups_drop_unresolvable_and_dedup_per_file() {
        let fx = fixture();
        let data = build_viz_data(&fx.input());

        // The group whose second instance lives outside `input.files` keeps
        // only 1 resolvable instance and is dropped entirely.
        assert_eq!(data.clones.len(), 2);
        assert_eq!(data.clones[0].instances.len(), 2);
        assert_eq!(data.clones[0].instances[0].file, 0);
        assert_eq!(data.clones[0].instances[1].file, 2);
        assert_eq!(data.clones[0].lines, 3);
        assert_eq!(data.clones[0].tokens, 12);
        // Two same-file instances in one group dedup to a single group id.
        assert_eq!(data.files[0].clone_groups, vec![0, 1]);
        assert_eq!(data.files[2].clone_groups, vec![0]);
        // dup_lines sums (end minus start plus 1) per resolvable instance.
        assert_eq!(data.files[0].dup_lines, 9);
        assert_eq!(data.files[2].dup_lines, 3);
        assert_eq!(data.files[1].dup_lines, 0);
    }

    #[test]
    fn truncate_preview_caps_lines_and_bytes() {
        // Line cap: more lines than the cap in, CLONE_PREVIEW_MAX_LINES out
        // plus the ellipsis appended directly after the last kept line.
        let last_kept = CLONE_PREVIEW_MAX_LINES - 1;
        let many_lines = (0..CLONE_PREVIEW_MAX_LINES + 5)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>();
        let out = truncate_preview(&many_lines.join("\n"));
        assert_eq!(out.matches('\n').count(), CLONE_PREVIEW_MAX_LINES - 1);
        assert!(out.contains(&format!("line {last_kept}")));
        assert!(!out.contains(&format!("line {CLONE_PREVIEW_MAX_LINES}")));
        assert!(out.ends_with('\u{2026}'));

        // Byte budget: the second big line would exceed CLONE_PREVIEW_MAX_BYTES,
        // so output stops after the first line.
        let big = CLONE_PREVIEW_MAX_BYTES * 3 / 4;
        let two_long_lines = format!("{}\n{}", "a".repeat(big), "b".repeat(big));
        let out = truncate_preview(&two_long_lines);
        assert_eq!(out, format!("{}\u{2026}", "a".repeat(big)));

        // Multi-byte content over budget truncates at a line boundary and
        // never slices inside a character (4 bytes per emoji, well over budget).
        let emoji_line = "\u{1f389}".repeat(CLONE_PREVIEW_MAX_BYTES);
        let out = truncate_preview(&emoji_line);
        assert_eq!(out, "\u{2026}");
    }

    #[test]
    fn clone_preview_windows_context_around_the_block() {
        use std::io::Write as _;

        // 20 numbered source lines; the copied block covers lines 8..=11.
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        let body = (1..=20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        file.write_all(body.as_bytes()).expect("write source");
        let inst = clone_instance(file.path().to_path_buf(), 8, 11);

        let (preview, highlight_start, highlight_lines) = build_clone_preview(&inst);
        let preview_lines: Vec<&str> = preview.lines().collect();

        // Block (4 lines) plus 4 lines of context each side fits the cap, so
        // the full window is kept: 4 dimmed + 4 highlighted + 4 dimmed.
        assert_eq!(preview_lines.len(), 12);
        assert_eq!(highlight_start, 4);
        assert_eq!(highlight_lines, 4);
        assert_eq!(preview_lines.first(), Some(&"line 4"));
        let start = highlight_start as usize;
        let end = start + highlight_lines as usize;
        assert_eq!(
            &preview_lines[start..end],
            ["line 8", "line 9", "line 10", "line 11"],
        );
        // The line directly above the block is dimmed context, not copied.
        assert_eq!(preview_lines[start - 1], "line 7");
    }

    #[test]
    fn clone_preview_keeps_leading_context_when_the_block_fills_the_cap() {
        use std::io::Write as _;

        // A block far larger than the cap. The old logic zeroed the context
        // and highlighted the whole (truncated) window; the fix keeps the
        // leading context dimmed so the highlight still reads against it.
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        let body = (1..=200)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        file.write_all(body.as_bytes()).expect("write source");
        let inst = clone_instance(file.path().to_path_buf(), 50, 150);

        let (preview, highlight_start, highlight_lines) = build_clone_preview(&inst);
        let preview_lines: Vec<&str> = preview.lines().collect();

        assert_eq!(highlight_start, CLONE_PREVIEW_CONTEXT as u32);
        assert!(
            highlight_start > 0,
            "leading context must survive a huge block"
        );
        assert_eq!(preview_lines.len(), CLONE_PREVIEW_MAX_LINES);
        assert_eq!(
            highlight_lines as usize,
            CLONE_PREVIEW_MAX_LINES - CLONE_PREVIEW_CONTEXT,
        );
        assert_eq!(preview_lines[highlight_start as usize - 1], "line 49");
        assert_eq!(preview_lines[highlight_start as usize], "line 50");
    }

    #[test]
    fn clone_preview_clamps_context_at_file_start() {
        use std::io::Write as _;

        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(b"line 1\nline 2\nline 3\nline 4\nline 5")
            .expect("write source");
        // Block at the very top: no context fits above it, so the highlight
        // starts at index 0 and the trailing lines are dimmed context.
        let inst = clone_instance(file.path().to_path_buf(), 1, 2);

        let (preview, highlight_start, highlight_lines) = build_clone_preview(&inst);
        assert_eq!(highlight_start, 0);
        assert_eq!(highlight_lines, 2);
        assert_eq!(preview, "line 1\nline 2\nline 3\nline 4\nline 5");
    }

    #[test]
    fn clone_preview_falls_back_when_source_is_unreadable() {
        // A missing file forces the fragment fallback: the whole block is
        // highlighted so nothing is dimmed.
        let inst = clone_instance(project_root().join("does-not-exist.ts"), 1, 3);
        let (preview, highlight_start, highlight_lines) = build_clone_preview(&inst);
        assert_eq!(preview, inst.fragment);
        assert_eq!(highlight_start, 0);
        assert_eq!(highlight_lines as usize, preview.lines().count());
    }

    #[test]
    fn cycles_drop_when_any_member_unresolved() {
        let fx = fixture();
        let data = build_viz_data(&fx.input());

        // The a/b cycle resolves fully; the cycle referencing the missing
        // file yields no entry at all (not a partial one).
        assert_eq!(data.cycles, vec![vec![0, 1]]);
        assert!(data.files[0].in_cycle);
        assert!(data.files[1].in_cycle);
        assert!(!data.files[2].in_cycle);
        // The summary counts the rendered cycles, not the raw results, so
        // the dropped cycle does not inflate the header number.
        assert_eq!(data.summary.circular_deps, data.cycles.len());
    }

    #[test]
    fn violations_resolve_zone_and_file_indices() {
        let fx = fixture();
        let data = build_viz_data(&fx.input());

        assert_eq!(data.zones.len(), 2);
        assert_eq!(data.zones[0].name, "app");
        assert_eq!(data.zones[0].files, 2);
        assert_eq!(data.zones[1].name, "shared");
        assert_eq!(data.zones[1].files, 1);
        assert_eq!(data.files[0].zone, Some(0));
        assert_eq!(data.files[1].zone, Some(0));
        assert_eq!(data.files[2].zone, Some(1));

        // The violation whose to_path is not in `input.files` is dropped.
        assert_eq!(data.violations.len(), 1);
        let v = &data.violations[0];
        assert_eq!((v.from, v.to), (0, 2));
        assert_eq!((v.from_zone, v.to_zone), (0, 1));
        assert_eq!(v.line, 2);
        assert_eq!(v.specifier, "../lib/c");
    }

    #[test]
    fn clone_group_cap_counts_truncated_groups() {
        let fx = fixture();
        let index = FileIndex::new(&fx.files);

        // The fixture report has two keepable groups plus one dropped for
        // unresolvable instances; a cap of 1 keeps the first keepable group
        // and counts only the second as truncated (the unresolvable drop is
        // not a truncation).
        let (clones, groups_by_file, _dup_lines, truncated) =
            build_clones(&fx.duplication, &index, 1);
        assert_eq!(clones.len(), 1);
        assert_eq!(truncated, 1);
        assert!(
            groups_by_file
                .values()
                .all(|ids| ids.iter().all(|&id| (id as usize) < clones.len()))
        );

        // The default cap leaves a small report untouched and unflagged.
        let data = build_viz_data(&fx.input());
        assert_eq!(data.clones.len(), 2);
        assert_eq!(data.summary.clone_groups_truncated, None);
    }

    #[test]
    fn summary_flags_clone_truncation_only_when_nonzero() {
        let fx = fixture();
        let data = build_viz_data(&fx.input());

        let summary = build_summary(&fx.input(), &data.files, &data.clones, &[], &[], 3);
        assert_eq!(summary.clone_groups_truncated, Some(3));
        let summary = build_summary(&fx.input(), &data.files, &data.clones, &[], &[], 0);
        assert_eq!(summary.clone_groups_truncated, None);
    }

    #[test]
    fn summary_counts_match_rendered_arrays() {
        let fx = fixture();
        let data = build_viz_data(&fx.input());
        let s = &data.summary;

        assert_eq!(s.total_files, data.files.len());
        assert_eq!(s.total_size, 175);
        assert_eq!(s.total_edges, data.edges.len());
        assert_eq!(s.clone_groups, data.clones.len());
        assert_eq!(s.duplicated_lines, 12);
        assert_eq!(s.hotspot_files, 0);
        assert_eq!(s.unused_files, 0);
        assert_eq!(s.unused_exports, 0);
        // The raw results carry one unresolvable cycle and one unresolvable
        // violation; the header counts only what the arrays render.
        assert_eq!(s.circular_deps, data.cycles.len());
        assert_eq!(s.circular_deps, 1);
        assert_eq!(s.boundary_violations, data.violations.len());
        assert_eq!(s.boundary_violations, 1);
    }

    #[test]
    fn payload_keeps_counts_and_availability_explicit() {
        let fx = fixture();
        let data = build_viz_data(&fx.input());
        let value = serde_json::to_value(&data).expect("viz data serializes");

        assert_eq!(value["architecture"]["availability"]["unit"], "violations");
        assert_eq!(value["dependencies"]["availability"]["unit"], "findings");
        assert_eq!(value["security"]["availability"]["unit"], "candidates");
        assert_eq!(value["security"]["availability"]["state"], "complete");
        assert_eq!(
            value["security"]["runtime_availability"]["state"],
            "unavailable"
        );
        assert_eq!(value["health"]["availability"]["state"], "unavailable");
        assert_eq!(
            value["health"]["capabilities"]["coverage"]["state"],
            "unavailable"
        );
        assert!(value["frameworks"]["detectors"].is_array());
        assert_eq!(
            value["frameworks"]["detector_availability"]["state"],
            "unavailable"
        );
        assert!(value["styling"].get("score").is_none());
    }

    /// A completed Health run still knows nothing about production execution
    /// unless a runtime coverage input was supplied. The lens must say that
    /// instead of letting a complete static answer imply a complete one.
    #[test]
    fn health_reports_runtime_evidence_as_unavailable_without_a_runtime_input() {
        let fx = fixture();
        let mut data = build_viz_data(&fx.input());
        apply_health_report(&mut data, &HealthReport::default(), Path::new("/project"));
        let value = serde_json::to_value(&data).expect("viz data serializes");

        assert_eq!(value["health"]["availability"]["state"], "complete");
        let runtime = &value["health"]["capabilities"]["runtime"];
        assert_eq!(runtime["state"], "unavailable");
        assert_eq!(runtime["unit"], "observations");
        assert_eq!(runtime["reason"], NO_RUNTIME_COVERAGE_REASON);
        assert_eq!(runtime["count"], 0);
        assert_eq!(
            value["security"]["runtime_availability"]["reason"],
            NO_RUNTIME_COVERAGE_REASON
        );
    }

    #[test]
    fn external_absolute_paths_are_redacted() {
        let root = Path::new("/project");
        assert_eq!(
            relative_path(Path::new("/project/src/a.ts"), root),
            "src/a.ts"
        );
        assert_eq!(
            relative_path(Path::new("/Users/private/secret.ts"), root),
            "<external>/secret.ts"
        );

        let mut detail = serde_json::json!({ "path": "/Users/private/secret.ts" });
        relativize_value_paths(&mut detail, root);
        assert_eq!(detail["path"], "<external>/secret.ts");

        let mut route = serde_json::json!({ "specifier": "/api/v1" });
        relativize_value_paths(&mut route, root);
        assert_eq!(route["specifier"], "/api/v1");

        let mut conflicts = serde_json::json!({
            "conflicting_paths": ["/project/app/a.ts", "/project/app/b.ts"]
        });
        relativize_value_paths(&mut conflicts, root);
        assert_eq!(conflicts["conflicting_paths"][0], "app/a.ts");
        assert_eq!(conflicts["conflicting_paths"][1], "app/b.ts");
    }

    #[test]
    fn config_action_values_are_not_rendered_as_commands() {
        let finding = finding_from_value(
            "dependency",
            "Dependency finding",
            serde_json::json!({
                "actions": [{
                    "kind": "add-to-config",
                    "auto_fixable": false,
                    "config_key": "entry",
                    "value": "./errors",
                    "description": "Add the entry to configuration"
                }]
            }),
            Path::new("/project"),
            &|_| None,
        );
        assert_eq!(finding.actions.len(), 1);
        let action = &finding.actions[0];
        assert_eq!(action.kind.as_deref(), Some("add-to-config"));
        assert!(!action.auto_fixable);
        assert_eq!(action.config_key.as_deref(), Some("entry"));
        assert_eq!(action.value, Some(Value::String("./errors".to_string())));
        assert!(action.command.is_none());
    }
}
