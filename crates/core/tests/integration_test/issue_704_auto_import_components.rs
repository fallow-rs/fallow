//! Issue #704: convention auto-import resolution for Nuxt components.
//!
//! Verifies the two behaviors of the `autoImports` flag against a Nuxt fixture
//! whose page references components only via template tags (no `import`):
//! - flag OFF (default): components stay alive via entry patterns, so no new
//!   `unused-file` false positives (additive guarantee);
//! - flag ON: the component entry patterns are dropped, so an unreferenced
//!   component reports as `unused-file` while referenced ones (resolved through
//!   synthesized auto-import edges, including the `Lazy` and directory-prefix
//!   name forms) stay reachable.
//!
//! Issue #2695 extends this to configs that switch the auto-import scan off. A
//! config that statically proves nothing is scanned (`components: { dirs: [] }`,
//! `imports: { scan: false }`) counts as the default, so its convention files
//! lose their entry patterns too, while a config with unmodeled custom
//! directories keeps them, as does an `imports: { autoImport: false }` that only
//! switches the injection off.
//!
//! Issue #2737 covers the precision of that model: a component under
//! `components/global` or `components/islands` is named after its own directory,
//! a config key the regexes cannot see (a computed key) keeps both surfaces'
//! patterns, each workspace root is classified on its own, and `components: true`,
//! `imports: {}` and `imports: { dirs: [] }` are the Nuxt defaults. A name a file
//! imports by hand from `#components` or `#imports` earns the same credit as the
//! template tag or the bare call.

use std::path::Path;

use super::common::{create_config, fixture_path};
use fallow_types::results::AnalysisResults;

fn unused_file_paths(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn flag_off_keeps_all_components_alive() {
    let root = fixture_path("nuxt-auto-import-components");
    let config = create_config(root.clone());
    assert!(!config.auto_imports, "default is additive (flag off)");

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        !unused.contains(&"components/DeadCard.vue".to_string()),
        "flag off must not report any component as unused, got: {unused:?}"
    );
}

#[test]
fn flag_on_reports_unreferenced_component_and_keeps_referenced_ones() {
    let root = fixture_path("nuxt-auto-import-components");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        unused.contains(&"components/DeadCard.vue".to_string()),
        "flag on must report the unreferenced component as unused, got: {unused:?}"
    );

    for reachable in [
        "components/Card001.vue",
        "components/base/Button.vue",
        "components/Widget.vue",
    ] {
        assert!(
            !unused.contains(&reachable.to_string()),
            "{reachable} should be reachable via auto-import edge, got: {unused:?}"
        );
    }
}

#[test]
fn disabled_auto_import_config_reports_unreferenced_convention_files() {
    let root = fixture_path("nuxt-auto-imports-disabled");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for dead in ["app/components/DeadCard.vue", "app/composables/useDead.ts"] {
        assert!(
            unused.contains(&dead.to_string()),
            "{dead} should be reported when the config switches auto-import off, got: {unused:?}"
        );
    }

    for reachable in ["app/components/UsedCard.vue", "app/composables/useUsed.ts"] {
        assert!(
            !unused.contains(&reachable.to_string()),
            "{reachable} is imported explicitly and must stay reachable, got: {unused:?}"
        );
    }
}

#[test]
fn flag_off_keeps_disabled_auto_import_config_files_alive() {
    let root = fixture_path("nuxt-auto-imports-disabled");
    let config = create_config(root.clone());
    assert!(!config.auto_imports, "default is additive (flag off)");

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for dead in ["app/components/DeadCard.vue", "app/composables/useDead.ts"] {
        assert!(
            !unused.contains(&dead.to_string()),
            "flag off must not report {dead}, got: {unused:?}"
        );
    }
}

#[test]
fn injection_only_opt_out_keeps_composable_entry_patterns() {
    let root = fixture_path("nuxt-auto-imports-explicit-imports");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        !unused.contains(&"app/composables/useExplicit.ts".to_string()),
        "an imports.autoImport opt-out only switches the injection off, so a composable \
         consumed through #imports must keep its entry pattern, got: {unused:?}"
    );
}

#[test]
fn custom_component_dirs_keep_component_entry_patterns() {
    let root = fixture_path("nuxt-auto-imports-custom-components");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for kept in ["app/components/KeptByGuard.vue", "app/ui/Card.vue"] {
        assert!(
            !unused.contains(&kept.to_string()),
            "{kept} must keep its entry pattern while components: dirs are custom, got: {unused:?}"
        );
    }
}

#[test]
fn global_and_island_components_are_named_after_their_own_directory() {
    let root = fixture_path("nuxt-global-components");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for dead in ["components/global/Bar.vue", "components/islands/Dead.vue"] {
        assert!(
            unused.contains(&dead.to_string()),
            "{dead} is referenced nowhere and should report, got: {unused:?}"
        );
    }

    for reachable in ["components/global/Foo.vue", "components/islands/Isle.vue"] {
        assert!(
            !unused.contains(&reachable.to_string()),
            "{reachable} is rendered under its own directory name and must stay \
             reachable, got: {unused:?}"
        );
    }
}

#[test]
fn flag_off_keeps_global_and_island_components_alive() {
    let root = fixture_path("nuxt-global-components");
    let config = create_config(root.clone());
    assert!(!config.auto_imports, "default is additive (flag off)");

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for kept in ["components/global/Bar.vue", "components/islands/Dead.vue"] {
        assert!(
            !unused.contains(&kept.to_string()),
            "flag off must not report {kept}, got: {unused:?}"
        );
    }
}

#[test]
fn computed_config_key_keeps_composable_entry_patterns() {
    let root = fixture_path("nuxt-auto-imports-computed-key");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        !unused.contains(&"app/composables/useExplicit.ts".to_string()),
        "a computed `imports` key configures the surface, so the composable keeps \
         its entry pattern, got: {unused:?}"
    );
    assert!(
        unused.contains(&"app/reporting-control.ts".to_string()),
        "the unreferenced file outside a convention directory still reports, \
         got: {unused:?}"
    );
}

#[test]
fn each_workspace_root_is_classified_on_its_own() {
    let root = fixture_path("nuxt-auto-imports-monorepo");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        unused.contains(&"packages/a/components/DeadA.vue".to_string()),
        "the default workspace reports its unreferenced component even though a \
         sibling is custom, got: {unused:?}"
    );
    assert!(
        !unused.contains(&"packages/b/components/KeptB.vue".to_string()),
        "the custom workspace keeps its component entry patterns, got: {unused:?}"
    );
}

#[test]
fn components_true_counts_as_the_nuxt_default() {
    let root = fixture_path("nuxt-auto-imports-components-true");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        unused.contains(&"app/components/DeadCard.vue".to_string()),
        "`components: true` resolves to the default directories, so an \
         unreferenced component reports, got: {unused:?}"
    );
    assert!(
        !unused.contains(&"app/components/UsedCard.vue".to_string()),
        "the rendered component must stay reachable, got: {unused:?}"
    );
}

#[test]
fn empty_imports_object_counts_as_the_nuxt_default() {
    let root = fixture_path("nuxt-auto-imports-default-shapes");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        unused.contains(&"app/composables/useDead.ts".to_string()),
        "`imports: {{}}` is the default shape, so an unreferenced composable \
         reports, got: {unused:?}"
    );
    assert!(
        !unused.contains(&"app/composables/useUsed.ts".to_string()),
        "the called composable must stay reachable, got: {unused:?}"
    );
}

#[test]
fn empty_import_dirs_count_as_the_nuxt_default() {
    let root = fixture_path("nuxt-auto-imports-empty-import-dirs");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        unused.contains(&"app/composables/useDead.ts".to_string()),
        "`imports: {{ dirs: [] }}` is the default shape, so an unreferenced \
         composable reports, got: {unused:?}"
    );
    assert!(
        !unused.contains(&"app/composables/useUsed.ts".to_string()),
        "the called composable must stay reachable, got: {unused:?}"
    );
}

#[test]
fn named_imports_from_nuxt_virtual_modules_credit_their_convention_files() {
    let root = fixture_path("nuxt-virtual-module-imports");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for reachable in [
        "app/components/UsedCard.vue",
        "app/components/DialogCard.vue",
        "app/components/PanelCard.vue",
        "app/composables/useUsed.ts",
    ] {
        assert!(
            !unused.contains(&reachable.to_string()),
            "{reachable} is named in an import or re-export from #components or \
             #imports and must be credited, got: {unused:?}"
        );
    }

    for dead in ["app/components/DeadCard.vue", "app/composables/useDead.ts"] {
        assert!(
            unused.contains(&dead.to_string()),
            "{dead} is referenced nowhere and must still report, got: {unused:?}"
        );
    }
}

#[test]
fn flag_off_keeps_virtual_module_import_siblings_alive() {
    let root = fixture_path("nuxt-virtual-module-imports");
    let config = create_config(root.clone());
    assert!(!config.auto_imports, "default is additive (flag off)");

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for kept in [
        "app/components/UsedCard.vue",
        "app/components/DialogCard.vue",
        "app/components/PanelCard.vue",
        "app/components/DeadCard.vue",
        "app/composables/useUsed.ts",
        "app/composables/useDead.ts",
    ] {
        assert!(
            !unused.contains(&kept.to_string()),
            "flag off must not report {kept}, got: {unused:?}"
        );
    }
}
