//! Read the options of a known plugin call in a Vite-style `plugins` array.

use std::path::Path;

use oxc_ast::ast::*;

use super::{
    MAX_VITE_PLUGIN_WRAPPER_DEPTH, PluginImportBindings, collect_plugin_import_bindings,
    expression_to_path_string, expression_to_string, extract_from_source, find_property,
    get_nested_expression,
};

/// Extract a path string at `option_path` in the options object of a call to
/// the default export of `package` in a Vite-style `plugins` array, such as
/// `vuetify({ styles: { configFile: "src/settings.scss" } })`.
///
/// The call must use a binding that an `import` of `package` declares, so an
/// unrelated local function with the same name never matches.
#[must_use]
pub fn extract_vite_plugin_call_option_path(
    source: &str,
    path: &Path,
    package: &str,
    option_path: &[&str],
) -> Option<String> {
    read_vite_plugin_call_options(source, path, package, |options| {
        expression_to_path_string(get_nested_expression(options?, option_path)?)
    })
}

/// One string-list option of a plugin call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringListOption {
    /// The call does not set the option, so the plugin default applies.
    Missing,
    /// The static strings of the option.
    Values(Vec<String>),
    /// The option has a value that is not a static string or array.
    Unreadable,
}

/// Extract string-list options of a call to the default export of `package`
/// in a Vite-style `plugins` array, such as
/// `Pages({ dirs: ["src/pages", { dir: "src/admin" }] })`.
///
/// Returns one entry for each key of `keys`, or `None` when no call matches
/// or the options argument is not an object literal. A call without
/// arguments gives [`StringListOption::Missing`] for each key. A string gives
/// one value. An array gives each string element, and the string at
/// `object_key` of each object element.
#[must_use]
pub fn extract_vite_plugin_call_string_lists(
    source: &str,
    path: &Path,
    package: &str,
    keys: &[&str],
    object_key: &str,
) -> Option<Vec<StringListOption>> {
    read_vite_plugin_call_options(source, path, package, |options| {
        Some(
            keys.iter()
                .map(|key| {
                    options
                        .and_then(|options| find_property(options, key))
                        .map_or(StringListOption::Missing, |property| {
                            string_list(&property.value, object_key)
                        })
                })
                .collect(),
        )
    })
}

fn string_list(expression: &Expression<'_>, object_key: &str) -> StringListOption {
    if let Some(value) = expression_to_string(expression) {
        return StringListOption::Values(vec![value]);
    }
    let Expression::ArrayExpression(array) = expression else {
        return StringListOption::Unreadable;
    };
    let values: Option<Vec<String>> = array
        .elements
        .iter()
        .map(|element| match element.as_expression()? {
            Expression::ObjectExpression(object) => {
                expression_to_string(&find_property(object, object_key)?.value)
            }
            expression => expression_to_string(expression),
        })
        .collect();
    values.map_or(StringListOption::Unreadable, StringListOption::Values)
}

/// Read the options of the last call to the default export of `package` in
/// a Vite-style `plugins` array. `read` gets `None` for a call without
/// arguments. A call whose first argument is not an object literal is
/// skipped, because its options are unknown.
fn read_vite_plugin_call_options<T: Send>(
    source: &str,
    path: &Path,
    package: &str,
    read: impl Fn(Option<&ObjectExpression<'_>>) -> Option<T> + Send + Sync,
) -> Option<T> {
    extract_from_source(source, path, |program| {
        let imports = collect_plugin_import_bindings(program, package);
        if imports.is_empty() {
            return None;
        }
        super::read_last_match(program, |obj| {
            let Expression::ArrayExpression(plugin_array) =
                get_nested_expression(obj, &["plugins"])?
            else {
                return None;
            };
            plugin_array.elements.iter().find_map(|element| {
                let Some(Expression::CallExpression(call)) = element.as_expression() else {
                    return None;
                };
                let call = find_plugin_binding_call(call, &imports, 0)?;
                match call.arguments.first().map(Argument::as_expression) {
                    None => read(None),
                    Some(Some(Expression::ObjectExpression(options))) => read(Some(options)),
                    Some(_) => None,
                }
            })
        })
    })
}

/// Find the call to a plugin binding, looking through wrapper calls such as
/// `profile(plugin({ ... }))`.
fn find_plugin_binding_call<'a>(
    call: &'a CallExpression<'a>,
    imports: &PluginImportBindings,
    depth: usize,
) -> Option<&'a CallExpression<'a>> {
    if is_plugin_binding_call(call, imports) {
        return Some(call);
    }
    if depth >= MAX_VITE_PLUGIN_WRAPPER_DEPTH {
        return None;
    }
    call.arguments
        .iter()
        .find_map(|argument| match argument.as_expression() {
            Some(Expression::CallExpression(inner)) => {
                find_plugin_binding_call(inner, imports, depth + 1)
            }
            _ => None,
        })
}

/// Whether `call` calls the default export of the plugin package, through a
/// default import or a namespace import.
pub(super) fn is_plugin_binding_call(
    call: &CallExpression<'_>,
    imports: &PluginImportBindings,
) -> bool {
    match &call.callee {
        Expression::Identifier(identifier) => imports
            .callables
            .iter()
            .any(|name| name == identifier.name.as_str()),
        Expression::StaticMemberExpression(member) if matches!(&member.object, Expression::Identifier(object) if imports.namespaces.iter().any(|name| name == object.name.as_str())) => {
            member.property.name == "default"
        }
        _ => false,
    }
}
