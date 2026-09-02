//! Storybook plugin.
//!
//! Detects Storybook projects and marks story files and config as entry points.
//! Parses web and React Native Storybook main config to extract addons,
//! framework, stories, core.builder, and typescript.reactDocgen as referenced
//! dependencies.

use super::config_parser;
use super::{Plugin, PluginResult, ProvidedDependencyRule};
use std::path::{Path, PathBuf};

const ENABLERS: &[&str] = &["storybook", "@storybook/"];

const ENTRY_PATTERNS: &[&str] = &[
    "**/*.stories.{ts,tsx,js,jsx,mdx}",
    ".storybook/**/*.{ts,tsx,js,jsx}",
    ".rnstorybook/index.{ts,tsx,js,jsx}",
    ".rnstorybook/storybook.requires.{ts,js}",
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
];

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
];

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

fn add_addon_dependencies(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    property: &str,
) {
    let addon_strings =
        config_parser::extract_config_shallow_strings(source, config_path, property)
            .into_iter()
            .chain(config_parser::extract_config_property_strings(
                source,
                config_path,
                property,
            ));

    for addon in addon_strings {
        let dependency = crate::resolve::extract_package_name(&addon);
        if !result.referenced_dependencies.contains(&dependency) {
            result.referenced_dependencies.push(dependency);
        }
    }
}

fn is_react_native_config(config_path: &Path) -> bool {
    config_path.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new(".rnstorybook"))
}

fn extract_story_patterns(source: &str, config_path: &Path, root: &Path) -> Vec<String> {
    let patterns = config_parser::extract_config_string_array(source, config_path, &["stories"]);
    if !is_react_native_config(config_path) {
        return patterns;
    }

    patterns
        .into_iter()
        .filter_map(|pattern| config_parser::normalize_config_path(&pattern, config_path, root))
        .collect()
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
        (".rnstorybook/main.{ts,js,mjs,cjs}", STORYBOOK_EXPORTS),
        (".rnstorybook/index.{ts,tsx,js,jsx}", STORYBOOK_EXPORTS),
        (".rnstorybook/storybook.requires.{ts,js}", STORYBOOK_EXPORTS),
    ],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        add_addon_dependencies(&mut result, source, config_path, "addons");
        if is_react_native_config(config_path) {
            add_addon_dependencies(&mut result, source, config_path, "deviceAddons");
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

        let stories = extract_story_patterns(source, config_path, root);
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
    fn react_native_storybook_registers_only_its_canonical_hidden_dir() {
        let plugin = StorybookPlugin;
        assert_eq!(plugin.discovery_hidden_dirs(), [".rnstorybook"]);
    }

    #[test]
    fn react_native_storybook_package_activates_plugin() {
        let plugin = StorybookPlugin;
        let dependencies = vec!["@storybook/react-native".to_string()];

        assert!(plugin.is_enabled_with_deps(&dependencies, Path::new("/project")));
    }

    #[test]
    fn resolve_config_device_addons_string_and_object_forms() {
        let source = r#"
            export default {
                deviceAddons: [
                    "@storybook/addon-ondevice-actions",
                    {
                        name: "storybook-addon-deep-controls",
                        options: { enabled: true }
                    }
                ]
            };
        "#;
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            Path::new(".rnstorybook/main.ts"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"@storybook/addon-ondevice-actions".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"storybook-addon-deep-controls".to_string())
        );
    }

    #[test]
    fn web_storybook_config_does_not_credit_device_addons() {
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            Path::new(".storybook/main.ts"),
            r#"export default { deviceAddons: ["@storybook/addon-ondevice-actions"] };"#,
            Path::new("/project"),
        );

        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_react_native_story_patterns_relative_to_config_dir() {
        let plugin = StorybookPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/.rnstorybook/main.ts"),
            r#"export default { stories: ["../src/mobile-case.tsx"] };"#,
            Path::new("/project"),
        );

        assert!(
            result
                .entry_patterns
                .iter()
                .any(|pattern| pattern.pattern == "src/mobile-case.tsx")
        );
    }

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
