//! Programmatic API contract types for fallow.
//!
//! Runtime execution for dead-code and duplication lives here. Health output
//! assembly is also API-owned, with the concrete runner injected while the
//! remaining health pipeline moves out of the CLI crate. This crate owns the
//! CLI-independent option, error, and output contracts so NAPI, future Rust
//! embedders, and the engine facade can share them without depending on the
//! CLI crate.
#![warn(missing_docs)]
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        reason = "tests use expect to keep fixture setup concise"
    )
)]

use std::path::{Path, PathBuf};

use fallow_config::EmailMode;
use fallow_output::EffortEstimate;
use serde::Serialize;

mod analysis_context;
/// Stable per-finding keys and audit ledgers that compare head results against
/// a base snapshot, plus helpers that annotate output JSON with
/// introduced-vs-pre-existing attribution.
pub mod audit_keys;
pub mod audit_output;
pub mod combined_output;
/// One-line-per-finding compact text builders for dead-code, grouped, health,
/// and duplication output.
pub mod compact_output;
pub mod dead_code_codeclimate;
pub mod dead_code_sarif;
pub mod decision_surface;
pub mod dupes_output;
mod duplication_filters;
pub mod editor;
pub mod explain;
pub mod grouped_output;
pub mod health_codeclimate;
pub mod json_output;
pub mod list_output;
mod list_runtime;
/// Markdown report builders for dead-code, grouped, duplication, health, and
/// walkthrough output.
pub mod markdown_output;
mod next_steps;
pub mod output_contracts;
pub mod review_deltas;
pub mod routing;
pub mod runtime;
mod runtime_json;
mod runtime_output;
pub mod sarif_output;
pub mod security_output;
mod type_aware;
pub mod ci_output {
    //! Compatibility re-exports for CI output builders now owned by
    //! `fallow-output`.

    pub use fallow_output::{
        CiIssue, CiProvider, GroupedReviewIssues, MARKER_PREFIX_V2, MARKER_SUFFIX_V2,
        MAX_COMMENT_BODY_BYTES, PROJECT_LEVEL_RULE_IDS, PrCommentRenderInput,
        ReviewCommentRenderInput, ReviewEnvelopeRenderInput, ReviewEnvelopeRenderResult,
        ReviewEnvelopeTruncation, ReviewGitlabDiffRefs, cap_body_with_marker, command_title,
        composite_fingerprint, escape_md, github_check_conclusion,
        group_review_issues_by_path_line, is_project_level_rule, issues_from_codeclimate,
        issues_from_codeclimate_issues, render_pr_comment, render_review_comment_for_group,
        render_review_envelope, review_label_from_codeclimate, summary_fingerprint, summary_label,
    };
}
pub use analysis_context::{ProgrammaticAnalysisContext, resolve_programmatic_analysis_context};
pub use audit_output::{
    AuditAttribution, AuditCodeClimateOutputInput, AuditJsonHeaderInput, AuditJsonOutputInput,
    AuditSarifOutputInput, AuditSummary, AuditVerdict,
    attach_audit_duplication_demotion_attribution, attach_audit_styling_attribution,
    attach_audit_wire_attribution, build_audit_codeclimate, build_audit_codeclimate_issues,
    build_audit_header_json, build_audit_header_map, build_audit_sarif, build_review_brief_header,
    serialize_audit_json,
};
pub use ci_output::{
    CiIssue, CiProvider, GroupedReviewIssues, MARKER_PREFIX_V2, MARKER_SUFFIX_V2,
    MAX_COMMENT_BODY_BYTES, PROJECT_LEVEL_RULE_IDS, PrCommentRenderInput, ReviewCommentRenderInput,
    ReviewEnvelopeRenderInput, ReviewEnvelopeRenderResult, ReviewEnvelopeTruncation,
    ReviewGitlabDiffRefs, cap_body_with_marker, command_title, composite_fingerprint, escape_md,
    github_check_conclusion, group_review_issues_by_path_line, is_project_level_rule,
    issues_from_codeclimate, issues_from_codeclimate_issues, render_pr_comment,
    render_review_comment_for_group, render_review_envelope, review_label_from_codeclimate,
    summary_fingerprint, summary_label,
};
pub use combined_output::{
    CombinedCheckJsonSection, CombinedJsonOutputInput, serialize_combined_dupes_json,
    serialize_combined_health_json, serialize_combined_json,
};
pub use compact_output::{
    build_compact_lines, build_duplication_compact_lines, build_grouped_compact_lines,
    build_health_compact_lines,
};
pub use dead_code_codeclimate::build_codeclimate;
pub use dead_code_sarif::build_sarif;
pub use dupes_output::{
    AttributedCloneGroup, AttributedCloneGroupFinding, AttributedInstance, CloneDemotionReason,
    CloneFamilyFinding, CloneGroupFinding, DupesReportPayload, DuplicationGroup,
    DuplicationGrouping, build_duplication_codeclimate,
};
pub use editor::{
    ChangedFilesError, EditorAnalysisOutput, EditorAnalysisResults, EditorAnalysisSession,
    EditorCloneFamily, EditorCloneFingerprintSet, EditorCloneGroup, EditorCloneInstance,
    EditorDeadCodeAnalysisOutput, EditorDuplicationReport, EditorDuplicationStats,
    EditorInlineComplexityExceeded, EditorInlineComplexityFinding, EditorMirroredDirectory,
    EditorProjectAnalysisOutput, EditorRefactoringKind, EditorRefactoringSuggestion,
    collect_inline_complexity, editor_duplicates, editor_extract, editor_results, editor_security,
    editor_suppress, filter_inline_complexity_by_changed_files, resolve_git_toplevel,
    try_get_changed_files_with_toplevel,
};
pub use explain::{
    CHECK_RULES, DUPES_RULES, FLAGS_RULES, HEALTH_RULES, RuleDef, RuleGuide, SECURITY_RULES,
    coverage_analyze_meta, coverage_setup_meta, explain_issue_type, rule_by_id, rule_by_token,
    rule_docs_url, rule_guide, security_meta, serialize_explain_programmatic_json,
    unknown_explain_error,
};
pub use fallow_config::{AuditGate, TypeAwareRequire};
pub use fallow_output::RootEnvelopeMode;
pub use fallow_types::trace::{
    CloneTrace, DependencyTrace, ExportReference, ExportTrace, FileTrace, ReExportChain,
    TracedCloneGroup, TracedExport, TracedReExport,
};
pub use grouped_output::{
    ResultGroup, UNOWNED_GROUP_LABEL, build_duplication_grouping_with, group_analysis_results_with,
    largest_clone_group_owner_with,
};
pub use health_codeclimate::build_health_codeclimate;
pub use json_output::{
    CheckJsonExtraOutputs, CheckJsonOutputInput, CheckJsonPayloadInput, DuplicationJsonOutputInput,
    GroupedCheckJsonOutputInput, GroupedDuplicationJsonOutputInput, serialize_check_json,
    serialize_check_json_payload, serialize_duplication_json, serialize_grouped_check_json,
    serialize_grouped_duplication_json,
};
pub use list_output::{
    ListJsonEnvelope, ListJsonOutputInput, build_list_json_output, serialize_list_json_output,
};
pub use list_runtime::{
    BoundaryData, ListBoundariesOptions, ListBoundariesProgrammaticOutput, LogicalGroupInfo,
    ProjectInfoOptions, ProjectInfoProgrammaticOutput, RuleInfo, ZoneInfo, boundary_data_to_output,
    compute_boundary_data, run_list_boundaries, run_project_info,
    serialize_list_boundaries_programmatic_json, serialize_project_info_programmatic_json,
};
pub use markdown_output::{
    build_duplication_markdown, build_grouped_markdown, build_health_markdown, build_markdown,
    build_walkthrough_markdown,
};
pub use output_contracts::{
    AuditOutput, BoundariesListLogicalGroup, BoundariesListRule, BoundariesListZone,
    BoundariesListing, CombinedOutput, FallowOutput, ImpactOutput, ListBoundariesOutput,
    ListEntryPointOutput, ListOutput, ListPluginOutput, ReviewBriefWireOutput, SecurityGate,
    SecurityOutput, SecurityOutputConfig, SecuritySummaryOutput, TraceOutput, WorkspacesOutput,
};
pub use runtime::{
    AuditProgrammaticKeySnapshot, AuditProgrammaticOutput, BoundaryViolationsOutput,
    BoundaryViolationsProgrammaticOutput, CircularDependenciesOutput,
    CircularDependenciesProgrammaticOutput, CombinedProgrammaticOutput, DeadCodeOutput,
    DeadCodeProgrammaticOutput, DecisionSurfaceProgrammaticOutput, DuplicationOutput,
    DuplicationProgrammaticOutput, EngineHealthRunner, FeatureFlagsOutput,
    FeatureFlagsProgrammaticOutput, HealthJsonReportInput, HealthProgrammaticOutput,
    ProgrammaticHealthAnalysis, ProgrammaticHealthNextStepFacts, ProgrammaticHealthRun,
    ProgrammaticHealthRunner, TraceClassMemberOutput, TraceCloneBenchmarkResult, TraceCloneOutput,
    TraceCloneProgrammaticOutput, TraceDependencyOutput, TraceDependencyProgrammaticOutput,
    TraceExportOutput, TraceExportProgrammaticOutput, TraceExportTargetOutput, TraceFileOutput,
    TraceFileProgrammaticOutput, benchmark_trace_clone_compact_json,
    benchmark_trace_graph_family_compact_json, run_audit, run_boundary_violations,
    run_circular_dependencies, run_combined, run_complexity_with_runner, run_dead_code,
    run_decision_surface, run_duplication, run_feature_flags, run_health, run_health_with_runner,
    run_trace_clone, run_trace_dependency, run_trace_export, run_trace_file,
    serialize_health_report_json,
};
pub use runtime_json::{
    serialize_audit_programmatic_json, serialize_boundary_violations_programmatic_json,
    serialize_circular_dependencies_programmatic_json, serialize_combined_programmatic_json,
    serialize_dead_code_programmatic_json, serialize_decision_surface_programmatic_json,
    serialize_duplication_programmatic_json, serialize_feature_flags_programmatic_json,
    serialize_health_programmatic_json, serialize_trace_clone_programmatic_json,
    serialize_trace_dependency_programmatic_json, serialize_trace_export_programmatic_json,
    serialize_trace_file_programmatic_json,
};
pub use sarif_output::{
    annotate_sarif_results, build_duplication_sarif, build_grouped_duplication_sarif,
    build_health_sarif,
};
pub use security_output::SecurityGateMode;
pub use type_aware::{
    SemanticCouplingOutcome, SemanticDeadCodeOutcome, SemanticInspectOutcome, TypeAwareError,
    TypeAwareFileChanges, TypeAwareOutcome, TypeAwareSession, TypeAwareStatus,
    discard_unverified_semantic_candidates, inspect_symbol as inspect_type_aware_symbol,
    merge_type_aware_meta,
    refine_configured_dead_code_results as refine_type_aware_results_with_config,
    refine_configured_dead_code_results_in_session as refine_type_aware_results_in_session_with_config,
    refine_dead_code_results as refine_type_aware_results,
    refine_dead_code_results_in_session as refine_type_aware_results_in_session,
    refine_programmatic_dead_code as refine_type_aware_dead_code, shutdown_type_aware_sidecars,
    status as type_aware_status, symbol_impact as run_type_aware_symbol_impact,
    symbol_impact as type_aware_symbol_impact, terminate_active_type_aware_sidecars,
    trace_symbol as run_type_aware_symbol_trace, trace_symbol as trace_type_aware_symbol,
    type_coupling as analyze_type_coupling,
};

/// Long names of the analysis-affecting global CLI flags that
/// [`AnalysisOptions`] mirrors for embedders.
///
/// A contract test in the CLI crate asserts this list stays in sync with the
/// clap globals, so drift between the CLI surface and the programmatic
/// options is caught at test time.
pub const COMMON_ANALYSIS_OPTION_FLAGS: &[&str] = &[
    "root",
    "config",
    "no-cache",
    "threads",
    "changed-since",
    "diff-file",
    "production",
    "workspace",
    "changed-workspaces",
    "explain",
    "allow-remote-extends",
];

/// Structured error surface for the programmatic API.
#[derive(Debug, Clone, Serialize)]
pub struct ProgrammaticError {
    /// Human-readable description; also the `Display` output.
    pub message: String,
    /// Process exit code the CLI maps this failure to.
    pub exit_code: u8,
    /// Stable machine-readable code such as `FALLOW_INVALID_COVERAGE_PATH`.
    pub code: Option<String>,
    /// Optional remediation hint for the caller.
    pub help: Option<String>,
    /// Dotted path of the offending input, such as `health.coverage`.
    pub context: Option<String>,
}

impl ProgrammaticError {
    /// Create an error from the required message and exit code, with all
    /// optional fields left empty.
    #[must_use]
    pub fn new(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: message.into(),
            exit_code,
            code: None,
            help: None,
            context: None,
        }
    }

    /// Attach a remediation hint shown to the caller alongside the message.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Attach a stable machine-readable code such as
    /// `FALLOW_INVALID_COVERAGE_PATH`.
    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Attach the dotted path of the offending input, such as
    /// `health.coverage`.
    #[must_use]
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

impl std::fmt::Display for ProgrammaticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ProgrammaticError {}

/// Shared options for all one-shot analyses.
#[derive(Debug, Clone, Default)]
pub struct AnalysisOptions {
    /// Project root to analyze. `None` resolves to the current working
    /// directory.
    pub root: Option<PathBuf>,
    /// Explicit config file path. `None` uses config discovery from the root.
    pub config_path: Option<PathBuf>,
    /// Permit `https://` config inheritance for this analysis call.
    pub allow_remote_extends: bool,
    /// Bypass the on-disk analysis cache for this call.
    pub no_cache: bool,
    /// Worker thread count. `None` picks the default; `Some(0)` is rejected.
    pub threads: Option<usize>,
    /// Explicit unified diff file that scopes changed-code analysis.
    pub diff_file: Option<PathBuf>,
    /// Legacy convenience override. `true` forces production mode; `false`
    /// defers to config unless `production_override` is set.
    pub production: bool,
    /// Explicit production override from an embedder option. `None` means
    /// use the project config for the current analysis.
    pub production_override: Option<bool>,
    /// Git base reference that scopes analysis to files changed since it.
    pub changed_since: Option<String>,
    /// Restrict analysis to the named workspace packages.
    pub workspace: Option<Vec<String>>,
    /// Restrict analysis to workspaces changed since the given git reference.
    pub changed_workspaces: Option<String>,
    /// Include rule and metric explanations (`_meta`) in machine output.
    pub explain: bool,
    /// Optional project-wide TypeScript semantic analysis. Disabled by default
    /// and never changes compiler or typed-lint ownership.
    pub type_aware: TypeAwareOptions,
}

/// Typed options for Fallow's optional TypeScript semantic companion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeAwareOptions {
    /// Turn on TypeScript semantic refinement for this call.
    pub enabled: bool,
    /// Explicit TypeScript project paths handed to the semantic sidecar.
    /// Empty defers to project discovery.
    pub projects: Vec<PathBuf>,
    /// How strictly semantic completion is required before results are kept.
    pub require: fallow_config::TypeAwareRequire,
}

/// Issue-type filters for the dead-code analysis.
///
/// Each flag opts one issue type into the report. When no flag is enabled the
/// analysis reports every issue type.
#[derive(Debug, Clone, Default)]
pub struct DeadCodeFilters {
    /// Files never reached from any entry point.
    pub unused_files: bool,
    /// Exported symbols never imported elsewhere.
    pub unused_exports: bool,
    /// Declared dependencies never imported.
    pub unused_deps: bool,
    /// Exported types never referenced.
    pub unused_types: bool,
    /// Exported APIs that expose non-exported types.
    pub private_type_leaks: bool,
    /// Enum members never read.
    pub unused_enum_members: bool,
    /// Class members never used outside their declaration.
    pub unused_class_members: bool,
    /// Store members (for example Pinia) never used outside the store.
    pub unused_store_members: bool,
    /// `inject` calls with no matching `provide`.
    pub unprovided_injects: bool,
    /// Components never rendered by any template or JSX.
    pub unrendered_components: bool,
    /// Declared component props never used.
    pub unused_component_props: bool,
    /// Declared component emits never used.
    pub unused_component_emits: bool,
    /// Declared component inputs never bound.
    pub unused_component_inputs: bool,
    /// Declared component outputs never listened to.
    pub unused_component_outputs: bool,
    /// Svelte component events never listened to.
    pub unused_svelte_events: bool,
    /// Server actions never invoked.
    pub unused_server_actions: bool,
    /// `load` data keys never read by the consuming page.
    pub unused_load_data_keys: bool,
    /// Imports that do not resolve to a file or package.
    pub unresolved_imports: bool,
    /// Imported packages missing from the dependency manifest.
    pub unlisted_deps: bool,
    /// The same symbol exported more than once.
    pub duplicate_exports: bool,
    /// Circular import chains.
    pub circular_deps: bool,
    /// Cycles formed through re-export chains.
    pub re_export_cycles: bool,
    /// Imports that cross configured architecture boundaries.
    pub boundary_violations: bool,
    /// Violations of configured dependency policy rules.
    pub policy_violations: bool,
    /// Suppression comments that no longer match a finding.
    pub stale_suppressions: bool,
    /// Catalog entries never referenced by a workspace package.
    pub unused_catalog_entries: bool,
    /// Catalog groups that contain no entries.
    pub empty_catalog_groups: bool,
    /// `catalog:` references without a matching catalog entry.
    pub unresolved_catalog_references: bool,
    /// Dependency overrides that never affect a resolved package.
    pub unused_dependency_overrides: bool,
    /// Dependency overrides that cannot apply as written.
    pub misconfigured_dependency_overrides: bool,
}

impl DeadCodeFilters {
    fn any_active(&self) -> bool {
        self.unused_files
            || self.unused_exports
            || self.unused_deps
            || self.unused_types
            || self.private_type_leaks
            || self.unused_enum_members
            || self.unused_class_members
            || self.unused_store_members
            || self.unprovided_injects
            || self.unrendered_components
            || self.unused_component_props
            || self.unused_component_emits
            || self.unused_component_inputs
            || self.unused_component_outputs
            || self.unused_svelte_events
            || self.unused_server_actions
            || self.unused_load_data_keys
            || self.unresolved_imports
            || self.unlisted_deps
            || self.duplicate_exports
            || self.circular_deps
            || self.re_export_cycles
            || self.boundary_violations
            || self.policy_violations
            || self.stale_suppressions
            || self.unused_catalog_entries
            || self.empty_catalog_groups
            || self.unresolved_catalog_references
            || self.unused_dependency_overrides
            || self.misconfigured_dependency_overrides
    }

    /// Enable the issue filter addressed by a shared registry selector.
    ///
    /// Returns `false` when the selector is not registered for dead-code
    /// filtering. Callers that expose user input should surface their own
    /// validation error with the accepted registry values.
    pub fn enable_registry_selector(&mut self, selector: &str) -> bool {
        let Some(flag) = fallow_types::issue_meta::MCP_ISSUE_TYPE_FLAGS
            .iter()
            .find_map(|&(name, flag)| (name == selector).then_some(flag))
        else {
            return false;
        };
        self.enable_cli_filter_flag(flag);
        true
    }

    fn enable_cli_filter_flag(&mut self, flag: &str) {
        match flag {
            "--unused-files" => self.unused_files = true,
            "--unused-exports" => self.unused_exports = true,
            "--unused-types" => self.unused_types = true,
            "--private-type-leaks" => self.private_type_leaks = true,
            "--unused-deps" => self.unused_deps = true,
            "--unused-enum-members" => self.unused_enum_members = true,
            "--unused-class-members" => self.unused_class_members = true,
            "--unused-store-members" => self.unused_store_members = true,
            "--unprovided-injects" => self.unprovided_injects = true,
            "--unrendered-components" => self.unrendered_components = true,
            "--unused-component-props" => self.unused_component_props = true,
            "--unused-component-emits" => self.unused_component_emits = true,
            "--unused-component-inputs" => self.unused_component_inputs = true,
            "--unused-component-outputs" => self.unused_component_outputs = true,
            "--unused-svelte-events" => self.unused_svelte_events = true,
            "--unused-server-actions" => self.unused_server_actions = true,
            "--unused-load-data-keys" => self.unused_load_data_keys = true,
            "--unresolved-imports" => self.unresolved_imports = true,
            "--unlisted-deps" => self.unlisted_deps = true,
            "--duplicate-exports" => self.duplicate_exports = true,
            "--circular-deps" => self.circular_deps = true,
            "--re-export-cycles" => self.re_export_cycles = true,
            "--boundary-violations" => self.boundary_violations = true,
            "--policy-violations" => self.policy_violations = true,
            "--stale-suppressions" => self.stale_suppressions = true,
            "--unused-catalog-entries" => self.unused_catalog_entries = true,
            "--empty-catalog-groups" => self.empty_catalog_groups = true,
            "--unresolved-catalog-references" => self.unresolved_catalog_references = true,
            "--unused-dependency-overrides" => self.unused_dependency_overrides = true,
            "--misconfigured-dependency-overrides" => {
                self.misconfigured_dependency_overrides = true;
            }
            _ => unreachable!("registry emitted unsupported dead-code filter flag: {flag}"),
        }
    }
}

/// Options for dead-code-oriented analyses.
#[derive(Debug, Clone, Default)]
pub struct DeadCodeOptions {
    /// Shared analysis options.
    pub analysis: AnalysisOptions,
    /// Issue-type selection; everything is reported when no filter is set.
    pub filters: DeadCodeFilters,
    /// Restrict findings to these files when non-empty.
    pub files: Vec<PathBuf>,
    /// Also report unused exports declared in entry-point files.
    pub include_entry_exports: bool,
}

/// Options for changed-code audit analysis.
#[derive(Debug, Clone, Default)]
pub struct AuditOptions {
    /// Shared analysis options.
    pub analysis: AnalysisOptions,
    /// Git base reference for the changed-code comparison. `None` lets the
    /// audit detect a base itself.
    pub base: Option<String>,
    /// Force production mode for every audit domain.
    pub production: bool,
    /// Production override for the dead-code domain; `None` defers to config.
    pub production_dead_code: Option<bool>,
    /// Production override for the health domain; `None` defers to config.
    pub production_health: Option<bool>,
    /// Production override for the duplication domain; `None` defers to
    /// config.
    pub production_dupes: Option<bool>,
    /// Enable CSS / styling analysis; `None` defers to config.
    pub css: Option<bool>,
    /// Enable deep cross-file CSS analysis; `None` defers to config.
    pub css_deep: Option<bool>,
    /// Gate mode deciding which findings fail the audit.
    pub gate: fallow_config::AuditGate,
    /// Fail the gate when a changed function exceeds this CRAP score.
    pub max_crap: Option<f64>,
    /// Test coverage report path for coverage-aware findings.
    pub coverage: Option<PathBuf>,
    /// Absolute path prefix the coverage report recorded its files under.
    pub coverage_root: Option<PathBuf>,
    /// Also report unused exports declared in entry-point files.
    pub include_entry_exports: bool,
    /// Runtime coverage capture merged into the audit.
    pub runtime_coverage: Option<PathBuf>,
    /// Minimum recorded invocations for a code path to count as hot.
    pub min_invocations_hot: u64,
}

/// Options for bare combined analysis through the programmatic API.
#[derive(Debug, Clone)]
pub struct CombinedOptions {
    /// Shared analysis options.
    pub analysis: AnalysisOptions,
    /// Run the dead-code domain.
    pub dead_code: bool,
    /// Run the duplication domain.
    pub duplication: bool,
    /// Run the health domain.
    pub health: bool,
    /// Also report unused exports declared in entry-point files.
    pub include_entry_exports: bool,
    /// Options for the duplication domain.
    pub duplication_options: DuplicationOptions,
    /// Options for the health domain.
    pub health_options: ComplexityOptions,
}

impl Default for CombinedOptions {
    fn default() -> Self {
        Self {
            analysis: AnalysisOptions::default(),
            dead_code: true,
            duplication: true,
            health: true,
            include_entry_exports: false,
            duplication_options: DuplicationOptions::default(),
            health_options: ComplexityOptions::default(),
        }
    }
}

/// Options for changed-code decision-surface analysis.
#[derive(Debug, Clone, Default)]
pub struct DecisionSurfaceOptions {
    /// Shared analysis options.
    pub analysis: AnalysisOptions,
    /// Git base reference for the changed-code comparison. `None` lets the
    /// analysis detect a base itself.
    pub base: Option<String>,
    /// Cap on the number of decisions surfaced.
    pub max_decisions: Option<usize>,
}

/// Options for feature-flag analysis.
#[derive(Debug, Clone, Default)]
pub struct FeatureFlagsOptions {
    /// Shared analysis options.
    pub analysis: AnalysisOptions,
    /// Cap on the number of reported flags.
    pub top: Option<usize>,
}

/// Programmatic duplication mode selection.
#[derive(Debug, Clone, Copy, Default)]
pub enum DuplicationMode {
    /// Preserve all tokens, including identifier names and literal values
    /// (Type-1 clones only).
    Strict,
    /// Default mode, equivalent to strict for AST-based tokenization.
    #[default]
    Mild,
    /// Blind string literal values while preserving structure.
    Weak,
    /// Blind all identifiers and literal values for structural (Type-2)
    /// detection.
    Semantic,
}

/// Options for duplication analysis.
#[derive(Debug, Clone, Default)]
pub struct DuplicationOptions {
    /// Shared analysis options.
    pub analysis: AnalysisOptions,
    /// Detection mode; `None` defers to the project config.
    pub mode: Option<DuplicationMode>,
    /// Detect function-scoped near-miss clones in addition to exact clones.
    /// `None` defers to the project config.
    pub near: Option<bool>,
    /// Minimum number of tokens for a clone.
    pub min_tokens: Option<usize>,
    /// Minimum number of lines for a clone.
    pub min_lines: Option<usize>,
    /// Minimum number of occurrences before a clone group is reported.
    /// Values below 2 are silently treated as 2 by the engine-facing adapter.
    pub min_occurrences: Option<usize>,
    /// Maximum allowed duplication percentage before the gate fails; 0 means
    /// no limit. `None` defers to the project config.
    pub threshold: Option<f64>,
    /// Only report cross-directory duplicates. `None` defers to the project
    /// config.
    pub skip_local: Option<bool>,
    /// Match clones across languages. `None` defers to the project config.
    pub cross_language: Option<bool>,
    /// Exclude module wiring from clone detection. `None` defers to the project
    /// config.
    pub ignore_imports: Option<bool>,
    /// Cap on the number of reported clone groups.
    pub top: Option<usize>,
}

/// Options for export trace analysis.
#[derive(Debug, Clone, Default)]
pub struct TraceExportOptions {
    /// Shared analysis options.
    pub analysis: AnalysisOptions,
    /// Path of the module that declares the export.
    pub file: String,
    /// Name of the export to trace.
    pub export_name: String,
}

/// Options for file trace analysis.
#[derive(Debug, Clone, Default)]
pub struct TraceFileOptions {
    /// Shared analysis options.
    pub analysis: AnalysisOptions,
    /// Path of the file to trace.
    pub file: String,
}

/// Options for dependency trace analysis.
#[derive(Debug, Clone, Default)]
pub struct TraceDependencyOptions {
    /// Shared analysis options.
    pub analysis: AnalysisOptions,
    /// Package whose importers are traced.
    pub package_name: String,
}

/// Duplicate-code trace target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceCloneTarget {
    /// Select the clone group covering this file and line.
    Location {
        /// Path of the file containing the clone instance.
        file: String,
        /// One-based line inside the clone instance.
        line: usize,
    },
    /// Select the clone group by its fingerprint.
    Fingerprint(String),
}

/// Options for duplicate-code trace analysis.
#[derive(Debug, Clone)]
pub struct TraceCloneOptions {
    /// Duplication options controlling detection before the trace.
    pub duplication: DuplicationOptions,
    /// Clone group to trace.
    pub target: TraceCloneTarget,
}

/// Sort criteria for complexity findings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ComplexitySort {
    /// Sort by cyclomatic complexity (default).
    #[default]
    Cyclomatic,
    /// Sort by cognitive complexity.
    Cognitive,
    /// Sort by function length in lines.
    Lines,
    /// Sort by finding severity.
    Severity,
}

/// Privacy mode for ownership-aware hotspot output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OwnershipEmailMode {
    /// Show the raw email address as it appears in git history.
    Raw,
    /// Show only the local part before the `@` (default).
    #[default]
    Handle,
    /// Show a stable non-cryptographic pseudonym derived from the raw email.
    Anonymized,
    /// Legacy spelling retained for embedders that already pass `hash`.
    Hash,
}

/// Effort filter for refactoring targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetEffort {
    /// Low estimated refactoring effort.
    Low,
    /// Medium estimated refactoring effort.
    Medium,
    /// High estimated refactoring effort.
    High,
}

/// Options for complexity / health analysis.
#[derive(Debug, Clone, Default)]
pub struct ComplexityOptions {
    /// Shared analysis options.
    pub analysis: AnalysisOptions,
    /// Override for the cyclomatic complexity threshold.
    pub max_cyclomatic: Option<u16>,
    /// Override for the cognitive complexity threshold.
    pub max_cognitive: Option<u16>,
    /// Override for the CRAP score threshold.
    pub max_crap: Option<f64>,
    /// Cap on the number of reported findings.
    pub top: Option<usize>,
    /// Sort order for complexity findings.
    pub sort: ComplexitySort,
    /// Include the per-metric complexity breakdown with each finding.
    pub complexity_breakdown: bool,
    /// Request the complexity findings section.
    pub complexity: bool,
    /// Request the per-file score section.
    pub file_scores: bool,
    /// Request the coverage-gap section.
    pub coverage_gaps: bool,
    /// Request the churn hotspot section.
    pub hotspots: bool,
    /// Include ownership data with hotspots; implies the hotspot section.
    pub ownership: bool,
    /// Email privacy mode for ownership output; implies ownership when set.
    pub ownership_emails: Option<OwnershipEmailMode>,
    /// Request the refactoring targets section.
    pub targets: bool,
    /// Include CSS / styling health.
    pub css: bool,
    /// Enable deep cross-file CSS analysis.
    pub css_deep: bool,
    /// Filter refactoring targets by estimated effort; implies the targets
    /// section when set.
    pub effort: Option<TargetEffort>,
    /// Request the overall health score.
    pub score: bool,
    /// Git time window (for example `30d`) for churn-based sections.
    pub since: Option<String>,
    /// Minimum commit count for a file to count as a hotspot.
    pub min_commits: Option<u32>,
    /// Test coverage report path for coverage-aware sections.
    pub coverage: Option<PathBuf>,
    /// Absolute path prefix the coverage report recorded its files under.
    pub coverage_root: Option<PathBuf>,
}

/// Health threshold overrides accepted by the programmatic API.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ComplexityThresholdOverrides {
    /// Override for the cyclomatic complexity threshold.
    pub max_cyclomatic: Option<u16>,
    /// Override for the cognitive complexity threshold.
    pub max_cognitive: Option<u16>,
    /// Override for the CRAP score threshold.
    pub max_crap: Option<f64>,
}

/// Coverage inputs accepted by the programmatic API.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComplexityCoverageInputs<'a> {
    /// Test coverage report path.
    pub coverage: Option<&'a Path>,
    /// Absolute path prefix the coverage report recorded its files under.
    pub coverage_root: Option<&'a Path>,
}

/// Input for deriving effective health sections from API-owned flags.
#[derive(Debug, Clone)]
pub struct HealthSectionOptions {
    /// Requested output format; `Badge` implies the score section.
    pub output: fallow_types::output_format::OutputFormat,
    /// The complexity findings section was requested.
    pub complexity: bool,
    /// The per-file score section was requested.
    pub file_scores: bool,
    /// The coverage-gap section was requested.
    pub coverage_gaps: bool,
    /// The churn hotspot section was requested.
    pub hotspots: bool,
    /// The refactoring targets section was requested.
    pub targets: bool,
    /// CSS / styling health was requested.
    pub css: bool,
    /// The overall health score was requested.
    pub score: bool,
    /// A score gate is active; implies the score section.
    pub score_gate: bool,
    /// A snapshot write was requested; forces full hidden inputs.
    pub snapshot_requested: bool,
    /// Trend output was requested; implies score and hotspot inputs.
    pub trend: bool,
}

/// Derived section selection for health runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedHealthSections {
    /// At least one section was explicitly requested.
    pub any_section: bool,
    /// Emit the complexity findings section.
    pub complexity: bool,
    /// Compute the per-file score section.
    pub file_scores: bool,
    /// Emit the coverage-gap section.
    pub coverage_gaps: bool,
    /// Compute the churn hotspot section.
    pub hotspots: bool,
    /// Emit the refactoring targets section.
    pub targets: bool,
    /// Include CSS / styling health.
    pub css: bool,
    /// Compute the overall health score.
    pub score: bool,
    /// Compute full inputs even for sections that are not emitted.
    pub force_full: bool,
    /// Only the score should be printed.
    pub score_only_output: bool,
}

/// Input for deriving effective programmatic complexity sections.
#[derive(Debug, Clone)]
pub struct ComplexitySectionOptions {
    /// The complexity findings section was requested.
    pub complexity: bool,
    /// The per-file score section was requested.
    pub file_scores: bool,
    /// The coverage-gap section was requested.
    pub coverage_gaps: bool,
    /// The churn hotspot section was requested.
    pub hotspots: bool,
    /// Ownership data was requested; implies the hotspot section.
    pub ownership: bool,
    /// The refactoring targets section was requested.
    pub targets: bool,
    /// CSS / styling health was requested.
    pub css: bool,
    /// The overall health score was requested.
    pub score: bool,
}

/// Derived section selection for programmatic health / complexity runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedComplexityOptions {
    /// At least one section was explicitly requested.
    pub any_section: bool,
    /// Emit the complexity findings section.
    pub complexity: bool,
    /// Compute the per-file score section.
    pub file_scores: bool,
    /// Emit the coverage-gap section.
    pub coverage_gaps: bool,
    /// Compute the churn hotspot section.
    pub hotspots: bool,
    /// Include ownership data with hotspots.
    pub ownership: bool,
    /// Emit the refactoring targets section.
    pub targets: bool,
    /// Compute full inputs even for sections that are not emitted.
    pub force_full: bool,
    /// Only the score should be printed.
    pub score_only_output: bool,
    /// Compute the overall health score.
    pub score: bool,
}

/// Normalized programmatic complexity / health inputs owned by `fallow-api`.
#[derive(Debug, Clone, PartialEq)]
pub struct ComplexityRunOptions<'a> {
    /// Complexity threshold overrides.
    pub thresholds: ComplexityThresholdOverrides,
    /// Cap on the number of reported findings.
    pub top: Option<usize>,
    /// Sort order for complexity findings.
    pub sort: ComplexitySort,
    /// Include the per-metric complexity breakdown with each finding.
    pub complexity_breakdown: bool,
    /// Derived effective section selection.
    pub sections: DerivedComplexityOptions,
    /// Email privacy mode for ownership output.
    pub ownership_emails: Option<OwnershipEmailMode>,
    /// Filter refactoring targets by estimated effort.
    pub effort: Option<TargetEffort>,
    /// Include CSS / styling health.
    pub css: bool,
    /// Enable deep cross-file CSS analysis.
    pub css_deep: bool,
    /// Git time window (for example `30d`) for churn-based sections.
    pub since: Option<&'a str>,
    /// Minimum commit count for a file to count as a hotspot.
    pub min_commits: Option<u32>,
    /// Test coverage inputs.
    pub coverage_inputs: ComplexityCoverageInputs<'a>,
}

/// Derive effective health section flags for API consumers.
#[must_use]
pub fn derive_health_sections(options: &HealthSectionOptions) -> DerivedHealthSections {
    let score = options.score
        || options.score_gate
        || options.trend
        || matches!(
            options.output,
            fallow_types::output_format::OutputFormat::Badge
        );
    let any_section = options.complexity
        || options.file_scores
        || options.coverage_gaps
        || options.hotspots
        || options.targets
        || score;
    let effective_score = if any_section { score } else { true } || options.snapshot_requested;
    let force_full = options.snapshot_requested || effective_score;

    DerivedHealthSections {
        any_section,
        complexity: if any_section {
            options.complexity
        } else {
            true
        },
        file_scores: if any_section {
            options.file_scores
        } else {
            true
        } || force_full,
        coverage_gaps: if any_section {
            options.coverage_gaps
        } else {
            false
        },
        hotspots: if any_section { options.hotspots } else { true }
            || options.snapshot_requested
            || options.trend,
        targets: if any_section { options.targets } else { true },
        css: options.css,
        score: effective_score,
        force_full,
        score_only_output: is_health_score_only_output(options, score),
    }
}

/// Derive effective programmatic health / complexity section flags.
#[must_use]
pub fn derive_complexity_sections(options: &ComplexitySectionOptions) -> DerivedComplexityOptions {
    let requested_hotspots = options.hotspots || options.ownership;
    let sections = derive_health_sections(&HealthSectionOptions {
        output: fallow_types::output_format::OutputFormat::Human,
        complexity: options.complexity,
        file_scores: options.file_scores,
        coverage_gaps: options.coverage_gaps,
        hotspots: requested_hotspots,
        targets: options.targets,
        css: options.css,
        score: options.score,
        score_gate: false,
        snapshot_requested: false,
        trend: false,
    });

    DerivedComplexityOptions {
        any_section: sections.any_section,
        complexity: sections.complexity,
        file_scores: sections.file_scores,
        coverage_gaps: sections.coverage_gaps,
        hotspots: sections.hotspots,
        ownership: options.ownership && sections.hotspots,
        targets: sections.targets,
        force_full: sections.force_full,
        score_only_output: sections.score_only_output,
        score: sections.score,
    }
}

/// Derive effective programmatic health / complexity section flags.
#[must_use]
pub fn derive_complexity_options(options: &ComplexityOptions) -> DerivedComplexityOptions {
    derive_complexity_sections(&complexity_section_options(options))
}

/// Normalize public API complexity options into engine-owned run contracts.
#[must_use]
pub fn derive_complexity_run_options(options: &ComplexityOptions) -> ComplexityRunOptions<'_> {
    ComplexityRunOptions {
        thresholds: ComplexityThresholdOverrides {
            max_cyclomatic: options.max_cyclomatic,
            max_cognitive: options.max_cognitive,
            max_crap: options.max_crap,
        },
        top: options.top,
        sort: options.sort,
        complexity_breakdown: options.complexity_breakdown,
        sections: derive_complexity_options(options),
        ownership_emails: options.ownership_emails,
        effort: options.effort,
        css: options.css,
        css_deep: options.css_deep,
        since: options.since.as_deref(),
        min_commits: options.min_commits,
        coverage_inputs: ComplexityCoverageInputs {
            coverage: options.coverage.as_deref(),
            coverage_root: options.coverage_root.as_deref(),
        },
    }
}

/// Validate programmatic complexity / health inputs before invoking a concrete
/// runner.
///
/// These option contracts belong to the API boundary because NAPI and future
/// Rust embedders construct the same [`ComplexityOptions`] type.
///
/// # Errors
///
/// Returns a structured programmatic error when a coverage path does not exist
/// or when `coverage_root` is not an absolute prefix from the coverage data.
pub fn validate_complexity_options(options: &ComplexityOptions) -> Result<(), ProgrammaticError> {
    if let Some(path) = &options.coverage
        && !path.exists()
    {
        return Err(ProgrammaticError::new(
            format!("coverage path does not exist: {}", path.display()),
            2,
        )
        .with_code("FALLOW_INVALID_COVERAGE_PATH")
        .with_context("health.coverage"));
    }
    if let Err(message) =
        fallow_engine::health::validate_coverage_root_absolute(options.coverage_root.as_deref())
    {
        return Err(ProgrammaticError::new(message, 2)
            .with_code("FALLOW_INVALID_COVERAGE_ROOT")
            .with_context("health.coverage_root"));
    }

    Ok(())
}

fn complexity_section_options(options: &ComplexityOptions) -> ComplexitySectionOptions {
    let ownership = options.ownership || options.ownership_emails.is_some();
    let requested_targets = options.targets || options.effort.is_some();
    ComplexitySectionOptions {
        complexity: options.complexity,
        file_scores: options.file_scores,
        coverage_gaps: options.coverage_gaps,
        hotspots: options.hotspots,
        ownership,
        targets: requested_targets,
        css: options.css,
        score: options.score,
    }
}

fn is_health_score_only_output(options: &HealthSectionOptions, score: bool) -> bool {
    score
        && !options.complexity
        && !options.file_scores
        && !options.coverage_gaps
        && !options.hotspots
        && !options.targets
        && !options.trend
}

const fn thresholds_to_engine(
    thresholds: ComplexityThresholdOverrides,
) -> fallow_engine::health::HealthThresholdOverrides {
    fallow_engine::health::HealthThresholdOverrides {
        max_cyclomatic: thresholds.max_cyclomatic,
        max_cognitive: thresholds.max_cognitive,
        max_crap: thresholds.max_crap,
    }
}

const fn complexity_sort_to_engine(sort: ComplexitySort) -> fallow_engine::health::HealthSort {
    match sort {
        ComplexitySort::Severity => fallow_engine::health::HealthSort::Severity,
        ComplexitySort::Cyclomatic => fallow_engine::health::HealthSort::Cyclomatic,
        ComplexitySort::Cognitive => fallow_engine::health::HealthSort::Cognitive,
        ComplexitySort::Lines => fallow_engine::health::HealthSort::Lines,
    }
}

const fn coverage_inputs_to_engine(
    coverage_inputs: ComplexityCoverageInputs<'_>,
) -> fallow_engine::health::HealthCoverageInputs<'_> {
    fallow_engine::health::HealthCoverageInputs {
        coverage: coverage_inputs.coverage,
        coverage_root: coverage_inputs.coverage_root,
    }
}

const fn ownership_email_mode_to_config(mode: OwnershipEmailMode) -> EmailMode {
    match mode {
        OwnershipEmailMode::Raw => EmailMode::Raw,
        OwnershipEmailMode::Handle => EmailMode::Handle,
        OwnershipEmailMode::Anonymized => EmailMode::Anonymized,
        OwnershipEmailMode::Hash => EmailMode::Hash,
    }
}

const fn target_effort_to_output(effort: TargetEffort) -> EffortEstimate {
    match effort {
        TargetEffort::Low => EffortEstimate::Low,
        TargetEffort::Medium => EffortEstimate::Medium,
        TargetEffort::High => EffortEstimate::High,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplication_defaults_match_cli_contract() {
        let options = DuplicationOptions::default();
        assert!(options.mode.is_none());
        assert!(options.min_tokens.is_none());
        assert!(options.min_lines.is_none());
        assert!(options.min_occurrences.is_none());
    }

    #[test]
    fn programmatic_error_builder_keeps_optional_fields() {
        let error = ProgrammaticError::new("boom", 2)
            .with_code("FALLOW_TEST")
            .with_help("Try again")
            .with_context("analysis.root");

        assert_eq!(error.message, "boom");
        assert_eq!(error.exit_code, 2);
        assert_eq!(error.code.as_deref(), Some("FALLOW_TEST"));
        assert_eq!(error.help.as_deref(), Some("Try again"));
        assert_eq!(error.context.as_deref(), Some("analysis.root"));
    }

    #[test]
    fn dead_code_filters_accept_shared_registry_selectors() {
        for (selector, _) in fallow_types::issue_meta::MCP_ISSUE_TYPE_FLAGS.iter() {
            let mut filters = DeadCodeFilters::default();
            assert!(
                filters.enable_registry_selector(selector),
                "{selector} should be accepted"
            );
        }

        let mut filters = DeadCodeFilters::default();
        assert!(filters.enable_registry_selector("unused-files"));
        assert!(filters.unused_files);
        assert!(filters.enable_registry_selector("boundary-violations"));
        assert!(filters.boundary_violations);
        assert!(!filters.enable_registry_selector("not-a-real-selector"));
    }

    #[test]
    fn default_complexity_options_match_programmatic_health_defaults() {
        let derived = derive_complexity_options(&ComplexityOptions::default());

        assert!(!derived.any_section);
        assert!(derived.complexity);
        assert!(derived.file_scores);
        assert!(!derived.coverage_gaps);
        assert!(derived.hotspots);
        assert!(!derived.ownership);
        assert!(derived.targets);
        assert!(derived.force_full);
        assert!(!derived.score_only_output);
        assert!(derived.score);
    }

    #[test]
    fn score_only_complexity_options_request_score_only_output() {
        let derived = derive_complexity_options(&ComplexityOptions {
            score: true,
            ..ComplexityOptions::default()
        });

        assert!(derived.any_section);
        assert!(!derived.complexity);
        assert!(derived.file_scores);
        assert!(!derived.hotspots);
        assert!(!derived.targets);
        assert!(derived.force_full);
        assert!(derived.score_only_output);
        assert!(derived.score);
    }

    #[test]
    fn ownership_implies_hotspots_when_requested() {
        let derived = derive_complexity_options(&ComplexityOptions {
            ownership: true,
            ..ComplexityOptions::default()
        });

        assert!(derived.any_section);
        assert!(derived.hotspots);
        assert!(derived.ownership);
        assert!(!derived.targets);
    }

    #[test]
    fn complexity_run_options_normalize_public_api_options() {
        let options = ComplexityOptions {
            max_cyclomatic: Some(42),
            max_cognitive: Some(21),
            max_crap: Some(18.5),
            top: Some(7),
            sort: ComplexitySort::Severity,
            complexity_breakdown: true,
            ownership_emails: Some(OwnershipEmailMode::Hash),
            effort: Some(TargetEffort::High),
            coverage: Some(PathBuf::from("coverage/coverage-final.json")),
            coverage_root: Some(PathBuf::from("/ci/workspace")),
            since: Some("30d".to_string()),
            min_commits: Some(4),
            ..ComplexityOptions::default()
        };

        let run = derive_complexity_run_options(&options);

        assert_eq!(run.thresholds.max_cyclomatic, Some(42));
        assert_eq!(run.thresholds.max_cognitive, Some(21));
        assert_eq!(run.thresholds.max_crap, Some(18.5));
        assert_eq!(run.top, Some(7));
        assert!(matches!(run.sort, ComplexitySort::Severity));
        assert!(run.complexity_breakdown);
        assert!(run.sections.hotspots);
        assert!(run.sections.ownership);
        assert!(run.sections.targets);
        assert!(matches!(
            run.ownership_emails,
            Some(OwnershipEmailMode::Hash)
        ));
        assert!(matches!(run.effort, Some(TargetEffort::High)));
        assert_eq!(run.since, Some("30d"));
        assert_eq!(run.min_commits, Some(4));
        assert_eq!(run.coverage_inputs.coverage, options.coverage.as_deref());
        assert_eq!(
            run.coverage_inputs.coverage_root,
            options.coverage_root.as_deref()
        );
    }

    #[test]
    fn complexity_options_validation_accepts_existing_coverage_path_and_absolute_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let coverage = dir.path().join("coverage-final.json");
        std::fs::write(&coverage, "{}").expect("coverage fixture");

        let result = validate_complexity_options(&ComplexityOptions {
            coverage: Some(coverage),
            coverage_root: Some(PathBuf::from("/ci/workspace")),
            ..ComplexityOptions::default()
        });

        assert!(result.is_ok());
    }

    #[test]
    fn complexity_options_validation_keeps_missing_coverage_error_contract() {
        let err = validate_complexity_options(&ComplexityOptions {
            coverage: Some(PathBuf::from("/missing/coverage-final.json")),
            ..ComplexityOptions::default()
        })
        .expect_err("missing coverage path should fail");

        assert_eq!(err.exit_code, 2);
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_COVERAGE_PATH"));
        assert_eq!(err.context.as_deref(), Some("health.coverage"));
    }

    #[test]
    fn complexity_options_validation_keeps_relative_coverage_root_error_contract() {
        let err = validate_complexity_options(&ComplexityOptions {
            coverage_root: Some(PathBuf::from("coverage")),
            ..ComplexityOptions::default()
        })
        .expect_err("relative coverage root should fail");

        assert_eq!(err.exit_code, 2);
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_COVERAGE_ROOT"));
        assert_eq!(err.context.as_deref(), Some("health.coverage_root"));
    }

    #[test]
    fn default_health_sections_match_full_health_output() {
        let derived = derive_health_sections(&HealthSectionOptions {
            output: fallow_types::output_format::OutputFormat::Human,
            complexity: false,
            file_scores: false,
            coverage_gaps: false,
            hotspots: false,
            targets: false,
            css: false,
            score: false,
            score_gate: false,
            snapshot_requested: false,
            trend: false,
        });

        assert!(!derived.any_section);
        assert!(derived.complexity);
        assert!(derived.file_scores);
        assert!(!derived.coverage_gaps);
        assert!(derived.hotspots);
        assert!(derived.targets);
        assert!(derived.score);
        assert!(derived.force_full);
        assert!(!derived.score_only_output);
    }

    #[test]
    fn health_score_gate_requests_score_only_output() {
        let derived = derive_health_sections(&HealthSectionOptions {
            output: fallow_types::output_format::OutputFormat::Human,
            complexity: false,
            file_scores: false,
            coverage_gaps: false,
            hotspots: false,
            targets: false,
            css: false,
            score: false,
            score_gate: true,
            snapshot_requested: false,
            trend: false,
        });

        assert!(derived.any_section);
        assert!(!derived.complexity);
        assert!(derived.file_scores);
        assert!(!derived.hotspots);
        assert!(!derived.targets);
        assert!(derived.score);
        assert!(derived.force_full);
        assert!(derived.score_only_output);
    }

    #[test]
    fn health_snapshot_keeps_full_hidden_inputs_without_section_request() {
        let derived = derive_health_sections(&HealthSectionOptions {
            output: fallow_types::output_format::OutputFormat::Human,
            complexity: false,
            file_scores: false,
            coverage_gaps: false,
            hotspots: false,
            targets: false,
            css: true,
            score: false,
            score_gate: false,
            snapshot_requested: true,
            trend: false,
        });

        assert!(!derived.any_section);
        assert!(derived.css);
        assert!(derived.file_scores);
        assert!(derived.hotspots);
        assert!(derived.score);
        assert!(derived.force_full);
    }
}
