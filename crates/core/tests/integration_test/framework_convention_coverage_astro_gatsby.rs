use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::{
    collect_unused_exports, collect_unused_files, has_unused_export,
};

#[test]
fn astro_template_script_src_and_inline_imports_are_followed() {
    let root = fixture_path("astro-script-references");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    for expected_used_file in ["src/scripts/foo.ts", "src/scripts/bar.ts"] {
        assert!(
            !unused_files.iter().any(|path| path == expected_used_file),
            "{expected_used_file} should be reachable via Astro <script>, unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_files.iter().any(|path| path == "src/scripts/raw.ts"),
        "is:inline script imports should not create reachability edges, unused files: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(&root, &results);
    assert!(
        has_unused_export(&unused_exports, "src/scripts/bar.ts", "unusedHelper"),
        "unusedHelper export should still be flagged, found: {unused_exports:?}"
    );
}

#[test]
fn astro_current_convention_files_and_exports_are_covered() {
    let root = fixture_path("astro-conventions");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    for expected_used_file in [
        "src/actions/index.ts",
        "src/content/config.ts",
        "src/middleware/index.ts",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == expected_used_file),
            "{expected_used_file} should be treated as framework-used, unused files: {unused_files:?}"
        );
    }

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("src/actions/index.ts", "server"),
        ("src/content/config.ts", "collections"),
        ("src/middleware/index.ts", "onRequest"),
        ("src/pages/blog/[slug].astro", "getStaticPaths"),
        ("src/pages/blog/[slug].astro", "prerender"),
        ("src/pages/blog/[slug].astro", "partial"),
        ("src/pages/api/data.ts", "GET"),
        ("src/pages/api/data.ts", "POST"),
        ("src/pages/api/data.ts", "prerender"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be treated as framework-used, found: {unused_exports:?}"
        );
    }

    for (path, export) in [
        ("src/actions/index.ts", "unusedActionHelper"),
        ("src/content/config.ts", "unusedCollectionHelper"),
        ("src/middleware/index.ts", "unusedMiddlewareHelper"),
        ("src/pages/blog/[slug].astro", "unusedPageHelper"),
        ("src/pages/api/data.ts", "unusedEndpointHelper"),
    ] {
        assert!(
            has_unused_export(&unused_exports, path, export),
            "{path}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn astro_component_props_type_is_framework_used() {
    let root = fixture_path("astro-props");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_types: Vec<(String, String)> = results
        .unused_types
        .iter()
        .map(|finding| {
            let path = finding
                .export
                .path
                .strip_prefix(&root)
                .unwrap_or(&finding.export.path)
                .to_string_lossy()
                .replace('\\', "/");
            (path, finding.export.export_name.clone())
        })
        .collect();
    let has = |path: &str, name: &str| unused_types.iter().any(|(p, n)| p == path && n == name);

    assert!(
        !has("src/components/Card.astro", "Props"),
        "Astro reads Props of a .astro file, found: {unused_types:?}"
    );
    assert!(
        has("src/components/Card.astro", "UnusedCardType"),
        "other unused types in a .astro file stay reported, found: {unused_types:?}"
    );
    assert!(
        has("src/shapes.ts", "Props"),
        "Props in a .ts file stays subject to the normal check, found: {unused_types:?}"
    );
}

#[test]
fn gatsby_pages_and_functions_keep_convention_exports_but_flag_dead_helpers() {
    let root = fixture_path("gatsby-conventions");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("src/pages/index.tsx", "default"),
        ("src/pages/index.tsx", "Head"),
        ("src/pages/index.tsx", "query"),
        ("src/pages/index.tsx", "config"),
        ("src/pages/index.tsx", "getServerData"),
        ("src/api/hello.ts", "default"),
        ("src/api/hello.ts", "config"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be treated as framework-used, found: {unused_exports:?}"
        );
    }

    for (path, export) in [
        ("src/pages/index.tsx", "unusedPageHelper"),
        ("src/api/hello.ts", "unusedFunctionHelper"),
    ] {
        assert!(
            has_unused_export(&unused_exports, path, export),
            "{path}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}
