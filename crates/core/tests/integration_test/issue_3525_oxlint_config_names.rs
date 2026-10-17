use super::common::{create_config, fixture_path};

/// Oxlint discovers `.oxlintrc.jsonc` and `oxlint.config.mts` on its own. The
/// plugin must read both files and credit `jsPlugins` at the top level and in
/// `overrides`.
fn assert_oxlint_config_credits_js_plugins(fixture: &str, config_file: &str) {
    let root = fixture_path(fixture);
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    for plugin in ["eslint-plugin-top-level", "eslint-plugin-override-only"] {
        assert!(
            !unused_dev_dependencies.contains(&plugin),
            "{plugin} should be credited through {config_file} jsPlugins, got {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"eslint-plugin-unused-control"),
        "unreferenced control dependency should still be reported, got {unused_dev_dependencies:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .filter_map(|file| {
            file.file
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    for file in [config_file, "local-plugin.js"] {
        assert!(
            !unused_files.contains(&file.to_string()),
            "{file} should be used through the Oxlint config, got {unused_files:?}"
        );
    }
}

#[test]
fn oxlintrc_jsonc_credits_top_level_and_override_js_plugins() {
    assert_oxlint_config_credits_js_plugins("issue-3525-oxlint-jsonc", ".oxlintrc.jsonc");
}

#[test]
fn oxlint_config_mts_credits_top_level_and_override_js_plugins() {
    assert_oxlint_config_credits_js_plugins("issue-3525-oxlint-mts", "oxlint.config.mts");
}
