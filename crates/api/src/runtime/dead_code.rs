use std::path::Path;
use std::time::Instant;

use fallow_config::ProductionAnalysis;
use fallow_engine::{
    dead_code::DeadCodeAnalysisArtifacts,
    project_config::{ProjectConfig, ProjectConfigOptions},
    session::AnalysisSession,
};
use fallow_output::{
    CHECK_SCHEMA_VERSION, CheckOutputInput, DeadCodeNextStepsInput, DiffIndex, build_check_output,
    build_dead_code_next_steps, check_meta, relative_to_diff_path,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::path_util::is_absolute_path_any_platform;
use fallow_types::results::{AnalysisResults, TraceHopRole};
use rustc_hash::FxHashSet;

use crate::{
    AnalysisOptions, BoundaryViolationsProgrammaticOutput, CircularDependenciesProgrammaticOutput,
    DeadCodeFilters, DeadCodeOptions, DeadCodeProgrammaticOutput, ProgrammaticError,
    analysis_context::{
        ProgrammaticAnalysisContext, changed_files_for_run,
        resolve_programmatic_analysis_context_deferred_workspace, workspace_roots_for_session,
    },
    next_steps::{
        default_workspace_ref_for_workspaces, setup_pointer_applicable, suggestions_enabled,
    },
};

use super::{ProgrammaticResult, root_envelope_mode};

pub(super) struct DeadCodeProgrammaticRunWithArtifacts {
    pub output: DeadCodeProgrammaticOutput,
    pub artifacts: DeadCodeAnalysisArtifacts,
}

/// Run dead-code analysis and return typed API output before serialization.
///
/// # Errors
///
/// Returns a structured programmatic error for unsupported options, invalid
/// options, config load failures, analysis failures, or git changed-file
/// failures.
pub fn run_dead_code(options: &DeadCodeOptions) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
    resolved.install(|| run_dead_code_inner(options, &resolved, |_| {}))
}

/// Run circular-dependency analysis and return typed API output before JSON.
///
/// # Errors
///
/// Returns the same structured errors as [`run_dead_code`].
pub fn run_circular_dependencies(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<CircularDependenciesProgrammaticOutput> {
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
    resolved.install(|| {
        run_dead_code_inner(options, &resolved, keep_circular_dependencies).map(Into::into)
    })
}

/// Run boundary-family analysis and return typed API output before JSON.
///
/// # Errors
///
/// Returns the same structured errors as [`run_dead_code`].
pub fn run_boundary_violations(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<BoundaryViolationsProgrammaticOutput> {
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
    resolved.install(|| {
        run_dead_code_inner(options, &resolved, keep_boundary_violations).map(Into::into)
    })
}

fn run_dead_code_inner(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    post_filter: impl FnOnce(&mut AnalysisResults),
) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    let start = Instant::now();
    let session = load_dead_code_session(options, resolved)?;
    run_dead_code_with_session(options, resolved, &session, None, post_filter, start)
}

pub(super) fn run_dead_code_with_session(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    changed_files: Option<&FxHashSet<std::path::PathBuf>>,
    post_filter: impl FnOnce(&mut AnalysisResults),
    start: Instant,
) -> ProgrammaticResult<DeadCodeProgrammaticOutput> {
    let analysis = session.analyze_dead_code().map_err(|err| {
        ProgrammaticError::new(format!("dead-code analysis failed: {err}"), 2)
            .with_code("FALLOW_DEAD_CODE_FAILED")
            .with_context("dead-code")
    })?;
    let mut results = analysis.results;

    apply_dead_code_scope(options, resolved, session, changed_files, &mut results)?;
    apply_dead_code_filters(&options.filters, &mut results);
    post_filter(&mut results);

    Ok(build_dead_code_programmatic_output(
        options, resolved, session, results, start,
    ))
}

pub(super) fn run_dead_code_with_session_artifacts(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    changed_files: Option<&FxHashSet<std::path::PathBuf>>,
    post_filter: impl FnOnce(&mut AnalysisResults),
    start: Instant,
) -> ProgrammaticResult<DeadCodeProgrammaticRunWithArtifacts> {
    let mut artifacts = session
        .analyze_dead_code_with_artifacts(true, true)
        .map_err(|err| {
            ProgrammaticError::new(format!("dead-code analysis failed: {err}"), 2)
                .with_code("FALLOW_DEAD_CODE_FAILED")
                .with_context("dead-code")
        })?;

    apply_dead_code_scope(
        options,
        resolved,
        session,
        changed_files,
        &mut artifacts.results,
    )?;
    apply_dead_code_filters(&options.filters, &mut artifacts.results);
    post_filter(&mut artifacts.results);

    Ok(build_dead_code_run_with_artifacts(
        options, resolved, session, artifacts, start,
    ))
}

pub(super) fn run_dead_code_from_artifacts(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    changed_files: Option<&FxHashSet<std::path::PathBuf>>,
    mut artifacts: DeadCodeAnalysisArtifacts,
    start: Instant,
) -> ProgrammaticResult<DeadCodeProgrammaticRunWithArtifacts> {
    apply_dead_code_scope(
        options,
        resolved,
        session,
        changed_files,
        &mut artifacts.results,
    )?;
    apply_dead_code_filters(&options.filters, &mut artifacts.results);

    Ok(build_dead_code_run_with_artifacts(
        options, resolved, session, artifacts, start,
    ))
}

fn build_dead_code_run_with_artifacts(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    artifacts: DeadCodeAnalysisArtifacts,
    start: Instant,
) -> DeadCodeProgrammaticRunWithArtifacts {
    let output = build_dead_code_programmatic_output(
        options,
        resolved,
        session,
        artifacts.results.clone(),
        start,
    );
    DeadCodeProgrammaticRunWithArtifacts { output, artifacts }
}

fn build_dead_code_programmatic_output(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    results: AnalysisResults,
    start: Instant,
) -> DeadCodeProgrammaticOutput {
    let root = session.root();
    let next_steps = build_dead_code_next_steps(DeadCodeNextStepsInput {
        suggestions_enabled: suggestions_enabled(),
        results: &results,
        root,
        offer_setup: setup_pointer_applicable(root),
        impact_digest: None,
        workspace_ref: default_workspace_ref_for_workspaces(root, session.workspaces()).as_deref(),
        audit_changed: fallow_engine::churn::is_git_repo(root),
    });
    let config_fixable =
        fallow_config::is_config_fixable(&resolved.root, resolved.config_path.as_ref());
    let output = build_check_output(CheckOutputInput {
        schema_version: CHECK_SCHEMA_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        elapsed: start.elapsed(),
        results,
        config_fixable,
        meta: options.analysis.explain.then(check_meta),
        workspace_diagnostics: session.workspace_diagnostics().to_vec(),
        next_steps,
    });
    DeadCodeProgrammaticOutput {
        output,
        root: session.root().to_path_buf(),
        config_fixable,
        envelope_mode: root_envelope_mode(),
        telemetry_analysis_run_id: None,
    }
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

pub(super) fn load_dead_code_session(
    options: &DeadCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<AnalysisSession> {
    let project_config = fallow_engine::project_config::config_for_project_analysis(
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

pub(super) fn default_dead_code_options_for_context(
    resolved: &ProgrammaticAnalysisContext,
) -> DeadCodeOptions {
    DeadCodeOptions {
        analysis: AnalysisOptions {
            root: Some(resolved.root().to_path_buf()),
            config_path: resolved.config_path().clone(),
            no_cache: resolved.no_cache(),
            threads: Some(resolved.threads()),
            production_override: resolved.production_override(),
            changed_since: resolved.changed_since().map(str::to_owned),
            workspace: resolved.workspace().map(<[String]>::to_vec),
            changed_workspaces: resolved.changed_workspaces().map(str::to_owned),
            explain: resolved.explain_enabled(),
            ..AnalysisOptions::default()
        },
        filters: DeadCodeFilters::default(),
        files: Vec::new(),
        include_entry_exports: false,
    }
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
    resolved: &ProgrammaticAnalysisContext,
    session: &AnalysisSession,
    changed_files: Option<&FxHashSet<std::path::PathBuf>>,
    results: &mut AnalysisResults,
) -> ProgrammaticResult<()> {
    let workspace_roots = workspace_roots_for_session(resolved, session.workspaces())?;
    if let Some(workspace_roots) = workspace_roots.as_ref() {
        fallow_engine::dead_code::filter_to_workspaces(results, workspace_roots);
    }
    let resolved_changed_files = if changed_files.is_some() {
        None
    } else {
        changed_files_for_run(resolved)?
    };
    if let Some(changed_files) = changed_files.or(resolved_changed_files.as_ref()) {
        fallow_engine::dead_code::filter_by_changed_files(results, changed_files);
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
                    || (matches!(hop.role, TraceHopRole::SecretSource) && touches_file(&hop.path))
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
    results.dev_dependencies_in_production.clear();
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
