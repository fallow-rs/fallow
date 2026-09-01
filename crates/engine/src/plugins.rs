//! Plugin registry helpers and types exposed through the engine boundary.

use std::path::{Path, PathBuf};

use fallow_config::{ExternalPluginDef, PackageJson};

use crate::core_backend;

/// External-plugin dry-run primitives for the CLI's `plugin-check` command.
pub use crate::core_backend::{
    CheckWarning, ManifestResult, RuleReport, WarningKind, check_manifest_entries,
    is_external_plugin_active,
};

/// Built-in plugin name roster and plugin-regex validation diagnostics.
pub mod registry {
    use crate::core_backend;

    /// Invalid user-authored regex extracted from a plugin config file.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PluginRegexValidationError {
        message: String,
    }

    impl From<core_backend::BackendPluginRegexValidationError> for PluginRegexValidationError {
        fn from(inner: core_backend::BackendPluginRegexValidationError) -> Self {
            Self {
                message: inner.message(),
            }
        }
    }

    /// Names of every built-in framework plugin in registry order.
    ///
    /// Delegates to the core registry rather than mirroring it. A hand-kept
    /// copy drifted here once already, silently omitting `deno`, and nothing
    /// pinned the two together.
    #[must_use]
    pub fn builtin_plugin_names() -> Vec<&'static str> {
        core_backend::builtin_plugin_names()
    }

    /// Format plugin regex validation errors for user-facing diagnostics.
    #[must_use]
    pub fn format_plugin_regex_errors(errors: &[PluginRegexValidationError]) -> String {
        let joined = errors
            .iter()
            .map(|error| error.message.as_str())
            .collect::<Vec<_>>();
        format!(
            "invalid plugin regex configuration:\n  - {}\n\nRewrite the plugin config with Rust-compatible regex syntax, or remove unsupported constructs such as JavaScript lookahead and lookbehind.",
            joined.join("\n  - ")
        )
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

    /// Merge active plugin names from another result, preserving insertion order.
    pub(crate) fn merge_active_plugins_from(&mut self, other: &Self) {
        self.inner.merge_active_plugins_from(&other.inner);
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

    /// Run all plugins against a project.
    pub(crate) fn try_run(
        &self,
        pkg: &PackageJson,
        root: &Path,
        discovered_files: &[PathBuf],
    ) -> Result<AggregatedPluginResult, Vec<registry::PluginRegexValidationError>> {
        self.inner
            .try_run(pkg, root, discovered_files)
            .map(Into::into)
            .map_err(|errors| errors.into_iter().map(Into::into).collect())
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new(vec![])
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{AggregatedPluginResult, PluginRegistry};

    #[test]
    fn plugin_registry_try_run_returns_engine_result() {
        let registry = PluginRegistry::default();
        let result = registry
            .try_run(
                &fallow_config::PackageJson::default(),
                &PathBuf::from("/repo"),
                &[],
            )
            .expect("empty package should not produce regex errors");

        assert!(result.active_plugins().is_empty());
    }

    #[test]
    fn aggregated_plugin_result_merges_active_plugins() {
        let mut base = AggregatedPluginResult::default();
        base.inner.push_active_plugin_for_test("nextjs");
        let mut incoming = AggregatedPluginResult::default();
        incoming.inner.push_active_plugin_for_test("nextjs");
        incoming.inner.push_active_plugin_for_test("vitest");

        base.merge_active_plugins_from(&incoming);

        assert_eq!(base.active_plugins(), ["nextjs", "vitest"]);
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
