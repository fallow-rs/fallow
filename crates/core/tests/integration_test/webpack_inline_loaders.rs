use super::common::{create_config, fixture_path};

#[test]
fn webpack_inline_loader_requests_resolve_resource_and_credit_loaders() {
    let root = fixture_path("webpack-inline-loaders");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "inline loader requests should resolve to their resource, got unresolved: {unresolved:?}"
    );

    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    unused_files.sort();
    assert_eq!(
        unused_files,
        vec!["src/orphan.js".to_string()],
        "loader resources should be reachable"
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|export| export.export.export_name.clone())
        .collect();
    assert!(
        unused_exports.is_empty(),
        "a loader replaces the exports of its resource, so a loader import uses the whole resource: {unused_exports:?}"
    );

    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .chain(
            results
                .unused_dev_dependencies
                .iter()
                .map(|dep| dep.dep.package_name.as_str()),
        )
        .collect();
    assert_eq!(
        unused_deps,
        vec!["left-pad"],
        "inline loader packages should count as used dependencies"
    );

    let dev_in_production: Vec<&str> = results
        .dev_dependencies_in_production
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        dev_in_production.is_empty(),
        "loaders run at build time, so a loader devDependency is not a production import: {dev_in_production:?}"
    );

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        unlisted.is_empty(),
        "loader requests must not report unlisted packages, got {unlisted:?}"
    );
}

#[test]
fn webpack_inline_loader_request_with_missing_resource_reports_the_full_request() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"missing-resource","main":"src/index.js","dependencies":{"raw-loader":"^4.0.2"}}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.js"),
        "export const text = require('!raw-loader!./missing.js');\n",
    )
    .expect("write index.js");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert_eq!(unresolved, vec!["!raw-loader!./missing.js"]);
    assert!(
        results.unused_dependencies.is_empty(),
        "the loader package is used even when the resource is missing: {:?}",
        results.unused_dependencies
    );
}
