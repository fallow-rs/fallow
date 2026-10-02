use super::common::{create_config, fixture_path};

/// A root `tsdown` config with a `workspace` option builds the packages that
/// its globs match. tsdown loads the config file of each matched package, so
/// those config files are used. A config outside the globs stays reportable.
#[test]
fn root_tsdown_workspace_globs_mark_nested_configs_as_used() {
    let root = fixture_path("tsdown-root-workspace-globs");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    for used in [
        "tsdown.config.ts",
        "packages/lib-a/tsdown.config.ts",
        "packages/lib-b/tsdown.config.mts",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == used),
            "{used} is loaded by the root tsdown workspace build, unused files: {unused_files:?}"
        );
    }

    assert!(
        unused_files
            .iter()
            .any(|unused| unused == "tools/build-helper/tsdown.config.ts"),
        "a config outside the workspace globs must stay reportable, unused files: {unused_files:?}"
    );
}
