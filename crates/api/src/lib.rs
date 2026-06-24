//! Programmatic API contract types for fallow.
//!
//! Runtime execution is moving here from `fallow-cli::programmatic` one
//! analysis at a time. This crate owns the CLI-independent option, error, and
//! output contracts so NAPI, future Rust embedders, and the engine facade can
//! share them without depending on the CLI crate.
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        reason = "tests use expect to keep fixture setup concise"
    )
)]

use std::path::PathBuf;

use serde::Serialize;

pub mod dupes_output;
pub mod runtime;
pub use dupes_output::{CloneFamilyFinding, CloneGroupFinding, DupesReportPayload};
pub use runtime::detect_duplication;

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
}
