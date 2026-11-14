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
            FixActionType::MoveToProd => "move-to-prod",
            FixActionType::RefactorCycle => "refactor-cycle",
            FixActionType::RefactorReExportCycle => "refactor-re-export-cycle",
            FixActionType::RefactorBoundary => "refactor-boundary",
            FixActionType::ExportType => "export-type",
            FixActionType::MigrateDeprecatedExport => "migrate-deprecated-export",
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
            FixActionType::ReviewComponentProp => "review-component-prop",
            FixActionType::ReviewConstantParameter => "review-constant-parameter",
            FixActionType::EmitComponentEvent => "emit-component-event",
            FixActionType::WireSvelteEvent => "wire-svelte-event",
            FixActionType::ComposeEffectLayer => "compose-effect-layer",
            FixActionType::FailTaggedError => "fail-tagged-error",
            FixActionType::RemoveCatchTagHandler => "remove-catch-tag-handler",
            FixActionType::ResolveRouteCollision => "resolve-route-collision",
            FixActionType::ResolveDynamicSegmentNameConflict => {
                "resolve-dynamic-segment-name-conflict"
            }
            FixActionType::AddSuppressionReason => "add-suppression-reason",
            FixActionType::RemoveStaleSuppression => "remove-stale-suppression",
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
/// Downstream consumers (MCP `actions[0].type` dispatch and JSON
/// consumers that read the first action) pattern-match on this string. A future refactor that puts the
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
