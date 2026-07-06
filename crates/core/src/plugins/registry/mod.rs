//! Plugin registry: discovers active plugins, collects patterns, parses configs.

use rustc_hash::FxHashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use fallow_config::{
    AutoImportRule, EntryPointRole, ExternalPluginDef, PackageJson, UsedClassMemberRule,
};

use crate::scripts;

use super::{PathRule, Plugin, PluginResult, PluginUsedExportRule, ProvidedDependencyRule};

pub(crate) mod builtin;
mod helpers;

/// Names of every built-in framework plugin, in registry order.
///
/// Derived live from the plugin registry so capability introspection
/// (`fallow schema`) can list plugins without a hand-maintained mirror.
#[must_use]
pub fn builtin_plugin_names() -> Vec<&'static str> {
    builtin::create_builtin_plugins()
        .iter()
        .map(|plugin| plugin.name())
        .collect()
}

/// Basenames from every built-in plugin config pattern, in stable order.
///
/// Engine-owned source discovery uses this to capture non-source config
/// candidates during the file walk without depending on discovery internals.
#[must_use]
pub fn builtin_plugin_config_candidate_basenames() -> Vec<String> {
    let mut set: FxHashSet<String> = FxHashSet::default();
    for plugin in builtin::create_builtin_plugins() {
        for pattern in plugin.config_patterns() {
            let basename = pattern.rsplit('/').next().unwrap_or(pattern);
            set.insert(basename.to_string());
        }
    }
    let mut basenames = set.into_iter().collect::<Vec<_>>();
    basenames.sort_unstable();
    basenames
}

pub use helpers::ConfigCandidateIndex;
use helpers::{
    check_has_config_file, discover_config_files, is_external_plugin_active,
    prepare_config_pattern, process_config_result, process_external_plugins,
    process_package_json_metadata, process_static_patterns,
};

fn must_parse_workspace_config_when_root_active(plugin_name: &str) -> bool {
    matches!(
        plugin_name,
        "eslint" | "docusaurus" | "jest" | "tanstack-router" | "vitest"
    )
}

fn compile_config_matchers<'a>(
    active: &[&'a dyn Plugin],
) -> Vec<(&'a dyn Plugin, Vec<globset::GlobMatcher>)> {
    active
        .iter()
        .filter(|plugin| !plugin.config_patterns().is_empty())
        .map(|plugin| {
            let matchers = plugin
                .config_patterns()
                .iter()
                .filter_map(|pattern| {
                    let prepared = prepare_config_pattern(pattern);
                    globset::Glob::new(&prepared)
                        .ok()
                        .map(|glob| glob.compile_matcher())
                })
                .collect();
            (*plugin, matchers)
        })
        .collect()
}

/// Emit one info-level line naming every active plugin.
fn log_active_plugins(active: &[&dyn Plugin]) {
    tracing::info!(
        plugins = active
            .iter()
            .map(|p| p.name())
            .collect::<Vec<_>>()
            .join(", "),
        "active plugins"
    );
}

/// Compute `(absolute, root-relative)` file pairs, but only when at least one
/// active plugin needs config matching or a package.json config key. Returns an
/// empty vec otherwise to skip the per-file path work.
fn compute_relative_files(
    config_matchers: &[(&dyn Plugin, Vec<globset::GlobMatcher>)],
    active: &[&dyn Plugin],
    discovered_files: &[PathBuf],
    root: &Path,
) -> Vec<(PathBuf, String)> {
    use rayon::prelude::*;
    let needs_relative_files =
        !config_matchers.is_empty() || active.iter().any(|p| p.package_json_config_key().is_some());
    if !needs_relative_files {
        return Vec::new();
    }
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
}

/// Registry of all available plugins (built-in + external).
pub struct PluginRegistry {
    plugins: Vec<Box<dyn Plugin>>,
    external_plugins: Vec<ExternalPluginDef>,
}

/// Inputs for the workspace-fast plugin path.
pub struct WorkspacePluginRunInput<'a> {
    pub pkg: &'a PackageJson,
    pub root: &'a Path,
    pub project_root: &'a Path,
    pub precompiled_config_matchers: &'a [(&'a dyn Plugin, Vec<globset::GlobMatcher>)],
    pub relative_files: &'a [(PathBuf, String)],
    pub skip_config_plugins: &'a FxHashSet<&'a str>,
    pub production_mode: bool,
    pub candidate_index: Option<&'a ConfigCandidateIndex>,
}

struct PluginRunContext<'a> {
    all_deps: Vec<String>,
    active: Vec<&'a dyn Plugin>,
}

/// Inputs governing which built-in plugins activate for a project.
struct PluginActivationInput<'a> {
    pkg: &'a PackageJson,
    root: &'a Path,
    discovered_files: &'a [PathBuf],
    all_deps: &'a [String],
    script_packages: &'a FxHashSet<String>,
    candidate_index: Option<&'a ConfigCandidateIndex>,
}

/// Invalid user-authored regex extracted from a plugin config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRegexValidationError {
    plugin_name: String,
    config_path: Option<PathBuf>,
    rule_kind: &'static str,
    field: &'static str,
    rule_pattern: String,
    regex_pattern: String,
    source: String,
}

impl PluginRegexValidationError {
    pub(crate) fn new(input: PluginRegexValidationErrorInput<'_>) -> Self {
        Self {
            plugin_name: input.plugin_name.to_owned(),
            config_path: input.config_path.map(Path::to_path_buf),
            rule_kind: input.rule_kind,
            field: input.field,
            rule_pattern: input.rule_pattern.to_owned(),
            regex_pattern: input.regex_pattern.to_owned(),
            source: input.source.to_string(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PluginRegexValidationErrorInput<'a> {
    pub(crate) plugin_name: &'a str,
    pub(crate) config_path: Option<&'a Path>,
    pub(crate) rule_kind: &'static str,
    pub(crate) field: &'static str,
    pub(crate) rule_pattern: &'a str,
    pub(crate) regex_pattern: &'a str,
    pub(crate) source: &'a regex::Error,
}

impl fmt::Display for PluginRegexValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let location = self
            .config_path
            .as_ref()
            .map(|path| format!(" in {}", path.display()))
            .unwrap_or_default();
        write!(
            f,
            "plugin '{}'{}: invalid regex '{}' in {}.{} for path rule '{}': {}",
            self.plugin_name,
            location,
            self.regex_pattern,
            self.rule_kind,
            self.field,
            self.rule_pattern,
            self.source
        )
    }
}

#[must_use]
pub fn format_plugin_regex_errors(errors: &[PluginRegexValidationError]) -> String {
    let joined = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n  - ");
    format!(
        "invalid plugin regex configuration:\n  - {joined}\n\nRewrite the plugin config with Rust-compatible regex syntax, or remove unsupported constructs such as JavaScript lookahead and lookbehind."
    )
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
    /// Dependencies referenced by package.json metadata, scoped to that package.json path.
    pub package_referenced_dependencies: Vec<(PathBuf, String)>,
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
            package_referenced_dependencies,
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
        self.package_referenced_dependencies
            .extend(package_referenced_dependencies);
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

    /// Test convenience wrapper for running all plugins against a project.
    ///
    /// This discovers which plugins are active, collects their static patterns,
    /// then parses any config files to extract dynamic information.
    #[cfg(test)]
    pub fn run(
        &self,
        pkg: &PackageJson,
        root: &Path,
        discovered_files: &[PathBuf],
    ) -> AggregatedPluginResult {
        self.try_run(pkg, root, discovered_files)
            .unwrap_or_else(|errors| panic!("{}", format_plugin_regex_errors(&errors)))
    }

    /// Run all plugins, returning invalid plugin regexes as hard errors.
    pub fn try_run(
        &self,
        pkg: &PackageJson,
        root: &Path,
        discovered_files: &[PathBuf],
    ) -> Result<AggregatedPluginResult, Vec<PluginRegexValidationError>> {
        self.try_run_with_search_roots(pkg, root, discovered_files, &[root], false, None)
    }

    /// Run all plugins against a project with explicit config-file search roots,
    /// returning invalid plugin regexes as hard errors.
    #[expect(
        clippy::too_many_arguments,
        reason = "public PluginRegistry API; signature is part of the crate surface for embedders"
    )]
    pub fn try_run_with_search_roots(
        &self,
        pkg: &PackageJson,
        root: &Path,
        discovered_files: &[PathBuf],
        config_search_roots: &[&Path],
        production_mode: bool,
        candidate_index: Option<&ConfigCandidateIndex>,
    ) -> Result<AggregatedPluginResult, Vec<PluginRegexValidationError>> {
        let _span = tracing::info_span!("run_plugins").entered();
        let mut result = AggregatedPluginResult::default();
        let mut regex_errors = Vec::new();

        let PluginRunContext { all_deps, active } = self.prepare_plugin_run_context(
            pkg,
            root,
            discovered_files,
            production_mode,
            candidate_index,
        );

        self.run_plugin_preflight(&active, &all_deps, root, discovered_files);

        for plugin in &active {
            process_static_patterns(*plugin, root, &mut result);
        }
        process_package_json_metadata(&active, pkg, root, &mut result, &mut regex_errors);

        process_external_plugins(
            &self.external_plugins,
            &all_deps,
            root,
            discovered_files,
            &mut result,
        );

        let config_matchers = compile_config_matchers(&active);
        let relative_files =
            compute_relative_files(&config_matchers, &active, discovered_files, root);

        resolve_plugin_config_files(&mut PluginConfigResolutionInput {
            config_matchers: &config_matchers,
            relative_files: &relative_files,
            config_search_roots,
            production_mode,
            candidate_index,
            root,
            result: &mut result,
            regex_errors: &mut regex_errors,
        });

        process_package_json_inline_configs(
            &active,
            &config_matchers,
            &relative_files,
            root,
            &mut result,
            &mut regex_errors,
        );

        if regex_errors.is_empty() {
            Ok(result)
        } else {
            Err(regex_errors)
        }
    }

    /// Test convenience wrapper for the fast workspace plugin path.
    ///
    /// Reuses pre-compiled config matchers and pre-computed relative files from the root
    /// project run, avoiding repeated glob compilation and path computation per workspace.
    /// Skips package.json inline config (workspace packages rarely have inline configs).
    #[cfg(test)]
    fn run_workspace_fast(&self, input: &WorkspacePluginRunInput<'_>) -> AggregatedPluginResult {
        self.try_run_workspace_fast(input)
            .unwrap_or_else(|errors| panic!("{}", format_plugin_regex_errors(&errors)))
    }

    /// Fast variant of `try_run()` for workspace packages.
    ///
    /// Reuses pre-compiled config matchers and pre-computed relative files from the root
    /// project run, avoiding repeated glob compilation and path computation per workspace.
    /// Skips package.json inline config (workspace packages rarely have inline configs).
    pub fn try_run_workspace_fast(
        &self,
        input: &WorkspacePluginRunInput<'_>,
    ) -> Result<AggregatedPluginResult, Vec<PluginRegexValidationError>> {
        let _span = tracing::info_span!("run_plugins").entered();
        let mut result = AggregatedPluginResult::default();
        let mut regex_errors = Vec::new();

        let all_deps = input.pkg.all_dependency_names();
        let script_packages =
            script_activation_packages(input.pkg, input.root, &all_deps, input.production_mode);
        let workspace_files: Vec<PathBuf> = input
            .relative_files
            .iter()
            .map(|(abs_path, _)| abs_path.clone())
            .collect();

        let active = self.collect_active_plugins(&PluginActivationInput {
            pkg: input.pkg,
            root: input.root,
            discovered_files: &workspace_files,
            all_deps: &all_deps,
            script_packages: &script_packages,
            candidate_index: input.candidate_index,
        });

        log_active_plugins(&active);

        self.emit_silent_fail_diagnostics(&active, &all_deps, input.root, &workspace_files);

        process_external_plugins(
            &self.external_plugins,
            &all_deps,
            input.root,
            &workspace_files,
            &mut result,
        );

        if active.is_empty() && result.active_plugins.is_empty() {
            return Ok(result);
        }

        process_workspace_active_plugins(&active, input, &mut result, &mut regex_errors);
        resolve_workspace_plugin_configs(&active, input, &mut result, &mut regex_errors);

        if regex_errors.is_empty() {
            Ok(result)
        } else {
            Err(regex_errors)
        }
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

fn process_workspace_active_plugins(
    active: &[&dyn Plugin],
    input: &WorkspacePluginRunInput<'_>,
    result: &mut AggregatedPluginResult,
    regex_errors: &mut Vec<PluginRegexValidationError>,
) {
    for plugin in active {
        process_static_patterns(*plugin, input.root, result);
    }
    process_package_json_metadata(active, input.pkg, input.root, result, regex_errors);
}

fn resolve_workspace_plugin_configs(
    active: &[&dyn Plugin],
    input: &WorkspacePluginRunInput<'_>,
    result: &mut AggregatedPluginResult,
    regex_errors: &mut Vec<PluginRegexValidationError>,
) {
    let workspace_matchers = select_workspace_matchers(
        input.precompiled_config_matchers,
        active,
        input.skip_config_plugins,
    );

    let mut resolved_ws_plugins: FxHashSet<&str> = FxHashSet::default();
    for (plugin, matchers) in &workspace_matchers {
        resolve_plugin_matching_files(&mut PluginMatchingFilesInput {
            plugin: *plugin,
            matchers,
            relative_files: input.relative_files,
            root: input.root,
            result,
            regex_errors,
            resolved_plugins: &mut resolved_ws_plugins,
        });
    }

    load_workspace_filesystem_configs(&mut WorkspaceFsConfigInput {
        workspace_matchers: &workspace_matchers,
        resolved_ws_plugins: &resolved_ws_plugins,
        root: input.root,
        project_root: input.project_root,
        production_mode: input.production_mode,
        candidate_index: input.candidate_index,
        result,
        regex_errors,
    });
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new(vec![])
    }
}

impl PluginRegistry {
    fn prepare_plugin_run_context<'a>(
        &'a self,
        pkg: &PackageJson,
        root: &Path,
        discovered_files: &[PathBuf],
        production_mode: bool,
        candidate_index: Option<&ConfigCandidateIndex>,
    ) -> PluginRunContext<'a> {
        let all_deps = pkg.all_dependency_names();
        let script_packages = script_activation_packages(pkg, root, &all_deps, production_mode);
        let active = self.collect_active_plugins(&PluginActivationInput {
            pkg,
            root,
            discovered_files,
            all_deps: &all_deps,
            script_packages: &script_packages,
            candidate_index,
        });

        PluginRunContext { all_deps, active }
    }

    fn run_plugin_preflight(
        &self,
        active: &[&dyn Plugin],
        all_deps: &[String],
        root: &Path,
        discovered_files: &[PathBuf],
    ) {
        log_active_plugins(active);
        check_meta_framework_prerequisites(active, root);
        self.emit_silent_fail_diagnostics(active, all_deps, root, discovered_files);
    }

    /// Collect every built-in plugin enabled for this project via files,
    /// scripts, or package.json. Shared by the root and workspace-fast paths.
    fn collect_active_plugins<'a>(
        &'a self,
        activation: &PluginActivationInput<'_>,
    ) -> Vec<&'a dyn Plugin> {
        self.plugins
            .iter()
            .filter(|p| {
                p.is_enabled_with_files(
                    activation.all_deps,
                    activation.root,
                    activation.discovered_files,
                    activation.candidate_index,
                ) || p.is_enabled_with_scripts(activation.script_packages, activation.root)
                    || p.is_enabled_with_package_json(activation.pkg, activation.root)
            })
            .map(AsRef::as_ref)
            .collect()
    }

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

struct PluginConfigResolutionInput<'a> {
    config_matchers: &'a [(&'a dyn Plugin, Vec<globset::GlobMatcher>)],
    relative_files: &'a [(PathBuf, String)],
    config_search_roots: &'a [&'a Path],
    production_mode: bool,
    candidate_index: Option<&'a ConfigCandidateIndex>,
    root: &'a Path,
    result: &'a mut AggregatedPluginResult,
    regex_errors: &'a mut Vec<PluginRegexValidationError>,
}

/// Filter pre-compiled matchers down to active plugins, keeping a config-skipped
/// plugin only when it must still parse its workspace config while root-active.
fn select_workspace_matchers<'a>(
    precompiled_config_matchers: &[(&'a dyn Plugin, Vec<globset::GlobMatcher>)],
    active: &[&dyn Plugin],
    skip_config_plugins: &FxHashSet<&str>,
) -> Vec<(&'a dyn Plugin, Vec<globset::GlobMatcher>)> {
    let active_names: FxHashSet<&str> = active.iter().map(|p| p.name()).collect();
    precompiled_config_matchers
        .iter()
        .filter(|(p, _)| {
            active_names.contains(p.name())
                && (!skip_config_plugins.contains(p.name())
                    || must_parse_workspace_config_when_root_active(p.name()))
        })
        .map(|(plugin, matchers)| (*plugin, matchers.clone()))
        .collect()
}

struct WorkspaceFsConfigInput<'a> {
    workspace_matchers: &'a [(&'a dyn Plugin, Vec<globset::GlobMatcher>)],
    resolved_ws_plugins: &'a FxHashSet<&'a str>,
    root: &'a Path,
    project_root: &'a Path,
    production_mode: bool,
    candidate_index: Option<&'a ConfigCandidateIndex>,
    result: &'a mut AggregatedPluginResult,
    regex_errors: &'a mut Vec<PluginRegexValidationError>,
}

/// Discover and parse workspace config files on disk for plugins not already
/// matched against discovered source files (workspace filesystem fallback).
fn load_workspace_filesystem_configs(input: &mut WorkspaceFsConfigInput<'_>) {
    let search_roots: &[&Path] = if input.root == input.project_root {
        &[input.root]
    } else {
        &[input.root, input.project_root]
    };
    let ws_json_configs = discover_config_files(
        input.workspace_matchers,
        input.resolved_ws_plugins,
        search_roots,
        input.production_mode,
        input.candidate_index,
    );
    for (abs_path, plugin) in &ws_json_configs {
        let Ok(source) = std::fs::read_to_string(abs_path) else {
            continue;
        };
        let plugin_result = plugin.resolve_config(abs_path, &source, input.root);
        if plugin_result.is_empty() {
            continue;
        }
        let rel = abs_path
            .strip_prefix(input.project_root)
            .map(|p| p.to_string_lossy())
            .unwrap_or_default();
        tracing::debug!(
            plugin = plugin.name(),
            config = %rel,
            entries = plugin_result.entry_patterns.len(),
            deps = plugin_result.referenced_dependencies.len(),
            "resolved config (workspace filesystem fallback)"
        );
        if let Err(mut errors) =
            process_config_result(plugin.name(), plugin_result, input.result, Some(abs_path))
        {
            input.regex_errors.append(&mut errors);
        }
    }
}

fn resolve_plugin_config_files(input: &mut PluginConfigResolutionInput<'_>) {
    if input.config_matchers.is_empty() {
        return;
    }

    let mut resolved_plugins: FxHashSet<&str> = FxHashSet::default();
    for (plugin, matchers) in input.config_matchers {
        resolve_plugin_matching_files(&mut PluginMatchingFilesInput {
            plugin: *plugin,
            matchers,
            relative_files: input.relative_files,
            root: input.root,
            result: input.result,
            regex_errors: input.regex_errors,
            resolved_plugins: &mut resolved_plugins,
        });
    }

    let json_configs = discover_config_files(
        input.config_matchers,
        &resolved_plugins,
        input.config_search_roots,
        input.production_mode,
        input.candidate_index,
    );
    for (abs_path, plugin) in &json_configs {
        resolve_plugin_filesystem_config(
            *plugin,
            abs_path,
            input.root,
            input.result,
            input.regex_errors,
        );
    }
}

struct PluginMatchingFilesInput<'plugins, 'data, 'state> {
    plugin: &'plugins dyn Plugin,
    matchers: &'data [globset::GlobMatcher],
    relative_files: &'data [(PathBuf, String)],
    root: &'data Path,
    result: &'state mut AggregatedPluginResult,
    regex_errors: &'state mut Vec<PluginRegexValidationError>,
    resolved_plugins: &'state mut FxHashSet<&'plugins str>,
}

fn resolve_plugin_matching_files(input: &mut PluginMatchingFilesInput<'_, '_, '_>) {
    use rayon::prelude::*;

    let plugin_hits: Vec<&PathBuf> = input
        .relative_files
        .par_iter()
        .filter_map(|(abs_path, rel_path)| {
            input
                .matchers
                .iter()
                .any(|m| m.is_match(rel_path.as_str()))
                .then_some(abs_path)
        })
        .collect();
    for abs_path in plugin_hits {
        let Ok(source) = std::fs::read_to_string(abs_path) else {
            continue;
        };
        let plugin_result = input.plugin.resolve_config(abs_path, &source, input.root);
        if plugin_result.is_empty() {
            continue;
        }
        input.resolved_plugins.insert(input.plugin.name());
        process_resolved_plugin_config(ResolvedPluginConfigInput {
            plugin: input.plugin,
            abs_path,
            plugin_result,
            result: input.result,
            regex_errors: input.regex_errors,
            message: "resolved config",
            config_display: abs_path.display(),
        });
    }
}

fn resolve_plugin_filesystem_config(
    plugin: &dyn Plugin,
    abs_path: &Path,
    root: &Path,
    result: &mut AggregatedPluginResult,
    regex_errors: &mut Vec<PluginRegexValidationError>,
) {
    let Ok(source) = std::fs::read_to_string(abs_path) else {
        return;
    };
    let plugin_result = plugin.resolve_config(abs_path, &source, root);
    if plugin_result.is_empty() {
        return;
    }
    let rel = abs_path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy())
        .unwrap_or_default();
    process_resolved_plugin_config(ResolvedPluginConfigInput {
        plugin,
        abs_path,
        plugin_result,
        result,
        regex_errors,
        message: "resolved config (filesystem fallback)",
        config_display: rel,
    });
}

struct ResolvedPluginConfigInput<'a, D> {
    plugin: &'a dyn Plugin,
    abs_path: &'a Path,
    plugin_result: PluginResult,
    result: &'a mut AggregatedPluginResult,
    regex_errors: &'a mut Vec<PluginRegexValidationError>,
    message: &'static str,
    config_display: D,
}

fn process_resolved_plugin_config(input: ResolvedPluginConfigInput<'_, impl std::fmt::Display>) {
    tracing::debug!(
        plugin = input.plugin.name(),
        config = %input.config_display,
        entries = input.plugin_result.entry_patterns.len(),
        deps = input.plugin_result.referenced_dependencies.len(),
        input.message
    );
    if let Err(mut errors) = process_config_result(
        input.plugin.name(),
        input.plugin_result,
        input.result,
        Some(input.abs_path),
    ) {
        input.regex_errors.append(&mut errors);
    }
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
/// intersection logic and are intentionally out of scope. The warning's purpose
/// is to surface USER-AUTHORED collisions between external plugins or between an
/// external plugin and a built-in, so the user can disambiguate by editing one
/// side.
///
/// Built-in-vs-built-in collisions are intentionally NOT reported: they are
/// curated and benign (Phase 3a config matching runs every matching plugin's
/// `resolve_config` independently, so there is no data loss), and the warning's
/// remediation advice ("rename one of the patterns or remove the duplicate
/// plugin") is impossible to follow for a built-in. Such a collision exists by
/// design, e.g. both `vite` and `tanstack-router` claim
/// `vite.config.{ts,js,mts,mjs}` because tanstack-router parses the
/// `tanstackRouter({...})` call inside the vite config to find a custom
/// `generatedRouteTree` path (#808). A finding is therefore emitted only when
/// at least one owner is an external (user-authored) plugin.
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

    // Names of built-in plugins. Built-in-only collisions are curated + benign
    // (every matching plugin runs `resolve_config` independently), so they must
    // not surface an un-actionable warning (#808). Keying on the built-in set
    // and emitting only when an owner is NOT built-in is robust even if a
    // user-authored external plugin happens to share a built-in's name: the
    // built-in owner alone never re-enables the warning.
    let builtin_names: FxHashSet<&str> = builtin_active.iter().map(|p| p.name()).collect();

    let mut findings: Vec<PluginDiagnostic> = pattern_owners
        .into_iter()
        .filter_map(|(pattern, (owners, _seen))| {
            if owners.len() < 2 || owners.iter().all(|o| builtin_names.contains(o.as_str())) {
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
    regex_errors: &mut Vec<PluginRegexValidationError>,
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
        if let Err(mut errors) =
            process_config_result(plugin.name(), plugin_result, result, Some(&pkg_path))
        {
            regex_errors.append(&mut errors);
        }
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
    let dep_set: FxHashSet<String> = all_deps.iter().cloned().collect();
    let script_names: FxHashSet<String> = pkg_scripts.keys().cloned().collect();

    scripts::analyze_scripts_with_dependency_context(
        &scripts_to_analyze,
        root,
        &bin_map,
        &dep_set,
        &script_names,
    )
    .used_packages
}

#[cfg(test)]
mod tests;
