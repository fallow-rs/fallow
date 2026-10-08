//! Engine-owned health runners for non-CLI callers.

use std::time::Instant;
use std::{path::PathBuf, sync::Arc};

use fallow_config::{ProductionAnalysis, WorkspaceInfo};
use fallow_types::output_format::OutputFormat;
use rustc_hash::FxHashSet;

use crate::{
    duplicates::DuplicationReport,
    project_config::{ProjectConfigOptions, config_for_project_analysis},
    results::DeadCodeAnalysisArtifacts,
    session::AnalysisSession,
};

use super::pipeline::HealthPipelineRunInputs;
use super::{
    HealthAnalysisResult, HealthError, HealthExecutionOptions, HealthScopeInputs, HealthSeams,
    NoGroupResolver, RuntimeCoverageOptions, RuntimeCoverageSeamInput, validate_health_churn_file,
};

/// A loaded project session for a health run without a presentation grouping
/// resolver.
///
/// The runner owns config loading, discovery, parser-cache use, parsing, and
/// command-neutral health execution for API and NAPI callers. CLI-only concerns
/// still stay outside this path: runtime coverage sidecar execution, grouping
/// resolver construction, process-global telemetry, and error rendering.
///
/// The load and the run are two steps, so that a caller can resolve its
/// workspace scope from the discovered workspaces. These workspaces include
/// the `workspaces.patterns` of the loaded config.
pub struct UngroupedHealthSession {
    session: AnalysisSession,
    config_ms: f64,
}

impl UngroupedHealthSession {
    /// Load the project config and open the analysis session.
    ///
    /// # Errors
    ///
    /// Returns the health command exit code for invalid inputs or a config
    /// that does not load.
    pub fn load(options: &HealthExecutionOptions<'_>) -> Result<Self, HealthError> {
        validate_health_churn_file(options)?;

        let start = Instant::now();
        let project_config = config_for_project_analysis(
            options.root,
            options.config_path.as_deref(),
            ProjectConfigOptions {
                output: OutputFormat::Human,
                no_cache: options.no_cache,
                threads: options.threads,
                production_override: options.production_override,
                quiet: true,
                analysis: ProductionAnalysis::Health,
                allow_remote_extends: options.allow_remote_extends,
            },
        )
        .map_err(|_| HealthError::message("failed to load health project config", 2))?;
        let config_ms = start.elapsed().as_secs_f64() * 1000.0;
        Ok(Self {
            session: AnalysisSession::from_config(project_config),
            config_ms,
        })
    }

    /// The workspaces that the session discovered.
    #[must_use]
    pub fn workspaces(&self) -> &[WorkspaceInfo] {
        self.session.workspaces()
    }

    /// Run the health analysis on the loaded session.
    ///
    /// # Errors
    ///
    /// Returns the health command exit code for analysis failures.
    pub fn run(
        self,
        options: &HealthExecutionOptions<'_>,
        ws_roots: Option<Vec<PathBuf>>,
    ) -> Result<HealthAnalysisResult<NoGroupResolver>, HealthError> {
        let Self { session, config_ms } = self;
        run_ungrouped_health_on_session(options, ws_roots, &session, config_ms)
    }
}

fn run_ungrouped_health_on_session(
    options: &HealthExecutionOptions<'_>,
    ws_roots: Option<Vec<PathBuf>>,
    session: &AnalysisSession,
    config_ms: f64,
) -> Result<HealthAnalysisResult<NoGroupResolver>, HealthError> {
    let changed_files = options
        .changed_since
        .and_then(|git_ref| session.changed_files_since(git_ref).ok());
    let parts = session.parsed_parts_uncached(true);
    let pre_computed_analysis =
        super::should_precompute_dead_code_analysis(options, session.config())
            .then(|| session.analyze_dead_code_with_parsed_modules(&parts.modules))
            .transpose()
            .map_err(|_| HealthError::message("analysis failed", 2))?;
    let workspace_diagnostics = if pre_computed_analysis.is_some() {
        session.current_workspace_diagnostics()
    } else {
        parts.workspace_diagnostics
    };

    let changed_files_analyzed = changed_files_analyzed(changed_files.as_ref(), &parts.files);
    run_ungrouped_health_from_parts(HealthRunPartsInput {
        options,
        ws_roots,
        config: parts.config,
        files: parts.files,
        modules: parts.modules,
        workspaces: parts.workspaces,
        workspace_diagnostics,
        parse_ms: parts.parse_ms,
        parse_cpu_ms: parts.parse_cpu_ms,
        changed_files,
        config_ms,
        shared_parse: false,
        pre_computed_analysis,
        pre_computed_duplication: None,
        styling_artifacts: None,
    })
    .map(|result| HealthAnalysisResult {
        changed_files_analyzed,
        ..result
    })
}

/// Run health analysis from an existing analysis session.
///
/// This lets audit and other compound programmatic surfaces share config,
/// discovery, and parser-cache state across analysis families.
///
/// # Errors
///
/// Returns the health command exit code for invalid inputs or analysis failures.
pub fn run_ungrouped_health_with_session(
    options: &HealthExecutionOptions<'_>,
    ws_roots: Option<Vec<PathBuf>>,
    session: &AnalysisSession,
    changed_files: Option<Vec<PathBuf>>,
) -> Result<HealthAnalysisResult<NoGroupResolver>, HealthError> {
    run_ungrouped_health_with_session_artifacts(
        options,
        ws_roots,
        session,
        changed_files,
        None,
        None,
    )
}

/// Run health analysis from an existing analysis session and retained
/// dead-code artifacts.
///
/// # Errors
///
/// Returns the health command exit code for invalid inputs or analysis failures.
pub fn run_ungrouped_health_with_session_artifacts(
    options: &HealthExecutionOptions<'_>,
    ws_roots: Option<Vec<PathBuf>>,
    session: &AnalysisSession,
    changed_files: Option<Vec<PathBuf>>,
    pre_computed_analysis: Option<DeadCodeAnalysisArtifacts>,
    pre_computed_duplication: Option<DuplicationReport>,
) -> Result<HealthAnalysisResult<NoGroupResolver>, HealthError> {
    validate_health_churn_file(options)?;

    let changed_files = changed_files.map(FxHashSet::from_iter).or_else(|| {
        options
            .changed_since
            .and_then(|git_ref| session.changed_files_since(git_ref).ok())
    });
    let parts = session.shared_parsed_parts(true);
    let shared_parse = parts.parse_ms == 0.0;
    let workspace_diagnostics = if pre_computed_analysis.is_some() {
        session.current_workspace_diagnostics()
    } else {
        parts.workspace_diagnostics
    };

    let styling_artifacts = options.css.then(|| session.styling_analysis_artifacts());
    let changed_files_analyzed = changed_files_analyzed(changed_files.as_ref(), &parts.files);
    run_ungrouped_health_from_parts(HealthRunPartsInput {
        options,
        ws_roots,
        config: parts.config,
        files: parts.files,
        modules: parts.modules,
        workspaces: parts.workspaces,
        workspace_diagnostics,
        parse_ms: parts.parse_ms,
        parse_cpu_ms: parts.parse_cpu_ms,
        changed_files,
        config_ms: 0.0,
        shared_parse,
        pre_computed_analysis,
        pre_computed_duplication,
        styling_artifacts,
    })
    .map(|result| HealthAnalysisResult {
        changed_files_analyzed,
        ..result
    })
}

/// The files of the changed set that discovery kept, or `None` when no changed
/// set narrowed the run.
fn changed_files_analyzed(
    changed_files: Option<&FxHashSet<PathBuf>>,
    files: &[fallow_types::discover::DiscoveredFile],
) -> Option<Vec<PathBuf>> {
    let changed = changed_files?;
    Some(
        files
            .iter()
            .filter(|file| changed.contains(&file.path))
            .map(|file| file.path.clone())
            .collect(),
    )
}

struct HealthRunPartsInput<'a, M> {
    options: &'a HealthExecutionOptions<'a>,
    ws_roots: Option<Vec<PathBuf>>,
    config: fallow_config::ResolvedConfig,
    files: Vec<fallow_types::discover::DiscoveredFile>,
    modules: M,
    workspaces: Vec<fallow_config::WorkspaceInfo>,
    workspace_diagnostics: Vec<fallow_types::workspace::WorkspaceDiagnostic>,
    parse_ms: f64,
    parse_cpu_ms: f64,
    changed_files: Option<FxHashSet<PathBuf>>,
    config_ms: f64,
    shared_parse: bool,
    pre_computed_analysis: Option<DeadCodeAnalysisArtifacts>,
    pre_computed_duplication: Option<DuplicationReport>,
    styling_artifacts: Option<Arc<super::StylingAnalysisArtifacts>>,
}

fn run_ungrouped_health_from_parts<M: AsRef<[fallow_types::extract::ModuleInfo]>>(
    input: HealthRunPartsInput<'_, M>,
) -> Result<HealthAnalysisResult<NoGroupResolver>, HealthError> {
    let HealthRunPartsInput {
        options,
        ws_roots,
        config,
        files,
        modules,
        workspaces,
        workspace_diagnostics,
        parse_ms,
        parse_cpu_ms,
        changed_files,
        config_ms,
        shared_parse,
        pre_computed_analysis,
        pre_computed_duplication,
        styling_artifacts,
    } = input;
    let scope_inputs = HealthScopeInputs::<NoGroupResolver> {
        changed_files,
        diff_index: options.diff_index,
        ws_roots,
        group_resolver: None,
    };
    let seams = HealthSeams {
        runtime_coverage_analyzer: &programmatic_runtime_coverage_seam,
        note_graph_structure: &|_module_count, _edge_count| {},
    };

    super::execute::execute_health_inner_shared(
        options,
        HealthPipelineRunInputs {
            config,
            files,
            modules,
            config_ms,
            discover_ms: 0.0,
            parse_ms,
            parse_cpu_ms,
            shared_parse,
            pre_computed_analysis,
            dead_code_results: None,
            styling_artifacts,
            pre_computed_duplication,
            workspaces,
            workspace_diagnostics,
        },
        scope_inputs,
        &seams,
    )
}

fn programmatic_runtime_coverage_seam(
    _options: &RuntimeCoverageOptions,
    _input: RuntimeCoverageSeamInput<'_>,
) -> Result<fallow_output::RuntimeCoverageReport, u8> {
    Err(2)
}
