//! Read the options of a known plugin call in a Vite-style `plugins` array.

use std::path::Path;

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;

use super::{
    MAX_VITE_PLUGIN_WRAPPER_DEPTH, PluginImportBindings, collect_plugin_import_bindings,
    expression_to_path_string, extract_from_source, find_config_object, get_nested_expression,
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
    extract_from_source(source, path, |program| {
        let imports = collect_plugin_import_bindings(program, package);
        if imports.is_empty() {
            return None;
        }
        let obj = find_config_object(program)?;
        let Expression::ArrayExpression(plugin_array) = get_nested_expression(obj, &["plugins"])?
        else {
            return None;
        };
        plugin_array.elements.iter().find_map(|element| {
            let Some(Expression::CallExpression(call)) = element.as_expression() else {
                return None;
            };
            let call = find_plugin_binding_call(call, &imports, 0)?;
            let Some(Expression::ObjectExpression(options)) =
                call.arguments.first().and_then(Argument::as_expression)
            else {
                return None;
            };
            expression_to_path_string(get_nested_expression(options, option_path)?)
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
