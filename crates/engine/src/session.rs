//! Engine-owned analysis session orchestration.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use fallow_config::{DuplicatesConfig, ResolvedConfig, WorkspaceInfo};
use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::ModuleInfo;
use fallow_types::source_fingerprint::SourceFingerprint;
use fallow_types::workspace::WorkspaceDiagnostic;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    EngineResult, core_backend, duplicates,
    project_analysis::{
        ProjectAnalysisArtifactOptions, ProjectAnalysisArtifacts, ProjectAnalysisOutput,
    },
    project_config::{ProjectConfig, config_for_project, default_project_config},
    results::{
        DeadCodeAnalysis, DeadCodeAnalysisArtifacts, DeadCodeAnalysisOutput, DuplicationAnalysis,
        SharedDeadCodeAnalysisArtifacts,
    },
};

/// Reusable engine session for one resolved project.
///
/// The session owns the resolved config and discovered file set so future
/// consumers can share graph-sensitive inputs without each surface recreating
/// its own partial orchestration.
#[derive(Debug)]
pub struct AnalysisSession {
    config: ResolvedConfig,
    config_path: Option<PathBuf>,
    discovery: crate::discover::AnalysisDiscovery,
    workspaces: Vec<WorkspaceInfo>,
    workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    parsed_cache: Mutex<Option<ParsedModuleCache>>,
    styling_cache: Mutex<Option<crate::health::StylingAnalysisArtifacts>>,
}

#[derive(Debug)]
struct ParsedModuleCache {
    need_complexity: bool,
    fingerprints: Vec<SourceFingerprint>,
    modules: Arc<[ModuleInfo]>,
}

/// Owned session parts for runners that need to continue an existing pipeline.
#[derive(Debug)]
pub struct AnalysisSessionParts {
    pub config: ResolvedConfig,
    pub config_path: Option<PathBuf>,
    pub files: Vec<DiscoveredFile>,
    pub workspaces: Vec<WorkspaceInfo>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

/// Owned session parts after parsing the discovered files.
#[derive(Debug)]
pub struct ParsedAnalysisSessionParts {
    pub config: ResolvedConfig,
    pub config_path: Option<PathBuf>,
    pub files: Vec<DiscoveredFile>,
    pub modules: Vec<ModuleInfo>,
    pub workspaces: Vec<WorkspaceInfo>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub parse_ms: f64,
    pub cache_update_ms: f64,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub parse_cpu_ms: f64,
}

/// Reusable artifacts produced by one session-owned dead-code run.
#[derive(Debug)]
pub struct AnalysisSessionArtifacts {
    pub analysis: DeadCodeAnalysisArtifacts,
    pub changed_files: Option<FxHashSet<PathBuf>>,
    pub source_fingerprints: FxHashMap<PathBuf, SourceFingerprint>,
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
        Self::load_with_config_options(
            root,
            config_path,
            fallow_config::ConfigLoadOptions::default(),
            configure,
        )
    }

    /// Load config with an explicit inheritance trust policy, apply one
    /// caller-supplied adjustment, then discover project files.
    ///
    /// # Errors
    ///
    /// Returns an error when config loading fails.
    pub fn load_with_config_options(
        root: &Path,
        config_path: Option<&Path>,
        load_options: fallow_config::ConfigLoadOptions,
        configure: impl FnOnce(&mut ResolvedConfig),
    ) -> EngineResult<Self> {
        let mut project_config = crate::project_config::config_for_project_with_load_options(
            root,
            config_path,
            load_options,
        )?;
        configure(&mut project_config.config);
        project_config.workspaces.clear();
        project_config.workspace_diagnostics.clear();
        project_config.workspace_discovery_ms = None;
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
        let uses_preloaded_workspaces = project_config.workspace_discovery_ms.is_some();
        let discovery = if let Some(workspace_discovery_ms) = project_config.workspace_discovery_ms
        {
            crate::discover::prepare_analysis_discovery_with_workspaces(
                &project_config.config,
                &project_config.workspaces,
                workspace_discovery_ms,
            )
        } else {
            crate::discover::prepare_analysis_discovery(&project_config.config)
        };
        let workspaces = if uses_preloaded_workspaces {
            project_config.workspaces
        } else {
            discovery.workspaces().to_vec()
        };
        let workspace_diagnostics = merge_workspace_diagnostics(
            project_config.workspace_diagnostics,
            fallow_config::workspace_diagnostics_for(&project_config.config.root),
        );
        Self {
            config: project_config.config,
            config_path: project_config.path,
            discovery,
            workspaces,
            workspace_diagnostics,
            parsed_cache: Mutex::new(None),
            styling_cache: Mutex::new(None),
        }
    }

    /// Build a session from a resolved config when the caller already owns
    /// command-specific config loading.
    #[must_use]
    pub fn from_resolved_config(config: ResolvedConfig) -> Self {
        Self::from_config(ProjectConfig {
            config,
            path: None,
            workspaces: Vec::new(),
            workspace_diagnostics: Vec::new(),
            workspace_discovery_ms: None,
        })
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

    /// Workspace packages discovered during config/session setup.
    #[must_use]
    pub fn workspaces(&self) -> &[WorkspaceInfo] {
        &self.workspaces
    }

    /// Source metadata fingerprints for every discovered source file.
    #[must_use]
    pub fn source_fingerprints(&self) -> FxHashMap<PathBuf, SourceFingerprint> {
        self.discovery
            .files()
            .iter()
            .map(|file| {
                let fingerprint = std::fs::metadata(&file.path).map_or_else(
                    |_| SourceFingerprint::new(0, file.size_bytes),
                    |metadata| SourceFingerprint::from_metadata(&metadata),
                );
                (file.path.clone(), fingerprint)
            })
            .collect()
    }

    /// Resolve files changed since a git ref against this session root.
    ///
    /// # Errors
    ///
    /// Returns an error when the ref is invalid, git is unavailable, or the
    /// root is not part of a repository.
    pub fn changed_files_since(
        &self,
        git_ref: &str,
    ) -> Result<FxHashSet<PathBuf>, crate::changed_files::ChangedFilesError> {
        crate::changed_files::changed_files(&self.config.root, git_ref)
    }

    /// Workspace and source-discovery diagnostics captured for this session.
    #[must_use]
    pub fn workspace_diagnostics(&self) -> &[WorkspaceDiagnostic] {
        &self.workspace_diagnostics
    }

    pub(crate) fn styling_analysis_artifacts(&self) -> crate::health::StylingAnalysisArtifacts {
        if let Ok(cache) = self.styling_cache.lock()
            && let Some(artifacts) = cache.as_ref()
        {
            return artifacts.clone();
        }

        let artifacts =
            crate::health::build_styling_analysis_artifacts(self.files(), self.config());
        if let Ok(mut cache) = self.styling_cache.lock() {
            *cache = Some(artifacts.clone());
        }
        artifacts
    }

    /// Consume the session and return the resolved config plus discovery data.
    #[must_use]
    pub fn into_parts(self) -> AnalysisSessionParts {
        AnalysisSessionParts {
            config: self.config,
            config_path: self.config_path,
            files: self.discovery.into_files(),
            workspaces: self.workspaces,
            workspace_diagnostics: self.workspace_diagnostics,
        }
    }

    /// Consume the session, load the parser cache, and parse discovered files.
    #[must_use]
    pub fn into_parsed_parts(self, need_complexity: bool) -> ParsedAnalysisSessionParts {
        let AnalysisSessionParts {
            config,
            config_path,
            files,
            workspaces,
            workspace_diagnostics,
        } = self.into_parts();
        let ParsedModules { modules, metrics } =
            parse_files_with_config(&config, &files, need_complexity);
        ParsedAnalysisSessionParts {
            config,
            config_path,
            files,
            modules,
            workspaces,
            workspace_diagnostics,
            parse_ms: metrics.parse_ms,
            cache_update_ms: metrics.cache_ms,
            cache_hits: metrics.cache_hits,
            cache_misses: metrics.cache_misses,
            parse_cpu_ms: metrics.parse_cpu_ms,
        }
    }

    /// Parse discovered files without consuming the session.
    #[must_use]
    pub fn parsed_parts(&self, need_complexity: bool) -> ParsedAnalysisSessionParts {
        let SharedParsedModules { modules, metrics } = self.parse_modules(need_complexity);
        self.parsed_parts_from_modules(modules.to_vec(), metrics)
    }

    /// Return immutable parsed modules backed by the reusable session cache.
    ///
    /// Workspace-owned consumers use this additive path when they only need
    /// parsed modules and can borrow discovery and config directly from the
    /// session. Stable owned callers can continue using [`Self::parsed_parts`].
    #[doc(hidden)]
    #[must_use]
    pub fn shared_parsed_modules(&self, need_complexity: bool) -> Arc<[ModuleInfo]> {
        self.parse_modules(need_complexity).modules
    }

    /// Parse discovered files without consuming the session or retaining parser
    /// output in the session cache.
    #[must_use]
    pub fn parsed_parts_uncached(&self, need_complexity: bool) -> ParsedAnalysisSessionParts {
        let ParsedModules { modules, metrics } =
            parse_files_with_config(&self.config, self.files(), need_complexity);
        self.parsed_parts_from_modules(modules, metrics)
    }

    fn parsed_parts_from_modules(
        &self,
        modules: Vec<ModuleInfo>,
        metrics: core_backend::ParseMetrics,
    ) -> ParsedAnalysisSessionParts {
        ParsedAnalysisSessionParts {
            config: self.config.clone(),
            config_path: self.config_path.clone(),
            files: self.discovery.files().to_vec(),
            modules,
            workspaces: self.workspaces.clone(),
            workspace_diagnostics: self.workspace_diagnostics.clone(),
            parse_ms: metrics.parse_ms,
            cache_update_ms: metrics.cache_ms,
            cache_hits: metrics.cache_hits,
            cache_misses: metrics.cache_misses,
            parse_cpu_ms: metrics.parse_cpu_ms,
        }
    }

    /// Run dead-code analysis for this session.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code(&self) -> EngineResult<DeadCodeAnalysis> {
        self.analyze_dead_code_with_artifacts(false, false)
            .map(|output| DeadCodeAnalysis {
                results: output.results,
            })
    }

    /// Run dead-code analysis with retained complexity artifacts.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code_with_complexity(&self) -> EngineResult<DeadCodeAnalysisOutput> {
        self.analyze_dead_code_with_artifacts(true, false)
            .map(|output| DeadCodeAnalysisOutput {
                results: output.results,
                modules: output.modules,
                files: output.files,
            })
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
        self.analyze_dead_code_with_shared_artifacts(need_complexity, retain_graph)
            .map(SharedDeadCodeAnalysisArtifacts::into_owned)
    }

    /// Run dead-code analysis with shared immutable parser artifacts.
    ///
    /// Workspace-owned consumers use this additive path to retain warm parser
    /// modules without deep-cloning the session cache. External callers can
    /// continue using [`Self::analyze_dead_code_with_artifacts`].
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    #[doc(hidden)]
    pub fn analyze_dead_code_with_shared_artifacts(
        &self,
        need_complexity: bool,
        retain_graph: bool,
    ) -> EngineResult<SharedDeadCodeAnalysisArtifacts> {
        self.analyze_dead_code_with_reuse_artifacts(need_complexity, retain_graph, need_complexity)
    }

    /// Run dead-code analysis while retaining discovered files for downstream
    /// command stages that reuse discovery but do not need parser modules.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code_retaining_files(
        &self,
        need_complexity: bool,
        retain_graph: bool,
    ) -> EngineResult<DeadCodeAnalysisArtifacts> {
        self.analyze_dead_code_with_reuse_artifacts(need_complexity, retain_graph, true)
            .map(SharedDeadCodeAnalysisArtifacts::into_owned)
    }

    /// Run dead-code analysis from modules already parsed through this session.
    ///
    /// This preserves the session's resolved config and discovered file set for
    /// follow-up analyses that reuse parser output without redoing discovery.
    ///
    /// # Errors
    ///
    /// Returns an error if graph construction or analysis fails.
    pub fn analyze_dead_code_with_parsed_modules(
        &self,
        modules: &[ModuleInfo],
    ) -> EngineResult<DeadCodeAnalysisArtifacts> {
        self.analyze_dead_code_with_shared_modules(Arc::from(modules))
    }

    /// Run dead-code analysis from shared immutable parser modules.
    ///
    /// # Errors
    ///
    /// Returns an error if graph construction or analysis fails.
    #[doc(hidden)]
    pub fn analyze_dead_code_with_shared_modules(
        &self,
        modules: Arc<[ModuleInfo]>,
    ) -> EngineResult<DeadCodeAnalysisArtifacts> {
        run_engine_owned_dead_code_pipeline(EngineDeadCodePipelineInput {
            config: &self.config,
            discovery: &self.discovery,
            modules,
            metrics: reused_parse_metrics(),
            collect_usages: true,
            retain_graph: true,
            retain_modules: false,
            retain_files: false,
        })
        .map(SharedDeadCodeAnalysisArtifacts::into_owned)
    }

    fn analyze_dead_code_with_reuse_artifacts(
        &self,
        need_complexity: bool,
        retain_graph: bool,
        retain_files: bool,
    ) -> EngineResult<SharedDeadCodeAnalysisArtifacts> {
        let SharedParsedModules { modules, metrics } = self.parse_modules(need_complexity);
        run_engine_owned_dead_code_pipeline(EngineDeadCodePipelineInput {
            config: &self.config,
            discovery: &self.discovery,
            modules,
            metrics,
            collect_usages: true,
            retain_graph,
            retain_modules: need_complexity,
            retain_files,
        })
    }

    /// Run dead-code analysis and return the session-scoped reuse artifacts.
    ///
    /// Callers pass a changed-file set they have already resolved for the
    /// command. The returned value keeps that set beside parser, graph, and
    /// source-fingerprint data so downstream runners do not have to rebuild or
    /// rediscover the same inputs.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code_with_session_artifacts(
        &self,
        need_complexity: bool,
        retain_graph: bool,
        changed_files: Option<FxHashSet<PathBuf>>,
    ) -> EngineResult<AnalysisSessionArtifacts> {
        Ok(AnalysisSessionArtifacts {
            analysis: self.analyze_dead_code_with_artifacts(need_complexity, retain_graph)?,
            changed_files,
            source_fingerprints: self.source_fingerprints(),
        })
    }

    /// Run duplication detection using the session's discovered files.
    #[must_use]
    pub fn find_duplicates(&self) -> duplicates::DuplicationReport {
        duplicates::find_duplicates(&self.config.root, self.files(), &self.config.duplicates)
    }

    /// Run duplication detection using custom duplicate options.
    #[must_use]
    pub fn find_duplicates_with(&self, config: &DuplicatesConfig) -> duplicates::DuplicationReport {
        duplicates::find_duplicates(&self.config.root, self.files(), config)
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
        self.analyze_project_with_artifacts(
            duplicates_config,
            ProjectAnalysisArtifactOptions {
                retain_complexity_artifacts,
                ..ProjectAnalysisArtifactOptions::default()
            },
        )
        .map(ProjectAnalysisArtifacts::into_output)
    }

    /// Run dead-code and duplication analysis with retained session reuse data.
    ///
    /// This is the engine-owned project artifact boundary for callers that need
    /// to hand one analysis result across audit, decision, editor, or follow-up
    /// analysis surfaces without rediscovering session metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if dead-code parsing or analysis fails.
    pub fn analyze_project_with_artifacts(
        &self,
        duplicates_config: &DuplicatesConfig,
        options: ProjectAnalysisArtifactOptions,
    ) -> EngineResult<ProjectAnalysisArtifacts> {
        let cache_dir = (!self.config.no_cache).then_some(self.config.cache_dir.as_path());
        let duplication = if let Some(changed_files) = options.changed_files.as_ref() {
            let changed_files = changed_files.iter().cloned().collect::<Vec<_>>();
            self.find_duplicates_touching_files_with_defaults(
                duplicates_config,
                &changed_files,
                cache_dir,
            )
            .report
        } else {
            self.find_duplicates_with_defaults(duplicates_config, cache_dir)
                .report
        };
        let source_fingerprints = options
            .collect_source_fingerprints
            .then(|| self.source_fingerprints());
        Ok(ProjectAnalysisArtifacts {
            dead_code: self.analyze_dead_code_with_artifacts(
                options.retain_complexity_artifacts,
                options.retain_graph,
            )?,
            duplication,
            changed_files: options.changed_files,
            source_fingerprints,
        })
    }

    /// Run duplication detection and return report sidecar metadata.
    #[must_use]
    pub fn find_duplicates_with_defaults(
        &self,
        config: &DuplicatesConfig,
        cache_dir: Option<&Path>,
    ) -> DuplicationAnalysis {
        duplicates::find_duplicates_with_defaults(
            &self.config.root,
            self.files(),
            config,
            cache_dir,
        )
    }

    /// Run focused duplication detection for a changed-file set.
    #[must_use]
    pub fn find_duplicates_touching_files_with_defaults(
        &self,
        config: &DuplicatesConfig,
        changed_files: &[PathBuf],
        cache_dir: Option<&Path>,
    ) -> DuplicationAnalysis {
        duplicates::find_duplicates_touching_files_with_defaults(
            &self.config.root,
            self.files(),
            config,
            changed_files,
            cache_dir,
        )
    }

    fn parse_modules(&self, need_complexity: bool) -> SharedParsedModules {
        let fingerprints = source_fingerprints_for_files(self.files());
        if let Some(fingerprints) = fingerprints.as_ref()
            && let Some(modules) = self.cached_modules(need_complexity, fingerprints)
        {
            return SharedParsedModules {
                modules,
                metrics: core_backend::ParseMetrics {
                    parse_ms: 0.0,
                    cache_ms: 0.0,
                    cache_hits: 0,
                    cache_misses: 0,
                    parse_cpu_ms: 0.0,
                },
            };
        }

        let ParsedModules { modules, metrics } =
            parse_files_with_config(&self.config, self.files(), need_complexity);
        let modules: Arc<[ModuleInfo]> = modules.into();
        if let Some(fingerprints) = fingerprints
            && let Ok(mut cache) = self.parsed_cache.lock()
        {
            *cache = Some(ParsedModuleCache {
                need_complexity,
                fingerprints,
                modules: Arc::clone(&modules),
            });
        }
        SharedParsedModules { modules, metrics }
    }

    fn cached_modules(
        &self,
        need_complexity: bool,
        fingerprints: &[SourceFingerprint],
    ) -> Option<Arc<[ModuleInfo]>> {
        let Ok(cache) = self.parsed_cache.lock() else {
            return None;
        };
        let cache = cache.as_ref()?;
        let complexity_mode_satisfies_request = cache.need_complexity || !need_complexity;
        if complexity_mode_satisfies_request && cache.fingerprints == fingerprints {
            return Some(Arc::clone(&cache.modules));
        }
        None
    }
}

fn merge_workspace_diagnostics(
    primary: Vec<WorkspaceDiagnostic>,
    secondary: Vec<WorkspaceDiagnostic>,
) -> Vec<WorkspaceDiagnostic> {
    let mut merged = Vec::with_capacity(primary.len() + secondary.len());
    let mut seen: FxHashSet<(String, PathBuf)> = FxHashSet::default();
    for diagnostic in primary.into_iter().chain(secondary) {
        let key = (diagnostic.kind.id().to_owned(), diagnostic.path.clone());
        if seen.insert(key) {
            merged.push(diagnostic);
        }
    }
    merged
}

struct ParsedModules {
    modules: Vec<ModuleInfo>,
    metrics: core_backend::ParseMetrics,
}

struct SharedParsedModules {
    modules: Arc<[ModuleInfo]>,
    metrics: core_backend::ParseMetrics,
}

fn parse_files_with_config(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    need_complexity: bool,
) -> ParsedModules {
    let parse_start = Instant::now();
    let cache_max_size_bytes = crate::project_config::resolve_cache_max_size_bytes(config);
    let mut cache = if config.no_cache {
        None
    } else {
        fallow_extract::cache::CacheStore::load(
            &config.cache_dir,
            config.cache_config_hash,
            cache_max_size_bytes,
        )
    };
    let parse_result = crate::source::parse_all_files(files, cache.as_ref(), need_complexity);
    let mut modules = parse_result.modules;
    for module in &mut modules {
        module.prepare_analysis_facts();
    }
    let parse_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
    let cache_ms = update_parse_cache_if_enabled(config, &mut cache, &modules, files);
    let metrics = core_backend::ParseMetrics {
        parse_ms,
        cache_ms,
        cache_hits: parse_result.cache_hits,
        cache_misses: parse_result.cache_misses,
        parse_cpu_ms: parse_result.parse_cpu_ms,
    };
    ParsedModules { modules, metrics }
}

fn reused_parse_metrics() -> core_backend::ParseMetrics {
    core_backend::ParseMetrics {
        parse_ms: 0.0,
        cache_ms: 0.0,
        cache_hits: 0,
        cache_misses: 0,
        parse_cpu_ms: 0.0,
    }
}

fn source_fingerprints_for_files(files: &[DiscoveredFile]) -> Option<Vec<SourceFingerprint>> {
    files
        .iter()
        .map(|file| {
            std::fs::metadata(&file.path)
                .ok()
                .map(|metadata| SourceFingerprint::from_metadata(&metadata))
                .filter(|fingerprint| fingerprint.has_known_mtime())
        })
        .collect()
}

fn update_parse_cache_if_enabled(
    config: &ResolvedConfig,
    cache: &mut Option<fallow_extract::cache::CacheStore>,
    modules: &[ModuleInfo],
    files: &[DiscoveredFile],
) -> f64 {
    let start = Instant::now();
    if config.no_cache {
        return start.elapsed().as_secs_f64() * 1000.0;
    }

    let cache_max_size_bytes = crate::project_config::resolve_cache_max_size_bytes(config);
    let store = cache.get_or_insert_with(fallow_extract::cache::CacheStore::new);
    if update_parse_cache(store, modules, files)
        && let Err(error) = store.save(
            &config.cache_dir,
            config.cache_config_hash,
            cache_max_size_bytes,
        )
    {
        tracing::warn!("Failed to save cache: {error}");
    }
    start.elapsed().as_secs_f64() * 1000.0
}

fn update_parse_cache(
    store: &mut fallow_extract::cache::CacheStore,
    modules: &[ModuleInfo],
    files: &[DiscoveredFile],
) -> bool {
    let mut dirty = false;
    for module in modules {
        if let Some(file) = files.get(module.file_id.0 as usize) {
            let fingerprint = source_fingerprint(&file.path);
            if let Some(cached) = store.get_by_path_only(&file.path)
                && cached.content_hash == module.content_hash
            {
                if cached.source_fingerprint() != fingerprint {
                    let preserved_last_access = cached.last_access_secs;
                    let mut refreshed =
                        fallow_extract::cache::module_to_cached(module, fingerprint);
                    refreshed.last_access_secs = preserved_last_access;
                    store.insert(&file.path, refreshed);
                    dirty = true;
                }
                continue;
            }
            store.insert(
                &file.path,
                fallow_extract::cache::module_to_cached(module, fingerprint),
            );
            dirty = true;
        }
    }
    store.retain_paths(files) || dirty
}

fn source_fingerprint(path: &Path) -> SourceFingerprint {
    std::fs::metadata(path).map_or_else(
        |_| SourceFingerprint::new(0, 0),
        |metadata| SourceFingerprint::from_metadata(&metadata),
    )
}

struct EngineDeadCodePipelineInput<'a> {
    config: &'a ResolvedConfig,
    discovery: &'a crate::discover::AnalysisDiscovery,
    modules: Arc<[ModuleInfo]>,
    metrics: core_backend::ParseMetrics,
    collect_usages: bool,
    retain_graph: bool,
    retain_modules: bool,
    retain_files: bool,
}

fn run_engine_owned_dead_code_pipeline(
    input: EngineDeadCodePipelineInput<'_>,
) -> EngineResult<SharedDeadCodeAnalysisArtifacts> {
    let EngineDeadCodePipelineInput {
        config,
        discovery,
        modules,
        metrics,
        collect_usages,
        retain_graph,
        retain_modules,
        retain_files,
    } = input;
    let prelude = core_backend::prepare_dead_code_backend_prelude(config, discovery)?;
    let prelude_timings = prelude.timings();
    let entry_points = core_backend::discover_dead_code_entry_points(&prelude);
    let (resolved, graph) = resolve_or_build_dead_code_graph(&prelude, &entry_points, &modules);

    let detector = core_backend::run_dead_code_detectors(
        &prelude,
        &graph.graph,
        &resolved.resolved,
        &modules,
        collect_usages,
        &entry_points,
    );
    let profile =
        core_backend::dead_code_pipeline_profile(core_backend::DeadCodePipelineProfileInput {
            retain_timings: retain_graph,
            prelude: &prelude,
            prelude_timings,
            parse_metrics: metrics,
            module_count: modules.len(),
            entry_points: &entry_points,
            resolved: &resolved,
            graph: &graph,
            detector: &detector,
            file_count: discovery.files().len(),
            workspace_count: discovery.workspaces().len(),
        });
    let script_used_packages = prelude.script_used_packages();
    prelude.finish();
    let file_hashes = collect_file_hashes(&modules, discovery.files());

    Ok(SharedDeadCodeAnalysisArtifacts {
        results: detector.results,
        timings: profile.timings,
        graph: retain_graph.then_some(graph.graph),
        modules: retain_modules.then_some(modules),
        files: retain_files.then(|| discovery.files().to_vec()),
        script_used_packages,
        file_hashes,
    })
}

fn resolve_or_build_dead_code_graph(
    prelude: &core_backend::DeadCodeBackendPrelude,
    entry_points: &core_backend::DeadCodeEntryPoints,
    modules: &[ModuleInfo],
) -> (
    core_backend::DeadCodeResolvedModules,
    core_backend::DeadCodeGraphRun,
) {
    if let Some((resolved, graph)) =
        core_backend::try_load_dead_code_graph_cache(prelude, entry_points, modules)
    {
        return (resolved, graph);
    }

    let resolved = core_backend::resolve_dead_code_imports(prelude, modules);
    let graph =
        core_backend::build_dead_code_graph(prelude, &resolved.resolved, entry_points, modules);
    (resolved, graph)
}

fn collect_file_hashes(
    modules: &[ModuleInfo],
    files: &[DiscoveredFile],
) -> FxHashMap<PathBuf, u64> {
    modules
        .iter()
        .filter_map(|module| {
            files
                .get(module.file_id.0 as usize)
                .map(|file| (file.path.clone(), module.content_hash))
        })
        .collect()
}

pub(crate) fn analyze_dead_code_with_parse_result_from_config(
    config: &ResolvedConfig,
    modules: &[ModuleInfo],
) -> EngineResult<DeadCodeAnalysisArtifacts> {
    let discovery = crate::discover::prepare_analysis_discovery(config);
    run_engine_owned_dead_code_pipeline(EngineDeadCodePipelineInput {
        config,
        discovery: &discovery,
        modules: Arc::from(modules),
        metrics: reused_parse_metrics(),
        collect_usages: true,
        retain_graph: true,
        retain_modules: false,
        retain_files: false,
    })
    .map(SharedDeadCodeAnalysisArtifacts::into_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with_source(source: &str) -> (tempfile::TempDir, AnalysisSession) {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("create source directory");
        std::fs::write(root.join("src/index.ts"), source).expect("write source");
        let session = AnalysisSession::load_default(root);
        (project, session)
    }

    #[test]
    fn session_retains_workspace_metadata_from_config_load() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        )
        .expect("write root package");
        std::fs::create_dir_all(root.join("packages/a")).expect("create workspace");
        std::fs::write(
            root.join("packages/a/package.json"),
            r#"{"name":"pkg-a","type":"module"}"#,
        )
        .expect("write workspace package");

        let session = AnalysisSession::load(root, None).expect("session loads");

        assert!(
            session
                .workspaces()
                .iter()
                .any(|workspace| workspace.name == "pkg-a"),
            "session must retain workspace metadata discovered during config load"
        );
    }

    #[test]
    fn warm_parse_cache_reuses_module_storage() {
        let (_project, session) = session_with_source("export function value() { return 1; }\n");
        let first = session.parse_modules(true);
        let second = session.parse_modules(false);

        assert!(
            Arc::ptr_eq(&first.modules, &second.modules),
            "warm session queries must share parsed module storage"
        );
    }

    #[test]
    fn shared_parsed_modules_reuse_public_session_storage() {
        let (_project, session) = session_with_source("export const value = 1;\n");
        let first = session.shared_parsed_modules(true);
        let second = session.shared_parsed_modules(false);

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn warm_complexity_artifacts_reuse_cached_module_storage() {
        let (_project, session) = session_with_source("export function value() { return 1; }\n");
        let cached = session.parse_modules(true);
        let artifacts = session
            .analyze_dead_code_with_reuse_artifacts(true, true, false)
            .expect("analysis succeeds");
        let retained = artifacts.modules.expect("complexity modules retained");

        assert!(
            Arc::ptr_eq(&cached.modules, &retained),
            "warm complexity artifacts must share parsed module storage"
        );
    }

    #[test]
    fn shared_and_owned_artifacts_preserve_output_bytes() {
        let (_project, session) = session_with_source(
            "export const used = 1;\nexport const unused = 2;\nconsole.log(used);\n",
        );
        let owned = session
            .analyze_dead_code_with_artifacts(true, true)
            .expect("owned analysis succeeds");
        let shared = session
            .analyze_dead_code_with_shared_artifacts(true, true)
            .expect("shared analysis succeeds");

        assert_eq!(
            serde_json::to_vec(&owned.results).expect("serialize owned results"),
            serde_json::to_vec(&shared.results).expect("serialize shared results")
        );
        assert_eq!(owned.file_hashes, shared.file_hashes);
        assert_eq!(
            owned
                .modules
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|module| module.content_hash)
                .collect::<Vec<_>>(),
            shared
                .modules
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|module| module.content_hash)
                .collect::<Vec<_>>()
        );
    }
}
