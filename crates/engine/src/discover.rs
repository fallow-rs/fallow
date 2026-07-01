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

use crate::core_backend;

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
    core_backend::is_allowed_hidden_dir(name)
}

/// Collect plugin-derived hidden directory scopes.
#[must_use]
pub fn collect_plugin_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    core_backend::collect_plugin_hidden_dir_scopes(config, root_pkg, workspaces)
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

/// Discover source files for a resolved config.
#[must_use]
pub fn discover_files(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    core_backend::discover_files(config)
}

/// Discover source files and non-source config candidates in one traversal.
#[must_use]
pub fn discover_files_and_config_candidates(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> (Vec<DiscoveredFile>, Vec<PathBuf>) {
    core_backend::discover_files_and_config_candidates(config, additional_hidden_dir_scopes)
}

/// Discover source files with additional package-scoped hidden directories.
#[must_use]
pub fn discover_files_with_additional_hidden_dirs(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> Vec<DiscoveredFile> {
    core_backend::discover_files_with_additional_hidden_dirs(config, additional_hidden_dir_scopes)
}

/// Discover source files for a resolved config, including plugin scopes.
#[must_use]
pub fn discover_files_with_plugin_scopes(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    core_backend::discover_files_with_plugin_scopes(config)
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

    use super::{CategorizedEntryPoints, EntryPoint, EntryPointSource, HiddenDirScope};

    #[test]
    fn hidden_dir_scope_exposes_root_and_dirs() {
        let scope = HiddenDirScope::new(PathBuf::from("/repo/packages/app"), vec![".next".into()]);

        assert_eq!(scope.root(), PathBuf::from("/repo/packages/app"));
        assert_eq!(scope.dirs(), [".next"]);
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
