//! Storybook plugin.
//!
//! Detects Storybook projects and marks story files and config as entry points.
//! Parses .storybook/main config to extract addons, framework, stories,
//! core.builder, and typescript.reactDocgen as referenced dependencies.
//!
//! React Native Storybook keeps the same shape under a different directory.
//! `withStorybook` defaults `configPath` to `./.rnstorybook`, entry-point
//! swapping makes `.rnstorybook/index` the application entry, the bundler
//! generates `.rnstorybook/storybook.requires.{ts,js}`, and on-device addons
//! are declared under `deviceAddons` instead of `addons`. That directory is
//! dot-prefixed and not on the discovery allowlist, so the plugin declares it
//! through `discovery_hidden_dirs`.

use super::config_parser;
use super::{Plugin, PluginResult, ProvidedDependencyRule};
use std::path::{Path, PathBuf};

const ENABLERS: &[&str] = &["storybook", "@storybook/"];

const ENTRY_PATTERNS: &[&str] = &[
    "**/*.stories.{ts,tsx,js,jsx,mdx}",
    ".storybook/**/*.{ts,tsx,js,jsx}",
    ".rnstorybook/**/*.{ts,tsx,js,jsx}",
];

const CONFIG_PATTERNS: &[&str] = &[
    ".storybook/main.{ts,js,mjs,cjs}",
    ".rnstorybook/main.{ts,js,mjs,cjs}",
];

const ALWAYS_USED: &[&str] = &[
    ".storybook/main.{ts,js,mjs,cjs}",
    ".storybook/preview.{ts,tsx,js,jsx}",
    ".storybook/preview-head.html",
    ".storybook/preview-body.html",
    ".storybook/manager.{ts,tsx,js,jsx}",
    ".rnstorybook/main.{ts,js,mjs,cjs}",
    ".rnstorybook/preview.{ts,tsx,js,jsx}",
    ".rnstorybook/index.{ts,tsx,js,jsx}",
    ".rnstorybook/storybook.requires.{ts,js}",
];

/// React Native Storybook's default `configPath`. Storybook owns every file in
/// it, including the generated requires module, so discovery has to reach it
/// before any of the patterns above can match.
const DISCOVERY_HIDDEN_DIRS: &[&str] = &[".rnstorybook"];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "storybook",
    "@storybook/react",
    "@storybook/vue3",
    "@storybook/angular",
    "@storybook/svelte",
    "@storybook/web-components",
    "@storybook/html",
    "@storybook/server",
    "@storybook/blocks",
    "@storybook/testing-library",
    "@storybook/test",
    "@storybook/manager-api",
    "@storybook/preview-api",
    "@storybook/react-native",
];

/// Config keys whose entries name an addon package. `addons` is the web list;
/// `deviceAddons` is the React Native on-device list.
const ADDON_KEYS: &[&str] = &["addons", "deviceAddons"];

const STORYBOOK_EXPORTS: &[&str] = &["*"];

const MANAGER_EXACT_SPECIFIERS: &[&str] = &[
    "react",
    "react-dom",
    "react-dom/client",
    "@emotion/react",
    "@emotion/styled",
    "@storybook/components",
    "@storybook/theming",
    "storybook/manager-api",
    "storybook/theming",
    "storybook/core-events",
];

const MANAGER_SPECIFIER_PREFIXES: &[&str] = &["storybook/internal/"];

fn manager_runtime_dependencies() -> Vec<ProvidedDependencyRule> {
    vec![ProvidedDependencyRule::new(
        ".storybook/manager.{ts,tsx,js,jsx}",
        MANAGER_EXACT_SPECIFIERS.iter().copied(),
        MANAGER_SPECIFIER_PREFIXES.iter().copied(),
    )]
}

fn normalize_mount(to: Option<&str>) -> String {
    let Some(to) = to else {
        return "/".to_string();
    };
    let trimmed = to.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }
    let with_slash = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    with_slash.trim_end_matches('/').to_string()
}

fn resolve_static_dir_from(config_path: &Path, from: &str) -> PathBuf {
    let path = Path::new(from);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

define_plugin! {
    struct StorybookPlugin => "storybook",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    discovery_hidden_dirs: DISCOVERY_HIDDEN_DIRS,
    provided_dependencies: manager_runtime_dependencies(),
    used_exports: [
        ("**/*.stories.{ts,tsx,js,jsx,mdx}", STORYBOOK_EXPORTS),
        (".storybook/**/*.{ts,tsx,js,jsx}", STORYBOOK_EXPORTS),
        (".rnstorybook/**/*.{ts,tsx,js,jsx}", STORYBOOK_EXPORTS),
    ],
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        for key in ADDON_KEYS {
            let addons = config_parser::extract_config_shallow_strings(source, config_path, key);
            for addon in &addons {
                let dep = crate::resolve::extract_package_name(addon);
                result.referenced_dependencies.push(dep);
            }
            let addon_strings =
                config_parser::extract_config_property_strings(source, config_path, key);
            for s in &addon_strings {
                let dep = crate::resolve::extract_package_name(s);
                if !result.referenced_dependencies.contains(&dep) {
                    result.referenced_dependencies.push(dep);
                }
            }
        }

        if let Some(framework) =
            config_parser::extract_config_string(source, config_path, &["framework"])
        {
            let dep = crate::resolve::extract_package_name(&framework);
            result.referenced_dependencies.push(dep);
        } else if let Some(framework_name) =
            config_parser::extract_config_string(source, config_path, &["framework", "name"])
        {
            let dep = crate::resolve::extract_package_name(&framework_name);
            result.referenced_dependencies.push(dep);
        }

        let stories = config_parser::extract_config_string_array(source, config_path, &["stories"]);
        result.extend_entry_patterns(stories);

        for (from, to) in
            config_parser::extract_config_static_dir_entries(source, config_path, &["staticDirs"])
        {
            result
                .static_dir_mappings
                .push((resolve_static_dir_from(config_path, &from), normalize_mount(to.as_deref())));
        }

        if let Some(builder) =
            config_parser::extract_config_string(source, config_path, &["core", "builder"])
        {
            let dep = crate::resolve::extract_package_name(&builder);
            result.referenced_dependencies.push(dep);
        } else if let Some(builder_name) =
            config_parser::extract_config_string(source, config_path, &["core", "builder", "name"])
        {
            let dep = crate::resolve::extract_package_name(&builder_name);
            result.referenced_dependencies.push(dep);
        }

        if let Some(docgen) = config_parser::extract_config_string(
            source,
            config_path,
            &["typescript", "reactDocgen"],
        ) && !matches!(docgen.as_str(), "false" | "none")
        {
            result.referenced_dependencies.push(docgen);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_core_builder() {
        let source = r#"
            export default {
                core: { builder: "@storybook/builder-vite" }
            };
        "#;
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".storybook/main.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@storybook/builder-vite".to_string())
        );
    }

    #[test]
    fn resolve_config_react_docgen() {
        let source = r#"
            export default {
                typescript: { reactDocgen: "react-docgen-typescript" }
            };
        "#;
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".storybook/main.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"react-docgen-typescript".to_string())
        );
    }

    #[test]
    fn resolve_config_addons_string_form() {
        let source = r#"
            export default {
                addons: ["@storybook/addon-essentials", "@storybook/addon-a11y"]
            };
        "#;
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".storybook/main.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@storybook/addon-essentials".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@storybook/addon-a11y".to_string())
        );
    }

    #[test]
    fn resolve_config_addons_object_form() {
        let source = r#"
            export default {
                addons: [
                    { name: "@storybook/addon-essentials", options: { docs: false } },
                    "@storybook/addon-a11y"
                ]
            };
        "#;
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".storybook/main.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@storybook/addon-essentials".to_string()),
            "should find addon in object form via name property"
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@storybook/addon-a11y".to_string()),
            "should find addon in string form"
        );
    }

    #[test]
    fn resolve_config_react_docgen_false_ignored() {
        let source = r#"
            export default {
                typescript: { reactDocgen: "false" }
            };
        "#;
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".storybook/main.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(!result.referenced_dependencies.iter().any(|d| d == "false"));
    }

    #[test]
    fn discovery_hidden_dirs_include_react_native_config_dir() {
        let plugin = StorybookPlugin;
        assert_eq!(plugin.discovery_hidden_dirs(), [".rnstorybook"]);
    }

    #[test]
    fn react_native_config_patterns_cover_the_rnstorybook_directory() {
        let plugin = StorybookPlugin;
        assert!(
            plugin
                .config_patterns()
                .contains(&".rnstorybook/main.{ts,js,mjs,cjs}")
        );
        assert!(
            plugin
                .entry_patterns()
                .contains(&".rnstorybook/**/*.{ts,tsx,js,jsx}")
        );
        assert!(
            plugin
                .always_used()
                .contains(&".rnstorybook/storybook.requires.{ts,js}")
        );
        assert!(
            plugin
                .always_used()
                .contains(&".rnstorybook/index.{ts,tsx,js,jsx}")
        );
    }

    #[test]
    fn resolve_config_device_addons_string_form() {
        let source = r#"
            import type { StorybookConfig } from '@storybook/react-native';

            const config: StorybookConfig = {
                stories: ["../src/**/*.stories.?(ts|tsx|js|jsx)"],
                deviceAddons: [
                    "@storybook/addon-ondevice-controls",
                    "@storybook/addon-ondevice-actions"
                ]
            };
            export default config;
        "#;
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".rnstorybook/main.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@storybook/addon-ondevice-controls".to_string()));
        assert!(deps.contains(&"@storybook/addon-ondevice-actions".to_string()));
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|pattern| pattern.contains("*.stories.")),
            "the story glob still becomes an entry pattern: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn resolve_config_device_addons_object_form() {
        let source = r#"
            export default {
                deviceAddons: [
                    { name: "@storybook/addon-ondevice-controls", options: {} },
                    "@storybook/addon-ondevice-actions"
                ]
            };
        "#;
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".rnstorybook/main.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(
            deps.contains(&"@storybook/addon-ondevice-controls".to_string()),
            "object-form device addon credited via its name property"
        );
        assert!(deps.contains(&"@storybook/addon-ondevice-actions".to_string()));
    }

    #[test]
    fn resolve_config_typed_const_variable_reference() {
        let source = r#"
            import type { StorybookConfig } from '@storybook/react-vite';

            const config: StorybookConfig = {
                "stories": ["../src/**/*.mdx", "../src/**/*.stories.@(js|jsx|mjs|ts|tsx)"],
                "addons": [
                    "@chromatic-com/storybook",
                    "@storybook/addon-vitest",
                    "@storybook/addon-a11y",
                    "@storybook/addon-docs",
                    "@storybook/addon-onboarding"
                ],
                "framework": "@storybook/react-vite"
            };
            export default config;
        "#;
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".storybook/main.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@chromatic-com/storybook".to_string()));
        assert!(deps.contains(&"@storybook/addon-vitest".to_string()));
        assert!(deps.contains(&"@storybook/addon-a11y".to_string()));
        assert!(deps.contains(&"@storybook/addon-docs".to_string()));
        assert!(deps.contains(&"@storybook/addon-onboarding".to_string()));
        assert!(deps.contains(&"@storybook/react-vite".to_string()));
    }
}
