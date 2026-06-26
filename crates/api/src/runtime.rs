//! Programmatic runtime entry points that do not depend on `fallow-cli`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use fallow_config::{
    DetectionMode, DuplicatesConfig, OutputFormat, ProductionAnalysis, WorkspaceInfo,
};
use fallow_engine::duplicates::{CloneInstance, DuplicationReport, DuplicationStats};
use fallow_engine::{
    AnalysisResults, AnalysisSession, ProgrammaticHealthRun, ProjectConfig, ProjectConfigOptions,
};
use fallow_output::{
    CHECK_SCHEMA_VERSION, CheckOutput, CheckOutputInput, DiffIndex, DupesOutput, DupesOutputInput,
    GroupByMode, HealthGroup, HealthGrouping, HealthJsonOutputInput, HealthOutputInput,
    HealthReport, MAX_DIFF_BYTES, RootEnvelopeMode, WorkspaceDiagnosticOutput, build_check_output,
    build_dupes_output, check_meta, health_meta, relative_to_diff_path,
    serialize_check_json_output, serialize_dupes_json_output, strip_root_prefix,
};
use fallow_types::{output::NextStep, path_util::is_absolute_path_any_platform};
use globset::Glob;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    AnalysisOptions, ComplexityOptions, DeadCodeFilters, DeadCodeOptions, DupesReportPayload,
    DuplicationMode, DuplicationOptions, ProgrammaticError,
};

const SCHEMA_VERSION: u32 = 1;
const HEALTH_SCHEMA_VERSION: u32 = 7;

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

/// Inputs for serializing health JSON output through the API boundary.
pub struct HealthJsonReportInput<'a> {
    pub report: HealthReport,
    pub root: &'a Path,
    pub elapsed: std::time::Duration,
    pub explain: bool,
    pub grouped_by: Option<GroupByMode>,
    pub groups: Option<Vec<HealthGroup>>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnosticOutput>,
    pub next_steps: Vec<NextStep>,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<&'a str>,
}

/// Temporary runner boundary for programmatic health while execution moves from
/// the CLI crate into the engine/API stack.
pub trait ProgrammaticHealthRunner {
    /// Run health analysis for public programmatic options.
    ///
    /// # Errors
    ///
    /// Returns a structured programmatic error when the concrete runner cannot
    /// resolve options or complete health analysis.
    fn run_programmatic_health(
        &self,
        options: &ComplexityOptions,
    ) -> Result<ProgrammaticHealthRun, ProgrammaticError>;
}

struct ResolvedAnalysisOptions {
    root: PathBuf,
    config_path: Option<PathBuf>,
    no_cache: bool,
    threads: usize,
    pool: rayon::ThreadPool,
    diff: Option<DiffIndex>,
    production_override: Option<bool>,
    changed_since: Option<String>,
    workspace_roots: Option<Vec<PathBuf>>,
    legacy_envelope: bool,
}

/// Typed programmatic dead-code output before JSON serialization.
///
/// This is the API boundary embedders should use when they need access to the
/// typed engine/output result. The `detect_*` helpers remain as JSON
/// compatibility shims over this type.
#[derive(Debug, Clone)]
pub struct DeadCodeProgrammaticOutput {
    pub output: CheckOutput,
    pub root: PathBuf,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<String>,
}

impl DeadCodeProgrammaticOutput {
    /// Serialize the typed programmatic result into the stable JSON contract.
    ///
    /// # Errors
    ///
    /// Returns a structured error if the output contract cannot be serialized.
    pub fn into_json(self) -> ProgrammaticResult<serde_json::Value> {
        let Self {
            output,
            root,
            envelope_mode,
            telemetry_analysis_run_id,
        } = self;
        let mut json = serialize_check_json_output(
            output,
            envelope_mode,
            telemetry_analysis_run_id.as_deref(),
        )
        .map_err(|err| {
            ProgrammaticError::new(format!("failed to serialize dead-code report: {err}"), 2)
                .with_code("FALLOW_SERIALIZE_DEAD_CODE_REPORT")
                .with_context("dead-code")
        })?;
        let root_prefix = format!("{}/", root.display());
        strip_root_prefix(&mut json, &root_prefix);
        Ok(json)
    }
}

/// Typed programmatic duplication output before JSON serialization.
#[derive(Debug, Clone)]
pub struct DuplicationProgrammaticOutput {
    pub output: DupesOutput<DupesReportPayload, serde_json::Value>,
    pub root: PathBuf,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<String>,
}

impl DuplicationProgrammaticOutput {
    /// Serialize the typed programmatic result into the stable JSON contract.
    ///
    /// # Errors
    ///
    /// Returns a structured error if the output contract cannot be serialized.
    pub fn into_json(self) -> ProgrammaticResult<serde_json::Value> {
        let Self {
            output,
            root,
            envelope_mode,
            telemetry_analysis_run_id,
        } = self;
        let mut json = serialize_dupes_json_output(
            output,
            envelope_mode,
            telemetry_analysis_run_id.as_deref(),
        )
        .map_err(|err| {
            ProgrammaticError::new(format!("failed to serialize duplication report: {err}"), 2)
                .with_code("FALLOW_SERIALIZE_DUPLICATION_REPORT")
                .with_context("dupes")
        })?;
        let root_prefix = format!("{}/", root.display());
        strip_root_prefix(&mut json, &root_prefix);
        Ok(json)
    }
}

/// Typed programmatic health / complexity output before JSON serialization.
#[derive(Debug, Clone)]
pub struct HealthProgrammaticOutput {
    pub report: HealthReport,
    pub grouping: Option<HealthGrouping>,
    pub root: PathBuf,
    pub elapsed: std::time::Duration,
    pub explain: bool,
    pub workspace_diagnostics: Vec<WorkspaceDiagnosticOutput>,
    pub next_steps: Vec<NextStep>,
    pub envelope_mode: RootEnvelopeMode,
    pub telemetry_analysis_run_id: Option<String>,
}

impl HealthProgrammaticOutput {
    /// Serialize the typed programmatic result into the stable JSON contract.
    ///
    /// # Errors
    ///
    /// Returns a structured error if the health output contract cannot be
    /// serialized.
    pub fn into_json(self) -> ProgrammaticResult<serde_json::Value> {
        let Self {
            report,
            grouping,
            root,
            elapsed,
            explain,
            workspace_diagnostics,
            next_steps,
            envelope_mode,
            telemetry_analysis_run_id,
        } = self;
        let (grouped_by, groups) = grouping.map_or((None, None), |grouping| {
            (
                group_by_mode_from_label(grouping.mode),
                Some(grouping.groups),
            )
        });
        serialize_health_report_json(HealthJsonReportInput {
            report,
            root: &root,
            elapsed,
            explain,
            grouped_by,
            groups,
            workspace_diagnostics,
            next_steps,
            envelope_mode,
            telemetry_analysis_run_id: telemetry_analysis_run_id.as_deref(),
        })
        .map_err(|err| {
            ProgrammaticError::new(format!("failed to serialize health report: {err}"), 2)
                .with_code("FALLOW_SERIALIZE_HEALTH_REPORT")
                .with_context("health")
        })
    }
}

/// Run duplication analysis and return the JSON output contract.
///
/// This is the first runtime path owned by `fallow-api` instead of the CLI
/// crate. It intentionally returns the same root JSON shape that embedders
/// already receive from `fallow-node`.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, git changed-file failures, or serialization failures.
pub fn detect_duplication(options: &DuplicationOptions) -> ProgrammaticResult<serde_json::Value> {
    run_duplication(options)?.into_json()
}

/// Run duplication analysis and return typed API output before serialization.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, or git changed-file failures.
pub fn run_duplication(
    options: &DuplicationOptions,
) -> ProgrammaticResult<DuplicationProgrammaticOutput> {
    let resolved = resolve_analysis_options(&options.analysis)?;
    resolved.install(|| detect_duplication_inner(options, &resolved))
}

/// Run dead-code analysis and return the JSON output contract.
///
/// This runtime path is owned by `fallow-api` and uses the typed engine plus
/// output crates directly. NAPI still calls the legacy CLI-backed route until
/// follow-up parity slices move the public binding over.
///
/// # Errors
///
/// Returns a structured programmatic error for unsupported options, invalid
/// options, config load failures, analysis failures, git changed-file failures,
/// or serialization failures.
pub fn detect_dead_code(options: &DeadCodeOptions) -> ProgrammaticResult<serde_json::Value> {
    run_dead_code(options)?.into_json()
}

/// Run dead-code analysis and return typed API output before serialization.
///
/// # Errors
///
/// Returns a structured programmatic error for unsupported options, invalid
/// options, config load failures, analysis failures, or git changed-file
/// failures.
pub fn run_dead_code(options: &DeadCodeOptions) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    let resolved = resolve_analysis_options(&options.analysis)?;
    resolved.install(|| detect_dead_code_inner(options, &resolved, |_| {}))
}

/// Run circular-dependency analysis and return the dead-code JSON envelope.
///
/// This is a convenience wrapper over the typed dead-code runtime. It keeps the
/// envelope shape stable while narrowing results to `circular_dependencies`.
///
/// # Errors
///
/// Returns the same structured errors as [`detect_dead_code`].
pub fn detect_circular_dependencies(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<serde_json::Value> {
    run_circular_dependencies(options)?.into_json()
}

/// Run circular-dependency analysis and return typed API output before JSON.
///
/// # Errors
///
/// Returns the same structured errors as [`run_dead_code`].
pub fn run_circular_dependencies(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    let resolved = resolve_analysis_options(&options.analysis)?;
    resolved.install(|| detect_dead_code_inner(options, &resolved, keep_circular_dependencies))
}

/// Run boundary-family analysis and return the dead-code JSON envelope.
///
/// This is a convenience wrapper over the typed dead-code runtime. It keeps
/// `boundary_violations`, `boundary_coverage_violations`, and
/// `boundary_call_violations`.
///
/// # Errors
///
/// Returns the same structured errors as [`detect_dead_code`].
pub fn detect_boundary_violations(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<serde_json::Value> {
    run_boundary_violations(options)?.into_json()
}

/// Run boundary-family analysis and return typed API output before JSON.
///
/// # Errors
///
/// Returns the same structured errors as [`run_dead_code`].
pub fn run_boundary_violations(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    let resolved = resolve_analysis_options(&options.analysis)?;
    resolved.install(|| detect_dead_code_inner(options, &resolved, keep_boundary_violations))
}

/// Serialize a health / complexity report into the stable JSON output contract.
///
/// The health runner is still migrating out of the CLI crate, so callers pass
/// the already assembled report plus CLI-owned suggestion and workspace
/// diagnostics policy as explicit typed inputs.
///
/// # Errors
///
/// Returns a serde error when the report cannot be converted to JSON.
pub fn serialize_health_report_json(
    input: HealthJsonReportInput<'_>,
) -> Result<serde_json::Value, serde_json::Error> {
    let root_prefix = format!("{}/", input.root.display());
    fallow_output::serialize_health_json_output(HealthJsonOutputInput {
        output: HealthOutputInput {
            schema_version: HEALTH_SCHEMA_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            elapsed: input.elapsed,
            report: input.report,
            grouped_by: input.grouped_by,
            groups: input.groups,
            meta: input.explain.then(health_meta),
            workspace_diagnostics: input.workspace_diagnostics,
            next_steps: input.next_steps,
        },
        root_prefix: Some(&root_prefix),
        envelope_mode: input.envelope_mode,
        analysis_run_id: input.telemetry_analysis_run_id,
    })
}

/// Run programmatic health / complexity through the API-owned output boundary.
///
/// The concrete runner is injected while the health implementation is still
/// being migrated out of the CLI crate. Runner-owned responsibilities are
/// limited to typed analysis plus runtime facts; this API crate owns the final
/// JSON contract assembly.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, runner
/// failures, or output serialization failures.
pub fn compute_complexity_with_runner(
    options: &ComplexityOptions,
    runner: &impl ProgrammaticHealthRunner,
) -> ProgrammaticResult<serde_json::Value> {
    run_complexity_with_runner(options, runner)?.into_json()
}

/// Run programmatic health / complexity and return typed API output.
///
/// The concrete runner is injected while the health implementation is still
/// being migrated out of the CLI crate. Runner-owned responsibilities are
/// limited to typed analysis plus runtime facts; this API crate owns the final
/// programmatic report assembly.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options or runner
/// failures.
pub fn run_complexity_with_runner(
    options: &ComplexityOptions,
    runner: &impl ProgrammaticHealthRunner,
) -> ProgrammaticResult<HealthProgrammaticOutput> {
    crate::validate_complexity_options(options)?;
    let ProgrammaticHealthRun {
        analysis,
        workspace_diagnostics,
        next_step_facts,
        telemetry_analysis_run_id,
    } = runner.run_programmatic_health(options)?;
    let root = analysis.config.root.clone();
    let next_steps =
        fallow_output::build_health_next_steps(fallow_output::build_health_next_steps_input(
            &analysis.report,
            next_step_facts.suggestions_enabled,
            next_step_facts.offer_setup,
            next_step_facts.impact_digest,
            next_step_facts.audit_changed,
        ));
    Ok(HealthProgrammaticOutput {
        report: analysis.report,
        grouping: analysis.grouping,
        root,
        elapsed: analysis.elapsed,
        explain: options.analysis.explain,
        workspace_diagnostics,
        next_steps,
        envelope_mode: root_envelope_mode(options.analysis.legacy_envelope),
        telemetry_analysis_run_id,
    })
}

/// Alias for [`compute_complexity_with_runner`] with a product-oriented name.
///
/// # Errors
///
/// Returns the same structured errors as [`compute_complexity_with_runner`].
pub fn compute_health_with_runner(
    options: &ComplexityOptions,
    runner: &impl ProgrammaticHealthRunner,
) -> ProgrammaticResult<serde_json::Value> {
    run_health_with_runner(options, runner)?.into_json()
}

/// Alias for [`run_complexity_with_runner`] with a product-oriented name.
///
/// # Errors
///
/// Returns the same structured errors as [`run_complexity_with_runner`].
pub fn run_health_with_runner(
    options: &ComplexityOptions,
    runner: &impl ProgrammaticHealthRunner,
) -> ProgrammaticResult<HealthProgrammaticOutput> {
    run_complexity_with_runner(options, runner)
}

fn group_by_mode_from_label(label: &str) -> Option<GroupByMode> {
    match label {
        "owner" => Some(GroupByMode::Owner),
        "directory" => Some(GroupByMode::Directory),
        "package" => Some(GroupByMode::Package),
        "section" => Some(GroupByMode::Section),
        _ => None,
    }
}

fn detect_dead_code_inner(
    options: &DeadCodeOptions,
    resolved: &ResolvedAnalysisOptions,
    post_filter: impl FnOnce(&mut AnalysisResults),
) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    let start = Instant::now();
    let session = load_dead_code_session(options, resolved)?;
    let analysis = session.analyze_dead_code().map_err(|err| {
        ProgrammaticError::new(format!("dead-code analysis failed: {err}"), 2)
            .with_code("FALLOW_DEAD_CODE_FAILED")
            .with_context("dead-code")
    })?;
    let mut results = analysis.results;

    apply_dead_code_scope(options, resolved, &session, &mut results)?;
    apply_dead_code_filters(&options.filters, &mut results);
    post_filter(&mut results);

    let output = build_check_output(CheckOutputInput {
        schema_version: CHECK_SCHEMA_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        elapsed: start.elapsed(),
        results,
        config_fixable: fallow_config::is_config_fixable(
            &resolved.root,
            resolved.config_path.as_ref(),
        ),
        meta: options.analysis.explain.then(check_meta),
        workspace_diagnostics: Vec::new(),
        next_steps: Vec::new(),
    });
    Ok(DeadCodeProgrammaticOutput {
        output,
        root: session.root().to_path_buf(),
        envelope_mode: root_envelope_mode(resolved.legacy_envelope),
        telemetry_analysis_run_id: None,
    })
}

fn keep_circular_dependencies(results: &mut AnalysisResults) {
    let entry_point_summary = results.entry_point_summary.take();
    let circular_dependencies = std::mem::take(&mut results.circular_dependencies);
    *results = AnalysisResults::default();
    results.entry_point_summary = entry_point_summary;
    results.circular_dependencies = circular_dependencies;
}

fn keep_boundary_violations(results: &mut AnalysisResults) {
    let entry_point_summary = results.entry_point_summary.take();
    let boundary_violations = std::mem::take(&mut results.boundary_violations);
    let boundary_coverage_violations = std::mem::take(&mut results.boundary_coverage_violations);
    let boundary_call_violations = std::mem::take(&mut results.boundary_call_violations);
    *results = AnalysisResults::default();
    results.entry_point_summary = entry_point_summary;
    results.boundary_violations = boundary_violations;
    results.boundary_coverage_violations = boundary_coverage_violations;
    results.boundary_call_violations = boundary_call_violations;
}

fn load_dead_code_session(
    options: &DeadCodeOptions,
    resolved: &ResolvedAnalysisOptions,
) -> ProgrammaticResult<AnalysisSession> {
    let project_config = fallow_engine::config_for_project_analysis(
        &resolved.root,
        resolved.config_path.as_deref(),
        ProjectConfigOptions {
            output: OutputFormat::Json,
            no_cache: resolved.no_cache,
            threads: resolved.threads,
            production_override: resolved.production_override,
            quiet: true,
            analysis: ProductionAnalysis::DeadCode,
        },
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to load config: {err}"), 2)
            .with_code("FALLOW_CONFIG_LOAD_FAILED")
            .with_context("analysis.configPath")
    })?;
    let project_config = configure_project_for_dead_code(project_config, options);
    Ok(AnalysisSession::from_config(project_config))
}

fn configure_project_for_dead_code(
    mut project_config: ProjectConfig,
    options: &DeadCodeOptions,
) -> ProjectConfig {
    if options.include_entry_exports {
        project_config.config.include_entry_exports = true;
    }
    activate_explicit_dead_code_opt_ins(&options.filters, &mut project_config.config.rules);
    project_config
}

fn activate_explicit_dead_code_opt_ins(
    filters: &DeadCodeFilters,
    rules: &mut fallow_config::RulesConfig,
) {
    if filters.private_type_leaks && rules.private_type_leaks == fallow_config::Severity::Off {
        rules.private_type_leaks = fallow_config::Severity::Warn;
    }
}

fn apply_dead_code_scope(
    options: &DeadCodeOptions,
    resolved: &ResolvedAnalysisOptions,
    session: &AnalysisSession,
    results: &mut AnalysisResults,
) -> ProgrammaticResult<()> {
    if let Some(workspace_roots) = resolved.workspace_roots.as_ref() {
        fallow_engine::dead_code::filter_to_workspaces(results, workspace_roots);
    }
    if let Some(changed_files) = changed_files_for_run(resolved)? {
        fallow_engine::dead_code::filter_by_changed_files(results, &changed_files);
    }
    if let Some(diff) = resolved.diff.as_ref() {
        filter_dead_code_by_diff(results, diff, session.root());
    }
    apply_dead_code_file_filter(options, session.root(), results);
    Ok(())
}

fn filter_dead_code_by_diff(results: &mut AnalysisResults, diff: &DiffIndex, root: &Path) {
    let touches_file = |path: &Path| -> bool {
        relative_to_diff_path(path, root).is_none_or(|rel| diff.touches_file(&rel))
    };
    let line_in_diff = |path: &Path, line: u32| -> bool {
        relative_to_diff_path(path, root)
            .is_none_or(|rel| diff.line_is_added(&rel, u64::from(line)))
    };

    filter_dead_code_source_findings(results, &touches_file, &line_in_diff);
    filter_dead_code_security_findings(results, &touches_file, &line_in_diff);
    filter_dead_code_dependency_findings(results, &line_in_diff);
    filter_dead_code_graph_findings(results, &touches_file, &line_in_diff);
    filter_dead_code_framework_findings(results, &line_in_diff);
}

fn filter_dead_code_source_findings(
    results: &mut AnalysisResults,
    touches_file: &dyn Fn(&Path) -> bool,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results
        .unused_files
        .retain(|finding| touches_file(&finding.file.path));
    results
        .unused_exports
        .retain(|finding| line_in_diff(&finding.export.path, finding.export.line));
    results
        .unused_types
        .retain(|finding| line_in_diff(&finding.export.path, finding.export.line));
    results
        .private_type_leaks
        .retain(|finding| line_in_diff(&finding.leak.path, finding.leak.line));
    results
        .unused_enum_members
        .retain(|finding| line_in_diff(&finding.member.path, finding.member.line));
    results
        .unused_class_members
        .retain(|finding| line_in_diff(&finding.member.path, finding.member.line));
    results
        .unused_store_members
        .retain(|finding| line_in_diff(&finding.member.path, finding.member.line));
    results
        .unprovided_injects
        .retain(|finding| line_in_diff(&finding.inject.path, finding.inject.line));
    results
        .unrendered_components
        .retain(|finding| line_in_diff(&finding.component.path, finding.component.line));
    results
        .unused_component_props
        .retain(|finding| line_in_diff(&finding.prop.path, finding.prop.line));
    results
        .unused_component_emits
        .retain(|finding| line_in_diff(&finding.emit.path, finding.emit.line));
    results
        .unused_component_inputs
        .retain(|finding| line_in_diff(&finding.input.path, finding.input.line));
    results
        .unused_component_outputs
        .retain(|finding| line_in_diff(&finding.output.path, finding.output.line));
    results
        .unused_svelte_events
        .retain(|finding| line_in_diff(&finding.event.path, finding.event.line));
    results
        .unused_server_actions
        .retain(|finding| line_in_diff(&finding.action.path, finding.action.line));
    results
        .unused_load_data_keys
        .retain(|finding| line_in_diff(&finding.key.path, finding.key.line));
    results
        .unresolved_imports
        .retain(|finding| line_in_diff(&finding.import.path, finding.import.line));
}

fn filter_dead_code_security_findings(
    results: &mut AnalysisResults,
    touches_file: &dyn Fn(&Path) -> bool,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results.security_findings.retain(|finding| {
        line_in_diff(&finding.path, finding.line)
            || finding.trace.iter().any(|hop| {
                line_in_diff(&hop.path, hop.line)
                    || (matches!(hop.role, fallow_engine::results::TraceHopRole::SecretSource)
                        && touches_file(&hop.path))
            })
            || finding.reachability.as_ref().is_some_and(|reachability| {
                reachability
                    .untrusted_source_trace
                    .iter()
                    .any(|hop| line_in_diff(&hop.path, hop.line))
            })
    });
    results
        .security_unresolved_callee_diagnostics
        .retain(|finding| line_in_diff(&finding.path, finding.line));
}

fn filter_dead_code_dependency_findings(
    results: &mut AnalysisResults,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    for finding in &mut results.unlisted_dependencies {
        finding
            .dep
            .imported_from
            .retain(|source| line_in_diff(&source.path, source.line));
    }
    results
        .unlisted_dependencies
        .retain(|finding| !finding.dep.imported_from.is_empty());
}

fn filter_dead_code_graph_findings(
    results: &mut AnalysisResults,
    touches_file: &dyn Fn(&Path) -> bool,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results.duplicate_exports.retain(|finding| {
        finding
            .export
            .locations
            .iter()
            .any(|location| line_in_diff(&location.path, location.line))
    });
    results
        .circular_dependencies
        .retain(|cycle| cycle.cycle.files.iter().any(|path| touches_file(path)));
    results
        .re_export_cycles
        .retain(|cycle| cycle.cycle.files.iter().any(|path| touches_file(path)));
    results
        .boundary_violations
        .retain(|finding| line_in_diff(&finding.violation.from_path, finding.violation.line));
    results
        .stale_suppressions
        .retain(|finding| line_in_diff(&finding.path, finding.line));
}

fn filter_dead_code_framework_findings(
    results: &mut AnalysisResults,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results
        .invalid_client_exports
        .retain(|finding| line_in_diff(&finding.export.path, finding.export.line));
    results
        .mixed_client_server_barrels
        .retain(|finding| line_in_diff(&finding.barrel.path, finding.barrel.line));
    results
        .misplaced_directives
        .retain(|finding| line_in_diff(&finding.directive_site.path, finding.directive_site.line));
    results
        .route_collisions
        .retain(|finding| line_in_diff(&finding.collision.path, finding.collision.line));
    results
        .dynamic_segment_name_conflicts
        .retain(|finding| line_in_diff(&finding.conflict.path, finding.conflict.line));
}

fn apply_dead_code_file_filter(
    options: &DeadCodeOptions,
    root: &Path,
    results: &mut AnalysisResults,
) {
    if options.files.is_empty() {
        return;
    }
    let file_set = options
        .files
        .iter()
        .map(|path| {
            if is_absolute_path_any_platform(path) {
                path.clone()
            } else {
                root.join(path)
            }
        })
        .collect::<FxHashSet<_>>();
    fallow_engine::dead_code::filter_by_changed_files(results, &file_set);
    clear_dead_code_dependency_findings(results);
}

fn apply_dead_code_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !dead_code_filters_active(filters) {
        return;
    }
    apply_dead_code_core_filters(filters, results);
    apply_dead_code_component_filters(filters, results);
    apply_dead_code_graph_filters(filters, results);
    apply_dead_code_policy_filters(filters, results);
    apply_dead_code_catalog_filters(filters, results);
}

fn dead_code_filters_active(filters: &DeadCodeFilters) -> bool {
    filters.unused_files
        || filters.unused_exports
        || filters.unused_deps
        || filters.unused_types
        || filters.private_type_leaks
        || filters.unused_enum_members
        || filters.unused_class_members
        || filters.unused_store_members
        || filters.unprovided_injects
        || filters.unrendered_components
        || filters.unused_component_props
        || filters.unused_component_emits
        || filters.unused_component_inputs
        || filters.unused_component_outputs
        || filters.unused_svelte_events
        || filters.unused_server_actions
        || filters.unused_load_data_keys
        || filters.unresolved_imports
        || filters.unlisted_deps
        || filters.duplicate_exports
        || filters.circular_deps
        || filters.re_export_cycles
        || filters.boundary_violations
        || filters.policy_violations
        || filters.stale_suppressions
        || filters.unused_catalog_entries
        || filters.empty_catalog_groups
        || filters.unresolved_catalog_references
        || filters.unused_dependency_overrides
        || filters.misconfigured_dependency_overrides
}

fn apply_dead_code_core_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.unused_files {
        results.unused_files.clear();
    }
    if !filters.unused_exports {
        results.unused_exports.clear();
    }
    if !filters.unused_types {
        results.unused_types.clear();
    }
    if !filters.private_type_leaks {
        results.private_type_leaks.clear();
    }
    if !filters.unused_deps {
        clear_dead_code_dependency_findings(results);
    }
    if !filters.unused_enum_members {
        results.unused_enum_members.clear();
    }
    if !filters.unused_class_members {
        results.unused_class_members.clear();
    }
    if !filters.unused_store_members {
        results.unused_store_members.clear();
    }
    if !filters.unlisted_deps {
        results.unlisted_dependencies.clear();
    }
}

fn clear_dead_code_dependency_findings(results: &mut AnalysisResults) {
    results.unused_dependencies.clear();
    results.unused_dev_dependencies.clear();
    results.unused_optional_dependencies.clear();
    results.type_only_dependencies.clear();
    results.test_only_dependencies.clear();
}

fn apply_dead_code_component_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.unprovided_injects {
        results.unprovided_injects.clear();
    }
    if !filters.unrendered_components {
        results.unrendered_components.clear();
    }
    if !filters.unused_component_props {
        results.unused_component_props.clear();
    }
    if !filters.unused_component_emits {
        results.unused_component_emits.clear();
    }
    if !filters.unused_component_inputs {
        results.unused_component_inputs.clear();
    }
    if !filters.unused_component_outputs {
        results.unused_component_outputs.clear();
    }
    if !filters.unused_svelte_events {
        results.unused_svelte_events.clear();
    }
    if !filters.unused_server_actions {
        results.unused_server_actions.clear();
    }
    if !filters.unused_load_data_keys {
        results.unused_load_data_keys.clear();
    }
    if !filters.unresolved_imports {
        results.unresolved_imports.clear();
    }
}

fn apply_dead_code_graph_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.duplicate_exports {
        results.duplicate_exports.clear();
    }
    if !filters.circular_deps {
        results.circular_dependencies.clear();
    }
    if !filters.re_export_cycles {
        results.re_export_cycles.clear();
    }
    if !filters.boundary_violations {
        results.boundary_violations.clear();
        results.boundary_coverage_violations.clear();
        results.boundary_call_violations.clear();
    }
}

fn apply_dead_code_policy_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.policy_violations {
        results.policy_violations.clear();
    }
    if !filters.stale_suppressions {
        results.stale_suppressions.clear();
    }
}

fn apply_dead_code_catalog_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.unused_catalog_entries {
        results.unused_catalog_entries.clear();
    }
    if !filters.empty_catalog_groups {
        results.empty_catalog_groups.clear();
    }
    if !filters.unresolved_catalog_references {
        results.unresolved_catalog_references.clear();
    }
    if !filters.unused_dependency_overrides {
        results.unused_dependency_overrides.clear();
    }
    if !filters.misconfigured_dependency_overrides {
        results.misconfigured_dependency_overrides.clear();
    }
}

fn detect_duplication_inner(
    options: &DuplicationOptions,
    resolved: &ResolvedAnalysisOptions,
) -> ProgrammaticResult<DuplicationProgrammaticOutput> {
    let start = Instant::now();
    let session = load_duplication_session(options, resolved)?;
    let dupes_config = build_dupes_config(options, &session.config().duplicates);
    let changed_files = changed_files_for_run(resolved)?;
    let cache_dir = (!resolved.no_cache).then_some(session.config().cache_dir.as_path());
    let mut report = if let Some(changed_files) = changed_files.as_ref() {
        let changed_files = changed_files.iter().cloned().collect::<Vec<_>>();
        session
            .find_duplicates_touching_files_with_defaults(&dupes_config, &changed_files, cache_dir)
            .report
    } else {
        session
            .find_duplicates_with_defaults(&dupes_config, cache_dir)
            .report
    };

    if let Some(diff) = resolved.diff.as_ref() {
        filter_by_diff(&mut report, diff, session.root());
    }
    if let Some(workspace_roots) = resolved.workspace_roots.as_ref() {
        filter_by_workspaces(&mut report, workspace_roots, session.root());
    }
    if let Some(top) = options.top {
        apply_top(&mut report, top, session.root());
    }

    let payload = DupesReportPayload::from_report(&report);
    let output: DupesOutput<DupesReportPayload, serde_json::Value> =
        build_dupes_output(DupesOutputInput {
            schema_version: SCHEMA_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            elapsed: start.elapsed(),
            report: payload,
            grouped_by: None,
            total_issues: None,
            groups: None,
            meta: None,
            workspace_diagnostics: Vec::new(),
            next_steps: Vec::new(),
        });
    Ok(DuplicationProgrammaticOutput {
        output,
        root: session.root().to_path_buf(),
        envelope_mode: root_envelope_mode(resolved.legacy_envelope),
        telemetry_analysis_run_id: None,
    })
}

fn load_duplication_session(
    options: &DuplicationOptions,
    resolved: &ResolvedAnalysisOptions,
) -> ProgrammaticResult<AnalysisSession> {
    let project_config =
        fallow_engine::config_for_project(&resolved.root, resolved.config_path.as_deref())
            .map_err(|err| {
                ProgrammaticError::new(format!("failed to load config: {err}"), 2)
                    .with_code("FALLOW_CONFIG_LOAD_FAILED")
                    .with_context("analysis.configPath")
            })?;
    let project_config = configure_project_for_duplication(project_config, options, resolved);
    Ok(AnalysisSession::from_config(project_config))
}

fn configure_project_for_duplication(
    mut project_config: ProjectConfig,
    options: &DuplicationOptions,
    resolved: &ResolvedAnalysisOptions,
) -> ProjectConfig {
    let production = resolved
        .production_override
        .unwrap_or(project_config.config.production);
    project_config.config.production = production;
    project_config.config.output = OutputFormat::Json;
    project_config.config.threads = resolved.threads;
    project_config.config.no_cache = resolved.no_cache;
    project_config.config.duplicates =
        build_dupes_config(options, &project_config.config.duplicates);
    project_config
}

fn build_dupes_config(options: &DuplicationOptions, config: &DuplicatesConfig) -> DuplicatesConfig {
    DuplicatesConfig {
        enabled: true,
        mode: duplication_mode_to_config(options.mode),
        min_tokens: options.min_tokens,
        min_lines: options.min_lines,
        min_occurrences: options.min_occurrences,
        threshold: options.threshold,
        ignore: config.ignore.clone(),
        ignore_defaults: config.ignore_defaults,
        skip_local: options.skip_local || config.skip_local,
        cross_language: options.cross_language || config.cross_language,
        ignore_imports: options.ignore_imports.unwrap_or(config.ignore_imports),
        normalization: config.normalization.clone(),
        min_corpus_size_for_shingle_filter: config.min_corpus_size_for_shingle_filter,
        min_corpus_size_for_token_cache: config.min_corpus_size_for_token_cache,
    }
}

const fn duplication_mode_to_config(mode: DuplicationMode) -> DetectionMode {
    match mode {
        DuplicationMode::Strict => DetectionMode::Strict,
        DuplicationMode::Mild => DetectionMode::Mild,
        DuplicationMode::Weak => DetectionMode::Weak,
        DuplicationMode::Semantic => DetectionMode::Semantic,
    }
}

fn resolve_analysis_options(
    options: &AnalysisOptions,
) -> ProgrammaticResult<ResolvedAnalysisOptions> {
    validate_analysis_option_shape(options)?;
    let root = resolve_analysis_root(options.root.as_deref())?;
    validate_analysis_config_path(options.config_path.as_deref())?;
    let threads = options.threads.unwrap_or_else(default_threads);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|err| {
            ProgrammaticError::new(format!("failed to build analysis thread pool: {err}"), 2)
                .with_code("FALLOW_THREAD_POOL_INIT_FAILED")
                .with_context("analysis.threads")
        })?;
    let diff = options
        .diff_file
        .as_deref()
        .map(|path| load_explicit_diff_file(path, &root))
        .transpose()?;
    let workspace_roots = resolve_workspace_scope(
        &root,
        options.workspace.as_deref(),
        options.changed_workspaces.as_deref(),
    )?;
    Ok(ResolvedAnalysisOptions {
        root,
        config_path: options.config_path.clone(),
        no_cache: options.no_cache,
        threads,
        pool,
        diff,
        production_override: options
            .production_override
            .or_else(|| options.production.then_some(true)),
        changed_since: options.changed_since.clone(),
        workspace_roots,
        legacy_envelope: options.legacy_envelope,
    })
}

fn validate_analysis_option_shape(options: &AnalysisOptions) -> ProgrammaticResult<()> {
    if options.threads == Some(0) {
        return Err(
            ProgrammaticError::new("`threads` must be greater than 0", 2)
                .with_code("FALLOW_INVALID_THREADS")
                .with_context("analysis.threads"),
        );
    }
    if options.workspace.is_some() && options.changed_workspaces.is_some() {
        return Err(ProgrammaticError::new(
            "`workspace` and `changed_workspaces` are mutually exclusive",
            2,
        )
        .with_code("FALLOW_MUTUALLY_EXCLUSIVE_SCOPE")
        .with_context("analysis.workspace"));
    }
    Ok(())
}

fn resolve_analysis_root(root: Option<&Path>) -> ProgrammaticResult<PathBuf> {
    let root = match root {
        Some(root) => root.to_path_buf(),
        None => std::env::current_dir().map_err(|err| {
            ProgrammaticError::new(
                format!("failed to resolve current working directory: {err}"),
                2,
            )
            .with_code("FALLOW_CWD_UNAVAILABLE")
            .with_context("analysis.root")
        })?,
    };
    if !root.exists() {
        return Err(ProgrammaticError::new(
            format!("analysis root does not exist: {}", root.display()),
            2,
        )
        .with_code("FALLOW_INVALID_ROOT")
        .with_context("analysis.root"));
    }
    if !root.is_dir() {
        return Err(ProgrammaticError::new(
            format!("analysis root is not a directory: {}", root.display()),
            2,
        )
        .with_code("FALLOW_INVALID_ROOT")
        .with_context("analysis.root"));
    }
    Ok(root)
}

fn validate_analysis_config_path(config_path: Option<&Path>) -> ProgrammaticResult<()> {
    if let Some(config_path) = config_path
        && !config_path.exists()
    {
        return Err(ProgrammaticError::new(
            format!("config file does not exist: {}", config_path.display()),
            2,
        )
        .with_code("FALLOW_INVALID_CONFIG_PATH")
        .with_context("analysis.configPath"));
    }
    Ok(())
}

impl ResolvedAnalysisOptions {
    fn install<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.pool.install(f)
    }
}

fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

fn load_explicit_diff_file(path: &Path, root: &Path) -> ProgrammaticResult<DiffIndex> {
    if path == Path::new("-") {
        return Err(ProgrammaticError::new(
            "`diff_file` does not support stdin; pass a file path",
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile"));
    }
    let abs = if is_absolute_path_any_platform(path) {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let meta = std::fs::metadata(&abs).map_err(|err| {
        ProgrammaticError::new(
            format!(
                "diff file does not exist or cannot be read: {} ({err})",
                abs.display()
            ),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile")
    })?;
    if !meta.is_file() {
        return Err(ProgrammaticError::new(
            format!("diff path is not a file: {}", abs.display()),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile"));
    }
    if meta.len() > MAX_DIFF_BYTES {
        return Err(ProgrammaticError::new(
            format!(
                "diff file is {} bytes, above the {MAX_DIFF_BYTES} byte limit: {}",
                meta.len(),
                abs.display()
            ),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile"));
    }
    let text = std::fs::read_to_string(&abs).map_err(|err| {
        ProgrammaticError::new(
            format!("failed to read diff file {}: {err}", abs.display()),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile")
    })?;
    Ok(DiffIndex::from_unified_diff(&text))
}

fn changed_files_for_run(
    resolved: &ResolvedAnalysisOptions,
) -> ProgrammaticResult<Option<FxHashSet<PathBuf>>> {
    let Some(git_ref) = resolved.changed_since.as_deref() else {
        return Ok(None);
    };
    fallow_engine::changed_files(&resolved.root, git_ref)
        .map(Some)
        .map_err(|err| {
            ProgrammaticError::new(
                format!(
                    "failed to resolve changed files for ref `{git_ref}`: {}",
                    err.describe()
                ),
                2,
            )
            .with_code("FALLOW_CHANGED_FILES_FAILED")
            .with_context("analysis.changedSince")
        })
}

fn resolve_workspace_scope(
    root: &Path,
    workspace: Option<&[String]>,
    changed_workspaces: Option<&str>,
) -> ProgrammaticResult<Option<Vec<PathBuf>>> {
    match (workspace, changed_workspaces) {
        (Some(patterns), None) => resolve_workspace_filters(root, patterns).map(Some),
        (None, Some(git_ref)) => resolve_changed_workspaces(root, git_ref).map(Some),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => Err(ProgrammaticError::new(
            "`workspace` and `changed_workspaces` are mutually exclusive",
            2,
        )
        .with_code("FALLOW_MUTUALLY_EXCLUSIVE_SCOPE")
        .with_context("analysis.workspace")),
    }
}

fn resolve_workspace_filters(root: &Path, patterns: &[String]) -> ProgrammaticResult<Vec<PathBuf>> {
    let workspaces = fallow_config::discover_workspaces(root);
    if workspaces.is_empty() {
        let joined = patterns
            .iter()
            .map(|pattern| format!("'{pattern}'"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ProgrammaticError::new(
            format!(
                "`workspace` {joined} specified but no workspaces found. Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, or tsconfig.json has \"references\"."
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACES_NOT_FOUND")
        .with_context("analysis.workspace"));
    }

    let rel_paths = workspaces
        .iter()
        .map(|workspace| relative_workspace_path(&workspace.root, root))
        .collect::<Vec<_>>();
    let (positive, negative) = split_workspace_patterns(patterns);
    let mut matched = match_positive_workspace_patterns(&positive, &workspaces, &rel_paths)?;

    for pattern in &negative {
        for index in find_workspace_matches(pattern, &workspaces, &rel_paths)? {
            matched.remove(&index);
        }
    }

    if matched.is_empty() {
        return Err(
            ProgrammaticError::new("`workspace` excluded every discovered workspace", 2)
                .with_code("FALLOW_WORKSPACE_SCOPE_EMPTY")
                .with_context("analysis.workspace"),
        );
    }

    let mut roots = matched
        .into_iter()
        .map(|index| workspaces[index].root.clone())
        .collect::<Vec<_>>();
    roots.sort();
    Ok(roots)
}

fn resolve_changed_workspaces(root: &Path, git_ref: &str) -> ProgrammaticResult<Vec<PathBuf>> {
    let workspaces = fallow_config::discover_workspaces(root);
    if workspaces.is_empty() {
        return Err(ProgrammaticError::new(
            format!(
                "`changed_workspaces` '{git_ref}' specified but no workspaces found. Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, or tsconfig.json has \"references\"."
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACES_NOT_FOUND")
        .with_context("analysis.changedWorkspaces"));
    }
    let changed_files = fallow_engine::changed_files(root, git_ref).map_err(|err| {
        ProgrammaticError::new(
            format!(
                "failed to resolve changed workspaces for ref `{git_ref}`: {}",
                err.describe()
            ),
            2,
        )
        .with_code("FALLOW_CHANGED_WORKSPACES_FAILED")
        .with_context("analysis.changedWorkspaces")
    })?;
    let mut roots = workspaces
        .into_iter()
        .filter(|workspace| {
            changed_files
                .iter()
                .any(|file| file.starts_with(&workspace.root))
        })
        .map(|workspace| workspace.root)
        .collect::<Vec<_>>();
    roots.sort();
    Ok(roots)
}

fn match_positive_workspace_patterns(
    positive: &[&str],
    workspaces: &[WorkspaceInfo],
    rel_paths: &[String],
) -> ProgrammaticResult<FxHashSet<usize>> {
    let mut matched = FxHashSet::default();
    let mut unmatched = Vec::new();

    if positive.is_empty() {
        matched.extend(0..workspaces.len());
    } else {
        for pattern in positive {
            let hits = find_workspace_matches(pattern, workspaces, rel_paths)?;
            if hits.is_empty() {
                unmatched.push((*pattern).to_string());
            }
            matched.extend(hits);
        }
    }

    if !unmatched.is_empty() {
        return Err(ProgrammaticError::new(
            format!(
                "`workspace` matched no workspace for pattern{}: {}. Available: {}",
                if unmatched.len() == 1 { "" } else { "s" },
                unmatched
                    .iter()
                    .map(|pattern| format!("'{pattern}'"))
                    .collect::<Vec<_>>()
                    .join(", "),
                format_available_workspaces(workspaces),
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACE_PATTERN_UNMATCHED")
        .with_context("analysis.workspace"));
    }

    Ok(matched)
}

fn find_workspace_matches(
    pattern: &str,
    workspaces: &[WorkspaceInfo],
    rel_paths: &[String],
) -> ProgrammaticResult<Vec<usize>> {
    if let Some(index) = workspaces
        .iter()
        .position(|workspace| workspace.name == pattern)
    {
        return Ok(vec![index]);
    }
    if let Some(index) = rel_paths.iter().position(|path| path == pattern) {
        return Ok(vec![index]);
    }

    let glob = Glob::new(pattern).map_err(|err| {
        ProgrammaticError::new(format!("invalid `workspace` pattern '{pattern}': {err}"), 2)
            .with_code("FALLOW_INVALID_WORKSPACE_PATTERN")
            .with_context("analysis.workspace")
    })?;
    let matcher = glob.compile_matcher();
    let hits = workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, workspace)| {
            (matcher.is_match(&workspace.name) || matcher.is_match(&rel_paths[index]))
                .then_some(index)
        })
        .collect();
    Ok(hits)
}

fn split_workspace_patterns(patterns: &[String]) -> (Vec<&str>, Vec<&str>) {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for pattern in patterns {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(negative_pattern) = trimmed.strip_prefix('!') {
            let negative_pattern = negative_pattern.trim();
            if !negative_pattern.is_empty() {
                negative.push(negative_pattern);
            }
        } else {
            positive.push(trimmed);
        }
    }
    (positive, negative)
}

fn format_available_workspaces(workspaces: &[WorkspaceInfo]) -> String {
    const MAX_SHOWN: usize = 10;
    let total = workspaces.len();
    if total <= MAX_SHOWN {
        return workspaces
            .iter()
            .map(|workspace| workspace.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
    }
    let shown = workspaces
        .iter()
        .take(MAX_SHOWN)
        .map(|workspace| workspace.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{shown}, ... and {} more ({total} total)",
        total - MAX_SHOWN
    )
}

fn relative_workspace_path(workspace_root: &Path, root: &Path) -> String {
    workspace_root
        .strip_prefix(root)
        .unwrap_or(workspace_root)
        .to_string_lossy()
        .replace('\\', "/")
}

fn filter_by_diff(report: &mut DuplicationReport, diff_index: &DiffIndex, root: &Path) {
    let instance_overlaps = |instance: &CloneInstance| -> bool {
        let Some(rel) = relative_to_diff_path(&instance.file, root) else {
            return true;
        };
        let start = u64::try_from(instance.start_line).unwrap_or(u64::MAX);
        let end = u64::try_from(instance.end_line).unwrap_or(u64::MAX);
        diff_index.range_overlaps_added(&rel, start, end)
    };
    report
        .clone_groups
        .retain(|g| g.instances.iter().any(instance_overlaps));
    rebuild_duplication_derived_fields(report, root);
}

fn filter_by_workspaces(report: &mut DuplicationReport, workspace_roots: &[PathBuf], root: &Path) {
    report.clone_groups.retain(|group| {
        group.instances.iter().any(|instance| {
            workspace_roots
                .iter()
                .any(|workspace_root| instance.file.starts_with(workspace_root))
        })
    });
    rebuild_duplication_derived_fields(report, root);
}

fn apply_top(report: &mut DuplicationReport, n: usize, root: &Path) {
    report.clone_groups.sort_by(|a, b| {
        b.instances
            .len()
            .cmp(&a.instances.len())
            .then(b.line_count.cmp(&a.line_count))
            .then_with(|| match (a.instances.first(), b.instances.first()) {
                (Some(ai), Some(bi)) => ai
                    .file
                    .cmp(&bi.file)
                    .then(ai.start_line.cmp(&bi.start_line)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
    });
    report.clone_groups.truncate(n);
    rebuild_duplication_derived_fields(report, root);
    report.sort();
}

fn rebuild_duplication_derived_fields(report: &mut DuplicationReport, root: &Path) {
    report.clone_families =
        fallow_engine::duplicates::families::group_into_families(&report.clone_groups, root);
    report.mirrored_directories = fallow_engine::duplicates::families::detect_mirrored_directories(
        &report.clone_families,
        root,
    );
    report.stats = recompute_stats(report);
}

fn recompute_stats(report: &DuplicationReport) -> DuplicationStats {
    let mut files_with_clones: FxHashSet<&Path> = FxHashSet::default();
    let mut line_ranges: FxHashMap<&Path, Vec<(usize, usize)>> = FxHashMap::default();
    let mut clone_instances = 0_usize;
    let mut duplicated_tokens = 0_usize;
    for group in &report.clone_groups {
        duplicated_tokens += group.token_count * group.instances.len();
        for instance in &group.instances {
            files_with_clones.insert(&instance.file);
            clone_instances += 1;
            line_ranges
                .entry(&instance.file)
                .or_default()
                .push((instance.start_line, instance.end_line));
        }
    }
    let duplicated_lines = line_ranges
        .into_values()
        .map(count_merged_lines)
        .sum::<usize>();
    let duplication_percentage = if report.stats.total_lines == 0 {
        0.0
    } else {
        (duplicated_lines as f64 / report.stats.total_lines as f64) * 100.0
    };
    DuplicationStats {
        total_files: report.stats.total_files,
        files_with_clones: files_with_clones.len(),
        total_lines: report.stats.total_lines,
        duplicated_lines,
        total_tokens: report.stats.total_tokens,
        duplicated_tokens,
        clone_groups: report.clone_groups.len(),
        clone_instances,
        duplication_percentage,
        clone_groups_below_min_occurrences: report.stats.clone_groups_below_min_occurrences,
    }
}

fn count_merged_lines(mut ranges: Vec<(usize, usize)>) -> usize {
    if ranges.is_empty() {
        return 0;
    }
    ranges.sort_unstable();
    let mut total = 0_usize;
    let mut current = ranges[0];
    for (start, end) in ranges.into_iter().skip(1) {
        if start <= current.1.saturating_add(1) {
            current.1 = current.1.max(end);
        } else {
            total += current.1.saturating_sub(current.0).saturating_add(1);
            current = (start, end);
        }
    }
    total + current.1.saturating_sub(current.0).saturating_add(1)
}

const fn root_envelope_mode(legacy_envelope: bool) -> RootEnvelopeMode {
    RootEnvelopeMode::from_legacy(legacy_envelope)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct FakeHealthRunner {
        root: PathBuf,
        telemetry_analysis_run_id: Option<String>,
    }

    impl ProgrammaticHealthRunner for FakeHealthRunner {
        fn run_programmatic_health(
            &self,
            _options: &ComplexityOptions,
        ) -> Result<ProgrammaticHealthRun, ProgrammaticError> {
            let project_config = fallow_engine::config_for_project_analysis(
                &self.root,
                None,
                ProjectConfigOptions {
                    output: OutputFormat::Json,
                    no_cache: true,
                    threads: 1,
                    production_override: None,
                    quiet: true,
                    analysis: ProductionAnalysis::Health,
                },
            )
            .expect("test config loads");

            Ok(ProgrammaticHealthRun {
                analysis: fallow_engine::HealthAnalysisResult {
                    report: HealthReport::default(),
                    grouping: None,
                    group_resolver: None,
                    config: project_config.config,
                    elapsed: std::time::Duration::ZERO,
                    timings: None,
                    coverage_gaps_has_findings: false,
                    should_fail_on_coverage_gaps: false,
                },
                workspace_diagnostics: vec![WorkspaceDiagnosticOutput(serde_json::json!({
                    "path": self.root.join("package.json")
                }))],
                next_step_facts: fallow_engine::ProgrammaticHealthNextStepFacts {
                    suggestions_enabled: true,
                    offer_setup: false,
                    impact_digest: Some(fallow_output::ImpactDigestCounts {
                        containment_count: 1,
                        resolved_total: 0,
                    }),
                    audit_changed: false,
                },
                telemetry_analysis_run_id: self.telemetry_analysis_run_id.clone(),
            })
        }
    }

    fn analysis_at(root: &Path) -> AnalysisOptions {
        AnalysisOptions {
            root: Some(root.to_path_buf()),
            ..AnalysisOptions::default()
        }
    }

    #[test]
    fn serialize_health_report_json_tags_meta_and_strips_paths() {
        let root = Path::new("/repo");
        let json = serialize_health_report_json(HealthJsonReportInput {
            report: HealthReport::default(),
            root,
            elapsed: std::time::Duration::ZERO,
            explain: true,
            grouped_by: None,
            groups: None,
            workspace_diagnostics: vec![WorkspaceDiagnosticOutput(serde_json::json!({
                "path": "/repo/package.json"
            }))],
            next_steps: vec![NextStep {
                id: "inspect-health".to_string(),
                command: "fallow health --format json".to_string(),
                reason: "inspect health details".to_string(),
            }],
            envelope_mode: RootEnvelopeMode::Tagged,
            telemetry_analysis_run_id: Some("run-api-health"),
        })
        .expect("health JSON serializes");

        assert_eq!(json["kind"], "health");
        assert_eq!(json["schema_version"], HEALTH_SCHEMA_VERSION);
        assert!(json["_meta"].is_object());
        assert_eq!(
            json["_meta"]["telemetry"]["analysis_run_id"],
            "run-api-health"
        );
        assert_eq!(json["workspace_diagnostics"][0]["path"], "package.json");
        assert_eq!(json["next_steps"][0]["id"], "inspect-health");
    }

    #[test]
    fn serialize_health_report_json_respects_legacy_envelope() {
        let json = serialize_health_report_json(HealthJsonReportInput {
            report: HealthReport::default(),
            root: Path::new("/repo"),
            elapsed: std::time::Duration::ZERO,
            explain: false,
            grouped_by: None,
            groups: None,
            workspace_diagnostics: Vec::new(),
            next_steps: Vec::new(),
            envelope_mode: RootEnvelopeMode::Legacy,
            telemetry_analysis_run_id: None,
        })
        .expect("health JSON serializes");

        assert!(json.get("kind").is_none());
    }

    #[test]
    fn programmatic_health_runner_serializes_api_owned_output() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path().to_path_buf();
        let json = compute_health_with_runner(
            &ComplexityOptions {
                analysis: AnalysisOptions {
                    explain: true,
                    ..AnalysisOptions::default()
                },
                ..ComplexityOptions::default()
            },
            &FakeHealthRunner {
                root,
                telemetry_analysis_run_id: Some("run-123".to_string()),
            },
        )
        .expect("programmatic health should serialize");

        assert_eq!(json["kind"], "health");
        assert_eq!(json["workspace_diagnostics"][0]["path"], "package.json");
        assert_eq!(json["next_steps"][0]["id"], "impact-report");
        assert_eq!(
            json["_meta"]["telemetry"]["analysis_run_id"],
            serde_json::Value::from("run-123")
        );
    }

    #[test]
    fn detect_duplication_returns_dupes_envelope() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("src dir");
        let code = "export function repeated() {\n  return ['a', 'b', 'c'].join(',');\n}\n";
        std::fs::write(root.join("src/a.ts"), code).expect("file");
        std::fs::write(root.join("src/b.ts"), code).expect("file");

        let json = detect_duplication(&DuplicationOptions {
            analysis: analysis_at(root),
            min_tokens: 1,
            min_lines: 1,
            ..DuplicationOptions::default()
        })
        .expect("duplication succeeds");

        assert_eq!(json["kind"], "dupes");
        assert!(json["clone_groups"].is_array());
        assert!(json["stats"].is_object());
    }

    #[test]
    fn run_duplication_returns_typed_output_before_json() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("src dir");
        std::fs::write(root.join("src/a.ts"), "export const a = 1;\n").expect("file");

        let run = run_duplication(&DuplicationOptions {
            analysis: analysis_at(root),
            ..DuplicationOptions::default()
        })
        .expect("duplication succeeds");

        assert_eq!(run.output.schema_version.0, SCHEMA_VERSION);
        assert_eq!(run.root, root);
        assert_eq!(run.envelope_mode, RootEnvelopeMode::Tagged);

        let json = run
            .into_json()
            .expect("typed duplication output serializes");
        assert_eq!(json["kind"], "dupes");
    }

    #[test]
    fn detect_duplication_legacy_envelope_removes_root_kind() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("src dir");
        std::fs::write(root.join("src/a.ts"), "export const a = 1;\n").expect("file");

        let json = detect_duplication(&DuplicationOptions {
            analysis: AnalysisOptions {
                legacy_envelope: true,
                ..analysis_at(root)
            },
            ..DuplicationOptions::default()
        })
        .expect("duplication succeeds");

        assert!(json.get("kind").is_none());
    }

    #[test]
    fn detect_dead_code_returns_dead_code_envelope() {
        let project = dead_code_project();
        let root = project.path();

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: analysis_at(root),
            filters: DeadCodeFilters {
                unused_exports: true,
                ..DeadCodeFilters::default()
            },
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        assert_eq!(json["kind"], "dead-code");
        assert_eq!(json["schema_version"], CHECK_SCHEMA_VERSION);
        assert_eq!(unused_export_names(&json), vec!["deadA", "deadB"]);
    }

    #[test]
    fn run_dead_code_returns_typed_output_before_json() {
        let project = dead_code_project();
        let root = project.path();

        let run = run_dead_code(&DeadCodeOptions {
            analysis: analysis_at(root),
            filters: DeadCodeFilters {
                unused_exports: true,
                ..DeadCodeFilters::default()
            },
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        assert_eq!(run.output.schema_version.0, CHECK_SCHEMA_VERSION);
        assert_eq!(run.output.results.unused_exports.len(), 2);
        assert_eq!(run.root, root);
        assert_eq!(run.envelope_mode, RootEnvelopeMode::Tagged);

        let json = run.into_json().expect("typed dead-code output serializes");
        assert_eq!(unused_export_names(&json), vec!["deadA", "deadB"]);
    }

    #[test]
    fn run_dead_code_family_helpers_return_typed_filtered_output() {
        let project = dead_code_project();
        let root = project.path();
        let options = DeadCodeOptions {
            analysis: analysis_at(root),
            ..DeadCodeOptions::default()
        };

        let circular = run_circular_dependencies(&options).expect("circular helper");
        let boundary = run_boundary_violations(&options).expect("boundary helper");

        assert!(circular.output.results.unused_exports.is_empty());
        assert!(boundary.output.results.unused_exports.is_empty());
        assert_eq!(circular.output.total_issues, 0);
        assert_eq!(boundary.output.total_issues, 0);
    }

    #[test]
    fn detect_dead_code_legacy_envelope_removes_root_kind() {
        let project = dead_code_project();
        let root = project.path();

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: AnalysisOptions {
                legacy_envelope: true,
                ..analysis_at(root)
            },
            filters: DeadCodeFilters {
                unused_exports: true,
                ..DeadCodeFilters::default()
            },
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        assert!(json.get("kind").is_none());
    }

    #[test]
    fn detect_dead_code_explain_includes_output_owned_meta() {
        let project = dead_code_project();
        let root = project.path();

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: AnalysisOptions {
                explain: true,
                ..analysis_at(root)
            },
            filters: DeadCodeFilters {
                unused_exports: true,
                ..DeadCodeFilters::default()
            },
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        assert_eq!(json["kind"], "dead-code");
        assert_eq!(
            json["_meta"]["docs"].as_str(),
            Some(fallow_output::CHECK_DOCS)
        );
        assert!(json["_meta"]["rules"]["unused-export"].is_object());
    }

    #[test]
    fn detect_dead_code_marks_duplicate_export_config_action_fixable() {
        let project = duplicate_export_project();
        let root = project.path();

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: analysis_at(root),
            filters: DeadCodeFilters {
                duplicate_exports: true,
                ..DeadCodeFilters::default()
            },
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        let action = &json["duplicate_exports"][0]["actions"][0];
        assert_eq!(action["type"], "add-to-config");
        assert_eq!(action["auto_fixable"], true);
    }

    #[test]
    fn detect_dead_code_keeps_duplicate_export_config_action_blocked_in_subpackage() {
        let workspace = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            workspace.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\n",
        )
        .expect("workspace");
        let root = workspace.path().join("packages/app");
        duplicate_export_project_at(&root);

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: analysis_at(&root),
            filters: DeadCodeFilters {
                duplicate_exports: true,
                ..DeadCodeFilters::default()
            },
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        let action = &json["duplicate_exports"][0]["actions"][0];
        assert_eq!(action["type"], "add-to-config");
        assert_eq!(action["auto_fixable"], false);
    }

    #[test]
    fn detect_dead_code_file_filter_scopes_source_findings() {
        let project = dead_code_project();
        let root = project.path();

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: analysis_at(root),
            filters: DeadCodeFilters {
                unused_exports: true,
                ..DeadCodeFilters::default()
            },
            files: vec![PathBuf::from("src/a.ts")],
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        assert_eq!(unused_export_names(&json), vec!["deadA"]);
    }

    #[test]
    fn detect_dead_code_diff_file_filters_source_findings() {
        let project = dead_code_project();
        let root = project.path();
        std::fs::write(
            root.join("a.diff"),
            "diff --git a/src/a.ts b/src/a.ts\n+++ b/src/a.ts\n@@ -1 +1 @@\n+export const deadA = 1;\n",
        )
        .expect("diff");

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: AnalysisOptions {
                diff_file: Some(PathBuf::from("a.diff")),
                ..analysis_at(root)
            },
            filters: DeadCodeFilters {
                unused_exports: true,
                ..DeadCodeFilters::default()
            },
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        assert_eq!(unused_export_names(&json), vec!["deadA"]);
    }

    #[test]
    fn detect_circular_dependencies_keeps_dead_code_envelope_but_filters_other_findings() {
        let project = dead_code_project();
        let root = project.path();

        let json = detect_circular_dependencies(&DeadCodeOptions {
            analysis: analysis_at(root),
            ..DeadCodeOptions::default()
        })
        .expect("circular helper succeeds");

        assert_eq!(json["kind"], "dead-code");
        assert_eq!(json["total_issues"], 0);
        assert!(json["circular_dependencies"].as_array().is_some());
        assert!(json["unused_exports"].as_array().is_none_or(Vec::is_empty));
    }

    #[test]
    fn detect_boundary_violations_keeps_only_boundary_family() {
        let project = dead_code_project();
        let root = project.path();

        let json = detect_boundary_violations(&DeadCodeOptions {
            analysis: analysis_at(root),
            ..DeadCodeOptions::default()
        })
        .expect("boundary helper succeeds");

        assert_eq!(json["kind"], "dead-code");
        assert_eq!(json["total_issues"], 0);
        assert!(json["boundary_violations"].as_array().is_some());
        assert!(json["unused_exports"].as_array().is_none_or(Vec::is_empty));
    }

    #[test]
    fn diff_file_filters_clone_groups() {
        let root = PathBuf::from("/repo");
        let mut report = DuplicationReport {
            clone_groups: vec![
                group(vec![
                    instance("/repo/src/a.ts", 1, 3),
                    instance("/repo/src/b.ts", 1, 3),
                ]),
                group(vec![
                    instance("/repo/src/c.ts", 10, 12),
                    instance("/repo/src/d.ts", 1, 3),
                ]),
            ],
            stats: DuplicationStats {
                total_files: 4,
                total_lines: 100,
                total_tokens: 100,
                clone_groups: 2,
                clone_instances: 4,
                ..DuplicationStats::default()
            },
            ..DuplicationReport::default()
        };
        let diff = DiffIndex::from_unified_diff(
            "diff --git a/src/a.ts b/src/a.ts\n+++ b/src/a.ts\n@@ -1,3 +1,3 @@\n+added\n context\n",
        );

        filter_by_diff(&mut report, &diff, &root);

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(
            report.clone_groups[0].instances[0].file,
            root.join("src/a.ts")
        );
    }

    #[test]
    fn workspace_scope_filters_clone_groups() {
        let root = PathBuf::from("/repo");
        let mut report = DuplicationReport {
            clone_groups: vec![
                group(vec![
                    instance("/repo/packages/app/a.ts", 1, 3),
                    instance("/repo/packages/shared/b.ts", 1, 3),
                ]),
                group(vec![
                    instance("/repo/packages/docs/c.ts", 1, 3),
                    instance("/repo/packages/docs/d.ts", 1, 3),
                ]),
            ],
            stats: DuplicationStats {
                total_files: 4,
                total_lines: 100,
                total_tokens: 100,
                clone_groups: 2,
                clone_instances: 4,
                ..DuplicationStats::default()
            },
            ..DuplicationReport::default()
        };

        filter_by_workspaces(&mut report, &[root.join("packages/app")], &root);

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(
            report.clone_groups[0].instances[0].file,
            root.join("packages/app/a.ts")
        );
    }

    #[test]
    fn workspace_patterns_match_names_paths_and_negation() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        write_json(
            root.join("package.json"),
            r#"{"workspaces":["packages/*"]}"#,
        );
        write_workspace(root, "packages/app", "@scope/app");
        write_workspace(root, "packages/docs", "docs");

        let roots =
            resolve_workspace_filters(root, &["packages/*".to_string(), "!docs".to_string()])
                .expect("workspace filters resolve");

        assert_eq!(roots, vec![root.join("packages/app")]);
    }

    fn instance(path: &str, start_line: usize, end_line: usize) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(path),
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn group(instances: Vec<CloneInstance>) -> fallow_engine::duplicates::CloneGroup {
        fallow_engine::duplicates::CloneGroup {
            instances,
            token_count: 10,
            line_count: 3,
        }
    }

    fn dead_code_project() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("src dir");
        write_json(
            root.join("package.json"),
            r#"{"name":"api-dead-code","main":"src/index.ts"}"#,
        );
        std::fs::write(
            root.join("src/index.ts"),
            "import './a';\nimport './b';\nexport const entry = 1;\nconsole.log(entry);\n",
        )
        .expect("entry");
        std::fs::write(root.join("src/a.ts"), "export const deadA = 1;\n").expect("a");
        std::fs::write(root.join("src/b.ts"), "export const deadB = 1;\n").expect("b");
        project
    }

    fn duplicate_export_project() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("temp dir");
        duplicate_export_project_at(project.path());
        project
    }

    fn duplicate_export_project_at(root: &Path) {
        std::fs::create_dir_all(root.join("src")).expect("src dir");
        write_json(
            root.join("package.json"),
            r#"{"name":"api-duplicate-export","main":"src/index.ts"}"#,
        );
        std::fs::write(root.join("src/index.ts"), "import './a';\nimport './b';\n").expect("entry");
        std::fs::write(root.join("src/a.ts"), "export const Button = 1;\n").expect("a");
        std::fs::write(root.join("src/b.ts"), "export const Button = 2;\n").expect("b");
    }

    fn unused_export_names(json: &serde_json::Value) -> Vec<&str> {
        json["unused_exports"]
            .as_array()
            .expect("unused exports array")
            .iter()
            .map(|item| {
                item["name"]
                    .as_str()
                    .or_else(|| item["export_name"].as_str())
                    .expect("unused export name")
            })
            .collect()
    }

    fn write_workspace(root: &Path, relative: &str, name: &str) {
        let dir = root.join(relative);
        std::fs::create_dir_all(&dir).expect("workspace dir");
        write_json(dir.join("package.json"), &format!(r#"{{"name":"{name}"}}"#));
    }

    fn write_json(path: PathBuf, json: &str) {
        std::fs::write(path, json).expect("json file");
    }
}
