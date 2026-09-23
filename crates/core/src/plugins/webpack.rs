//! Webpack bundler plugin.
//!
//! Detects Webpack projects and marks conventional entry points and config files.
//! Parses webpack config to extract entry points, plugin dependencies, loader
//! packages from module.rules, and external dependencies.

use std::path::{Path, PathBuf};

use super::config_parser;
use super::{Plugin, PluginResult};

/// Webpack config files. A root name matches at any depth. A project that keeps
/// one config per target commonly puts them under `config/` with the target in
/// the name, such as `config/webpack.client.js`.
const CONFIG_PATTERNS: &[&str] = &[
    "webpack.config.{ts,js,mjs,cjs}",
    "webpack.*.config.{ts,js,mjs,cjs}",
    "config/webpack.*.{ts,js,mjs,cjs}",
];

define_plugin!(
    struct WebpackPlugin => "webpack",
    enablers: &["webpack"],
    entry_patterns: &["src/index.{ts,tsx,js,jsx}"],
    config_patterns: CONFIG_PATTERNS,
    always_used: CONFIG_PATTERNS,
    tooling_dependencies: &[
        "webpack",
        "webpack-cli",
        "webpack-dev-server",
        "html-webpack-plugin",
    ],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        let context = apply_entries(
            &mut result,
            source,
            ConfigFile { path: config_path, root },
            &["entry"],
            "context",
        );

        super::module_federation::apply_bundler_plugin_options(
            &mut result,
            source,
            config_path,
            root,
            context.as_deref(),
            "webpack",
        );

        for (find, replacement) in
            config_parser::extract_config_path_aliases(source, config_path, &["resolve", "alias"])
        {
            if let Some(normalized) =
                config_parser::normalize_config_path(&replacement, config_path, root)
            {
                result.path_aliases.push((find, normalized));
            }
        }

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

        parse_webpack_loaders(source, config_path, &mut result);

        result
    },
);

/// Extract loader package names from webpack `module.rules` config.
///
/// Handles common patterns:
/// - `{ loader: 'ts-loader' }`
/// - `{ use: ['style-loader', 'css-loader'] }`
/// - `{ use: [{ loader: 'css-loader', options: {} }] }`
/// - `{ oneOf: [...rules] }`
pub(super) fn parse_webpack_loaders(source: &str, path: &Path, result: &mut PluginResult) {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::Expression;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let source_type = SourceType::from_path(path).unwrap_or_default();
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, source, source_type).parse();

    let Some(obj) = config_parser::find_config_object(&parsed.program) else {
        return;
    };

    let Some(module_prop) = find_obj_prop(obj, "module") else {
        return;
    };
    let Expression::ObjectExpression(module_obj) = &module_prop.value else {
        return;
    };
    let Some(rules_prop) = find_obj_prop(module_obj, "rules") else {
        return;
    };
    let Expression::ArrayExpression(rules_arr) = &rules_prop.value else {
        return;
    };

    walk_rules(rules_arr, result);
}

fn find_obj_prop<'a>(
    obj: &'a oxc_ast::ast::ObjectExpression<'a>,
    key: &str,
) -> Option<&'a oxc_ast::ast::ObjectProperty<'a>> {
    use oxc_ast::ast::{ObjectPropertyKind, PropertyKey};
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let is_match = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name == key,
                PropertyKey::StringLiteral(s) => s.value == key,
                _ => false,
            };
            if is_match {
                return Some(p);
            }
        }
    }
    None
}

fn walk_rules(rules: &oxc_ast::ast::ArrayExpression, result: &mut PluginResult) {
    use oxc_ast::ast::Expression;
    for el in &rules.elements {
        if let Some(Expression::ObjectExpression(rule_obj)) = el.as_expression() {
            walk_rule(rule_obj, result);
        }
    }
}

fn walk_rule(rule: &oxc_ast::ast::ObjectExpression, result: &mut PluginResult) {
    use oxc_ast::ast::{Expression, ObjectPropertyKind, PropertyKey};

    for prop in &rule.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let key_name = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
            PropertyKey::StringLiteral(s) => s.value.as_str(),
            _ => continue,
        };

        match key_name {
            "loader" => {
                if let Expression::StringLiteral(s) = &p.value {
                    let dep = crate::resolve::extract_package_name(&s.value);
                    result.referenced_dependencies.push(dep);
                }
            }
            "use" => match &p.value {
                Expression::StringLiteral(s) => {
                    let dep = crate::resolve::extract_package_name(&s.value);
                    result.referenced_dependencies.push(dep);
                }
                Expression::ArrayExpression(arr) => {
                    for use_el in &arr.elements {
                        if let Some(expr) = use_el.as_expression() {
                            match expr {
                                Expression::StringLiteral(s) => {
                                    let dep = crate::resolve::extract_package_name(&s.value);
                                    result.referenced_dependencies.push(dep);
                                }
                                Expression::ObjectExpression(use_obj) => {
                                    if let Some(loader_prop) = find_obj_prop(use_obj, "loader")
                                        && let Expression::StringLiteral(s) = &loader_prop.value
                                    {
                                        let dep = crate::resolve::extract_package_name(&s.value);
                                        result.referenced_dependencies.push(dep);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            },
            "oneOf" => {
                if let Expression::ArrayExpression(one_of) = &p.value {
                    walk_rules(one_of, result);
                }
            }
            _ => {}
        }
    }
}

/// A config file and the project root it sits under.
#[derive(Clone, Copy)]
pub(super) struct ConfigFile<'a> {
    pub path: &'a Path,
    pub root: &'a Path,
}

/// Register the entries of a webpack-compatible config, resolved against the
/// base directory option the config declares.
///
/// Webpack and rspack name the base directory `context`, and rsbuild names it
/// `root`. A relative entry resolves against it, not against the config file.
/// Returns the project-relative base directory, so the Module Federation reader
/// can resolve `exposes` targets the same way.
pub(super) fn apply_entries(
    result: &mut PluginResult,
    source: &str,
    config: ConfigFile<'_>,
    entry_key: &[&str],
    base_key: &str,
) -> Option<PathBuf> {
    let ConfigFile { path, root } = config;
    let entries = config_parser::extract_config_string_or_array(source, path, entry_key);
    let base = config_parser::extract_config_path(source, path, &[base_key])
        .and_then(|raw| config_parser::normalize_config_path_buf(&raw, path, root));
    result.extend_entry_patterns_or_dependencies(entries.into_iter().map(|entry| {
        base.as_ref()
            .map(|base| normalize_context_entry(&entry, base, path, root))
            .unwrap_or(entry)
    }));
    base
}

fn normalize_context_entry(entry: &str, context: &Path, config_path: &Path, root: &Path) -> String {
    let entry_path = config_parser::path_from_config_string(entry);
    if entry.starts_with('/') || entry_path.is_absolute() {
        return config_parser::normalize_config_path(entry, config_path, root)
            .unwrap_or_else(|| entry.to_string());
    }

    if entry.starts_with("./")
        || entry.starts_with("../")
        || entry.starts_with(".\\")
        || entry.starts_with("..\\")
    {
        return normalize_project_relative_join(context, &entry_path);
    }

    entry.to_string()
}

fn normalize_project_relative_join(base: &Path, child: &Path) -> String {
    let normalized = config_parser::lexical_normalize(&base.join(child));
    config_parser::path_to_config_string(&normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_entry_string() {
        let source = r#"module.exports = { entry: "./src/app.js" };"#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/app.js"]);
    }

    #[test]
    fn resolve_config_entry_descriptor() {
        let source = r#"
            module.exports = {
                entry: {
                    app: { import: "./src/app.js", filename: "pages/app.js" },
                    admin: { import: ["./src/admin-polyfill.js", "./src/admin.js"] },
                    shared: ["react", "react-dom"],
                },
            };
        "#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(
            result.entry_patterns,
            vec!["src/app.js", "src/admin-polyfill.js", "src/admin.js"]
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"react-dom".to_string()));
    }

    #[test]
    fn resolve_config_entry_module_request_is_credited_as_dependency() {
        let source = r#"
            module.exports = {
                entry: ["react-hot-loader/patch", "./src/index.tsx"],
            };
        "#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
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
    fn resolve_config_entry_module_request_keeps_its_resource_query_out_of_the_package() {
        let source = r#"
            module.exports = {
                entry: [
                    "webpack-hot-middleware/client?reload=true",
                    "react-hot-loader/patch",
                    "./src/index.ts",
                ],
            };
        "#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/index.ts"]);
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"webpack-hot-middleware".to_string()));
        assert!(deps.contains(&"react-hot-loader".to_string()));
    }

    #[test]
    fn resolve_config_keeps_glob_shaped_entries_as_patterns() {
        let source = r#"
            module.exports = {
                entry: [
                    "src/glob-like/**",
                    "src/pages/*.entry.ts",
                    "src/{a,b}/main",
                    "src/pag?.ts",
                    "src/pag?/main",
                ],
            };
        "#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(
            result.entry_patterns,
            vec![
                "src/glob-like/**",
                "src/pages/*.entry.ts",
                "src/{a,b}/main",
                "src/pag?.ts",
                "src/pag?/main",
            ]
        );
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_keeps_relative_and_absolute_entries_as_patterns() {
        let source = r#"
            module.exports = {
                entry: ["./src/relative.js", "src/bare.js", "/src/absolute.js"],
            };
        "#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(
            result.entry_patterns,
            vec!["src/relative.js", "src/bare.js", "/src/absolute.js"]
        );
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_context_roots_relative_entries() {
        let source = r#"
            const path = require("path");

            module.exports = {
                context: path.resolve(__dirname, "app"),
                entry: {
                    main: { import: "./main.ts" },
                    admin: ["./admin-polyfill.ts", "./admin.ts"],
                    shared: ["react", "react-dom"],
                },
            };
        "#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(
            result.entry_patterns,
            vec!["app/main.ts", "app/admin-polyfill.ts", "app/admin.ts"]
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"react-dom".to_string()));
    }

    #[test]
    fn resolve_config_context_normalizes_mixed_separator_entries() {
        let source = r#"
            module.exports = {
                context: "./app/features",
                entry: {
                    main: ".\\dashboard\\main.ts",
                    shared: "../shared/index.ts",
                },
            };
        "#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(
            result.entry_patterns,
            vec!["app/features/dashboard/main.ts", "app/shared/index.ts"]
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .all(|entry| !entry.contains('\\')),
            "entry patterns should use forward slashes: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn resolve_config_loaders() {
        let source = r"
            module.exports = {
                module: {
                    rules: [
                        { test: /\.tsx?$/, loader: 'ts-loader' },
                        { test: /\.css$/, use: ['style-loader', 'css-loader'] },
                        { test: /\.scss$/, use: [
                            'style-loader',
                            { loader: 'css-loader', options: { modules: true } },
                            'sass-loader'
                        ]}
                    ]
                }
            };
        ";
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"ts-loader".to_string()));
        assert!(deps.contains(&"style-loader".to_string()));
        assert!(deps.contains(&"css-loader".to_string()));
        assert!(deps.contains(&"sass-loader".to_string()));
    }

    #[test]
    fn resolve_config_one_of_loaders() {
        let source = r"
            module.exports = {
                module: {
                    rules: [
                        { oneOf: [
                            { test: /\.svg$/, loader: 'svg-loader' },
                            { test: /\.png$/, use: 'file-loader' }
                        ]}
                    ]
                }
            };
        ";
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"svg-loader".to_string()));
        assert!(deps.contains(&"file-loader".to_string()));
    }

    #[test]
    fn resolve_config_externals() {
        let source = r#"module.exports = { externals: ["react", "react-dom"] };"#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"react-dom".to_string()));
    }

    #[test]
    fn resolve_config_extracts_cjs_path_aliases() {
        let source = r"
            const path = require('path');

            module.exports = {
                resolve: {
                    alias: {
                        '@components': path.resolve(__dirname, 'src/components'),
                        '@utils': path.join(__dirname, 'src/utils'),
                    },
                },
            };
        ";
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(
            result.path_aliases,
            vec![
                ("@components".to_string(), "src/components".to_string()),
                ("@utils".to_string(), "src/utils".to_string()),
            ]
        );
    }

    #[test]
    fn resolve_config_extracts_esm_string_aliases() {
        let source = r#"
            export default {
                resolve: {
                    alias: {
                        "@components": "./src/components",
                        "@utils": "src/utils",
                    },
                },
            };
        "#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.mjs"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(
            result.path_aliases,
            vec![
                ("@components".to_string(), "src/components".to_string()),
                ("@utils".to_string(), "src/utils".to_string()),
            ]
        );
    }

    #[test]
    fn resolve_config_reads_inline_module_federation_options() {
        let source = r#"
            const { ModuleFederationPlugin } = require("webpack").container;

            module.exports = {
                plugins: [
                    new ModuleFederationPlugin({
                        name: "host",
                        exposes: { "./Button": "./src/Button.tsx" },
                        remotes: { checkout: "checkout@https://example.test/remoteEntry.js" },
                    }),
                ],
            };
        "#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(result.entry_patterns, vec!["src/Button.tsx"]);
        assert_eq!(result.provided_dependencies.len(), 1);
        assert!(result.provided_dependencies[0].covers_specifier("checkout/Button"));
    }

    #[test]
    fn resolve_config_context_roots_exposed_federation_targets() {
        let source = r#"
            const path = require("path");

            module.exports = {
                context: path.resolve(__dirname, "app"),
                plugins: [
                    new ModuleFederationPlugin({
                        exposes: { "./B": "./src/B.tsx" },
                    }),
                ],
            };
        "#;
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(result.entry_patterns, vec!["app/src/B.tsx"]);
    }

    #[test]
    fn extensionless_entry_covers_the_file_and_the_directory_index() {
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            r#"module.exports = { entry: { lib: "./lib/", app: "./src/app" } };"#,
            std::path::Path::new("/project"),
        );
        let exts = super::super::REQUEST_EXTENSIONS;
        assert_eq!(
            result.entry_patterns,
            vec![
                "lib/".to_string(),
                format!("lib.{exts}"),
                format!("lib/index.{exts}"),
                "src/app".to_string(),
                format!("src/app.{exts}"),
                format!("src/app/index.{exts}"),
            ]
        );
    }

    #[test]
    fn a_config_under_the_config_directory_is_a_webpack_config() {
        let matchers: Vec<globset::GlobMatcher> = CONFIG_PATTERNS
            .iter()
            .map(|pattern| {
                globset::Glob::new(pattern)
                    .expect("config pattern compiles")
                    .compile_matcher()
            })
            .collect();
        for path in ["config/webpack.client.js", "config/webpack.server.ts"] {
            assert!(
                matchers.iter().any(|matcher| matcher.is_match(path)),
                "{path} is a webpack config"
            );
        }
        assert!(
            !matchers
                .iter()
                .any(|matcher| matcher.is_match("config/webpack-helpers.js")),
            "a file without the `webpack.` prefix is not a config"
        );
    }
}
