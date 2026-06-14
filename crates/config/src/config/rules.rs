use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Severity level for rules.
///
/// Controls whether an issue type causes CI failure (`error`), is reported
/// without failing (`warn`), or is suppressed entirely (`off`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Report and fail CI (non-zero exit code).
    #[default]
    Error,
    /// Report but don't fail CI.
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
    #[serde(default, alias = "unused-file")]
    pub unused_files: Severity,
    #[serde(default, alias = "unused-export")]
    pub unused_exports: Severity,
    #[serde(default, alias = "unused-type")]
    pub unused_types: Severity,
    #[serde(default = "Severity::default_off", alias = "private-type-leak")]
    pub private_type_leaks: Severity,
    #[serde(default, alias = "unused-dependency")]
    pub unused_dependencies: Severity,
    #[serde(default = "Severity::default_warn", alias = "unused-dev-dependency")]
    pub unused_dev_dependencies: Severity,
    #[serde(
        default = "Severity::default_warn",
        alias = "unused-optional-dependency"
    )]
    pub unused_optional_dependencies: Severity,
    #[serde(default, alias = "unused-enum-member")]
    pub unused_enum_members: Severity,
    #[serde(default, alias = "unused-class-member")]
    pub unused_class_members: Severity,
    /// Store members (Pinia `state` / `getters` / `actions` key, or a
    /// setup-store returned key) declared but never accessed by any consumer
    /// project-wide. Defaults to `warn`, not `error` like the closed-set
    /// class/enum member rules: a store has an OPEN declaration surface
    /// (plugins, `$onAction`, dynamic dispatch) so analyzer confidence is
    /// genuinely lower; warn encodes that without failing CI. Promotable to
    /// `error` once validated on a codebase.
    #[serde(default, alias = "unused-store-member")]
    pub unused_store_members: Severity,
    /// Vue `inject(KEY)` / Svelte `getContext(KEY)` whose symbol KEY is
    /// `provide`/`setContext`'d nowhere in the project (the
    /// injected-never-provided dead-half). Defaults to `warn`, not `error`:
    /// a DI key has an open provide surface (plugins, app-level provide) so
    /// analyzer confidence is lower; warn encodes that without failing CI.
    #[serde(default, alias = "unprovided-inject")]
    pub unprovided_injects: Severity,
    /// Vue/Svelte single-file component reachable in the module graph but
    /// rendered nowhere in the project (the imported-but-never-rendered
    /// dead-half). Defaults to `warn`, not `error`: a component can be rendered
    /// reflectively (dynamic `<component :is>`), so analyzer confidence is
    /// lower; warn encodes that without failing CI.
    #[serde(default, alias = "unrendered-component")]
    pub unrendered_components: Severity,
    /// Vue `<script setup>` `defineProps` declared prop referenced nowhere
    /// inside its own single-file component (neither `<script>` nor
    /// `<template>`). The single-file dead-input direction. Defaults to `warn`,
    /// not `error`: a prop can be part of a deliberately-stable public component
    /// API, so analyzer confidence is lower; warn encodes that without failing
    /// CI.
    #[serde(default, alias = "unused-component-prop")]
    pub unused_component_props: Severity,
    /// Vue `<script setup>` `defineEmits` declared event emitted nowhere inside
    /// its own single-file component (no `emit('<name>')` call). The single-file
    /// dead-input direction. Defaults to `warn`, not `error`: an emit can be part
    /// of a deliberately-stable public component API, so analyzer confidence is
    /// lower; warn encodes that without failing CI.
    #[serde(default, alias = "unused-component-emit")]
    pub unused_component_emits: Severity,
    /// Next.js Server Action (an export of a `"use server"` file) referenced by
    /// no code in the project: no import-and-call, no `action={fn}` binding, no
    /// `<form action={fn}>`. Cross-graph dead-export direction, reclassified out
    /// of `unused-export` for `"use server"` files. Defaults to `warn`, not
    /// `error`: the rule is new and false-negative-preferring, and reflective
    /// action-dispatch shapes can hide a real consumer; warn encodes that
    /// without failing CI until corpus-validated.
    #[serde(default, alias = "unused-server-action")]
    pub unused_server_actions: Severity,
    #[serde(default, alias = "unresolved-import")]
    pub unresolved_imports: Severity,
    #[serde(default, alias = "unlisted-dependency")]
    pub unlisted_dependencies: Severity,
    #[serde(default, alias = "duplicate-export")]
    pub duplicate_exports: Severity,
    #[serde(default = "Severity::default_warn", alias = "type-only-dependency")]
    pub type_only_dependencies: Severity,
    #[serde(default = "Severity::default_warn", alias = "test-only-dependency")]
    pub test_only_dependencies: Severity,
    #[serde(default, alias = "circular-dependency")]
    pub circular_dependencies: Severity,
    #[serde(
        default = "Severity::default_warn",
        alias = "re-export-cycles",
        alias = "reexport-cycle",
        alias = "reexport-cycles"
    )]
    pub re_export_cycle: Severity,
    #[serde(default, alias = "boundary-violations")]
    pub boundary_violation: Severity,
    #[serde(default, alias = "coverage-gap")]
    pub coverage_gaps: Severity,
    #[serde(default = "Severity::default_off", alias = "feature-flag")]
    pub feature_flags: Severity,
    #[serde(default = "Severity::default_warn", alias = "stale-suppression")]
    pub stale_suppressions: Severity,
    #[serde(default = "Severity::default_warn", alias = "unused-catalog-entry")]
    pub unused_catalog_entries: Severity,
    #[serde(default = "Severity::default_warn", alias = "empty-catalog-group")]
    pub empty_catalog_groups: Severity,
    #[serde(default, alias = "unresolved-catalog-reference")]
    pub unresolved_catalog_references: Severity,
    #[serde(
        default = "Severity::default_warn",
        alias = "unused-dependency-override"
    )]
    pub unused_dependency_overrides: Severity,
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
    /// first request. fallow catches it statically. Defaults to `warn` for now
    /// (graduates to `error` in a later release once field-proven).
    #[serde(
        default = "Severity::default_warn",
        alias = "dynamic-segment-name-conflicts"
    )]
    pub dynamic_segment_name_conflict: Severity,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            unused_files: Severity::Error,
            unused_exports: Severity::Error,
            unused_types: Severity::Error,
            private_type_leaks: Severity::Off,
            unused_dependencies: Severity::Error,
            unused_dev_dependencies: Severity::Warn,
            unused_optional_dependencies: Severity::Warn,
            unused_enum_members: Severity::Error,
            unused_class_members: Severity::Error,
            unused_store_members: Severity::Warn,
            unprovided_injects: Severity::Warn,
            unrendered_components: Severity::Warn,
            unused_component_props: Severity::Warn,
            unused_component_emits: Severity::Warn,
            unused_server_actions: Severity::Warn,
            unresolved_imports: Severity::Error,
            unlisted_dependencies: Severity::Error,
            duplicate_exports: Severity::Error,
            type_only_dependencies: Severity::Warn,
            test_only_dependencies: Severity::Warn,
            circular_dependencies: Severity::Error,
            re_export_cycle: Severity::Warn,
            boundary_violation: Severity::Error,
            coverage_gaps: Severity::Off,
            feature_flags: Severity::Off,
            stale_suppressions: Severity::Warn,
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
            dynamic_segment_name_conflict: Severity::Warn,
        }
    }
}

impl RulesConfig {
    /// Apply a partial rules config on top. Only `Some` fields override.
    pub const fn apply_partial(&mut self, partial: &PartialRulesConfig) {
        if let Some(s) = partial.unused_files {
            self.unused_files = s;
        }
        if let Some(s) = partial.unused_exports {
            self.unused_exports = s;
        }
        if let Some(s) = partial.unused_types {
            self.unused_types = s;
        }
        if let Some(s) = partial.private_type_leaks {
            self.private_type_leaks = s;
        }
        if let Some(s) = partial.unused_dependencies {
            self.unused_dependencies = s;
        }
        if let Some(s) = partial.unused_dev_dependencies {
            self.unused_dev_dependencies = s;
        }
        if let Some(s) = partial.unused_optional_dependencies {
            self.unused_optional_dependencies = s;
        }
        if let Some(s) = partial.unused_enum_members {
            self.unused_enum_members = s;
        }
        if let Some(s) = partial.unused_class_members {
            self.unused_class_members = s;
        }
        if let Some(s) = partial.unused_store_members {
            self.unused_store_members = s;
        }
        if let Some(s) = partial.unprovided_injects {
            self.unprovided_injects = s;
        }
        if let Some(s) = partial.unrendered_components {
            self.unrendered_components = s;
        }
        if let Some(s) = partial.unused_component_props {
            self.unused_component_props = s;
        }
        if let Some(s) = partial.unused_component_emits {
            self.unused_component_emits = s;
        }
        if let Some(s) = partial.unused_server_actions {
            self.unused_server_actions = s;
        }
        if let Some(s) = partial.unresolved_imports {
            self.unresolved_imports = s;
        }
        if let Some(s) = partial.unlisted_dependencies {
            self.unlisted_dependencies = s;
        }
        if let Some(s) = partial.duplicate_exports {
            self.duplicate_exports = s;
        }
        if let Some(s) = partial.type_only_dependencies {
            self.type_only_dependencies = s;
        }
        if let Some(s) = partial.test_only_dependencies {
            self.test_only_dependencies = s;
        }
        if let Some(s) = partial.circular_dependencies {
            self.circular_dependencies = s;
        }
        if let Some(s) = partial.re_export_cycle {
            self.re_export_cycle = s;
        }
        if let Some(s) = partial.boundary_violation {
            self.boundary_violation = s;
        }
        if let Some(s) = partial.coverage_gaps {
            self.coverage_gaps = s;
        }
        if let Some(s) = partial.feature_flags {
            self.feature_flags = s;
        }
        if let Some(s) = partial.stale_suppressions {
            self.stale_suppressions = s;
        }
        if let Some(s) = partial.unused_catalog_entries {
            self.unused_catalog_entries = s;
        }
        if let Some(s) = partial.empty_catalog_groups {
            self.empty_catalog_groups = s;
        }
        if let Some(s) = partial.unresolved_catalog_references {
            self.unresolved_catalog_references = s;
        }
        if let Some(s) = partial.unused_dependency_overrides {
            self.unused_dependency_overrides = s;
        }
        if let Some(s) = partial.misconfigured_dependency_overrides {
            self.misconfigured_dependency_overrides = s;
        }
        if let Some(s) = partial.security_client_server_leak {
            self.security_client_server_leak = s;
        }
        if let Some(s) = partial.security_sink {
            self.security_sink = s;
        }
        if let Some(s) = partial.policy_violation {
            self.policy_violation = s;
        }
        if let Some(s) = partial.invalid_client_export {
            self.invalid_client_export = s;
        }
        if let Some(s) = partial.mixed_client_server_barrel {
            self.mixed_client_server_barrel = s;
        }
        if let Some(s) = partial.misplaced_directive {
            self.misplaced_directive = s;
        }
        if let Some(s) = partial.route_collision {
            self.route_collision = s;
        }
        if let Some(s) = partial.dynamic_segment_name_conflict {
            self.dynamic_segment_name_conflict = s;
        }
    }
}

/// Partial per-issue-type severity for overrides. All fields optional.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct PartialRulesConfig {
    #[serde(
        default,
        alias = "unused-file",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_files: Option<Severity>,
    #[serde(
        default,
        alias = "unused-export",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_exports: Option<Severity>,
    #[serde(
        default,
        alias = "unused-type",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_types: Option<Severity>,
    #[serde(
        default,
        alias = "private-type-leak",
        skip_serializing_if = "Option::is_none"
    )]
    pub private_type_leaks: Option<Severity>,
    #[serde(
        default,
        alias = "unused-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "unused-dev-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_dev_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "unused-optional-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_optional_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "unused-enum-member",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_enum_members: Option<Severity>,
    #[serde(
        default,
        alias = "unused-class-member",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_class_members: Option<Severity>,
    #[serde(
        default,
        alias = "unused-store-member",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_store_members: Option<Severity>,
    #[serde(
        default,
        alias = "unprovided-inject",
        skip_serializing_if = "Option::is_none"
    )]
    pub unprovided_injects: Option<Severity>,
    #[serde(
        default,
        alias = "unrendered-component",
        skip_serializing_if = "Option::is_none"
    )]
    pub unrendered_components: Option<Severity>,
    #[serde(
        default,
        alias = "unused-component-prop",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_component_props: Option<Severity>,
    #[serde(
        default,
        alias = "unused-component-emit",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_component_emits: Option<Severity>,
    #[serde(
        default,
        alias = "unused-server-action",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_server_actions: Option<Severity>,
    #[serde(
        default,
        alias = "unresolved-import",
        skip_serializing_if = "Option::is_none"
    )]
    pub unresolved_imports: Option<Severity>,
    #[serde(
        default,
        alias = "unlisted-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub unlisted_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "duplicate-export",
        skip_serializing_if = "Option::is_none"
    )]
    pub duplicate_exports: Option<Severity>,
    #[serde(
        default,
        alias = "type-only-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub type_only_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "test-only-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub test_only_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "circular-dependency",
        skip_serializing_if = "Option::is_none"
    )]
    pub circular_dependencies: Option<Severity>,
    #[serde(
        default,
        alias = "re-export-cycles",
        alias = "reexport-cycle",
        alias = "reexport-cycles",
        skip_serializing_if = "Option::is_none"
    )]
    pub re_export_cycle: Option<Severity>,
    #[serde(
        default,
        alias = "boundary-violations",
        skip_serializing_if = "Option::is_none"
    )]
    pub boundary_violation: Option<Severity>,
    #[serde(
        default,
        alias = "coverage-gap",
        skip_serializing_if = "Option::is_none"
    )]
    pub coverage_gaps: Option<Severity>,
    #[serde(
        default,
        alias = "feature-flag",
        skip_serializing_if = "Option::is_none"
    )]
    pub feature_flags: Option<Severity>,
    #[serde(
        default,
        alias = "stale-suppression",
        skip_serializing_if = "Option::is_none"
    )]
    pub stale_suppressions: Option<Severity>,
    #[serde(
        default,
        alias = "unused-catalog-entry",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_catalog_entries: Option<Severity>,
    #[serde(
        default,
        alias = "empty-catalog-group",
        skip_serializing_if = "Option::is_none"
    )]
    pub empty_catalog_groups: Option<Severity>,
    #[serde(
        default,
        alias = "unresolved-catalog-reference",
        skip_serializing_if = "Option::is_none"
    )]
    pub unresolved_catalog_references: Option<Severity>,
    #[serde(
        default,
        alias = "unused-dependency-override",
        skip_serializing_if = "Option::is_none"
    )]
    pub unused_dependency_overrides: Option<Severity>,
    #[serde(
        default,
        alias = "misconfigured-dependency-override",
        skip_serializing_if = "Option::is_none"
    )]
    pub misconfigured_dependency_overrides: Option<Severity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_client_server_leak: Option<Severity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_sink: Option<Severity>,
    #[serde(
        default,
        alias = "policy-violations",
        skip_serializing_if = "Option::is_none"
    )]
    pub policy_violation: Option<Severity>,
    #[serde(
        default,
        alias = "invalid-client-exports",
        skip_serializing_if = "Option::is_none"
    )]
    pub invalid_client_export: Option<Severity>,
    #[serde(
        default,
        alias = "mixed-client-server-barrels",
        skip_serializing_if = "Option::is_none"
    )]
    pub mixed_client_server_barrel: Option<Severity>,
    #[serde(
        default,
        alias = "misplaced-directives",
        skip_serializing_if = "Option::is_none"
    )]
    pub misplaced_directive: Option<Severity>,
    #[serde(
        default,
        alias = "route-collisions",
        skip_serializing_if = "Option::is_none"
    )]
    pub route_collision: Option<Severity>,
    #[serde(
        default,
        alias = "dynamic-segment-name-conflicts",
        skip_serializing_if = "Option::is_none"
    )]
    pub dynamic_segment_name_conflict: Option<Severity>,
}

/// Every rule name accepted by `RulesConfig` deserialization, in kebab-case.
///
/// Includes both the canonical name produced by `#[serde(rename_all = "kebab-case")]`
/// and every `#[serde(alias = ...)]` value. Used by
/// [`find_unknown_rule_keys`] to detect typos in user-supplied configs and
/// emit a `tracing::warn!` suggestion at config load time.
///
/// Keep in sync with the `#[serde]` attributes on `RulesConfig` and
/// `PartialRulesConfig`; the `known_rule_names_count_matches_struct` test
/// fails when the lists drift.
pub const KNOWN_RULE_NAMES: &[&str] = &[
    "unused-files",
    "unused-exports",
    "unused-types",
    "private-type-leaks",
    "unused-dependencies",
    "unused-dev-dependencies",
    "unused-optional-dependencies",
    "unused-enum-members",
    "unused-class-members",
    "unused-store-members",
    "unprovided-injects",
    "unrendered-components",
    "unused-component-props",
    "unused-component-emits",
    "unused-server-actions",
    "unresolved-imports",
    "unlisted-dependencies",
    "duplicate-exports",
    "type-only-dependencies",
    "test-only-dependencies",
    "circular-dependencies",
    "re-export-cycle",
    "boundary-violation",
    "coverage-gaps",
    "feature-flags",
    "stale-suppressions",
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
    "unused-dependency",
    "unused-dev-dependency",
    "unused-optional-dependency",
    "unused-enum-member",
    "unused-class-member",
    "unused-store-member",
    "unprovided-inject",
    "unrendered-component",
    "unused-component-prop",
    "unused-component-emit",
    "unused-server-action",
    "unresolved-import",
    "unlisted-dependency",
    "duplicate-export",
    "type-only-dependency",
    "test-only-dependency",
    "circular-dependency",
    "re-export-cycles",
    "reexport-cycle",
    "reexport-cycles",
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
];

/// Find the closest known rule name to `input` when it is plausibly a typo.
///
/// Thin wrapper over [`crate::levenshtein::closest_match`] that scopes the
/// candidate set to [`KNOWN_RULE_NAMES`] and returns a `'static` reference so
/// the suggestion can be embedded in tracing warnings without allocation.
#[must_use]
pub fn closest_known_rule_name(input: &str) -> Option<&'static str> {
    let input_lower = input.to_ascii_lowercase();
    let candidates = KNOWN_RULE_NAMES.iter().copied();
    let suggestion = crate::levenshtein::closest_match(&input_lower, candidates)?;
    KNOWN_RULE_NAMES.iter().copied().find(|&c| c == suggestion)
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
mod tests {
    use super::*;

    #[test]
    fn rules_default_severities() {
        let rules = RulesConfig::default();
        assert_eq!(rules.unused_files, Severity::Error);
        assert_eq!(rules.unused_exports, Severity::Error);
        assert_eq!(rules.unused_types, Severity::Error);
        assert_eq!(rules.private_type_leaks, Severity::Off);
        assert_eq!(rules.unused_dependencies, Severity::Error);
        assert_eq!(rules.unused_dev_dependencies, Severity::Warn);
        assert_eq!(rules.unused_optional_dependencies, Severity::Warn);
        assert_eq!(rules.unused_enum_members, Severity::Error);
        assert_eq!(rules.unused_class_members, Severity::Error);
        assert_eq!(rules.unresolved_imports, Severity::Error);
        assert_eq!(rules.unlisted_dependencies, Severity::Error);
        assert_eq!(rules.duplicate_exports, Severity::Error);
        assert_eq!(rules.type_only_dependencies, Severity::Warn);
        assert_eq!(rules.test_only_dependencies, Severity::Warn);
        assert_eq!(rules.circular_dependencies, Severity::Error);
        assert_eq!(rules.boundary_violation, Severity::Error);
        assert_eq!(rules.coverage_gaps, Severity::Off);
        assert_eq!(rules.feature_flags, Severity::Off);
        assert_eq!(rules.stale_suppressions, Severity::Warn);
        assert_eq!(rules.unused_catalog_entries, Severity::Warn);
        assert_eq!(rules.empty_catalog_groups, Severity::Warn);
        assert_eq!(rules.unresolved_catalog_references, Severity::Error);
    }

    #[test]
    fn rules_deserialize_kebab_case() {
        let json_str = r#"{
            "unused-files": "error",
            "unused-exports": "warn",
            "unused-types": "off"
        }"#;
        let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(rules.unused_files, Severity::Error);
        assert_eq!(rules.unused_exports, Severity::Warn);
        assert_eq!(rules.unused_types, Severity::Off);
        assert_eq!(rules.unresolved_imports, Severity::Error);
    }

    #[test]
    fn rules_re_export_cycle_default_is_warn() {
        let rules = RulesConfig::default();
        assert_eq!(rules.re_export_cycle, Severity::Warn);
    }

    #[test]
    fn rules_deserialize_re_export_cycle_aliases() {
        for token in [
            "re-export-cycle",
            "re-export-cycles",
            "reexport-cycle",
            "reexport-cycles",
        ] {
            let json_str = format!(r#"{{ "{token}": "error" }}"#);
            let rules: RulesConfig = serde_json::from_str(&json_str)
                .unwrap_or_else(|e| panic!("alias {token} did not deserialize: {e}"));
            assert_eq!(
                rules.re_export_cycle,
                Severity::Error,
                "alias {token} should set re_export_cycle"
            );
        }
    }

    #[test]
    fn rules_deserialize_circular_dependency_alias() {
        let json_str = r#"{
            "circular-dependency": "off"
        }"#;
        let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(rules.circular_dependencies, Severity::Off);
    }

    #[test]
    fn rules_deserialize_boundary_violations_alias() {
        let json_str = r#"{
            "boundary-violations": "off"
        }"#;
        let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(rules.boundary_violation, Severity::Off);

        let partial: PartialRulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(partial.boundary_violation, Some(Severity::Off));
    }

    #[test]
    fn rules_deserialize_singular_aliases_for_every_plural_rule() {
        let json_str = r#"{
            "unused-file": "off",
            "unused-export": "off",
            "unused-type": "off",
            "private-type-leak": "warn",
            "unused-dependency": "off",
            "unused-dev-dependency": "off",
            "unused-optional-dependency": "off",
            "unused-enum-member": "off",
            "unused-class-member": "off",
            "unresolved-import": "off",
            "unlisted-dependency": "off",
            "duplicate-export": "off",
            "type-only-dependency": "off",
            "test-only-dependency": "off",
            "coverage-gap": "warn",
            "feature-flag": "warn",
            "stale-suppression": "off",
            "unused-catalog-entry": "error",
            "empty-catalog-group": "error",
            "unresolved-catalog-reference": "warn"
        }"#;

        let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(rules.unused_files, Severity::Off);
        assert_eq!(rules.unused_exports, Severity::Off);
        assert_eq!(rules.unused_types, Severity::Off);
        assert_eq!(rules.private_type_leaks, Severity::Warn);
        assert_eq!(rules.unused_dependencies, Severity::Off);
        assert_eq!(rules.unused_dev_dependencies, Severity::Off);
        assert_eq!(rules.unused_optional_dependencies, Severity::Off);
        assert_eq!(rules.unused_enum_members, Severity::Off);
        assert_eq!(rules.unused_class_members, Severity::Off);
        assert_eq!(rules.unresolved_imports, Severity::Off);
        assert_eq!(rules.unlisted_dependencies, Severity::Off);
        assert_eq!(rules.duplicate_exports, Severity::Off);
        assert_eq!(rules.type_only_dependencies, Severity::Off);
        assert_eq!(rules.test_only_dependencies, Severity::Off);
        assert_eq!(rules.coverage_gaps, Severity::Warn);
        assert_eq!(rules.feature_flags, Severity::Warn);
        assert_eq!(rules.stale_suppressions, Severity::Off);
        assert_eq!(rules.unused_catalog_entries, Severity::Error);
        assert_eq!(rules.empty_catalog_groups, Severity::Error);
        assert_eq!(rules.unresolved_catalog_references, Severity::Warn);

        let partial: PartialRulesConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(partial.unused_files, Some(Severity::Off));
        assert_eq!(partial.unused_exports, Some(Severity::Off));
        assert_eq!(partial.unused_types, Some(Severity::Off));
        assert_eq!(partial.private_type_leaks, Some(Severity::Warn));
        assert_eq!(partial.unused_dependencies, Some(Severity::Off));
        assert_eq!(partial.unused_dev_dependencies, Some(Severity::Off));
        assert_eq!(partial.unused_optional_dependencies, Some(Severity::Off));
        assert_eq!(partial.unused_enum_members, Some(Severity::Off));
        assert_eq!(partial.unused_class_members, Some(Severity::Off));
        assert_eq!(partial.unresolved_imports, Some(Severity::Off));
        assert_eq!(partial.unlisted_dependencies, Some(Severity::Off));
        assert_eq!(partial.duplicate_exports, Some(Severity::Off));
        assert_eq!(partial.type_only_dependencies, Some(Severity::Off));
        assert_eq!(partial.test_only_dependencies, Some(Severity::Off));
        assert_eq!(partial.coverage_gaps, Some(Severity::Warn));
        assert_eq!(partial.feature_flags, Some(Severity::Warn));
        assert_eq!(partial.stale_suppressions, Some(Severity::Off));
        assert_eq!(partial.unused_catalog_entries, Some(Severity::Error));
        assert_eq!(partial.empty_catalog_groups, Some(Severity::Error));
        assert_eq!(partial.unresolved_catalog_references, Some(Severity::Warn));
    }

    #[test]
    fn severity_from_str() {
        assert_eq!("error".parse::<Severity>().unwrap(), Severity::Error);
        assert_eq!("warn".parse::<Severity>().unwrap(), Severity::Warn);
        assert_eq!("warning".parse::<Severity>().unwrap(), Severity::Warn);
        assert_eq!("off".parse::<Severity>().unwrap(), Severity::Off);
        assert_eq!("none".parse::<Severity>().unwrap(), Severity::Off);
        assert!("invalid".parse::<Severity>().is_err());
    }

    #[test]
    fn apply_partial_only_some_fields() {
        let mut rules = RulesConfig::default();
        let partial = PartialRulesConfig {
            unused_files: Some(Severity::Warn),
            unused_exports: Some(Severity::Off),
            ..Default::default()
        };
        rules.apply_partial(&partial);
        assert_eq!(rules.unused_files, Severity::Warn);
        assert_eq!(rules.unused_exports, Severity::Off);
        assert_eq!(rules.unused_types, Severity::Error);
        assert_eq!(rules.unresolved_imports, Severity::Error);
    }

    #[test]
    fn severity_display() {
        assert_eq!(Severity::Error.to_string(), "error");
        assert_eq!(Severity::Warn.to_string(), "warn");
        assert_eq!(Severity::Off.to_string(), "off");
    }

    #[test]
    fn apply_partial_all_none_changes_nothing() {
        let mut rules = RulesConfig::default();
        let original = rules.clone();
        let partial = PartialRulesConfig::default(); // all None
        rules.apply_partial(&partial);
        assert_eq!(rules.unused_files, original.unused_files);
        assert_eq!(rules.unused_exports, original.unused_exports);
        assert_eq!(
            rules.type_only_dependencies,
            original.type_only_dependencies
        );
    }

    #[test]
    fn apply_partial_all_fields_set() {
        let mut rules = RulesConfig::default();
        let partial = PartialRulesConfig {
            unused_files: Some(Severity::Off),
            unused_exports: Some(Severity::Off),
            unused_types: Some(Severity::Off),
            private_type_leaks: Some(Severity::Off),
            unused_dependencies: Some(Severity::Off),
            unused_dev_dependencies: Some(Severity::Off),
            unused_optional_dependencies: Some(Severity::Off),
            unused_enum_members: Some(Severity::Off),
            unused_class_members: Some(Severity::Off),
            unused_store_members: Some(Severity::Off),
            unprovided_injects: Some(Severity::Off),
            unrendered_components: Some(Severity::Off),
            unused_component_props: Some(Severity::Off),
            unused_component_emits: Some(Severity::Off),
            unused_server_actions: Some(Severity::Off),
            unresolved_imports: Some(Severity::Off),
            unlisted_dependencies: Some(Severity::Off),
            duplicate_exports: Some(Severity::Off),
            type_only_dependencies: Some(Severity::Off),
            test_only_dependencies: Some(Severity::Off),
            circular_dependencies: Some(Severity::Off),
            re_export_cycle: Some(Severity::Off),
            boundary_violation: Some(Severity::Off),
            coverage_gaps: Some(Severity::Off),
            feature_flags: Some(Severity::Off),
            stale_suppressions: Some(Severity::Off),
            unused_catalog_entries: Some(Severity::Off),
            empty_catalog_groups: Some(Severity::Off),
            unresolved_catalog_references: Some(Severity::Off),
            unused_dependency_overrides: Some(Severity::Off),
            misconfigured_dependency_overrides: Some(Severity::Off),
            security_client_server_leak: Some(Severity::Off),
            security_sink: Some(Severity::Off),
            policy_violation: Some(Severity::Off),
            invalid_client_export: Some(Severity::Off),
            mixed_client_server_barrel: Some(Severity::Off),
            misplaced_directive: Some(Severity::Off),
            route_collision: Some(Severity::Off),
            dynamic_segment_name_conflict: Some(Severity::Off),
        };
        rules.apply_partial(&partial);
        assert_eq!(rules.unused_files, Severity::Off);
        assert_eq!(rules.private_type_leaks, Severity::Off);
        assert_eq!(rules.circular_dependencies, Severity::Off);
        assert_eq!(rules.type_only_dependencies, Severity::Off);
        assert_eq!(rules.test_only_dependencies, Severity::Off);
        assert_eq!(rules.boundary_violation, Severity::Off);
        assert_eq!(rules.coverage_gaps, Severity::Off);
        assert_eq!(rules.feature_flags, Severity::Off);
        assert_eq!(rules.stale_suppressions, Severity::Off);
        assert_eq!(rules.security_sink, Severity::Off);
        assert_eq!(rules.policy_violation, Severity::Off);
        assert_eq!(rules.invalid_client_export, Severity::Off);
        assert_eq!(rules.mixed_client_server_barrel, Severity::Off);
        assert_eq!(rules.misplaced_directive, Severity::Off);
        assert_eq!(rules.unrendered_components, Severity::Off);
        assert_eq!(rules.unused_component_props, Severity::Off);
        assert_eq!(rules.unused_component_emits, Severity::Off);
        assert_eq!(rules.route_collision, Severity::Off);
        assert_eq!(rules.dynamic_segment_name_conflict, Severity::Off);
    }

    #[test]
    fn rules_config_defaults_include_optional_deps() {
        let rules = RulesConfig::default();
        assert_eq!(rules.unused_optional_dependencies, Severity::Warn);
    }

    #[test]
    fn policy_violation_defaults_to_warn() {
        let rules = RulesConfig::default();
        assert_eq!(rules.policy_violation, Severity::Warn);
    }

    #[test]
    fn policy_violation_accepts_plural_alias() {
        let json = r#"{ "policy-violations": "error" }"#;
        let rules: RulesConfig = serde_json::from_str(json).unwrap();
        assert_eq!(rules.policy_violation, Severity::Error);
    }

    #[test]
    fn severity_from_str_case_insensitive() {
        assert_eq!("ERROR".parse::<Severity>().unwrap(), Severity::Error);
        assert_eq!("Warn".parse::<Severity>().unwrap(), Severity::Warn);
        assert_eq!("OFF".parse::<Severity>().unwrap(), Severity::Off);
        assert_eq!("Warning".parse::<Severity>().unwrap(), Severity::Warn);
        assert_eq!("NONE".parse::<Severity>().unwrap(), Severity::Off);
    }

    #[test]
    fn severity_from_str_invalid_returns_error() {
        let result = "critical".parse::<Severity>();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("unknown severity"),
            "Expected descriptive error, got: {err}"
        );
    }

    #[test]
    fn known_rule_names_count_matches_struct() {
        assert_eq!(KNOWN_RULE_NAMES.len(), 78);
    }

    #[test]
    fn known_rule_names_has_no_duplicates() {
        let mut sorted: Vec<&str> = KNOWN_RULE_NAMES.to_vec();
        sorted.sort_unstable();
        let original_len = sorted.len();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            original_len,
            "KNOWN_RULE_NAMES contains a duplicate"
        );
    }

    #[test]
    fn known_rule_names_covers_every_serde_alias_in_source() {
        let source = include_str!("rules.rs");

        let mut aliases_found = Vec::new();
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            let Some(after) = trimmed.split("alias = \"").nth(1) else {
                continue;
            };
            let Some(end) = after.find('"') else {
                continue;
            };
            let alias = &after[..end];
            if alias.is_empty() || !alias.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
                continue;
            }
            aliases_found.push(alias.to_owned());
        }

        assert_eq!(
            aliases_found.len(),
            78,
            "expected 78 source-level alias attrs (39 per struct); got {}: {:?}",
            aliases_found.len(),
            aliases_found
        );

        for alias in &aliases_found {
            assert!(
                KNOWN_RULE_NAMES.contains(&alias.as_str()),
                "serde alias '{alias}' is in rules.rs source but missing from KNOWN_RULE_NAMES"
            );
        }
    }

    #[test]
    fn re_export_cycle_aliases_all_round_trip_to_the_same_field() {
        for alias in [
            "re-export-cycle",
            "re-export-cycles",
            "reexport-cycle",
            "reexport-cycles",
        ] {
            let json = format!(r#"{{"{alias}": "warn"}}"#);
            let partial: PartialRulesConfig = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("'{alias}' should deserialize: {e}"));
            assert_eq!(
                partial.re_export_cycle,
                Some(Severity::Warn),
                "'{alias}' should set re_export_cycle to Warn"
            );
            let serialized = serde_json::to_value(&partial).unwrap();
            let map = serialized.as_object().unwrap();
            assert_eq!(
                map.len(),
                1,
                "'{alias}' should resolve to exactly one field, got: {map:?}"
            );
        }
    }

    #[test]
    fn every_known_rule_name_round_trips_through_partial() {
        for &name in KNOWN_RULE_NAMES {
            let json = format!(r#"{{"{name}": "warn"}}"#);
            let partial: PartialRulesConfig = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("'{name}' should deserialize: {e}"));

            let serialized = serde_json::to_value(&partial).unwrap();
            let map = serialized.as_object().unwrap();
            assert_eq!(
                map.len(),
                1,
                "'{name}' should resolve to exactly one field, got: {map:?}"
            );
        }
    }

    #[test]
    fn known_rule_names_covers_every_struct_field() {
        let json = serde_json::to_value(RulesConfig::default()).unwrap();
        let obj = json.as_object().unwrap();
        for key in obj.keys() {
            assert!(
                KNOWN_RULE_NAMES.contains(&key.as_str()),
                "field '{key}' is serialized but missing from KNOWN_RULE_NAMES"
            );
        }
    }

    #[test]
    fn closest_known_rule_name_suggests_for_obvious_typo() {
        assert_eq!(
            closest_known_rule_name("unsued-files"),
            Some("unused-files")
        );
        assert_eq!(
            closest_known_rule_name("circular-dependnecy"),
            Some("circular-dependency")
        );
        assert_eq!(
            closest_known_rule_name("unused-dep"),
            None,
            "too short for a confident suggestion"
        );
    }

    #[test]
    fn closest_known_rule_name_returns_none_for_novel_input() {
        assert_eq!(closest_known_rule_name("totally-fabricated"), None);
        assert_eq!(closest_known_rule_name("foo"), None);
    }

    #[test]
    fn closest_known_rule_name_is_case_insensitive() {
        assert_eq!(
            closest_known_rule_name("UNSUED-FILES"),
            Some("unused-files")
        );
    }

    #[test]
    fn closest_known_rule_name_returns_none_for_exact_match() {
        assert_eq!(closest_known_rule_name("unused-files"), None);
    }

    #[test]
    fn find_unknown_rule_keys_flags_typo() {
        let v = serde_json::json!({
            "unsued-files": "warn",
            "unused-exports": "off",
        });
        let unknown = find_unknown_rule_keys(&v, "rules");
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].key, "unsued-files");
        assert_eq!(unknown[0].context, "rules");
        assert_eq!(unknown[0].suggestion, Some("unused-files"));
    }

    #[test]
    fn find_unknown_rule_keys_passes_aliases() {
        let v = serde_json::json!({
            "unused-file": "warn",
            "circular-dependency": "off",
            "boundary-violations": "warn",
        });
        let unknown = find_unknown_rule_keys(&v, "rules");
        assert!(
            unknown.is_empty(),
            "documented aliases must not flag as unknown: {unknown:?}"
        );
    }

    #[test]
    fn find_unknown_rule_keys_returns_multiple_typos() {
        let v = serde_json::json!({
            "unsued-files": "warn",
            "circular-dependnecy": "off",
        });
        let unknown = find_unknown_rule_keys(&v, "rules");
        assert_eq!(unknown.len(), 2);
    }

    #[test]
    fn find_unknown_rule_keys_carries_context() {
        let v = serde_json::json!({ "unsued-files": "warn" });
        let unknown = find_unknown_rule_keys(&v, "overrides[2].rules");
        assert_eq!(unknown[0].context, "overrides[2].rules");
    }

    #[test]
    fn find_unknown_rule_keys_empty_when_not_object() {
        let v = serde_json::json!(null);
        assert!(find_unknown_rule_keys(&v, "rules").is_empty());

        let v = serde_json::json!([1, 2, 3]);
        assert!(find_unknown_rule_keys(&v, "rules").is_empty());
    }

    #[test]
    fn find_unknown_rule_keys_no_suggestion_for_novel_name() {
        let v = serde_json::json!({ "totally-fabricated-rule": "warn" });
        let unknown = find_unknown_rule_keys(&v, "rules");
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].suggestion, None);
    }

    #[test]
    fn partial_rules_empty_json() {
        let partial: PartialRulesConfig = serde_json::from_str("{}").unwrap();
        assert!(partial.unused_files.is_none());
        assert!(partial.unused_exports.is_none());
        assert!(partial.unused_types.is_none());
        assert!(partial.unused_dependencies.is_none());
        assert!(partial.circular_dependencies.is_none());
        assert!(partial.boundary_violation.is_none());
        assert!(partial.coverage_gaps.is_none());
        assert!(partial.feature_flags.is_none());
        assert!(partial.stale_suppressions.is_none());
    }

    #[test]
    fn partial_rules_subset_json() {
        let json = r#"{
            "unused-files": "warn",
            "circular-dependencies": "off"
        }"#;
        let partial: PartialRulesConfig = serde_json::from_str(json).unwrap();
        assert_eq!(partial.unused_files, Some(Severity::Warn));
        assert_eq!(partial.circular_dependencies, Some(Severity::Off));
        assert!(partial.unused_exports.is_none());
    }

    #[test]
    fn partial_rules_deserialize_circular_dependency_alias() {
        let json = r#"{
            "circular-dependency": "warn"
        }"#;
        let partial: PartialRulesConfig = serde_json::from_str(json).unwrap();
        assert_eq!(partial.circular_dependencies, Some(Severity::Warn));
    }

    #[test]
    fn partial_rules_all_fields_json() {
        let json = r#"{
            "unused-files": "error",
            "unused-exports": "warn",
            "unused-types": "off",
            "unused-dependencies": "error",
            "unused-dev-dependencies": "warn",
            "unused-optional-dependencies": "off",
            "unused-enum-members": "error",
            "unused-class-members": "warn",
            "unresolved-imports": "off",
            "unlisted-dependencies": "error",
            "duplicate-exports": "warn",
            "type-only-dependencies": "off",
            "test-only-dependencies": "error",
            "circular-dependencies": "warn",
            "boundary-violation": "off",
            "coverage-gaps": "warn",
            "feature-flags": "error",
            "stale-suppressions": "off"
        }"#;
        let partial: PartialRulesConfig = serde_json::from_str(json).unwrap();
        assert_eq!(partial.unused_files, Some(Severity::Error));
        assert_eq!(partial.unused_exports, Some(Severity::Warn));
        assert_eq!(partial.unused_types, Some(Severity::Off));
        assert_eq!(partial.unused_dependencies, Some(Severity::Error));
        assert_eq!(partial.unused_dev_dependencies, Some(Severity::Warn));
        assert_eq!(partial.unused_optional_dependencies, Some(Severity::Off));
        assert_eq!(partial.unused_enum_members, Some(Severity::Error));
        assert_eq!(partial.unused_class_members, Some(Severity::Warn));
        assert_eq!(partial.unresolved_imports, Some(Severity::Off));
        assert_eq!(partial.unlisted_dependencies, Some(Severity::Error));
        assert_eq!(partial.duplicate_exports, Some(Severity::Warn));
        assert_eq!(partial.type_only_dependencies, Some(Severity::Off));
        assert_eq!(partial.test_only_dependencies, Some(Severity::Error));
        assert_eq!(partial.circular_dependencies, Some(Severity::Warn));
        assert_eq!(partial.boundary_violation, Some(Severity::Off));
        assert_eq!(partial.coverage_gaps, Some(Severity::Warn));
        assert_eq!(partial.feature_flags, Some(Severity::Error));
        assert_eq!(partial.stale_suppressions, Some(Severity::Off));
    }

    #[test]
    fn partial_rules_none_fields_not_serialized() {
        let partial = PartialRulesConfig::default();
        let json = serde_json::to_string(&partial).unwrap();
        assert_eq!(
            json, "{}",
            "all-None partial should serialize to empty object"
        );
    }

    #[test]
    fn partial_rules_some_fields_serialized() {
        let partial = PartialRulesConfig {
            unused_files: Some(Severity::Warn),
            ..Default::default()
        };
        let json = serde_json::to_string(&partial).unwrap();
        assert!(json.contains("unused-files"));
        assert!(!json.contains("unused-exports"));
    }

    #[test]
    fn severity_json_deserialization() {
        let error: Severity = serde_json::from_str(r#""error""#).unwrap();
        assert_eq!(error, Severity::Error);

        let warn: Severity = serde_json::from_str(r#""warn""#).unwrap();
        assert_eq!(warn, Severity::Warn);

        let off: Severity = serde_json::from_str(r#""off""#).unwrap();
        assert_eq!(off, Severity::Off);
    }

    #[test]
    fn severity_invalid_json_value_rejected() {
        let result: Result<Severity, _> = serde_json::from_str(r#""critical""#);
        assert!(result.is_err());
    }

    #[test]
    fn severity_default_is_error() {
        assert_eq!(Severity::default(), Severity::Error);
    }

    #[test]
    fn rules_config_json_roundtrip() {
        let rules = RulesConfig {
            unused_files: Severity::Warn,
            unused_exports: Severity::Off,
            type_only_dependencies: Severity::Error,
            ..RulesConfig::default()
        };
        let json = serde_json::to_string(&rules).unwrap();
        let restored: RulesConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.unused_files, Severity::Warn);
        assert_eq!(restored.unused_exports, Severity::Off);
        assert_eq!(restored.type_only_dependencies, Severity::Error);
        assert_eq!(restored.unused_dependencies, Severity::Error); // default
    }

    #[test]
    fn apply_partial_preserves_type_only_default() {
        let mut rules = RulesConfig::default();
        let partial = PartialRulesConfig {
            unused_files: Some(Severity::Off),
            ..Default::default()
        };
        rules.apply_partial(&partial);
        assert_eq!(rules.type_only_dependencies, Severity::Warn);
        assert_eq!(rules.test_only_dependencies, Severity::Warn);
    }
}
