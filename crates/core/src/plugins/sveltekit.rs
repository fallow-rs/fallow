//! `SvelteKit` framework plugin.
//!
//! Detects `SvelteKit` projects and marks route files, hooks, and convention files
//! as entry points. Parses svelte.config.js to extract adapter dependencies.

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

pub struct SvelteKitPlugin;

const ENABLERS: &[&str] = &["@sveltejs/kit"];

const ENTRY_PATTERNS: &[&str] = &[
    // Route files (split svelte/ts for correct used_exports matching)
    "src/routes/**/+page.svelte",
    "src/routes/**/+page.{ts,js}",
    "src/routes/**/+page.server.{ts,js}",
    "src/routes/**/+layout.svelte",
    "src/routes/**/+layout.{ts,js}",
    "src/routes/**/+layout.server.{ts,js}",
    "src/routes/**/+server.{ts,js}",
    "src/routes/**/+error.svelte",
    // Layout-reset variants (`+page@.svelte`, `+page@named.svelte`, etc.): the `@`
    // suffix breaks a route out of its parent layout chain. Only the component files
    // (`.svelte`) take this suffix; the co-located load files keep their plain names.
    // `*` matches both the empty (`@.svelte`) and named (`@named.svelte`) forms.
    "src/routes/**/+page@*.svelte",
    "src/routes/**/+layout@*.svelte",
    // Hooks
    "src/hooks.server.{ts,js}",
    "src/hooks.client.{ts,js}",
    "src/hooks.{ts,js}",
    // Service worker
    "src/service-worker.{ts,js}",
    // Params matchers
    "src/params/**/*.{ts,js}",
    // Remote functions (SvelteKit 2.27+): invoked through generated client/server
    // bindings, so the file is never reached through fallow's import graph.
    "src/**/*.remote.{ts,js}",
];

const CONFIG_PATTERNS: &[&str] = &["svelte.config.{js,cjs,mjs,ts}"];

const ALWAYS_USED: &[&str] = &[
    "svelte.config.{js,cjs,mjs,ts}",
    "src/app.html",
    "src/app.d.ts",
    "src/app.{css,scss,less}",
];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "svelte",
    "@sveltejs/kit",
    "@sveltejs/adapter-auto",
    "@sveltejs/adapter-node",
    "@sveltejs/adapter-static",
    "@sveltejs/adapter-vercel",
    "@sveltejs/adapter-netlify",
    "@sveltejs/adapter-cloudflare",
    "@sveltejs/vite-plugin-svelte",
    "svelte-check",
    "svelte-preprocess",
];

/// Virtual module prefixes provided by `SvelteKit` at build time.
/// `$app/` provides runtime modules (environment, forms, navigation, paths, server, state).
/// `$env/` provides environment variable access (static/dynamic, public/private).
/// `$lib/` is an alias for the `src/lib` directory.
/// `$service-worker` provides service worker build info.
const VIRTUAL_MODULE_PREFIXES: &[&str] = &["$app/", "$env/", "$lib/", "$service-worker"];

/// Import suffixes for build-time generated relative imports.
/// SvelteKit generates `./$types` (and `./$types.js`/`./$types.ts`) in route files
/// containing type definitions for `PageLoad`, `PageData`, etc.
const GENERATED_IMPORT_PATTERNS: &[&str] = &["/$types"];

// SvelteKit route convention exports
const PAGE_EXPORTS: &[&str] = &["default"];
const PAGE_LOAD_EXPORTS: &[&str] = &[
    "load",
    "prerender",
    "csr",
    "ssr",
    "trailingSlash",
    "entries",
];
const PAGE_SERVER_EXPORTS: &[&str] = &[
    "load",
    "prerender",
    "csr",
    "ssr",
    "trailingSlash",
    "entries",
    "actions",
];
const LAYOUT_EXPORTS: &[&str] = &["default"];
const LAYOUT_LOAD_EXPORTS: &[&str] = &["load", "prerender", "csr", "ssr", "trailingSlash"];
const SERVER_EXPORTS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "fallback",
];
const HOOKS_SERVER_EXPORTS: &[&str] = &["handle", "handleError", "handleFetch", "init"];
const HOOKS_CLIENT_EXPORTS: &[&str] = &["handleError", "init"];
const HOOKS_SHARED_EXPORTS: &[&str] = &["reroute", "transport", "handleError", "init"];
const PARAM_MATCHER_EXPORTS: &[&str] = &["match"];
// Remote-function exports (`export const getPosts = query(...)`) carry
// user-defined names rather than a fixed convention set, so every export is
// credited via the `"*"` wildcard. SvelteKit consumes them through generated
// bindings, keeping them exempt even under `--include-entry-exports`.
const REMOTE_EXPORTS: &[&str] = &["*"];

impl Plugin for SvelteKitPlugin {
    fn name(&self) -> &'static str {
        "sveltekit"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn virtual_module_prefixes(&self) -> &'static [&'static str] {
        VIRTUAL_MODULE_PREFIXES
    }

    fn generated_import_patterns(&self) -> &'static [&'static str] {
        GENERATED_IMPORT_PATTERNS
    }

    fn path_aliases(&self, _root: &Path) -> Vec<(&'static str, String)> {
        // $lib/ is SvelteKit's built-in alias for src/lib/
        vec![
            ("$lib/", "src/lib".to_string()),
            ("$lib", "src/lib".to_string()),
        ]
    }

    fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
        vec![
            ("src/routes/**/+page.svelte", PAGE_EXPORTS),
            ("src/routes/**/+page@*.svelte", PAGE_EXPORTS),
            ("src/routes/**/+page.{ts,js}", PAGE_LOAD_EXPORTS),
            ("src/routes/**/+page.server.{ts,js}", PAGE_SERVER_EXPORTS),
            ("src/routes/**/+layout.svelte", LAYOUT_EXPORTS),
            ("src/routes/**/+layout@*.svelte", LAYOUT_EXPORTS),
            ("src/routes/**/+layout.{ts,js}", LAYOUT_LOAD_EXPORTS),
            ("src/routes/**/+layout.server.{ts,js}", LAYOUT_LOAD_EXPORTS),
            ("src/routes/**/+server.{ts,js}", SERVER_EXPORTS),
            ("src/hooks.server.{ts,js}", HOOKS_SERVER_EXPORTS),
            ("src/hooks.client.{ts,js}", HOOKS_CLIENT_EXPORTS),
            ("src/hooks.{ts,js}", HOOKS_SHARED_EXPORTS),
            ("src/params/**/*.{ts,js}", PARAM_MATCHER_EXPORTS),
            ("src/**/*.remote.{ts,js}", REMOTE_EXPORTS),
        ]
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();

        // Extract import sources as referenced dependencies
        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        for (find, replacement) in
            config_parser::extract_config_aliases(source, config_path, &["kit", "alias"])
        {
            if let Some(normalized) =
                config_parser::normalize_config_path(&replacement, config_path, root)
            {
                result.path_aliases.push((find, normalized));
            }
        }

        // Extract require() calls (CJS configs)
        let require_deps =
            config_parser::extract_config_require_strings(source, config_path, "adapter");
        for dep in &require_deps {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        // Extract preprocess plugins
        let preprocess_deps =
            config_parser::extract_config_require_strings(source, config_path, "preprocess");
        for dep in &preprocess_deps {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_adapter_import() {
        let source = r"
            import adapter from '@sveltejs/adapter-node';
            export default { kit: { adapter: adapter() } };
        ";
        let plugin = SvelteKitPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("svelte.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@sveltejs/adapter-node".to_string())
        );
    }

    #[test]
    fn resolve_config_preprocess_import() {
        let source = r"
            import adapter from '@sveltejs/adapter-auto';
            import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
            export default { preprocess: vitePreprocess(), kit: { adapter: adapter() } };
        ";
        let plugin = SvelteKitPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("svelte.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@sveltejs/adapter-auto".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@sveltejs/vite-plugin-svelte".to_string())
        );
    }

    #[test]
    fn virtual_module_prefixes_includes_sveltekit_builtins() {
        let plugin = SvelteKitPlugin;
        let prefixes = plugin.virtual_module_prefixes();
        assert!(prefixes.contains(&"$app/"));
        assert!(prefixes.contains(&"$env/"));
        assert!(prefixes.contains(&"$lib/"));
        assert!(prefixes.contains(&"$service-worker"));
    }

    #[test]
    fn generated_import_patterns_includes_types() {
        let plugin = SvelteKitPlugin;
        let patterns = plugin.generated_import_patterns();
        assert!(
            patterns.contains(&"/$types"),
            "should include /$types for SvelteKit generated route types"
        );
    }

    #[test]
    fn used_exports_include_param_matchers() {
        let plugin = SvelteKitPlugin;
        let exports = plugin.used_exports();
        let matcher_entry = exports
            .iter()
            .find(|(pat, _)| *pat == "src/params/**/*.{ts,js}")
            .expect("param matcher used exports");
        assert!(matcher_entry.1.contains(&"match"));
    }

    #[test]
    fn entry_patterns_include_remote_functions() {
        let plugin = SvelteKitPlugin;
        let patterns = plugin.entry_patterns();
        assert!(
            patterns.contains(&"src/**/*.remote.{ts,js}"),
            "SvelteKit remote-function files should be entry points: {patterns:?}"
        );
    }

    #[test]
    fn used_exports_credit_all_remote_exports() {
        let plugin = SvelteKitPlugin;
        let exports = plugin.used_exports();
        let remote_entry = exports
            .iter()
            .find(|(pat, _)| *pat == "src/**/*.remote.{ts,js}")
            .expect("remote used_exports rule");
        assert!(
            remote_entry.1.contains(&"*"),
            "remote functions carry user-defined names, so all exports must be credited: {:?}",
            remote_entry.1
        );
    }

    #[test]
    fn entry_patterns_include_layout_reset_variants() {
        let plugin = SvelteKitPlugin;
        let patterns = plugin.entry_patterns();
        assert!(
            patterns.contains(&"src/routes/**/+page@*.svelte"),
            "layout-reset page files should be entry points: {patterns:?}"
        );
        assert!(
            patterns.contains(&"src/routes/**/+layout@*.svelte"),
            "layout-reset layout files should be entry points: {patterns:?}"
        );
    }

    #[test]
    fn layout_reset_glob_matches_empty_and_named_forms() {
        let glob = globset::GlobBuilder::new("src/routes/**/+page@*.svelte")
            .literal_separator(true)
            .build()
            .expect("valid glob")
            .compile_matcher();
        assert!(
            glob.is_match("src/routes/marketing/+page@.svelte"),
            "empty layout-reset form (`+page@.svelte`) must match"
        );
        assert!(
            glob.is_match("src/routes/marketing/+page@named.svelte"),
            "named layout-reset form (`+page@named.svelte`) must match"
        );
        assert!(
            !glob.is_match("src/routes/marketing/+page.svelte"),
            "non-reset form should not match the `@` glob"
        );
    }

    #[test]
    fn used_exports_credit_layout_reset_default_export() {
        let plugin = SvelteKitPlugin;
        let exports = plugin.used_exports();
        let page_reset = exports
            .iter()
            .find(|(pat, _)| *pat == "src/routes/**/+page@*.svelte")
            .expect("layout-reset page used_exports rule");
        assert!(page_reset.1.contains(&"default"));
        let layout_reset = exports
            .iter()
            .find(|(pat, _)| *pat == "src/routes/**/+layout@*.svelte")
            .expect("layout-reset layout used_exports rule");
        assert!(layout_reset.1.contains(&"default"));
    }

    #[test]
    fn resolve_config_extracts_aliases() {
        let source = r#"
            export default {
                kit: {
                    alias: {
                        $utils: "./src/lib/utils"
                    }
                }
            };
        "#;
        let plugin = SvelteKitPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/svelte.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(
            result.path_aliases,
            vec![("$utils".to_string(), "src/lib/utils".to_string())]
        );
    }
}
