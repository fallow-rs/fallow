//! Issue filters of the `check` command, built from its parsed flags.

use super::*;

/// First half of the `IssueFilters` mapping: core/general filter flags over a
/// `Default` base. The framework/catalog half layers on top via struct update.
pub(super) fn check_issue_filters_core(command: &Command) -> IssueFilters {
    let Command::Check {
        unused_files,
        unused_exports,
        unused_deps,
        unused_types,
        private_type_leaks,
        deprecated_exports_in_use,
        unused_enum_members,
        unused_class_members,
        unresolved_imports,
        unlisted_deps,
        duplicate_exports,
        duplicate_export_aliases,
        circular_deps,
        re_export_cycles,
        package_cycles,
        boundary_violations,
        policy_violations,
        stale_suppressions,
        ..
    } = command
    else {
        unreachable!("check filter builder only handles check commands");
    };

    let mut filters = IssueFilters::default();
    for (flag, active) in [
        ("--unused-files", *unused_files),
        ("--unused-exports", *unused_exports),
        ("--unused-deps", *unused_deps),
        ("--unused-types", *unused_types),
        ("--private-type-leaks", *private_type_leaks),
        ("--deprecated-exports-in-use", *deprecated_exports_in_use),
        ("--unused-enum-members", *unused_enum_members),
        ("--unused-class-members", *unused_class_members),
        ("--unresolved-imports", *unresolved_imports),
        ("--unlisted-deps", *unlisted_deps),
        ("--duplicate-exports", *duplicate_exports),
        ("--duplicate-export-aliases", *duplicate_export_aliases),
        ("--circular-deps", *circular_deps),
        ("--re-export-cycles", *re_export_cycles),
        ("--package-cycles", *package_cycles),
        ("--boundary-violations", *boundary_violations),
        ("--policy-violations", *policy_violations),
        ("--stale-suppressions", *stale_suppressions),
    ] {
        enable_check_filter(&mut filters, flag, active);
    }
    filters
}

/// Second half of the `IssueFilters` mapping: framework/component, store, svelte,
/// catalog, and dependency-override flags, layered onto the core `base`.
pub(super) fn check_issue_filters_framework(
    command: &Command,
    base: &IssueFilters,
) -> IssueFilters {
    let Command::Check {
        unused_namespace_members,
        unused_store_members,
        unprovided_injects,
        unrendered_components,
        unused_component_props,
        absent_component_props,
        constant_parameters,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_svelte_events,
        unused_effect_layers,
        unused_tagged_errors,
        unproduced_catch_tags,
        unused_server_actions,
        unused_load_data_keys,
        unlisted_binaries,
        unused_catalog_entries,
        empty_catalog_groups,
        unresolved_catalog_references,
        unused_dependency_overrides,
        misconfigured_dependency_overrides,
        ..
    } = command
    else {
        unreachable!("check filter builder only handles check commands");
    };

    let mut filters = base.clone();
    for (flag, active) in [
        ("--unused-namespace-members", *unused_namespace_members),
        ("--unused-store-members", *unused_store_members),
        ("--unprovided-injects", *unprovided_injects),
        ("--unrendered-components", *unrendered_components),
        ("--unused-component-props", *unused_component_props),
        ("--absent-component-props", *absent_component_props),
        ("--constant-parameters", *constant_parameters),
        ("--unused-component-emits", *unused_component_emits),
        ("--unused-component-inputs", *unused_component_inputs),
        ("--unused-component-outputs", *unused_component_outputs),
        ("--unused-svelte-events", *unused_svelte_events),
        ("--unused-effect-layers", *unused_effect_layers),
        ("--unused-tagged-errors", *unused_tagged_errors),
        ("--unproduced-catch-tags", *unproduced_catch_tags),
        ("--unused-server-actions", *unused_server_actions),
        ("--unused-load-data-keys", *unused_load_data_keys),
        ("--unlisted-binaries", *unlisted_binaries),
        ("--unused-catalog-entries", *unused_catalog_entries),
        ("--empty-catalog-groups", *empty_catalog_groups),
        (
            "--unresolved-catalog-references",
            *unresolved_catalog_references,
        ),
        (
            "--unused-dependency-overrides",
            *unused_dependency_overrides,
        ),
        (
            "--misconfigured-dependency-overrides",
            *misconfigured_dependency_overrides,
        ),
    ] {
        enable_check_filter(&mut filters, flag, active);
    }
    filters
}
