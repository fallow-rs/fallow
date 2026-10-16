use super::common::{create_config, fixture_path};

/// The `playwright` package ships the test runner at `playwright/test`. A
/// project that depends only on `playwright` and has a Playwright config must
/// activate the plugin, so its spec files are entry points.
#[test]
fn bare_playwright_package_with_config_keeps_spec_files() {
    let root = fixture_path("playwright-bare-package");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert!(
        unused_files.is_empty(),
        "spec files of the Playwright runner must not be reported, got {unused_files:?}"
    );
}
