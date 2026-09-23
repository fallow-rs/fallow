//! Rspack bundler plugin.
//!
//! Detects Rspack projects and marks config files as entry points.
//! Parses rspack.config to extract entry points, loader dependencies,
//! and plugin packages -- using the same webpack-compatible config format.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["@rspack/core", "@rspack/cli"];

const ENTRY_PATTERNS: &[&str] = &["src/index.{ts,tsx,js,jsx}"];

const CONFIG_PATTERNS: &[&str] = &[
    "rspack.config.{ts,js,mjs,cjs}",
    "rspack.*.config.{ts,js,mjs,cjs}",
];

const ALWAYS_USED: &[&str] = &[
    "rspack.config.{ts,js,mjs,cjs}",
    "rspack.*.config.{ts,js,mjs,cjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "@rspack/core",
    "@rspack/cli",
    "@rspack/dev-server",
    "@rspack/plugin-react-refresh",
    "@rspack/plugin-minify",
    "@rspack/plugin-html",
];

define_plugin! {
    struct RspackPlugin => "rspack",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
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

        let base = super::webpack::apply_entries(
            &mut result,
            source,
            super::webpack::ConfigFile { path: config_path, root },
            &["entry"],
            "context",
        );

        let require_deps =
            config_parser::extract_config_require_strings(source, config_path, "plugins");
        for dep in &require_deps {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        let externals =
            config_parser::extract_config_shallow_strings(source, config_path, "externals");
        for ext in &externals {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(ext));
        }

        super::webpack::parse_webpack_loaders(source, config_path, &mut result);

        super::module_federation::apply_bundler_plugin_options(
            &mut result,
            source,
            config_path,
            root,
            super::module_federation::FederationBase {
                context: base.as_deref(),
                package_dir: None,
            },
            "rspack",
        );

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_entry_string() {
        let source = r#"module.exports = { entry: "./src/app.tsx" };"#;
        let plugin = RspackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("rspack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/app.tsx"]);
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import { defineConfig } from '@rspack/cli';
            export default defineConfig({ entry: "./src/main.ts" });
        "#;
        let plugin = RspackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("rspack.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@rspack/cli".to_string())
        );
        assert_eq!(result.entry_patterns, vec!["src/main.ts"]);
    }

    #[test]
    fn resolve_config_entry_module_request_is_credited_as_dependency() {
        let source = r#"
            module.exports = {
                entry: ["react-hot-loader/patch", "./src/index.tsx"],
            };
        "#;
        let plugin = RspackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/rspack.config.js"),
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
    fn resolve_config_loaders() {
        let source = r"
            module.exports = {
                module: {
                    rules: [
                        { test: /\.css$/, use: ['style-loader', 'css-loader'] },
                        { test: /\.svg$/, loader: 'svgr-loader' },
                    ]
                }
            };
        ";
        let plugin = RspackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("rspack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"style-loader".to_string()));
        assert!(deps.contains(&"css-loader".to_string()));
        assert!(deps.contains(&"svgr-loader".to_string()));
    }

    #[test]
    fn resolve_config_reads_inline_module_federation_options() {
        let source = r#"
            const { ModuleFederationPlugin } = require("@module-federation/enhanced/rspack");

            module.exports = {
                plugins: [
                    new ModuleFederationPlugin({
                        name: "checkout",
                        exposes: { "./Button": "./src/Button.tsx" },
                    }),
                ],
            };
        "#;
        let plugin = RspackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/rspack.config.js"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(result.entry_patterns, vec!["src/Button.tsx"]);
    }

    #[test]
    fn resolve_config_context_roots_relative_entries_and_exposes() {
        let source = r#"
            const path = require("path");
            module.exports = {
                context: path.resolve(__dirname, "src"),
                entry: "./app.ts",
                plugins: [
                    new ModuleFederationPlugin({ exposes: { "./B": "./B.tsx" } }),
                ],
            };
        "#;
        let result = RspackPlugin.resolve_config(
            std::path::Path::new("/project/rspack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/app.ts", "src/B.tsx"]);
    }
}
