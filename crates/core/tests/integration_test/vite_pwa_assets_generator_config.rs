use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::collect_unused_files;

#[test]
fn pwa_assets_config_is_used_when_the_generator_is_a_dependency() {
    let root = fixture_path("vite-pwa-assets-generator-config");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    assert!(
        !unused_files
            .iter()
            .any(|unused| unused == "pwa-assets.config.ts"),
        "the generator reads pwa-assets.config.ts, unused files: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|unused| unused == "src/pwa-assets.config.ts"),
        "the generator reads only the root config, unused files: {unused_files:?}"
    );
    assert!(
        unused_files.iter().any(|unused| unused == "src/orphan.ts"),
        "the unreferenced control file must stay unused, unused files: {unused_files:?}"
    );
}
