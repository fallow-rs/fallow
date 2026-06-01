//! Webpack bundler plugin.
//!
//! Detects Webpack projects and marks conventional entry points and config files.
//! Parses webpack config to extract entry points, plugin dependencies, loader
//! packages from module.rules, and external dependencies.

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

define_plugin!(
    struct WebpackPlugin => "webpack",
    enablers: &["webpack"],
    entry_patterns: &["src/index.{ts,tsx,js,jsx}"],
    config_patterns: &[
        "webpack.config.{ts,js,mjs,cjs}",
        "webpack.*.config.{ts,js,mjs,cjs}",
    ],
    always_used: &[
        "webpack.config.{ts,js,mjs,cjs}",
        "webpack.*.config.{ts,js,mjs,cjs}",
    ],
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

        let entries =
            config_parser::extract_config_string_or_array(source, config_path, &["entry"]);
        let context = config_parser::extract_config_path(source, config_path, &["context"])
            .and_then(|raw| config_parser::normalize_config_path_buf(&raw, config_path, root));
        result.extend_entry_patterns(entries.into_iter().map(|entry| {
            context
                .as_ref()
                .map(|context| normalize_context_entry(&entry, context, config_path, root))
                .unwrap_or(entry)
        }));

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

    let Some(obj) = config_parser::find_config_object_pub(&parsed.program) else {
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
            vec![
                "src/app.js",
                "src/admin-polyfill.js",
                "src/admin.js",
                "react",
                "react-dom",
            ]
        );
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
            vec![
                "app/main.ts",
                "app/admin-polyfill.ts",
                "app/admin.ts",
                "react",
                "react-dom",
            ]
        );
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
}
