//! TypeScript plugin.
//!
//! Detects TypeScript projects and parses `tsconfig.json` for references,
//! extended configs, type packages, language service plugins, and array extends.
#![expect(
    clippy::excessive_nesting,
    reason = "tsconfig AST parsing requires deep nesting"
)]

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

define_plugin!(
    struct TypeScriptPlugin => "typescript",
    enablers: &["typescript"],
    config_patterns: &["tsconfig.json", "tsconfig.*.json"],
    always_used: &["tsconfig.json", "tsconfig.*.json"],
    tooling_dependencies: &["typescript", "ts-node", "tsx", "ts-loader"],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let is_json = config_path.extension().is_some_and(|ext| ext == "json");
        let (parse_source, parse_path_buf) = if is_json {
            (format!("({source})"), config_path.with_extension("js"))
        } else {
            (source.to_string(), config_path.to_path_buf())
        };
        let parse_path: &Path = &parse_path_buf;

        if let Some(extends) =
            config_parser::extract_config_string(&parse_source, parse_path, &["extends"])
        {
            if extends.starts_with('.') || extends.starts_with('/') {
                result
                    .setup_files
                    .push(root.join(extends.trim_start_matches("./")));
            } else {
                let dep = crate::resolve::extract_package_name(&extends);
                result.referenced_dependencies.push(dep);
            }
        }

        let extends_arr =
            config_parser::extract_config_string_array(&parse_source, parse_path, &["extends"]);
        for ext in &extends_arr {
            if ext.starts_with('.') || ext.starts_with('/') {
                result
                    .setup_files
                    .push(root.join(ext.trim_start_matches("./")));
            } else {
                let dep = crate::resolve::extract_package_name(ext);
                result.referenced_dependencies.push(dep);
            }
        }

        let types = config_parser::extract_config_string_array(
            &parse_source,
            parse_path,
            &["compilerOptions", "types"],
        );
        for ty in &types {
            let base = crate::resolve::extract_package_name(ty);
            if !base.starts_with('@') {
                result
                    .referenced_dependencies
                    .push(format!("@types/{base}"));
            }
            result.referenced_dependencies.push(base);
        }

        if let Some(jsx_source) = config_parser::extract_config_string(
            &parse_source,
            parse_path,
            &["compilerOptions", "jsxImportSource"],
        ) {
            result.referenced_dependencies.push(jsx_source);
        }

        for (find, replacement) in config_parser::extract_config_path_aliases(
            &parse_source,
            parse_path,
            &["compilerOptions", "paths"],
        ) {
            let Some((normalized_find, normalized_replacement)) =
                normalize_tsconfig_path_alias(&find, &replacement, parse_path, root)
            else {
                continue;
            };
            result
                .path_aliases
                .push((normalized_find, normalized_replacement));
        }

        parse_tsconfig_plugins(&parse_source, parse_path, &mut result);

        parse_tsconfig_references(&parse_source, parse_path, root, &mut result);

        result
    },
);

fn normalize_tsconfig_path_alias(
    find: &str,
    replacement: &Path,
    config_path: &Path,
    root: &Path,
) -> Option<(String, String)> {
    let normalized_find = find.strip_suffix('*').unwrap_or(find).to_string();
    if normalized_find.is_empty() {
        return None;
    }
    let replacement = config_parser::path_to_config_string(replacement);
    let normalized_replacement = replacement
        .strip_suffix("/*")
        .or_else(|| replacement.strip_suffix('*'))
        .unwrap_or(&replacement);
    let normalized_replacement =
        config_parser::normalize_config_path(normalized_replacement, config_path, root)?;

    Some((normalized_find, normalized_replacement))
}

/// Extract `compilerOptions.plugins[].name` from a tsconfig as referenced dependencies.
fn parse_tsconfig_plugins(source: &str, path: &Path, result: &mut PluginResult) {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::{Expression, ObjectPropertyKind, PropertyKey};
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let source_type = SourceType::from_path(path).unwrap_or_default();
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, source, source_type).parse();

    let Some(obj) = config_parser::find_config_object_pub(&parsed.program) else {
        return;
    };

    let compiler_opts = obj.properties.iter().find_map(|prop| {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let is_compiler_opts = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name == "compilerOptions",
                PropertyKey::StringLiteral(s) => s.value == "compilerOptions",
                _ => false,
            };
            if is_compiler_opts && let Expression::ObjectExpression(obj) = &p.value {
                return Some(obj);
            }
        }
        None
    });
    let Some(compiler_opts) = compiler_opts else {
        return;
    };

    let plugins_arr = compiler_opts.properties.iter().find_map(|prop| {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let is_plugins = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name == "plugins",
                PropertyKey::StringLiteral(s) => s.value == "plugins",
                _ => false,
            };
            if is_plugins && let Expression::ArrayExpression(arr) = &p.value {
                return Some(arr);
            }
        }
        None
    });
    let Some(plugins_arr) = plugins_arr else {
        return;
    };

    for el in &plugins_arr.elements {
        if let Some(Expression::ObjectExpression(plugin_obj)) = el.as_expression() {
            for prop in &plugin_obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    let is_name = match &p.key {
                        PropertyKey::StaticIdentifier(id) => id.name == "name",
                        PropertyKey::StringLiteral(s) => s.value == "name",
                        _ => false,
                    };
                    if is_name && let Expression::StringLiteral(s) = &p.value {
                        let dep = crate::resolve::extract_package_name(&s.value);
                        result.referenced_dependencies.push(dep);
                    }
                }
            }
        }
    }
}

/// Extract `references[].path` from a tsconfig and add them as setup files.
fn parse_tsconfig_references(source: &str, path: &Path, root: &Path, result: &mut PluginResult) {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::{Expression, ObjectPropertyKind, PropertyKey};
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let source_type = SourceType::from_path(path).unwrap_or_default();
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, source, source_type).parse();

    let Some(obj) = config_parser::find_config_object_pub(&parsed.program) else {
        return;
    };

    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let is_references = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name == "references",
                PropertyKey::StringLiteral(s) => s.value == "references",
                _ => false,
            };
            if !is_references {
                continue;
            }
            if let Expression::ArrayExpression(arr) = &p.value {
                for el in &arr.elements {
                    if let Some(Expression::ObjectExpression(ref_obj)) = el.as_expression() {
                        for ref_prop in &ref_obj.properties {
                            if let ObjectPropertyKind::ObjectProperty(rp) = ref_prop {
                                let is_path = match &rp.key {
                                    PropertyKey::StaticIdentifier(id) => id.name == "path",
                                    PropertyKey::StringLiteral(s) => s.value == "path",
                                    _ => false,
                                };
                                if is_path && let Expression::StringLiteral(s) = &rp.value {
                                    let ref_path = s.value.to_string();
                                    let ref_target = root.join(ref_path.trim_start_matches("./"));
                                    let tsconfig_path = if ref_target
                                        .extension()
                                        .is_some_and(|ext| ext == "json")
                                    {
                                        ref_target
                                    } else {
                                        ref_target.join("tsconfig.json")
                                    };
                                    result.setup_files.push(tsconfig_path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_extends_package() {
        let source = r#"{"extends": "@tsconfig/node18/tsconfig.json"}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@tsconfig/node18".to_string())
        );
    }

    #[test]
    fn resolve_config_extends_relative_path() {
        let source = r#"{"extends": "./tsconfig.base.json"}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(result.referenced_dependencies.is_empty());
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/tsconfig.base.json"))
        );
    }

    #[test]
    fn resolve_config_extends_array() {
        let source = r#"{"extends": ["./tsconfig.base.json", "@tsconfig/node18/tsconfig.json"]}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/tsconfig.base.json"))
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@tsconfig/node18".to_string())
        );
    }

    #[test]
    fn resolve_config_compiler_options_types() {
        let source = r#"{"compilerOptions": {"types": ["node", "jest"]}}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@types/node".to_string()));
        assert!(deps.contains(&"node".to_string()));
        assert!(deps.contains(&"@types/jest".to_string()));
        assert!(deps.contains(&"jest".to_string()));
    }

    #[test]
    fn resolve_config_jsx_import_source() {
        let source = r#"{"compilerOptions": {"jsxImportSource": "react"}}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"react".to_string())
        );
    }

    #[test]
    fn resolve_config_extracts_path_aliases_from_paths() {
        let source = r#"{
            "compilerOptions": {
                "paths": {
                    "@/*": ["./src/*"],
                    "@shared/*": ["./shared/*", "./fallback/*"]
                }
            }
        }"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/tsconfig.app.json"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(
            result.path_aliases,
            vec![
                ("@/".to_string(), "src".to_string()),
                ("@shared/".to_string(), "shared".to_string())
            ]
        );
    }

    #[test]
    fn resolve_config_drops_wildcard_only_path_alias() {
        let source = r#"{
            "compilerOptions": {
                "paths": {
                    "*": ["./src/*"],
                    "@/*": ["./src/*"]
                }
            }
        }"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(
            result.path_aliases,
            vec![("@/".to_string(), "src".to_string())],
        );
    }

    #[test]
    fn resolve_config_compiler_options_plugins() {
        let source =
            r#"{"compilerOptions": {"plugins": [{"name": "typescript-plugin-css-modules"}]}}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"typescript-plugin-css-modules".to_string())
        );
    }

    #[test]
    fn resolve_config_references() {
        let source = r#"{"references": [{"path": "./packages/core"}, {"path": "./packages/ui"}]}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(result.setup_files.contains(&std::path::PathBuf::from(
            "/project/packages/core/tsconfig.json"
        )));
        assert!(result.setup_files.contains(&std::path::PathBuf::from(
            "/project/packages/ui/tsconfig.json"
        )));
    }

    #[test]
    fn resolve_config_references_accept_direct_tsconfig_files() {
        let source = r#"{
            "references": [
                {"path": "./tsconfig.app.json"},
                {"path": "./packages/ui"}
            ]
        }"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/tsconfig.app.json"))
        );
        assert!(result.setup_files.contains(&std::path::PathBuf::from(
            "/project/packages/ui/tsconfig.json"
        )));
    }

    #[test]
    fn resolve_config_with_comments_and_trailing_commas() {
        let source = r#"{
            // Base config for all packages
            "extends": "@tsconfig/strictest",
            "compilerOptions": {
                "types": ["node"],
            },
        }"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@tsconfig/strictest".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@types/node".to_string())
        );
    }
}
