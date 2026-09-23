//! Rolldown bundler plugin.
//!
//! Detects Rolldown projects (Rust-based Rollup replacement) and marks config
//! files as always used. Parses rolldown config to extract imports and entry
//! point references as dependencies.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["rolldown"];

const CONFIG_PATTERNS: &[&str] = &["rolldown.config.{js,cjs,mjs,ts,mts,cts}"];

const ALWAYS_USED: &[&str] = &["rolldown.config.{js,cjs,mjs,ts,mts,cts}"];

const TOOLING_DEPENDENCIES: &[&str] = &["rolldown", "@rolldown/pluginutils"];

define_plugin! {
    struct RolldownPlugin => "rolldown",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);

        let inputs = config_parser::extract_config_string_or_array(source, config_path, &["input"]);
        result.extend_entry_patterns_and_dependencies(inputs, root);

        let external =
            config_parser::extract_config_shallow_strings(source, config_path, "external");
        for ext in &external {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(ext));
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn resolve_config_input_string() {
        let source = r#"export default { input: "./src/index.js" };"#;
        let plugin = RolldownPlugin;
        let result = plugin.resolve_config(
            Path::new("rolldown.config.js"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/index.js"]);
    }

    #[test]
    fn resolve_config_input_array() {
        let source = r#"
            export default {
                input: ["./src/index.js", "./src/cli.js"]
            };
        "#;
        let plugin = RolldownPlugin;
        let result = plugin.resolve_config(
            Path::new("rolldown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/index.js", "src/cli.js"]);
    }

    #[test]
    fn resolve_config_input_object() {
        let source = r#"
            export default {
                input: {
                    main: "./src/main.js",
                    utils: "./src/utils.js"
                }
            };
        "#;
        let plugin = RolldownPlugin;
        let result = plugin.resolve_config(
            Path::new("rolldown.config.mjs"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/main.js", "src/utils.js"]);
    }

    #[test]
    fn resolve_config_external() {
        let source = r#"
            export default {
                input: "./src/index.js",
                external: ["react", "react-dom", "@scope/lib"]
            };
        "#;
        let plugin = RolldownPlugin;
        let result = plugin.resolve_config(
            Path::new("rolldown.config.js"),
            source,
            Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"react-dom".to_string()));
        assert!(deps.contains(&"@scope/lib".to_string()));
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import { defineConfig } from 'rolldown';
            import pluginA from '@rolldown/pluginutils';
            export default defineConfig({
                input: "./src/main.ts"
            });
        "#;
        let plugin = RolldownPlugin;
        let result = plugin.resolve_config(
            Path::new("rolldown.config.ts"),
            source,
            Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"rolldown".to_string()));
        assert!(deps.contains(&"@rolldown/pluginutils".to_string()));
        assert_eq!(result.entry_patterns, vec!["src/main.ts"]);
    }

    #[test]
    fn resolve_config_empty() {
        let source = r"export default {};";
        let plugin = RolldownPlugin;
        let result = plugin.resolve_config(
            Path::new("rolldown.config.js"),
            source,
            Path::new("/project"),
        );
        assert!(result.entry_patterns.is_empty());
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_no_input() {
        let source = r#"
            export default {
                output: { dir: "dist" }
            };
        "#;
        let plugin = RolldownPlugin;
        let result = plugin.resolve_config(
            Path::new("rolldown.config.js"),
            source,
            Path::new("/project"),
        );
        assert!(result.entry_patterns.is_empty());
    }

    /// A bare `input` value is either a module request or a path that rollup
    /// resolves against the working directory, so it credits the package and
    /// keeps the entry pattern (issue #2753).
    #[test]
    fn a_bare_input_credits_the_package_and_keeps_the_entry_pattern() {
        let source = r#"export default { input: { client: "my-lib/client", app: "src/app", main: "./src/main.js" } };"#;
        let result = RolldownPlugin.resolve_config(
            Path::new("rolldown.config.js"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"my-lib".to_string()),
            "got {:?}",
            result.referenced_dependencies
        );
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        for expected in ["my-lib/client", "src/app", "src/main.js"] {
            assert!(patterns.contains(&expected), "{expected}: got {patterns:?}");
        }
        assert!(
            patterns
                .iter()
                .any(|pattern| pattern.starts_with("src/app.{")),
            "an extensionless input resolves to the file, got {patterns:?}"
        );
    }
}
