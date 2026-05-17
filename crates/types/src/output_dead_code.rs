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
//! `SuppressFile`) are small, but reusing the existing enum keeps the
//! wire-shape contract identical to the legacy post-pass.
//!
//! `introduced` is typed as `Option<AuditIntroduced>` (transparent newtype
//! over `bool`) so the regenerated schema renders the field via
//! `$ref: #/definitions/AuditIntroduced`, matching the reference the prior
//! post-pass augmentation graft used. The audit pass continues to inject a
//! bare bool via `map.insert("introduced", ...)`; serde reads it back into
//! `AuditIntroduced` transparently. The field stays absent at the wire when
//! `None` (`skip_serializing_if`).

use serde::Serialize;

use crate::envelope::AuditIntroduced;
use crate::output::{
    FixAction, FixActionType, IssueAction, SuppressFileAction, SuppressFileKind,
    SuppressLineAction, SuppressLineKind,
};
use crate::results::{
    BoundaryViolation, CircularDependency, PrivateTypeLeak, UnresolvedImport, UnusedExport,
    UnusedFile, UnusedMember,
};

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
/// `actions` array (`resolve-import` primary plus `suppress-line`
/// secondary).
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
