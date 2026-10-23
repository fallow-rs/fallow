//! A dependency installed under an npm alias, such as
//! `"@typescript/native": "npm:typescript@~7.0.2"`, provides the binaries of
//! the aliased package. A script that runs such a binary credits the alias.
//! An alias that no script or import uses is still reported, and so is an
//! alias next to a plain declaration of the same package, because the plain
//! declaration provides the binary.

use super::common::{create_config, fixture_path};

#[cfg_attr(miri, ignore)]
#[test]
fn script_binary_credits_the_npm_alias_that_installs_it() {
    let root = fixture_path("npm-alias-dependency-binaries");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert_eq!(
        unused,
        ["vite-6", "idle-alias"],
        "a script binary credits the npm alias of the package that provides it"
    );
}

#[cfg_attr(miri, ignore)]
#[test]
fn plain_declaration_in_another_workspace_keeps_the_alias_credit() {
    let root = fixture_path("npm-alias-workspace-binaries");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unused.contains(&"@typescript/native"),
        "a plain declaration in another workspace must not block the alias credit, got {unused:?}"
    );
}
