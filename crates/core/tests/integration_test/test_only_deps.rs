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

    // test-utils-lib is in dependencies but only imported by a test file
    assert!(
        test_only_names.contains(&"test-utils-lib"),
        "test-utils-lib should be detected as test-only dependency, found: {test_only_names:?}"
    );

    // lodash is in dependencies and imported by a production file
    assert!(
        !test_only_names.contains(&"lodash"),
        "lodash should NOT be test-only (has production import), found: {test_only_names:?}"
    );
}
