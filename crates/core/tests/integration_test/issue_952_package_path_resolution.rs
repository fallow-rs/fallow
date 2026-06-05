use super::common::{create_config, fixture_path};

#[test]
fn package_path_resolution_credits_runtime_dependencies() {
    let root = fixture_path("issue-952-package-path-resolution");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dependencies: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    for package_name in ["ffmpeg-static", "ffprobe-static", "@fontsource/inter"] {
        assert!(
            !unused_dependencies.contains(&package_name),
            "{package_name} should be credited from static package path resolution, got {unused_dependencies:?}"
        );
    }
    assert!(
        unused_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, got {unused_dependencies:?}"
    );
}
