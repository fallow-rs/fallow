//! AST-based config file parser utilities.
//!
//! Helpers for statically extracting config values from JS/TS files.

use std::path::{Path, PathBuf};

use fallow_extract::visitor::extract_import_from_callable;
use oxc_allocator::Allocator;
#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

/// Extract all import source specifiers from JS/TS source code.
#[must_use]
pub fn extract_imports(source: &str, path: &Path) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let mut sources = Vec::new();
        for stmt in &program.body {
            if let Statement::ImportDeclaration(decl) = stmt {
                sources.push(decl.source.value.to_string());
            }
        }
        Some(sources)
    })
    .unwrap_or_default()
}

/// Extract import sources and top-level `require('...')` statements.
#[must_use]
pub fn extract_imports_and_requires(source: &str, path: &Path) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let mut sources = Vec::new();
        for stmt in &program.body {
            match stmt {
                Statement::ImportDeclaration(decl) => {
                    sources.push(decl.source.value.to_string());
                }
                Statement::ExpressionStatement(expr) => {
                    if let Expression::CallExpression(call) = &expr.expression
                        && is_require_call(call)
                        && let Some(s) = get_require_source(call)
                    {
                        sources.push(s);
                    }
                }
                _ => {}
            }
        }
        Some(sources)
    })
    .unwrap_or_default()
}

/// Extract string array from a property at a nested path in a config's default export.
#[must_use]
pub fn extract_config_string_array(source: &str, path: &Path, prop_path: &[&str]) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        get_nested_string_array_from_object(obj, prop_path)
    })
    .unwrap_or_default()
}

/// Extract a single string from a property at a nested path.
#[must_use]
pub fn extract_config_string(source: &str, path: &Path, prop_path: &[&str]) -> Option<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        get_nested_string_from_object(obj, prop_path)
    })
}

/// Extract a shell command string from a property at a nested path.
#[must_use]
pub fn extract_config_command(source: &str, path: &Path, prop_path: &[&str]) -> Option<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        get_nested_command_from_object(obj, prop_path)
    })
}

/// Extract string values from top-level properties of the default export or
/// `module.exports` object.
#[must_use]
pub fn extract_config_property_strings(source: &str, path: &Path, key: &str) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let mut values = Vec::new();
        if let Some(prop) = find_property(obj, key) {
            collect_all_string_values(&prop.value, &mut values);
        }
        Some(values)
    })
    .unwrap_or_default()
}

/// Extract only top-level string values from a property's array.
#[must_use]
pub fn extract_config_shallow_strings(source: &str, path: &Path, key: &str) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let prop = find_property(obj, key)?;
        Some(collect_shallow_string_values(&prop.value))
    })
    .unwrap_or_default()
}

/// Extract top-level string values from a config array, including object entries.
#[must_use]
pub fn extract_config_shallow_strings_or_object_property(
    source: &str,
    path: &Path,
    key: &str,
    object_property: &str,
) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let prop = find_property(obj, key)?;
        Some(collect_shallow_string_or_object_property_values(
            &prop.value,
            object_property,
        ))
    })
    .unwrap_or_default()
}

/// Extract shallow strings from an array property inside a nested object path.
#[must_use]
pub fn extract_config_nested_shallow_strings(
    source: &str,
    path: &Path,
    outer_path: &[&str],
    key: &str,
) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let nested = get_nested_expression(obj, outer_path)?;
        if let Expression::ObjectExpression(nested_obj) = nested {
            let prop = find_property(nested_obj, key)?;
            Some(collect_shallow_string_values(&prop.value))
        } else {
            None
        }
    })
    .unwrap_or_default()
}

/// Public wrapper for `find_config_object`.
pub fn find_config_object_pub<'a>(program: &'a Program) -> Option<&'a ObjectExpression<'a>> {
    find_config_object(program)
}

/// Get a top-level property expression from an object.
pub(crate) fn property_expr<'a>(
    obj: &'a ObjectExpression<'a>,
    key: &str,
) -> Option<&'a Expression<'a>> {
    find_property(obj, key).map(|prop| &prop.value)
}

/// Get a top-level property object from an object.
pub(crate) fn property_object<'a>(
    obj: &'a ObjectExpression<'a>,
    key: &str,
) -> Option<&'a ObjectExpression<'a>> {
    property_expr(obj, key).and_then(object_expression)
}

/// Get a string-like top-level property value from an object.
pub(crate) fn property_string(obj: &ObjectExpression<'_>, key: &str) -> Option<String> {
    property_expr(obj, key).and_then(expression_to_string)
}

/// Convert an expression to an object expression when it is statically recoverable.
pub(crate) fn object_expression<'a>(expr: &'a Expression<'a>) -> Option<&'a ObjectExpression<'a>> {
    match expr {
        Expression::ObjectExpression(obj) => Some(obj),
        Expression::ParenthesizedExpression(paren) => object_expression(&paren.expression),
        Expression::TSSatisfiesExpression(ts_sat) => object_expression(&ts_sat.expression),
        Expression::TSAsExpression(ts_as) => object_expression(&ts_as.expression),
        _ => None,
    }
}

/// Convert an expression to an array expression when it is statically recoverable.
pub(crate) fn array_expression<'a>(expr: &'a Expression<'a>) -> Option<&'a ArrayExpression<'a>> {
    match expr {
        Expression::ArrayExpression(arr) => Some(arr),
        Expression::ParenthesizedExpression(paren) => array_expression(&paren.expression),
        Expression::TSSatisfiesExpression(ts_sat) => array_expression(&ts_sat.expression),
        Expression::TSAsExpression(ts_as) => array_expression(&ts_as.expression),
        _ => None,
    }
}

/// Convert a config path string to a `PathBuf` with platform-independent
/// separator handling.
pub(crate) fn path_from_config_string(raw: &str) -> PathBuf {
    PathBuf::from(raw.replace('\\', "/"))
}

/// Convert a config path to the forward-slash string form used in plugin output.
pub(crate) fn path_to_config_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Convert a path-like expression to a statically recoverable path.
pub(crate) fn expression_to_path(expr: &Expression<'_>) -> Option<PathBuf> {
    expression_to_path_string(expr).map(|path| path_from_config_string(&path))
}

/// Convert a path-like expression to zero or more statically recoverable paths.
pub(crate) fn expression_to_path_values(expr: &Expression<'_>) -> Vec<PathBuf> {
    match expr {
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|element| element.as_expression().and_then(expression_to_path))
            .collect(),
        _ => expression_to_path(expr).into_iter().collect(),
    }
}

/// True when an expression explicitly disables a config section.
pub(crate) fn is_disabled_expression(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::BooleanLiteral(boolean) if !boolean.value)
        || matches!(expr, Expression::NullLiteral(_))
}

/// True when a nested config property is a static `true` boolean or object value.
#[must_use]
pub fn extract_config_truthy_bool_or_object(source: &str, path: &Path, prop_path: &[&str]) -> bool {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let expr = get_nested_expression(obj, prop_path)?;
        Some(is_truthy_bool_or_object(expr))
    })
    .unwrap_or(false)
}

fn is_truthy_bool_or_object(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::BooleanLiteral(boolean) => boolean.value,
        Expression::ObjectExpression(_) => true,
        Expression::ParenthesizedExpression(paren) => is_truthy_bool_or_object(&paren.expression),
        Expression::TSSatisfiesExpression(ts_sat) => is_truthy_bool_or_object(&ts_sat.expression),
        Expression::TSAsExpression(ts_as) => is_truthy_bool_or_object(&ts_as.expression),
        _ => false,
    }
}

/// Extract keys of an object property at a nested path.
#[must_use]
pub fn extract_config_object_keys(source: &str, path: &Path, prop_path: &[&str]) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        get_nested_object_keys(obj, prop_path)
    })
    .unwrap_or_default()
}

/// Extract a value that may be a single string, string array, or object with
/// string/array values.
#[must_use]
pub fn extract_config_string_or_array(
    source: &str,
    path: &Path,
    prop_path: &[&str],
) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        get_nested_string_or_array(obj, prop_path)
    })
    .unwrap_or_default()
}

/// Extract a statically recoverable path-like value from a property path.
#[must_use]
pub fn extract_config_path(source: &str, path: &Path, prop_path: &[&str]) -> Option<PathBuf> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let expr = get_nested_expression(obj, prop_path)?;
        expression_to_path(expr)
    })
}

/// Extract string values from a property path, also searching inside array elements.
#[must_use]
pub fn extract_config_array_nested_string_or_array(
    source: &str,
    path: &Path,
    array_path: &[&str],
    inner_path: &[&str],
) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let array_expr = get_nested_expression(obj, array_path)?;
        let Expression::ArrayExpression(arr) = array_expr else {
            return None;
        };
        let mut results = Vec::new();
        for element in &arr.elements {
            if let Some(Expression::ObjectExpression(element_obj)) = element.as_expression()
                && let Some(values) = get_nested_string_or_array(element_obj, inner_path)
            {
                results.extend(values);
            }
        }
        if results.is_empty() {
            None
        } else {
            Some(results)
        }
    })
    .unwrap_or_default()
}

/// Extract string values from a property path, searching inside all values of an object.
#[must_use]
pub fn extract_config_object_nested_string_or_array(
    source: &str,
    path: &Path,
    object_path: &[&str],
    inner_path: &[&str],
) -> Vec<String> {
    extract_config_object_nested(source, path, object_path, |value_obj| {
        get_nested_string_or_array(value_obj, inner_path)
    })
}

/// Extract a single string value from each object under a property path.
#[must_use]
pub fn extract_config_object_nested_strings(
    source: &str,
    path: &Path,
    object_path: &[&str],
    inner_path: &[&str],
) -> Vec<String> {
    extract_config_object_nested(source, path, object_path, |value_obj| {
        get_nested_string_from_object(value_obj, inner_path).map(|s| vec![s])
    })
}

/// Shared helper for object-nested extraction.
fn extract_config_object_nested(
    source: &str,
    path: &Path,
    object_path: &[&str],
    extract_fn: impl Fn(&ObjectExpression<'_>) -> Option<Vec<String>>,
) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let obj_expr = get_nested_expression(obj, object_path)?;
        let Expression::ObjectExpression(target_obj) = obj_expr else {
            return None;
        };
        let mut results = Vec::new();
        for prop in &target_obj.properties {
            if let ObjectPropertyKind::ObjectProperty(p) = prop
                && let Expression::ObjectExpression(value_obj) = &p.value
                && let Some(values) = extract_fn(value_obj)
            {
                results.extend(values);
            }
        }
        if results.is_empty() {
            None
        } else {
            Some(results)
        }
    })
    .unwrap_or_default()
}

/// Extract `require('...')` call argument strings from a property's value.
#[must_use]
pub fn extract_config_require_strings(source: &str, path: &Path, key: &str) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let prop = find_property(obj, key)?;
        Some(collect_require_sources(&prop.value))
    })
    .unwrap_or_default()
}

/// Extract alias mappings from an object or array-based alias config.
#[must_use]
pub fn extract_config_aliases(
    source: &str,
    path: &Path,
    prop_path: &[&str],
) -> Vec<(String, String)> {
    extract_config_aliases_kinded(source, path, prop_path)
        .into_iter()
        .map(|(find, replacement, _is_bare)| (find, replacement))
        .collect()
}

/// Extract alias mappings where the replacement is a filesystem path value.
#[must_use]
pub fn extract_config_path_aliases(
    source: &str,
    path: &Path,
    prop_path: &[&str],
) -> Vec<(String, PathBuf)> {
    extract_config_aliases_kinded(source, path, prop_path)
        .into_iter()
        .map(|(find, replacement, _is_bare)| (find, path_from_config_string(&replacement)))
        .collect()
}

/// Extract alias mappings nested inside an array of config objects.
#[must_use]
pub fn extract_config_array_nested_aliases(
    source: &str,
    path: &Path,
    array_path: &[&str],
    alias_path: &[&str],
) -> Vec<(String, String)> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let array_expr = get_nested_expression(obj, array_path)?;
        let Expression::ArrayExpression(arr) = array_expr else {
            return None;
        };
        let mut results = Vec::new();
        for element in &arr.elements {
            if let Some(Expression::ObjectExpression(element_obj)) = element.as_expression()
                && let Some(alias_expr) = get_nested_expression(element_obj, alias_path)
            {
                results.extend(expression_to_alias_pairs(alias_expr));
            }
        }
        (!results.is_empty()).then_some(results)
    })
    .unwrap_or_default()
}

/// Like [`extract_config_aliases`] but each tuple carries a bare-string flag.
#[must_use]
pub fn extract_config_aliases_kinded(
    source: &str,
    path: &Path,
    prop_path: &[&str],
) -> Vec<(String, String, bool)> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let expr = get_nested_expression(obj, prop_path)?;
        let mut visited = FxHashSet::default();
        let aliases = resolve_alias_pairs_kinded(program, path, expr, &mut visited, 0);
        (!aliases.is_empty()).then_some(aliases)
    })
    .unwrap_or_default()
}

/// Kinded variant of [`extract_config_array_nested_aliases`].
#[must_use]
pub fn extract_config_array_nested_aliases_kinded(
    source: &str,
    path: &Path,
    array_path: &[&str],
    alias_path: &[&str],
) -> Vec<(String, String, bool)> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let array_expr = get_nested_expression(obj, array_path)?;
        let Expression::ArrayExpression(arr) = array_expr else {
            return None;
        };
        let mut results = Vec::new();
        for element in &arr.elements {
            if let Some(Expression::ObjectExpression(element_obj)) = element.as_expression()
                && let Some(alias_expr) = get_nested_expression(element_obj, alias_path)
            {
                results.extend(expression_to_alias_pairs_kinded(alias_expr));
            }
        }
        (!results.is_empty()).then_some(results)
    })
    .unwrap_or_default()
}

/// Extract kinded aliases from a default-exported ARRAY config.
#[must_use]
pub fn extract_default_export_array_aliases_kinded(
    source: &str,
    path: &Path,
    alias_path: &[&str],
) -> Vec<(String, String, bool)> {
    extract_from_source(source, path, |program| {
        let arr = find_default_export_array(program)?;
        let mut results = Vec::new();
        for element in &arr.elements {
            if let Some(Expression::ObjectExpression(element_obj)) = element.as_expression()
                && let Some(alias_expr) = get_nested_expression(element_obj, alias_path)
            {
                results.extend(expression_to_alias_pairs_kinded(alias_expr));
            }
        }
        (!results.is_empty()).then_some(results)
    })
    .unwrap_or_default()
}

/// True when a parsed config has neither an object nor array default export.
#[must_use]
pub fn config_default_export_unreachable(source: &str, path: &Path) -> bool {
    extract_from_source(source, path, |program| {
        let reachable =
            find_config_object(program).is_some() || find_default_export_array(program).is_some();
        Some(reachable)
    })
    .is_some_and(|reachable| !reachable)
}

/// Extract string values from a nested array, supporting both string elements and
/// object elements with a named string/path field.
///
/// Useful for configs like:
/// - `components: ["~/components", { path: "~/feature-components" }]`
#[must_use]
pub fn extract_config_array_object_strings(
    source: &str,
    path: &Path,
    array_path: &[&str],
    key: &str,
) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let array_expr = get_nested_expression(obj, array_path)?;
        let Expression::ArrayExpression(arr) = array_expr else {
            return None;
        };

        let mut results = Vec::new();
        for element in &arr.elements {
            let Some(expr) = element.as_expression() else {
                continue;
            };
            match expr {
                Expression::ObjectExpression(item) => {
                    if let Some(prop) = find_property(item, key)
                        && let Some(value) = expression_to_path_string(&prop.value)
                    {
                        results.push(value);
                    }
                }
                _ => {
                    if let Some(value) = expression_to_path_string(expr) {
                        results.push(value);
                    }
                }
            }
        }

        (!results.is_empty()).then_some(results)
    })
    .unwrap_or_default()
}

/// Extract Storybook-style static directory entries from an array.
///
/// Supports string entries and object entries with a string-like `from` plus
/// optional string-like `to`.
#[must_use]
pub fn extract_config_static_dir_entries(
    source: &str,
    path: &Path,
    array_path: &[&str],
) -> Vec<(String, Option<String>)> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let array_expr = get_nested_expression(obj, array_path)?;
        let Expression::ArrayExpression(arr) = array_expr else {
            return None;
        };

        let mut results = Vec::new();
        for element in &arr.elements {
            let Some(expr) = element.as_expression() else {
                continue;
            };
            match expr {
                Expression::ObjectExpression(item) => {
                    if let Some(from) = property_string(item, "from") {
                        let to = property_string(item, "to");
                        results.push((from, to));
                    }
                }
                _ => {
                    if let Some(from) = expression_to_path_string(expr) {
                        results.push((from, None));
                    }
                }
            }
        }

        (!results.is_empty()).then_some(results)
    })
    .unwrap_or_default()
}

/// Extract paired `(primary, optional secondary)` string values from each object
/// element of an array at `array_path`.
///
/// Mirrors [`extract_config_array_object_strings`] but keeps a per-element
/// secondary value alongside the primary one, so correlated fields stay paired.
/// An element is included only when its `primary_key` resolves to a recoverable
/// path string; the `secondary_key` is `None` when absent or non-recoverable.
///
/// Used for Playwright's `webServer: [{ command, cwd }]` form where each
/// `command` must be resolved relative to its own `cwd`.
#[must_use]
pub fn extract_config_array_object_string_pairs(
    source: &str,
    path: &Path,
    array_path: &[&str],
    primary_key: &str,
    secondary_key: &str,
) -> Vec<(String, Option<String>)> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let array_expr = get_nested_expression(obj, array_path)?;
        let Expression::ArrayExpression(arr) = array_expr else {
            return None;
        };

        let mut results = Vec::new();
        for element in &arr.elements {
            let Some(Expression::ObjectExpression(item)) = element.as_expression() else {
                continue;
            };
            let Some(primary) = find_property(item, primary_key)
                .and_then(|prop| expression_to_path_string(&prop.value))
            else {
                continue;
            };
            let secondary = find_property(item, secondary_key)
                .and_then(|prop| expression_to_path_string(&prop.value));
            results.push((primary, secondary));
        }

        (!results.is_empty()).then_some(results)
    })
    .unwrap_or_default()
}

/// Extract paired shell command and string values from each object element of an array.
#[must_use]
pub fn extract_config_array_object_command_pairs(
    source: &str,
    path: &Path,
    array_path: &[&str],
    primary_key: &str,
    secondary_key: &str,
) -> Vec<(String, Option<String>)> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let array_expr = get_nested_expression(obj, array_path)?;
        let Expression::ArrayExpression(arr) = array_expr else {
            return None;
        };

        let mut results = Vec::new();
        for element in &arr.elements {
            let Some(Expression::ObjectExpression(item)) = element.as_expression() else {
                continue;
            };
            let Some(primary) = find_property(item, primary_key)
                .and_then(|prop| expression_to_command(&prop.value))
            else {
                continue;
            };
            let secondary = find_property(item, secondary_key)
                .and_then(|prop| expression_to_path_string(&prop.value));
            results.push((primary, secondary));
        }

        (!results.is_empty()).then_some(results)
    })
    .unwrap_or_default()
}

/// Extract static specifiers from thunk-wrapped dynamic imports inside an
/// array property.
///
/// Captures the `SPEC` argument from each `() => import('SPEC')` element of
/// an array nested under `prop_path` in the config's default-exported object.
///
/// # The pattern
///
/// Configs and registries that need to defer module evaluation commonly hold
/// arrays of *thunks* — zero-argument arrow functions whose body is a single
/// dynamic import:
///
/// ```ts
/// export default defineConfig({
///     modules: [
///         () => import('./feature-a'),
///         { file: () => import('./feature-b'), enabled: true },
///     ],
/// })
/// ```
///
/// `import('SPEC')` is the ECMAScript dynamic-import expression (TC39
/// dynamic-import proposal, shipped in ES2020): a runtime module loader call
/// that returns a `Promise<Module>`. Wrapping it in `() => import('SPEC')`
/// turns "load module X now" into "value that, when invoked, loads module X"
/// — a thunk the host can call lazily.
///
/// The technique predates any single framework. It's the same shape used by
/// route-level code-splitting (`Vue Router`, `React Router`, `Next.js`),
/// `React.lazy`, Webpack's documented dynamic-import code-splitting recipes,
/// and any registry that wants to keep boot cheap, break import cycles, or
/// let bundlers tree-shake unused branches. Configs that adopt the pattern
/// can therefore declare large module graphs without forcing eager
/// evaluation of every entry at config parse time.
///
/// # Recognised array element shapes
///
/// - Concise arrow: `() => import('SPEC')`
/// - Block-body arrow with explicit return: `() => { return import('SPEC') }`
/// - Object form with a `file` property holding the arrow:
///   `{ file: () => import('SPEC'), /* peer fields */ }`
///
/// Non-matching elements (string literals, variables, template-string
/// specifiers, computed expressions) are silently skipped: callers receive
/// only the statically-resolvable specifiers, in source order.
#[must_use]
pub fn extract_lazy_imports_in_array(source: &str, path: &Path, prop_path: &[&str]) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let array_expr = get_nested_expression(obj, prop_path)?;
        let Expression::ArrayExpression(arr) = array_expr else {
            return None;
        };
        let mut specs = Vec::new();
        for element in &arr.elements {
            let Some(expr) = element.as_expression() else {
                continue;
            };
            if let Some(spec) = lazy_import_specifier(expr) {
                specs.push(spec);
            }
        }
        (!specs.is_empty()).then_some(specs)
    })
    .unwrap_or_default()
}

/// Read a lazy-import specifier from a single array element expression.
///
/// Two outer shapes are accepted at this level (array-element navigation):
/// - A bare callable: `() => import('SPEC')` or the function-expression
///   equivalent.
/// - An object with a `file` property holding the callable:
///   `{ file: () => import('SPEC'), /* peer fields */ }`.
///
/// The actual callable → import peeling is delegated to
/// [`extract_import_from_callable`], which is shared with the visitor-side
/// dynamic-import helpers so all three navigation pipelines stay in lockstep
/// when ECMAScript adds new wrapper shapes.
fn lazy_import_specifier(expr: &Expression<'_>) -> Option<String> {
    let callable = match expr {
        Expression::ObjectExpression(obj) => &find_property(obj, "file")?.value,
        _ => expr,
    };
    let import_expr = extract_import_from_callable(callable)?;
    expression_to_string(&import_expr.source)
}

/// Extract a string-like option from a plugin tuple inside a config plugin array.
///
/// Supports config shapes like:
/// - `{ expo: { plugins: [["expo-router", { root: "src/app" }]] } }`
/// - `export default { expo: { plugins: [["expo-router", { root: "./src/app" }]] } }`
/// - `{ plugins: [["expo-router", { root: "./src/routes" }]] }`
#[must_use]
pub fn extract_config_plugin_option_string(
    source: &str,
    path: &Path,
    plugins_path: &[&str],
    plugin_name: &str,
    option_key: &str,
) -> Option<String> {
    extract_from_source(source, path, |program| {
        let obj = find_config_object(program)?;
        let plugins_expr = get_nested_expression(obj, plugins_path)?;
        let Expression::ArrayExpression(plugins) = plugins_expr else {
            return None;
        };

        for entry in &plugins.elements {
            let Some(Expression::ArrayExpression(tuple)) = entry.as_expression() else {
                continue;
            };
            let Some(plugin_expr) = tuple
                .elements
                .first()
                .and_then(ArrayExpressionElement::as_expression)
            else {
                continue;
            };
            if expression_to_string(plugin_expr).as_deref() != Some(plugin_name) {
                continue;
            }

            let Some(options_expr) = tuple
                .elements
                .get(1)
                .and_then(ArrayExpressionElement::as_expression)
            else {
                continue;
            };
            let Expression::ObjectExpression(options_obj) = options_expr else {
                continue;
            };
            let option = find_property(options_obj, option_key)?;
            return expression_to_path_string(&option.value);
        }

        None
    })
}

/// Extract a string-like option from the first plugin array path that contains it.
#[must_use]
pub fn extract_config_plugin_option_string_from_paths(
    source: &str,
    path: &Path,
    plugin_paths: &[&[&str]],
    plugin_name: &str,
    option_key: &str,
) -> Option<String> {
    plugin_paths.iter().find_map(|plugins_path| {
        extract_config_plugin_option_string(source, path, plugins_path, plugin_name, option_key)
    })
}

/// Extract Babel plugin and preset package names configured through
/// `@vitejs/plugin-react` options in a Vite-style `plugins` array.
#[must_use]
pub fn extract_vite_react_babel_dependencies(source: &str, path: &Path) -> Vec<String> {
    extract_from_source(source, path, |program| {
        let react_plugin_imports = collect_vite_react_plugin_imports(program);
        if react_plugin_imports.is_empty() {
            return None;
        }

        let obj = find_config_object(program)?;
        let plugins = get_nested_expression(obj, &["plugins"])?;
        let Expression::ArrayExpression(plugin_array) = plugins else {
            return None;
        };

        let mut deps = Vec::new();
        for element in &plugin_array.elements {
            let Some(Expression::CallExpression(call)) = element.as_expression() else {
                continue;
            };
            if !is_vite_react_plugin_call(call, &react_plugin_imports) {
                continue;
            }
            let Some(Expression::ObjectExpression(options)) =
                call.arguments.first().and_then(Argument::as_expression)
            else {
                continue;
            };
            collect_vite_react_babel_dependencies(options, &mut deps);
        }

        (!deps.is_empty()).then_some(deps)
    })
    .unwrap_or_default()
}

/// Normalize a config-relative path to a project-root-relative path.
///
/// Handles values extracted from config files such as `"./src"`, `"src/lib"`,
/// `"/src"`, or absolute filesystem paths under `root`.
#[must_use]
pub fn normalize_config_path_buf(
    raw: impl AsRef<Path>,
    config_path: &Path,
    root: &Path,
) -> Option<PathBuf> {
    let raw = raw.as_ref();
    if raw.as_os_str().is_empty() {
        return None;
    }

    let raw_string = path_to_config_string(raw);
    let raw_path = Path::new(&raw_string);
    let candidate = if let Some(stripped) = raw_string.strip_prefix('/') {
        lexical_normalize(&root.join(stripped))
    } else if raw_path.is_absolute() {
        lexical_normalize(raw_path)
    } else {
        let base = config_path.parent().unwrap_or(root);
        lexical_normalize(&base.join(raw_path))
    };

    let relative = candidate.strip_prefix(root).ok()?;
    (!relative.as_os_str().is_empty()).then(|| relative.to_path_buf())
}

/// Normalize a config-relative path to a project-root-relative forward-slash string.
#[must_use]
pub fn normalize_config_path(
    raw: impl AsRef<Path>,
    config_path: &Path,
    root: &Path,
) -> Option<String> {
    normalize_config_path_buf(raw, config_path, root).map(|path| path_to_config_string(&path))
}

/// Parse source and run an extraction function on the AST.
///
/// JSON files (`.json`, `.jsonc`) are parsed as JavaScript expressions wrapped in
/// parentheses to produce an AST compatible with `find_config_object`. The native
/// JSON source type in Oxc produces a different AST structure that our helpers
/// don't handle.
pub(crate) fn extract_from_source<T>(
    source: &str,
    path: &Path,
    extractor: impl FnOnce(&Program) -> Option<T>,
) -> Option<T> {
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let alloc = Allocator::default();

    let is_json = path
        .extension()
        .is_some_and(|ext| ext == "json" || ext == "jsonc");
    if is_json {
        let wrapped = format!("({source})");
        let parsed = Parser::new(&alloc, &wrapped, SourceType::mjs()).parse();
        return extractor(&parsed.program);
    }

    let parsed = Parser::new(&alloc, source, source_type).parse();
    extractor(&parsed.program)
}

#[derive(Default)]
struct ViteReactPluginImports {
    callables: Vec<String>,
    namespaces: Vec<String>,
}

impl ViteReactPluginImports {
    fn is_empty(&self) -> bool {
        self.callables.is_empty() && self.namespaces.is_empty()
    }
}

fn collect_vite_react_plugin_imports(program: &Program<'_>) -> ViteReactPluginImports {
    let mut imports = ViteReactPluginImports::default();

    for stmt in &program.body {
        let Statement::ImportDeclaration(decl) = stmt else {
            continue;
        };
        if decl.source.value != "@vitejs/plugin-react" {
            continue;
        }
        let Some(specifiers) = &decl.specifiers else {
            continue;
        };
        for specifier in specifiers {
            match specifier {
                ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                    push_unique_string(&mut imports.callables, specifier.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportSpecifier(specifier)
                    if specifier.imported.name().as_ref() == "default" =>
                {
                    push_unique_string(&mut imports.callables, specifier.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                    push_unique_string(&mut imports.namespaces, specifier.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportSpecifier(_) => {}
            }
        }
    }

    imports
}

fn is_vite_react_plugin_call(call: &CallExpression<'_>, imports: &ViteReactPluginImports) -> bool {
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

fn collect_vite_react_babel_dependencies(options: &ObjectExpression<'_>, deps: &mut Vec<String>) {
    let Some(babel) = property_object(options, "babel") else {
        return;
    };
    for key in ["plugins", "presets"] {
        let Some(prop) = find_property(babel, key) else {
            continue;
        };
        for raw in collect_shallow_string_values(&prop.value) {
            if let Some(dep) = vite_react_babel_dependency_name(&raw) {
                push_unique_string(deps, dep);
            }
        }
    }
}

fn vite_react_babel_dependency_name(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let specifier = raw.strip_prefix("module:").unwrap_or(raw).trim();
    if specifier.is_empty()
        || specifier.starts_with('.')
        || specifier.starts_with('/')
        || specifier.contains(':')
        || specifier.contains('\\')
    {
        return None;
    }
    Some(crate::resolve::extract_package_name(specifier))
}

fn push_unique_string(items: &mut Vec<String>, value: String) {
    if !items.contains(&value) {
        items.push(value);
    }
}

/// Find the "config object": the object expression in the default export or module.exports.
///
/// Handles these patterns:
/// - `export default { ... }`
/// - `export default defineConfig({ ... })`
/// - `export default defineConfig(async () => ({ ... }))`
/// - `export default { ... } satisfies Config` / `export default { ... } as Config`
/// - `const config = { ... }; export default config;`
/// - `const config: Config = { ... }; export default config;`
/// - `module.exports = { ... }`
/// - Top-level JSON object (for .json files)
fn find_config_object<'a>(program: &'a Program) -> Option<&'a ObjectExpression<'a>> {
    for stmt in &program.body {
        match stmt {
            Statement::ExportDefaultDeclaration(decl) => {
                let expr: Option<&Expression> = match &decl.declaration {
                    ExportDefaultDeclarationKind::ObjectExpression(obj) => {
                        return Some(obj);
                    }
                    ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                        return extract_object_from_function(func);
                    }
                    _ => decl.declaration.as_expression(),
                };
                if let Some(expr) = expr {
                    if let Some(obj) = extract_object_from_expression(expr) {
                        return Some(obj);
                    }
                    if let Some(name) = unwrap_to_identifier_name(expr) {
                        return find_variable_init_object(program, name);
                    }
                }
            }
            Statement::ExpressionStatement(expr_stmt) => {
                if let Expression::AssignmentExpression(assign) = &expr_stmt.expression
                    && is_module_exports_target(&assign.left)
                {
                    return extract_object_from_expression(&assign.right);
                }
            }
            _ => {}
        }
    }

    if program.body.len() == 1
        && let Statement::ExpressionStatement(expr_stmt) = &program.body[0]
    {
        match &expr_stmt.expression {
            Expression::ObjectExpression(obj) => return Some(obj),
            Expression::ParenthesizedExpression(paren) => {
                if let Expression::ObjectExpression(obj) = &paren.expression {
                    return Some(obj);
                }
            }
            _ => {}
        }
    }

    None
}

/// Extract an `ObjectExpression` from an expression, handling wrapper patterns.
fn extract_object_from_expression<'a>(
    expr: &'a Expression<'a>,
) -> Option<&'a ObjectExpression<'a>> {
    match expr {
        Expression::ObjectExpression(obj) => Some(obj),
        Expression::CallExpression(call) => {
            for arg in &call.arguments {
                match arg {
                    Argument::ObjectExpression(obj) => return Some(obj),
                    Argument::ArrowFunctionExpression(arrow) => {
                        if arrow.expression
                            && !arrow.body.statements.is_empty()
                            && let Statement::ExpressionStatement(expr_stmt) =
                                &arrow.body.statements[0]
                        {
                            return extract_object_from_expression(&expr_stmt.expression);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        Expression::ParenthesizedExpression(paren) => {
            extract_object_from_expression(&paren.expression)
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            extract_object_from_expression(&ts_sat.expression)
        }
        Expression::TSAsExpression(ts_as) => extract_object_from_expression(&ts_as.expression),
        Expression::ArrowFunctionExpression(arrow) => extract_object_from_arrow_function(arrow),
        Expression::FunctionExpression(func) => extract_object_from_function(func),
        _ => None,
    }
}

fn extract_object_from_arrow_function<'a>(
    arrow: &'a ArrowFunctionExpression<'a>,
) -> Option<&'a ObjectExpression<'a>> {
    if arrow.expression {
        arrow.body.statements.first().and_then(|stmt| {
            if let Statement::ExpressionStatement(expr_stmt) = stmt {
                extract_object_from_expression(&expr_stmt.expression)
            } else {
                None
            }
        })
    } else {
        extract_object_from_function_body(&arrow.body)
    }
}

fn extract_object_from_function<'a>(func: &'a Function<'a>) -> Option<&'a ObjectExpression<'a>> {
    func.body
        .as_ref()
        .and_then(|body| extract_object_from_function_body(body))
}

fn extract_object_from_function_body<'a>(
    body: &'a FunctionBody<'a>,
) -> Option<&'a ObjectExpression<'a>> {
    for stmt in &body.statements {
        if let Statement::ReturnStatement(ret) = stmt
            && let Some(argument) = &ret.argument
            && let Some(obj) = extract_object_from_expression(argument)
        {
            return Some(obj);
        }
    }
    None
}

/// Check if an assignment target is `module.exports`.
fn is_module_exports_target(target: &AssignmentTarget) -> bool {
    if let AssignmentTarget::StaticMemberExpression(member) = target
        && let Expression::Identifier(obj) = &member.object
    {
        return obj.name == "module" && member.property.name == "exports";
    }
    false
}

/// Unwrap TS annotations and return the identifier name if the expression resolves to one.
///
/// Handles `config`, `config satisfies Type`, `config as Type`.
fn unwrap_to_identifier_name<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::Identifier(id) => Some(&id.name),
        Expression::TSSatisfiesExpression(ts_sat) => unwrap_to_identifier_name(&ts_sat.expression),
        Expression::TSAsExpression(ts_as) => unwrap_to_identifier_name(&ts_as.expression),
        _ => None,
    }
}

/// Find a top-level variable declaration by name and extract its init as an object expression.
///
/// Handles `const config = { ... }`, `const config: Type = { ... }`,
/// and `const config = defineConfig({ ... })`.
fn find_variable_init_object<'a>(
    program: &'a Program,
    name: &str,
) -> Option<&'a ObjectExpression<'a>> {
    for stmt in &program.body {
        if let Statement::VariableDeclaration(decl) = stmt {
            for declarator in &decl.declarations {
                if let BindingPattern::BindingIdentifier(id) = &declarator.id
                    && id.name == name
                    && let Some(init) = &declarator.init
                {
                    return extract_object_from_expression(init);
                }
            }
        }
    }
    None
}

/// Find a named property in an object expression.
pub(crate) fn find_property<'a>(
    obj: &'a ObjectExpression<'a>,
    key: &str,
) -> Option<&'a ObjectProperty<'a>> {
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop
            && property_key_matches(&p.key, key)
        {
            return Some(p);
        }
    }
    None
}

/// Check if a property key matches a string.
pub(crate) fn property_key_matches(key: &PropertyKey, name: &str) -> bool {
    match key {
        PropertyKey::StaticIdentifier(id) => id.name == name,
        PropertyKey::StringLiteral(s) => s.value == name,
        _ => false,
    }
}

/// Get a string value from an object property.
fn get_object_string_property(obj: &ObjectExpression, key: &str) -> Option<String> {
    find_property(obj, key).and_then(|p| expression_to_string(&p.value))
}

/// Get an array of strings from an object property.
fn get_object_string_array_property(obj: &ObjectExpression, key: &str) -> Vec<String> {
    find_property(obj, key)
        .map(|p| expression_to_string_array(&p.value))
        .unwrap_or_default()
}

/// Navigate a nested property path and get a string array.
fn get_nested_string_array_from_object(
    obj: &ObjectExpression,
    path: &[&str],
) -> Option<Vec<String>> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return Some(get_object_string_array_property(obj, path[0]));
    }
    let prop = find_property(obj, path[0])?;
    if let Expression::ObjectExpression(nested) = &prop.value {
        get_nested_string_array_from_object(nested, &path[1..])
    } else {
        None
    }
}

/// Navigate a nested property path and get a string value.
fn get_nested_string_from_object(obj: &ObjectExpression, path: &[&str]) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return get_object_string_property(obj, path[0]);
    }
    let prop = find_property(obj, path[0])?;
    if let Expression::ObjectExpression(nested) = &prop.value {
        get_nested_string_from_object(nested, &path[1..])
    } else {
        None
    }
}

/// Navigate a nested property path and get a shell command value.
fn get_nested_command_from_object(obj: &ObjectExpression, path: &[&str]) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        return find_property(obj, path[0]).and_then(|prop| expression_to_command(&prop.value));
    }
    let prop = find_property(obj, path[0])?;
    if let Expression::ObjectExpression(nested) = &prop.value {
        get_nested_command_from_object(nested, &path[1..])
    } else {
        None
    }
}

/// Convert an expression to a string if it's a string literal.
pub(crate) fn expression_to_string(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            t.quasis.first().map(|q| q.value.raw.to_string())
        }
        _ => None,
    }
}

/// Convert an expression to a shell command when static command tokens are recoverable.
fn expression_to_command(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::TemplateLiteral(template) => template_literal_to_command(template),
        Expression::ParenthesizedExpression(paren) => expression_to_command(&paren.expression),
        Expression::TSAsExpression(ts_as) => expression_to_command(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => expression_to_command(&ts_sat.expression),
        _ => None,
    }
}

fn template_literal_to_command(template: &TemplateLiteral<'_>) -> Option<String> {
    let first = template.quasis.first()?.value.raw.as_str();
    if first.trim_start().is_empty() {
        return None;
    }

    let mut command = String::new();
    for (idx, quasi) in template.quasis.iter().enumerate() {
        command.push_str(quasi.value.raw.as_str());
        if idx < template.expressions.len() {
            let next = template
                .quasis
                .get(idx + 1)
                .map_or("", |next| next.value.raw.as_str());
            if dynamic_template_boundary_splits_static_token(quasi.value.raw.as_str(), next) {
                return None;
            }
            command.push(' ');
        }
    }

    Some(command)
}

fn dynamic_template_boundary_splits_static_token(before: &str, after: &str) -> bool {
    before
        .chars()
        .next_back()
        .is_some_and(is_command_token_char)
        && after.chars().next().is_some_and(is_command_token_char)
}

fn is_command_token_char(ch: char) -> bool {
    !ch.is_whitespace() && !matches!(ch, '&' | '|' | ';' | '"' | '\'')
}

/// Convert an expression to a path-like string if it's statically recoverable.
pub(crate) fn expression_to_path_string(expr: &Expression) -> Option<String> {
    match expr {
        Expression::ParenthesizedExpression(paren) => expression_to_path_string(&paren.expression),
        Expression::TSAsExpression(ts_as) => expression_to_path_string(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => expression_to_path_string(&ts_sat.expression),
        Expression::StaticMemberExpression(member) if member.property.name == "pathname" => {
            expression_to_path_string(&member.object)
        }
        Expression::CallExpression(call) => call_expression_to_path_string(call),
        Expression::NewExpression(new_expr) => new_expression_to_path_string(new_expr),
        _ => expression_to_string(expr),
    }
}

fn call_expression_to_path_string(call: &CallExpression) -> Option<String> {
    if matches!(&call.callee, Expression::Identifier(id) if id.name == "fileURLToPath") {
        return call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(expression_to_path_string);
    }

    let callee_name = match &call.callee {
        Expression::Identifier(id) => Some(id.name.as_str()),
        Expression::StaticMemberExpression(member) => Some(member.property.name.as_str()),
        _ => None,
    }?;

    if !matches!(callee_name, "resolve" | "join") {
        return None;
    }

    let mut segments = Vec::new();
    for (index, arg) in call.arguments.iter().enumerate() {
        let expr = arg.as_expression()?;

        if is_dirname_anchor(expr) {
            if index == 0 {
                continue;
            }
            return None;
        }

        segments.push(expression_to_string(expr)?);
    }

    (!segments.is_empty()).then(|| join_path_segments(&segments))
}

/// True when an expression is a "current directory" anchor: the `__dirname`
/// CommonJS global or its ESM equivalent `import.meta.dirname` (Node 20.11+).
/// As the leading argument of `resolve(...)` / `join(...)` it is dropped so the
/// remaining literal segments yield a config-directory-relative path.
fn is_dirname_anchor(expr: &Expression) -> bool {
    match expr {
        Expression::Identifier(id) => id.name == "__dirname",
        Expression::StaticMemberExpression(member) => {
            member.property.name == "dirname" && is_import_meta_expression(&member.object)
        }
        _ => false,
    }
}

/// True for the `import.meta` meta-property, distinct from `new.target`.
fn is_import_meta_expression(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::MetaProperty(meta) if meta.meta.name == "import" && meta.property.name == "meta"
    )
}

fn new_expression_to_path_string(new_expr: &NewExpression) -> Option<String> {
    if !matches!(&new_expr.callee, Expression::Identifier(id) if id.name == "URL") {
        return None;
    }

    let source = new_expr
        .arguments
        .first()
        .and_then(Argument::as_expression)
        .and_then(expression_to_string)?;

    let base = new_expr
        .arguments
        .get(1)
        .and_then(Argument::as_expression)?;
    is_import_meta_url_expression(base).then_some(source)
}

fn is_import_meta_url_expression(expr: &Expression) -> bool {
    if let Expression::StaticMemberExpression(member) = expr {
        member.property.name == "url" && matches!(member.object, Expression::MetaProperty(_))
    } else {
        false
    }
}

fn join_path_segments(segments: &[String]) -> String {
    let mut joined = PathBuf::new();
    for segment in segments {
        joined.push(segment);
    }
    joined.to_string_lossy().replace('\\', "/")
}

fn expression_to_alias_pairs(expr: &Expression) -> Vec<(String, String)> {
    match expr {
        Expression::ObjectExpression(obj) => obj
            .properties
            .iter()
            .filter_map(|prop| {
                let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                    return None;
                };
                let find = property_key_to_string(&prop.key)?;
                let replacement = expression_to_path_values(&prop.value)
                    .into_iter()
                    .next()
                    .map(|path| path_to_config_string(&path))?;
                Some((find, replacement))
            })
            .collect(),
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|element| {
                let Expression::ObjectExpression(obj) = element.as_expression()? else {
                    return None;
                };
                let find = find_property(obj, "find")
                    .and_then(|prop| expression_to_string(&prop.value))?;
                let replacement = find_property(obj, "replacement")
                    .and_then(|prop| expression_to_path_string(&prop.value))?;
                Some((find, replacement))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Kinded variant of [`expression_to_alias_pairs`]: each tuple gains a
/// `replacement_is_bare_string_literal` flag. See
/// [`extract_config_aliases_kinded`].
fn expression_to_alias_pairs_kinded(expr: &Expression) -> Vec<(String, String, bool)> {
    match expr {
        Expression::ObjectExpression(obj) => obj
            .properties
            .iter()
            .filter_map(|prop| {
                let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                    return None;
                };
                let find = property_key_to_string(&prop.key)?;
                let (replacement, is_bare) = alias_replacement_kinded(&prop.value)?;
                Some((find, replacement, is_bare))
            })
            .collect(),
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|element| {
                let Expression::ObjectExpression(obj) = element.as_expression()? else {
                    return None;
                };
                let find = find_property(obj, "find")
                    .and_then(|prop| expression_to_string(&prop.value))?;
                let (replacement, is_bare) = find_property(obj, "replacement")
                    .and_then(|prop| alias_replacement_kinded(&prop.value))?;
                Some((find, replacement, is_bare))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Extract an alias replacement string plus whether it was written as a plain
/// bare string literal. A bare string literal (not starting with `./`/`../`/`/`)
/// signals a potential package-to-package alias; a path expression
/// (`path.resolve(...)`, `path.join(...)`, `fileURLToPath(...)`, `new URL(...)`)
/// or a `./`-prefixed string is always a filesystem path. This is the
/// filesystem-free discriminator the package-to-package gate relies on.
fn alias_replacement_kinded(expr: &Expression) -> Option<(String, bool)> {
    match expr {
        Expression::ParenthesizedExpression(paren) => alias_replacement_kinded(&paren.expression),
        Expression::TSAsExpression(ts_as) => alias_replacement_kinded(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => alias_replacement_kinded(&ts_sat.expression),
        Expression::StringLiteral(s) => {
            let value = s.value.to_string();
            let is_bare =
                !value.starts_with("./") && !value.starts_with("../") && !value.starts_with('/');
            Some((value, is_bare))
        }
        // tsconfig `compilerOptions.paths` maps each key to an ARRAY of targets
        // (`{ "@/*": ["./src/*"] }`); take the first entry, matching the prior
        // non-kinded `expression_to_path_values().next()` behavior.
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .find_map(ArrayExpressionElement::as_expression)
            .and_then(alias_replacement_kinded),
        _ => expression_to_path_string(expr).map(|value| (value, false)),
    }
}

/// Maximum identifier-indirection hops the alias resolver follows before giving
/// up. Each local-variable or imported-binding resolution counts one hop. The
/// per-file `visited` set is the real cycle guard; this bound additionally
/// terminates pathological local self-references (`const a = a`). Real configs
/// rarely exceed one or two hops (`alias: importedAliases`).
const MAX_ALIAS_RESOLVE_DEPTH: usize = 8;

/// Sibling-file extensions probed when an alias identifier is imported from a
/// relative specifier. Mirrors the JS/TS config extensions Vite/Vitest configs
/// and their shared alias modules use. `.js` first matches the common
/// JS-project case; the direct-as-written read happens before any probing. JSON
/// is intentionally excluded: it parses as a bare expression with no `export`,
/// so `find_exported_init` could never recover an alias literal from it.
const ALIAS_SIBLING_EXTS: [&str; 6] = ["js", "mjs", "cjs", "ts", "mts", "cts"];

/// Resolve an alias expression into `(find, replacement, is_bare)` tuples,
/// following identifiers and expanding spreads.
///
/// Beyond the inline object (`{ '@': './src' }`) and array
/// (`[{ find, replacement }]`) forms, this handles the indirection shapes from
/// issue #811:
/// - an identifier bound to a local `const NAME = [...] | {...}`,
/// - an identifier imported from a relative sibling file
///   (`import { sharedAliases } from "./vite.shared.js"`), read one hop and
///   parsed for `export const NAME` / `export default` / `export { NAME }`,
/// - array spread elements (`[...a, ...b]`) and object spread properties
///   (`{ ...a, '@': './src' }`), each resolved recursively.
///
/// `config_path` is the file `expr` lives in (used to resolve relative sibling
/// imports). `visited` holds already-read sibling paths to break import cycles;
/// `depth` bounds identifier indirection via [`MAX_ALIAS_RESOLVE_DEPTH`].
fn resolve_alias_pairs_kinded(
    program: &Program,
    config_path: &Path,
    expr: &Expression,
    visited: &mut FxHashSet<PathBuf>,
    depth: usize,
) -> Vec<(String, String, bool)> {
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            resolve_alias_pairs_kinded(program, config_path, &paren.expression, visited, depth)
        }
        Expression::TSAsExpression(ts_as) => {
            resolve_alias_pairs_kinded(program, config_path, &ts_as.expression, visited, depth)
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            resolve_alias_pairs_kinded(program, config_path, &ts_sat.expression, visited, depth)
        }
        Expression::ObjectExpression(obj) => {
            resolve_object_alias_pairs_kinded(program, config_path, obj, visited, depth)
        }
        Expression::ArrayExpression(arr) => {
            resolve_array_alias_pairs_kinded(program, config_path, arr, visited, depth)
        }
        Expression::Identifier(id) => {
            resolve_identifier_alias_pairs(program, config_path, id.name.as_str(), visited, depth)
        }
        _ => Vec::new(),
    }
}

/// Resolve object-form alias pairs (`{ '@': './src', ...spread }`), expanding
/// spread properties recursively.
fn resolve_object_alias_pairs_kinded(
    program: &Program,
    config_path: &Path,
    obj: &ObjectExpression,
    visited: &mut FxHashSet<PathBuf>,
    depth: usize,
) -> Vec<(String, String, bool)> {
    let mut pairs = Vec::new();
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(prop) => {
                if let Some(find) = property_key_to_string(&prop.key)
                    && let Some((replacement, is_bare)) = alias_replacement_kinded(&prop.value)
                {
                    pairs.push((find, replacement, is_bare));
                }
            }
            // `{ ...sharedAliases, '@': './src' }`
            ObjectPropertyKind::SpreadProperty(spread) => {
                pairs.extend(resolve_alias_pairs_kinded(
                    program,
                    config_path,
                    &spread.argument,
                    visited,
                    depth,
                ));
            }
        }
    }
    pairs
}

/// Resolve array-form alias pairs (`[{ find, replacement }, ...spread]`),
/// expanding spread elements recursively.
fn resolve_array_alias_pairs_kinded(
    program: &Program,
    config_path: &Path,
    arr: &ArrayExpression,
    visited: &mut FxHashSet<PathBuf>,
    depth: usize,
) -> Vec<(String, String, bool)> {
    let mut pairs = Vec::new();
    for element in &arr.elements {
        match element {
            // `[...sharedAliases, { find, replacement }]`
            ArrayExpressionElement::SpreadElement(spread) => {
                pairs.extend(resolve_alias_pairs_kinded(
                    program,
                    config_path,
                    &spread.argument,
                    visited,
                    depth,
                ));
            }
            _ => {
                if let Some(Expression::ObjectExpression(obj)) = element.as_expression()
                    && let Some(find) = find_property(obj, "find")
                        .and_then(|prop| expression_to_string(&prop.value))
                    && let Some((replacement, is_bare)) = find_property(obj, "replacement")
                        .and_then(|prop| alias_replacement_kinded(&prop.value))
                {
                    pairs.push((find, replacement, is_bare));
                }
            }
        }
    }
    pairs
}

/// Resolve an identifier used as an alias value to its literal pairs, first by
/// local `const`/`let`/`var` binding, then by a one-hop relative import.
fn resolve_identifier_alias_pairs(
    program: &Program,
    config_path: &Path,
    name: &str,
    visited: &mut FxHashSet<PathBuf>,
    depth: usize,
) -> Vec<(String, String, bool)> {
    if depth >= MAX_ALIAS_RESOLVE_DEPTH {
        return Vec::new();
    }
    // Local `const NAME = [...] | {...}` (or `const NAME = otherIdentifier`).
    if let Some(init) = find_variable_init_expression(program, name) {
        return resolve_alias_pairs_kinded(program, config_path, init, visited, depth + 1);
    }
    // `import { NAME } from "./sibling"` / `import NAME from "./sibling"`.
    let Some((specifier, imported_name)) = find_relative_import_binding(program, name) else {
        return Vec::new();
    };
    resolve_imported_alias_pairs(
        config_path,
        &specifier,
        imported_name.as_deref(),
        visited,
        depth + 1,
    )
}

/// Read a relative sibling file and resolve the alias literal it exports under
/// `imported_name` (`None` = default export).
fn resolve_imported_alias_pairs(
    config_path: &Path,
    specifier: &str,
    imported_name: Option<&str>,
    visited: &mut FxHashSet<PathBuf>,
    depth: usize,
) -> Vec<(String, String, bool)> {
    let Some((sibling_path, sibling_source)) = resolve_sibling_module(config_path, specifier)
    else {
        return Vec::new();
    };
    if !visited.insert(sibling_path.clone()) {
        return Vec::new();
    }
    extract_from_source(&sibling_source, &sibling_path, |program| {
        let init = find_exported_init(program, imported_name)?;
        let pairs = resolve_alias_pairs_kinded(program, &sibling_path, init, visited, depth);
        (!pairs.is_empty()).then_some(pairs)
    })
    .unwrap_or_default()
}

/// Find a top-level variable declaration by name and return its init expression
/// (array, object, or another identifier). Covers bare `const NAME = ...` and
/// `export const NAME = ...`. Generalizes [`find_variable_init_object`] to any
/// init shape so the alias resolver can recurse on array/identifier inits.
fn find_variable_init_expression<'a>(
    program: &'a Program<'a>,
    name: &str,
) -> Option<&'a Expression<'a>> {
    for stmt in &program.body {
        let decl = match stmt {
            Statement::VariableDeclaration(decl) => decl,
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(Declaration::VariableDeclaration(decl)) => decl,
                _ => continue,
            },
            _ => continue,
        };
        for declarator in &decl.declarations {
            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && id.name == name
                && let Some(init) = &declarator.init
            {
                return Some(init);
            }
        }
    }
    None
}

/// Find the init expression a sibling module exports under `name`
/// (`None` = default export). For named exports this covers both
/// `export const NAME = ...` and a local `const NAME = ...` later re-exported
/// via `export { NAME }` (both surface through [`find_variable_init_expression`]).
fn find_exported_init<'a>(
    program: &'a Program<'a>,
    name: Option<&str>,
) -> Option<&'a Expression<'a>> {
    match name {
        Some(name) => find_variable_init_expression(program, name),
        None => program.body.iter().find_map(|stmt| {
            if let Statement::ExportDefaultDeclaration(decl) = stmt {
                decl.declaration.as_expression()
            } else {
                None
            }
        }),
    }
}

/// Find the import that binds local `name` to a RELATIVE module, returning the
/// specifier and the imported name (`None` for a default import). Bare-package
/// imports are intentionally skipped: reading a literal alias table out of
/// `node_modules` is not a real-world config shape.
fn find_relative_import_binding(program: &Program, name: &str) -> Option<(String, Option<String>)> {
    for stmt in &program.body {
        let Statement::ImportDeclaration(decl) = stmt else {
            continue;
        };
        let specifier = decl.source.value.as_str();
        if !is_relative_specifier(specifier) {
            continue;
        }
        let Some(specifiers) = &decl.specifiers else {
            continue;
        };
        for spec in specifiers {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(spec) if spec.local.name == name => {
                    return Some((
                        specifier.to_string(),
                        Some(spec.imported.name().to_string()),
                    ));
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(spec)
                    if spec.local.name == name =>
                {
                    return Some((specifier.to_string(), None));
                }
                _ => {}
            }
        }
    }
    None
}

/// True for a relative/absolute module specifier (`./x`, `../x`, `/x`), the
/// shapes that point at a sibling file rather than an npm package.
fn is_relative_specifier(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/')
}

/// Resolve a relative specifier against `config_path`'s directory to a readable
/// sibling file, returning the resolved path and its source. Tries the path as
/// written first (covers `./vite.shared.js`), then appends each known config
/// extension (covers extensionless `./vite.shared` and dotted basenames where
/// `Path::extension` would misread `.shared`), then an `index.*` directory file.
fn resolve_sibling_module(config_path: &Path, specifier: &str) -> Option<(PathBuf, String)> {
    let parent = config_path.parent().unwrap_or(config_path);
    let direct = parent.join(specifier);
    if let Ok(source) = std::fs::read_to_string(&direct) {
        return Some((direct, source));
    }
    for ext in ALIAS_SIBLING_EXTS {
        let candidate = parent.join(format!("{specifier}.{ext}"));
        if let Ok(source) = std::fs::read_to_string(&candidate) {
            return Some((candidate, source));
        }
    }
    for ext in ALIAS_SIBLING_EXTS {
        let candidate = direct.join(format!("index.{ext}"));
        if let Ok(source) = std::fs::read_to_string(&candidate) {
            return Some((candidate, source));
        }
    }
    None
}

/// Find a default-exported array config, the `defineWorkspace([...])` /
/// `vitest.workspace.{ts,js}` shape. Handles `export default [...]` and
/// `export default defineWorkspace([...])` / `defineConfig([...])` (the array as
/// the call's first argument), plus parenthesised / `as` wrappers.
fn find_default_export_array<'a>(program: &'a Program<'a>) -> Option<&'a ArrayExpression<'a>> {
    for stmt in &program.body {
        if let Statement::ExportDefaultDeclaration(decl) = stmt
            && let Some(expr) = decl.declaration.as_expression()
        {
            return array_from_expression(expr);
        }
    }
    None
}

fn array_from_expression<'a>(expr: &'a Expression<'a>) -> Option<&'a ArrayExpression<'a>> {
    match expr {
        Expression::ArrayExpression(arr) => Some(arr),
        Expression::ParenthesizedExpression(paren) => array_from_expression(&paren.expression),
        Expression::TSAsExpression(ts_as) => array_from_expression(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => array_from_expression(&ts_sat.expression),
        Expression::CallExpression(call) => call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(array_from_expression),
        _ => None,
    }
}

pub(crate) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }

    normalized
}

/// Convert an expression to a string array if it's an array of string literals.
fn expression_to_string_array(expr: &Expression) -> Vec<String> {
    match expr {
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|el| match el {
                ArrayExpressionElement::SpreadElement(_) => None,
                _ => el.as_expression().and_then(expression_to_string),
            })
            .collect(),
        _ => vec![],
    }
}

/// Collect only top-level string values from an expression.
///
/// For arrays, extracts direct string elements and the first string element of sub-arrays
/// (to handle `["pkg-name", { options }]` tuples). Does NOT recurse into objects.
fn collect_shallow_string_values(expr: &Expression) -> Vec<String> {
    let mut values = Vec::new();
    match expr {
        Expression::StringLiteral(s) => {
            values.push(s.value.to_string());
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(inner) = el.as_expression() {
                    match inner {
                        Expression::StringLiteral(s) => {
                            values.push(s.value.to_string());
                        }
                        Expression::ArrayExpression(sub_arr) => {
                            if let Some(first) = sub_arr.elements.first()
                                && let Some(first_expr) = first.as_expression()
                                && let Some(s) = expression_to_string(first_expr)
                            {
                                values.push(s);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    match &p.value {
                        Expression::StringLiteral(s) => {
                            values.push(s.value.to_string());
                        }
                        Expression::ArrayExpression(sub_arr) => {
                            if let Some(first) = sub_arr.elements.first()
                                && let Some(first_expr) = first.as_expression()
                                && let Some(s) = expression_to_string(first_expr)
                            {
                                values.push(s);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    values
}

/// Collect top-level string values, plus a named string property from object entries.
fn collect_shallow_string_or_object_property_values(
    expr: &Expression,
    object_property: &str,
) -> Vec<String> {
    match expr {
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|element| {
                element
                    .as_expression()
                    .and_then(|expr| shallow_string_or_object_property(expr, object_property))
            })
            .collect(),
        _ => shallow_string_or_object_property(expr, object_property)
            .into_iter()
            .collect(),
    }
}

fn shallow_string_or_object_property(expr: &Expression, object_property: &str) -> Option<String> {
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            shallow_string_or_object_property(&paren.expression, object_property)
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            shallow_string_or_object_property(&ts_sat.expression, object_property)
        }
        Expression::TSAsExpression(ts_as) => {
            shallow_string_or_object_property(&ts_as.expression, object_property)
        }
        Expression::ArrayExpression(sub_arr) => sub_arr
            .elements
            .first()
            .and_then(ArrayExpressionElement::as_expression)
            .and_then(expression_to_string),
        Expression::ObjectExpression(obj) => {
            find_property(obj, object_property).and_then(|prop| expression_to_string(&prop.value))
        }
        _ => expression_to_string(expr),
    }
}

/// Recursively collect all string literal values from an expression tree.
fn collect_all_string_values(expr: &Expression, values: &mut Vec<String>) {
    match expr {
        Expression::StringLiteral(s) => {
            values.push(s.value.to_string());
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(expr) = el.as_expression() {
                    collect_all_string_values(expr, values);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    collect_all_string_values(&p.value, values);
                }
            }
        }
        _ => {}
    }
}

/// Convert a `PropertyKey` to a `String`.
fn property_key_to_string(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        _ => None,
    }
}

/// Extract keys of an object at a nested property path.
fn get_nested_object_keys(obj: &ObjectExpression, path: &[&str]) -> Option<Vec<String>> {
    if path.is_empty() {
        return None;
    }
    let prop = find_property(obj, path[0])?;
    if path.len() == 1 {
        if let Expression::ObjectExpression(nested) = &prop.value {
            let keys = nested
                .properties
                .iter()
                .filter_map(|p| {
                    if let ObjectPropertyKind::ObjectProperty(p) = p {
                        property_key_to_string(&p.key)
                    } else {
                        None
                    }
                })
                .collect();
            return Some(keys);
        }
        return None;
    }
    if let Expression::ObjectExpression(nested) = &prop.value {
        get_nested_object_keys(nested, &path[1..])
    } else {
        None
    }
}

/// Navigate a nested property path and return the raw expression at the end.
fn get_nested_expression<'a>(
    obj: &'a ObjectExpression<'a>,
    path: &[&str],
) -> Option<&'a Expression<'a>> {
    if path.is_empty() {
        return None;
    }
    let prop = find_property(obj, path[0])?;
    if path.len() == 1 {
        return Some(&prop.value);
    }
    if let Expression::ObjectExpression(nested) = &prop.value {
        get_nested_expression(nested, &path[1..])
    } else {
        None
    }
}

/// Navigate a nested path and extract a string, string array, or object string/array values.
fn get_nested_string_or_array(obj: &ObjectExpression, path: &[&str]) -> Option<Vec<String>> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        let prop = find_property(obj, path[0])?;
        return Some(expression_to_string_or_array(&prop.value));
    }
    let prop = find_property(obj, path[0])?;
    if let Expression::ObjectExpression(nested) = &prop.value {
        get_nested_string_or_array(nested, &path[1..])
    } else {
        None
    }
}

/// Convert an expression to a `Vec<String>`, handling string, array, object-with-string/array values,
/// and Webpack 5 entry descriptors (`{ import: "..." }`).
///
/// Array elements that are object literals are inspected for an `input` property
/// (Angular CLI schema for `styles`/`scripts`/`polyfills`:
/// `{ "input": "src/x.scss", "bundleName": "x", "inject": false }`). Extracting
/// `input` prevents object-form entries from being silently dropped. See #126.
fn expression_to_string_or_array(expr: &Expression) -> Vec<String> {
    match expr {
        Expression::StringLiteral(s) => vec![s.value.to_string()],
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .map(|q| vec![q.value.raw.to_string()])
            .unwrap_or_default(),
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|el| el.as_expression())
            .flat_map(|e| match e {
                Expression::ObjectExpression(obj) => find_property(obj, "input")
                    .map(|p| expression_to_string_or_array(&p.value))
                    .unwrap_or_default(),
                _ => expression_to_path_string(e).into_iter().collect(),
            })
            .collect(),
        Expression::ObjectExpression(obj) => obj
            .properties
            .iter()
            .flat_map(|p| {
                if let ObjectPropertyKind::ObjectProperty(p) = p {
                    match &p.value {
                        Expression::ArrayExpression(_) => expression_to_string_or_array(&p.value),
                        Expression::ObjectExpression(value_obj) => {
                            find_property(value_obj, "import")
                                .map(|import_prop| {
                                    expression_to_string_or_array(&import_prop.value)
                                })
                                .unwrap_or_default()
                        }
                        _ => expression_to_path_string(&p.value).into_iter().collect(),
                    }
                } else {
                    Vec::new()
                }
            })
            .collect(),
        _ => expression_to_path_string(expr).into_iter().collect(),
    }
}

/// Collect `require('...')` argument strings from an expression.
fn collect_require_sources(expr: &Expression) -> Vec<String> {
    let mut sources = Vec::new();
    match expr {
        Expression::CallExpression(call) if is_require_call(call) => {
            if let Some(s) = get_require_source(call) {
                sources.push(s);
            }
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(inner) = el.as_expression() {
                    match inner {
                        Expression::CallExpression(call) if is_require_call(call) => {
                            if let Some(s) = get_require_source(call) {
                                sources.push(s);
                            }
                        }
                        Expression::ArrayExpression(sub_arr) => {
                            if let Some(first) = sub_arr.elements.first()
                                && let Some(Expression::CallExpression(call)) =
                                    first.as_expression()
                                && is_require_call(call)
                                && let Some(s) = get_require_source(call)
                            {
                                sources.push(s);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    sources
}

/// Check if a call expression is `require(...)`.
fn is_require_call(call: &CallExpression) -> bool {
    matches!(&call.callee, Expression::Identifier(id) if id.name == "require")
}

/// Get the first string argument of a `require()` call.
fn get_require_source(call: &CallExpression) -> Option<String> {
    call.arguments.first().and_then(|arg| {
        if let Argument::StringLiteral(s) = arg {
            Some(s.value.to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn js_path() -> PathBuf {
        PathBuf::from("config.js")
    }

    fn ts_path() -> PathBuf {
        PathBuf::from("config.ts")
    }

    #[test]
    fn extract_lazy_imports_bare_arrows() {
        let source = r"
            import { defineConfig } from '@adonisjs/core/app'
            export default defineConfig({
                preloads: [
                    () => import('#start/routes'),
                    () => import('#start/kernel'),
                ],
            })
        ";
        let specs = extract_lazy_imports_in_array(source, &ts_path(), &["preloads"]);
        assert_eq!(specs, vec!["#start/routes", "#start/kernel"]);
    }

    #[test]
    fn extract_lazy_imports_object_form_with_file_key() {
        let source = r"
            export default defineConfig({
                providers: [
                    () => import('@adonisjs/core/providers/app_provider'),
                    {
                        file: () => import('@adonisjs/core/providers/repl_provider'),
                        environment: ['repl', 'test'],
                    },
                ],
            })
        ";
        let specs = extract_lazy_imports_in_array(source, &ts_path(), &["providers"]);
        assert_eq!(
            specs,
            vec![
                "@adonisjs/core/providers/app_provider",
                "@adonisjs/core/providers/repl_provider",
            ]
        );
    }

    #[test]
    fn extract_lazy_imports_block_body_with_return() {
        let source = r"
            export default defineConfig({
                commands: [
                    () => { return import('@adonisjs/core/commands') },
                ],
            })
        ";
        let specs = extract_lazy_imports_in_array(source, &ts_path(), &["commands"]);
        assert_eq!(specs, vec!["@adonisjs/core/commands"]);
    }

    #[test]
    fn extract_lazy_imports_skips_unknown_element_shapes() {
        let source = r"
            export default defineConfig({
                commands: [
                    'string-entry',
                    42,
                    { other: 'value' },
                    () => import('@adonisjs/lucid/commands'),
                ],
            })
        ";
        let specs = extract_lazy_imports_in_array(source, &ts_path(), &["commands"]);
        assert_eq!(specs, vec!["@adonisjs/lucid/commands"]);
    }

    #[test]
    fn extract_lazy_imports_missing_property_returns_empty() {
        let source = r"
            export default defineConfig({
                preloads: [() => import('#start/routes')],
            })
        ";
        let specs = extract_lazy_imports_in_array(source, &ts_path(), &["providers"]);
        assert!(specs.is_empty());
    }

    #[test]
    fn extract_imports_basic() {
        let source = r"
            import foo from 'foo-pkg';
            import { bar } from '@scope/bar';
            export default {};
        ";
        let imports = extract_imports(source, &js_path());
        assert_eq!(imports, vec!["foo-pkg", "@scope/bar"]);
    }

    #[test]
    fn extract_default_export_object_property() {
        let source = r#"export default { testDir: "./tests" };"#;
        let val = extract_config_string(source, &js_path(), &["testDir"]);
        assert_eq!(val, Some("./tests".to_string()));
    }

    #[test]
    fn extract_define_config_property() {
        let source = r#"
            import { defineConfig } from 'vitest/config';
            export default defineConfig({
                test: {
                    include: ["**/*.test.ts", "**/*.spec.ts"],
                    setupFiles: ["./test/setup.ts"]
                }
            });
        "#;
        let include = extract_config_string_array(source, &ts_path(), &["test", "include"]);
        assert_eq!(include, vec!["**/*.test.ts", "**/*.spec.ts"]);

        let setup = extract_config_string_array(source, &ts_path(), &["test", "setupFiles"]);
        assert_eq!(setup, vec!["./test/setup.ts"]);
    }

    #[test]
    fn extract_module_exports_property() {
        let source = r#"module.exports = { testEnvironment: "jsdom" };"#;
        let val = extract_config_string(source, &js_path(), &["testEnvironment"]);
        assert_eq!(val, Some("jsdom".to_string()));
    }

    #[test]
    fn extract_nested_string_array() {
        let source = r#"
            export default {
                resolve: {
                    alias: {
                        "@": "./src"
                    }
                },
                test: {
                    include: ["src/**/*.test.ts"]
                }
            };
        "#;
        let include = extract_config_string_array(source, &js_path(), &["test", "include"]);
        assert_eq!(include, vec!["src/**/*.test.ts"]);
    }

    #[test]
    fn extract_addons_array() {
        let source = r#"
            export default {
                addons: [
                    "@storybook/addon-a11y",
                    "@storybook/addon-docs",
                    "@storybook/addon-links"
                ]
            };
        "#;
        let addons = extract_config_property_strings(source, &ts_path(), "addons");
        assert_eq!(
            addons,
            vec![
                "@storybook/addon-a11y",
                "@storybook/addon-docs",
                "@storybook/addon-links"
            ]
        );
    }

    #[test]
    fn handle_empty_config() {
        let source = "";
        let result = extract_config_string(source, &js_path(), &["key"]);
        assert_eq!(result, None);
    }

    #[test]
    fn object_keys_postcss_plugins() {
        let source = r"
            module.exports = {
                plugins: {
                    autoprefixer: {},
                    tailwindcss: {},
                    'postcss-import': {}
                }
            };
        ";
        let keys = extract_config_object_keys(source, &js_path(), &["plugins"]);
        assert_eq!(keys, vec!["autoprefixer", "tailwindcss", "postcss-import"]);
    }

    #[test]
    fn object_keys_nested_path() {
        let source = r"
            export default {
                build: {
                    plugins: {
                        minify: {},
                        compress: {}
                    }
                }
            };
        ";
        let keys = extract_config_object_keys(source, &js_path(), &["build", "plugins"]);
        assert_eq!(keys, vec!["minify", "compress"]);
    }

    #[test]
    fn object_keys_empty_object() {
        let source = r"export default { plugins: {} };";
        let keys = extract_config_object_keys(source, &js_path(), &["plugins"]);
        assert!(keys.is_empty());
    }

    #[test]
    fn object_keys_non_object_returns_empty() {
        let source = r#"export default { plugins: ["a", "b"] };"#;
        let keys = extract_config_object_keys(source, &js_path(), &["plugins"]);
        assert!(keys.is_empty());
    }

    #[test]
    fn string_or_array_single_string() {
        let source = r#"export default { entry: "./src/index.js" };"#;
        let result = extract_config_string_or_array(source, &js_path(), &["entry"]);
        assert_eq!(result, vec!["./src/index.js"]);
    }

    #[test]
    fn string_or_array_array() {
        let source = r#"export default { entry: ["./src/a.js", "./src/b.js"] };"#;
        let result = extract_config_string_or_array(source, &js_path(), &["entry"]);
        assert_eq!(result, vec!["./src/a.js", "./src/b.js"]);
    }

    #[test]
    fn string_or_array_object_values() {
        let source =
            r#"export default { entry: { main: "./src/main.js", vendor: "./src/vendor.js" } };"#;
        let result = extract_config_string_or_array(source, &js_path(), &["entry"]);
        assert_eq!(result, vec!["./src/main.js", "./src/vendor.js"]);
    }

    #[test]
    fn string_or_array_object_array_values() {
        let source = r#"export default { entry: { app: ["./src/polyfill.js", "./src/app.js"] } };"#;
        let result = extract_config_string_or_array(source, &js_path(), &["entry"]);
        assert_eq!(result, vec!["./src/polyfill.js", "./src/app.js"]);
    }

    #[test]
    fn string_or_array_webpack_entry_descriptors() {
        let source = r#"
            export default {
                entry: {
                    app: {
                        import: "./src/app.js",
                        filename: "pages/app.js",
                        dependOn: "shared",
                    },
                    admin: {
                        import: ["./src/admin-polyfill.js", "./src/admin.js"],
                        runtime: "runtime",
                    },
                    shared: ["react", "react-dom"],
                },
            };
        "#;
        let result = extract_config_string_or_array(source, &js_path(), &["entry"]);
        assert_eq!(
            result,
            vec![
                "./src/app.js",
                "./src/admin-polyfill.js",
                "./src/admin.js",
                "react",
                "react-dom"
            ]
        );
    }

    #[test]
    fn string_or_array_nested_path() {
        let source = r#"
            export default {
                build: {
                    rollupOptions: {
                        input: ["./index.html", "./about.html"]
                    }
                }
            };
        "#;
        let result = extract_config_string_or_array(
            source,
            &js_path(),
            &["build", "rollupOptions", "input"],
        );
        assert_eq!(result, vec!["./index.html", "./about.html"]);
    }

    #[test]
    fn string_or_array_template_literal() {
        let source = r"export default { entry: `./src/index.js` };";
        let result = extract_config_string_or_array(source, &js_path(), &["entry"]);
        assert_eq!(result, vec!["./src/index.js"]);
    }

    #[test]
    fn string_or_array_object_path_helper_values() {
        let source = r#"
            import { resolve, join } from "node:path";
            import path from "node:path";
            export default {
                build: {
                    rollupOptions: {
                        input: {
                            app: resolve(__dirname, "src/app.ts"),
                            modal: path.resolve(__dirname, "src/modal.ts"),
                            tabs: join(__dirname, "src/tabs.ts"),
                            styles: resolve(__dirname, "src/index.css"),
                        },
                    },
                },
            };
        "#;
        let result = extract_config_string_or_array(
            source,
            &js_path(),
            &["build", "rollupOptions", "input"],
        );
        assert_eq!(
            result,
            vec!["src/app.ts", "src/modal.ts", "src/tabs.ts", "src/index.css"]
        );
    }

    #[test]
    fn string_or_array_array_path_helper_values() {
        let source = r#"
            import { resolve } from "node:path";
            export default {
                build: {
                    rollupOptions: {
                        input: [resolve(__dirname, "src/a.ts"), "./src/b.ts"],
                    },
                },
            };
        "#;
        let result = extract_config_string_or_array(
            source,
            &js_path(),
            &["build", "rollupOptions", "input"],
        );
        assert_eq!(result, vec!["src/a.ts", "./src/b.ts"]);
    }

    #[test]
    fn string_or_array_top_level_path_helper_call() {
        let source = r#"
            import { resolve } from "node:path";
            export default { build: { lib: { entry: resolve(__dirname, "src/index.ts") } } };
        "#;
        let result = extract_config_string_or_array(source, &js_path(), &["build", "lib", "entry"]);
        assert_eq!(result, vec!["src/index.ts"]);
    }

    #[test]
    fn string_or_array_import_meta_dirname_anchor() {
        let source = r#"
            import { resolve } from "node:path";
            export default {
                build: { lib: { entry: resolve(import.meta.dirname, "src/index.ts") } },
            };
        "#;
        let result = extract_config_string_or_array(source, &ts_path(), &["build", "lib", "entry"]);
        assert_eq!(result, vec!["src/index.ts"]);
    }

    #[test]
    fn string_or_array_non_literal_path_helper_args_dropped() {
        let source = r#"
            import { resolve } from "node:path";
            export default { build: { lib: { entry: resolve(baseDir, "src/index.ts") } } };
        "#;
        let result = extract_config_string_or_array(source, &js_path(), &["build", "lib", "entry"]);
        assert!(
            result.is_empty(),
            "non-literal path-helper args must be dropped: {result:?}"
        );
    }

    #[test]
    fn require_strings_array() {
        let source = r"
            module.exports = {
                plugins: [
                    require('autoprefixer'),
                    require('postcss-import')
                ]
            };
        ";
        let deps = extract_config_require_strings(source, &js_path(), "plugins");
        assert_eq!(deps, vec!["autoprefixer", "postcss-import"]);
    }

    #[test]
    fn require_strings_with_tuples() {
        let source = r"
            module.exports = {
                plugins: [
                    require('autoprefixer'),
                    [require('postcss-preset-env'), { stage: 3 }]
                ]
            };
        ";
        let deps = extract_config_require_strings(source, &js_path(), "plugins");
        assert_eq!(deps, vec!["autoprefixer", "postcss-preset-env"]);
    }

    #[test]
    fn require_strings_empty_array() {
        let source = r"module.exports = { plugins: [] };";
        let deps = extract_config_require_strings(source, &js_path(), "plugins");
        assert!(deps.is_empty());
    }

    #[test]
    fn require_strings_no_require_calls() {
        let source = r#"module.exports = { plugins: ["a", "b"] };"#;
        let deps = extract_config_require_strings(source, &js_path(), "plugins");
        assert!(deps.is_empty());
    }

    #[test]
    fn extract_aliases_from_object_with_file_url_to_path() {
        let source = r#"
            import { defineConfig } from 'vite';
            import { fileURLToPath, URL } from 'node:url';

            export default defineConfig({
                resolve: {
                    alias: {
                        "@": fileURLToPath(new URL("./src", import.meta.url))
                    }
                }
            });
        "#;

        let aliases = extract_config_aliases(source, &ts_path(), &["resolve", "alias"]);
        assert_eq!(aliases, vec![("@".to_string(), "./src".to_string())]);
    }

    #[test]
    fn extract_aliases_from_array_form() {
        let source = r#"
            export default {
                resolve: {
                    alias: [
                        { find: "@", replacement: "./src" },
                        { find: "$utils", replacement: "src/lib/utils" }
                    ]
                }
            };
        "#;

        let aliases = extract_config_aliases(source, &ts_path(), &["resolve", "alias"]);
        assert_eq!(
            aliases,
            vec![
                ("@".to_string(), "./src".to_string()),
                ("$utils".to_string(), "src/lib/utils".to_string())
            ]
        );
    }

    #[test]
    fn extract_aliases_from_object_with_array_values() {
        let source = r#"
            ({
                compilerOptions: {
                    paths: {
                        "@/*": ["./src/*"],
                        "@shared/*": ["./shared/*", "./fallback/*"]
                    }
                }
            })
        "#;

        let aliases = extract_config_aliases(source, &js_path(), &["compilerOptions", "paths"]);
        assert_eq!(
            aliases,
            vec![
                ("@/*".to_string(), "./src/*".to_string()),
                ("@shared/*".to_string(), "./shared/*".to_string())
            ]
        );
    }

    #[test]
    fn extract_array_object_strings_mixed_forms() {
        let source = r#"
            export default {
                components: [
                    "~/components",
                    { path: "@/feature-components" }
                ]
            };
        "#;

        let values =
            extract_config_array_object_strings(source, &ts_path(), &["components"], "path");
        assert_eq!(
            values,
            vec![
                "~/components".to_string(),
                "@/feature-components".to_string()
            ]
        );
    }

    #[test]
    fn extract_array_object_string_pairs_with_and_without_secondary() {
        let source = r#"
            export default {
                webServer: [
                    { command: "tsx scripts/api.ts", cwd: "packages/api" },
                    { command: "tsx scripts/web.ts" }
                ]
            };
        "#;

        let pairs = extract_config_array_object_string_pairs(
            source,
            &ts_path(),
            &["webServer"],
            "command",
            "cwd",
        );
        assert_eq!(
            pairs,
            vec![
                (
                    "tsx scripts/api.ts".to_string(),
                    Some("packages/api".to_string())
                ),
                ("tsx scripts/web.ts".to_string(), None),
            ]
        );
    }

    #[test]
    fn extract_array_object_string_pairs_skips_elements_missing_primary() {
        let source = r#"
            export default {
                webServer: [
                    { cwd: "packages/api" },
                    { command: "srvx --port 3000" }
                ]
            };
        "#;

        let pairs = extract_config_array_object_string_pairs(
            source,
            &ts_path(),
            &["webServer"],
            "command",
            "cwd",
        );
        assert_eq!(pairs, vec![("srvx --port 3000".to_string(), None)]);
    }

    #[test]
    fn extract_array_object_string_pairs_empty_for_object_form() {
        let source = r#"
            export default {
                webServer: { command: "srvx --port 3000" }
            };
        "#;

        let pairs = extract_config_array_object_string_pairs(
            source,
            &ts_path(),
            &["webServer"],
            "command",
            "cwd",
        );
        assert!(pairs.is_empty());
    }

    #[test]
    fn extract_config_plugin_option_string_from_json() {
        let source = r#"{
            "expo": {
                "plugins": [
                    ["expo-router", { "root": "src/app" }]
                ]
            }
        }"#;

        let value = extract_config_plugin_option_string(
            source,
            &json_path(),
            &["expo", "plugins"],
            "expo-router",
            "root",
        );

        assert_eq!(value, Some("src/app".to_string()));
    }

    #[test]
    fn extract_config_plugin_option_string_from_top_level_plugins() {
        let source = r#"{
            "plugins": [
                ["expo-router", { "root": "./src/routes" }]
            ]
        }"#;

        let value = extract_config_plugin_option_string_from_paths(
            source,
            &json_path(),
            &[&["plugins"], &["expo", "plugins"]],
            "expo-router",
            "root",
        );

        assert_eq!(value, Some("./src/routes".to_string()));
    }

    #[test]
    fn extract_config_plugin_option_string_from_ts_config() {
        let source = r"
            export default {
                expo: {
                    plugins: [
                        ['expo-router', { root: './src/app' }]
                    ]
                }
            };
        ";

        let value = extract_config_plugin_option_string(
            source,
            &ts_path(),
            &["expo", "plugins"],
            "expo-router",
            "root",
        );

        assert_eq!(value, Some("./src/app".to_string()));
    }

    #[test]
    fn extract_config_plugin_option_string_returns_none_when_plugin_missing() {
        let source = r#"{
            "expo": {
                "plugins": [
                    ["expo-font", {}]
                ]
            }
        }"#;

        let value = extract_config_plugin_option_string(
            source,
            &json_path(),
            &["expo", "plugins"],
            "expo-router",
            "root",
        );

        assert_eq!(value, None);
    }

    #[test]
    fn vite_react_babel_dependencies_extract_plain_tuple_and_prefixed_entries() {
        let source = r#"
            import react from "@vitejs/plugin-react";

            export default defineConfig({
                plugins: [
                    react({
                        babel: {
                            plugins: [
                                "babel-plugin-plain",
                                ["module:@preact/signals-react-transform", { mode: "auto" }],
                            ],
                            presets: [["@babel/preset-react", { runtime: "automatic" }]],
                        },
                    }),
                ],
            });
        "#;

        let deps = extract_vite_react_babel_dependencies(source, &ts_path());

        assert_eq!(
            deps,
            vec![
                "babel-plugin-plain".to_string(),
                "@preact/signals-react-transform".to_string(),
                "@babel/preset-react".to_string(),
            ]
        );
    }

    #[test]
    fn vite_react_babel_dependencies_support_default_alias_import() {
        let source = r#"
            import { default as viteReact } from "@vitejs/plugin-react";

            export default {
                plugins: [
                    viteReact({
                        babel: {
                            plugins: [["module:@scope/pkg/plugin", {}]],
                        },
                    }),
                ],
            };
        "#;

        let deps = extract_vite_react_babel_dependencies(source, &ts_path());

        assert_eq!(deps, vec!["@scope/pkg".to_string()]);
    }

    #[test]
    fn vite_react_babel_dependencies_ignore_unrelated_plugin_calls() {
        let source = r#"
            import vue from "@vitejs/plugin-vue";

            export default {
                plugins: [
                    vue({
                        babel: {
                            plugins: ["@preact/signals-react-transform"],
                        },
                    }),
                ],
            };
        "#;

        let deps = extract_vite_react_babel_dependencies(source, &ts_path());

        assert!(deps.is_empty());
    }

    #[test]
    fn vite_react_babel_dependencies_skip_relative_and_protocol_entries() {
        let source = r#"
            import react from "@vitejs/plugin-react";

            export default {
                plugins: [
                    react({
                        babel: {
                            plugins: ["./local-plugin", "module:./local-prefixed", "http://example.com/plugin"],
                        },
                    }),
                ],
            };
        "#;

        let deps = extract_vite_react_babel_dependencies(source, &ts_path());

        assert!(deps.is_empty());
    }

    #[test]
    fn normalize_config_path_relative_to_root() {
        let config_path = PathBuf::from("/project/vite.config.ts");
        let root = PathBuf::from("/project");

        assert_eq!(
            normalize_config_path("./src/lib", &config_path, &root),
            Some("src/lib".to_string())
        );
        assert_eq!(
            normalize_config_path("/src/lib", &config_path, &root),
            Some("src/lib".to_string())
        );
    }

    #[test]
    fn normalize_config_path_mixed_separators_and_parent_dirs() {
        let config_path = PathBuf::from("/project/config/vite.config.ts");
        let root = PathBuf::from("/project");

        assert_eq!(
            normalize_config_path(".\\src\\..\\app\\lib", &config_path, &root),
            Some("config/app/lib".to_string())
        );
    }

    #[test]
    fn normalize_config_path_leading_slash_stays_project_relative() {
        let config_path = PathBuf::from("/project/vite.config.ts");
        let root = PathBuf::from("/project");

        assert_eq!(
            normalize_config_path("/src\\lib", &config_path, &root),
            Some("src/lib".to_string())
        );
    }

    #[test]
    fn json_wrapped_in_parens_string() {
        let source = r#"({"extends": "@tsconfig/node18/tsconfig.json"})"#;
        let val = extract_config_string(source, &js_path(), &["extends"]);
        assert_eq!(val, Some("@tsconfig/node18/tsconfig.json".to_string()));
    }

    #[test]
    fn json_wrapped_in_parens_nested_array() {
        let source =
            r#"({"compilerOptions": {"types": ["node", "jest"]}, "include": ["src/**/*"]})"#;
        let types = extract_config_string_array(source, &js_path(), &["compilerOptions", "types"]);
        assert_eq!(types, vec!["node", "jest"]);

        let include = extract_config_string_array(source, &js_path(), &["include"]);
        assert_eq!(include, vec!["src/**/*"]);
    }

    #[test]
    fn json_wrapped_in_parens_object_keys() {
        let source = r#"({"plugins": {"autoprefixer": {}, "tailwindcss": {}}})"#;
        let keys = extract_config_object_keys(source, &js_path(), &["plugins"]);
        assert_eq!(keys, vec!["autoprefixer", "tailwindcss"]);
    }

    fn json_path() -> PathBuf {
        PathBuf::from("config.json")
    }

    #[test]
    fn json_file_parsed_correctly() {
        let source = r#"{"key": "value", "list": ["a", "b"]}"#;
        let val = extract_config_string(source, &json_path(), &["key"]);
        assert_eq!(val, Some("value".to_string()));

        let list = extract_config_string_array(source, &json_path(), &["list"]);
        assert_eq!(list, vec!["a", "b"]);
    }

    #[test]
    fn jsonc_file_parsed_correctly() {
        let source = r#"{"key": "value"}"#;
        let path = PathBuf::from("tsconfig.jsonc");
        let val = extract_config_string(source, &path, &["key"]);
        assert_eq!(val, Some("value".to_string()));
    }

    #[test]
    fn extract_define_config_arrow_function() {
        let source = r#"
            import { defineConfig } from 'vite';
            export default defineConfig(() => ({
                test: {
                    include: ["**/*.test.ts"]
                }
            }));
        "#;
        let include = extract_config_string_array(source, &ts_path(), &["test", "include"]);
        assert_eq!(include, vec!["**/*.test.ts"]);
    }

    #[test]
    fn extract_config_from_default_export_function_declaration() {
        let source = r#"
            export default function createConfig() {
                return {
                    clientModules: ["./src/client/global.js"]
                };
            }
        "#;

        let client_modules = extract_config_string_array(source, &ts_path(), &["clientModules"]);
        assert_eq!(client_modules, vec!["./src/client/global.js"]);
    }

    #[test]
    fn extract_config_from_default_export_async_function_declaration() {
        let source = r#"
            export default async function createConfigAsync() {
                return {
                    docs: {
                        path: "knowledge"
                    }
                };
            }
        "#;

        let docs_path = extract_config_string(source, &ts_path(), &["docs", "path"]);
        assert_eq!(docs_path, Some("knowledge".to_string()));
    }

    #[test]
    fn extract_config_from_exported_arrow_function_identifier() {
        let source = r#"
            const config = async () => {
                return {
                    themes: ["classic"]
                };
            };

            export default config;
        "#;

        let themes = extract_config_shallow_strings(source, &ts_path(), "themes");
        assert_eq!(themes, vec!["classic"]);
    }

    #[test]
    fn module_exports_nested_string() {
        let source = r#"
            module.exports = {
                resolve: {
                    alias: {
                        "@": "./src"
                    }
                }
            };
        "#;
        let val = extract_config_string(source, &js_path(), &["resolve", "alias", "@"]);
        assert_eq!(val, Some("./src".to_string()));
    }

    #[test]
    fn property_strings_nested_objects() {
        let source = r#"
            export default {
                plugins: {
                    group1: { a: "val-a" },
                    group2: { b: "val-b" }
                }
            };
        "#;
        let values = extract_config_property_strings(source, &js_path(), "plugins");
        assert!(values.contains(&"val-a".to_string()));
        assert!(values.contains(&"val-b".to_string()));
    }

    #[test]
    fn property_strings_missing_key_returns_empty() {
        let source = r#"export default { other: "value" };"#;
        let values = extract_config_property_strings(source, &js_path(), "missing");
        assert!(values.is_empty());
    }

    #[test]
    fn shallow_strings_tuple_array() {
        let source = r#"
            module.exports = {
                reporters: ["default", ["jest-junit", { outputDirectory: "reports" }]]
            };
        "#;
        let values = extract_config_shallow_strings(source, &js_path(), "reporters");
        assert_eq!(values, vec!["default", "jest-junit"]);
        assert!(!values.contains(&"reports".to_string()));
    }

    #[test]
    fn shallow_strings_single_string() {
        let source = r#"export default { preset: "ts-jest" };"#;
        let values = extract_config_shallow_strings(source, &js_path(), "preset");
        assert_eq!(values, vec!["ts-jest"]);
    }

    #[test]
    fn shallow_strings_missing_key() {
        let source = r#"export default { other: "val" };"#;
        let values = extract_config_shallow_strings(source, &js_path(), "missing");
        assert!(values.is_empty());
    }

    #[test]
    fn shallow_strings_or_object_property_alias_objects() {
        let source = r#"
            export default {
                jsPlugins: [
                    "eslint-plugin-playwright",
                    ["eslint-plugin-regexp", { rules: {} }],
                    { name: "short", specifier: "eslint-plugin-with-long-name" }
                ]
            };
        "#;
        let values = extract_config_shallow_strings_or_object_property(
            source,
            &ts_path(),
            "jsPlugins",
            "specifier",
        );
        assert_eq!(
            values,
            vec![
                "eslint-plugin-playwright",
                "eslint-plugin-regexp",
                "eslint-plugin-with-long-name"
            ]
        );
    }

    #[test]
    fn nested_shallow_strings_vitest_reporters() {
        let source = r#"
            export default {
                test: {
                    reporters: ["default", "vitest-sonar-reporter"]
                }
            };
        "#;
        let values =
            extract_config_nested_shallow_strings(source, &js_path(), &["test"], "reporters");
        assert_eq!(values, vec!["default", "vitest-sonar-reporter"]);
    }

    #[test]
    fn nested_shallow_strings_tuple_format() {
        let source = r#"
            export default {
                test: {
                    reporters: ["default", ["vitest-sonar-reporter", { outputFile: "report.xml" }]]
                }
            };
        "#;
        let values =
            extract_config_nested_shallow_strings(source, &js_path(), &["test"], "reporters");
        assert_eq!(values, vec!["default", "vitest-sonar-reporter"]);
    }

    #[test]
    fn nested_shallow_strings_missing_outer() {
        let source = r"export default { other: {} };";
        let values =
            extract_config_nested_shallow_strings(source, &js_path(), &["test"], "reporters");
        assert!(values.is_empty());
    }

    #[test]
    fn nested_shallow_strings_missing_inner() {
        let source = r#"export default { test: { include: ["**/*.test.ts"] } };"#;
        let values =
            extract_config_nested_shallow_strings(source, &js_path(), &["test"], "reporters");
        assert!(values.is_empty());
    }

    #[test]
    fn string_or_array_missing_path() {
        let source = r"export default {};";
        let result = extract_config_string_or_array(source, &js_path(), &["entry"]);
        assert!(result.is_empty());
    }

    #[test]
    fn string_or_array_non_string_values() {
        let source = r"export default { entry: [42, true] };";
        let result = extract_config_string_or_array(source, &js_path(), &["entry"]);
        assert!(result.is_empty());
    }

    #[test]
    fn array_nested_extraction() {
        let source = r#"
            export default defineConfig({
                test: {
                    projects: [
                        {
                            test: {
                                setupFiles: ["./test/setup-a.ts"]
                            }
                        },
                        {
                            test: {
                                setupFiles: "./test/setup-b.ts"
                            }
                        }
                    ]
                }
            });
        "#;
        let results = extract_config_array_nested_string_or_array(
            source,
            &ts_path(),
            &["test", "projects"],
            &["test", "setupFiles"],
        );
        assert!(results.contains(&"./test/setup-a.ts".to_string()));
        assert!(results.contains(&"./test/setup-b.ts".to_string()));
    }

    #[test]
    fn array_nested_empty_when_no_array() {
        let source = r#"export default { test: { projects: "not-an-array" } };"#;
        let results = extract_config_array_nested_string_or_array(
            source,
            &js_path(),
            &["test", "projects"],
            &["test", "setupFiles"],
        );
        assert!(results.is_empty());
    }

    #[test]
    fn object_nested_extraction() {
        let source = r#"{
            "projects": {
                "app-one": {
                    "architect": {
                        "build": {
                            "options": {
                                "styles": ["src/styles.css"]
                            }
                        }
                    }
                }
            }
        }"#;
        let results = extract_config_object_nested_string_or_array(
            source,
            &json_path(),
            &["projects"],
            &["architect", "build", "options", "styles"],
        );
        assert_eq!(results, vec!["src/styles.css"]);
    }

    #[test]
    fn array_with_object_input_form_extracted() {
        let source = r#"{
            "projects": {
                "app": {
                    "architect": {
                        "build": {
                            "options": {
                                "styles": [
                                    "src/styles.scss",
                                    { "input": "src/theme.scss", "bundleName": "theme", "inject": false },
                                    { "bundleName": "lazy-only" }
                                ]
                            }
                        }
                    }
                }
            }
        }"#;
        let results = extract_config_object_nested_string_or_array(
            source,
            &json_path(),
            &["projects"],
            &["architect", "build", "options", "styles"],
        );
        assert!(
            results.contains(&"src/styles.scss".to_string()),
            "string form must still work: {results:?}"
        );
        assert!(
            results.contains(&"src/theme.scss".to_string()),
            "object form with `input` must be extracted: {results:?}"
        );
        assert!(
            !results.contains(&"lazy-only".to_string()),
            "bundleName must not be misinterpreted as a path: {results:?}"
        );
        assert!(
            !results.contains(&"theme".to_string()),
            "bundleName from full object must not leak: {results:?}"
        );
    }

    #[test]
    fn object_nested_strings_extraction() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application"
                },
                "test": {
                    "executor": "@nx/vite:test"
                }
            }
        }"#;
        let results =
            extract_config_object_nested_strings(source, &json_path(), &["targets"], &["executor"]);
        assert!(results.contains(&"@angular/build:application".to_string()));
        assert!(results.contains(&"@nx/vite:test".to_string()));
    }

    #[test]
    fn require_strings_direct_call() {
        let source = r"module.exports = { adapter: require('@sveltejs/adapter-node') };";
        let deps = extract_config_require_strings(source, &js_path(), "adapter");
        assert_eq!(deps, vec!["@sveltejs/adapter-node"]);
    }

    #[test]
    fn require_strings_no_matching_key() {
        let source = r"module.exports = { other: require('something') };";
        let deps = extract_config_require_strings(source, &js_path(), "plugins");
        assert!(deps.is_empty());
    }

    #[test]
    fn extract_imports_no_imports() {
        let source = r"export default {};";
        let imports = extract_imports(source, &js_path());
        assert!(imports.is_empty());
    }

    #[test]
    fn extract_imports_side_effect_import() {
        let source = r"
            import 'polyfill';
            import './local-setup';
            export default {};
        ";
        let imports = extract_imports(source, &js_path());
        assert_eq!(imports, vec!["polyfill", "./local-setup"]);
    }

    #[test]
    fn extract_imports_mixed_specifiers() {
        let source = r"
            import defaultExport from 'module-a';
            import { named } from 'module-b';
            import * as ns from 'module-c';
            export default {};
        ";
        let imports = extract_imports(source, &js_path());
        assert_eq!(imports, vec!["module-a", "module-b", "module-c"]);
    }

    #[test]
    fn template_literal_in_string_or_array() {
        let source = r"export default { entry: `./src/index.ts` };";
        let result = extract_config_string_or_array(source, &ts_path(), &["entry"]);
        assert_eq!(result, vec!["./src/index.ts"]);
    }

    #[test]
    fn template_literal_in_config_string() {
        let source = r"export default { testDir: `./tests` };";
        let val = extract_config_string(source, &js_path(), &["testDir"]);
        assert_eq!(val, Some("./tests".to_string()));
    }

    #[test]
    fn template_literal_command_recovers_static_command_tokens() {
        let source = r"
            const PORT = 3000;
            export default {
                webServer: {
                    command: `pnpm exec srvx --port ${PORT} --hostname 127.0.0.1`
                }
            };
        ";
        let val = extract_config_command(source, &ts_path(), &["webServer", "command"]);
        assert_eq!(
            val,
            Some("pnpm exec srvx --port   --hostname 127.0.0.1".to_string())
        );
    }

    #[test]
    fn template_literal_command_skips_dynamic_prefix() {
        let source = r"
            export default {
                webServer: { command: `${serverCommand} && pnpm exec srvx` }
            };
        ";
        let val = extract_config_command(source, &ts_path(), &["webServer", "command"]);
        assert!(val.is_none());
    }

    #[test]
    fn template_literal_command_skips_split_static_token() {
        let source = r"
            export default {
                webServer: { command: `pnpm exec sr${part}vx --port 3000` }
            };
        ";
        let val = extract_config_command(source, &ts_path(), &["webServer", "command"]);
        assert!(val.is_none());
    }

    #[test]
    fn array_object_command_pairs_recover_template_command() {
        let source = r"
            const PORT = 3000;
            export default {
                webServer: [
                    {
                        command: `pnpm exec srvx --port ${PORT}`,
                        cwd: 'apps/web'
                    }
                ]
            };
        ";
        let pairs = extract_config_array_object_command_pairs(
            source,
            &ts_path(),
            &["webServer"],
            "command",
            "cwd",
        );
        assert_eq!(
            pairs,
            vec![(
                "pnpm exec srvx --port  ".to_string(),
                Some("apps/web".to_string())
            )]
        );
    }

    #[test]
    fn nested_string_array_empty_path() {
        let source = r#"export default { items: ["a", "b"] };"#;
        let result = extract_config_string_array(source, &js_path(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn nested_string_empty_path() {
        let source = r#"export default { key: "val" };"#;
        let result = extract_config_string(source, &js_path(), &[]);
        assert!(result.is_none());
    }

    #[test]
    fn object_keys_empty_path() {
        let source = r"export default { plugins: {} };";
        let result = extract_config_object_keys(source, &js_path(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn no_config_object_returns_empty() {
        let source = r"const x = 42;";
        let result = extract_config_string(source, &js_path(), &["key"]);
        assert!(result.is_none());

        let arr = extract_config_string_array(source, &js_path(), &["items"]);
        assert!(arr.is_empty());

        let keys = extract_config_object_keys(source, &js_path(), &["plugins"]);
        assert!(keys.is_empty());
    }

    #[test]
    fn property_with_string_key() {
        let source = r#"export default { "string-key": "value" };"#;
        let val = extract_config_string(source, &js_path(), &["string-key"]);
        assert_eq!(val, Some("value".to_string()));
    }

    #[test]
    fn nested_navigation_through_non_object() {
        let source = r#"export default { level1: "not-an-object" };"#;
        let val = extract_config_string(source, &js_path(), &["level1", "level2"]);
        assert!(val.is_none());
    }

    #[test]
    fn variable_reference_untyped() {
        let source = r#"
            const config = {
                testDir: "./tests"
            };
            export default config;
        "#;
        let val = extract_config_string(source, &js_path(), &["testDir"]);
        assert_eq!(val, Some("./tests".to_string()));
    }

    #[test]
    fn variable_reference_with_type_annotation() {
        let source = r#"
            import type { StorybookConfig } from '@storybook/react-vite';
            const config: StorybookConfig = {
                addons: ["@storybook/addon-a11y", "@storybook/addon-docs"],
                framework: "@storybook/react-vite"
            };
            export default config;
        "#;
        let addons = extract_config_shallow_strings(source, &ts_path(), "addons");
        assert_eq!(
            addons,
            vec!["@storybook/addon-a11y", "@storybook/addon-docs"]
        );

        let framework = extract_config_string(source, &ts_path(), &["framework"]);
        assert_eq!(framework, Some("@storybook/react-vite".to_string()));
    }

    #[test]
    fn variable_reference_with_define_config() {
        let source = r#"
            import { defineConfig } from 'vitest/config';
            const config = defineConfig({
                test: {
                    include: ["**/*.test.ts"]
                }
            });
            export default config;
        "#;
        let include = extract_config_string_array(source, &ts_path(), &["test", "include"]);
        assert_eq!(include, vec!["**/*.test.ts"]);
    }

    #[test]
    fn ts_satisfies_direct_export() {
        let source = r#"
            export default {
                testDir: "./tests"
            } satisfies PlaywrightTestConfig;
        "#;
        let val = extract_config_string(source, &ts_path(), &["testDir"]);
        assert_eq!(val, Some("./tests".to_string()));
    }

    #[test]
    fn ts_as_direct_export() {
        let source = r#"
            export default {
                testDir: "./tests"
            } as const;
        "#;
        let val = extract_config_string(source, &ts_path(), &["testDir"]);
        assert_eq!(val, Some("./tests".to_string()));
    }

    // --- issue #811: resolve.alias as imported identifier / spread ---

    fn aliases(source: &str) -> Vec<(String, String)> {
        extract_config_aliases(source, &js_path(), &["resolve", "alias"])
    }

    #[test]
    fn aliases_inline_object_still_extracted() {
        // Regression: the resolver must not change inline-object behavior.
        let source = r#"
            export default defineConfig({
                resolve: { alias: { "@": "./src", utils: "../../utils" } }
            });
        "#;
        let mut got = aliases(source);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("@".to_string(), "./src".to_string()),
                ("utils".to_string(), "../../utils".to_string()),
            ]
        );
    }

    #[test]
    fn aliases_inline_array_still_extracted() {
        let source = r#"
            export default defineConfig({
                resolve: { alias: [{ find: "@", replacement: "./src" }] }
            });
        "#;
        assert_eq!(
            aliases(source),
            vec![("@".to_string(), "./src".to_string())]
        );
    }

    #[test]
    fn aliases_local_const_array_identifier() {
        let source = r#"
            const sharedAliases = [{ find: "@", replacement: "./src" }];
            export default defineConfig({ resolve: { alias: sharedAliases } });
        "#;
        assert_eq!(
            aliases(source),
            vec![("@".to_string(), "./src".to_string())]
        );
    }

    #[test]
    fn aliases_local_const_object_identifier() {
        let source = r#"
            const sharedAliases = { "@": "./src" };
            export default defineConfig({ resolve: { alias: sharedAliases } });
        "#;
        assert_eq!(
            aliases(source),
            vec![("@".to_string(), "./src".to_string())]
        );
    }

    #[test]
    fn aliases_array_spread_of_identifiers_and_inline() {
        let source = r##"
            const a = [{ find: "@", replacement: "./src" }];
            const b = [{ find: "~", replacement: "./lib" }];
            export default defineConfig({
                resolve: { alias: [...a, ...b, { find: "#", replacement: "./test" }] }
            });
        "##;
        let mut got = aliases(source);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("#".to_string(), "./test".to_string()),
                ("@".to_string(), "./src".to_string()),
                ("~".to_string(), "./lib".to_string()),
            ]
        );
    }

    #[test]
    fn aliases_object_spread_of_identifier_and_inline() {
        let source = r#"
            const base = { "@": "./src" };
            export default defineConfig({
                resolve: { alias: { ...base, "~": "./lib" } }
            });
        "#;
        let mut got = aliases(source);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("@".to_string(), "./src".to_string()),
                ("~".to_string(), "./lib".to_string()),
            ]
        );
    }

    #[test]
    fn aliases_local_const_chained_identifier() {
        // `const a = b` indirection resolves through the chain.
        let source = r#"
            const real = [{ find: "@", replacement: "./src" }];
            const alias2 = real;
            export default defineConfig({ resolve: { alias: alias2 } });
        "#;
        assert_eq!(
            aliases(source),
            vec![("@".to_string(), "./src".to_string())]
        );
    }

    #[test]
    fn aliases_imported_named_identifier_from_sibling() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("vite.shared.js"),
            r#"export const sharedAliases = [
                { find: "@", replacement: new URL("./src", import.meta.url).pathname },
            ];"#,
        )
        .unwrap();
        let config = dir.path().join("vite.config.js");
        let source = r#"
            import { defineConfig } from "vite";
            import { sharedAliases } from "./vite.shared.js";
            export default defineConfig({ resolve: { alias: sharedAliases } });
        "#;
        let got = extract_config_aliases(source, &config, &["resolve", "alias"]);
        assert_eq!(got, vec![("@".to_string(), "./src".to_string())]);
    }

    #[test]
    fn aliases_imported_extensionless_specifier_probed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("aliases.mjs"),
            r#"export const sharedAliases = { "@": "./src" };"#,
        )
        .unwrap();
        let config = dir.path().join("vite.config.ts");
        let source = r#"
            import { sharedAliases } from "./aliases";
            export default defineConfig({ resolve: { alias: sharedAliases } });
        "#;
        let got = extract_config_aliases(source, &config, &["resolve", "alias"]);
        assert_eq!(got, vec![("@".to_string(), "./src".to_string())]);
    }

    #[test]
    fn aliases_imported_default_export_from_sibling() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("aliases.js"),
            r#"export default [{ find: "@", replacement: "./src" }];"#,
        )
        .unwrap();
        let config = dir.path().join("vite.config.js");
        let source = r#"
            import sharedAliases from "./aliases.js";
            export default defineConfig({ resolve: { alias: sharedAliases } });
        "#;
        let got = extract_config_aliases(source, &config, &["resolve", "alias"]);
        assert_eq!(got, vec![("@".to_string(), "./src".to_string())]);
    }

    #[test]
    fn aliases_imported_spread_from_two_siblings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.js"),
            r#"export const a = [{ find: "@", replacement: "./src" }];"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("b.js"),
            r#"export const b = [{ find: "~", replacement: "./lib" }];"#,
        )
        .unwrap();
        let config = dir.path().join("vite.config.js");
        let source = r#"
            import { a } from "./a.js";
            import { b } from "./b.js";
            export default defineConfig({ resolve: { alias: [...a, ...b] } });
        "#;
        let mut got = extract_config_aliases(source, &config, &["resolve", "alias"]);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("@".to_string(), "./src".to_string()),
                ("~".to_string(), "./lib".to_string()),
            ]
        );
    }

    #[test]
    fn aliases_import_cycle_terminates() {
        // a.js imports from b.js and vice versa; resolution must not hang and
        // should still recover the literal pairs present.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.js"),
            r#"import { b } from "./b.js";
               export const a = [{ find: "@", replacement: "./src" }, ...b];"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("b.js"),
            r#"import { a } from "./a.js";
               export const b = [...a];"#,
        )
        .unwrap();
        let config = dir.path().join("vite.config.js");
        let source = r#"
            import { a } from "./a.js";
            export default defineConfig({ resolve: { alias: a } });
        "#;
        let got = extract_config_aliases(source, &config, &["resolve", "alias"]);
        assert_eq!(got, vec![("@".to_string(), "./src".to_string())]);
    }

    #[test]
    fn aliases_non_relative_import_not_followed() {
        // A bare-package import is intentionally out of scope: no node_modules
        // read for an alias literal.
        let source = r#"
            import { sharedAliases } from "some-pkg";
            export default defineConfig({ resolve: { alias: sharedAliases } });
        "#;
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("vite.config.js");
        assert!(extract_config_aliases(source, &config, &["resolve", "alias"]).is_empty());
    }

    #[test]
    fn aliases_object_array_value_takes_first_entry() {
        // tsconfig `compilerOptions.paths` maps each key to an ARRAY of targets;
        // the resolver must take the first, matching the long-standing non-kinded
        // behavior the TypeScript plugin depends on. Regression guard for the
        // array-value case that the kinded unification briefly dropped.
        let source = r#"
            export default {
                compilerOptions: { paths: { "@/*": ["./src/*"], "~/*": ["./lib/*", "./vendor/*"] } }
            };
        "#;
        let mut got = extract_config_aliases(source, &js_path(), &["compilerOptions", "paths"]);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("@/*".to_string(), "./src/*".to_string()),
                ("~/*".to_string(), "./lib/*".to_string()),
            ]
        );
    }

    #[test]
    fn aliases_kinded_preserves_is_bare_through_resolution() {
        // The bare-string vs path discriminator must survive identifier + spread
        // resolution (the test.alias package-to-package gate depends on it).
        let source = r#"
            const a = [{ find: "lodash-es", replacement: "lodash" }];
            export default defineConfig({
                resolve: { alias: [...a, { find: "@", replacement: "./src" }] }
            });
        "#;
        let mut got = extract_config_aliases_kinded(source, &js_path(), &["resolve", "alias"]);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("@".to_string(), "./src".to_string(), false),
                ("lodash-es".to_string(), "lodash".to_string(), true),
            ]
        );
    }

    #[test]
    fn aliases_kinded_preserves_is_bare_through_imported_spread() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("aliases.js"),
            r#"export const packageAliases = [{ find: "lodash-es", replacement: "lodash" }];"#,
        )
        .unwrap();
        let config = dir.path().join("vite.config.js");
        let source = r#"
            import { packageAliases } from "./aliases.js";
            export default defineConfig({
                resolve: { alias: [...packageAliases, { find: "@", replacement: "./src" }] }
            });
        "#;
        let mut got = extract_config_aliases_kinded(source, &config, &["resolve", "alias"]);
        got.sort();
        assert_eq!(
            got,
            vec![
                ("@".to_string(), "./src".to_string(), false),
                ("lodash-es".to_string(), "lodash".to_string(), true),
            ]
        );
    }
}
