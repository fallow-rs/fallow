//! Plugin registry: discovers active plugins, collects patterns, parses configs.

use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use fallow_config::{EntryPointRole, ExternalPluginDef, PackageJson, UsedClassMemberRule};

use super::{PathRule, Plugin, PluginUsedExportRule};

pub(crate) mod builtin;
mod helpers;

use helpers::{
    check_has_config_file, discover_config_files, prepare_config_pattern, process_config_result,
    process_external_plugins, process_static_patterns,
};

// ESLint is included because each workspace owns its own eslint.config.{mjs,js,...}
// that may import a shared workspace eslint-config package. Those transitive deps
// (e.g. eslint-config-next, eslint-plugin-react) are declared in the workspace's
// devDependencies and will be flagged as unused if we skip config parsing here.
fn must_parse_workspace_config_when_root_active(plugin_name: &str) -> bool {
    matches!(
        plugin_name,
        "eslint" | "docusaurus" | "jest" | "tanstack-router" | "vitest"
    )
}

/// Registry of all available plugins (built-in + external).
pub struct PluginRegistry {
    plugins: Vec<Box<dyn Plugin>>,
    external_plugins: Vec<ExternalPluginDef>,
}

/// Aggregated results from all active plugins for a project.
#[derive(Debug, Default)]
pub struct AggregatedPluginResult {
    /// All entry point patterns from active plugins: (rule, plugin_name).
    pub entry_patterns: Vec<(PathRule, String)>,
    /// Coverage role for each plugin contributing entry point patterns.
    pub entry_point_roles: rustc_hash::FxHashMap<String, EntryPointRole>,
    /// All config file patterns from active plugins.
    pub config_patterns: Vec<String>,
    /// All always-used file patterns from active plugins: (pattern, plugin_name).
    pub always_used: Vec<(String, String)>,
    /// All used export rules from active plugins.
    pub used_exports: Vec<PluginUsedExportRule>,
    /// Class member rules contributed by active plugins that should never be
    /// flagged as unused. Extends the built-in Angular/React lifecycle allowlist
    /// with framework-invoked method names, optionally scoped by class heritage.
    pub used_class_members: Vec<UsedClassMemberRule>,
    /// Dependencies referenced in config files (should not be flagged unused).
    pub referenced_dependencies: Vec<String>,
    /// Additional always-used files discovered from config parsing: (pattern, plugin_name).
    pub discovered_always_used: Vec<(String, String)>,
    /// Setup files discovered from config parsing: (path, plugin_name).
    pub setup_files: Vec<(PathBuf, String)>,
    /// Tooling dependencies (should not be flagged as unused devDeps).
    pub tooling_dependencies: Vec<String>,
    /// Package names discovered as used in package.json scripts (binary invocations).
    pub script_used_packages: FxHashSet<String>,
    /// Import prefixes for virtual modules provided by active frameworks.
    /// Imports matching these prefixes should not be flagged as unlisted dependencies.
    pub virtual_module_prefixes: Vec<String>,
    /// Package name suffixes that identify virtual or convention-based specifiers.
    /// Extracted package names ending with any of these suffixes are not flagged as unlisted.
    pub virtual_package_suffixes: Vec<String>,
    /// Import suffixes for build-time generated relative imports.
    /// Unresolved imports ending with these suffixes are suppressed.
    pub generated_import_patterns: Vec<String>,
    /// Path alias mappings from active plugins (prefix → replacement directory).
    /// Used by the resolver to substitute import prefixes before re-resolving.
    pub path_aliases: Vec<(String, String)>,
    /// Names of active plugins.
    pub active_plugins: Vec<String>,
    /// Test fixture glob patterns from active plugins: (pattern, plugin_name).
    pub fixture_patterns: Vec<(String, String)>,
    /// Absolute directories contributed by plugins that should be searched
    /// when resolving SCSS/Sass `@import`/`@use` specifiers. Populated from
    /// Angular's `stylePreprocessorOptions.includePaths` and equivalent
    /// framework settings. See issue #103.
    pub scss_include_paths: Vec<PathBuf>,
}

impl PluginRegistry {
    /// Create a registry with all built-in plugins and optional external plugins.
    #[must_use]
    pub fn new(external: Vec<ExternalPluginDef>) -> Self {
        Self {
            plugins: builtin::create_builtin_plugins(),
            external_plugins: external,
        }
    }

    /// Hidden directory names that should be traversed before full plugin execution.
    ///
    /// Source discovery runs before plugin config parsing, so this helper only uses
    /// package-activation checks and static plugin metadata.
    #[must_use]
    pub fn discovery_hidden_dirs(&self, pkg: &PackageJson, root: &Path) -> Vec<String> {
        let all_deps = pkg.all_dependency_names();
        let mut seen = FxHashSet::default();
        let mut dirs = Vec::new();

        for plugin in &self.plugins {
            if !plugin.is_enabled_with_deps(&all_deps, root) {
                continue;
            }
            for dir in plugin.discovery_hidden_dirs() {
                if seen.insert(*dir) {
                    dirs.push((*dir).to_string());
                }
            }
        }

        dirs
    }

    /// Run all plugins against a project, returning aggregated results.
    ///
    /// This discovers which plugins are active, collects their static patterns,
    /// then parses any config files to extract dynamic information.
    pub fn run(
        &self,
        pkg: &PackageJson,
        root: &Path,
        discovered_files: &[PathBuf],
    ) -> AggregatedPluginResult {
        self.run_with_search_roots(pkg, root, discovered_files, &[root], false)
    }

    /// Run all plugins against a project with explicit config-file search roots.
    ///
    /// `config_search_roots` should stay narrowly focused to directories that are
    /// already known to matter for this project. Broad recursive scans are
    /// intentionally avoided because they become prohibitively expensive on
    /// large monorepos with populated `node_modules` trees.
    ///
    /// `production_mode` controls the FS fallback for source-extension config
    /// patterns. In production mode the source walker excludes `*.config.*` so
    /// the FS walk is required; otherwise Phase 3a's in-memory matcher covers
    /// them and the walk is skipped.
    pub fn run_with_search_roots(
        &self,
        pkg: &PackageJson,
        root: &Path,
        discovered_files: &[PathBuf],
        config_search_roots: &[&Path],
        production_mode: bool,
    ) -> AggregatedPluginResult {
        let _span = tracing::info_span!("run_plugins").entered();
        let mut result = AggregatedPluginResult::default();

        // Phase 1: Determine which plugins are active
        // Compute deps once to avoid repeated Vec<String> allocation per plugin
        let all_deps = pkg.all_dependency_names();
        let active: Vec<&dyn Plugin> = self
            .plugins
            .iter()
            .filter(|p| p.is_enabled_with_deps(&all_deps, root))
            .map(AsRef::as_ref)
            .collect();

        tracing::info!(
            plugins = active
                .iter()
                .map(|p| p.name())
                .collect::<Vec<_>>()
                .join(", "),
            "active plugins"
        );

        // Warn when meta-frameworks are active but their generated configs are missing.
        // Without these, tsconfig extends chains break and import resolution fails.
        check_meta_framework_prerequisites(&active, root);

        // Phase 2: Collect static patterns from active plugins
        for plugin in &active {
            process_static_patterns(*plugin, root, &mut result);
        }

        // Phase 2b: Process external plugins (includes inline framework definitions)
        process_external_plugins(
            &self.external_plugins,
            &all_deps,
            root,
            discovered_files,
            &mut result,
        );

        // Phase 3: Find and parse config files for dynamic resolution
        // Pre-compile all config patterns. Source-extension root-anchored
        // patterns are wrapped with `**/` so they match nested files via the
        // discovered file set (Phase 3a), letting Phase 3b skip those plugins
        // and avoid a per-directory stat storm on large monorepos.
        let config_matchers: Vec<(&dyn Plugin, Vec<globset::GlobMatcher>)> = active
            .iter()
            .filter(|p| !p.config_patterns().is_empty())
            .map(|p| {
                let matchers: Vec<globset::GlobMatcher> = p
                    .config_patterns()
                    .iter()
                    .filter_map(|pat| {
                        let prepared = prepare_config_pattern(pat);
                        globset::Glob::new(&prepared)
                            .ok()
                            .map(|g| g.compile_matcher())
                    })
                    .collect();
                (*p, matchers)
            })
            .collect();

        // Build relative paths lazily: only needed when config matchers exist
        // or plugins have package_json_config_key. Skip entirely for projects
        // with no config-parsing plugins (e.g., only React), avoiding O(files)
        // String allocations.
        let needs_relative_files = !config_matchers.is_empty()
            || active.iter().any(|p| p.package_json_config_key().is_some());
        let relative_files: Vec<(PathBuf, String)> = if needs_relative_files {
            discovered_files
                .iter()
                .map(|f| {
                    let rel = f
                        .strip_prefix(root)
                        .unwrap_or(f)
                        .to_string_lossy()
                        .into_owned();
                    (f.clone(), rel)
                })
                .collect()
        } else {
            Vec::new()
        };

        if !config_matchers.is_empty() {
            // Phase 3a: Match config files from discovered source files
            let mut resolved_plugins: FxHashSet<&str> = FxHashSet::default();

            for (plugin, matchers) in &config_matchers {
                for (abs_path, rel_path) in &relative_files {
                    if matchers.iter().any(|m| m.is_match(rel_path.as_str()))
                        && let Ok(source) = std::fs::read_to_string(abs_path)
                    {
                        let plugin_result = plugin.resolve_config(abs_path, &source, root);
                        if !plugin_result.is_empty() {
                            resolved_plugins.insert(plugin.name());
                            tracing::debug!(
                                plugin = plugin.name(),
                                config = rel_path.as_str(),
                                entries = plugin_result.entry_patterns.len(),
                                deps = plugin_result.referenced_dependencies.len(),
                                "resolved config"
                            );
                            process_config_result(plugin.name(), plugin_result, &mut result);
                        }
                    }
                }
            }

            // Phase 3b: Filesystem fallback for JSON config files.
            // JSON files (angular.json, project.json) are not in the discovered file set
            // because fallow only discovers JS/TS/CSS/Vue/etc. files. In production
            // mode, source-extension configs (`*.config.*`, dotfiles) are also
            // excluded from the walker, so the FS walk runs for those patterns too.
            let json_configs = discover_config_files(
                &config_matchers,
                &resolved_plugins,
                config_search_roots,
                production_mode,
            );
            for (abs_path, plugin) in &json_configs {
                if let Ok(source) = std::fs::read_to_string(abs_path) {
                    let plugin_result = plugin.resolve_config(abs_path, &source, root);
                    if !plugin_result.is_empty() {
                        let rel = abs_path
                            .strip_prefix(root)
                            .map(|p| p.to_string_lossy())
                            .unwrap_or_default();
                        tracing::debug!(
                            plugin = plugin.name(),
                            config = %rel,
                            entries = plugin_result.entry_patterns.len(),
                            deps = plugin_result.referenced_dependencies.len(),
                            "resolved config (filesystem fallback)"
                        );
                        process_config_result(plugin.name(), plugin_result, &mut result);
                    }
                }
            }
        }

        // Phase 4: Package.json inline config fallback
        // For plugins that define `package_json_config_key()`, check if the root
        // package.json contains that key and no standalone config file was found.
        for plugin in &active {
            if let Some(key) = plugin.package_json_config_key()
                && !check_has_config_file(*plugin, &config_matchers, &relative_files)
            {
                // Try to extract the key from package.json
                let pkg_path = root.join("package.json");
                if let Ok(content) = std::fs::read_to_string(&pkg_path)
                    && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
                    && let Some(config_value) = json.get(key)
                {
                    let config_json = serde_json::to_string(config_value).unwrap_or_default();
                    let fake_path = root.join(format!("{key}.config.json"));
                    let plugin_result = plugin.resolve_config(&fake_path, &config_json, root);
                    if !plugin_result.is_empty() {
                        tracing::debug!(
                            plugin = plugin.name(),
                            key = key,
                            "resolved inline package.json config"
                        );
                        process_config_result(plugin.name(), plugin_result, &mut result);
                    }
                }
            }
        }

        result
    }

    /// Fast variant of `run()` for workspace packages.
    ///
    /// Reuses pre-compiled config matchers and pre-computed relative files from the root
    /// project run, avoiding repeated glob compilation and path computation per workspace.
    /// Skips package.json inline config (workspace packages rarely have inline configs).
    #[expect(
        clippy::too_many_arguments,
        reason = "Each parameter is a distinct, small value with no natural grouping; \
                  bundling them into a struct hurts call-site readability."
    )]
    pub fn run_workspace_fast(
        &self,
        pkg: &PackageJson,
        root: &Path,
        project_root: &Path,
        precompiled_config_matchers: &[(&dyn Plugin, Vec<globset::GlobMatcher>)],
        relative_files: &[(PathBuf, String)],
        skip_config_plugins: &FxHashSet<&str>,
        production_mode: bool,
    ) -> AggregatedPluginResult {
        let _span = tracing::info_span!("run_plugins").entered();
        let mut result = AggregatedPluginResult::default();

        // Phase 1: Determine which plugins are active (with pre-computed deps)
        let all_deps = pkg.all_dependency_names();
        let active: Vec<&dyn Plugin> = self
            .plugins
            .iter()
            .filter(|p| p.is_enabled_with_deps(&all_deps, root))
            .map(AsRef::as_ref)
            .collect();

        let workspace_files: Vec<PathBuf> = relative_files
            .iter()
            .map(|(abs_path, _)| abs_path.clone())
            .collect();

        tracing::info!(
            plugins = active
                .iter()
                .map(|p| p.name())
                .collect::<Vec<_>>()
                .join(", "),
            "active plugins"
        );

        process_external_plugins(
            &self.external_plugins,
            &all_deps,
            root,
            &workspace_files,
            &mut result,
        );

        // Early exit if no plugins are active (common for leaf workspace packages)
        if active.is_empty() && result.active_plugins.is_empty() {
            return result;
        }

        // Phase 2: Collect static patterns from active plugins
        for plugin in &active {
            process_static_patterns(*plugin, root, &mut result);
        }

        // Phase 3: Find and parse config files using pre-compiled matchers
        // Only check matchers for plugins that are active in this workspace
        let active_names: FxHashSet<&str> = active.iter().map(|p| p.name()).collect();
        let workspace_matchers: Vec<_> = precompiled_config_matchers
            .iter()
            .filter(|(p, _)| {
                active_names.contains(p.name())
                    && (!skip_config_plugins.contains(p.name())
                        || must_parse_workspace_config_when_root_active(p.name()))
            })
            .map(|(plugin, matchers)| (*plugin, matchers.clone()))
            .collect();

        let mut resolved_ws_plugins: FxHashSet<&str> = FxHashSet::default();
        if !workspace_matchers.is_empty() {
            for (plugin, matchers) in &workspace_matchers {
                for (abs_path, rel_path) in relative_files {
                    if matchers.iter().any(|m| m.is_match(rel_path.as_str()))
                        && let Ok(source) = std::fs::read_to_string(abs_path)
                    {
                        let plugin_result = plugin.resolve_config(abs_path, &source, root);
                        if !plugin_result.is_empty() {
                            resolved_ws_plugins.insert(plugin.name());
                            tracing::debug!(
                                plugin = plugin.name(),
                                config = rel_path.as_str(),
                                entries = plugin_result.entry_patterns.len(),
                                deps = plugin_result.referenced_dependencies.len(),
                                "resolved config"
                            );
                            process_config_result(plugin.name(), plugin_result, &mut result);
                        }
                    }
                }
            }
        }

        // Phase 3b: Filesystem fallback for JSON config files at the project root.
        // Config files like angular.json live at the monorepo root, but Angular is
        // only active in workspace packages. Check the project root for unresolved
        // config patterns.
        let ws_json_configs = if root == project_root {
            discover_config_files(
                &workspace_matchers,
                &resolved_ws_plugins,
                &[root],
                production_mode,
            )
        } else {
            discover_config_files(
                &workspace_matchers,
                &resolved_ws_plugins,
                &[root, project_root],
                production_mode,
            )
        };
        // Parse discovered JSON config files
        for (abs_path, plugin) in &ws_json_configs {
            if let Ok(source) = std::fs::read_to_string(abs_path) {
                let plugin_result = plugin.resolve_config(abs_path, &source, root);
                if !plugin_result.is_empty() {
                    let rel = abs_path
                        .strip_prefix(project_root)
                        .map(|p| p.to_string_lossy())
                        .unwrap_or_default();
                    tracing::debug!(
                        plugin = plugin.name(),
                        config = %rel,
                        entries = plugin_result.entry_patterns.len(),
                        deps = plugin_result.referenced_dependencies.len(),
                        "resolved config (workspace filesystem fallback)"
                    );
                    process_config_result(plugin.name(), plugin_result, &mut result);
                }
            }
        }

        result
    }

    /// Pre-compile config pattern glob matchers for all plugins that have config patterns.
    /// Returns a vec of (plugin, matchers) pairs that can be reused across multiple `run_workspace_fast` calls.
    #[must_use]
    pub fn precompile_config_matchers(&self) -> Vec<(&dyn Plugin, Vec<globset::GlobMatcher>)> {
        self.plugins
            .iter()
            .filter(|p| !p.config_patterns().is_empty())
            .map(|p| {
                let matchers: Vec<globset::GlobMatcher> = p
                    .config_patterns()
                    .iter()
                    .filter_map(|pat| {
                        let prepared = prepare_config_pattern(pat);
                        globset::Glob::new(&prepared)
                            .ok()
                            .map(|g| g.compile_matcher())
                    })
                    .collect();
                (p.as_ref(), matchers)
            })
            .collect()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new(vec![])
    }
}

/// Warn when meta-frameworks are active but their generated configs are missing.
///
/// Meta-frameworks like Nuxt and Astro generate tsconfig/types files during a
/// "prepare" step. Without these, the tsconfig extends chain breaks and
/// extensionless imports fail wholesale (e.g. 2000+ unresolved imports).
fn check_meta_framework_prerequisites(active_plugins: &[&dyn Plugin], root: &Path) {
    for plugin in active_plugins {
        match plugin.name() {
            "nuxt" if !root.join(".nuxt/tsconfig.json").exists() => {
                tracing::warn!(
                    "Nuxt project missing .nuxt/tsconfig.json: run `nuxt prepare` \
                     before fallow for accurate analysis"
                );
            }
            "astro" if !root.join(".astro").exists() => {
                tracing::warn!(
                    "Astro project missing .astro/ types: run `astro sync` \
                     before fallow for accurate analysis"
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
