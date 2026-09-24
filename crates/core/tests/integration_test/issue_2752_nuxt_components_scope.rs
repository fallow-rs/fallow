//! Issue #2752: `#components` namespace imports and auto-import scope.
//!
//! - A member access on a namespace import from `#components` or `#imports`
//!   credits the convention file of that name. A reached module that holds
//!   `export * from '#components'` credits the names its importers take from
//!   it. An access that fallow cannot read credits every name of that module
//!   and records a `plugin-effect-not-modeled` diagnostic on the file.
//! - An auto-import rule is visible only to the root that declared it and to
//!   the apps that extend that root as a Nuxt layer. A component name in one
//!   workspace does not credit the file of the same name in a sibling
//!   workspace. The link works in both directions, for a layer named by a
//!   relative path and for one named by its package name, and for the rules
//!   of every plugin, such as a Pinia store in a layer.

use std::path::Path;

use super::common::{create_config, fixture_path};
use fallow_config::{WorkspaceDiagnosticKind, workspace_diagnostics_for};
use fallow_types::results::AnalysisResults;

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_file_paths(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| relative(&finding.file.path, root))
        .collect()
}

/// The `plugin-effect-not-modeled` diagnostics of the last run, as
/// `(path, key)` pairs.
fn not_modeled_diagnostics(root: &Path) -> Vec<(String, String)> {
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    workspace_diagnostics_for(root)
        .into_iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            WorkspaceDiagnosticKind::PluginEffectNotModeled { key, .. } => {
                let path = if diagnostic.path.is_absolute() {
                    let path = dunce::canonicalize(&diagnostic.path).unwrap_or(diagnostic.path);
                    relative(&path, &canonical_root)
                } else {
                    relative(&diagnostic.path, root)
                };
                Some((path, key))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn namespace_member_access_and_star_re_export_credit_components() {
    let root = fixture_path("nuxt-virtual-module-namespace");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for reachable in [
        "app/components/Card.vue",
        "app/components/Named.vue",
        "app/components/ForwardedCard.vue",
        "app/components/ForwardedPanel.vue",
        "app/composables/useForwarded.ts",
    ] {
        assert!(
            !unused.contains(&reachable.to_string()),
            "{reachable} is read through a namespace or a star re-export of a \
             Nuxt virtual module and must be credited, got: {unused:?}"
        );
    }
    for dead in ["app/components/DeadCard.vue", "app/composables/useDead.ts"] {
        assert!(
            unused.contains(&dead.to_string()),
            "{dead} is referenced nowhere and must still report, got: {unused:?}"
        );
    }
    assert!(
        not_modeled_diagnostics(&root).is_empty(),
        "every access is readable, so no diagnostic is recorded"
    );
}

#[test]
fn unreadable_namespace_use_credits_every_component_and_records_a_diagnostic() {
    let root = fixture_path("nuxt-virtual-module-namespace-unreadable");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for kept in ["app/components/Alpha.vue", "app/components/Beta.vue"] {
        assert!(
            !unused.contains(&kept.to_string()),
            "a spread of the #components namespace can use {kept}, got: {unused:?}"
        );
    }
    assert_eq!(
        not_modeled_diagnostics(&root),
        vec![("app/lib/registry.ts".to_string(), "#components".to_string())],
    );
}

#[test]
fn a_component_name_does_not_credit_a_sibling_workspace() {
    let root = fixture_path("nuxt-auto-imports-workspace-scope");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        unused.contains(&"packages/b/components/Card.vue".to_string()),
        "only packages/a renders <Card />, so the Card of packages/b must report, \
         got: {unused:?}"
    );
    for reachable in [
        "packages/a/components/Card.vue",
        "packages/shared/components/Badge.vue",
        "packages/ui/components/UiButton.vue",
        "packages/a/components/AppLogo.vue",
        "packages/b/components/AppFooter.vue",
        "packages/shared/stores/cart.ts",
    ] {
        assert!(
            !unused.contains(&reachable.to_string()),
            "{reachable} is used by its own app, by an app that extends its layer, \
             or by a layer that its app extends, got: {unused:?}"
        );
    }
}

#[test]
fn sibling_apps_of_one_layer_do_not_credit_each_other() {
    let root = fixture_path("nuxt-auto-imports-shared-layer-siblings");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut unused = unused_file_paths(&results, &root);
    unused.sort();

    assert_eq!(
        unused,
        vec![
            "packages/one/components/Card.vue".to_string(),
            "packages/three/components/Panel.vue".to_string(),
        ],
        "an app that extends a layer must not see the components of another \
         app that extends the same layer, by package name or by relative path"
    );
}

#[test]
fn flag_off_keeps_every_workspace_component_alive() {
    for fixture in [
        "nuxt-auto-imports-workspace-scope",
        "nuxt-auto-imports-shared-layer-siblings",
        "nuxt-virtual-module-namespace",
    ] {
        let root = fixture_path(fixture);
        let config = create_config(root.clone());
        assert!(!config.auto_imports, "default is additive (flag off)");

        let results = fallow_core::analyze(&config).expect("analysis should succeed");
        let unused = unused_file_paths(&results, &root);
        assert!(
            unused.is_empty(),
            "flag off must not report a convention file in {fixture}, got: {unused:?}"
        );
    }
}
