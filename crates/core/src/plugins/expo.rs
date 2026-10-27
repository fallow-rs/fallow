//! Expo framework plugin.
//!
//! Detects Expo projects and marks app entry points and config files. Reads
//! the config plugins that the app config lists.

use std::path::Path;

use oxc_ast::ast::{
    Argument, ArrayExpression, ArrayExpressionElement, Expression, ObjectExpression, Program,
    Statement,
};
use rustc_hash::FxHashSet;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["expo"];

const ENTRY_PATTERNS: &[&str] = &[
    "App.{ts,tsx,js,jsx}",
    "app/**/*.{ts,tsx,js,jsx}",
    "src/App.{ts,tsx,js,jsx}",
];

const ALWAYS_USED: &[&str] = &[
    "app.json",
    "app.config.{ts,js,mjs,cjs}",
    "metro.config.{ts,js,mjs,cjs}",
    "babel.config.{ts,js,mjs,cjs}",
];

const CONFIG_PATTERNS: &[&str] = &["app.json", "app.config.{ts,js,mjs,cjs}"];

/// The property paths of the config plugin list. `app.json` nests it under
/// `expo`, and `app.config.*` can return it at the top level.
const CONFIG_PLUGIN_PATHS: &[&[&str]] = &[&["plugins"], &["expo", "plugins"]];

const TOOLING_DEPENDENCIES: &[&str] = &["expo", "expo-cli", "@expo/webpack-config"];

pub struct ExpoPlugin;

impl Plugin for ExpoPlugin {
    fn name(&self) -> &'static str {
        "expo"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], _root: &std::path::Path) -> bool {
        deps.iter().any(|dep| dep == "expo") && !deps.iter().any(|dep| dep == "expo-router")
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);
        add_config_plugins(&mut result, source, config_path, root);
        result
    }
}

/// Credit the config plugins that an Expo app config lists in `plugins`.
///
/// Expo loads each entry by name at prebuild time, so the source never imports
/// it. An entry is a string or a `[name, options]` tuple. A bare name is a
/// package. A relative name is a local plugin file. The list, a spread, or an
/// entry can come from a stable local binding.
pub(super) fn add_config_plugins(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
) {
    for plugins_path in CONFIG_PLUGIN_PATHS {
        for name in config_plugin_names(source, config_path, plugins_path) {
            add_config_plugin(result, name.trim(), config_path, root);
        }
    }
}

/// The name of each entry in the plugin list at `plugins_path`. An entry is a
/// string or the first string of a `[name, options]` tuple.
///
/// The list, a spread argument, an entry, or a tuple head can be a top-level
/// `const` or `let` binding that the program never assigns to. A `push`,
/// `unshift` or `splice` call on a bound list at the top level adds its
/// arguments. A spread of an array literal adds its entries, and a conditional
/// spread adds the entries of each branch. Any other expression adds no entry,
/// so a dynamic value never counts as proof of a missing plugin.
fn config_plugin_names(source: &str, config_path: &Path, plugins_path: &[&str]) -> Vec<String> {
    config_parser::extract_from_source(source, config_path, |program| {
        let config = config_parser::find_config_object(program)?;
        let plugins = nested_value(config, plugins_path)?;
        let mut entries = StaticPluginEntries {
            program,
            followed: FxHashSet::default(),
            names: Vec::new(),
        };
        entries.collect_spread(plugins);
        Some(entries.names)
    })
    .unwrap_or_default()
}

/// The value at a property path. Each value can carry a type assertion, such
/// as `expo: { ... } satisfies ExpoConfig`.
fn nested_value<'a>(object: &'a ObjectExpression<'a>, path: &[&str]) -> Option<&'a Expression<'a>> {
    let (key, rest) = path.split_first()?;
    let value = config_parser::find_property(object, key)?
        .value
        .get_inner_expression();
    if rest.is_empty() {
        return Some(value);
    }
    match value {
        Expression::ObjectExpression(nested) => nested_value(nested, rest),
        _ => None,
    }
}

/// Collects the static entry names of a plugin list and follows stable local
/// bindings. Each binding is followed once, so a binding that refers to itself
/// ends the walk.
struct StaticPluginEntries<'a> {
    program: &'a Program<'a>,
    followed: FxHashSet<&'a str>,
    names: Vec<String>,
}

impl<'a> StaticPluginEntries<'a> {
    /// The expression that `expr` holds, through parentheses, type assertions
    /// and stable top-level bindings.
    fn resolve(&mut self, expr: &'a Expression<'a>) -> Option<&'a Expression<'a>> {
        let mut expr = expr.get_inner_expression();
        while let Expression::Identifier(id) = expr {
            if !self.followed.insert(id.name.as_str()) {
                return None;
            }
            expr = config_parser::find_stable_binding_init(self.program, &id.name)?
                .get_inner_expression();
        }
        Some(expr)
    }

    /// Collect the entries of an array. When the array is the value of
    /// `binding`, the entries that `push`, `unshift` or `splice` calls on the
    /// binding add count too.
    fn collect_array(&mut self, array: &'a ArrayExpression<'a>, binding: Option<&'a str>) {
        for element in &array.elements {
            if let ArrayExpressionElement::SpreadElement(spread) = element {
                self.collect_spread(&spread.argument);
            } else if let Some(entry) = element.as_expression() {
                self.collect_entry(entry);
            }
        }
        let Some(binding) = binding else {
            return;
        };
        let mut added = Vec::new();
        collect_added_entries(&self.program.body, binding, &mut added);
        for argument in added {
            if let Argument::SpreadElement(spread) = argument {
                self.collect_spread(&spread.argument);
            } else if let Some(entry) = argument.as_expression() {
                self.collect_entry(entry);
            }
        }
    }

    /// Collect the entries of a list or a spread argument: an array, or a
    /// condition with array branches.
    fn collect_spread(&mut self, expr: &'a Expression<'a>) {
        let binding = match expr.get_inner_expression() {
            Expression::Identifier(id) => Some(id.name.as_str()),
            _ => None,
        };
        match self.resolve(expr) {
            Some(Expression::ArrayExpression(array)) => self.collect_array(array, binding),
            Some(Expression::ConditionalExpression(conditional)) => {
                self.collect_spread(&conditional.consequent);
                self.collect_spread(&conditional.alternate);
            }
            Some(Expression::LogicalExpression(logical)) => {
                self.collect_spread(&logical.left);
                self.collect_spread(&logical.right);
            }
            _ => {}
        }
    }

    fn collect_entry(&mut self, expr: &'a Expression<'a>) {
        let name = match self.resolve(expr) {
            Some(Expression::ArrayExpression(tuple)) => tuple
                .elements
                .first()
                .and_then(ArrayExpressionElement::as_expression)
                .and_then(|head| self.resolve(head))
                .and_then(config_parser::expression_to_string),
            Some(entry) => config_parser::expression_to_string(entry),
            None => None,
        };
        if let Some(name) = name
            && !self.names.contains(&name)
        {
            self.names.push(name);
        }
    }
}

/// Collect the arguments that `binding.push(...)`, `binding.unshift(...)` and
/// `binding.splice(start, count, ...)` calls add to the array of `binding`.
/// The calls can be top-level statements or sit in a top-level block or `if`
/// statement, such as `if (isDev) plugins.push("expo-dev-client")`.
fn collect_added_entries<'a>(
    statements: &'a [Statement<'a>],
    binding: &str,
    added: &mut Vec<&'a Argument<'a>>,
) {
    for statement in statements {
        match statement {
            Statement::ExpressionStatement(statement) => {
                let Expression::CallExpression(call) = statement.expression.get_inner_expression()
                else {
                    continue;
                };
                let Expression::StaticMemberExpression(member) = &call.callee else {
                    continue;
                };
                if !matches!(member.object.get_inner_expression(), Expression::Identifier(id) if id.name == binding)
                {
                    continue;
                }
                let skip = match member.property.name.as_str() {
                    "push" | "unshift" => 0,
                    "splice" => 2,
                    _ => continue,
                };
                added.extend(call.arguments.iter().skip(skip));
            }
            Statement::BlockStatement(block) => collect_added_entries(&block.body, binding, added),
            Statement::IfStatement(statement) => {
                collect_added_entries(std::slice::from_ref(&statement.consequent), binding, added);
                if let Some(alternate) = &statement.alternate {
                    collect_added_entries(std::slice::from_ref(alternate), binding, added);
                }
            }
            _ => {}
        }
    }
}

fn add_config_plugin(result: &mut PluginResult, name: &str, config_path: &Path, root: &Path) {
    if name.is_empty() {
        return;
    }
    if config_parser::is_relative_specifier(name) {
        if let Some(path) = config_parser::normalize_config_path(name, config_path, root) {
            result.push_entry_path(path);
        }
        return;
    }
    result
        .referenced_dependencies
        .push(crate::resolve::extract_package_name(name));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credited(source: &str) -> Vec<String> {
        let result = ExpoPlugin.resolve_config(
            Path::new("/project/app.config.ts"),
            source,
            Path::new("/project"),
        );
        let mut deps = result.referenced_dependencies;
        deps.sort();
        deps
    }

    #[test]
    fn credits_plugins_from_a_direct_array() {
        let source = r#"export default { plugins: ["plugin-a", ["plugin-b", {}]] };"#;
        assert_eq!(credited(source), ["plugin-a", "plugin-b"]);
    }

    #[test]
    fn credits_plugins_from_a_local_binding() {
        let source = r#"
            const plugins = ["plugin-a", ["plugin-b", { enabled: true }]];
            export default { plugins };
        "#;
        assert_eq!(credited(source), ["plugin-a", "plugin-b"]);
    }

    #[test]
    fn credits_plugins_from_a_binding_under_expo() {
        let source = r#"
            const plugins = ["plugin-a"];
            export default { expo: { name: "app", plugins: plugins } satisfies ExpoConfig };
        "#;
        assert_eq!(credited(source), ["plugin-a"]);
    }

    #[test]
    fn credits_plugins_from_spreads_and_bound_entries() {
        let source = r#"
            const shared = ["plugin-a"];
            const sentry = ["plugin-b", { organization: "example" }];
            const camera = "plugin-c";
            export default {
                plugins: [...shared, sentry, camera, ...["plugin-d"], ...(isDev ? ["plugin-e"] : [])],
            };
        "#;
        assert_eq!(
            credited(source),
            ["plugin-a", "plugin-b", "plugin-c", "plugin-d", "plugin-e"]
        );
    }

    #[test]
    fn skips_reassigned_and_dynamic_bindings() {
        let source = r#"
            let plugins = ["plugin-a"];
            plugins = ["plugin-b"];
            const other = loadPlugins();
            export default { plugins, expo: { plugins: other } };
        "#;
        assert!(credited(source).is_empty());
    }

    #[test]
    fn stops_on_a_self_referencing_binding() {
        let source = r#"
            const plugins = [...plugins, "plugin-a"];
            export default { plugins };
        "#;
        assert_eq!(credited(source), ["plugin-a"]);
    }

    #[test]
    fn credits_plugins_that_mutating_calls_add() {
        let source = r#"
            const plugins = ["plugin-a"];
            plugins.push("plugin-b", ["plugin-c", { enabled: true }]);
            plugins!.unshift("plugin-d");
            plugins.splice(1, 0, "plugin-e");
            if (isDev) {
                plugins.push("plugin-f");
            } else plugins.push(...["plugin-g"]);
            other.push("not-a-plugin");
            export default { plugins };
        "#;
        assert_eq!(
            credited(source),
            [
                "plugin-a", "plugin-b", "plugin-c", "plugin-d", "plugin-e", "plugin-f", "plugin-g"
            ]
        );
    }

    #[test]
    fn credits_plugins_from_a_module_exports_config() {
        let source = r#"
            const plugins = ["plugin-a"];
            module.exports = { expo: { plugins } };
        "#;
        assert_eq!(credited(source), ["plugin-a"]);
    }

    #[test]
    fn credits_plugins_from_a_config_function() {
        let source = r#"
            const plugins = ["plugin-a"];
            export default ({ config }) => ({ ...config, plugins });
        "#;
        assert_eq!(credited(source), ["plugin-a"]);
    }

    #[test]
    fn stops_on_mutual_spreads() {
        let source = r#"
            const first = [...second, "plugin-a"];
            const second = [...first, "plugin-b"];
            export default { plugins: first };
        "#;
        assert_eq!(credited(source), ["plugin-a", "plugin-b"]);
    }

    #[test]
    fn credits_both_sides_of_a_logical_spread() {
        let source = r#"
            const extra = ["plugin-a"];
            export default { plugins: [...(extra || ["plugin-b"])] };
        "#;
        assert_eq!(credited(source), ["plugin-a", "plugin-b"]);
    }

    #[test]
    fn skips_a_name_that_a_function_binds_again() {
        let source = r#"
            const plugins = ["plugin-a"];
            function local() {
                const plugins = ["plugin-b"];
                return plugins;
            }
            export default { plugins };
        "#;
        assert!(credited(source).is_empty());
    }
}
