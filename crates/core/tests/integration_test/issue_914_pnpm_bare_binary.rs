use super::common::{create_config, fixture_path};

#[test]
fn pnpm_bare_declared_binary_script_credits_dev_dependency() {
    let root = fixture_path("issue-914-pnpm-bare-binary");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dev_dependencies.contains(&"envinfo"),
        "envinfo should be credited from `pnpm envinfo`, got: {unused_dev_dependencies:?}"
    );
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, got: {unused_dev_dependencies:?}"
    );
}
