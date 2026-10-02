//! Tsup TypeScript library bundler plugin.
//!
//! Detects Tsup projects and marks config files as always used.
//! Parses tsup config to extract referenced dependencies.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["tsup"];

const CONFIG_PATTERNS: &[&str] = &["tsup.config.{ts,js,cjs,mjs}"];

const ALWAYS_USED: &[&str] = &["tsup.config.{ts,js,cjs,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["tsup"];

define_plugin! {
    struct TsupPlugin => "tsup",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);

        let entries = config_parser::extract_config_string_or_array(source, config_path, &["entry"]);
        result.extend_entry_patterns(entries);

        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn resolve_config_entry_array() {
        let source = r#"
            export default {
                entry: ["src/index.ts", "src/cli.ts"]
            };
        "#;
        let plugin = TsupPlugin;
        let result =
            plugin.resolve_config(Path::new("tsup.config.ts"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/index.ts", "src/cli.ts"]);
    }

    #[test]
    fn resolve_config_entry_single() {
        let source = r#"
            export default {
                entry: ["src/index.ts"]
            };
        "#;
        let plugin = TsupPlugin;
        let result =
            plugin.resolve_config(Path::new("tsup.config.ts"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/index.ts"]);
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import { defineConfig } from 'tsup';
            import react from '@vitejs/plugin-react';
            export default defineConfig({
                entry: ["src/index.ts"]
            });
        "#;
        let plugin = TsupPlugin;
        let result =
            plugin.resolve_config(Path::new("tsup.config.ts"), source, Path::new("/project"));
        assert!(result.referenced_dependencies.contains(&"tsup".to_string()));
        assert!(
            result
                .referenced_dependencies
                .contains(&"@vitejs/plugin-react".to_string())
        );
        assert_eq!(result.entry_patterns, vec!["src/index.ts"]);
    }

    #[test]
    fn resolve_config_empty() {
        let source = r"export default {};";
        let plugin = TsupPlugin;
        let result =
            plugin.resolve_config(Path::new("tsup.config.ts"), source, Path::new("/project"));
        assert!(result.entry_patterns.is_empty());
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_no_entry() {
        let source = r#"
            export default {
                format: ["cjs", "esm"]
            };
        "#;
        let plugin = TsupPlugin;
        let result =
            plugin.resolve_config(Path::new("tsup.config.ts"), source, Path::new("/project"));
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn resolve_config_define_config() {
        let source = r#"
            import { defineConfig } from 'tsup';
            export default defineConfig({
                entry: ["src/main.ts", "src/worker.ts"]
            });
        "#;
        let plugin = TsupPlugin;
        let result =
            plugin.resolve_config(Path::new("tsup.config.ts"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/main.ts", "src/worker.ts"]);
        assert!(result.referenced_dependencies.contains(&"tsup".to_string()));
    }

    #[test]
    fn resolve_config_entry_string() {
        let source = r#"
            import { defineConfig } from 'tsup';
            export default defineConfig({
                entry: "src/index.ts"
            });
        "#;
        let plugin = TsupPlugin;
        let result =
            plugin.resolve_config(Path::new("tsup.config.ts"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/index.ts"]);
    }

    #[test]
    fn resolve_config_entry_object_map() {
        let source = r#"
            import { defineConfig } from 'tsup';
            export default defineConfig({
                entry: { main: "src/main.ts", worker: "src/worker.ts" }
            });
        "#;
        let plugin = TsupPlugin;
        let result =
            plugin.resolve_config(Path::new("tsup.config.ts"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/main.ts", "src/worker.ts"]);
    }
}
