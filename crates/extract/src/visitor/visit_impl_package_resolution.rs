#[allow(
    clippy::wildcard_imports,
    reason = "package-resolution helpers use many AST node shapes"
)]
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use rustc_hash::{FxHashMap, FxHashSet};

use super::super::ModuleInfoExtractor;
use super::{
    StaticPackageLoopBindings, for_of_binding_name, object_values_or_entries_argument_name,
};

pub(super) fn is_require_resolve_callee(expr: &Expression<'_>) -> bool {
    let Expression::StaticMemberExpression(member) = expr else {
        return false;
    };
    let Expression::Identifier(object) = &member.object else {
        return false;
    };
    object.name == "require" && member.property.name == "resolve"
}

pub(super) fn package_from_resolution_specifier(specifier: &str) -> Option<String> {
    if !is_package_resolution_specifier(specifier) {
        return None;
    }
    let package_name = package_name_from_specifier(specifier)?;
    let suffix = specifier
        .strip_prefix(package_name.as_str())
        .unwrap_or_default();
    (suffix.is_empty() || suffix == "/package.json").then_some(package_name)
}

fn is_package_resolution_specifier(specifier: &str) -> bool {
    if specifier.is_empty()
        || specifier.starts_with('.')
        || specifier.starts_with('/')
        || specifier.starts_with('#')
        || specifier.starts_with('$')
        || specifier.contains('\\')
        || specifier.contains(' ')
        || specifier.contains('?')
        || specifier.contains('!')
        || specifier.contains(':')
    {
        return false;
    }
    specifier
        .bytes()
        .any(|b| b.is_ascii_alphabetic() || b == b'@')
}

fn package_name_from_specifier(specifier: &str) -> Option<String> {
    if specifier.starts_with('@') {
        let mut parts = specifier.split('/');
        let scope = parts.next()?;
        let package = parts.next()?;
        if package.is_empty() {
            return None;
        }
        return Some(format!("{scope}/{package}"));
    }

    specifier
        .split('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

pub(super) fn package_values_from_raw_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| package_from_resolution_specifier(value))
        .collect()
}

pub(super) fn static_object_string_property_values(
    obj: &ObjectExpression<'_>,
) -> FxHashMap<String, Vec<String>> {
    let mut values = FxHashMap::default();
    collect_static_object_string_property_values(obj, &mut values);
    values
}

fn collect_static_object_string_property_values(
    obj: &ObjectExpression<'_>,
    values: &mut FxHashMap<String, Vec<String>>,
) {
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(key_name) = prop.key.static_name() else {
            continue;
        };
        match &prop.value {
            Expression::StringLiteral(lit) => {
                values
                    .entry(key_name.to_string())
                    .or_default()
                    .push(lit.value.to_string());
            }
            Expression::ObjectExpression(child) => {
                collect_static_object_string_property_values(child, values);
            }
            _ => {}
        }
    }
}

impl ModuleInfoExtractor {
    pub(super) fn record_static_package_values(&mut self, name: &str, init: &Expression<'_>) {
        match init {
            Expression::StringLiteral(lit) => {
                self.static_string_bindings
                    .insert(name.to_string(), lit.value.to_string());
            }
            Expression::ArrayExpression(array) => {
                let values: Vec<String> = array
                    .elements
                    .iter()
                    .filter_map(|element| match element {
                        ArrayExpressionElement::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    })
                    .collect();
                if !values.is_empty() {
                    self.static_string_arrays.insert(name.to_string(), values);
                }
            }
            Expression::ObjectExpression(obj) => {
                let values = static_object_string_property_values(obj);
                if !values.is_empty() {
                    self.static_object_property_values
                        .insert(name.to_string(), values);
                }
            }
            _ => {}
        }
    }

    pub(super) fn try_record_package_path_reference(&mut self, call: &CallExpression<'_>) {
        if is_require_resolve_callee(&call.callee)
            && let Some(arg) = call.arguments.first()
        {
            let references = self.package_references_from_argument(arg);
            self.push_package_path_references(references);
        }

        if let Expression::Identifier(callee) = &call.callee
            && let Some(arg_index) = self
                .package_resolution_function_args
                .get(callee.name.as_str())
                .copied()
            && let Some(arg) = call.arguments.get(arg_index)
        {
            let references = self.package_references_from_argument(arg);
            self.push_package_path_references(references);
        }
    }

    fn push_package_path_references(&mut self, references: Vec<String>) {
        for package_name in references {
            if !self.package_path_references.contains(&package_name) {
                self.package_path_references.push(package_name);
            }
        }
    }

    fn package_references_from_argument(&self, arg: &Argument<'_>) -> Vec<String> {
        match arg {
            Argument::StringLiteral(lit) => package_from_resolution_specifier(lit.value.as_str())
                .into_iter()
                .collect(),
            Argument::TemplateLiteral(tpl) => self.package_references_from_template(tpl),
            Argument::Identifier(ident) => self.package_values_for_identifier(&ident.name),
            Argument::StaticMemberExpression(member) => {
                self.package_values_for_static_member(member)
            }
            _ => arg.as_expression().map_or_else(Vec::new, |expr| {
                self.package_references_from_expression(expr)
            }),
        }
    }

    fn package_references_from_expression(&self, expr: &Expression<'_>) -> Vec<String> {
        match expr {
            Expression::StringLiteral(lit) => package_from_resolution_specifier(lit.value.as_str())
                .into_iter()
                .collect(),
            Expression::TemplateLiteral(tpl) => self.package_references_from_template(tpl),
            Expression::Identifier(ident) => self.package_values_for_identifier(&ident.name),
            Expression::StaticMemberExpression(member) => {
                self.package_values_for_static_member(member)
            }
            _ => Vec::new(),
        }
    }

    fn package_references_from_template(&self, tpl: &TemplateLiteral<'_>) -> Vec<String> {
        if tpl.expressions.is_empty() {
            return tpl
                .quasis
                .first()
                .and_then(|quasi| package_from_resolution_specifier(quasi.value.raw.as_str()))
                .into_iter()
                .collect();
        }

        if tpl.expressions.len() != 1 || tpl.quasis.len() != 2 {
            return Vec::new();
        }

        let Some(first) = tpl.quasis.first() else {
            return Vec::new();
        };
        let Some(last) = tpl.quasis.last() else {
            return Vec::new();
        };
        if !first.value.raw.is_empty() || last.value.raw.as_str() != "/package.json" {
            return Vec::new();
        }

        self.package_references_from_expression(&tpl.expressions[0])
    }

    fn package_values_for_identifier(&self, name: &str) -> Vec<String> {
        for scope in self.loop_string_bindings.iter().rev() {
            if let Some(values) = scope.get(name) {
                return package_values_from_raw_values(values);
            }
        }

        self.static_string_bindings
            .get(name)
            .map_or_else(Vec::new, |value| {
                package_from_resolution_specifier(value)
                    .into_iter()
                    .collect()
            })
    }

    fn package_values_for_static_member(&self, member: &StaticMemberExpression<'_>) -> Vec<String> {
        let Expression::Identifier(object) = &member.object else {
            return Vec::new();
        };
        let property = member.property.name.as_str();

        for scope in self.loop_object_property_values.iter().rev() {
            if let Some(properties) = scope.get(object.name.as_str())
                && let Some(values) = properties.get(property)
            {
                return package_values_from_raw_values(values);
            }
        }

        self.static_object_property_values
            .get(object.name.as_str())
            .and_then(|properties| properties.get(property))
            .map_or_else(Vec::new, |values| package_values_from_raw_values(values))
    }

    pub(super) fn static_package_loop_bindings(
        &self,
        stmt: &ForOfStatement<'_>,
    ) -> Option<StaticPackageLoopBindings> {
        let loop_name = for_of_binding_name(&stmt.left)?;
        let mut strings = FxHashMap::default();
        let mut objects = FxHashMap::default();

        if let Expression::Identifier(iterable) = &stmt.right
            && let Some(values) = self.static_string_arrays.get(iterable.name.as_str())
        {
            strings.insert(loop_name.clone(), values.clone());
        }

        if let Some(object_name) = object_values_or_entries_argument_name(&stmt.right)
            && let Some(properties) = self.static_object_property_values.get(&object_name)
        {
            objects.insert(loop_name, properties.clone());
        }

        (!strings.is_empty() || !objects.is_empty()).then_some((strings, objects))
    }
}

pub(super) fn package_resolution_arg_index(
    params: &FormalParameters<'_>,
    body: &FunctionBody<'_>,
    known_helpers: &FxHashMap<String, usize>,
) -> Option<usize> {
    let param_names: Vec<String> = params
        .items
        .iter()
        .filter_map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
            _ => None,
        })
        .collect();
    let param_set: FxHashSet<String> = param_names.iter().cloned().collect();
    let mut collector = PackageResolutionParamCollector {
        params: &param_set,
        known_helpers,
        matched: FxHashSet::default(),
    };
    collector.visit_function_body(body);

    param_names
        .iter()
        .position(|name| collector.matched.contains(name))
}

struct PackageResolutionParamCollector<'p> {
    params: &'p FxHashSet<String>,
    known_helpers: &'p FxHashMap<String, usize>,
    matched: FxHashSet<String>,
}

impl<'a> Visit<'a> for PackageResolutionParamCollector<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if is_require_resolve_callee(&call.callee)
            && let Some(arg) = call.arguments.first()
            && let Some(param) = package_resolution_param_from_argument(arg, self.params)
        {
            self.matched.insert(param);
        }

        if call_joins_node_modules_with_param(call, self.params)
            && let Some(param) = call
                .arguments
                .iter()
                .find_map(|arg| package_param_argument_identifier_name(arg, self.params))
        {
            self.matched.insert(param);
        }

        if let Expression::Identifier(callee) = &call.callee
            && let Some(arg_index) = self.known_helpers.get(callee.name.as_str()).copied()
            && let Some(arg) = call.arguments.get(arg_index)
            && let Some(param) = package_param_argument_identifier_name(arg, self.params)
        {
            self.matched.insert(param);
        }

        walk::walk_call_expression(self, call);
    }
}

fn package_resolution_param_from_argument(
    arg: &Argument<'_>,
    params: &FxHashSet<String>,
) -> Option<String> {
    match arg {
        Argument::Identifier(ident) if params.contains(ident.name.as_str()) => {
            Some(ident.name.to_string())
        }
        Argument::TemplateLiteral(tpl)
            if tpl.expressions.len() == 1
                && tpl.quasis.len() == 2
                && tpl.quasis.first()?.value.raw.is_empty()
                && tpl.quasis.last()?.value.raw.as_str() == "/package.json" =>
        {
            package_param_expression_identifier_name(&tpl.expressions[0], params)
        }
        _ => arg
            .as_expression()
            .and_then(|expr| package_param_expression_identifier_name(expr, params)),
    }
}

fn call_joins_node_modules_with_param(
    call: &CallExpression<'_>,
    params: &FxHashSet<String>,
) -> bool {
    let has_node_modules = call.arguments.iter().any(
        |arg| matches!(arg, Argument::StringLiteral(lit) if lit.value.as_str() == "node_modules"),
    );
    has_node_modules
        && call
            .arguments
            .iter()
            .any(|arg| package_param_argument_identifier_name(arg, params).is_some())
}

fn package_param_argument_identifier_name(
    arg: &Argument<'_>,
    params: &FxHashSet<String>,
) -> Option<String> {
    match arg {
        Argument::Identifier(ident) if params.contains(ident.name.as_str()) => {
            Some(ident.name.to_string())
        }
        _ => arg
            .as_expression()
            .and_then(|expr| package_param_expression_identifier_name(expr, params)),
    }
}

fn package_param_expression_identifier_name(
    expr: &Expression<'_>,
    params: &FxHashSet<String>,
) -> Option<String> {
    match expr {
        Expression::Identifier(ident) if params.contains(ident.name.as_str()) => {
            Some(ident.name.to_string())
        }
        _ => None,
    }
}
