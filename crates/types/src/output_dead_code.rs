//! Typed envelope wrappers for the simple 1:1 dead-code findings whose
//! actions are entirely determined by the wrapper type (no per-instance
//! discriminants beyond what the bare finding already exposes).
//!
//! Each wrapper flattens the bare finding via `#[serde(flatten)]` so the
//! wire shape matches the previous `actions`-grafted output byte-for-byte.
//! `actions` is populated at construction time via each wrapper's
//! `with_actions` constructor and replaces the per-finding `inject_actions`
//! post-pass in `crates/cli/src/report/json.rs`. `introduced` carries the optional audit
//! breadcrumb that `crates/cli/src/audit.rs::annotate_issue_array` inserts
//! into the JSON object via `map.insert`; the wrapper-level field stays
//! `None` when serialized directly from Rust and is set by the audit pass
//! only when the issue was introduced relative to the merge-base.
//!
//! All nine wrappers ship with `IssueAction` arrays today; they pay the
//! `serde_json` dependency cost because `IssueAction` transitively
//! references `AddToConfigValue::RuleObject(serde_json::Map<...>)`. The
//! variants the wrappers actually emit (`Fix`, `SuppressLine`,
//! `SuppressFile`, `AddToConfig`) are small, but reusing the existing enum
//! keeps the wire-shape contract identical to the legacy post-pass.
//!
//! `introduced` is typed as `Option<AuditIntroduced>` (transparent newtype
//! over `bool`) so the regenerated schema renders the field via
//! `$ref: #/definitions/AuditIntroduced`, matching the reference the prior
//! post-pass augmentation graft used. The audit pass continues to inject a
//! bare bool via `map.insert("introduced", ...)`; serde reads it back into
//! `AuditIntroduced` transparently. The field stays absent at the wire when
//! `None` (`skip_serializing_if`).

use serde::Serialize;
use std::path::Path;

use crate::envelope::AuditIntroduced;
use crate::output::{
    AddToConfigAction, AddToConfigKind, AddToConfigValue, FixAction, FixActionType,
    IgnoreExportsRule, IssueAction, SuppressFileAction, SuppressFileKind, SuppressLineAction,
    SuppressLineKind, SuppressLineScope,
};
use crate::results::{
    BoundaryCallViolation, BoundaryCoverageViolation, BoundaryViolation, CircularDependency,
    DependencyOverrideSource, DuplicateExport, DuplicatePropShape, DynamicSegmentNameConflict,
    EmptyCatalogGroup, InvalidClientExport, MisconfiguredDependencyOverride, MisplacedDirective,
    MixedClientServerBarrel, PolicyViolation, PrivateTypeLeak, PropDrillingChain, ReExportCycle,
    ReExportCycleKind, RouteCollision, TestOnlyDependency, ThinWrapper, TypeOnlyDependency,
    UnlistedDependency, UnprovidedInject, UnrenderedComponent, UnresolvedCatalogReference,
    UnresolvedImport, UnusedCatalogEntry, UnusedComponentEmit, UnusedComponentInput,
    UnusedComponentOutput, UnusedComponentProp, UnusedDependency, UnusedDependencyOverride,
    UnusedExport, UnusedFile, UnusedLoadDataKey, UnusedMember, UnusedServerAction,
    UnusedSvelteEvent,
};

/// Shared note for the `duplicate-exports` fix action. Mirrors the const used
/// by the human report (see `crates/cli/src/report/shared.rs`); kept here so
/// the wire-format builder reads from the same source of truth.
pub const NAMESPACE_BARREL_HINT: &str = "If every location is the sole `index.*` of its directory, this is likely an intentional namespace-barrel API. Prefer adding these files to `ignoreExports` over removing exports.";

/// JSON Schema fragment URL for the `add-to-config` `ignoreExports` action's
/// `value` payload. Pinned to the main branch so users browsing the action
/// value can navigate directly to the rule shape.
const IGNORE_EXPORTS_VALUE_SCHEMA: &str =
    "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreExports";

/// JSON Schema fragment URL for the `ignoreCatalogReferences` rule items
/// referenced by `add-to-config` actions on `unresolved-catalog-references`.
const IGNORE_CATALOG_REFERENCES_VALUE_SCHEMA: &str = "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreCatalogReferences/items";

/// JSON Schema fragment URL for the `ignoreDependencyOverrides` rule items
/// referenced by `add-to-config` actions on both the unused- and
/// misconfigured-override findings.
const IGNORE_DEPENDENCY_OVERRIDES_VALUE_SCHEMA: &str = "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreDependencyOverrides/items";

const PNPM_WORKSPACE_FILE: &str = "pnpm-workspace.yaml";

fn manual_framework_fix(kind: FixActionType, description: &str, note: &str) -> IssueAction {
    IssueAction::Fix(FixAction {
        kind,
        auto_fixable: false,
        description: description.to_string(),
        note: Some(note.to_string()),
        available_in_catalogs: None,
        suggested_target: None,
    })
}

fn suppress_line(comment: &str) -> IssueAction {
    IssueAction::SuppressLine(SuppressLineAction {
        kind: SuppressLineKind::SuppressLine,
        auto_fixable: false,
        description: "Suppress with an inline comment above the line".to_string(),
        comment: comment.to_string(),
        scope: None,
    })
}

/// Wire-shape envelope for an [`UnusedFile`] finding. The bare finding
/// flattens in via `#[serde(flatten)]`, with a typed `actions` array
/// populated at construction time and the audit-pass `introduced` flag
/// attached as an optional sibling.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedFileFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub file: UnusedFile,
    /// Suggested next steps: a `delete-file` primary and a `suppress-file`
    /// secondary. Always emitted (possibly empty for forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base. `None` when serialized directly from Rust.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedFileFinding {
    /// Build the wrapper from a raw [`UnusedFile`], computing the typed
    /// `actions` array inline. `introduced` stays `None` and is set later
    /// by `annotate_dead_code_json` if the audit pass runs.
    #[must_use]
    pub fn with_actions(file: UnusedFile) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::DeleteFile,
                auto_fixable: false,
                description: "Delete this file".to_string(),
                note: Some(
                    "File deletion may remove runtime functionality not visible to static analysis"
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressFile(SuppressFileAction {
                kind: SuppressFileKind::SuppressFile,
                auto_fixable: false,
                description: "Suppress with a file-level comment at the top of the file"
                    .to_string(),
                comment: "// fallow-ignore-file unused-file".to_string(),
            }),
        ];
        Self {
            file,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`PrivateTypeLeak`] finding. Mirrors
/// [`UnusedFileFinding`]: flattens the bare finding and carries a typed
/// `actions` array (`export-type` primary plus `suppress-line` secondary).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PrivateTypeLeakFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub leak: PrivateTypeLeak,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl PrivateTypeLeakFinding {
    /// Build the wrapper from a raw [`PrivateTypeLeak`].
    #[must_use]
    pub fn with_actions(leak: PrivateTypeLeak) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::ExportType,
                auto_fixable: false,
                description: "Export the referenced private type by name".to_string(),
                note: Some(
                    "Keep the type exported while it is part of a public signature".to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line private-type-leak".to_string(),
                scope: None,
            }),
        ];
        Self {
            leak,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnresolvedImport`] finding. Mirrors
/// [`UnusedFileFinding`]: flattens the bare finding and carries a typed
/// `actions` array (`resolve-import` primary plus config and inline
/// suppression actions).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnresolvedImportFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub import: UnresolvedImport,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnresolvedImportFinding {
    /// Build the wrapper from a raw [`UnresolvedImport`].
    #[must_use]
    pub fn with_actions(import: UnresolvedImport) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::ResolveImport,
                auto_fixable: false,
                description: "Fix the import specifier or install the missing module".to_string(),
                note: Some(
                    "Verify the module path and check tsconfig paths configuration".to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::AddToConfig(AddToConfigAction {
                kind: AddToConfigKind::AddToConfig,
                auto_fixable: false,
                description: format!(
                    "Add \"{}\" to ignoreUnresolvedImports in fallow config",
                    import.specifier
                ),
                config_key: "ignoreUnresolvedImports".to_string(),
                value: AddToConfigValue::Scalar(import.specifier.clone()),
                value_schema: Some(
                    "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreUnresolvedImports/items"
                        .to_string(),
                ),
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line unresolved-import".to_string(),
                scope: None,
            }),
        ];
        Self {
            import,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`CircularDependency`] finding. Mirrors
/// [`UnusedFileFinding`]: flattens the bare finding and carries a typed
/// `actions` array (`refactor-cycle` primary plus `suppress-line`
/// secondary).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CircularDependencyFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub cycle: CircularDependency,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl CircularDependencyFinding {
    /// Build the wrapper from a raw [`CircularDependency`].
    #[must_use]
    pub fn with_actions(cycle: CircularDependency) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RefactorCycle,
                auto_fixable: false,
                description: "Extract shared logic into a separate module to break the cycle"
                    .to_string(),
                note: Some(
                    "Circular imports can cause initialization issues and make code harder to reason about"
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line circular-dependency".to_string(),
                scope: None,
            }),
        ];
        Self {
            cycle,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`ReExportCycle`] finding. Mirrors
/// [`CircularDependencyFinding`]: flattens the bare finding and carries a
/// typed `actions` array (`refactor-re-export-cycle` informational primary
/// plus `suppress-file` secondary; cycles are file-scoped so a single
/// file-level suppression on the alphabetically-first member breaks the
/// cycle, and no `// fallow-ignore-next-line` form makes sense because the
/// diagnostic is anchored at line 1 col 0 of each member).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReExportCycleFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub cycle: ReExportCycle,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl ReExportCycleFinding {
    /// Build the wrapper from a raw [`ReExportCycle`].
    ///
    /// The `SuppressFile` action targets the alphabetically-first member
    /// (`cycle.files[0]`; the `files` Vec is already sorted at graph layer);
    /// for multi-node cycles the description names the other members so
    /// consumers see context for why one file-level suppression suffices.
    #[must_use]
    pub fn with_actions(cycle: ReExportCycle) -> Self {
        // The description is a path-free hint about the suppression's
        // structural effect; the cycle's member list already ships in the
        // sibling `files` field, so consumers can correlate without
        // re-reading the description (and absolute paths cannot leak in
        // here, which the wrapper has no root-prefix context to strip).
        let suppress_description = match cycle.kind {
            ReExportCycleKind::SelfLoop => {
                "Suppress with a file-level comment at the top of this file. \
                 The cycle is a self-loop, so the suppression covers the entire finding."
                    .to_string()
            }
            ReExportCycleKind::MultiNode => {
                "Suppress with a file-level comment at the top of this file. \
                 One suppression on any member breaks the cycle for every member \
                 (see the sibling `files` array)."
                    .to_string()
            }
        };
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RefactorReExportCycle,
                auto_fixable: false,
                description: "Remove one `export * from` (or `export { ... } from`) \
                              statement on any one member to break the cycle"
                    .to_string(),
                note: Some(
                    "Re-export cycles are structurally a no-op: chain propagation through \
                     the loop never reaches a terminating module, so imports from any member \
                     may silently come up empty."
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressFile(SuppressFileAction {
                kind: SuppressFileKind::SuppressFile,
                auto_fixable: false,
                description: suppress_description,
                comment: "// fallow-ignore-file re-export-cycle".to_string(),
            }),
        ];
        Self {
            cycle,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`BoundaryViolation`] finding. Mirrors
/// [`UnusedFileFinding`]: flattens the bare finding and carries a typed
/// `actions` array (`refactor-boundary` primary plus `suppress-line`
/// secondary).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundaryViolationFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub violation: BoundaryViolation,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl BoundaryViolationFinding {
    /// Build the wrapper from a raw [`BoundaryViolation`].
    #[must_use]
    pub fn with_actions(violation: BoundaryViolation) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RefactorBoundary,
                auto_fixable: false,
                description: "Move the import through an allowed zone or restructure the dependency"
                    .to_string(),
                note: Some(
                    "This import crosses an architecture boundary that is not permitted by the configured rules"
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line boundary-violation".to_string(),
                scope: None,
            }),
        ];
        Self {
            violation,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`BoundaryCoverageViolation`] finding. Carries
/// actions for assigning the file to a zone or explicitly allowing it to stay
/// unmatched.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundaryCoverageViolationFinding {
    /// The underlying coverage entry.
    #[serde(flatten)]
    pub violation: BoundaryCoverageViolation,
    /// Suggested next steps.
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl BoundaryCoverageViolationFinding {
    /// Build the wrapper from a raw [`BoundaryCoverageViolation`].
    #[must_use]
    pub fn with_actions(violation: BoundaryCoverageViolation) -> Self {
        let path = violation.path.to_string_lossy().replace('\\', "/");
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RefactorBoundary,
                auto_fixable: false,
                description: "Add this file to a boundary zone pattern or move it under an existing zone"
                    .to_string(),
                note: Some(
                    "Boundary coverage is enabled, so every analyzed source file must match a zone unless allow-listed"
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::AddToConfig(AddToConfigAction {
                kind: AddToConfigKind::AddToConfig,
                auto_fixable: false,
                description: format!(
                    "Add \"{path}\" to boundaries.coverage.allowUnmatched in fallow config"
                ),
                config_key: "boundaries.coverage.allowUnmatched".to_string(),
                value: AddToConfigValue::Scalar(path),
                value_schema: Some(
                    "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/boundaries/properties/coverage/properties/allowUnmatched/items"
                        .to_string(),
                ),
            }),
            IssueAction::SuppressFile(SuppressFileAction {
                kind: SuppressFileKind::SuppressFile,
                auto_fixable: false,
                description: "Suppress with a file-level comment at the top of the file"
                    .to_string(),
                comment: "// fallow-ignore-file boundary-violation".to_string(),
            }),
        ];
        Self {
            violation,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`BoundaryCallViolation`] finding. Carries
/// actions for refactoring the forbidden call out of the zone or suppressing
/// it with the shared `boundary-violation` token.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundaryCallViolationFinding {
    /// The underlying forbidden-call entry.
    #[serde(flatten)]
    pub violation: BoundaryCallViolation,
    /// Suggested next steps.
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl BoundaryCallViolationFinding {
    /// Build the wrapper from a raw [`BoundaryCallViolation`].
    #[must_use]
    pub fn with_actions(violation: BoundaryCallViolation) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RefactorBoundary,
                auto_fixable: false,
                description: format!(
                    "Move the `{}` call out of zone '{}' or behind an allowed abstraction",
                    violation.callee, violation.zone,
                ),
                note: Some(format!(
                    "`boundaries.calls.forbidden` bans callees matching `{}` from zone '{}'. The check is syntactic: it applies only to files classified into a zone and does not follow aliased or re-bound callees",
                    violation.pattern, violation.zone,
                )),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line boundary-violation".to_string(),
                scope: None,
            }),
            IssueAction::SuppressFile(SuppressFileAction {
                kind: SuppressFileKind::SuppressFile,
                auto_fixable: false,
                description: "Suppress with a file-level comment at the top of the file"
                    .to_string(),
                comment: "// fallow-ignore-file boundary-violation".to_string(),
            }),
        ];
        Self {
            violation,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`PolicyViolation`] finding. Carries actions for
/// replacing the banned call or import, or suppressing it with a scoped
/// `policy-violation:<pack>/<rule-id>` token.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PolicyViolationFinding {
    /// The underlying rule-pack policy entry.
    #[serde(flatten)]
    pub violation: PolicyViolation,
    /// Suggested next steps.
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl PolicyViolationFinding {
    /// Build the wrapper from a raw [`PolicyViolation`].
    #[must_use]
    pub fn with_actions(violation: PolicyViolation) -> Self {
        let what = match violation.kind {
            crate::results::PolicyRuleKind::BannedCall => "call",
            crate::results::PolicyRuleKind::BannedImport => "import",
        };
        let description = match &violation.message {
            Some(message) => format!("Replace the `{}` {what}: {message}", violation.matched),
            None => format!("Replace the `{}` {what}", violation.matched),
        };
        let suppress_token = format!("policy-violation:{}/{}", violation.pack, violation.rule_id);
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::ResolvePolicyViolation,
                auto_fixable: false,
                description,
                note: Some(format!(
                    "Rule `{}/{}` from the configured rule packs bans this {what}. The check is syntactic: it does not follow aliased or re-bound callees, and import matching uses the raw specifier",
                    violation.pack, violation.rule_id,
                )),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress this rule-pack rule with an inline comment above the line"
                    .to_string(),
                comment: format!("// fallow-ignore-next-line {suppress_token}"),
                scope: None,
            }),
            IssueAction::SuppressFile(SuppressFileAction {
                kind: SuppressFileKind::SuppressFile,
                auto_fixable: false,
                description:
                    "Suppress this rule-pack rule with a file-level comment at the top of the file"
                        .to_string(),
                comment: format!("// fallow-ignore-file {suppress_token}"),
            }),
        ];
        Self {
            violation,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedExport`] finding consumed under the
/// `unused_exports` key. Same Rust struct as [`UnusedTypeFinding`], with a
/// different fix description so consumers can tell value-export from
/// type-export removal at the action level.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedExportFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub export: UnusedExport,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedExportFinding {
    /// Build the wrapper. When `export.is_re_export` is true, the fix
    /// action's `note` warns about possible public-API surface; otherwise
    /// `note` is absent on the fix action.
    #[must_use]
    pub fn with_actions(export: UnusedExport) -> Self {
        let note = if export.is_re_export {
            Some(
                "This finding originates from a re-export; verify it is not part of your public API before removing"
                    .to_string(),
            )
        } else {
            None
        };
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RemoveExport,
                auto_fixable: true,
                description: "Remove the unused export from the public API".to_string(),
                note,
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line unused-export".to_string(),
                scope: None,
            }),
        ];
        Self {
            export,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedExport`] finding consumed under the
/// `unused_types` key. Wraps the same bare [`UnusedExport`] struct as
/// [`UnusedExportFinding`] but emits a fix action targeted at type-only
/// declarations, with the same `is_re_export`-aware note swap.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedTypeFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub export: UnusedExport,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedTypeFinding {
    /// Build the wrapper. `is_re_export` swaps the fix note the same way as
    /// [`UnusedExportFinding::with_actions`].
    #[must_use]
    pub fn with_actions(export: UnusedExport) -> Self {
        let note = if export.is_re_export {
            Some(
                "This finding originates from a re-export; verify it is not part of your public API before removing"
                    .to_string(),
            )
        } else {
            None
        };
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RemoveExport,
                auto_fixable: true,
                description:
                    "Remove the `export` (or `export type`) keyword from the type declaration"
                        .to_string(),
                note,
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line unused-type".to_string(),
                scope: None,
            }),
        ];
        Self {
            export,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`InvalidClientExport`] finding. There is no safe
/// auto-fix: the export itself may be a legitimate client-component value
/// export that happens to collide with a Next.js server-only name, so removing
/// it could break the component. Actions are a manual `move-to-server-module`
/// fix (the real remediation) plus a line-level suppress.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InvalidClientExportFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub export: InvalidClientExport,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl InvalidClientExportFinding {
    /// Build the wrapper from a raw [`InvalidClientExport`]. Emits a manual
    /// fix action (move the server-only export to a non-client module) plus a
    /// line-level suppress: there is no safe auto-fix because removing the
    /// export could break a legitimate client component.
    #[must_use]
    pub fn with_actions(export: InvalidClientExport) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::MoveToServerModule,
                auto_fixable: false,
                description: "Move the server-only export to a non-client module and import it from there"
                    .to_string(),
                note: Some(
                    "A \"use client\" file cannot export a Next.js server-only or route-config name; Next.js rejects it at build time"
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line invalid-client-export".to_string(),
                scope: None,
            }),
        ];
        Self {
            export,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`MixedClientServerBarrel`] finding. There is no
/// safe auto-fix: splitting a barrel into separate client and server modules is
/// a human decision (the barrel may intentionally aggregate both surfaces).
/// Actions are a manual `split-mixed-barrel` fix (the real remediation) plus a
/// line-level suppress.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MixedClientServerBarrelFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub barrel: MixedClientServerBarrel,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl MixedClientServerBarrelFinding {
    /// Build the wrapper from a raw [`MixedClientServerBarrel`]. Emits a manual
    /// fix action (split the barrel into separate client and server halves)
    /// plus a line-level suppress: there is no safe auto-fix because splitting
    /// the barrel is a human decision.
    #[must_use]
    pub fn with_actions(barrel: MixedClientServerBarrel) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::SplitMixedBarrel,
                auto_fixable: false,
                description: "Split the barrel so client and server-only modules are re-exported from separate files"
                    .to_string(),
                note: Some(
                    "Importing one name from this barrel drags the other's directive across the client/server boundary"
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line mixed-client-server-barrel".to_string(),
                scope: None,
            }),
        ];
        Self {
            barrel,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`MisplacedDirective`] finding. There is no safe
/// auto-fix: moving a directive to the leading prologue is a small but
/// judgement-bearing edit (the author may have intended the file to be a
/// server module after all). Actions are a manual `hoist-directive` fix (the
/// real remediation) plus a line-level suppress.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MisplacedDirectiveFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub directive_site: MisplacedDirective,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl MisplacedDirectiveFinding {
    /// Build the wrapper from a raw [`MisplacedDirective`]. Emits a manual fix
    /// action (hoist the directive to the leading prologue) plus a line-level
    /// suppress: there is no safe auto-fix because moving a directive can
    /// change module semantics and is a human decision.
    #[must_use]
    pub fn with_actions(directive_site: MisplacedDirective) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::HoistDirective,
                auto_fixable: false,
                description: "Move the directive to the very top of the file, above all imports and statements"
                    .to_string(),
                note: Some(
                    "An RSC bundler honors the directive only in the leading prologue; here it precedes other statements and is silently ignored"
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line misplaced-directive".to_string(),
                scope: None,
            }),
        ];
        Self {
            directive_site,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnprovidedInject`] finding. There is no safe
/// auto-fix: the fix is binary but judgement-bearing (add a `provide` for the
/// key, or delete the dead inject). Actions are manual remediation guidance
/// plus a line-level suppress.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnprovidedInjectFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub inject: UnprovidedInject,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnprovidedInjectFinding {
    /// Build the wrapper from a raw [`UnprovidedInject`]. Emits a manual fix
    /// action plus a line-level suppress.
    #[must_use]
    pub fn with_actions(inject: UnprovidedInject) -> Self {
        let actions = vec![
            manual_framework_fix(
                FixActionType::ProvideInject,
                "Provide this injected key, or remove the inject / getContext call",
                "Manual review required: dependency-injection keys can be provided by framework wiring, tests, or package consumers outside this project.",
            ),
            suppress_line("// fallow-ignore-next-line unprovided-inject"),
        ];
        Self {
            inject,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedServerAction`] finding. There is no safe
/// auto-fix: the fix is binary but judgement-bearing (wire the action up to a
/// consumer, or delete it). Actions are manual remediation guidance plus a
/// line-level suppress.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedServerActionFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub action: UnusedServerAction,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedServerActionFinding {
    /// Build the wrapper from a raw [`UnusedServerAction`]. Emits a manual fix
    /// action plus a line-level suppress.
    #[must_use]
    pub fn with_actions(action: UnusedServerAction) -> Self {
        let actions = vec![
            manual_framework_fix(
                FixActionType::WireServerAction,
                "Wire the server action to a caller or form action, or remove it",
                "Manual review required: server actions may still be POST-able by action id or invoked reflectively outside the static project graph.",
            ),
            suppress_line("// fallow-ignore-next-line unused-server-action"),
        ];
        Self {
            action,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedLoadDataKey`] finding. There is no safe
/// auto-fix: a `load()` fetch can have side effects, so deleting the key is a
/// human call. Actions are manual remediation guidance plus a line-level
/// suppress.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedLoadDataKeyFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub key: UnusedLoadDataKey,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedLoadDataKeyFinding {
    /// Build the wrapper from a raw [`UnusedLoadDataKey`]. Emits a manual fix
    /// action plus a line-level suppress.
    #[must_use]
    pub fn with_actions(key: UnusedLoadDataKey) -> Self {
        let actions = vec![
            manual_framework_fix(
                FixActionType::UseLoadData,
                "Read this load data key from the route UI, or remove it from the load return",
                "Manual review required: load functions can perform real server or database work, so verify side effects before deleting the producer.",
            ),
            suppress_line("// fallow-ignore-next-line unused-load-data-key"),
        ];
        Self {
            key,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnrenderedComponent`] finding. There is no safe
/// auto-fix: the fix is binary but judgement-bearing (render the component
/// somewhere, or delete the dead component). Actions are manual remediation
/// guidance plus a line-level suppress.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnrenderedComponentFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub component: UnrenderedComponent,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnrenderedComponentFinding {
    /// Build the wrapper from a raw [`UnrenderedComponent`]. Emits a manual
    /// fix action plus a line-level suppress.
    #[must_use]
    pub fn with_actions(component: UnrenderedComponent) -> Self {
        let actions = vec![
            manual_framework_fix(
                FixActionType::RenderComponent,
                "Render the reachable component from project code, or remove it",
                "Manual review required: exported library components and dynamic render registries can be intentionally reachable without static template usage.",
            ),
            suppress_line("// fallow-ignore-next-line unrendered-component"),
        ];
        Self {
            component,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedComponentProp`] finding. There is no safe
/// auto-fix: removing a declared prop is judgement-bearing (the prop may be part
/// of a deliberately-stable public component API). Actions are manual
/// remediation guidance plus a line-level suppress at the prop declaration.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentPropFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub prop: UnusedComponentProp,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedComponentPropFinding {
    /// Build the wrapper from a raw [`UnusedComponentProp`]. Emits a manual
    /// fix action plus a line-level suppress.
    #[must_use]
    pub fn with_actions(prop: UnusedComponentProp) -> Self {
        let actions = vec![
            manual_framework_fix(
                FixActionType::UseComponentProp,
                "Use the declared prop in the component, or remove it from the component API",
                "Manual review required: public component APIs can intentionally keep stable props for external consumers.",
            ),
            suppress_line("// fallow-ignore-next-line unused-component-prop"),
        ];
        Self {
            prop,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedComponentEmit`] finding. There is no safe
/// auto-fix: removing a declared emit is judgement-bearing (the event may be
/// part of a deliberately-stable public component API). Actions are manual
/// remediation guidance plus a line-level suppress at the emit declaration.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentEmitFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub emit: UnusedComponentEmit,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedComponentEmitFinding {
    /// Build the wrapper from a raw [`UnusedComponentEmit`]. Emits a manual
    /// fix action plus a line-level suppress.
    #[must_use]
    pub fn with_actions(emit: UnusedComponentEmit) -> Self {
        let actions = vec![
            manual_framework_fix(
                FixActionType::EmitComponentEvent,
                "Emit the declared event from the component, or remove it from the component API",
                "Manual review required: public component APIs can intentionally keep stable events for external listeners.",
            ),
            suppress_line("// fallow-ignore-next-line unused-component-emit"),
        ];
        Self {
            emit,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedSvelteEvent`] finding. There is no safe
/// auto-fix: removing a dispatched event is judgement-bearing (the event may be
/// part of a deliberately-stable public component API, or a listener may be
/// added later). Actions are manual remediation guidance plus a line-level
/// suppress at the `dispatch` call.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedSvelteEventFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub event: UnusedSvelteEvent,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedSvelteEventFinding {
    /// Build the wrapper from a raw [`UnusedSvelteEvent`]. Emits a manual fix
    /// action plus a line-level suppress.
    #[must_use]
    pub fn with_actions(event: UnusedSvelteEvent) -> Self {
        let actions = vec![
            manual_framework_fix(
                FixActionType::WireSvelteEvent,
                "Add or forward a listener for this custom event, or remove the dispatch",
                "Manual review required: public Svelte component APIs can intentionally dispatch events for package consumers outside this project.",
            ),
            suppress_line("// fallow-ignore-next-line unused-svelte-event"),
        ];
        Self {
            event,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`PropDrillingChain`] finding. There is no safe
/// auto-fix: collapsing a drilling chain (colocate the consumer, lift to a
/// context, or compose the component) is a design decision. The only action is a
/// line-level suppress at the source hop's prop declaration. The rule defaults
/// to `off` (opt-in health signal), so this finding is dormant by default.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PropDrillingChainFinding {
    /// The underlying located chain.
    #[serde(flatten)]
    pub chain: PropDrillingChain,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl PropDrillingChainFinding {
    /// Build the wrapper from a raw [`PropDrillingChain`]. Emits only a
    /// line-level suppress action anchored at the source hop: there is no safe
    /// auto-fix because collapsing the chain is a design decision (colocate,
    /// lift to context, or compose).
    #[must_use]
    pub fn with_actions(chain: PropDrillingChain) -> Self {
        let actions = vec![IssueAction::SuppressLine(SuppressLineAction {
            kind: SuppressLineKind::SuppressLine,
            auto_fixable: false,
            description: "Suppress with an inline comment above the source prop declaration"
                .to_string(),
            comment: "// fallow-ignore-next-line prop-drilling".to_string(),
            scope: None,
        })];
        Self {
            chain,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`ThinWrapper`] finding. There is no safe
/// auto-fix: inlining a thin wrapper at its call sites (or deleting it) is a
/// design decision. The only action is a line-level suppress at the wrapper's
/// definition. The rule defaults to `off` (opt-in health signal), so this
/// finding is dormant by default.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ThinWrapperFinding {
    /// The underlying located thin wrapper.
    #[serde(flatten)]
    pub wrapper: ThinWrapper,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl ThinWrapperFinding {
    /// Build the wrapper from a raw [`ThinWrapper`]. Emits only a line-level
    /// suppress action anchored at the wrapper definition: there is no safe
    /// auto-fix because inlining or deleting the wrapper is a design decision.
    #[must_use]
    pub fn with_actions(wrapper: ThinWrapper) -> Self {
        let actions = vec![IssueAction::SuppressLine(SuppressLineAction {
            kind: SuppressLineKind::SuppressLine,
            auto_fixable: false,
            description: "Suppress with an inline comment above the component definition"
                .to_string(),
            comment: "// fallow-ignore-next-line thin-wrapper".to_string(),
            scope: None,
        })];
        Self {
            wrapper,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`DuplicatePropShape`] finding. There is no safe
/// auto-fix: extracting a shared `Props` type or a base component for a group of
/// same-shaped components is a design decision. The actions are manual guidance
/// (extract the shared shape) plus a line-level suppress at the component
/// definition and a file-level suppress escape hatch (mirroring the
/// route-collision multi-file model). The rule defaults to `off` (opt-in health
/// signal), so this finding is dormant by default.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicatePropShapeFinding {
    /// The underlying duplicate-prop-shape entry.
    #[serde(flatten)]
    pub shape: DuplicatePropShape,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl DuplicatePropShapeFinding {
    /// Build the wrapper from a raw [`DuplicatePropShape`]. Manual guidance is
    /// the primary action (extract a shared shape); a line-level suppress at the
    /// component definition and a file-level suppress escape hatch follow,
    /// mirroring the multi-file route-collision suppress model. There is no safe
    /// auto-fix because extracting a shared type or base component is a design
    /// decision.
    #[must_use]
    pub fn with_actions(shape: DuplicatePropShape) -> Self {
        let actions = vec![
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Three or more components share this exact prop shape. Extract one \
                              shared `Props` type (or a base component) that every member reuses, \
                              or keep them separate if a per-variant divergence is planned. \
                              Suppress one member with an inline comment above the component \
                              definition."
                    .to_string(),
                comment: "// fallow-ignore-next-line duplicate-prop-shape".to_string(),
                scope: None,
            }),
            IssueAction::SuppressFile(SuppressFileAction {
                kind: SuppressFileKind::SuppressFile,
                auto_fixable: false,
                description: "Escape hatch: a file-level suppress silences this member but it \
                              still appears in its siblings' `sharing_components` (the group is \
                              real regardless of suppression)."
                    .to_string(),
                comment: "// fallow-ignore-file duplicate-prop-shape".to_string(),
            }),
        ];
        Self {
            shape,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedComponentInput`] finding. There is no safe
/// auto-fix: removing a declared input is judgement-bearing (the input may be
/// part of a deliberately-stable public component API). The only action is a
/// line-level suppress at the input declaration.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentInputFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub input: UnusedComponentInput,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedComponentInputFinding {
    /// Build the wrapper from a raw [`UnusedComponentInput`]. Emits only a
    /// line-level suppress action: there is no safe auto-fix because removing an
    /// input is a human decision (it may be part of a stable component API).
    #[must_use]
    pub fn with_actions(input: UnusedComponentInput) -> Self {
        let actions = vec![IssueAction::SuppressLine(SuppressLineAction {
            kind: SuppressLineKind::SuppressLine,
            auto_fixable: false,
            description: "Suppress with an inline comment above the line".to_string(),
            comment: "// fallow-ignore-next-line unused-component-input".to_string(),
            scope: None,
        })];
        Self {
            input,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedComponentOutput`] finding. There is no safe
/// auto-fix: removing a declared output is judgement-bearing (the event may be
/// part of a deliberately-stable public component API). The only action is a
/// line-level suppress at the output declaration.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentOutputFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub output: UnusedComponentOutput,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedComponentOutputFinding {
    /// Build the wrapper from a raw [`UnusedComponentOutput`]. Emits only a
    /// line-level suppress action: there is no safe auto-fix because removing an
    /// output is a human decision (it may be part of a stable component API).
    #[must_use]
    pub fn with_actions(output: UnusedComponentOutput) -> Self {
        let actions = vec![IssueAction::SuppressLine(SuppressLineAction {
            kind: SuppressLineKind::SuppressLine,
            auto_fixable: false,
            description: "Suppress with an inline comment above the line".to_string(),
            comment: "// fallow-ignore-next-line unused-component-output".to_string(),
            scope: None,
        })];
        Self {
            output,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`RouteCollision`] finding. A route collision is a
/// guaranteed `next build` failure, so the PRIMARY action is manual guidance
/// (move or merge one of the colliding files), NOT a suppress: suppressing a
/// build error never makes the build pass. A file-level suppress is offered as
/// an escape hatch only.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RouteCollisionFinding {
    /// The underlying route-collision entry.
    #[serde(flatten)]
    pub collision: RouteCollision,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl RouteCollisionFinding {
    /// Build the wrapper from a raw [`RouteCollision`]. The primary action is
    /// manual guidance because suppressing a guaranteed build error is never
    /// the right fix; a file-level suppress is the escape hatch only.
    #[must_use]
    pub fn with_actions(collision: RouteCollision) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::ResolveRouteCollision,
                auto_fixable: false,
                description: "Two or more files resolve to the same URL. Move or merge one so \
                              each URL has a single owner. Route groups `(name)` and parallel \
                              slots `@name` are the only legal same-URL shapes."
                    .to_string(),
                note: Some(
                    "Next.js fails the build with \"You cannot have two parallel pages that \
                     resolve to the same path\". See the sibling `conflicting_paths` array for \
                     the other files that own this URL."
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressFile(SuppressFileAction {
                kind: SuppressFileKind::SuppressFile,
                auto_fixable: false,
                description: "Escape hatch only: a file-level suppress silences the finding but \
                              does NOT make `next build` pass. Prefer moving or merging a file."
                    .to_string(),
                comment: "// fallow-ignore-file route-collision".to_string(),
            }),
        ];
        Self {
            collision,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`DynamicSegmentNameConflict`] finding. The
/// conflict is a Next.js dev / runtime error (`next build` does NOT catch it),
/// so the primary action is manual guidance (rename the dynamic segments to a
/// single consistent slug name), with a file-level suppress as escape hatch.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DynamicSegmentNameConflictFinding {
    /// The underlying dynamic-segment-name-conflict entry.
    #[serde(flatten)]
    pub conflict: DynamicSegmentNameConflict,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl DynamicSegmentNameConflictFinding {
    /// Build the wrapper from a raw [`DynamicSegmentNameConflict`]. Manual
    /// guidance primary action; file-level suppress escape hatch only.
    #[must_use]
    pub fn with_actions(conflict: DynamicSegmentNameConflict) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::ResolveDynamicSegmentNameConflict,
                auto_fixable: false,
                description: "Sibling dynamic segments at the same position use different param \
                              names. Rename them to one consistent slug name (e.g. pick `[id]` \
                              or `[slug]` for both)."
                    .to_string(),
                note: Some(
                    "Next.js throws \"You cannot use different slug names for the same dynamic \
                     path\" at dev / runtime when the position is hit; `next build` does not \
                     catch it. See the sibling `conflicting_segments` array."
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressFile(SuppressFileAction {
                kind: SuppressFileKind::SuppressFile,
                auto_fixable: false,
                description: "Escape hatch only: a file-level suppress silences the finding but \
                              does NOT stop Next.js from throwing at dev / runtime. Prefer \
                              renaming the segments."
                    .to_string(),
                comment: "// fallow-ignore-file dynamic-segment-name-conflict".to_string(),
            }),
        ];
        Self {
            conflict,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedMember`] finding consumed under the
/// `unused_enum_members` key.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedEnumMemberFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub member: UnusedMember,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedEnumMemberFinding {
    /// Build the wrapper from a raw [`UnusedMember`].
    #[must_use]
    pub fn with_actions(member: UnusedMember) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RemoveEnumMember,
                auto_fixable: true,
                description: "Remove this enum member".to_string(),
                note: None,
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line unused-enum-member".to_string(),
                scope: None,
            }),
        ];
        Self {
            member,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedMember`] finding consumed under the
/// `unused_class_members` key. Same Rust struct as
/// [`UnusedEnumMemberFinding`]; the fix action and suppress comment carry
/// the class-member kebab-case identifier instead.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedClassMemberFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub member: UnusedMember,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedClassMemberFinding {
    /// Build the wrapper from a raw [`UnusedMember`]. Class-member fixes
    /// are not auto-applied (members can be used via dependency injection
    /// or decorators), so `auto_fixable` is `false` and a context note is
    /// attached.
    #[must_use]
    pub fn with_actions(member: UnusedMember) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RemoveClassMember,
                auto_fixable: false,
                description: "Remove this class member".to_string(),
                note: Some(
                    "Class member may be used via dependency injection or decorators".to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line unused-class-member".to_string(),
                scope: None,
            }),
        ];
        Self {
            member,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedMember`] finding consumed under the
/// `unused_store_members` key (a Pinia `state` / `getters` / `actions` key, or
/// a setup-store returned key, declared but never accessed by any consumer
/// project-wide). Same Rust struct as [`UnusedClassMemberFinding`]. Emits only
/// a line-level suppress action: there is no safe auto-fix because a store
/// member can be accessed reflectively (a Pinia plugin, `store.$onAction`, or
/// dynamic dispatch) in ways syntactic analysis cannot see, so removal is a
/// behavioral change the user must own.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedStoreMemberFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub member: UnusedMember,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedStoreMemberFinding {
    /// Build the wrapper from a raw [`UnusedMember`]. Emits only a line-level
    /// suppress action (no auto-fix: store members can be accessed
    /// reflectively, so removal is never provably safe).
    #[must_use]
    pub fn with_actions(member: UnusedMember) -> Self {
        let actions = vec![IssueAction::SuppressLine(SuppressLineAction {
            kind: SuppressLineKind::SuppressLine,
            auto_fixable: false,
            description: "Suppress with an inline comment above the line".to_string(),
            comment: "// fallow-ignore-next-line unused-store-member".to_string(),
            scope: None,
        })];
        Self {
            member,
            actions,
            introduced: None,
        }
    }
}

/// Build the `IssueAction` vec for the three `unused_dependencies`,
/// `unused_dev_dependencies`, `unused_optional_dependencies` views over the
/// same bare [`UnusedDependency`] struct. Each wrapper differs only in the
/// `package_json_location` string (`"dependencies"` / `"devDependencies"` /
/// `"optionalDependencies"`) baked into the fix-action description and in
/// the `suppress_issue_kind` used by the inline-suppress comment. All three
/// share the cross-workspace swap (when `dep.used_in_workspaces` is
/// non-empty the primary fix flips from `remove-dependency` to
/// `move-dependency` because the dep is imported by ANOTHER workspace and
/// `fallow fix` cannot safely remove it).
fn build_unused_dependency_actions(
    dep: &UnusedDependency,
    package_json_location: &str,
    suppress_issue_kind: &str,
) -> Vec<IssueAction> {
    let mut actions = Vec::with_capacity(2);
    let cross_workspace = !dep.used_in_workspaces.is_empty();
    actions.push(if cross_workspace {
        IssueAction::Fix(FixAction {
            kind: FixActionType::MoveDependency,
            auto_fixable: false,
            description: "Move this dependency to the workspace package.json that imports it"
                .to_string(),
            note: Some(
                "fallow fix will not remove dependencies that are imported by another workspace"
                    .to_string(),
            ),
            available_in_catalogs: None,
            suggested_target: None,
        })
    } else {
        IssueAction::Fix(FixAction {
            kind: FixActionType::RemoveDependency,
            auto_fixable: true,
            description: format!("Remove from {package_json_location} in package.json"),
            note: None,
            available_in_catalogs: None,
            suggested_target: None,
        })
    });
    actions.push(build_ignore_dependencies_suppress_action(
        &dep.package_name,
        suppress_issue_kind,
    ));
    actions
}

/// Build the standard `add-to-config` `ignoreDependencies` suppress action
/// for any finding whose primary key is a package name. Used by the four
/// dependency-family wrappers (unused / unlisted / type-only / test-only).
/// The `_suppress_issue_kind` argument is currently unused; the pre-2.76
/// `inject_actions` post-pass also did not embed the issue kind in this
/// shape (no inline `// fallow-ignore-next-line ...` comment because the
/// finding is anchored at a package.json line, not at a source-file line).
fn build_ignore_dependencies_suppress_action(
    package_name: &str,
    _suppress_issue_kind: &str,
) -> IssueAction {
    IssueAction::AddToConfig(AddToConfigAction {
        kind: AddToConfigKind::AddToConfig,
        auto_fixable: false,
        description: format!("Add \"{package_name}\" to ignoreDependencies in fallow config"),
        config_key: "ignoreDependencies".to_string(),
        value: AddToConfigValue::Scalar(package_name.to_string()),
        value_schema: Some(
            "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreDependencies/items"
                .to_string(),
        ),
    })
}

/// Wire-shape envelope for an [`UnusedDependency`] finding consumed under
/// the `unused_dependencies` key (production deps). Flattens the bare
/// finding; the typed `actions` array carries either a `remove-dependency`
/// or `move-dependency` primary depending on
/// `inner.used_in_workspaces`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedDependencyFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub dep: UnusedDependency,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedDependencyFinding {
    /// Build the wrapper. Switches the primary fix from `remove-dependency`
    /// to `move-dependency` when the dep is imported by another workspace.
    #[must_use]
    pub fn with_actions(dep: UnusedDependency) -> Self {
        let actions = build_unused_dependency_actions(&dep, "dependencies", "unused-dependency");
        Self {
            dep,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedDependency`] finding consumed under
/// the `unused_dev_dependencies` key. Same bare struct as
/// [`UnusedDependencyFinding`]; the fix description points at
/// `devDependencies` and the suppress comment uses
/// `unused-dev-dependency`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedDevDependencyFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub dep: UnusedDependency,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedDevDependencyFinding {
    /// Build the wrapper.
    #[must_use]
    pub fn with_actions(dep: UnusedDependency) -> Self {
        let actions =
            build_unused_dependency_actions(&dep, "devDependencies", "unused-dev-dependency");
        Self {
            dep,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedDependency`] finding consumed under
/// the `unused_optional_dependencies` key. Same bare struct as
/// [`UnusedDependencyFinding`]; the fix description points at
/// `optionalDependencies`. Reuses the `unused-dependency` suppress
/// `IssueKind` because there is no dedicated variant for optional deps.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedOptionalDependencyFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub dep: UnusedDependency,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedOptionalDependencyFinding {
    /// Build the wrapper.
    #[must_use]
    pub fn with_actions(dep: UnusedDependency) -> Self {
        let actions =
            build_unused_dependency_actions(&dep, "optionalDependencies", "unused-dependency");
        Self {
            dep,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnlistedDependency`] finding. Carries an
/// `install-dependency` primary (non-auto-fixable) plus the standard
/// `ignoreDependencies` config suppress.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnlistedDependencyFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub dep: UnlistedDependency,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnlistedDependencyFinding {
    /// Build the wrapper.
    #[must_use]
    pub fn with_actions(dep: UnlistedDependency) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::InstallDependency,
                auto_fixable: false,
                description: "Add this package to dependencies in package.json".to_string(),
                note: Some(
                    "Verify this package should be a direct dependency before adding".to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            build_ignore_dependencies_suppress_action(&dep.package_name, "unlisted-dependency"),
        ];
        Self {
            dep,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`TypeOnlyDependency`] finding. Carries a
/// `move-to-dev` primary plus the standard `ignoreDependencies` config
/// suppress.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeOnlyDependencyFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub dep: TypeOnlyDependency,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl TypeOnlyDependencyFinding {
    /// Build the wrapper.
    #[must_use]
    pub fn with_actions(dep: TypeOnlyDependency) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::MoveToDev,
                auto_fixable: false,
                description: "Move to devDependencies (only type imports are used)".to_string(),
                note: Some(
                    "Type imports are erased at runtime so this dependency is not needed in production"
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            build_ignore_dependencies_suppress_action(&dep.package_name, "type-only-dependency"),
        ];
        Self {
            dep,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`TestOnlyDependency`] finding. Carries a
/// `move-to-dev` primary (different prose than [`TypeOnlyDependencyFinding`])
/// plus the standard `ignoreDependencies` config suppress.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TestOnlyDependencyFinding {
    /// The underlying dead-code entry.
    #[serde(flatten)]
    pub dep: TestOnlyDependency,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl TestOnlyDependencyFinding {
    /// Build the wrapper.
    #[must_use]
    pub fn with_actions(dep: TestOnlyDependency) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::MoveToDev,
                auto_fixable: false,
                description: "Move to devDependencies (only test files import this)".to_string(),
                note: Some(
                    "Only test files import this package so it does not need to be a production dependency"
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            build_ignore_dependencies_suppress_action(&dep.package_name, "test-only-dependency"),
        ];
        Self {
            dep,
            actions,
            introduced: None,
        }
    }
}

// ── Catalog / dep-override family ───────────────────────────────
//
// These six wrappers replace the legacy `inject_actions` post-pass in
// `crates/cli/src/report/json.rs` for the catalog and dependency-override
// findings. Each `with_actions(...)` builds the typed `actions` array
// directly from the inner struct (and any per-call context such as
// `config_fixable`), so the wire shape is identical to the pre-2.76
// post-pass output but the Rust compiler now owns the action contract.

/// Wire-shape envelope for a [`DuplicateExport`] finding. Carries up to
/// three actions in position-locked order: an `add-to-config` `ignoreExports`
/// snippet (only when `locations[]` carries at least one path) followed by
/// the `remove-duplicate` fix and the multi-location suppress.
///
/// The `add-to-config` action sits at position 0 because the documented
/// primary slot points at the safe, non-destructive path: the shadcn /
/// Radix / bits-ui namespace-barrel case where every `index.*` reexports
/// the directory's neighbours. The `remove-duplicate` fix stays as the
/// secondary so consumers that pattern-match on `actions[0].type` for
/// "primary fix" never propose deletion of an intentional barrel surface.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicateExportFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub export: DuplicateExport,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl DuplicateExportFinding {
    /// Build the wrapper with the `add-to-config` action's `auto_fixable`
    /// defaulting to `false`. The CLI's `build_json_with_config_fixable`
    /// path layers the actual `config_fixable` signal via
    /// [`Self::set_config_fixable`] right before serialization (the
    /// fix-applier readiness check lives in `fallow-cli::fix` and is not
    /// reachable from the analyzer layer where wrappers are first built).
    /// Embedders that build `AnalysisResults` directly and never route
    /// through the CLI's JSON path keep the conservative default.
    #[must_use]
    pub fn with_actions(export: DuplicateExport) -> Self {
        let mut actions: Vec<IssueAction> = Vec::with_capacity(3);

        if let Some(rules) = build_duplicate_exports_ignore_rules(&export) {
            actions.push(IssueAction::AddToConfig(AddToConfigAction {
                kind: AddToConfigKind::AddToConfig,
                auto_fixable: false,
                description: "Add an ignoreExports rule so these files are excluded from duplicate-export grouping (use when this duplication is an intentional namespace-barrel API).".to_string(),
                config_key: "ignoreExports".to_string(),
                value: AddToConfigValue::ExportsRules(rules),
                value_schema: Some(IGNORE_EXPORTS_VALUE_SCHEMA.to_string()),
            }));
        }

        actions.push(IssueAction::Fix(FixAction {
            kind: FixActionType::RemoveDuplicate,
            auto_fixable: false,
            description: "Keep one canonical export location and remove the others".to_string(),
            note: Some(NAMESPACE_BARREL_HINT.to_string()),
            available_in_catalogs: None,
            suggested_target: None,
        }));

        actions.push(IssueAction::SuppressLine(SuppressLineAction {
            kind: SuppressLineKind::SuppressLine,
            auto_fixable: false,
            description: "Suppress with an inline comment above the line".to_string(),
            comment: "// fallow-ignore-next-line duplicate-export".to_string(),
            scope: Some(SuppressLineScope::PerLocation),
        }));

        Self {
            export,
            actions,
            introduced: None,
        }
    }

    /// Update the position-0 `add-to-config` action's `auto_fixable` flag.
    /// Idempotent and a no-op when position 0 is not an `add-to-config`
    /// action (happens when the finding has no locations). Called by the
    /// CLI's JSON serializer with the result of
    /// `crate::fix::is_config_fixable` before emitting bytes.
    pub fn set_config_fixable(&mut self, fixable: bool) {
        if let Some(IssueAction::AddToConfig(action)) = self.actions.first_mut() {
            action.auto_fixable = fixable;
        }
    }
}

/// Build a paste-ready `ignoreExports` config value from a duplicate-export
/// finding's locations. Returns one `{ file, exports: ["*"] }` entry per
/// distinct file in insertion order. `None` when no locations carry a path.
fn build_duplicate_exports_ignore_rules(
    export: &DuplicateExport,
) -> Option<Vec<IgnoreExportsRule>> {
    let mut entries: Vec<IgnoreExportsRule> = Vec::with_capacity(export.locations.len());
    for loc in &export.locations {
        // Normalize separators to forward slashes so pasting the action value
        // into `.fallowrc.json` produces a portable rule. On Windows
        // `to_string_lossy` preserves backslashes, which the old
        // `inject_actions` post-pass implicitly normalized because it read
        // the path AFTER `strip_root_prefix` had already run through
        // `normalize_uri`; the typed wrapper builds the value before
        // serialization, so the normalization has to be explicit here.
        let path = loc.path.to_string_lossy().replace('\\', "/");
        if path.is_empty() {
            continue;
        }
        if entries.iter().any(|existing| existing.file == path) {
            continue;
        }
        entries.push(IgnoreExportsRule {
            file: path,
            exports: vec!["*".to_string()],
        });
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Wire-shape envelope for an [`UnusedCatalogEntry`] finding. Per-instance
/// `auto_fixable` flips to `false` when `hardcoded_consumers` is non-empty or
/// the source is not `pnpm-workspace.yaml`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedCatalogEntryFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub entry: UnusedCatalogEntry,
    /// Suggested next steps. Always emitted.
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedCatalogEntryFinding {
    /// Build the wrapper. Per-instance `auto_fixable` is `true` only when
    /// `hardcoded_consumers` is empty and the source is `pnpm-workspace.yaml`;
    /// otherwise `fallow fix` skips the entry to avoid breaking installs or
    /// applying YAML edits to Bun `package.json` catalogs.
    #[must_use]
    pub fn with_actions(entry: UnusedCatalogEntry) -> Self {
        let is_pnpm_source = is_pnpm_catalog_source(&entry.path);
        let auto_fixable = entry.hardcoded_consumers.is_empty() && is_pnpm_source;
        let note = if is_pnpm_source {
            Some(
                "If any consumer declares the same package with a hardcoded version, switch the consumer to `catalog:` before removing"
                    .to_string(),
            )
        } else {
            Some(
                "fallow fix only edits pnpm-workspace.yaml catalog entries. Edit Bun package.json catalogs manually."
                    .to_string(),
            )
        };
        let mut actions = vec![IssueAction::Fix(FixAction {
            kind: FixActionType::RemoveCatalogEntry,
            auto_fixable,
            description: if is_pnpm_source {
                "Remove the entry from pnpm-workspace.yaml".to_string()
            } else {
                "Remove the entry from the catalog source file manually".to_string()
            },
            note,
            available_in_catalogs: None,
            suggested_target: None,
        })];
        if is_pnpm_source {
            actions.push(IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with a YAML comment above the line".to_string(),
                comment: "# fallow-ignore-next-line unused-catalog-entry".to_string(),
                scope: None,
            }));
        }
        Self {
            entry,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`EmptyCatalogGroup`] finding. Carries a
/// `remove-empty-catalog-group` primary. YAML-sourced findings also include a
/// YAML-comment suppress action.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EmptyCatalogGroupFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub group: EmptyCatalogGroup,
    /// Suggested next steps. Always emitted.
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl EmptyCatalogGroupFinding {
    /// Build the wrapper.
    #[must_use]
    pub fn with_actions(group: EmptyCatalogGroup) -> Self {
        let auto_fixable = is_pnpm_catalog_source(&group.path);
        let mut actions = vec![IssueAction::Fix(FixAction {
            kind: FixActionType::RemoveEmptyCatalogGroup,
            auto_fixable,
            description: if auto_fixable {
                "Remove the empty named catalog group from pnpm-workspace.yaml".to_string()
            } else {
                "Remove the empty named catalog group from the catalog source file manually"
                    .to_string()
            },
            note: Some(if auto_fixable {
                "Only named groups under `catalogs:` are flagged; the top-level `catalog:` hook is intentionally ignored"
                    .to_string()
            } else {
                "fallow fix only edits pnpm-workspace.yaml catalog groups. Edit Bun package.json catalogs manually."
                    .to_string()
            }),
            available_in_catalogs: None,
            suggested_target: None,
        })];
        if auto_fixable {
            actions.push(IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with a YAML comment above the line".to_string(),
                comment: "# fallow-ignore-next-line empty-catalog-group".to_string(),
                scope: None,
            }));
        }
        Self {
            group,
            actions,
            introduced: None,
        }
    }
}

fn is_pnpm_catalog_source(path: &Path) -> bool {
    path == Path::new(PNPM_WORKSPACE_FILE)
}

/// Wire-shape envelope for an [`UnresolvedCatalogReference`] finding. The
/// primary action at position 0 discriminates on `available_in_catalogs`:
/// `add-catalog-entry` when the array is empty (no other catalog declares
/// the package), or `update-catalog-reference` when at least one
/// alternative exists. When exactly one alternative exists, the action
/// also carries `suggested_target` so deterministic agents can land the
/// edit without picking from a list.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnresolvedCatalogReferenceFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub reference: UnresolvedCatalogReference,
    /// Suggested next steps. Always emitted; position 0 is the discriminated
    /// primary (see struct docs).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnresolvedCatalogReferenceFinding {
    /// Build the wrapper. The discriminator at position 0 is the
    /// `add-catalog-entry` vs `update-catalog-reference` pick documented on
    /// the struct.
    #[must_use]
    pub fn with_actions(reference: UnresolvedCatalogReference) -> Self {
        // Normalize separators to forward slashes so the
        // `ignoreCatalogReferences.consumer` action value is portable when
        // pasted into a Windows-authored config. See
        // `build_duplicate_exports_ignore_rules` for the same pattern.
        let consumer_path = reference.path.to_string_lossy().replace('\\', "/");
        let primary = if reference.available_in_catalogs.is_empty() {
            IssueAction::Fix(FixAction {
                kind: FixActionType::AddCatalogEntry,
                auto_fixable: false,
                description: format!(
                    "Add `{}` to the `{}` catalog in pnpm-workspace.yaml",
                    reference.entry_name, reference.catalog_name
                ),
                note: Some(
                    "Pin a version that satisfies the consumer's import; no other catalog declares this package today"
                        .to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            })
        } else {
            let available = reference.available_in_catalogs.clone();
            let suggested_target = (available.len() == 1).then(|| available[0].clone());
            IssueAction::Fix(FixAction {
                kind: FixActionType::UpdateCatalogReference,
                auto_fixable: false,
                description: format!(
                    "Switch the reference from `catalog:{}` to a catalog that declares `{}`",
                    reference.catalog_name, reference.entry_name
                ),
                note: None,
                available_in_catalogs: Some(available),
                suggested_target,
            })
        };

        let fallback = IssueAction::Fix(FixAction {
            kind: FixActionType::RemoveCatalogReference,
            auto_fixable: false,
            description:
                "Remove the catalog reference and pin a hardcoded version in package.json"
                    .to_string(),
            note: Some(
                "Use only when neither another catalog declares the package nor the named catalog should grow to include it"
                    .to_string(),
            ),
            available_in_catalogs: None,
            suggested_target: None,
        });

        let mut suppress_value = serde_json::Map::new();
        suppress_value.insert(
            "package".to_string(),
            serde_json::Value::String(reference.entry_name.clone()),
        );
        suppress_value.insert(
            "catalog".to_string(),
            serde_json::Value::String(reference.catalog_name.clone()),
        );
        suppress_value.insert(
            "consumer".to_string(),
            serde_json::Value::String(consumer_path),
        );
        let suppress = IssueAction::AddToConfig(AddToConfigAction {
            kind: AddToConfigKind::AddToConfig,
            auto_fixable: false,
            description: "Suppress this reference via ignoreCatalogReferences in fallow config (use when the catalog edit is intentionally landing in a separate PR or the package is a placeholder).".to_string(),
            config_key: "ignoreCatalogReferences".to_string(),
            value: AddToConfigValue::RuleObject(suppress_value),
            value_schema: Some(IGNORE_CATALOG_REFERENCES_VALUE_SCHEMA.to_string()),
        });

        Self {
            reference,
            actions: vec![primary, fallback, suppress],
            introduced: None,
        }
    }
}

/// Wire-shape envelope for an [`UnusedDependencyOverride`] finding. Carries
/// a `remove-dependency-override` primary plus an `add-to-config`
/// `ignoreDependencyOverrides` suppress scoped to the target package and
/// declaration source.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedDependencyOverrideFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub entry: UnusedDependencyOverride,
    /// Suggested next steps. Always emitted.
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl UnusedDependencyOverrideFinding {
    /// Build the wrapper.
    #[must_use]
    pub fn with_actions(entry: UnusedDependencyOverride) -> Self {
        let mut actions: Vec<IssueAction> = Vec::with_capacity(2);
        actions.push(IssueAction::Fix(FixAction {
            kind: FixActionType::RemoveDependencyOverride,
            auto_fixable: false,
            description: "Remove the override entry from pnpm-workspace.yaml or pnpm.overrides"
                .to_string(),
            note: Some(
                "Conservative static check; verify against `pnpm install --frozen-lockfile` before removing in case the override targets a transitive dependency (CVE-fix pattern)"
                    .to_string(),
            ),
            available_in_catalogs: None,
            suggested_target: None,
        }));

        if let Some(suppress) = build_ignore_dependency_overrides_suppress(
            Some(&entry.target_package),
            &entry.raw_key,
            entry.source,
        ) {
            actions.push(suppress);
        }

        Self {
            entry,
            actions,
            introduced: None,
        }
    }
}

/// Wire-shape envelope for a [`MisconfiguredDependencyOverride`] finding.
/// Carries a `fix-dependency-override` primary plus the conditional
/// `add-to-config` `ignoreDependencyOverrides` suppress (skipped when both
/// `target_package` and `raw_key` are empty, since the rule matcher keys on
/// a non-empty package name).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MisconfiguredDependencyOverrideFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub entry: MisconfiguredDependencyOverride,
    /// Suggested next steps. Always emitted.
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
}

impl MisconfiguredDependencyOverrideFinding {
    /// Build the wrapper. The suppress action is omitted when neither
    /// `target_package` (set on `EmptyValue` cases) nor `raw_key` provides a
    /// non-empty package name; an `ignoreDependencyOverrides` entry with
    /// `package: ""` would be silently ignored by the config parser.
    #[must_use]
    pub fn with_actions(entry: MisconfiguredDependencyOverride) -> Self {
        let mut actions: Vec<IssueAction> = Vec::with_capacity(2);
        actions.push(IssueAction::Fix(FixAction {
            kind: FixActionType::FixDependencyOverride,
            auto_fixable: false,
            description:
                "Fix the override key or value: pnpm refuses to honor entries with an unparsable key or empty value"
                    .to_string(),
            note: Some(
                "Common shapes: bare `pkg`, scoped `@scope/pkg`, version-selector `pkg@<2`, parent-chain `parent>child`. Valid values include semver ranges, `-` (removal), `$ref` (self-ref), and `npm:alias@^1`."
                    .to_string(),
            ),
            available_in_catalogs: None,
            suggested_target: None,
        }));

        if let Some(suppress) = build_ignore_dependency_overrides_suppress(
            entry.target_package.as_deref(),
            &entry.raw_key,
            entry.source,
        ) {
            actions.push(suppress);
        }

        Self {
            entry,
            actions,
            introduced: None,
        }
    }
}

/// Shared `add-to-config` `ignoreDependencyOverrides` builder for the two
/// override findings. Returns `None` when no non-empty package name is
/// available; the config parser silently drops entries with an empty
/// `package` field, so emitting one would be a no-op that misleads agents.
fn build_ignore_dependency_overrides_suppress(
    target_package: Option<&str>,
    raw_key: &str,
    source: DependencyOverrideSource,
) -> Option<IssueAction> {
    let package = target_package
        .filter(|s| !s.is_empty())
        .or_else(|| Some(raw_key).filter(|s| !s.is_empty()))?
        .to_string();
    let mut value = serde_json::Map::new();
    value.insert("package".to_string(), serde_json::Value::String(package));
    value.insert(
        "source".to_string(),
        serde_json::Value::String(source.as_label().to_string()),
    );
    Some(IssueAction::AddToConfig(AddToConfigAction {
        kind: AddToConfigKind::AddToConfig,
        auto_fixable: false,
        description: "Suppress this override finding via ignoreDependencyOverrides in fallow config (use for CVE-fix overrides that target a purely-transitive package).".to_string(),
        config_key: "ignoreDependencyOverrides".to_string(),
        value: AddToConfigValue::RuleObject(value),
        value_schema: Some(IGNORE_DEPENDENCY_OVERRIDES_VALUE_SCHEMA.to_string()),
    }))
}

// ── Position-0 invariant golden tests ───────────────────────────
//
// These tests document the load-bearing position-0 semantics that flow
// downstream into the GitHub Action / GitLab CI jq scripts, the MCP server
// `actions[0].type` pattern-match, and the VS Code LSP code-action
// rendering. Snapshot tests assert structural equality; these named tests
// document WHY position 0 has a specific value, so a future refactor that
// re-orders actions tells you what broke instead of just "the snapshot
// changed".
#[cfg(test)]
mod position_0_invariants {
    use super::*;
    use crate::output::FixActionType;
    use crate::results::{DependencyOverrideSource, DuplicateLocation};
    use std::path::PathBuf;

    /// Helper: extract the kebab-case `type` discriminant from an
    /// [`IssueAction`] at a specific position. Returns `None` when the
    /// position is out of bounds or the action shape lacks a discriminant
    /// (today every variant has one).
    fn action_type(action: &IssueAction) -> &'static str {
        match action {
            IssueAction::Fix(fix) => match fix.kind {
                FixActionType::RemoveExport => "remove-export",
                FixActionType::DeleteFile => "delete-file",
                FixActionType::RemoveDependency => "remove-dependency",
                FixActionType::MoveDependency => "move-dependency",
                FixActionType::RemoveEnumMember => "remove-enum-member",
                FixActionType::RemoveClassMember => "remove-class-member",
                FixActionType::ResolveImport => "resolve-import",
                FixActionType::InstallDependency => "install-dependency",
                FixActionType::RemoveDuplicate => "remove-duplicate",
                FixActionType::MoveToDev => "move-to-dev",
                FixActionType::RefactorCycle => "refactor-cycle",
                FixActionType::RefactorReExportCycle => "refactor-re-export-cycle",
                FixActionType::RefactorBoundary => "refactor-boundary",
                FixActionType::ExportType => "export-type",
                FixActionType::RemoveCatalogEntry => "remove-catalog-entry",
                FixActionType::RemoveEmptyCatalogGroup => "remove-empty-catalog-group",
                FixActionType::UpdateCatalogReference => "update-catalog-reference",
                FixActionType::AddCatalogEntry => "add-catalog-entry",
                FixActionType::RemoveCatalogReference => "remove-catalog-reference",
                FixActionType::RemoveDependencyOverride => "remove-dependency-override",
                FixActionType::FixDependencyOverride => "fix-dependency-override",
                FixActionType::ResolvePolicyViolation => "resolve-policy-violation",
                FixActionType::MoveToServerModule => "move-to-server-module",
                FixActionType::SplitMixedBarrel => "split-mixed-barrel",
                FixActionType::HoistDirective => "hoist-directive",
                FixActionType::WireServerAction => "wire-server-action",
                FixActionType::ProvideInject => "provide-inject",
                FixActionType::UseLoadData => "use-load-data",
                FixActionType::RenderComponent => "render-component",
                FixActionType::UseComponentProp => "use-component-prop",
                FixActionType::EmitComponentEvent => "emit-component-event",
                FixActionType::WireSvelteEvent => "wire-svelte-event",
                FixActionType::ResolveRouteCollision => "resolve-route-collision",
                FixActionType::ResolveDynamicSegmentNameConflict => {
                    "resolve-dynamic-segment-name-conflict"
                }
            },
            IssueAction::SuppressLine(_) => "suppress-line",
            IssueAction::SuppressFile(_) => "suppress-file",
            IssueAction::AddToConfig(_) => "add-to-config",
        }
    }

    fn assert_manual_fix_then_suppress(
        actions: &[IssueAction],
        primary_type: &str,
        suppress_comment: &str,
    ) {
        assert_eq!(actions.len(), 2);
        assert_eq!(action_type(&actions[0]), primary_type);
        let IssueAction::Fix(primary) = &actions[0] else {
            panic!("position-0 should be a manual fix action");
        };
        assert!(!primary.auto_fixable);
        assert!(primary.note.is_some());
        assert_eq!(action_type(&actions[1]), "suppress-line");
        let IssueAction::SuppressLine(suppress) = &actions[1] else {
            panic!("position-1 should be a suppress-line action");
        };
        assert_eq!(suppress.comment, suppress_comment);
    }

    #[test]
    fn pnpm_catalog_entry_action_is_auto_fixable() {
        let finding = UnusedCatalogEntryFinding::with_actions(UnusedCatalogEntry {
            entry_name: "unused".to_string(),
            catalog_name: "default".to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line: 3,
            hardcoded_consumers: vec![],
        });

        let IssueAction::Fix(fix) = &finding.actions[0] else {
            panic!("position-0 should be a fix action");
        };
        assert!(fix.auto_fixable);
        assert_eq!(finding.actions.len(), 2);
        assert_eq!(action_type(&finding.actions[1]), "suppress-line");
    }

    #[test]
    fn bun_package_json_catalog_entry_action_is_manual_only() {
        let finding = UnusedCatalogEntryFinding::with_actions(UnusedCatalogEntry {
            entry_name: "unused".to_string(),
            catalog_name: "default".to_string(),
            path: PathBuf::from("package.json"),
            line: 4,
            hardcoded_consumers: vec![],
        });

        let IssueAction::Fix(fix) = &finding.actions[0] else {
            panic!("position-0 should be a fix action");
        };
        assert!(!fix.auto_fixable);
        assert!(fix.description.contains("manually"));
        assert_eq!(finding.actions.len(), 1);
    }

    #[test]
    fn bun_package_json_empty_catalog_group_action_is_manual_only() {
        let finding = EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
            catalog_name: "empty".to_string(),
            path: PathBuf::from("package.json"),
            line: 4,
        });

        let IssueAction::Fix(fix) = &finding.actions[0] else {
            panic!("position-0 should be a fix action");
        };
        assert!(!fix.auto_fixable);
        assert!(fix.description.contains("manually"));
        assert_eq!(finding.actions.len(), 1);
    }

    #[test]
    fn unprovided_inject_primary_action_is_provide_inject() {
        let finding = UnprovidedInjectFinding::with_actions(UnprovidedInject {
            path: PathBuf::from("src/context.ts"),
            key_name: "userKey".to_string(),
            framework: "svelte".to_string(),
            line: 7,
            col: 12,
        });

        assert_manual_fix_then_suppress(
            &finding.actions,
            "provide-inject",
            "// fallow-ignore-next-line unprovided-inject",
        );
    }

    #[test]
    fn unused_server_action_primary_action_is_wire_server_action() {
        let finding = UnusedServerActionFinding::with_actions(UnusedServerAction {
            path: PathBuf::from("app/actions.ts"),
            action_name: "saveDraft".to_string(),
            line: 3,
            col: 13,
        });

        assert_manual_fix_then_suppress(
            &finding.actions,
            "wire-server-action",
            "// fallow-ignore-next-line unused-server-action",
        );
    }

    #[test]
    fn unused_load_data_key_primary_action_is_use_load_data() {
        let finding = UnusedLoadDataKeyFinding::with_actions(UnusedLoadDataKey {
            path: PathBuf::from("src/routes/+page.server.ts"),
            key_name: "profile".to_string(),
            line: 12,
            col: 6,
            route_dir: Some("src/routes".to_string()),
        });

        assert_manual_fix_then_suppress(
            &finding.actions,
            "use-load-data",
            "// fallow-ignore-next-line unused-load-data-key",
        );
    }

    #[test]
    fn unrendered_component_primary_action_is_render_component() {
        let finding = UnrenderedComponentFinding::with_actions(UnrenderedComponent {
            path: PathBuf::from("src/components/EmptyState.vue"),
            component_name: "EmptyState".to_string(),
            framework: "vue".to_string(),
            reachable_via: None,
            line: 1,
            col: 0,
        });

        assert_manual_fix_then_suppress(
            &finding.actions,
            "render-component",
            "// fallow-ignore-next-line unrendered-component",
        );
    }

    #[test]
    fn unused_component_prop_primary_action_is_use_component_prop() {
        let finding = UnusedComponentPropFinding::with_actions(UnusedComponentProp {
            path: PathBuf::from("src/components/Card.vue"),
            component_name: "Card".to_string(),
            prop_name: "variant".to_string(),
            line: 5,
            col: 10,
        });

        assert_manual_fix_then_suppress(
            &finding.actions,
            "use-component-prop",
            "// fallow-ignore-next-line unused-component-prop",
        );
    }

    #[test]
    fn unused_component_emit_primary_action_is_emit_component_event() {
        let finding = UnusedComponentEmitFinding::with_actions(UnusedComponentEmit {
            path: PathBuf::from("src/components/Picker.vue"),
            component_name: "Picker".to_string(),
            emit_name: "focus".to_string(),
            line: 6,
            col: 14,
        });

        assert_manual_fix_then_suppress(
            &finding.actions,
            "emit-component-event",
            "// fallow-ignore-next-line unused-component-emit",
        );
    }

    #[test]
    fn unused_svelte_event_primary_action_is_wire_svelte_event() {
        let finding = UnusedSvelteEventFinding::with_actions(UnusedSvelteEvent {
            path: PathBuf::from("src/Dialog.svelte"),
            component_name: "Dialog".to_string(),
            event_name: "closed".to_string(),
            line: 19,
            col: 8,
        });

        assert_manual_fix_then_suppress(
            &finding.actions,
            "wire-svelte-event",
            "// fallow-ignore-next-line unused-svelte-event",
        );
    }

    #[test]
    fn unresolved_import_actions_include_ignore_unresolved_imports_config_suppress() {
        let inner = UnresolvedImport {
            specifier: "@example/icons".to_string(),
            path: PathBuf::from("src/index.ts"),
            line: 4,
            col: 12,
            specifier_col: 18,
        };
        let finding = UnresolvedImportFinding::with_actions(inner);

        assert_eq!(action_type(&finding.actions[0]), "resolve-import");
        assert_eq!(action_type(&finding.actions[1]), "add-to-config");
        let IssueAction::AddToConfig(action) = &finding.actions[1] else {
            panic!("position-1 should be AddToConfig");
        };
        assert!(!action.auto_fixable);
        assert_eq!(action.config_key, "ignoreUnresolvedImports");
        let AddToConfigValue::Scalar(value) = &action.value else {
            panic!("ignoreUnresolvedImports action should carry a scalar value");
        };
        assert_eq!(value, "@example/icons");
        assert_eq!(
            action.value_schema.as_deref(),
            Some(
                "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreUnresolvedImports/items"
            )
        );
    }

    /// Invariant: when no other catalog declares the package, position 0
    /// of `unresolved_catalog_references[].actions` is `add-catalog-entry`,
    /// directing the agent to grow the targeted catalog.
    ///
    /// Downstream consumers (MCP `actions[0].type` dispatch, jq scripts in
    /// `action/jq/review-comments-check.jq` and `ci/jq/review-check.jq`)
    /// pattern-match on this string. A future refactor that puts the
    /// generic `remove-catalog-reference` fallback at position 0 would
    /// flip every CI annotation from "add this entry" to "remove this
    /// reference", reversing the recommended action.
    #[test]
    fn unresolved_catalog_position_0_is_add_when_no_alternatives() {
        let inner = UnresolvedCatalogReference {
            entry_name: "react".to_string(),
            catalog_name: "default".to_string(),
            path: PathBuf::from("apps/web/package.json"),
            line: 7,
            available_in_catalogs: Vec::new(),
        };
        let finding = UnresolvedCatalogReferenceFinding::with_actions(inner);
        assert_eq!(
            action_type(&finding.actions[0]),
            "add-catalog-entry",
            "position-0 must be `add-catalog-entry` when no alternative catalog declares the package"
        );
        let IssueAction::Fix(fix) = &finding.actions[0] else {
            panic!("position-0 should be an IssueAction::Fix");
        };
        assert!(
            fix.available_in_catalogs.is_none(),
            "add-catalog-entry must NOT carry available_in_catalogs"
        );
        assert!(
            fix.suggested_target.is_none(),
            "add-catalog-entry must NOT carry suggested_target"
        );
    }

    /// Invariant: when at least one alternative catalog declares the
    /// package, position 0 flips to `update-catalog-reference` and carries
    /// the alternative list. When exactly one alternative exists, the
    /// action also carries `suggested_target` so deterministic agents can
    /// land the edit without picking from the list. This is the
    /// counterpart to `unresolved_catalog_position_0_is_add_when_no_alternatives`.
    #[test]
    fn unresolved_catalog_position_0_is_update_when_alternatives_exist() {
        let inner = UnresolvedCatalogReference {
            entry_name: "react".to_string(),
            catalog_name: "default".to_string(),
            path: PathBuf::from("apps/web/package.json"),
            line: 7,
            available_in_catalogs: vec!["react18".to_string()],
        };
        let finding = UnresolvedCatalogReferenceFinding::with_actions(inner);
        assert_eq!(
            action_type(&finding.actions[0]),
            "update-catalog-reference",
            "position-0 must be `update-catalog-reference` when at least one alternative catalog declares the package"
        );
        let IssueAction::Fix(fix) = &finding.actions[0] else {
            panic!("position-0 should be an IssueAction::Fix");
        };
        assert_eq!(
            fix.available_in_catalogs.as_deref(),
            Some(&["react18".to_string()][..]),
            "update-catalog-reference must carry the alternative list"
        );
        assert_eq!(
            fix.suggested_target.as_deref(),
            Some("react18"),
            "single-alternative case must surface `suggested_target` for deterministic agents"
        );

        // Two alternatives: still update, but no unambiguous target.
        let inner_two = UnresolvedCatalogReference {
            entry_name: "react".to_string(),
            catalog_name: "default".to_string(),
            path: PathBuf::from("apps/web/package.json"),
            line: 7,
            available_in_catalogs: vec!["react17".to_string(), "react18".to_string()],
        };
        let finding_two = UnresolvedCatalogReferenceFinding::with_actions(inner_two);
        assert_eq!(
            action_type(&finding_two.actions[0]),
            "update-catalog-reference"
        );
        let IssueAction::Fix(fix_two) = &finding_two.actions[0] else {
            panic!("position-0 should be an IssueAction::Fix");
        };
        assert!(
            fix_two.suggested_target.is_none(),
            "multi-alternative case must NOT carry `suggested_target` (agent must pick)"
        );
    }

    /// Invariant: position 0 of `duplicate_exports[].actions` is
    /// `add-to-config` (the safe `ignoreExports` rule for the
    /// namespace-barrel case), NOT the destructive `remove-duplicate`.
    ///
    /// This protects the shadcn / Radix / bits-ui pattern where every
    /// `components/ui/<name>/index.ts` intentionally re-exports the same
    /// short names. Any consumer that reads `actions[0].type` as "the
    /// recommended fix" must see the non-destructive path first; flipping
    /// position 0 to `remove-duplicate` would propose deleting an
    /// intentional API surface.
    ///
    /// This test pins position 0 across both possible auto_fixable values
    /// for the add-to-config action (the per-instance flip flag handled
    /// by `set_config_fixable`).
    #[test]
    fn duplicate_exports_position_0_is_add_to_config_not_remove_duplicate() {
        let inner = DuplicateExport {
            export_name: "Root".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("components/ui/accordion/index.ts"),
                    line: 1,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("components/ui/dialog/index.ts"),
                    line: 1,
                    col: 0,
                },
            ],
        };
        let finding = DuplicateExportFinding::with_actions(inner);
        assert_eq!(
            action_type(&finding.actions[0]),
            "add-to-config",
            "position-0 must be `add-to-config` (safe `ignoreExports` path), NOT `remove-duplicate`"
        );
        assert_eq!(
            action_type(&finding.actions[1]),
            "remove-duplicate",
            "position-1 must be the destructive `remove-duplicate` fallback"
        );

        // `set_config_fixable(true)` flips the position-0 add-to-config
        // bool but must NOT re-order positions.
        let mut promoted = finding;
        promoted.set_config_fixable(true);
        assert_eq!(action_type(&promoted.actions[0]), "add-to-config");
        let IssueAction::AddToConfig(action) = &promoted.actions[0] else {
            panic!("position-0 should still be AddToConfig after set_config_fixable");
        };
        assert!(
            action.auto_fixable,
            "set_config_fixable(true) must flip auto_fixable"
        );
    }

    /// Invariant: a duplicate-exports finding with empty `locations`
    /// degenerate input drops the `add-to-config` action entirely, so
    /// position 0 falls through to `remove-duplicate`. Documents the
    /// degenerate-case contract.
    #[test]
    fn duplicate_exports_no_locations_falls_through_to_remove_duplicate() {
        let inner = DuplicateExport {
            export_name: "Root".to_string(),
            locations: Vec::new(),
        };
        let finding = DuplicateExportFinding::with_actions(inner);
        assert_eq!(
            action_type(&finding.actions[0]),
            "remove-duplicate",
            "with no locations there is no ignoreExports rule to suggest; the destructive remove becomes position-0"
        );

        // `set_config_fixable(true)` is a no-op on this shape.
        let mut promoted = finding;
        promoted.set_config_fixable(true);
        assert_eq!(
            action_type(&promoted.actions[0]),
            "remove-duplicate",
            "set_config_fixable is a no-op when position-0 is not add-to-config"
        );
    }

    /// Invariant: misconfigured-dependency-override with empty
    /// `target_package` AND empty `raw_key` drops the suppress action
    /// (no usable package name for the `ignoreDependencyOverrides`
    /// matcher; emitting `package: ""` would be silently dropped by the
    /// config parser). Documents the suppress-omission contract.
    #[test]
    fn misconfigured_override_drops_suppress_when_no_package_name() {
        let inner = MisconfiguredDependencyOverride {
            raw_key: String::new(),
            target_package: None,
            raw_value: String::new(),
            reason: crate::results::DependencyOverrideMisconfigReason::EmptyValue,
            source: DependencyOverrideSource::PnpmWorkspaceYaml,
            path: PathBuf::from("pnpm-workspace.yaml"),
            line: 12,
        };
        let finding = MisconfiguredDependencyOverrideFinding::with_actions(inner);
        // Only the primary fix-dependency-override action: no suppress.
        assert_eq!(finding.actions.len(), 1);
        assert_eq!(action_type(&finding.actions[0]), "fix-dependency-override");
    }
}
