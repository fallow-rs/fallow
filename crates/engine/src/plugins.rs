//! Plugin registry helpers and types exposed through the engine boundary.

use std::path::{Path, PathBuf};

use fallow_config::{ExternalPluginDef, PackageJson};

pub mod registry {
    /// Invalid user-authored regex extracted from a plugin config file.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PluginRegexValidationError {
        pub(super) inner: fallow_core::plugins::registry::PluginRegexValidationError,
    }

    impl From<fallow_core::plugins::registry::PluginRegexValidationError>
        for PluginRegexValidationError
    {
        fn from(inner: fallow_core::plugins::registry::PluginRegexValidationError) -> Self {
            Self { inner }
        }
    }

    /// Names of every built-in framework plugin in registry order.
    #[must_use]
    pub fn builtin_plugin_names() -> Vec<&'static str> {
        fallow_core::plugins::registry::builtin_plugin_names()
    }

    /// Format plugin regex validation errors for user-facing diagnostics.
    #[must_use]
    pub fn format_plugin_regex_errors(errors: &[PluginRegexValidationError]) -> String {
        let core_errors = errors
            .iter()
            .map(|error| error.inner.clone())
            .collect::<Vec<_>>();
        fallow_core::plugins::registry::format_plugin_regex_errors(&core_errors)
    }
}

/// Aggregated results from all active plugins for a project.
#[derive(Debug, Clone, Default)]
pub struct AggregatedPluginResult {
    inner: fallow_core::plugins::AggregatedPluginResult,
}

impl AggregatedPluginResult {
    pub(crate) const fn as_core(&self) -> &fallow_core::plugins::AggregatedPluginResult {
        &self.inner
    }

    /// Names of active plugins.
    #[must_use]
    pub fn active_plugins(&self) -> &[String] {
        &self.inner.active_plugins
    }

    /// Merge active plugin names from another result, preserving insertion order.
    pub fn merge_active_plugins_from(&mut self, other: &Self) {
        for plugin_name in &other.inner.active_plugins {
            if !self.inner.active_plugins.contains(plugin_name) {
                self.inner.active_plugins.push(plugin_name.clone());
            }
        }
    }
}

impl From<fallow_core::plugins::AggregatedPluginResult> for AggregatedPluginResult {
    fn from(inner: fallow_core::plugins::AggregatedPluginResult) -> Self {
        Self { inner }
    }
}

/// Registry of all available plugins.
pub struct PluginRegistry {
    inner: fallow_core::plugins::PluginRegistry,
}

impl PluginRegistry {
    /// Create a registry with all built-in plugins and optional external plugins.
    #[must_use]
    pub fn new(external: Vec<ExternalPluginDef>) -> Self {
        Self {
            inner: fallow_core::plugins::PluginRegistry::new(external),
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
        base.inner.active_plugins.push("nextjs".into());
        let mut incoming = AggregatedPluginResult::default();
        incoming.inner.active_plugins.push("nextjs".into());
        incoming.inner.active_plugins.push("vitest".into());

        base.merge_active_plugins_from(&incoming);

        assert_eq!(base.active_plugins(), ["nextjs", "vitest"]);
    }
}
