//! Typed analysis engine boundary for fallow consumers.
//!
//! `fallow-core` remains the internal orchestration backend. This crate owns
//! the typed boundary that editor, API, and embedding surfaces can depend on
//! without calling deprecated core entry points directly. Public modules should
//! expose owned engine runners, typed result structs, or narrowly scoped aliases
//! instead of broad core re-exports.

#![cfg_attr(not(test), deny(clippy::disallowed_methods))]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

use std::fmt;
#[cfg(test)]
use std::path::Path;

use rustc_hash::FxHashMap;

pub mod baseline;
#[path = "changed_files.rs"]
mod changed_files_impl;
#[path = "churn.rs"]
mod churn_impl;
pub mod codeowners;
mod core_backend;
mod cross_reference;
mod css;
mod dead_code;
mod discover;
mod duplicates;
mod error;
mod feature_flags;
mod flags;
#[path = "git_env.rs"]
mod git_env_impl;
mod health;
mod module_graph;
mod plugins;
mod project_config;
mod public_api;
pub mod results;
mod security;
mod session;
mod source;
mod suppress;
mod trace;
mod trace_chain;
pub mod validate;
pub mod vital_signs;

pub use changed_files_impl::{
    ChangedFilesError, ChangedFilesSpawnHook, changed_files, filter_duplication_by_changed_files,
    filter_results_by_changed_files, get_changed_files, resolve_git_common_dir,
    resolve_git_toplevel, set_spawn_hook, try_get_changed_diff, try_get_changed_files,
    try_get_changed_files_with_toplevel, validate_git_ref,
};
pub use churn_impl::{
    AuthorContribution, ChurnResult, ChurnSpawnHook, ChurnTrend, FileChurn, SinceDuration,
    analyze_churn, analyze_churn_cached, analyze_churn_from_file, is_git_repo, parse_since,
    set_spawn_hook as set_churn_spawn_hook,
};
pub use cross_reference::{CombinedFinding, CrossReferenceResult, DeadCodeKind, cross_reference};
pub use dead_code::{
    analyze, analyze_retaining_modules, analyze_with_file_hashes, analyze_with_parse_result,
    analyze_with_trace, analyze_with_usages, analyze_with_usages_and_complexity,
    filter_by_changed_files, filter_to_workspaces,
};
pub use discover::{
    AnalysisDiscovery, CategorizedEntryPoints, DiscoveredFile, EntryPoint, EntryPointSource,
    FileId, HiddenDirScope, PRODUCTION_EXCLUDE_PATTERNS, SOURCE_EXTENSIONS,
    collect_hidden_dir_scopes, collect_plugin_hidden_dir_scopes, compile_glob_set,
    discover_dynamically_loaded_entry_points, discover_entry_points, discover_files,
    discover_files_and_config_candidates, discover_files_with_additional_hidden_dirs,
    discover_files_with_plugin_scopes, discover_infrastructure_entry_points,
    discover_plugin_entry_point_sets, discover_plugin_entry_points,
    discover_workspace_entry_points, is_allowed_hidden_dir,
};
pub use duplicates::{
    CloneFingerprintSet, FINGERPRINT_PREFIX, clone_fingerprint, dominant_identifier,
    find_duplicates, find_duplicates_touching_files_with_defaults, find_duplicates_with_defaults,
    fingerprint_for_fragment, recompute_stats, refresh_clone_families,
    source_token_kinds_equivalent,
};
pub use error::emit_error;
use fallow_types::extract::ModuleInfo;
use fallow_types::results::AnalysisResults;
pub use flags::{
    FeatureFlagsAnalysis, analyze_feature_flags, builtin_env_prefixes, builtin_sdk_providers,
};
pub use git_env_impl::{AMBIENT_GIT_ENV_VARS, clear_ambient_git_env};
pub use health::{
    ComplexityRunOptions, ComplexitySectionOptions, DerivedComplexityOptions,
    DerivedHealthSections, HealthCoverageInputs, HealthExecutionOptions, HealthGateOptions,
    HealthGroupResolver, HealthPipelineInputs, HealthRunOptions, HealthRunOptionsInput,
    HealthScopeInputs, HealthSeams, HealthSectionOptions, HealthSharedParseData, HealthSort,
    HealthThresholdOverrides, RuntimeCoverageOptions, RuntimeCoverageSeamInput,
    derive_complexity_sections, derive_health_run_options, derive_health_sections,
    execute_health_inner, run_ungrouped_health, validate_coverage_root_absolute,
    validate_health_churn_file,
};
pub use health::{ownership as health_ownership, scoring as health_scoring};
pub use module_graph::{
    CoordinationGapPaths, DirectImporterSummary, FocusFileFactsPaths, ImpactClosurePaths,
    ImportedSymbolSummary, ModuleValueExport, PartitionOrderPaths, RetainedModuleGraph,
    ReviewUnitPaths, export_lines_for_changed_paths, focus_facts_for_changed_paths,
    impact_closure_for_changed_paths, internal_consumers_for_changed_paths, module_value_exports,
    partition_order_for_changed_paths,
};
pub use plugins::registry::{
    PluginRegexValidationError, builtin_plugin_names, format_plugin_regex_errors,
};
pub use plugins::{AggregatedPluginResult, PluginRegistry};
pub use project_config::{
    ProjectConfig, ProjectConfigOptions, config_for_project, config_for_project_analysis,
    resolve_cache_max_size_bytes,
};
pub use public_api::public_api_package_entry_points;
pub use results::{
    DeadCodeAnalysis, DeadCodeAnalysisArtifacts, DeadCodeAnalysisOutput,
    DeadCodeAnalysisWithHashes, DuplicationAnalysis, HealthAnalysisResult, ProjectAnalysisOutput,
};
pub use security::{derive_security_severity, security_catalogue_title};
pub use session::{AnalysisSession, AnalysisSessionParts};
pub use source::inventory::{
    InventoryComplexity, InventoryEntry, walk_source, walk_source_with_complexity,
};
pub use suppress::{IssueKind, Suppression, is_file_suppressed, is_suppressed};
pub use trace::{
    CloneTrace, DependencyTrace, ExportReference, ExportTrace, FileTrace, ImpactClosureGap,
    ImpactClosureTrace, PipelineTimings, ReExportChain, TracedCloneGroup, TracedExport,
    TracedReExport, trace_clone, trace_clone_by_fingerprint, trace_dependency, trace_export,
    trace_file, trace_impact_closure,
};
pub use trace_chain::trace_symbol_chain;

/// Result alias for typed engine operations.
pub type EngineResult<T> = Result<T, EngineError>;

/// Error type exposed by the typed engine boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError {
    message: String,
}

impl EngineError {
    /// Create an engine error from a user-facing message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// User-facing error message from the backend.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EngineError {}

pub(crate) fn engine_error(err: impl fmt::Display) -> EngineError {
    EngineError::new(err.to_string())
}

/// Build health shared parse data from retained dead-code artifacts.
#[must_use]
pub fn health_shared_parse_data_from_artifacts(
    results: &AnalysisResults,
    graph: Option<RetainedModuleGraph>,
    modules: Option<Vec<ModuleInfo>>,
    files: Option<Vec<DiscoveredFile>>,
    script_used_packages: impl IntoIterator<Item = String>,
) -> Option<HealthSharedParseData> {
    let (Some(modules), Some(files)) = (modules, files) else {
        return None;
    };
    let analysis_output = graph.map(|graph| DeadCodeAnalysisArtifacts {
        results: results.clone(),
        timings: None,
        graph: Some(graph),
        modules: None,
        files: None,
        script_used_packages: script_used_packages.into_iter().collect(),
        file_hashes: FxHashMap::default(),
    });
    Some(HealthSharedParseData {
        files,
        modules,
        analysis_output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::ProductionAnalysis;
    use fallow_types::output_format::OutputFormat;

    #[test]
    fn engine_error_displays_message() {
        let err = EngineError::new("config failed");

        assert_eq!(err.message(), "config failed");
        assert_eq!(err.to_string(), "config failed");
    }

    #[test]
    fn analysis_session_loads_config_and_discovered_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");

        assert_eq!(session.root(), temp.path());
        assert!(session.config_path().is_none());
        assert!(session.files().iter().any(|file| {
            file.path
                .strip_prefix(temp.path())
                .is_ok_and(|path| path == Path::new("src/index.ts"))
        }));
    }

    #[test]
    fn analysis_session_applies_config_adjustment_before_discovery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "export const value = 1;\n").expect("source file");
        std::fs::write(src.join("index.test.ts"), "export const testValue = 1;\n")
            .expect("test source file");

        let session = AnalysisSession::load_with_config(temp.path(), None, |config| {
            config.production = true;
        })
        .expect("session loads");

        let relative_paths: Vec<_> = session
            .files()
            .iter()
            .filter_map(|file| file.path.strip_prefix(temp.path()).ok())
            .collect();
        assert!(relative_paths.contains(&Path::new("src/index.ts")));
        assert!(!relative_paths.contains(&Path::new("src/index.test.ts")));
    }

    #[test]
    fn analysis_session_captures_workspace_diagnostics() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"name":"diagnostic-root","workspaces":["packages/*"]}"#,
        )
        .expect("package json");
        std::fs::create_dir_all(temp.path().join("packages/empty")).expect("workspace dir");
        std::fs::create_dir(temp.path().join("src")).expect("src dir");
        std::fs::write(
            temp.path().join("src/index.ts"),
            "export const value = 1;\n",
        )
        .expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");

        assert!(session.workspace_diagnostics().iter().any(|diagnostic| {
            diagnostic.kind.id() == "glob-matched-no-package-json"
                && diagnostic.path.ends_with("packages/empty")
        }));
    }

    #[test]
    fn analysis_session_can_be_consumed_into_pipeline_parts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let parts = session.into_parts();

        assert_eq!(parts.config.root, temp.path());
        assert!(parts.config_path.is_none());
        assert!(parts.files.iter().any(|file| {
            file.path
                .strip_prefix(temp.path())
                .is_ok_and(|path| path == Path::new("src/index.ts"))
        }));
    }

    #[test]
    fn analysis_session_can_be_consumed_into_parsed_pipeline_parts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        std::fs::write(src.join("late.ts"), "export const late = 1;\n").expect("late source file");
        let parts = session.into_parsed_parts(false);

        assert_eq!(parts.config.root, temp.path());
        assert!(parts.config_path.is_none());
        assert!(parts.modules.iter().any(|module| {
            parts.files[module.file_id.0 as usize]
                .path
                .strip_prefix(temp.path())
                .is_ok_and(|path| path == Path::new("src/index.ts"))
        }));
        assert!(parts.modules.iter().all(|module| {
            !parts.files[module.file_id.0 as usize]
                .path
                .ends_with("late.ts")
        }));
    }

    #[test]
    fn analysis_session_returns_combined_project_analysis() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        let repeated =
            "export function repeated() {\n  return ['alpha', 'beta', 'gamma'].join(',');\n}\n";
        std::fs::write(src.join("a.ts"), repeated).expect("source file");
        std::fs::write(src.join("b.ts"), repeated).expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let mut config = session.config().duplicates.clone();
        config.min_tokens = 1;
        config.min_lines = 1;

        let analysis = session
            .analyze_project_with(&config, true)
            .expect("project analysis succeeds");

        assert!(analysis.dead_code.modules.is_some());
        assert!(analysis.dead_code.files.is_some());
        assert!(!analysis.duplication.clone_groups.is_empty());
    }

    #[test]
    fn analysis_session_reuses_discovery_for_dead_code() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        std::fs::write(src.join("late.ts"), "export const late = 1;\n").expect("late source file");

        let analysis = session.analyze_dead_code().expect("analysis succeeds");

        assert!(
            analysis
                .results
                .unused_files
                .iter()
                .all(|finding| !finding.file.path.ends_with("late.ts")),
            "session analysis must not rediscover files added after session load"
        );
    }

    #[test]
    fn analysis_session_returns_retained_artifacts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(
            src.join("index.ts"),
            "export function used() { return 1; }\nused();\n",
        )
        .expect("source file");

        let config = config_for_project(temp.path(), None)
            .expect("config")
            .config;
        let session = AnalysisSession::from_resolved_config(config);
        let artifacts = session
            .analyze_dead_code_with_artifacts(true, true)
            .expect("analysis succeeds");

        assert!(artifacts.graph.is_some());
        assert!(artifacts.modules.is_some_and(|modules| !modules.is_empty()));
        assert!(artifacts.files.is_some_and(|files| !files.is_empty()));
    }

    #[test]
    fn analysis_session_runs_duplication_with_default_skip_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        let generated = temp.path().join("storybook-static");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::create_dir(&generated).expect("generated dir");
        let repeated =
            "export function repeated() {\n  return ['alpha', 'beta', 'gamma'].join(',');\n}\n";
        std::fs::write(src.join("a.ts"), repeated).expect("source file");
        std::fs::write(src.join("b.ts"), repeated).expect("source file");
        std::fs::write(generated.join("generated.ts"), repeated).expect("generated file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let mut config = session.config().duplicates.clone();
        config.min_tokens = 1;
        config.min_lines = 1;

        let analysis = session.find_duplicates_with_defaults(&config, None);

        assert!(!analysis.report.clone_groups.is_empty());
        assert!(analysis.default_ignore_skips.total > 0);
    }

    #[test]
    fn trace_symbol_chain_uses_retained_engine_analysis() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(
            src.join("util.ts"),
            "export function helper() { return 1; }\n",
        )
        .expect("util source");
        std::fs::write(
            src.join("index.ts"),
            "import { helper } from './util';\nexport const value = helper();\n",
        )
        .expect("index source");

        let project_config = config_for_project_analysis(
            temp.path(),
            None,
            ProjectConfigOptions {
                output: OutputFormat::Json,
                no_cache: true,
                threads: 1,
                production_override: None,
                quiet: true,
                analysis: ProductionAnalysis::DeadCode,
            },
        )
        .expect("project config loads");
        let trace = crate::trace_symbol_chain(
            &project_config.config,
            fallow_types::trace_chain::SymbolChainQuery {
                file: "src/util.ts",
                symbol: "helper",
                depth: 1,
                directions: fallow_types::trace_chain::TraceDirections {
                    callers: true,
                    callees: false,
                },
            },
        )
        .expect("trace succeeds")
        .expect("trace target exists");

        assert!(trace.symbol_found);
        assert_eq!(trace.file, Path::new("src/util.ts"));
        assert!(trace.callers.is_some_and(|callers| {
            callers
                .iter()
                .any(|caller| caller.file == Path::new("src/index.ts"))
        }));
    }
}
