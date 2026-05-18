use super::common::{create_config, fixture_path};

#[test]
fn divergent_binary_name_not_flagged_as_unused() {
    let root = fixture_path("bin-script-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    // @arethetypeswrong/cli provides binary "attw" used in the "lint" script.
    // The bin-to-package map should resolve "attw" → "@arethetypeswrong/cli".
    assert!(
        !unused_dev_dep_names.contains(&"@arethetypeswrong/cli"),
        "@arethetypeswrong/cli should be detected as used via its 'attw' binary in scripts, unused dev deps: {unused_dev_dep_names:?}"
    );

    // publint uses string bin form (binary name = package name), should also work.
    assert!(
        !unused_dev_dep_names.contains(&"publint"),
        "publint should be detected as used via scripts, unused dev deps: {unused_dev_dep_names:?}"
    );
}
