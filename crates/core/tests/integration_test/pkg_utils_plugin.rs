use std::path::Path;

use super::common::{create_config, fixture_path};

fn unused_file_paths(results: &fallow_core::results::AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn pkg_utils_build_configs_are_kept_reachable() {
    let root = fixture_path("pkg-utils-build-config");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results, &root);

    // Root and nested build configs, discovered by @sanity/pkg-utils on a
    // filename convention rather than imported, must not surface as unused.
    for kept in [
        "package.config.ts",
        "package.bundle.ts",
        "packages/lib/package.config.ts",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == kept),
            "{kept} should be kept reachable by the pkg-utils plugin, unused files: {unused_files:?}"
        );
    }

    // The plugin must not over-credit: a genuinely unreferenced source file
    // still surfaces as unused.
    assert!(
        unused_files.iter().any(|unused| unused == "src/orphan.ts"),
        "unreferenced source files must still be reported, unused files: {unused_files:?}"
    );

    // The build tool itself is invoked via a script binary, not imported from
    // application code, so it must not be reported as an unused dependency.
    assert!(
        !results
            .unused_dev_dependencies
            .iter()
            .any(|dep| dep.dep.package_name == "@sanity/pkg-utils"),
        "@sanity/pkg-utils should be credited as a tooling dependency"
    );
}

#[test]
fn package_config_without_pkg_utils_dependency_stays_flagged() {
    // Control: the gate is strict. A project that does not depend on
    // @sanity/pkg-utils keeps reporting a stray package.config.ts as unused.
    let root = fixture_path("pkg-utils-no-dep");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results, &root);

    assert!(
        unused_files
            .iter()
            .any(|unused| unused == "package.config.ts"),
        "package.config.ts must stay flagged without @sanity/pkg-utils, unused files: {unused_files:?}"
    );
}
