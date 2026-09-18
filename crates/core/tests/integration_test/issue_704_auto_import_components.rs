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
//! Issue #2695 extends this to configs that switch auto-import off. A config
//! that statically proves nothing is scanned (`components: { dirs: [] }`,
//! `imports: { scan: false }`) counts as the default, so its convention files
//! lose their entry patterns too, while a config with unmodeled custom
//! directories keeps them.

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
