//! Discovery helpers and types exposed through the engine boundary.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use fallow_config::{PackageJson, ResolvedConfig, WorkspaceInfo};
pub use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};

use crate::core_backend;

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
    pub(crate) const fn from_backend(inner: core_backend::BackendAnalysisDiscovery) -> Self {
        Self { inner }
    }

    pub(crate) const fn as_backend(&self) -> &core_backend::BackendAnalysisDiscovery {
        &self.inner
    }

    /// Discovered source files, indexed by stable `FileId` for this session.
    #[must_use]
    pub fn files(&self) -> &[DiscoveredFile] {
        self.inner.files()
    }

    /// Consume this discovery prelude and return its source file registry.
    #[must_use]
    pub fn into_files(self) -> Vec<DiscoveredFile> {
        self.inner.into_files()
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

/// Discover source files for a resolved config.
#[must_use]
pub fn discover_files(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    core_backend::discover_files(config)
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
