//! Plugin registry helpers and types exposed through the engine boundary.

use std::path::Path;

use fallow_config::{ExternalPluginDef, PackageJson};

use crate::core_backend;

/// External-plugin dry-run primitives for the CLI's `plugin-check` command.
pub use crate::core_backend::{
    CheckWarning, ManifestResult, RuleReport, WarningKind, check_manifest_entries,
    is_external_plugin_active,
};

/// Built-in plugin name roster.
pub mod registry {
    use crate::core_backend;

    /// Names of every built-in framework plugin in registry order.
    ///
    /// Delegates to the core registry rather than mirroring it. A hand-kept
    /// copy drifted here once already, silently omitting `deno`, and nothing
    /// pinned the two together.
    #[must_use]
    pub fn builtin_plugin_names() -> Vec<&'static str> {
        core_backend::builtin_plugin_names()
    }
}

/// Aggregated results from all active plugins for a project.
#[derive(Debug, Clone, Default)]
pub struct AggregatedPluginResult {
    inner: core_backend::BackendAggregatedPluginResult,
}

impl AggregatedPluginResult {
    /// Names of active plugins.
    #[must_use]
    pub fn active_plugins(&self) -> &[String] {
        self.inner.active_plugins()
    }

    /// The plugin stage's advisories (`plugin-config-unreadable`,
    /// `plugin-effect-not-modeled`), with paths rendered against `root`.
    #[must_use]
    pub fn plugin_diagnostics(&self, root: &Path) -> Vec<fallow_config::WorkspaceDiagnostic> {
        self.inner.plugin_diagnostics(root)
    }

    pub(crate) fn backend(&self) -> &core_backend::BackendAggregatedPluginResult {
        &self.inner
    }
}

impl From<core_backend::BackendAggregatedPluginResult> for AggregatedPluginResult {
    fn from(inner: core_backend::BackendAggregatedPluginResult) -> Self {
        Self { inner }
    }
}

/// Registry of all available plugins.
pub struct PluginRegistry {
    inner: core_backend::BackendPluginRegistry,
}

impl PluginRegistry {
    /// Create a registry with all built-in plugins and optional external plugins.
    #[must_use]
    pub(crate) fn new(external: Vec<ExternalPluginDef>) -> Self {
        Self {
            inner: core_backend::BackendPluginRegistry::new(external),
        }
    }

    /// Hidden directory names that should be traversed before full plugin execution.
    #[must_use]
    pub(crate) fn discovery_hidden_dirs(&self, pkg: &PackageJson, root: &Path) -> Vec<String> {
        self.inner.discovery_hidden_dirs(pkg, root)
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new(vec![])
    }
}

#[cfg(test)]
mod roster_tests {
    /// The engine mirrored core's roster by hand and the copy drifted, omitting
    /// `deno`. Delegation removes the copy, so there is nothing left to diverge;
    /// this pins the name the drift lost and that the roster is really populated.
    ///
    /// The roster is read through `core_backend`, not from `fallow_core`
    /// directly, because the boundary guard requires every crossing to go
    /// through that adapter.
    #[test]
    fn roster_carries_every_registered_plugin() {
        let names = super::registry::builtin_plugin_names();
        assert!(
            names.contains(&"deno"),
            "deno is registered in core and must reach the roster"
        );
        assert!(
            names.len() > 100,
            "roster looks truncated: {} names",
            names.len()
        );
    }
}
