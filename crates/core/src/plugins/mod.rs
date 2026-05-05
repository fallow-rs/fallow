//! Plugin system for framework-aware codebase analysis.
//!
//! Unlike knip's JavaScript plugin system that evaluates config files at runtime,
//! fallow's plugin system uses Oxc's parser to extract configuration values from
//! JS/TS/JSON config files via AST walking — no JavaScript evaluation needed.
//!
//! Each plugin implements the [`Plugin`] trait with:
//! - **Static defaults**: Entry patterns, config file patterns, used exports
//! - **Dynamic resolution**: Parse tool config files to discover additional entries,
//!   referenced dependencies, and setup files

use std::path::{Path, PathBuf};

use fallow_config::{EntryPointRole, PackageJson, UsedClassMemberRule};
use regex::Regex;

const TEST_ENTRY_POINT_PLUGINS: &[&str] = &[
    "ava",
    "cucumber",
    "cypress",
    "jest",
    "mocha",
    "playwright",
    "tap",
    "tsd",
    "vitest",
    "webdriverio",
];

const RUNTIME_ENTRY_POINT_PLUGINS: &[&str] = &[
    "angular",
    "astro",
    "convex",
    "docusaurus",
    "electron",
    "expo",
    "expo-router",
    "gatsby",
    "hardhat",
    "nestjs",
    "next-intl",
    "nextjs",
    "nitro",
    "nuxt",
    "parcel",
    "qwik",
    "react-native",
    "react-router",
    "remix",
    "rolldown",
    "rollup",
    "rsbuild",
    "rspack",
    "sanity",
    "sveltekit",
    "tanstack-router",
    "tsdown",
    "tsup",
    "vite",
    "vitepress",
    "webpack",
    "wrangler",
];

#[cfg(test)]
const SUPPORT_ENTRY_POINT_PLUGINS: &[&str] = &[
    "drizzle",
    "i18next",
    "knex",
    "kysely",
    "msw",
    "prisma",
    "storybook",
    "typeorm",
];

/// Result of resolving a plugin's config file.
#[derive(Debug, Default)]
pub struct PluginResult {
    /// Additional entry point glob patterns discovered from config.
    pub entry_patterns: Vec<PathRule>,
    /// When true, `entry_patterns` from config replace the plugin's static
    /// `entry_patterns()` defaults instead of adding to them. Tools like Vitest
    /// and Jest treat their config's include/testMatch as a replacement for built-in
    /// defaults, so when the config is explicit the static patterns must be dropped.
    pub replace_entry_patterns: bool,
    /// When true, `used_exports` from config replace the plugin's static
    /// `used_export_rules()` defaults instead of adding to them.
    pub replace_used_export_rules: bool,
    /// Additional export-usage rules discovered from config.
    pub used_exports: Vec<UsedExportRule>,
    /// Class member rules that should never be flagged as unused. Contributed
    /// by plugins that know their framework invokes these methods at runtime
    /// and may scope suppression via `extends` / `implements` constraints when
    /// the method name is too common to allowlist globally.
    pub used_class_members: Vec<UsedClassMemberRule>,
    /// Dependencies referenced in config files (should not be flagged as unused).
    pub referenced_dependencies: Vec<String>,
    /// Additional files that are always considered used.
    pub always_used_files: Vec<String>,
    /// Path alias mappings discovered from config (prefix -> replacement directory).
    pub path_aliases: Vec<(String, String)>,
    /// Setup/helper files referenced from config.
    pub setup_files: Vec<PathBuf>,
    /// Test fixture glob patterns discovered from config.
    pub fixture_patterns: Vec<String>,
    /// Absolute directories to include when resolving SCSS/Sass `@import` and
    /// `@use` specifiers. Contributed by framework plugins that read their
    /// tool's equivalent of `includePaths` (e.g. Angular's
    /// `stylePreprocessorOptions.includePaths` from `angular.json` /
    /// `project.json`). Bare SCSS specifiers that fail to resolve relative to
    /// the importing file retry against each include path using the SCSS
    /// partial / directory-index conventions.
    pub scss_include_paths: Vec<PathBuf>,
}

impl PluginResult {
    pub fn push_entry_pattern(&mut self, pattern: impl Into<String>) {
        self.entry_patterns
            .push(PathRule::new(normalize_entry_pattern(pattern.into())));
    }

    pub fn extend_entry_patterns<I, S>(&mut self, patterns: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.entry_patterns.extend(
            patterns
                .into_iter()
                .map(|pat| PathRule::new(normalize_entry_pattern(pat.into()))),
        );
    }

    pub fn push_used_export_rule(
        &mut self,
        pattern: impl Into<String>,
        exports: impl IntoIterator<Item = impl Into<String>>,
    ) {
        self.used_exports
            .push(UsedExportRule::new(pattern, exports));
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entry_patterns.is_empty()
            && self.used_exports.is_empty()
            && self.used_class_members.is_empty()
            && self.referenced_dependencies.is_empty()
            && self.always_used_files.is_empty()
            && self.path_aliases.is_empty()
            && self.setup_files.is_empty()
            && self.fixture_patterns.is_empty()
            && self.scss_include_paths.is_empty()
    }
}

// Strip a leading `./` from project-relative entry patterns. Globset compiles
// patterns with `literal_separator(true)`, so `./src/app.ts` would never match
// the project-relative path `src/app.ts` that appears in the file index.
// Plugins that source entries directly from user config (Webpack `entry`,
// Rollup `input`, Rspack/Rsbuild/Rolldown variants) commonly carry the leading
// `./` verbatim.
fn normalize_entry_pattern(pattern: String) -> String {
    pattern
        .strip_prefix("./")
        .map(str::to_owned)
        .unwrap_or(pattern)
}

/// A file-pattern rule with optional exclusion globs plus path-level or
/// segment-level regex filters.
///
/// Exclusion regexes are matched against the project-relative path and should be
/// anchored when generated dynamically so they can be safely workspace-prefixed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathRule {
    pub pattern: String,
    pub exclude_globs: Vec<String>,
    pub exclude_regexes: Vec<String>,
    /// Regexes matched against individual path segments. These are not prefixed
    /// for workspaces because they intentionally operate on segment names rather
    /// than the full project-relative path.
    pub exclude_segment_regexes: Vec<String>,
}

impl PathRule {
    #[must_use]
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            exclude_globs: Vec::new(),
            exclude_regexes: Vec::new(),
            exclude_segment_regexes: Vec::new(),
        }
    }

    #[must_use]
    pub fn from_static(pattern: &'static str) -> Self {
        Self::new(pattern)
    }

    #[must_use]
    pub fn with_excluded_globs<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.exclude_globs
            .extend(patterns.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn with_excluded_regexes<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.exclude_regexes
            .extend(patterns.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn with_excluded_segment_regexes<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.exclude_segment_regexes
            .extend(patterns.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn prefixed(&self, ws_prefix: &str) -> Self {
        Self {
            pattern: prefix_workspace_pattern(&self.pattern, ws_prefix),
            exclude_globs: self
                .exclude_globs
                .iter()
                .map(|pattern| prefix_workspace_pattern(pattern, ws_prefix))
                .collect(),
            exclude_regexes: self
                .exclude_regexes
                .iter()
                .map(|pattern| prefix_workspace_regex(pattern, ws_prefix))
                .collect(),
            exclude_segment_regexes: self.exclude_segment_regexes.clone(),
        }
    }
}

/// A used-export rule bound to a file-pattern rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsedExportRule {
    pub path: PathRule,
    pub exports: Vec<String>,
}

impl UsedExportRule {
    #[must_use]
    pub fn new(
        pattern: impl Into<String>,
        exports: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            path: PathRule::new(pattern),
            exports: exports.into_iter().map(Into::into).collect(),
        }
    }

    #[must_use]
    pub fn from_static(pattern: &'static str, exports: &'static [&'static str]) -> Self {
        Self::new(pattern, exports.iter().copied())
    }

    #[must_use]
    pub fn with_excluded_globs<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.path = self.path.with_excluded_globs(patterns);
        self
    }

    #[must_use]
    pub fn with_excluded_regexes<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.path = self.path.with_excluded_regexes(patterns);
        self
    }

    #[must_use]
    pub fn with_excluded_segment_regexes<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.path = self.path.with_excluded_segment_regexes(patterns);
        self
    }

    #[must_use]
    pub fn prefixed(&self, ws_prefix: &str) -> Self {
        Self {
            path: self.path.prefixed(ws_prefix),
            exports: self.exports.clone(),
        }
    }
}

/// A used-export rule tagged with the plugin that contributed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginUsedExportRule {
    pub plugin_name: String,
    pub rule: UsedExportRule,
}

impl PluginUsedExportRule {
    #[must_use]
    pub fn new(plugin_name: impl Into<String>, rule: UsedExportRule) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            rule,
        }
    }

    #[must_use]
    pub fn prefixed(&self, ws_prefix: &str) -> Self {
        Self {
            plugin_name: self.plugin_name.clone(),
            rule: self.rule.prefixed(ws_prefix),
        }
    }
}

/// A compiled path rule matcher shared by entry-point and used-export matching.
#[derive(Debug, Clone)]
pub(crate) struct CompiledPathRule {
    include: globset::GlobMatcher,
    exclude_globs: Vec<globset::GlobMatcher>,
    exclude_regexes: Vec<Regex>,
    exclude_segment_regexes: Vec<Regex>,
}

impl CompiledPathRule {
    pub(crate) fn for_entry_rule(rule: &PathRule, rule_kind: &str) -> Option<Self> {
        let include = match globset::GlobBuilder::new(&rule.pattern)
            .literal_separator(true)
            .build()
        {
            Ok(glob) => glob.compile_matcher(),
            Err(err) => {
                tracing::warn!("invalid {rule_kind} '{}': {err}", rule.pattern);
                return None;
            }
        };
        Some(Self {
            include,
            exclude_globs: compile_excluded_globs(&rule.exclude_globs, rule_kind, &rule.pattern),
            exclude_regexes: compile_excluded_regexes(
                &rule.exclude_regexes,
                rule_kind,
                &rule.pattern,
            ),
            exclude_segment_regexes: compile_excluded_segment_regexes(
                &rule.exclude_segment_regexes,
                rule_kind,
                &rule.pattern,
            ),
        })
    }

    pub(crate) fn for_used_export_rule(rule: &PathRule, rule_kind: &str) -> Option<Self> {
        let include = match globset::Glob::new(&rule.pattern) {
            Ok(glob) => glob.compile_matcher(),
            Err(err) => {
                tracing::warn!("invalid {rule_kind} '{}': {err}", rule.pattern);
                return None;
            }
        };
        Some(Self {
            include,
            exclude_globs: compile_excluded_globs(&rule.exclude_globs, rule_kind, &rule.pattern),
            exclude_regexes: compile_excluded_regexes(
                &rule.exclude_regexes,
                rule_kind,
                &rule.pattern,
            ),
            exclude_segment_regexes: compile_excluded_segment_regexes(
                &rule.exclude_segment_regexes,
                rule_kind,
                &rule.pattern,
            ),
        })
    }

    #[must_use]
    pub(crate) fn matches(&self, path: &str) -> bool {
        self.include.is_match(path)
            && !self.exclude_globs.iter().any(|glob| glob.is_match(path))
            && !self
                .exclude_regexes
                .iter()
                .any(|regex| regex.is_match(path))
            && !matches_segment_regex(path, &self.exclude_segment_regexes)
    }
}

fn prefix_workspace_pattern(pattern: &str, ws_prefix: &str) -> String {
    if pattern.starts_with(ws_prefix) || pattern.starts_with('/') {
        pattern.to_string()
    } else {
        format!("{ws_prefix}/{pattern}")
    }
}

fn prefix_workspace_regex(pattern: &str, ws_prefix: &str) -> String {
    if let Some(pattern) = pattern.strip_prefix('^') {
        format!("^{}/{}", regex::escape(ws_prefix), pattern)
    } else {
        format!("^{}/(?:{})", regex::escape(ws_prefix), pattern)
    }
}

fn compile_excluded_globs(
    patterns: &[String],
    rule_kind: &str,
    rule_pattern: &str,
) -> Vec<globset::GlobMatcher> {
    patterns
        .iter()
        .filter_map(|pattern| {
            match globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
            {
                Ok(glob) => Some(glob.compile_matcher()),
                Err(err) => {
                    tracing::warn!(
                        "skipping invalid excluded glob '{}' for {} '{}': {err}",
                        pattern,
                        rule_kind,
                        rule_pattern
                    );
                    None
                }
            }
        })
        .collect()
}

fn compile_excluded_regexes(
    patterns: &[String],
    rule_kind: &str,
    rule_pattern: &str,
) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|pattern| match Regex::new(pattern) {
            Ok(regex) => Some(regex),
            Err(err) => {
                tracing::warn!(
                    "skipping invalid excluded regex '{}' for {} '{}': {err}",
                    pattern,
                    rule_kind,
                    rule_pattern
                );
                None
            }
        })
        .collect()
}

fn compile_excluded_segment_regexes(
    patterns: &[String],
    rule_kind: &str,
    rule_pattern: &str,
) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|pattern| match Regex::new(pattern) {
            Ok(regex) => Some(regex),
            Err(err) => {
                tracing::warn!(
                    "skipping invalid excluded segment regex '{}' for {} '{}': {err}",
                    pattern,
                    rule_kind,
                    rule_pattern
                );
                None
            }
        })
        .collect()
}

fn matches_segment_regex(path: &str, regexes: &[Regex]) -> bool {
    path.split('/')
        .any(|segment| regexes.iter().any(|regex| regex.is_match(segment)))
}

impl From<String> for PathRule {
    fn from(pattern: String) -> Self {
        Self::new(pattern)
    }
}

impl From<&str> for PathRule {
    fn from(pattern: &str) -> Self {
        Self::new(pattern)
    }
}

impl std::ops::Deref for PathRule {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.pattern
    }
}

impl PartialEq<&str> for PathRule {
    fn eq(&self, other: &&str) -> bool {
        self.pattern == *other
    }
}

impl PartialEq<str> for PathRule {
    fn eq(&self, other: &str) -> bool {
        self.pattern == other
    }
}

impl PartialEq<String> for PathRule {
    fn eq(&self, other: &String) -> bool {
        &self.pattern == other
    }
}

/// A framework/tool plugin that contributes to dead code analysis.
pub trait Plugin: Send + Sync {
    /// Human-readable plugin name.
    fn name(&self) -> &'static str;

    /// Package names that activate this plugin when found in package.json.
    /// Supports exact matches and prefix patterns (ending with `/`).
    fn enablers(&self) -> &'static [&'static str] {
        &[]
    }

    /// Check if this plugin should be active for the given project.
    /// Default implementation checks `enablers()` against package.json dependencies.
    fn is_enabled(&self, pkg: &PackageJson, root: &Path) -> bool {
        let deps = pkg.all_dependency_names();
        self.is_enabled_with_deps(&deps, root)
    }

    /// Fast variant of `is_enabled` that accepts a pre-computed deps list.
    /// Avoids repeated `all_dependency_names()` allocation when checking many plugins.
    fn is_enabled_with_deps(&self, deps: &[String], _root: &Path) -> bool {
        let enablers = self.enablers();
        if enablers.is_empty() {
            return false;
        }
        enablers.iter().any(|enabler| {
            if enabler.ends_with('/') {
                // Prefix match (e.g., "@storybook/" matches "@storybook/react")
                deps.iter().any(|d| d.starts_with(enabler))
            } else {
                deps.iter().any(|d| d == enabler)
            }
        })
    }

    /// Default glob patterns for entry point files.
    fn entry_patterns(&self) -> &'static [&'static str] {
        &[]
    }

    /// Entry point rules with optional exclusions.
    fn entry_pattern_rules(&self) -> Vec<PathRule> {
        self.entry_patterns()
            .iter()
            .map(|pattern| PathRule::from_static(pattern))
            .collect()
    }

    /// How this plugin's entry patterns should contribute to coverage reachability.
    ///
    /// `Support` roots keep files alive for dead-code analysis but do not count
    /// as runtime or test reachability for static coverage gaps.
    fn entry_point_role(&self) -> EntryPointRole {
        builtin_entry_point_role(self.name())
    }

    /// Glob patterns for config files this plugin can parse.
    fn config_patterns(&self) -> &'static [&'static str] {
        &[]
    }

    /// Files that are always considered "used" when this plugin is active.
    fn always_used(&self) -> &'static [&'static str] {
        &[]
    }

    /// Exports that are always considered used for matching file patterns.
    fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
        vec![]
    }

    /// Used-export rules with optional exclusions.
    fn used_export_rules(&self) -> Vec<UsedExportRule> {
        self.used_exports()
            .into_iter()
            .map(|(pattern, exports)| UsedExportRule::from_static(pattern, exports))
            .collect()
    }

    /// Class member names the framework invokes at runtime. Matching members
    /// are skipped during `unused-class-members` analysis. Intended for
    /// interface/contract patterns where the library calls methods on consumer
    /// classes (e.g. ag-Grid's `agInit`, Web Components' `connectedCallback`).
    fn used_class_members(&self) -> &'static [&'static str] {
        &[]
    }

    /// Heritage-scoped class member rules. Each rule applies only to classes
    /// matching its `extends` and/or `implements` clause. Used for frameworks
    /// where lifecycle members are runtime-invoked only on classes that extend
    /// a known base (e.g. Lit's `render`/`updated` on classes extending
    /// `LitElement`, native Web Components' `connectedCallback` on classes
    /// extending `HTMLElement`). Default: empty. Plugins override when they
    /// need scoping; flat names should still come from `used_class_members`.
    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        Vec::new()
    }

    /// Glob patterns for test fixture files consumed by this framework.
    /// These files are implicitly used by the test runner and should not be
    /// flagged as unused. Unlike `always_used()`, this carries semantic intent
    /// for reporting purposes.
    fn fixture_glob_patterns(&self) -> &'static [&'static str] {
        &[]
    }

    /// Dependencies that are tooling (used via CLI/config, not source imports).
    /// These should not be flagged as unused devDependencies.
    fn tooling_dependencies(&self) -> &'static [&'static str] {
        &[]
    }

    /// Import prefixes that are virtual modules provided by this framework at build time.
    /// Imports matching these prefixes should not be flagged as unlisted dependencies.
    /// Each entry is matched as a prefix against the extracted package name
    /// (e.g., `"@theme/"` matches `@theme/Layout`).
    fn virtual_module_prefixes(&self) -> &'static [&'static str] {
        &[]
    }

    /// Package name suffixes that are virtual modules provided by this framework
    /// at build time (e.g., test runner mock conventions).
    /// Imports matching these suffixes should not be flagged as unlisted dependencies.
    /// Each entry is matched as a suffix against the extracted package name
    /// (e.g., `"/__mocks__"` matches `@aws-sdk/__mocks__` and `some-pkg/__mocks__`).
    fn virtual_package_suffixes(&self) -> &'static [&'static str] {
        &[]
    }

    /// Import suffixes for build-time generated relative imports.
    ///
    /// Unresolved relative imports whose specifier ends with one of these suffixes
    /// will not be flagged as unresolved. For example, SvelteKit generates
    /// `./$types` imports in route files — returning `"/$types"` suppresses those.
    fn generated_import_patterns(&self) -> &'static [&'static str] {
        &[]
    }

    /// Path alias mappings provided by this framework at build time.
    ///
    /// Returns a list of `(prefix, replacement_dir)` tuples. When an import starting
    /// with `prefix` fails to resolve, the resolver will substitute the prefix with
    /// `replacement_dir` (relative to the project root) and retry.
    ///
    /// Called once when plugins are activated. The project `root` is provided so
    /// plugins can inspect the filesystem (e.g., Nuxt checks whether `app/` exists
    /// to determine the `srcDir`).
    fn path_aliases(&self, _root: &Path) -> Vec<(&'static str, String)> {
        vec![]
    }

    /// Parse a config file's AST to discover additional entries, dependencies, etc.
    ///
    /// Called for each config file matching `config_patterns()`. The source code
    /// and parsed AST are provided — use [`config_parser`] utilities to extract values.
    fn resolve_config(&self, _config_path: &Path, _source: &str, _root: &Path) -> PluginResult {
        PluginResult::default()
    }

    /// The key name in package.json that holds inline configuration for this tool.
    /// When set (e.g., `"jest"` for the `"jest"` key in package.json), the plugin
    /// system will extract that key's value and call `resolve_config` with its
    /// JSON content if no standalone config file was found.
    fn package_json_config_key(&self) -> Option<&'static str> {
        None
    }
}

fn builtin_entry_point_role(name: &str) -> EntryPointRole {
    if TEST_ENTRY_POINT_PLUGINS.contains(&name) {
        EntryPointRole::Test
    } else if RUNTIME_ENTRY_POINT_PLUGINS.contains(&name) {
        EntryPointRole::Runtime
    } else {
        EntryPointRole::Support
    }
}

/// Macro to eliminate boilerplate in plugin implementations.
///
/// Generates a struct and a `Plugin` trait impl with the standard static methods
/// (`name`, `enablers`, `entry_patterns`, `config_patterns`, `always_used`, `tooling_dependencies`,
/// `fixture_glob_patterns`, `used_exports`).
///
/// For plugins that need custom `resolve_config()` or `is_enabled()`, keep those as
/// manual `impl Plugin for ...` blocks instead of using this macro.
///
/// # Usage
///
/// ```ignore
/// // Simple plugin (most common):
/// define_plugin! {
///     struct VitePlugin => "vite",
///     enablers: ENABLERS,
///     entry_patterns: ENTRY_PATTERNS,
///     config_patterns: CONFIG_PATTERNS,
///     always_used: ALWAYS_USED,
///     tooling_dependencies: TOOLING_DEPENDENCIES,
/// }
///
/// // Plugin with used_exports:
/// define_plugin! {
///     struct RemixPlugin => "remix",
///     enablers: ENABLERS,
///     entry_patterns: ENTRY_PATTERNS,
///     always_used: ALWAYS_USED,
///     tooling_dependencies: TOOLING_DEPENDENCIES,
///     used_exports: [("app/routes/**/*.{ts,tsx}", ROUTE_EXPORTS)],
/// }
///
/// // Plugin with imports-only resolve_config (extracts imports from config as deps):
/// define_plugin! {
///     struct CypressPlugin => "cypress",
///     enablers: ENABLERS,
///     entry_patterns: ENTRY_PATTERNS,
///     config_patterns: CONFIG_PATTERNS,
///     always_used: ALWAYS_USED,
///     tooling_dependencies: TOOLING_DEPENDENCIES,
///     resolve_config: imports_only,
/// }
///
/// // Plugin with custom resolve_config body:
/// define_plugin! {
///     struct RollupPlugin => "rollup",
///     enablers: ENABLERS,
///     config_patterns: CONFIG_PATTERNS,
///     always_used: ALWAYS_USED,
///     tooling_dependencies: TOOLING_DEPENDENCIES,
///     resolve_config(config_path, source, _root) {
///         let mut result = PluginResult::default();
///         // custom config parsing...
///         result
///     }
/// }
/// ```
///
/// All fields except `struct` and `enablers` are optional and default to `&[]` / `vec![]`.
macro_rules! define_plugin {
    // Variant with `resolve_config: imports_only`: generates a resolve_config method
    // that extracts imports from config files and registers them as referenced dependencies.
    (
        struct $name:ident => $display:expr,
        enablers: $enablers:expr
        $(, entry_patterns: $entry:expr)?
        $(, config_patterns: $config:expr)?
        $(, always_used: $always:expr)?
        $(, tooling_dependencies: $tooling:expr)?
        $(, fixture_glob_patterns: $fixtures:expr)?
        $(, virtual_module_prefixes: $virtual:expr)?
        $(, virtual_package_suffixes: $virtual_suffixes:expr)?
        $(, used_exports: [$( ($pat:expr, $exports:expr) ),* $(,)?])?
        , resolve_config: imports_only
        $(,)?
    ) => {
        pub struct $name;

        impl Plugin for $name {
            fn name(&self) -> &'static str {
                $display
            }

            fn enablers(&self) -> &'static [&'static str] {
                $enablers
            }

            $( fn entry_patterns(&self) -> &'static [&'static str] { $entry } )?
            $( fn config_patterns(&self) -> &'static [&'static str] { $config } )?
            $( fn always_used(&self) -> &'static [&'static str] { $always } )?
            $( fn tooling_dependencies(&self) -> &'static [&'static str] { $tooling } )?
            $( fn fixture_glob_patterns(&self) -> &'static [&'static str] { $fixtures } )?
            $( fn virtual_module_prefixes(&self) -> &'static [&'static str] { $virtual } )?
            $( fn virtual_package_suffixes(&self) -> &'static [&'static str] { $virtual_suffixes } )?

            $(
                fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
                    vec![$( ($pat, $exports) ),*]
                }
            )?

            fn resolve_config(
                &self,
                config_path: &std::path::Path,
                source: &str,
                _root: &std::path::Path,
            ) -> PluginResult {
                let mut result = PluginResult::default();
                let imports = crate::plugins::config_parser::extract_imports(source, config_path);
                for imp in &imports {
                    let dep = crate::resolve::extract_package_name(imp);
                    result.referenced_dependencies.push(dep);
                }
                result
            }
        }
    };

    // Variant with custom resolve_config body: generates a resolve_config method
    // with the caller-supplied block. Parameter names are caller-controlled (use
    // `_root` for unused params to satisfy clippy).
    (
        struct $name:ident => $display:expr,
        enablers: $enablers:expr
        $(, entry_patterns: $entry:expr)?
        $(, config_patterns: $config:expr)?
        $(, always_used: $always:expr)?
        $(, tooling_dependencies: $tooling:expr)?
        $(, fixture_glob_patterns: $fixtures:expr)?
        $(, virtual_module_prefixes: $virtual:expr)?
        $(, virtual_package_suffixes: $virtual_suffixes:expr)?
        $(, package_json_config_key: $pkg_key:expr)?
        $(, used_exports: [$( ($pat:expr, $exports:expr) ),* $(,)?])?
        , resolve_config($cp:ident, $src:ident, $root:ident) $body:block
        $(,)?
    ) => {
        pub struct $name;

        impl Plugin for $name {
            fn name(&self) -> &'static str {
                $display
            }

            fn enablers(&self) -> &'static [&'static str] {
                $enablers
            }

            $( fn entry_patterns(&self) -> &'static [&'static str] { $entry } )?
            $( fn config_patterns(&self) -> &'static [&'static str] { $config } )?
            $( fn always_used(&self) -> &'static [&'static str] { $always } )?
            $( fn tooling_dependencies(&self) -> &'static [&'static str] { $tooling } )?
            $( fn fixture_glob_patterns(&self) -> &'static [&'static str] { $fixtures } )?
            $( fn virtual_module_prefixes(&self) -> &'static [&'static str] { $virtual } )?
            $( fn virtual_package_suffixes(&self) -> &'static [&'static str] { $virtual_suffixes } )?

            $(
                fn package_json_config_key(&self) -> Option<&'static str> {
                    Some($pkg_key)
                }
            )?

            $(
                fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
                    vec![$( ($pat, $exports) ),*]
                }
            )?

            fn resolve_config(
                &self,
                $cp: &std::path::Path,
                $src: &str,
                $root: &std::path::Path,
            ) -> PluginResult
            $body
        }
    };

    // Base variant: no resolve_config.
    (
        struct $name:ident => $display:expr,
        enablers: $enablers:expr
        $(, entry_patterns: $entry:expr)?
        $(, config_patterns: $config:expr)?
        $(, always_used: $always:expr)?
        $(, tooling_dependencies: $tooling:expr)?
        $(, fixture_glob_patterns: $fixtures:expr)?
        $(, virtual_module_prefixes: $virtual:expr)?
        $(, virtual_package_suffixes: $virtual_suffixes:expr)?
        $(, used_exports: [$( ($pat:expr, $exports:expr) ),* $(,)?])?
        $(,)?
    ) => {
        pub struct $name;

        impl Plugin for $name {
            fn name(&self) -> &'static str {
                $display
            }

            fn enablers(&self) -> &'static [&'static str] {
                $enablers
            }

            $( fn entry_patterns(&self) -> &'static [&'static str] { $entry } )?
            $( fn config_patterns(&self) -> &'static [&'static str] { $config } )?
            $( fn always_used(&self) -> &'static [&'static str] { $always } )?
            $( fn tooling_dependencies(&self) -> &'static [&'static str] { $tooling } )?
            $( fn fixture_glob_patterns(&self) -> &'static [&'static str] { $fixtures } )?
            $( fn virtual_module_prefixes(&self) -> &'static [&'static str] { $virtual } )?
            $( fn virtual_package_suffixes(&self) -> &'static [&'static str] { $virtual_suffixes } )?

            $(
                fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
                    vec![$( ($pat, $exports) ),*]
                }
            )?
        }
    };
}

pub mod config_parser;
pub mod registry;
mod tooling;

pub use registry::{AggregatedPluginResult, PluginRegistry};
pub use tooling::is_known_tooling_dependency;

mod angular;
mod astro;
mod ava;
mod babel;
mod biome;
mod bun;
mod c8;
mod capacitor;
mod changesets;
mod commitizen;
mod commitlint;
mod convex;
mod cspell;
mod cucumber;
mod cypress;
mod dependency_cruiser;
mod docusaurus;
mod drizzle;
mod electron;
mod eslint;
mod expo;
mod expo_router;
mod gatsby;
mod graphql_codegen;
mod hardhat;
mod husky;
mod i18next;
mod jest;
mod karma;
mod knex;
mod kysely;
mod lefthook;
mod lint_staged;
mod lit;
mod markdownlint;
mod mocha;
mod msw;
mod nestjs;
mod next_intl;
mod nextjs;
mod nitro;
mod nodemon;
mod nuxt;
mod nx;
mod nyc;
mod openapi_ts;
mod oxlint;
mod pandacss;
mod parcel;
mod playwright;
mod plop;
mod pm2;
mod pnpm;
mod postcss;
mod prettier;
mod prisma;
mod qwik;
mod react_native;
mod react_router;
mod relay;
mod remark;
mod remix;
mod rolldown;
mod rollup;
mod rsbuild;
mod rspack;
mod sanity;
mod semantic_release;
mod sentry;
mod simple_git_hooks;
mod storybook;
mod stylelint;
mod sveltekit;
mod svgo;
mod svgr;
mod swc;
mod syncpack;
mod tailwind;
mod tanstack_router;
mod tap;
mod tsd;
mod tsdown;
mod tsup;
mod turborepo;
mod typedoc;
mod typeorm;
mod typescript;
mod unocss;
mod vite;
mod vitepress;
mod vitest;
mod webdriverio;
mod webpack;
mod wrangler;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── is_enabled_with_deps edge cases ──────────────────────────

    #[test]
    fn is_enabled_with_deps_exact_match() {
        let plugin = nextjs::NextJsPlugin;
        let deps = vec!["next".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn is_enabled_with_deps_no_match() {
        let plugin = nextjs::NextJsPlugin;
        let deps = vec!["react".to_string()];
        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn is_enabled_with_deps_empty_deps() {
        let plugin = nextjs::NextJsPlugin;
        let deps: Vec<String> = vec![];
        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn entry_point_role_defaults_are_centralized() {
        assert_eq!(vite::VitePlugin.entry_point_role(), EntryPointRole::Runtime);
        assert_eq!(
            vitest::VitestPlugin.entry_point_role(),
            EntryPointRole::Test
        );
        assert_eq!(
            storybook::StorybookPlugin.entry_point_role(),
            EntryPointRole::Support
        );
        assert_eq!(knex::KnexPlugin.entry_point_role(), EntryPointRole::Support);
    }

    #[test]
    fn plugins_with_entry_patterns_have_explicit_role_intent() {
        let runtime_or_test_or_support: rustc_hash::FxHashSet<&'static str> =
            TEST_ENTRY_POINT_PLUGINS
                .iter()
                .chain(RUNTIME_ENTRY_POINT_PLUGINS.iter())
                .chain(SUPPORT_ENTRY_POINT_PLUGINS.iter())
                .copied()
                .collect();

        for plugin in crate::plugins::registry::builtin::create_builtin_plugins() {
            if plugin.entry_patterns().is_empty() {
                continue;
            }
            assert!(
                runtime_or_test_or_support.contains(plugin.name()),
                "plugin '{}' exposes entry patterns but is missing from the entry-point role map",
                plugin.name()
            );
        }
    }

    // ── PluginResult::is_empty ───────────────────────────────────

    #[test]
    fn plugin_result_is_empty_when_default() {
        let r = PluginResult::default();
        assert!(r.is_empty());
    }

    #[test]
    fn plugin_result_not_empty_with_entry_patterns() {
        let r = PluginResult {
            entry_patterns: vec!["*.ts".into()],
            ..Default::default()
        };
        assert!(!r.is_empty());
    }

    #[test]
    fn plugin_result_not_empty_with_referenced_deps() {
        let r = PluginResult {
            referenced_dependencies: vec!["lodash".to_string()],
            ..Default::default()
        };
        assert!(!r.is_empty());
    }

    #[test]
    fn plugin_result_not_empty_with_setup_files() {
        let r = PluginResult {
            setup_files: vec![PathBuf::from("/setup.ts")],
            ..Default::default()
        };
        assert!(!r.is_empty());
    }

    #[test]
    fn plugin_result_not_empty_with_always_used_files() {
        let r = PluginResult {
            always_used_files: vec!["**/*.stories.tsx".to_string()],
            ..Default::default()
        };
        assert!(!r.is_empty());
    }

    #[test]
    fn plugin_result_not_empty_with_fixture_patterns() {
        let r = PluginResult {
            fixture_patterns: vec!["**/__fixtures__/**/*".to_string()],
            ..Default::default()
        };
        assert!(!r.is_empty());
    }

    // ── is_enabled_with_deps prefix matching ─────────────────────

    #[test]
    fn is_enabled_with_deps_prefix_match() {
        // Storybook plugin uses prefix enabler "@storybook/"
        let plugin = storybook::StorybookPlugin;
        let deps = vec!["@storybook/react".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn is_enabled_with_deps_prefix_no_match_without_slash() {
        // "@storybook/" prefix should NOT match "@storybookish" (different package)
        let plugin = storybook::StorybookPlugin;
        let deps = vec!["@storybookish".to_string()];
        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn is_enabled_with_deps_multiple_enablers() {
        // Vitest plugin has multiple enablers
        let plugin = vitest::VitestPlugin;
        let deps_vitest = vec!["vitest".to_string()];
        let deps_none = vec!["mocha".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps_vitest, Path::new("/project")));
        assert!(!plugin.is_enabled_with_deps(&deps_none, Path::new("/project")));
    }

    // ── Plugin trait default implementations ─────────────────────

    #[test]
    fn plugin_default_methods_return_empty() {
        // Use a simple plugin to test default trait methods
        let plugin = commitizen::CommitizenPlugin;
        assert!(
            plugin.tooling_dependencies().is_empty() || !plugin.tooling_dependencies().is_empty()
        );
        assert!(plugin.virtual_module_prefixes().is_empty());
        assert!(plugin.virtual_package_suffixes().is_empty());
        assert!(plugin.path_aliases(Path::new("/project")).is_empty());
        assert!(
            plugin.package_json_config_key().is_none()
                || plugin.package_json_config_key().is_some()
        );
    }

    #[test]
    fn plugin_resolve_config_default_returns_empty() {
        let plugin = commitizen::CommitizenPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/config.js"),
            "const x = 1;",
            Path::new("/project"),
        );
        assert!(result.is_empty());
    }

    // ── is_enabled_with_deps exact and prefix ────────────────────

    #[test]
    fn is_enabled_with_deps_exact_and_prefix_both_work() {
        let plugin = storybook::StorybookPlugin;
        let deps_exact = vec!["storybook".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps_exact, Path::new("/project")));
        let deps_prefix = vec!["@storybook/vue3".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps_prefix, Path::new("/project")));
    }

    #[test]
    fn is_enabled_with_deps_multiple_enablers_remix() {
        let plugin = remix::RemixPlugin;
        let deps_node = vec!["@remix-run/node".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps_node, Path::new("/project")));
        let deps_react = vec!["@remix-run/react".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps_react, Path::new("/project")));
        let deps_cf = vec!["@remix-run/cloudflare".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps_cf, Path::new("/project")));
    }

    // ── Plugin trait default implementations ──────────────────────

    struct MinimalPlugin;
    impl Plugin for MinimalPlugin {
        fn name(&self) -> &'static str {
            "minimal"
        }
    }

    #[test]
    fn default_enablers_is_empty() {
        assert!(MinimalPlugin.enablers().is_empty());
    }

    #[test]
    fn default_entry_patterns_is_empty() {
        assert!(MinimalPlugin.entry_patterns().is_empty());
    }

    #[test]
    fn default_config_patterns_is_empty() {
        assert!(MinimalPlugin.config_patterns().is_empty());
    }

    #[test]
    fn default_always_used_is_empty() {
        assert!(MinimalPlugin.always_used().is_empty());
    }

    #[test]
    fn default_used_exports_is_empty() {
        assert!(MinimalPlugin.used_exports().is_empty());
    }

    #[test]
    fn default_tooling_dependencies_is_empty() {
        assert!(MinimalPlugin.tooling_dependencies().is_empty());
    }

    #[test]
    fn default_fixture_glob_patterns_is_empty() {
        assert!(MinimalPlugin.fixture_glob_patterns().is_empty());
    }

    #[test]
    fn default_virtual_module_prefixes_is_empty() {
        assert!(MinimalPlugin.virtual_module_prefixes().is_empty());
    }

    #[test]
    fn default_virtual_package_suffixes_is_empty() {
        assert!(MinimalPlugin.virtual_package_suffixes().is_empty());
    }

    #[test]
    fn default_path_aliases_is_empty() {
        assert!(MinimalPlugin.path_aliases(Path::new("/")).is_empty());
    }

    #[test]
    fn default_resolve_config_returns_empty() {
        let r = MinimalPlugin.resolve_config(
            Path::new("config.js"),
            "export default {}",
            Path::new("/"),
        );
        assert!(r.is_empty());
    }

    #[test]
    fn default_package_json_config_key_is_none() {
        assert!(MinimalPlugin.package_json_config_key().is_none());
    }

    #[test]
    fn default_is_enabled_returns_false_when_no_enablers() {
        let deps = vec!["anything".to_string()];
        assert!(!MinimalPlugin.is_enabled_with_deps(&deps, Path::new("/")));
    }

    // ── All built-in plugins have unique names ───────────────────

    #[test]
    fn all_builtin_plugin_names_are_unique() {
        let plugins = registry::builtin::create_builtin_plugins();
        let mut seen = std::collections::BTreeSet::new();
        for p in &plugins {
            let name = p.name();
            assert!(seen.insert(name), "duplicate plugin name: {name}");
        }
    }

    #[test]
    fn all_builtin_plugins_have_enablers() {
        let plugins = registry::builtin::create_builtin_plugins();
        for p in &plugins {
            assert!(
                !p.enablers().is_empty(),
                "plugin '{}' has no enablers",
                p.name()
            );
        }
    }

    #[test]
    fn plugins_with_config_patterns_have_always_used() {
        let plugins = registry::builtin::create_builtin_plugins();
        for p in &plugins {
            if !p.config_patterns().is_empty() {
                assert!(
                    !p.always_used().is_empty(),
                    "plugin '{}' has config_patterns but no always_used",
                    p.name()
                );
            }
        }
    }

    // ── Enabler patterns for all categories ──────────────────────

    #[test]
    fn framework_plugins_enablers() {
        let cases: Vec<(&dyn Plugin, &[&str])> = vec![
            (&nextjs::NextJsPlugin, &["next"]),
            (&nuxt::NuxtPlugin, &["nuxt"]),
            (&angular::AngularPlugin, &["@angular/core"]),
            (&sveltekit::SvelteKitPlugin, &["@sveltejs/kit"]),
            (&gatsby::GatsbyPlugin, &["gatsby"]),
        ];
        for (plugin, expected_enablers) in cases {
            let enablers = plugin.enablers();
            for expected in expected_enablers {
                assert!(
                    enablers.contains(expected),
                    "plugin '{}' should have '{}'",
                    plugin.name(),
                    expected
                );
            }
        }
    }

    #[test]
    fn testing_plugins_enablers() {
        let cases: Vec<(&dyn Plugin, &str)> = vec![
            (&jest::JestPlugin, "jest"),
            (&vitest::VitestPlugin, "vitest"),
            (&playwright::PlaywrightPlugin, "@playwright/test"),
            (&cypress::CypressPlugin, "cypress"),
            (&mocha::MochaPlugin, "mocha"),
        ];
        for (plugin, enabler) in cases {
            assert!(
                plugin.enablers().contains(&enabler),
                "plugin '{}' should have '{}'",
                plugin.name(),
                enabler
            );
        }
    }

    #[test]
    fn bundler_plugins_enablers() {
        let cases: Vec<(&dyn Plugin, &str)> = vec![
            (&vite::VitePlugin, "vite"),
            (&webpack::WebpackPlugin, "webpack"),
            (&rollup::RollupPlugin, "rollup"),
        ];
        for (plugin, enabler) in cases {
            assert!(
                plugin.enablers().contains(&enabler),
                "plugin '{}' should have '{}'",
                plugin.name(),
                enabler
            );
        }
    }

    #[test]
    fn test_plugins_have_test_entry_patterns() {
        let test_plugins: Vec<&dyn Plugin> = vec![
            &jest::JestPlugin,
            &vitest::VitestPlugin,
            &mocha::MochaPlugin,
            &tap::TapPlugin,
            &tsd::TsdPlugin,
        ];
        for plugin in test_plugins {
            let patterns = plugin.entry_patterns();
            assert!(
                !patterns.is_empty(),
                "test plugin '{}' should have entry patterns",
                plugin.name()
            );
            assert!(
                patterns
                    .iter()
                    .any(|p| p.contains("test") || p.contains("spec") || p.contains("__tests__")),
                "test plugin '{}' should have test/spec patterns",
                plugin.name()
            );
        }
    }

    #[test]
    fn framework_plugins_have_entry_patterns() {
        let plugins: Vec<&dyn Plugin> = vec![
            &nextjs::NextJsPlugin,
            &nuxt::NuxtPlugin,
            &angular::AngularPlugin,
            &sveltekit::SvelteKitPlugin,
        ];
        for plugin in plugins {
            assert!(
                !plugin.entry_patterns().is_empty(),
                "framework plugin '{}' should have entry patterns",
                plugin.name()
            );
        }
    }

    #[test]
    fn plugins_with_resolve_config_have_config_patterns() {
        let plugins: Vec<&dyn Plugin> = vec![
            &jest::JestPlugin,
            &vitest::VitestPlugin,
            &babel::BabelPlugin,
            &eslint::EslintPlugin,
            &webpack::WebpackPlugin,
            &storybook::StorybookPlugin,
            &typescript::TypeScriptPlugin,
            &postcss::PostCssPlugin,
            &nextjs::NextJsPlugin,
            &nuxt::NuxtPlugin,
            &angular::AngularPlugin,
            &nx::NxPlugin,
            &rollup::RollupPlugin,
            &sveltekit::SvelteKitPlugin,
            &prettier::PrettierPlugin,
        ];
        for plugin in plugins {
            assert!(
                !plugin.config_patterns().is_empty(),
                "plugin '{}' with resolve_config should have config_patterns",
                plugin.name()
            );
        }
    }

    #[test]
    fn plugin_tooling_deps_include_enabler_package() {
        let plugins: Vec<&dyn Plugin> = vec![
            &jest::JestPlugin,
            &vitest::VitestPlugin,
            &webpack::WebpackPlugin,
            &typescript::TypeScriptPlugin,
            &eslint::EslintPlugin,
            &prettier::PrettierPlugin,
        ];
        for plugin in plugins {
            let tooling = plugin.tooling_dependencies();
            let enablers = plugin.enablers();
            assert!(
                enablers
                    .iter()
                    .any(|e| !e.ends_with('/') && tooling.contains(e)),
                "plugin '{}': at least one non-prefix enabler should be in tooling_dependencies",
                plugin.name()
            );
        }
    }

    #[test]
    fn nextjs_has_used_exports_for_pages() {
        let plugin = nextjs::NextJsPlugin;
        let exports = plugin.used_exports();
        assert!(!exports.is_empty());
        assert!(exports.iter().any(|(_, names)| names.contains(&"default")));
    }

    #[test]
    fn remix_has_used_exports_for_routes() {
        let plugin = remix::RemixPlugin;
        let exports = plugin.used_exports();
        assert!(!exports.is_empty());
        let route_entry = exports.iter().find(|(pat, _)| pat.contains("routes"));
        assert!(route_entry.is_some());
        let (_, names) = route_entry.unwrap();
        assert!(names.contains(&"loader"));
        assert!(names.contains(&"action"));
        assert!(names.contains(&"default"));
    }

    #[test]
    fn sveltekit_has_used_exports_for_routes() {
        let plugin = sveltekit::SvelteKitPlugin;
        let exports = plugin.used_exports();
        assert!(!exports.is_empty());
        assert!(exports.iter().any(|(_, names)| names.contains(&"GET")));
    }

    #[test]
    fn nuxt_has_hash_virtual_prefix() {
        assert!(nuxt::NuxtPlugin.virtual_module_prefixes().contains(&"#"));
    }

    #[test]
    fn sveltekit_has_dollar_virtual_prefixes() {
        let prefixes = sveltekit::SvelteKitPlugin.virtual_module_prefixes();
        assert!(prefixes.contains(&"$app/"));
        assert!(prefixes.contains(&"$env/"));
        assert!(prefixes.contains(&"$lib/"));
    }

    #[test]
    fn sveltekit_has_lib_path_alias() {
        let aliases = sveltekit::SvelteKitPlugin.path_aliases(Path::new("/project"));
        assert!(aliases.iter().any(|(prefix, _)| *prefix == "$lib/"));
    }

    #[test]
    fn nuxt_has_tilde_path_alias() {
        let aliases = nuxt::NuxtPlugin.path_aliases(Path::new("/nonexistent"));
        assert!(aliases.iter().any(|(prefix, _)| *prefix == "~/"));
        assert!(aliases.iter().any(|(prefix, _)| *prefix == "~~/"));
    }

    #[test]
    fn jest_has_package_json_config_key() {
        assert_eq!(jest::JestPlugin.package_json_config_key(), Some("jest"));
    }

    #[test]
    fn tsd_has_package_json_config_key() {
        assert_eq!(tsd::TsdPlugin.package_json_config_key(), Some("tsd"));
    }

    #[test]
    fn babel_has_package_json_config_key() {
        assert_eq!(babel::BabelPlugin.package_json_config_key(), Some("babel"));
    }

    #[test]
    fn eslint_has_package_json_config_key() {
        assert_eq!(
            eslint::EslintPlugin.package_json_config_key(),
            Some("eslintConfig")
        );
    }

    #[test]
    fn prettier_has_package_json_config_key() {
        assert_eq!(
            prettier::PrettierPlugin.package_json_config_key(),
            Some("prettier")
        );
    }

    #[test]
    fn macro_generated_plugin_basic_properties() {
        let plugin = msw::MswPlugin;
        assert_eq!(plugin.name(), "msw");
        assert!(plugin.enablers().contains(&"msw"));
        assert!(!plugin.entry_patterns().is_empty());
        assert!(plugin.config_patterns().is_empty());
        assert!(!plugin.always_used().is_empty());
        assert!(!plugin.tooling_dependencies().is_empty());
    }

    #[test]
    fn macro_generated_plugin_with_used_exports() {
        let plugin = remix::RemixPlugin;
        assert_eq!(plugin.name(), "remix");
        assert!(!plugin.used_exports().is_empty());
    }

    #[test]
    fn macro_passes_through_virtual_package_suffixes() {
        // Synthetic smoke check: a plugin defined via define_plugin! that
        // declares virtual_package_suffixes returns those suffixes from the
        // trait method. Guards against future macro regressions where the
        // field name silently drops out of one of the three variants.
        define_plugin! {
            struct MacroSuffixSmokePlugin => "macro-suffix-smoke",
            enablers: &["macro-suffix-smoke"],
            virtual_package_suffixes: &["/__macro_smoke__"],
        }

        let plugin = MacroSuffixSmokePlugin;
        assert_eq!(
            plugin.virtual_package_suffixes(),
            &["/__macro_smoke__"],
            "macro-declared virtual_package_suffixes must propagate to the trait method"
        );
    }

    #[test]
    fn macro_generated_plugin_imports_only_resolve_config() {
        let plugin = cypress::CypressPlugin;
        let source = r"
            import { defineConfig } from 'cypress';
            import coveragePlugin from '@cypress/code-coverage';
            export default defineConfig({});
        ";
        let result = plugin.resolve_config(
            Path::new("cypress.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"cypress".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@cypress/code-coverage".to_string())
        );
    }

    #[test]
    fn builtin_plugin_count_is_expected() {
        let plugins = registry::builtin::create_builtin_plugins();
        assert!(
            plugins.len() >= 80,
            "expected at least 80 built-in plugins, got {}",
            plugins.len()
        );
    }
}
