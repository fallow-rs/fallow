//! Content Collections plugin.
//!
//! Detects Content Collections projects and marks the root config as used.

use super::Plugin;

/// Detect Content Collections projects via any of the canonical packages
/// users actually list in `dependencies` / `devDependencies`. The framework
/// integrations (`vite`, `next`, `solid-start`, `remix-vite`, `qwik`,
/// `vinxi`) are what most authors install at the top level; `core` arrives
/// transitively in those setups, so an enabler restricted to `core` would
/// miss real-world projects that only declare the integration package.
const ENABLERS: &[&str] = &[
    "@content-collections/core",
    "@content-collections/vite",
    "@content-collections/next",
    "@content-collections/solid-start",
    "@content-collections/remix-vite",
    "@content-collections/qwik",
    "@content-collections/vinxi",
];

const ENTRY_PATTERNS: &[&str] = &["content-collections.{ts,tsx,js,jsx,mts,mjs,cts,cjs}"];

/// The same root config, listed as the plugin's own config file, so an
/// existing one credits the tooling dependencies below.
const ALWAYS_USED: &[&str] = &["content-collections.{ts,tsx,js,jsx,mts,mjs,cts,cjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "@content-collections/core",
    "@content-collections/vite",
    "@content-collections/next",
    "@content-collections/solid-start",
    "@content-collections/remix-vite",
    "@content-collections/qwik",
    "@content-collections/vinxi",
    "@content-collections/markdown",
    "@content-collections/mdx",
];

define_plugin! {
    struct ContentCollectionsPlugin => "content-collections",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protects_root_config_and_tooling_packages() {
        let plugin = ContentCollectionsPlugin;

        assert!(
            plugin
                .entry_patterns()
                .contains(&"content-collections.{ts,tsx,js,jsx,mts,mjs,cts,cjs}"),
            "entry pattern must accept every JS/TS extension content-collections honors at runtime"
        );
        assert!(
            plugin
                .tooling_dependencies()
                .contains(&"@content-collections/vite")
        );
    }

    #[test]
    fn framework_integrations_activate_the_plugin() {
        let plugin = ContentCollectionsPlugin;

        for framework_pkg in [
            "@content-collections/vite",
            "@content-collections/next",
            "@content-collections/solid-start",
            "@content-collections/remix-vite",
            "@content-collections/qwik",
            "@content-collections/vinxi",
        ] {
            assert!(
                plugin.enablers().contains(&framework_pkg),
                "{framework_pkg} should activate the plugin without requiring @content-collections/core to be a direct dep"
            );
        }
    }
}
