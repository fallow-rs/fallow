//! Rollup module bundler plugin.
//!
//! Detects Rollup projects and marks config files as always used.
//! Parses rollup config to extract imports and plugin references as dependencies.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["rollup"];

const CONFIG_PATTERNS: &[&str] = &["rollup.config.{js,ts,mjs,cjs}"];

const ALWAYS_USED: &[&str] = &["rollup.config.{js,ts,mjs,cjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["rollup"];

define_plugin! {
    struct RollupPlugin => "rollup",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);

        let inputs = config_parser::extract_config_string_or_array(source, config_path, &["input"]);
        result.extend_entry_patterns_and_dependencies(inputs);

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
        let plugin = RollupPlugin;
        let result =
            plugin.resolve_config(Path::new("rollup.config.js"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/index.js"]);
    }

    #[test]
    fn resolve_config_input_array() {
        let source = r#"
            export default {
                input: ["./src/index.js", "./src/cli.js"]
            };
        "#;
        let plugin = RollupPlugin;
        let result =
            plugin.resolve_config(Path::new("rollup.config.js"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/index.js", "src/cli.js"]);
    }

    #[test]
    fn resolve_config_input_object() {
        let source = r#"
            export default {
                input: {
                    main: "./src/main.js",
                    vendor: "./src/vendor.js"
                }
            };
        "#;
        let plugin = RollupPlugin;
        let result =
            plugin.resolve_config(Path::new("rollup.config.js"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/main.js", "src/vendor.js"]);
    }

    #[test]
    fn resolve_config_external() {
        let source = r#"
            export default {
                input: "./src/index.js",
                external: ["lodash", "react", "@scope/pkg"]
            };
        "#;
        let plugin = RollupPlugin;
        let result =
            plugin.resolve_config(Path::new("rollup.config.js"), source, Path::new("/project"));
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"lodash".to_string()));
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"@scope/pkg".to_string()));
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import resolve from '@rollup/plugin-node-resolve';
            import commonjs from '@rollup/plugin-commonjs';
            export default {
                input: "./src/index.js"
            };
        "#;
        let plugin = RollupPlugin;
        let result =
            plugin.resolve_config(Path::new("rollup.config.js"), source, Path::new("/project"));
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@rollup/plugin-node-resolve".to_string()));
        assert!(deps.contains(&"@rollup/plugin-commonjs".to_string()));
    }

    #[test]
    fn resolve_config_empty() {
        let source = r"export default {};";
        let plugin = RollupPlugin;
        let result =
            plugin.resolve_config(Path::new("rollup.config.js"), source, Path::new("/project"));
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
        let plugin = RollupPlugin;
        let result =
            plugin.resolve_config(Path::new("rollup.config.js"), source, Path::new("/project"));
        assert!(result.entry_patterns.is_empty());
    }

    /// A bare `input` value is either a module request or a path that rollup
    /// resolves against the working directory, so it credits the package and
    /// keeps the entry pattern (issue #2753).
    #[test]
    fn a_bare_input_credits_the_package_and_keeps_the_entry_pattern() {
        let source = r#"export default { input: ["my-lib/client", "src/app", "./src/main.js"] };"#;
        let result = RollupPlugin.resolve_config(
            Path::new("rollup.config.js"),
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
