//! Prettier plugin.
//!
//! Detects Prettier projects and marks config files as always used.
//! Parses prettier config to extract plugins as referenced dependencies.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["prettier"];

const CONFIG_PATTERNS: &[&str] = &[
    ".prettierrc",
    ".prettierrc.{json,json5,yml,yaml,toml,js,cjs,mjs,ts,cts}",
    "prettier.config.{js,cjs,mjs,ts,cts}",
];

const ALWAYS_USED: &[&str] = &[
    ".prettierrc",
    ".prettierrc.{json,json5,yml,yaml,toml,js,cjs,mjs,ts,cts}",
    "prettier.config.{js,cjs,mjs,ts,cts}",
    ".prettierignore",
];

const TOOLING_DEPENDENCIES: &[&str] = &["prettier"];

/// Data-format prettier config flavors that carry a top-level `plugins` array.
#[derive(Clone, Copy)]
enum DataFormat {
    Yaml,
    Toml,
}

/// Extract the top-level `plugins` string array from a YAML or TOML prettier
/// config. Returns an empty vec on parse failure or when `plugins` is absent or
/// not a string array, so a malformed config never panics analysis.
fn extract_data_format_plugins(format: DataFormat, source: &str) -> Vec<String> {
    match format {
        DataFormat::Yaml => serde_yaml_ng::from_str::<serde_yaml_ng::Value>(source)
            .ok()
            .and_then(|value| {
                value
                    .get("plugins")
                    .and_then(serde_yaml_ng::Value::as_sequence)
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
            })
            .unwrap_or_default(),
        DataFormat::Toml => toml::from_str::<toml::Value>(source)
            .ok()
            .and_then(|value| {
                value
                    .get("plugins")
                    .and_then(toml::Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
            })
            .unwrap_or_default(),
    }
}

define_plugin! {
    struct PrettierPlugin => "prettier",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    package_json_config_key: "prettier",
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        // YAML / TOML configs (.prettierrc.{yml,yaml,toml}) are not JS, so the
        // JS/TS parse path below cannot read their `plugins` array. Parse them
        // with the matching data-format parser instead (issue #462: prettier
        // plugins are no longer in the general tooling catalogue, so crediting
        // them from the config file is the only path that keeps a genuinely-used
        // plugin from surfacing as unused).
        if let Some(ext) = config_path.extension().and_then(|e| e.to_str()) {
            match ext {
                "yml" | "yaml" => {
                    for plugin in extract_data_format_plugins(DataFormat::Yaml, source) {
                        result
                            .referenced_dependencies
                            .push(crate::resolve::extract_package_name(&plugin));
                    }
                    return result;
                }
                "toml" => {
                    for plugin in extract_data_format_plugins(DataFormat::Toml, source) {
                        result
                            .referenced_dependencies
                            .push(crate::resolve::extract_package_name(&plugin));
                    }
                    return result;
                }
                _ => {}
            }
        }

        // Handle JSON configs (.prettierrc, .prettierrc.json)
        let is_json = config_path.extension().is_some_and(|ext| ext == "json")
            || config_path
                .file_name()
                .is_some_and(|name| name == ".prettierrc");
        let (parse_source, parse_path_buf) = if is_json {
            (format!("({source})"), config_path.with_extension("js"))
        } else {
            (source.to_string(), config_path.to_path_buf())
        };
        let parse_path: &std::path::Path = &parse_path_buf;

        // Extract imports from JS/TS configs
        let imports = config_parser::extract_imports(&parse_source, parse_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        // plugins -> referenced dependencies
        // e.g. { "plugins": ["prettier-plugin-svelte", "prettier-plugin-tailwindcss"] }
        let plugins =
            config_parser::extract_config_shallow_strings(&parse_source, parse_path, "plugins");
        for plugin in &plugins {
            let dep = crate::resolve::extract_package_name(plugin);
            result.referenced_dependencies.push(dep);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn resolve_config_json_plugins() {
        let source = r#"{"plugins": ["prettier-plugin-svelte", "prettier-plugin-tailwindcss"]}"#;
        let plugin = PrettierPlugin;
        let result = plugin.resolve_config(Path::new(".prettierrc"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"prettier-plugin-svelte".to_string()));
        assert!(deps.contains(&"prettier-plugin-tailwindcss".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins() {
        let source = r#"
            export default {
                plugins: ["prettier-plugin-svelte"]
            };
        "#;
        let plugin = PrettierPlugin;
        let result = plugin.resolve_config(
            Path::new("prettier.config.js"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"prettier-plugin-svelte".to_string())
        );
    }

    #[test]
    fn resolve_config_empty() {
        let source = r#"{"singleQuote": true}"#;
        let plugin = PrettierPlugin;
        let result = plugin.resolve_config(Path::new(".prettierrc"), source, Path::new("/project"));

        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_yaml_plugins() {
        // Issue #462: prettier-plugin-* is no longer in the tooling catalogue,
        // so a YAML config's plugins array must be parsed to credit the plugin.
        let source = "plugins:\n  - prettier-plugin-svelte\n  - prettier-plugin-tailwindcss\n";
        let plugin = PrettierPlugin;
        let result =
            plugin.resolve_config(Path::new(".prettierrc.yaml"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"prettier-plugin-svelte".to_string()));
        assert!(deps.contains(&"prettier-plugin-tailwindcss".to_string()));
    }

    #[test]
    fn resolve_config_yml_plugins() {
        let source = "plugins:\n  - prettier-plugin-organize-imports\n";
        let plugin = PrettierPlugin;
        let result =
            plugin.resolve_config(Path::new(".prettierrc.yml"), source, Path::new("/project"));

        assert!(
            result
                .referenced_dependencies
                .contains(&"prettier-plugin-organize-imports".to_string())
        );
    }

    #[test]
    fn resolve_config_toml_plugins() {
        let source = "plugins = [\"prettier-plugin-tailwindcss\", \"@ianvs/prettier-plugin-sort-imports\"]\n";
        let plugin = PrettierPlugin;
        let result =
            plugin.resolve_config(Path::new(".prettierrc.toml"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"prettier-plugin-tailwindcss".to_string()));
        assert!(deps.contains(&"@ianvs/prettier-plugin-sort-imports".to_string()));
    }

    #[test]
    fn resolve_config_yaml_no_plugins_is_empty() {
        let source = "singleQuote: true\nsemi: false\n";
        let plugin = PrettierPlugin;
        let result =
            plugin.resolve_config(Path::new(".prettierrc.yaml"), source, Path::new("/project"));

        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_malformed_yaml_does_not_panic() {
        let source = "plugins: [unterminated\n";
        let plugin = PrettierPlugin;
        let result =
            plugin.resolve_config(Path::new(".prettierrc.yaml"), source, Path::new("/project"));

        assert!(result.referenced_dependencies.is_empty());
    }
}
