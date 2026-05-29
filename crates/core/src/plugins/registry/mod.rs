//! Plugin registry: discovers active plugins, collects patterns, parses configs.

use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use fallow_config::{
    AutoImportRule, EntryPointRole, ExternalPluginDef, PackageJson, UsedClassMemberRule,
};

use crate::scripts;

use super::{PathRule, Plugin, PluginUsedExportRule, ProvidedDependencyRule};

pub(crate) mod builtin;
mod helpers;

use helpers::{
    check_has_config_file, discover_config_files, is_external_plugin_active,
    prepare_config_pattern, process_config_result, process_external_plugins,
    process_static_patterns,
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
#[derive(Debug, Clone, Default)]
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
    /// Import prefixes for build-time generated type-only relative imports.
    /// Unresolved type-only imports starting with these prefixes are suppressed.
    pub generated_type_import_prefixes: Vec<String>,
    /// Path alias mappings from active plugins (prefix → replacement directory).
    /// Used by the resolver to substitute import prefixes before re-resolving.
    pub path_aliases: Vec<(String, String)>,
    /// Convention-based auto-import rules from active plugins (Nuxt components).
    /// The resolver matches each file's captured `auto_import_candidates` against
    /// these and synthesizes a graph edge to the rule's source. See issue #704.
    pub auto_imports: Vec<AutoImportRule>,
    /// Names of active plugins.
    pub active_plugins: Vec<String>,
    /// Test fixture glob patterns from active plugins: (pattern, plugin_name).
    pub fixture_patterns: Vec<(String, String)>,
    /// Absolute directories contributed by plugins that should be searched
    /// when resolving SCSS/Sass `@import`/`@use` specifiers. Populated from
    /// Angular's `stylePreprocessorOptions.includePaths` and equivalent
    /// framework settings. See issue #103.
    pub scss_include_paths: Vec<PathBuf>,
    /// Static directory mappings contributed by plugins.
    pub static_dir_mappings: Vec<(PathBuf, String)>,
    /// File-scoped dependency provider rules from active plugins.
    pub provided_dependencies: Vec<ProvidedDependencyRule>,
}

/// Append `incoming` string items to `target`, skipping values already present
/// in `target` or earlier in `incoming`. Matches the deduplication the
/// workspace merge applied via per-field `seen` sets before #444 centralized
/// it on [`AggregatedPluginResult::merge_into`].
fn extend_unique(target: &mut Vec<String>, incoming: Vec<String>) {
    let mut seen: FxHashSet<String> = target.iter().cloned().collect();
    for item in incoming {
        if seen.insert(item.clone()) {
            target.push(item);
        }
    }
}

/// Prefix a workspace-relative pattern so it matches from the monorepo root,
/// unless it is already workspace-prefixed or project-root-relative (leading
/// `/`, e.g. an angular.json path). Mirrors the pre-#444 inline closure.
fn prefix_if_needed(pat: &str, ws_prefix: &str) -> String {
    if pat.starts_with(ws_prefix) || pat.starts_with('/') {
        pat.to_string()
    } else {
        format!("{ws_prefix}/{pat}")
    }
}

impl AggregatedPluginResult {
    /// Apply a workspace prefix to every path-bearing field in place.
    ///
    /// Workspace-package results are collected with patterns relative to the
    /// package root; to be matchable from the monorepo root they need the
    /// package's prefix applied. This transform is call-site-specific (it
    /// depends on `ws_prefix`), so it stays separate from [`Self::merge_into`],
    /// which is a prefix-agnostic union. The root project's own result is
    /// never prefixed.
    ///
    /// Fields that carry package names, absolute paths, or import-specifier
    /// boundaries (referenced/tooling deps, setup files, static dir mappings,
    /// auto-imports, virtual prefixes/suffixes, generated patterns) are left
    /// untouched, matching the pre-#444 merge loop.
    pub fn apply_workspace_prefix(&mut self, ws_prefix: &str) {
        for (rule, _) in &mut self.entry_patterns {
            *rule = rule.prefixed(ws_prefix);
        }
        for (pat, _) in &mut self.always_used {
            *pat = prefix_if_needed(pat, ws_prefix);
        }
        for (pat, _) in &mut self.discovered_always_used {
            *pat = prefix_if_needed(pat, ws_prefix);
        }
        for (pat, _) in &mut self.fixture_patterns {
            *pat = prefix_if_needed(pat, ws_prefix);
        }
        for rule in &mut self.used_exports {
            *rule = rule.prefixed(ws_prefix);
        }
        for rule in &mut self.provided_dependencies {
            *rule = rule.prefixed(ws_prefix);
        }
        // Path aliases: the alias key (prefix) is unchanged, but the
        // replacement directory is prefixed so it resolves from the root.
        for (_, replacement) in &mut self.path_aliases {
            *replacement = format!("{ws_prefix}/{replacement}");
        }
    }

    /// Merge `other` into `self`, taking the union of every field.
    ///
    /// Exhaustively destructures `Self` so adding a field to
    /// `AggregatedPluginResult` becomes a `missing field in pattern` compile
    /// error here instead of a silently-dropped field. See issue #444.
    ///
    /// Callers that need the workspace prefix applied must call
    /// [`Self::apply_workspace_prefix`] on `other` first; this method does not
    /// transform any path. Dedup-bearing fields (`active_plugins`, the virtual
    /// prefix/suffix and generated-pattern lists) deduplicate the incoming
    /// values against the contents already in `self`, matching the pre-#444
    /// `seen`-set behavior. `entry_point_roles` is first-writer-wins.
    pub fn merge_into(&mut self, other: Self) {
        let Self {
            entry_patterns,
            entry_point_roles,
            config_patterns,
            always_used,
            used_exports,
            used_class_members,
            referenced_dependencies,
            discovered_always_used,
            setup_files,
            tooling_dependencies,
            script_used_packages,
            virtual_module_prefixes,
            virtual_package_suffixes,
            generated_import_patterns,
            generated_type_import_prefixes,
            path_aliases,
            auto_imports,
            active_plugins,
            fixture_patterns,
            scss_include_paths,
            static_dir_mappings,
            provided_dependencies,
        } = other;

        self.entry_patterns.extend(entry_patterns);
        for (plugin_name, role) in entry_point_roles {
            self.entry_point_roles.entry(plugin_name).or_insert(role);
        }
        self.config_patterns.extend(config_patterns);
        self.always_used.extend(always_used);
        self.used_exports.extend(used_exports);
        self.used_class_members.extend(used_class_members);
        self.referenced_dependencies.extend(referenced_dependencies);
        self.discovered_always_used.extend(discovered_always_used);
        self.setup_files.extend(setup_files);
        self.tooling_dependencies.extend(tooling_dependencies);
        self.script_used_packages.extend(script_used_packages);
        extend_unique(&mut self.virtual_module_prefixes, virtual_module_prefixes);
        extend_unique(&mut self.virtual_package_suffixes, virtual_package_suffixes);
        extend_unique(
            &mut self.generated_import_patterns,
            generated_import_patterns,
        );
        extend_unique(
            &mut self.generated_type_import_prefixes,
            generated_type_import_prefixes,
        );
        self.path_aliases.extend(path_aliases);
        self.auto_imports.extend(auto_imports);
        extend_unique(&mut self.active_plugins, active_plugins);
        self.fixture_patterns.extend(fixture_patterns);
        self.scss_include_paths.extend(scss_include_paths);
        self.static_dir_mappings.extend(static_dir_mappings);
        self.provided_dependencies.extend(provided_dependencies);
    }
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
        let script_packages = script_activation_packages(pkg, root, &all_deps, production_mode);
        let active: Vec<&dyn Plugin> = self
            .plugins
            .iter()
            .filter(|p| {
                p.is_enabled_with_files(&all_deps, root, discovered_files)
                    || p.is_enabled_with_scripts(&script_packages, root)
            })
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

        // Silent-fail diagnostics for the plugin system (#479).
        self.emit_silent_fail_diagnostics(&active, &all_deps, root, discovered_files);

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

        use rayon::prelude::*;
        // Build relative paths lazily: only needed when config matchers exist
        // or plugins have package_json_config_key. Skip entirely for projects
        // with no config-parsing plugins (e.g., only React), avoiding O(files)
        // String allocations.
        let needs_relative_files = !config_matchers.is_empty()
            || active.iter().any(|p| p.package_json_config_key().is_some());
        let relative_files: Vec<(PathBuf, String)> = if needs_relative_files {
            discovered_files
                .par_iter()
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
            // Phase 3a: Match config files from discovered source files. Per-file
            // glob matching is parallelized: on monorepos with tens of thousands
            // of source files, the file-scan cost dominates the plugins phase.
            let mut resolved_plugins: FxHashSet<&str> = FxHashSet::default();

            for (plugin, matchers) in &config_matchers {
                let plugin_hits: Vec<&PathBuf> = relative_files
                    .par_iter()
                    .filter_map(|(abs_path, rel_path)| {
                        matchers
                            .iter()
                            .any(|m| m.is_match(rel_path.as_str()))
                            .then_some(abs_path)
                    })
                    .collect();
                for abs_path in plugin_hits {
                    if let Ok(source) = std::fs::read_to_string(abs_path) {
                        let plugin_result = plugin.resolve_config(abs_path, &source, root);
                        if !plugin_result.is_empty() {
                            resolved_plugins.insert(plugin.name());
                            tracing::debug!(
                                plugin = plugin.name(),
                                config = %abs_path.display(),
                                entries = plugin_result.entry_patterns.len(),
                                deps = plugin_result.referenced_dependencies.len(),
                                "resolved config"
                            );
                            process_config_result(
                                plugin.name(),
                                plugin_result,
                                &mut result,
                                Some(abs_path),
                            );
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
                        process_config_result(
                            plugin.name(),
                            plugin_result,
                            &mut result,
                            Some(abs_path),
                        );
                    }
                }
            }
        }

        // Phase 4: Package.json inline config fallback.
        process_package_json_inline_configs(
            &active,
            &config_matchers,
            &relative_files,
            root,
            &mut result,
        );

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
        let script_packages = script_activation_packages(pkg, root, &all_deps, production_mode);
        let workspace_files: Vec<PathBuf> = relative_files
            .iter()
            .map(|(abs_path, _)| abs_path.clone())
            .collect();

        let active: Vec<&dyn Plugin> = self
            .plugins
            .iter()
            .filter(|p| {
                p.is_enabled_with_files(&all_deps, root, &workspace_files)
                    || p.is_enabled_with_scripts(&script_packages, root)
            })
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

        // Silent-fail diagnostics (#479); the shared dedupe set means the
        // same external plugin's enabler typo or pattern collision only warns
        // once per process even when this fast path runs per workspace.
        self.emit_silent_fail_diagnostics(&active, &all_deps, root, &workspace_files);

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
            use rayon::prelude::*;
            for (plugin, matchers) in &workspace_matchers {
                let plugin_hits: Vec<&PathBuf> = relative_files
                    .par_iter()
                    .filter_map(|(abs_path, rel_path)| {
                        matchers
                            .iter()
                            .any(|m| m.is_match(rel_path.as_str()))
                            .then_some(abs_path)
                    })
                    .collect();
                for abs_path in plugin_hits {
                    if let Ok(source) = std::fs::read_to_string(abs_path) {
                        let plugin_result = plugin.resolve_config(abs_path, &source, root);
                        if !plugin_result.is_empty() {
                            resolved_ws_plugins.insert(plugin.name());
                            tracing::debug!(
                                plugin = plugin.name(),
                                config = %abs_path.display(),
                                entries = plugin_result.entry_patterns.len(),
                                deps = plugin_result.referenced_dependencies.len(),
                                "resolved config"
                            );
                            process_config_result(
                                plugin.name(),
                                plugin_result,
                                &mut result,
                                Some(abs_path),
                            );
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
                    process_config_result(
                        plugin.name(),
                        plugin_result,
                        &mut result,
                        Some(abs_path),
                    );
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

impl PluginRegistry {
    /// Collect the active subset of external plugins, run the silent-fail
    /// diagnostics (#479), and emit one `tracing::warn!` per finding (dedup'd
    /// across analysis passes via [`plugin_warn_dedupe`]).
    ///
    /// Called from both `run_with_search_roots` (top-level) and
    /// `run_workspace_fast` (per-workspace) so a typo'd enabler or pattern
    /// collision surfaces regardless of which entry point dispatched the
    /// analysis.
    fn emit_silent_fail_diagnostics(
        &self,
        active: &[&dyn Plugin],
        all_deps: &[String],
        root: &Path,
        discovered_files: &[PathBuf],
    ) {
        let active_external: Vec<&ExternalPluginDef> = self
            .external_plugins
            .iter()
            .filter(|ext| is_external_plugin_active(ext, all_deps, root, discovered_files))
            .collect();
        let mut diagnostics = detect_pattern_collisions(active, &active_external);
        diagnostics.extend(detect_enabler_typos(&self.external_plugins, all_deps));
        emit_plugin_diagnostics(&diagnostics);
    }
}

/// Process-wide dedupe key cache for plugin-system diagnostic warnings.
///
/// Combined-mode runs `PluginRegistry::run_with_search_roots` three times
/// (check + dupes + health) per analysis, so a naive warn would triple-emit
/// every diagnostic. Each warn helper builds a unique key, inserts it here,
/// and only emits when the key was previously absent.
fn plugin_warn_dedupe() -> &'static std::sync::Mutex<FxHashSet<String>> {
    static WARNED: std::sync::OnceLock<std::sync::Mutex<FxHashSet<String>>> =
        std::sync::OnceLock::new();
    WARNED.get_or_init(|| std::sync::Mutex::new(FxHashSet::default()))
}

/// Insert `key` into the dedupe set and return `true` when it was newly
/// inserted (caller should emit). Returns `true` on a poisoned mutex so
/// over-warning beats swallowing.
fn should_warn(key: String) -> bool {
    plugin_warn_dedupe()
        .lock()
        .map_or(true, |mut set| set.insert(key))
}

/// Structured diagnostic surfaced by the silent-fail plugin checks (#479).
///
/// Returned by [`detect_pattern_collisions`] and [`detect_enabler_typos`] so
/// unit tests can assert on the findings without standing up a tracing
/// subscriber. The runtime path calls [`emit_plugin_diagnostics`] to convert
/// each variant into one `tracing::warn!` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PluginDiagnostic {
    /// Two or more plugins declared an identical `config_patterns` entry.
    PatternCollision {
        pattern: String,
        owners: Vec<String>,
    },
    /// An external plugin enabler does not match any project dependency, but
    /// at least one Levenshtein-close dep name exists.
    EnablerTypo {
        plugin: String,
        enabler: String,
        suggestion: String,
    },
}

/// Detect plugins whose `config_patterns` collide byte-for-byte.
///
/// Detection is byte-equal on the pattern string. Overlapping but non-identical
/// globs (e.g. `vite.config.{ts,js}` vs `vite.config.ts`) require pattern
/// intersection logic and are intentionally out of scope; there are no known
/// collisions in the built-in plugin set. The warning's purpose is to surface
/// USER-AUTHORED collisions between external plugins or between an external
/// plugin and a built-in, so the user can disambiguate by editing one side.
///
/// Precedence rule when two plugins claim the same pattern: the one registered
/// first wins. For built-in plugins, registration order is defined in
/// [`builtin::create_builtin_plugins`]. External plugins (file-loaded plus
/// inline `framework[]`) run AFTER built-ins, so they cannot displace a
/// built-in's `resolve_config` result for the same file.
pub(crate) fn detect_pattern_collisions(
    builtin_active: &[&dyn Plugin],
    external_active: &[&ExternalPluginDef],
) -> Vec<PluginDiagnostic> {
    use rustc_hash::FxHashMap;

    // Owners are stored as a Vec to preserve REGISTRATION ORDER: owners[0]
    // is the plugin that wins Phase 3a config matching, and the warning text
    // names it as the winner. A `FxHashSet` is held alongside to dedupe a
    // single plugin that legitimately lists the same pattern twice in its
    // own `config_patterns` (rare but legal) so it does not look like a
    // self-vs-self collision.
    let mut pattern_owners: FxHashMap<String, (Vec<String>, FxHashSet<String>)> =
        FxHashMap::default();

    let record = |pattern_owners: &mut FxHashMap<_, (Vec<String>, FxHashSet<String>)>,
                  pattern: String,
                  name: String| {
        let (list, seen) = pattern_owners.entry(pattern).or_default();
        if seen.insert(name.clone()) {
            list.push(name);
        }
    };

    for plugin in builtin_active {
        for pat in plugin.config_patterns() {
            record(
                &mut pattern_owners,
                (*pat).to_string(),
                plugin.name().to_string(),
            );
        }
    }
    for ext in external_active {
        for pat in &ext.config_patterns {
            record(&mut pattern_owners, pat.clone(), ext.name.clone());
        }
    }

    let mut findings: Vec<PluginDiagnostic> = pattern_owners
        .into_iter()
        .filter_map(|(pattern, (owners, _seen))| {
            if owners.len() < 2 {
                None
            } else {
                Some(PluginDiagnostic::PatternCollision { pattern, owners })
            }
        })
        .collect();
    findings.sort_unstable_by(|a, b| match (a, b) {
        (
            PluginDiagnostic::PatternCollision { pattern: ap, .. },
            PluginDiagnostic::PatternCollision { pattern: bp, .. },
        ) => ap.cmp(bp),
        _ => std::cmp::Ordering::Equal,
    });
    findings
}

/// Detect external plugins whose enablers do not match any project dependency
/// AND at least one enabler is a plausible typo of a real dep.
///
/// Scope:
/// - Only external plugins (file-loaded plus inline `framework[]`). Built-in
///   plugins' enablers are hard-coded so cannot be misspelled.
/// - Skip plugins with a `detection` block: detection is the rich-logic path
///   and false negatives there are not enabler typos.
/// - Skip plugins with empty `enablers` (no signal to validate against).
/// - Stay silent when no Levenshtein-close dep exists: the plugin may
///   legitimately not apply to this project.
///
/// Matches the established #467 / #510 pattern: tracing-warn with a `did you
/// mean` suggestion at the call site. No exit non-zero, no new CLI flag.
pub(crate) fn detect_enabler_typos(
    external_plugins: &[ExternalPluginDef],
    all_deps: &[String],
) -> Vec<PluginDiagnostic> {
    let mut findings = Vec::new();

    for ext in external_plugins {
        if ext.detection.is_some() || ext.enablers.is_empty() {
            continue;
        }

        let any_match = ext.enablers.iter().any(|enabler| {
            if enabler.ends_with('/') {
                all_deps.iter().any(|d| d.starts_with(enabler))
            } else {
                all_deps.iter().any(|d| d == enabler)
            }
        });
        if any_match {
            continue;
        }

        for enabler in &ext.enablers {
            let candidates = all_deps.iter().map(String::as_str);
            let Some(suggestion) = fallow_config::levenshtein::closest_match(enabler, candidates)
            else {
                continue;
            };

            findings.push(PluginDiagnostic::EnablerTypo {
                plugin: ext.name.clone(),
                enabler: enabler.clone(),
                suggestion: suggestion.to_string(),
            });
        }
    }

    findings
}

/// Emit one `tracing::warn!` per finding, dedup'd against the process-wide
/// `plugin_warn_dedupe` set so combined-mode does not triple-warn.
fn emit_plugin_diagnostics(findings: &[PluginDiagnostic]) {
    for finding in findings {
        match finding {
            PluginDiagnostic::PatternCollision { pattern, owners } => {
                let key = format!("collision::{pattern}::{owners:?}");
                if !should_warn(key) {
                    continue;
                }
                let winner = &owners[0];
                let others = owners[1..].join(", ");
                tracing::warn!(
                    "plugin config_patterns collision: identical pattern \
                     '{pattern}' is claimed by plugins [{joined}]; '{winner}' \
                     runs first (registration order), others ({others}) \
                     follow. Rename one of the patterns or remove the \
                     duplicate plugin to make resolution explicit. A future \
                     release may reject identical-pattern collisions.",
                    joined = owners.join(", "),
                );
            }
            PluginDiagnostic::EnablerTypo {
                plugin,
                enabler,
                suggestion,
            } => {
                let key = format!("enabler::{plugin}::{enabler}");
                if !should_warn(key) {
                    continue;
                }
                tracing::warn!(
                    "plugin '{plugin}' enabler '{enabler}' does not match any \
                     dependency in package.json; did you mean '{suggestion}'? \
                     The plugin will not activate. A future release may reject \
                     unmatched enablers.",
                );
            }
        }
    }
}

/// Phase 4 of `PluginRegistry::run_with_search_roots`: for any active plugin
/// that supports inline package.json configuration via
/// [`Plugin::package_json_config_key`], read the root `package.json`, extract
/// the relevant key, and feed the result through `resolve_config`.
fn process_package_json_inline_configs(
    active: &[&dyn Plugin],
    config_matchers: &[(&dyn Plugin, Vec<globset::GlobMatcher>)],
    relative_files: &[(PathBuf, String)],
    root: &Path,
    result: &mut AggregatedPluginResult,
) {
    for plugin in active {
        let Some(key) = plugin.package_json_config_key() else {
            continue;
        };
        if check_has_config_file(*plugin, config_matchers, relative_files) {
            continue;
        }
        let pkg_path = root.join("package.json");
        let Ok(content) = std::fs::read_to_string(&pkg_path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(config_value) = json.get(key) else {
            continue;
        };
        let config_json = serde_json::to_string(config_value).unwrap_or_default();
        let fake_path = root.join(format!("{key}.config.json"));
        let plugin_result = plugin.resolve_config(&fake_path, &config_json, root);
        if plugin_result.is_empty() {
            continue;
        }
        tracing::debug!(
            plugin = plugin.name(),
            key = key,
            "resolved inline package.json config"
        );
        process_config_result(plugin.name(), plugin_result, result, Some(&pkg_path));
    }
}

/// A missing meta-framework prerequisite: the per-process dedupe key and the
/// warning message to emit.
#[derive(Debug)]
struct MetaFrameworkWarning {
    dedupe_key: &'static str,
    message: &'static str,
}

/// Pure detection: which active meta-frameworks are missing their generated
/// config/types directory under `root`. Separated from emission so the
/// detection logic is unit-testable without a tracing subscriber or the
/// process-wide dedupe set.
///
/// When adding a framework here, also extend `MATERIALIZED_CONTEXT_DIRS` in
/// `fallow-cli`'s `audit.rs` with its generated dir, otherwise `fallow audit`'s
/// base worktree will not symlink that dir and the broken-tsconfig-chain bug
/// resurfaces on the base pass for the new framework.
fn missing_meta_framework_prerequisites(
    active_plugins: &[&dyn Plugin],
    root: &Path,
) -> Vec<MetaFrameworkWarning> {
    active_plugins
        .iter()
        .filter_map(|plugin| match plugin.name() {
            "nuxt" if !root.join(".nuxt/tsconfig.json").exists() => Some(MetaFrameworkWarning {
                dedupe_key: "meta-prereq::nuxt",
                message: "Nuxt project missing .nuxt/tsconfig.json: run `nuxt prepare` \
                          before fallow for accurate analysis",
            }),
            "astro" if !root.join(".astro").exists() => Some(MetaFrameworkWarning {
                dedupe_key: "meta-prereq::astro",
                message: "Astro project missing .astro/ types: run `astro sync` \
                          before fallow for accurate analysis",
            }),
            _ => None,
        })
        .collect()
}

/// Warn when meta-frameworks are active but their generated configs are missing.
///
/// Meta-frameworks like Nuxt and Astro generate tsconfig/types files during a
/// "prepare" step. Without these, the tsconfig extends chain breaks and
/// extensionless imports fail wholesale (e.g. 2000+ unresolved imports).
///
/// Deduped per framework so combined-mode (check + dupes + health through one
/// loader) does not re-warn. The advice is generic and does not name the root,
/// so one line per process per framework is the right bound (issue #637).
fn check_meta_framework_prerequisites(active_plugins: &[&dyn Plugin], root: &Path) {
    for warning in missing_meta_framework_prerequisites(active_plugins, root) {
        if should_warn(warning.dedupe_key.to_owned()) {
            tracing::warn!("{}", warning.message);
        }
    }
}

fn script_activation_packages(
    pkg: &PackageJson,
    root: &Path,
    all_deps: &[String],
    production_mode: bool,
) -> FxHashSet<String> {
    let Some(pkg_scripts) = pkg.scripts.as_ref() else {
        return FxHashSet::default();
    };

    let scripts_to_analyze = if production_mode {
        scripts::filter_production_scripts(pkg_scripts)
    } else {
        pkg_scripts.clone()
    };

    let mut nm_roots = Vec::new();
    if root.join("node_modules").is_dir() {
        nm_roots.push(root);
    }
    let bin_map = scripts::build_bin_to_package_map(&nm_roots, all_deps);

    scripts::analyze_scripts(&scripts_to_analyze, root, &bin_map).used_packages
}

#[cfg(test)]
mod tests;
