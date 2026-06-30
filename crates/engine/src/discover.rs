//! Discovery helpers and types exposed through the engine boundary.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use fallow_config::{PackageJson, ResolvedConfig, WorkspaceInfo};
pub use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};

pub const SOURCE_EXTENSIONS: &[&str] = fallow_core::discover::SOURCE_EXTENSIONS;
pub const PRODUCTION_EXCLUDE_PATTERNS: &[&str] = fallow_core::discover::PRODUCTION_EXCLUDE_PATTERNS;

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

impl From<fallow_core::discover::CategorizedEntryPoints> for CategorizedEntryPoints {
    fn from(value: fallow_core::discover::CategorizedEntryPoints) -> Self {
        Self {
            all: value.all,
            runtime: value.runtime,
            test: value.test,
        }
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

impl From<fallow_core::discover::HiddenDirScope> for HiddenDirScope {
    fn from(value: fallow_core::discover::HiddenDirScope) -> Self {
        Self {
            root: value.root().to_path_buf(),
            dirs: value.dirs().to_vec(),
        }
    }
}

impl From<HiddenDirScope> for fallow_core::discover::HiddenDirScope {
    fn from(value: HiddenDirScope) -> Self {
        Self::new(value.root, value.dirs)
    }
}

/// Reusable engine discovery prelude for one resolved project.
#[derive(Debug, Clone)]
pub struct AnalysisDiscovery {
    inner: fallow_core::AnalysisDiscovery,
}

impl AnalysisDiscovery {
    pub(crate) const fn from_core(inner: fallow_core::AnalysisDiscovery) -> Self {
        Self { inner }
    }

    pub(crate) const fn as_core(&self) -> &fallow_core::AnalysisDiscovery {
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
    fallow_core::discover::is_allowed_hidden_dir(name)
}

/// Collect plugin-derived hidden directory scopes.
#[must_use]
pub fn collect_plugin_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    fallow_core::discover::collect_plugin_hidden_dir_scopes(config, root_pkg, workspaces)
        .into_iter()
        .map(Into::into)
        .collect()
}

/// Collect plugin-derived and script-derived hidden directory scopes.
#[must_use]
pub fn collect_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    fallow_core::discover::collect_hidden_dir_scopes(config, root_pkg, workspaces)
        .into_iter()
        .map(Into::into)
        .collect()
}

/// Discover source files for a resolved config.
#[must_use]
pub fn discover_files(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    fallow_core::discover::discover_files(config)
}

/// Discover source files and config candidates in one traversal.
#[must_use]
pub fn discover_files_and_config_candidates(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> (Vec<DiscoveredFile>, Vec<std::path::PathBuf>) {
    let scopes = to_core_hidden_dir_scopes(additional_hidden_dir_scopes);
    fallow_core::discover::discover_files_and_config_candidates(config, &scopes)
}

/// Discover source files with additional package-scoped hidden directories.
#[must_use]
pub fn discover_files_with_additional_hidden_dirs(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> Vec<DiscoveredFile> {
    let scopes = to_core_hidden_dir_scopes(additional_hidden_dir_scopes);
    fallow_core::discover::discover_files_with_additional_hidden_dirs(config, &scopes)
}

/// Discover source files for a resolved config, including plugin scopes.
#[must_use]
pub fn discover_files_with_plugin_scopes(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    fallow_core::discover::discover_files_with_plugin_scopes(config)
}

/// Discover configured and inferred entry points.
#[must_use]
pub fn discover_entry_points(config: &ResolvedConfig, files: &[DiscoveredFile]) -> Vec<EntryPoint> {
    fallow_core::discover::discover_entry_points(config, files)
}

/// Discover entry points for a workspace package.
#[must_use]
pub fn discover_workspace_entry_points(
    ws_root: &Path,
    config: &ResolvedConfig,
    all_files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    fallow_core::discover::discover_workspace_entry_points(ws_root, config, all_files)
}

/// Discover entry points from plugin results.
#[must_use]
pub fn discover_plugin_entry_points(
    plugin_result: &crate::plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    fallow_core::discover::discover_plugin_entry_points(plugin_result.as_core(), config, files)
}

/// Discover plugin-derived entry points with roles preserved.
#[must_use]
pub fn discover_plugin_entry_point_sets(
    plugin_result: &crate::plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> CategorizedEntryPoints {
    fallow_core::discover::discover_plugin_entry_point_sets(plugin_result.as_core(), config, files)
        .into()
}

/// Discover entry points from `dynamicallyLoaded` config patterns.
#[must_use]
pub fn discover_dynamically_loaded_entry_points(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    fallow_core::discover::discover_dynamically_loaded_entry_points(config, files)
}

/// Pre-compile glob patterns for efficient path matching.
#[must_use]
pub fn compile_glob_set(patterns: &[String]) -> Option<globset::GlobSet> {
    fallow_core::discover::compile_glob_set(patterns)
}

/// Discover entry points from infrastructure config files.
#[must_use]
pub fn discover_infrastructure_entry_points(root: &Path) -> Vec<EntryPoint> {
    fallow_core::discover::discover_infrastructure_entry_points(root)
}

fn to_core_hidden_dir_scopes(
    scopes: &[HiddenDirScope],
) -> Vec<fallow_core::discover::HiddenDirScope> {
    scopes.iter().cloned().map(Into::into).collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{CategorizedEntryPoints, EntryPoint, EntryPointSource, HiddenDirScope};

    #[test]
    fn hidden_dir_scope_round_trips_through_core() {
        let scope = HiddenDirScope::new(PathBuf::from("/repo/packages/app"), vec![".next".into()]);

        let core: fallow_core::discover::HiddenDirScope = scope.clone().into();
        let engine: HiddenDirScope = core.into();

        assert_eq!(engine, scope);
        assert_eq!(engine.root(), scope.root());
        assert_eq!(engine.dirs(), scope.dirs());
    }

    #[test]
    fn categorized_entry_points_converts_from_core() {
        let entry = EntryPoint {
            path: PathBuf::from("/repo/src/index.ts"),
            source: EntryPointSource::DefaultIndex,
        };
        let mut core = fallow_core::discover::CategorizedEntryPoints::default();
        core.push_runtime(entry.clone());

        let engine: CategorizedEntryPoints = core.into();

        assert_eq!(engine.all.len(), 1);
        assert_eq!(engine.runtime.len(), 1);
        assert_eq!(engine.test.len(), 0);
        assert_eq!(engine.all[0].path, entry.path);
    }
}
