use super::common::{create_config, fixture_path};

#[test]
fn dotenv_cli_packages_are_credited_when_scripts_run_the_wrappers() {
    let root = fixture_path("dotenv-plugin");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();

    for used in ["dotenv-cli", "@dotenvx/dotenvx"] {
        assert!(
            !unused_dev_dependencies.contains(&used),
            "{used} is invoked by a package script, unused dev dependencies: {unused_dev_dependencies:?}"
        );
    }
    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "an unrelated dependency must stay unused: {unused_dev_dependencies:?}"
    );
    assert!(
        results
            .unused_dependencies
            .iter()
            .any(|finding| finding.dep.package_name == "dotenv"),
        "the dotenv library must not be credited merely because a CLI wrapper is used"
    );
}
