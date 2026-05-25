use super::common::{create_config, fixture_path};

#[test]
fn oxlint_js_plugins_credit_dev_dependencies() {
    let root = fixture_path("issue-607-oxlint-js-plugins");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    for plugin in [
        "eslint-plugin-testing-library",
        "eslint-plugin-playwright",
        "eslint-plugin-sonarjs",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&plugin),
            "{plugin} should be credited through oxlint jsPlugins, got {unused_dev_dependencies:?}"
        );
    }

    assert!(
        unused_dev_dependencies.contains(&"eslint-plugin-unused-control"),
        "unreferenced control dependency should still be reported, got {unused_dev_dependencies:?}"
    );
}

#[test]
fn oxlint_ts_config_credits_object_specifiers_and_local_plugins() {
    let root = fixture_path("issue-607-oxlint-ts-config");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dev_dependencies.contains(&"eslint-plugin-regexp"),
        "object-form jsPlugins specifier should be credited, got {unused_dev_dependencies:?}"
    );
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

    assert!(
        !unused_files.contains(&"oxlint.config.ts".to_string()),
        "oxlint.config.ts should be treated as an Oxlint config, got {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"local-plugin.js".to_string()),
        "local jsPlugins files should be treated as support entry files, got {unused_files:?}"
    );
}
