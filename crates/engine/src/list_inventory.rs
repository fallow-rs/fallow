//! Engine-owned inventory helpers for list-style project metadata.

use std::path::{Path, PathBuf};

use fallow_config::{ResolvedConfig, WorkspaceInfo};

use crate::{
    discover::{DiscoveredFile, EntryPoint},
    plugins::{AggregatedPluginResult, PluginRegistry, registry::PluginRegexValidationError},
};

/// Error raised while assembling list inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListInventoryError {
    /// One or more plugin regexes failed validation.
    PluginRegex(Vec<PluginRegexValidationError>),
}

/// Collect active plugins from the root package and every workspace package.
///
/// Missing package manifests are ignored, matching the historical list-command
/// behavior. Deno members are loaded via `deno.json` the same way analysis does.
///
/// # Errors
///
/// Returns plugin regex validation errors from user-authored plugin settings.
pub fn collect_active_plugins(
    root: &Path,
    config: &ResolvedConfig,
    discovered: &[DiscoveredFile],
    workspaces: &[WorkspaceInfo],
) -> Result<AggregatedPluginResult, ListInventoryError> {
    let file_paths = discovered
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let registry = PluginRegistry::new(config.external_plugins.clone());
    let mut result = run_package_plugins(&registry, root, &file_paths)?.unwrap_or_default();

    for workspace in workspaces {
        let Some(workspace_result) = run_package_plugins(&registry, &workspace.root, &file_paths)?
        else {
            continue;
        };
        result.merge_active_plugins_from(&workspace_result);
    }

    Ok(result)
}

/// Collect root, workspace, and plugin entry points in one engine-owned pass.
#[must_use]
pub fn collect_entry_points(
    config: &ResolvedConfig,
    discovered: &[DiscoveredFile],
    workspaces: &[WorkspaceInfo],
    plugin_result: Option<&AggregatedPluginResult>,
) -> Vec<EntryPoint> {
    let mut entries = crate::discover::discover_entry_points(config, discovered);
    for workspace in workspaces {
        entries.extend(crate::discover::discover_workspace_entry_points(
            &workspace.root,
            config,
            discovered,
        ));
    }
    if let Some(plugin_result) = plugin_result {
        entries.extend(crate::discover::discover_plugin_entry_points(
            plugin_result,
            config,
            discovered,
        ));
    }
    entries
}

fn run_package_plugins(
    registry: &PluginRegistry,
    package_root: &Path,
    file_paths: &[PathBuf],
) -> Result<Option<AggregatedPluginResult>, ListInventoryError> {
    let Some(package) = fallow_config::load_dir_package_json(package_root) else {
        return Ok(None);
    };
    registry
        .try_run(&package, package_root, file_paths)
        .map(Some)
        .map_err(ListInventoryError::PluginRegex)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fallow_config::{FallowConfig, WorkspaceInfo};
    use fallow_types::output_format::OutputFormat;

    use super::*;
    use crate::discover::{EntryPointSource, FileId};

    #[test]
    fn entry_points_include_root_and_workspace_entries() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let config = FallowConfig::default().resolve(
            root.to_path_buf(),
            OutputFormat::Json,
            1,
            false,
            true,
            None,
        );
        let workspace = WorkspaceInfo {
            root: root.join("packages/web"),
            name: "web".to_owned(),
            is_internal_dependency: false,
        };
        let discovered = vec![
            DiscoveredFile {
                id: FileId(0),
                path: root.join("src/main.ts"),
                size_bytes: 0,
            },
            DiscoveredFile {
                id: FileId(1),
                path: root.join("packages/web/src/index.ts"),
                size_bytes: 0,
            },
        ];

        let entries = collect_entry_points(&config, &discovered, &[workspace], None);

        assert!(
            entries
                .iter()
                .any(|entry| entry.path.ends_with("src/main.ts"))
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.path.ends_with("packages/web/src/index.ts"))
        );
    }

    #[test]
    fn active_plugins_ignores_missing_package_manifests() {
        let config = FallowConfig::default().resolve(
            Path::new("/missing-project").to_path_buf(),
            OutputFormat::Json,
            1,
            false,
            true,
            None,
        );
        let result = collect_active_plugins(Path::new("/missing-project"), &config, &[], &[])
            .expect("missing package should not fail");

        assert!(result.active_plugins().is_empty());
    }

    #[test]
    fn entry_points_accept_plugin_result() {
        let config = FallowConfig::default().resolve(
            Path::new("/project").to_path_buf(),
            OutputFormat::Json,
            1,
            false,
            true,
            None,
        );
        let discovered = Vec::new();

        let entries = collect_entry_points(&config, &discovered, &[], None);

        assert!(
            entries
                .iter()
                .all(|entry| !matches!(entry.source, EntryPointSource::Plugin { .. }))
        );
    }
}
