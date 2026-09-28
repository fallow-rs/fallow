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
        vec![
            "src/orphan.js".to_string(),
            "src/sandbox/helper.js".to_string()
        ],
        "loader resources should be used, and an asset loader resource must not keep its own imports alive"
    );

    // `src/sandbox/helper.js` is an unused file. Its export is referenced only
    // from the unreachable raw-loader resource, so it is reported too, as for
    // any chain of unused files.
    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .filter(|export| !export.export.path.ends_with("src/sandbox/helper.js"))
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
        "loaders run at build time and a raw-loader resource never runs, so neither is a production import: {dev_in_production:?}"
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

#[test]
fn plain_path_with_a_bang_resolves_as_a_plain_path() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"bang-path","main":"src/index.js"}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.js"),
        "import w from './we!rd.js';\nimport r from './a!b/c.js';\nexport const both = [w, r];\n",
    )
    .expect("write index.js");
    std::fs::write(root.join("src/we!rd.js"), "export default 1;\n").expect("write we!rd.js");
    std::fs::create_dir_all(root.join("src/a!b")).expect("create a!b");
    std::fs::write(root.join("src/a!b/c.js"), "export default 2;\n").expect("write c.js");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(unresolved.is_empty(), "got unresolved: {unresolved:?}");
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(unlisted.is_empty(), "got unlisted: {unlisted:?}");
    let unused_files: Vec<_> = results.unused_files.iter().map(|f| &f.file.path).collect();
    assert!(
        unused_files.is_empty(),
        "got unused files: {unused_files:?}"
    );
    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|export| export.export.export_name.as_str())
        .collect();
    assert!(
        unused_exports.is_empty(),
        "got unused exports: {unused_exports:?}"
    );
}

#[test]
fn webpack_1_short_loader_name_credits_the_loader_package() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"short-loader","main":"src/index.js","devDependencies":{"raw-loader":"^0.5.1"}}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.js"),
        "export const text = require('raw!./template.js');\n",
    )
    .expect("write index.js");
    std::fs::write(root.join("src/template.js"), "export const t = 1;\n")
        .expect("write template.js");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_dev_dependencies.is_empty(),
        "webpack 1 resolved `raw` to `raw-loader`: {:?}",
        results.unused_dev_dependencies
    );
    assert!(results.unresolved_imports.is_empty());
    assert!(results.unused_files.is_empty());
}

/// Write a project whose entry re-exports resources through inline loaders,
/// with named and star re-exports through an asset loader and a code loader.
fn write_loader_re_export_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src/sandbox")).expect("create src/sandbox");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"loader-re-export","main":"src/index.js","devDependencies":{"dev-only":"^1.0.0","raw-loader":"^4.0.2","worker-loader":"^3.0.8"}}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.js"),
        "export { default as source } from 'raw-loader!./sandbox/reex.js';\n\
         export { default as Worker } from 'worker-loader!./worker.js';\n\
         export * from 'raw-loader!./sandbox/star.js';\n\
         export * from 'worker-loader!./star-worker.js';\n",
    )
    .expect("write index.js");
    std::fs::write(
        root.join("src/sandbox/reex.js"),
        "import 'dev-only';\nimport { local } from './local.js';\n\
         export default local;\nexport const extra = 2;\n",
    )
    .expect("write reex.js");
    std::fs::write(
        root.join("src/sandbox/local.js"),
        "export const local = 1;\n",
    )
    .expect("write local.js");
    std::fs::write(
        root.join("src/worker.js"),
        "import { job } from './job.js';\n\
         export const first = 1;\nexport const second = 2;\n\
         self.onmessage = () => job();\n",
    )
    .expect("write worker.js");
    std::fs::write(root.join("src/job.js"), "export const job = () => 1;\n").expect("write job.js");
    std::fs::write(
        root.join("src/sandbox/star.js"),
        "import { starLocal } from './star-local.js';\n\
         export const alpha = starLocal;\nexport default 3;\n",
    )
    .expect("write star.js");
    std::fs::write(
        root.join("src/sandbox/star-local.js"),
        "export const starLocal = 1;\n",
    )
    .expect("write star-local.js");
    std::fs::write(
        root.join("src/star-worker.js"),
        "import { starJob } from './star-job.js';\n\
         export const beta = 1;\nself.onmessage = () => starJob();\n",
    )
    .expect("write star-worker.js");
    std::fs::write(
        root.join("src/star-job.js"),
        "export const starJob = () => 1;\n",
    )
    .expect("write star-job.js");
}

#[test]
fn webpack_inline_loader_re_export_credits_the_whole_resource() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_loader_re_export_project(root);
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(unresolved.is_empty(), "got unresolved: {unresolved:?}");

    let dev_in_production: Vec<&str> = results
        .dev_dependencies_in_production
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        dev_in_production.is_empty(),
        "a raw-loader resource never runs, so its imports are not production imports: {dev_in_production:?}"
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
        vec![
            "src/sandbox/local.js".to_string(),
            "src/sandbox/star-local.js".to_string(),
        ],
        "a re-exported raw-loader resource must not keep its own imports in use, a worker-loader resource must"
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .filter(|export| {
            !export.export.path.ends_with("src/sandbox/local.js")
                && !export.export.path.ends_with("src/sandbox/star-local.js")
        })
        .map(|export| export.export.export_name.clone())
        .collect();
    assert!(
        unused_exports.is_empty(),
        "a loader re-export uses the whole resource: {unused_exports:?}"
    );

    let unused_deps: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"raw-loader") && !unused_deps.contains(&"worker-loader"),
        "inline loader packages in a re-export are used: {unused_deps:?}"
    );
}
