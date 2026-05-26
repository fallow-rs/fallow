use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::collect_unused_files;

#[test]
fn mintlify_docs_root_is_credited_and_cli_dependency_is_tooling() {
    let root = fixture_path("mintlify-docs-project");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);

    // Acceptance criterion 1: docs MDX under the docs.json directory is treated
    // as runtime-used without manual `dynamicallyLoaded` config.
    for docs_page in [
        "apps/docs/introduction.mdx",
        "apps/docs/guides/quickstart.mdx",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == docs_page),
            "{docs_page} should be credited as Mintlify docs content, unused files: {unused_files:?}"
        );
    }

    // Acceptance criterion 2: `mint` invoked via the docs script and active as
    // tooling must not be reported as an unused (dev) dependency. Collect every
    // package name across the regular, dev, and optional dependency buckets.
    let mut unused_dependencies: Vec<String> = Vec::new();
    unused_dependencies.extend(
        results
            .unused_dependencies
            .iter()
            .map(|finding| finding.dep.package_name.clone()),
    );
    unused_dependencies.extend(
        results
            .unused_dev_dependencies
            .iter()
            .map(|finding| finding.dep.package_name.clone()),
    );
    unused_dependencies.extend(
        results
            .unused_optional_dependencies
            .iter()
            .map(|finding| finding.dep.package_name.clone()),
    );
    assert!(
        !unused_dependencies.iter().any(|name| name == "mint"),
        "mint CLI should be credited as Mintlify tooling, unused deps: {unused_dependencies:?}"
    );

    // Acceptance criterion 3: MDX outside the Mintlify docs root is NOT credited
    // by this plugin and remains reportable as an unused file.
    assert!(
        unused_files
            .iter()
            .any(|path| path == "apps/web/src/orphan.mdx"),
        "MDX outside the docs root must stay governed by normal analysis, unused files: {unused_files:?}"
    );
}
