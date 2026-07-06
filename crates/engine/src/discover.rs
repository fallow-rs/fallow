//! Discovery helpers and types exposed through the engine boundary.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Instant;

use fallow_config::{
    PackageJson, ResolvedConfig, WorkspaceDiagnostic, WorkspaceInfo, discover_workspaces,
    find_undeclared_workspaces_with_ignores,
};
pub use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use rustc_hash::FxHashSet;

use crate::{EngineError, EngineResult, core_backend, plugins::PluginRegistry};

const UNDECLARED_WORKSPACE_WARNING_PREVIEW: usize = 5;

pub const SOURCE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "mts", "cts", "gts", "js", "jsx", "mjs", "cjs", "gjs", "vue", "svelte", "astro",
    "mdx", "css", "scss", "sass", "less", "html", "graphql", "gql",
];

/// Glob patterns for test/dev/story files excluded in production mode.
pub const PRODUCTION_EXCLUDE_PATTERNS: &[&str] = &[
    "**/*.test.*",
    "**/*.spec.*",
    "**/*.e2e.*",
    "**/*.e2e-spec.*",
    "**/*.bench.*",
    "**/*.fixture.*",
    "**/*.stories.*",
    "**/*.story.*",
    "**/__tests__/**",
    "**/__mocks__/**",
    "**/__snapshots__/**",
    "**/__fixtures__/**",
    "**/test/**",
    "**/tests/**",
    "*.config.*",
    "**/.*.js",
    "**/.*.ts",
    "**/.*.mjs",
    "**/.*.cjs",
];

const ALLOWED_HIDDEN_DIRS: &[&str] = &[
    ".storybook",
    ".vitepress",
    ".well-known",
    ".changeset",
    ".github",
];

/// Discover workspace packages through the engine boundary.
///
/// Use this for callers that only need workspace metadata and do not yet own an
/// `AnalysisSession`. Session-backed flows should prefer
/// [`AnalysisSession::workspaces`](crate::session::AnalysisSession::workspaces)
/// so discovery is reused with the rest of the analysis context.
#[must_use]
pub fn discover_workspace_packages(root: &Path) -> Vec<WorkspaceInfo> {
    discover_workspaces(root)
}

/// Discover workspace packages and diagnostics through the engine boundary.
///
/// This is for CLI/API surfaces that need to render workspace diagnostics but
/// do not otherwise need a full [`AnalysisSession`](crate::session::AnalysisSession).
///
/// # Errors
///
/// Returns an engine error when workspace manifest loading fails.
pub fn discover_workspace_packages_with_diagnostics(
    root: &Path,
    ignore_patterns: &globset::GlobSet,
) -> EngineResult<(Vec<WorkspaceInfo>, Vec<WorkspaceDiagnostic>)> {
    fallow_config::discover_workspaces_with_diagnostics(root, ignore_patterns)
        .map_err(|err| EngineError::new(err.to_string()))
}

/// Entry points grouped by reachability role.
#[derive(Debug, Clone, Default)]
pub struct CategorizedEntryPoints {
    pub all: Vec<EntryPoint>,
    pub runtime: Vec<EntryPoint>,
    pub test: Vec<EntryPoint>,
}

impl CategorizedEntryPoints {
    #[must_use]
    pub fn dedup(mut self) -> Self {
        dedup_entry_paths(&mut self.all);
        dedup_entry_paths(&mut self.runtime);
        dedup_entry_paths(&mut self.test);
        self
    }
}

fn dedup_entry_paths(entries: &mut Vec<EntryPoint>) {
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries.dedup_by(|a, b| a.path == b.path);
}

/// Package-scoped hidden directories that source discovery should traverse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenDirScope {
    root: PathBuf,
    dirs: Vec<String>,
}

impl HiddenDirScope {
    #[must_use]
    pub const fn new(root: PathBuf, dirs: Vec<String>) -> Self {
        Self { root, dirs }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn dirs(&self) -> &[String] {
        &self.dirs
    }
}

/// Reusable engine discovery prelude for one resolved project.
#[derive(Debug, Clone)]
pub struct AnalysisDiscovery {
    inner: core_backend::BackendAnalysisDiscovery,
}

impl AnalysisDiscovery {
    pub(crate) const fn as_backend(&self) -> &core_backend::BackendAnalysisDiscovery {
        &self.inner
    }

    fn from_parts(
        files: Vec<DiscoveredFile>,
        workspaces: Vec<WorkspaceInfo>,
        root_pkg: Option<PackageJson>,
        config_candidates: Vec<PathBuf>,
        discover_ms: f64,
        workspaces_ms: f64,
    ) -> Self {
        Self {
            inner: core_backend::BackendAnalysisDiscovery::from_parts(
                files,
                workspaces,
                root_pkg,
                config_candidates,
                discover_ms,
                workspaces_ms,
            ),
        }
    }

    /// Discovered source files, indexed by stable `FileId` for this session.
    #[must_use]
    pub fn files(&self) -> &[DiscoveredFile] {
        self.inner.files()
    }

    /// Discovered workspace packages for this session.
    #[must_use]
    pub fn workspaces(&self) -> &[WorkspaceInfo] {
        self.inner.workspaces()
    }

    /// Consume this discovery prelude and return its source file registry.
    #[must_use]
    pub fn into_files(self) -> Vec<DiscoveredFile> {
        self.inner.into_files()
    }
}

/// Run engine-owned workspace and source discovery for a resolved project.
#[must_use]
pub fn prepare_analysis_discovery(config: &ResolvedConfig) -> AnalysisDiscovery {
    warn_missing_node_modules(config);

    let workspaces_start = Instant::now();
    let workspaces = discover_workspaces(&config.root);
    let workspaces_ms = workspaces_start.elapsed().as_secs_f64() * 1000.0;
    if !workspaces.is_empty() {
        tracing::info!(count = workspaces.len(), "workspaces discovered");
    }
    warn_undeclared_workspaces(
        &config.root,
        &workspaces,
        &config.ignore_patterns,
        config.quiet,
    );

    let root_pkg = PackageJson::load(&config.root.join("package.json")).ok();
    let hidden_dir_scopes = collect_hidden_dir_scopes(config, root_pkg.as_ref(), &workspaces);

    let discover_start = Instant::now();
    let (files, config_candidates) =
        discover_files_and_config_candidates(config, &hidden_dir_scopes);
    let discover_ms = discover_start.elapsed().as_secs_f64() * 1000.0;

    AnalysisDiscovery::from_parts(
        files,
        workspaces,
        root_pkg,
        config_candidates,
        discover_ms,
        workspaces_ms,
    )
}

/// Run source discovery with workspace metadata already resolved by config load.
///
/// This is the normal [`AnalysisSession`](crate::session::AnalysisSession) path:
/// config loading already expanded workspace globs and collected diagnostics, so
/// source discovery can reuse that set instead of walking workspace manifests a
/// second time.
#[must_use]
pub fn prepare_analysis_discovery_with_workspaces(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    workspaces_ms: f64,
) -> AnalysisDiscovery {
    warn_missing_node_modules(config);

    if !workspaces.is_empty() {
        tracing::info!(count = workspaces.len(), "workspaces discovered");
    }

    let root_pkg = PackageJson::load(&config.root.join("package.json")).ok();
    let hidden_dir_scopes = collect_hidden_dir_scopes(config, root_pkg.as_ref(), workspaces);

    let discover_start = Instant::now();
    let (files, config_candidates) =
        discover_files_and_config_candidates(config, &hidden_dir_scopes);
    let discover_ms = discover_start.elapsed().as_secs_f64() * 1000.0;

    AnalysisDiscovery::from_parts(
        files,
        workspaces.to_vec(),
        root_pkg,
        config_candidates,
        discover_ms,
        workspaces_ms,
    )
}

fn warn_missing_node_modules(config: &ResolvedConfig) {
    if config.root.join("node_modules").is_dir() {
        return;
    }

    tracing::warn!(
        "node_modules directory not found. Run `npm install` / `pnpm install` first for accurate results."
    );
}

fn format_undeclared_workspace_warning(
    root: &Path,
    undeclared: &[WorkspaceDiagnostic],
) -> Option<String> {
    if undeclared.is_empty() {
        return None;
    }

    let preview = undeclared
        .iter()
        .take(UNDECLARED_WORKSPACE_WARNING_PREVIEW)
        .map(|diagnostic| {
            diagnostic
                .path
                .strip_prefix(root)
                .unwrap_or(&diagnostic.path)
                .display()
                .to_string()
                .replace('\\', "/")
        })
        .collect::<Vec<_>>();
    let remaining = undeclared
        .len()
        .saturating_sub(UNDECLARED_WORKSPACE_WARNING_PREVIEW);
    let tail = if remaining > 0 {
        format!(" (and {remaining} more)")
    } else {
        String::new()
    };
    let noun = if undeclared.len() == 1 {
        "directory with package.json is"
    } else {
        "directories with package.json are"
    };
    let guidance = if undeclared.len() == 1 {
        "Add that path to package.json workspaces or pnpm-workspace.yaml if it should be analyzed as a workspace."
    } else {
        "Add those paths to package.json workspaces or pnpm-workspace.yaml if they should be analyzed as workspaces."
    };

    Some(format!(
        "{} {} not declared as {}: {}{}. {}",
        undeclared.len(),
        noun,
        if undeclared.len() == 1 {
            "a workspace"
        } else {
            "workspaces"
        },
        preview.join(", "),
        tail,
        guidance
    ))
}

fn warn_undeclared_workspaces(
    root: &Path,
    workspaces: &[WorkspaceInfo],
    ignore_patterns: &globset::GlobSet,
    quiet: bool,
) {
    let undeclared = find_undeclared_workspaces_with_ignores(root, workspaces, ignore_patterns);
    if undeclared.is_empty() {
        return;
    }

    let existing = fallow_config::workspace_diagnostics_for(root);
    let already_flagged: FxHashSet<PathBuf> = existing
        .iter()
        .map(|diagnostic| {
            dunce::canonicalize(&diagnostic.path).unwrap_or_else(|_| diagnostic.path.clone())
        })
        .collect();
    let undeclared: Vec<_> = undeclared
        .into_iter()
        .filter(|diagnostic| {
            let canonical =
                dunce::canonicalize(&diagnostic.path).unwrap_or_else(|_| diagnostic.path.clone());
            !already_flagged.contains(&canonical)
        })
        .collect();
    if undeclared.is_empty() {
        return;
    }

    fallow_config::append_workspace_diagnostics(root, undeclared.clone());

    if !quiet && let Some(message) = format_undeclared_workspace_warning(root, &undeclared) {
        tracing::warn!("{message}");
    }
}

/// Check if a hidden directory name is on the discovery allowlist.
#[must_use]
pub fn is_allowed_hidden_dir(name: &OsStr) -> bool {
    ALLOWED_HIDDEN_DIRS
        .iter()
        .any(|&dir| OsStr::new(dir) == name)
}

/// Collect plugin-derived hidden directory scopes.
#[must_use]
pub fn collect_plugin_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    let registry = PluginRegistry::new(config.external_plugins.clone());
    let mut scopes = Vec::new();

    if let Some(pkg) = root_pkg {
        push_plugin_hidden_dir_scope(&mut scopes, &registry, pkg, &config.root);
    }

    for ws in workspaces {
        if let Ok(pkg) = PackageJson::load(&ws.root.join("package.json")) {
            push_plugin_hidden_dir_scope(&mut scopes, &registry, &pkg, &ws.root);
        }
    }

    scopes
}

fn push_plugin_hidden_dir_scope(
    scopes: &mut Vec<HiddenDirScope>,
    registry: &PluginRegistry,
    pkg: &PackageJson,
    root: &Path,
) {
    let dirs = registry.discovery_hidden_dirs(pkg, root);
    if !dirs.is_empty() {
        scopes.push(HiddenDirScope::new(root.to_path_buf(), dirs));
    }
}

/// Collect plugin and script-derived hidden directory scopes.
#[must_use]
pub fn collect_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    core_backend::collect_hidden_dir_scopes(config, root_pkg, workspaces)
}

/// Discover source files and non-source config candidates in one traversal.
#[must_use]
pub fn discover_files_and_config_candidates(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> (Vec<DiscoveredFile>, Vec<PathBuf>) {
    core_backend::discover_files_and_config_candidates(config, additional_hidden_dir_scopes)
}

/// Discover configured and inferred entry points.
#[must_use]
pub fn discover_entry_points(config: &ResolvedConfig, files: &[DiscoveredFile]) -> Vec<EntryPoint> {
    core_backend::discover_entry_points(config, files)
}

/// Discover entry points for a workspace package.
#[must_use]
pub fn discover_workspace_entry_points(
    ws_root: &Path,
    config: &ResolvedConfig,
    all_files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    core_backend::discover_workspace_entry_points(ws_root, config, all_files)
}

/// Discover entry points from plugin results.
#[must_use]
pub fn discover_plugin_entry_points(
    plugin_result: &crate::plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    core_backend::discover_plugin_entry_points(plugin_result.as_backend(), config, files)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_config::PackageJson;

    use super::{
        ALLOWED_HIDDEN_DIRS, CategorizedEntryPoints, EntryPoint, EntryPointSource, HiddenDirScope,
        collect_plugin_hidden_dir_scopes, is_allowed_hidden_dir,
    };

    #[test]
    fn hidden_dir_scope_exposes_root_and_dirs() {
        let scope = HiddenDirScope::new(PathBuf::from("/repo/packages/app"), vec![".next".into()]);

        assert_eq!(scope.root(), PathBuf::from("/repo/packages/app"));
        assert_eq!(scope.dirs(), [".next"]);
    }

    #[test]
    fn hidden_dir_allowlist_is_engine_owned() {
        for dir in ALLOWED_HIDDEN_DIRS {
            assert!(is_allowed_hidden_dir(std::ffi::OsStr::new(dir)));
        }
        assert!(!is_allowed_hidden_dir(std::ffi::OsStr::new(".git")));
    }

    #[test]
    fn plugin_hidden_dir_scopes_are_engine_owned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = fallow_config::FallowConfig::default().resolve(
            dir.path().to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        let pkg: PackageJson = serde_json::from_value(serde_json::json!({
            "devDependencies": {
                "@react-router/dev": "^7.0.0"
            }
        }))
        .expect("valid package fixture");

        let scopes = collect_plugin_hidden_dir_scopes(&config, Some(&pkg), &[]);

        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].root(), dir.path());
        assert_eq!(scopes[0].dirs(), [".client", ".server"]);
    }

    #[test]
    fn categorized_entry_points_dedups_each_bucket() {
        let entry = EntryPoint {
            path: PathBuf::from("/repo/src/index.ts"),
            source: EntryPointSource::DefaultIndex,
        };
        let engine = CategorizedEntryPoints {
            all: vec![entry.clone(), entry.clone()],
            runtime: vec![entry.clone(), entry.clone()],
            test: Vec::new(),
        }
        .dedup();

        assert_eq!(engine.all.len(), 1);
        assert_eq!(engine.runtime.len(), 1);
        assert_eq!(engine.test.len(), 0);
        assert_eq!(engine.all[0].path, entry.path);
    }
}
