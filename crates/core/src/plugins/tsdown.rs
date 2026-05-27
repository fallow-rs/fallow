//! tsdown TypeScript library bundler plugin.
//!
//! Detects tsdown projects and marks config files as always used.
//! Parses tsdown config to extract referenced dependencies.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["tsdown"];

const CONFIG_PATTERNS: &[&str] = &["tsdown.config.{ts,mts,cts,js,cjs,mjs}"];

const ALWAYS_USED: &[&str] = &["tsdown.config.{ts,mts,cts,js,cjs,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["tsdown"];

define_plugin! {
    struct TsdownPlugin => "tsdown",
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

        // entry -> source entry points for the library
        let entries = config_parser::extract_config_string_array(source, config_path, &["entry"]);
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
        let plugin = TsdownPlugin;
        let result =
            plugin.resolve_config(Path::new("tsdown.config.ts"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/index.ts", "src/cli.ts"]);
    }

    #[test]
    fn resolve_config_entry_single() {
        let source = r#"
            export default {
                entry: ["src/index.ts"]
            };
        "#;
        let plugin = TsdownPlugin;
        let result =
            plugin.resolve_config(Path::new("tsdown.config.ts"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/index.ts"]);
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import { defineConfig } from 'tsdown';
            import react from '@vitejs/plugin-react';
            export default defineConfig({
                entry: ["src/index.ts"]
            });
        "#;
        let plugin = TsdownPlugin;
        let result =
            plugin.resolve_config(Path::new("tsdown.config.ts"), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"tsdown".to_string())
        );
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
        let plugin = TsdownPlugin;
        let result =
            plugin.resolve_config(Path::new("tsdown.config.ts"), source, Path::new("/project"));
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
        let plugin = TsdownPlugin;
        let result =
            plugin.resolve_config(Path::new("tsdown.config.ts"), source, Path::new("/project"));
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn resolve_config_define_config() {
        let source = r#"
            import { defineConfig } from 'tsdown';
            export default defineConfig({
                entry: ["src/main.ts", "src/worker.ts"]
            });
        "#;
        let plugin = TsdownPlugin;
        let result =
            plugin.resolve_config(Path::new("tsdown.config.ts"), source, Path::new("/project"));
        assert_eq!(result.entry_patterns, vec!["src/main.ts", "src/worker.ts"]);
        assert!(
            result
                .referenced_dependencies
                .contains(&"tsdown".to_string())
        );
    }
}
