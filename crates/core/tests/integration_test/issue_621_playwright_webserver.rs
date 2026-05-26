use super::common::{create_config, fixture_path};

/// Playwright `webServer.command` should credit CLI dependencies invoked there
/// and mark local script files launched there as reachable, while leaving an
/// unreferenced control dependency reported. See issue #621.
#[test]
fn web_server_command_credits_deps_and_script_entries() {
    let root = fixture_path("issue-621-playwright-webserver-command");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"srvx"),
        "srvx is invoked by webServer.command and must be credited, got {unused_deps:?}"
    );

    let unused_dev_deps: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dev_deps.contains(&"tsx"),
        "tsx runs the e2e server via webServer.command and must be credited, got {unused_dev_deps:?}"
    );
    assert!(
        unused_dev_deps.contains(&"unused-control"),
        "an unreferenced control dependency must still be reported, got {unused_dev_deps:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|p| p.ends_with("scripts/e2e-server.ts")),
        "scripts/e2e-server.ts is launched by webServer.command and must be reachable, got {unused_files:?}"
    );
}
