//! Engine-owned analysis session orchestration.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use fallow_config::{DuplicatesConfig, ResolvedConfig, WorkspaceInfo};
use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::ModuleInfo;
#[cfg(test)]
use fallow_types::results::AnalysisResults;
use fallow_types::source_fingerprint::SourceFingerprint;
use fallow_types::workspace::{WorkspaceDiagnostic, merge_workspace_diagnostics};
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
    styling_cache: Mutex<Option<Arc<crate::health::StylingAnalysisArtifacts>>>,
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
    /// Resolved project config the session was created with.
    pub config: ResolvedConfig,
    /// Path of the loaded config file; `None` when defaults were used.
    pub config_path: Option<PathBuf>,
    /// Files discovered under the session root.
    pub files: Vec<DiscoveredFile>,
    /// Workspace metadata discovered during config resolution.
    pub workspaces: Vec<WorkspaceInfo>,
    /// Diagnostics from workspace discovery (undeclared or invalid members).
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

/// Owned session parts after parsing the discovered files.
#[derive(Debug)]
pub struct ParsedAnalysisSessionParts {
    /// Resolved project config the session was created with.
    pub config: ResolvedConfig,
    /// Path of the loaded config file; `None` when defaults were used.
    pub config_path: Option<PathBuf>,
    /// Files discovered under the session root.
    pub files: Vec<DiscoveredFile>,
    /// Parsed modules, one per discovered file.
    pub modules: Vec<ModuleInfo>,
    /// Workspace metadata discovered during config resolution.
    pub workspaces: Vec<WorkspaceInfo>,
    /// Diagnostics from workspace discovery (undeclared or invalid members).
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// Parse wall time in milliseconds.
    pub parse_ms: f64,
    /// Parse-cache write-back wall time in milliseconds.
    pub cache_update_ms: f64,
    /// Files served from the parse cache.
    pub cache_hits: usize,
    /// Files that had to be parsed fresh.
    pub cache_misses: usize,
    /// Summed parse CPU time across rayon workers in milliseconds.
    pub parse_cpu_ms: f64,
}

#[derive(Debug)]
pub(crate) struct SharedParsedAnalysisSessionParts {
    pub(crate) config: ResolvedConfig,
    pub(crate) files: Vec<DiscoveredFile>,
    pub(crate) modules: Arc<[ModuleInfo]>,
    pub workspaces: Vec<WorkspaceInfo>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub parse_ms: f64,
    pub parse_cpu_ms: f64,
}

/// Reusable artifacts produced by one session-owned dead-code run.
#[derive(Debug)]
pub struct AnalysisSessionArtifacts {
    /// Retained dead-code analysis output (results, graph, timings).
    pub analysis: DeadCodeAnalysisArtifacts,
    /// Diff scope the run was limited to, when one was resolved.
    pub changed_files: Option<FxHashSet<PathBuf>>,
    /// Per-file source fingerprints for downstream cache invalidation.
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
        // Analysis-stage diagnostics are owned by the analyze pass, which
        // refreshes the registry on every run; pinning them in the session
        // snapshot would keep a stale entry alive after the cause is fixed
        // (issue #2366). `current_workspace_diagnostics` reads them live.
        //
        // Source-discovery entries come from THIS walk's return value, not from
        // the registry: combined mode runs the dead-code and duplication walks
        // concurrently whenever a per-analysis `production` split stops them
        // from sharing a file list, and each walk replaces the registry's
        // source-discovery set for the root, so a registry read here would
        // report whichever walk happened to write last (issue #2366).
        let workspace_diagnostics = merge_workspace_diagnostics(
            merge_workspace_diagnostics(
                project_config.workspace_diagnostics,
                fallow_config::workspace_diagnostics_for(&project_config.config.root)
                    .into_iter()
                    .filter(|diagnostic| {
                        !diagnostic.kind.is_analysis_stage()
                            && !diagnostic.kind.is_source_discovery()
                    })
                    .collect(),
            ),
            discovery.source_diagnostics().to_vec(),
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
    ///
    /// # Errors
    ///
    /// Returns an engine error when root manifest loading fails during
    /// workspace discovery, matching `ProjectConfig::load`.
    pub fn from_resolved_config(config: ResolvedConfig) -> EngineResult<Self> {
        let (workspaces, workspace_diagnostics, workspace_discovery_ms) =
            crate::project_config::collect_workspace_metadata(&config)?;
        Ok(Self::from_config(ProjectConfig {
            config,
            path: None,
            workspaces,
            workspace_diagnostics,
            workspace_discovery_ms: Some(workspace_discovery_ms),
        }))
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
    fn source_fingerprints(&self) -> FxHashMap<PathBuf, SourceFingerprint> {
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
    pub(crate) fn changed_files_since(
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

    /// Current diagnostics, including the source read failures the parse stage
    /// discovers and the analysis-stage entries the analyze pass records, both
    /// of which land in the registry after the session was created.
    ///
    /// The live read drops walk-recorded entries
    /// ([`fallow_types::workspace::WorkspaceDiagnosticKind::is_source_walk_recorded`])
    /// for the same
    /// reason the constructor does: a concurrent walk on the same root
    /// replaces that set, so importing it here would make this session's list
    /// depend on which walk wrote last, and the combined root's union would
    /// come out in a different ORDER between runs of the same command (issue
    /// #2366). This session's own walk-recorded entries are already in the
    /// snapshot, by value, from its own walk.
    #[must_use]
    pub fn current_workspace_diagnostics(&self) -> Vec<WorkspaceDiagnostic> {
        merge_workspace_diagnostics(
            self.workspace_diagnostics.clone(),
            fallow_config::workspace_diagnostics_for(&self.config.root)
                .into_iter()
                .filter(|diagnostic| !diagnostic.kind.is_source_walk_recorded())
                .collect(),
        )
    }

    pub(crate) fn styling_analysis_artifacts(
        &self,
    ) -> Arc<crate::health::StylingAnalysisArtifacts> {
        if let Ok(cache) = self.styling_cache.lock()
            && let Some(artifacts) = cache.as_ref()
        {
            return Arc::clone(artifacts);
        }

        let artifacts = Arc::new(crate::health::build_styling_analysis_artifacts(
            self.files(),
            self.config(),
        ));
        if let Ok(mut cache) = self.styling_cache.lock() {
            *cache = Some(Arc::clone(&artifacts));
        }
        artifacts
    }

    /// Consume the session and return the resolved config plus discovery data.
    #[must_use]
    pub fn into_parts(self) -> AnalysisSessionParts {
        let workspace_diagnostics = self.current_workspace_diagnostics();
        AnalysisSessionParts {
            config: self.config,
            config_path: self.config_path,
            files: self.discovery.into_files(),
            workspaces: self.workspaces,
            workspace_diagnostics,
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
        let ParsedModules {
            modules,
            metrics,
            source_diagnostics,
        } = parse_files_with_config(&config, &files, need_complexity);
        ParsedAnalysisSessionParts {
            config,
            config_path,
            files,
            modules,
            workspaces,
            workspace_diagnostics: merge_workspace_diagnostics(
                workspace_diagnostics,
                source_diagnostics,
            ),
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

    /// Parse discovered files while retaining shared immutable module storage.
    #[must_use]
    pub(crate) fn shared_parsed_parts(
        &self,
        need_complexity: bool,
    ) -> SharedParsedAnalysisSessionParts {
        let SharedParsedModules { modules, metrics } = self.parse_modules(need_complexity);
        SharedParsedAnalysisSessionParts {
            config: self.config.clone(),
            files: self.discovery.files().to_vec(),
            modules,
            workspaces: self.workspaces.clone(),
            workspace_diagnostics: self.current_workspace_diagnostics(),
            parse_ms: metrics.parse_ms,
            parse_cpu_ms: metrics.parse_cpu_ms,
        }
    }

    /// Return immutable parsed modules backed by the reusable session cache.
    ///
    /// Workspace-owned consumers use this additive path when they only need
    /// parsed modules and can borrow discovery and config directly from the
    /// session. Stable owned callers can continue using [`Self::parsed_parts`].
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn shared_parsed_modules(&self, need_complexity: bool) -> Arc<[ModuleInfo]> {
        self.parse_modules(need_complexity).modules
    }

    /// Parse discovered files without consuming the session or retaining parser
    /// output in the session cache.
    #[must_use]
    pub fn parsed_parts_uncached(&self, need_complexity: bool) -> ParsedAnalysisSessionParts {
        let ParsedModules {
            modules,
            metrics,
            source_diagnostics: _,
        } = parse_files_with_config(&self.config, self.files(), need_complexity);
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
            workspace_diagnostics: self.current_workspace_diagnostics(),
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
    pub(crate) fn analyze_dead_code_with_shared_modules(
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

        let ParsedModules {
            modules,
            metrics,
            source_diagnostics: _,
        } = parse_files_with_config(&self.config, self.files(), need_complexity);
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

struct ParsedModules {
    modules: Vec<ModuleInfo>,
    metrics: core_backend::ParseMetrics,
    source_diagnostics: Vec<WorkspaceDiagnostic>,
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
    let source_diagnostics =
        fallow_config::record_source_read_failures(&config.root, &parse_result.read_failures);
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
    ParsedModules {
        modules,
        metrics,
        source_diagnostics,
    }
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

    let mut detector = core_backend::run_dead_code_detectors(
        &prelude,
        &graph.graph,
        &resolved.project.modules,
        &modules,
        collect_usages,
        &entry_points,
    );
    crate::dead_code::filter_configured_ignored_findings(&mut detector.results, config);
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
        core_backend::build_dead_code_graph(prelude, &resolved.project, entry_points, modules);
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
    let (workspaces, _diagnostics, workspaces_ms) =
        crate::project_config::collect_workspace_metadata(config)?;
    let discovery = crate::discover::prepare_analysis_discovery_with_workspaces(
        config,
        &workspaces,
        workspaces_ms,
    );
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
    fn finding_ignore_filters_results_without_removing_graph_inputs() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("create source directory");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"finding-ignore","devDependencies":{"vitest":"latest"}}"#,
        )
        .expect("write package manifest");
        std::fs::write(
            root.join("vitest.config.ts"),
            "import './src/feature';\nexport default {};\n",
        )
        .expect("write vitest config");
        std::fs::write(
            root.join("src/feature.ts"),
            "export const feature = true;\n",
        )
        .expect("write reachable source");
        std::fs::write(root.join("src/hidden.ts"), "export const hidden = true;\n")
            .expect("write hidden source");

        let unfiltered = AnalysisSession::load(root, None)
            .expect("unfiltered session loads")
            .analyze_dead_code()
            .expect("unfiltered analysis succeeds");
        assert!(
            unfiltered
                .results
                .unused_files
                .iter()
                .any(|finding| finding.file.path.ends_with("src/hidden.ts"))
        );

        std::fs::write(
            root.join(".fallowrc.json"),
            r#"{"ignoreFindings":["src/hidden.ts"]}"#,
        )
        .expect("write fallow config");
        let session = AnalysisSession::load(root, None).expect("filtered session loads");
        let hidden_path = root.join("src/hidden.ts");
        assert!(session.files().iter().any(|file| file.path == hidden_path));

        let filtered = session
            .analyze_dead_code_with_artifacts(false, true)
            .expect("filtered analysis succeeds");
        assert!(
            filtered
                .results
                .unused_files
                .iter()
                .all(|finding| finding.file.path != hidden_path)
        );
        assert!(
            filtered
                .graph
                .as_ref()
                .is_some_and(|graph| graph.module_count() == session.files().len())
        );
    }

    #[test]
    fn finding_ignore_normalizes_separators_and_rejects_outside_paths() {
        use fallow_types::output_dead_code::UnusedFileFinding;
        use fallow_types::results::UnusedFile;

        let project = tempfile::tempdir().expect("project");
        let config = serde_json::from_str::<fallow_config::FallowConfig>(
            r#"{"ignoreFindings":["**/*.ts"]}"#,
        )
        .expect("config parses")
        .resolve(
            project.path().to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        let outside = project
            .path()
            .parent()
            .expect("project has parent")
            .join("outside.ts");
        let mut results = AnalysisResults {
            unused_files: vec![
                UnusedFileFinding::with_actions(UnusedFile {
                    path: PathBuf::from(r"src\hidden.ts"),
                }),
                UnusedFileFinding::with_actions(UnusedFile {
                    path: outside.clone(),
                }),
            ],
            ..AnalysisResults::default()
        };

        crate::dead_code::filter_configured_ignored_findings(&mut results, &config);

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(results.unused_files[0].file.path, outside);
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
    fn warm_styling_cache_reuses_artifact_allocation() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        std::fs::write(root.join("styles.css"), ".button { color: red; }\n")
            .expect("write stylesheet");
        let session = AnalysisSession::load_default(root);

        let first = session.styling_analysis_artifacts();
        let second = session.styling_analysis_artifacts();

        assert!(
            Arc::ptr_eq(&first, &second),
            "warm styling queries must share the cached artifact allocation"
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
    fn parsed_parts_keep_owned_module_compatibility() {
        let (_project, session) = session_with_source("export const value = 1;\n");
        let parts: ParsedAnalysisSessionParts = session.parsed_parts(false);

        let _: Vec<ModuleInfo> = parts.modules;
    }

    #[test]
    fn shared_parsed_parts_reuse_public_session_storage() {
        let (_project, session) = session_with_source("export const value = 1;\n");
        let cached = session.shared_parsed_modules(true);
        let parts = session.shared_parsed_parts(false);

        assert!(Arc::ptr_eq(&cached, &parts.modules));
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

    #[test]
    fn route_loader_whole_use_matches_across_cold_and_warm_sessions() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        std::fs::create_dir_all(root.join("app/routes")).expect("create route directory");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"route-cache-parity","dependencies":{"react-router":"latest"}}"#,
        )
        .expect("write package manifest");
        std::fs::write(
            root.join("app/routes/home.tsx"),
            r#"
import { useLoaderData } from "react-router";
export function loader() { return { opaque: "value" }; }
export default function Home() {
  const data = useLoaderData<typeof loader>();
  const copy = { ...data };
  return JSON.stringify(copy);
}
"#,
        )
        .expect("write route module");

        let cold_session = AnalysisSession::load(root, None).expect("cold session loads");
        let cold_parse = cold_session.parsed_parts(false);
        assert_eq!(cold_parse.cache_hits, 0, "first parse must be cold");
        let cold = cold_session
            .analyze_dead_code()
            .expect("cold analysis succeeds");

        let warm_session = AnalysisSession::load(root, None).expect("warm session loads");
        let warm_parse = warm_session.parsed_parts(false);
        assert!(
            warm_parse.cache_hits > 0,
            "second session must use disk cache"
        );
        let warm = warm_session
            .analyze_dead_code()
            .expect("warm analysis succeeds");

        assert!(
            cold.results.unused_load_data_keys.is_empty(),
            "cold analysis must abstain for an opaque route-loader use"
        );
        assert_eq!(
            serde_json::to_vec(&cold.results).expect("serialize cold results"),
            serde_json::to_vec(&warm.results).expect("serialize warm results"),
            "warm route-loader analysis must match cold analysis"
        );
    }

    #[test]
    fn replaced_module_coverage_matches_across_cold_and_warm_graph_cache() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("create source directory");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"mock-cache-parity","main":"src/index.ts","devDependencies":{"vitest":"latest"}}"#,
        )
        .expect("write package manifest");
        std::fs::write(
            root.join("src/dependency.ts"),
            "export function dependency() { return 'real'; }\n",
        )
        .expect("write dependency");
        std::fs::write(
            root.join("src/wrapper.ts"),
            "import { dependency } from './dependency';\nexport function wrapper() { return dependency(); }\n",
        )
        .expect("write wrapper");
        std::fs::write(
            root.join("src/index.ts"),
            "export { wrapper } from './wrapper';\n",
        )
        .expect("write entry point");
        std::fs::write(
            root.join("src/wrapper.test.ts"),
            r#"
import { vi } from "vitest";
vi.mock("./dependency", () => ({ dependency: () => "mock" }));
import { wrapper } from "./wrapper";
wrapper();
"#,
        )
        .expect("write test");

        let cold_session = AnalysisSession::load(root, None).expect("cold session loads");
        let dependency_id = cold_session
            .files()
            .iter()
            .find(|file| file.path == root.join("src/dependency.ts"))
            .expect("dependency discovered")
            .id;
        let cold = cold_session
            .analyze_dead_code_with_artifacts(false, true)
            .expect("cold analysis succeeds");
        let cold_exports = crate::module_graph::module_value_exports(
            cold.graph.as_ref().expect("cold graph retained"),
        );
        assert!(
            fallow_graph::cache::GraphCacheStore::load(&cold_session.config().cache_dir).is_some(),
            "cold analysis must persist the graph cache"
        );

        let warm_session = AnalysisSession::load(root, None).expect("warm session loads");
        let warm = warm_session
            .analyze_dead_code_with_artifacts(false, true)
            .expect("warm analysis succeeds");
        let warm_exports = crate::module_graph::module_value_exports(
            warm.graph.as_ref().expect("warm graph retained"),
        );

        let dependency = cold_exports
            .iter()
            .find(|export| export.file_id == dependency_id && export.name == "dependency")
            .expect("dependency export retained");
        assert!(!dependency.test_referenced);
        assert_eq!(warm_exports, cold_exports);
    }

    #[test]
    fn session_parse_surfaces_removed_source_with_sparse_file_ids() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("create source directory");
        std::fs::write(root.join("package.json"), r#"{"name":"read-failure"}"#)
            .expect("write package manifest");
        for name in ["a.ts", "b.ts", "c.ts"] {
            std::fs::write(
                root.join("src").join(name),
                format!("export const {} = 1;\n", name.replace('.', "_")),
            )
            .expect("write source");
        }
        let session = AnalysisSession::load(root, None).expect("session loads");
        let removed_path = root.join("src/b.ts");
        let removed_id = session
            .files()
            .iter()
            .find(|file| file.path == removed_path)
            .expect("removed source discovered")
            .id;
        std::fs::remove_file(&removed_path).expect("remove source after discovery");

        let parts = session.parsed_parts(false);

        assert!(
            parts
                .modules
                .iter()
                .all(|module| module.file_id != removed_id),
            "unreadable file must not receive a placeholder module"
        );
        let diagnostic = parts
            .workspace_diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind.id() == "source-read-failure")
            .expect("parsed session parts carry source read failure");
        assert_eq!(diagnostic.path, removed_path);
        assert!(
            session
                .current_workspace_diagnostics()
                .iter()
                .any(|diagnostic| {
                    diagnostic.kind.id() == "source-read-failure" && diagnostic.path == removed_path
                }),
            "session output carries parse-time source diagnostics"
        );
    }

    const MALFORMED_PNPM_WORKSPACE_YAML: &str =
        "catalog:\n  react: ^18.2.0\n{this is\nnot: valid: yaml: at: all\n";
    const VALID_PNPM_WORKSPACE_YAML: &str = "catalog:\n  react: ^18.2.0\n";

    fn has_diagnostic_kind(diagnostics: &[WorkspaceDiagnostic], id: &str) -> bool {
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind.id() == id)
    }

    fn write_single_source_project(root: &Path, manifest: &str) {
        std::fs::create_dir(root.join("src")).expect("create source directory");
        std::fs::write(root.join("package.json"), manifest).expect("write package manifest");
        std::fs::write(root.join("src/index.ts"), "export const value = 1;\n")
            .expect("write source");
    }

    /// Issue #2366: engine sessions (the MCP and LSP path) never re-stash the
    /// registry, so a session created after an earlier analysis in the same
    /// process must not keep that analysis's analysis-stage diagnostic once
    /// the cause is fixed: the analyze pass refreshes the entry and the
    /// session snapshot must not pin it.
    #[test]
    fn later_session_drops_stale_analysis_stage_diagnostic_after_cause_is_fixed() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        write_single_source_project(
            root,
            r#"{"name":"issue-2366-engine-session","private":true}"#,
        );
        std::fs::write(
            root.join("pnpm-workspace.yaml"),
            MALFORMED_PNPM_WORKSPACE_YAML,
        )
        .expect("write malformed workspace yaml");

        let broken = AnalysisSession::load(root, None).expect("session loads");
        broken
            .analyze_dead_code()
            .expect("analysis on the malformed yaml succeeds");
        assert!(
            has_diagnostic_kind(
                &broken.current_workspace_diagnostics(),
                "malformed-pnpm-workspace-yaml"
            ),
            "the first session surfaces the malformed yaml: {:?}",
            broken.current_workspace_diagnostics()
        );

        std::fs::write(root.join("pnpm-workspace.yaml"), VALID_PNPM_WORKSPACE_YAML)
            .expect("fix workspace yaml");

        let fixed = AnalysisSession::load(root, None).expect("session loads");
        fixed
            .analyze_dead_code()
            .expect("analysis on the fixed yaml succeeds");
        let current = fixed.current_workspace_diagnostics();
        assert!(
            !has_diagnostic_kind(&current, "malformed-pnpm-workspace-yaml"),
            "a later session must not keep the stale analysis-stage entry (#2366): {current:?}"
        );
    }

    /// Watch-mode rerun shape (issue #2366): the CLI reloads config, which
    /// re-stashes the workspace-discovery set, and builds a fresh session from
    /// the resolved config before re-analyzing. Once a text `bun.lock` exists
    /// the rerun must drop the bun.lockb skip diagnostic. Regression pin: the
    /// old stash wiped analysis-stage entries instead of preserving them, so
    /// this passes before and after the fix.
    #[test]
    fn watch_style_rerun_drops_bun_lockb_skip_once_text_lockfile_exists() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        write_single_source_project(
            root,
            r#"{"name":"issue-2366-watch-rerun","private":true,"overrides":{"ws":"^8.21.0"}}"#,
        );
        std::fs::write(root.join("bun.lockb"), b"placeholder binary lockfile")
            .expect("write bun.lockb placeholder");
        let config = fallow_config::FallowConfig::default().resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
        let reload_config = || {
            let (_, diagnostics) =
                fallow_config::discover_workspaces_with_diagnostics(root, &config.ignore_patterns)
                    .expect("workspace discovery succeeds");
            fallow_config::stash_workspace_diagnostics(root, diagnostics);
        };

        reload_config();
        let first =
            AnalysisSession::from_resolved_config(config.clone()).expect("first session loads");
        first
            .analyze_dead_code()
            .expect("analysis with bun.lockb only succeeds");
        assert!(
            has_diagnostic_kind(
                &first.current_workspace_diagnostics(),
                "bun-lockb-override-resolution-skipped"
            ),
            "the first run surfaces the bun.lockb skip: {:?}",
            first.current_workspace_diagnostics()
        );

        std::fs::write(
            root.join("bun.lock"),
            r#"{"lockfileVersion":1,"workspaces":{"":{"name":"issue-2366-watch-rerun"}},"packages":{"ws":["ws@8.21.3","",{},"sha512-20"]}}"#,
        )
        .expect("write text bun.lock");

        reload_config();
        let rerun =
            AnalysisSession::from_resolved_config(config.clone()).expect("rerun session loads");
        rerun
            .analyze_dead_code()
            .expect("analysis with the text bun.lock succeeds");
        let current = rerun.current_workspace_diagnostics();
        assert!(
            !has_diagnostic_kind(&current, "bun-lockb-override-resolution-skipped"),
            "the rerun drops the skip once a text bun.lock exists (#2366): {current:?}"
        );
    }

    /// Issue #2366: `current_workspace_diagnostics` reads the registry live so
    /// the parse-stage and analyze-stage entries that land after the session
    /// was created still reach the envelope, but it must not import another
    /// walk's skips along with them.
    ///
    /// Combined mode runs the dead-code and duplication walks on the same root
    /// under `rayon::join` whenever a per-analysis `production` split stops
    /// them from sharing a file list, and each walk replaces the registry's
    /// source-discovery set. A session that read that set back would answer
    /// "whichever walk wrote last", which decides where the other walk's skip
    /// lands in the combined root's union and made the array come out in a
    /// different ORDER between runs of the same command.
    #[test]
    fn session_keeps_its_own_walk_skips_and_ignores_another_walks_registry_write() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        write_single_source_project(
            root,
            r#"{"name":"issue-2366-parallel-walks","private":true}"#,
        );
        std::fs::write(root.join("src/huge.ts"), "// filler\n".repeat(400))
            .expect("write oversized source");
        let mut config = fallow_config::FallowConfig::default().resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
        config.max_file_size_bytes = Some(1024);

        let session = AnalysisSession::from_resolved_config(config).expect("session loads");

        // The state a concurrent walk leaves behind: its own skip in this
        // root's registry entry. It writes that through the registry's
        // replace-in-one-operation call, which an architecture guard reserves
        // for the walk itself, so the append is the stand-in here.
        fallow_config::append_workspace_diagnostics(
            root,
            vec![WorkspaceDiagnostic::new(
                root,
                root.join("src/other-walk-only.ts"),
                fallow_types::workspace::WorkspaceDiagnosticKind::SkippedLargeFile {
                    size_bytes: 4096,
                },
            )],
        );

        let current = session.current_workspace_diagnostics();
        let skipped: Vec<&Path> = current
            .iter()
            .filter(|diagnostic| diagnostic.kind.id() == "skipped-large-file")
            .map(|diagnostic| diagnostic.path.as_path())
            .collect();
        assert_eq!(
            skipped.len(),
            1,
            "the session reports its own walk's skips only: {skipped:?}"
        );
        assert!(
            skipped[0].ends_with("src/huge.ts"),
            "the surviving skip is this walk's own: {skipped:?}"
        );
    }

    /// Issue #2366: a config reload that happens AFTER the analyze pass, with
    /// no further pass to re-record, must not wipe the analysis-stage entry
    /// from the process registry. This is the long-lived-server shape: an MCP
    /// or LSP process analyzes once, a later request reloads config for a
    /// different analysis family, and a session built after that reload still
    /// reads the registry live. Pins the analysis-stage preserve in
    /// `stash_workspace_diagnostics`; without it this session reports nothing.
    #[test]
    fn config_reload_after_the_analyze_pass_keeps_the_bun_lockb_skip_readable() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        write_single_source_project(
            root,
            r#"{"name":"issue-2366-reload-preserve","private":true,"overrides":{"ws":"^8.21.0"}}"#,
        );
        std::fs::write(root.join("bun.lockb"), b"placeholder binary lockfile")
            .expect("write bun.lockb placeholder");
        let config = fallow_config::FallowConfig::default().resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
        let reload_config = || {
            let (_, diagnostics) =
                fallow_config::discover_workspaces_with_diagnostics(root, &config.ignore_patterns)
                    .expect("workspace discovery succeeds");
            fallow_config::stash_workspace_diagnostics(root, diagnostics);
        };

        reload_config();
        let analyzing =
            AnalysisSession::from_resolved_config(config.clone()).expect("session loads");
        analyzing
            .analyze_dead_code()
            .expect("analysis with bun.lockb only succeeds");

        // A later request reloads config for another analysis family and never
        // runs a second dead-code pass.
        reload_config();

        let later = AnalysisSession::from_resolved_config(config.clone()).expect("session loads");
        let current = later.current_workspace_diagnostics();
        assert!(
            has_diagnostic_kind(&current, "bun-lockb-override-resolution-skipped"),
            "the reload must preserve the analysis-stage entry the pass recorded (#2366): \
             {current:?}"
        );
    }
}
