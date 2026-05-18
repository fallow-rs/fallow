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
    AddToConfigAction, AddToConfigKind, AddToConfigValue, FixAction, FixActionType, IssueAction,
    SuppressFileAction, SuppressFileKind, SuppressLineAction, SuppressLineKind,
};
use crate::results::{
    BoundaryViolation, CircularDependency, PrivateTypeLeak, TestOnlyDependency, TypeOnlyDependency,
    UnlistedDependency, UnresolvedImport, UnusedDependency, UnusedExport, UnusedFile, UnusedMember,
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
        })
    } else {
        IssueAction::Fix(FixAction {
            kind: FixActionType::RemoveDependency,
            auto_fixable: true,
            description: format!("Remove from {package_json_location} in package.json"),
            note: None,
            available_in_catalogs: None,
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
