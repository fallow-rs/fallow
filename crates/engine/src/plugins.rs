//! Plugin registry helpers and types exposed through the engine boundary.

use std::path::{Path, PathBuf};

use fallow_config::{EntryPointRole, ExternalPluginDef, PackageJson};

use crate::core_backend;

pub mod registry {
    use crate::core_backend;

    const BUILTIN_PLUGIN_NAMES: &[&str] = &[
        "nextjs",
        "nuxt",
        "pinia",
        "remix",
        "astro",
        "browser-extension",
        "wxt",
        "angular",
        "react-router",
        "redwoodsdk",
        "tanstack-router",
        "react-native",
        "expo",
        "expo-router",
        "firebase",
        "nestjs",
        "adonis",
        "docusaurus",
        "gatsby",
        "sveltekit",
        "nitro",
        "capacitor",
        "ionic",
        "sanity",
        "supabase",
        "vitepress",
        "rspress",
        "next-intl",
        "relay",
        "electron",
        "i18next",
        "qwik",
        "convex",
        "lit",
        "lexical",
        "obsidian",
        "content-collections",
        "contentlayer",
        "fumadocs",
        "mintlify",
        "velite",
        "ember",
        "vite",
        "vscode",
        "webpack",
        "rollup",
        "rolldown",
        "rspack",
        "rsbuild",
        "tsup",
        "tsdown",
        "pkg-utils",
        "parcel",
        "vitest",
        "jest",
        "playwright",
        "cypress",
        "mocha",
        "ava",
        "tap",
        "tsd",
        "k6",
        "storybook",
        "stryker",
        "karma",
        "cucumber",
        "webdriverio",
        "eslint",
        "biome",
        "stylelint",
        "prettier",
        "oxlint",
        "markdownlint",
        "cspell",
        "remark",
        "typescript",
        "babel",
        "swc",
        "tailwind",
        "postcss",
        "unocss",
        "pandacss",
        "prisma",
        "drizzle",
        "knex",
        "typeorm",
        "kysely",
        "turborepo",
        "nx",
        "changesets",
        "syncpack",
        "commitlint",
        "commitizen",
        "commit-and-tag-version",
        "semantic-release",
        "danger",
        "hardhat",
        "vercel",
        "wrangler",
        "opennext-cloudflare",
        "sentry",
        "husky",
        "lint-staged",
        "lefthook",
        "simple-git-hooks",
        "svgo",
        "svgr",
        "graphql-codegen",
        "typedoc",
        "openapi-ts",
        "plop",
        "c8",
        "nyc",
        "msw",
        "napi-rs",
        "opencode",
        "nodemon",
        "pm2",
        "dependency-cruiser",
        "wuchale",
        "varlock",
        "pnpm",
        "bun",
    ];

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
    #[must_use]
    pub fn builtin_plugin_names() -> Vec<&'static str> {
        BUILTIN_PLUGIN_NAMES.to_vec()
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
    pub fn merge_active_plugins_from(&mut self, other: &Self) {
        self.inner.merge_active_plugins_from(&other.inner);
    }

    pub(crate) fn entry_patterns(&self) -> Vec<PluginEntryPattern> {
        self.inner.entry_patterns()
    }

    pub(crate) fn support_patterns(&self) -> Vec<PluginNamedPattern> {
        self.inner.support_patterns()
    }

    pub(crate) fn setup_files(&self) -> Vec<PluginSetupFile> {
        self.inner.setup_files()
    }

    pub(crate) fn entry_point_role(&self, plugin_name: &str) -> EntryPointRole {
        self.inner.entry_point_role(plugin_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginPathRule {
    pub(crate) pattern: String,
    pub(crate) exclude_globs: Vec<String>,
    pub(crate) exclude_regexes: Vec<String>,
    pub(crate) exclude_segment_regexes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginEntryPattern {
    pub(crate) rule: PluginPathRule,
    pub(crate) plugin_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginNamedPattern {
    pub(crate) pattern: String,
    pub(crate) plugin_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginSetupFile {
    pub(crate) path: PathBuf,
    pub(crate) plugin_name: String,
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
