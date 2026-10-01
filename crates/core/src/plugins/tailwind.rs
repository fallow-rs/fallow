//! Tailwind CSS plugin.
//!
//! Detects Tailwind projects and marks config files as always used.
//! Parses tailwind.config to extract content globs and plugin dependencies.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["tailwindcss", "@tailwindcss/postcss"];

const CONFIG_PATTERNS: &[&str] = &["tailwind.config.{ts,js,cjs,mjs}"];

const ALWAYS_USED: &[&str] = &["tailwind.config.{ts,js,cjs,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["tailwindcss", "@tailwindcss/postcss"];

define_plugin! {
    struct TailwindPlugin => "tailwind",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        let content = config_parser::extract_config_string_array(source, config_path, &["content"]);
        result.always_used_files.extend(content);

        let require_deps =
            config_parser::extract_config_require_strings(source, config_path, "plugins");
        for dep in &require_deps {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        let plugin_strings =
            config_parser::extract_config_shallow_strings(source, config_path, "plugins");
        for plugin in &plugin_strings {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(plugin));
        }

        let presets = config_parser::extract_config_shallow_strings(source, config_path, "presets");
        for preset in &presets {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(preset));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_content_globs() {
        let source = r#"
            module.exports = {
                content: ["./src/**/*.{js,ts,jsx,tsx}", "./index.html"]
            };
        "#;
        let plugin = TailwindPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tailwind.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(
            result.always_used_files,
            vec!["./src/**/*.{js,ts,jsx,tsx}", "./index.html"]
        );
    }

    #[test]
    fn resolve_config_plugins_require() {
        let source = r#"
            module.exports = {
                plugins: [require("@tailwindcss/typography"), require("@tailwindcss/forms")]
            };
        "#;
        let plugin = TailwindPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tailwind.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@tailwindcss/typography".to_string()));
        assert!(deps.contains(&"@tailwindcss/forms".to_string()));
    }

    #[test]
    fn resolve_config_plugins_string_array() {
        let source = r#"
            module.exports = {
                plugins: ["@tailwindcss/typography", "@tailwindcss/forms"]
            };
        "#;
        let plugin = TailwindPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tailwind.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@tailwindcss/typography".to_string()));
        assert!(deps.contains(&"@tailwindcss/forms".to_string()));
    }

    #[test]
    fn resolve_config_presets() {
        let source = r#"
            module.exports = {
                presets: ["@acme/tailwind-preset", "my-preset"]
            };
        "#;
        let plugin = TailwindPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tailwind.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@acme/tailwind-preset".to_string()));
        assert!(deps.contains(&"my-preset".to_string()));
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import defaultTheme from 'tailwindcss/defaultTheme';
            import forms from '@tailwindcss/forms';
            module.exports = {
                content: ["./src/**/*.tsx"]
            };
        "#;
        let plugin = TailwindPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tailwind.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"tailwindcss".to_string()));
        assert!(deps.contains(&"@tailwindcss/forms".to_string()));
    }

    #[test]
    fn resolve_config_combined() {
        let source = r#"
            import defaultTheme from 'tailwindcss/defaultTheme';
            module.exports = {
                content: ["./src/**/*.tsx"],
                plugins: [require("@tailwindcss/typography")],
                presets: ["my-preset"]
            };
        "#;
        let plugin = TailwindPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tailwind.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.always_used_files, vec!["./src/**/*.tsx"]);
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"tailwindcss".to_string()));
        assert!(deps.contains(&"@tailwindcss/typography".to_string()));
        assert!(deps.contains(&"my-preset".to_string()));
    }

    #[test]
    fn resolve_config_empty() {
        let source = r"module.exports = {};";
        let plugin = TailwindPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tailwind.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.always_used_files, [] as [String; 0]);
        assert_eq!(result.referenced_dependencies, [] as [String; 0]);
    }

    #[test]
    fn resolve_config_scoped_package_in_plugins() {
        let source = r#"
            module.exports = {
                plugins: [require("@scope/plugin/nested")]
            };
        "#;
        let plugin = TailwindPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tailwind.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@scope/plugin".to_string())
        );
    }
}
