//! Engine-owned inventory helpers for list-style project metadata.

use fallow_config::{ResolvedConfig, WorkspaceInfo};

use crate::{
    discover::{DiscoveredFile, EntryPoint},
    plugins::AggregatedPluginResult,
    session::AnalysisSession,
};

/// Error raised while assembling list inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListInventoryError {
    /// The plugin stage failed, for example on an invalid user-authored plugin
    /// regex. Carries the message the analysis reports for the same failure.
    Plugins(String),
}

impl ListInventoryError {
    /// The user-facing message.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Plugins(message) => message,
        }
    }
}

/// The plugins and entry points of a project, as the analysis sees them.
#[derive(Debug, Clone)]
pub struct ListingInventory {
    /// The plugin stage's result: active plugins and plugin diagnostics.
    pub plugins: AggregatedPluginResult,
    /// Every entry point the analysis uses, deduplicated. `None` when the
    /// caller did not ask for entry points, so their discovery did not run.
    pub entry_points: Option<Vec<EntryPoint>>,
}

/// Run the analysis prelude (plugins and scripts) and its entry-point
/// discovery over the session's whole discovery.
///
/// One implementation for the listing and the analysis: the workspace merge,
/// the auto-import gate, the script-derived entries and the plugin diagnostics
/// are the same, so `fallow list --entry-points` names the entry points the
/// analysis uses, and the listing can report the `plugin-config-unreadable`
/// and `plugin-effect-not-modeled` diagnostics the analysis reports (issue
/// #2804). A path or changed-file scope narrows what a listing shows, never
/// which plugins are active, so the caller filters the result afterwards.
///
/// `with_entry_points` false skips the entry-point discovery, for a listing
/// of plugins only.
///
/// # Errors
///
/// Returns the plugin stage's error, such as an invalid plugin regex.
pub fn collect_listing_inventory(
    session: &AnalysisSession,
    with_entry_points: bool,
) -> Result<ListingInventory, ListInventoryError> {
    let prelude = crate::core_backend::prepare_dead_code_backend_prelude(
        session.config(),
        session.discovery(),
    )
    .map_err(|err| ListInventoryError::Plugins(err.message().to_owned()))?;
    let entry_points = with_entry_points.then(|| {
        crate::core_backend::discover_dead_code_entry_points(&prelude)
            .all()
            .to_vec()
    });
    let plugins = AggregatedPluginResult::from(prelude.plugin_result());
    prelude.finish();
    Ok(ListingInventory {
        plugins,
        entry_points,
    })
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

    fn session_at(root: &Path) -> AnalysisSession {
        let config = FallowConfig::default().resolve(
            root.to_path_buf(),
            OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
        AnalysisSession::from_resolved_config(config).expect("session")
    }

    #[test]
    fn active_plugins_ignores_missing_package_manifests() {
        let temp = tempfile::tempdir().expect("tempdir");
        let session = session_at(temp.path());
        let inventory =
            collect_listing_inventory(&session, true).expect("missing package should not fail");

        assert!(inventory.plugins.active_plugins().is_empty());
        let plugins_only =
            collect_listing_inventory(&session, false).expect("missing package should not fail");
        assert!(
            plugins_only.entry_points.is_none(),
            "a plugins-only listing skips entry-point discovery"
        );
    }

    #[test]
    fn entry_points_accept_plugin_result() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies":{"next":"15.0.0"}}"#,
        )
        .expect("package manifest");
        for file in ["src/app/dashboard/page.tsx", "src/helpers/format.ts"] {
            let path = temp.path().join(file);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, "export const x = 1;\n").expect("source");
        }
        let session = session_at(temp.path());
        let config = session.config();
        let discovered = session.files();
        let inventory = collect_listing_inventory(&session, true).expect("Next.js plugins load");
        assert!(
            inventory
                .entry_points
                .as_deref()
                .unwrap_or_default()
                .iter()
                .any(|entry| entry.path.ends_with("src/app/dashboard/page.tsx")),
            "the listing inventory carries the analysis entry points"
        );
        let plugin_result = inventory.plugins;

        let entries = collect_entry_points(config, discovered, &[], None);

        assert!(
            entries
                .iter()
                .all(|entry| !matches!(entry.source, EntryPointSource::Plugin { .. }))
        );

        let entries = collect_entry_points(config, discovered, &[], Some(&plugin_result));
        let plugin_entries: Vec<_> = entries
            .iter()
            .filter_map(|entry| match &entry.source {
                EntryPointSource::Plugin { name } => Some((
                    entry.path.strip_prefix(&config.root).expect("under root"),
                    name.as_str(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            plugin_entries,
            vec![(Path::new("src/app/dashboard/page.tsx"), "nextjs")]
        );
    }
}
