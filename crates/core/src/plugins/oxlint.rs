//! Oxlint plugin.
//!
//! Detects Oxlint projects and marks config files as always used.

use std::path::{Component, Path, PathBuf};

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["oxlint"];

const CONFIG_PATTERNS: &[&str] = &[".oxlintrc.json", "oxlint.json", "oxlint.config.ts"];

const ALWAYS_USED: &[&str] = CONFIG_PATTERNS;

const TOOLING_DEPENDENCIES: &[&str] = &["oxlint", "oxlint-tsgolint"];

define_plugin! {
    struct OxlintPlugin => "oxlint",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        let js_plugins = config_parser::extract_config_shallow_strings_or_object_property(
            source,
            config_path,
            "jsPlugins",
            "specifier",
        );
        for specifier in js_plugins {
            credit_config_specifier(&mut result, config_path, root, &specifier);
        }

        let extends = config_parser::extract_config_shallow_strings(source, config_path, "extends");
        for entry in extends {
            credit_config_specifier(&mut result, config_path, root, &entry);
        }

        result
    }
}

fn credit_config_specifier(
    result: &mut PluginResult,
    config_path: &Path,
    root: &Path,
    specifier: &str,
) {
    if is_local_specifier(specifier) {
        result
            .setup_files
            .push(resolve_config_relative_path(config_path, root, specifier));
    } else if is_package_specifier(specifier) {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(specifier));
    }
}

fn is_local_specifier(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/')
}

fn is_package_specifier(specifier: &str) -> bool {
    !specifier.is_empty()
        && !is_local_specifier(specifier)
        && !specifier.contains(':')
        && !specifier.contains('\\')
}

fn resolve_config_relative_path(config_path: &Path, root: &Path, specifier: &str) -> PathBuf {
    let config_abs = if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        root.join(config_path)
    };
    lexical_normalize(&config_abs.parent().unwrap_or(root).join(specifier))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_js_plugins() {
        let source = r#"
            {
            "plugins": ["typescript", "vitest", "unicorn", "import", "promise", "node"],
            "jsPlugins": [
                "eslint-plugin-testing-library",
                "eslint-plugin-playwright",
                "eslint-plugin-sonarjs"
            ]
            }
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxlintrc.json"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-testing-library".to_string()));
        assert!(deps.contains(&"eslint-plugin-playwright".to_string()));
        assert!(deps.contains(&"eslint-plugin-sonarjs".to_string()));
        assert!(!deps.contains(&"typescript".to_string()));
        assert!(!deps.contains(&"vitest".to_string()));
        assert!(!deps.contains(&"unicorn".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins_object_aliases() {
        let source = r#"
            {
                "jsPlugins": [
                    { "name": "testing", "specifier": "eslint-plugin-testing-library" },
                    { "name": "playwright", "specifier": "eslint-plugin-playwright" }
                ]
            }
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxlintrc.json"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-testing-library".to_string()));
        assert!(deps.contains(&"eslint-plugin-playwright".to_string()));
        assert!(!deps.contains(&"testing".to_string()));
        assert!(!deps.contains(&"playwright".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins_oxlint_json() {
        let source = r#"
            {
                "jsPlugins": ["eslint-plugin-testing-library", "eslint-plugin-playwright"]
            }
        "#;
        let plugin = OxlintPlugin;

        let result = plugin.resolve_config(Path::new("oxlint.json"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-testing-library".to_string()));
        assert!(deps.contains(&"eslint-plugin-playwright".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins_oxlint_ts_config() {
        let source = r#"
            import { defineConfig } from "oxlint";

            export default defineConfig({
                jsPlugins: ["eslint-plugin-testing-library", "eslint-plugin-playwright"]
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-testing-library".to_string()));
        assert!(deps.contains(&"eslint-plugin-playwright".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins_tuple_with_options() {
        let source = r#"
            {
                "jsPlugins": [
                    "eslint-plugin-testing-library",
                    ["eslint-plugin-playwright", { "rules": {} }]
                ]
            }
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxlintrc.json"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-testing-library".to_string()));
        assert!(deps.contains(&"eslint-plugin-playwright".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins_local_paths_are_setup_files() {
        let source = r#"
            {
                "jsPlugins": [
                    "./plugins/local.js",
                    "../shared/other-plugin.js",
                    "eslint-plugin-playwright"
                ]
            }
        "#;
        let plugin = OxlintPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/config/.oxlintrc.json"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/config/plugins/local.js"))
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/shared/other-plugin.js"))
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"eslint-plugin-playwright".to_string())
        );
        assert!(!result.referenced_dependencies.contains(&".".to_string()));
    }

    #[test]
    fn resolve_config_empty() {
        let source = r#"{ "options": { "typeAware": true } }"#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxlintrc.json"), source, Path::new("/project"));

        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn tooling_dependencies_include_cli_tooling_packages() {
        let plugin = OxlintPlugin;
        let tooling = plugin.tooling_dependencies();
        assert!(tooling.contains(&"oxlint"));
        assert!(tooling.contains(&"oxlint-tsgolint"));
    }

    #[test]
    fn resolve_config_no_js_plugins() {
        let source = r#"
            {
                "plugins": ["typescript", "import"],
                "rules": { "no-console": "warn" }
            }
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxlintrc.json"), source, Path::new("/project"));

        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_extends_credits_package_and_records_local_path() {
        let source = r#"
            {
                "extends": ["@nkzw/oxlint-config", "./local.json"]
            }
        "#;
        let plugin = OxlintPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/.oxlintrc.json"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"@nkzw/oxlint-config".to_string()),
            "expected @nkzw/oxlint-config in referenced_dependencies"
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/local.json")),
            "expected /project/local.json in setup_files"
        );
        assert!(
            !result
                .referenced_dependencies
                .contains(&"./local.json".to_string()),
            "local path must not appear in referenced_dependencies"
        );
    }
}
