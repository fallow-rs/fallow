use super::common::{create_config, fixture_path};

#[test]
fn quoted_wrapper_option_does_not_credit_a_dependency_named_inside_it() {
    let config = create_config(fixture_path("varlock-quoted-args"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        unused.contains(&"is-ci"),
        "option content is not a child executable: {unused:?}"
    );
    assert!(
        !unused.contains(&"publint"),
        "the real child must be credited: {unused:?}"
    );
    assert!(
        !unused.contains(&"varlock"),
        "the wrapper must be credited: {unused:?}"
    );
}

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

    assert!(
        !unused_dev_dep_names.contains(&"@arethetypeswrong/cli"),
        "@arethetypeswrong/cli should be detected as used via its 'attw' binary in scripts, unused dev deps: {unused_dev_dep_names:?}"
    );

    assert!(
        !unused_dev_dep_names.contains(&"publint"),
        "publint should be detected as used via scripts, unused dev deps: {unused_dev_dep_names:?}"
    );

    assert!(
        !unused_dev_dep_names.contains(&"@j178/prek"),
        "@j178/prek should be detected as used via its 'prek' binary in `bun --bun prek install`, unused dev deps: {unused_dev_dep_names:?}"
    );

    assert!(
        !unused_dev_dep_names.contains(&"is-ci"),
        "is-ci should be detected as used via scripts, unused dev deps: {unused_dev_dep_names:?}"
    );

    assert!(
        !unused_dev_dep_names.contains(&"varlock"),
        "varlock should be detected as used via scripts, unused dev deps: {unused_dev_dep_names:?}"
    );
}
