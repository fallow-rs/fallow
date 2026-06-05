use super::common::{create_config, fixture_path};

#[test]
fn pino_transport_target_credits_runtime_dependency() {
    let root = fixture_path("issue-954-pino-transport-target");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dependencies: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dependencies.contains(&"pino-pretty"),
        "pino-pretty is referenced by pino transport.target and must be credited, got {unused_dependencies:?}"
    );
    assert!(
        unused_dependencies.contains(&"unused-control"),
        "an unreferenced control dependency must still be reported, got {unused_dependencies:?}"
    );
}
