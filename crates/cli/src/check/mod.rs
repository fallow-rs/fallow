use std::process::ExitCode;
use std::time::{Duration, Instant};

use fallow_config::{OutputFormat, ResolvedConfig, RulesConfig, Severity, WorkspaceInfo};
use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::ModuleInfo;
use fallow_types::results::AnalysisResults;

use crate::baseline::{BaselineData, filter_new_issues};
use crate::error::emit_error;
use crate::load_config_for_analysis;
use crate::regression::{self, RegressionOpts, RegressionOutcome};
use crate::report;

#[expect(
    clippy::redundant_pub_crate,
    reason = "reused by crate::security; check is crate-private so pub(crate) is the minimal widening that exposes filtering crate-wide"
)]
pub(crate) mod filtering;
mod output;
mod rules;

pub use filtering::get_changed_files;
pub use filtering::resolve_workspace_scope;

#[derive(Default, Clone)]
pub struct IssueFilters {
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
    pub invalid_client_exports: bool,
    pub mixed_client_server_barrels: bool,
    pub misplaced_directives: bool,
    pub route_collisions: bool,
    pub dynamic_segment_name_conflicts: bool,
}

impl IssueFilters {
    pub fn enable_cli_filter_flag(&mut self, flag: &str) -> bool {
        match flag {
            "--unused-files" => self.unused_files = true,
            "--unused-exports" => self.unused_exports = true,
            "--unused-deps" => self.unused_deps = true,
            "--unused-types" => self.unused_types = true,
            "--private-type-leaks" => self.private_type_leaks = true,
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
            _ => return false,
        }
        true
    }

    pub const fn any_active(&self) -> bool {
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
            || self.invalid_client_exports
            || self.mixed_client_server_barrels
            || self.misplaced_directives
            || self.route_collisions
            || self.dynamic_segment_name_conflicts
    }

    /// Enable off-by-default issue types when explicitly requested as filters.
    pub fn activate_explicit_opt_ins(&self, rules: &mut RulesConfig) {
        if self.private_type_leaks && rules.private_type_leaks == Severity::Off {
            rules.private_type_leaks = Severity::Warn;
        }
    }

    /// When any filter is active, clear issue types that were NOT requested.
    pub fn apply(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.any_active() {
            return;
        }
        self.apply_core_filters(results);
        self.apply_component_filters(results);
        self.apply_graph_filters(results);
        self.apply_policy_filters(results);
        self.apply_catalog_filters(results);
    }

    fn apply_core_filters(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.unused_files {
            results.unused_files.clear();
        }
        if !self.unused_exports {
            results.unused_exports.clear();
        }
        if !self.unused_types {
            results.unused_types.clear();
        }
        if !self.private_type_leaks {
            results.private_type_leaks.clear();
        }
        if !self.unused_deps {
            results.unused_dependencies.clear();
            results.unused_dev_dependencies.clear();
            results.unused_optional_dependencies.clear();
            results.type_only_dependencies.clear();
            results.test_only_dependencies.clear();
            results.dev_dependencies_in_production.clear();
        }
        if !self.unused_enum_members {
            results.unused_enum_members.clear();
        }
        if !self.unused_class_members {
            results.unused_class_members.clear();
        }
        if !self.unused_store_members {
            results.unused_store_members.clear();
        }
        if !self.unlisted_deps {
            results.unlisted_dependencies.clear();
        }
    }

    fn apply_component_filters(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.unprovided_injects {
            results.unprovided_injects.clear();
        }
        if !self.unrendered_components {
            results.unrendered_components.clear();
        }
        if !self.unused_component_props {
            results.unused_component_props.clear();
        }
        if !self.unused_component_emits {
            results.unused_component_emits.clear();
        }
        if !self.unused_component_inputs {
            results.unused_component_inputs.clear();
        }
        if !self.unused_component_outputs {
            results.unused_component_outputs.clear();
        }
        if !self.unused_svelte_events {
            results.unused_svelte_events.clear();
        }
        if !self.unused_server_actions {
            results.unused_server_actions.clear();
        }
        if !self.unused_load_data_keys {
            results.unused_load_data_keys.clear();
        }
        if !self.unresolved_imports {
            results.unresolved_imports.clear();
        }
        if !self.invalid_client_exports {
            results.invalid_client_exports.clear();
        }
        if !self.mixed_client_server_barrels {
            results.mixed_client_server_barrels.clear();
        }
        if !self.misplaced_directives {
            results.misplaced_directives.clear();
        }
        if !self.route_collisions {
            results.route_collisions.clear();
        }
        if !self.dynamic_segment_name_conflicts {
            results.dynamic_segment_name_conflicts.clear();
        }
    }

    fn apply_graph_filters(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.duplicate_exports {
            results.duplicate_exports.clear();
        }
        if !self.circular_deps {
            results.circular_dependencies.clear();
        }
        if !self.re_export_cycles {
            results.re_export_cycles.clear();
        }
        if !self.boundary_violations {
            results.boundary_violations.clear();
            results.boundary_coverage_violations.clear();
            results.boundary_call_violations.clear();
        }
    }

    fn apply_policy_filters(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.policy_violations {
            results.policy_violations.clear();
        }
        if !self.stale_suppressions {
            results.stale_suppressions.clear();
        }
    }

    fn apply_catalog_filters(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.unused_catalog_entries {
            results.unused_catalog_entries.clear();
        }
        if !self.empty_catalog_groups {
            results.empty_catalog_groups.clear();
        }
        if !self.unresolved_catalog_references {
            results.unresolved_catalog_references.clear();
        }
        if !self.unused_dependency_overrides {
            results.unused_dependency_overrides.clear();
        }
        if !self.misconfigured_dependency_overrides {
            results.misconfigured_dependency_overrides.clear();
        }
    }
}

pub struct TraceOptions {
    pub trace_export: Option<String>,
    pub trace_file: Option<String>,
    pub trace_dependency: Option<String>,
    /// Impact closure for a single file as the seed. Powers the
    /// `inspect_target` MCP tool's `impact_closure` evidence section.
    pub impact_closure: Option<String>,
    /// Exact symbol-level impact query, including targeted tests when the
    /// type-aware backend can prove the relation.
    pub symbol_impact: Option<String>,
    pub performance: bool,
}

impl TraceOptions {
    pub const fn any_active(&self) -> bool {
        self.trace_export.is_some()
            || self.trace_file.is_some()
            || self.trace_dependency.is_some()
            || self.impact_closure.is_some()
            || self.symbol_impact.is_some()
            || self.performance
    }
}

pub struct CheckOptions<'a> {
    pub root: &'a std::path::Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub allow_remote_extends: bool,
    pub fail_on_issues: bool,
    pub filters: &'a IssueFilters,
    pub changed_since: Option<&'a str>,
    pub diff_index: Option<&'a crate::report::ci::diff_filter::DiffIndex>,
    pub use_shared_diff_index: bool,
    pub baseline: Option<&'a std::path::Path>,
    pub save_baseline: Option<&'a std::path::Path>,
    pub sarif_file: Option<&'a std::path::Path>,
    pub production: bool,
    pub production_override: Option<bool>,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub group_by: Option<crate::GroupBy>,
    pub include_dupes: bool,
    /// CLI override for the opt-in TypeScript semantic refinement:
    /// `Some(true)` for `--type-aware`, `Some(false)` for `--no-type-aware`,
    /// `None` when neither flag was passed (environment and config decide).
    pub type_aware: Option<bool>,
    /// Command-scoped config override (for example `audit.typeAware`), applied
    /// below the CLI flags and the `FALLOW_TYPE_AWARE` environment variable but
    /// above the top-level `typeAware.enabled` opt-in.
    pub type_aware_config_override: Option<bool>,
    /// Explicit TypeScript project configs used by the semantic refinement.
    pub type_aware_projects: &'a [std::path::PathBuf],
    /// CLI completeness override. Config and environment are resolved later.
    pub type_aware_require: Option<fallow_config::TypeAwareRequire>,
    pub trace_opts: &'a TraceOptions,
    pub explain: bool,
    pub top: Option<usize>,
    /// Only report issues in these file(s). Empty means no file filter.
    pub file: &'a [std::path::PathBuf],
    /// Report unused exports in entry files instead of auto-marking them as used.
    pub include_entry_exports: bool,
    /// When true, emit a condensed summary instead of full item-level output.
    /// Consumed by combined mode only; standalone check ignores this flag.
    pub summary: bool,
    pub regression_opts: RegressionOpts<'a>,
    /// When true, retain parsed modules and discovered files for sharing with health.
    pub retain_modules_for_health: bool,
    /// When true, return timings without printing them so combined mode can add
    /// later stages before rendering the table.
    pub defer_performance: bool,
    /// Which revision this pass analyzes. `Base` marks the isolated
    /// `audit --base` pass so revision-specific diagnostics name the base
    /// revision instead of reading as a current-configuration defect.
    pub analysis_snapshot: fallow_config::AnalysisSnapshot,
}

/// Result of executing check analysis without printing.
pub struct CheckResult {
    pub results: AnalysisResults,
    pub config: ResolvedConfig,
    pub config_fixable: bool,
    pub elapsed: Duration,
    pub fail_on_issues: bool,
    pub regression: Option<RegressionOutcome>,
    pub baseline_deltas: Option<crate::baseline::BaselineDeltas>,
    /// When a baseline was loaded: (total entries in baseline, entries that matched current issues).
    pub baseline_matched: Option<(usize, usize)>,
    pub timings: Option<fallow_types::trace::PipelineTimings>,
    /// Retained parse data for sharing with health (only populated when retain_modules_for_health=true).
    pub shared_parse: Option<fallow_engine::health::HealthSharedParseData>,
    /// Provenance for the opt-in TypeScript semantic analysis pass.
    pub type_aware_meta: Option<fallow_types::envelope::TypeAwareMeta>,
    /// Advisory coupling report produced in the same semantic batch when a
    /// combined run also requested health analysis.
    pub type_coupling: Option<fallow_types::semantic::TypeCouplingReport>,
    /// Bounded non-fatal diagnostics from the semantic backend.
    pub type_aware_warnings: Vec<String>,
    /// Pre-refinement dead-code audit keys captured immediately before the
    /// type-aware pass mutated `results`. `None` when type-aware analysis was
    /// not enabled. The audit gate uses this identity-independent set to fall
    /// back to syntactic attribution when base and head semantic identities
    /// cannot be compared.
    pub syntactic_dead_code_keys: Option<rustc_hash::FxHashSet<String>>,
    /// Impact closure for the review brief: the transitive
    /// affected-but-not-in-diff set plus coordination gaps. Populated by the
    /// audit brief path from the retained graph against the changed-file set;
    /// `None` outside the brief path. Holds root-relative paths so it survives
    /// the graph drop and serializes directly.
    pub impact_closure: Option<fallow_engine::module_graph::ImpactClosurePaths>,
    /// Exports-aware public-export key set for the review brief: the
    /// `<rel_path>::<name>` keys reachable through `package.json` `exports` +
    /// re-export reachability. Computed from the retained graph on the brief
    /// path before the graph is dropped; `None` outside the brief path. Diffed
    /// against the base snapshot's `public_api` set to produce the public-API
    /// surface delta.
    pub public_api_keys: Option<rustc_hash::FxHashSet<String>>,
    /// Partition + order for the review brief's stage 2: the by-module
    /// units the changed files cluster into, plus a dependency-sensible review
    /// order. Computed from the retained graph on the brief path against the
    /// changed-file set, before the graph is dropped; `None` outside the brief
    /// path. Holds root-relative paths so it survives the graph drop and
    /// serializes directly.
    pub partition_order: Option<fallow_engine::module_graph::PartitionOrderPaths>,
    /// Per-changed-file graph facts for the review brief's stage 4 weighted
    /// focus map: fan-in/out (blast radius) plus the dynamic-dispatch and
    /// re-export-indirection confidence-flag signals. Computed from the retained
    /// graph on the brief path against the changed-file set, before the graph is
    /// dropped; `None` outside the brief path. Holds root-relative paths so it
    /// survives the graph drop.
    pub focus_facts: Option<Vec<fallow_engine::module_graph::FocusFileFactsPaths>>,
    /// Per-changed-file `rel_path -> [(exported-symbol, 1-based declaration line)]`
    /// map for the decision surface, so a coordination / public-API decision can
    /// anchor an inline comment to the exact export line. Computed from the
    /// retained graph on the brief path BEFORE the graph is dropped; `None`
    /// otherwise. Internal (CheckResult is not serialized).
    pub export_lines: Option<rustc_hash::FxHashMap<String, Vec<(String, u32)>>>,
    /// Per-anchor `rel_path -> count of in-repo modules OUTSIDE the diff that
    /// directly import it`, for the decision surface's honest per-decision consumer
    /// number. Computed from the retained graph's reverse-deps on the brief path
    /// BEFORE the graph is dropped; `None` otherwise. Internal (not serialized).
    pub internal_consumers: Option<rustc_hash::FxHashMap<String, u64>>,
    pub workspaces: Vec<WorkspaceInfo>,
    retained_files: Option<Vec<DiscoveredFile>>,
}

struct CheckAnalysisData {
    results: AnalysisResults,
    trace_graph: Option<fallow_engine::module_graph::RetainedModuleGraph>,
    trace_timings: Option<fallow_types::trace::PipelineTimings>,
    retained_modules: Option<Vec<ModuleInfo>>,
    retained_files: Option<Vec<DiscoveredFile>>,
    workspaces: Vec<WorkspaceInfo>,
    script_used_packages: rustc_hash::FxHashSet<String>,
}

fn check_data_from_artifacts(
    output: fallow_engine::dead_code::DeadCodeAnalysisArtifacts,
    workspaces: &[WorkspaceInfo],
) -> CheckAnalysisData {
    CheckAnalysisData {
        results: output.results,
        trace_graph: output.graph,
        trace_timings: output.timings,
        retained_modules: output.modules,
        retained_files: output.files,
        workspaces: workspaces.to_vec(),
        script_used_packages: output.script_used_packages,
    }
}

fn check_data_from_plain_artifacts(
    output: fallow_engine::dead_code::DeadCodeAnalysisArtifacts,
    workspaces: &[WorkspaceInfo],
) -> CheckAnalysisData {
    CheckAnalysisData {
        results: output.results,
        trace_graph: None,
        trace_timings: None,
        retained_modules: None,
        retained_files: None,
        workspaces: workspaces.to_vec(),
        script_used_packages: output.script_used_packages,
    }
}

fn run_check_analysis(
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
) -> Result<CheckAnalysisData, ExitCode> {
    let session = fallow_engine::session::AnalysisSession::from_resolved_config(config.clone())
        .map_err(|e| emit_error(&format!("Analysis error: {e}"), 2, opts.output))?;

    if opts.retain_modules_for_health {
        return session
            .analyze_dead_code_with_artifacts(true, true)
            .map(|output| check_data_from_artifacts(output, session.workspaces()))
            .map_err(|e| emit_error(&format!("Analysis error: {e}"), 2, opts.output));
    }

    if opts.include_dupes {
        return session
            .analyze_dead_code_retaining_files(false, opts.trace_opts.any_active())
            .map(|mut output| {
                output.modules = None;
                check_data_from_artifacts(output, session.workspaces())
            })
            .map_err(|e| emit_error(&format!("Analysis error: {e}"), 2, opts.output));
    }

    if opts.trace_opts.any_active() || config.type_aware.enabled {
        return session
            .analyze_dead_code_with_artifacts(false, true)
            .map(|mut output| {
                output.modules = None;
                output.files = None;
                check_data_from_artifacts(output, session.workspaces())
            })
            .map_err(|e| emit_error(&format!("Analysis error: {e}"), 2, opts.output));
    }

    session
        .analyze_dead_code_with_artifacts(false, false)
        .map(|output| check_data_from_plain_artifacts(output, session.workspaces()))
        .map_err(|e| emit_error(&format!("Analysis error: {e}"), 2, opts.output))
}

fn prepare_check_config(opts: &CheckOptions<'_>) -> Result<ResolvedConfig, ExitCode> {
    let mut config = load_config_for_analysis(
        opts.root,
        opts.config_path,
        crate::ConfigLoadOptions {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production_override: opts
                .production_override
                .or_else(|| opts.production.then_some(true)),
            quiet: opts.quiet,
            allow_remote_extends: opts.allow_remote_extends,
        },
        fallow_config::ProductionAnalysis::DeadCode,
    )?;
    config.analysis_snapshot = opts.analysis_snapshot;
    if opts.include_entry_exports {
        config.include_entry_exports = true;
    }
    apply_type_aware_overrides(opts, &mut config)?;
    opts.filters.activate_explicit_opt_ins(&mut config.rules);
    apply_type_aware_private_leak_default(&mut config, opts.filters.any_active());
    Ok(config)
}

/// Default the opt-in `private-type-leaks` rule to `warn` for type-aware runs,
/// where semantic confirmation makes it trustworthy. An explicit user setting
/// always wins: `"private-type-leaks": "off"` stays off (issue #2170).
fn apply_type_aware_private_leak_default(config: &mut ResolvedConfig, filters_active: bool) {
    if config.type_aware.enabled
        && !filters_active
        && config.rules.private_type_leaks == Severity::Off
        && !config.rules.private_type_leaks_configured
    {
        config.rules.private_type_leaks = Severity::Warn;
    }
}

fn apply_type_aware_overrides(
    opts: &CheckOptions<'_>,
    config: &mut ResolvedConfig,
) -> Result<(), ExitCode> {
    apply_type_aware_overrides_from(
        opts.output,
        opts.type_aware,
        opts.type_aware_config_override,
        opts.type_aware_projects,
        opts.type_aware_require,
        config,
    )
}

pub fn apply_type_aware_overrides_from(
    output: fallow_config::OutputFormat,
    enabled: Option<bool>,
    config_override: Option<bool>,
    projects: &[std::path::PathBuf],
    require: Option<fallow_config::TypeAwareRequire>,
    config: &mut ResolvedConfig,
) -> Result<(), ExitCode> {
    let env_enabled = match std::env::var("FALLOW_TYPE_AWARE") {
        Ok(value) => Some(parse_type_aware_bool(&value, output)?),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(emit_error(
                "FALLOW_TYPE_AWARE must be valid UTF-8",
                2,
                output,
            ));
        }
    };
    // First match wins: CLI flag, environment, command-scoped config override
    // (audit.typeAware), then the top-level typeAware.enabled opt-in.
    config.type_aware.enabled = enabled
        .or(env_enabled)
        .or(config_override)
        .unwrap_or(config.type_aware.enabled);

    if !projects.is_empty() {
        config.type_aware.projects = projects
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
    } else if let Some(paths) = std::env::var_os("FALLOW_TYPE_AWARE_PROJECTS") {
        config.type_aware.projects = std::env::split_paths(&paths)
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
    }

    config.type_aware.require = if let Some(require) = require {
        require
    } else if let Ok(value) = std::env::var("FALLOW_TYPE_AWARE_REQUIRE") {
        parse_type_aware_require(&value, output)?
    } else {
        config.type_aware.require
    };
    Ok(())
}

fn parse_type_aware_bool(value: &str, output: OutputFormat) -> Result<bool, ExitCode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(emit_error(
            "FALLOW_TYPE_AWARE must be one of true, false, 1, 0, yes, no, on, or off",
            2,
            output,
        )),
    }
}

fn parse_type_aware_require(
    value: &str,
    output: OutputFormat,
) -> Result<fallow_config::TypeAwareRequire, ExitCode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "best-effort" => Ok(fallow_config::TypeAwareRequire::BestEffort),
        "complete" => Ok(fallow_config::TypeAwareRequire::Complete),
        _ => Err(emit_error(
            "FALLOW_TYPE_AWARE_REQUIRE must be best-effort or complete",
            2,
            output,
        )),
    }
}

fn handle_trace_side_effects(
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
    trace_graph: Option<&fallow_engine::module_graph::RetainedModuleGraph>,
    trace_timings: Option<&fallow_types::trace::PipelineTimings>,
    script_used_packages: &rustc_hash::FxHashSet<String>,
) -> Result<(), ExitCode> {
    if let Some(timings) = trace_timings
        && opts.trace_opts.performance
        && !opts.defer_performance
    {
        report::print_performance(timings, config.output, opts.json_style);
    }
    if let Some(graph) = trace_graph {
        crate::telemetry::note_graph_structure(graph);
        if let Some(code) = output::handle_type_aware_trace_output(
            graph,
            opts.trace_opts,
            config,
            opts.explain,
            opts.json_style,
        ) {
            return Err(code);
        }
        if let Some(code) = output::handle_trace_output(
            graph,
            opts.trace_opts,
            &config.root,
            config.output,
            opts.json_style,
            script_used_packages,
        ) {
            return Err(code);
        }
    }
    Ok(())
}

fn apply_scope_filters(
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
    results: &mut AnalysisResults,
    ws_roots: Option<&Vec<std::path::PathBuf>>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) {
    if let Some(ws_roots) = ws_roots {
        filtering::filter_to_workspaces(results, ws_roots);
    }
    if let Some(changed) = changed_files {
        filtering::filter_changed_files(results, changed);
    }
    let diff_index = match opts.diff_index {
        Some(index) => Some(index),
        None if opts.use_shared_diff_index => crate::report::ci::diff_filter::shared_diff_index(),
        None => None,
    };
    if let Some(diff_index) = diff_index {
        filtering::filter_results_by_diff(results, diff_index, opts.root);
    }

    // Scope filters prune the owner list of a multi-owner finding in place
    // (`duplicate_exports` narrows `locations`), so a group the engine kept
    // because one owner was outside `ignoreFindings` can end up holding only
    // ignored owners. Re-apply the ignore filter over the scoped result so the
    // "hidden only when every owner matches" contract holds for what is actually
    // reported. The helper returns immediately when no patterns are configured.
    fallow_engine::dead_code::filter_configured_ignored_findings(results, config);
}

fn apply_rules_and_filters(
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
    results: &mut AnalysisResults,
) {
    rules::apply_rules(results, config);
    if opts.fail_on_issues {
        rules::promote_policy_finding_warns(results);
    }
    opts.filters.apply(results);
}

fn apply_file_filter(opts: &CheckOptions<'_>, results: &mut AnalysisResults) {
    if opts.file.is_empty() {
        return;
    }
    let file_set: rustc_hash::FxHashSet<std::path::PathBuf> = opts
        .file
        .iter()
        .map(|path| {
            if crate::path_util::is_absolute_path_any_platform(path) {
                path.clone()
            } else {
                opts.root.join(path)
            }
        })
        .collect();
    for (original, resolved) in opts.file.iter().zip(file_set.iter()) {
        if !resolved.exists() {
            eprintln!(
                "Warning: --file '{}' (resolved to '{}') was not found in the project",
                original.display(),
                resolved.display()
            );
        }
    }
    filtering::filter_changed_files(results, &file_set);
    results.unused_dependencies.clear();
    results.unused_dev_dependencies.clear();
    results.unused_optional_dependencies.clear();
    results.type_only_dependencies.clear();
    results.test_only_dependencies.clear();
    results.dev_dependencies_in_production.clear();
}

fn warn_scoped_regression_save(opts: &CheckOptions<'_>) {
    if matches!(
        opts.regression_opts.save_target,
        regression::SaveRegressionTarget::None
    ) || !opts.regression_opts.scoped
    {
        return;
    }
    eprintln!(
        "Warning: saving regression baseline with --changed-since, --workspace, or \
         --changed-workspaces active. The baseline will reflect only scoped results, \
         not the full project."
    );
}

fn save_check_regression_baseline(
    opts: &CheckOptions<'_>,
    results: &AnalysisResults,
    analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity,
) -> Result<Option<regression::CheckCounts>, ExitCode> {
    let counts = match opts.regression_opts.save_target {
        regression::SaveRegressionTarget::None => return Ok(None),
        regression::SaveRegressionTarget::File(save_path) => {
            let counts = regression::CheckCounts::from_results(results);
            regression::save_regression_baseline_with_identity(
                save_path,
                opts.root,
                Some(&counts),
                None,
                opts.output,
                analysis_identity,
            )?;
            counts
        }
        regression::SaveRegressionTarget::Config => {
            let counts = regression::CheckCounts::from_results(results);
            let config_path = regression_config_path(opts);
            regression::save_baseline_to_config_with_identity(
                &config_path,
                &counts,
                opts.output,
                analysis_identity,
            )?;
            counts
        }
    };
    Ok(Some(counts))
}

fn regression_config_path(opts: &CheckOptions<'_>) -> std::path::PathBuf {
    opts.config_path.as_ref().map_or_else(
        || {
            fallow_config::FallowConfig::find_config_path(opts.root)
                .unwrap_or_else(|| opts.root.join(".fallowrc.json"))
        },
        Clone::clone,
    )
}

fn build_shared_parse_data(
    results: &AnalysisResults,
    trace_graph: Option<fallow_engine::module_graph::RetainedModuleGraph>,
    retained_modules: Option<Vec<ModuleInfo>>,
    retained_files: Option<Vec<DiscoveredFile>>,
    workspaces: Vec<WorkspaceInfo>,
    script_used_packages: &rustc_hash::FxHashSet<String>,
) -> Option<fallow_engine::health::HealthSharedParseData> {
    fallow_engine::health::shared_parse_data_from_artifacts(
        results,
        trace_graph,
        retained_modules,
        retained_files,
        workspaces,
        script_used_packages.iter().cloned(),
    )
}

/// Warn on a scoped regression save, persist any configured regression
/// baseline, then compare the current counts against the effective baseline
/// (a just-saved baseline wins over the config baseline).
fn resolve_check_regression(
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
    results: &AnalysisResults,
    analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity,
) -> Result<Option<RegressionOutcome>, ExitCode> {
    warn_scoped_regression_save(opts);

    let just_saved_baseline = save_check_regression_baseline(opts, results, analysis_identity)?;

    let config_baseline_ref = just_saved_baseline.as_ref().map(|counts| {
        let mut baseline = counts.to_config_baseline();
        baseline.analysis_identity = analysis_identity.clone();
        baseline
    });
    let config_baseline = config_baseline_ref
        .as_ref()
        .or_else(|| config.regression.as_ref().and_then(|r| r.baseline.as_ref()));
    regression::compare_check_regression_with_identity(
        results,
        &opts.regression_opts,
        config_baseline,
        analysis_identity,
    )
}

struct CheckCompletionInput<'a> {
    opts: &'a CheckOptions<'a>,
    config: ResolvedConfig,
    data: CheckAnalysisData,
    elapsed: Duration,
    regression_outcome: Option<RegressionOutcome>,
    baseline_matched: Option<(usize, usize)>,
    type_aware: Option<fallow_api::TypeAwareOutcome>,
    type_coupling: Option<fallow_types::semantic::TypeCouplingReport>,
    syntactic_dead_code_keys: Option<rustc_hash::FxHashSet<String>>,
}

fn complete_check_execution(input: CheckCompletionInput<'_>) -> CheckResult {
    let CheckCompletionInput {
        opts,
        config,
        data,
        elapsed,
        regression_outcome,
        baseline_matched,
        type_aware,
        type_coupling,
        syntactic_dead_code_keys,
    } = input;
    let CheckAnalysisData {
        results,
        trace_graph,
        trace_timings,
        retained_modules,
        mut retained_files,
        workspaces,
        script_used_packages,
    } = data;

    if let Some(sarif_path) = opts.sarif_file {
        output::write_sarif_file(
            &results,
            &config,
            sarif_path,
            opts.quiet,
            type_aware.as_ref().map(|outcome| &outcome.meta),
        );
    }

    let retained_files_for_cross_reference = if opts.include_dupes && retained_modules.is_some() {
        retained_files.clone()
    } else if opts.include_dupes {
        retained_files.take()
    } else {
        None
    };

    let shared_parse = build_shared_parse_data(
        &results,
        trace_graph,
        retained_modules,
        retained_files,
        workspaces.clone(),
        &script_used_packages,
    );

    let config_fixable = crate::fix::is_config_fixable(opts.root, opts.config_path.as_ref());
    let required_completeness = config.type_aware.require.into();
    let (type_aware_meta, type_aware_warnings) = type_aware.map_or_else(
        || (None, Vec::new()),
        |outcome| {
            let mut meta = outcome.meta;
            meta.required_completeness = Some(required_completeness);
            (Some(meta), outcome.warnings)
        },
    );

    // Report result volume to telemetry from the real result, independent of
    // the exit-code gate. Exact counts are bucketed before serialization.
    crate::telemetry::note_result_count(results.total_issues());

    CheckResult {
        results,
        config,
        config_fixable,
        elapsed,
        fail_on_issues: opts.fail_on_issues,
        regression: regression_outcome,
        baseline_deltas: None,
        baseline_matched,
        timings: trace_timings,
        shared_parse,
        type_aware_meta,
        type_coupling,
        type_aware_warnings,
        syntactic_dead_code_keys,
        impact_closure: None,
        public_api_keys: None,
        partition_order: None,
        focus_facts: None,
        export_lines: None,
        internal_consumers: None,
        workspaces,
        retained_files: retained_files_for_cross_reference,
    }
}

/// Run analysis, filtering, and baseline handling. Returns results without printing.
#[expect(
    clippy::too_many_lines,
    reason = "check execution keeps pipeline ordering and stored identity handling explicit"
)]
pub fn execute_check(opts: &CheckOptions<'_>) -> Result<CheckResult, ExitCode> {
    let start = Instant::now();

    let config = prepare_check_config(opts)?;
    validate_effective_type_aware_output(opts.output, config.type_aware.enabled)?;

    let ws_roots = filtering::resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;

    let changed_files: Option<rustc_hash::FxHashSet<std::path::PathBuf>> = opts
        .changed_since
        .and_then(|git_ref| filtering::get_changed_files(opts.root, git_ref));

    let mut data = run_check_analysis(opts, &config)?;

    if let Err(code) = handle_trace_side_effects(
        opts,
        &config,
        data.trace_graph.as_ref(),
        data.trace_timings.as_ref(),
        &data.script_used_packages,
    ) {
        // A focused trace / closure view exits here without building the full
        // CheckResult (where the normal path records find-state below). The full
        // analysis still ran, so record its find-state for telemetry on the
        // focused-success exit, keeping the DeadCode workflow's findings_present
        // populated regardless of the output view (issue #1650). A trace error
        // (exit 2) is a failed run and is left unset.
        if code == ExitCode::SUCCESS {
            crate::telemetry::note_result_count(data.results.total_issues());
        }
        return Err(code);
    }

    apply_scope_filters(
        opts,
        &config,
        &mut data.results,
        ws_roots.as_ref(),
        changed_files.as_ref(),
    );
    apply_file_filter(opts, &mut data.results);

    apply_rules_and_filters(opts, &config, &mut data.results);

    // Capture the pre-refinement dead-code keys so the audit gate can fall
    // back to identity-independent syntactic attribution when base and head
    // semantic identities differ. Must run after scope/rule filters and before
    // the refinement below mutates `data.results`.
    let syntactic_dead_code_keys = config
        .type_aware
        .enabled
        .then(|| fallow_api::audit_keys::dead_code_keys(&data.results, &config.root));
    let (type_aware, type_coupling) = if config.type_aware.enabled {
        let include_symbol_use = if opts.filters.any_active() {
            opts.filters.unused_exports
                || opts.filters.unused_types
                || opts.filters.unused_class_members
        } else {
            config.rules.unused_exports != Severity::Off
                || config.rules.unused_types != Severity::Off
                || config.rules.unused_class_members != Severity::Off
        };
        let type_aware_projects = config
            .type_aware
            .projects
            .iter()
            .map(std::path::PathBuf::from)
            .collect::<Vec<_>>();
        let entry_points = data.trace_graph.as_ref().map_or_else(Vec::new, |graph| {
            fallow_engine::project_analysis::public_api_entry_paths_for_graph(
                graph,
                &config,
                &data.workspaces,
            )
        });
        let outcome = fallow_api::refine_type_aware_results_with_config(
            &config,
            &mut data.results,
            &type_aware_projects,
            &entry_points,
            include_symbol_use,
            config.rules.private_type_leaks != Severity::Off,
            opts.retain_modules_for_health,
        )
        .map_err(|error| {
            emit_error(
                &format!("Type-aware analysis failed: {error}"),
                2,
                opts.output,
            )
        })?;
        outcome.map_or((None, None), |outcome| {
            (Some(outcome.type_aware), outcome.type_coupling)
        })
    } else {
        (None, None)
    };
    let elapsed = start.elapsed();
    let analysis_identity = type_aware
        .as_ref()
        .and_then(|outcome| outcome.meta.identity.clone())
        .unwrap_or_default();

    let baseline_matched = handle_baseline(
        &mut data.results,
        opts.save_baseline,
        opts.baseline,
        &config.root,
        opts.quiet,
        opts.output,
        &analysis_identity,
    )?;

    let regression_outcome =
        resolve_check_regression(opts, &config, &data.results, &analysis_identity)?;

    Ok(complete_check_execution(CheckCompletionInput {
        opts,
        config,
        data,
        elapsed,
        regression_outcome,
        baseline_matched,
        type_aware,
        type_coupling,
        syntactic_dead_code_keys,
    }))
}

pub fn benchmark_dead_code_json(
    root: &std::path::Path,
    threads: usize,
) -> Result<(usize, usize), ExitCode> {
    let config_path = None;
    let filters = IssueFilters::default();
    let trace_opts = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: None,
        impact_closure: None,
        symbol_impact: None,
        performance: false,
    };
    let result = execute_check(&CheckOptions {
        root,
        config_path: &config_path,
        output: OutputFormat::Json,
        json_style: crate::json_style::JsonStyle::Compact,
        no_cache: true,
        threads,
        quiet: true,
        allow_remote_extends: false,
        fail_on_issues: false,
        filters: &filters,
        changed_since: None,
        diff_index: None,
        use_shared_diff_index: true,
        baseline: None,
        save_baseline: None,
        sarif_file: None,
        production: false,
        production_override: Some(false),
        workspace: None,
        changed_workspaces: None,
        group_by: None,
        include_dupes: false,
        type_aware: None,
        type_aware_config_override: None,
        type_aware_projects: &[],
        type_aware_require: None,
        trace_opts: &trace_opts,
        explain: false,
        top: None,
        file: &[],
        include_entry_exports: false,
        summary: false,
        regression_opts: RegressionOpts {
            fail_on_regression: false,
            tolerance: crate::regression::Tolerance::Absolute(0),
            regression_baseline_file: None,
            save_target: crate::regression::SaveRegressionTarget::None,
            scoped: false,
            quiet: true,
            output: OutputFormat::Json,
        },
        retain_modules_for_health: false,
        defer_performance: false,
        analysis_snapshot: fallow_config::AnalysisSnapshot::Current,
    })?;
    let rendered = report::render_check_json(&report::CheckJsonRenderInput {
        results: &result.results,
        root: &result.config.root,
        elapsed: result.elapsed,
        type_aware: result.type_aware_meta.as_ref(),
        regression: result.regression.as_ref(),
        baseline_matched: result.baseline_matched,
        config_fixable: result.config_fixable,
        json_style: crate::json_style::JsonStyle::Compact,
    })
    .map_err(|_| ExitCode::from(2))?;
    Ok((result.results.total_issues(), rendered.len()))
}

fn validate_effective_type_aware_output(
    output: fallow_config::OutputFormat,
    enabled: bool,
) -> Result<(), ExitCode> {
    if enabled
        && !matches!(
            output,
            fallow_config::OutputFormat::Human
                | fallow_config::OutputFormat::Json
                | fallow_config::OutputFormat::Sarif
                | fallow_config::OutputFormat::Compact
                | fallow_config::OutputFormat::Markdown
                | fallow_config::OutputFormat::CodeClimate
                | fallow_config::OutputFormat::PrCommentGithub
                | fallow_config::OutputFormat::PrCommentGitlab
                | fallow_config::OutputFormat::ReviewGithub
                | fallow_config::OutputFormat::ReviewGitlab
        )
    {
        return Err(emit_error(
            "type-aware analysis supports human, JSON, SARIF, compact, markdown, CodeClimate, PR-comment, and review output; pair presentation formats with the JSON artifact to preserve semantic provenance",
            2,
            output,
        ));
    }
    Ok(())
}

pub struct PrintCheckOptions {
    pub quiet: bool,
    pub explain: bool,
    pub regression_json: bool,
    pub group_by: Option<report::OwnershipResolver>,
    pub top: Option<usize>,
    pub summary: bool,
    pub summary_heading: bool,
    pub show_explain_tip: bool,
    pub type_aware_scope: Option<&'static str>,
    pub json_style: crate::json_style::JsonStyle,
}

struct PreparedPrintCheck<'a> {
    effective_rules: RulesConfig,
    report_ctx: report::ReportContext<'a>,
    regression_json: bool,
    quiet: bool,
}

fn prepare_print_check(result: &CheckResult, opts: PrintCheckOptions) -> PreparedPrintCheck<'_> {
    PreparedPrintCheck {
        effective_rules: effective_check_rules(result),
        report_ctx: report::ReportContext {
            root: &result.config.root,
            rules: &result.config.rules,
            elapsed: result.elapsed,
            quiet: opts.quiet,
            explain: opts.explain,
            type_aware: result.type_aware_meta.as_ref(),
            type_aware_scope: opts.type_aware_scope,
            group_by: opts.group_by,
            top: opts.top,
            summary: opts.summary,
            summary_heading: opts.summary_heading,
            show_explain_tip: opts.show_explain_tip,
            baseline_matched: result.baseline_matched,
            config_fixable: result.config_fixable,
            skip_score_and_trend: false,
            css_requested: false,
            json_style: opts.json_style,
        },
        regression_json: opts.regression_json,
        quiet: opts.quiet,
    }
}

fn effective_check_rules(result: &CheckResult) -> RulesConfig {
    if result.fail_on_issues {
        let mut rules = result.config.rules.clone();
        rules::promote_warns_to_errors(&mut rules);
        rules
    } else {
        result.config.rules.clone()
    }
}

/// Print check results and return appropriate exit code.
pub fn print_check_result(result: &CheckResult, opts: PrintCheckOptions) -> ExitCode {
    let prepared = prepare_print_check(result, opts);
    let report_code = report::print_results(
        &result.results,
        &prepared.report_ctx,
        result.config.output,
        if prepared.regression_json {
            result.regression.as_ref()
        } else {
            None
        },
    );
    if report_code != ExitCode::SUCCESS {
        return report_code;
    }

    print_type_aware_summary(result);
    print_type_aware_warnings(result);

    if type_aware_completeness_failed(result, prepared.quiet) {
        return ExitCode::from(1);
    }

    if let Some(exit) = check_regression_exit_code(result.regression.as_ref(), prepared.quiet) {
        return exit;
    }

    print_load_data_key_abstain_note(result, prepared.quiet);
    print_unused_component_props_exempted_note(result, prepared.quiet);
    print_unmatched_ignore_findings_note(result, prepared.quiet);
    issue_severity_exit_code(result, &prepared.effective_rules)
}

fn type_aware_completeness_failed(result: &CheckResult, quiet: bool) -> bool {
    if result.config.type_aware.require != fallow_config::TypeAwareRequire::Complete {
        return false;
    }
    let Some(meta) = &result.type_aware_meta else {
        return false;
    };
    let incomplete = crate::report::ci::required_type_aware_incomplete(Some(meta));
    if !incomplete {
        return false;
    }
    if !quiet {
        eprintln!(
            "{}",
            crate::report::human_status_line(
                crate::report::HumanStatus::Warning,
                "Type-aware completeness gate failed because semantic analysis was unavailable or partial."
            )
        );
    }
    true
}

fn print_type_aware_summary(result: &CheckResult) {
    if !matches!(result.config.output, OutputFormat::Human) {
        return;
    }
    if let Some(meta) = &result.type_aware_meta {
        println!(
            "{}",
            crate::report::human_status_line(
                crate::report::type_aware_meta_status(meta),
                format_type_aware_summary(meta)
            )
        );
    }
}

fn format_type_aware_summary(meta: &fallow_types::envelope::TypeAwareMeta) -> String {
    let candidates = count_noun(meta.candidate_count, "candidate", "candidates");
    let confirmed = count_noun(meta.confirmed_used_count, "confirmed use", "confirmed uses");
    let contracts = count_noun(
        meta.contract_preserved_count,
        "preserved contract",
        "preserved contracts",
    );
    let no_static_references = count_noun(
        meta.no_static_references_count,
        "candidate without static references",
        "candidates without static references",
    );
    let fixable = count_noun(meta.fix_eligible_count, "guarded fix", "guarded fixes");
    let unresolved = count_noun(
        meta.unresolved_count,
        "unresolved finding",
        "unresolved findings",
    );
    let abstained = count_noun(
        meta.abstained_count,
        "abstained finding",
        "abstained findings",
    );
    format!(
        "Type-aware refinement: {candidates}, {confirmed}, {contracts}, \
         {no_static_references} ({fixable}), {unresolved}, {abstained}"
    )
}

fn count_noun(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

fn print_type_aware_warnings(result: &CheckResult) {
    for warning in &result.type_aware_warnings {
        eprintln!(
            "{}",
            crate::report::human_status_line(
                crate::report::HumanStatus::Warning,
                format_args!("Type-aware: {warning}")
            )
        );
    }
}

fn check_regression_exit_code(
    outcome: Option<&RegressionOutcome>,
    quiet: bool,
) -> Option<ExitCode> {
    let outcome = outcome?;
    if !quiet {
        regression::print_regression_outcome(outcome);
    }
    outcome.is_failure().then(|| ExitCode::from(1))
}

fn print_load_data_key_abstain_note(result: &CheckResult, quiet: bool) {
    if !result.results.unused_load_data_keys_global_abstain
        || quiet
        || !matches!(result.config.output, OutputFormat::Human)
    {
        return;
    }
    eprintln!(
        "Note: unused-load-data-key abstained project-wide (a whole-object use of \
         page.data / $page.data was seen; any returned key could be read reflectively)."
    );
}

/// Human-output note when `unusedComponentProps.ignorePattern` exempted at least
/// one prop this run. Closes the silent-no-op loop (a typo'd pattern matching
/// nothing prints nothing) and teaches that the match is on the LOCAL destructure
/// binding name (`_stage`), not the public prop name the finding would report.
fn print_unused_component_props_exempted_note(result: &CheckResult, quiet: bool) {
    if result.config.unused_component_props_ignore.is_none()
        || result.results.unused_component_props_exempted == 0
        || quiet
        || !matches!(result.config.output, OutputFormat::Human)
    {
        return;
    }
    let count = result.results.unused_component_props_exempted;
    let noun = if count == 1 { "prop" } else { "props" };
    eprintln!(
        "Note: {count} component {noun} exempted by unusedComponentProps.ignorePattern \
         (matched on the local binding name, e.g. _stage, not the public prop name)."
    );
}

/// Human-output note when an `ignoreFindings` pattern matched no candidate
/// finding this run. A typo'd pattern is otherwise a silent no-op.
fn print_unmatched_ignore_findings_note(result: &CheckResult, quiet: bool) {
    if quiet || !matches!(result.config.output, OutputFormat::Human) {
        return;
    }
    let unmatched = result.config.ignore_findings.unmatched_patterns();
    if unmatched.is_empty() {
        return;
    }
    let noun = if unmatched.len() == 1 {
        "pattern"
    } else {
        "patterns"
    };
    eprintln!(
        "Note: ignoreFindings {noun} matched no finding this run: {} (patterns are \
         project-root-relative globs; check for typos).",
        unmatched.join(", ")
    );
}

fn issue_severity_exit_code(result: &CheckResult, effective_rules: &RulesConfig) -> ExitCode {
    if rules::has_error_severity_issues(&result.results, effective_rules, Some(&result.config)) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

pub fn run_check(opts: &CheckOptions<'_>) -> ExitCode {
    let result = match execute_check(opts) {
        Ok(r) => r,
        Err(code) => return code,
    };

    if !opts.quiet && matches!(opts.output, OutputFormat::Human) {
        crate::combined::print_entry_point_summary(&result.results);
    }

    let resolver = match crate::build_ownership_resolver(
        opts.group_by,
        opts.root,
        result.config.codeowners.as_deref(),
        opts.output,
    ) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let exit = print_check_result(
        &result,
        PrintCheckOptions {
            quiet: opts.quiet,
            explain: opts.explain,
            regression_json: true,
            group_by: resolver,
            top: opts.top,
            summary: opts.summary,
            summary_heading: true,
            show_explain_tip: true,
            type_aware_scope: None,
            json_style: opts.json_style,
        },
    );

    if opts.include_dupes && result.config.duplicates.enabled {
        let Some(files) = result.retained_files.as_deref() else {
            return emit_error(
                "internal error: --include-dupes analysis did not retain discovered files",
                2,
                opts.output,
            );
        };
        output::run_cross_reference(&result.config, &result.results, files, opts.quiet);
    }

    exit
}

/// Save baseline and/or compare against an existing baseline.
///
/// Returns `Some(ExitCode)` on fatal errors (serialization/IO failure),
/// `Ok(None)` when no baseline was loaded, `Ok(Some((entries, matched)))` when
/// a baseline was loaded, or `Err(ExitCode)` on fatal errors.
#[expect(
    clippy::too_many_arguments,
    reason = "baseline I/O keeps scope, output, and semantic compatibility inputs explicit"
)]
fn handle_baseline(
    results: &mut fallow_types::results::AnalysisResults,
    save_path: Option<&std::path::Path>,
    load_path: Option<&std::path::Path>,
    root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
    analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity,
) -> Result<Option<(usize, usize)>, ExitCode> {
    if let Some(baseline_path) = save_path {
        save_baseline_file(
            results,
            baseline_path,
            root,
            quiet,
            output,
            analysis_identity,
        )?;
    }

    if let Some(baseline_path) = load_path {
        return load_and_compare_baseline(
            results,
            baseline_path,
            root,
            quiet,
            output,
            analysis_identity,
        )
        .map(Some);
    }

    Ok(None)
}

/// Serialize the current results to a baseline file, creating parent dirs.
fn save_baseline_file(
    results: &fallow_types::results::AnalysisResults,
    baseline_path: &std::path::Path,
    root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
    analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity,
) -> Result<(), ExitCode> {
    let baseline_data =
        BaselineData::from_results_with_identity(results, root, analysis_identity.clone());
    let mut json = serde_json::to_string_pretty(&baseline_data)
        .map_err(|e| emit_error(&format!("failed to serialize baseline: {e}"), 2, output))?;
    json.push('\n');
    if let Some(parent) = baseline_path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return Err(emit_error(
            &format!("failed to create baseline directory: {e}"),
            2,
            output,
        ));
    }
    if let Err(e) = std::fs::write(baseline_path, json) {
        return Err(emit_error(
            &format!("failed to save baseline: {e}"),
            2,
            output,
        ));
    }
    if !quiet {
        eprintln!("Baseline saved to {}", baseline_path.display());
    }
    Ok(())
}

/// Load a baseline file, filter out matched issues, and return
/// `(baseline_entries, matched)`.
fn load_and_compare_baseline(
    results: &mut fallow_types::results::AnalysisResults,
    baseline_path: &std::path::Path,
    root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
    analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity,
) -> Result<(usize, usize), ExitCode> {
    let content = std::fs::read_to_string(baseline_path)
        .map_err(|e| emit_error(&format!("failed to read baseline: {e}"), 2, output))?;
    let baseline_data = serde_json::from_str::<BaselineData>(&content)
        .map_err(|e| emit_error(&format!("failed to parse baseline: {e}"), 2, output))?;
    let incompatible = baseline_data
        .analysis_identity()
        .incompatible_fields(analysis_identity);
    if !incompatible.is_empty() {
        let type_aware_flag = if matches!(
            analysis_identity.mode,
            fallow_types::semantic::SemanticAnalysisMode::TypeAware
        ) {
            " --type-aware"
        } else {
            ""
        };
        return Err(emit_error(
            &format!(
                "baseline analysis identity is incompatible in: {}. Regenerate it with: fallow dead-code{type_aware_flag} --save-baseline {}",
                incompatible.join(", "),
                baseline_path.display(),
            ),
            2,
            output,
        ));
    }
    let baseline_entries = baseline_data.total_entries();
    let before = results.total_issues();
    *results = filter_new_issues(std::mem::take(results), &baseline_data, root);
    let matched = before.saturating_sub(results.total_issues());
    if !quiet {
        eprintln!("Comparing against baseline: {}", baseline_path.display());
    }
    if baseline_entries > 0 && matched == 0 && !quiet {
        eprintln!(
            "Warning: baseline has {baseline_entries} entries but matched \
             0 current issues. Your paths may have changed, or the baseline \
             was saved on a different machine. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        );
    }
    Ok((baseline_entries, matched))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::extract::MemberKind;
    use fallow_types::output_dead_code::*;
    use fallow_types::results::*;
    use std::path::PathBuf;

    fn resolved_config(type_aware_enabled: bool) -> ResolvedConfig {
        let config: fallow_config::FallowConfig = serde_json::from_value(serde_json::json!({
            "typeAware": { "enabled": type_aware_enabled }
        }))
        .expect("config");
        config.resolve(
            PathBuf::from("/repo"),
            fallow_config::OutputFormat::Json,
            1,
            false,
            true,
            None,
        )
    }

    /// First match wins: CLI flag, then command-scoped config override, then
    /// the top-level `typeAware.enabled` opt-in. The environment layer is not
    /// exercised here because env vars are process-global across tests.
    #[test]
    fn type_aware_override_precedence_is_cli_then_scoped_config_then_config() {
        let cases: [(Option<bool>, Option<bool>, bool, bool); 6] = [
            // --no-type-aware beats every config layer.
            (Some(false), Some(true), true, false),
            // --type-aware beats a scoped opt-out.
            (Some(true), Some(false), false, true),
            // audit.typeAware=false keeps audit syntactic under typeAware.enabled=true.
            (None, Some(false), true, false),
            // audit.typeAware=true opts audit in alone.
            (None, Some(true), false, true),
            // Unset layers inherit typeAware.enabled.
            (None, None, true, true),
            (None, None, false, false),
        ];
        for (cli, scoped, config_enabled, expected) in cases {
            let mut config = resolved_config(config_enabled);
            apply_type_aware_overrides_from(
                fallow_config::OutputFormat::Json,
                cli,
                scoped,
                &[],
                None,
                &mut config,
            )
            .expect("overrides should apply");
            assert_eq!(
                config.type_aware.enabled, expected,
                "cli={cli:?} scoped={scoped:?} config={config_enabled}"
            );
        }
    }

    fn no_filters() -> IssueFilters {
        IssueFilters {
            unused_files: false,
            unused_exports: false,
            unused_deps: false,
            unused_types: false,
            private_type_leaks: false,
            unused_enum_members: false,
            unused_class_members: false,
            unused_store_members: false,
            unprovided_injects: false,
            unrendered_components: false,
            unused_component_props: false,
            unused_component_emits: false,
            unused_component_inputs: false,
            unused_component_outputs: false,
            unused_svelte_events: false,
            unused_server_actions: false,
            unused_load_data_keys: false,
            unresolved_imports: false,
            unlisted_deps: false,
            duplicate_exports: false,
            circular_deps: false,
            re_export_cycles: false,
            boundary_violations: false,
            policy_violations: false,
            stale_suppressions: false,
            unused_catalog_entries: false,
            empty_catalog_groups: false,
            unresolved_catalog_references: false,
            unused_dependency_overrides: false,
            misconfigured_dependency_overrides: false,
            invalid_client_exports: false,
            mixed_client_server_barrels: false,
            misplaced_directives: false,
            route_collisions: false,
            dynamic_segment_name_conflicts: false,
        }
    }

    #[test]
    fn type_aware_summary_reports_clean_and_retained_outcomes() {
        let clean = fallow_types::envelope::TypeAwareMeta {
            candidate_count: 1,
            confirmed_used_count: 1,
            ..fallow_types::envelope::TypeAwareMeta::default()
        };
        assert_eq!(
            format_type_aware_summary(&clean),
            "Type-aware refinement: 1 candidate, 1 confirmed use, 0 preserved contracts, \
             0 candidates without static references (0 guarded fixes), 0 unresolved findings, \
             0 abstained findings"
        );

        let retained = fallow_types::envelope::TypeAwareMeta {
            candidate_count: 3,
            confirmed_used_count: 1,
            no_static_references_count: 1,
            fix_eligible_count: 1,
            unresolved_count: 1,
            ..fallow_types::envelope::TypeAwareMeta::default()
        };
        assert_eq!(
            format_type_aware_summary(&retained),
            "Type-aware refinement: 3 candidates, 1 confirmed use, 0 preserved contracts, \
             1 candidate without static references (1 guarded fix), 1 unresolved finding, \
             0 abstained findings"
        );

        let abstained = fallow_types::envelope::TypeAwareMeta {
            candidate_count: 2,
            contract_preserved_count: 1,
            abstained_count: 1,
            ..fallow_types::envelope::TypeAwareMeta::default()
        };
        assert_eq!(
            format_type_aware_summary(&abstained),
            "Type-aware refinement: 2 candidates, 0 confirmed uses, 1 preserved contract, \
             0 candidates without static references (0 guarded fixes), 0 unresolved findings, \
             1 abstained finding"
        );
    }

    #[test]
    fn private_type_leaks_filter_opts_in_off_by_default_rule() {
        let mut rules = fallow_config::RulesConfig::default();
        assert_eq!(rules.private_type_leaks, fallow_config::Severity::Off);

        let mut filters = no_filters();
        filters.private_type_leaks = true;
        filters.activate_explicit_opt_ins(&mut rules);

        assert_eq!(rules.private_type_leaks, fallow_config::Severity::Warn);
    }

    fn type_aware_resolved_config() -> ResolvedConfig {
        let mut config = fallow_config::FallowConfig::default().resolve(
            PathBuf::from("/project"),
            OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
        config.type_aware.enabled = true;
        config
    }

    #[test]
    fn type_aware_defaults_private_type_leaks_on_when_unconfigured() {
        let mut config = type_aware_resolved_config();
        assert_eq!(config.rules.private_type_leaks, Severity::Off);
        assert!(!config.rules.private_type_leaks_configured);

        apply_type_aware_private_leak_default(&mut config, false);

        assert_eq!(config.rules.private_type_leaks, Severity::Warn);
    }

    #[test]
    fn type_aware_respects_explicit_private_type_leaks_off() {
        let mut config = type_aware_resolved_config();
        config.rules.private_type_leaks_configured = true;

        apply_type_aware_private_leak_default(&mut config, false);

        assert_eq!(config.rules.private_type_leaks, Severity::Off);
    }

    fn load_and_resolve_config_file(json: &str) -> ResolvedConfig {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join(".fallowrc.json");
        std::fs::write(&config_path, json).expect("config file is written");
        fallow_config::FallowConfig::load(&config_path)
            .expect("config file loads")
            .resolve(
                dir.path().to_path_buf(),
                OutputFormat::Json,
                1,
                true,
                true,
                None,
            )
    }

    #[test]
    fn loaded_explicit_private_type_leaks_off_survives_resolve_and_type_aware_default() {
        let mut config = load_and_resolve_config_file(
            r#"{"typeAware": {"enabled": true}, "rules": {"private-type-leaks": "off"}}"#,
        );
        assert!(config.type_aware.enabled);
        assert!(config.rules.private_type_leaks_configured);

        apply_type_aware_private_leak_default(&mut config, false);

        assert_eq!(config.rules.private_type_leaks, Severity::Off);
    }

    #[test]
    fn loaded_unset_private_type_leaks_defaults_on_after_resolve_under_type_aware() {
        let mut config = load_and_resolve_config_file(r#"{"typeAware": {"enabled": true}}"#);
        assert!(config.type_aware.enabled);
        assert!(!config.rules.private_type_leaks_configured);

        apply_type_aware_private_leak_default(&mut config, false);

        assert_eq!(config.rules.private_type_leaks, Severity::Warn);
    }

    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
    fn make_results() -> AnalysisResults {
        let mut r = AnalysisResults::default();
        r.unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/a.ts"),
            }));
        r.unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/b.ts"),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        r.unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/c.ts"),
                export_name: "MyType".into(),
                is_type_only: true,
                line: 5,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        r.unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/project/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        r.unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "jest".into(),
                location: DependencyLocation::DevDependencies,
                path: PathBuf::from("/project/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        r.test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "msw".into(),
                    path: PathBuf::from("/project/package.json"),
                    line: 9,
                },
            ));
        r.unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/d.ts"),
                parent_name: "Status".into(),
                member_name: "Pending".into(),
                kind: MemberKind::EnumMember,
                line: 3,
                col: 0,
            }));
        r.unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/e.ts"),
                parent_name: "Service".into(),
                member_name: "helper".into(),
                kind: MemberKind::ClassMethod,
                line: 10,
                col: 0,
            }));
        r.unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/src/f.ts"),
                specifier: "./missing".into(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));
        r.unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".into(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("/project/src/g.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            ));
        r.duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "helper".into(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/h.ts"),
                        line: 15,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/i.ts"),
                        line: 30,
                        col: 0,
                    },
                ],
            }));
        r
    }

    #[test]
    fn save_baseline_writes_trailing_newline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let baseline_path = dir.path().join("baseline.json");
        let mut results = make_results();

        handle_baseline(
            &mut results,
            Some(&baseline_path),
            None,
            std::path::Path::new("/project"),
            true,
            OutputFormat::Json,
            &fallow_types::semantic::SemanticAnalysisIdentity::default(),
        )
        .expect("baseline save succeeds");

        let saved = std::fs::read_to_string(&baseline_path).expect("baseline is written");
        assert!(saved.ends_with('\n'));
        assert!(!saved.ends_with("\n\n"));
    }

    #[test]
    fn no_filters_means_none_active() {
        assert!(!no_filters().any_active());
    }

    #[test]
    fn single_filter_is_active() {
        let mut f = no_filters();
        f.unused_files = true;
        assert!(f.any_active());
    }

    #[test]
    fn every_registry_filter_flag_registers_as_active() {
        for flag in fallow_types::issue_meta::DEAD_CODE_FILTER_FLAGS.iter() {
            let mut f = no_filters();
            assert!(
                f.enable_cli_filter_flag(flag),
                "registry filter flag {flag} has no CLI IssueFilters mapping"
            );
            assert!(
                f.any_active(),
                "registry filter flag {flag} stayed inactive"
            );
        }
    }

    #[test]
    fn apply_no_active_filters_preserves_all_results() {
        let mut results = make_results();
        let original_total = results.total_issues();
        no_filters().apply(&mut results);
        assert_eq!(results.total_issues(), original_total);
    }

    #[test]
    fn apply_unused_files_filter_keeps_only_unused_files() {
        let mut results = make_results();
        let mut f = no_filters();
        f.unused_files = true;
        f.apply(&mut results);

        assert_eq!(results.unused_files.len(), 1);
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_types.is_empty());
        assert!(results.unused_dependencies.is_empty());
        assert!(results.unused_dev_dependencies.is_empty());
        assert!(results.test_only_dependencies.is_empty());
        assert!(results.unused_enum_members.is_empty());
        assert!(results.unused_class_members.is_empty());
        assert!(results.unresolved_imports.is_empty());
        assert!(results.unlisted_dependencies.is_empty());
        assert!(results.duplicate_exports.is_empty());
    }

    #[test]
    fn apply_unused_deps_filter_keeps_both_dep_types() {
        let mut results = make_results();
        let mut f = no_filters();
        f.unused_deps = true;
        f.apply(&mut results);

        assert_eq!(results.unused_dependencies.len(), 1);
        assert_eq!(results.unused_dev_dependencies.len(), 1);
        assert_eq!(results.test_only_dependencies.len(), 1);
        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
    }

    #[test]
    fn apply_single_type_filter_clears_test_only_dependencies() {
        // Regression for #1192: a single-type filter that is not --unused-deps must clear
        // test-only-dependency findings, matching every other dependency kind. Before the fix the
        // --unused-deps clear arm omitted test_only_dependencies, so it leaked into the output of
        // any single-type filter run (e.g. `fallow dead-code --unused-files`).
        let mut results = make_results();
        assert_eq!(results.test_only_dependencies.len(), 1);

        let mut f = no_filters();
        f.unused_files = true;
        f.apply(&mut results);

        assert!(
            results.test_only_dependencies.is_empty(),
            "test-only-dependency findings must be cleared when --unused-deps is not active"
        );
    }

    #[test]
    fn apply_multiple_filters_keeps_selected_types() {
        let mut results = make_results();
        let mut f = no_filters();
        f.unused_files = true;
        f.unresolved_imports = true;
        f.apply(&mut results);

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(results.unresolved_imports.len(), 1);
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_types.is_empty());
        assert!(results.duplicate_exports.is_empty());
    }

    #[test]
    fn apply_circular_deps_filter_keeps_only_circular_deps() {
        let mut results = make_results();
        results.circular_dependencies.push(
            fallow_types::output_dead_code::CircularDependencyFinding::with_actions(
                fallow_types::results::CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/a.ts"),
                        PathBuf::from("/project/src/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ),
        );
        let mut f = no_filters();
        f.circular_deps = true;
        f.apply(&mut results);

        assert_eq!(results.circular_dependencies.len(), 1);
        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_dependencies.is_empty());
    }

    #[test]
    fn no_trace_options_means_none_active() {
        let t = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
            symbol_impact: None,
            performance: false,
        };
        assert!(!t.any_active());
    }

    #[test]
    fn trace_export_is_active() {
        let t = TraceOptions {
            trace_export: Some("src/foo.ts:bar".into()),
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
            symbol_impact: None,
            performance: false,
        };
        assert!(t.any_active());
    }

    #[test]
    fn trace_file_is_active() {
        let t = TraceOptions {
            trace_export: None,
            trace_file: Some("src/foo.ts".into()),
            trace_dependency: None,
            impact_closure: None,
            symbol_impact: None,
            performance: false,
        };
        assert!(t.any_active());
    }

    #[test]
    fn trace_dependency_is_active() {
        let t = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: Some("lodash".into()),
            impact_closure: None,
            symbol_impact: None,
            performance: false,
        };
        assert!(t.any_active());

        let t = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: Some("src/foo.ts".into()),
            symbol_impact: None,
            performance: false,
        };
        assert!(t.any_active());
    }

    #[test]
    fn performance_flag_is_active() {
        let t = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
            symbol_impact: None,
            performance: true,
        };
        assert!(t.any_active());
    }

    #[test]
    fn apply_boundary_violations_filter() {
        let mut results = make_results();
        results.boundary_violations.push(
            fallow_types::output_dead_code::BoundaryViolationFinding::with_actions(
                fallow_types::results::BoundaryViolation {
                    from_path: PathBuf::from("/project/src/bad.ts"),
                    to_path: PathBuf::from("/project/lib/secret.ts"),
                    from_zone: "src".to_string(),
                    to_zone: "lib".to_string(),
                    import_specifier: "../lib/secret".to_string(),
                    line: 1,
                    col: 0,
                },
            ),
        );
        let mut f = no_filters();
        f.boundary_violations = true;
        f.apply(&mut results);

        assert_eq!(results.boundary_violations.len(), 1);
        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_dependencies.is_empty());
        assert!(results.circular_dependencies.is_empty());
    }

    #[test]
    fn apply_all_filter_types_simultaneously() {
        let mut results = make_results();
        results.circular_dependencies.push(
            fallow_types::output_dead_code::CircularDependencyFinding::with_actions(
                fallow_types::results::CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/a.ts"),
                        PathBuf::from("/project/src/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ),
        );
        results.boundary_violations.push(
            fallow_types::output_dead_code::BoundaryViolationFinding::with_actions(
                fallow_types::results::BoundaryViolation {
                    from_path: PathBuf::from("/project/src/x.ts"),
                    to_path: PathBuf::from("/project/lib/y.ts"),
                    from_zone: "src".to_string(),
                    to_zone: "lib".to_string(),
                    import_specifier: "../lib/y".to_string(),
                    line: 1,
                    col: 0,
                },
            ),
        );

        let f = IssueFilters {
            unused_files: true,
            unused_exports: true,
            unused_deps: true,
            unused_types: true,
            private_type_leaks: true,
            unused_enum_members: true,
            unused_class_members: true,
            unused_store_members: true,
            unprovided_injects: true,
            unrendered_components: true,
            unused_component_props: true,
            unused_component_emits: true,
            unused_component_inputs: true,
            unused_component_outputs: true,
            unused_svelte_events: true,
            unused_server_actions: true,
            unused_load_data_keys: true,
            unresolved_imports: true,
            unlisted_deps: true,
            duplicate_exports: true,
            circular_deps: true,
            re_export_cycles: true,
            boundary_violations: true,
            policy_violations: true,
            stale_suppressions: true,
            unused_catalog_entries: true,
            empty_catalog_groups: true,
            unresolved_catalog_references: true,
            unused_dependency_overrides: true,
            misconfigured_dependency_overrides: true,
            invalid_client_exports: true,
            mixed_client_server_barrels: true,
            misplaced_directives: true,
            route_collisions: true,
            dynamic_segment_name_conflicts: true,
        };
        let total_before = results.total_issues();
        f.apply(&mut results);
        assert_eq!(results.total_issues(), total_before);
    }

    #[test]
    fn apply_unused_deps_clears_optional_and_type_only() {
        let mut results = make_results();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".into(),
                    location: DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("/project/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            ));
        results.type_only_dependencies.push(
            fallow_types::output_dead_code::TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".into(),
                    path: PathBuf::from("/project/package.json"),
                    line: 8,
                },
            ),
        );

        let mut f = no_filters();
        f.unused_exports = true; // Only keep unused exports
        f.apply(&mut results);

        assert!(results.unused_dependencies.is_empty());
        assert!(results.unused_dev_dependencies.is_empty());
        assert!(results.unused_optional_dependencies.is_empty());
        assert!(results.type_only_dependencies.is_empty());
        assert_eq!(results.unused_exports.len(), 1);
    }
}
