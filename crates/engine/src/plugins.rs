//! Plugin registry helpers and types exposed through the engine boundary.

use std::path::{Path, PathBuf};

use fallow_config::{ExternalPluginDef, PackageJson};

use crate::core_backend;

pub mod registry {
    use crate::core_backend;

    /// Invalid user-authored regex extracted from a plugin config file.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PluginRegexValidationError {
        pub(super) inner: core_backend::BackendPluginRegexValidationError,
    }

    impl From<core_backend::BackendPluginRegexValidationError> for PluginRegexValidationError {
        fn from(inner: core_backend::BackendPluginRegexValidationError) -> Self {
            Self { inner }
        }
    }

    /// Names of every built-in framework plugin in registry order.
    #[must_use]
    pub fn builtin_plugin_names() -> Vec<&'static str> {
        core_backend::builtin_plugin_names()
    }

    /// Format plugin regex validation errors for user-facing diagnostics.
    #[must_use]
    pub fn format_plugin_regex_errors(errors: &[PluginRegexValidationError]) -> String {
        let backend_errors = errors
            .iter()
            .map(|error| error.inner.clone())
            .collect::<Vec<_>>();
        core_backend::format_plugin_regex_errors(&backend_errors)
    }
}

/// Aggregated results from all active plugins for a project.
#[derive(Debug, Clone, Default)]
pub struct AggregatedPluginResult {
    inner: core_backend::BackendAggregatedPluginResult,
}

impl AggregatedPluginResult {
    pub(crate) const fn as_backend(&self) -> &core_backend::BackendAggregatedPluginResult {
        &self.inner
    }

    /// Names of active plugins.
    #[must_use]
    pub fn active_plugins(&self) -> &[String] {
        self.inner.active_plugins()
    }

    /// Merge active plugin names from another result, preserving insertion order.
    pub fn merge_active_plugins_from(&mut self, other: &Self) {
        self.inner.merge_active_plugins_from(&other.inner);
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
    pub fn new(external: Vec<ExternalPluginDef>) -> Self {
        Self {
            inner: core_backend::BackendPluginRegistry::new(external),
        }
    }

    /// Hidden directory names that should be traversed before full plugin execution.
    #[must_use]
    pub fn discovery_hidden_dirs(&self, pkg: &PackageJson, root: &Path) -> Vec<String> {
        self.inner.discovery_hidden_dirs(pkg, root)
    }

    /// Run all plugins against a project.
    pub fn try_run(
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
