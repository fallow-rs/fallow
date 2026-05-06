//! Remix framework plugin.
//!
//! Detects Remix projects and marks route files, root layout, and entry points.
//! Recognizes conventional route exports (loader, action, meta, etc.).

use super::Plugin;

const ENABLERS: &[&str] = &[
    "@remix-run/node",
    "@remix-run/react",
    "@remix-run/cloudflare",
    "@remix-run/cloudflare-pages",
    "@remix-run/deno",
];

const ENTRY_PATTERNS: &[&str] = &[
    "app/routes/**/*.{ts,tsx,js,jsx}",
    "app/root.{ts,tsx,js,jsx}",
    "app/entry.client.{ts,tsx,js,jsx}",
    "app/entry.server.{ts,tsx,js,jsx}",
];

const ALWAYS_USED: &[&str] = &["remix.config.{ts,js,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "@remix-run/dev",
    "@remix-run/node",
    "@remix-run/react",
    "@remix-run/cloudflare",
    "@remix-run/serve",
];

const BUNDLE_BOUNDARY_DIRS: &[&str] = &[".client", ".server"];

macro_rules! route_module_exports {
    ($($export:literal),+ $(,)?) => {
        const ROUTE_EXPORTS: &[&str] = &[$($export),+];
        const ROOT_EXPORTS: &[&str] = &[$($export,)+ "Layout"];
    };
}

route_module_exports!(
    "default",
    "loader",
    "clientLoader",
    "action",
    "clientAction",
    "meta",
    "links",
    "headers",
    "handle",
    "ErrorBoundary",
    "HydrateFallback",
    "shouldRevalidate",
);

define_plugin! {
    struct RemixPlugin => "remix",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    discovery_hidden_dirs: BUNDLE_BOUNDARY_DIRS,
    used_exports: [
        ("app/routes/**/*.{ts,tsx,js,jsx}", ROUTE_EXPORTS),
        ("app/root.{ts,tsx,js,jsx}", ROOT_EXPORTS),
    ],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_exports_cover_root_client_data_and_layout() {
        let plugin = RemixPlugin;
        let exports = plugin.used_exports();

        assert!(exports.iter().any(|(pattern, names)| {
            pattern == &"app/root.{ts,tsx,js,jsx}"
                && names.contains(&"Layout")
                && names.contains(&"clientLoader")
                && names.contains(&"clientAction")
        }));
        assert!(exports.iter().any(|(pattern, names)| {
            pattern == &"app/routes/**/*.{ts,tsx,js,jsx}"
                && names.contains(&"shouldRevalidate")
                && names.contains(&"clientLoader")
                && names.contains(&"clientAction")
        }));
    }

    #[test]
    fn discovery_hidden_dirs_include_bundle_boundaries() {
        let plugin = RemixPlugin;
        assert_eq!(plugin.discovery_hidden_dirs(), [".client", ".server"]);
    }
}
