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
use rustc_hash::FxHashSet;

/// Duplication result types exposed through the engine boundary.
pub mod duplicates {
    pub use fallow_core::duplicates::*;
}

pub mod dead_code;
pub mod health;

/// Extracted semantic types exposed through the engine boundary.
pub mod extract {
    pub use fallow_types::extract::*;
}

/// Module graph types exposed through the engine boundary.
pub mod graph {
    pub use fallow_core::graph::*;
}

/// Public API graph helpers exposed through the engine boundary.
pub mod public_api {
    pub use fallow_core::analyze::public_api_package_entry_points;
}

/// Git process environment helpers exposed through the engine boundary.
pub mod git_env {
    pub use fallow_core::git_env::{AMBIENT_GIT_ENV_VARS, clear_ambient_git_env};
}

/// Analysis result types exposed through the engine boundary.
pub mod results {
    pub use fallow_core::results::*;
}

/// Suppression helpers exposed for editor and embedding surfaces.
pub mod suppress {
    pub use fallow_core::suppress::{IssueKind, is_suppressed};
}

/// Changed-file helpers exposed through the engine boundary for editor and
/// embedding surfaces.
pub mod changed_files {
    pub use fallow_core::changed_files::{
        ChangedFilesError, filter_duplication_by_changed_files, filter_results_by_changed_files,
        get_changed_files, resolve_git_common_dir, resolve_git_toplevel, try_get_changed_diff,
        try_get_changed_files_with_toplevel, validate_git_ref,
    };
}

/// Git churn helpers and types exposed through the engine boundary.
pub mod churn {
    pub use fallow_core::churn::*;
}

/// Security metadata helpers exposed through the engine boundary.
pub mod security {
    pub use fallow_core::analyze::security_catalogue_title;
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

/// Typed dead-code analysis result with retained parser artifacts.
#[derive(Debug)]
pub struct DeadCodeAnalysisOutput {
    pub results: AnalysisResults,
    pub modules: Option<Vec<ModuleInfo>>,
    pub files: Option<Vec<DiscoveredFile>>,
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
    files: Vec<DiscoveredFile>,
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
        let files = discover_files_with_plugin_scopes(&project_config.config);
        Self {
            config: project_config.config,
            config_path: project_config.path,
            files,
        }
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
        &self.files
    }

    /// Run dead-code analysis for this session.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code(&self) -> EngineResult<DeadCodeAnalysis> {
        analyze_with_usages(&self.config)
    }

    /// Run dead-code analysis with retained complexity artifacts.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code_with_complexity(&self) -> EngineResult<DeadCodeAnalysisOutput> {
        analyze_with_usages_and_complexity(&self.config)
    }

    /// Run duplication detection using the session's discovered files.
    #[must_use]
    pub fn find_duplicates(&self) -> DuplicationReport {
        find_duplicates(&self.config.root, &self.files, &self.config.duplicates)
    }

    /// Run duplication detection using custom duplicate options.
    #[must_use]
    pub fn find_duplicates_with(&self, config: &DuplicatesConfig) -> DuplicationReport {
        find_duplicates(&self.config.root, &self.files, config)
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
        find_duplicates_with_defaults(&self.config.root, &self.files, config, cache_dir)
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
            &self.files,
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
