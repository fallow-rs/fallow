//! rspress documentation framework plugin.
//!
//! rspress (Rsbuild / Rspack ecosystem) exposes its theme layer through the
//! `@theme` build-time virtual module, mirroring Docusaurus. Importing
//! `@theme` or a `@theme/<component>` subpath from docs/source resolves at
//! build time and is not an npm package, so the plugin contributes the prefix
//! to suppress false `unlisted-dependency` / `unresolved-import` findings.

use super::Plugin;

const ENABLERS: &[&str] = &["rspress", "@rspress/"];

/// Virtual module prefixes provided by rspress at build time.
/// `@theme/` exposes the active theme's components; `@theme-original/` is the
/// swizzle escape hatch for extending the default theme (same convention as
/// Docusaurus). The trailing slash also covers the bare `@theme` import via the
/// exact-bare branch of `matches_virtual_prefix`.
const VIRTUAL_MODULE_PREFIXES: &[&str] = &["@theme/", "@theme-original/"];

define_plugin! {
    struct RspressPlugin => "rspress",
    enablers: ENABLERS,
    virtual_module_prefixes: VIRTUAL_MODULE_PREFIXES,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::analyze::matches_virtual_prefix;

    #[test]
    fn activates_on_rspress_and_rspress_scope() {
        let plugin = RspressPlugin;
        assert!(plugin.is_enabled_with_deps(&["rspress".to_string()], Path::new("/p")));
        assert!(plugin.is_enabled_with_deps(&["@rspress/core".to_string()], Path::new("/p")));
        assert!(
            plugin.is_enabled_with_deps(&["@rspress/theme-default".to_string()], Path::new("/p"))
        );
    }

    #[test]
    fn inactive_without_rspress() {
        let plugin = RspressPlugin;
        assert!(!plugin.is_enabled_with_deps(&["react".to_string()], Path::new("/p")));
        // A near-miss package that merely starts with `rspress` but is not the
        // scope prefix must not activate the plugin.
        assert!(!plugin.is_enabled_with_deps(&["rspress-plugin-foo".to_string()], Path::new("/p")));
    }

    #[test]
    fn contributes_theme_virtual_prefixes() {
        let prefixes = RspressPlugin.virtual_module_prefixes();
        assert!(prefixes.contains(&"@theme/"));
        assert!(prefixes.contains(&"@theme-original/"));
    }

    #[test]
    fn theme_prefix_covers_bare_and_subpath_imports() {
        let prefixes = RspressPlugin.virtual_module_prefixes();
        // Bare `@theme` (the reported repro) matches the `@theme/` entry via the
        // exact-bare branch; `@theme/Layout` matches as an ordinary prefix.
        assert!(
            prefixes
                .iter()
                .any(|prefix| matches_virtual_prefix(prefix, "@theme"))
        );
        assert!(
            prefixes
                .iter()
                .any(|prefix| matches_virtual_prefix(prefix, "@theme/Layout"))
        );
        assert!(
            prefixes
                .iter()
                .any(|prefix| matches_virtual_prefix(prefix, "@theme-original/Layout"))
        );
        // A real scoped package sharing the `@theme` lexical start must NOT be
        // swallowed.
        assert!(
            !prefixes
                .iter()
                .any(|prefix| matches_virtual_prefix(prefix, "@theme-ui/core"))
        );
    }
}
