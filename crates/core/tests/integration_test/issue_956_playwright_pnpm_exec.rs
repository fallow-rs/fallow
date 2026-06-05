use super::common::{create_config, fixture_path};

/// Playwright `webServer.command` can be a template literal with env interpolation.
/// The static command still invokes `srvx` through `pnpm exec`; see issue #956.
#[test]
fn playwright_web_server_template_command_credits_pnpm_exec_cli() {
    let root = fixture_path("issue-956-playwright-pnpm-exec");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_deps: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dev_deps.contains(&"srvx"),
        "srvx is invoked by Playwright webServer.command and must be credited, got {unused_dev_deps:?}"
    );
    assert!(
        unused_dev_deps.contains(&"unused-control"),
        "an unreferenced control dependency must still be reported, got {unused_dev_deps:?}"
    );
}
