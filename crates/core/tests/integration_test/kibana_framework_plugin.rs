use super::common::{create_config, fixture_path};

fn unused_file_paths(
    root: &std::path::Path,
    results: &fallow_types::results::AnalysisResults,
) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

/// The built-in Kibana plugin seeds plugin entries from `kibana.jsonc`
/// manifests. The fixture has no external plugin file.
#[test]
fn kibana_manifests_seed_plugin_entries_without_external_plugin() {
    let root = fixture_path("kibana-framework-plugin");
    let config = create_config(root.clone());
    assert!(
        config.external_plugins.is_empty(),
        "the fixture must not load an external plugin"
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&root, &results);

    for reachable in [
        "x-pack/plugins/alpha/public/index.ts",
        "x-pack/plugins/alpha/public/helper.ts",
        "x-pack/plugins/alpha/server/index.ts",
        "x-pack/plugins/alpha/common/index.ts",
        "x-pack/plugins/alpha/common/constants/index.ts",
        "x-pack/plugins/alpha/test/scout/ui/parallel.playwright.config.ts",
        "x-pack/plugins/alpha/test/scout/ui/parallel_tests/global.setup.ts",
        "x-pack/plugins/beta/public/index.ts",
    ] {
        assert!(
            !unused.contains(&reachable.to_string()),
            "{reachable} must be reachable from its kibana.jsonc manifest, unused files: {unused:?}"
        );
    }

    for still_unused in [
        // `plugin.server` is false, so the server entry is not seeded.
        "x-pack/plugins/beta/server/index.ts",
        // A file that no entry imports stays unused inside a plugin.
        "x-pack/plugins/alpha/public/orphan.ts",
        // A manifest with a type other than `plugin` seeds no plugin entries.
        "packages/kbn-shared/index.ts",
        "orphan.ts",
    ] {
        assert!(
            unused.contains(&still_unused.to_string()),
            "{still_unused} must stay unused, unused files: {unused:?}"
        );
    }
}

/// The Kibana platform calls `setup`, `start` and `stop` on the class that a
/// plugin entry returns. Other members of that class stay checked.
#[test]
fn kibana_plugin_lifecycle_members_are_used() {
    let root = fixture_path("kibana-framework-plugin");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .filter(|finding| finding.member.parent_name == "AlphaPlugin")
        .map(|finding| finding.member.member_name.clone())
        .collect();

    for lifecycle in ["setup", "start", "stop"] {
        assert!(
            !unused_members.contains(&lifecycle.to_string()),
            "{lifecycle} is a Kibana lifecycle member, unused members: {unused_members:?}"
        );
    }
    assert!(
        unused_members.contains(&"neverCalled".to_string()),
        "a member outside the lifecycle must stay reported, unused members: {unused_members:?}"
    );
}
