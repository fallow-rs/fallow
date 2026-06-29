use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::boundaries::ResolvedBoundaryConfig;
use super::duplicates_config::DuplicatesConfig;
use super::flags::FlagsConfig;
use super::format::OutputFormat;
use super::health::HealthConfig;
use super::resolve::ResolveConfig;
use super::rules::{PartialRulesConfig, RulesConfig, Severity};
use super::used_class_members::UsedClassMemberRule;
use crate::external_plugin::{ExternalPluginDef, discover_external_plugins};

use super::IgnoreExportsUsedInFileConfig;
use super::{BoundaryConfig, FallowConfig, ProductionConfig, SecurityConfig};

/// Process-local dedup state for inter-file rule warnings.
static INTER_FILE_WARN_SEEN: OnceLock<Mutex<FxHashSet<u64>>> = OnceLock::new();

/// Stable hash of `(rule_name, sorted glob list)`.
fn inter_file_warn_key(rule_name: &str, files: &[String]) -> u64 {
    let mut sorted: Vec<&str> = files.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let mut hasher = DefaultHasher::new();
    rule_name.hash(&mut hasher);
    for s in &sorted {
        s.hash(&mut hasher);
    }
    hasher.finish()
}

/// Returns `true` if this warning has not yet fired in the current process.
fn record_inter_file_warn_seen(rule_name: &str, files: &[String]) -> bool {
    let seen = INTER_FILE_WARN_SEEN.get_or_init(|| Mutex::new(FxHashSet::default()));
    let key = inter_file_warn_key(rule_name, files);
    seen.lock().map_or(true, |mut set| set.insert(key))
}

#[cfg(test)]
fn reset_inter_file_warn_dedup_for_test() {
    if let Some(seen) = INTER_FILE_WARN_SEEN.get()
        && let Ok(mut set) = seen.lock()
    {
        set.clear();
    }
}

/// Rule for ignoring specific exports.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct IgnoreExportRule {
    /// Glob pattern for files.
    pub file: String,
    /// Export names to ignore (`*` for all).
    pub exports: Vec<String>,
}

/// `IgnoreExportRule` with the glob pre-compiled into a matcher.
#[derive(Debug)]
pub struct CompiledIgnoreExportRule {
    pub matcher: globset::GlobMatcher,
    pub exports: Vec<String>,
}

/// Rule for suppressing an `unresolved-catalog-reference` finding.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IgnoreCatalogReferenceRule {
    pub package: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
}

/// `IgnoreCatalogReferenceRule` with the optional consumer glob pre-compiled.
#[derive(Debug)]
pub struct CompiledIgnoreCatalogReferenceRule {
    pub package: String,
    pub catalog: Option<String>,
    pub consumer_matcher: Option<globset::GlobMatcher>,
}

impl CompiledIgnoreCatalogReferenceRule {
    /// Whether this rule suppresses an `unresolved-catalog-reference` finding.
    #[must_use]
    pub fn matches(&self, package: &str, catalog: &str, consumer_path: &str) -> bool {
        if self.package != package {
            return false;
        }
        if let Some(catalog_filter) = &self.catalog
            && catalog_filter != catalog
        {
            return false;
        }
        if let Some(matcher) = &self.consumer_matcher
            && !matcher.is_match(consumer_path)
        {
            return false;
        }
        true
    }
}

/// Rule for suppressing dependency-override findings.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IgnoreDependencyOverrideRule {
    pub package: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// `IgnoreDependencyOverrideRule` ready for matching.
#[derive(Debug)]
pub struct CompiledIgnoreDependencyOverrideRule {
    pub package: String,
    pub source: Option<String>,
}

impl CompiledIgnoreDependencyOverrideRule {
    /// Whether this rule suppresses a dependency-override finding.
    #[must_use]
    pub fn matches(&self, package: &str, source_label: &str) -> bool {
        if self.package != package {
            return false;
        }
        if let Some(source_filter) = &self.source
            && source_filter != source_label
        {
            return false;
        }
        true
    }
}

/// Per-file override entry.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOverride {
    pub files: Vec<String>,
    #[serde(default)]
    pub rules: PartialRulesConfig,
}

/// Resolved override with pre-compiled glob matchers.
#[derive(Debug)]
pub struct ResolvedOverride {
    pub matchers: Vec<globset::GlobMatcher>,
    pub rules: PartialRulesConfig,
}

/// Fully resolved configuration with all globs pre-compiled.
#[derive(Debug)]
pub struct ResolvedConfig {
    pub root: PathBuf,
    pub entry_patterns: Vec<String>,
    pub ignore_patterns: GlobSet,
    pub output: OutputFormat,
    pub cache_dir: PathBuf,
    pub threads: usize,
    pub no_cache: bool,
    pub cache_max_size_mb: Option<u32>,
    pub cache_config_hash: u64,
    pub ignore_dependencies: Vec<String>,
    pub ignore_unresolved_imports: Vec<GlobMatcher>,
    pub ignore_export_rules: Vec<IgnoreExportRule>,
    pub compiled_ignore_exports: Vec<CompiledIgnoreExportRule>,
    pub compiled_ignore_catalog_references: Vec<CompiledIgnoreCatalogReferenceRule>,
    pub compiled_ignore_dependency_overrides: Vec<CompiledIgnoreDependencyOverrideRule>,
    pub ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig,
    pub used_class_members: Vec<UsedClassMemberRule>,
    pub ignore_decorators: Vec<String>,
    /// Compiled regex matched against each declared component prop's local
    /// destructure binding name; a matching prop is exempted from
    /// `unused-component-props`. `None` when `unusedComponentProps.ignorePattern`
    /// is unset. Compiled from the validated raw pattern in [`Self::resolve`].
    pub unused_component_props_ignore: Option<regex::Regex>,
    pub duplicates: DuplicatesConfig,
    pub health: HealthConfig,
    pub rules: RulesConfig,
    pub boundaries: ResolvedBoundaryConfig,
    /// Rule packs loaded from the `rulePacks` config key, in config order.
    /// Validated at config load (`load_rule_packs` is also the validation
    /// gate in the CLI and programmatic entry points); a pack that fails to
    /// load here is skipped with a `tracing::error!` as defense in depth.
    pub rule_packs: Vec<crate::rule_pack::RulePackDef>,
    pub production: bool,
    pub quiet: bool,
    pub external_plugins: Vec<ExternalPluginDef>,
    pub dynamically_loaded: Vec<String>,
    pub overrides: Vec<ResolvedOverride>,
    pub regression: Option<super::RegressionConfig>,
    pub audit: super::AuditConfig,
    pub codeowners: Option<String>,
    pub public_packages: Vec<String>,
    pub flags: FlagsConfig,
    pub security: SecurityConfig,
    pub fix: super::FixConfig,
    pub resolve: ResolveConfig,
    pub include_entry_exports: bool,
    pub auto_imports: bool,
    /// Source files strictly larger than this many bytes are skipped at
    /// discovery (never read, parsed, or analyzed), guarding against the
    /// out-of-memory blowup a single multi-MB generated/vendored/bundled file
    /// causes (issue #1086). `None` means no limit. Declaration files
    /// (`.d.ts`/`.d.mts`/`.d.cts`) are exempt regardless of size because they
    /// are reachability roots for global types. Defaults to
    /// [`DEFAULT_MAX_FILE_SIZE_MB`] MB; the CLI overrides it post-resolve from
    /// `--max-file-size` / `FALLOW_MAX_FILE_SIZE` (`0` = unlimited).
    pub max_file_size_bytes: Option<u64>,
}

/// Default per-file size ceiling (in megabytes) for source discovery. A value
/// chosen so hand-written source effectively never reaches it while generated
/// API clients, vendored bundles, and minified blobs do. See issue #1086.
pub const DEFAULT_MAX_FILE_SIZE_MB: u32 = 5;

/// [`DEFAULT_MAX_FILE_SIZE_MB`] expressed in bytes.
pub const DEFAULT_MAX_FILE_SIZE_BYTES: u64 = DEFAULT_MAX_FILE_SIZE_MB as u64 * 1024 * 1024;

/// Convert a user-supplied megabyte ceiling into the byte limit stored on
/// [`ResolvedConfig::max_file_size_bytes`]. `Some(0)` means "no limit"
/// (`None`); any other `Some(n)` is `n` MB in bytes; `None` (unset) keeps the
/// built-in [`DEFAULT_MAX_FILE_SIZE_BYTES`].
#[must_use]
pub fn resolve_max_file_size_bytes(max_file_size_mb: Option<u32>) -> Option<u64> {
    match max_file_size_mb {
        None => Some(DEFAULT_MAX_FILE_SIZE_BYTES),
        Some(0) => None,
        Some(mb) => Some(u64::from(mb) * 1024 * 1024),
    }
}

/// Compute the cache-invalidation hash over extraction-affecting config fields.
fn compute_cache_config_hash(external_plugins: &[ExternalPluginDef]) -> u64 {
    let mut names: Vec<&str> = external_plugins.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    for name in names {
        hasher.update(&(name.len() as u32).to_le_bytes());
        hasher.update(name.as_bytes());
    }
    hasher.digest()
}

fn resolve_cache_dir(root: &Path, configured: Option<PathBuf>) -> PathBuf {
    let Some(dir) = configured else {
        return root.join(".fallow");
    };
    if dir.is_absolute() {
        dir
    } else {
        root.join(dir)
    }
}

fn normalize_user_glob_pattern(pattern: &str) -> &str {
    pattern.strip_prefix("./").unwrap_or(pattern)
}

#[expect(
    clippy::expect_used,
    reason = "user glob patterns are validated before config resolution"
)]
fn compile_ignore_patterns(ignore_patterns: &[String]) -> GlobSet {
    let mut ignore_builder = GlobSetBuilder::new();
    for pattern in ignore_patterns {
        let normalized = normalize_user_glob_pattern(pattern);
        ignore_builder.add(
            Glob::new(normalized).expect("ignorePatterns entry was validated at config load time"),
        );
    }

    let default_ignores = [
        "**/node_modules/**",
        "**/dist/**",
        "build/**",
        "**/.git/**",
        "**/coverage/**",
        "**/*.min.js",
        "**/*.min.mjs",
        "**/*.min.cjs",
        "**/*.bundle.js",
    ];
    for pattern in &default_ignores {
        ignore_builder.add(Glob::new(pattern).expect("default ignore pattern is valid"));
    }

    ignore_builder.build().unwrap_or_default()
}

#[expect(
    clippy::expect_used,
    reason = "user glob patterns are validated before config resolution"
)]
fn compile_ignore_unresolved_imports(patterns: &[String]) -> Vec<GlobMatcher> {
    patterns
        .iter()
        .map(|pattern| {
            let normalized = normalize_user_glob_pattern(pattern);
            Glob::new(normalized)
                .expect("ignoreUnresolvedImports entry was validated at config load time")
                .compile_matcher()
        })
        .collect()
}

fn resolve_rules_for_production(mut rules: RulesConfig, production: bool) -> RulesConfig {
    if production {
        rules.unused_dev_dependencies = Severity::Off;
        rules.unused_optional_dependencies = Severity::Off;
    }
    rules
}

fn resolve_boundaries(
    mut boundaries: super::boundaries::BoundaryConfig,
    root: &Path,
) -> ResolvedBoundaryConfig {
    if boundaries.preset.is_some() {
        let source_root = crate::workspace::parse_tsconfig_root_dir(root)
            .filter(|r| r != "." && !r.starts_with("..") && !std::path::Path::new(r).is_absolute())
            .unwrap_or_else(|| "src".to_owned());
        if source_root != "src" {
            tracing::info!("boundary preset: using rootDir '{source_root}' from tsconfig.json");
        }
        boundaries.expand(&source_root);
    }
    let logical_groups = boundaries.expand_auto_discover(root);
    let mut resolved = boundaries.resolve();
    resolved.logical_groups = logical_groups;
    resolved
}

fn warn_inter_file_overrides(rules: &PartialRulesConfig, files: &[String]) {
    if rules.duplicate_exports.is_some() && record_inter_file_warn_seen("duplicate-exports", files)
    {
        let files = files.join(", ");
        tracing::warn!(
            "overrides.rules.duplicate-exports has no effect for files matching [{files}]: duplicate-exports is an inter-file rule. Use top-level `ignoreExports` to exclude these files from duplicate-export grouping."
        );
    }
    if rules.circular_dependencies.is_some()
        && record_inter_file_warn_seen("circular-dependency", files)
    {
        let files = files.join(", ");
        tracing::warn!(
            "overrides.rules.circular-dependency has no effect for files matching [{files}]: circular-dependency is an inter-file rule. Use a file-level `// fallow-ignore-file circular-dependency` comment in one participating file instead."
        );
    }
    if rules.re_export_cycle.is_some() && record_inter_file_warn_seen("re-export-cycle", files) {
        let files = files.join(", ");
        tracing::warn!(
            "overrides.rules.re-export-cycle has no effect for files matching [{files}]: re-export-cycle is an inter-file rule (the cycle spans multiple barrels). Use a file-level `// fallow-ignore-file re-export-cycle` comment in one participating file instead, or set `rules.re-export-cycle: off` at the top level."
        );
    }
}

#[expect(
    clippy::expect_used,
    reason = "override glob patterns are validated before config resolution"
)]
fn compile_overrides(overrides: Vec<ConfigOverride>) -> Vec<ResolvedOverride> {
    overrides
        .into_iter()
        .filter_map(|override_entry| {
            warn_inter_file_overrides(&override_entry.rules, &override_entry.files);
            let matchers: Vec<globset::GlobMatcher> = override_entry
                .files
                .iter()
                .map(|pattern| {
                    Glob::new(pattern)
                        .expect("overrides[].files pattern was validated at config load time")
                        .compile_matcher()
                })
                .collect();
            if matchers.is_empty() {
                None
            } else {
                Some(ResolvedOverride {
                    matchers,
                    rules: override_entry.rules,
                })
            }
        })
        .collect()
}

/// Compile `ignoreExports` file globs into matchers paired with export names.
#[expect(
    clippy::expect_used,
    reason = "user glob patterns are validated before config resolution"
)]
fn compile_ignore_export_rules(rules: &[IgnoreExportRule]) -> Vec<CompiledIgnoreExportRule> {
    rules
        .iter()
        .map(|rule| CompiledIgnoreExportRule {
            matcher: Glob::new(&rule.file)
                .expect("ignoreExports[].file was validated at config load time")
                .compile_matcher(),
            exports: rule.exports.clone(),
        })
        .collect()
}

/// Compile `ignoreCatalogReferences` rules, pre-compiling the consumer glob.
#[expect(
    clippy::expect_used,
    reason = "user glob patterns are validated before config resolution"
)]
fn compile_ignore_catalog_reference_rules(
    rules: &[IgnoreCatalogReferenceRule],
) -> Vec<CompiledIgnoreCatalogReferenceRule> {
    rules
        .iter()
        .map(|rule| CompiledIgnoreCatalogReferenceRule {
            package: rule.package.clone(),
            catalog: rule.catalog.clone(),
            consumer_matcher: rule.consumer.as_ref().map(|pattern| {
                Glob::new(pattern)
                    .expect("ignoreCatalogReferences[].consumer was validated at config load time")
                    .compile_matcher()
            }),
        })
        .collect()
}

/// Convert `ignoreDependencyOverrides` rules into their match-ready form.
fn compile_ignore_dependency_override_rules(
    rules: &[IgnoreDependencyOverrideRule],
) -> Vec<CompiledIgnoreDependencyOverrideRule> {
    rules
        .iter()
        .map(|rule| CompiledIgnoreDependencyOverrideRule {
            package: rule.package.clone(),
            source: rule.source.clone(),
        })
        .collect()
}

struct CompiledIgnoreSettings {
    patterns: GlobSet,
    unresolved_imports: Vec<GlobMatcher>,
    exports: Vec<CompiledIgnoreExportRule>,
    catalog_references: Vec<CompiledIgnoreCatalogReferenceRule>,
    dependency_overrides: Vec<CompiledIgnoreDependencyOverrideRule>,
}

fn compile_ignore_settings(config: &FallowConfig) -> CompiledIgnoreSettings {
    CompiledIgnoreSettings {
        patterns: compile_ignore_patterns(&config.ignore_patterns),
        unresolved_imports: compile_ignore_unresolved_imports(&config.ignore_unresolved_imports),
        exports: compile_ignore_export_rules(&config.ignore_exports),
        catalog_references: compile_ignore_catalog_reference_rules(
            &config.ignore_catalog_references,
        ),
        dependency_overrides: compile_ignore_dependency_override_rules(
            &config.ignore_dependency_overrides,
        ),
    }
}

struct ResolvedPluginSettings {
    external_plugins: Vec<ExternalPluginDef>,
    rule_packs: Vec<crate::rule_pack::RulePackDef>,
}

fn resolve_plugin_settings(
    root: &Path,
    configured_plugins: &[String],
    framework: Vec<ExternalPluginDef>,
    rule_packs: &[String],
) -> ResolvedPluginSettings {
    let mut external_plugins = discover_external_plugins(root, configured_plugins);
    external_plugins.extend(framework);

    let rule_packs = crate::rule_pack::load_rule_packs(root, rule_packs).unwrap_or_else(|errors| {
        for error in &errors {
            tracing::error!("invalid rule pack: {error}");
        }
        Vec::new()
    });

    ResolvedPluginSettings {
        external_plugins,
        rule_packs,
    }
}

struct ResolvedCacheSettings {
    dir: PathBuf,
    max_size_mb: Option<u32>,
    config_hash: u64,
}

struct ResolvedProductionRules {
    production: bool,
    rules: RulesConfig,
}

fn resolve_production_rules(
    production_config: ProductionConfig,
    rules: RulesConfig,
) -> ResolvedProductionRules {
    let production = production_config.global();
    ResolvedProductionRules {
        production,
        rules: resolve_rules_for_production(rules, production),
    }
}

fn resolve_cache_settings(
    root: &Path,
    configured_dir: Option<PathBuf>,
    configured_max_size_mb: Option<u32>,
    override_max_size_mb: Option<u32>,
    no_cache: bool,
    external_plugins: &[ExternalPluginDef],
) -> ResolvedCacheSettings {
    ResolvedCacheSettings {
        dir: resolve_cache_dir(root, configured_dir),
        max_size_mb: override_max_size_mb.or(configured_max_size_mb),
        config_hash: if no_cache {
            0
        } else {
            compute_cache_config_hash(external_plugins)
        },
    }
}

fn normalize_security_config(security: SecurityConfig) -> SecurityConfig {
    SecurityConfig {
        request_receivers: security.normalized_request_receivers(),
        ..security
    }
}

struct ResolvedPathPolicySettings {
    boundaries: ResolvedBoundaryConfig,
    overrides: Vec<ResolvedOverride>,
}

fn resolve_path_policy_settings(
    boundaries: BoundaryConfig,
    overrides: Vec<ConfigOverride>,
    root: &Path,
) -> ResolvedPathPolicySettings {
    ResolvedPathPolicySettings {
        boundaries: resolve_boundaries(boundaries, root),
        overrides: compile_overrides(overrides),
    }
}

impl FallowConfig {
    /// Resolve into a fully resolved config with compiled globs.
    #[expect(
        clippy::too_many_arguments,
        reason = "public cross-crate API: ResolvedConfig builder whose runtime-override parameters (root, output, threads, no_cache, quiet, cache_max_size_mb) are an established stable signature; bundling them would break callers"
    )]
    pub fn resolve(
        self,
        root: PathBuf,
        output: OutputFormat,
        threads: usize,
        no_cache: bool,
        quiet: bool,
        cache_max_size_mb: Option<u32>,
    ) -> ResolvedConfig {
        let compiled_ignores = compile_ignore_settings(&self);

        let production_rules = resolve_production_rules(self.production, self.rules);

        let plugins =
            resolve_plugin_settings(&root, &self.plugins, self.framework, &self.rule_packs);

        let cache = resolve_cache_settings(
            &root,
            self.cache.dir,
            self.cache.max_size_mb,
            cache_max_size_mb,
            no_cache,
            &plugins.external_plugins,
        );

        let path_policy = resolve_path_policy_settings(self.boundaries, self.overrides, &root);

        // Compile the validated pattern defensively. `FallowConfig::load`
        // already proved it compiles, so the error arm is unreachable on the
        // load path; programmatic / napi / LSP callers that construct a config
        // without going through `load` fail open to "no exemption" (default
        // behavior) rather than panicking.
        let unused_component_props_ignore = self
            .unused_component_props
            .ignore_pattern
            .as_deref()
            .and_then(|pattern| match regex::Regex::new(pattern) {
                Ok(re) => Some(re),
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "ignoring invalid unusedComponentProps.ignorePattern; this config was \
                         not validated through FallowConfig::load"
                    );
                    None
                }
            });

        ResolvedConfig {
            root,
            entry_patterns: self.entry,
            ignore_patterns: compiled_ignores.patterns,
            output,
            cache_dir: cache.dir,
            threads,
            no_cache,
            cache_max_size_mb: cache.max_size_mb,
            cache_config_hash: cache.config_hash,
            ignore_dependencies: self.ignore_dependencies,
            ignore_unresolved_imports: compiled_ignores.unresolved_imports,
            ignore_export_rules: self.ignore_exports,
            compiled_ignore_exports: compiled_ignores.exports,
            compiled_ignore_catalog_references: compiled_ignores.catalog_references,
            compiled_ignore_dependency_overrides: compiled_ignores.dependency_overrides,
            ignore_exports_used_in_file: self.ignore_exports_used_in_file,
            used_class_members: self.used_class_members,
            ignore_decorators: self.ignore_decorators,
            unused_component_props_ignore,
            duplicates: self.duplicates,
            health: self.health,
            rules: production_rules.rules,
            boundaries: path_policy.boundaries,
            rule_packs: plugins.rule_packs,
            production: production_rules.production,
            quiet,
            external_plugins: plugins.external_plugins,
            dynamically_loaded: self.dynamically_loaded,
            overrides: path_policy.overrides,
            regression: self.regression,
            audit: self.audit,
            codeowners: self.codeowners,
            public_packages: self.public_packages,
            flags: self.flags,
            security: normalize_security_config(self.security),
            fix: self.fix,
            resolve: self.resolve,
            include_entry_exports: self.include_entry_exports,
            auto_imports: self.auto_imports,
            max_file_size_bytes: Some(DEFAULT_MAX_FILE_SIZE_BYTES),
        }
    }
}

impl ResolvedConfig {
    /// Resolve the effective rules for a given file path.
    /// Starts with base rules and applies matching overrides in order.
    #[must_use]
    pub fn resolve_rules_for_path(&self, path: &Path) -> RulesConfig {
        if self.overrides.is_empty() {
            return self.rules.clone();
        }

        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        let relative_str = relative.to_string_lossy();

        let mut rules = self.rules.clone();
        for override_entry in &self.overrides {
            let matches = override_entry
                .matchers
                .iter()
                .any(|m| m.is_match(relative_str.as_ref()));
            if matches {
                rules.apply_partial(&override_entry.rules);
            }
        }
        rules
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CacheConfig;
    use crate::config::boundaries::BoundaryConfig;
    use crate::config::health::HealthConfig;

    #[test]
    fn overrides_deserialize() {
        let json_str = r#"{
            "overrides": [{
                "files": ["*.test.ts"],
                "rules": {
                    "unused-exports": "off"
                }
            }]
        }"#;
        let config: FallowConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.overrides.len(), 1);
        assert_eq!(config.overrides[0].files, vec!["*.test.ts"]);
        assert_eq!(
            config.overrides[0].rules.unused_exports,
            Some(Severity::Off)
        );
        assert_eq!(config.overrides[0].rules.unused_files, None);
    }

    #[test]
    fn resolve_rules_for_path_no_overrides() {
        let config = FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            unused_component_props: crate::UnusedComponentPropsConfig::default(),
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            overrides: vec![],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            security: SecurityConfig::default(),
            fix: crate::config::FixConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: CacheConfig::default(),
        };
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        let rules = resolved.resolve_rules_for_path(Path::new("/project/src/foo.ts"));
        assert_eq!(rules.unused_files, Severity::Error);
    }

    #[test]
    fn resolve_rules_for_path_with_matching_override() {
        let config = FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            unused_component_props: crate::UnusedComponentPropsConfig::default(),
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            overrides: vec![ConfigOverride {
                files: vec!["*.test.ts".to_string()],
                rules: PartialRulesConfig {
                    unused_exports: Some(Severity::Off),
                    ..Default::default()
                },
            }],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            security: SecurityConfig::default(),
            fix: crate::config::FixConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: CacheConfig::default(),
        };
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );

        let test_rules = resolved.resolve_rules_for_path(Path::new("/project/src/utils.test.ts"));
        assert_eq!(test_rules.unused_exports, Severity::Off);
        assert_eq!(test_rules.unused_files, Severity::Error); // not overridden

        let src_rules = resolved.resolve_rules_for_path(Path::new("/project/src/utils.ts"));
        assert_eq!(src_rules.unused_exports, Severity::Error);
    }

    #[test]
    fn resolve_rules_for_path_later_override_wins() {
        let config = FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            unused_component_props: crate::UnusedComponentPropsConfig::default(),
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            overrides: vec![
                ConfigOverride {
                    files: vec!["*.ts".to_string()],
                    rules: PartialRulesConfig {
                        unused_files: Some(Severity::Warn),
                        ..Default::default()
                    },
                },
                ConfigOverride {
                    files: vec!["*.test.ts".to_string()],
                    rules: PartialRulesConfig {
                        unused_files: Some(Severity::Off),
                        ..Default::default()
                    },
                },
            ],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            security: SecurityConfig::default(),
            fix: crate::config::FixConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: CacheConfig::default(),
        };
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );

        let rules = resolved.resolve_rules_for_path(Path::new("/project/foo.test.ts"));
        assert_eq!(rules.unused_files, Severity::Off);

        let rules2 = resolved.resolve_rules_for_path(Path::new("/project/foo.ts"));
        assert_eq!(rules2.unused_files, Severity::Warn);
    }

    #[test]
    fn resolve_keeps_inter_file_rule_override_after_warning() {
        let config = FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            unused_component_props: crate::UnusedComponentPropsConfig::default(),
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            overrides: vec![ConfigOverride {
                files: vec!["**/ui/**".to_string()],
                rules: PartialRulesConfig {
                    duplicate_exports: Some(Severity::Off),
                    unused_files: Some(Severity::Warn),
                    ..Default::default()
                },
            }],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            security: SecurityConfig::default(),
            fix: crate::config::FixConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: CacheConfig::default(),
        };
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(
            resolved.overrides.len(),
            1,
            "inter-file rule warning must not drop the override; co-located non-inter-file rules still apply"
        );
        let rules = resolved.resolve_rules_for_path(Path::new("/project/ui/dialog.ts"));
        assert_eq!(rules.unused_files, Severity::Warn);
    }

    #[test]
    fn inter_file_warn_dedup_returns_true_only_on_first_key_match() {
        reset_inter_file_warn_dedup_for_test();
        let files_a = vec!["__test_dedup_a/*".to_string()];
        let files_b = vec!["__test_dedup_b/*".to_string()];

        assert!(record_inter_file_warn_seen("duplicate-exports", &files_a));
        assert!(!record_inter_file_warn_seen("duplicate-exports", &files_a));
        assert!(!record_inter_file_warn_seen("duplicate-exports", &files_a));

        assert!(record_inter_file_warn_seen("circular-dependency", &files_a));
        assert!(!record_inter_file_warn_seen(
            "circular-dependency",
            &files_a
        ));

        assert!(record_inter_file_warn_seen("duplicate-exports", &files_b));

        let files_reordered = vec![
            "__test_dedup_b/*".to_string(),
            "__test_dedup_a/*".to_string(),
        ];
        let files_natural = vec![
            "__test_dedup_a/*".to_string(),
            "__test_dedup_b/*".to_string(),
        ];
        reset_inter_file_warn_dedup_for_test();
        assert!(record_inter_file_warn_seen(
            "duplicate-exports",
            &files_natural
        ));
        assert!(!record_inter_file_warn_seen(
            "duplicate-exports",
            &files_reordered
        ));
    }

    #[test]
    fn resolve_called_n_times_dedupes_inter_file_warning_to_one() {
        reset_inter_file_warn_dedup_for_test();
        let files = vec!["__test_resolve_dedup/**".to_string()];
        let build_config = || FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            unused_component_props: crate::UnusedComponentPropsConfig::default(),
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            overrides: vec![ConfigOverride {
                files: files.clone(),
                rules: PartialRulesConfig {
                    duplicate_exports: Some(Severity::Off),
                    ..Default::default()
                },
            }],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            security: SecurityConfig::default(),
            fix: crate::config::FixConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: CacheConfig::default(),
        };
        for _ in 0..10 {
            let _ = build_config().resolve(
                PathBuf::from("/project"),
                OutputFormat::Human,
                1,
                true,
                true,
                None,
            );
        }
        assert!(
            !record_inter_file_warn_seen("duplicate-exports", &files),
            "warn key for duplicate-exports + __test_resolve_dedup/** should be marked after the first resolve"
        );
    }

    /// Helper to build a FallowConfig with minimal boilerplate.
    fn make_config(production: bool) -> FallowConfig {
        FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            unused_component_props: crate::UnusedComponentPropsConfig::default(),
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: production.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            overrides: vec![],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            security: SecurityConfig::default(),
            fix: crate::config::FixConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: CacheConfig::default(),
        }
    }

    #[test]
    fn resolve_production_forces_dev_deps_off() {
        let resolved = make_config(true).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(
            resolved.rules.unused_dev_dependencies,
            Severity::Off,
            "production mode should force unused_dev_dependencies to off"
        );
    }

    #[test]
    fn resolve_production_forces_optional_deps_off() {
        let resolved = make_config(true).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(
            resolved.rules.unused_optional_dependencies,
            Severity::Off,
            "production mode should force unused_optional_dependencies to off"
        );
    }

    #[test]
    fn resolve_production_preserves_other_rules() {
        let resolved = make_config(true).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(resolved.rules.unused_files, Severity::Error);
        assert_eq!(resolved.rules.unused_exports, Severity::Error);
        assert_eq!(resolved.rules.unused_dependencies, Severity::Error);
    }

    #[test]
    fn resolve_non_production_keeps_dev_deps_default() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(
            resolved.rules.unused_dev_dependencies,
            Severity::Warn,
            "non-production should keep default severity"
        );
        assert_eq!(resolved.rules.unused_optional_dependencies, Severity::Warn);
    }

    #[test]
    fn resolve_production_flag_stored() {
        let resolved = make_config(true).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(resolved.production);

        let resolved2 = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(!resolved2.production);
    }

    #[test]
    fn resolve_default_ignores_node_modules() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(
            resolved
                .ignore_patterns
                .is_match("node_modules/lodash/index.js")
        );
        assert!(
            resolved
                .ignore_patterns
                .is_match("packages/a/node_modules/react/index.js")
        );
    }

    #[test]
    fn resolve_default_ignores_dist() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(resolved.ignore_patterns.is_match("dist/bundle.js"));
        assert!(
            resolved
                .ignore_patterns
                .is_match("packages/ui/dist/index.js")
        );
    }

    #[test]
    fn resolve_default_ignores_root_build_only() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(
            resolved.ignore_patterns.is_match("build/output.js"),
            "root build/ should be ignored"
        );
        assert!(
            !resolved.ignore_patterns.is_match("src/build/helper.ts"),
            "nested build/ should NOT be ignored by default"
        );
    }

    #[test]
    fn resolve_default_ignores_minified_files() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(resolved.ignore_patterns.is_match("vendor/jquery.min.js"));
        assert!(resolved.ignore_patterns.is_match("lib/utils.min.mjs"));
        assert!(resolved.ignore_patterns.is_match("lib/legacy.min.cjs"));
        assert!(resolved.ignore_patterns.is_match("public/app.bundle.js"));
        assert!(
            resolved
                .ignore_patterns
                .is_match("src/vendor/app.bundle.js")
        );
        // Hand-written source with a similar name stays analyzed.
        assert!(!resolved.ignore_patterns.is_match("src/bundle.ts"));
        assert!(!resolved.ignore_patterns.is_match("src/app.cjs"));
    }

    #[test]
    fn resolve_max_file_size_bytes_default_and_unlimited() {
        // Unset keeps the built-in default.
        assert_eq!(
            resolve_max_file_size_bytes(None),
            Some(DEFAULT_MAX_FILE_SIZE_BYTES)
        );
        // `0` means no limit.
        assert_eq!(resolve_max_file_size_bytes(Some(0)), None);
        // Any other value is that many megabytes in bytes.
        assert_eq!(resolve_max_file_size_bytes(Some(2)), Some(2 * 1024 * 1024));
        assert_eq!(DEFAULT_MAX_FILE_SIZE_MB, 5);
    }

    #[test]
    fn resolve_sets_default_max_file_size() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(
            resolved.max_file_size_bytes,
            Some(DEFAULT_MAX_FILE_SIZE_BYTES)
        );
    }

    #[test]
    fn resolve_default_ignores_git() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(resolved.ignore_patterns.is_match(".git/objects/ab/123.js"));
    }

    #[test]
    fn resolve_default_ignores_coverage() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(
            resolved
                .ignore_patterns
                .is_match("coverage/lcov-report/index.js")
        );
    }

    #[test]
    fn resolve_source_files_not_ignored_by_default() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(!resolved.ignore_patterns.is_match("src/index.ts"));
        assert!(
            !resolved
                .ignore_patterns
                .is_match("src/components/Button.tsx")
        );
        assert!(!resolved.ignore_patterns.is_match("lib/utils.js"));
    }

    #[test]
    fn resolve_custom_ignore_patterns_merged_with_defaults() {
        let mut config = make_config(false);
        config.ignore_patterns = vec!["**/__generated__/**".to_string()];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(
            resolved
                .ignore_patterns
                .is_match("src/__generated__/types.ts")
        );
        assert!(resolved.ignore_patterns.is_match("node_modules/foo/bar.js"));
    }

    #[test]
    fn resolve_normalizes_leading_dot_ignore_patterns() {
        let mut config = make_config(false);
        config.ignore_patterns = vec!["./src/generated/**".to_string()];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );

        assert!(resolved.ignore_patterns.is_match("src/generated/client.ts"));
        assert!(
            !resolved
                .ignore_patterns
                .is_match("./src/generated/client.ts")
        );
    }

    #[test]
    fn resolve_normalizes_leading_dot_ignore_unresolved_imports() {
        let mut config = make_config(false);
        config.ignore_unresolved_imports = vec!["./src/generated/**".to_string()];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );

        assert!(
            resolved
                .ignore_unresolved_imports
                .iter()
                .any(|matcher| matcher.is_match("src/generated/client"))
        );
        assert!(
            !resolved
                .ignore_unresolved_imports
                .iter()
                .any(|matcher| matcher.is_match("./src/generated/client"))
        );
    }

    #[test]
    fn resolve_passes_through_entry_patterns() {
        let mut config = make_config(false);
        config.entry = vec!["src/**/*.ts".to_string(), "lib/**/*.js".to_string()];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(resolved.entry_patterns, vec!["src/**/*.ts", "lib/**/*.js"]);
    }

    #[test]
    fn resolve_passes_through_ignore_dependencies() {
        let mut config = make_config(false);
        config.ignore_dependencies = vec!["postcss".to_string(), "autoprefixer".to_string()];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(
            resolved.ignore_dependencies,
            vec!["postcss", "autoprefixer"]
        );
    }

    #[test]
    fn resolve_compiles_ignore_unresolved_imports_as_raw_specifier_globs() {
        let mut config = make_config(false);
        config.ignore_unresolved_imports = vec![
            "@example/icons".to_string(),
            "@example/icons/**".to_string(),
            "../generated/**".to_string(),
        ];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );

        assert!(
            resolved
                .ignore_unresolved_imports
                .iter()
                .any(|matcher| matcher.is_match("@example/icons"))
        );
        assert!(
            resolved
                .ignore_unresolved_imports
                .iter()
                .any(|matcher| matcher.is_match("@example/icons/metadata"))
        );
        assert!(
            resolved
                .ignore_unresolved_imports
                .iter()
                .any(|matcher| matcher.is_match("../generated/client"))
        );
    }

    #[test]
    fn ignore_unresolved_imports_subpath_glob_does_not_match_bare_specifier() {
        let mut config = make_config(false);
        config.ignore_unresolved_imports = vec!["@example/icons/**".to_string()];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );

        assert!(
            !resolved.ignore_unresolved_imports[0].is_match("@example/icons"),
            "globset treats @example/icons/** as subpaths only; list the bare specifier separately"
        );
        assert!(resolved.ignore_unresolved_imports[0].is_match("@example/icons/metadata"));
    }

    #[test]
    fn resolve_sets_cache_dir() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/my/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(resolved.cache_dir, PathBuf::from("/my/project/.fallow"));
    }

    #[test]
    fn resolve_uses_relative_configured_cache_dir_from_root() {
        let config = FallowConfig {
            cache: crate::CacheConfig {
                dir: Some(PathBuf::from(".cache/fallow")),
                ..Default::default()
            },
            ..make_config(false)
        };
        let resolved = config.resolve(
            PathBuf::from("/my/project"),
            OutputFormat::Human,
            1,
            false,
            true,
            None,
        );
        assert_eq!(
            resolved.cache_dir,
            PathBuf::from("/my/project/.cache/fallow")
        );
    }

    #[test]
    fn resolve_keeps_absolute_configured_cache_dir() {
        let config = FallowConfig {
            cache: crate::CacheConfig {
                dir: Some(PathBuf::from("/tmp/fallow-cache")),
                ..Default::default()
            },
            ..make_config(false)
        };
        let resolved = config.resolve(
            PathBuf::from("/my/project"),
            OutputFormat::Human,
            1,
            false,
            true,
            None,
        );
        assert_eq!(resolved.cache_dir, PathBuf::from("/tmp/fallow-cache"));
    }

    #[test]
    fn resolve_passes_through_thread_count() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            8,
            true,
            true,
            None,
        );
        assert_eq!(resolved.threads, 8);
    }

    #[test]
    fn resolve_passes_through_quiet_flag() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            false,
            None,
        );
        assert!(!resolved.quiet);

        let resolved2 = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(resolved2.quiet);
    }

    #[test]
    fn resolve_passes_through_no_cache_flag() {
        let resolved_no_cache = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(resolved_no_cache.no_cache);

        let resolved_with_cache = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            false,
            true,
            None,
        );
        assert!(!resolved_with_cache.no_cache);
    }

    #[test]
    #[should_panic(expected = "validated at config load time")]
    fn resolve_panics_on_unvalidated_invalid_override_glob() {
        let mut config = make_config(false);
        config.overrides = vec![ConfigOverride {
            files: vec!["[invalid".to_string()],
            rules: PartialRulesConfig {
                unused_files: Some(Severity::Off),
                ..Default::default()
            },
        }];
        let _ = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
    }

    #[test]
    fn resolve_override_with_empty_files_skipped() {
        let mut config = make_config(false);
        config.overrides = vec![ConfigOverride {
            files: vec![],
            rules: PartialRulesConfig {
                unused_files: Some(Severity::Off),
                ..Default::default()
            },
        }];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(
            resolved.overrides.is_empty(),
            "override with no file patterns should be skipped"
        );
    }

    #[test]
    fn resolve_multiple_valid_overrides() {
        let mut config = make_config(false);
        config.overrides = vec![
            ConfigOverride {
                files: vec!["*.test.ts".to_string()],
                rules: PartialRulesConfig {
                    unused_exports: Some(Severity::Off),
                    ..Default::default()
                },
            },
            ConfigOverride {
                files: vec!["*.stories.tsx".to_string()],
                rules: PartialRulesConfig {
                    unused_files: Some(Severity::Off),
                    ..Default::default()
                },
            },
        ];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(resolved.overrides.len(), 2);
    }

    #[test]
    fn ignore_export_rule_deserialize() {
        let json = r#"{"file": "src/types/*.ts", "exports": ["*"]}"#;
        let rule: IgnoreExportRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.file, "src/types/*.ts");
        assert_eq!(rule.exports, vec!["*"]);
    }

    #[test]
    fn ignore_export_rule_specific_exports() {
        let json = r#"{"file": "src/constants.ts", "exports": ["FOO", "BAR", "BAZ"]}"#;
        let rule: IgnoreExportRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.exports.len(), 3);
        assert!(rule.exports.contains(&"FOO".to_string()));
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_resolved_config(production: bool) -> ResolvedConfig {
            make_config(production).resolve(
                PathBuf::from("/project"),
                OutputFormat::Human,
                1,
                true,
                true,
                None,
            )
        }

        proptest! {
            /// Resolved config always has non-empty ignore patterns (defaults are always added).
            #[test]
            fn resolved_config_has_default_ignores(production in any::<bool>()) {
                let resolved = arb_resolved_config(production);
                prop_assert!(
                    resolved.ignore_patterns.is_match("node_modules/foo/bar.js"),
                    "Default ignore should match node_modules"
                );
                prop_assert!(
                    resolved.ignore_patterns.is_match("dist/bundle.js"),
                    "Default ignore should match dist"
                );
            }

            /// Production mode always forces dev and optional deps to Off.
            #[test]
            fn production_forces_dev_deps_off(_unused in Just(())) {
                let resolved = arb_resolved_config(true);
                prop_assert_eq!(
                    resolved.rules.unused_dev_dependencies,
                    Severity::Off,
                    "Production should force unused_dev_dependencies off"
                );
                prop_assert_eq!(
                    resolved.rules.unused_optional_dependencies,
                    Severity::Off,
                    "Production should force unused_optional_dependencies off"
                );
            }

            /// Non-production mode preserves default severity for dev deps.
            #[test]
            fn non_production_preserves_dev_deps_default(_unused in Just(())) {
                let resolved = arb_resolved_config(false);
                prop_assert_eq!(
                    resolved.rules.unused_dev_dependencies,
                    Severity::Warn,
                    "Non-production should keep default dev dep severity"
                );
            }

            /// Default cache dir is root/.fallow.
            #[test]
            fn cache_dir_defaults_to_root_fallow(dir_suffix in "[a-zA-Z0-9_]{1,20}") {
                let root = PathBuf::from(format!("/project/{dir_suffix}"));
                let expected_cache = root.join(".fallow");
                let resolved = make_config(false).resolve(
                    root,
                    OutputFormat::Human,
                    1,
                    true,
                    true,
                    None,
                );
                prop_assert_eq!(
                    resolved.cache_dir, expected_cache,
                    "Default cache dir should be root/.fallow"
                );
            }

            /// Thread count is always passed through exactly.
            #[test]
            fn threads_passed_through(threads in 1..64usize) {
                let resolved = make_config(false).resolve(
                    PathBuf::from("/project"),
                    OutputFormat::Human,
                    threads,
                    true,
                    true, None,
                );
                prop_assert_eq!(
                    resolved.threads, threads,
                    "Thread count should be passed through"
                );
            }

            /// Custom ignore patterns are merged with defaults, not replacing them.
            /// Uses a pattern regex that cannot match node_modules paths, so the
            /// assertion proves the default pattern is what provides the match.
            #[test]
            fn custom_ignores_dont_replace_defaults(pattern in "[a-z_]{1,10}/[a-z_]{1,10}") {
                let mut config = make_config(false);
                config.ignore_patterns = vec![pattern];
                let resolved = config.resolve(
                    PathBuf::from("/project"),
                    OutputFormat::Human,
                    1,
                    true,
                    true, None,
                );
                prop_assert!(
                    resolved.ignore_patterns.is_match("node_modules/foo/bar.js"),
                    "Default node_modules ignore should still be active"
                );
            }
        }
    }

    #[test]
    fn resolve_expands_boundary_preset() {
        use crate::config::boundaries::BoundaryPreset;

        let mut config = make_config(false);
        config.boundaries.preset = Some(BoundaryPreset::Hexagonal);
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(resolved.boundaries.zones.len(), 3);
        assert_eq!(resolved.boundaries.rules.len(), 3);
        assert_eq!(resolved.boundaries.zones[0].name, "adapters");
        assert_eq!(
            resolved.boundaries.classify_zone("src/adapters/http.ts"),
            Some("adapters")
        );
    }

    #[test]
    fn resolve_boundary_preset_with_user_override() {
        use crate::config::boundaries::{BoundaryPreset, BoundaryZone};

        let mut config = make_config(false);
        config.boundaries.preset = Some(BoundaryPreset::Hexagonal);
        config.boundaries.zones = vec![BoundaryZone {
            name: "domain".to_string(),
            patterns: vec!["src/core/**".to_string()],
            auto_discover: vec![],
            root: None,
        }];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert_eq!(resolved.boundaries.zones.len(), 3);
        assert_eq!(
            resolved.boundaries.classify_zone("src/core/user.ts"),
            Some("domain")
        );
        assert_eq!(
            resolved.boundaries.classify_zone("src/domain/user.ts"),
            None
        );
    }

    #[test]
    fn resolve_no_preset_unchanged() {
        let config = make_config(false);
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(resolved.boundaries.is_empty());
    }
}
