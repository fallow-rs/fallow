use std::{fs, path::Path};

use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::{
    collect_unused_exports, collect_unused_files, has_unused_export,
};
use tempfile::tempdir;

fn write_project_file(root: &Path, relative_path: &str, source: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    fs::write(path, source).expect("write test file");
}

#[test]
fn expo_router_special_files_and_exports_are_covered() {
    let root = fixture_path("expo-router-conventions");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    assert!(
        !unused_files.iter().any(|path| path == "src/app/index.tsx"),
        "configured route root should be treated as entry points, unused files: {unused_files:?}"
    );
    assert!(
        unused_files.iter().any(|path| path == "app/legacy.tsx"),
        "default app/ directory should not stay alive when expo-router root is src/app: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("src/app/_layout.tsx", "default"),
        ("src/app/_layout.tsx", "ErrorBoundary"),
        ("src/app/_layout.tsx", "unstable_settings"),
        ("src/app/index.tsx", "default"),
        ("src/app/index.tsx", "ErrorBoundary"),
        ("src/app/index.tsx", "loader"),
        ("src/app/index.tsx", "generateStaticParams"),
        ("src/app/+html.tsx", "default"),
        ("src/app/+not-found.tsx", "default"),
        ("src/app/+native-intent.tsx", "redirectSystemPath"),
        ("src/app/+native-intent.tsx", "legacy_subscribe"),
        ("src/app/+middleware.ts", "default"),
        ("src/app/+middleware.ts", "unstable_settings"),
        ("src/app/hello+api.ts", "GET"),
        ("src/app/hello+api.ts", "POST"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be framework-used, found: {unused_exports:?}"
        );
    }

    for (path, export) in [
        ("src/app/_layout.tsx", "unusedLayoutHelper"),
        ("src/app/index.tsx", "unusedIndexHelper"),
        ("src/app/+html.tsx", "unusedHtmlHelper"),
        ("src/app/+not-found.tsx", "unusedNotFoundHelper"),
        ("src/app/+native-intent.tsx", "unusedIntentHelper"),
        ("src/app/+middleware.ts", "unusedMiddlewareHelper"),
        ("src/app/hello+api.ts", "unusedApiHelper"),
    ] {
        assert!(
            has_unused_export(&unused_exports, path, export),
            "{path}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn tanstack_router_custom_route_dir_and_lazy_exports_are_covered() {
    let root = fixture_path("tanstack-router-conventions");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "app/pages/index.tsx"),
        "custom route dir should be reachable through generated route tree, unused files: {unused_files:?}"
    );
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "src/routeTree.gen.ts"),
        "custom route dir should not relocate the default generated route tree path, unused files: {unused_files:?}"
    );
    assert!(
        !unused_files.iter().any(|path| path == "src/router.ts"),
        "custom route dir should not relocate the default router entry path, unused files: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path == "src/routes/legacy.tsx"),
        "default src/routes should not stay alive when tsr.config.json points elsewhere: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("app/pages/__root.tsx", "Route"),
        ("app/pages/index.tsx", "Route"),
        ("app/pages/index.tsx", "loader"),
        ("app/pages/index.tsx", "beforeLoad"),
        ("app/pages/posts.lazy.tsx", "Route"),
        ("app/pages/posts.lazy.tsx", "component"),
        ("app/pages/posts.lazy.tsx", "pendingComponent"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be framework-used, found: {unused_exports:?}"
        );
    }

    for (path, export) in [
        ("app/pages/__root.tsx", "unusedRootHelper"),
        ("app/pages/index.tsx", "unusedIndexHelper"),
        ("app/pages/posts.lazy.tsx", "unusedLazyHelper"),
    ] {
        assert!(
            has_unused_export(&unused_exports, path, export),
            "{path}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn tanstack_router_prefix_and_ignore_patterns_stay_strict() {
    let root = fixture_path("tanstack-router-prefix-and-ignore");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    for path in ["src/routes/helper.tsx", "src/routes/ignored.page.tsx"] {
        assert!(
            unused_files.iter().any(|unused| unused == path),
            "{path} should not be treated as a live route file, unused files: {unused_files:?}"
        );
    }
    for path in [
        "src/routes/route-home.tsx",
        "src/routes/route-posts.lazy.tsx",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == path),
            "{path} should stay reachable as a configured route file, unused files: {unused_files:?}"
        );
    }

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("src/routes/route-home.tsx", "Route"),
        ("src/routes/route-posts.lazy.tsx", "Route"),
        ("src/routes/route-posts.lazy.tsx", "component"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be framework-used, found: {unused_exports:?}"
        );
    }
    assert!(
        has_unused_export(&unused_exports, "src/routes/route-posts.lazy.tsx", "loader"),
        "lazy routes should not inherit non-lazy exports, found: {unused_exports:?}"
    );
}

#[test]
fn tanstack_router_generated_route_tree_import_without_file_is_not_unresolved() {
    let root = fixture_path("tanstack-router-generated-route-tree-import");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();

    assert!(
        !unresolved_specifiers.contains(&"./routeTree.gen"),
        "TanStack Router generated route tree imports should not be unresolved, found: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"./missing-control"),
        "ordinary missing relative imports should still be unresolved, found: {unresolved_specifiers:?}"
    );
}

#[test]
fn route_tree_generated_import_stays_unresolved_without_tanstack_router_plugin() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();

    write_project_file(
        root,
        "package.json",
        r#"{
  "name": "route-tree-without-tanstack",
  "main": "src/router.ts"
}"#,
    );
    write_project_file(root, "src/router.ts", "import './routeTree.gen';\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();

    assert!(
        unresolved_specifiers.contains(&"./routeTree.gen"),
        "routeTree.gen should only be suppressed by the active TanStack Router plugin, found: {unresolved_specifiers:?}"
    );
}

#[test]
fn tanstack_start_virtual_modules_are_not_unlisted() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();

    write_project_file(
        root,
        "package.json",
        r#"{
  "name": "tanstack-start-virtual-modules",
  "main": "src/router-manifest.ts",
  "dependencies": {
    "@tanstack/react-start": "1.0.0"
  }
}"#,
    );
    write_project_file(
        root,
        "src/router-manifest.ts",
        r#"import manifestModule from "tanstack-start-manifest:v";

export async function loadManifest() {
  const { tsrStartManifest } = await import("tanstack-start-manifest:v");
  const mod = await import("tanstack-start-injected-head-scripts:v");
  return [manifestModule, tsrStartManifest, mod];
}
"#,
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();

    for specifier in [
        "tanstack-start-manifest:v",
        "tanstack-start-injected-head-scripts:v",
    ] {
        assert!(
            !unlisted_names.contains(&specifier),
            "{specifier} should be treated as a TanStack Start virtual module, unlisted dependencies: {unlisted_names:?}"
        );
        assert!(
            !unresolved_specifiers.contains(&specifier),
            "{specifier} should be treated as a TanStack Start virtual module, unresolved imports: {unresolved_specifiers:?}"
        );
    }
}

#[test]
fn tanstack_start_virtual_modules_stay_unlisted_without_plugin() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();

    write_project_file(
        root,
        "package.json",
        r#"{
  "name": "tanstack-start-virtual-modules-without-plugin",
  "main": "src/router-manifest.ts"
}"#,
    );
    write_project_file(
        root,
        "src/router-manifest.ts",
        r#"export async function loadManifest() {
  const { tsrStartManifest } = await import("tanstack-start-manifest:v");
  const mod = await import("tanstack-start-injected-head-scripts:v");
  return [tsrStartManifest, mod];
}
"#,
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    for specifier in [
        "tanstack-start-manifest:v",
        "tanstack-start-injected-head-scripts:v",
    ] {
        assert!(
            unlisted_names.contains(&specifier),
            "{specifier} should only be suppressed by the active TanStack plugin, unlisted dependencies: {unlisted_names:?}"
        );
    }
}

#[test]
fn tanstack_router_inline_virtual_route_config_is_covered() {
    let root = fixture_path("tanstack-router-virtual-routes");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    for path in [
        "src/routeTree.gen.ts",
        "src/virtual-routes/root.tsx",
        "src/virtual-routes/home.tsx",
        "src/virtual-routes/admin/dashboard.tsx",
        "src/virtual-routes/layouts/shell.tsx",
        "src/virtual-routes/settings.tsx",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == path),
            "{path} should be reachable through inline virtualRouteConfig, unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_files
            .iter()
            .any(|unused| unused == "src/virtual-routes/orphan.tsx"),
        "virtualRouteConfig should not keep unlisted route files alive, unused files: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("src/virtual-routes/root.tsx", "Route"),
        ("src/virtual-routes/home.tsx", "Route"),
        ("src/virtual-routes/home.tsx", "loader"),
        ("src/virtual-routes/admin/dashboard.tsx", "ServerRoute"),
        ("src/virtual-routes/layouts/shell.tsx", "beforeLoad"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be framework-used through virtualRouteConfig, found: {unused_exports:?}"
        );
    }
    assert!(
        has_unused_export(
            &unused_exports,
            "src/virtual-routes/home.tsx",
            "unusedHomeHelper"
        ),
        "ordinary helpers in virtual route files should still be reported, found: {unused_exports:?}"
    );
}

#[test]
fn tanstack_router_virtual_route_config_file_is_covered() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();

    write_project_file(
        root,
        "package.json",
        r#"{
  "dependencies": {
    "@tanstack/react-start": "1.0.0",
    "@tanstack/virtual-file-routes": "1.0.0"
  }
}"#,
    );
    write_project_file(
        root,
        "tsr.config.json",
        r#"{
  "virtualRouteConfig": "./routes.ts",
  "generatedRouteTree": "./routeTree.gen.ts"
}"#,
    );
    write_project_file(
        root,
        "routes.ts",
        r#"import { index, layout, physical, rootRoute, route } from "@tanstack/virtual-file-routes";

export const routes = rootRoute("root.tsx", [
  index("home.tsx"),
  route("/admin", "admin/dashboard.tsx"),
  layout("shell", "layouts/shell.tsx", [
    route("/settings", "settings.tsx")
  ]),
  physical("physical")
]);
"#,
    );
    write_project_file(root, "routeTree.gen.ts", "export const routeTree = {};\n");
    write_project_file(root, "root.tsx", "export const Route = {};\n");
    write_project_file(root, "home.tsx", "export const Route = {};\n");
    write_project_file(
        root,
        "admin/dashboard.tsx",
        "export const ServerRoute = {};\nexport const unusedDashboardHelper = 1;\n",
    );
    write_project_file(
        root,
        "layouts/shell.tsx",
        "export function beforeLoad() {}\n",
    );
    write_project_file(root, "settings.tsx", "export const Route = {};\n");
    write_project_file(root, "physical/index.tsx", "export const Route = {};\n");
    write_project_file(root, "physical/-helper.tsx", "export const Route = {};\n");
    write_project_file(root, "src/routes/orphan.tsx", "export const Route = {};\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = collect_unused_files(root, &results);
    for path in [
        "routes.ts",
        "routeTree.gen.ts",
        "root.tsx",
        "home.tsx",
        "admin/dashboard.tsx",
        "layouts/shell.tsx",
        "settings.tsx",
        "physical/index.tsx",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == path),
            "{path} should be reachable through virtual route config file, unused files: {unused_files:?}"
        );
    }
    for path in ["physical/-helper.tsx", "src/routes/orphan.tsx"] {
        assert!(
            unused_files.iter().any(|unused| unused == path),
            "{path} should not be treated as a configured virtual route, unused files: {unused_files:?}"
        );
    }

    let unused_exports = collect_unused_exports(root, &results);
    assert!(
        !has_unused_export(&unused_exports, "admin/dashboard.tsx", "ServerRoute"),
        "Start ServerRoute export should be framework-used, found: {unused_exports:?}"
    );
    assert!(
        has_unused_export(
            &unused_exports,
            "admin/dashboard.tsx",
            "unusedDashboardHelper"
        ),
        "non-framework exports should still be reported, found: {unused_exports:?}"
    );
}

#[test]
fn tanstack_router_vite_plugin_inline_virtual_routes_are_covered() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();

    write_project_file(
        root,
        "package.json",
        r#"{
  "dependencies": {
    "@tanstack/react-start": "1.0.0",
    "@tanstack/router-plugin": "1.0.0",
    "@tanstack/virtual-file-routes": "1.0.0",
    "vite": "1.0.0"
  }
}"#,
    );
    write_project_file(
        root,
        "vite.config.ts",
        r#"import { defineConfig } from "vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import { index, layout, physical, rootRoute, route } from "@tanstack/virtual-file-routes";

const routes = rootRoute("root.tsx", [
  index("home.tsx"),
  route("/admin", "admin/dashboard.tsx"),
  layout("shell", "layouts/shell.tsx", [
    route("/settings", "settings.tsx")
  ]),
  physical("physical")
]);

export default defineConfig({
  plugins: [
    tanstackRouter({
      target: "react",
      routesDirectory: "./src/virtual-routes",
      generatedRouteTree: "./src/routeTree.gen.ts",
      virtualRouteConfig: routes
    })
  ]
});
"#,
    );
    write_project_file(
        root,
        "src/routeTree.gen.ts",
        "export const routeTree = {};\n",
    );
    write_project_file(
        root,
        "src/virtual-routes/root.tsx",
        "export const Route = {};\n",
    );
    write_project_file(
        root,
        "src/virtual-routes/home.tsx",
        "export const Route = {};\nexport function loader() {}\nexport const unusedHomeHelper = 1;\n",
    );
    write_project_file(
        root,
        "src/virtual-routes/admin/dashboard.tsx",
        "export const ServerRoute = {};\n",
    );
    write_project_file(
        root,
        "src/virtual-routes/layouts/shell.tsx",
        "export function beforeLoad() {}\n",
    );
    write_project_file(
        root,
        "src/virtual-routes/settings.tsx",
        "export const Route = {};\n",
    );
    write_project_file(
        root,
        "src/virtual-routes/physical/index.tsx",
        "export const Route = {};\n",
    );
    write_project_file(
        root,
        "src/virtual-routes/physical/-helper.tsx",
        "export const Route = {};\n",
    );
    write_project_file(
        root,
        "src/virtual-routes/orphan.tsx",
        "export const Route = {};\n",
    );
    write_project_file(root, "src/routes/legacy.tsx", "export const Route = {};\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = collect_unused_files(root, &results);
    for path in [
        "src/routeTree.gen.ts",
        "src/virtual-routes/root.tsx",
        "src/virtual-routes/home.tsx",
        "src/virtual-routes/admin/dashboard.tsx",
        "src/virtual-routes/layouts/shell.tsx",
        "src/virtual-routes/settings.tsx",
        "src/virtual-routes/physical/index.tsx",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == path),
            "{path} should be reachable through vite tanstackRouter virtualRouteConfig, unused files: {unused_files:?}"
        );
    }
    for path in [
        "src/virtual-routes/physical/-helper.tsx",
        "src/virtual-routes/orphan.tsx",
        "src/routes/legacy.tsx",
    ] {
        assert!(
            unused_files.iter().any(|unused| unused == path),
            "{path} should not be treated as a configured virtual route, unused files: {unused_files:?}"
        );
    }

    let unused_exports = collect_unused_exports(root, &results);
    for (path, export) in [
        ("src/virtual-routes/home.tsx", "loader"),
        ("src/virtual-routes/admin/dashboard.tsx", "ServerRoute"),
        ("src/virtual-routes/layouts/shell.tsx", "beforeLoad"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "{path}:{export} should be framework-used through vite tanstackRouter config, found: {unused_exports:?}"
        );
    }
    assert!(
        has_unused_export(
            &unused_exports,
            "src/virtual-routes/home.tsx",
            "unusedHomeHelper"
        ),
        "ordinary helpers in virtual route files should still be reported, found: {unused_exports:?}"
    );
}

#[test]
fn tanstack_router_webpack_plugin_virtual_route_file_is_covered() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();

    write_project_file(
        root,
        "package.json",
        r#"{
  "dependencies": {
    "@tanstack/react-router": "1.0.0",
    "@tanstack/router-plugin": "1.0.0",
    "@tanstack/virtual-file-routes": "1.0.0",
    "webpack": "1.0.0"
  }
}"#,
    );
    write_project_file(
        root,
        "webpack.config.ts",
        r#"import { tanstackRouter } from "@tanstack/router-plugin/webpack";

export default {
  plugins: [
    tanstackRouter({
      target: "react",
      routesDirectory: "./app/pages",
      generatedRouteTree: "./app/routeTree.gen.ts",
      virtualRouteConfig: "./routes.ts"
    })
  ]
};
"#,
    );
    write_project_file(
        root,
        "routes.ts",
        r#"import { index, rootRoute, route } from "@tanstack/virtual-file-routes";

export const routes = rootRoute("root.tsx", [
  index("home.tsx"),
  route("/admin", "admin/dashboard.tsx")
]);
"#,
    );
    write_project_file(
        root,
        "app/routeTree.gen.ts",
        "export const routeTree = {};\n",
    );
    write_project_file(root, "root.tsx", "export const Route = {};\n");
    write_project_file(root, "home.tsx", "export const Route = {};\n");
    write_project_file(
        root,
        "admin/dashboard.tsx",
        "export const ServerRoute = {};\nexport const unusedDashboardHelper = 1;\n",
    );
    write_project_file(root, "app/pages/legacy.tsx", "export const Route = {};\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = collect_unused_files(root, &results);
    for path in [
        "webpack.config.ts",
        "routes.ts",
        "app/routeTree.gen.ts",
        "root.tsx",
        "home.tsx",
        "admin/dashboard.tsx",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == path),
            "{path} should be reachable through webpack tanstackRouter virtualRouteConfig, unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_files
            .iter()
            .any(|unused| unused == "app/pages/legacy.tsx"),
        "virtualRouteConfig should replace the default route directory walk, unused files: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(root, &results);
    assert!(
        !has_unused_export(&unused_exports, "admin/dashboard.tsx", "ServerRoute"),
        "ServerRoute export should be framework-used through webpack tanstackRouter config, found: {unused_exports:?}"
    );
    assert!(
        has_unused_export(
            &unused_exports,
            "admin/dashboard.tsx",
            "unusedDashboardHelper"
        ),
        "non-framework exports should still be reported, found: {unused_exports:?}"
    );
}

#[test]
fn tanstack_router_plain_vite_config_does_not_shadow_tsr_config() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();

    write_project_file(
        root,
        "package.json",
        r#"{
  "dependencies": {
    "@tanstack/react-router": "1.0.0",
    "vite": "1.0.0"
  }
}"#,
    );
    write_project_file(
        root,
        "vite.config.ts",
        r#"import { defineConfig } from "vite";

export default defineConfig({});
"#,
    );
    write_project_file(
        root,
        "tsr.config.json",
        r#"{
  "routesDirectory": "./app/pages"
}"#,
    );
    write_project_file(root, "app/pages/index.tsx", "export const Route = {};\n");
    write_project_file(root, "src/routes/legacy.tsx", "export const Route = {};\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = collect_unused_files(root, &results);
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "app/pages/index.tsx"),
        "tsr.config.json should keep custom route directory live even when a plain vite config is present, unused files: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path == "src/routes/legacy.tsx"),
        "default src/routes should not stay alive after tsr.config.json moves routesDirectory, unused files: {unused_files:?}"
    );
}

#[test]
fn tanstack_router_webpack_cjs_config_is_covered() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();

    write_project_file(
        root,
        "package.json",
        r#"{
  "dependencies": {
    "@tanstack/react-router": "1.0.0",
    "@tanstack/router-plugin": "1.0.0",
    "webpack": "1.0.0"
  }
}"#,
    );
    write_project_file(
        root,
        "webpack.config.cjs",
        r#"const { tanstackRouter } = require("@tanstack/router-plugin/webpack");

module.exports = {
  plugins: [
    tanstackRouter({
      target: "react",
      routesDirectory: "./app/pages"
    })
  ]
};
"#,
    );
    write_project_file(root, "app/pages/index.tsx", "export const Route = {};\n");
    write_project_file(root, "src/routes/legacy.tsx", "export const Route = {};\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = collect_unused_files(root, &results);
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "app/pages/index.tsx"),
        "CommonJS webpack tanstackRouter config should keep custom route directory live, unused files: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path == "src/routes/legacy.tsx"),
        "default src/routes should not stay alive after webpack config moves routesDirectory, unused files: {unused_files:?}"
    );
}

#[test]
fn tanstack_router_custom_route_dir_replaces_default_used_export_rules() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();

    write_project_file(
        root,
        "package.json",
        r#"{
  "dependencies": {
    "@tanstack/react-router": "1.0.0"
  }
}"#,
    );
    write_project_file(
        root,
        "tsr.config.json",
        r#"{
  "routesDirectory": "./app/pages"
}"#,
    );
    write_project_file(
        root,
        "app/pages/index.tsx",
        "import '../shared';\nexport const Route = {};\n",
    );
    write_project_file(
        root,
        "app/shared.ts",
        "import { helper } from '../src/routes/legacy';\nconsole.log(helper);\n",
    );
    write_project_file(
        root,
        "src/routes/legacy.tsx",
        "export const Route = {};\nexport const helper = 1;\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = collect_unused_files(root, &results);
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "src/routes/legacy.tsx"),
        "helper import should keep the legacy file reachable, unused files: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(root, &results);
    assert!(
        has_unused_export(&unused_exports, "src/routes/legacy.tsx", "Route"),
        "default route-dir exports should not stay framework-used after routesDirectory moves, found: {unused_exports:?}"
    );
    assert!(
        !has_unused_export(&unused_exports, "src/routes/legacy.tsx", "helper"),
        "regular live exports should stay used, found: {unused_exports:?}"
    );
}

#[test]
fn tanstack_router_invalid_ignore_pattern_only_drops_the_bad_filter() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();

    write_project_file(
        root,
        "package.json",
        r#"{
  "dependencies": {
    "@tanstack/react-router": "1.0.0"
  }
}"#,
    );
    write_project_file(
        root,
        "tsr.config.json",
        r#"{
  "routeFileIgnorePattern": "["
}"#,
    );
    write_project_file(root, "src/routes/index.tsx", "export const Route = {};\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = collect_unused_files(root, &results);
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "src/routes/index.tsx"),
        "invalid ignore patterns should not disable route discovery, unused files: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(root, &results);
    assert!(
        !has_unused_export(&unused_exports, "src/routes/index.tsx", "Route"),
        "invalid ignore patterns should not disable framework-used export rules, found: {unused_exports:?}"
    );
}
