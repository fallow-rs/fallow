//! pkg-utils library build-config plugin.
//!
//! `@sanity/pkg-utils` builds npm packages from a `package.config.ts` build
//! config (and an optional `package.bundle.ts` Vite bundle config). Those files
//! are discovered by the tool on a filename convention rather than imported from
//! source, so without this plugin they surface as `unused-file`. Activation is
//! gated strictly on the `@sanity/pkg-utils` dependency (exact match): a plain
//! `@sanity/client` consumer must NOT get its `package.config.ts` auto-credited,
//! which is why this is a dedicated plugin and not an extension of the broad
//! `@sanity/`-enabler CMS `sanity` plugin.
//!
//! Library source entries (`src/_exports/**`) are not seeded here: the
//! `exports.source` condition resolution plus the workspace-package fallback
//! already keep them reachable, so parsing `package.bundle.ts`'s `build.lib.entry`
//! would be dead weight.

use super::Plugin;

const ENABLERS: &[&str] = &["@sanity/pkg-utils"];

// `always_used` matching uses `literal_separator(true)` with no automatic `**/`
// prefix, so both the root and the nested form are listed (mirrors the
// `varlock` / `wuchale` precedent). Covers monorepo `packages/<pkg>/...` configs.
const ALWAYS_USED: &[&str] = &[
    "package.config.{ts,js,mts,mjs,cts,cjs}",
    "**/package.config.{ts,js,mts,mjs,cts,cjs}",
    "package.bundle.{ts,js,mts,mjs,cts,cjs}",
    "**/package.bundle.{ts,js,mts,mjs,cts,cjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["@sanity/pkg-utils"];

define_plugin! {
    struct PkgUtilsPlugin => "pkg-utils",
    enablers: ENABLERS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn enabler_is_exact_pkg_utils_only() {
        let plugin = PkgUtilsPlugin;
        assert_eq!(plugin.enablers(), &["@sanity/pkg-utils"]);
    }

    #[test]
    fn activates_when_pkg_utils_dependency_present() {
        let plugin = PkgUtilsPlugin;
        let deps = vec!["@sanity/pkg-utils".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn does_not_activate_without_pkg_utils_dependency() {
        let plugin = PkgUtilsPlugin;
        let deps = vec!["vite".to_string(), "typescript".to_string()];
        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn does_not_activate_for_plain_sanity_client_consumer() {
        // The exact-match enabler must NOT fire on a sibling `@sanity/*` package;
        // a project that only uses the API client should keep reporting a stray
        // package.config.ts as unused.
        let plugin = PkgUtilsPlugin;
        let deps = vec!["@sanity/client".to_string(), "@sanity/vision".to_string()];
        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn always_used_covers_root_and_nested_build_configs() {
        let plugin = PkgUtilsPlugin;
        let patterns = plugin.always_used();
        for expected in [
            "package.config.{ts,js,mts,mjs,cts,cjs}",
            "**/package.config.{ts,js,mts,mjs,cts,cjs}",
            "package.bundle.{ts,js,mts,mjs,cts,cjs}",
            "**/package.bundle.{ts,js,mts,mjs,cts,cjs}",
        ] {
            assert!(
                patterns.contains(&expected),
                "always_used should include {expected}"
            );
        }
    }

    #[test]
    fn credits_pkg_utils_as_tooling_dependency() {
        let plugin = PkgUtilsPlugin;
        assert!(plugin.tooling_dependencies().contains(&"@sanity/pkg-utils"));
    }
}
