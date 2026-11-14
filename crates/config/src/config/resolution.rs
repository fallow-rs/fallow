use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::boundaries::ResolvedBoundaryConfig;
use super::duplicates_config::DuplicatesConfig;
use super::entry_effect::EntryEffectRecord;
use super::flags::FlagsConfig;
use super::format::OutputFormat;
use super::glob_validation::compile_path_glob;
use super::health::HealthConfig;
use super::ignore_patterns::compile_ignore_patterns;
use super::mutation::MutationConfig;
use super::resolve::ResolveConfig;
use super::rules::{PartialRulesConfig, RulesConfig, Severity};
use super::similar_code::SimilarCodeConfig;
use super::used_class_members::UsedClassMemberRule;
use crate::external_plugin::{ExternalPluginDef, resolve_external_plugins};

use super::{
    BoundaryConfig, FallowConfig, FindingIgnoreMatcher, IgnoreDependencyMatcher,
    IgnoreExportsUsedInFileConfig, IgnorePatternSet, ProductionConfig, SecurityConfig,
    TypeAwareConfig,
};

/// Rule for ignoring specific exports.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IgnoreExportRule {
    /// Glob pattern for files.
    /// A single `*` stays within one path segment; use `**` to match across directories.
    pub file: String,
    /// Export names to ignore (`*` for all).
    pub exports: Vec<String>,
}

/// `IgnoreExportRule` with the glob pre-compiled into a matcher.
#[derive(Debug, Clone)]
pub struct CompiledIgnoreExportRule {
    /// Pre-compiled matcher for the rule's `file` glob.
    pub matcher: globset::GlobMatcher,
    /// Export names to ignore (`*` for all), copied from the raw rule.
    pub exports: Vec<String>,
}

/// Rule for suppressing an `unresolved-catalog-reference` finding.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IgnoreCatalogReferenceRule {
    /// Required exact package name whose `unresolved-catalog-reference` finding this rule suppresses; compared by string equality against the referenced package, so one rule targets one package's catalog reference (further narrowed by the optional `catalog` and `consumer` filters, all of which must match).
    pub package: String,
    /// Optional catalog-name filter: when set, the rule suppresses only references to this exact catalog name (string equality), and when omitted it applies regardless of which catalog is referenced. Use it to scope suppression to one catalog (e.g. `"react18"`) while leaving other catalog references for the same package reportable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    /// Optional glob matched against the consuming workspace `package.json` path (compiled into a glob matcher at config load): when set, the rule suppresses the finding only for consumers whose path matches, and when omitted it applies to every consumer. Use it to suppress a catalog reference in one specific workspace during a staged migration. A single `*` stays within one path segment; use `**` to match across directories.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
}

/// `IgnoreCatalogReferenceRule` with the optional consumer glob pre-compiled.
#[derive(Debug, Clone)]
pub struct CompiledIgnoreCatalogReferenceRule {
    /// Exact package name the rule suppresses, compared by string equality.
    pub package: String,
    /// Optional exact catalog-name filter; `None` matches any catalog.
    pub catalog: Option<String>,
    /// Optional pre-compiled glob over the consuming workspace `package.json`
    /// path; `None` matches any consumer.
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
    /// Required exact package name whose `unused-dependency-override` or `misconfigured-dependency-override` finding this rule suppresses; compared by string equality against the override's target package, so one rule targets one override entry (further narrowable with the optional `source` filter).
    pub package: String,
    /// Optional source filter matched by exact string equality against the override's declaring-file label: set it to `"pnpm-workspace.yaml"` or `"package.json"` to scope the suppression to overrides declared in that file, or omit it to suppress the package's override regardless of where it is declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// `IgnoreDependencyOverrideRule` ready for matching.
#[derive(Debug, Clone)]
pub struct CompiledIgnoreDependencyOverrideRule {
    /// Exact target package name the rule suppresses.
    pub package: String,
    /// Optional declaring-file filter (`"pnpm-workspace.yaml"` or
    /// `"package.json"`); `None` matches either source.
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigOverride {
    /// Glob-pattern string array selecting which source files this override entry applies to (patterns are validated and compiled to matchers at config load). Set to scope the entry's rule severities to a subset of paths (e.g. `["src/generated/**", "**/*.test.ts"]`); when several override entries match one file, its severities come from every matching entry, applied in list order (later entries win on conflict). A single `*` stays within one path segment; use `**` to match across directories.
    pub files: Vec<String>,
    /// Partial per-rule severity map applied only to files matching this entry's `files` globs; each rule key takes `error`, `warn`, or `off`, and omitted rules keep their top-level severity. Set to change how specific rules (e.g. unused-exports, unused-files) behave for the scoped paths. Project-level rules ignore overrides: unused-dependencies, unused-dev-dependencies, unused-optional-dependencies, unlisted-dependencies, unlisted-binaries, type-only-dependencies, test-only-dependencies, dev-dependencies-in-production, duplicate-exports, re-export-cycle and unused-catalog-entries. Fallow reports each such entry as an `override-rule-ineffective` diagnostic that names the mechanism that works. For most other rules, an override cannot start a detector for a rule that is off at top level.
    #[serde(default)]
    pub rules: PartialRulesConfig,
}

/// Resolved override with pre-compiled glob matchers.
#[derive(Debug, Clone)]
pub struct ResolvedOverride {
    /// The `files` globs as written in the config, for diagnostics.
    pub files: Vec<String>,
    /// Pre-compiled matchers for the entry's `files` globs; the override
    /// applies to a file when any matcher matches.
    pub matchers: Vec<globset::GlobMatcher>,
    /// Partial severity map applied to matching files.
    pub rules: PartialRulesConfig,
}

/// Which revision an analysis pass describes.
///
/// `fallow audit --base <ref>` analyzes the base revision in an isolated
/// worktree in addition to the working tree. Diagnostics raised while the base
/// pass runs must say which revision they came from, otherwise a base-only
/// condition reads as a defect in the current configuration (issue #2013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnalysisSnapshot {
    /// The working tree, which is what every command analyzes by default.
    #[default]
    Current,
    /// The base revision analyzed by `audit --base`.
    Base,
}

impl AnalysisSnapshot {
    /// True when this pass analyzes the `audit --base` revision.
    #[must_use]
    pub const fn is_base(self) -> bool {
        matches!(self, Self::Base)
    }
}

/// Fully resolved configuration with all globs pre-compiled.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    /// Project root every analysis path resolves against.
    pub root: PathBuf,
    /// Manual entry-point globs from the `entry` config key, matched against
    /// discovered files on top of plugin- and manifest-derived entries.
    pub entry_patterns: Vec<String>,
    /// Support entry-point globs from the `supportEntry` config key. In
    /// production mode, a file that only these globs match takes no part in
    /// the analysis.
    pub support_entry_patterns: Vec<String>,
    /// Compiled union of user `ignorePatterns` and the built-in default
    /// ignores (`node_modules`, `dist`, minified bundles, ...); matching files
    /// are excluded from discovery entirely, unless a `!` entry in
    /// `ignorePatterns` lifts them (issue #2940).
    pub ignore_patterns: IgnorePatternSet,
    /// How many globs at the FRONT of [`Self::ignore_patterns`] came from the
    /// user's `ignorePatterns` (the `!` exceptions are not globs of this
    /// union and do not count). The rest, in order, are
    /// [`DEFAULT_IGNORE_PATTERNS`].
    ///
    /// Source discovery needs the split to answer "which pattern removed this
    /// file": a match index below this count is the project's own explicit
    /// choice and is reported nowhere, while an index at or above it names a
    /// built-in the user never asked for (issue #2638).
    pub user_ignore_pattern_count: usize,
    /// Which [`Self::entry_patterns`] matched a file that no other runtime
    /// entry source declares, in the latest entry point discovery (issue
    /// #3824).
    pub entry_pattern_effect: EntryEffectRecord,
    /// Post-analysis finding-path matcher built from `ignoreFindings`; hides
    /// findings without removing files from the module graph.
    pub ignore_findings: FindingIgnoreMatcher,
    /// Output format for this run, passed through from the CLI at resolve time.
    pub output: OutputFormat,
    /// Cache directory: `cache.dir` resolved against the root, or the default
    /// `<root>/.fallow`.
    pub cache_dir: PathBuf,
    /// Worker-thread count, passed through from the CLI at resolve time.
    pub threads: usize,
    /// When true, skip reading and writing the persistent caches for this run.
    pub no_cache: bool,
    /// Extraction-cache size ceiling in megabytes (`None` = no ceiling), from
    /// the CLI override, `FALLOW_CACHE_MAX_SIZE`, or `cache.maxSizeMb`.
    pub cache_max_size_mb: Option<u32>,
    /// Hash over extraction-affecting config (the sorted external plugin
    /// names), mixed into cache keys so plugin changes invalidate cached
    /// extractions instead of serving stale results.
    pub cache_config_hash: u64,
    /// Optional facts collected for this run. Immutable after session creation.
    pub extraction_mode: fallow_types::shared_state::ExtractionMode,
    /// Exact global root/slot pairs omitted from the inventory.
    pub shared_state_allowlist: Vec<[String; 2]>,
    /// Package names and package-name globs excluded from both
    /// unused-dependency and unlisted-dependency detection.
    pub ignore_dependencies: IgnoreDependencyMatcher,
    /// Binary names and name globs that the `unlisted-binaries` rule does
    /// not report.
    pub ignore_binaries: IgnoreDependencyMatcher,
    /// Compiled `workspaces.settings`: per-workspace `ignoreDependencies`
    /// and `includeEntryExports`, bound to the workspaces of each pass.
    pub workspace_settings: super::ResolvedWorkspaceSettings,
    /// Command names whose file arguments do not become entry points; `*`
    /// matches every command.
    pub ignore_command_entries: Vec<String>,
    /// Compiled globs matched against raw import specifiers (not filesystem
    /// paths) whose `unresolved-import` findings are suppressed.
    pub ignore_unresolved_imports: Vec<GlobMatcher>,
    /// Raw `ignoreExports` rules as configured, kept alongside the compiled
    /// form for surfaces that need the original glob text (config editing,
    /// diagnostics).
    pub ignore_export_rules: Vec<IgnoreExportRule>,
    /// `ignoreExports` rules with their file globs pre-compiled for matching.
    pub compiled_ignore_exports: Vec<CompiledIgnoreExportRule>,
    /// `ignoreCatalogReferences` rules with consumer globs pre-compiled.
    pub compiled_ignore_catalog_references: Vec<CompiledIgnoreCatalogReferenceRule>,
    /// `ignoreDependencyOverrides` rules ready for matching.
    pub compiled_ignore_dependency_overrides: Vec<CompiledIgnoreDependencyOverrideRule>,
    /// Same-file-use suppression setting for `unused-export`.
    pub ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig,
    /// Class-member names, globs, or heritage-scoped rules treated as
    /// framework-used and exempt from `unused-class-member`.
    pub used_class_members: Vec<UsedClassMemberRule>,
    /// Decorator names stripped of the automatic `unused-class-member`
    /// exemption that decorated members otherwise receive.
    pub ignore_decorators: Vec<String>,
    /// Compiled regex matched against each declared component prop's local
    /// destructure binding name; a matching prop is exempted from
    /// `unused-component-props`. `None` when `unusedComponentProps.ignorePattern`
    /// is unset. Compiled from the validated raw pattern in [`Self::resolve`].
    pub unused_component_props_ignore: Option<regex::Regex>,
    /// Compiled `health.tailwindIgnorePatterns` matched against normalized
    /// Tailwind arbitrary-value tokens before findings and analytics are built.
    pub tailwind_ignore_patterns: Vec<regex::Regex>,
    /// Options for the `circular-dependencies` rule, passed through unchanged.
    pub circular_dependencies: super::CircularDependenciesConfig,
    /// Effect library options, passed through unchanged.
    pub effect: super::EffectConfig,
    /// Clone-detection settings, passed through unchanged.
    pub duplicates: DuplicatesConfig,
    /// Explicit similar-code candidate settings, passed through unchanged.
    pub similar_code: SimilarCodeConfig,
    /// Explicit mutation-testing settings, passed through unchanged.
    pub mutation: MutationConfig,
    /// Health and complexity thresholds, passed through unchanged.
    pub health: HealthConfig,
    /// TypeScript semantic-analysis opt-in, passed through unchanged.
    pub type_aware: TypeAwareConfig,
    /// Per-rule severities with production-mode adjustments applied: when
    /// [`Self::production`] is set, `unused-dev-dependencies` and
    /// `unused-optional-dependencies` are forced to `off`.
    pub rules: RulesConfig,
    /// Resolved architecture boundaries: preset expanded (honoring the
    /// tsconfig `rootDir`), auto-discovered zones added, and rules validated.
    pub boundaries: ResolvedBoundaryConfig,
    /// Rule packs loaded from the `rulePacks` config key, in config order.
    /// Validated at config load (`load_rule_packs` is also the validation
    /// gate in the CLI and programmatic entry points); a pack that fails to
    /// load here is skipped with a `tracing::error!` as defense in depth.
    pub rule_packs: Vec<crate::rule_pack::RulePackDef>,
    /// Source paths from the `rulePacks` config key, index-aligned with
    /// [`Self::rule_packs`] when every configured pack loaded successfully.
    pub rule_pack_sources: Vec<PathBuf>,
    /// Production mode for this analysis pass: test/spec/story/dev files are
    /// excluded from discovery. Out of [`FallowConfig::resolve`] this is the
    /// global `production` bool ([`super::ProductionConfig::global`]); the
    /// per-analysis object form and CLI/env overrides are applied post-resolve.
    pub production: bool,
    /// Quiet mode from the CLI: suppress non-essential progress and warning
    /// output.
    pub quiet: bool,
    /// External plugin definitions: inline `framework` entries plus those
    /// discovered from `plugins` paths, `.fallow/plugins/`, and root
    /// `fallow-plugin-*` files (first occurrence of a name wins).
    pub external_plugins: Vec<ExternalPluginDef>,
    /// Globs for files loaded dynamically at runtime; matching files are
    /// seeded as entry points so they stay reachable.
    pub dynamically_loaded: Vec<String>,
    /// Template extensions from `markupExtensions`, in addition to
    /// [`super::DEFAULT_MARKUP_EXTENSIONS`]: normalized, sorted, and
    /// deduplicated. Their files count as CSS class references only.
    pub markup_extensions: Vec<String>,
    /// Per-file severity overrides with globs pre-compiled, in config order.
    pub overrides: Vec<ResolvedOverride>,
    /// Authored Git baseline refs keyed by workspace root; validated against
    /// discovered packages when an analysis requests package-scoped changes.
    pub workspace_changed_since: BTreeMap<String, String>,
    /// Workspace package root globs from `workspaces.patterns`, added to the
    /// patterns that the manifests declare.
    pub workspace_patterns: Vec<String>,
    /// Saved regression baseline for `--fail-on-regression`, when embedded.
    pub regression: Option<super::RegressionConfig>,
    /// In-repo `fallow audit` defaults, passed through unchanged.
    pub audit: super::AuditConfig,
    /// Configured CODEOWNERS path override; `None` probes the standard
    /// locations.
    pub codeowners: Option<String>,
    /// Workspace package names (or globs over them) whose public API is
    /// treated as externally consumed, making their export surface a
    /// reachability root.
    pub public_packages: Vec<String>,
    /// Feature-flag detection settings, passed through unchanged.
    pub flags: FlagsConfig,
    /// Security catalogue scoping with `requestReceivers` normalized
    /// (trimmed, lowercased, deduplicated).
    pub security: SecurityConfig,
    /// `fallow fix` behavior settings, passed through unchanged.
    pub fix: super::FixConfig,
    /// Module-resolver settings (extra `exports` conditions), passed through
    /// unchanged.
    pub resolve: ResolveConfig,
    /// When true, entry-point exports are subject to `unused-export`
    /// detection instead of being auto-credited as used.
    pub include_entry_exports: bool,
    /// When true, the report keeps the export and member findings of an
    /// unused file. When false, the rule pass hides them.
    pub show_cascade: bool,
    /// When true, drop Nuxt convention entry-pattern fallbacks that
    /// `nuxt.config` does not explicitly declare; auto-import graph edges are
    /// synthesized regardless.
    pub auto_imports: bool,
    /// When true, a source file that did not parse cleanly fails the run
    /// through the `parse-error` gate. The CLI flag `--fail-on-parse-error`
    /// arms the same gate.
    pub fail_on_parse_error: bool,
    /// Source files strictly larger than this many bytes are skipped at
    /// discovery (never read, parsed, or analyzed), guarding against the
    /// out-of-memory blowup a single multi-MB generated/vendored/bundled file
    /// causes (issue #1086). `None` means no limit. Declaration files
    /// (`.d.ts`/`.d.mts`/`.d.cts`) are exempt regardless of size because they
    /// are reachability roots for global types. Defaults to
    /// [`DEFAULT_MAX_FILE_SIZE_MB`] MB; the CLI overrides it post-resolve from
    /// `--max-file-size` / `FALLOW_MAX_FILE_SIZE` (`0` = unlimited).
    pub max_file_size_bytes: Option<u64>,
    /// Which revision this analysis pass describes. Always
    /// [`AnalysisSnapshot::Current`] out of [`FallowConfig::resolve`]; the CLI
    /// sets [`AnalysisSnapshot::Base`] post-resolve for the isolated
    /// `audit --base` pass so diagnostics can name the base revision.
    pub analysis_snapshot: AnalysisSnapshot,
    /// A stable hash of the config that decides which dead-code findings a
    /// run reports: the merged user config after `extends`, without the keys
    /// that only shape other commands or output, plus the loaded external
    /// plugin and rule pack definitions. It holds no path of this machine,
    /// so two checkouts of one commit give the same value. Settings that a
    /// surface changes after resolution (for example `--include-entry-exports`)
    /// are not in it; the analysis fingerprint adds them.
    pub detection_config_digest: String,
    /// User config path glob lists with a single `*` or `?` that matched `/`
    /// under the old glob rule. Source discovery compares the old and the new
    /// rule on these lists and warns for each pattern that lost matches.
    pub legacy_glob_groups: Vec<super::LegacyGlobGroup>,
}

/// Top-level config keys that do not change which dead-code findings a run
/// reports. The detection digest leaves them out, so a health threshold or a
/// cache setting does not change the analysis fingerprint.
const NON_DETECTION_CONFIG_KEYS: &[&str] = &[
    "minimumVersion",
    "duplicates",
    "similarCode",
    "mutation",
    "health",
    "markupExtensions",
    "security",
    "fix",
    "codeowners",
    "regression",
    "audit",
    "failOnParseError",
    "cache",
];

/// Serialize `value` with every object key sorted, so a map field gives the
/// same bytes whatever its insertion order.
fn canonical_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(String, serde_json::Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonical_json(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(canonical_json).collect())
        }
        other => other,
    }
}

fn canonical_json_string<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .map(canonical_json)
        .map(|value| value.to_string())
        .unwrap_or_default()
}

/// The canonical JSON of the detection-affecting user config keys.
fn detection_config_json(config: &FallowConfig) -> String {
    let mut value = serde_json::to_value(config).unwrap_or_default();
    if let serde_json::Value::Object(map) = &mut value {
        for key in NON_DETECTION_CONFIG_KEYS {
            map.shift_remove(*key);
        }
    }
    canonical_json(value).to_string()
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

/// Hash the extraction-affecting configuration a persisted cache is keyed on:
/// the external plugin names and the user flag patterns, which the parse
/// applies. Built-in-only flag patterns add nothing, so a config without a
/// `flags` section keeps the hash it had before flag patterns joined it.
///
/// Public because a run is not the only thing that needs it: `fallow doctor`
/// inspects the cache without running an analysis, and it resolves config with
/// caching disabled, which zeroes the stored hash. Comparing against that zero
/// reported every healthy cache as config drift.
#[must_use]
pub fn cache_config_hash(external_plugins: &[ExternalPluginDef], flags: &FlagsConfig) -> u64 {
    let mut names: Vec<&str> = external_plugins.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    for name in names {
        hash_str(&mut hasher, name);
    }
    let patterns = flags.patterns();
    if !patterns.is_builtin_only() {
        hasher.update(b"\0flags");
        hasher.update(&(patterns.sdk_patterns.len() as u64).to_le_bytes());
        for (function, name_arg, provider) in &patterns.sdk_patterns {
            hash_str(&mut hasher, function);
            hasher.update(&(*name_arg as u64).to_le_bytes());
            hash_str(&mut hasher, provider);
        }
        hasher.update(&(patterns.env_prefixes.len() as u64).to_le_bytes());
        for prefix in &patterns.env_prefixes {
            hash_str(&mut hasher, prefix);
        }
        hasher.update(&[u8::from(patterns.config_object_heuristics)]);
    }
    hasher.digest()
}

fn hash_str(hasher: &mut xxhash_rust::xxh3::Xxh3, value: &str) {
    hasher.update(&(value.len() as u32).to_le_bytes());
    hasher.update(value.as_bytes());
}

/// Environment variable that moves the persistent analysis cache.
pub const CACHE_DIR_ENV: &str = "FALLOW_CACHE_DIR";

/// Read the non-empty `FALLOW_CACHE_DIR` value from the process environment.
///
/// Every host that loads a project config (CLI, LSP, MCP, Node bindings)
/// applies this value with [`ResolvedConfig::override_cache_dir`], so the
/// variable has one meaning on every surface. It wins over `cache.dir`.
#[must_use]
pub fn cache_dir_env_override() -> Option<PathBuf> {
    std::env::var_os(CACHE_DIR_ENV).and_then(cache_dir_from_env_value)
}

/// Parse a raw `FALLOW_CACHE_DIR` value. An empty value is no override.
#[must_use]
pub fn cache_dir_from_env_value(raw: OsString) -> Option<PathBuf> {
    Some(PathBuf::from(raw)).filter(|path| !path.as_os_str().is_empty())
}

/// Environment variable that caps the extraction cache size in megabytes.
/// It is not a CLI flag because the cap is a platform or CI concern, not an
/// analysis input (ADR-009).
pub const CACHE_MAX_SIZE_ENV: &str = "FALLOW_CACHE_MAX_SIZE";

/// Read `FALLOW_CACHE_MAX_SIZE` from the process environment. It wins over
/// `cache.maxSizeMb` on every host, like [`cache_dir_env_override`].
#[must_use]
pub fn cache_max_size_env_override() -> Option<u32> {
    std::env::var_os(CACHE_MAX_SIZE_ENV)
        .as_deref()
        .and_then(cache_max_size_from_env_value)
}

/// Parse a raw `FALLOW_CACHE_MAX_SIZE` value. Only a positive whole number of
/// megabytes is an override.
#[must_use]
pub fn cache_max_size_from_env_value(raw: &OsStr) -> Option<u32> {
    raw.to_str()?
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|mb| *mb > 0)
}

/// Environment variable that turns `workspaces.changedSince` off for every
/// run of a process, like `--no-package-baselines` does for one run. A CI job
/// that saves or gates a whole-project baseline sets it once.
pub const PACKAGE_BASELINES_ENV: &str = "FALLOW_PACKAGE_BASELINES";

/// Whether a raw `FALLOW_PACKAGE_BASELINES` value turns the package map off.
/// `false`, `0`, `no` and `off` do, in any case. Every other value keeps the
/// map, so a typo never widens a run.
#[must_use]
pub fn package_baselines_disabled_by_env_value(raw: &OsStr) -> bool {
    raw.to_str().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "false" | "0" | "no" | "off"
        )
    })
}

/// The cache directory for the `cache.dir` value of a config file.
///
/// A config file can come from the analyzed repository, so its `cache.dir`
/// must stay below the project root. An absolute path or a `..` path that
/// leaves the root, the root itself, and a path in `.git` are ignored with a
/// warning, and the
/// default `<root>/.fallow` is used. A kept path has its `..` parts resolved.
/// `FALLOW_CACHE_DIR` can still move the cache to any directory.
#[must_use]
pub fn config_cache_dir(root: &Path, configured: Option<PathBuf>) -> PathBuf {
    let default = root.join(".fallow");
    let Some(configured) = configured else {
        return default;
    };
    let absolute = |path: &Path| {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        fallow_types::path_guard::lexically_normalized(&path)
    };
    let dir = resolve_cache_dir(root, Some(configured.clone()));
    // The check uses absolute paths, so a relative root such as `.` cannot
    // accept every path. The result keeps the form of the caller's root.
    let relative = absolute(&dir)
        .strip_prefix(absolute(root))
        .map(Path::to_path_buf)
        .ok();
    if let Some(relative) = relative
        && relative.components().next().is_some()
        && !relative.components().any(|part| part.as_os_str() == ".git")
    {
        return root.join(relative);
    }
    tracing::warn!(
        "ignoring cache.dir `{}` from the config file: it must be a directory inside the project; set {CACHE_DIR_ENV} to put the cache outside the project",
        configured.display()
    );
    default
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

fn cache_dir_is_inside_root(root: &Path, cache_dir: &Path) -> bool {
    if cache_dir.starts_with(root) {
        return true;
    }
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let canonical_cache =
        dunce::canonicalize(cache_dir).unwrap_or_else(|_| cache_dir.to_path_buf());
    canonical_cache.starts_with(&canonical_root)
}

/// Name of the subdirectory that one project root owns inside a shared cache
/// directory: the root's folder name for a reader, then a hash of the full
/// root path, so two roots with the same folder name stay apart.
fn per_root_cache_subdir(root: &Path) -> String {
    let path = root.to_string_lossy().replace('\\', "/");
    let hash = xxhash_rust::xxh3::xxh3_64(path.as_bytes());
    let name: String = root
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        format!("{hash:016x}")
    } else {
        format!("{name}-{hash:016x}")
    }
}

pub(super) fn normalize_user_glob_pattern(pattern: &str) -> &str {
    pattern.strip_prefix("./").unwrap_or(pattern)
}

/// Built-in discovery ignore patterns, unioned into
/// [`ResolvedConfig::ignore_patterns`] after the user's own `ignorePatterns`.
///
/// Public and ordered because the order IS the index layout of that union:
/// user patterns occupy `0..user_ignore_pattern_count` and these follow, in
/// this order. Source discovery maps a match index back to the pattern text
/// through that layout when it reports which built-in removed a candidate
/// source file (issue #2638), so reordering this list or changing where it is
/// appended changes an output contract.
///
/// A `!` entry in `ignorePatterns` is not part of this union. It is an
/// exception that the ignore set applies after the union, so it can lift a
/// built-in for its own subtree (issue #2940). The `node_modules` and `.git`
/// patterns can never be lifted.
pub const DEFAULT_IGNORE_PATTERNS: &[&str] = &[
    "**/node_modules/**",
    "**/dist/**",
    "**/build/**",
    "**/.git/**",
    "**/coverage/**",
    "**/*.min.js",
    "**/*.min.mjs",
    "**/*.min.cjs",
    "**/*.bundle.js",
];

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
    expand_boundary_preset(&mut boundaries, root);
    let logical_groups = boundaries.expand_auto_discover(root);
    let mut resolved = boundaries.resolve();
    resolved.logical_groups = logical_groups;
    resolved
}

/// Expand the boundary preset in place, with the tsconfig `rootDir` as the
/// source root. Does nothing without a preset.
pub(super) fn expand_boundary_preset(
    boundaries: &mut super::boundaries::BoundaryConfig,
    root: &Path,
) {
    if boundaries.preset.is_some() {
        let source_root = crate::workspace::parse_tsconfig_root_dir(root)
            .filter(|r| r != "." && !r.starts_with("..") && !std::path::Path::new(r).is_absolute())
            .unwrap_or_else(|| "src".to_owned());
        if source_root != "src" {
            tracing::info!("boundary preset: using rootDir '{source_root}' from tsconfig.json");
        }
        boundaries.expand(&source_root);
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
            let matchers: Vec<globset::GlobMatcher> = override_entry
                .files
                .iter()
                .map(|pattern| {
                    compile_path_glob(pattern)
                        .expect("overrides[].files pattern was validated at config load time")
                        .compile_matcher()
                })
                .collect();
            if matchers.is_empty() {
                None
            } else {
                Some(ResolvedOverride {
                    files: override_entry.files,
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
            matcher: compile_path_glob(&rule.file)
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
                compile_path_glob(pattern)
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
    patterns: IgnorePatternSet,
    user_pattern_count: usize,
    findings: FindingIgnoreMatcher,
    unresolved_imports: Vec<GlobMatcher>,
    exports: Vec<CompiledIgnoreExportRule>,
    catalog_references: Vec<CompiledIgnoreCatalogReferenceRule>,
    dependency_overrides: Vec<CompiledIgnoreDependencyOverrideRule>,
}

fn compile_ignore_settings(config: &FallowConfig) -> CompiledIgnoreSettings {
    let (patterns, user_pattern_count) = compile_ignore_patterns(&config.ignore_patterns);
    CompiledIgnoreSettings {
        patterns,
        user_pattern_count,
        findings: FindingIgnoreMatcher::compile(&config.ignore_findings),
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
    rule_pack_sources: Vec<PathBuf>,
}

fn resolve_plugin_settings(
    root: &Path,
    configured_plugins: &[String],
    framework: Vec<ExternalPluginDef>,
    rule_packs: &[String],
) -> ResolvedPluginSettings {
    let external_plugins = resolve_external_plugins(root, configured_plugins, framework);

    let configured_rule_packs = rule_packs;
    let rule_packs =
        crate::rule_pack::load_rule_packs(root, configured_rule_packs).unwrap_or_else(|errors| {
            for error in &errors {
                tracing::error!("invalid rule pack: {error}");
            }
            Vec::new()
        });
    let rule_pack_sources = if rule_packs.len() == configured_rule_packs.len() {
        configured_rule_packs.iter().map(PathBuf::from).collect()
    } else {
        Vec::new()
    };

    ResolvedPluginSettings {
        external_plugins,
        rule_packs,
        rule_pack_sources,
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
    config_hash: impl FnOnce() -> u64,
) -> ResolvedCacheSettings {
    ResolvedCacheSettings {
        dir: config_cache_dir(root, configured_dir),
        max_size_mb: override_max_size_mb.or(configured_max_size_mb),
        config_hash: if no_cache { 0 } else { config_hash() },
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

fn compile_unused_component_props_ignore(pattern: Option<&str>) -> Option<regex::Regex> {
    pattern.and_then(|pattern| match regex::Regex::new(pattern) {
        Ok(re) => Some(re),
        Err(error) => {
            tracing::warn!(
                %error,
                "ignoring invalid unusedComponentProps.ignorePattern; this config was \
                 not validated through FallowConfig::load"
            );
            None
        }
    })
}

fn compile_tailwind_ignores(health: &HealthConfig) -> Vec<regex::Regex> {
    health
        .tailwind_ignore_patterns
        .iter()
        .filter_map(|pattern| match regex::Regex::new(pattern) {
            Ok(re) => Some(re),
            Err(error) => {
                tracing::warn!(
                    %error,
                    "ignoring invalid health.tailwindIgnorePatterns entry; this config was \
                     not validated through FallowConfig::load"
                );
                None
            }
        })
        .collect()
}

/// Split `workspaces` into its patterns, its per-package baselines and its
/// compiled per-workspace settings.
fn resolve_workspace_settings(
    workspaces: Option<crate::WorkspaceConfig>,
) -> (
    Vec<String>,
    BTreeMap<String, String>,
    super::ResolvedWorkspaceSettings,
) {
    workspaces
        .map(|workspaces| {
            (
                workspaces.patterns,
                workspaces.changed_since,
                super::ResolvedWorkspaceSettings::compile(&workspaces.settings),
            )
        })
        .unwrap_or_default()
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
        let mut legacy_glob_groups = super::legacy_glob::collect_config_legacy_glob_groups(&self);
        let compiled_ignores = compile_ignore_settings(&self);
        let config_json = detection_config_json(&self);

        let production_rules = resolve_production_rules(self.production, self.rules);

        let plugins =
            resolve_plugin_settings(&root, &self.plugins, self.framework, &self.rule_packs);

        let cache = resolve_cache_settings(
            &root,
            self.cache.dir,
            self.cache.max_size_mb,
            cache_max_size_mb,
            no_cache,
            || cache_config_hash(&plugins.external_plugins, &self.flags),
        );

        let detection_config_digest = fallow_types::identity::fnv1a64_parts(&[
            "config",
            &config_json,
            &canonical_json_string(&{
                let mut sorted: Vec<&ExternalPluginDef> = plugins.external_plugins.iter().collect();
                sorted.sort_by(|a, b| a.name.cmp(&b.name));
                sorted
            }),
            &canonical_json_string(&plugins.rule_packs),
        ]);

        super::legacy_glob::extend_legacy_glob_groups(
            &mut legacy_glob_groups,
            &plugins.external_plugins,
            &plugins.rule_packs,
        );

        let path_policy = resolve_path_policy_settings(self.boundaries, self.overrides, &root);
        let (workspace_patterns, workspace_changed_since, workspace_settings) =
            resolve_workspace_settings(self.workspaces);

        ResolvedConfig {
            root,
            entry_patterns: self.entry,
            support_entry_patterns: self.support_entry,
            ignore_patterns: compiled_ignores.patterns,
            user_ignore_pattern_count: compiled_ignores.user_pattern_count,
            entry_pattern_effect: EntryEffectRecord::default(),
            ignore_findings: compiled_ignores.findings,
            output,
            cache_dir: cache.dir,
            threads,
            no_cache,
            cache_max_size_mb: cache.max_size_mb,
            cache_config_hash: cache.config_hash,
            extraction_mode: fallow_types::shared_state::ExtractionMode::Standard,
            shared_state_allowlist: self.shared_state_allowlist,
            ignore_dependencies: IgnoreDependencyMatcher::compile(&self.ignore_dependencies),
            ignore_binaries: IgnoreDependencyMatcher::compile(&self.ignore_binaries),
            workspace_settings,
            ignore_command_entries: self.ignore_command_entries,
            ignore_unresolved_imports: compiled_ignores.unresolved_imports,
            ignore_export_rules: self.ignore_exports,
            compiled_ignore_exports: compiled_ignores.exports,
            compiled_ignore_catalog_references: compiled_ignores.catalog_references,
            compiled_ignore_dependency_overrides: compiled_ignores.dependency_overrides,
            ignore_exports_used_in_file: self.ignore_exports_used_in_file,
            used_class_members: self.used_class_members,
            ignore_decorators: self.ignore_decorators,
            unused_component_props_ignore: compile_unused_component_props_ignore(
                self.unused_component_props.ignore_pattern.as_deref(),
            ),
            tailwind_ignore_patterns: compile_tailwind_ignores(&self.health),
            circular_dependencies: self.circular_dependencies,
            effect: self.effect,
            duplicates: self.duplicates,
            similar_code: self.similar_code,
            mutation: self.mutation,
            health: self.health,
            type_aware: self.type_aware,
            rules: production_rules.rules,
            boundaries: path_policy.boundaries,
            rule_packs: plugins.rule_packs,
            rule_pack_sources: plugins.rule_pack_sources,
            production: production_rules.production,
            quiet,
            external_plugins: plugins.external_plugins,
            dynamically_loaded: self.dynamically_loaded,
            markup_extensions: super::resolve_markup_extensions(&self.markup_extensions),
            overrides: path_policy.overrides,
            workspace_changed_since,
            workspace_patterns,
            regression: self.regression,
            audit: self.audit,
            codeowners: self.codeowners,
            public_packages: self.public_packages,
            flags: self.flags,
            security: normalize_security_config(self.security),
            fix: self.fix,
            resolve: self.resolve,
            include_entry_exports: self.include_entry_exports,
            show_cascade: self.show_cascade,
            auto_imports: self.auto_imports,
            fail_on_parse_error: self.fail_on_parse_error,
            max_file_size_bytes: Some(DEFAULT_MAX_FILE_SIZE_BYTES),
            analysis_snapshot: AnalysisSnapshot::Current,
            detection_config_digest,
            legacy_glob_groups,
        }
    }
}

impl ResolvedConfig {
    /// Turn `includeEntryExports` on in every workspace for this run, the
    /// way the `--include-entry-exports` flag does. It wins over a
    /// `workspaces.settings` value of `false`.
    pub const fn force_include_entry_exports(&mut self) {
        self.include_entry_exports = true;
        self.workspace_settings.force_include_entry_exports();
    }

    /// The effective `includeEntryExports` value for the file at `path`.
    #[must_use]
    pub fn include_entry_exports_for(&self, path: &Path) -> bool {
        self.workspace_settings
            .include_entry_exports_for(path, self.include_entry_exports)
    }

    /// Whether `ignoreDependencies` hides the dependency finding for
    /// `package_name` that `path` owns: a `package.json` for an unused
    /// dependency, or the importing file for an unlisted dependency. The
    /// global list applies everywhere; a `workspaces.settings` list applies
    /// in the workspaces that its key selects.
    #[must_use]
    pub fn suppresses_dependency_at(&self, path: &Path, package_name: &str) -> bool {
        self.ignore_dependencies.suppresses(package_name)
            || self
                .workspace_settings
                .suppresses_dependency_at(path, package_name)
    }

    /// Apply a `FALLOW_PACKAGE_BASELINES` value: a false value empties
    /// `workspaces.changedSince`, so no run of the process reads the map.
    pub fn apply_package_baselines_env(&mut self, raw: Option<&OsStr>) {
        if raw.is_some_and(package_baselines_disabled_by_env_value) {
            self.workspace_changed_since.clear();
        }
    }

    /// Replace the resolved cache directory with a host override, such as
    /// `FALLOW_CACHE_DIR`. A relative path resolves from the project root,
    /// the same base as `cache.dir`.
    pub fn override_cache_dir(&mut self, dir: PathBuf) {
        self.cache_dir = resolve_cache_dir(&self.root, Some(dir));
    }

    /// The cache directory with the base that its writes must stay in.
    #[must_use]
    pub fn cache_target(&self) -> fallow_types::path_guard::CacheTarget<'_> {
        fallow_types::path_guard::CacheTarget::new(&self.root, &self.cache_dir)
    }

    /// Give this project root its own subdirectory when the cache directory
    /// is outside the root.
    ///
    /// A host that analyzes more than one root in one process, such as the
    /// language server with a multi-root workspace, calls this. Without it,
    /// the roots write the same `cache.bin` and `graph-cache.bin`, and each
    /// save replaces the cache of the other root. The CLI does not call it,
    /// so a shared CI cache stays valid when the checkout path changes. A
    /// cache directory inside the root already belongs to that root and
    /// stays unchanged.
    pub fn scope_shared_cache_dir_to_root(&mut self) {
        if cache_dir_is_inside_root(&self.root, &self.cache_dir) {
            return;
        }
        self.cache_dir = self.cache_dir.join(per_root_cache_subdir(&self.root));
    }

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
#[path = "resolution_tests.rs"]
mod tests;

impl ResolvedConfig {
    /// Whether a rule of this config reads the call facts of the modules: the
    /// `constant-parameter` rule is on, globally or in an override.
    ///
    /// A run for which this is `false` neither stores nor decodes call facts.
    /// The parse cache records which entries hold them, so a later run that
    /// reads them parses only the files whose entry lacks them.
    #[must_use]
    pub fn reads_call_facts(&self) -> bool {
        self.rules.constant_parameters != crate::Severity::Off
            || self.overrides.iter().any(|overrides| {
                overrides
                    .rules
                    .constant_parameters
                    .is_some_and(|severity| severity != crate::Severity::Off)
            })
    }

    /// Request optional shared-state facts with a distinct cache identity.
    pub fn request_shared_state(&mut self) {
        if self.extraction_mode == fallow_types::shared_state::ExtractionMode::Standard {
            self.extraction_mode = fallow_types::shared_state::ExtractionMode::SharedState;
            // Domain separation preserves the exact normal-mode cache identity.
            const SHARED_STATE_CACHE_DOMAIN: u64 = 0x7368_6172_6564_0001;
            self.cache_config_hash ^= SHARED_STATE_CACHE_DOMAIN;
        }
    }
}
