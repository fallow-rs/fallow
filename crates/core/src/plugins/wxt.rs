//! WXT browser extension framework plugin.
//!
//! WXT owns `wxt.config.*` plus file-based extension entrypoints under
//! `entrypoints/`. Static `modules` declarations in config load npm packages
//! at build time, so those package names must be credited even when app source
//! never imports them.

use std::path::Path;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["wxt", "@wxt-dev/"];
const CONFIG_FILES: &[&str] = &[
    "wxt.config.ts",
    "wxt.config.js",
    "wxt.config.mts",
    "wxt.config.mjs",
    "wxt.config.cjs",
    "wxt.config.cts",
];
const CONFIG_PATTERNS: &[&str] = &["wxt.config.{ts,js,mts,mjs,cjs,cts}"];
const ENTRY_PATTERNS: &[&str] = &[
    "entrypoints/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs,html,svelte,vue}",
    "entrypoints/**/index.{ts,tsx,js,jsx,mts,mjs,cts,cjs,html,svelte,vue}",
    "src/entrypoints/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs,html,svelte,vue}",
    "src/entrypoints/**/index.{ts,tsx,js,jsx,mts,mjs,cts,cjs,html,svelte,vue}",
];
const ALWAYS_USED: &[&str] = CONFIG_PATTERNS;
const CONFIG_EXPORTS: &[&str] = &["default"];
const TOOLING_DEPENDENCIES: &[&str] = &[
    "wxt",
    "@wxt-dev/analytics",
    "@wxt-dev/auto-icons",
    "@wxt-dev/browser",
    "@wxt-dev/i18n",
    "@wxt-dev/module-react",
    "@wxt-dev/module-svelte",
    "@wxt-dev/module-vue",
];

pub struct WxtPlugin;

impl Plugin for WxtPlugin {
    fn name(&self) -> &'static str {
        "wxt"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter().any(|dep| {
            dep == "wxt"
                || dep
                    .strip_prefix("@wxt-dev/")
                    .is_some_and(|suffix| !suffix.is_empty())
        }) || has_wxt_config(root)
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

    fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
        vec![("wxt.config.{ts,js,mts,mjs,cjs,cts}", CONFIG_EXPORTS)]
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, _root: &Path) -> PluginResult {
        let mut referenced_dependencies: Vec<String> =
            config_parser::extract_config_string_or_array(source, config_path, &["modules"])
                .into_iter()
                .filter_map(|specifier| package_name_for_specifier(&specifier))
                .collect();
        referenced_dependencies.sort();
        referenced_dependencies.dedup();

        PluginResult {
            referenced_dependencies,
            ..PluginResult::default()
        }
    }
}

fn has_wxt_config(root: &Path) -> bool {
    CONFIG_FILES
        .iter()
        .any(|config| root.join(config).is_file())
}

fn package_name_for_specifier(specifier: &str) -> Option<String> {
    let specifier = specifier.trim();
    is_package_specifier(specifier).then(|| crate::resolve::extract_package_name(specifier))
}

fn is_package_specifier(specifier: &str) -> bool {
    !specifier.is_empty()
        && specifier != "."
        && specifier != ".."
        && !specifier.starts_with("./")
        && !specifier.starts_with("../")
        && !specifier.starts_with('/')
        && !specifier.contains(':')
        && !specifier.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activates_from_wxt_dependency_scoped_dependency_or_config_file() {
        let plugin = WxtPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");

        assert!(plugin.is_enabled_with_deps(&["wxt".to_string()], tmp.path()));
        assert!(plugin.is_enabled_with_deps(&["@wxt-dev/module-svelte".to_string()], tmp.path()));
        assert!(!plugin.is_enabled_with_deps(&["@wxt-dev".to_string()], tmp.path()));
        assert!(!plugin.is_enabled_with_deps(&["@wxt-devish/module".to_string()], tmp.path()));
        assert!(!plugin.is_enabled_with_deps(&[], tmp.path()));

        std::fs::write(tmp.path().join("wxt.config.ts"), "export default {};\n")
            .expect("wxt config");
        assert!(plugin.is_enabled_with_deps(&[], tmp.path()));
    }

    #[test]
    fn exposes_static_wxt_conventions() {
        let plugin = WxtPlugin;

        assert_eq!(plugin.config_patterns(), CONFIG_PATTERNS);
        assert_eq!(plugin.always_used(), ALWAYS_USED);
        assert_eq!(plugin.tooling_dependencies(), TOOLING_DEPENDENCIES);
        assert!(
            plugin
                .entry_patterns()
                .contains(&"entrypoints/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs,html,svelte,vue}")
        );
        assert!(
            plugin
                .entry_patterns()
                .contains(&"entrypoints/**/index.{ts,tsx,js,jsx,mts,mjs,cts,cjs,html,svelte,vue}")
        );
        assert!(
            !plugin
                .entry_patterns()
                .iter()
                .any(|pattern| pattern.contains("entrypoints/**/*.{")),
            "WXT entry globs must not credit every helper under entrypoint directories"
        );
    }

    #[test]
    fn config_default_export_is_used() {
        let plugin = WxtPlugin;
        let exports = plugin.used_exports();

        assert!(exports.iter().any(|(pattern, names)| {
            *pattern == "wxt.config.{ts,js,mts,mjs,cjs,cts}" && names.contains(&"default")
        }));
    }

    #[test]
    fn resolve_config_credits_static_modules_and_module_subpaths() {
        let plugin = WxtPlugin;
        let source = r"
            import { defineConfig } from 'wxt';

            export default defineConfig({
                modules: [
                    '@wxt-dev/module-svelte',
                    '@wxt-dev/i18n/module',
                    'wxt-sample-module',
                    './local-module',
                    'node:fs'
                ],
            });
        ";

        let result = plugin.resolve_config(Path::new("wxt.config.ts"), source, Path::new("/repo"));

        assert_eq!(
            result.referenced_dependencies,
            vec![
                "@wxt-dev/i18n".to_string(),
                "@wxt-dev/module-svelte".to_string(),
                "wxt-sample-module".to_string(),
            ]
        );
    }

    #[test]
    fn resolve_config_ignores_dynamic_modules() {
        let plugin = WxtPlugin;
        let source = r"
            const moduleName = '@wxt-dev/module-svelte';
            export default defineConfig({ modules: [moduleName] });
        ";

        let result = plugin.resolve_config(Path::new("wxt.config.ts"), source, Path::new("/repo"));

        assert!(result.referenced_dependencies.is_empty());
    }
}
