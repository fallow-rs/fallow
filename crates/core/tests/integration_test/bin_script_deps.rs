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

    // @j178/prek provides binary "prek" invoked via `bun --bun prek install` in
    // the "prepare" script. The bun executor flag (`--bun`) must be skipped so
    // `prek` is extracted and resolved to @j178/prek through the bin map.
    assert!(
        !unused_dev_dep_names.contains(&"@j178/prek"),
        "@j178/prek should be detected as used via its 'prek' binary in `bun --bun prek install`, unused dev deps: {unused_dev_dep_names:?}"
    );

    // is-ci is invoked as a bare binary in the same script (identity resolution).
    assert!(
        !unused_dev_dep_names.contains(&"is-ci"),
        "is-ci should be detected as used via scripts, unused dev deps: {unused_dev_dep_names:?}"
    );
}
