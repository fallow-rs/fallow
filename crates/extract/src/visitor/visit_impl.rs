//! `Visit` trait implementation for `ModuleInfoExtractor`.
//!
//! Handles all AST node types: imports, exports, expressions, statements.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_semantic::ScopeFlags;
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Component, PathBuf};

use crate::{
    DynamicImportInfo, DynamicImportPattern, ExportInfo, ExportName, ImportInfo, ImportedName,
    MemberAccess, ReExportInfo, RequireCallInfo, VisibilityTag,
};
use fallow_types::extract::{
    ClassHeritageInfo, LocalTypeDeclaration, PublicSignatureTypeReference,
};

use crate::asset_url::normalize_asset_url;
use crate::html::is_remote_url;

use super::helpers::{
    extract_angular_component_metadata, extract_angular_signal_query, extract_class_members,
    extract_concat_parts, extract_custom_elements_define, extract_implemented_interface_names,
    extract_nested_type_bindings, extract_query_list_element_type, extract_super_class_name,
    extract_type_annotation_name, has_angular_class_decorator, has_angular_plural_query_decorator,
    is_meta_url_arg, lit_custom_element_decorator, regex_pattern_to_suffix,
    ts_import_type_qualifier_root,
};
use super::{
    ModuleInfoExtractor, PendingLocalExportSpecifier, SideEffectRegistrationTarget,
    try_extract_arrow_wrapped_import, try_extract_dynamic_import, try_extract_import_then_callback,
    try_extract_property_callback_import, try_extract_require,
};

#[derive(Default)]
struct SignatureTypeCollector {
    refs: Vec<(String, Span)>,
}

impl<'a> Visit<'a> for SignatureTypeCollector {
    fn visit_ts_type_reference(&mut self, type_ref: &TSTypeReference<'a>) {
        if let Some((name, span)) = type_name_root(&type_ref.type_name) {
            self.refs.push((name, span));
        }
        walk::walk_ts_type_reference(self, type_ref);
    }
}

fn type_name_root(name: &TSTypeName<'_>) -> Option<(String, Span)> {
    match name {
        TSTypeName::IdentifierReference(ident) => Some((ident.name.to_string(), ident.span)),
        TSTypeName::QualifiedName(qualified) => type_name_root(&qualified.left),
        TSTypeName::ThisExpression(_) => None,
    }
}

fn expression_root_name(expr: &Expression<'_>) -> Option<(String, Span)> {
    match expr {
        Expression::Identifier(ident) => Some((ident.name.to_string(), ident.span)),
        Expression::StaticMemberExpression(member) => expression_root_name(&member.object),
        _ => None,
    }
}

fn is_private_member_key(key: &PropertyKey<'_>) -> bool {
    matches!(key, PropertyKey::PrivateIdentifier(_))
}

fn vitest_mock_source(call: &CallExpression<'_>) -> Option<String> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if member.property.name != "mock" {
        return None;
    }
    let Expression::Identifier(object) = &member.object else {
        return None;
    };
    if object.name != "vi" {
        return None;
    }

    call.arguments.first().and_then(|argument| match argument {
        Argument::StringLiteral(value) => Some(value.value.to_string()),
        Argument::TemplateLiteral(value) if value.expressions.is_empty() => value
            .quasis
            .first()
            .map(|quasi| quasi.value.raw.to_string()),
        Argument::ImportExpression(value) => match &value.source {
            Expression::StringLiteral(source) => Some(source.value.to_string()),
            _ => None,
        },
        _ => None,
    })
}

fn vitest_auto_mock_source(source: &str) -> Option<String> {
    if source.is_empty()
        || source.contains("://")
        || source.starts_with("data:")
        || source.split('/').any(|segment| segment == "__mocks__")
    {
        return None;
    }

    let (dir, file_name) = source.rsplit_once('/')?;
    if file_name.is_empty() {
        return None;
    }

    Some(format!("{dir}/__mocks__/{file_name}"))
}

/// Detect whether a `vi.mock(specifier, factory, ...)` call provides a factory
/// function as the second argument.
///
/// Vitest only consults the `__mocks__/<file>` sibling convention when the
/// caller does NOT pass a factory; with a factory, vitest uses the factory
/// directly and the `__mocks__/<file>` sibling is irrelevant. Synthesizing
/// the auto-mock import in the factory case produces a spurious
/// `unresolved-import` finding when no `__mocks__/<file>` exists. See issue
/// #311. The factory is detected as either an arrow function or a function
/// expression in the second-argument position; an object literal in that
/// position is treated as `vi.mock(spec, options)` (rare auto-mock options
/// form), where vitest still consults `__mocks__/<file>`. Oxc parses with
/// `preserve_parens: true` by default, so parenthesized factories
/// (`vi.mock('x', (((() => ({})))))`) arrive wrapped in one or more
/// `ParenthesizedExpression` nodes; unwrap through those so the callable is
/// recognised.
fn vi_mock_has_factory(call: &CallExpression<'_>) -> bool {
    fn is_factory_expression(expr: &Expression<'_>) -> bool {
        match expr {
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => true,
            Expression::ParenthesizedExpression(paren) => is_factory_expression(&paren.expression),
            _ => false,
        }
    }

    fn is_factory_arg(arg: &Argument<'_>) -> bool {
        match arg {
            Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_) => true,
            Argument::ParenthesizedExpression(paren) => is_factory_expression(&paren.expression),
            _ => false,
        }
    }

    call.arguments.get(1).is_some_and(is_factory_arg)
}

/// Specifier source string from the first argument of a `register(...)` call.
///
/// `node:module`'s `register` hook (issue #293) loads a loader module by
/// specifier (a bare package, package subpath, or relative URL). Returns the
/// raw string when the first argument is a string or no-substitution template
/// literal so the caller can credit it as a dynamic import.
fn node_module_register_specifier(call: &CallExpression<'_>) -> Option<String> {
    match call.arguments.first()? {
        Argument::StringLiteral(value) => Some(value.value.to_string()),
        Argument::TemplateLiteral(value) if value.expressions.is_empty() => value
            .quasis
            .first()
            .map(|quasi| quasi.value.raw.to_string()),
        _ => None,
    }
}

/// Node `module.register()` invokes loader-hook exports reflectively. The
/// allowlist covers the current Node 22+ hook set (`initialize`, `resolve`,
/// `load`, `globalPreload`) plus the legacy hooks (`getFormat`, `getSource`,
/// `transformSource`) that older Node versions still in the wild (16.x with
/// `--experimental-loader`, downstream forks) invoke. Extra entries are
/// inert: if the loader file does not export them, no symbol reference is
/// recorded. See issue #589.
const NODE_MODULE_REGISTER_HOOK_EXPORTS: &[&str] = &[
    "initialize",
    "resolve",
    "load",
    "globalPreload",
    "getFormat",
    "getSource",
    "transformSource",
];

fn loader_hook_exports_for_source(source: &str) -> Vec<String> {
    if source.starts_with("./")
        || source.starts_with("../")
        || source.starts_with('/')
        || source.starts_with("file:")
    {
        NODE_MODULE_REGISTER_HOOK_EXPORTS
            .iter()
            .map(|name| (*name).to_string())
            .collect()
    } else {
        Vec::new()
    }
}

fn new_url_import_source(expr: &NewExpression<'_>) -> Option<String> {
    if let Expression::Identifier(callee) = &expr.callee
        && callee.name == "URL"
        && expr.arguments.len() == 2
        && let Some(Argument::StringLiteral(path_lit)) = expr.arguments.first()
        && is_meta_url_arg(&expr.arguments[1])
        && (path_lit.value.starts_with("./") || path_lit.value.starts_with("../"))
        && !path_lit.value.ends_with('/')
    {
        Some(path_lit.value.to_string())
    } else {
        None
    }
}

fn is_child_process_source(source: &str) -> bool {
    matches!(source, "node:child_process" | "child_process")
}

fn is_node_path_source(source: &str) -> bool {
    matches!(source, "node:path" | "path")
}

fn is_node_url_source(source: &str) -> bool {
    matches!(source, "node:url" | "url")
}

fn local_fork_source(source: &str) -> Option<String> {
    if (source.starts_with("./") || source.starts_with("../")) && !source.ends_with('/') {
        Some(source.to_string())
    } else {
        None
    }
}

fn normalize_module_file_relative_path(relative: &str) -> Option<String> {
    if relative.is_empty() || relative.starts_with('/') || relative.ends_with('/') {
        return None;
    }

    let normalized = PathBuf::from("__fallow_current_file__").join(relative);

    let mut parts = Vec::new();
    for component in normalized.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if parts
        .first()
        .is_some_and(|part| part == "__fallow_current_file__")
        || parts.is_empty()
    {
        return None;
    }

    let joined = parts.join("/");
    if joined.starts_with("../") {
        Some(joined)
    } else {
        Some(format!("./{joined}"))
    }
}

#[derive(Default)]
struct PlaywrightFixtureMemberCollector {
    fixture_by_local: FxHashMap<String, String>,
    accesses: Vec<MemberAccess>,
}

impl PlaywrightFixtureMemberCollector {
    fn new(fixture_by_local: FxHashMap<String, String>) -> Self {
        Self {
            fixture_by_local,
            accesses: Vec::new(),
        }
    }
}

impl<'a> Visit<'a> for PlaywrightFixtureMemberCollector {
    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        if let Some(object_dotted) = static_member_object_name(&expr.object)
            && let Some(fixture_path) =
                resolve_object_to_fixture_path(&object_dotted, &self.fixture_by_local)
        {
            self.accesses.push(MemberAccess {
                object: fixture_path,
                member: expr.property.name.to_string(),
            });
            // The chain has been fully attributed; descending further would re-visit
            // intermediate `pages.adminPage` member exprs and emit spurious
            // `(pages, adminPage)` accesses. Walk into the property node only.
            return;
        }
        walk::walk_static_member_expression(self, expr);
    }
}

fn extract_binding_local_name<'a>(pattern: &'a BindingPattern<'a>) -> Option<&'a str> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        BindingPattern::AssignmentPattern(assign) => extract_binding_local_name(&assign.left),
        _ => None,
    }
}

fn extract_object_pattern_bindings(pattern: &ObjectPattern<'_>) -> FxHashMap<String, String> {
    let mut bindings = FxHashMap::default();
    collect_object_pattern_bindings(pattern, "", &mut bindings);
    bindings
}

fn collect_object_pattern_bindings(
    pattern: &ObjectPattern<'_>,
    path_prefix: &str,
    bindings: &mut FxHashMap<String, String>,
) {
    for prop in &pattern.properties {
        let Some(fixture_name) = prop.key.static_name() else {
            continue;
        };
        let next_path = if path_prefix.is_empty() {
            fixture_name.to_string()
        } else {
            format!("{path_prefix}.{fixture_name}")
        };
        match &prop.value {
            BindingPattern::ObjectPattern(inner) => {
                collect_object_pattern_bindings(inner, &next_path, bindings);
            }
            other => {
                if let Some(local_name) = extract_binding_local_name(other) {
                    bindings.insert(local_name.to_string(), next_path);
                }
            }
        }
    }
}

fn resolve_object_to_fixture_path(
    object_dotted: &str,
    fixture_by_local: &FxHashMap<String, String>,
) -> Option<String> {
    let (root, rest) = object_dotted
        .split_once('.')
        .map_or((object_dotted, ""), |(r, x)| (r, x));
    let base = fixture_by_local.get(root)?;
    if rest.is_empty() {
        Some(base.clone())
    } else {
        Some(format!("{base}.{rest}"))
    }
}

fn playwright_test_callee_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => playwright_test_callee_name(&member.object),
        // Curried form `appTest()(...)` where the test value is produced by
        // a helper function call. Recurse into the inner call's callee so
        // the def sentinel keyed by the helper name (recorded via
        // `resolve_playwright_factory_call_definitions`) correlates with
        // the use sentinel emitted here. Safe because def sentinels gate
        // the analyzer; use sentinels for unmatched names produce no credit.
        // See issue #491.
        Expression::CallExpression(call) => playwright_test_callee_name(&call.callee),
        _ => None,
    }
}

/// Find the call expression from a function body's final `return <call>`
/// statement, or `None` if the body is empty or returns anything other than a
/// call expression.
///
/// Used to detect helper-function Playwright fixtures such as
/// `function appTest() { return base.extend<T>(...); }`, helpers with local
/// setup before the return, and chained helpers
/// `function appTest() { return setupTestFixture(); }`. See issues #491/#586.
fn extract_function_body_final_return_call<'a, 'b>(
    body: &'b oxc_ast::ast::FunctionBody<'a>,
) -> Option<&'b CallExpression<'a>> {
    let Statement::ReturnStatement(ret) = body.statements.last()? else {
        return None;
    };
    let Expression::CallExpression(call) = ret.argument.as_ref()? else {
        return None;
    };
    Some(call.as_ref())
}

/// Find the call expression that is the body of an arrow function, supporting
/// both expression-body (`() => base.extend<T>(...)`) and block-body
/// (`() => { return base.extend<T>(...); }`) shapes. See issues #491/#586.
fn extract_arrow_return_call<'a, 'b>(
    arrow: &'b oxc_ast::ast::ArrowFunctionExpression<'a>,
) -> Option<&'b CallExpression<'a>> {
    if arrow.expression {
        // Expression-body arrows: oxc wraps the body expression as the sole
        // ExpressionStatement of `arrow.body.statements`.
        if arrow.body.statements.len() != 1 {
            return None;
        }
        let Statement::ExpressionStatement(stmt) = arrow.body.statements.first()? else {
            return None;
        };
        let Expression::CallExpression(call) = &stmt.expression else {
            return None;
        };
        return Some(call.as_ref());
    }
    extract_function_body_final_return_call(&arrow.body)
}

fn collect_playwright_fixture_member_uses(
    test_name: &str,
    arguments: &[Argument<'_>],
) -> Vec<MemberAccess> {
    let Some(callback) = arguments.iter().find_map(|arg| match arg {
        Argument::ArrowFunctionExpression(arrow) => {
            Some((arrow.params.items.first()?, arrow.body.as_ref()))
        }
        Argument::FunctionExpression(function) => {
            Some((function.params.items.first()?, function.body.as_deref()?))
        }
        _ => None,
    }) else {
        return Vec::new();
    };

    let BindingPattern::ObjectPattern(pattern) = &callback.0.pattern else {
        return Vec::new();
    };
    let fixture_by_local = extract_object_pattern_bindings(pattern);
    if fixture_by_local.is_empty() {
        return Vec::new();
    }

    let mut collector = PlaywrightFixtureMemberCollector::new(fixture_by_local);
    collector.visit_function_body(callback.1);
    collector
        .accesses
        .into_iter()
        .map(|access| MemberAccess {
            object: format!(
                "{}{}:{}",
                crate::PLAYWRIGHT_FIXTURE_USE_SENTINEL,
                test_name,
                access.object
            ),
            member: access.member,
        })
        .collect()
}

fn playwright_extend_base_name(call: &CallExpression<'_>) -> Option<String> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if member.property.name != "extend" {
        return None;
    }
    let Expression::Identifier(base) = &member.object else {
        return None;
    };
    Some(base.name.to_string())
}

fn collect_fixture_type_bindings_from_type(
    ty: &TSType<'_>,
    path_prefix: &str,
    aliases: &FxHashMap<String, Vec<(String, String)>>,
    bindings: &mut Vec<(String, String)>,
) {
    match ty {
        TSType::TSTypeLiteral(type_lit) => {
            for member in &type_lit.members {
                let TSSignature::TSPropertySignature(prop) = member else {
                    continue;
                };
                let Some(fixture_name) = prop.key.static_name() else {
                    continue;
                };
                let Some(type_annotation) = prop.type_annotation.as_deref() else {
                    continue;
                };
                let next_path = if path_prefix.is_empty() {
                    fixture_name.to_string()
                } else {
                    format!("{path_prefix}.{fixture_name}")
                };
                if let Some((alias_name, _)) =
                    fixture_type_reference_name(&type_annotation.type_annotation)
                    && aliases.contains_key(alias_name.as_str())
                {
                    collect_fixture_type_bindings_from_type(
                        &type_annotation.type_annotation,
                        &next_path,
                        aliases,
                        bindings,
                    );
                } else if let Some(type_name) = extract_type_annotation_name(type_annotation) {
                    bindings.push((next_path, type_name));
                } else {
                    collect_fixture_type_bindings_from_type(
                        &type_annotation.type_annotation,
                        &next_path,
                        aliases,
                        bindings,
                    );
                }
            }
        }
        TSType::TSTypeReference(type_ref) => {
            let Some((alias_name, _)) = type_name_root(&type_ref.type_name) else {
                return;
            };
            if let Some(alias_bindings) = aliases.get(alias_name.as_str()) {
                for (suffix, type_name) in alias_bindings {
                    let combined = if path_prefix.is_empty() {
                        suffix.clone()
                    } else {
                        format!("{path_prefix}.{suffix}")
                    };
                    bindings.push((combined, type_name.clone()));
                }
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for branch in &intersection.types {
                collect_fixture_type_bindings_from_type(branch, path_prefix, aliases, bindings);
            }
        }
        TSType::TSParenthesizedType(paren) => {
            collect_fixture_type_bindings_from_type(
                &paren.type_annotation,
                path_prefix,
                aliases,
                bindings,
            );
        }
        _ => {}
    }
}

fn fixture_type_reference_name(ty: &TSType<'_>) -> Option<(String, Span)> {
    match ty {
        TSType::TSTypeReference(type_ref) => type_name_root(&type_ref.type_name),
        TSType::TSParenthesizedType(paren) => fixture_type_reference_name(&paren.type_annotation),
        _ => None,
    }
}

impl ModuleInfoExtractor {
    fn is_module_scope(&self) -> bool {
        self.block_depth == 0 && self.function_depth == 0 && self.namespace_depth == 0
    }

    fn is_module_or_function_runtime_scope(&self) -> bool {
        self.namespace_depth == 0
    }

    fn nested_scope_shadows(&self, name: &str) -> bool {
        self.nested_declaration_stack
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
    }

    fn record_nested_declaration_names<'a>(
        &mut self,
        declarations: impl IntoIterator<Item = &'a BindingIdentifier<'a>>,
    ) {
        if self.namespace_depth > 0 {
            return;
        }
        let Some(scope) = self.nested_declaration_stack.last_mut() else {
            return;
        };
        scope.extend(declarations.into_iter().map(|id| id.name.to_string()));
    }

    fn push_function_declaration_scope(&mut self, params: &FormalParameters<'_>) {
        if self.namespace_depth > 0 {
            return;
        }

        let mut scope = FxHashSet::default();
        for param in &params.items {
            scope.extend(
                param
                    .pattern
                    .get_binding_identifiers()
                    .into_iter()
                    .map(|id| id.name.to_string()),
            );
        }
        self.nested_declaration_stack.push(scope);
    }

    fn pop_function_declaration_scope(&mut self) {
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.pop();
        }
    }

    fn record_node_module_register_url_binding(&mut self, name: String, sources: Vec<String>) {
        let entry = self
            .node_module_register_url_bindings
            .entry(name)
            .or_default();
        for source in sources {
            if !entry.contains(&source) {
                entry.push(source);
            }
        }
    }

    fn node_module_register_url_binding(&self, name: &str) -> Vec<String> {
        self.node_module_register_url_bindings
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    fn record_local_type_declaration(&mut self, name: &str, span: Span) {
        if self
            .local_type_declarations
            .iter()
            .any(|decl| decl.name == name)
        {
            return;
        }
        self.local_type_declarations.push(LocalTypeDeclaration {
            name: name.to_string(),
            span,
        });
    }

    fn record_local_signature_refs(&mut self, owner_name: &str, refs: Vec<(String, Span)>) {
        self.local_signature_type_references
            .extend(
                refs.into_iter()
                    .map(|(type_name, span)| super::LocalSignatureTypeReference {
                        owner_name: owner_name.to_string(),
                        type_name,
                        span,
                    }),
            );
    }

    fn record_public_signature_refs(&mut self, export_name: &str, refs: Vec<(String, Span)>) {
        self.public_signature_type_references
            .extend(
                refs.into_iter()
                    .map(|(type_name, span)| PublicSignatureTypeReference {
                        export_name: export_name.to_string(),
                        type_name,
                        span,
                    }),
            );
    }

    fn collect_type_refs_from_annotation(annotation: &TSTypeAnnotation<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        collector.visit_ts_type_annotation(annotation);
        collector.refs
    }

    fn collect_function_signature_refs(function: &Function<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = function.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        if let Some(this_param) = function.this_param.as_deref() {
            collector.visit_ts_this_parameter(this_param);
        }
        for param in &function.params.items {
            if let Some(annotation) = param.type_annotation.as_deref() {
                collector.visit_ts_type_annotation(annotation);
            }
        }
        if let Some(rest) = function.params.rest.as_deref()
            && let Some(annotation) = rest.type_annotation.as_deref()
        {
            collector.visit_ts_type_annotation(annotation);
        }
        if let Some(return_type) = function.return_type.as_deref() {
            collector.visit_ts_type_annotation(return_type);
        }
        collector.refs
    }

    fn collect_arrow_signature_refs(arrow: &ArrowFunctionExpression<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = arrow.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        for param in &arrow.params.items {
            if let Some(annotation) = param.type_annotation.as_deref() {
                collector.visit_ts_type_annotation(annotation);
            }
        }
        if let Some(rest) = arrow.params.rest.as_deref()
            && let Some(annotation) = rest.type_annotation.as_deref()
        {
            collector.visit_ts_type_annotation(annotation);
        }
        if let Some(return_type) = arrow.return_type.as_deref() {
            collector.visit_ts_type_annotation(return_type);
        }
        collector.refs
    }

    fn collect_variable_signature_refs(declarator: &VariableDeclarator<'_>) -> Vec<(String, Span)> {
        let mut refs = Vec::new();
        if let Some(annotation) = declarator.type_annotation.as_deref() {
            refs.extend(Self::collect_type_refs_from_annotation(annotation));
        }
        if let Some(init) = &declarator.init {
            match init {
                Expression::ArrowFunctionExpression(arrow) => {
                    refs.extend(Self::collect_arrow_signature_refs(arrow));
                }
                Expression::FunctionExpression(function) => {
                    refs.extend(Self::collect_function_signature_refs(function));
                }
                _ => {}
            }
        }
        refs
    }

    fn collect_class_signature_refs(class: &Class<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = class.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        if let Some(super_class) = class.super_class.as_ref()
            && let Some((name, span)) = expression_root_name(super_class)
        {
            collector.refs.push((name, span));
        }
        if let Some(type_arguments) = class.super_type_arguments.as_deref() {
            collector.visit_ts_type_parameter_instantiation(type_arguments);
        }
        for implemented in &class.implements {
            if let Some((name, span)) = type_name_root(&implemented.expression) {
                collector.refs.push((name, span));
            }
            if let Some(type_arguments) = implemented.type_arguments.as_deref() {
                collector.visit_ts_type_parameter_instantiation(type_arguments);
            }
        }
        for element in &class.body.body {
            match element {
                ClassElement::MethodDefinition(method) => {
                    if matches!(method.accessibility, Some(TSAccessibility::Private))
                        || is_private_member_key(&method.key)
                    {
                        continue;
                    }
                    collector
                        .refs
                        .extend(Self::collect_function_signature_refs(&method.value));
                }
                ClassElement::PropertyDefinition(prop) => {
                    if matches!(prop.accessibility, Some(TSAccessibility::Private))
                        || is_private_member_key(&prop.key)
                    {
                        continue;
                    }
                    if let Some(annotation) = prop.type_annotation.as_deref() {
                        collector.visit_ts_type_annotation(annotation);
                    }
                }
                ClassElement::AccessorProperty(prop) => {
                    if matches!(prop.accessibility, Some(TSAccessibility::Private))
                        || is_private_member_key(&prop.key)
                    {
                        continue;
                    }
                    if let Some(annotation) = prop.type_annotation.as_deref() {
                        collector.visit_ts_type_annotation(annotation);
                    }
                }
                ClassElement::TSIndexSignature(index) => {
                    collector.visit_ts_index_signature(index);
                }
                ClassElement::StaticBlock(_) => {}
            }
        }
        collector.refs
    }

    fn collect_interface_signature_refs(iface: &TSInterfaceDeclaration<'_>) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = iface.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        for heritage in &iface.extends {
            if let Some((name, span)) = expression_root_name(&heritage.expression) {
                collector.refs.push((name, span));
            }
            if let Some(type_arguments) = heritage.type_arguments.as_deref() {
                collector.visit_ts_type_parameter_instantiation(type_arguments);
            }
        }
        collector.visit_ts_interface_body(&iface.body);
        collector.refs
    }

    fn collect_type_alias_signature_refs(
        alias: &TSTypeAliasDeclaration<'_>,
    ) -> Vec<(String, Span)> {
        let mut collector = SignatureTypeCollector::default();
        if let Some(type_parameters) = alias.type_parameters.as_deref() {
            collector.visit_ts_type_parameter_declaration(type_parameters);
        }
        collector.visit_ts_type(&alias.type_annotation);
        collector.refs
    }

    fn record_typed_binding(&mut self, binding_name: &str, type_annotation: &TSTypeAnnotation<'_>) {
        if let Some(type_name) = extract_type_annotation_name(type_annotation)
            && let Some(resolved) = self.resolve_class_type_param(&type_name)
        {
            self.binding_target_names
                .insert(binding_name.to_string(), resolved);
        }

        for (property_path, type_name) in extract_nested_type_bindings(type_annotation) {
            let Some(resolved) = self.resolve_class_type_param(&type_name) else {
                continue;
            };
            self.binding_target_names
                .insert(format!("{binding_name}.{property_path}"), resolved);
        }
    }

    /// Substitute a class type-parameter with its constraint when the visitor
    /// is currently inside a class that declares `<T extends Foo>`.
    /// Returns `Some(constraint)` for a constrained parameter, `None` for an
    /// unconstrained parameter (drop the binding: there is no concrete class),
    /// or `Some(original)` for a non-parameter type name. See issue #388.
    fn resolve_class_type_param(&self, type_name: &str) -> Option<String> {
        let Some(frame) = self.class_type_param_constraints.last() else {
            return Some(type_name.to_string());
        };
        match frame.get(type_name) {
            Some(Some(constraint)) => Some(constraint.clone()),
            Some(None) => None,
            None => Some(type_name.to_string()),
        }
    }

    /// Emit a fluent-chain sentinel `MemberAccess` when this call is chained
    /// off a previous call, walking back to the root `ID.root_method()`.
    /// Encoded as `MemberAccess { object:
    /// "{FLUENT_CHAIN_SENTINEL}{root_id}:{root_method}:{chain_prefix}",
    /// member: this_method }`, where `chain_prefix` is a comma-separated list
    /// of intermediate chained method names (empty when this call is the
    /// first after the root). The analyze layer decodes the sentinel and
    /// validates each step against `is_instance_returning_static` (root) and
    /// `is_self_returning` (chain segments) before crediting. See issue #387.
    fn try_record_fluent_chain_access(&mut self, expr: &CallExpression<'_>) {
        let Expression::StaticMemberExpression(member) = &expr.callee else {
            return;
        };
        // Receiver must itself be a call expression for this to be a chain.
        // Direct `ID.method()` calls are handled by the existing
        // `static_member_object_name`-based flow.
        let Expression::CallExpression(_) = &member.object else {
            return;
        };
        let this_method = member.property.name.as_str();
        let mut chain_prefix_reversed: Vec<String> = Vec::new();
        let mut current = &member.object;
        loop {
            let Expression::CallExpression(call) = current else {
                return;
            };
            let Expression::StaticMemberExpression(inner_member) = &call.callee else {
                return;
            };
            if let Expression::Identifier(root_id) = &inner_member.object {
                chain_prefix_reversed.reverse();
                let chain_prefix = chain_prefix_reversed.join(",");
                self.member_accesses.push(MemberAccess {
                    object: format!(
                        "{}{}:{}:{}",
                        crate::FLUENT_CHAIN_SENTINEL,
                        root_id.name,
                        inner_member.property.name,
                        chain_prefix,
                    ),
                    member: this_method.to_string(),
                });
                return;
            }
            // Constructor root: `new Class(...).first().<...>.this_method()`.
            // The constructor always returns a `Class` instance, so there is no
            // root method to validate. The first instance method off the
            // constructor (`inner_member.property.name`) is itself a chain step
            // that must be `is_self_returning` to reach `this_method`, so it is
            // pushed into the chain prefix before encoding. See issue #605.
            if let Expression::NewExpression(new_expr) = &inner_member.object
                && let Expression::Identifier(class_id) = &new_expr.callee
            {
                chain_prefix_reversed.push(inner_member.property.name.to_string());
                chain_prefix_reversed.reverse();
                let chain_prefix = chain_prefix_reversed.join(",");
                self.member_accesses.push(MemberAccess {
                    object: format!(
                        "{}{}:{}",
                        crate::FLUENT_CHAIN_NEW_SENTINEL,
                        class_id.name,
                        chain_prefix,
                    ),
                    member: this_method.to_string(),
                });
                return;
            }
            chain_prefix_reversed.push(inner_member.property.name.to_string());
            current = &inner_member.object;
        }
    }

    /// Recognize `<receiver>.forEach(c => ...)` (and the optional-chained
    /// `<receiver>?.forEach(c => ...)`) where `<receiver>` was previously
    /// registered as an iterable with a known element type, and bind the
    /// arrow callback's first parameter to that element type. The binding is
    /// stored in `binding_target_names` so subsequent `c.method()` accesses
    /// flow through the existing bound-member-access resolution at end-of-visit.
    fn bind_iterable_callback_parameter(&mut self, expr: &CallExpression<'_>) {
        let (receiver_expr, method_name) = match &expr.callee {
            Expression::StaticMemberExpression(member) => (&member.object, &member.property.name),
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::StaticMemberExpression(member) => {
                    (&member.object, &member.property.name)
                }
                _ => return,
            },
            _ => return,
        };
        if method_name.as_str() != "forEach" {
            return;
        }
        let Some(receiver_name) = static_member_object_name(receiver_expr) else {
            return;
        };
        let Some(element_type) = self.iterable_element_types.get(&receiver_name).cloned() else {
            return;
        };
        let Some(first_arg) = expr.arguments.first() else {
            return;
        };
        let param_name = match first_arg {
            Argument::ArrowFunctionExpression(arrow) => {
                arrow.params.items.first().and_then(|p| match &p.pattern {
                    BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
                    _ => None,
                })
            }
            Argument::FunctionExpression(func) => {
                func.params.items.first().and_then(|p| match &p.pattern {
                    BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
                    _ => None,
                })
            }
            _ => None,
        };
        if let Some(name) = param_name {
            self.binding_target_names.insert(name, element_type);
        }
    }

    fn is_named_import_from(&self, local_name: &str, source: &str, imported_name: &str) -> bool {
        self.imports.iter().any(|import| {
            import.source == source
                && import.local_name == local_name
                && matches!(&import.imported_name, ImportedName::Named(name) if name == imported_name)
        })
    }

    /// Record `register('loader', ...)` from `node:module` as a dynamic import.
    /// The loader package is loaded by specifier rather than imported, so without
    /// this hook it would be reported as an unused dev dependency (issue #293).
    /// Recognizes both `import { register }` and `import * as Module from
    /// 'node:module'` forms.
    fn try_record_node_module_register(&mut self, expr: &CallExpression<'_>) {
        let register_match = match &expr.callee {
            Expression::Identifier(ident) => {
                self.is_node_module_register(ident.name.as_str(), false)
            }
            Expression::StaticMemberExpression(member) => {
                member.property.name == "register"
                    && matches!(&member.object, Expression::Identifier(obj)
                        if self.is_node_module_register(obj.name.as_str(), true))
            }
            _ => false,
        };
        if !register_match {
            return;
        }

        let sources = self.node_module_register_sources(expr);
        for source in sources.into_iter().filter(|source| !source.is_empty()) {
            let destructured_names = loader_hook_exports_for_source(&source);
            self.dynamic_imports.push(DynamicImportInfo {
                source,
                span: expr.span,
                destructured_names,
                local_name: None,
                is_speculative: false,
            });
        }
    }

    fn node_module_register_sources(&self, call: &CallExpression<'_>) -> Vec<String> {
        if let Some(source) = node_module_register_specifier(call) {
            return vec![source];
        }

        let Some(first_arg) = call.arguments.first() else {
            return Vec::new();
        };
        first_arg
            .as_expression()
            .map(|expr| self.node_module_register_sources_from_expression(expr))
            .unwrap_or_default()
    }

    fn node_module_register_sources_from_expression(&self, expr: &Expression<'_>) -> Vec<String> {
        match expr {
            Expression::Identifier(ident) => {
                self.node_module_register_url_binding(ident.name.as_str())
            }
            Expression::NewExpression(new_expr) => {
                new_url_import_source(new_expr).into_iter().collect()
            }
            Expression::ConditionalExpression(conditional) => {
                let mut sources =
                    self.node_module_register_sources_from_expression(&conditional.consequent);
                sources.extend(
                    self.node_module_register_sources_from_expression(&conditional.alternate),
                );
                sources.sort();
                sources.dedup();
                sources
            }
            Expression::ParenthesizedExpression(paren) => {
                self.node_module_register_sources_from_expression(&paren.expression)
            }
            Expression::TSAsExpression(ts_as) => {
                self.node_module_register_sources_from_expression(&ts_as.expression)
            }
            Expression::TSSatisfiesExpression(ts_sat) => {
                self.node_module_register_sources_from_expression(&ts_sat.expression)
            }
            _ => Vec::new(),
        }
    }

    fn record_child_process_require_binding(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        source: &str,
    ) {
        if !self.is_module_scope() {
            return;
        }

        match &declarator.id {
            BindingPattern::BindingIdentifier(id) if is_child_process_source(source) => {
                self.child_process_namespace_bindings
                    .insert(id.name.to_string());
            }
            BindingPattern::ObjectPattern(obj_pat) if is_child_process_source(source) => {
                for (local_name, source_name) in extract_object_pattern_bindings(obj_pat) {
                    if source_name == "fork" {
                        self.child_process_fork_bindings.insert(local_name);
                    }
                }
            }
            BindingPattern::BindingIdentifier(id) if is_node_path_source(source) => {
                self.node_path_namespace_bindings
                    .insert(id.name.to_string());
            }
            BindingPattern::ObjectPattern(obj_pat) if is_node_url_source(source) => {
                for (local_name, source_name) in extract_object_pattern_bindings(obj_pat) {
                    if source_name == "fileURLToPath" {
                        self.node_url_file_url_to_path_bindings.insert(local_name);
                    }
                }
            }
            _ => {}
        }
    }

    fn record_current_module_file_path_binding(&mut self, name: &str, expr: &Expression<'_>) {
        if !self.is_module_scope() {
            return;
        }
        let Expression::CallExpression(call) = expr else {
            return;
        };
        let Some(first_arg) = call.arguments.first() else {
            return;
        };
        if !is_meta_url_arg(first_arg) {
            return;
        }

        let is_file_url_to_path = match &call.callee {
            Expression::Identifier(ident) => self
                .node_url_file_url_to_path_bindings
                .contains(ident.name.as_str()),
            Expression::StaticMemberExpression(member) => {
                member.property.name == "fileURLToPath"
                    && matches!(&member.object, Expression::Identifier(obj)
                        if self.node_url_file_url_to_path_bindings.contains(obj.name.as_str()))
            }
            _ => false,
        };

        if is_file_url_to_path {
            self.current_module_file_path_bindings
                .insert(name.to_string());
        }
    }

    fn record_child_process_fork_target_binding(&mut self, name: &str, expr: &Expression<'_>) {
        if !self.is_module_scope() {
            return;
        }
        let sources = self.child_process_fork_sources_from_expression(expr);
        if !sources.is_empty() {
            self.child_process_fork_target_bindings
                .insert(name.to_string(), sources);
        }
    }

    fn child_process_fork_sources_from_expression(&self, expr: &Expression<'_>) -> Vec<String> {
        match expr {
            Expression::StringLiteral(lit) => local_fork_source(&lit.value)
                .into_iter()
                .collect::<Vec<_>>(),
            Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => tpl
                .quasis
                .first()
                .and_then(|quasi| local_fork_source(&quasi.value.raw))
                .into_iter()
                .collect(),
            Expression::Identifier(ident) => self
                .child_process_fork_target_bindings
                .get(ident.name.as_str())
                .filter(|_| !self.nested_scope_shadows(ident.name.as_str()))
                .cloned()
                .unwrap_or_default(),
            Expression::NewExpression(new_expr) => new_url_import_source(new_expr)
                .and_then(|source| local_fork_source(&source))
                .into_iter()
                .collect(),
            Expression::CallExpression(call) => self.child_process_fork_sources_from_call(call),
            Expression::ParenthesizedExpression(paren) => {
                self.child_process_fork_sources_from_expression(&paren.expression)
            }
            Expression::TSAsExpression(ts_as) => {
                self.child_process_fork_sources_from_expression(&ts_as.expression)
            }
            Expression::TSSatisfiesExpression(ts_sat) => {
                self.child_process_fork_sources_from_expression(&ts_sat.expression)
            }
            _ => Vec::new(),
        }
    }

    fn child_process_fork_sources_from_call(&self, call: &CallExpression<'_>) -> Vec<String> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Vec::new();
        };
        if member.property.name != "resolve" {
            return Vec::new();
        }
        let Expression::Identifier(object) = &member.object else {
            return Vec::new();
        };
        if !self
            .node_path_namespace_bindings
            .contains(object.name.as_str())
        {
            return Vec::new();
        }
        let Some(Argument::Identifier(base)) = call.arguments.first() else {
            return Vec::new();
        };
        if !self
            .current_module_file_path_bindings
            .contains(base.name.as_str())
        {
            return Vec::new();
        }
        let Some(Argument::StringLiteral(relative)) = call.arguments.get(1) else {
            return Vec::new();
        };
        normalize_module_file_relative_path(&relative.value)
            .and_then(|source| local_fork_source(&source))
            .into_iter()
            .collect()
    }

    fn try_record_child_process_fork(&mut self, expr: &CallExpression<'_>) {
        if !self.is_module_or_function_runtime_scope() {
            return;
        }

        let is_fork_call = match &expr.callee {
            Expression::Identifier(ident) => {
                self.child_process_fork_bindings
                    .contains(ident.name.as_str())
                    && !self.nested_scope_shadows(ident.name.as_str())
            }
            Expression::StaticMemberExpression(member) => {
                member.property.name == "fork"
                    && matches!(&member.object, Expression::Identifier(obj)
                        if self.child_process_namespace_bindings.contains(obj.name.as_str())
                            && !self.nested_scope_shadows(obj.name.as_str()))
            }
            _ => false,
        };
        if !is_fork_call {
            return;
        }

        let Some(first_arg) = expr.arguments.first().and_then(Argument::as_expression) else {
            return;
        };
        for source in self.child_process_fork_sources_from_expression(first_arg) {
            self.dynamic_imports.push(DynamicImportInfo {
                source,
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
                is_speculative: false,
            });
        }
    }

    fn extract_angular_inject_target(&self, call: &CallExpression<'_>) -> Option<String> {
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        if !self.is_named_import_from(callee.name.as_str(), "@angular/core", "inject") {
            return None;
        }

        if let Some(type_arguments) = call.type_arguments.as_deref()
            && let Some(TSType::TSTypeReference(type_ref)) = type_arguments.params.first()
            && let Some((type_name, _)) = type_name_root(&type_ref.type_name)
        {
            return Some(type_name);
        }

        let Some(Argument::Identifier(target)) = call.arguments.first() else {
            return None;
        };
        Some(target.name.to_string())
    }

    fn copy_nested_binding_targets(&mut self, source_binding: &str, target_binding: &str) -> bool {
        let source_prefix = format!("{source_binding}.");
        let target_prefix = format!("{target_binding}.");
        let copied: Vec<(String, String)> = self
            .binding_target_names
            .iter()
            .filter_map(|(binding, target)| {
                binding
                    .strip_prefix(&source_prefix)
                    .map(|suffix| (format!("{target_prefix}{suffix}"), target.clone()))
            })
            .collect();

        let mut changed = false;
        for (binding, target) in copied {
            changed |= self.insert_binding_target(binding, target);
        }
        changed
    }

    fn insert_binding_target(&mut self, binding: String, target: String) -> bool {
        if self.binding_target_names.get(&binding) == Some(&target) {
            return false;
        }
        self.binding_target_names.insert(binding, target);
        true
    }

    pub(super) fn resolve_object_binding_candidate(
        &mut self,
        candidate: &super::ObjectBindingCandidate,
    ) -> bool {
        let mut changed = false;
        if self
            .namespace_binding_names
            .iter()
            .any(|name| name == candidate.source_name.as_str())
        {
            changed |= self.insert_binding_target(
                candidate.binding_path.clone(),
                candidate.source_name.clone(),
            );
        } else if let Some(target_name) = self
            .binding_target_names
            .get(candidate.source_name.as_str())
            .cloned()
        {
            changed |= self.insert_binding_target(candidate.binding_path.clone(), target_name);
        }
        changed | self.copy_nested_binding_targets(&candidate.source_name, &candidate.binding_path)
    }

    fn record_object_binding_targets(&mut self, binding_name: &str, obj: &ObjectExpression<'_>) {
        self.record_object_binding_targets_at_path(binding_name, obj);
    }

    fn record_object_binding_targets_at_path(
        &mut self,
        object_path: &str,
        obj: &ObjectExpression<'_>,
    ) {
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                continue;
            };
            let Some(key_name) = prop.key.static_name() else {
                continue;
            };

            let binding_path = format!("{object_path}.{key_name}");
            match &prop.value {
                Expression::Identifier(ident) => {
                    self.object_binding_candidates
                        .push(super::ObjectBindingCandidate {
                            binding_path,
                            source_name: ident.name.to_string(),
                        });
                }
                Expression::ObjectExpression(child) => {
                    self.record_object_binding_targets_at_path(&binding_path, child);
                }
                _ => {}
            }
        }
    }

    fn collect_playwright_fixture_type_bindings(&self, ty: &TSType<'_>) -> Vec<(String, String)> {
        let mut bindings = Vec::new();
        collect_fixture_type_bindings_from_type(
            ty,
            "",
            &self.playwright_fixture_types,
            &mut bindings,
        );
        bindings.sort_unstable();
        bindings.dedup();
        bindings
    }

    fn record_playwright_fixture_type_alias(&mut self, alias: &TSTypeAliasDeclaration<'_>) {
        let bindings = self.collect_playwright_fixture_type_bindings(&alias.type_annotation);
        if !bindings.is_empty() {
            self.playwright_fixture_types
                .insert(alias.id.name.to_string(), bindings);
        }
    }

    fn record_playwright_fixture_definitions(
        &mut self,
        test_name: &str,
        call: &CallExpression<'_>,
    ) {
        let Some(base_name) = playwright_extend_base_name(call) else {
            return;
        };
        if !self.is_named_import_from(base_name.as_str(), "@playwright/test", "test") {
            return;
        }
        let Some(type_arguments) = call.type_arguments.as_deref() else {
            return;
        };
        let mut bindings = Vec::new();
        for type_arg in &type_arguments.params {
            bindings.extend(self.collect_playwright_fixture_type_bindings(type_arg));
        }
        bindings.sort_unstable();
        bindings.dedup();
        self.member_accesses
            .extend(
                bindings
                    .into_iter()
                    .map(|(fixture_name, type_name)| MemberAccess {
                        object: format!(
                            "{}{}:{}",
                            crate::PLAYWRIGHT_FIXTURE_DEF_SENTINEL,
                            test_name,
                            fixture_name
                        ),
                        member: type_name,
                    }),
            );
    }

    /// Capture a helper-function Playwright fixture or alias from the call
    /// returned by the helper's final statement.
    ///
    /// Distinguishes two shapes:
    /// 1. `return base.extend<T>(...)`: collect type bindings now (mirrors
    ///    `record_playwright_fixture_definitions` but defers the import gate
    ///    to finalize so source order is irrelevant).
    /// 2. `return otherHelper()`: record a `(test_name, otherHelper)` alias
    ///    that finalize's fixed-point pass resolves into bindings when
    ///    `otherHelper` itself is, transitively, a Playwright fixture
    ///    factory in the same file.
    ///
    /// Non-matching shapes (member-call return, no callee identifier) are
    /// dropped silently: false matches would just produce inert def
    /// sentinels with no use-side correlate. See issues #491 and #586.
    pub(super) fn try_capture_playwright_factory_helper(
        &mut self,
        test_name: &str,
        call: &CallExpression<'_>,
    ) {
        if let Some(base_name) = playwright_extend_base_name(call) {
            let Some(type_arguments) = call.type_arguments.as_deref() else {
                return;
            };
            let mut bindings = Vec::new();
            for type_arg in &type_arguments.params {
                bindings.extend(self.collect_playwright_fixture_type_bindings(type_arg));
            }
            bindings.sort_unstable();
            bindings.dedup();
            if bindings.is_empty() {
                return;
            }
            self.pending_playwright_factory_calls
                .push(super::PendingPlaywrightFactory {
                    test_name: test_name.to_string(),
                    base_name,
                    type_bindings: bindings,
                });
        } else if let Expression::Identifier(ident) = &call.callee {
            self.pending_playwright_factory_aliases
                .push((test_name.to_string(), ident.name.to_string()));
        }
    }
}

impl<'a> Visit<'a> for ModuleInfoExtractor {
    fn visit_formal_parameter(&mut self, param: &FormalParameter<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &param.pattern
            && let Some(type_annotation) = param.type_annotation.as_deref()
        {
            self.record_typed_binding(id.name.as_str(), type_annotation);
            if param.accessibility.is_some() {
                self.record_typed_binding(format!("this.{}", id.name).as_str(), type_annotation);
            }
        }

        walk::walk_formal_parameter(self, param);
    }

    fn visit_property_definition(&mut self, prop: &PropertyDefinition<'a>) {
        if let Some(name) = prop.key.static_name() {
            if let Some(type_annotation) = prop.type_annotation.as_deref() {
                self.record_typed_binding(format!("this.{name}").as_str(), type_annotation);

                // `@ViewChildren ... readonly dvcs?: QueryList<ChildComponent>`:
                // peel the element type out of the `QueryList<T>` annotation so
                // `this.dvcs?.forEach(c => c.method())` can resolve `c` to `T`.
                if has_angular_plural_query_decorator(&prop.decorators)
                    && let Some(element_type) = extract_query_list_element_type(type_annotation)
                {
                    self.iterable_element_types
                        .insert(format!("this.{name}"), element_type);
                }
            }

            if let Some(Expression::NewExpression(new_expr)) = &prop.value
                && let Expression::Identifier(callee) = &new_expr.callee
                && !super::helpers::is_builtin_constructor(callee.name.as_str())
            {
                self.binding_target_names
                    .insert(format!("this.{name}"), callee.name.to_string());
            }

            if let Some(Expression::CallExpression(call)) = &prop.value
                && let Some(type_name) = self.extract_angular_inject_target(call)
            {
                self.binding_target_names
                    .insert(format!("this.{name}"), type_name);
            }

            // Angular signal queries: `readonly vc = viewChild<T>(...)` etc.
            // Singular factories produce `Signal<T>`, called as `this.vc()`,
            // so the synthetic key `this.<name>()` is bound to `T`. Plural
            // factories produce `Signal<readonly T[]>`, iterated via
            // `this.vcs().forEach(c => c.method())`, so the same call-form
            // key is recorded as an iterable whose element type is `T`.
            if let Some(value) = prop.value.as_ref()
                && let Some(query) = extract_angular_signal_query(value)
            {
                let call_key = format!("this.{name}()");
                if query.plural {
                    self.iterable_element_types.insert(call_key, query.type_arg);
                } else {
                    self.binding_target_names.insert(call_key, query.type_arg);
                }
            }
        }

        walk::walk_property_definition(self, prop);
    }

    fn visit_block_statement(&mut self, stmt: &BlockStatement<'a>) {
        self.block_depth += 1;
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.push(FxHashSet::default());
        }
        walk::walk_block_statement(self, stmt);
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.pop();
        }
        self.block_depth -= 1;
    }

    fn visit_declaration(&mut self, decl: &Declaration<'a>) {
        if self.block_depth == 0 && self.function_depth == 0 && self.namespace_depth == 0 {
            match decl {
                Declaration::VariableDeclaration(var) => {
                    for declarator in &var.declarations {
                        for id in declarator.id.get_binding_identifiers() {
                            self.record_local_declaration_name(&id.name);
                        }
                    }
                }
                Declaration::ClassDeclaration(class) => {
                    if let Some(id) = class.id.as_ref() {
                        self.record_local_declaration_name(&id.name);
                        self.record_local_type_declaration(&id.name, id.span);
                        let is_angular = has_angular_class_decorator(class);
                        let instance_bindings =
                            super::helpers::extract_class_instance_bindings(class);
                        self.record_local_class_export(
                            id.name.to_string(),
                            extract_class_members(class, is_angular),
                            extract_super_class_name(class),
                            extract_implemented_interface_names(class),
                            instance_bindings,
                        );
                        let refs = Self::collect_class_signature_refs(class);
                        self.record_local_signature_refs(&id.name, refs);
                    }
                }
                Declaration::FunctionDeclaration(function) => {
                    if let Some(id) = function.id.as_ref() {
                        self.record_local_declaration_name(&id.name);
                        let refs = Self::collect_function_signature_refs(function);
                        self.record_local_signature_refs(&id.name, refs);

                        // `function appTest() { const state = ...; return base.extend<T>(...); }`
                        // or `function appTest() { return setupTestFixture(); }`
                        // is a helper-function Playwright fixture consumed via
                        // the curried `appTest()(...)` form. See issues #491/#586.
                        if let Some(body) = function.body.as_deref()
                            && let Some(call) = extract_function_body_final_return_call(body)
                        {
                            self.try_capture_playwright_factory_helper(id.name.as_str(), call);
                        }
                    }
                }
                Declaration::TSTypeAliasDeclaration(alias) => {
                    self.record_local_declaration_name(&alias.id.name);
                    self.record_local_type_declaration(&alias.id.name, alias.id.span);
                    self.record_playwright_fixture_type_alias(alias);
                    let refs = Self::collect_type_alias_signature_refs(alias);
                    self.record_local_signature_refs(&alias.id.name, refs);
                }
                Declaration::TSInterfaceDeclaration(iface) => {
                    self.record_local_declaration_name(&iface.id.name);
                    self.record_local_type_declaration(&iface.id.name, iface.id.span);
                    let refs = Self::collect_interface_signature_refs(iface);
                    self.record_local_signature_refs(&iface.id.name, refs);
                }
                Declaration::TSEnumDeclaration(enumd) => {
                    self.record_local_declaration_name(&enumd.id.name);
                    self.record_local_type_declaration(&enumd.id.name, enumd.id.span);
                }
                Declaration::TSModuleDeclaration(module) => {
                    if let TSModuleDeclarationName::Identifier(id) = &module.id {
                        self.record_local_declaration_name(&id.name);
                        self.record_local_type_declaration(&id.name, id.span);
                    }
                }
                _ => {}
            }
        } else if self.namespace_depth == 0 {
            match decl {
                Declaration::VariableDeclaration(var) => {
                    for declarator in &var.declarations {
                        self.record_nested_declaration_names(
                            declarator.id.get_binding_identifiers(),
                        );
                    }
                }
                Declaration::ClassDeclaration(class) => {
                    if let Some(id) = class.id.as_ref() {
                        self.record_nested_declaration_names(std::iter::once(id));
                    }
                }
                Declaration::FunctionDeclaration(function) => {
                    if let Some(id) = function.id.as_ref() {
                        self.record_nested_declaration_names(std::iter::once(id));
                    }
                }
                _ => {}
            }
        }

        walk::walk_declaration(self, decl);
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        self.push_function_declaration_scope(&func.params);
        self.function_depth += 1;
        walk::walk_function(self, func, flags);
        self.function_depth -= 1;
        self.pop_function_declaration_scope();
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        self.push_function_declaration_scope(&expr.params);
        self.function_depth += 1;
        walk::walk_arrow_function_expression(self, expr);
        self.function_depth -= 1;
        self.pop_function_declaration_scope();
    }

    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        let source = decl.source.value.to_string();
        let is_type_only = decl.import_kind.is_type();

        let source_span = decl.source.span;

        if let Some(specifiers) = &decl.specifiers {
            for spec in specifiers {
                match spec {
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        if self.is_module_scope()
                            && is_child_process_source(&source)
                            && s.imported.name() == "fork"
                        {
                            self.child_process_fork_bindings
                                .insert(s.local.name.to_string());
                        }
                        if self.is_module_scope()
                            && is_node_url_source(&source)
                            && s.imported.name() == "fileURLToPath"
                        {
                            self.node_url_file_url_to_path_bindings
                                .insert(s.local.name.to_string());
                        }
                        self.imports.push(ImportInfo {
                            source: source.clone(),
                            imported_name: ImportedName::Named(s.imported.name().to_string()),
                            local_name: s.local.name.to_string(),
                            is_type_only: is_type_only || s.import_kind.is_type(),
                            from_style: false,
                            span: s.span,
                            source_span,
                        });
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                        if self.is_module_scope() && is_node_path_source(&source) {
                            self.node_path_namespace_bindings
                                .insert(s.local.name.to_string());
                        }
                        self.imports.push(ImportInfo {
                            source: source.clone(),
                            imported_name: ImportedName::Default,
                            local_name: s.local.name.to_string(),
                            is_type_only,
                            from_style: false,
                            span: s.span,
                            source_span,
                        });
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        let local = s.local.name.to_string();
                        if self.is_module_scope() && is_child_process_source(&source) {
                            self.child_process_namespace_bindings.insert(local.clone());
                        }
                        if self.is_module_scope() && is_node_path_source(&source) {
                            self.node_path_namespace_bindings.insert(local.clone());
                        }
                        if self.is_module_scope() && is_node_url_source(&source) {
                            self.node_url_file_url_to_path_bindings
                                .insert(local.clone());
                        }
                        self.namespace_binding_names.push(local.clone());
                        self.imports.push(ImportInfo {
                            source: source.clone(),
                            imported_name: ImportedName::Namespace,
                            local_name: local,
                            is_type_only,
                            from_style: false,
                            span: s.span,
                            source_span,
                        });
                    }
                }
            }
        } else {
            // Side-effect import: import './styles.css'
            self.imports.push(ImportInfo {
                source,
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: decl.span,
                source_span,
            });
        }
    }

    fn visit_export_named_declaration(&mut self, decl: &ExportNamedDeclaration<'a>) {
        let is_namespace = matches!(&decl.declaration, Some(Declaration::TSModuleDeclaration(_)));

        // Inside a namespace body: collect as member, not top-level export
        if self.namespace_depth > 0 {
            if let Some(declaration) = &decl.declaration {
                self.extract_namespace_members(declaration);
            }
            if is_namespace {
                self.namespace_depth += 1;
            }
            walk::walk_export_named_declaration(self, decl);
            if is_namespace {
                self.namespace_depth -= 1;
            }
            return;
        }

        let is_type_only = decl.export_kind.is_type();

        if let Some(source) = &decl.source {
            // Re-export: export { foo } from './bar'
            for spec in &decl.specifiers {
                self.re_exports.push(ReExportInfo {
                    source: source.value.to_string(),
                    imported_name: spec.local.name().to_string(),
                    exported_name: spec.exported.name().to_string(),
                    is_type_only: is_type_only || spec.export_kind.is_type(),
                    span: spec.span,
                });
            }
        } else {
            // Local export
            if let Some(declaration) = &decl.declaration {
                self.extract_declaration_exports(declaration, is_type_only);
            }
            for spec in &decl.specifiers {
                let local_name_str = spec.local.name().as_str();
                let spec_type_only = is_type_only || spec.export_kind.is_type();

                self.pending_local_export_specifiers
                    .push(PendingLocalExportSpecifier {
                        local_name: local_name_str.to_string(),
                        exported_name: spec.exported.name().to_string(),
                        is_type_only: spec_type_only,
                        span: spec.span,
                    });
            }
        }

        // For namespace declarations: walk the body while tracking depth,
        // then attach collected members to the namespace export.
        if is_namespace {
            self.namespace_depth += 1;
            self.pending_namespace_members.clear();
        }
        walk::walk_export_named_declaration(self, decl);
        if is_namespace {
            self.namespace_depth -= 1;
            if let Some(ns_export) = self.exports.last_mut() {
                ns_export.members = std::mem::take(&mut self.pending_namespace_members);
            }
        }
    }

    fn visit_export_default_declaration(&mut self, decl: &ExportDefaultDeclaration<'a>) {
        // Extract members and super_class for default-exported classes
        let (members, super_class, implemented_interfaces, instance_bindings) =
            if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &decl.declaration {
                let is_angular = has_angular_class_decorator(class);
                let bindings = super::helpers::extract_class_instance_bindings(class);
                (
                    extract_class_members(class, is_angular),
                    extract_super_class_name(class),
                    extract_implemented_interface_names(class),
                    bindings,
                )
            } else {
                (vec![], None, vec![], vec![])
            };
        let local_name = match &decl.declaration {
            ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                class.id.as_ref().map(|id| id.name.to_string())
            }
            ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                function.id.as_ref().map(|id| id.name.to_string())
            }
            _ => None,
        };

        match &decl.declaration {
            ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                let refs = Self::collect_class_signature_refs(class);
                if let Some(id) = class.id.as_ref() {
                    self.record_local_type_declaration(&id.name, id.span);
                    self.record_local_signature_refs(&id.name, refs);
                } else {
                    self.record_public_signature_refs("default", refs);
                }
            }
            ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                let refs = Self::collect_function_signature_refs(function);
                if let Some(id) = function.id.as_ref() {
                    self.record_local_signature_refs(&id.name, refs);
                } else {
                    self.record_public_signature_refs("default", refs);
                }
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(iface) => {
                self.record_local_type_declaration(&iface.id.name, iface.id.span);
                let refs = Self::collect_interface_signature_refs(iface);
                self.record_public_signature_refs("default", refs);
            }
            _ => {}
        }

        if super_class.is_some()
            || !implemented_interfaces.is_empty()
            || !instance_bindings.is_empty()
        {
            self.class_heritage.push(ClassHeritageInfo {
                export_name: "default".to_string(),
                super_class: super_class.clone(),
                implements: implemented_interfaces,
                instance_bindings,
            });
        }

        self.exports.push(ExportInfo {
            name: ExportName::Default,
            local_name,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: decl.span,
            members,
            super_class,
        });

        walk::walk_export_default_declaration(self, decl);
    }

    fn visit_export_all_declaration(&mut self, decl: &ExportAllDeclaration<'a>) {
        let exported_name = decl
            .exported
            .as_ref()
            .map_or_else(|| "*".to_string(), |e| e.name().to_string());

        self.re_exports.push(ReExportInfo {
            source: decl.source.value.to_string(),
            imported_name: "*".to_string(),
            exported_name,
            is_type_only: decl.export_kind.is_type(),
            span: decl.span,
        });

        walk::walk_export_all_declaration(self, decl);
    }

    fn visit_import_expression(&mut self, expr: &ImportExpression<'a>) {
        // Skip imports already handled via visit_variable_declaration (with local_name capture)
        if self.handled_import_spans.contains(&expr.span) {
            walk::walk_import_expression(self, expr);
            return;
        }

        match &expr.source {
            Expression::StringLiteral(lit) => {
                self.dynamic_imports.push(DynamicImportInfo {
                    source: lit.value.to_string(),
                    span: expr.span,
                    destructured_names: Vec::new(),
                    local_name: None,
                    is_speculative: false,
                });
            }
            Expression::TemplateLiteral(tpl)
                if !tpl.quasis.is_empty() && !tpl.expressions.is_empty() =>
            {
                // Template literal with expressions: extract prefix/suffix.
                // For multi-expression templates like `./a/${x}/${y}.js` (3 quasis),
                // use `**/` in the prefix so the glob can match nested directories.
                let first_quasi = tpl.quasis[0].value.raw.to_string();
                if first_quasi.starts_with("./") || first_quasi.starts_with("../") {
                    let prefix = if tpl.expressions.len() > 1 {
                        // Multiple dynamic segments: use ** to match any nesting depth
                        format!("{first_quasi}**/")
                    } else {
                        first_quasi
                    };
                    let suffix = if tpl.quasis.len() > 1 {
                        let last = &tpl.quasis[tpl.quasis.len() - 1];
                        let s = last.value.raw.to_string();
                        if s.is_empty() { None } else { Some(s) }
                    } else {
                        None
                    };
                    self.dynamic_import_patterns.push(DynamicImportPattern {
                        prefix,
                        suffix,
                        span: expr.span,
                    });
                }
            }
            Expression::TemplateLiteral(tpl)
                if !tpl.quasis.is_empty() && tpl.expressions.is_empty() =>
            {
                // No-substitution template literal: treat as exact string
                let value = tpl.quasis[0].value.raw.to_string();
                if !value.is_empty() {
                    self.dynamic_imports.push(DynamicImportInfo {
                        source: value,
                        span: expr.span,
                        destructured_names: Vec::new(),
                        local_name: None,
                        is_speculative: false,
                    });
                }
            }
            Expression::BinaryExpression(bin)
                if bin.operator == oxc_ast::ast::BinaryOperator::Addition =>
            {
                if let Some((prefix, suffix)) = extract_concat_parts(bin)
                    && (prefix.starts_with("./") || prefix.starts_with("../"))
                {
                    self.dynamic_import_patterns.push(DynamicImportPattern {
                        prefix,
                        suffix,
                        span: expr.span,
                    });
                }
            }
            _ => {}
        }

        walk::walk_import_expression(self, expr);
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        for declarator in &decl.declarations {
            if self.is_module_scope() {
                let refs = Self::collect_variable_signature_refs(declarator);
                for id in declarator.id.get_binding_identifiers() {
                    self.record_local_signature_refs(&id.name, refs.clone());
                }
            }

            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && let Some(type_annotation) = declarator.type_annotation.as_deref()
            {
                self.record_typed_binding(id.name.as_str(), type_annotation);
            }

            let Some(init) = &declarator.init else {
                continue;
            };

            if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                let sources = self.node_module_register_sources_from_expression(init);
                if !sources.is_empty() {
                    self.record_node_module_register_url_binding(id.name.to_string(), sources);
                }
                self.record_current_module_file_path_binding(id.name.as_str(), init);
                self.record_child_process_fork_target_binding(id.name.as_str(), init);
            }

            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && let Expression::CallExpression(call) = init
            {
                self.record_playwright_fixture_definitions(id.name.as_str(), call);
            }

            // `const appTest = () => base.extend<T>(...)` or
            // `const appTest = () => { const state = ...; return base.extend<T>(...); }` or
            // `const appTest = function () { return base.extend<T>(...); }`
            // are helper-function Playwright fixtures consumed via the curried
            // `appTest()(...)` form. See issues #491/#586.
            if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                let helper_call = match init {
                    Expression::ArrowFunctionExpression(arrow) => extract_arrow_return_call(arrow),
                    Expression::FunctionExpression(func) => func
                        .body
                        .as_deref()
                        .and_then(extract_function_body_final_return_call),
                    _ => None,
                };
                if let Some(call) = helper_call {
                    self.try_capture_playwright_factory_helper(id.name.as_str(), call);
                }
            }

            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && let Expression::ObjectExpression(obj) = init
            {
                self.record_object_binding_targets(id.name.as_str(), obj);
            }

            // `const x = require('./y')` — static require
            if let Some((call, source)) = try_extract_require(init) {
                self.record_child_process_require_binding(declarator, source);
                self.handle_require_declaration(declarator, call, source);
                continue;
            }

            // `const x = new ClassName(...)` — instance creation for member tracking.
            // Scope-unaware: shadowing causes false negatives, not false positives.
            // Built-in constructors are skipped to avoid spurious mappings.
            if let Expression::NewExpression(new_expr) = init
                && let Expression::Identifier(callee) = &new_expr.callee
                && let BindingPattern::BindingIdentifier(id) = &declarator.id
                && !super::helpers::is_builtin_constructor(callee.name.as_str())
            {
                self.binding_target_names
                    .insert(id.name.to_string(), callee.name.to_string());
                // No `continue` — falls through to dynamic import detection (which
                // won't match NewExpression) and then the loop continues.
            }

            // `const [x] = wrapper(() => new ClassName(...))` — instance creation
            // through a wrapper function with a factory initializer (e.g., React's
            // `useState`, `useMemo`). The first array-destructured element is bound
            // to the class returned by the factory.
            if let Expression::CallExpression(call) = init
                && let BindingPattern::ArrayPattern(arr_pat) = &declarator.id
                && let Some(Some(BindingPattern::BindingIdentifier(id))) = arr_pat.elements.first()
                && let Some(class_name) =
                    super::helpers::try_extract_factory_new_class(&call.arguments)
            {
                self.binding_target_names
                    .insert(id.name.to_string(), class_name);
            }

            // `const x = ID.METHOD(...)`: static-factory call candidate.
            // We cannot decide here whether `ID` resolves to a class whose
            // `METHOD` is an instance-returning static factory because the
            // class declaration may appear later in the file and the import
            // statements may also be unresolved. Record a candidate; the
            // finalize step (`resolve_factory_call_candidates`) checks each
            // candidate against local classes and imports and inserts the
            // appropriate `binding_target_names` entry (direct class name
            // for same-file matches, sentinel-encoded for cross-file). See
            // issue #346.
            if let Expression::CallExpression(call) = init
                && let BindingPattern::BindingIdentifier(id) = &declarator.id
                && let Expression::StaticMemberExpression(member) = &call.callee
                && let Expression::Identifier(callee_object) = &member.object
            {
                self.factory_call_candidates
                    .push(super::FactoryCallCandidate {
                        local_name: id.name.to_string(),
                        callee_object: callee_object.name.to_string(),
                        callee_method: member.property.name.to_string(),
                    });
            }

            // `const { a, b } = ns` — namespace destructuring for member narrowing.
            // Scope-unaware: consistent with flat member_accesses approach.
            if let Expression::Identifier(ident) = init
                && self
                    .namespace_binding_names
                    .iter()
                    .any(|n| n == ident.name.as_str())
            {
                self.handle_namespace_destructuring(declarator, &ident.name);
                continue;
            }

            // `const x = await import('./y')` or `const x = import('./y')`
            let Some((import_expr, source)) = try_extract_dynamic_import(init) else {
                continue;
            };
            self.handle_dynamic_import_declaration(declarator, import_expr, source);
        }
        walk::walk_variable_declaration(self, decl);
    }

    fn visit_object_property(&mut self, prop: &ObjectProperty<'a>) {
        if let Some((import_expr, source)) = try_extract_property_callback_import(prop) {
            self.dynamic_imports.push(DynamicImportInfo {
                source: source.to_string(),
                span: import_expr.span,
                destructured_names: vec!["default".to_string()],
                local_name: None,
                is_speculative: false,
            });
            self.handled_import_spans.insert(import_expr.span);
        }

        walk::walk_object_property(self, prop);
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        if let Some(test_name) = playwright_test_callee_name(&expr.callee) {
            self.member_accesses
                .extend(collect_playwright_fixture_member_uses(
                    test_name.as_str(),
                    &expr.arguments,
                ));
        }

        // Detect `customElements.define('tag', ClassRef)` Web Component
        // registration. The class identifier IS referenced syntactically (so
        // oxc_semantic counts the in-file ref) but no other file imports the
        // class by name, so the cross-file references list stays empty. Mark
        // the class export as side-effect-used so unused-export ignores it.
        if let Some((_tag, class_name)) = extract_custom_elements_define(expr) {
            self.side_effect_registered_class_names.insert(class_name);
        }

        // Angular plural-query iteration: `this.vcs().forEach(c => c.m())`,
        // `this.dvcs?.forEach(c => c.m())`. When the receiver was registered
        // as an iterable with a known element type, bind the arrow callback's
        // first parameter to that type so the inner `c.m()` member access
        // resolves through `binding_target_names`.
        self.bind_iterable_callback_parameter(expr);

        if let Some(target_source) = vitest_mock_source(expr) {
            // Always credit the vi.mock target itself as a referenced module.
            // Whether vitest auto-mocks (no factory) or runs a factory in place
            // of the original module, the target's path must resolve at test
            // time, so the file is conceptually used. Without this, the target
            // surfaces as `unused-file` whenever the factory replaces every
            // export and no other test file imports it directly. See issue #311.
            self.dynamic_imports.push(DynamicImportInfo {
                source: target_source.clone(),
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
                is_speculative: false,
            });

            // Synthesize the `__mocks__/<file>` sibling only when vitest will
            // actually consult it: vi.mock without a factory falls through to
            // the auto-mock convention; vi.mock WITH a factory uses the
            // factory directly and the `__mocks__/<file>` sibling is ignored.
            // Synthesizing in the factory case produces a spurious
            // `unresolved-import` finding when no `__mocks__/<file>` exists.
            // See issue #311.
            //
            // Marked `is_speculative: true` so the resolver silently drops the
            // entry when no `__mocks__/<file>` exists on disk. Vitest's
            // auto-mock system works in-memory and does not require a
            // `__mocks__/` directory the way Jest does, so the synthesised
            // path is a credit hint, not a contract the user must satisfy.
            // Without the speculative drop, projects that rely on Vitest's
            // in-memory auto-mocking surface a spurious `unresolved-import`
            // finding pointing at a path they never wrote. See issue #378.
            if !vi_mock_has_factory(expr)
                && let Some(mock_source) = vitest_auto_mock_source(&target_source)
            {
                self.dynamic_imports.push(DynamicImportInfo {
                    source: mock_source,
                    span: expr.span,
                    destructured_names: Vec::new(),
                    local_name: Some(String::new()),
                    is_speculative: true,
                });
            }
        }

        self.try_record_node_module_register(expr);
        self.try_record_child_process_fork(expr);

        // Detect require()
        if let Expression::Identifier(ident) = &expr.callee
            && ident.name == "require"
            && let Some(Argument::StringLiteral(lit)) = expr.arguments.first()
            && !self.handled_require_spans.contains(&expr.span)
        {
            self.require_calls.push(RequireCallInfo {
                source: lit.value.to_string(),
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
            });
        }

        // Detect Object.values(X), Object.keys(X), Object.entries(X) — whole-object use
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && let Expression::Identifier(obj) = &member.object
            && obj.name == "Object"
            && matches!(
                member.property.name.as_str(),
                "values" | "keys" | "entries" | "getOwnPropertyNames"
            )
            && let Some(arg_name) = expr.arguments.first().and_then(static_argument_object_name)
        {
            self.whole_object_uses.push(arg_name);
        }

        // Detect import.meta.glob() — Vite pattern
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && member.property.name == "glob"
            && matches!(member.object, Expression::MetaProperty(_))
            && let Some(first_arg) = expr.arguments.first()
        {
            match first_arg {
                Argument::StringLiteral(lit) => {
                    let s = lit.value.to_string();
                    if s.starts_with("./") || s.starts_with("../") {
                        self.dynamic_import_patterns.push(DynamicImportPattern {
                            prefix: s,
                            suffix: None,
                            span: expr.span,
                        });
                    }
                }
                Argument::ArrayExpression(arr) => {
                    for elem in &arr.elements {
                        if let ArrayExpressionElement::StringLiteral(lit) = elem {
                            let s = lit.value.to_string();
                            if s.starts_with("./") || s.starts_with("../") {
                                self.dynamic_import_patterns.push(DynamicImportPattern {
                                    prefix: s,
                                    suffix: None,
                                    span: expr.span,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Detect require.context() — Webpack pattern
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && member.property.name == "context"
            && let Expression::Identifier(obj) = &member.object
            && obj.name == "require"
            && let Some(Argument::StringLiteral(dir_lit)) = expr.arguments.first()
        {
            let dir = dir_lit.value.to_string();
            if dir.starts_with("./") || dir.starts_with("../") {
                let recursive = expr
                    .arguments
                    .get(1)
                    .is_some_and(|arg| matches!(arg, Argument::BooleanLiteral(b) if b.value));
                let prefix = if recursive {
                    format!("{dir}/**/")
                } else {
                    format!("{dir}/")
                };
                // Parse the optional third argument (regex filter) and convert
                // simple extension patterns (e.g., /\.vue$/) to a glob suffix.
                let suffix = expr.arguments.get(2).and_then(|arg| match arg {
                    Argument::RegExpLiteral(re) => regex_pattern_to_suffix(&re.regex.pattern.text),
                    _ => None,
                });
                self.dynamic_import_patterns.push(DynamicImportPattern {
                    prefix,
                    suffix,
                    span: expr.span,
                });
            }
        }

        // Detect `import('./lib').then(m => m.foo)` — dynamic import with `.then()` callback.
        // The callback parameter binds to the module namespace, and member accesses or
        // destructured parameters indicate which exports are consumed.
        if let Some(then_cb) = try_extract_import_then_callback(expr) {
            if let Some(local) = &then_cb.local_name {
                self.namespace_binding_names.push(local.clone());
            }
            self.handled_import_spans.insert(then_cb.import_span);
            self.dynamic_imports.push(DynamicImportInfo {
                source: then_cb.source,
                span: then_cb.import_span,
                destructured_names: then_cb.destructured_names,
                local_name: then_cb.local_name,
                is_speculative: false,
            });
        }

        // Detect arrow-wrapped dynamic imports in call arguments:
        // `React.lazy(() => import('./Foo'))`, `loadable(() => import('./X'))`, etc.
        // Lazy loading wrappers always consume the default export.
        if let Some((import_expr, source)) = try_extract_arrow_wrapped_import(&expr.arguments) {
            self.dynamic_imports.push(DynamicImportInfo {
                source: source.to_string(),
                span: import_expr.span,
                destructured_names: vec!["default".to_string()],
                local_name: None,
                is_speculative: false,
            });
            self.handled_import_spans.insert(import_expr.span);
        }

        // Fluent-builder chain credit (issue #387).
        //
        // When `expr.callee` is `<some chain of calls>.this_method`, walk back
        // through the chain to a root `ID.root_method()`. Record one synthetic
        // `MemberAccess` keyed on the fluent-chain sentinel; the analyze layer
        // validates root_method is `is_instance_returning_static` and each
        // intermediate chain method is `is_self_returning` on the class before
        // crediting `this_method`. Without this, calls like
        // `EventBuilder.create().setX().setY()` flag every `setX` as unused.
        self.try_record_fluent_chain_access(expr);

        walk::walk_call_expression(self, expr);
    }

    fn visit_new_expression(&mut self, expr: &oxc_ast::ast::NewExpression<'a>) {
        // Detect `new URL('./path', import.meta.url)` pattern.
        // This is the standard Vite/bundler pattern for referencing worker files and assets.
        // Treat the path as a dynamic import so the target file is considered reachable.
        //
        // Directory-only specifiers (`./`, `../`, `./foo/`) construct a directory URL,
        // not a file URL; the canonical __dirname idiom
        // `fileURLToPath(new URL('./', import.meta.url))` must not surface as an import.
        // See issue #399.
        if let Some(source) = new_url_import_source(expr) {
            self.dynamic_imports.push(DynamicImportInfo {
                source,
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
                is_speculative: false,
            });
        }

        walk::walk_new_expression(self, expr);
    }

    /// Trace `typeof import('./path').X` references inside type positions.
    ///
    /// `auto-imports.d.ts` (unplugin-auto-import) and `components.d.ts`
    /// (unplugin-vue-components) embed these references inside
    /// `declare global { ... }` and `declare module 'vue' { ... }` ambient
    /// declarations. Without this handler, oxc walks the bodies but the
    /// `TSImportType` node has no extractor, so the referenced files end up
    /// flagged as `unused-files`. See issues #396 and #397.
    fn visit_ts_import_type(&mut self, node: &oxc_ast::ast::TSImportType<'a>) {
        let source = node.source.value.to_string();
        let source_span = node.source.span;

        let imported_name = node.qualifier.as_ref().map_or_else(
            || ImportedName::SideEffect,
            |q| ImportedName::Named(ts_import_type_qualifier_root(q).to_string()),
        );

        self.imports.push(ImportInfo {
            source,
            imported_name,
            local_name: String::new(),
            is_type_only: true,
            from_style: false,
            span: node.span,
            source_span,
        });

        walk::walk_ts_import_type(self, node);
    }

    #[expect(
        clippy::excessive_nesting,
        reason = "CJS export pattern matching requires deep nesting"
    )]
    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        // Detect module.exports = ... and exports.foo = ...
        if let AssignmentTarget::StaticMemberExpression(member) = &expr.left {
            if let Expression::Identifier(obj) = &member.object {
                if obj.name == "module" && member.property.name == "exports" {
                    self.has_cjs_exports = true;
                    // Extract exports from `module.exports = { foo, bar }`
                    if let Expression::ObjectExpression(obj_expr) = &expr.right {
                        for prop in &obj_expr.properties {
                            if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop
                                && let Some(name) = p.key.static_name()
                            {
                                self.exports.push(ExportInfo {
                                    name: ExportName::Named(name.to_string()),
                                    local_name: None,
                                    is_type_only: false,
                                    visibility: VisibilityTag::None,
                                    span: p.span,
                                    members: vec![],
                                    is_side_effect_used: false,
                                    super_class: None,
                                });
                            }
                        }
                    }
                }
                if obj.name == "exports" {
                    self.has_cjs_exports = true;
                    self.exports.push(ExportInfo {
                        name: ExportName::Named(member.property.name.to_string()),
                        local_name: None,
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: expr.span,
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    });
                }
            } else if let Expression::StaticMemberExpression(inner) = &member.object
                && let Expression::Identifier(obj) = &inner.object
                && obj.name == "module"
                && inner.property.name == "exports"
            {
                // Extract `module.exports.foo = value` as named export
                self.has_cjs_exports = true;
                self.exports.push(ExportInfo {
                    name: ExportName::Named(member.property.name.to_string()),
                    local_name: None,
                    is_type_only: false,
                    visibility: VisibilityTag::None,
                    span: expr.span,
                    members: vec![],
                    is_side_effect_used: false,
                    super_class: None,
                });
            }
            // Capture `this.member = ...` assignment patterns within class bodies.
            // This indicates the class uses the member internally.
            if matches!(member.object, Expression::ThisExpression(_)) {
                self.member_accesses.push(MemberAccess {
                    object: "this".to_string(),
                    member: member.property.name.to_string(),
                });
                // Track `this.field = new ClassName(...)` and `this.field = local`
                // for chained member access resolution. This lets
                // `this.field.method()` count as usage of the resolved target
                // symbol via the synthetic `"this.field"` binding key.
                if let Expression::NewExpression(new_expr) = &expr.right
                    && let Expression::Identifier(callee) = &new_expr.callee
                    && !super::helpers::is_builtin_constructor(callee.name.as_str())
                {
                    self.binding_target_names.insert(
                        format!("this.{}", member.property.name),
                        callee.name.to_string(),
                    );
                } else if let Expression::Identifier(ident) = &expr.right
                    && let Some(target_name) =
                        self.binding_target_names.get(ident.name.as_str()).cloned()
                {
                    self.binding_target_names
                        .insert(format!("this.{}", member.property.name), target_name);
                }
                if let Expression::Identifier(ident) = &expr.right {
                    self.copy_nested_binding_targets(
                        ident.name.as_str(),
                        format!("this.{}", member.property.name).as_str(),
                    );
                }
            }
        }
        walk::walk_assignment_expression(self, expr);
    }

    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        // Capture static member chains. `this.field.method()` is recorded as
        // object `this.field`; deeper chains like `this.deps.foo.method()` are
        // recorded as `this.deps.foo` and resolved through typed object bindings.
        if let Some(object_name) = static_member_object_name(&expr.object) {
            self.member_accesses.push(MemberAccess {
                object: object_name,
                member: expr.property.name.to_string(),
            });
        }
        // Capture `super.member` patterns inside a subclass body. `super.x()` in
        // `class Dog extends Animal` is semantically a use of `Animal.x`, so we emit
        // the access against the super class's local identifier. `local_to_imported`
        // in `find_unused_members` maps it back to the parent's export name.
        if matches!(expr.object, Expression::Super(_))
            && let Some(Some(super_local)) = self.class_super_stack.last()
        {
            self.member_accesses.push(MemberAccess {
                object: super_local.clone(),
                member: expr.property.name.to_string(),
            });
        }
        walk::walk_static_member_expression(self, expr);
    }

    fn visit_computed_member_expression(&mut self, expr: &ComputedMemberExpression<'a>) {
        if let Expression::Identifier(obj) = &expr.object {
            if let Expression::StringLiteral(lit) = &expr.expression {
                // Computed access with string literal resolves to a specific member
                self.member_accesses.push(MemberAccess {
                    object: obj.name.to_string(),
                    member: lit.value.to_string(),
                });
            } else {
                // Dynamic computed access — mark all members as used
                self.whole_object_uses.push(obj.name.to_string());
            }
        }
        walk::walk_computed_member_expression(self, expr);
    }

    fn visit_ts_qualified_name(&mut self, it: &TSQualifiedName<'a>) {
        // Capture `Enum.Member` in type positions (e.g., `type X = Status.Active`)
        if let TSTypeName::IdentifierReference(obj) = &it.left {
            self.member_accesses.push(MemberAccess {
                object: obj.name.to_string(),
                member: it.right.name.to_string(),
            });
        }
        walk::walk_ts_qualified_name(self, it);
    }

    fn visit_ts_mapped_type(&mut self, it: &TSMappedType<'a>) {
        // `{ [K in SomeEnum]: ... }` — all members of the constraint type are implicitly used
        if let TSType::TSTypeReference(type_ref) = &it.constraint
            && let TSTypeName::IdentifierReference(ident) = &type_ref.type_name
        {
            self.whole_object_uses.push(ident.name.to_string());
        }
        // `{ [K in keyof typeof SomeEnum]: ... }` — whole-object use via keyof typeof
        if let TSType::TSTypeOperatorType(op) = &it.constraint
            && op.operator == TSTypeOperatorOperator::Keyof
            && let TSType::TSTypeQuery(query) = &op.type_annotation
            && let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name
        {
            self.whole_object_uses.push(ident.name.to_string());
        }
        walk::walk_ts_mapped_type(self, it);
    }

    fn visit_ts_type_reference(&mut self, it: &TSTypeReference<'a>) {
        // `Record<SomeEnum, T>` — the first type arg is iterated as mapped keys.
        // Syntactically approximate: also fires for non-enum identifiers (interfaces,
        // classes), consistent with the conservative approach in other whole-object heuristics.
        if let TSTypeName::IdentifierReference(name) = &it.type_name
            && name.name == "Record"
            && let Some(type_args) = &it.type_arguments
            && let Some(first_arg) = type_args.params.first()
            && let TSType::TSTypeReference(key_ref) = first_arg
            && let TSTypeName::IdentifierReference(key_ident) = &key_ref.type_name
        {
            self.whole_object_uses.push(key_ident.name.to_string());
        }
        walk::walk_ts_type_reference(self, it);
    }

    fn visit_for_in_statement(&mut self, stmt: &ForInStatement<'a>) {
        if let Expression::Identifier(ident) = &stmt.right {
            self.whole_object_uses.push(ident.name.to_string());
        }
        walk::walk_for_in_statement(self, stmt);
    }

    fn visit_spread_element(&mut self, elem: &SpreadElement<'a>) {
        if let Expression::Identifier(ident) = &elem.argument {
            self.whole_object_uses.push(ident.name.to_string());
        }
        walk::walk_spread_element(self, elem);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        // Detect Lit `@customElement('tag')` decorator. The class is registered
        // as a Web Component at module load time without anyone importing the
        // class identifier, so its export must be flagged as side-effect-used.
        if let Some(decorator) = lit_custom_element_decorator(class) {
            if let Some(id) = class.id.as_ref() {
                self.record_lit_custom_element_candidate(
                    decorator,
                    SideEffectRegistrationTarget::LocalClass(id.name.to_string()),
                );
            } else if let Some(export) = self.exports.last()
                && matches!(export.name, crate::ExportName::Default)
                && export.local_name.is_none()
            {
                // Anonymous `export default @customElement(...) class extends LitElement {}`
                // has no class identifier to key off and an unset local_name on the
                // Default export. Remember the export slot and validate the decorator
                // import after the full walk.
                let export_index = self.exports.len() - 1;
                self.record_lit_custom_element_candidate(
                    decorator,
                    SideEffectRegistrationTarget::AnonymousDefaultExport(export_index),
                );
            }
        }

        // Detect Angular @Component decorator and extract all metadata:
        // templateUrl/styleUrl imports, inline template refs, host binding refs,
        // and inputs/outputs member names.
        if let Some(meta) = extract_angular_component_metadata(class) {
            // Emit SideEffect imports for templateUrl and styleUrl/styleUrls.
            // Angular resolves both `'app.html'` and `'./app.html'` relative to
            // the component file; normalize bare filenames so downstream
            // resolution doesn't misclassify them as npm packages.
            if let Some(ref template_url) = meta.template_url {
                self.imports.push(ImportInfo {
                    source: normalize_asset_url(template_url),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::default(),
                    source_span: oxc_span::Span::default(),
                });
                // Flag the module as an Angular component-template owner so
                // the CRAP-inherit walker accepts it as an inheritance source
                // for the `.html` target. Plain `import './x.html'` does not
                // set this flag and is correctly rejected.
                self.has_angular_component_template_url = true;
            }
            for style_url in &meta.style_urls {
                self.imports.push(ImportInfo {
                    source: normalize_asset_url(style_url),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::default(),
                    source_span: oxc_span::Span::default(),
                });
            }

            // Scan inline template for member references.
            //
            // Bare identifier refs are emitted as sentinel `MemberAccess` so
            // the analysis phase credits them as members of the component's
            // own class (via `self_accessed_members`).
            //
            // Static member-access chains (`dataService.getTotal`) are emitted
            // as regular `MemberAccess` entries and resolved at end of visit
            // by `resolve_bound_member_accesses`, which maps `dataService`
            // through the class's typed constructor params or properties to
            // the concrete type name (e.g. `DataService`). This credits the
            // target class's member as used through the existing member-access
            // pipeline, without any Angular-specific analysis code.
            if let Some(ref template) = meta.inline_template {
                let refs = crate::sfc_template::angular::collect_angular_template_refs(template);
                for name in refs.identifiers {
                    self.member_accesses.push(MemberAccess {
                        object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                        member: name,
                    });
                }
                self.member_accesses.extend(refs.member_accesses);

                // Defer template-complexity scanning to `parse.rs`, where the
                // per-file `line_offsets` table is available to remap the
                // synthetic finding onto the host `.ts` file's coordinates.
                self.inline_template_findings
                    .push(super::InlineTemplateFinding {
                        template_source: template.clone(),
                        decorator_start: meta.decorator_span.start,
                    });
            }

            // Emit sentinel accesses for host binding member references
            for name in &meta.host_member_refs {
                self.member_accesses.push(MemberAccess {
                    object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                    member: name.clone(),
                });
            }

            // Emit sentinel accesses for inputs/outputs metadata members
            for name in &meta.input_output_members {
                self.member_accesses.push(MemberAccess {
                    object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                    member: name.clone(),
                });
            }
        }
        // Track the super class name so `super.member` accesses inside this class
        // body can be attributed to the parent (see `visit_static_member_expression`).
        // Pushed for every class (including ones without a super clause) so the stack
        // depth matches the visit depth when nested classes appear.
        self.class_super_stack
            .push(super::helpers::extract_super_class_name(class));
        // Track class type-parameter constraints so `constructor(client: TClient)`
        // inside `class BaseService<TClient extends BaseClient>` resolves to
        // `this.client -> BaseClient`. Pushed for every class so the stack depth
        // matches the visit depth when nested classes appear. See issue #388.
        self.class_type_param_constraints
            .push(super::helpers::collect_class_type_param_constraints(class));
        walk::walk_class(self, class);
        self.class_type_param_constraints.pop();
        self.class_super_stack.pop();
    }

    /// Track asset references inside `` html`...` `` tagged template literals
    /// as `SideEffect` imports.
    ///
    /// SSR helpers like `hono/html`, `lit-html`, and `htm` emit HTML via a
    /// tagged template whose tag is the identifier `html`. The static markup
    /// lives in the template quasis, and `${...}` interpolations are used for
    /// dynamic content only. When a layout component writes
    /// `` html`<script src="/static/app.js"></script>` ``, the `/static/app.js`
    /// file must stay reachable from that module, exactly like the HTML parser
    /// handles the same markup in `.html` files. See issue #105 (till's
    /// follow-up comment).
    ///
    /// Only the `Expression::Identifier` tag named `html` is matched — member
    /// expressions (`lit.html`), call expressions, and other identifiers are
    /// deliberately skipped to avoid conflating unrelated tagged templates
    /// (`css`, `sql`, `gql`, `styled.div`) with HTML. Each quasi is scanned
    /// independently so an asset reference spanning an interpolation boundary
    /// is ignored rather than producing a garbled, unresolvable specifier.
    fn visit_tagged_template_expression(&mut self, expr: &TaggedTemplateExpression<'a>) {
        if is_html_tagged_template(&expr.tag) {
            for quasi in &expr.quasi.quasis {
                let text = quasi
                    .value
                    .cooked
                    .as_ref()
                    .map_or_else(|| quasi.value.raw.as_str(), |c| c.as_str());
                for raw in crate::html::collect_asset_refs(text) {
                    self.push_html_template_asset_import(&raw);
                }
            }
        }
        walk::walk_tagged_template_expression(self, expr);
    }
}

fn static_argument_object_name(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::Identifier(ident) => Some(ident.name.to_string()),
        Argument::ThisExpression(_) => Some("this".to_string()),
        Argument::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            static_member_object_name(&member.object)?,
            member.property.name
        )),
        _ => None,
    }
}

fn static_member_object_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(obj) => Some(obj.name.to_string()),
        Expression::ThisExpression(_) => Some("this".to_string()),
        Expression::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            static_member_object_name(&member.object)?,
            member.property.name
        )),
        // `this.vc()` — Angular signal query call. The synthetic name
        // `this.vc()` is registered in `binding_target_names` so the
        // surrounding chain `this.vc()?.method()` can be resolved through
        // the existing bound-member-access pipeline. Restricted to zero-arg
        // calls so non-getter call sites do not steal the member access.
        Expression::CallExpression(call) if call.arguments.is_empty() => {
            Some(format!("{}()", static_member_object_name(&call.callee)?))
        }
        // `new Class(...).method()`: credit the method on the constructed
        // class. The receiver `new Class(...)` resolves to the bare class name
        // so the surrounding member access is recorded as `Class.method` and
        // credited through the same `local_to_export_keys` path as a direct
        // static access. The bare identifier is recorded UNCONDITIONALLY, even
        // for builtin-shaped names: distinguishing a global `new Map()` from a
        // user `class Map {}` is the analyze layer's job, not extraction's. A
        // global resolves to no user export there and drops silently, while a
        // user-defined class named `URL` / `Request` / `Map` is credited
        // correctly. Guarding on `is_builtin_constructor` here would silently
        // re-introduce the issue-605 false positive for those classes.
        // Member-expression callees (`new ns.Class()`) are out of scope.
        // See issue #605.
        Expression::NewExpression(new_expr) => match &new_expr.callee {
            Expression::Identifier(callee) => Some(callee.name.to_string()),
            _ => None,
        },
        // `(this.vc()?.method)` and similar optional chains wrap the
        // member-access in a `ChainExpression`; descend into the wrapped form
        // so `this.vc()?.refresh()` resolves the same way as `this.vc().refresh()`.
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) if call.arguments.is_empty() => {
                Some(format!("{}()", static_member_object_name(&call.callee)?))
            }
            ChainElement::StaticMemberExpression(member) => Some(format!(
                "{}.{}",
                static_member_object_name(&member.object)?,
                member.property.name
            )),
            _ => None,
        },
        _ => None,
    }
}

/// Returns true when the tagged template's tag is the bare identifier `html`.
fn is_html_tagged_template(tag: &Expression<'_>) -> bool {
    matches!(tag, Expression::Identifier(id) if id.name == "html")
}

impl ModuleInfoExtractor {
    /// Push an HTML-template-sourced asset reference onto `imports`, mirroring
    /// the HTML parser's remote-url, normalization, and `SideEffect` pipeline.
    fn push_html_template_asset_import(&mut self, raw: &str) {
        let trimmed = raw.trim();
        if trimmed.is_empty() || is_remote_url(trimmed) {
            return;
        }
        self.imports.push(ImportInfo {
            source: normalize_asset_url(trimmed),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: false,
            span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        });
    }
}
