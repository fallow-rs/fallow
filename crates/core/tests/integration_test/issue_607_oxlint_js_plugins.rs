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

#[test]
fn local_js_plugin_default_export_is_used_with_include_entry_exports() {
    // Issue #3871: oxlint loads the default export of a local jsPlugins file.
    let root = fixture_path("issue-607-oxlint-ts-config");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let plugin_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .filter(|export| {
            export
                .export
                .path
                .ends_with(std::path::Path::new("plugins/local-plugin.js"))
        })
        .map(|export| export.export.export_name.as_str())
        .collect();

    assert_eq!(
        plugin_exports,
        vec!["unusedHelper"],
        "only the named export of the local plugin stays reportable"
    );
}

#[test]
fn local_js_plugin_directory_and_extensionless_specifiers_credit_the_resolved_file() {
    // Issue #3961: a jsPlugins entry can name a directory (resolved through
    // `package.json` `main` or `index.js`) or a file without its extension.
    let root = fixture_path("issue-3961-oxlint-js-plugin-directory");
    let mut config = create_config(root.clone());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|export| {
            let relative = export
                .export
                .path
                .strip_prefix(&root)
                .unwrap_or(&export.export.path)
                .to_string_lossy()
                .replace('\\', "/");
            format!("{relative}:{}", export.export.export_name)
        })
        .collect();
    unused_exports.sort();

    assert_eq!(
        unused_exports,
        vec![
            "plugins/bare-plugin.js:unusedHelper",
            "plugins/dir-plugin/lib/plugin.js:unusedHelper",
            "plugins/index-plugin/index.js:unusedHelper",
            "src/index.ts:app",
        ],
        "the default export of each resolved plugin file is credited"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        unused_files.is_empty(),
        "resolved plugin files are support entry files, got {unused_files:?}"
    );
}

#[test]
fn local_js_plugin_directory_with_build_output_main_credits_the_source_file() {
    // Issue #4034: the `main` field of a jsPlugins directory points at build
    // output. The tsconfig maps that output back to `src/index.ts`, so the
    // default export of the source file is credited.
    let root = fixture_path("issue-4034-oxlint-js-plugin-dist");
    let mut config = create_config(root.clone());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|export| {
            let relative = export
                .export
                .path
                .strip_prefix(&root)
                .unwrap_or(&export.export.path)
                .to_string_lossy()
                .replace('\\', "/");
            format!("{relative}:{}", export.export.export_name)
        })
        .collect();
    unused_exports.sort();

    assert_eq!(
        unused_exports,
        vec![
            "packages/plugin/src/index.ts:unusedHelper",
            "src/index.ts:app",
        ],
        "the default export of the mapped source file is credited"
    );
}

#[test]
fn ultracite_selected_js_plugins_credit_only_selected_dependencies() {
    let root = fixture_path("ultracite-oxlint-js-plugins");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dev_dependencies.contains(&"eslint-plugin-github"),
        "eslint-plugin-github should be credited through Ultracite's selected jsPlugins, got {unused_dev_dependencies:?}"
    );

    for plugin in ["eslint-plugin-sonarjs", "eslint-plugin-unused-control"] {
        assert!(
            unused_dev_dependencies.contains(&plugin),
            "unselected {plugin} should still be reported, got {unused_dev_dependencies:?}"
        );
    }
}
