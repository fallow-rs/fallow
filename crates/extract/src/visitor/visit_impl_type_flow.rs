//! The type flow of an inferred expression.
//!
//! A binding, a class field or a function without a type annotation gets its
//! type from an expression. Declaration emit writes that inferred type into
//! the declaration file, so each type that the expression carries must stay
//! importable from the module that declares it (issue #4850).
//!
//! [`TypeFlow`] is the one walk of these expressions. It gives two parts:
//!
//! - the type references that the expression names itself, for example the
//!   asserted type of `x as Foo` or the parameter types of an arrow;
//! - the held names: what the expression holds as a value. A held name is a
//!   module binding (`entryBox`), a property path of a module binding
//!   (`holder.box`) or the result of a call of a module function
//!   (`makeBox()`).
//!
//! The visitor records the held names of each owner. After the walk,
//! `signature_value_aliases` adds the type references of each held name to
//! the export that holds it. The walk does not follow a call argument into
//! the call result, because only the type checker knows the type of a
//! generic call or a library call.

use super::visit_helpers::*;
use super::*;
use crate::js_text::JsText;

/// The most property links that a held member path keeps. A longer chain
/// gives no held name.
const MAX_HELD_PATH_LINKS: usize = 8;

/// The suffix of the owner name that holds the return type of a function:
/// `makeBox()` for `function makeBox()`.
pub(super) const CALL_RESULT_SUFFIX: &str = "()";

/// The type references and the held names of an inferred type.
#[derive(Debug, Default)]
pub(super) struct TypeFlow {
    /// The type references of the whole type.
    pub(super) refs: Vec<(String, Span)>,
    /// The held names of the whole type.
    pub(super) held: Vec<String>,
    /// The type references of each static property path of an object literal
    /// initializer, as `(".a.b", reference)`. Only
    /// [`TypeFlow::of_initializer`] fills it.
    pub(super) property_refs: Vec<(String, (String, Span))>,
    /// The held names of each static property path, as `(".a.b", held)`.
    pub(super) property_held: Vec<(String, String)>,
}

impl TypeFlow {
    /// The type flow of a module-level initializer, with the facts of each
    /// static property path of an object literal. A property read such as
    /// `holder.box` then finds the facts of `.box`.
    pub(super) fn of_initializer(init: &Expression<'_>) -> Self {
        let scope = Scope::default();
        let mut walk = Walk::new(&scope, true);
        walk.expression(init);
        walk.flow
    }

    /// The type flow of an expression at module level, without property
    /// paths. A class field initializer is an example.
    pub(super) fn of_module_expression(expression: &Expression<'_>) -> Self {
        let scope = Scope::default();
        let mut walk = Walk::new(&scope, false);
        walk.expression(expression);
        walk.flow
    }

    /// The type flow of the whole signature of a module-level function.
    pub(super) fn of_function(function: &Function<'_>) -> Self {
        function_flow(function, &FxHashSet::default())
    }

    /// The whole type flow and the return type flow of a module-level
    /// function.
    pub(super) fn of_function_parts(function: &Function<'_>) -> (Self, Self) {
        function_parts(function, &FxHashSet::default())
    }

    /// The whole type flow and the return type flow of a module-level arrow.
    pub(super) fn of_arrow_parts(arrow: &ArrowFunctionExpression<'_>) -> (Self, Self) {
        arrow_parts(arrow, &FxHashSet::default())
    }

    /// Add the parts of a nested flow to the whole type.
    fn absorb(&mut self, other: Self) {
        self.refs.extend(other.refs);
        self.held.extend(other.held);
    }
}

/// Remove the references to the type parameters of a declaration. A type
/// parameter is not a module type.
pub(super) fn remove_type_parameter_refs(
    refs: &mut Vec<(String, Span)>,
    type_parameters: Option<&TSTypeParameterDeclaration<'_>>,
) {
    let Some(type_parameters) = type_parameters else {
        return;
    };
    refs.retain(|(name, _)| {
        !type_parameters
            .params
            .iter()
            .any(|parameter| parameter.name.name == name.as_str())
    });
}

/// A function-valued binding declared directly in a function body.
#[derive(Clone, Copy)]
enum LocalCallable<'b, 'a> {
    Function(&'b Function<'a>),
    Arrow(&'b ArrowFunctionExpression<'a>),
}

/// The names that a function declares, and its local callables. A held name
/// must not be one of these names, because the name then holds a local value
/// and not the module binding.
#[derive(Default)]
struct Scope<'b, 'a> {
    shadowed: FxHashSet<String>,
    locals: FxHashMap<&'b str, LocalCallable<'b, 'a>>,
}

struct Walk<'s, 'b, 'a> {
    scope: &'s Scope<'b, 'a>,
    flow: TypeFlow,
    /// The static property keys from the root object literal to the current
    /// value. `None` when the walk does not keep property paths, and inside
    /// an array element.
    path: Option<Vec<String>>,
}

impl<'s, 'b, 'a> Walk<'s, 'b, 'a> {
    fn new(scope: &'s Scope<'b, 'a>, keep_paths: bool) -> Self {
        Self {
            scope,
            flow: TypeFlow::default(),
            path: keep_paths.then(Vec::new),
        }
    }

    /// The property path prefixes of the current value: `.a` and `.a.b`.
    fn path_prefixes(&self) -> Vec<String> {
        let Some(path) = &self.path else {
            return Vec::new();
        };
        let mut prefix = String::new();
        path.iter()
            .map(|key| {
                prefix.push('.');
                prefix.push_str(key);
                prefix.clone()
            })
            .collect()
    }

    fn push_refs(&mut self, refs: Vec<(String, Span)>) {
        for prefix in self.path_prefixes() {
            self.flow.property_refs.extend(
                refs.iter()
                    .cloned()
                    .map(|reference| (prefix.clone(), reference)),
            );
        }
        self.flow.refs.extend(refs);
    }

    fn push_held(&mut self, held: String) {
        for prefix in self.path_prefixes() {
            self.flow.property_held.push((prefix, held.clone()));
        }
        self.flow.held.push(held);
    }

    fn push_flow(&mut self, flow: TypeFlow) {
        self.push_refs(flow.refs);
        for held in flow.held {
            self.push_held(held);
        }
    }

    fn push_type(&mut self, ty: &TSType<'_>) {
        let mut collector = SignatureTypeCollector::default();
        collector.visit_ts_type(ty);
        self.push_refs(collector.refs);
    }

    fn is_shadowed(&self, name: &str) -> bool {
        self.scope.shadowed.contains(name)
    }

    fn expression(&mut self, expression: &Expression<'_>) {
        match expression {
            Expression::ParenthesizedExpression(inner) => self.expression(&inner.expression),
            Expression::TSNonNullExpression(inner) => self.expression(&inner.expression),
            // A `satisfies` clause does not change the type of the value.
            Expression::TSSatisfiesExpression(inner) => self.expression(&inner.expression),
            // `as const` keeps the structure of the value. Any other
            // assertion sets the type.
            Expression::TSAsExpression(assertion) => {
                if is_const_assertion(&assertion.type_annotation) {
                    self.expression(&assertion.expression);
                } else {
                    self.push_type(&assertion.type_annotation);
                }
            }
            Expression::TSTypeAssertion(assertion) => self.push_type(&assertion.type_annotation),
            Expression::ArrowFunctionExpression(arrow) => {
                self.push_flow(arrow_flow(arrow, &self.scope.shadowed));
            }
            Expression::FunctionExpression(function) => {
                self.push_flow(function_flow(function, &self.scope.shadowed));
            }
            Expression::Identifier(identifier) => self.identifier(identifier.name.as_str()),
            Expression::StaticMemberExpression(_) => self.member_read(expression),
            Expression::CallExpression(call) => self.call(call),
            Expression::NewExpression(new_expression) => {
                self.wrapper_call(
                    new_expression.type_arguments.as_deref(),
                    &new_expression.arguments,
                );
            }
            Expression::ObjectExpression(object) => self.object(object),
            Expression::ArrayExpression(array) => self.array(array),
            Expression::ConditionalExpression(conditional) => {
                self.expression(&conditional.consequent);
                self.expression(&conditional.alternate);
            }
            Expression::LogicalExpression(logical) => {
                self.expression(&logical.left);
                self.expression(&logical.right);
            }
            _ => {}
        }
    }

    /// A static member read on a module binding holds the property path.
    fn member_read(&mut self, expression: &Expression<'_>) {
        if let Some(path) = member_path(expression)
            && !self.is_shadowed(path.split('.').next().unwrap_or_default())
        {
            self.push_held(path);
        }
    }

    /// A call of a module function holds the return type of that function.
    fn call(&mut self, call: &CallExpression<'_>) {
        self.wrapper_call(call.type_arguments.as_deref(), &call.arguments);
        if let Expression::Identifier(callee) = call.callee.without_parentheses()
            && !self.is_shadowed(callee.name.as_str())
        {
            self.push_held(format!("{}{CALL_RESULT_SUFFIX}", callee.name));
        }
    }

    fn array(&mut self, array: &ArrayExpression<'_>) {
        for element in &array.elements {
            if matches!(element, ArrayExpressionElement::SpreadElement(_)) {
                continue;
            }
            if let Some(element) = element.as_expression() {
                self.expression_without_paths(element);
            }
        }
    }

    /// Walk a value whose own property keys do not extend the property path,
    /// such as an array element. Its facts belong to the current path.
    fn expression_without_paths(&mut self, expression: &Expression<'_>) {
        if self.path.is_none() {
            self.expression(expression);
            return;
        }
        let mut inner = Walk::new(self.scope, false);
        inner.expression(expression);
        self.push_flow(inner.flow);
    }

    fn identifier(&mut self, name: &str) {
        match self.scope.locals.get(name) {
            Some(LocalCallable::Function(function)) => {
                self.push_flow(function_flow(function, &self.scope.shadowed));
            }
            Some(LocalCallable::Arrow(arrow)) => {
                self.push_flow(arrow_flow(arrow, &self.scope.shadowed));
            }
            None if !self.is_shadowed(name) => self.push_held(name.to_string()),
            None => {}
        }
    }

    fn object(&mut self, object: &ObjectExpression<'_>) {
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            let key = (property.kind == PropertyKind::Init && !property.method)
                .then(|| property.key.static_name())
                .flatten()
                .map(|name| name.text().into_owned());
            let Some(key) = key.filter(|_| self.path.is_some()) else {
                self.expression_without_paths(&property.value);
                continue;
            };
            if let Some(path) = self.path.as_mut() {
                path.push(key);
            }
            self.expression(&property.value);
            if let Some(path) = self.path.as_mut() {
                path.pop();
            }
        }
    }

    /// A call or `new` wrapper takes its type from its type arguments and
    /// from the signature of a function argument, for example
    /// `memo(function C(p: Props) {})`. A value that a function argument
    /// returns does not count, because the callee decides the result type.
    fn wrapper_call(
        &mut self,
        type_arguments: Option<&TSTypeParameterInstantiation<'_>>,
        arguments: &[Argument<'_>],
    ) {
        if let Some(type_arguments) = type_arguments {
            let mut collector = SignatureTypeCollector::default();
            collector.visit_ts_type_parameter_instantiation(type_arguments);
            self.push_refs(collector.refs);
        }
        for argument in arguments {
            let refs = match argument {
                Argument::ArrowFunctionExpression(arrow) => {
                    arrow_flow(arrow, &self.scope.shadowed).refs
                }
                Argument::FunctionExpression(function) => {
                    function_flow(function, &self.scope.shadowed).refs
                }
                _ => continue,
            };
            self.push_refs(refs);
        }
    }
}

/// `as const` keeps the structure of the expression, so the held bindings
/// keep their types.
fn is_const_assertion(ty: &TSType<'_>) -> bool {
    matches!(
        ty,
        TSType::TSTypeReference(reference)
            if matches!(&reference.type_name, TSTypeName::IdentifierReference(name) if name.name == "const")
    )
}

/// The dotted path of a static member read on an identifier: `holder.box`
/// for `holder.box`. Any other object gives `None`.
fn member_path(expression: &Expression<'_>) -> Option<String> {
    let mut links = Vec::new();
    let mut current = expression;
    loop {
        match current {
            Expression::StaticMemberExpression(member) => {
                if links.len() == MAX_HELD_PATH_LINKS {
                    return None;
                }
                links.push(member.property.name.as_str());
                current = &member.object;
            }
            Expression::Identifier(identifier) => {
                links.push(identifier.name.as_str());
                break;
            }
            _ => return None,
        }
    }
    links.reverse();
    Some(links.join("."))
}

/// The whole type flow and the return type flow of a function. The whole
/// flow has the types of the type parameters, the `this` parameter and the
/// parameters too. The walk of the body runs once for both flows.
fn function_parts(
    function: &Function<'_>,
    outer_shadowed: &FxHashSet<String>,
) -> (TypeFlow, TypeFlow) {
    let mut signature = SignatureTypeCollector::default();
    if let Some(type_parameters) = function.type_parameters.as_deref() {
        signature.visit_ts_type_parameter_declaration(type_parameters);
    }
    if let Some(this_param) = function.this_param.as_deref() {
        signature.visit_ts_this_parameter(this_param);
    }
    visit_parameter_annotations(&function.params, &mut signature);
    let mut returned = TypeFlow::default();
    if let Some(return_type) = function.return_type.as_deref() {
        returned.refs = annotation_refs(return_type);
    } else if let Some(body) = function.body.as_deref() {
        let mut shadowed = outer_shadowed.clone();
        if let Some(id) = function.id.as_ref() {
            shadowed.insert(id.name.to_string());
        }
        collect_declared_names(&function.params, Some(body), &mut shadowed);
        let scope = Scope {
            shadowed,
            locals: collect_local_callables(body),
        };
        returned.absorb(returned_values_flow(body, &scope));
    }
    combine_parts(
        signature.refs,
        returned,
        function.type_parameters.as_deref(),
    )
}

/// The whole type flow and the return type flow of an arrow, as
/// [`function_parts`] gives them for a function.
fn arrow_parts(
    arrow: &ArrowFunctionExpression<'_>,
    outer_shadowed: &FxHashSet<String>,
) -> (TypeFlow, TypeFlow) {
    let mut signature = SignatureTypeCollector::default();
    if let Some(type_parameters) = arrow.type_parameters.as_deref() {
        signature.visit_ts_type_parameter_declaration(type_parameters);
    }
    visit_parameter_annotations(&arrow.params, &mut signature);
    let mut returned = TypeFlow::default();
    if let Some(return_type) = arrow.return_type.as_deref() {
        returned.refs = annotation_refs(return_type);
    } else if let Some(expression) = arrow.get_expression() {
        let mut shadowed = outer_shadowed.clone();
        let mut names = DeclaredNameCollector {
            names: &mut shadowed,
        };
        names.visit_formal_parameters(&arrow.params);
        names.visit_expression(expression);
        let scope = Scope {
            shadowed,
            locals: FxHashMap::default(),
        };
        let mut walk = Walk::new(&scope, false);
        walk.expression(expression);
        returned.absorb(walk.flow);
    } else if let Some(body) = arrow.get_function_body() {
        let mut shadowed = outer_shadowed.clone();
        collect_declared_names(&arrow.params, Some(body), &mut shadowed);
        let scope = Scope {
            shadowed,
            locals: collect_local_callables(body),
        };
        returned.absorb(returned_values_flow(body, &scope));
    }
    combine_parts(signature.refs, returned, arrow.type_parameters.as_deref())
}

fn function_flow(function: &Function<'_>, outer_shadowed: &FxHashSet<String>) -> TypeFlow {
    function_parts(function, outer_shadowed).0
}

fn arrow_flow(arrow: &ArrowFunctionExpression<'_>, outer_shadowed: &FxHashSet<String>) -> TypeFlow {
    arrow_parts(arrow, outer_shadowed).0
}

fn annotation_refs(annotation: &TSTypeAnnotation<'_>) -> Vec<(String, Span)> {
    let mut collector = SignatureTypeCollector::default();
    collector.visit_ts_type_annotation(annotation);
    collector.refs
}

/// Build the whole flow from the signature references and the return flow.
/// Both flows lose the references to the type parameters.
fn combine_parts(
    signature_refs: Vec<(String, Span)>,
    mut returned: TypeFlow,
    type_parameters: Option<&TSTypeParameterDeclaration<'_>>,
) -> (TypeFlow, TypeFlow) {
    remove_type_parameter_refs(&mut returned.refs, type_parameters);
    let mut whole = TypeFlow {
        refs: signature_refs,
        held: returned.held.clone(),
        ..TypeFlow::default()
    };
    remove_type_parameter_refs(&mut whole.refs, type_parameters);
    whole.refs.extend(returned.refs.iter().cloned());
    (whole, returned)
}

fn visit_parameter_annotations(
    params: &FormalParameters<'_>,
    collector: &mut SignatureTypeCollector,
) {
    for param in &params.items {
        if let Some(annotation) = param.type_annotation.as_deref() {
            collector.visit_ts_type_annotation(annotation);
        }
    }
    if let Some(rest) = params.rest.as_deref()
        && let Some(annotation) = rest.type_annotation.as_deref()
    {
        collector.visit_ts_type_annotation(annotation);
    }
}

/// Collect each name that the parameters and the body declare, nested
/// scopes included. A name that a nested scope declares can hide a module
/// binding at a return, so the walk treats it as hidden everywhere in the
/// function. This can miss a held binding, but it never credits a local.
fn collect_declared_names(
    params: &FormalParameters<'_>,
    body: Option<&FunctionBody<'_>>,
    names: &mut FxHashSet<String>,
) {
    let mut collector = DeclaredNameCollector { names };
    collector.visit_formal_parameters(params);
    if let Some(body) = body {
        collector.visit_function_body(body);
    }
}

struct DeclaredNameCollector<'n> {
    names: &'n mut FxHashSet<String>,
}

impl<'a> Visit<'a> for DeclaredNameCollector<'_> {
    fn visit_binding_identifier(&mut self, identifier: &BindingIdentifier<'a>) {
        self.names.insert(identifier.name.to_string());
    }
}

/// The type flow of the values that the return statements of one function
/// body return. Type annotations on other body values stay private, so a type
/// that only annotates a local value keeps its unused finding.
fn returned_values_flow(body: &FunctionBody<'_>, scope: &Scope<'_, '_>) -> TypeFlow {
    let mut collector = ReturnedValueCollector {
        walk: Walk::new(scope, false),
    };
    collector.visit_function_body(body);
    collector.walk.flow
}

fn collect_local_callables<'b, 'a>(
    body: &'b FunctionBody<'a>,
) -> FxHashMap<&'b str, LocalCallable<'b, 'a>> {
    let mut locals = FxHashMap::default();
    for statement in &body.statements {
        match statement {
            Statement::FunctionDeclaration(function) => {
                if let Some(id) = function.id.as_ref() {
                    locals.insert(id.name.as_str(), LocalCallable::Function(function));
                }
            }
            Statement::VariableDeclaration(declaration) => {
                for declarator in &declaration.declarations {
                    let (BindingPattern::BindingIdentifier(id), Some(init)) =
                        (&declarator.id, &declarator.init)
                    else {
                        continue;
                    };
                    let callable = match init.without_parentheses() {
                        Expression::ArrowFunctionExpression(arrow) => LocalCallable::Arrow(arrow),
                        Expression::FunctionExpression(function) => {
                            LocalCallable::Function(function)
                        }
                        _ => continue,
                    };
                    locals.insert(id.name.as_str(), callable);
                }
            }
            _ => {}
        }
    }
    locals
}

/// Visit the return statements of one function body. Nested functions and
/// classes own their return statements, so the walk does not enter them.
struct ReturnedValueCollector<'s, 'b, 'a> {
    walk: Walk<'s, 'b, 'a>,
}

impl<'x> Visit<'x> for ReturnedValueCollector<'_, '_, '_> {
    fn visit_return_statement(&mut self, statement: &ReturnStatement<'x>) {
        if let Some(argument) = &statement.argument {
            self.walk.expression(argument);
        }
    }

    fn visit_function(&mut self, _function: &Function<'x>, _flags: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _arrow: &ArrowFunctionExpression<'x>) {}

    fn visit_class(&mut self, _class: &Class<'x>) {}
}

impl ModuleInfoExtractor {
    /// Record the type flow of the module binding `owner`: its type
    /// references and its held names.
    pub(super) fn record_type_flow(&mut self, owner: &str, flow: TypeFlow) {
        let TypeFlow {
            refs,
            held,
            property_refs,
            property_held,
        } = flow;
        self.record_local_signature_refs(owner, refs);
        self.record_held_names(owner, held);
        for (path, reference) in property_refs {
            self.record_local_signature_refs(&format!("{owner}{path}"), vec![reference]);
        }
        for (path, held) in property_held {
            self.local_value_aliases
                .push((format!("{owner}{path}"), held));
        }
    }

    fn record_held_names(&mut self, owner: &str, held: Vec<String>) {
        self.local_value_aliases
            .extend(held.into_iter().map(|held| (owner.to_string(), held)));
    }

    /// Record the type flow of a module-level function `name`, and the flow
    /// of its return type under `name()`. A call `name()` in an inferred
    /// type then holds that return type.
    pub(super) fn record_function_type_flow(&mut self, name: &str, function: &Function<'_>) {
        let (whole, returned) = TypeFlow::of_function_parts(function);
        self.record_type_flow(name, whole);
        self.record_type_flow(&format!("{name}{CALL_RESULT_SUFFIX}"), returned);
    }

    /// Record public-API / local signature type references for a default
    /// export. A named class or function records a local type flow. An
    /// anonymous class or function and an expression record public refs keyed
    /// on `"default"`, and their held names under the owner `"default"`. An
    /// identifier forwards to its binding through the export local name.
    pub(super) fn record_default_export_signature_refs(
        &mut self,
        decl: &ExportDefaultDeclaration<'_>,
    ) {
        match &decl.declaration {
            ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                let flow = Self::collect_class_type_flow(class);
                if let Some(id) = class.id.as_ref() {
                    self.record_local_type_declaration(&id.name, id.span);
                    self.record_type_flow(&id.name, flow);
                } else {
                    self.record_default_type_flow(flow);
                }
            }
            ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                if let Some(id) = function.id.as_ref() {
                    self.record_function_type_flow(&id.name, function);
                } else {
                    self.record_default_type_flow(TypeFlow::of_function(function));
                }
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(iface) => {
                self.record_local_type_declaration(&iface.id.name, iface.id.span);
                let refs = Self::collect_interface_signature_refs(iface);
                self.record_public_signature_refs("default", refs);
            }
            ExportDefaultDeclarationKind::Identifier(_) => {}
            other => {
                if let Some(expression) = other.as_expression() {
                    self.record_default_type_flow(TypeFlow::of_module_expression(expression));
                }
            }
        }
    }

    fn record_default_type_flow(&mut self, flow: TypeFlow) {
        self.record_public_signature_refs("default", flow.refs);
        self.record_held_names("default", flow.held);
    }
}
