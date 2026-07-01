//! Engine-owned analysis session orchestration.

use std::path::{Path, PathBuf};
use std::time::Instant;

use fallow_config::{DuplicatesConfig, ResolvedConfig};
use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::{ModuleInfo, ParseResult};
use fallow_types::workspace::WorkspaceDiagnostic;
use rustc_hash::FxHashSet;

use crate::{
    DeadCodeAnalysis, DeadCodeAnalysisArtifacts, DeadCodeAnalysisOutput, DuplicationAnalysis,
    EngineResult, ProjectAnalysisOutput, ProjectConfig, config_for_project, core_backend,
    duplicates, project_config::default_project_config,
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
    workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

/// Owned session parts for runners that need to continue an existing pipeline.
#[derive(Debug)]
pub struct AnalysisSessionParts {
    pub config: ResolvedConfig,
    pub config_path: Option<PathBuf>,
    pub files: Vec<DiscoveredFile>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
}

/// Owned session parts after parsing the discovered files.
#[derive(Debug)]
pub struct ParsedAnalysisSessionParts {
    pub config: ResolvedConfig,
    pub config_path: Option<PathBuf>,
    pub files: Vec<DiscoveredFile>,
    pub modules: Vec<ModuleInfo>,
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    pub parse_ms: f64,
    pub parse_cpu_ms: f64,
}

/// Borrowed session view for callers that expose `&ResolvedConfig`.
///
/// This keeps existing helper signatures intact while routing discovery and
/// analysis through the same session-owned orchestration shape as
/// [`AnalysisSession`].
struct AnalysisSessionView<'a> {
    config: &'a ResolvedConfig,
    discovery: crate::discover::AnalysisDiscovery,
}

impl<'a> AnalysisSessionView<'a> {
    fn new(config: &'a ResolvedConfig) -> Self {
        Self {
            config,
            discovery: core_backend::prepare_analysis_discovery(config),
        }
    }

    fn analyze_dead_code(&self) -> EngineResult<DeadCodeAnalysis> {
        core_backend::analyze_with_usages_from_discovery(self.config, &self.discovery)
    }

    fn analyze_dead_code_with_complexity(&self) -> EngineResult<DeadCodeAnalysisOutput> {
        core_backend::analyze_with_usages_and_complexity_from_discovery(
            self.config,
            &self.discovery,
        )
    }

    fn analyze_dead_code_with_artifacts(
        &self,
        need_complexity: bool,
        retain_graph: bool,
    ) -> EngineResult<DeadCodeAnalysisArtifacts> {
        core_backend::analyze_retaining_modules_from_discovery(
            self.config,
            &self.discovery,
            need_complexity,
            retain_graph,
        )
    }
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
        let discovery = core_backend::prepare_analysis_discovery(&project_config.config);
        let workspace_diagnostics = merge_workspace_diagnostics(
            project_config.workspace_diagnostics,
            fallow_config::workspace_diagnostics_for(&project_config.config.root),
        );
        Self {
            config: project_config.config,
            config_path: project_config.path,
            discovery,
            workspace_diagnostics,
        }
    }

    /// Build a session from a resolved config when the caller already owns
    /// command-specific config loading.
    #[must_use]
    pub fn from_resolved_config(config: ResolvedConfig) -> Self {
        Self::from_config(ProjectConfig {
            config,
            path: None,
            workspace_diagnostics: Vec::new(),
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

    /// Workspace and source-discovery diagnostics captured for this session.
    #[must_use]
    pub fn workspace_diagnostics(&self) -> &[WorkspaceDiagnostic] {
        &self.workspace_diagnostics
    }

    /// Consume the session and return the resolved config plus discovery data.
    #[must_use]
    pub fn into_parts(self) -> AnalysisSessionParts {
        AnalysisSessionParts {
            config: self.config,
            config_path: self.config_path,
            files: self.discovery.into_files(),
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
            workspace_diagnostics,
        } = self.into_parts();
        let (parse_result, parse_ms) = parse_files_with_config(&config, &files, need_complexity);
        ParsedAnalysisSessionParts {
            config,
            config_path,
            files,
            modules: parse_result.modules,
            workspace_diagnostics,
            parse_ms,
            parse_cpu_ms: parse_result.parse_cpu_ms,
        }
    }

    /// Run dead-code analysis for this session.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code(&self) -> EngineResult<DeadCodeAnalysis> {
        core_backend::analyze_with_usages_from_discovery(&self.config, &self.discovery)
    }

    /// Run dead-code analysis with retained complexity artifacts.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing or analysis fails.
    pub fn analyze_dead_code_with_complexity(&self) -> EngineResult<DeadCodeAnalysisOutput> {
        core_backend::analyze_with_usages_and_complexity_from_discovery(
            &self.config,
            &self.discovery,
        )
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
        core_backend::analyze_retaining_modules_from_discovery(
            &self.config,
            &self.discovery,
            need_complexity,
            retain_graph,
        )
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
}

pub fn parse_files_for_config(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    need_complexity: bool,
) -> ParseResult {
    parse_files_with_config(config, files, need_complexity).0
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

fn parse_files_with_config(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    need_complexity: bool,
) -> (ParseResult, f64) {
    let parse_start = Instant::now();
    let cache = if config.no_cache {
        None
    } else {
        fallow_extract::cache::CacheStore::load(
            &config.cache_dir,
            config.cache_config_hash,
            crate::resolve_cache_max_size_bytes(config),
        )
    };
    let parse_result = crate::source::parse_all_files(files, cache.as_ref(), need_complexity);
    (parse_result, parse_start.elapsed().as_secs_f64() * 1000.0)
}

pub fn analyze_dead_code_from_config(config: &ResolvedConfig) -> EngineResult<DeadCodeAnalysis> {
    AnalysisSessionView::new(config).analyze_dead_code()
}

pub fn analyze_dead_code_with_complexity_from_config(
    config: &ResolvedConfig,
) -> EngineResult<DeadCodeAnalysisOutput> {
    AnalysisSessionView::new(config).analyze_dead_code_with_complexity()
}

pub fn analyze_dead_code_with_artifacts_from_config(
    config: &ResolvedConfig,
    need_complexity: bool,
    retain_graph: bool,
) -> EngineResult<DeadCodeAnalysisArtifacts> {
    AnalysisSessionView::new(config).analyze_dead_code_with_artifacts(need_complexity, retain_graph)
}
