mod boundaries;
mod duplicates_config;
mod flags;
mod format;
pub mod glob_validation;
mod health;
mod parsing;
mod resolution;
mod resolve;
mod rules;
mod used_class_members;

#[expect(
    clippy::redundant_pub_crate,
    reason = "this module is glob re-exported from lib.rs, so `pub` would leak the helper into the public API; pub(crate) keeps it internal to the crate"
)]
pub(crate) use boundaries::wildcard_placement_error;
pub use boundaries::{
    AuthoredRule, BoundaryCallsConfig, BoundaryConfig, BoundaryCoverageConfig, BoundaryPreset,
    BoundaryRule, BoundaryZone, ForbiddenCallRule, ForbiddenCallee, InvalidForbiddenCallee,
    LogicalGroup, LogicalGroupStatus, RedundantRootPrefix, ResolvedBoundaryConfig,
    ResolvedBoundaryCoverageConfig, ResolvedBoundaryRule, ResolvedZone, UnknownZoneRef,
    ZoneReferenceKind, ZoneValidationError,
};
pub use duplicates_config::{
    DetectionMode, DuplicatesConfig, NormalizationConfig, ResolvedNormalization,
};
pub use flags::{FlagsConfig, SdkPattern};
pub use format::OutputFormat;
pub use health::{EmailMode, HealthConfig, HealthThresholdOverride, OwnershipConfig};
pub use parsing::ConfigLoadOptions;
pub use resolution::{
    CompiledIgnoreCatalogReferenceRule, CompiledIgnoreDependencyOverrideRule,
    CompiledIgnoreExportRule, ConfigOverride, DEFAULT_MAX_FILE_SIZE_BYTES,
    DEFAULT_MAX_FILE_SIZE_MB, IgnoreCatalogReferenceRule, IgnoreDependencyOverrideRule,
    IgnoreExportRule, ResolvedConfig, ResolvedOverride, resolve_max_file_size_bytes,
};
pub use resolve::ResolveConfig;
pub use rules::{
    KNOWN_RULE_NAMES, PartialRulesConfig, RulesConfig, Severity, closest_known_rule_name,
    default_severity_for_kind, is_opt_in_kind,
};
pub use used_class_members::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::ops::Not;
use std::path::PathBuf;

use crate::external_plugin::ExternalPluginDef;
use crate::workspace::WorkspaceConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged, rename_all = "camelCase")]
pub enum IgnoreExportsUsedInFileConfig {
    Bool(bool),
    ByKind(IgnoreExportsUsedInFileByKind),
}

impl Default for IgnoreExportsUsedInFileConfig {
    fn default() -> Self {
        Self::Bool(false)
    }
}

impl From<bool> for IgnoreExportsUsedInFileConfig {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<IgnoreExportsUsedInFileByKind> for IgnoreExportsUsedInFileConfig {
    fn from(value: IgnoreExportsUsedInFileByKind) -> Self {
        Self::ByKind(value)
    }
}

impl IgnoreExportsUsedInFileConfig {
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        match self {
            Self::Bool(value) => value,
            Self::ByKind(kind) => kind.type_ || kind.interface,
        }
    }

    #[must_use]
    pub const fn suppresses(self, is_type_only: bool) -> bool {
        match self {
            Self::Bool(value) => value,
            Self::ByKind(kind) => is_type_only && (kind.type_ || kind.interface),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IgnoreExportsUsedInFileByKind {
    /// When `true`, enables the same-file-use suppression for type-only exports (serialized as `type`; part of the object form of `ignoreExportsUsedInFile`). Because fallow groups type aliases and interfaces under one issue kind, setting either `type` or `interface` enables the identical type-only suppression, applied only to exports fallow classifies as type-only.
    #[serde(default, rename = "type")]
    pub type_: bool,
    /// When `true`, enables the same-file-use suppression for type-only exports (part of the object form of `ignoreExportsUsedInFile`). Fallow does not distinguish interfaces from type aliases in this issue kind, so `interface` behaves identically to `type`: setting either one turns on the type-only same-file suppression.
    #[serde(default)]
    pub interface: bool,
}

/// Options for the `unused-component-props` rule.
///
/// Lets a project exempt component props whose local destructure binding name
/// matches a regex from `unused-component-props`, honoring the
/// "accepted-but-intentionally-unused" leading-underscore convention (Svelte 5
/// `$props()`, React destructure) that mirrors TypeScript `noUnusedParameters`
/// and ESLint `@typescript-eslint/no-unused-vars` `varsIgnorePattern` /
/// `argsIgnorePattern`. Opt-in; an unset `ignorePattern` leaves the rule's
/// behavior unchanged.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct UnusedComponentPropsConfig {
    /// Regex matched against each declared prop's LOCAL destructure binding name
    /// (e.g. `_stage` in `let { stage: _stage } = $props()`), which falls back
    /// to the public prop name when there is no alias. A prop whose local name
    /// matches is treated as intentionally unused and never reported as
    /// `unused-component-props`. Matching is unanchored (substring), like
    /// ESLint's `RegExp.test`, so anchor with `^_` to match a leading
    /// underscore. Compiled and validated at config load (an invalid regex fails
    /// load). Applies to Vue, Svelte, Astro, and React/Preact props.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_pattern: Option<String>,
}

impl UnusedComponentPropsConfig {
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.ignore_pattern.is_none()
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FixConfig {
    /// Groups `fallow fix` settings for pnpm workspace catalog cleanup. Its only key, `deletePrecedingComments` (`auto` default, `always`, `never`), controls whether a comment block directly above a removed unused `pnpm-workspace.yaml` catalog entry is deleted with the entry.
    #[serde(default)]
    pub catalog: CatalogFixConfig,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFixConfig {
    /// Controls whether comment lines immediately above an unused `pnpm-workspace.yaml` catalog entry are removed when `fallow fix` deletes that entry: `auto` (default: delete only when the comment block is preceded by a blank line or sits directly under the parent catalog header, and never when it is a section banner like `# ====`), `always` (always remove the adjacent comment block), or `never` (leave all preceding comments). A `fallow-keep` marker anywhere in the block always preserves it regardless of this setting. Set `never` for teams that keep hand-authored notes above catalog pins.
    #[serde(default)]
    pub delete_preceding_comments: CatalogPrecedingCommentPolicy,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CatalogPrecedingCommentPolicy {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FallowConfig {
    /// A string pointing at fallow's JSON Schema URL, used only by editors for autocomplete and validation of the config file; it has no effect on analysis and is stripped before serialization (serde skip_serializing, writeOnly in the schema). Set it to `https://fallow.dev/schema.json` to get editor IntelliSense; any other value is ignored by fallow.
    #[serde(rename = "$schema", default, skip_serializing)]
    pub schema: Option<String>,

    /// An ordered array of parent config sources to inherit before this file's own keys apply; each entry is a file-relative path, an `npm:<package>` specifier, or an `https://` URL (`http://` is rejected), deep-merged in order so objects merge field-by-field while arrays and scalars in this file replace the parent's, with cycle and depth guards. Set it to share a base config across a monorepo or team; it is consumed at load and stripped before serialization (serde skip_serializing).
    #[serde(default, skip_serializing)]
    pub extends: Vec<String>,

    /// An array of project-root-relative glob patterns whose matching files are seeded as manual entry points, on top of the framework and package.json entries fallow discovers automatically, so their transitive imports are not reported as unused. Set it (e.g. `["src/main.ts"]`) when a file is a real runtime root that no plugin or manifest declares; patterns are validated at load and matched against discovered files.
    #[serde(default)]
    pub entry: Vec<String>,

    /// An array of project-root-relative glob patterns for files to exclude from analysis entirely; entries are unioned with fallow's built-in defaults (**/node_modules/**, **/dist/**, build/**, **/.git/**, **/coverage/**, **/*.min.js, **/*.min.mjs, **/*.min.cjs, **/*.bundle.js), so custom globs add to rather than replace them. Set it (e.g. `["generated/**"]`) to drop generated or vendored trees from every detector; patterns are validated at load.
    #[serde(default)]
    pub ignore_patterns: Vec<String>,

    /// Declares inline external framework plugins as data (array of plugin objects), each with `name` plus optional `enablers` (package names that activate it) or richer `detection` (dependency/file-existence/`all`/`any` checks, taking priority over `enablers`), `entryPoints` (+ `entryPointRole` runtime/support/test), `configPatterns`, `alwaysUsed`, `toolingDependencies`, `usedExports` (`{ pattern, exports }`), and `usedClassMembers`. Set it to keep a custom or in-house framework's entry points, config files, and conventions reachable without a Rust plugin; these definitions are appended to plugins discovered via `plugins`, `.fallow/plugins/`, and root `fallow-plugin-*` files (first occurrence of a name wins), and cannot do AST-based config parsing.
    #[serde(default)]
    pub framework: Vec<ExternalPluginDef>,

    /// Monorepo workspace configuration whose sole sub-key patterns (array of globs) adds workspace package roots beyond those discovered from package.json workspaces, pnpm-workspace.yaml, and tsconfig references. Optional and absent by default (discovery uses the manifests alone); set it only when workspaces live in directories the standard manifests do not declare.
    #[serde(default)]
    pub workspaces: Option<WorkspaceConfig>,

    /// A list of exact package names excluded from BOTH unused-dependency and unlisted-dependency detection, so a runtime-provided or otherwise-untracked package (e.g. `bun:sqlite`, a peer supplied at deploy time) is never flagged as unused when declared nor as unlisted when imported. Set it for packages fallow cannot observe being used and cannot observe being declared; matching is exact string equality against the package name, not a glob.
    #[serde(default)]
    pub ignore_dependencies: Vec<String>,

    /// A list of glob patterns that suppress only `unresolved-import` findings whose raw import specifier matches; it does not change dependency usage accounting or resolver behavior. Patterns match the import string as written (not a filesystem path), so list both `@example/icons` and `@example/icons/**` to cover a bare package and its subpaths; parent-relative generated specifiers like `../generated/**` are valid, and broad values like `**` can hide real missing modules.
    #[serde(default)]
    pub ignore_unresolved_imports: Vec<String>,

    /// A list of per-file rules that exempt named exports from `unused-export` and from duplicate-exports grouping for files matching a glob. Each entry is `{ file: <glob>, exports: [<name>, ...] }` where `exports: ["*"]` exempts every export in the file and a name list exempts only those names; built for component-library barrels (shadcn/Radix/bits-ui `index.ts`) that intentionally re-export the same short names across many files.
    #[serde(default)]
    pub ignore_exports: Vec<IgnoreExportRule>,

    /// A list of rules that suppress `unresolved-catalog-reference` findings (a workspace `package.json` referencing a `catalog:` or `catalog:<name>` that the catalog does not declare); config-only because `package.json` has no inline-suppression comment surface. Each entry needs a `package` (exact match) plus optional `catalog` (exact catalog-name match) and `consumer` (glob on the consuming package.json path); use it for staged catalog migrations where the catalog edit lands in a separate change.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_catalog_references: Vec<IgnoreCatalogReferenceRule>,

    /// A list of rules that suppress `unused-dependency-override` and `misconfigured-dependency-override` findings for pnpm `overrides` entries; config-only, matched against the override's target package. Each entry needs a `package` (exact match) plus an optional `source` to scope the suppression to `"pnpm-workspace.yaml"` or `"package.json"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_dependency_overrides: Vec<IgnoreDependencyOverrideRule>,

    /// Controls whether an export referenced only by another symbol in the same file is treated as used (suppressed from `unused-export`) until it becomes completely unreferenced; references inside an export specifier itself (`export { foo }`, `export default foo`) do not count as same-file uses. Accepts `true`/`false` (default `false`, suppress nothing) or the knip-parity object `{ "type": true, "interface": true }`, which restricts the suppression to type-only exports; fallow groups type aliases and interfaces under one kind, so both object fields behave identically.
    #[serde(default)]
    pub ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig,

    /// A list of decorator names that no longer grant a class member automatic exemption from `unused-class-member`: a member whose every decorator is in this set is checked normally, while a member carrying any decorator NOT listed here stays skipped (frameworks consume decorated members reflectively). Dotted entries match the full decorator path (`ns.foo`) and bare entries match the leftmost segment (so `"decorators"` collapses every `@decorators.*`); both `"@step"` and `"step"` are accepted (leading `@` stripped), and an unmatched entry emits a one-time warning.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_decorators: Vec<String>,

    /// A list of class-member names or glob patterns treated as framework-used, so a method a library invokes reflectively (ag-Grid `agInit`/`refresh`, Web Component `connectedCallback`) is not reported as `unused-class-member`; it applies to class members only, not enum members. Each entry is either a plain string/glob (`"agInit"`, `"enter*"`, `"*"`) applied to every class, or a scoped object `{ extends?, implements?, members: [...] }` that applies only when the class matches that heritage clause (a scoped rule requires `extends` or `implements`); patterns matching zero members warn once.
    #[serde(default)]
    pub used_class_members: Vec<UsedClassMemberRule>,

    /// Configures clone detection: `enabled` (default true), `mode` (`strict`, `mild` default, `weak`, `semantic`, from least to most identifier/literal blinding; `strict` and `mild` are equivalent under fallow's AST tokenizer, `weak` blinds string literals, `semantic` blinds all identifiers and literals for Type-2 renamed-variable detection), `minTokens` (50), `minLines` (5), `minOccurrences` (integer >= 2, deserialization fails below 2), `threshold` (max duplication percentage, 0 = no limit), `ignore` globs, `ignoreDefaults` (true, merge built-in generated-file ignores), `skipLocal` (only report cross-directory clones), `crossLanguage` (strip TS type annotations to match .ts against .js), `ignoreImports` (true, strip ES import/re-export/top-level require wiring from the token stream), and `normalization` (per-flag `ignoreIdentifiers`/`ignoreStringValues`/`ignoreNumericValues` overrides on top of `mode`). Raise `minOccurrences` to focus on widespread copy-paste, or set `mode` to `semantic` to catch renamed-variable clones.
    #[serde(default)]
    pub duplicates: DuplicatesConfig,

    /// Sets complexity and health thresholds for `fallow health` (also applied in combined `fallow` and `fallow audit`): `maxCyclomatic` (20), `maxCognitive` (15), `maxCrap` (30.0, findings at or above this are reported), `crapRefactorBand` (5, cyclomatic band below `maxCyclomatic` where a secondary refactor action is added), `maxUnitSize` (max function lines before a large-function finding, 60), `coverage`/`coverageRoot` (Istanbul coverage path and path-prefix strip for accurate CRAP), `ignore` globs (remove files from findings AND the health score), `thresholdOverrides` (per-file/per-function ceilings via `files`/`functions`/`maxCyclomatic`/`maxCognitive`/`maxCrap`/`maxUnitSize`/`reason`), `ownership` (`botPatterns` and `emailMode` for `--ownership`), and `suggestInlineSuppression` (true, emit `suppress-line` action hints in JSON). Raise thresholds to relax which functions are flagged, wire `coverage` for real CRAP scores, or exempt generated/test files via `ignore` (drops them from the score too) or `thresholdOverrides` (keeps them visible under a higher ceiling).
    #[serde(default)]
    pub health: HealthConfig,

    /// Sets per-issue-type severity, keyed by kebab-case rule id: `error` reports and fails CI (non-zero exit), `warn` reports without failing, `off` disables detection and reporting entirely (e.g. `{ "unused-files": "error", "unused-exports": "warn", "private-type-leaks": "off" }`). Set a rule `off` to silence it, `warn` to demote below CI gating, or `error` to promote a warn/off-default rule to gating; most rules default to `error`, dev/optional-dependency and component/store/inject/CSS/catalog rules default to `warn`, and opt-in rules (`private-type-leaks`, `security-*`, `prop-drilling`, `thin-wrapper`, `duplicate-prop-shape`, `coverage-gaps`, `feature-flags`, `require-suppression-reason`) default to `off`. Singular aliases (`unused-file`) and `warning`/`none` severity spellings are accepted.
    #[serde(default)]
    pub rules: RulesConfig,

    #[serde(
        default,
        skip_serializing_if = "UnusedComponentPropsConfig::is_default"
    )]
    /// Options for the `unused-component-props` rule, currently only `ignorePattern`: a regex matched against each declared prop's local destructure binding name (falling back to the public prop name when unaliased) to exempt intentionally-unused props such as the leading-underscore convention. Set `{ "ignorePattern": "^_" }` to skip props like `_stage`; matching is unanchored (substring, like ESLint's `RegExp.test`) so anchor with `^`, the pattern is validated at config load (invalid regex fails load), and it applies to Vue, Svelte, Astro, and React/Preact props (unset leaves the rule unchanged).
    pub unused_component_props: UnusedComponentPropsConfig,

    /// Configures architecture boundary enforcement: which source directories belong to which named zone and which zones may import which others, reported as boundary-violation, boundary-coverage-violation, and boundary-call-violation findings (severity via rules.boundary-violation, default error). Set to enforce a layered/module architecture; the object holds `preset` (one of layered, hexagonal, feature-sliced, bulletproof, whose default zones/rules are merged in with the user-declared zones/rules taking precedence), `zones` (each with `name`, `patterns`, `autoDiscover`, optional `root`), `rules` (each with `from`, `allow`, `allowTypeOnly` target-zone lists), `coverage` (`requireAllFiles` plus `allowUnmatched` globs for files matching no zone), and `calls` (a `forbidden` list of `{from, callee}` banned-call rules per zone).
    #[serde(default)]
    pub boundaries: BoundaryConfig,

    /// Configures feature-flag detection: `sdkPatterns` (custom flag-evaluating call signatures, each `{ function, nameArg (zero-based arg index of the flag name, default 0), provider? }`, merged with built-ins for LaunchDarkly, Statsig, Unleash, GrowthBook, Split, PostHog, Vercel Flags, ConfigCat, Flagsmith, Optimizely, and Eppo), `envPrefixes` (env-var prefixes marking `process.env.*` accesses as flags, merged with built-ins), and `configObjectHeuristics` (default false; when true, property accesses on objects whose name contains `feature`/`flag`/`toggle` are reported as low-confidence flags). Set `sdkPatterns`/`envPrefixes` to teach fallow a proprietary flag SDK or naming convention, or enable `configObjectHeuristics` for projects that read flags off config objects (higher false-positive rate). Feature-flag findings surface only when the `feature-flags` rule is enabled (default `off`).
    #[serde(default)]
    pub flags: FlagsConfig,

    /// Scopes the opt-in `fallow security` catalogue: which candidate categories run and which extra local identifiers count as HTTP request objects. Set when tuning security-candidate detection; the object holds `categories` (an object with `include` and/or `exclude` string arrays of catalogue category ids, where `include` restricts to a whitelist and `exclude` removes from the admitted set, both unset admits all ordinary categories) and `requestReceivers` (a string array of project-local names that extend, not replace, the built-in `*.query`/`*.params`/`*.body` source-receiver allowlist). The `hardcoded-secret` and `secret-to-network` categories are include-required: they fire only when explicitly listed in `categories.include`, even when no include list is otherwise set. The valid category ids are enumerated (with title, CWE, and include-required flag) in the `security_categories` block of `fallow schema`, and also listed by `fallow security --help`; they are not in this config-schema.
    #[serde(default)]
    pub security: SecurityConfig,

    /// Configures `fallow fix` behavior. Currently holds one nested section, `catalog` (a `CatalogFixConfig`), whose only key `deletePrecedingComments` (`auto` default, `always`, `never`) governs whether comment lines directly above a removed unused `pnpm-workspace.yaml` catalog entry are deleted with it.
    #[serde(default)]
    pub fix: FixConfig,

    /// Configures the module resolver. Its one key `conditions` is a list of additional package.json `exports`/`imports` condition names to honor, matched at higher priority than fallow's built-ins (`development`, `import`, `require`, `default`, `types`, `node`, plus `react-native`/`browser` when the React Native or Expo plugin is active). Set it when a package's `exports` map has custom branches (e.g. `worker`, `deno`, `edge`) that fallow should follow instead of the default branch.
    #[serde(default)]
    pub resolve: ResolveConfig,

    /// Enables production mode, which excludes test/spec/story/dev files from discovery and forces `unused-dev-dependencies` and `unused-optional-dependencies` to `off`. Accepts a boolean (default false) applied to all analyses, or a per-analysis object `{ deadCode?, health?, dupes? }` (each boolean, default false) that scopes production mode to individual analyses in combined `fallow` and `fallow audit`. Set it to analyze only shipped code; the `--production`/`--no-production` and `--production-{dead-code,health,dupes}` CLI flags and `FALLOW_PRODUCTION*` env vars override this value (CLI flags win, then per-analysis env, then global env, then config).
    #[serde(default)]
    pub production: ProductionConfig,

    /// List of paths (relative to the project root, must resolve within it) to external plugin definition files or directories in JSONC/JSON/TOML, loaded in addition to the auto-discovered `.fallow/plugins/` directory and root `fallow-plugin-*` files. Set it to load plugin definitions kept outside those default locations; a path resolving outside the project root is skipped with a `tracing::warn`, and paths listed here are searched before the auto-discovered locations (first occurrence of a plugin name wins).
    #[serde(default)]
    pub plugins: Vec<String>,

    /// Paths to declarative rule-pack files (JSON or JSONC), relative to the
    /// project root. Each pack declares `banned-call`, `banned-import`, or
    /// `banned-effect` rules that report as `policy-violation` findings. Packs
    /// are pure data: no project code is executed. Invalid or missing packs
    /// fail config load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rule_packs: Vec<String>,

    /// An array of project-root-relative glob patterns for files loaded at runtime by a mechanism the static graph cannot see (dynamic path resolution, config-driven loading); matching files are seeded as entry points so they and their imports stay reachable. Empty by default; set it (e.g. `["plugins/**/*.ts", "locales/**/*.json"]`) for plugin or locale trees pulled in dynamically.
    #[serde(default)]
    pub dynamically_loaded: Vec<String>,

    /// An ordered list of per-file rule-severity overrides: each entry re-severities specific analysis rules for files its globs match, layered on top of the top-level `rules` defaults. Set to relax or tighten rules for a subset of paths (e.g. downgrade unused-exports to warn under a generated directory); each entry has `files` (glob-pattern array) and `rules` (a partial per-rule severity map of error/warn/off). Entries apply in list order and a file matched by several entries takes every matching entry's overrides (later entries win on conflict); inter-file rules (duplicate-exports, circular-dependencies, re-export-cycle) have no effect in an override (fallow warns during analysis and points to the right mechanism: top-level `ignoreExports` for duplicate-exports, a file-level `// fallow-ignore-file` comment for the others).
    #[serde(default)]
    pub overrides: Vec<ConfigOverride>,

    /// A project-root-relative path to a CODEOWNERS file, used by fallow health --hotspots --ownership to attribute declared owners and compute unowned/drifting ownership state; setting it overrides the default probe order (CODEOWNERS, .github/CODEOWNERS, .gitlab/CODEOWNERS, docs/CODEOWNERS). String, defaults to null (auto-probe the standard locations); set it only when the CODEOWNERS file lives at a non-standard location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codeowners: Option<String>,

    /// An array of internal workspace package names (or globs matched against workspace package names) whose public API is intentionally consumed outside the analyzed graph; their entry points and re-export surface become reachability roots, so their exported files, exports, and class members are not reported as unused. Set it (e.g. `["@myorg/shared-lib", "@myorg/*"]`) for library packages in a monorepo that ship an API to external consumers; only meaningful when workspaces are present (an empty list or no workspaces is a no-op).
    #[serde(default)]
    pub public_packages: Vec<String>,

    /// Holds a saved issue-count baseline that the `--fail-on-regression` gate compares the current run against, failing only when counts grow beyond tolerance relative to the baseline. Usually written by `--save-baseline` rather than hand-authored; the object has a single `baseline` sub-key holding per-issue-type counts (total_issues plus per-kind fields like unused_exports, boundary_violations, policy_violations, each defaulting to 0). Absent means no baseline is embedded in config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regression: Option<RegressionConfig>,

    /// Sets in-repo defaults for `fallow audit` (the changed-files quality gate) so CLI flags need not repeat per run. Set to pin audit behavior; the object holds `gate` (`new-only` or `all`, which findings drive the verdict), `css`/`cssDeep` (booleans toggling styling analysis and the project-wide CSS reachability pass), `deadCodeBaseline`/`healthBaseline`/`dupesBaseline` (per-sub-analysis baseline file paths), and `cacheMaxAgeDays` (GC window in days for the reusable base-snapshot worktree cache). The matching CLI flag overrides each field.
    #[serde(default, skip_serializing_if = "AuditConfig::is_empty")]
    pub audit: AuditConfig,

    /// When true, restricts this config's extends entries to file-relative paths that resolve inside the config file's own directory; any https:// URL, npm: package, or relative path escaping that directory is rejected at load with a hard error. Boolean, defaults to false (URL, npm, and any-relative extends are permitted); set it to true to harden a config against pulling in remote or out-of-tree bases.
    #[serde(default)]
    pub sealed: bool,

    /// When true, exports of entry-point files are subject to unused-export detection instead of being auto-credited as used, so a typo'd or stray export in a framework route or package entry (e.g. meatdata for metadata) is flagged; plugin used_exports allowlists are still honored. Boolean, defaults to false; the CLI flag --include-entry-exports applies the same behavior for one run.
    #[serde(default)]
    pub include_entry_exports: bool,

    /// When true, drops Nuxt convention-based entry-pattern fallbacks: component fallbacks are dropped unless nuxt.config declares components:, and composable/util fallbacks are dropped unless it declares imports:, so genuinely-unreferenced convention files surface as unused-file. Boolean, defaults to false; set it for a Nuxt project that has explicitly configured its auto-import directories. Synthesis of auto-import graph edges (resolving `<Card />` or `useUserStore()` to their convention files) happens regardless of this flag.
    #[serde(default)]
    pub auto_imports: bool,

    /// Overrides the location and size ceiling of fallow's persistent extraction cache (default `.fallow/cache.bin` under the project root). Set to relocate the cache or cap its footprint; the object holds `dir` (cache directory, relative paths resolve from the project root) and `maxSizeMb` (extraction-cache size limit in megabytes). The `FALLOW_CACHE_MAX_SIZE` environment variable overrides `maxSizeMb`.
    #[serde(default, skip_serializing_if = "CacheConfig::is_default")]
    pub cache: CacheConfig,
}

/// Scopes `fallow security` catalogue behavior. An absent category block admits
/// every catalogue category. `hardcoded-secret` is include-required and only
/// runs when explicitly listed in `security.categories.include`.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SecurityConfig {
    /// Include/exclude filter over category ids (e.g. `dangerous-html`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<SecurityCategories>,
    /// Additional project-local names for HTTP request objects. These names
    /// extend the built-in receiver allowlist for `*.query`, `*.params`, and
    /// `*.body` source patterns. They do not replace the built-ins and do not
    /// gate `*.searchParams`, which intentionally stays ungated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_receivers: Vec<String>,
}

impl SecurityConfig {
    #[must_use]
    pub fn normalized_request_receivers(&self) -> Vec<String> {
        let mut receivers = Vec::new();
        for receiver in &self.request_receivers {
            let normalized = receiver.trim().to_ascii_lowercase();
            if !normalized.is_empty() && !receivers.contains(&normalized) {
                receivers.push(normalized);
            }
        }
        receivers
    }

    #[must_use]
    pub fn request_receivers_are_valid(&self) -> bool {
        self.request_receivers
            .iter()
            .all(|receiver| !receiver.trim().is_empty())
    }
}

/// Include/exclude lists scoping the active security categories. When `include`
/// is set, only those categories are active; `exclude` removes categories from
/// the admitted set. Both unset admits catalogue categories. `hardcoded-secret`
/// still requires explicit inclusion.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SecurityCategories {
    /// Catalogue category ids to admit. When set, all others are excluded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// Catalogue category ids to remove from the admitted set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CacheConfig {
    /// Directory for fallow's persistent analysis cache. Relative paths resolve
    /// from the project root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<PathBuf>,
    /// Maximum size of the persistent extraction cache, in megabytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_size_mb: Option<u32>,
}

impl CacheConfig {
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.dir.is_none() && self.max_size_mb.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionAnalysis {
    DeadCode,
    Health,
    Dupes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ProductionConfig {
    Global(bool),
    PerAnalysis(PerAnalysisProductionConfig),
}

impl<'de> Deserialize<'de> for ProductionConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ProductionConfigVisitor;

        impl<'de> serde::de::Visitor<'de> for ProductionConfigVisitor {
            type Value = ProductionConfig;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a boolean or per-analysis production config object")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ProductionConfig::Global(value))
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                PerAnalysisProductionConfig::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )
                .map(ProductionConfig::PerAnalysis)
            }
        }

        deserializer.deserialize_any(ProductionConfigVisitor)
    }
}

impl Default for ProductionConfig {
    fn default() -> Self {
        Self::Global(false)
    }
}

impl From<bool> for ProductionConfig {
    fn from(value: bool) -> Self {
        Self::Global(value)
    }
}

impl Not for ProductionConfig {
    type Output = bool;

    fn not(self) -> Self::Output {
        !self.any_enabled()
    }
}

impl ProductionConfig {
    #[must_use]
    pub const fn for_analysis(self, analysis: ProductionAnalysis) -> bool {
        match self {
            Self::Global(value) => value,
            Self::PerAnalysis(config) => match analysis {
                ProductionAnalysis::DeadCode => config.dead_code,
                ProductionAnalysis::Health => config.health,
                ProductionAnalysis::Dupes => config.dupes,
            },
        }
    }

    #[must_use]
    pub const fn global(self) -> bool {
        match self {
            Self::Global(value) => value,
            Self::PerAnalysis(_) => false,
        }
    }

    #[must_use]
    pub const fn any_enabled(self) -> bool {
        match self {
            Self::Global(value) => value,
            Self::PerAnalysis(config) => config.dead_code || config.health || config.dupes,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct PerAnalysisProductionConfig {
    /// When `production` is a per-analysis object, enables production mode for dead-code analysis only (boolean, default false): unused-files/exports/dependencies detection excludes test/spec/story/dev files and forces `unused-dev-dependencies`/`unused-optional-dependencies` to `off`, while health and dupes stay on the full tree. Set it to scope production analysis to dead code independently.
    pub dead_code: bool,
    /// When `production` is a per-analysis object, enables production mode for the health/complexity analysis only (boolean, default false), so `fallow health` in combined `fallow` and `fallow audit` scores only shipped code (test/spec/story/dev files excluded) while dead-code and dupes stay on the full tree. Set it to scope production analysis to health independently.
    pub health: bool,
    /// When `production` is a per-analysis object, enables production mode for duplication analysis only (boolean, default false), so clone detection runs on shipped code only (test/spec/story/dev files excluded) while dead-code and health stay on the full tree. Set it to scope production analysis to dupes independently.
    pub dupes: bool,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditConfig {
    /// Selects which findings affect the `fallow audit` verdict: `new-only` (default) fails only on findings introduced by the current changeset (running a base-snapshot attribution pass), while `all` fails on every finding in changed files and skips that pass. Set to `all` to gate the full backlog in changed files; the `--gate` CLI flag overrides this.
    #[serde(default, skip_serializing_if = "AuditGate::is_default")]
    pub gate: AuditGate,

    /// Toggles styling analytics (CSS and CSS-in-JS) in the `fallow audit` health sub-pass; these findings are descriptive and verdict-neutral by default (they change the exit code only when a css-* rule is set to error). Defaults to on when unset; set `false` to skip styling analysis. The `--no-css` CLI flag forces it off regardless.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<bool>,

    /// Toggles the project-wide CSS reachability pass in `fallow audit`, whose cross-file findings are narrowed back to changed anchors. Defaults to on when unset and runs only when css analytics are enabled; set `false` to keep local styling analytics but skip the whole-project scan. The `--css-deep` flag re-enables it and `--no-css-deep` forces it off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css_deep: Option<bool>,

    /// Path to a saved dead-code baseline file (produced by `fallow dead-code --save-baseline`) that the audit's dead-code sub-analysis compares against, suppressing pre-existing dead-code issues. The `--dead-code-baseline` CLI flag overrides it and both resolve relative to the project root; each sub-analysis uses a distinct baseline format, so this is separate from `healthBaseline` and `dupesBaseline`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code_baseline: Option<String>,

    /// Path to a saved health/complexity baseline file (produced by `fallow health --save-baseline`) that the audit's health sub-analysis compares against, suppressing pre-existing complexity/health findings. The `--health-baseline` CLI flag overrides it and both resolve relative to the project root; its baseline format is distinct from the dead-code and dupes baselines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_baseline: Option<String>,

    /// Path to a saved duplication baseline file (produced by `fallow dupes --save-baseline`) that the audit's duplication sub-analysis compares clone groups against, suppressing pre-existing duplicate clones. The `--dupes-baseline` CLI flag overrides it and both resolve relative to the project root; its baseline format is distinct from the dead-code and health baselines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes_baseline: Option<String>,

    /// Garbage-collection threshold, in whole days, for the persistent reusable base-snapshot worktree caches `fallow audit` creates: entries older than this window are swept on each audit run. Set to control cache accumulation; `0` disables the sweep and unset defaults to 30 days. The `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS` environment variable overrides this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_max_age_days: Option<u32>,
}

impl AuditConfig {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.gate.is_default()
            && self.css.is_none()
            && self.css_deep.is_none()
            && self.dead_code_baseline.is_none()
            && self.health_baseline.is_none()
            && self.dupes_baseline.is_none()
            && self.cache_max_age_days.is_none()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AuditGate {
    #[default]
    NewOnly,
    All,
}

impl AuditGate {
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::NewOnly)
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegressionConfig {
    /// The saved per-issue-type issue counts that `--fail-on-regression` compares the current run against; the gate fails only when counts grow beyond the configured tolerance. Typically written by `--save-baseline` rather than hand-authored; each field (total_issues plus per-kind counts like unused_exports, boundary_violations, policy_violations) is an integer defaulting to 0 when omitted. Absent means no baseline is embedded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<RegressionBaseline>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegressionBaseline {
    #[serde(default)]
    pub total_issues: usize,
    #[serde(default)]
    pub unused_files: usize,
    #[serde(default)]
    pub unused_exports: usize,
    #[serde(default)]
    pub unused_types: usize,
    #[serde(default)]
    pub unused_dependencies: usize,
    #[serde(default)]
    pub unused_dev_dependencies: usize,
    #[serde(default)]
    pub unused_optional_dependencies: usize,
    #[serde(default)]
    pub unused_enum_members: usize,
    #[serde(default)]
    pub unused_class_members: usize,
    #[serde(default)]
    pub unresolved_imports: usize,
    #[serde(default)]
    pub unlisted_dependencies: usize,
    #[serde(default)]
    pub duplicate_exports: usize,
    #[serde(default)]
    pub circular_dependencies: usize,
    #[serde(default)]
    pub re_export_cycles: usize,
    #[serde(default)]
    pub type_only_dependencies: usize,
    #[serde(default)]
    pub test_only_dependencies: usize,
    #[serde(default)]
    pub dev_dependencies_in_production: usize,
    #[serde(default)]
    pub boundary_violations: usize,
    #[serde(default)]
    pub boundary_coverage_violations: usize,
    #[serde(default)]
    pub boundary_call_violations: usize,
    #[serde(default)]
    pub policy_violations: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_empty_collections() {
        let config = FallowConfig::default();
        assert!(config.schema.is_none());
        assert!(config.extends.is_empty());
        assert!(config.entry.is_empty());
        assert!(config.ignore_patterns.is_empty());
        assert!(config.framework.is_empty());
        assert!(config.workspaces.is_none());
        assert!(config.ignore_dependencies.is_empty());
        assert!(config.ignore_exports.is_empty());
        assert!(config.used_class_members.is_empty());
        assert!(config.plugins.is_empty());
        assert!(config.dynamically_loaded.is_empty());
        assert!(config.overrides.is_empty());
        assert!(config.public_packages.is_empty());
        assert_eq!(
            config.fix.catalog.delete_preceding_comments,
            CatalogPrecedingCommentPolicy::Auto
        );
        assert!(!config.production);
    }

    #[test]
    fn default_config_rules_are_error() {
        let config = FallowConfig::default();
        assert_eq!(config.rules.unused_files, Severity::Error);
        assert_eq!(config.rules.unused_exports, Severity::Error);
        assert_eq!(config.rules.unused_dependencies, Severity::Error);
    }

    #[test]
    fn default_config_duplicates_enabled() {
        let config = FallowConfig::default();
        assert!(config.duplicates.enabled);
        assert_eq!(config.duplicates.min_tokens, 50);
        assert_eq!(config.duplicates.min_lines, 5);
    }

    #[test]
    fn default_config_health_thresholds() {
        let config = FallowConfig::default();
        assert_eq!(config.health.max_cyclomatic, 20);
        assert_eq!(config.health.max_cognitive, 15);
    }

    #[test]
    fn deserialize_empty_json_object() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(config.entry.is_empty());
        assert!(!config.production);
    }

    #[test]
    fn deserialize_json_with_all_top_level_fields() {
        let json = r#"{
            "$schema": "https://fallow.dev/schema.json",
            "entry": ["src/main.ts"],
            "ignorePatterns": ["generated/**"],
            "ignoreDependencies": ["postcss"],
            "production": true,
            "plugins": ["custom-plugin.toml"],
            "rules": {"unused-files": "warn"},
            "duplicates": {"enabled": false},
            "health": {"maxCyclomatic": 30}
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.schema.as_deref(),
            Some("https://fallow.dev/schema.json")
        );
        assert_eq!(config.entry, vec!["src/main.ts"]);
        assert_eq!(config.ignore_patterns, vec!["generated/**"]);
        assert_eq!(config.ignore_dependencies, vec!["postcss"]);
        assert!(config.production);
        assert_eq!(config.plugins, vec!["custom-plugin.toml"]);
        assert_eq!(config.rules.unused_files, Severity::Warn);
        assert!(!config.duplicates.enabled);
        assert_eq!(config.health.max_cyclomatic, 30);
    }

    #[test]
    fn deserialize_json_deny_unknown_fields() {
        let json = r#"{"unknownField": true}"#;
        let result: Result<FallowConfig, _> = serde_json::from_str(json);
        assert!(result.is_err(), "unknown fields should be rejected");
    }

    #[test]
    fn deserialize_json_production_mode_default_false() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.production);
    }

    #[test]
    fn deserialize_json_production_mode_true() {
        let config: FallowConfig = serde_json::from_str(r#"{"production": true}"#).unwrap();
        assert!(config.production);
    }

    #[test]
    fn deserialize_json_per_analysis_production_mode() {
        let config: FallowConfig = serde_json::from_str(
            r#"{"production": {"deadCode": false, "health": true, "dupes": false}}"#,
        )
        .unwrap();
        assert!(!config.production.for_analysis(ProductionAnalysis::DeadCode));
        assert!(config.production.for_analysis(ProductionAnalysis::Health));
        assert!(!config.production.for_analysis(ProductionAnalysis::Dupes));
    }

    #[test]
    fn deserialize_json_per_analysis_production_mode_rejects_unknown_fields() {
        let err = serde_json::from_str::<FallowConfig>(r#"{"production": {"healthTypo": true}}"#)
            .unwrap_err();
        assert!(
            err.to_string().contains("healthTypo"),
            "error should name the unknown field: {err}"
        );
    }

    #[test]
    fn deserialize_json_dynamically_loaded() {
        let json = r#"{"dynamicallyLoaded": ["plugins/**/*.ts", "locales/**/*.json"]}"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.dynamically_loaded,
            vec!["plugins/**/*.ts", "locales/**/*.json"]
        );
    }

    #[test]
    fn deserialize_json_dynamically_loaded_defaults_empty() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(config.dynamically_loaded.is_empty());
    }

    #[test]
    fn deserialize_json_fix_catalog_delete_preceding_comments() {
        let config: FallowConfig =
            serde_json::from_str(r#"{"fix": {"catalog": {"deletePrecedingComments": "always"}}}"#)
                .unwrap();
        assert_eq!(
            config.fix.catalog.delete_preceding_comments,
            CatalogPrecedingCommentPolicy::Always
        );
    }

    #[test]
    fn deserialize_json_fix_catalog_delete_preceding_comments_rejects_unknown_policy() {
        let err = serde_json::from_str::<FallowConfig>(
            r#"{"fix": {"catalog": {"deletePrecedingComments": "sometimes"}}}"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("sometimes"),
            "error should name the bad policy: {err}"
        );
    }

    #[test]
    fn deserialize_json_used_class_members_supports_strings_and_scoped_rules() {
        let json = r#"{
            "usedClassMembers": [
                "agInit",
                { "implements": "ICellRendererAngularComp", "members": ["refresh"] },
                { "extends": "BaseCommand", "implements": "CanActivate", "members": ["execute"] }
            ]
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.used_class_members,
            vec![
                UsedClassMemberRule::from("agInit"),
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: None,
                    implements: Some("ICellRendererAngularComp".to_string()),
                    members: vec!["refresh".to_string()],
                }),
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: Some("BaseCommand".to_string()),
                    implements: Some("CanActivate".to_string()),
                    members: vec!["execute".to_string()],
                }),
            ]
        );
    }

    #[test]
    fn deserialize_toml_minimal() {
        let toml_str = r#"
entry = ["src/index.ts"]
production = true
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.entry, vec!["src/index.ts"]);
        assert!(config.production);
    }

    #[test]
    fn workspaces_packages_key_is_accepted_as_patterns_alias() {
        // An older `fallow init --toml` wrote `[workspaces]` with a `packages`
        // key; the back-compat serde alias keeps those existing configs scoping
        // instead of silently dropping the (unknown) key and losing the patterns.
        let config: FallowConfig =
            toml::from_str("[workspaces]\npackages = [\"packages/*\", \"apps/*\"]").unwrap();
        assert_eq!(
            config.workspaces.map(|w| w.patterns).unwrap_or_default(),
            vec!["packages/*".to_string(), "apps/*".to_string()],
            "the `packages` alias must populate `patterns`"
        );
    }

    #[test]
    fn deserialize_toml_per_analysis_production_mode() {
        let toml_str = r"
[production]
deadCode = false
health = true
dupes = false
";
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.production.for_analysis(ProductionAnalysis::DeadCode));
        assert!(config.production.for_analysis(ProductionAnalysis::Health));
        assert!(!config.production.for_analysis(ProductionAnalysis::Dupes));
    }

    #[test]
    fn deserialize_toml_per_analysis_production_mode_rejects_unknown_fields() {
        let err = toml::from_str::<FallowConfig>(
            r"
[production]
healthTypo = true
",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("healthTypo"),
            "error should name the unknown field: {err}"
        );
    }

    #[test]
    fn deserialize_toml_with_inline_framework() {
        let toml_str = r#"
[[framework]]
name = "my-framework"
enablers = ["my-framework-pkg"]
entryPoints = ["src/routes/**/*.tsx"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.framework.len(), 1);
        assert_eq!(config.framework[0].name, "my-framework");
        assert_eq!(config.framework[0].enablers, vec!["my-framework-pkg"]);
        assert_eq!(
            config.framework[0].entry_points,
            vec!["src/routes/**/*.tsx"]
        );
    }

    #[test]
    fn deserialize_toml_fix_catalog_delete_preceding_comments() {
        let toml_str = r#"
[fix.catalog]
deletePrecedingComments = "never"
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.fix.catalog.delete_preceding_comments,
            CatalogPrecedingCommentPolicy::Never
        );
    }

    #[test]
    fn deserialize_toml_with_workspace_config() {
        let toml_str = r#"
[workspaces]
patterns = ["packages/*", "apps/*"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert!(config.workspaces.is_some());
        let ws = config.workspaces.unwrap();
        assert_eq!(ws.patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn deserialize_toml_with_ignore_exports() {
        let toml_str = r#"
[[ignoreExports]]
file = "src/types/**/*.ts"
exports = ["*"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.ignore_exports.len(), 1);
        assert_eq!(config.ignore_exports[0].file, "src/types/**/*.ts");
        assert_eq!(config.ignore_exports[0].exports, vec!["*"]);
    }

    #[test]
    fn deserialize_toml_used_class_members_supports_scoped_rules() {
        let toml_str = r#"
usedClassMembers = [
  { implements = "ICellRendererAngularComp", members = ["refresh"] },
  { extends = "BaseCommand", members = ["execute"] },
]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.used_class_members,
            vec![
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: None,
                    implements: Some("ICellRendererAngularComp".to_string()),
                    members: vec!["refresh".to_string()],
                }),
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: Some("BaseCommand".to_string()),
                    implements: None,
                    members: vec!["execute".to_string()],
                }),
            ]
        );
    }

    #[test]
    fn deserialize_json_used_class_members_rejects_unconstrained_scoped_rules() {
        let result = serde_json::from_str::<FallowConfig>(
            r#"{"usedClassMembers":[{"members":["refresh"]}]}"#,
        );
        assert!(
            result.is_err(),
            "unconstrained scoped rule should be rejected"
        );
    }

    #[test]
    fn deserialize_ignore_exports_used_in_file_bool() {
        let config: FallowConfig =
            serde_json::from_str(r#"{"ignoreExportsUsedInFile":true}"#).unwrap();

        assert!(config.ignore_exports_used_in_file.suppresses(false));
        assert!(config.ignore_exports_used_in_file.suppresses(true));
    }

    #[test]
    fn deserialize_ignore_exports_used_in_file_kind_form() {
        let config: FallowConfig =
            serde_json::from_str(r#"{"ignoreExportsUsedInFile":{"type":true}}"#).unwrap();

        assert!(!config.ignore_exports_used_in_file.suppresses(false));
        assert!(config.ignore_exports_used_in_file.suppresses(true));
    }

    #[test]
    fn deserialize_toml_deny_unknown_fields() {
        let toml_str = r"bogus_field = true";
        let result: Result<FallowConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "unknown fields should be rejected");
    }

    #[test]
    fn json_serialize_roundtrip() {
        let config = FallowConfig {
            entry: vec!["src/main.ts".to_string()],
            production: true.into(),
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: FallowConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.entry, vec!["src/main.ts"]);
        assert!(restored.production);
    }

    #[test]
    fn schema_field_not_serialized() {
        let config = FallowConfig {
            schema: Some("https://example.com/schema.json".to_string()),
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            !json.contains("$schema"),
            "schema field should be skipped in serialization"
        );
    }

    #[test]
    fn extends_field_not_serialized() {
        let config = FallowConfig {
            extends: vec!["base.json".to_string()],
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            !json.contains("extends"),
            "extends field should be skipped in serialization"
        );
    }

    #[test]
    fn regression_config_deserialize_json() {
        let json = r#"{
            "regression": {
                "baseline": {
                    "totalIssues": 42,
                    "unusedFiles": 10,
                    "unusedExports": 5,
                    "circularDependencies": 2
                }
            }
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        let regression = config.regression.unwrap();
        let baseline = regression.baseline.unwrap();
        assert_eq!(baseline.total_issues, 42);
        assert_eq!(baseline.unused_files, 10);
        assert_eq!(baseline.unused_exports, 5);
        assert_eq!(baseline.circular_dependencies, 2);
        assert_eq!(baseline.unused_types, 0);
        assert_eq!(baseline.boundary_violations, 0);
    }

    #[test]
    fn regression_config_defaults_to_none() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(config.regression.is_none());
    }

    #[test]
    fn regression_baseline_all_zeros_by_default() {
        let baseline = RegressionBaseline::default();
        assert_eq!(baseline.total_issues, 0);
        assert_eq!(baseline.unused_files, 0);
        assert_eq!(baseline.unused_exports, 0);
        assert_eq!(baseline.unused_types, 0);
        assert_eq!(baseline.unused_dependencies, 0);
        assert_eq!(baseline.unused_dev_dependencies, 0);
        assert_eq!(baseline.unused_optional_dependencies, 0);
        assert_eq!(baseline.unused_enum_members, 0);
        assert_eq!(baseline.unused_class_members, 0);
        assert_eq!(baseline.unresolved_imports, 0);
        assert_eq!(baseline.unlisted_dependencies, 0);
        assert_eq!(baseline.duplicate_exports, 0);
        assert_eq!(baseline.circular_dependencies, 0);
        assert_eq!(baseline.type_only_dependencies, 0);
        assert_eq!(baseline.test_only_dependencies, 0);
        assert_eq!(baseline.boundary_violations, 0);
    }

    #[test]
    fn regression_config_serialize_roundtrip() {
        let baseline = RegressionBaseline {
            total_issues: 100,
            unused_files: 20,
            unused_exports: 30,
            ..RegressionBaseline::default()
        };
        let regression = RegressionConfig {
            baseline: Some(baseline),
        };
        let config = FallowConfig {
            regression: Some(regression),
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: FallowConfig = serde_json::from_str(&json).unwrap();
        let restored_baseline = restored.regression.unwrap().baseline.unwrap();
        assert_eq!(restored_baseline.total_issues, 100);
        assert_eq!(restored_baseline.unused_files, 20);
        assert_eq!(restored_baseline.unused_exports, 30);
        assert_eq!(restored_baseline.unused_types, 0);
    }

    #[test]
    fn regression_config_empty_baseline_deserialize() {
        let json = r#"{"regression": {}}"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        let regression = config.regression.unwrap();
        assert!(regression.baseline.is_none());
    }

    #[test]
    fn regression_baseline_not_serialized_when_none() {
        let config = FallowConfig {
            regression: None,
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(
            !json.contains("regression"),
            "regression should be skipped when None"
        );
    }

    #[test]
    fn deserialize_json_with_overrides() {
        let json = r#"{
            "overrides": [
                {
                    "files": ["*.test.ts", "*.spec.ts"],
                    "rules": {
                        "unused-exports": "off",
                        "unused-files": "warn"
                    }
                }
            ]
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.overrides.len(), 1);
        assert_eq!(config.overrides[0].files.len(), 2);
        assert_eq!(
            config.overrides[0].rules.unused_exports,
            Some(Severity::Off)
        );
        assert_eq!(config.overrides[0].rules.unused_files, Some(Severity::Warn));
    }

    #[test]
    fn deserialize_json_with_boundaries() {
        let json = r#"{
            "boundaries": {
                "preset": "layered"
            }
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.boundaries.preset, Some(BoundaryPreset::Layered));
    }

    #[test]
    fn deserialize_toml_with_regression_baseline() {
        let toml_str = r"
[regression.baseline]
totalIssues = 50
unusedFiles = 10
unusedExports = 15
";
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        let baseline = config.regression.unwrap().baseline.unwrap();
        assert_eq!(baseline.total_issues, 50);
        assert_eq!(baseline.unused_files, 10);
        assert_eq!(baseline.unused_exports, 15);
    }

    #[test]
    fn deserialize_toml_with_overrides() {
        let toml_str = r#"
[[overrides]]
files = ["*.test.ts"]

[overrides.rules]
unused-exports = "off"

[[overrides]]
files = ["*.stories.tsx"]

[overrides.rules]
unused-files = "off"
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.overrides.len(), 2);
        assert_eq!(
            config.overrides[0].rules.unused_exports,
            Some(Severity::Off)
        );
        assert_eq!(config.overrides[1].rules.unused_files, Some(Severity::Off));
    }

    #[test]
    fn regression_config_default_is_none_baseline() {
        let config = RegressionConfig::default();
        assert!(config.baseline.is_none());
    }

    #[test]
    fn deserialize_json_multiple_ignore_export_rules() {
        let json = r#"{
            "ignoreExports": [
                {"file": "src/types/**/*.ts", "exports": ["*"]},
                {"file": "src/constants.ts", "exports": ["FOO", "BAR"]},
                {"file": "src/index.ts", "exports": ["default"]}
            ]
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.ignore_exports.len(), 3);
        assert_eq!(config.ignore_exports[2].exports, vec!["default"]);
    }

    #[test]
    fn deserialize_json_public_packages_camel_case() {
        let json = r#"{"publicPackages": ["@myorg/shared-lib", "@myorg/utils"]}"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.public_packages,
            vec!["@myorg/shared-lib", "@myorg/utils"]
        );
    }

    #[test]
    fn deserialize_json_public_packages_rejects_snake_case() {
        let json = r#"{"public_packages": ["@myorg/shared-lib"]}"#;
        let result: Result<FallowConfig, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "snake_case should be rejected by deny_unknown_fields + rename_all camelCase"
        );
    }

    #[test]
    fn deserialize_json_public_packages_empty() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(config.public_packages.is_empty());
    }

    #[test]
    fn deserialize_toml_public_packages() {
        let toml_str = r#"
publicPackages = ["@myorg/shared-lib", "@myorg/ui"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.public_packages,
            vec!["@myorg/shared-lib", "@myorg/ui"]
        );
    }

    #[test]
    fn public_packages_serialize_roundtrip() {
        let config = FallowConfig {
            public_packages: vec!["@myorg/shared-lib".to_string()],
            ..FallowConfig::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: FallowConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.public_packages, vec!["@myorg/shared-lib"]);
    }
}
