//! Oxlint plugin.
//!
//! Detects Oxlint projects and marks config files as always used.

use std::path::{Path, PathBuf};

use oxc_ast::ast::{
    Argument, ArrayExpressionElement, ArrowFunctionExpression, Expression, Function,
    ImportDeclarationSpecifier, ObjectExpression, Program, Statement, VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::Span;
use oxc_syntax::scope::ScopeFlags;

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["oxlint"];

const CONFIG_PATTERNS: &[&str] = &[".oxlintrc.json", "oxlint.json", "oxlint.config.ts"];

const ALWAYS_USED: &[&str] = CONFIG_PATTERNS;

const TOOLING_DEPENDENCIES: &[&str] = &["oxlint", "oxlint-tsgolint"];

const ULTRACITE_JS_PLUGINS_SOURCE: &str = "ultracite/oxlint/js-plugins";

const ULTRACITE_JS_PLUGIN_DEPENDENCIES: &[(&str, &str)] = &[
    ("github", "eslint-plugin-github"),
    ("sonarjs", "eslint-plugin-sonarjs"),
    ("react-doctor", "oxlint-plugin-react-doctor"),
];

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

        result.referenced_dependencies.extend(
            extract_ultracite_selected_js_plugin_dependencies(source, config_path),
        );

        let extends = config_parser::extract_config_shallow_strings(source, config_path, "extends");
        for entry in extends {
            credit_config_specifier(&mut result, config_path, root, &entry);
        }

        result
    }
}

fn extract_ultracite_selected_js_plugin_dependencies(
    source: &str,
    config_path: &Path,
) -> Vec<String> {
    config_parser::extract_from_source(source, config_path, |program| {
        let helper_binding = ultracite_selector_binding(program)?;
        let selected_plugins_binding = wired_js_plugins_binding(program)?;

        for statement in &program.body {
            let Statement::VariableDeclaration(declaration) = statement else {
                continue;
            };
            if declaration.kind != VariableDeclarationKind::Const {
                continue;
            }

            for declarator in &declaration.declarations {
                let Some(binding) = declarator.id.get_binding_identifier() else {
                    continue;
                };
                if binding.name.as_str() != selected_plugins_binding {
                    continue;
                }

                let Some(Expression::CallExpression(call)) = &declarator.init else {
                    return None;
                };
                let Expression::Identifier(callee) = &call.callee else {
                    return None;
                };
                if callee.name.as_str() != helper_binding || call.arguments.len() != 1 {
                    return None;
                }
                let Some(Argument::ArrayExpression(selection)) = call.arguments.first() else {
                    return None;
                };
                if !selection
                    .elements
                    .iter()
                    .all(|element| matches!(element, ArrayExpressionElement::StringLiteral(_)))
                {
                    return None;
                }

                let dependencies = selection
                    .elements
                    .iter()
                    .filter_map(|element| {
                        let ArrayExpressionElement::StringLiteral(plugin) = element else {
                            return None;
                        };
                        ULTRACITE_JS_PLUGIN_DEPENDENCIES
                            .iter()
                            .find_map(|(name, dependency)| {
                                (plugin.value == *name).then(|| (*dependency).to_string())
                            })
                    })
                    .collect();
                return Some(dependencies);
            }
        }

        None
    })
    .unwrap_or_default()
}

fn ultracite_selector_binding(program: &Program<'_>) -> Option<String> {
    let mut binding = None;

    for statement in &program.body {
        let Statement::ImportDeclaration(declaration) = statement else {
            continue;
        };
        if declaration.source.value != ULTRACITE_JS_PLUGINS_SOURCE
            || declaration.import_kind.is_type()
        {
            continue;
        }
        let Some(specifiers) = &declaration.specifiers else {
            continue;
        };

        for specifier in specifiers {
            let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier else {
                continue;
            };
            if specifier.import_kind.is_type() || specifier.imported.name() != "selectJsPlugins" {
                continue;
            }
            if binding.is_some() {
                return None;
            }
            binding = Some(specifier.local.name.to_string());
        }
    }

    binding
}

fn wired_js_plugins_binding(program: &Program<'_>) -> Option<String> {
    let config = config_parser::find_config_object_pub(program)?;
    if function_scoped_config_object(program, config) {
        return None;
    }
    let Expression::StaticMemberExpression(member) =
        config_parser::property_expr(config, "jsPlugins")?
    else {
        return None;
    };
    if member.property.name != "jsPlugins" {
        return None;
    }
    let Expression::Identifier(binding) = &member.object else {
        return None;
    };
    Some(binding.name.to_string())
}

fn function_scoped_config_object(program: &Program<'_>, config: &ObjectExpression<'_>) -> bool {
    let mut detector = FunctionScopeDetector {
        target: config.span,
        found: false,
    };
    detector.visit_program(program);
    detector.found
}

struct FunctionScopeDetector {
    target: Span,
    found: bool,
}

impl<'a> Visit<'a> for FunctionScopeDetector {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        if function.span.contains_inclusive(self.target) {
            self.found = true;
            return;
        }
        walk::walk_function(self, function, flags);
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        if arrow.span.contains_inclusive(self.target) {
            self.found = true;
            return;
        }
        walk::walk_arrow_function_expression(self, arrow);
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
    config_parser::lexical_normalize(&config_abs.parent().unwrap_or(root).join(specifier))
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
    fn resolve_config_ultracite_selected_js_plugins() {
        let source = r#"
            import { defineConfig } from "oxlint";
            import core from "ultracite/oxlint/core";
            import { jsPluginSettings, selectJsPlugins } from "ultracite/oxlint/js-plugins";

            const jsPlugins = selectJsPlugins(["github", "sonarjs", "react-doctor"]);

            export default defineConfig({
                extends: [core, jsPlugins],
                ignorePatterns: core.ignorePatterns,
                jsPlugins: jsPlugins.jsPlugins,
                settings: jsPluginSettings,
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-github".to_string()));
        assert!(deps.contains(&"eslint-plugin-sonarjs".to_string()));
        assert!(deps.contains(&"oxlint-plugin-react-doctor".to_string()));
    }

    #[test]
    fn resolve_config_ignores_unwired_and_unrelated_plugin_selectors() {
        let source = r#"
            import { defineConfig } from "oxlint";
            import { selectJsPlugins } from "ultracite/oxlint/js-plugins";

            const unusedSelection = selectJsPlugins(["github"]);
            const jsPlugins = chooseJsPlugins(["sonarjs"]);

            export default defineConfig({
                jsPlugins: jsPlugins.jsPlugins,
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(!deps.contains(&"eslint-plugin-github".to_string()));
        assert!(!deps.contains(&"eslint-plugin-sonarjs".to_string()));
    }

    #[test]
    fn resolve_config_requires_ultracite_selector_provenance() {
        let source = r#"
            import { defineConfig } from "oxlint";
            import { selectJsPlugins } from "another-package";

            const jsPlugins = selectJsPlugins(["github"]);

            export default defineConfig({
                jsPlugins: jsPlugins.jsPlugins,
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        assert!(
            !result
                .referenced_dependencies
                .contains(&"eslint-plugin-github".to_string())
        );
    }

    #[test]
    fn resolve_config_ignores_dynamic_ultracite_plugin_selections() {
        let source = r#"
            import { defineConfig } from "oxlint";
            import { selectJsPlugins } from "ultracite/oxlint/js-plugins";

            const extraPlugins = ["sonarjs"];
            const jsPlugins = selectJsPlugins([
                "github",
                "anti-slop",
                "unknown-plugin",
                configuredPlugin,
                ...extraPlugins,
            ]);

            export default defineConfig({
                jsPlugins: jsPlugins.jsPlugins,
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(!deps.contains(&"eslint-plugin-github".to_string()));
        assert!(!deps.contains(&"eslint-plugin-sonarjs".to_string()));
        assert!(!deps.contains(&"anti-slop".to_string()));
        assert!(!deps.contains(&"unknown-plugin".to_string()));
    }

    #[test]
    fn resolve_config_supports_an_aliased_ultracite_selector_import() {
        let source = r#"
            import { defineConfig } from "oxlint";
            import { selectJsPlugins as select } from "ultracite/oxlint/js-plugins";

            const selected = select(["github", "anti-slop", "unknown-plugin"]);

            export default defineConfig({
                jsPlugins: selected.jsPlugins,
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-github".to_string()));
        assert!(!deps.contains(&"anti-slop".to_string()));
        assert!(!deps.contains(&"unknown-plugin".to_string()));
    }

    #[test]
    fn resolve_config_requires_const_and_exact_member_wiring() {
        for source in [
            r#"
                import { defineConfig } from "oxlint";
                import { selectJsPlugins } from "ultracite/oxlint/js-plugins";
                let selected = selectJsPlugins(["github"]);
                export default defineConfig({ jsPlugins: selected.jsPlugins });
            "#,
            r#"
                import { defineConfig } from "oxlint";
                import { selectJsPlugins } from "ultracite/oxlint/js-plugins";
                const selected = selectJsPlugins(["github"]);
                export default defineConfig({ jsPlugins: selected.plugins });
            "#,
        ] {
            let plugin = OxlintPlugin;
            let result =
                plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

            assert!(
                !result
                    .referenced_dependencies
                    .contains(&"eslint-plugin-github".to_string())
            );
        }
    }

    #[test]
    fn resolve_config_rejects_function_scoped_shadowing() {
        for source in [
            r#"
                import { defineConfig } from "oxlint";
                import { selectJsPlugins } from "ultracite/oxlint/js-plugins";
                const selected = selectJsPlugins(["github"]);
                export default defineConfig((selected) => ({
                    jsPlugins: selected.jsPlugins,
                }));
            "#,
            r#"
                import { defineConfig } from "oxlint";
                import { selectJsPlugins } from "ultracite/oxlint/js-plugins";
                const selected = selectJsPlugins(["github"]);
                export default defineConfig(function (selected) {
                    return { jsPlugins: selected.jsPlugins };
                });
            "#,
        ] {
            let plugin = OxlintPlugin;
            let result =
                plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

            assert!(
                !result
                    .referenced_dependencies
                    .contains(&"eslint-plugin-github".to_string())
            );
        }
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
