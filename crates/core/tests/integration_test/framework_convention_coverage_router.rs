use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::{
    collect_unused_exports, collect_unused_files, has_unused_export,
};

fn assert_bundle_boundary_modules_are_traversed(
    root: &std::path::Path,
    results: &fallow_core::results::AnalysisResults,
) {
    let unresolved: Vec<(String, String)> = results
        .unresolved_imports
        .iter()
        .map(|import| {
            (
                import
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&import.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                import.specifier.clone(),
            )
        })
        .collect();

    for specifier in ["../.client/analytics", "../.server/db"] {
        assert!(
            !unresolved
                .iter()
                .any(|(path, spec)| path == "app/routes/_index.tsx" && spec == specifier),
            "{specifier} should resolve through .client/.server discovery, found: {unresolved:?}"
        );
    }

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.package_name.as_str())
        .collect();
    for dep in ["@prisma/client", "browser-analytics"] {
        assert!(
            !unused_dep_names.contains(&dep),
            "{dep} is imported from .client/.server code and should be marked used: {unused_dep_names:?}"
        );
    }
}

#[test]
fn react_router_route_config_root_and_route_exports_are_covered() {
    let root = fixture_path("react-router-conventions");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert_bundle_boundary_modules_are_traversed(&root, &results);

    let unused_files = collect_unused_files(&root, &results);
    assert!(
        !unused_files.iter().any(|path| path == "app/routes.ts"),
        "app/routes.ts should be treated as framework-used, unused files: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("app/routes.ts", "default"),
        ("app/root.tsx", "Layout"),
        ("app/root.tsx", "clientLoader"),
        ("app/root.tsx", "clientAction"),
        ("app/root.tsx", "HydrateFallback"),
        ("app/routes/_index.tsx", "middleware"),
        ("app/routes/_index.tsx", "clientMiddleware"),
        ("app/routes/_index.tsx", "shouldRevalidate"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be treated as framework-used, found: {unused_exports:?}"
        );
    }

    for (path, export) in [
        ("app/routes.ts", "unusedRouteConfigHelper"),
        ("app/root.tsx", "unusedRootHelper"),
        ("app/routes/_index.tsx", "unusedRouteHelper"),
    ] {
        assert!(
            has_unused_export(&unused_exports, path, export),
            "{path}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn remix_root_and_client_data_exports_are_covered() {
    let root = fixture_path("remix-conventions");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert_bundle_boundary_modules_are_traversed(&root, &results);

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("app/root.tsx", "Layout"),
        ("app/root.tsx", "clientLoader"),
        ("app/root.tsx", "clientAction"),
        ("app/root.tsx", "shouldRevalidate"),
        ("app/root.tsx", "HydrateFallback"),
        ("app/routes/_index.tsx", "clientLoader"),
        ("app/routes/_index.tsx", "clientAction"),
        ("app/routes/_index.tsx", "shouldRevalidate"),
        ("app/routes/_index.tsx", "HydrateFallback"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be treated as framework-used, found: {unused_exports:?}"
        );
    }

    for (path, export) in [
        ("app/root.tsx", "unusedRootHelper"),
        ("app/routes/_index.tsx", "unusedRouteHelper"),
    ] {
        assert!(
            has_unused_export(&unused_exports, path, export),
            "{path}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn react_router_route_config_discovers_modules_outside_routes_dir() {
    let root = fixture_path("react-router-config-routes");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    for expected_used_file in [
        "app/routes.ts",
        "app/root.tsx",
        "app/marketing/home.tsx",
        "app/account/layout.tsx",
        "app/account/login.tsx",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == expected_used_file),
            "{expected_used_file} should be treated as framework-used, unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_files.iter().any(|path| path == "app/not-routed.tsx"),
        "plain file outside the route config should stay unused, unused files: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("app/routes.ts", "default"),
        ("app/root.tsx", "Layout"),
        ("app/root.tsx", "HydrateFallback"),
        ("app/marketing/home.tsx", "loader"),
        ("app/account/layout.tsx", "handle"),
        ("app/account/login.tsx", "action"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be treated as framework-used, found: {unused_exports:?}"
        );
    }

    for (path, export) in [
        ("app/routes.ts", "unusedRouteConfigHelper"),
        ("app/root.tsx", "unusedRootHelper"),
        ("app/marketing/home.tsx", "unusedHomeHelper"),
        ("app/account/layout.tsx", "unusedLayoutHelper"),
        ("app/account/login.tsx", "unusedLoginHelper"),
    ] {
        assert!(
            has_unused_export(&unused_exports, path, export),
            "{path}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn react_router_custom_app_directory_keeps_src_routes_alive() {
    let root = fixture_path("react-router-src-app");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    for expected_used_file in [
        "react-router.config.ts",
        "src/routes.ts",
        "src/root.tsx",
        "src/marketing/home.tsx",
        "src/account/layout.tsx",
        "src/account/login.tsx",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == expected_used_file),
            "{expected_used_file} should be treated as framework-used, unused files: {unused_files:?}"
        );
    }

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("src/routes.ts", "default"),
        ("src/root.tsx", "Layout"),
        ("src/marketing/home.tsx", "loader"),
        ("src/account/layout.tsx", "handle"),
        ("src/account/login.tsx", "action"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be treated as framework-used, found: {unused_exports:?}"
        );
    }

    for (path, export) in [
        ("src/routes.ts", "unusedRouteConfigHelper"),
        ("src/root.tsx", "unusedRootHelper"),
        ("src/marketing/home.tsx", "unusedHomeHelper"),
        ("src/account/layout.tsx", "unusedLayoutHelper"),
        ("src/account/login.tsx", "unusedLoginHelper"),
    ] {
        assert!(
            has_unused_export(&unused_exports, path, export),
            "{path}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn react_router_flat_routes_custom_root_is_framework_used() {
    let root = fixture_path("react-router-flat-routes-root");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "app/file-routes/home.tsx"),
        "custom flat-routes root should keep route modules alive, unused files: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("app/routes.ts", "default"),
        ("app/file-routes/home.tsx", "loader"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be treated as framework-used, found: {unused_exports:?}"
        );
    }
    for (path, export) in [
        ("app/routes.ts", "unusedRouteConfigHelper"),
        ("app/file-routes/home.tsx", "unusedHomeHelper"),
    ] {
        assert!(
            has_unused_export(&unused_exports, path, export),
            "{path}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}
