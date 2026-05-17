use super::common::{create_config, fixture_path};

fn create_private_type_leak_config(root: std::path::PathBuf) -> fallow_config::ResolvedConfig {
    let mut config = create_config(root);
    config.rules.private_type_leaks = fallow_config::Severity::Warn;
    config
}

#[test]
fn exported_signatures_report_same_file_private_types() {
    let root = fixture_path("private-type-leaks");
    let config = create_private_type_leak_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let leaks: Vec<(&str, &str)> = results
        .private_type_leaks
        .iter()
        .map(|leak| (leak.leak.export_name.as_str(), leak.leak.type_name.as_str()))
        .collect();

    assert!(
        leaks.contains(&("Component", "Props")),
        "Component should report Props as a private type leak, found: {leaks:?}"
    );
    assert!(
        leaks.contains(&("Service", "Options")),
        "Service should report Options as a private type leak, found: {leaks:?}"
    );
    assert!(
        !leaks.contains(&("Service", "InternalState")),
        "ECMAScript private fields should not be treated as public signature leaks: {leaks:?}"
    );
    assert!(
        !leaks.contains(&("UsesExportedType", "PublicBacking")),
        "exported backing types should not be reported as private leaks: {leaks:?}"
    );
}

#[test]
fn exported_signature_backing_types_are_not_unused_type_exports() {
    let root = fixture_path("private-type-leaks");
    let config = create_private_type_leak_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_types: Vec<&str> = results
        .unused_types
        .iter()
        .map(|export| export.export.export_name.as_str())
        .collect();

    assert!(
        !unused_types.contains(&"PublicBacking"),
        "PublicBacking backs public signatures and should not become an unused type export: {unused_types:?}"
    );
}

#[test]
fn storybook_story_files_are_skipped() {
    let root = fixture_path("private-type-leaks");
    let config = create_private_type_leak_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // The fixture's Component.stories.ts uses the canonical
    // `type Story = StoryObj<...>; export const Default: Story = ...`
    // pattern. Without the storybook-suffix skip, every story export would
    // be reported as a private-type-leak. Reverting `is_storybook_file`
    // makes this assertion fail.
    let storybook_leaks: Vec<&str> = results
        .private_type_leaks
        .iter()
        .filter(|leak| leak.leak.path.ends_with("Component.stories.ts"))
        .map(|leak| leak.leak.export_name.as_str())
        .collect();

    assert!(
        storybook_leaks.is_empty(),
        "storybook story files should be skipped, but found leaks for: {storybook_leaks:?}"
    );
}

#[test]
fn route_convention_files_are_skipped() {
    let root = fixture_path("private-type-leaks");
    let config = create_private_type_leak_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Each fixture file declares a local type and uses it across 2+ exports,
    // the canonical noise pattern for framework routing conventions. Without
    // the route-convention skip these would generate multiple leaks each.
    // Reverting `is_route_convention_file` makes any of these assertions fail.
    let convention_paths = [
        "app/blog/[slug]/page.tsx", // Next.js App Router
        "pages/[slug].tsx",         // Next.js Pages Router
        "app/routes/posts.$id.tsx", // Remix / TanStack Router
        "src/templates/post.tsx",   // Gatsby
        "app/+not-found.tsx",       // Expo Router special file (`+*` glob)
    ];

    for relative in &convention_paths {
        let leaks: Vec<&str> = results
            .private_type_leaks
            .iter()
            .filter(|leak| {
                leak.leak
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .contains(relative)
            })
            .map(|leak| leak.leak.export_name.as_str())
            .collect();
        assert!(
            leaks.is_empty(),
            "route convention file {relative} should be skipped, but found leaks for: {leaks:?}"
        );
    }

    // Counter-check: non-route files in the same fixture must still be
    // analyzed. Locks the skip predicate as path-scoped so a regression that
    // makes `is_route_convention_file` always-true would fail here.
    let index_leaks: Vec<&str> = results
        .private_type_leaks
        .iter()
        .filter(|leak| {
            leak.leak
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/index.ts")
        })
        .map(|leak| leak.leak.export_name.as_str())
        .collect();
    assert!(
        index_leaks.contains(&"Component"),
        "non-route src/index.ts must still be analyzed, expected Component leak in {index_leaks:?}"
    );

    // Co-located helper inside `app/routes/<segment>/` is NOT a route file and
    // its leak must still be reported. Locks down the `literal_separator(true)`
    // contract on `**/routes/*.{ts,tsx,...}`; without that flag a single `*`
    // would cross `/` and silently swallow this file.
    let helper_leaks: Vec<&str> = results
        .private_type_leaks
        .iter()
        .filter(|leak| {
            leak.leak
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("app/routes/utils/format.ts")
        })
        .map(|leak| leak.leak.export_name.as_str())
        .collect();
    assert!(
        helper_leaks.contains(&"formatDate"),
        "co-located route helper should still report private-type-leak, found: {helper_leaks:?}"
    );
}
