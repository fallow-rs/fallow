use fallow_types::suppress::IssueKind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Severity level for rules.
///
/// Controls whether an issue type sets a non-zero exit code (`error`), is
/// reported without an effect on the exit code (`warn`), or is suppressed
/// entirely (`off`). `--fail-on-issues` promotes `warn` to `error`. The
/// dead-code count gate of the GitHub Action counts `warn` findings unless its
/// `fail-on-issues` input is `false`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Report and set a non-zero exit code.
    #[default]
    Error,
    /// Report without an effect on the exit code.
    Warn,
    /// Don't detect or report.
    Off,
}

impl Severity {
    /// Default value for fields that should default to `Warn` instead of `Error`.
    const fn default_warn() -> Self {
        Self::Warn
    }

    /// Default value for fields that should default to `Off`.
    const fn default_off() -> Self {
        Self::Off
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warn => write!(f, "warn"),
            Self::Off => write!(f, "off"),
        }
    }
}

impl std::str::FromStr for Severity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "error" => Ok(Self::Error),
            "warn" | "warning" => Ok(Self::Warn),
            "off" | "none" => Ok(Self::Off),
            other => Err(format!(
                "unknown severity: '{other}' (expected error, warn, or off)"
            )),
        }
    }
}

/// Per-issue-type severity configuration.
///
/// Controls which issue types cause CI failure, are reported as warnings,
/// or are suppressed entirely. Most fields default to `Severity::Error`.
///
/// Rule names use kebab-case in config files (e.g., `"unused-files": "error"`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct RulesConfig {
    /// A file reachable from no entry point. Defaults to `error`.
    #[serde(default, alias = "unused-file")]
    pub unused_files: Severity,
    /// An exported symbol no other module imports. Defaults to `error`.
    #[serde(default, alias = "unused-export")]
    pub unused_exports: Severity,
    /// An exported type no other module uses. Defaults to `error`.
    #[serde(default, alias = "unused-type")]
    pub unused_types: Severity,
    /// An exported signature referencing a same-file private type. Opt-in;
    /// defaults to `off`.
    #[serde(default = "Severity::default_off", alias = "private-type-leak")]
    pub private_type_leaks: Severity,
    /// Whether the user explicitly configured `private-type-leaks` (rather
    /// than the field taking its `off` default). Populated during config file
    /// loading, not by serde: type-aware hosts default this opt-in rule to
    /// `warn`, and this flag lets an explicit user `off` win over that
    /// default (issue #2170).
    ///
    /// Because the field is `serde(skip)`, any serialize/deserialize
    /// round-trip of [`FallowConfig`](crate::FallowConfig) silently resets it
    /// to `false`, which would re-enable the type-aware `warn` default for a
    /// user who explicitly set the rule to `off`. Do not route configs through
    /// a serde round-trip after [`FallowConfig::load`](crate::FallowConfig::load)
    /// without re-recording this flag.
    #[serde(skip)]
    pub private_type_leaks_configured: bool,
    /// An export marked `@deprecated` that still has at least one reachable
    /// reference. Opt-in; defaults to `off`. A per-path override resolves on
    /// the file that declares the export, not on the consumer.
    #[serde(default = "Severity::default_off", alias = "deprecated-export-in-use")]
    pub deprecated_exports_in_use: Severity,
    /// A declared `dependencies` entry never observed used. Defaults to
    /// `error`.
    #[serde(default, alias = "unused-dependency")]
    pub unused_dependencies: Severity,
    /// A declared `devDependencies` entry never observed used. Defaults to
    /// `warn`; production mode forces it to `off`.
    #[serde(default = "Severity::default_warn", alias = "unused-dev-dependency")]
    pub unused_dev_dependencies: Severity,
    /// A declared `optionalDependencies` entry never observed used. Defaults
    /// to `warn`; production mode forces it to `off`.
    #[serde(
        default = "Severity::default_warn",
        alias = "unused-optional-dependency"
    )]
    pub unused_optional_dependencies: Severity,
    /// An enum member read nowhere in the project. Defaults to `error`.
    #[serde(default, alias = "unused-enum-member")]
    pub unused_enum_members: Severity,
    /// A class member used nowhere; `usedClassMembers` and decorator
    /// exemptions carve out framework-invoked members. Defaults to `error`.
    #[serde(default, alias = "unused-class-member")]
    pub unused_class_members: Severity,
    /// Store members (Pinia `state` / `getters` / `actions` key, or a
    /// setup-store returned key) declared but never accessed by any consumer
    /// project-wide. Defaults to `warn`, not `error` like the closed-set
    /// class/enum member rules: a store has an OPEN declaration surface
    /// (plugins, `$onAction`, dynamic dispatch) so analyzer confidence is
    /// genuinely lower. A `warn` finding does not change the CLI exit code
    /// unless `--fail-on-issues` is set. The dead-code count gate of the GitHub
    /// Action counts it unless `fail-on-issues` is `false`. Promotable to
    /// `error` once validated on a codebase.
    #[serde(default = "Severity::default_warn", alias = "unused-store-member")]
    pub unused_store_members: Severity,
    /// Vue `inject(KEY)` / Svelte `getContext(KEY)` whose symbol KEY is
    /// `provide`/`setContext`'d nowhere in the project (the
    /// injected-never-provided dead-half). Defaults to `warn`, not `error`: a
    /// DI key has an open provide surface (plugins, app-level provide) so
    /// analyzer confidence is lower. A `warn` finding does not change the CLI
    /// exit code unless `--fail-on-issues` is set. The dead-code count gate of
    /// the GitHub Action counts it unless `fail-on-issues` is `false`.
    #[serde(default = "Severity::default_warn", alias = "unprovided-inject")]
    pub unprovided_injects: Severity,
    /// Vue/Svelte single-file component reachable in the module graph but
    /// rendered nowhere in the project (the imported-but-never-rendered
    /// dead-half). Defaults to `warn`, not `error`: a component can be rendered
    /// reflectively (dynamic `<component :is>`), so analyzer confidence is
    /// lower. A `warn` finding does not change the CLI exit code unless
    /// `--fail-on-issues` is set. The dead-code count gate of the GitHub Action
    /// counts it unless `fail-on-issues` is `false`.
    #[serde(default = "Severity::default_warn", alias = "unrendered-component")]
    pub unrendered_components: Severity,
    /// Vue `<script setup>` `defineProps`, Svelte 5 `$props()`, or React
    /// declared prop referenced nowhere inside its own component. The
    /// single-component dead-input direction. Defaults to `warn`, not `error`:
    /// a prop can be part of a deliberately-stable public component API, so
    /// analyzer confidence is lower. A `warn` finding does not change the CLI
    /// exit code unless `--fail-on-issues` is set. The dead-code count gate of
    /// the GitHub Action counts it unless `fail-on-issues` is `false`.
    #[serde(default = "Severity::default_warn", alias = "unused-component-prop")]
    pub unused_component_props: Severity,
    /// Used optional inputs absent from inspected callers. Requires manual review; off by default.
    #[serde(default = "Severity::default_off", alias = "absent-component-prop")]
    pub absent_component_props: Severity,
    /// Vue `<script setup>` `defineEmits` declared event emitted nowhere inside
    /// its own single-file component (no `emit('<name>')` call). The
    /// single-file dead-input direction. Defaults to `warn`, not `error`: an
    /// emit can be part of a deliberately-stable public component API, so
    /// analyzer confidence is lower. A `warn` finding does not change the CLI
    /// exit code unless `--fail-on-issues` is set. The dead-code count gate of
    /// the GitHub Action counts it unless `fail-on-issues` is `false`.
    #[serde(default = "Severity::default_warn", alias = "unused-component-emit")]
    pub unused_component_emits: Severity,
    /// Angular `@Input()` / signal `input()` / `model()` declared input read
    /// nowhere inside its own component (neither the inline/external template
    /// nor the class body). The single-file dead-input direction, the Angular
    /// analogue of `unused-component-prop`. Defaults to `warn`, not `error`: an
    /// input can be part of a deliberately-stable public component API, so
    /// analyzer confidence is lower. A `warn` finding does not change the CLI
    /// exit code unless `--fail-on-issues` is set. The dead-code count gate of
    /// the GitHub Action counts it unless `fail-on-issues` is `false`.
    #[serde(default = "Severity::default_warn", alias = "unused-component-input")]
    pub unused_component_inputs: Severity,
    /// Angular `@Output()` / signal `output()` declared output emitted nowhere
    /// inside its own component (no `this.<output>.emit(...)`). The single-file
    /// dead-output direction, the Angular analogue of `unused-component-emit`.
    /// Defaults to `warn`, not `error`: an output can be part of a
    /// deliberately-stable public component API, so analyzer confidence is
    /// lower. A `warn` finding does not change the CLI exit code unless
    /// `--fail-on-issues` is set. The dead-code count gate of the GitHub Action
    /// counts it unless `fail-on-issues` is `false`.
    #[serde(default = "Severity::default_warn", alias = "unused-component-output")]
    pub unused_component_outputs: Severity,
    /// Svelte component dispatching a custom event via
    /// `createEventDispatcher()` whose event name is listened to nowhere in the
    /// analyzed project. The cross-file dead-output direction (no
    /// eslint-plugin-svelte / svelte-check rule covers the listener side).
    /// Defaults to `warn`, not `error`: a dispatched event can be part of a
    /// deliberately-stable public component API, or a listener may be added
    /// later, so analyzer confidence is lower. A `warn` finding does not change
    /// the CLI exit code unless `--fail-on-issues` is set. The dead-code count
    /// gate of the GitHub Action counts it unless `fail-on-issues` is `false`.
    #[serde(default = "Severity::default_warn", alias = "unused-svelte-event")]
    pub unused_svelte_events: Severity,
    /// Effect layer composed only into other layers that nothing uses. Gated
    /// on the project declaring `effect`. Defaults to `warn`: code outside the
    /// analyzed project can still compose the layer.
    #[serde(default = "Severity::default_warn", alias = "unused-effect-layer")]
    pub unused_effect_layers: Severity,
    /// Effect tagged error class that only type positions or `catchTag`
    /// handlers reference. Gated on the project declaring `effect`. Defaults
    /// to `warn`: consumers outside the project can construct the error.
    #[serde(default = "Severity::default_warn", alias = "unused-tagged-error")]
    pub unused_tagged_errors: Severity,
    /// `catchTag` handler for a project tag that no code produces, when no
    /// `unused-tagged-errors` finding covers it. Gated on the project
    /// declaring `effect`. Defaults to `warn`.
    #[serde(default = "Severity::default_warn", alias = "unproduced-catch-tag")]
    pub unproduced_catch_tags: Severity,
    /// Next.js Server Action (an export of a `"use server"` file) referenced by
    /// no code in the project: no import-and-call, no `action={fn}` binding, no
    /// `<form action={fn}>`. Cross-graph dead-export direction, reclassified
    /// out of `unused-export` for `"use server"` files. Defaults to `warn`, not
    /// `error`: the rule is new and false-negative-preferring, and reflective
    /// action-dispatch shapes can hide a real consumer. The rule stays at
    /// `warn` until corpus validation. A `warn` finding does not change the CLI
    /// exit code unless `--fail-on-issues` is set. The dead-code count gate of
    /// the GitHub Action counts it unless `fail-on-issues` is `false`.
    #[serde(default = "Severity::default_warn", alias = "unused-server-action")]
    pub unused_server_actions: Severity,
    /// SvelteKit `+page.{ts,server.ts,js,server.js}` `load()` return-object key
    /// read by no consumer: not off the sibling `+page.svelte`'s `data.<key>`,
    /// nor project-wide via `page.data.<key>` / `$page.data.<key>`. Cross-file
    /// dead-input direction. Defaults to `warn`, not `error`: the rule is new
    /// and false-negative-preferring (a whole-object `data` pass abstains), and
    /// a load fetch can have side effects so deletion is a human call. The rule
    /// stays at `warn` until corpus validation. A `warn` finding does not
    /// change the CLI exit code unless `--fail-on-issues` is set. The dead-code
    /// count gate of the GitHub Action counts it unless `fail-on-issues` is
    /// `false`.
    #[serde(default = "Severity::default_warn", alias = "unused-load-data-key")]
    pub unused_load_data_keys: Severity,
    /// React/Preact prop forwarded unchanged through `>= N` intermediate
    /// pass-through components until a component that substantively consumes it.
    /// A graph-derived health signal. Defaults to `off` (opt-in), like
    /// `private-type-leak` / `security-*`: the located per-chain records and the
    /// small capped health penalty are dormant until the user enables the rule.
    /// `error` enforces the finding in CI; `warn` stays advisory even under
    /// strict flags. Overrides resolve for the source hop and can enable the
    /// rule while the base rule is off.
    #[serde(default = "Severity::default_off", alias = "prop-drilling")]
    pub prop_drilling: Severity,
    /// A React/Preact component whose entire body is `return <Child {...props}/>`
    /// (pure structural indirection, a candidate for inlining). A graph-derived
    /// health signal. Defaults to `off` (opt-in), like `prop-drilling`: the
    /// located per-wrapper records are dormant until the user enables the rule.
    #[serde(default = "Severity::default_off", alias = "thin-wrapper")]
    pub thin_wrapper: Severity,
    /// Three or more React/Preact components across two or more files whose
    /// statically-harvested prop NAME set is identical after stripping ubiquitous
    /// DOM / passthrough names (a missing shared `Props` type / base component).
    /// A graph-derived structural-refactor health signal. Defaults to `off`
    /// (opt-in), like `thin-wrapper`: the located per-component records are
    /// dormant until the user enables the rule.
    #[serde(default = "Severity::default_off", alias = "duplicate-prop-shape")]
    pub duplicate_prop_shape: Severity,
    /// A CSS / CSS-in-JS design-token DRIFT finding (a hardcoded value where a
    /// design token exists, e.g. a Tailwind arbitrary value). A styling-domain
    /// advisory surfaced in `fallow audit`; defaults to `warn` (verdict-neutral).
    /// Set to `error` to gate CI on styling drift, or `off` to silence.
    #[serde(default = "Severity::default_warn", alias = "css-token-drift")]
    pub css_token_drift: Severity,
    /// A CSS / CSS-in-JS DUPLICATE declaration block (copy-pasted rule body).
    /// A styling-domain advisory surfaced in `fallow audit`; defaults to `warn`
    /// (verdict-neutral). Set to `error` to gate, or `off` to silence.
    #[serde(default = "Severity::default_warn", alias = "css-duplicate-block")]
    pub css_duplicate_block: Severity,
    /// CSS selector / nesting / important-density complexity. A styling-domain
    /// advisory surfaced in `fallow audit`; defaults to `warn`
    /// (verdict-neutral). Set to `error` to gate, or `off` to silence.
    #[serde(default = "Severity::default_warn", alias = "css-selector-complexity")]
    pub css_selector_complexity: Severity,
    /// CSS dead surface, such as unused scoped SFC classes. A styling-domain
    /// advisory surfaced in `fallow audit`; defaults to `warn`
    /// (verdict-neutral). Set to `error` to gate, or `off` to silence.
    #[serde(default = "Severity::default_warn", alias = "css-dead-surface")]
    pub css_dead_surface: Severity,
    /// CSS broken references, such as missing classes or keyframes. A
    /// styling-domain advisory surfaced by deep CSS audit mode; defaults
    /// to `warn` (verdict-neutral). Set to `error` to gate, or `off` to silence.
    #[serde(default = "Severity::default_warn", alias = "css-broken-reference")]
    pub css_broken_reference: Severity,
    /// A function above the cyclomatic ceiling (`health.maxCyclomatic` or a
    /// `thresholdOverrides` entry). The threshold decides if the finding
    /// exists; this rule decides if it fails the run. Defaults to `error`.
    /// `warn` reports the finding without a failure, and `off` hides it. A
    /// finding above several ceilings takes the most severe of their rules.
    /// The rule applies before the `health --min-severity` band gate.
    /// After you set a kind to `off`, save the health baseline again: its
    /// entries for the hidden findings no longer match.
    #[serde(default)]
    pub complexity_cyclomatic: Severity,
    /// A function above the cognitive ceiling (`health.maxCognitive` or a
    /// `thresholdOverrides` entry). The threshold decides if the finding
    /// exists; this rule decides if it fails the run. Defaults to `error`.
    #[serde(default)]
    pub complexity_cognitive: Severity,
    /// A function above the CRAP ceiling (`health.maxCrap` or a
    /// `thresholdOverrides` entry). The threshold decides if the finding
    /// exists; this rule decides if it fails the run. Defaults to `error`.
    /// `off` hides the findings only. `health.maxCrap: 0` also turns off the
    /// threshold-relative file-score signals.
    #[serde(default)]
    pub complexity_crap: Severity,
    /// A file above the file-length ceiling (`health.maxFileLines` or a
    /// `thresholdOverrides` entry). The threshold decides if the finding
    /// exists, and it is off by default; this rule decides if it fails the
    /// run. Defaults to `error`.
    #[serde(default)]
    pub complexity_file_lines: Severity,
    /// An import specifier that resolves to no file or package. Defaults to
    /// `error`.
    #[serde(default, alias = "unresolved-import")]
    pub unresolved_imports: Severity,
    /// An imported package not declared in any relevant `package.json`.
    /// Defaults to `error`.
    #[serde(default, alias = "unlisted-dependency")]
    pub unlisted_dependencies: Severity,
    /// The same export name provided by multiple modules; `ignoreExports`
    /// excludes intentional barrel re-exports. Defaults to `error`.
    #[serde(default, alias = "duplicate-export")]
    pub duplicate_exports: Severity,
    /// A production dependency imported only via type-only imports (a
    /// `devDependencies` candidate). Defaults to `warn`.
    #[serde(default = "Severity::default_warn", alias = "type-only-dependency")]
    pub type_only_dependencies: Severity,
    /// A production dependency imported only by test files. Defaults to
    /// `warn`.
    #[serde(default = "Severity::default_warn", alias = "test-only-dependency")]
    pub test_only_dependencies: Severity,
    /// A `devDependencies` entry imported by production code, which a
    /// production-only install would omit and break at runtime. Defaults to
    /// `warn`.
    #[serde(
        default = "Severity::default_warn",
        alias = "dev-dependency-in-production"
    )]
    pub dev_dependencies_in_production: Severity,
    /// A circular import chain between modules. Defaults to `error`.
    #[serde(default, alias = "circular-dependency")]
    pub circular_dependencies: Severity,
    /// A cycle or self-loop in the re-export subgraph (barrel files
    /// re-exporting from each other in a loop). Defaults to `warn`.
    #[serde(
        default = "Severity::default_warn",
        alias = "re-export-cycles",
        alias = "reexport-cycle",
        alias = "reexport-cycles"
    )]
    pub re_export_cycle: Severity,
    /// A dependency cycle between workspace packages, built from resolved
    /// cross-package imports. Defaults to `warn`.
    #[serde(default = "Severity::default_warn", alias = "package-cycles")]
    pub package_cycle: Severity,
    /// An import crossing a forbidden architecture-boundary edge declared in
    /// the `boundaries` config. Defaults to `error`.
    #[serde(default, alias = "boundary-violations")]
    pub boundary_violation: Severity,
    /// A runtime file or export with no test dependency path. Opt-in;
    /// defaults to `off`.
    #[serde(default = "Severity::default_off", alias = "coverage-gap")]
    pub coverage_gaps: Severity,
    /// A detected feature-flag pattern (tuned via the `flags` config).
    /// Opt-in; defaults to `off`.
    #[serde(default = "Severity::default_off", alias = "feature-flag")]
    pub feature_flags: Severity,
    /// A `fallow-ignore` comment or `@expected-unused` tag that no longer
    /// matches any issue. Defaults to `warn`.
    #[serde(default = "Severity::default_warn", alias = "stale-suppression")]
    pub stale_suppressions: Severity,
    /// Opt-in suppression hygiene rule: when enabled, every `fallow-ignore-*`
    /// comment and `@expected-unused` tag must carry a `-- <reason>` suffix.
    #[serde(default = "Severity::default_off", alias = "suppression-reason")]
    pub require_suppression_reason: Severity,
    /// A `pnpm-workspace.yaml` catalog entry referenced by no workspace
    /// package. Defaults to `warn`.
    #[serde(default = "Severity::default_warn", alias = "unused-catalog-entry")]
    pub unused_catalog_entries: Severity,
    /// A named pnpm catalog group declaring no entries. Defaults to `warn`.
    #[serde(default = "Severity::default_warn", alias = "empty-catalog-group")]
    pub empty_catalog_groups: Severity,
    /// A workspace `package.json` `catalog:` / `catalog:<name>` reference
    /// pointing at a catalog that does not declare the consumed package.
    /// Defaults to `error`; suppressible only via `ignoreCatalogReferences`.
    #[serde(default, alias = "unresolved-catalog-reference")]
    pub unresolved_catalog_references: Severity,
    /// A pnpm, npm, or Bun override entry whose target package no workspace
    /// `package.json` declares and the active readable lockfile does not
    /// resolve. Defaults to `warn`.
    #[serde(
        default = "Severity::default_warn",
        alias = "unused-dependency-override"
    )]
    pub unused_dependency_overrides: Severity,
    /// A pnpm, npm, or Bun override or Bun `resolutions` entry whose key or
    /// value cannot be parsed in its declaration source's grammar. Defaults to
    /// `error`.
    #[serde(default, alias = "misconfigured-dependency-override")]
    pub misconfigured_dependency_overrides: Severity,
    /// Opt-in (default off): a `"use client"` file that transitively imports a
    /// module reading a non-public `process.env` secret. Surfaced only by
    /// `fallow security`; never under bare `fallow` or the `audit` gate.
    #[serde(default = "Severity::default_off")]
    pub security_client_server_leak: Severity,
    /// Opt-in (default off): a syntactic tainted-sink candidate matched against
    /// the data-driven catalogue (`security_matchers.toml`). ONE knob gates ALL
    /// catalogue categories. Surfaced only by `fallow security`; never under
    /// bare `fallow` or the `audit` gate.
    #[serde(default = "Severity::default_off")]
    pub security_sink: Severity,
    /// Master severity for rule-pack findings (`rulePacks` config). Defaults
    /// to `warn` so enabling a brand-new policy pack never hard-fails CI on
    /// its first run; individual pack rules opt up via `"severity": "error"`.
    /// `off` is a kill switch that disables the whole evaluator (per-rule
    /// severity cannot resurrect it).
    #[serde(default = "Severity::default_warn", alias = "policy-violations")]
    pub policy_violation: Severity,
    /// A `"use client"` file that exports a Next.js server-only /
    /// route-segment config name (e.g. `metadata`, `revalidate`, `GET`).
    /// Next.js rejects this at build time; fallow catches it statically.
    /// Defaults to `warn`.
    #[serde(default = "Severity::default_warn", alias = "invalid-client-exports")]
    pub invalid_client_export: Severity,
    /// A barrel file that re-exports BOTH a `"use client"` origin module AND a
    /// server-only origin module. Importing one name from such a barrel drags
    /// the other's directive context across the React Server Components
    /// boundary (the Next.js App Router footgun). Defaults to `warn`.
    #[serde(
        default = "Severity::default_warn",
        alias = "mixed-client-server-barrels"
    )]
    pub mixed_client_server_barrel: Severity,
    /// A `"use client"` / `"use server"` directive written as an expression
    /// statement after a non-directive statement (an import, a const), so the
    /// RSC bundler parses it as an ordinary string and silently ignores it.
    /// The intended client/server boundary never takes effect. Defaults to
    /// `warn`.
    #[serde(default = "Severity::default_warn", alias = "misplaced-directives")]
    pub misplaced_directive: Severity,
    /// Two or more Next.js App Router route files that resolve to the same URL
    /// within one app-root. Next.js fails the build ("You cannot have two
    /// parallel pages that resolve to the same path"); fallow catches it
    /// statically and names every colliding file. Defaults to `error`: the
    /// project already fails `next build`, so flagging it as an error aligns
    /// fallow's exit code with the build it mirrors.
    #[serde(default, alias = "route-collisions")]
    pub route_collision: Severity,
    /// Sibling Next.js dynamic route segments at one tree position using
    /// different param spellings (`[id]` vs `[slug]`). Next.js throws "You
    /// cannot use different slug names for the same dynamic path" at dev and
    /// production runtime when the position is hit; `next build` does NOT catch
    /// it (the build succeeds), so CI passes while the route crashes on its
    /// first request. fallow catches it statically. Defaults to `error`: the
    /// route is a deterministic runtime crash on first request, so failing CI
    /// is the honest signal even though `next build` stays green (this is the
    /// "error-runtime" severity tier, shared with `route-collision`).
    #[serde(default, alias = "dynamic-segment-name-conflicts")]
    pub dynamic_segment_name_conflict: Severity,
    /// An audit diff that weakens tests: a new skip or focus marker, removed
    /// test cases, fewer assertions, or a lowered coverage threshold.
    /// `fallow audit` only. Defaults to `warn`.
    #[serde(default = "Severity::default_warn")]
    pub weakened_tests: Severity,
    /// An audit diff that adds a lint, type-check, coverage or Fallow
    /// suppression comment. `fallow audit` only. Defaults to `warn`.
    #[serde(default = "Severity::default_warn", alias = "added-suppressions")]
    pub added_suppression: Severity,
    /// An audit diff that adds a test-environment branch to a non-test source
    /// file. `fallow audit` only. Defaults to `warn`.
    #[serde(default = "Severity::default_warn", alias = "test-only-branches")]
    pub test_only_branch: Severity,
    /// An audit diff that edits one copy of a base clone group so that it no
    /// longer matches its siblings, while no sibling changed. `fallow audit`
    /// only. Opt-in; defaults to `off`, because copies often differ on
    /// purpose.
    #[serde(default = "Severity::default_off", alias = "diverged-clone-siblings")]
    pub diverged_clone_sibling: Severity,
    /// An audit diff that adds a dependency to a `package.json`. `fallow
    /// audit` only. Opt-in; defaults to `off`.
    #[serde(default = "Severity::default_off", alias = "added-dependencies")]
    pub added_dependency: Severity,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            unused_files: Severity::Error,
            unused_exports: Severity::Error,
            unused_types: Severity::Error,
            private_type_leaks: Severity::Off,
            private_type_leaks_configured: false,
            deprecated_exports_in_use: Severity::Off,
            unused_dependencies: Severity::Error,
            unused_dev_dependencies: Severity::Warn,
            unused_optional_dependencies: Severity::Warn,
            unused_enum_members: Severity::Error,
            unused_class_members: Severity::Error,
            unused_store_members: Severity::Warn,
            unprovided_injects: Severity::Warn,
            unrendered_components: Severity::Warn,
            unused_component_props: Severity::Warn,
            absent_component_props: Severity::Off,
            unused_component_emits: Severity::Warn,
            unused_component_inputs: Severity::Warn,
            unused_component_outputs: Severity::Warn,
            unused_svelte_events: Severity::Warn,
            unused_effect_layers: Severity::Warn,
            unused_tagged_errors: Severity::Warn,
            unproduced_catch_tags: Severity::Warn,
            unused_server_actions: Severity::Warn,
            unused_load_data_keys: Severity::Warn,
            prop_drilling: Severity::Off,
            thin_wrapper: Severity::Off,
            duplicate_prop_shape: Severity::Off,
            css_token_drift: Severity::Warn,
            css_duplicate_block: Severity::Warn,
            css_selector_complexity: Severity::Warn,
            css_dead_surface: Severity::Warn,
            css_broken_reference: Severity::Warn,
            complexity_cyclomatic: Severity::Error,
            complexity_cognitive: Severity::Error,
            complexity_crap: Severity::Error,
            complexity_file_lines: Severity::Error,
            unresolved_imports: Severity::Error,
            unlisted_dependencies: Severity::Error,
            duplicate_exports: Severity::Error,
            type_only_dependencies: Severity::Warn,
            test_only_dependencies: Severity::Warn,
            dev_dependencies_in_production: Severity::Warn,
            circular_dependencies: Severity::Error,
            re_export_cycle: Severity::Warn,
            package_cycle: Severity::Warn,
            boundary_violation: Severity::Error,
            coverage_gaps: Severity::Off,
            feature_flags: Severity::Off,
            stale_suppressions: Severity::Warn,
            require_suppression_reason: Severity::Off,
            unused_catalog_entries: Severity::Warn,
            empty_catalog_groups: Severity::Warn,
            unresolved_catalog_references: Severity::Error,
            unused_dependency_overrides: Severity::Warn,
            misconfigured_dependency_overrides: Severity::Error,
            security_client_server_leak: Severity::Off,
            security_sink: Severity::Off,
            policy_violation: Severity::Warn,
            invalid_client_export: Severity::Warn,
            mixed_client_server_barrel: Severity::Warn,
            misplaced_directive: Severity::Warn,
            route_collision: Severity::Error,
            dynamic_segment_name_conflict: Severity::Error,
            weakened_tests: Severity::Warn,
            added_suppression: Severity::Warn,
            test_only_branch: Severity::Warn,
            diverged_clone_sibling: Severity::Off,
            added_dependency: Severity::Off,
        }
    }
}

macro_rules! apply_partial_rules {
    ($target:expr, $partial:expr, [$($field:ident),+ $(,)?]) => {
        $(
            if let Some(severity) = $partial.$field {
                $target.$field = severity;
            }
        )+
    };
}

impl RulesConfig {
    /// Map an [`IssueKind`] to its configured [`Severity`] in this config.
    ///
    /// Single source of truth for the kind-to-severity mapping, shared by core
    /// suppression gating (`severity_for_kind`) and the agent capability
    /// manifest (`fallow schema`'s per-rule `default_severity`). Exhaustive by
    /// design: a new `IssueKind` variant is a compile error here, forcing the
    /// implementer to decide which `RulesConfig` field (if any) gates emission.
    /// Kinds with no matching field (`Complexity`, `CodeDuplication`, gated by
    /// their own command rather than a rule) return the non-`Off`
    /// `Severity::Error`; in core these short-circuit earlier via
    /// `NON_CORE_KINDS` so the value is unobservable there.
    #[must_use]
    pub const fn severity_for_kind(&self, kind: IssueKind) -> Severity {
        match kind {
            IssueKind::UnusedFile => self.unused_files,
            IssueKind::UnusedExport => self.unused_exports,
            IssueKind::UnusedType => self.unused_types,
            IssueKind::PrivateTypeLeak => self.private_type_leaks,
            IssueKind::DeprecatedExportInUse => self.deprecated_exports_in_use,
            IssueKind::UnusedDependency => self.unused_dependencies,
            IssueKind::UnusedDevDependency => self.unused_dev_dependencies,
            IssueKind::UnusedEnumMember => self.unused_enum_members,
            IssueKind::UnusedClassMember => self.unused_class_members,
            IssueKind::UnusedStoreMember => self.unused_store_members,
            IssueKind::UnprovidedInject => self.unprovided_injects,
            IssueKind::UnresolvedImport => self.unresolved_imports,
            IssueKind::UnlistedDependency => self.unlisted_dependencies,
            IssueKind::DuplicateExport => self.duplicate_exports,
            IssueKind::CircularDependency => self.circular_dependencies,
            IssueKind::ReExportCycle => self.re_export_cycle,
            IssueKind::PackageCycle => self.package_cycle,
            IssueKind::TypeOnlyDependency => self.type_only_dependencies,
            IssueKind::TestOnlyDependency => self.test_only_dependencies,
            IssueKind::DevDependencyInProduction => self.dev_dependencies_in_production,
            IssueKind::BoundaryViolation => self.boundary_violation,
            IssueKind::CoverageGaps => self.coverage_gaps,
            IssueKind::FeatureFlag => self.feature_flags,
            IssueKind::StaleSuppression => self.stale_suppressions,
            IssueKind::PnpmCatalogEntry => self.unused_catalog_entries,
            IssueKind::EmptyCatalogGroup => self.empty_catalog_groups,
            IssueKind::UnresolvedCatalogReference => self.unresolved_catalog_references,
            IssueKind::UnusedDependencyOverride => self.unused_dependency_overrides,
            IssueKind::MisconfiguredDependencyOverride => self.misconfigured_dependency_overrides,
            IssueKind::SecurityClientServerLeak => self.security_client_server_leak,
            IssueKind::SecuritySink => self.security_sink,
            IssueKind::PolicyViolation => self.policy_violation,
            IssueKind::InvalidClientExport => self.invalid_client_export,
            IssueKind::MixedClientServerBarrel => self.mixed_client_server_barrel,
            IssueKind::MisplacedDirective => self.misplaced_directive,
            IssueKind::RouteCollision => self.route_collision,
            IssueKind::DynamicSegmentNameConflict => self.dynamic_segment_name_conflict,
            IssueKind::UnrenderedComponent => self.unrendered_components,
            IssueKind::UnusedComponentProp => self.unused_component_props,
            IssueKind::AbsentComponentProp => self.absent_component_props,
            IssueKind::UnusedComponentEmit => self.unused_component_emits,
            IssueKind::UnusedComponentInput => self.unused_component_inputs,
            IssueKind::UnusedComponentOutput => self.unused_component_outputs,
            IssueKind::UnusedSvelteEvent => self.unused_svelte_events,
            IssueKind::UnusedEffectLayer => self.unused_effect_layers,
            IssueKind::UnusedTaggedError => self.unused_tagged_errors,
            IssueKind::UnproducedCatchTag => self.unproduced_catch_tags,
            IssueKind::UnusedServerAction => self.unused_server_actions,
            IssueKind::UnusedLoadDataKey => self.unused_load_data_keys,
            IssueKind::PropDrilling => self.prop_drilling,
            IssueKind::ThinWrapper => self.thin_wrapper,
            IssueKind::DuplicatePropShape => self.duplicate_prop_shape,
            IssueKind::CssTokenDrift => self.css_token_drift,
            IssueKind::CssDuplicateBlock => self.css_duplicate_block,
            IssueKind::CssSelectorComplexity => self.css_selector_complexity,
            IssueKind::CssDeadSurface => self.css_dead_surface,
            IssueKind::CssBrokenReference => self.css_broken_reference,
            IssueKind::WeakenedTests => self.weakened_tests,
            IssueKind::AddedSuppression => self.added_suppression,
            IssueKind::TestOnlyBranch => self.test_only_branch,
            IssueKind::DivergedCloneSibling => self.diverged_clone_sibling,
            IssueKind::AddedDependency => self.added_dependency,
            IssueKind::Complexity | IssueKind::CodeDuplication => Severity::Error,
        }
    }

    /// The gate severity of a complexity finding from the kinds that exceeded
    /// their threshold.
    ///
    /// The most severe rule of the contributing kinds wins. The result is
    /// `Off` only when every contributing kind is `off`, and the finding is
    /// then dropped. A kind that did not contribute has no effect.
    #[must_use]
    pub fn complexity_severity(&self, cyclomatic: bool, cognitive: bool, crap: bool) -> Severity {
        [
            (cyclomatic, self.complexity_cyclomatic),
            (cognitive, self.complexity_cognitive),
            (crap, self.complexity_crap),
        ]
        .into_iter()
        .filter_map(|(contributed, severity)| contributed.then_some(severity))
        .max_by_key(|severity| match severity {
            Severity::Error => 2,
            Severity::Warn => 1,
            Severity::Off => 0,
        })
        .unwrap_or(Severity::Off)
    }

    /// Apply a partial rules config on top. Only `Some` fields override.
    pub const fn apply_partial(&mut self, partial: &PartialRulesConfig) {
        apply_partial_rules!(
            self,
            partial,
            [
                unused_files,
                unused_exports,
                unused_types,
                private_type_leaks,
                deprecated_exports_in_use,
                unused_dependencies,
                unused_dev_dependencies,
                unused_optional_dependencies,
            ]
        );
        apply_partial_rules!(
            self,
            partial,
            [
                unused_enum_members,
                unused_class_members,
                unused_store_members,
                unprovided_injects,
                unrendered_components,
                unused_component_props,
                absent_component_props,
                unused_component_emits,
                unused_component_inputs,
                unused_component_outputs,
                unused_svelte_events,
                unused_effect_layers,
                unused_tagged_errors,
                unproduced_catch_tags,
                unused_server_actions,
                unused_load_data_keys,
                prop_drilling,
                thin_wrapper,
                duplicate_prop_shape,
                css_token_drift,
                css_duplicate_block,
                css_selector_complexity,
                css_dead_surface,
                css_broken_reference,
                complexity_cyclomatic,
                complexity_cognitive,
                complexity_crap,
                complexity_file_lines,
            ]
        );
        apply_partial_rules!(
            self,
            partial,
            [
                unresolved_imports,
                unlisted_dependencies,
                duplicate_exports,
                type_only_dependencies,
                test_only_dependencies,
                dev_dependencies_in_production,
                circular_dependencies,
                re_export_cycle,
                package_cycle,
                boundary_violation,
            ]
        );
        apply_partial_rules!(
            self,
            partial,
            [
                coverage_gaps,
                feature_flags,
                stale_suppressions,
                require_suppression_reason,
                unused_catalog_entries,
                empty_catalog_groups,
                unresolved_catalog_references,
                unused_dependency_overrides,
                misconfigured_dependency_overrides,
            ]
        );
        apply_partial_rules!(
            self,
            partial,
            [
                security_client_server_leak,
                security_sink,
                policy_violation,
                invalid_client_export,
                mixed_client_server_barrel,
                misplaced_directive,
                route_collision,
                dynamic_segment_name_conflict,
                weakened_tests,
                added_suppression,
                test_only_branch,
                diverged_clone_sibling,
                added_dependency,
            ]
        );
    }
}

/// The default [`Severity`] for an [`IssueKind`] under zero config.
///
/// Equivalent to `RulesConfig::default().severity_for_kind(kind)`, exposed as a
/// free function so the agent capability manifest (`fallow schema`) can publish
/// each rule's out-of-the-box severity without threading a config through. There
/// is exactly ONE kind-to-severity table (`RulesConfig::severity_for_kind`); this
/// reuses it rather than duplicating the mapping.
#[must_use]
pub fn default_severity_for_kind(kind: IssueKind) -> Severity {
    RulesConfig::default().severity_for_kind(kind)
}

/// Whether a rule is opt-in: its default severity is `Off`, so it detects and
/// reports nothing until a user explicitly enables it.
///
/// This is the single most load-bearing manifest signal for agents: enabling an
/// opt-in rule blindly (`private-type-leak`, `security-*`) is the most common way
/// to flood a repo with findings, so the manifest exposes it as a first-class bool.
#[must_use]
pub fn is_opt_in_kind(kind: IssueKind) -> bool {
    matches!(default_severity_for_kind(kind), Severity::Off)
}

/// Partial per-issue-type severity for overrides. All fields optional.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct PartialRulesConfig {
    /// Optional override for [`RulesConfig::unused_files`].
    #[serde(
        default,
        alias = "unused-file",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_files: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_exports`].
    #[serde(
        default,
        alias = "unused-export",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_exports: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_types`].
    #[serde(
        default,
        alias = "unused-type",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_types: Option<Severity>,
    /// Optional override for [`RulesConfig::private_type_leaks`].
    #[serde(
        default,
        alias = "private-type-leak",
        skip_serializing_if = "Option::is_none"
    )]
    pub private_type_leaks: Option<Severity>,
    /// Optional override for [`RulesConfig::deprecated_exports_in_use`].
    #[serde(
        default,
        alias = "deprecated-export-in-use",
        skip_serializing_if = "Option::is_none"
    )]
    pub deprecated_exports_in_use: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_dependencies`].
    #[serde(
        default,
        alias = "unused-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_dependencies: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_dev_dependencies`].
    #[serde(
        default,
        alias = "unused-dev-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_dev_dependencies: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_optional_dependencies`].
    #[serde(
        default,
        alias = "unused-optional-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_optional_dependencies: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_enum_members`].
    #[serde(
        default,
        alias = "unused-enum-member",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_enum_members: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_class_members`].
    #[serde(
        default,
        alias = "unused-class-member",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_class_members: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_store_members`].
    #[serde(
        default,
        alias = "unused-store-member",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_store_members: Option<Severity>,
    /// Optional override for [`RulesConfig::unprovided_injects`].
    #[serde(
        default,
        alias = "unprovided-inject",
        skip_serializing_if = "Option::is_none"
    )]
    pub unprovided_injects: Option<Severity>,
    /// Optional override for [`RulesConfig::unrendered_components`].
    #[serde(
        default,
        alias = "unrendered-component",
        skip_serializing_if = "Option::is_none"
    )]
    pub unrendered_components: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_component_props`].
    #[serde(
        default,
        alias = "unused-component-prop",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_component_props: Option<Severity>,
    /// Optional override for absent optional component inputs.
    #[serde(
        default,
        alias = "absent-component-prop",
        skip_serializing_if = "Option::is_none"
    )]
    pub absent_component_props: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_component_emits`].
    #[serde(
        default,
        alias = "unused-component-emit",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_component_emits: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_component_inputs`].
    #[serde(
        default,
        alias = "unused-component-input",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_component_inputs: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_component_outputs`].
    #[serde(
        default,
        alias = "unused-component-output",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_component_outputs: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_svelte_events`].
    #[serde(
        default,
        alias = "unused-svelte-event",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_svelte_events: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_effect_layers`].
    #[serde(
        default,
        alias = "unused-effect-layer",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_effect_layers: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_tagged_errors`].
    #[serde(
        default,
        alias = "unused-tagged-error",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_tagged_errors: Option<Severity>,
    /// Optional override for [`RulesConfig::unproduced_catch_tags`].
    #[serde(
        default,
        alias = "unproduced-catch-tag",
        skip_serializing_if = "Option::is_none"
    )]
    pub unproduced_catch_tags: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_server_actions`].
    #[serde(
        default,
        alias = "unused-server-action",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_server_actions: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_load_data_keys`].
    #[serde(
        default,
        alias = "unused-load-data-key",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_load_data_keys: Option<Severity>,
    /// Optional override for [`RulesConfig::prop_drilling`].
    #[serde(
        default,
        alias = "prop-drilling",
        skip_serializing_if = "Option::is_none"
    )]
    pub prop_drilling: Option<Severity>,
    /// Optional override for [`RulesConfig::thin_wrapper`].
    #[serde(
        default,
        alias = "thin-wrapper",
        skip_serializing_if = "Option::is_none"
    )]
    pub thin_wrapper: Option<Severity>,
    /// Optional override for [`RulesConfig::duplicate_prop_shape`].
    #[serde(
        default,
        alias = "duplicate-prop-shape",
        skip_serializing_if = "Option::is_none"
    )]
    pub duplicate_prop_shape: Option<Severity>,
    /// Optional override for [`RulesConfig::css_token_drift`].
    #[serde(
        default,
        alias = "css-token-drift",
        skip_serializing_if = "Option::is_none"
    )]
    pub css_token_drift: Option<Severity>,
    /// Optional override for [`RulesConfig::css_duplicate_block`].
    #[serde(
        default,
        alias = "css-duplicate-block",
        skip_serializing_if = "Option::is_none"
    )]
    pub css_duplicate_block: Option<Severity>,
    /// Optional override for [`RulesConfig::css_selector_complexity`].
    #[serde(
        default,
        alias = "css-selector-complexity",
        skip_serializing_if = "Option::is_none"
    )]
    pub css_selector_complexity: Option<Severity>,
    /// Optional override for [`RulesConfig::css_dead_surface`].
    #[serde(
        default,
        alias = "css-dead-surface",
        skip_serializing_if = "Option::is_none"
    )]
    pub css_dead_surface: Option<Severity>,
    /// Optional override for [`RulesConfig::css_broken_reference`].
    #[serde(
        default,
        alias = "css-broken-reference",
        skip_serializing_if = "Option::is_none"
    )]
    pub css_broken_reference: Option<Severity>,
    /// Optional override for [`RulesConfig::complexity_cyclomatic`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity_cyclomatic: Option<Severity>,
    /// Optional override for [`RulesConfig::complexity_cognitive`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity_cognitive: Option<Severity>,
    /// Optional override for [`RulesConfig::complexity_crap`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity_crap: Option<Severity>,
    /// Optional override for [`RulesConfig::complexity_file_lines`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity_file_lines: Option<Severity>,
    /// Optional override for [`RulesConfig::unresolved_imports`].
    #[serde(
        default,
        alias = "unresolved-import",
        skip_serializing_if = "Option::is_none"
    )]
    pub unresolved_imports: Option<Severity>,
    /// Optional override for [`RulesConfig::unlisted_dependencies`].
    #[serde(
        default,
        alias = "unlisted-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unlisted_dependencies: Option<Severity>,
    /// Optional override for [`RulesConfig::duplicate_exports`].
    #[serde(
        default,
        alias = "duplicate-export",
        skip_serializing_if = "Option::is_none"
    )]
    pub duplicate_exports: Option<Severity>,
    /// Optional override for [`RulesConfig::type_only_dependencies`].
    #[serde(
        default,
        alias = "type-only-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub type_only_dependencies: Option<Severity>,
    /// Optional override for [`RulesConfig::test_only_dependencies`].
    #[serde(
        default,
        alias = "test-only-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub test_only_dependencies: Option<Severity>,
    /// Optional override for [`RulesConfig::dev_dependencies_in_production`].
    #[serde(
        default,
        alias = "dev-dependency-in-production",
        skip_serializing_if = "Option::is_none"
    )]
    pub dev_dependencies_in_production: Option<Severity>,
    /// Optional override for [`RulesConfig::circular_dependencies`].
    #[serde(
        default,
        alias = "circular-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub circular_dependencies: Option<Severity>,
    /// Optional override for [`RulesConfig::re_export_cycle`].
    #[serde(
        default,
        alias = "re-export-cycles",
        alias = "reexport-cycle",
        alias = "reexport-cycles",
        skip_serializing_if = "Option::is_none"
    )]
    pub re_export_cycle: Option<Severity>,
    /// Optional override for [`RulesConfig::package_cycle`].
    #[serde(
        default,
        alias = "package-cycles",
        skip_serializing_if = "Option::is_none"
    )]
    pub package_cycle: Option<Severity>,
    /// Optional override for [`RulesConfig::boundary_violation`].
    #[serde(
        default,
        alias = "boundary-violations",
        skip_serializing_if = "Option::is_none"
    )]
    pub boundary_violation: Option<Severity>,
    /// Optional override for [`RulesConfig::coverage_gaps`].
    #[serde(
        default,
        alias = "coverage-gap",
        skip_serializing_if = "Option::is_none"
    )]
    pub coverage_gaps: Option<Severity>,
    /// Optional override for [`RulesConfig::feature_flags`].
    #[serde(
        default,
        alias = "feature-flag",
        skip_serializing_if = "Option::is_none"
    )]
    pub feature_flags: Option<Severity>,
    /// Optional override for [`RulesConfig::stale_suppressions`].
    #[serde(
        default,
        alias = "stale-suppression",
        skip_serializing_if = "Option::is_none"
    )]
    pub stale_suppressions: Option<Severity>,
    /// Optional override for [`RulesConfig::require_suppression_reason`].
    #[serde(
        default,
        alias = "suppression-reason",
        skip_serializing_if = "Option::is_none"
    )]
    pub require_suppression_reason: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_catalog_entries`].
    #[serde(
        default,
        alias = "unused-catalog-entry",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_catalog_entries: Option<Severity>,
    /// Optional override for [`RulesConfig::empty_catalog_groups`].
    #[serde(
        default,
        alias = "empty-catalog-group",
        skip_serializing_if = "Option::is_none"
    )]
    pub empty_catalog_groups: Option<Severity>,
    /// Optional override for [`RulesConfig::unresolved_catalog_references`].
    #[serde(
        default,
        alias = "unresolved-catalog-reference",
        skip_serializing_if = "Option::is_none"
    )]
    pub unresolved_catalog_references: Option<Severity>,
    /// Optional override for [`RulesConfig::unused_dependency_overrides`].
    #[serde(
        default,
        alias = "unused-dependency-override",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_dependency_overrides: Option<Severity>,
    /// Optional override for [`RulesConfig::misconfigured_dependency_overrides`].
    #[serde(
        default,
        alias = "misconfigured-dependency-override",
        skip_serializing_if = "Option::is_none"
    )]
    pub misconfigured_dependency_overrides: Option<Severity>,
    /// Optional override for [`RulesConfig::security_client_server_leak`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_client_server_leak: Option<Severity>,
    /// Optional override for [`RulesConfig::security_sink`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_sink: Option<Severity>,
    /// Optional override for [`RulesConfig::policy_violation`].
    #[serde(
        default,
        alias = "policy-violations",
        skip_serializing_if = "Option::is_none"
    )]
    pub policy_violation: Option<Severity>,
    /// Optional override for [`RulesConfig::invalid_client_export`].
    #[serde(
        default,
        alias = "invalid-client-exports",
        skip_serializing_if = "Option::is_none"
    )]
    pub invalid_client_export: Option<Severity>,
    /// Optional override for [`RulesConfig::mixed_client_server_barrel`].
    #[serde(
        default,
        alias = "mixed-client-server-barrels",
        skip_serializing_if = "Option::is_none"
    )]
    pub mixed_client_server_barrel: Option<Severity>,
    /// Optional override for [`RulesConfig::misplaced_directive`].
    #[serde(
        default,
        alias = "misplaced-directives",
        skip_serializing_if = "Option::is_none"
    )]
    pub misplaced_directive: Option<Severity>,
    /// Optional override for [`RulesConfig::route_collision`].
    #[serde(
        default,
        alias = "route-collisions",
        skip_serializing_if = "Option::is_none"
    )]
    pub route_collision: Option<Severity>,
    /// Optional override for [`RulesConfig::dynamic_segment_name_conflict`].
    #[serde(
        default,
        alias = "dynamic-segment-name-conflicts",
        skip_serializing_if = "Option::is_none"
    )]
    pub dynamic_segment_name_conflict: Option<Severity>,
    /// Optional override for [`RulesConfig::weakened_tests`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weakened_tests: Option<Severity>,
    /// Optional override for [`RulesConfig::added_suppression`].
    #[serde(
        default,
        alias = "added-suppressions",
        skip_serializing_if = "Option::is_none"
    )]
    pub added_suppression: Option<Severity>,
    /// Optional override for [`RulesConfig::test_only_branch`].
    #[serde(
        default,
        alias = "test-only-branches",
        skip_serializing_if = "Option::is_none"
    )]
    pub test_only_branch: Option<Severity>,
    /// Optional override for [`RulesConfig::diverged_clone_sibling`].
    #[serde(
        default,
        alias = "diverged-clone-siblings",
        skip_serializing_if = "Option::is_none"
    )]
    pub diverged_clone_sibling: Option<Severity>,
    /// Optional override for [`RulesConfig::added_dependency`].
    #[serde(
        default,
        alias = "added-dependencies",
        skip_serializing_if = "Option::is_none"
    )]
    pub added_dependency: Option<Severity>,
}

/// Every rule name accepted by `RulesConfig` deserialization, in kebab-case.
///
/// Includes both the canonical name produced by `#[serde(rename_all = "kebab-case")]`
/// and every `#[serde(alias = ...)]` value. Used by
/// `find_unknown_rule_keys` to detect typos in user-supplied configs and
/// emit a `tracing::warn!` suggestion at config load time.
///
/// Keep in sync with the `#[serde]` attributes on `RulesConfig` and
/// `PartialRulesConfig`. Nothing enforces that: the only guard here pins the
/// list's length to a literal, so adding a rule to the struct and forgetting
/// this list leaves every test green while the new name warns as an unknown
/// key in user configs. A real pin needs the provenance of each entry, because
/// the list also covers rule names that no `RulesConfig` field produces, so a
/// field-count comparison would not even be correct.
pub const KNOWN_RULE_NAMES: &[&str] = &[
    "unused-files",
    "unused-exports",
    "unused-types",
    "private-type-leaks",
    "deprecated-exports-in-use",
    "unused-dependencies",
    "unused-dev-dependencies",
    "unused-optional-dependencies",
    "unused-enum-members",
    "unused-class-members",
    "unused-store-members",
    "unprovided-injects",
    "unrendered-components",
    "unused-component-props",
    "absent-component-props",
    "unused-component-emits",
    "unused-component-inputs",
    "unused-component-outputs",
    "unused-svelte-events",
    "unused-effect-layers",
    "unused-tagged-errors",
    "unproduced-catch-tags",
    "unused-server-actions",
    "unused-load-data-keys",
    "prop-drilling",
    "thin-wrapper",
    "duplicate-prop-shape",
    "css-token-drift",
    "css-duplicate-block",
    "css-selector-complexity",
    "css-dead-surface",
    "css-broken-reference",
    "complexity-cyclomatic",
    "complexity-cognitive",
    "complexity-crap",
    "complexity-file-lines",
    "unresolved-imports",
    "unlisted-dependencies",
    "duplicate-exports",
    "type-only-dependencies",
    "test-only-dependencies",
    "dev-dependencies-in-production",
    "circular-dependencies",
    "re-export-cycle",
    "package-cycle",
    "boundary-violation",
    "coverage-gaps",
    "feature-flags",
    "stale-suppressions",
    "require-suppression-reason",
    "suppression-reason",
    "unused-catalog-entries",
    "empty-catalog-groups",
    "unresolved-catalog-references",
    "unused-dependency-overrides",
    "misconfigured-dependency-overrides",
    "security-client-server-leak",
    "security-sink",
    "policy-violation",
    "policy-violations",
    "invalid-client-export",
    "mixed-client-server-barrel",
    "misplaced-directive",
    "route-collision",
    "dynamic-segment-name-conflict",
    "unused-file",
    "unused-export",
    "unused-type",
    "private-type-leak",
    "deprecated-export-in-use",
    "unused-dependency",
    "unused-dev-dependency",
    "unused-optional-dependency",
    "unused-enum-member",
    "unused-class-member",
    "unused-store-member",
    "unprovided-inject",
    "unrendered-component",
    "unused-component-prop",
    "absent-component-prop",
    "unused-component-emit",
    "unused-component-input",
    "unused-component-output",
    "unused-svelte-event",
    "unused-effect-layer",
    "unused-tagged-error",
    "unproduced-catch-tag",
    "unused-server-action",
    "unused-load-data-key",
    "unresolved-import",
    "unlisted-dependency",
    "duplicate-export",
    "type-only-dependency",
    "test-only-dependency",
    "dev-dependency-in-production",
    "circular-dependency",
    "re-export-cycles",
    "reexport-cycle",
    "reexport-cycles",
    "package-cycles",
    "boundary-violations",
    "coverage-gap",
    "feature-flag",
    "stale-suppression",
    "unused-catalog-entry",
    "empty-catalog-group",
    "unresolved-catalog-reference",
    "unused-dependency-override",
    "misconfigured-dependency-override",
    "invalid-client-exports",
    "mixed-client-server-barrels",
    "misplaced-directives",
    "route-collisions",
    "dynamic-segment-name-conflicts",
    "weakened-tests",
    "added-suppression",
    "added-suppressions",
    "test-only-branch",
    "test-only-branches",
    "diverged-clone-sibling",
    "diverged-clone-siblings",
    "added-dependency",
    "added-dependencies",
];

/// Find the closest known rule name to `input` when it is plausibly a typo.
///
/// Thin wrapper over [`crate::levenshtein::closest_match`] that scopes the
/// candidate set to [`KNOWN_RULE_NAMES`]. The `'static` lifetime comes from that
/// slice, so the suggestion can be embedded in tracing warnings without
/// allocation. `closest_match` lowercases `input` itself.
#[must_use]
pub fn closest_known_rule_name(input: &str) -> Option<&'static str> {
    crate::levenshtein::closest_match(input, KNOWN_RULE_NAMES.iter().copied())
}

/// An unknown key found inside a `rules` (or `overrides[].rules`) object.
///
/// Surfaced by [`find_unknown_rule_keys`] so the caller (config loader) can
/// emit one `tracing::warn!` per entry without coupling the detection logic
/// to a tracing subscriber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownRuleKey {
    /// Human-readable source label, e.g. `"rules"` or `"overrides[2].rules"`.
    pub context: String,
    /// The unknown key as it appeared in the user's config.
    pub key: String,
    /// Closest known rule name when one is within plausible-typo distance.
    pub suggestion: Option<&'static str>,
}

/// Collect every unknown key from a `rules`-shaped JSON object.
///
/// Returns an empty `Vec` when `value` is not an object or every key is
/// recognized (canonical kebab-case or a documented alias). Called from
/// [`crate::config::parsing`] after `extends` merge and before
/// `serde_json::from_value::<FallowConfig>`, so the warning lists keys from
/// the final merged config rather than per-file partials.
#[must_use]
pub fn find_unknown_rule_keys(value: &serde_json::Value, context: &str) -> Vec<UnknownRuleKey> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };

    map.keys()
        .filter(|key| !KNOWN_RULE_NAMES.contains(&key.as_str()))
        .map(|key| UnknownRuleKey {
            context: context.to_owned(),
            key: key.clone(),
            suggestion: closest_known_rule_name(key),
        })
        .collect()
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
