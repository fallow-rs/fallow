//! Typed analysis engine facade for fallow consumers.
//!
//! `fallow-core` remains the internal orchestration backend. This crate owns
//! the typed boundary that editor, API, and embedding surfaces can depend on
//! without calling deprecated core entry points directly.

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
use std::path::{Path, PathBuf};

use fallow_config::{
    DuplicatesConfig, FallowConfig, OutputFormat, ProductionAnalysis, ResolvedConfig,
};
use rustc_hash::{FxHashMap, FxHashSet};

/// Duplication result types exposed through the engine boundary.
pub mod duplicates {
    pub mod families {
        pub use fallow_core::duplicates::families::{
            detect_mirrored_directories, group_into_families,
        };
    }

    pub mod tokenize {
        pub use fallow_core::duplicates::tokenize::tokenize_file;
    }

    pub use fallow_core::duplicates::{
        CloneFamily, CloneFingerprintSet, CloneGroup, CloneInstance, DefaultIgnoreSkips,
        DuplicationReport, DuplicationStats, FINGERPRINT_PREFIX, MirroredDirectory,
        RefactoringKind, RefactoringSuggestion, clone_fingerprint, dominant_identifier,
        find_duplicates, find_duplicates_cached, find_duplicates_cached_with_default_ignore_skips,
        find_duplicates_in_project, find_duplicates_touching_files,
        find_duplicates_touching_files_cached,
        find_duplicates_touching_files_cached_with_default_ignore_skips,
        find_duplicates_touching_files_with_default_ignore_skips,
        find_duplicates_with_default_ignore_skips, fingerprint_for_fragment,
    };
}

/// Discovery helpers and types exposed through the engine boundary.
pub mod discover {
    pub use fallow_core::discover::{
        CategorizedEntryPoints, HiddenDirScope, PRODUCTION_EXCLUDE_PATTERNS, SOURCE_EXTENSIONS,
        collect_hidden_dir_scopes, collect_plugin_hidden_dir_scopes, compile_glob_set,
        discover_dynamically_loaded_entry_points, discover_entry_points, discover_files,
        discover_files_and_config_candidates, discover_files_with_additional_hidden_dirs,
        discover_files_with_plugin_scopes, discover_infrastructure_entry_points,
        discover_plugin_entry_point_sets, discover_plugin_entry_points,
        discover_workspace_entry_points, is_allowed_hidden_dir,
    };
    pub use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
}

pub mod baseline;
pub mod codeowners;
pub mod dead_code;
pub mod error;
pub mod flags;
pub mod health;
pub mod validate;
pub mod vital_signs;

/// Extracted semantic types exposed through the engine boundary.
pub mod extract {
    pub mod inventory {
        pub use fallow_extract::inventory::{
            InventoryComplexity, InventoryEntry, walk_source, walk_source_with_complexity,
        };
    }

    pub use fallow_extract::css::{
        extract_apply_tokens, extract_css_module_exports, scan_theme_blocks,
    };
    pub use fallow_extract::css_classes::{is_typo_edit, scan_markup_class_tokens};
    pub use fallow_extract::css_metrics::compute_css_analytics;
    pub use fallow_extract::parse_all_files;
    pub use fallow_extract::sfc::extract_sfc_styles;
    pub use fallow_extract::sfc_css::{scoped_unused_classes, sfc_virtual_stylesheet};
    pub use fallow_extract::tailwind::scan_tailwind_arbitrary_values;
    pub use fallow_types::extract::*;
}

/// Parse cache helpers exposed through the engine boundary.
pub mod cache {
    pub use fallow_extract::cache::CacheStore;
}

/// Module graph types exposed through the engine boundary.
pub mod graph {
    pub use fallow_graph::graph::{
        CoordinationGapPaths, ExportSymbol, FocusFileFactsPaths, ImpactClosurePaths, ModuleGraph,
        ModuleNode, PartitionOrderPaths,
    };
}

/// Module resolution types exposed through the engine boundary.
pub mod resolve {
    pub use fallow_graph::resolve::ResolvedModule;
}

/// Public API graph helpers exposed through the engine boundary.
pub mod public_api {
    pub use fallow_core::analyze::public_api_package_entry_points;
}

/// Plugin registry helpers and types exposed through the engine boundary.
pub mod plugins {
    pub mod registry {
        pub use fallow_core::plugins::registry::{
            builtin_plugin_names, format_plugin_regex_errors,
        };
    }

    pub use fallow_core::plugins::{AggregatedPluginResult, PluginRegistry};
}

/// Git process environment helpers exposed through the engine boundary.
pub mod git_env {
    pub use fallow_core::git_env::{AMBIENT_GIT_ENV_VARS, clear_ambient_git_env};
}

/// Analysis result types exposed through the engine boundary.
pub mod results {
    pub use fallow_types::output_dead_code::*;
    pub use fallow_types::results::*;
}

/// Suppression helpers exposed for editor and embedding surfaces.
pub mod suppress {
    pub use fallow_core::suppress::{IssueKind, Suppression, is_file_suppressed, is_suppressed};
}

/// Changed-file helpers exposed through the engine boundary for editor and
/// embedding surfaces.
pub mod changed_files {
    pub use fallow_core::changed_files::{
        ChangedFilesError, filter_duplication_by_changed_files, filter_results_by_changed_files,
        get_changed_files, resolve_git_common_dir, resolve_git_toplevel, set_spawn_hook,
        try_get_changed_diff, try_get_changed_files, try_get_changed_files_with_toplevel,
        validate_git_ref,
    };
}

/// Cross-reference helpers exposed through the engine boundary.
pub mod cross_reference {
    pub use fallow_core::cross_reference::{
        CombinedFinding, CrossReferenceResult, DeadCodeKind, cross_reference,
    };
}

/// Git churn helpers and types exposed through the engine boundary.
pub mod churn {
    pub use fallow_core::churn::{
        AuthorContribution, ChurnResult, ChurnSpawnHook, FileChurn, SinceDuration, analyze_churn,
        analyze_churn_cached, analyze_churn_from_file, is_git_repo, parse_since, set_spawn_hook,
    };
    pub use fallow_types::churn::ChurnTrend;
}

/// Security metadata helpers exposed through the engine boundary.
pub mod security {
    pub use fallow_core::analyze::{derive_security_severity, security_catalogue_title};
}

/// Symbol trace types exposed through the engine boundary.
pub mod trace_chain {
    pub use fallow_core::trace_chain::{
        ChainHop, DEFAULT_TRACE_DEPTH, SymbolChainQuery, SymbolChainTrace, TraceDirections,
        UnresolvedCallee, UnresolvedReason,
    };
}

/// Read-only trace helpers exposed through the engine boundary.
pub mod trace {
    pub use fallow_core::trace::{
        CloneTrace, DependencyTrace, ExportReference, ExportTrace, FileTrace, ImpactClosureGap,
        ImpactClosureTrace, PipelineTimings, ReExportChain, TracedCloneGroup, TracedExport,
        TracedReExport, trace_clone, trace_clone_by_fingerprint, trace_dependency, trace_export,
        trace_file, trace_impact_closure,
    };
}

pub use fallow_core::AnalysisDiscovery;
pub use fallow_core::duplicates::{
    CloneFamily, CloneGroup, CloneInstance, DefaultIgnoreSkips, DuplicationReport,
    DuplicationStats, MirroredDirectory, RefactoringSuggestion,
};
pub use fallow_types::discover::{DiscoveredFile, FileId};
pub use fallow_types::extract::ModuleInfo;
pub use fallow_types::results::AnalysisResults;
pub use health::{
    ComplexityRunOptions, ComplexitySectionOptions, DerivedComplexityOptions,
    DerivedHealthSections, HealthAnalysisResult, HealthCoverageInputs, HealthExecutionOptions,
    HealthGateOptions, HealthRunOptions, HealthRunOptionsInput, HealthSectionOptions,
    HealthSharedParseData, HealthSort, HealthThresholdOverrides, RuntimeCoverageOptions,
    derive_complexity_sections, derive_health_run_options, derive_health_sections,
    validate_coverage_root_absolute,
};

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

fn engine_error(err: impl fmt::Display) -> EngineError {
    EngineError::new(err.to_string())
}

/// Resolved project config plus the config file path when one was loaded.
#[derive(Debug)]
pub struct ProjectConfig {
    pub config: ResolvedConfig,
    pub path: Option<PathBuf>,
}

/// Scalar config-loading knobs for one analysis family.
#[derive(Debug, Clone, Copy)]
pub struct ProjectConfigOptions {
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub production_override: Option<bool>,
    pub quiet: bool,
    pub analysis: ProductionAnalysis,
}

/// Typed dead-code analysis result.
#[derive(Debug)]
pub struct DeadCodeAnalysis {
    pub results: AnalysisResults,
}

/// Typed dead-code analysis result with per-file source hashes.
#[derive(Debug)]
pub struct DeadCodeAnalysisWithHashes {
    pub results: AnalysisResults,
    pub file_hashes: FxHashMap<PathBuf, u64>,
}

/// Typed dead-code analysis result with retained parser artifacts.
#[derive(Debug)]
pub struct DeadCodeAnalysisOutput {
    pub results: AnalysisResults,
    pub modules: Option<Vec<ModuleInfo>>,
    pub files: Option<Vec<DiscoveredFile>>,
}

/// Typed dead-code analysis result with all reusable pipeline artifacts.
#[derive(Debug)]
pub struct DeadCodeAnalysisArtifacts {
    pub results: AnalysisResults,
    pub timings: Option<trace::PipelineTimings>,
    pub graph: Option<graph::ModuleGraph>,
    pub modules: Option<Vec<ModuleInfo>>,
    pub files: Option<Vec<DiscoveredFile>>,
    pub script_used_packages: FxHashSet<String>,
    pub file_hashes: FxHashMap<PathBuf, u64>,
}

/// Typed project analysis result combining dead-code and duplication outputs.
#[derive(Debug)]
pub struct ProjectAnalysisOutput {
    pub dead_code: DeadCodeAnalysisOutput,
    pub duplication: DuplicationReport,
}

/// Typed duplication analysis result.
#[derive(Debug)]
pub struct DuplicationAnalysis {
    pub report: DuplicationReport,
    pub default_ignore_skips: DefaultIgnoreSkips,
}

/// Reusable engine session for one resolved project.
///
/// The session owns the resolved config and discovered file set so future
/// consumers can share graph-sensitive inputs without each surface recreating
/// its own partial orchestration.
#[derive(Debug)]
pub struct AnalysisSession {
    config: ResolvedConfig,
    config_path: Option<PathBuf>,
    discovery: AnalysisDiscovery,
}

/// Owned session parts for runners that need to continue an existing pipeline.
#[derive(Debug)]
pub struct AnalysisSessionParts {
    pub config: ResolvedConfig,
    pub config_path: Option<PathBuf>,
    pub files: Vec<DiscoveredFile>,
}

impl AnalysisSession {
    /// Load config and discover files for a project root.
    ///
    /// # Errors
    ///
    /// Returns an error when config loading fails.
    pub fn load(root: &Path, config_path: Option<&Path>) -> EngineResult<Self> {
        let project_config = config_for_project(root, config_path)?;
        Ok(Self::from_config(project_config))
    }

    /// Load config, apply one caller-supplied config adjustment, then discover
    /// files for a project root.
    ///
    /// # Errors
    ///
    /// Returns an error when config loading fails.
    pub fn load_with_config(
        root: &Path,
        config_path: Option<&Path>,
        configure: impl FnOnce(&mut ResolvedConfig),
    ) -> EngineResult<Self> {
        let mut project_config = config_for_project(root, config_path)?;
        configure(&mut project_config.config);
        Ok(Self::from_config(project_config))
    }

    /// Build a session from built-in defaults, ignoring project config files.
    ///
    /// This is intended for editor fallback paths that have already reported a
    /// config-load warning but should still surface best-effort diagnostics.
    #[must_use]
    pub fn load_default(root: &Path) -> Self {
        Self::from_config(default_project_config(root))
    }

    /// Build a session from a previously resolved config.
    #[must_use]
    pub fn from_config(project_config: ProjectConfig) -> Self {
        let discovery = fallow_core::prepare_analysis_discovery(&project_config.config);
        Self {
            config: project_config.config,
            config_path: project_config.path,
            discovery,
        }
    }

    /// Build a session from a resolved config when the caller already owns
    /// command-specific config loading.
    #[must_use]
    pub fn from_resolved_config(config: ResolvedConfig) -> Self {
        Self::from_config(ProjectConfig { config, path: None })
    }

    /// Resolved project root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.config.root
    }

    /// Resolved project config.
    #[must_use]
    pub fn config(&self) -> &ResolvedConfig {
        &self.config
    }

    /// Config file path when one was loaded.
    #[must_use]
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Discovered files for this session.
    #[must_use]
    pub fn files(&self) -> &[DiscoveredFile] {
        self.discovery.files()
    }

    /// Consume the session and return the resolved config plus discovery data.
    #[must_use]
    pub fn into_parts(self) -> AnalysisSessionParts {
        AnalysisSessionParts {
            config: self.config,
            config_path: self.config_path,
            files: self.discovery.into_files(),
        }
    }

    /// Run dead-code analysis for this session.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code(&self) -> EngineResult<DeadCodeAnalysis> {
        fallow_core::analyze_with_usages_from_discovery(&self.config, &self.discovery)
            .map(|results| DeadCodeAnalysis { results })
            .map_err(engine_error)
    }

    /// Run dead-code analysis with retained complexity artifacts.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code_with_complexity(&self) -> EngineResult<DeadCodeAnalysisOutput> {
        fallow_core::analyze_with_usages_and_complexity_from_discovery(
            &self.config,
            &self.discovery,
        )
        .map(|output| DeadCodeAnalysisOutput {
            results: output.results,
            modules: output.modules,
            files: output.files,
        })
        .map_err(engine_error)
    }

    /// Run dead-code analysis with retained modules, discovered files and graph.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code_with_artifacts(
        &self,
        need_complexity: bool,
        retain_graph: bool,
    ) -> EngineResult<DeadCodeAnalysisArtifacts> {
        fallow_core::analyze_retaining_modules_from_discovery(
            &self.config,
            &self.discovery,
            need_complexity,
            retain_graph,
        )
        .map(dead_code_artifacts)
        .map_err(engine_error)
    }

    /// Run duplication detection using the session's discovered files.
    #[must_use]
    pub fn find_duplicates(&self) -> DuplicationReport {
        find_duplicates(&self.config.root, self.files(), &self.config.duplicates)
    }

    /// Run duplication detection using custom duplicate options.
    #[must_use]
    pub fn find_duplicates_with(&self, config: &DuplicatesConfig) -> DuplicationReport {
        find_duplicates(&self.config.root, self.files(), config)
    }

    /// Run dead-code and duplication analysis for this session.
    ///
    /// When `retain_complexity_artifacts` is true, the dead-code result keeps
    /// parser artifacts needed by editor overlays such as inline complexity.
    ///
    /// # Errors
    ///
    /// Returns an error if dead-code parsing or analysis fails.
    pub fn analyze_project_with(
        &self,
        duplicates_config: &DuplicatesConfig,
        retain_complexity_artifacts: bool,
    ) -> EngineResult<ProjectAnalysisOutput> {
        let dead_code = if retain_complexity_artifacts {
            self.analyze_dead_code_with_complexity()?
        } else {
            let analysis = self.analyze_dead_code()?;
            DeadCodeAnalysisOutput {
                results: analysis.results,
                modules: None,
                files: None,
            }
        };
        let duplication = self.find_duplicates_with(duplicates_config);
        Ok(ProjectAnalysisOutput {
            dead_code,
            duplication,
        })
    }

    /// Run duplication detection and return report sidecar metadata.
    #[must_use]
    pub fn find_duplicates_with_defaults(
        &self,
        config: &DuplicatesConfig,
        cache_dir: Option<&Path>,
    ) -> DuplicationAnalysis {
        find_duplicates_with_defaults(&self.config.root, self.files(), config, cache_dir)
    }

    /// Run focused duplication detection for a changed-file set.
    #[must_use]
    pub fn find_duplicates_touching_files_with_defaults(
        &self,
        config: &DuplicatesConfig,
        changed_files: &[PathBuf],
        cache_dir: Option<&Path>,
    ) -> DuplicationAnalysis {
        find_duplicates_touching_files_with_defaults(
            &self.config.root,
            self.files(),
            config,
            changed_files,
            cache_dir,
        )
    }
}

/// Resolve the analysis config for a project.
///
/// # Errors
///
/// Returns an error when an explicit config cannot be loaded or automatic
/// config discovery finds an invalid config.
pub fn config_for_project(root: &Path, config_path: Option<&Path>) -> EngineResult<ProjectConfig> {
    fallow_core::config_for_project(root, config_path)
        .map(|(config, path)| ProjectConfig { config, path })
        .map_err(engine_error)
}

/// Resolve the parse-cache size limit for a resolved config.
#[must_use]
pub fn resolve_cache_max_size_bytes(config: &ResolvedConfig) -> usize {
    fallow_core::resolve_cache_max_size_bytes(config)
}

fn default_project_config(root: &Path) -> ProjectConfig {
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    ProjectConfig {
        config: FallowConfig::default().resolve(
            root.to_path_buf(),
            OutputFormat::Human,
            threads,
            false,
            true,
            None,
        ),
        path: None,
    }
}

/// Resolve config for a specific analysis without depending on the CLI crate.
///
/// This mirrors the CLI's core config semantics: explicit production overrides
/// are applied before resolution, per-analysis production config is flattened
/// for the requested analysis, and boundary / external plugin / rule-pack
/// validation happens before the resolved config reaches the engine.
///
/// # Errors
///
/// Returns an engine error when config loading or validation fails.
pub fn config_for_project_analysis(
    root: &Path,
    config_path: Option<&Path>,
    options: ProjectConfigOptions,
) -> EngineResult<ProjectConfig> {
    let user_config = load_user_config(root, config_path)?;
    let loaded_user_config = user_config.is_some();
    let (mut config, path) = match user_config {
        Some((config, path)) => (config, Some(path)),
        None => (
            FallowConfig {
                production: options.production_override.unwrap_or(false).into(),
                ..FallowConfig::default()
            },
            None,
        ),
    };

    if loaded_user_config {
        let production = options
            .production_override
            .unwrap_or_else(|| config.production.for_analysis(options.analysis));
        config.production = production.into();
    }
    validate_config(root, &config)?;
    let resolved = config.resolve(
        root.to_path_buf(),
        options.output,
        options.threads,
        options.no_cache,
        options.quiet,
        None,
    );
    Ok(ProjectConfig {
        config: resolved,
        path,
    })
}

fn load_user_config(
    root: &Path,
    config_path: Option<&Path>,
) -> EngineResult<Option<(FallowConfig, PathBuf)>> {
    if let Some(path) = config_path {
        let config = FallowConfig::load(path)
            .map_err(|err| EngineError::new(format!("invalid config: {err:#}")))?;
        return Ok(Some((config, path.to_path_buf())));
    }
    FallowConfig::find_and_load(root)
        .map_err(|err| EngineError::new(format!("invalid config: {err}")))
}

fn validate_config(root: &Path, config: &FallowConfig) -> EngineResult<()> {
    fallow_config::discover_and_validate_external_plugins(root, &config.plugins)
        .map_err(|errors| joined_config_errors("invalid external plugin definition", &errors))?;
    config
        .validate_resolved_boundaries(root)
        .map_err(|errors| joined_config_errors("invalid boundary configuration", &errors))?;
    fallow_config::load_rule_packs(root, &config.rule_packs)
        .map_err(|errors| joined_config_errors("invalid rule pack", &errors))?;
    Ok(())
}

fn joined_config_errors(label: &str, errors: &[impl ToString]) -> EngineError {
    let joined = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n  - ");
    EngineError::new(format!("{label}:\n  - {joined}"))
}

/// Run dead-code analysis for a resolved config.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
pub fn analyze(config: &ResolvedConfig) -> EngineResult<DeadCodeAnalysis> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze(config)
        .map(|results| DeadCodeAnalysis { results })
        .map_err(engine_error)
}

/// Run dead-code analysis with export usage collection for a resolved config.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
pub fn analyze_with_usages(config: &ResolvedConfig) -> EngineResult<DeadCodeAnalysis> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze_with_usages(config)
        .map(|results| DeadCodeAnalysis { results })
        .map_err(engine_error)
}

/// Run dead-code analysis with source hashes for drift-sensitive fixers.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
pub fn analyze_with_file_hashes(
    config: &ResolvedConfig,
) -> EngineResult<DeadCodeAnalysisWithHashes> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze_with_file_hashes(config)
        .map(|output| DeadCodeAnalysisWithHashes {
            results: output.results,
            file_hashes: output.file_hashes,
        })
        .map_err(engine_error)
}

/// Run dead-code analysis with trace timings and retained graph artifacts.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
pub fn analyze_with_trace(config: &ResolvedConfig) -> EngineResult<DeadCodeAnalysisArtifacts> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze_with_trace(config)
        .map(dead_code_artifacts)
        .map_err(engine_error)
}

/// Run dead-code analysis while retaining module and file artifacts.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
pub fn analyze_retaining_modules(
    config: &ResolvedConfig,
    need_complexity: bool,
    retain_graph: bool,
) -> EngineResult<DeadCodeAnalysisArtifacts> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze_retaining_modules(config, need_complexity, retain_graph)
        .map(dead_code_artifacts)
        .map_err(engine_error)
}

/// Run dead-code analysis from pre-parsed modules.
///
/// # Errors
///
/// Returns an error if discovery, graph construction, or analysis fails.
pub fn analyze_with_parse_result(
    config: &ResolvedConfig,
    modules: &[ModuleInfo],
) -> EngineResult<DeadCodeAnalysisArtifacts> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze_with_parse_result(config, modules)
        .map(dead_code_artifacts)
        .map_err(engine_error)
}

/// Run dead-code analysis with export usage and retained complexity artifacts.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
pub fn analyze_with_usages_and_complexity(
    config: &ResolvedConfig,
) -> EngineResult<DeadCodeAnalysisOutput> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze_with_usages_and_complexity(config)
        .map(|output| DeadCodeAnalysisOutput {
            results: output.results,
            modules: output.modules,
            files: output.files,
        })
        .map_err(engine_error)
}

/// Build health shared parse data from retained dead-code artifacts.
#[must_use]
pub fn health_shared_parse_data_from_artifacts(
    results: &AnalysisResults,
    graph: Option<graph::ModuleGraph>,
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

/// Discover source files for a resolved config, including plugin scopes.
#[must_use]
pub fn discover_files_with_plugin_scopes(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    fallow_core::discover::discover_files_with_plugin_scopes(config)
}

/// Run duplication detection on a discovered file set.
#[must_use]
pub fn find_duplicates(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
) -> DuplicationReport {
    fallow_core::duplicates::find_duplicates(root, files, config)
}

/// Resolve changed files for a git ref relative to a project root.
///
/// # Errors
///
/// Returns an error when git cannot resolve the ref or repository state.
pub fn changed_files(
    root: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, fallow_core::changed_files::ChangedFilesError> {
    fallow_core::changed_files::try_get_changed_files(root, git_ref)
}

/// Run symbol-level call-chain tracing through the engine boundary.
///
/// # Errors
///
/// Returns an error if parsing, graph construction, or retained module
/// analysis fails.
pub fn trace_symbol_chain(
    config: &ResolvedConfig,
    query: trace_chain::SymbolChainQuery<'_>,
) -> EngineResult<Option<trace_chain::SymbolChainTrace>> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    let output =
        fallow_core::analyze_retaining_modules(config, true, true).map_err(engine_error)?;
    let graph = output
        .graph
        .as_ref()
        .ok_or_else(|| EngineError::new("trace requires a retained module graph"))?;
    let modules = output.modules.as_deref().unwrap_or(&[]);
    Ok(fallow_core::trace_chain::trace_symbol_chain(
        graph,
        modules,
        &config.root,
        query,
    ))
}

fn dead_code_artifacts(output: fallow_core::AnalysisOutput) -> DeadCodeAnalysisArtifacts {
    DeadCodeAnalysisArtifacts {
        results: output.results,
        timings: output.timings,
        graph: output.graph,
        modules: output.modules,
        files: output.files,
        script_used_packages: output.script_used_packages,
        file_hashes: output.file_hashes,
    }
}

/// Run duplication detection and include metadata about built-in ignored files.
#[must_use]
pub fn find_duplicates_with_defaults(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    cache_dir: Option<&Path>,
) -> DuplicationAnalysis {
    let (report, default_ignore_skips) = if let Some(cache_dir) = cache_dir {
        fallow_core::duplicates::find_duplicates_cached_with_default_ignore_skips(
            root, files, config, cache_dir,
        )
    } else {
        fallow_core::duplicates::find_duplicates_with_default_ignore_skips(root, files, config)
    };
    DuplicationAnalysis {
        report,
        default_ignore_skips,
    }
}

/// Run focused duplication detection and include metadata about built-in ignored files.
#[must_use]
pub fn find_duplicates_touching_files_with_defaults(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    changed_files: &[PathBuf],
    cache_dir: Option<&Path>,
) -> DuplicationAnalysis {
    let changed_files = changed_files.iter().cloned().collect::<FxHashSet<_>>();
    let (report, default_ignore_skips) = if let Some(cache_dir) = cache_dir {
        fallow_core::duplicates::find_duplicates_touching_files_cached_with_default_ignore_skips(
            root,
            files,
            config,
            &changed_files,
            cache_dir,
        )
    } else {
        fallow_core::duplicates::find_duplicates_touching_files_with_default_ignore_skips(
            root,
            files,
            config,
            &changed_files,
        )
    };
    DuplicationAnalysis {
        report,
        default_ignore_skips,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let trace = trace_symbol_chain(
            &project_config.config,
            trace_chain::SymbolChainQuery {
                file: "src/util.ts",
                symbol: "helper",
                depth: 1,
                directions: trace_chain::TraceDirections {
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
