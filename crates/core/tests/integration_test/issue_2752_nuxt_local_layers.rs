//! Issue #2752: Nuxt local layers and nested config keys.
//!
//! - A nested key that ends in `components` or `imports` (a `routeRules` path)
//!   does not keep a surface's convention patterns when the config object is
//!   readable, so unreferenced convention files report with `autoImports` on.
//! - A local directory in `extends`, and each `layers/*` directory, that holds a
//!   Nuxt config is a layer root: its convention files are entry points with
//!   `autoImports` off, and its components and composables are auto-import
//!   sources with `autoImports` on. A directory without a config is no layer.
//! - `#layers/<name>/` resolves to a local layer named by its `$meta.name`. A
//!   `layers/*` directory without one is named by its directory name.

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
fn nested_surface_keys_do_not_keep_convention_patterns() {
    let root = fixture_path("nuxt-auto-imports-nested-keys");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for dead in ["components/Foo.vue", "composables/useThing.ts"] {
        assert!(
            unused.contains(&dead.to_string()),
            "{dead} must report as unused, got: {unused:?}"
        );
    }
}

/// Directories without a Nuxt config: Nuxt skips them as layers, so their
/// unreferenced files keep reporting.
const NOT_LAYER_FILES: &[&str] = &[
    "bare/components/Bare.vue",
    "layers/legacy/components/Old.vue",
    "layers/legacy/utils/oldHelper.ts",
];

fn assert_not_layer_files_report(unused: &[String]) {
    for dead in NOT_LAYER_FILES {
        assert!(
            unused.contains(&(*dead).to_string()),
            "{dead} is not in a layer and must report, got: {unused:?}"
        );
    }
}

const LAYER_FILES_IN_USE: &[&str] = &[
    "base/components/Banner.vue",
    "base/composables/useMark.ts",
    "base/pages/about.vue",
    "layers/extra/components/ExtraPanel.vue",
    "layers/extra/utils/formatLabel.ts",
];

#[test]
fn local_layer_conventions_are_entry_points_with_flag_off() {
    let root = fixture_path("nuxt-local-layers");
    let config = create_config(root.clone());
    assert!(!config.auto_imports);

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for alive in LAYER_FILES_IN_USE
        .iter()
        .chain(&["base/components/DeadBanner.vue"])
    {
        assert!(
            !unused.contains(&(*alive).to_string()),
            "{alive} is a layer convention file and must not report, got: {unused:?}"
        );
    }
    assert_not_layer_files_report(&unused);
}

#[test]
fn local_layer_sources_are_auto_imported_with_flag_on() {
    let root = fixture_path("nuxt-local-layers");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for alive in LAYER_FILES_IN_USE {
        assert!(
            !unused.contains(&(*alive).to_string()),
            "{alive} is used through the layer and must not report, got: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"base/components/DeadBanner.vue".to_string()),
        "an unreferenced layer component must report with the flag on, got: {unused:?}"
    );
    assert_not_layer_files_report(&unused);
}

#[test]
fn layer_alias_follows_meta_name_and_auto_registered_directory_name() {
    let root = fixture_path("nuxt-layer-aliases");
    let config = create_config(root.clone());

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "every #layers/ import must resolve, got: {unresolved:?}"
    );
    for alive in ["tiers/marketing/lib/mark.ts", "layers/docs/lib/guide.ts"] {
        assert!(
            !unused.contains(&alive.to_string()),
            "{alive} is imported through #layers/ and must not report, got: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"tiers/plain/lib/tool.ts".to_string()),
        "an extends layer without `$meta.name` gets no #layers/ alias, got: {unused:?}"
    );
}
