use super::common::{create_config, fixture_path};

#[test]
fn test_only_dependency_detected() {
    let root = fixture_path("test-only-prod-dep");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let test_only_names: Vec<&str> = results
        .test_only_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        test_only_names.contains(&"test-utils-lib"),
        "test-utils-lib should be detected as test-only dependency, found: {test_only_names:?}"
    );

    assert!(
        !test_only_names.contains(&"lodash"),
        "lodash should NOT be test-only (has production import), found: {test_only_names:?}"
    );
}

/// A test runner config can name test files that the built-in name patterns
/// do not match, such as Vitest `include: ['src/**/*.vitest.ts']`. A package
/// that only such files import is test-only (issue #4618).
#[test]
fn test_runner_entry_files_count_as_test_files() {
    let root = fixture_path("test-only-runner-test-files");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let test_only_names: Vec<&str> = results
        .test_only_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        test_only_names.contains(&"left-pad"),
        "only a Vitest test file imports left-pad, found: {test_only_names:?}"
    );
    assert!(
        !test_only_names.contains(&"lodash"),
        "lodash has a production import, found: {test_only_names:?}"
    );
}

/// A test runner can select every file below a directory, such as Playwright
/// `testDir: './src'` with a regular expression `testMatch`. A test entry
/// that a runtime entry point or a package.json script reaches stays
/// production code.
#[test]
fn runtime_reachable_test_runner_entries_stay_production_code() {
    let root = fixture_path("test-only-runner-test-dir");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let test_only_names: Vec<&str> = results
        .test_only_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        test_only_names.contains(&"left-pad"),
        "only a Playwright test file imports left-pad, found: {test_only_names:?}"
    );
    assert!(
        !test_only_names.contains(&"lodash"),
        "the runtime entry reaches the lodash importer, found: {test_only_names:?}"
    );
    assert!(
        !test_only_names.contains(&"chalk"),
        "the seed script runs the chalk importer, found: {test_only_names:?}"
    );
}
