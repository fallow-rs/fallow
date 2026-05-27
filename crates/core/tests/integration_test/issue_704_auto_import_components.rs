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

    // Additive guarantee: components/** stays an entry pattern, so even the
    // unreferenced component is not reported as unused.
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

    // The genuinely-unreferenced component now surfaces (the recall win).
    assert!(
        unused.contains(&"components/DeadCard.vue".to_string()),
        "flag on must report the unreferenced component as unused, got: {unused:?}"
    );

    // Referenced components stay reachable through synthesized auto-import edges:
    //   <Card001 />   -> components/Card001.vue           (flat name)
    //   <BaseButton/> -> components/base/Button.vue        (directory-prefix name)
    //   <LazyWidget/> -> components/Widget.vue             (Lazy variant)
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
