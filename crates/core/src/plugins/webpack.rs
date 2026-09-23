//! Webpack bundler plugin.
//!
//! Detects Webpack projects and marks conventional entry points and config files.
//! Parses webpack config to extract entry points, plugin dependencies, loader
//! packages from module.rules, and external dependencies.

use std::path::{Path, PathBuf};

use oxc_ast::ast::{BindingPattern, Expression, ImportDeclarationSpecifier, Program, Statement};

use super::config_parser;
use super::{Plugin, PluginResult};

/// Webpack config file names. A root name matches at any depth.
const ROOT_CONFIG_PATTERNS: &[&str] = &[
    "webpack.config.{ts,js,mjs,cjs}",
    "webpack.*.config.{ts,js,mjs,cjs}",
];

/// Webpack config files. A project that keeps one config per target commonly
/// puts them in a config directory with the target in the name, such as
/// `config/webpack.client.js`. The same directory often holds helper modules,
/// such as `config/webpack.paths.js`, so a file there counts as a config only
/// when [`exports_webpack_config`] accepts it.
const CONFIG_PATTERNS: &[&str] = &[
    "webpack.config.{ts,js,mjs,cjs}",
    "webpack.*.config.{ts,js,mjs,cjs}",
    "config/webpack.*.{ts,js,mjs,cjs}",
    "build/webpack.*.{ts,js,mjs,cjs}",
    "webpack/webpack.*.{ts,js,mjs,cjs}",
];

/// Directories that hold webpack configs one level below the package root.
const CONFIG_DIRECTORIES: &[&str] = &["config", "build", "webpack"];

/// Top-level keys of a webpack configuration object. A helper module exports
/// none of them.
const CONFIG_KEYS: &[&str] = &[
    "amd",
    "bail",
    "cache",
    "context",
    "devServer",
    "devtool",
    "entry",
    "experiments",
    "externals",
    "externalsPresets",
    "externalsType",
    "ignoreWarnings",
    "infrastructureLogging",
    "loader",
    "mode",
    "module",
    "node",
    "optimization",
    "output",
    "parallelism",
    "performance",
    "plugins",
    "profile",
    "recordsPath",
    "resolve",
    "resolveLoader",
    "snapshot",
    "stats",
    "target",
    "watch",
    "watchOptions",
];

define_plugin!(
    struct WebpackPlugin => "webpack",
    enablers: &["webpack"],
    entry_patterns: &["src/index.{ts,tsx,js,jsx}"],
    config_patterns: CONFIG_PATTERNS,
    always_used: ROOT_CONFIG_PATTERNS,
    tooling_dependencies: &[
        "webpack",
        "webpack-cli",
        "webpack-dev-server",
        "html-webpack-plugin",
    ],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let package_dir = config_directory_package(config_path, root);
        if package_dir.is_some() && !accept_directory_config(&mut result, config_path, source, root) {
            return result;
        }

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
            super::module_federation::FederationBase {
                context: context.as_deref(),
                package_dir: package_dir.as_deref(),
            },
            "webpack",
        );

        push_path_aliases(&mut result, source, config_path, root);

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
        .and_then(|raw| config_parser::normalize_filesystem_config_path_buf(&raw, path, root));
    result.extend_entry_patterns_or_dependencies(entries, |entry| {
        base.as_ref()
            .map(|base| normalize_context_entry(&entry, base, path, root))
            .unwrap_or(entry)
    });
    base
}

/// The project-relative package directory of a config that sits in a config
/// directory, such as `apps/web` for `apps/web/config/webpack.client.js`.
///
/// Webpack resolves `exposes` targets against its working directory when a
/// config sets no `context`, and a script runs a config in a config directory
/// from the package root.
fn config_directory_package(config_path: &Path, root: &Path) -> Option<PathBuf> {
    let relative = config_path.strip_prefix(root).ok()?;
    let directory = relative.parent()?;
    let name = directory.file_name()?.to_str()?;
    if !CONFIG_DIRECTORIES.contains(&name) {
        return None;
    }
    directory.parent().map(Path::to_path_buf)
}

/// Register the `resolve.alias` entries of a config as path aliases.
fn push_path_aliases(result: &mut PluginResult, source: &str, config_path: &Path, root: &Path) {
    for (find, replacement) in
        config_parser::extract_config_path_aliases(source, config_path, &["resolve", "alias"])
    {
        if let Some(normalized) =
            config_parser::normalize_filesystem_config_path(&replacement, config_path, root)
        {
            result.path_aliases.push((find, normalized));
        }
    }
}

/// Decide whether a file in a config directory is a config, and credit it.
///
/// A root config name is a config, except in `build/`: that directory holds
/// build output, so a `webpack.config.js` there can be compiled or stale. A
/// `webpack.<target>` name is a config only when [`exports_webpack_config`]
/// accepts it. `always_used` covers root names only, so the accepted file is
/// credited here, at any depth.
fn accept_directory_config(
    result: &mut PluginResult,
    config_path: &Path,
    source: &str,
    root: &Path,
) -> bool {
    if !is_directory_config_name(config_path) {
        return !is_in_output_directory(config_path);
    }
    if !exports_webpack_config(source, config_path) {
        return false;
    }
    if let Ok(relative) = config_path.strip_prefix(root) {
        result
            .always_used_files
            .push(globset::escape(&config_parser::path_to_config_string(
                relative,
            )));
    }
    true
}

/// The config directory that also holds build output.
const OUTPUT_DIRECTORY: &str = "build";

fn is_in_output_directory(config_path: &Path) -> bool {
    config_path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == OUTPUT_DIRECTORY)
}

/// Whether a file name has the config directory form `webpack.<target>.<ext>`
/// rather than a root config name, which is always a config.
fn is_directory_config_name(config_path: &Path) -> bool {
    config_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with("webpack.")
                && !name.starts_with("webpack.config.")
                && !name.contains(".config.")
        })
}

/// Whether a module exports a webpack configuration: an object with at least
/// one webpack config key (also from a function that returns one), an array of
/// configurations, or a `webpack-merge` call such as `merge(common, { ... })`.
fn exports_webpack_config(source: &str, path: &Path) -> bool {
    config_parser::extract_from_source(source, path, |program| {
        let declares_key = config_parser::find_config_object(program).is_some_and(|object| {
            CONFIG_KEYS
                .iter()
                .any(|key| config_parser::property_expr(object, key).is_some())
        });
        if declares_key {
            return Some(true);
        }
        let exported = config_parser::find_module_export_expression(program)?;
        Some(match exported {
            Expression::ArrayExpression(_) => true,
            Expression::CallExpression(call) => {
                is_merge_callee(&call.callee, &webpack_merge_bindings(program))
            }
            _ => false,
        })
    })
    .unwrap_or(false)
}

/// The `webpack-merge` functions that combine configurations.
const MERGE_FUNCTIONS: &[&str] = &["merge", "mergeWithCustomize", "mergeWithRules"];

/// The package that exports [`MERGE_FUNCTIONS`].
const WEBPACK_MERGE: &str = "webpack-merge";

/// Whether a callee is a `webpack-merge` function: `merge(...)`, a curried
/// `mergeWithCustomize({ ... })(...)`, a member of the imported package such
/// as `webpackMerge.merge(...)`, or the default import called directly.
fn is_merge_callee(callee: &Expression<'_>, package_bindings: &[String]) -> bool {
    match callee {
        Expression::Identifier(identifier) => {
            let name = identifier.name.as_str();
            MERGE_FUNCTIONS.contains(&name)
                || package_bindings.iter().any(|binding| binding == name)
        }
        Expression::StaticMemberExpression(member) => {
            MERGE_FUNCTIONS.contains(&member.property.name.as_str())
                && matches!(&member.object, Expression::Identifier(object)
                    if package_bindings.iter().any(|binding| binding == object.name.as_str()))
        }
        Expression::CallExpression(call) => is_merge_callee(&call.callee, package_bindings),
        _ => false,
    }
}

/// Local names bound to the whole `webpack-merge` module: a default or
/// namespace import, or `const name = require("webpack-merge")`.
fn webpack_merge_bindings(program: &Program<'_>) -> Vec<String> {
    let mut bindings = Vec::new();
    for statement in &program.body {
        match statement {
            Statement::ImportDeclaration(import) if import.source.value == WEBPACK_MERGE => {
                for specifier in import.specifiers.iter().flatten() {
                    match specifier {
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                            bindings.push(default.local.name.to_string());
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                            bindings.push(namespace.local.name.to_string());
                        }
                        ImportDeclarationSpecifier::ImportSpecifier(_) => {}
                    }
                }
            }
            Statement::VariableDeclaration(declaration) => {
                for declarator in &declaration.declarations {
                    if let BindingPattern::BindingIdentifier(identifier) = &declarator.id
                        && let Some(Expression::CallExpression(call)) = &declarator.init
                        && config_parser::is_require_call(call)
                        && config_parser::get_require_source(call).as_deref() == Some(WEBPACK_MERGE)
                    {
                        bindings.push(identifier.name.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    bindings
}

fn normalize_context_entry(entry: &str, context: &Path, config_path: &Path, root: &Path) -> String {
    let entry_path = config_parser::path_from_config_string(entry);
    if entry.starts_with('/') || entry_path.is_absolute() {
        return config_parser::normalize_filesystem_config_path(entry, config_path, root)
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

    /// Issue #2806: webpack reads a leading `/` as a filesystem path, so an
    /// alias or a context entry outside the project is not read as a project
    /// path.
    #[test]
    fn resolve_config_reads_a_leading_slash_as_a_filesystem_path() {
        let root = std::env::temp_dir().join("fallow-2806-webpack");
        let project = config_parser::path_to_config_string(&root);
        let source = format!(
            r#"
            module.exports = {{
                context: "{project}/app",
                entry: ["./main.ts", "/src/absolute.js", "{project}/app/admin.ts"],
                resolve: {{
                    alias: {{
                        "@outside": "/src/absolute",
                        "@inside": "{project}/src/inside",
                    }},
                }},
            }};
        "#
        );
        let plugin = WebpackPlugin;
        let result = plugin.resolve_config(&root.join("webpack.config.js"), &source, &root);

        assert_eq!(
            result.path_aliases,
            vec![("@inside".to_string(), "src/inside".to_string())]
        );
        assert_eq!(
            result.entry_patterns,
            vec!["app/main.ts", "/src/absolute.js", "app/admin.ts"]
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
        for path in [
            "config/webpack.client.js",
            "config/webpack.server.ts",
            "build/webpack.prod.js",
            "webpack/webpack.dev.js",
        ] {
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

    #[test]
    fn a_config_directory_file_counts_only_when_it_exports_a_webpack_config() {
        let path = std::path::Path::new("config/webpack.client.js");
        for source in [
            r#"module.exports = { mode: "production" };"#,
            r#"module.exports = (env) => ({ entry: "./src/index.ts" });"#,
            r"module.exports = [client, server];",
            r#"module.exports = merge(common, require("./webpack.parts"));"#,
            r"export default { plugins: [] };",
            r"module.exports = mergeWithCustomize({ customizeArray })(common, prod);",
            r"module.exports = mergeWithRules({ module: {} })(common, prod);",
            r#"
            const webpackMerge = require("webpack-merge");
            module.exports = webpackMerge.merge(common, prod);
            "#,
            r#"
            import webpackMerge from "webpack-merge";
            export default webpackMerge(common, prod);
            "#,
        ] {
            assert!(exports_webpack_config(source, path), "source: {source}");
        }
        for source in [
            r#"module.exports = { src: path.resolve(__dirname, "../src") };"#,
            r"exports.devServer = () => ({ devServer: { hot: true } });",
            r"module.exports = { loadCss, loadImages };",
            r#"module.exports = { name: "shared-settings" };"#,
            r"module.exports = mergeOptions(defaults, overrides);",
            r"module.exports = helpers.merge(defaults, overrides);",
        ] {
            assert!(!exports_webpack_config(source, path), "source: {source}");
        }
    }

    #[test]
    fn a_helper_in_a_config_directory_contributes_nothing() {
        let result = WebpackPlugin.resolve_config(
            std::path::Path::new("/project/config/webpack.paths.js"),
            r#"module.exports = { src: "./src/client.ts" };"#,
            std::path::Path::new("/project"),
        );
        assert!(result.always_used_files.is_empty());
        assert!(result.entry_patterns.is_empty());

        let result = WebpackPlugin.resolve_config(
            std::path::Path::new("/project/config/webpack.client.js"),
            r#"module.exports = { entry: "./src/client.ts" };"#,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.always_used_files, vec!["config/webpack.client.js"]);
        assert_eq!(result.entry_patterns, vec!["src/client.ts"]);
    }
}
