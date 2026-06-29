//! Programmatic API contract types for fallow.
//!
//! Runtime execution for dead-code and duplication lives here. Health output
//! assembly is also API-owned, with the concrete runner injected while the
//! remaining health pipeline moves out of the CLI crate. This crate owns the
//! CLI-independent option, error, and output contracts so NAPI, future Rust
//! embedders, and the engine facade can share them without depending on the
//! CLI crate.
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        reason = "tests use expect to keep fixture setup concise"
    )
)]

use std::path::PathBuf;

use fallow_config::EmailMode;
use fallow_output::EffortEstimate;
use serde::Serialize;

pub mod audit_output;
pub mod combined_output;
pub mod compact_output;
pub mod dead_code_codeclimate;
pub mod dead_code_sarif;
pub mod dupes_output;
pub mod editor;
pub mod grouped_output;
pub mod health_codeclimate;
pub mod json_output;
pub mod list_output;
pub mod markdown_output;
pub mod output_contracts;
pub mod runtime;
pub mod sarif_output;
pub mod security_output;
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
pub use audit_output::{
    AuditAttribution, AuditCodeClimateOutputInput, AuditJsonHeaderInput, AuditJsonOutputInput,
    AuditSarifOutputInput, AuditSummary, AuditVerdict, build_audit_codeclimate,
    build_audit_codeclimate_issues, build_audit_header_json, build_audit_header_map,
    build_audit_sarif, serialize_audit_json,
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
    AttributedCloneGroup, AttributedCloneGroupFinding, AttributedInstance, CloneFamilyFinding,
    CloneGroupFinding, DupesReportPayload, DuplicationGroup, DuplicationGrouping,
    build_duplication_codeclimate,
};
pub use editor::{
    ChangedFilesError, EditorAnalysisOutput, EditorAnalysisResults, EditorAnalysisSession,
    EditorDeadCodeAnalysisOutput, EditorDuplicationReport, EditorInlineComplexityExceeded,
    EditorInlineComplexityFinding, EditorProjectAnalysisOutput, collect_inline_complexity,
    editor_duplicates, editor_extract, editor_results, editor_security, editor_suppress,
    filter_inline_complexity_by_changed_files, resolve_git_toplevel,
    try_get_changed_files_with_toplevel,
};
pub use grouped_output::{
    ResultGroup, UNOWNED_GROUP_LABEL, build_duplication_grouping_with, group_analysis_results_with,
    largest_clone_group_owner_with,
};
pub use health_codeclimate::build_health_codeclimate;
pub use json_output::{
    CheckJsonExtraOutputs, CheckJsonOutputInput, CheckJsonPayloadInput, DuplicationJsonOutputInput,
    GroupedCheckJsonOutputInput, GroupedDuplicationJsonOutputInput,
    harmonize_multi_kind_suppress_line_actions, serialize_check_json, serialize_check_json_payload,
    serialize_duplication_json, serialize_grouped_check_json, serialize_grouped_duplication_json,
};
pub use list_output::{
    ListJsonEnvelope, ListJsonOutputInput, build_list_json_output, serialize_list_json_output,
};
pub use markdown_output::{
    build_duplication_markdown, build_grouped_markdown, build_health_markdown, build_markdown,
    build_walkthrough_markdown,
};
pub use output_contracts::{
    AuditOutput, BoundariesListLogicalGroup, BoundariesListRule, BoundariesListZone,
    BoundariesListing, CombinedOutput, FallowOutput, ListBoundariesOutput, ListEntryPointOutput,
    ListOutput, ListPluginOutput, SecurityGate, SecurityOutput, SecurityOutputConfig,
    SecuritySummaryOutput, WorkspacesOutput,
};
pub use runtime::{
    DeadCodeProgrammaticOutput, DuplicationProgrammaticOutput, EngineHealthRunner,
    HealthJsonReportInput, HealthProgrammaticOutput, ProgrammaticAnalysisContext,
    ProgrammaticHealthNextStepFacts, ProgrammaticHealthRun, ProgrammaticHealthRunner,
    compute_complexity_with_runner, compute_health, compute_health_with_runner,
    derive_programmatic_health_execution_options, detect_boundary_violations,
    detect_circular_dependencies, detect_dead_code, detect_duplication,
    resolve_programmatic_analysis_context, run_boundary_violations, run_circular_dependencies,
    run_complexity_with_runner, run_dead_code, run_duplication, run_health, run_health_with_runner,
    serialize_health_report_json,
};
pub use sarif_output::{
    annotate_sarif_results, build_duplication_sarif, build_grouped_duplication_sarif,
    build_health_sarif,
};
pub use security_output::SecurityGateMode;

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
    "legacy-envelope",
];

/// Structured error surface for the programmatic API.
#[derive(Debug, Clone, Serialize)]
pub struct ProgrammaticError {
    pub message: String,
    pub exit_code: u8,
    pub code: Option<String>,
    pub help: Option<String>,
    pub context: Option<String>,
}

impl ProgrammaticError {
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

    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

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
    pub root: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub no_cache: bool,
    pub threads: Option<usize>,
    pub diff_file: Option<PathBuf>,
    /// Legacy convenience override. `true` forces production mode; `false`
    /// defers to config unless `production_override` is set.
    pub production: bool,
    /// Explicit production override from an embedder option. `None` means
    /// use the project config for the current analysis.
    pub production_override: Option<bool>,
    pub changed_since: Option<String>,
    pub workspace: Option<Vec<String>>,
    pub changed_workspaces: Option<String>,
    pub explain: bool,
    /// Return the one-cycle legacy root envelope without top-level `kind`.
    pub legacy_envelope: bool,
}

/// Issue-type filters for the dead-code analysis.
#[derive(Debug, Clone, Default)]
pub struct DeadCodeFilters {
    pub unused_files: bool,
    pub unused_exports: bool,
    pub unused_deps: bool,
    pub unused_types: bool,
    pub private_type_leaks: bool,
    pub unused_enum_members: bool,
    pub unused_class_members: bool,
    pub unused_store_members: bool,
    pub unprovided_injects: bool,
    pub unrendered_components: bool,
    pub unused_component_props: bool,
    pub unused_component_emits: bool,
    pub unused_component_inputs: bool,
    pub unused_component_outputs: bool,
    pub unused_svelte_events: bool,
    pub unused_server_actions: bool,
    pub unused_load_data_keys: bool,
    pub unresolved_imports: bool,
    pub unlisted_deps: bool,
    pub duplicate_exports: bool,
    pub circular_deps: bool,
    pub re_export_cycles: bool,
    pub boundary_violations: bool,
    pub policy_violations: bool,
    pub stale_suppressions: bool,
    pub unused_catalog_entries: bool,
    pub empty_catalog_groups: bool,
    pub unresolved_catalog_references: bool,
    pub unused_dependency_overrides: bool,
    pub misconfigured_dependency_overrides: bool,
}

/// Options for dead-code-oriented analyses.
#[derive(Debug, Clone, Default)]
pub struct DeadCodeOptions {
    pub analysis: AnalysisOptions,
    pub filters: DeadCodeFilters,
    pub files: Vec<PathBuf>,
    pub include_entry_exports: bool,
}

/// Programmatic duplication mode selection.
#[derive(Debug, Clone, Copy, Default)]
pub enum DuplicationMode {
    Strict,
    #[default]
    Mild,
    Weak,
    Semantic,
}

/// Options for duplication analysis.
#[derive(Debug, Clone)]
pub struct DuplicationOptions {
    pub analysis: AnalysisOptions,
    pub mode: DuplicationMode,
    pub min_tokens: usize,
    pub min_lines: usize,
    /// Minimum number of occurrences before a clone group is reported.
    /// Values below 2 are silently treated as 2 by the engine-facing adapter.
    pub min_occurrences: usize,
    pub threshold: f64,
    pub skip_local: bool,
    pub cross_language: bool,
    /// Exclude module wiring from clone detection. `None` defers to the project
    /// config.
    pub ignore_imports: Option<bool>,
    pub top: Option<usize>,
}

impl Default for DuplicationOptions {
    fn default() -> Self {
        Self {
            analysis: AnalysisOptions::default(),
            mode: DuplicationMode::Mild,
            min_tokens: 50,
            min_lines: 5,
            min_occurrences: 2,
            threshold: 0.0,
            skip_local: false,
            cross_language: false,
            ignore_imports: None,
            top: None,
        }
    }
}

/// Sort criteria for complexity findings.
#[derive(Debug, Clone, Copy, Default)]
pub enum ComplexitySort {
    #[default]
    Cyclomatic,
    Cognitive,
    Lines,
    Severity,
}

/// Privacy mode for ownership-aware hotspot output.
#[derive(Debug, Clone, Copy, Default)]
pub enum OwnershipEmailMode {
    Raw,
    #[default]
    Handle,
    Anonymized,
    /// Legacy spelling retained for embedders that already pass `hash`.
    Hash,
}

/// Effort filter for refactoring targets.
#[derive(Debug, Clone, Copy)]
pub enum TargetEffort {
    Low,
    Medium,
    High,
}

/// Options for complexity / health analysis.
#[derive(Debug, Clone, Default)]
pub struct ComplexityOptions {
    pub analysis: AnalysisOptions,
    pub max_cyclomatic: Option<u16>,
    pub max_cognitive: Option<u16>,
    pub max_crap: Option<f64>,
    pub top: Option<usize>,
    pub sort: ComplexitySort,
    pub complexity: bool,
    pub file_scores: bool,
    pub coverage_gaps: bool,
    pub hotspots: bool,
    pub ownership: bool,
    pub ownership_emails: Option<OwnershipEmailMode>,
    pub targets: bool,
    pub css: bool,
    pub effort: Option<TargetEffort>,
    pub score: bool,
    pub since: Option<String>,
    pub min_commits: Option<u32>,
    pub coverage: Option<PathBuf>,
    pub coverage_root: Option<PathBuf>,
}

pub use fallow_engine::{
    ComplexityRunOptions, ComplexitySectionOptions, DerivedComplexityOptions,
    DerivedHealthSections, HealthSectionOptions, derive_complexity_sections,
    derive_health_sections,
};

/// Derive effective programmatic health / complexity section flags.
#[must_use]
pub fn derive_complexity_options(options: &ComplexityOptions) -> DerivedComplexityOptions {
    derive_complexity_sections(&complexity_section_options(options))
}

/// Normalize public API complexity options into engine-owned run contracts.
#[must_use]
pub fn derive_complexity_run_options(options: &ComplexityOptions) -> ComplexityRunOptions<'_> {
    ComplexityRunOptions {
        thresholds: fallow_engine::HealthThresholdOverrides {
            max_cyclomatic: options.max_cyclomatic,
            max_cognitive: options.max_cognitive,
            max_crap: options.max_crap,
        },
        top: options.top,
        sort: complexity_sort_to_engine(options.sort),
        sections: derive_complexity_options(options),
        ownership_emails: options.ownership_emails.map(ownership_email_mode_to_config),
        effort: options.effort.map(target_effort_to_output),
        css: options.css,
        since: options.since.as_deref(),
        min_commits: options.min_commits,
        coverage_inputs: fallow_engine::HealthCoverageInputs {
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
        fallow_engine::validate_coverage_root_absolute(options.coverage_root.as_deref())
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

const fn complexity_sort_to_engine(sort: ComplexitySort) -> fallow_engine::HealthSort {
    match sort {
        ComplexitySort::Severity => fallow_engine::HealthSort::Severity,
        ComplexitySort::Cyclomatic => fallow_engine::HealthSort::Cyclomatic,
        ComplexitySort::Cognitive => fallow_engine::HealthSort::Cognitive,
        ComplexitySort::Lines => fallow_engine::HealthSort::Lines,
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
        assert!(matches!(options.mode, DuplicationMode::Mild));
        assert_eq!(options.min_tokens, 50);
        assert_eq!(options.min_lines, 5);
        assert_eq!(options.min_occurrences, 2);
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
        assert!(matches!(run.sort, fallow_engine::HealthSort::Severity));
        assert!(run.sections.hotspots);
        assert!(run.sections.ownership);
        assert!(run.sections.targets);
        assert!(matches!(
            run.ownership_emails,
            Some(fallow_config::EmailMode::Hash)
        ));
        assert!(matches!(
            run.effort,
            Some(fallow_output::EffortEstimate::High)
        ));
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
            output: fallow_config::OutputFormat::Human,
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
            output: fallow_config::OutputFormat::Human,
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
            output: fallow_config::OutputFormat::Human,
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
