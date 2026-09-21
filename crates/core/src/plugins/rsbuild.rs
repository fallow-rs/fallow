//! Rsbuild bundler plugin.
//!
//! Detects Rsbuild projects and marks config files as always used.
//! Parses rsbuild config to extract entry points, plugin dependencies,
//! and import references.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["@rsbuild/core"];

const CONFIG_PATTERNS: &[&str] = &["rsbuild.config.{ts,js,mjs,cjs}"];

const ALWAYS_USED: &[&str] = &["rsbuild.config.{ts,js,mjs,cjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["@rsbuild/core"];

define_plugin! {
    struct RsbuildPlugin => "rsbuild",
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

        let entries = config_parser::extract_config_string_or_array(
            source,
            config_path,
            &["source", "entry"],
        );
        result.extend_entry_patterns_or_dependencies(entries);

        let plugin_requires =
            config_parser::extract_config_require_strings(source, config_path, "plugins");
        for dep in &plugin_requires {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        super::module_federation::apply_bundler_plugin_options(
            &mut result,
            source,
            config_path,
            root,
            None,
            "rsbuild",
        );

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_entry_pattern(result: &PluginResult, pattern: &str) -> bool {
        result
            .entry_patterns
            .iter()
            .any(|entry_pattern| entry_pattern.pattern == pattern)
    }

    #[test]
    fn resolve_config_entry_string() {
        let source = r#"
            export default {
                source: {
                    entry: "./src/index.tsx"
                }
            };
        "#;
        let plugin = RsbuildPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("rsbuild.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/index.tsx"]);
    }

    #[test]
    fn resolve_config_entry_object() {
        let source = r#"
            export default {
                source: {
                    entry: {
                        main: "./src/main.tsx",
                        admin: "./src/admin.tsx"
                    }
                }
            };
        "#;
        let plugin = RsbuildPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("rsbuild.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(has_entry_pattern(&result, "src/main.tsx"));
        assert!(has_entry_pattern(&result, "src/admin.tsx"));
    }

    #[test]
    fn resolve_config_entry_module_request_is_credited_as_dependency() {
        let source = r#"
            export default {
                source: {
                    entry: {
                        index: ["react-hot-loader/patch", "./src/index.tsx"],
                    },
                },
            };
        "#;
        let plugin = RsbuildPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/rsbuild.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/index.tsx"]);
        assert!(
            result
                .referenced_dependencies
                .contains(&"react-hot-loader".to_string())
        );
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import { defineConfig } from '@rsbuild/core';
            import { pluginReact } from '@rsbuild/plugin-react';
            export default defineConfig({
                plugins: [pluginReact()],
                source: {
                    entry: "./src/index.tsx"
                }
            });
        "#;
        let plugin = RsbuildPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("rsbuild.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@rsbuild/core".to_string()));
        assert!(deps.contains(&"@rsbuild/plugin-react".to_string()));
        assert_eq!(result.entry_patterns, vec!["src/index.tsx"]);
    }

    #[test]
    fn resolve_config_define_config() {
        let source = r#"
            import { defineConfig } from '@rsbuild/core';
            export default defineConfig({
                source: {
                    entry: {
                        index: "./src/index.ts"
                    }
                }
            });
        "#;
        let plugin = RsbuildPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("rsbuild.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/index.ts"]);
    }

    #[test]
    fn resolve_config_reads_inline_module_federation_options() {
        let source = r#"
            import { defineConfig } from "@rsbuild/core";
            import { pluginModuleFederation } from "@module-federation/rsbuild-plugin";

            export default defineConfig({
                plugins: [
                    pluginModuleFederation({
                        name: "host",
                        remotes: { checkout: "checkout@https://example.test/remoteEntry.js" },
                        shared: ["react"],
                    }),
                ],
            });
        "#;
        let plugin = RsbuildPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/rsbuild.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(result.provided_dependencies.len(), 1);
        assert!(result.provided_dependencies[0].covers_specifier("checkout"));
    }
}
