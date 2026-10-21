//! `VitePress` plugin.
//!
//! Detects `VitePress` projects and marks config/theme files as always used.
//! The `vite` block of the `VitePress` config is a Vite config: its
//! `optimizeDeps` and `ssr` lists credit packages, and its `resolve.alias`
//! entries resolve imports.

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["vitepress"];

const ENTRY_PATTERNS: &[&str] = &[
    ".vitepress/theme/index.{ts,js,mts,mjs}",
    ".vitepress/theme/**/*.{vue,ts,js,mts,mjs}",
    "docs/.vitepress/theme/index.{ts,js,mts,mjs}",
    "docs/.vitepress/theme/**/*.{vue,ts,js,mts,mjs}",
];

const CONFIG_PATTERNS: &[&str] = &[
    ".vitepress/config.{ts,js,mts,mjs}",
    "docs/.vitepress/config.{ts,js,mts,mjs}",
    ".vitepress/config/index.{ts,js,mts,mjs}",
    "docs/.vitepress/config/index.{ts,js,mts,mjs}",
];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[
    (".vitepress/config.{ts,js,mts,mjs}", &["default"]),
    ("docs/.vitepress/config.{ts,js,mts,mjs}", &["default"]),
    (".vitepress/config/index.{ts,js,mts,mjs}", &["default"]),
    ("docs/.vitepress/config/index.{ts,js,mts,mjs}", &["default"]),
];

const TOOLING_DEPENDENCIES: &[&str] = &["vitepress"];

const THEME_ENTRY_EXPORTS: &[&str] = &["default"];

/// The key of the Vite config object inside a `VitePress` config.
const VITE_BLOCK: &str = "vite";

define_plugin! {
    struct VitePressPlugin => "vitepress",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: CONFIG_PATTERNS,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    used_exports: [
        (".vitepress/theme/index.{ts,js,mts,mjs}", THEME_ENTRY_EXPORTS),
        ("docs/.vitepress/theme/index.{ts,js,mts,mjs}", THEME_ENTRY_EXPORTS),
    ],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        super::vite::credit_dependency_lists(&mut result, source, config_path, &[VITE_BLOCK]);
        for (find, replacement) in config_parser::extract_config_path_aliases(
            source,
            config_path,
            &[VITE_BLOCK, "resolve", "alias"],
        ) {
            if let Some(normalized) =
                config_parser::normalize_config_path(&replacement, config_path, root)
            {
                result.path_aliases.push((find, normalized));
            }
        }
        result
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_docs_scaffold_layout_and_theme_entry_exports() {
        let plugin = VitePressPlugin;

        assert!(
            plugin
                .entry_patterns()
                .contains(&"docs/.vitepress/theme/index.{ts,js,mts,mjs}")
        );
        assert!(
            plugin
                .always_used()
                .contains(&"docs/.vitepress/config.{ts,js,mts,mjs}")
        );
        assert!(plugin.used_exports().iter().any(|(pattern, names)| {
            pattern == &"docs/.vitepress/theme/index.{ts,js,mts,mjs}" && names == &["default"]
        }));
    }

    fn resolve(source: &str) -> PluginResult {
        let root = std::env::temp_dir();
        let config_path = root.join("docs/.vitepress/config.mts");
        VitePressPlugin.resolve_config(&config_path, source, &root)
    }

    #[test]
    fn vite_block_credits_dependency_lists_through_wrapper_calls() {
        let result = resolve(
            r"
            import { defineConfig } from 'vitepress';
            export default extendConfig(withMermaid(defineConfig({
                vite: {
                    optimizeDeps: {
                        include: ['mermaid > @braintree/sanitize-url', 'nested-only'],
                        exclude: ['excluded-dep'],
                    },
                    ssr: { external: ['ssr-external-dep'], noExternal: ['ssr-dep'] },
                },
            })));
            ",
        );
        let deps = &result.referenced_dependencies;
        for dep in [
            "mermaid",
            "nested-only",
            "excluded-dep",
            "ssr-external-dep",
            "ssr-dep",
        ] {
            assert!(deps.contains(&dep.to_string()), "{dep} missing: {deps:?}");
        }
    }

    #[test]
    fn vite_block_reads_resolve_alias() {
        let result = resolve(
            r"
            export default defineConfig({
                vite: { resolve: { alias: { '@layouts': './theme/layouts' } } },
            });
            ",
        );
        assert_eq!(
            result.path_aliases,
            vec![(
                "@layouts".to_string(),
                "docs/.vitepress/theme/layouts".to_string()
            )]
        );
    }

    #[test]
    fn top_level_optimize_deps_outside_the_vite_block_is_not_credited() {
        let result = resolve(
            r"
            export default defineConfig({
                optimizeDeps: { include: ['not-a-vitepress-key'] },
            });
            ",
        );
        assert!(
            result.referenced_dependencies.is_empty(),
            "{:?}",
            result.referenced_dependencies
        );
    }
}
