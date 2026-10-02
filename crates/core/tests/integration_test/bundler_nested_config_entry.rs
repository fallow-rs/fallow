use super::common::{create_config, fixture_path};

/// A `tsdown` or `tsup` config in a workspace package names its `entry` files
/// relative to the config file directory. The tool is also a dependency of the
/// workspace root, so the root run reads the nested config.
#[test]
fn nested_bundler_config_entries_resolve_from_the_config_directory() {
    let root = fixture_path("bundler-nested-config-entry");
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

    for entry in [
        "packages/lib-tsdown/src/cli.ts",
        "packages/lib-tsup/src/cli.ts",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == entry),
            "{entry} is a bundler entry and must be reachable, unused files: {unused_files:?}"
        );
    }

    for orphan in [
        "packages/lib-tsdown/src/orphan.ts",
        "packages/lib-tsup/src/orphan.ts",
    ] {
        assert!(
            unused_files.iter().any(|unused| unused == orphan),
            "{orphan} is not an entry and must stay reportable, unused files: {unused_files:?}"
        );
    }
}
