use super::common::{create_config, fixture_path};

/// `oxlint-tsgolint` is the type-aware companion package the oxlint binary loads at
/// runtime. It is never imported in source nor listed in an `.oxlintrc.json`
/// `jsPlugins` array, so the #607 jsPlugins credit does not cover it. When declared
/// in prod `dependencies` (where the general tooling-prefix credit does not apply),
/// it was falsely reported as unused. The oxlint plugin now credits it as a CLI
/// tooling dependency, which is honored for both prod and dev categories.
#[test]
fn oxlint_cli_tooling_credited_in_prod_dependencies() {
    let root = fixture_path("issue-753-oxlint-cli-tooling");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dependencies: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dependencies.contains(&"oxlint-tsgolint"),
        "oxlint-tsgolint should be credited as an oxlint CLI tooling dependency, got {unused_dependencies:?}"
    );

    // Exact-name credit, NOT an `oxlint-*` prefix: an unrelated `oxlint-` prefixed
    // prod dependency that is not a known CLI tooling package still reports.
    assert!(
        unused_dependencies.contains(&"oxlint-other"),
        "an unknown oxlint-prefixed prod dependency should still report, got {unused_dependencies:?}"
    );

    // Generic control: a genuinely-unused unrelated prod dependency still reports.
    assert!(
        unused_dependencies.contains(&"unused-control-dep"),
        "an unrelated unused prod dependency should still report, got {unused_dependencies:?}"
    );
}
