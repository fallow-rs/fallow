use super::common::{create_config, fixture_path};

#[test]
fn vite_and_vitest_react_babel_plugins_credit_dependencies() {
    let root = fixture_path("issue-619-vite-react-babel-plugins");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    for dep in [
        "@preact/signals-react-transform",
        "@babel/preset-react",
        "babel-plugin-plain",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&dep),
            "{dep} should be credited through @vitejs/plugin-react Babel options, got {unused_dev_dependencies:?}"
        );
    }

    assert!(
        unused_dev_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, got {unused_dev_dependencies:?}"
    );
}
