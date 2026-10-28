//! Webpack bundler plugin.
//!
//! Detects Webpack projects and marks conventional entry points and config files.
//! Parses webpack config to extract entry points, plugin dependencies, loader
//! packages from module.rules, and external dependencies.

use std::path::{Path, PathBuf};

use fallow_extract::JsText;
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
            config_parser::extract_each_config_require_strings(source, config_path, "plugins");
        for dep in &require_deps {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        let externals =
            config_parser::extract_each_config_shallow_strings(source, config_path, "externals");
        for ext in &externals {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(ext));
        }

        parse_webpack_loaders(source, config_path, &mut result);

        result
    },
);

/// Extract loader package names from webpack `module.rules` config, in each
/// config of a multi-config export.
///
/// Handles common patterns:
/// - `{ loader: 'ts-loader' }`
/// - `{ use: ['style-loader', 'css-loader'] }`
/// - `{ use: [{ loader: 'css-loader', options: {} }] }`
/// - `{ oneOf: [...rules] }`
pub(super) fn parse_webpack_loaders(source: &str, path: &Path, result: &mut PluginResult) {
    let loaders = config_parser::extract_from_source(source, path, |program| {
        let mut loaders = PluginResult::default();
        for obj in config_parser::find_config_objects(program) {
            if let Some(rules) = module_rules(obj) {
                walk_rules(rules, &mut loaders);
            }
        }
        Some(loaders.referenced_dependencies)
    })
    .unwrap_or_default();
    result.referenced_dependencies.extend(loaders);
}

/// The `module.rules` array of one config object.
fn module_rules<'a>(
    obj: &'a oxc_ast::ast::ObjectExpression<'a>,
) -> Option<&'a oxc_ast::ast::ArrayExpression<'a>> {
    let module_prop = find_obj_prop(obj, "module")?;
    let Expression::ObjectExpression(module_obj) = &module_prop.value else {
        return None;
    };
    let rules_prop = find_obj_prop(module_obj, "rules")?;
    let Expression::ArrayExpression(rules_arr) = &rules_prop.value else {
        return None;
    };
    Some(rules_arr)
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
        let key_name: std::borrow::Cow<'_, str> = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str().into(),
            PropertyKey::StringLiteral(s) => s.value.text(),
            _ => continue,
        };

        match &*key_name {
            "loader" => {
                if let Expression::StringLiteral(s) = &p.value {
                    let dep = crate::resolve::extract_package_name(&s.value.text());
                    result.referenced_dependencies.push(dep);
                }
            }
            "use" => match &p.value {
                Expression::StringLiteral(s) => {
                    let dep = crate::resolve::extract_package_name(&s.value.text());
                    result.referenced_dependencies.push(dep);
                }
                Expression::ArrayExpression(arr) => {
                    for use_el in &arr.elements {
                        if let Some(expr) = use_el.as_expression() {
                            match expr {
                                Expression::StringLiteral(s) => {
                                    let dep = crate::resolve::extract_package_name(&s.value.text());
                                    result.referenced_dependencies.push(dep);
                                }
                                Expression::ObjectExpression(use_obj) => {
                                    if let Some(loader_prop) = find_obj_prop(use_obj, "loader")
                                        && let Expression::StringLiteral(s) = &loader_prop.value
                                    {
                                        let dep =
                                            crate::resolve::extract_package_name(&s.value.text());
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
/// In a multi-config export, each config resolves its entries against its own
/// base directory.
///
/// Returns the project-relative base directory that all configs share, so the
/// Module Federation reader can resolve `exposes` targets the same way. The
/// Federation reader reads the plugin calls of the whole file and does not
/// know which config holds a call. When the configs declare different base
/// directories, this returns `None`, and the reader resolves `exposes`
/// against the config directory.
pub(super) fn apply_entries(
    result: &mut PluginResult,
    source: &str,
    config: ConfigFile<'_>,
    entry_key: &[&str],
    base_key: &str,
) -> Option<PathBuf> {
    let ConfigFile { path, root } = config;
    let mut bases = Vec::new();
    for (entries, raw_base) in
        config_parser::extract_each_config_entries(source, path, entry_key, base_key)
    {
        let base = raw_base
            .and_then(|raw| config_parser::normalize_filesystem_config_path_buf(&raw, path, root));
        result.extend_entry_patterns_or_dependencies(entries, |entry| {
            base.as_ref()
                .map(|base| normalize_context_entry(&entry, base, path, root))
                .unwrap_or(entry)
        });
        bases.push(base);
    }
    shared_base(bases)
}

/// The one base directory of all configs, or `None` when the configs declare
/// different base directories.
fn shared_base(bases: Vec<Option<PathBuf>>) -> Option<PathBuf> {
    let mut bases = bases.into_iter();
    let first = bases.next()??;
    bases
        .all(|base| base.as_ref() == Some(&first))
        .then_some(first)
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

/// Register the `resolve.alias` entries of each config as path aliases.
///
/// Webpack applies an alias before it resolves an import, but fallow applies it
/// only as a fallback. A replacement file that the fallback never reaches is
/// made an entry point, because nothing else in the graph imports it. A
/// trailing `$` marks an exact-match key, such as `react$`, and names the same
/// package.
fn push_path_aliases(result: &mut PluginResult, source: &str, config_path: &Path, root: &Path) {
    for (find, replacement) in
        config_parser::extract_each_config_path_aliases(source, config_path, &["resolve", "alias"])
    {
        if let Some(normalized) =
            config_parser::normalize_filesystem_config_path(&replacement, config_path, root)
        {
            let key = find.strip_suffix('$').unwrap_or(&find);
            if super::alias_replacement::credit_unrouted_replacement(
                result,
                key,
                &normalized,
                config_path,
            ) {
                result.push_entry_pattern(globset::escape(&normalized));
            }
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
/// one webpack config key (also from a function that returns one or an array
/// of them), an array of configurations, or a `webpack-merge` call such as
/// `merge(common, { ... })`.
fn exports_webpack_config(source: &str, path: &Path) -> bool {
    config_parser::extract_from_source(source, path, |program| {
        let declares_key = config_parser::find_config_objects(program)
            .into_iter()
            .any(|object| {
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
    fn resolve_config_seeds_replacements_of_unrouted_alias_keys() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let pkg = root.join("node_modules/pkg");
        std::fs::create_dir_all(&pkg).expect("create package dir");
        std::fs::write(pkg.join("package.json"), r#"{ "name": "pkg" }"#).expect("write manifest");
        let source = r#"
            const path = require("path");
            module.exports = {
                resolve: {
                    alias: {
                        "./src/clock.js": path.resolve(__dirname, "mocks/clock.js"),
                        pkg: path.resolve(__dirname, "mocks/pkg.js"),
                        pkg$: path.resolve(__dirname, "mocks/[exact].js"),
                        missing: path.resolve(__dirname, "mocks/missing.js"),
                        "@lib": path.resolve(__dirname, "src/lib/index.js"),
                        "@": path.resolve(__dirname, "src"),
                    },
                },
            };
        "#;
        let result = WebpackPlugin.resolve_config(&root.join("webpack.config.js"), source, root);

        assert_eq!(
            result.entry_export_credits,
            vec![
                crate::plugins::UsedExportRule::new("mocks/clock.js", ["*"]),
                crate::plugins::UsedExportRule::new("mocks/pkg.js", ["*"]),
                crate::plugins::UsedExportRule::new("mocks/[[]exact[]].js", ["*"]),
            ]
        );
        assert_eq!(
            result.entry_patterns,
            vec!["mocks/clock.js", "mocks/pkg.js", "mocks/[[]exact[]].js"]
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

    /// A config whose inlined values nest much deeper than the source does.
    /// Two top-level constants name each other at the leaf, so the first pass
    /// stacks their copies. Two callback locals do the same and also name the
    /// constants, so the second pass stacks copies of values that already
    /// hold copies of the first pass.
    fn deep_inline_source(open: &str, close: &str, depth: usize) -> String {
        let deep = |leaf: &str| format!("{}{leaf}{}", open.repeat(depth), close.repeat(depth));
        format!(
            "const A = {a};\nconst B = {b};\nmodule.exports = () => {{\n\
             const l1 = {l1};\nconst l2 = {l2};\n\
             return {{ entry: \"./src/a.ts\", resolve: {{ alias: l1 }}, plugins: [l1, l2],\n\
             externals: l2, module: {{ rules: [l1] }}, other: [l1, l2, A, B] }};\n}};\n",
            a = deep("B"),
            b = deep("A"),
            l1 = deep("[l2, A]"),
            l2 = deep("[l1, B]"),
        )
    }

    /// The shape of [`deep_inline_source`] with one constant and one local
    /// that each name themselves, which gives the deepest AST per source byte.
    fn self_inline_source(open: &str, close: &str, depth: usize) -> String {
        let deep = |leaf: &str| format!("{}{leaf}{}", open.repeat(depth), close.repeat(depth));
        format!(
            "const A = {a};\nmodule.exports = () => {{\nconst l = {l};\n\
             return {{ entry: \"./src/a.ts\", resolve: {{ alias: l }}, plugins: [l],\n\
             externals: l, module: {{ rules: [l] }}, other: [l, A] }};\n}};\n",
            a = deep("A"),
            l = deep("[l, A]"),
        )
    }

    #[test]
    fn deep_inlined_values_stay_within_the_parse_stack() {
        // Each parse runs on a stack that `parse_bounded` sizes from the source
        // length and the source bracket depth. The inline passes can make the
        // AST deeper than the source. This test fails with a stack overflow
        // when the walks over the inlined AST need more stack than that.
        // A probe measured about 1.1 AST levels per source byte for the
        // self-naming array shape, against about 0.12 levels per byte for the
        // source alone.
        for (open, close) in [("[", "]"), ("{a:", "}"), ("{a:[", "]}")] {
            for source in [
                deep_inline_source(open, close, 480),
                self_inline_source(open, close, 480),
            ] {
                let result = WebpackPlugin.resolve_config(
                    std::path::Path::new("/project/webpack.config.js"),
                    &source,
                    std::path::Path::new("/project"),
                );
                assert!(
                    result
                        .entry_patterns
                        .iter()
                        .any(|rule| rule.pattern == "src/a.ts"),
                    "{source}"
                );
            }
        }
    }

    fn credits_admin_entry(source: &str) -> bool {
        WebpackPlugin
            .resolve_config(
                std::path::Path::new("/project/webpack.config.js"),
                source,
                std::path::Path::new("/project"),
            )
            .entry_patterns
            .iter()
            .any(|rule| rule.pattern == "src/admin.ts")
    }

    #[test]
    fn entry_constant_that_a_read_only_or_exported_call_gets_still_resolves() {
        let cases = [
            // A `webpack-merge` call only reads its arguments.
            r#"const { merge } = require("webpack-merge"); const common = require("./webpack.common.js");
               const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               module.exports = merge(common, { mode: "production", entry });"#,
            r#"import { merge } from "webpack-merge"; import common from "./webpack.common.js";
               const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               export default merge(common, { entry });"#,
            // A named import of the `path` module only reads its arguments.
            r#"const { resolve } = require("path"); const HtmlWebpackPlugin = require("html-webpack-plugin");
               const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               module.exports = { entry, plugins: [new HtmlWebpackPlugin({ template: resolve(__dirname, entry.main) })] };"#,
            r#"import { join } from "node:path";
               const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               export default { entry, output: { path: join(__dirname, entry.main) } };"#,
            // The exported config call gives the config to webpack.
            r#"const SpeedMeasurePlugin = require("speed-measure-webpack-plugin"); const smp = new SpeedMeasurePlugin();
               const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               module.exports = smp.wrap({ entry });"#,
            r#"module.exports = (env) => {
                 const config = { entry: { main: "./src/main.ts", admin: "./src/admin.ts" } };
                 if (env.analyze) return withAnalyzer(config);
                 return config;
               };"#,
            // The local pass: the returned `merge` call.
            r#"const { merge } = require("webpack-merge"); const common = require("./webpack.common.js");
               module.exports = (env) => {
                 const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
                 return merge(common, { mode: env.production ? "production" : "development", entry });
               };"#,
        ];
        for source in cases {
            assert!(credits_admin_entry(source), "{source}");
        }
    }

    #[test]
    fn entry_constant_that_a_local_function_changes_does_not_resolve() {
        let cases = [
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               function configure(e) { e.admin = "./src/other.ts"; }
               configure(entry);
               module.exports = { entry };"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry };
               function retarget(cfg) { cfg.entry.admin = "./src/other.ts"; }
               retarget(config);
               module.exports = config;"#,
        ];
        for source in cases {
            assert!(!credits_admin_entry(source), "{source}");
        }
    }

    #[test]
    fn entry_constant_that_an_imported_or_unknown_call_gets_still_resolves() {
        let cases = [
            // An imported helper (fixture u9).
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const addAnalyzer = require("./analyzer.js");
               const config = { mode: "production", entry, plugins: [] };
               if (process.env.ANALYZE) addAnalyzer(config);
               module.exports = config;"#,
            r#"const { merge } = require("lodash");
               const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               merge(entry, { admin: "./src/other.ts" });
               module.exports = { entry };"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const configure = (e) => addPages(e);
               configure(entry);
               module.exports = { entry };"#,
        ];
        for source in cases {
            assert!(credits_admin_entry(source), "{source}");
        }
    }

    #[test]
    fn entry_constant_that_an_alias_or_a_local_function_changes_under_another_key_still_resolves() {
        let cases = [
            // fixture u4
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry, plugins: [] };
               const plugins = config.plugins;
               if (process.env.X) plugins.push(1);
               module.exports = config;"#,
            // fixture u5
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry, plugins: [] };
               const { plugins } = config;
               if (process.env.X) plugins.push(1);
               module.exports = config;"#,
            // fixture u6
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry };
               const c2 = config;
               c2.devtool = "source-map";
               module.exports = config;"#,
            // fixture u8
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry, plugins: [] };
               function addAnalyzer(cfg) { cfg.plugins.push(1); }
               if (process.env.ANALYZE) addAnalyzer(config);
               module.exports = config;"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const holder = { a: { entry, list: [] } };
               holder.a.list.push(1);
               holder.a.mode = "x";
               holder.b = 1;
               module.exports = { entry };"#,
        ];
        for source in cases {
            assert!(credits_admin_entry(source), "{source}");
        }
    }

    #[test]
    fn entry_constant_in_a_config_that_changes_other_keys_still_resolves() {
        let cases = [
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry, plugins: [] };
               if (process.env.ANALYZE) { config.devtool = "source-map"; }
               module.exports = config;"#,
            r#"const webpack = require("webpack");
               const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry, plugins: [] };
               if (process.env.NODE_ENV === "production") { config.plugins.push(new webpack.ProgressPlugin()); }
               module.exports = config;"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry: entry, output: { path: "dist" } };
               config.output.filename = "[name].js";
               delete config.mode;
               module.exports = config;"#,
        ];
        for source in cases {
            assert!(credits_admin_entry(source), "{source}");
        }
    }

    #[test]
    fn entry_constant_that_a_holder_changes_through_its_key_does_not_resolve() {
        let cases = [
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry };
               config.entry.admin = "./src/other.ts";
               module.exports = config;"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry };
               config[key].admin = "./src/other.ts";
               module.exports = config;"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry };
               Object.assign(config.entry, { admin: "./src/other.ts" });
               module.exports = config;"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry };
               const c2 = config;
               c2.entry.admin = "./src/other.ts";
               module.exports = config;"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { mode: "production", entry };
               const { entry: e } = config;
               e.admin = "./src/other.ts";
               module.exports = config;"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const holder = { a: { entry } };
               holder.a.entry.admin = "./src/other.ts";
               module.exports = { entry };"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const list = [entry];
               list[0].admin = "./src/other.ts";
               module.exports = { entry };"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const config = { ...{ entry } };
               config.entry.admin = "./src/other.ts";
               module.exports = { entry };"#,
        ];
        for source in cases {
            assert!(!credits_admin_entry(source), "{source}");
        }
    }

    #[test]
    fn local_function_whose_parameter_shares_a_name_still_reads_its_argument() {
        // The parameter `config` of `htmlPages` is not the top-level `config`.
        let source = r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
            function htmlPages(config) { return Object.keys(config).map(() => null); }
            const config = { mode: "production", entry, plugins: htmlPages(entry) };
            if (process.env.ANALYZE) { config.devtool = "source-map"; }
            module.exports = config;"#;
        assert!(credits_admin_entry(source));
    }

    #[test]
    fn local_function_that_changes_its_parameter_through_a_call_counts_as_a_change() {
        let cases = [
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               function outer(x) { inner(x); }
               function inner(y) { y.admin = "./src/other.ts"; }
               outer(entry);
               module.exports = { entry };"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               const walk = function (e, n) { if (n > 0) return walk(e, n - 1); e.admin = "./src/other.ts"; };
               walk(entry, 2);
               module.exports = { entry };"#,
            r#"const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
               function configure(e) { const alias = e; alias.admin = "./src/other.ts"; }
               configure(entry);
               module.exports = { entry };"#,
        ];
        for source in cases {
            assert!(!credits_admin_entry(source), "{source}");
        }
    }

    #[test]
    fn local_binding_that_shadows_a_top_level_function_does_not_use_its_body() {
        // The top-level `setEntry` changes its parameter, but the call names
        // the local binding from `require`, which fallow cannot see.
        let source = r#"function setEntry(e) { e.admin = "./src/other.ts"; }
            module.exports = () => {
              const { setEntry } = require("./helpers.js");
              const entry = { main: "./src/main.ts", admin: "./src/admin.ts" };
              setEntry(entry);
              return { entry };
            };"#;
        assert!(credits_admin_entry(source));
    }

    #[test]
    fn entry_constant_that_a_local_function_only_reads_still_resolves() {
        let source = r#"const entries = { main: "./src/main.ts", admin: "./src/admin.ts" };
            function htmlPages(e) { return Object.keys(e).map(() => null); }
            module.exports = { entry: entries, plugins: htmlPages(entries) };"#;
        assert!(credits_admin_entry(source));
    }

    fn federation_exposes_of(source: &str) -> Vec<String> {
        let result = WebpackPlugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        result
            .entry_patterns
            .into_iter()
            .map(|rule| rule.pattern)
            .filter(|pattern| pattern.contains("x.ts"))
            .collect()
    }

    #[test]
    fn multi_config_with_different_contexts_roots_exposes_at_the_config_directory() {
        // The plugin call sits in the second config, which sets no `context`.
        // The `context` of the first config must not move the exposed file.
        let source = r#"
            const path = require("path");
            const { ModuleFederationPlugin } = require("webpack").container;
            module.exports = [
                { context: path.resolve(__dirname, "client"), entry: "./main.ts" },
                { entry: "./src/server.ts",
                  plugins: [new ModuleFederationPlugin({ name: "app", exposes: { "./X": "./src/x.ts" } })] },
            ];
        "#;
        assert_eq!(federation_exposes_of(source), vec!["src/x.ts"]);
    }

    #[test]
    fn multi_config_with_one_shared_context_roots_exposes_at_that_context() {
        let source = r#"
            const path = require("path");
            module.exports = [
                { context: path.resolve(__dirname, "app"), entry: "./main.ts" },
                { context: path.resolve(__dirname, "app"), entry: "./server.ts",
                  plugins: [new ModuleFederationPlugin({ name: "app", exposes: { "./X": "./src/x.ts" } })] },
            ];
        "#;
        assert_eq!(federation_exposes_of(source), vec!["app/src/x.ts"]);
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

    /// The shape of a browser extension build: a function with local configs
    /// that spread a shared local, and a returned array of two configs. The
    /// source is about as large as the real config that this shape comes from.
    const FUNCTION_MULTI_CONFIG: &str = r#"
        const path = require("path");
        const webpack = require("webpack");
        const TerserPlugin = require("terser-webpack-plugin");

        module.exports = (env, argv) => {
            const mode = argv.mode || "development";
            const plugins = [
                new webpack.IgnorePlugin({
                    checkResource(resource) {
                        return /wordlists/.test(resource);
                    },
                }),
                new webpack.ProvidePlugin({ process: "process/browser" }),
                new webpack.DefinePlugin({ "process.env.NODE_ENV": JSON.stringify(mode) }),
            ];
            const common = {
                mode,
                devtool: mode === "development" ? "inline-source-map" : false,
                module: {
                    rules: [{ test: /\.tsx?$/, use: "ts-loader", exclude: /node_modules/ }],
                },
                performance: {
                    hints: mode === "production" ? "warning" : false,
                    maxAssetSize: 2000000,
                    maxEntrypointSize: 2000000,
                },
                optimization: {
                    minimize: mode === "production",
                    usedExports: true,
                    minimizer: [
                        new TerserPlugin({
                            terserOptions: {
                                compress: { drop_console: mode === "production", unused: true },
                                format: { comments: false },
                            },
                            extractComments: false,
                        }),
                    ],
                },
                resolve: {
                    alias: { linkedom: path.resolve(__dirname, "src/shims/linkedom.ts") },
                    extensions: [".tsx", ".ts", ".js"],
                    fallback: { process: false, canvas: false },
                },
                plugins,
                output: { filename: "js/[name].js", path: path.resolve(__dirname, "dist") },
            };
            const bgConfig = {
                ...common,
                entry: { background: path.resolve(__dirname, "./src/worker/background.ts") },
                target: "webworker",
                resolve: {
                    ...common.resolve,
                    fallback: {
                        ...common.resolve.fallback,
                        fs: false,
                        net: false,
                        tls: false,
                        path: false,
                        stream: false,
                        crypto: false,
                    },
                },
            };
            const webConfig = {
                ...common,
                entry: {
                    page_0: path.resolve(__dirname, "./src/pages/page_0.ts"),
                    page_1: path.resolve(__dirname, "./src/pages/page_1.ts"),
                    page_2: path.resolve(__dirname, "./src/pages/page_2.ts"),
                    page_3: path.resolve(__dirname, "./src/pages/page_3.ts"),
                    page_4: path.resolve(__dirname, "./src/pages/page_4.ts"),
                    page_5: path.resolve(__dirname, "./src/pages/page_5.ts"),
                    page_6: path.resolve(__dirname, "./src/pages/page_6.ts"),
                    page_7: path.resolve(__dirname, "./src/pages/page_7.ts"),
                    page_8: path.resolve(__dirname, "./src/pages/page_8.ts"),
                    page_9: path.resolve(__dirname, "./src/pages/page_9.ts"),
                    page_10: path.resolve(__dirname, "./src/pages/page_10.ts"),
                    page_11: path.resolve(__dirname, "./src/pages/page_11.ts"),
                    page_12: path.resolve(__dirname, "./src/pages/page_12.ts"),
                    page_13: path.resolve(__dirname, "./src/pages/page_13.ts"),
                    page_14: path.resolve(__dirname, "./src/pages/page_14.ts"),
                    page_15: path.resolve(__dirname, "./src/pages/page_15.ts"),
                    page_16: path.resolve(__dirname, "./src/pages/page_16.ts"),
                    page_17: path.resolve(__dirname, "./src/pages/page_17.ts"),
                    page_18: path.resolve(__dirname, "./src/pages/page_18.ts"),
                },
                target: "web",
            };
            return [bgConfig, webConfig];
        };
    "#;

    #[test]
    fn resolve_config_reads_each_config_of_a_function_multi_config() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let pkg = root.join("node_modules/linkedom");
        std::fs::create_dir_all(&pkg).expect("create package dir");
        std::fs::write(pkg.join("package.json"), r#"{ "name": "linkedom" }"#)
            .expect("write manifest");
        let result = WebpackPlugin.resolve_config(
            &root.join("webpack.config.js"),
            FUNCTION_MULTI_CONFIG,
            root,
        );

        let mut expected = vec!["src/worker/background.ts".to_string()];
        expected.extend((0..19).map(|i| format!("src/pages/page_{i}.ts")));
        expected.push("src/shims/linkedom.ts".to_string());
        assert_eq!(result.entry_patterns, expected);
        assert_eq!(
            result.path_aliases,
            vec![("linkedom".to_string(), "src/shims/linkedom.ts".to_string())]
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"ts-loader".to_string())
        );
    }

    #[test]
    fn resolve_config_resolves_each_config_against_its_own_context() {
        let source = r#"
            const path = require("path");
            module.exports = [
                { context: path.resolve(__dirname, "client"), entry: "./main.ts" },
                { context: path.resolve(__dirname, "server"), entry: "./main.ts" },
                { entry: "./src/tool.ts" },
            ];
        "#;
        let result = WebpackPlugin.resolve_config(
            std::path::Path::new("/project/webpack.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(
            result.entry_patterns,
            vec!["client/main.ts", "server/main.ts", "src/tool.ts"]
        );
    }

    #[test]
    fn config_directory_file_that_returns_an_array_of_configs_is_a_config() {
        let source = r#"
            module.exports = (env) => {
                const shared = { mode: "production" };
                return [{ ...shared, target: "web" }, { ...shared, target: "node" }];
            };
        "#;
        assert!(exports_webpack_config(
            source,
            std::path::Path::new("config/webpack.targets.js")
        ));
        assert!(!exports_webpack_config(
            "module.exports = () => [{ src: \"a\" }];",
            std::path::Path::new("config/webpack.paths.js")
        ));
    }
}
