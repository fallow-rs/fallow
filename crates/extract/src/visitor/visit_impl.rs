//! `Visit` trait implementation for `ModuleInfoExtractor`.

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
    ClassHeritageInfo, LocalTypeDeclaration, PublicSignatureTypeReference, SanitizedSinkArg,
    SanitizerScope, SinkArgKind, SinkLiteralValue, SinkObjectProperty, SinkShape, SinkSite,
    TaintedBinding,
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
    ModuleInfoExtractor, PendingLocalExportSpecifier, SecurityPathSinkBinding,
    SideEffectRegistrationTarget, try_extract_arrow_wrapped_import, try_extract_dynamic_import,
    try_extract_import_then_callback, try_extract_property_callback_import, try_extract_require,
};

const PINO_PACKAGE: &str = "pino";
const PINO_FACTORY_EXPORT: &str = "pino";
const PINO_TRANSPORT_KEY: &str = "transport";
const PINO_TARGET_KEY: &str = "target";
const PINO_TARGETS_KEY: &str = "targets";
const FRAMEWORK_REQUEST_SOURCE: &str = "framework.request";
const NEXT_REQUEST_SOURCE: &str = "next.request";
const NEXT_FORM_DATA_SOURCE: &str = "next.form-data";
const QUEUE_JOB_SOURCE: &str = "queue.job";
const MCP_TOOL_INPUT_SOURCE: &str = "mcp.tool-input";

type StaticPackageStringBindings = FxHashMap<String, Vec<String>>;
type StaticPackageObjectBindings = FxHashMap<String, FxHashMap<String, Vec<String>>>;
type StaticPackageLoopBindings = (StaticPackageStringBindings, StaticPackageObjectBindings);

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

struct StructuralParamMemberCollector {
    target_params: FxHashSet<String>,
    shadowed_stack: Vec<FxHashSet<String>>,
    members: FxHashMap<String, FxHashSet<String>>,
}

impl StructuralParamMemberCollector {
    fn new(target_params: FxHashSet<String>) -> Self {
        Self {
            target_params,
            shadowed_stack: Vec::new(),
            members: FxHashMap::default(),
        }
    }

    fn is_shadowed(&self, name: &str) -> bool {
        self.shadowed_stack.iter().any(|scope| scope.contains(name))
    }

    fn collect_shadowed_params(&self, params: &FormalParameters<'_>) -> FxHashSet<String> {
        let mut shadowed = FxHashSet::default();
        for param in &params.items {
            if let BindingPattern::BindingIdentifier(id) = &param.pattern
                && self.target_params.contains(id.name.as_str())
            {
                shadowed.insert(id.name.to_string());
            }
        }
        shadowed
    }

    fn record_shadowed_bindings<'a>(
        &mut self,
        bindings: impl Iterator<Item = &'a BindingIdentifier<'a>>,
    ) {
        let Some(scope) = self.shadowed_stack.last_mut() else {
            return;
        };
        for binding in bindings {
            if self.target_params.contains(binding.name.as_str()) {
                scope.insert(binding.name.to_string());
            }
        }
    }
}

impl<'a> Visit<'a> for StructuralParamMemberCollector {
    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        if let Expression::Identifier(object) = &expr.object
            && self.target_params.contains(object.name.as_str())
            && !self.is_shadowed(object.name.as_str())
        {
            self.members
                .entry(object.name.to_string())
                .or_default()
                .insert(expr.property.name.to_string());
        }
        walk::walk_static_member_expression(self, expr);
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        let shadowed = self.collect_shadowed_params(&func.params);
        self.shadowed_stack.push(shadowed);
        walk::walk_function(self, func, flags);
        self.shadowed_stack.pop();
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        let shadowed = self.collect_shadowed_params(&expr.params);
        self.shadowed_stack.push(shadowed);
        walk::walk_arrow_function_expression(self, expr);
        self.shadowed_stack.pop();
    }

    fn visit_block_statement(&mut self, stmt: &BlockStatement<'a>) {
        self.shadowed_stack.push(FxHashSet::default());
        walk::walk_block_statement(self, stmt);
        self.shadowed_stack.pop();
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        if matches!(
            decl.kind,
            VariableDeclarationKind::Const | VariableDeclarationKind::Let
        ) {
            self.record_shadowed_bindings(
                decl.declarations
                    .iter()
                    .flat_map(|declarator| declarator.id.get_binding_identifiers()),
            );
        }
        walk::walk_variable_declaration(self, decl);
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

fn pino_factory_callee_name(callee: &Expression<'_>) -> Option<String> {
    match unwrap_static_expr(callee) {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}

fn collect_pino_config_targets(expr: &Expression<'_>, out: &mut Vec<String>) {
    match unwrap_static_expr(expr) {
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                    continue;
                };
                if prop
                    .key
                    .static_name()
                    .is_some_and(|name| name == PINO_TRANSPORT_KEY)
                {
                    collect_pino_transport_targets(&prop.value, out);
                }
            }
        }
        Expression::ConditionalExpression(cond) => {
            collect_pino_config_targets(&cond.consequent, out);
            collect_pino_config_targets(&cond.alternate, out);
        }
        _ => {}
    }
}

fn collect_pino_transport_targets(expr: &Expression<'_>, out: &mut Vec<String>) {
    match unwrap_static_expr(expr) {
        Expression::ObjectExpression(obj) => collect_pino_transport_object_targets(obj, out),
        Expression::ConditionalExpression(cond) => {
            collect_pino_transport_targets(&cond.consequent, out);
            collect_pino_transport_targets(&cond.alternate, out);
        }
        _ => {}
    }
}

fn collect_pino_transport_object_targets(obj: &ObjectExpression<'_>, out: &mut Vec<String>) {
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        match prop.key.static_name().as_deref() {
            Some(PINO_TARGET_KEY) => record_pino_target_value(&prop.value, out),
            Some(PINO_TARGETS_KEY) => record_pino_targets_array(&prop.value, out),
            _ => {}
        }
    }
}

fn record_pino_target_value(expr: &Expression<'_>, out: &mut Vec<String>) {
    if let Expression::StringLiteral(lit) = unwrap_static_expr(expr) {
        record_pino_target(lit.value.as_str(), out);
    }
}

fn record_pino_targets_array(expr: &Expression<'_>, out: &mut Vec<String>) {
    let Expression::ArrayExpression(array) = unwrap_static_expr(expr) else {
        return;
    };
    for element in &array.elements {
        match element {
            ArrayExpressionElement::ObjectExpression(obj) => {
                collect_pino_transport_object_targets(obj, out);
            }
            ArrayExpressionElement::ParenthesizedExpression(paren) => {
                if let Expression::ObjectExpression(obj) = unwrap_static_expr(&paren.expression) {
                    collect_pino_transport_object_targets(obj, out);
                }
            }
            _ => {}
        }
    }
}

fn record_pino_target(source: &str, out: &mut Vec<String>) {
    if source.is_empty() || out.iter().any(|existing| existing == source) {
        return;
    }
    out.push(source.to_string());
}

fn is_dompurify_source(source: &str) -> bool {
    matches!(source, "dompurify" | "isomorphic-dompurify")
}

/// Detect whether `vi.mock(specifier, factory, ...)` passes a factory.
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

/// Whether `callee` is a `useMemo` / `React.useMemo` reference.
///
/// `useMemo(factory)` returns the factory's product directly, so
/// `const svc = useMemo(() => new Svc())` binds `svc` to a `Svc` instance.
/// This is unlike `useState`, which returns a `[value, setter]` tuple (handled
/// only through the array-destructure factory path), so an unqualified
/// `const x = useState(() => new Foo())` must NOT bind. Scoped to `useMemo`
/// because an arbitrary wrapper does not necessarily return the instance;
/// binding generically would over-credit and hide genuinely-unused members.
/// See issue #844.
fn is_value_returning_memo_callee(callee: &Expression<'_>) -> bool {
    match callee {
        Expression::Identifier(id) => id.name == "useMemo",
        Expression::StaticMemberExpression(member) => member.property.name == "useMemo",
        _ => false,
    }
}

/// Specifier source string from the first argument of `register(...)`.
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

/// Allowlisted loader-hook exports for `module.register()`.
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

/// Collect `property -> class type` mappings from an object-type member list.
fn collect_object_type_property_types(members: &[TSSignature<'_>]) -> FxHashMap<String, String> {
    let mut properties = FxHashMap::default();
    for member in members {
        let TSSignature::TSPropertySignature(prop) = member else {
            continue;
        };
        let Some(property_name) = prop.key.static_name() else {
            continue;
        };
        let Some(type_annotation) = prop.type_annotation.as_deref() else {
            continue;
        };
        if let Some(type_name) = extract_type_annotation_name(type_annotation) {
            properties.insert(property_name.to_string(), type_name);
        }
    }
    properties
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
        Expression::CallExpression(call) => playwright_test_callee_name(&call.callee),
        _ => None,
    }
}

/// Find the call expression returned by a function body.
fn extract_function_body_final_return_call<'a, 'b>(
    body: &'b oxc_ast::ast::FunctionBody<'a>,
) -> Option<&'b CallExpression<'a>> {
    let Expression::CallExpression(call) = extract_function_body_final_return_expr(body)? else {
        return None;
    };
    Some(call.as_ref())
}

fn extract_function_body_final_return_expr<'a, 'b>(
    body: &'b oxc_ast::ast::FunctionBody<'a>,
) -> Option<&'b Expression<'a>> {
    let Statement::ReturnStatement(ret) = body.statements.last()? else {
        return None;
    };
    ret.argument.as_ref()
}

/// Find the call expression used as an arrow function body.
fn extract_arrow_return_call<'a, 'b>(
    arrow: &'b oxc_ast::ast::ArrowFunctionExpression<'a>,
) -> Option<&'b CallExpression<'a>> {
    let Expression::CallExpression(call) = extract_arrow_return_expr(arrow)? else {
        return None;
    };
    Some(call.as_ref())
}

fn extract_arrow_return_expr<'a, 'b>(
    arrow: &'b oxc_ast::ast::ArrowFunctionExpression<'a>,
) -> Option<&'b Expression<'a>> {
    if arrow.expression {
        if arrow.body.statements.len() != 1 {
            return None;
        }
        let Statement::ExpressionStatement(stmt) = arrow.body.statements.first()? else {
            return None;
        };
        return Some(&stmt.expression);
    }
    extract_function_body_final_return_expr(&arrow.body)
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

    fn record_sanitizer_binding(&mut self, name: &str, scope: Option<SanitizerScope>) {
        if self.is_module_scope() {
            self.module_sanitizer_bindings
                .insert(name.to_string(), scope);
            return;
        }
        if let Some(bindings) = self.sanitizer_binding_stack.last_mut() {
            bindings.insert(name.to_string(), scope);
        }
    }

    fn record_literal_allowlist_binding(&mut self, name: &str, trusted: bool) {
        if self.is_module_scope() {
            self.module_literal_allowlist_bindings
                .insert(name.to_string(), trusted);
            return;
        }
        if let Some(bindings) = self.literal_allowlist_binding_stack.last_mut() {
            bindings.insert(name.to_string(), trusted);
        }
    }

    fn literal_allowlist_binding(&self, name: &str) -> bool {
        for bindings in self.literal_allowlist_binding_stack.iter().rev() {
            if let Some(trusted) = bindings.get(name) {
                return *trusted;
            }
        }
        self.module_literal_allowlist_bindings
            .get(name)
            .copied()
            .unwrap_or(false)
    }

    fn record_path_sink_binding(&mut self, name: &str, binding: Option<SecurityPathSinkBinding>) {
        if self.is_module_scope() {
            self.module_path_sink_bindings
                .insert(name.to_string(), binding);
            return;
        }
        if let Some(bindings) = self.path_sink_binding_stack.last_mut() {
            bindings.insert(name.to_string(), binding);
        }
    }

    fn path_sink_binding(&self, name: &str) -> Option<SecurityPathSinkBinding> {
        for bindings in self.path_sink_binding_stack.iter().rev() {
            if let Some(binding) = bindings.get(name) {
                return *binding;
            }
        }
        self.module_path_sink_bindings
            .get(name)
            .and_then(|binding| *binding)
    }

    fn record_path_relative_binding(&mut self, name: &str, target: Option<String>) {
        if self.is_module_scope() {
            self.module_path_relative_bindings
                .insert(name.to_string(), target);
            return;
        }
        if let Some(bindings) = self.path_relative_binding_stack.last_mut() {
            bindings.insert(name.to_string(), target);
        }
    }

    fn path_relative_binding(&self, name: &str) -> Option<&str> {
        for bindings in self.path_relative_binding_stack.iter().rev() {
            if let Some(target) = bindings.get(name) {
                return target.as_deref();
            }
        }
        self.module_path_relative_bindings
            .get(name)
            .and_then(Option::as_deref)
    }

    fn sanitizer_scope_for_identifier(&self, name: &str) -> Option<SanitizerScope> {
        for bindings in self.sanitizer_binding_stack.iter().rev() {
            if let Some(scope) = bindings.get(name) {
                return *scope;
            }
        }
        self.module_sanitizer_bindings
            .get(name)
            .and_then(|scope| *scope)
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
        let sanitizer_scope = scope
            .iter()
            .map(|name| (name.clone(), None))
            .collect::<FxHashMap<_, _>>();
        let allowlist_scope = scope
            .iter()
            .map(|name| (name.clone(), false))
            .collect::<FxHashMap<_, _>>();
        let path_sink_scope = scope
            .iter()
            .map(|name| (name.clone(), None))
            .collect::<FxHashMap<_, _>>();
        let path_relative_scope = scope
            .iter()
            .map(|name| (name.clone(), None))
            .collect::<FxHashMap<_, _>>();
        self.nested_declaration_stack.push(scope);
        self.sanitizer_binding_stack.push(sanitizer_scope);
        self.literal_allowlist_binding_stack.push(allowlist_scope);
        self.path_sink_binding_stack.push(path_sink_scope);
        self.path_relative_binding_stack.push(path_relative_scope);
    }

    fn pop_function_declaration_scope(&mut self) {
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.pop();
            self.sanitizer_binding_stack.pop();
            self.literal_allowlist_binding_stack.pop();
            self.path_sink_binding_stack.pop();
            self.path_relative_binding_stack.pop();
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

    /// Record destructured bindings with type annotations.
    fn record_typed_destructure_binding(
        &mut self,
        pattern: &ObjectPattern<'_>,
        type_annotation: &TSTypeAnnotation<'_>,
    ) {
        let bindings = extract_object_pattern_bindings(pattern);
        if bindings.is_empty() {
            return;
        }
        if let TSType::TSTypeLiteral(type_lit) = &type_annotation.type_annotation {
            let properties = collect_object_type_property_types(&type_lit.members);
            for (local, key) in bindings {
                if let Some(class_name) = properties.get(&key) {
                    self.binding_target_names
                        .entry(local)
                        .or_insert_with(|| class_name.clone());
                }
            }
        } else if let Some(type_name) = extract_type_annotation_name(type_annotation) {
            for (local, key) in bindings {
                self.pending_typed_destructures
                    .push((local, key, type_name.clone()));
            }
        }
    }

    fn record_local_structural_function(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        body: Option<&FunctionBody<'_>>,
    ) {
        let Some(body) = body else {
            return;
        };
        let typed_params: Vec<(usize, String, String)> = params
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                    return None;
                };
                let type_annotation = param.type_annotation.as_deref()?;
                let type_name = extract_type_annotation_name(type_annotation)?;
                Some((index, id.name.to_string(), type_name))
            })
            .collect();
        if typed_params.is_empty() {
            return;
        }

        let target_params = typed_params
            .iter()
            .map(|(_, param_name, _)| param_name.clone())
            .collect();
        let mut collector = StructuralParamMemberCollector::new(target_params);
        collector.visit_function_body(body);

        let mut function = super::LocalStructuralFunction::default();
        for (index, param_name, type_name) in typed_params {
            let Some(members) = collector.members.remove(param_name.as_str()) else {
                continue;
            };
            if members.is_empty() {
                continue;
            }
            function
                .params
                .insert(index, super::StructuralParameterUse { type_name, members });
        }

        if !function.params.is_empty() {
            self.local_structural_functions
                .insert(name.to_string(), function);
        }
    }

    fn structural_call_argument(arg: &Argument<'_>) -> Option<super::StructuralCallArgument> {
        let expr = arg.as_expression()?;
        match expr {
            Expression::NewExpression(new_expr) => {
                let Expression::Identifier(callee) = &new_expr.callee else {
                    return None;
                };
                if super::helpers::is_builtin_constructor(callee.name.as_str()) {
                    return None;
                }
                Some(super::StructuralCallArgument::DirectClass(
                    callee.name.to_string(),
                ))
            }
            Expression::Identifier(ident) => Some(super::StructuralCallArgument::Binding(
                ident.name.to_string(),
            )),
            _ => None,
        }
    }

    fn record_structural_class_call_candidate(&mut self, call: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };

        let arguments: Vec<Option<super::StructuralCallArgument>> = call
            .arguments
            .iter()
            .map(Self::structural_call_argument)
            .collect();
        if arguments.iter().all(Option::is_none) {
            return;
        }

        self.structural_class_call_candidates
            .push(super::StructuralClassCallCandidate {
                callee_name: callee.name.to_string(),
                arguments,
            });
    }

    fn record_local_structural_function_from_variable_declarator(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                self.record_local_structural_function(
                    id.name.as_str(),
                    &arrow.params,
                    Some(arrow.body.as_ref()),
                );
            }
            Expression::FunctionExpression(function) => {
                self.record_local_structural_function(
                    id.name.as_str(),
                    &function.params,
                    function.body.as_deref(),
                );
            }
            _ => {}
        }
    }

    /// Record `const TOKEN = new InjectionToken<Interface>(...)` declarations
    /// so the analyze layer can follow the token's interface type argument to
    /// the classes that `implement` it. Gated on `InjectionToken` being a named
    /// import from `@angular/core` (a same-named local class is ignored). A
    /// token with no type argument carries no interface and is skipped. See
    /// issue #920.
    fn record_injection_token(&mut self, name: &str, init: &Expression<'_>) {
        if !self.is_module_scope() {
            return;
        }
        let Expression::NewExpression(new_expr) = init else {
            return;
        };
        let Expression::Identifier(callee) = &new_expr.callee else {
            return;
        };
        if !self.is_named_import_from(callee.name.as_str(), "@angular/core", "InjectionToken") {
            return;
        }
        let Some(type_arguments) = new_expr.type_arguments.as_deref() else {
            return;
        };
        let Some(TSType::TSTypeReference(type_ref)) = type_arguments.params.first() else {
            return;
        };
        if let Some((interface_name, _)) = type_name_root(&type_ref.type_name) {
            self.injection_tokens
                .push((name.to_string(), interface_name));
        }
    }

    fn clear_literal_allowlist_on_mutating_member_call(&mut self, call: &CallExpression<'_>) {
        if let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(object) = &member.object
            && !matches!(member.property.name.as_str(), "has" | "includes")
            && self.literal_allowlist_binding(&object.name)
        {
            self.record_literal_allowlist_binding(object.name.as_str(), false);
        }
    }

    fn svelte_derived_new_class(init: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = init else {
            return None;
        };
        if !Self::is_svelte_derived_call(call) {
            return None;
        }

        if let Some(expr) = call.arguments.first().and_then(Argument::as_expression)
            && let Expression::NewExpression(new_expr) = expr
            && let Expression::Identifier(callee) = &new_expr.callee
            && !super::helpers::is_builtin_constructor(callee.name.as_str())
        {
            return Some(callee.name.to_string());
        }

        super::helpers::try_extract_factory_new_class(&call.arguments)
    }

    fn is_svelte_derived_call(call: &CallExpression<'_>) -> bool {
        match &call.callee {
            Expression::Identifier(id) => id.name == "$derived",
            Expression::StaticMemberExpression(member) => {
                member.property.name == "by"
                    && matches!(&member.object, Expression::Identifier(id) if id.name == "$derived")
            }
            _ => false,
        }
    }

    /// Substitute a class type-parameter with its constraint when available.
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

    /// Emit a fluent-chain sentinel `MemberAccess` for chained calls.
    fn try_record_fluent_chain_access(&mut self, expr: &CallExpression<'_>) {
        let Expression::StaticMemberExpression(member) = &expr.callee else {
            return;
        };
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

    /// Recognize `.forEach(...)` on iterables and bind the callback element.
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

    pub(super) fn is_named_import_from(
        &self,
        local_name: &str,
        source: &str,
        imported_name: &str,
    ) -> bool {
        self.imports.iter().any(|import| {
            import.source == source
                && import.local_name == local_name
                && matches!(&import.imported_name, ImportedName::Named(name) if name == imported_name)
        })
    }

    fn is_pino_factory_binding(&self, local_name: &str) -> bool {
        let imported = self.imports.iter().any(|import| {
            import.source == PINO_PACKAGE
                && import.local_name == local_name
                && !import.is_type_only
                && match &import.imported_name {
                    ImportedName::Default => true,
                    ImportedName::Named(name) => name == PINO_FACTORY_EXPORT,
                    ImportedName::Namespace | ImportedName::SideEffect => false,
                }
        });
        let required = self.require_calls.iter().any(|require| {
            require.source == PINO_PACKAGE
                && require.local_name.as_deref() == Some(local_name)
                && require.destructured_names.is_empty()
        });
        (imported || required) && !self.nested_scope_shadows(local_name)
    }

    fn try_record_pino_transport_targets(&mut self, expr: &CallExpression<'_>) {
        let Some(local_name) = pino_factory_callee_name(&expr.callee) else {
            return;
        };
        if !self.is_pino_factory_binding(&local_name) {
            return;
        }

        let Some(config) = expr.arguments.first().and_then(Argument::as_expression) else {
            return;
        };

        let mut targets = Vec::new();
        collect_pino_config_targets(config, &mut targets);
        for source in targets.into_iter().filter(|source| !source.is_empty()) {
            self.dynamic_imports.push(DynamicImportInfo {
                source,
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
                is_speculative: false,
            });
        }
    }

    /// Record `register('loader', ...)` from `node:module` as a dynamic import.
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

    /// Record a tainted-source binding for `const <name> = <object>.<prop>`,
    /// where the initializer is a member-access chain. The recorded
    /// `source_path` is the flattened OBJECT path (the chain with its final
    /// property dropped), so `const id = req.query.id` records
    /// `{ local: "id", source_path: "req.query" }` and the analyze layer can
    /// match it against a `req.query` source pattern. The init may be wrapped in
    /// `await` / parens (`const id = await req.json()`-style chains resolve to
    /// the member object). A non-member init records nothing. Captured at any
    /// scope (no `is_module_scope` gate): a sink inside a route handler reading a
    /// function-local `const id = req.query.id` is exactly the target case.
    fn record_tainted_source_binding(&mut self, name: &str, expr: &Expression<'_>) {
        if let Some(source_path) = tainted_source_path(expr) {
            self.tainted_bindings.push(TaintedBinding {
                local: name.to_string(),
                source_path,
            });
        }
    }

    fn record_tainted_param_binding(&mut self, name: &str, source_path: &'static str) {
        if self
            .tainted_bindings
            .iter()
            .any(|b| b.local == name && b.source_path == source_path)
        {
            return;
        }
        self.tainted_bindings.push(TaintedBinding {
            local: name.to_string(),
            source_path: source_path.to_string(),
        });
    }

    fn record_first_param_source(
        &mut self,
        params: &FormalParameters<'_>,
        source_path: &'static str,
    ) {
        let Some(param) = params.items.first() else {
            return;
        };
        match &param.pattern {
            BindingPattern::BindingIdentifier(id) => {
                self.record_tainted_param_binding(id.name.as_str(), source_path);
            }
            BindingPattern::ObjectPattern(obj_pat) => {
                for local in super::extract_destructured_names(obj_pat) {
                    self.record_tainted_param_binding(&local, source_path);
                }
            }
            _ => {}
        }
    }

    fn record_named_param_source(
        &mut self,
        params: &FormalParameters<'_>,
        names: &[&str],
        source_path: &'static str,
    ) {
        for param in &params.items {
            if let BindingPattern::BindingIdentifier(id) = &param.pattern
                && names.iter().any(|name| *name == id.name.as_str())
            {
                self.record_tainted_param_binding(id.name.as_str(), source_path);
            }
        }
    }

    fn record_next_function_param_sources(&mut self, func: &Function<'_>) {
        if func
            .id
            .as_ref()
            .is_some_and(|id| is_http_route_handler_name(id.name.as_str()))
        {
            self.record_first_param_source(&func.params, NEXT_REQUEST_SOURCE);
        }
        if function_body_has_use_server(func.body.as_deref()) {
            self.record_named_param_source(&func.params, &["formData"], NEXT_FORM_DATA_SOURCE);
        }
    }

    fn record_next_arrow_param_sources(&mut self, expr: &ArrowFunctionExpression<'_>) {
        if function_body_has_use_server(Some(&expr.body)) {
            self.record_named_param_source(&expr.params, &["formData"], NEXT_FORM_DATA_SOURCE);
        }
    }

    fn record_framework_callback_param_sources(&mut self, call: &CallExpression<'_>) {
        let Some(callee_path) = flatten_callee_path(&call.callee) else {
            return;
        };
        let Some((_, method)) = callee_path.rsplit_once('.') else {
            return;
        };
        if is_route_registration_method(method) {
            if let Some(params) = route_callback_params(&call.arguments, method) {
                self.record_first_param_source(params, FRAMEWORK_REQUEST_SOURCE);
            }
            return;
        }
        if method == "process"
            && let Some(params) = last_callback_params(&call.arguments)
        {
            self.record_first_param_source(params, QUEUE_JOB_SOURCE);
            return;
        }
        if method == "tool"
            && let Some(params) = last_callback_params(&call.arguments)
        {
            self.record_first_param_source(params, MCP_TOOL_INPUT_SOURCE);
        }
    }

    fn record_queue_worker_constructor_param_sources(&mut self, expr: &NewExpression<'_>) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            return;
        };
        if callee_path.rsplit('.').next() != Some("Worker") {
            return;
        }
        if let Some(params) = expr.arguments.iter().skip(1).find_map(callback_params) {
            self.record_first_param_source(params, QUEUE_JOB_SOURCE);
        }
    }

    fn record_tainted_helper_call_binding(&mut self, name: &str, expr: &Expression<'_>) {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if self.nested_scope_shadows(callee.name.as_str()) {
            return;
        }
        let Some(helper) = self
            .source_returning_helpers
            .get(callee.name.as_str())
            .cloned()
        else {
            return;
        };

        let mut source_paths = Vec::new();
        for path in &helper.paths {
            let Some(arg_expr) = call
                .arguments
                .get(path.arg_index)
                .and_then(Argument::as_expression)
            else {
                continue;
            };
            source_paths.extend(apply_source_return_path(arg_expr, &path.suffixes));
        }
        source_paths.sort();
        source_paths.dedup();
        for source_path in source_paths {
            self.tainted_bindings.push(TaintedBinding {
                local: name.to_string(),
                source_path,
            });
        }
    }

    fn record_source_returning_function_helper(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        body: &oxc_ast::ast::FunctionBody<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let Some(expr) = extract_function_body_final_return_expr(body) else {
            self.source_returning_helpers.remove(name);
            return;
        };
        if let Some(helper) = source_returning_helper(params, expr) {
            self.source_returning_helpers
                .insert(name.to_string(), helper);
        } else {
            self.source_returning_helpers.remove(name);
        }
    }

    fn record_source_returning_function_declaration(&mut self, function: &Function<'_>) {
        let (Some(id), Some(body)) = (function.id.as_ref(), function.body.as_deref()) else {
            return;
        };
        self.record_source_returning_function_helper(id.name.as_str(), &function.params, body);
    }

    fn record_source_returning_helper_from_variable_declarator(
        &mut self,
        decl: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        if decl.kind != VariableDeclarationKind::Const {
            self.source_returning_helpers.remove(id.name.as_str());
            return;
        }
        let helper = match init {
            Expression::ArrowFunctionExpression(arrow) => extract_arrow_return_expr(arrow)
                .and_then(|expr| source_returning_helper(&arrow.params, expr)),
            Expression::FunctionExpression(function) => function
                .body
                .as_deref()
                .and_then(extract_function_body_final_return_expr)
                .and_then(|expr| source_returning_helper(&function.params, expr)),
            _ => None,
        };
        if let Some(helper) = helper {
            self.source_returning_helpers
                .insert(id.name.to_string(), helper);
        } else {
            self.source_returning_helpers.remove(id.name.as_str());
        }
    }

    fn record_dompurify_import_binding(&mut self, source: &str, local: &str, is_type_only: bool) {
        if !is_type_only && self.is_module_scope() && is_dompurify_source(source) {
            self.dompurify_bindings.insert(local.to_string());
        }
    }

    fn record_dompurify_require_binding(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        source: &str,
    ) {
        if !self.is_module_scope() || !is_dompurify_source(source) {
            return;
        }
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            self.dompurify_bindings.insert(id.name.to_string());
        }
    }

    fn sanitizer_scope_for_expr(&self, expr: &Expression<'_>) -> Option<SanitizerScope> {
        match unwrap_parens(expr) {
            Expression::Identifier(ident) => self.sanitizer_scope_for_identifier(&ident.name),
            Expression::AwaitExpression(await_expr) => {
                self.sanitizer_scope_for_expr(&await_expr.argument)
            }
            Expression::CallExpression(call) if self.is_dompurify_sanitize_call(call) => {
                Some(SanitizerScope::Html)
            }
            Expression::ObjectExpression(obj) => self.sanitizer_scope_for_object(obj),
            _ => None,
        }
    }

    fn sanitizer_scope_for_object(&self, obj: &ObjectExpression<'_>) -> Option<SanitizerScope> {
        obj.properties.iter().find_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return None;
            };
            if prop.key.static_name().is_none_or(|name| name != "__html") {
                return None;
            }
            self.sanitizer_scope_for_expr(&prop.value)
        })
    }

    fn is_dompurify_sanitize_call(&self, call: &CallExpression<'_>) -> bool {
        let Some(callee_path) = flatten_callee_path(&call.callee) else {
            return false;
        };
        let Some((object, method)) = callee_path.rsplit_once('.') else {
            return false;
        };
        method == "sanitize"
            && self.dompurify_bindings.contains(object)
            && !self.nested_scope_shadows(object)
    }

    fn record_sanitized_sink_arg(
        &mut self,
        span_start: u32,
        arg_index: u32,
        expr: &Expression<'_>,
    ) {
        let Some(scope) = self.sanitizer_scope_for_expr(expr) else {
            return;
        };
        self.sanitized_sink_args.push(SanitizedSinkArg {
            span_start,
            arg_index,
            scope,
        });
    }

    fn record_guarded_path_sink_arg(&mut self, local: &str) {
        let Some(binding) = self.path_sink_binding(local) else {
            return;
        };
        self.sanitized_sink_args.push(SanitizedSinkArg {
            span_start: binding.span_start,
            arg_index: binding.arg_index,
            scope: SanitizerScope::Path,
        });
    }

    fn record_fail_closed_guard_after_statement(&mut self, stmt: &Statement<'_>) {
        let Statement::IfStatement(if_stmt) = stmt else {
            return;
        };
        if if_stmt.alternate.is_some() || !statement_exits_current_flow(&if_stmt.consequent) {
            return;
        }
        if let Some(target) = self.url_allowlist_guard_target(&if_stmt.test) {
            self.record_sanitizer_binding(&target, Some(SanitizerScope::Url));
        }
        if let Some(target) = self.path_containment_guard_target(&if_stmt.test) {
            self.record_sanitizer_binding(&target, Some(SanitizerScope::Path));
            self.record_guarded_path_sink_arg(&target);
        }
    }

    fn url_allowlist_guard_target(&self, expr: &Expression<'_>) -> Option<String> {
        let Expression::UnaryExpression(unary) = unwrap_parens(expr) else {
            return None;
        };
        if unary.operator != UnaryOperator::LogicalNot {
            return None;
        }
        let Expression::CallExpression(call) = unwrap_parens(&unary.argument) else {
            return None;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        if !matches!(member.property.name.as_str(), "has" | "includes") {
            return None;
        }
        let Expression::Identifier(allowlist) = &member.object else {
            return None;
        };
        if !self.literal_allowlist_binding(&allowlist.name) {
            return None;
        }
        let Some(Argument::Identifier(target)) = call.arguments.first() else {
            return None;
        };
        Some(target.name.to_string())
    }

    fn path_containment_guard_target(&self, expr: &Expression<'_>) -> Option<String> {
        let Expression::LogicalExpression(logical) = unwrap_parens(expr) else {
            return None;
        };
        if logical.operator != LogicalOperator::Or {
            return None;
        }
        let left = path_relative_starts_with_parent(&logical.left)
            .or_else(|| self.path_is_absolute_relative_arg(&logical.left));
        let right = path_relative_starts_with_parent(&logical.right)
            .or_else(|| self.path_is_absolute_relative_arg(&logical.right));
        let (Some(left), Some(right)) = (left, right) else {
            return None;
        };
        if left != right {
            return None;
        }
        self.path_relative_binding(left).map(str::to_string)
    }

    fn path_is_absolute_relative_arg<'b>(&self, expr: &'b Expression<'_>) -> Option<&'b str> {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return None;
        };
        if !self.is_node_path_method_call(call, "isAbsolute") {
            return None;
        }
        let Some(Argument::Identifier(rel)) = call.arguments.first() else {
            return None;
        };
        Some(rel.name.as_str())
    }

    fn is_node_path_method_call(&self, call: &CallExpression<'_>, method: &str) -> bool {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        if member.property.name != method {
            return false;
        }
        let Expression::Identifier(object) = &member.object else {
            return false;
        };
        self.node_path_namespace_bindings
            .contains(object.name.as_str())
            && !self.nested_scope_shadows(object.name.as_str())
    }

    fn path_sink_binding_for_expr(&self, expr: &Expression<'_>) -> Option<SecurityPathSinkBinding> {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return None;
        };
        if !["join", "normalize", "resolve"]
            .iter()
            .any(|method| self.is_node_path_method_call(call, method))
        {
            return None;
        }
        Some(SecurityPathSinkBinding {
            span_start: call.span.start,
            arg_index: 0,
        })
    }

    fn path_relative_target_for_expr(&self, expr: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return None;
        };
        if !self.is_node_path_method_call(call, "relative") {
            return None;
        }
        let Some(Argument::Identifier(target)) = call.arguments.get(1) else {
            return None;
        };
        Some(target.name.to_string())
    }

    /// Record tainted-source bindings for `const { a, b } = <object>.<prop>`,
    /// where the destructured initializer is a member-access chain (or bare
    /// identifier root). Each bound local maps to the FULL flattened init path:
    /// `const { id } = req.query` records `{ local: "id", source_path:
    /// "req.query" }`. Rest patterns are skipped (whole-object capture is out of
    /// the cheap scope). Nested patterns are not destructured.
    fn record_tainted_destructure_bindings(
        &mut self,
        obj_pat: &ObjectPattern<'_>,
        expr: &Expression<'_>,
    ) {
        let Some(source_path) = destructure_source_path(expr) else {
            return;
        };
        for local in super::extract_destructured_names(obj_pat) {
            self.tainted_bindings.push(TaintedBinding {
                local,
                source_path: source_path.clone(),
            });
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
        super::helpers::extract_angular_inject_target(call, &|local_name, source, imported_name| {
            self.is_named_import_from(local_name, source, imported_name)
        })
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

    fn record_static_package_values(&mut self, name: &str, init: &Expression<'_>) {
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

    fn try_record_package_path_reference(&mut self, call: &CallExpression<'_>) {
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

    fn static_package_loop_bindings(
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

    fn record_initialized_declarator_bindings(
        &mut self,
        decl: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            if decl.kind == VariableDeclarationKind::Const && self.is_module_scope() {
                self.record_static_package_values(id.name.as_str(), init);
            }

            let sources = self.node_module_register_sources_from_expression(init);
            if !sources.is_empty() {
                self.record_node_module_register_url_binding(id.name.to_string(), sources);
            }
            self.record_current_module_file_path_binding(id.name.as_str(), init);
            self.record_injection_token(id.name.as_str(), init);
            self.record_child_process_fork_target_binding(id.name.as_str(), init);
            self.record_tainted_source_binding(id.name.as_str(), init);
            self.record_tainted_helper_call_binding(id.name.as_str(), init);
            let sanitizer_scope = self.sanitizer_scope_for_expr(init);
            self.record_sanitizer_binding(id.name.as_str(), sanitizer_scope);
            let allowlist = decl.kind == VariableDeclarationKind::Const
                && is_literal_string_allowlist_expr(init);
            self.record_literal_allowlist_binding(id.name.as_str(), allowlist);
            self.record_path_sink_binding(id.name.as_str(), self.path_sink_binding_for_expr(init));
            self.record_path_relative_binding(
                id.name.as_str(),
                self.path_relative_target_for_expr(init),
            );
        } else {
            for id in declarator.id.get_binding_identifiers() {
                self.record_sanitizer_binding(id.name.as_str(), None);
                self.record_literal_allowlist_binding(id.name.as_str(), false);
                self.record_path_sink_binding(id.name.as_str(), None);
                self.record_path_relative_binding(id.name.as_str(), None);
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

    /// Capture helper-function Playwright fixtures or aliases from returns.
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
    fn visit_program(&mut self, program: &Program<'a>) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    self.record_source_returning_function_declaration(function);
                }
                Statement::ExportNamedDeclaration(export)
                    if export.source.is_none()
                        && matches!(
                            export.declaration,
                            Some(Declaration::FunctionDeclaration(_))
                        ) =>
                {
                    if let Some(Declaration::FunctionDeclaration(function)) = &export.declaration {
                        self.record_source_returning_function_declaration(function);
                    }
                }
                _ => {}
            }
        }

        // Capture file-level string directives (`"use client"`, `"use server"`)
        // for the security client-server-leak detector. `directive.directive` is
        // the cooked directive text without surrounding quotes.
        for directive in &program.directives {
            self.directives
                .push(directive.directive.as_str().to_string());
        }
        walk::walk_program(self, program);
    }

    fn visit_formal_parameter(&mut self, param: &FormalParameter<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &param.pattern
            && let Some(type_annotation) = param.type_annotation.as_deref()
        {
            self.record_typed_binding(id.name.as_str(), type_annotation);
            if param.accessibility.is_some() {
                self.record_typed_binding(format!("this.{}", id.name).as_str(), type_annotation);
            }
        }

        if let BindingPattern::ObjectPattern(obj_pat) = &param.pattern
            && let Some(type_annotation) = param.type_annotation.as_deref()
        {
            self.record_typed_destructure_binding(obj_pat, type_annotation);
        }

        walk::walk_formal_parameter(self, param);
    }

    fn visit_property_definition(&mut self, prop: &PropertyDefinition<'a>) {
        if let Some(name) = prop.key.static_name() {
            if let Some(type_annotation) = prop.type_annotation.as_deref() {
                self.record_typed_binding(format!("this.{name}").as_str(), type_annotation);

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

            if let Some(value) = prop.value.as_ref() {
                self.capture_hardcoded_secret_literal_sink(name.as_ref(), value, prop.span);
            }
        }

        walk::walk_property_definition(self, prop);
    }

    fn visit_block_statement(&mut self, stmt: &BlockStatement<'a>) {
        self.block_depth += 1;
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.push(FxHashSet::default());
            self.sanitizer_binding_stack.push(FxHashMap::default());
            self.literal_allowlist_binding_stack
                .push(FxHashMap::default());
            self.path_sink_binding_stack.push(FxHashMap::default());
            self.path_relative_binding_stack.push(FxHashMap::default());
        }
        for statement in &stmt.body {
            self.visit_statement(statement);
            if self.namespace_depth == 0 {
                self.record_fail_closed_guard_after_statement(statement);
            }
        }
        if self.namespace_depth == 0 {
            self.nested_declaration_stack.pop();
            self.sanitizer_binding_stack.pop();
            self.literal_allowlist_binding_stack.pop();
            self.path_sink_binding_stack.pop();
            self.path_relative_binding_stack.pop();
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
                        self.record_sanitizer_binding(id.name.as_str(), None);
                        self.record_literal_allowlist_binding(id.name.as_str(), false);
                        self.record_path_sink_binding(id.name.as_str(), None);
                        self.record_path_relative_binding(id.name.as_str(), None);
                        self.record_local_type_declaration(&id.name, id.span);
                        let is_angular = has_angular_class_decorator(class);
                        let instance_bindings = super::helpers::extract_class_instance_bindings(
                            class,
                            |local_name, source, imported_name| {
                                self.is_named_import_from(local_name, source, imported_name)
                            },
                        );
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
                        self.record_sanitizer_binding(id.name.as_str(), None);
                        self.record_literal_allowlist_binding(id.name.as_str(), false);
                        self.record_path_sink_binding(id.name.as_str(), None);
                        self.record_path_relative_binding(id.name.as_str(), None);
                        let refs = Self::collect_function_signature_refs(function);
                        self.record_local_signature_refs(&id.name, refs);
                        self.record_local_structural_function(
                            id.name.as_str(),
                            &function.params,
                            function.body.as_deref(),
                        );
                        self.record_source_returning_function_declaration(function);
                        if let Some(body) = function.body.as_deref()
                            && let Some(arg_index) = package_resolution_arg_index(
                                &function.params,
                                body,
                                &self.package_resolution_function_args,
                            )
                        {
                            self.package_resolution_function_args
                                .insert(id.name.to_string(), arg_index);
                        }

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
                    if let TSType::TSTypeLiteral(type_lit) = &alias.type_annotation {
                        let properties = collect_object_type_property_types(&type_lit.members);
                        if !properties.is_empty() {
                            self.interface_property_types
                                .insert(alias.id.name.to_string(), properties);
                        }
                    }
                }
                Declaration::TSInterfaceDeclaration(iface) => {
                    self.record_local_declaration_name(&iface.id.name);
                    self.record_local_type_declaration(&iface.id.name, iface.id.span);
                    let refs = Self::collect_interface_signature_refs(iface);
                    self.record_local_signature_refs(&iface.id.name, refs);
                    let properties = collect_object_type_property_types(&iface.body.body);
                    if !properties.is_empty() {
                        self.interface_property_types
                            .insert(iface.id.name.to_string(), properties);
                    }
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
                        self.record_sanitizer_binding(id.name.as_str(), None);
                        self.record_literal_allowlist_binding(id.name.as_str(), false);
                        self.record_path_sink_binding(id.name.as_str(), None);
                        self.record_path_relative_binding(id.name.as_str(), None);
                    }
                }
                Declaration::FunctionDeclaration(function) => {
                    if let Some(id) = function.id.as_ref() {
                        self.record_nested_declaration_names(std::iter::once(id));
                        self.record_sanitizer_binding(id.name.as_str(), None);
                        self.record_literal_allowlist_binding(id.name.as_str(), false);
                        self.record_path_sink_binding(id.name.as_str(), None);
                        self.record_path_relative_binding(id.name.as_str(), None);
                    }
                }
                _ => {}
            }
        }

        walk::walk_declaration(self, decl);
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        self.record_next_function_param_sources(func);
        self.push_function_declaration_scope(&func.params);
        self.function_depth += 1;
        walk::walk_function(self, func, flags);
        self.function_depth -= 1;
        self.pop_function_declaration_scope();
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        self.record_next_arrow_param_sources(expr);
        self.push_function_declaration_scope(&expr.params);
        self.function_depth += 1;
        walk::walk_arrow_function_expression(self, expr);
        self.function_depth -= 1;
        self.pop_function_declaration_scope();
    }

    fn visit_function_body(&mut self, body: &FunctionBody<'a>) {
        for statement in &body.statements {
            self.visit_statement(statement);
            if self.namespace_depth == 0 {
                self.record_fail_closed_guard_after_statement(statement);
            }
        }
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
                        self.record_dompurify_import_binding(
                            &source,
                            s.local.name.as_str(),
                            is_type_only,
                        );
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
                        self.record_dompurify_import_binding(&source, &local, is_type_only);
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
        let (members, super_class, implemented_interfaces, instance_bindings) =
            if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &decl.declaration {
                let is_angular = has_angular_class_decorator(class);
                let bindings = super::helpers::extract_class_instance_bindings(
                    class,
                    |local_name, source, imported_name| {
                        self.is_named_import_from(local_name, source, imported_name)
                    },
                );
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
                let first_quasi = tpl.quasis[0].value.raw.to_string();
                if first_quasi.starts_with("./") || first_quasi.starts_with("../") {
                    let prefix = if tpl.expressions.len() > 1 {
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

            if let BindingPattern::ObjectPattern(obj_pat) = &declarator.id
                && let Some(type_annotation) = declarator.type_annotation.as_deref()
            {
                self.record_typed_destructure_binding(obj_pat, type_annotation);
            }

            let Some(init) = &declarator.init else {
                for id in declarator.id.get_binding_identifiers() {
                    self.record_sanitizer_binding(id.name.as_str(), None);
                    self.record_literal_allowlist_binding(id.name.as_str(), false);
                    self.record_path_sink_binding(id.name.as_str(), None);
                    self.record_path_relative_binding(id.name.as_str(), None);
                }
                continue;
            };

            self.record_local_structural_function_from_variable_declarator(declarator, init);
            self.record_source_returning_helper_from_variable_declarator(decl, declarator, init);
            self.record_initialized_declarator_bindings(decl, declarator, init);
            if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                self.capture_math_random_context_sink(id.name.as_str(), init, declarator.span);
                self.capture_hardcoded_secret_literal_sink(id.name.as_str(), init, declarator.span);
            }

            if let BindingPattern::ObjectPattern(obj_pat) = &declarator.id {
                self.record_tainted_destructure_bindings(obj_pat, init);
            }

            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && let Expression::CallExpression(call) = init
            {
                self.record_playwright_fixture_definitions(id.name.as_str(), call);
            }

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

            if let Some((call, source)) = try_extract_require(init) {
                self.record_dompurify_require_binding(declarator, source);
                self.record_child_process_require_binding(declarator, source);
                self.handle_require_declaration(declarator, call, source);
                continue;
            }

            if let Expression::NewExpression(new_expr) = init
                && let Expression::Identifier(callee) = &new_expr.callee
                && let BindingPattern::BindingIdentifier(id) = &declarator.id
                && !super::helpers::is_builtin_constructor(callee.name.as_str())
            {
                self.binding_target_names
                    .insert(id.name.to_string(), callee.name.to_string());
            }

            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && let Some(class_name) = Self::svelte_derived_new_class(init)
            {
                self.binding_target_names
                    .insert(id.name.to_string(), class_name);
            }

            if let Expression::CallExpression(call) = init
                && let BindingPattern::ArrayPattern(arr_pat) = &declarator.id
                && let Some(Some(BindingPattern::BindingIdentifier(id))) = arr_pat.elements.first()
                && let Some(class_name) =
                    super::helpers::try_extract_factory_new_class(&call.arguments)
            {
                self.binding_target_names
                    .insert(id.name.to_string(), class_name);
            }

            // `const svc = useMemo(() => new Svc())`: useMemo returns the
            // factory's product directly, so the non-destructured binding is a
            // class instance. Scoped to useMemo (see `is_value_returning_memo_callee`)
            // so arbitrary wrappers and tuple-returning hooks like useState are
            // not over-credited. `or_insert` so a stronger pre-existing binding
            // wins. See issue #844.
            if let Expression::CallExpression(call) = init
                && let BindingPattern::BindingIdentifier(id) = &declarator.id
                && is_value_returning_memo_callee(&call.callee)
                && let Some(class_name) =
                    super::helpers::try_extract_factory_new_class(&call.arguments)
            {
                self.binding_target_names
                    .entry(id.name.to_string())
                    .or_insert(class_name);
            }

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

            if let Expression::Identifier(ident) = init
                && self
                    .namespace_binding_names
                    .iter()
                    .any(|n| n == ident.name.as_str())
            {
                self.handle_namespace_destructuring(declarator, &ident.name);
                continue;
            }

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

        if let Some(name) = prop.key.static_name() {
            self.capture_hardcoded_secret_literal_sink(name.as_ref(), &prop.value, prop.span);
        }

        walk::walk_object_property(self, prop);
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        self.record_structural_class_call_candidate(expr);
        self.clear_literal_allowlist_on_mutating_member_call(expr);
        self.record_framework_callback_param_sources(expr);

        if let Some(test_name) = playwright_test_callee_name(&expr.callee) {
            self.member_accesses
                .extend(collect_playwright_fixture_member_uses(
                    test_name.as_str(),
                    &expr.arguments,
                ));
        }

        if let Some((_tag, class_name)) = extract_custom_elements_define(expr) {
            self.side_effect_registered_class_names.insert(class_name);
        }

        self.bind_iterable_callback_parameter(expr);

        if let Some(target_source) = vitest_mock_source(expr) {
            self.dynamic_imports.push(DynamicImportInfo {
                source: target_source.clone(),
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
                is_speculative: false,
            });

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

        self.try_record_pino_transport_targets(expr);
        self.try_record_node_module_register(expr);
        self.try_record_child_process_fork(expr);
        self.try_record_package_path_reference(expr);

        if let Expression::Identifier(ident) = &expr.callee
            && ident.name == "require"
            && let Some(Argument::StringLiteral(lit)) = expr.arguments.first()
            && !self.handled_require_spans.contains(&expr.span)
        {
            self.require_calls.push(RequireCallInfo {
                source: lit.value.to_string(),
                span: expr.span,
                source_span: lit.span,
                destructured_names: Vec::new(),
                local_name: None,
            });
        }

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

        self.try_record_fluent_chain_access(expr);

        self.capture_call_sink(expr);

        walk::walk_call_expression(self, expr);
    }

    fn visit_for_of_statement(&mut self, stmt: &ForOfStatement<'a>) {
        if let Some((strings, objects)) = self.static_package_loop_bindings(stmt) {
            self.loop_string_bindings.push(strings);
            self.loop_object_property_values.push(objects);
            walk::walk_for_of_statement(self, stmt);
            self.loop_object_property_values.pop();
            self.loop_string_bindings.pop();
            return;
        }

        walk::walk_for_of_statement(self, stmt);
    }

    fn visit_new_expression(&mut self, expr: &oxc_ast::ast::NewExpression<'a>) {
        self.record_queue_worker_constructor_param_sources(expr);

        if let Some(source) = new_url_import_source(expr) {
            // A `new URL(specifier, import.meta.url)` whose specifier has no file
            // extension may refer to a directory rather than a module (e.g.
            // `new URL("./services", import.meta.url)` to obtain the directory URL
            // via `fileURLToPath(...)`). Such a specifier cannot be resolved to a
            // module, so marking it speculative causes the resolver to silently drop
            // it when the target is unresolvable. Specifiers with an extension
            // (e.g. `./worker.js`) keep `is_speculative = false` so genuinely
            // missing files are still reported as `unresolved-import`.
            // See issue #840.
            let is_speculative = PathBuf::from(&source).extension().is_none();
            self.dynamic_imports.push(DynamicImportInfo {
                source,
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
                is_speculative,
            });
        }

        self.capture_new_expression_sink(expr);

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
        if let Some(name) = assignment_target_security_context_name(&expr.left) {
            self.capture_math_random_context_sink(name.as_str(), &expr.right, expr.span);
            self.capture_hardcoded_secret_literal_sink(name.as_str(), &expr.right, expr.span);
        }

        if let Some(name) = assignment_target_identifier_name(&expr.left) {
            self.record_sanitizer_binding(name, None);
            self.record_literal_allowlist_binding(name, false);
            self.record_path_sink_binding(name, None);
            self.record_path_relative_binding(name, None);
        } else if let Some(name) = assignment_target_member_object_name(&expr.left)
            && self.literal_allowlist_binding(name)
        {
            self.record_literal_allowlist_binding(name, false);
        }

        if let AssignmentTarget::StaticMemberExpression(member) = &expr.left {
            if let Expression::Identifier(obj) = &member.object {
                if obj.name == "module" && member.property.name == "exports" {
                    self.has_cjs_exports = true;
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
            if matches!(member.object, Expression::ThisExpression(_)) {
                self.member_accesses.push(MemberAccess {
                    object: "this".to_string(),
                    member: member.property.name.to_string(),
                });
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
        self.capture_member_assign_sink(expr);
        walk::walk_assignment_expression(self, expr);
    }

    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        if is_import_meta_env_object(&expr.object) {
            self.member_accesses.push(MemberAccess {
                object: "import.meta.env".to_string(),
                member: expr.property.name.to_string(),
            });
        }
        if let Some(object_name) = static_member_object_name(&expr.object) {
            self.member_accesses.push(MemberAccess {
                object: object_name,
                member: expr.property.name.to_string(),
            });
        }
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
                self.member_accesses.push(MemberAccess {
                    object: obj.name.to_string(),
                    member: lit.value.to_string(),
                });
            } else {
                self.whole_object_uses.push(obj.name.to_string());
            }
        }
        walk::walk_computed_member_expression(self, expr);
    }

    fn visit_ts_qualified_name(&mut self, it: &TSQualifiedName<'a>) {
        if let TSTypeName::IdentifierReference(obj) = &it.left {
            self.member_accesses.push(MemberAccess {
                object: obj.name.to_string(),
                member: it.right.name.to_string(),
            });
        }
        walk::walk_ts_qualified_name(self, it);
    }

    fn visit_ts_mapped_type(&mut self, it: &TSMappedType<'a>) {
        if let TSType::TSTypeReference(type_ref) = &it.constraint
            && let TSTypeName::IdentifierReference(ident) = &type_ref.type_name
        {
            self.whole_object_uses.push(ident.name.to_string());
        }
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

    fn visit_if_statement(&mut self, stmt: &IfStatement<'a>) {
        // Record `x instanceof ClassName` narrowings from the test condition so
        // that method calls on `x` inside the body (e.g. `x.getMessage()`) are
        // credited as uses of `ClassName.getMessage`, preventing false
        // unused-class-member findings. The bindings are module-scoped (not
        // strictly block-scoped), which is conservative: it may credit accesses
        // outside the guard, but that produces at most false negatives, not false
        // positives.
        let mut narrowings = Vec::new();
        collect_instanceof_narrowings(&stmt.test, &mut narrowings);
        for (local, class_name) in narrowings {
            self.binding_target_names.entry(local).or_insert(class_name);
        }
        walk::walk_if_statement(self, stmt);
    }

    fn visit_spread_element(&mut self, elem: &SpreadElement<'a>) {
        if let Expression::Identifier(ident) = &elem.argument {
            self.whole_object_uses.push(ident.name.to_string());
        }
        walk::walk_spread_element(self, elem);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
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
                let export_index = self.exports.len() - 1;
                self.record_lit_custom_element_candidate(
                    decorator,
                    SideEffectRegistrationTarget::AnonymousDefaultExport(export_index),
                );
            }
        }

        if let Some(meta) = extract_angular_component_metadata(class) {
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

            if let Some(ref template) = meta.inline_template {
                let refs = crate::sfc_template::angular::collect_angular_template_refs(template);
                for name in refs.identifiers {
                    self.member_accesses.push(MemberAccess {
                        object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                        member: name,
                    });
                }
                self.member_accesses.extend(refs.member_accesses);
                let template_offset = meta
                    .inline_template_offset
                    .unwrap_or(meta.decorator_span.start);
                self.security_sinks
                    .extend(refs.security_sinks.into_iter().map(|mut sink| {
                        sink.span_start = sink.span_start.saturating_add(template_offset);
                        sink.span_end = sink.span_end.saturating_add(template_offset);
                        sink
                    }));

                self.inline_template_findings
                    .push(super::InlineTemplateFinding {
                        template_source: template.clone(),
                        decorator_start: meta.decorator_span.start,
                    });
            }

            for name in &meta.host_member_refs {
                self.member_accesses.push(MemberAccess {
                    object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                    member: name.clone(),
                });
            }

            for name in &meta.input_output_members {
                self.member_accesses.push(MemberAccess {
                    object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                    member: name.clone(),
                });
            }
        }
        self.class_super_stack
            .push(super::helpers::extract_super_class_name(class));
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
        self.capture_tagged_template_sink(expr);
        walk::walk_tagged_template_expression(self, expr);
    }

    fn visit_jsx_attribute(&mut self, attr: &oxc_ast::ast::JSXAttribute<'a>) {
        self.capture_jsx_attr_sink(attr);
        walk::walk_jsx_attribute(self, attr);
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

fn assignment_target_identifier_name<'b>(target: &'b AssignmentTarget<'_>) -> Option<&'b str> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(ident) => Some(ident.name.as_str()),
        AssignmentTarget::TSAsExpression(ts_as) => expression_identifier_name(&ts_as.expression),
        AssignmentTarget::TSSatisfiesExpression(ts_sat) => {
            expression_identifier_name(&ts_sat.expression)
        }
        AssignmentTarget::TSNonNullExpression(ts_non_null) => {
            expression_identifier_name(&ts_non_null.expression)
        }
        AssignmentTarget::TSTypeAssertion(ts_assertion) => {
            expression_identifier_name(&ts_assertion.expression)
        }
        _ => None,
    }
}

fn expression_identifier_name<'b>(expr: &'b Expression<'_>) -> Option<&'b str> {
    match expr {
        Expression::Identifier(ident) => Some(ident.name.as_str()),
        Expression::ParenthesizedExpression(paren) => expression_identifier_name(&paren.expression),
        Expression::TSAsExpression(ts_as) => expression_identifier_name(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => expression_identifier_name(&ts_sat.expression),
        Expression::TSNonNullExpression(ts_non_null) => {
            expression_identifier_name(&ts_non_null.expression)
        }
        Expression::TSTypeAssertion(ts_assertion) => {
            expression_identifier_name(&ts_assertion.expression)
        }
        _ => None,
    }
}

fn assignment_target_member_object_name<'b>(target: &'b AssignmentTarget<'_>) -> Option<&'b str> {
    match target {
        AssignmentTarget::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) => Some(object.name.as_str()),
            _ => None,
        },
        AssignmentTarget::ComputedMemberExpression(member) => match &member.object {
            Expression::Identifier(object) => Some(object.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn statement_exits_current_flow(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => {
            block.body.first().is_some_and(statement_exits_current_flow)
        }
        _ => false,
    }
}

fn path_relative_starts_with_parent<'b>(expr: &'b Expression<'_>) -> Option<&'b str> {
    let Expression::CallExpression(call) = unwrap_parens(expr) else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if member.property.name != "startsWith" {
        return None;
    }
    let Expression::Identifier(relative) = &member.object else {
        return None;
    };
    let Some(Argument::StringLiteral(prefix)) = call.arguments.first() else {
        return None;
    };
    if prefix.value.as_str() != ".." {
        return None;
    }
    Some(relative.name.as_str())
}

fn is_literal_string_allowlist_expr(expr: &Expression<'_>) -> bool {
    match unwrap_static_expr(expr) {
        Expression::ArrayExpression(array) => is_string_literal_array(array),
        Expression::NewExpression(new_expr) => {
            let Expression::Identifier(callee) = &new_expr.callee else {
                return false;
            };
            if callee.name != "Set" {
                return false;
            }
            let Some(Argument::ArrayExpression(array)) = new_expr.arguments.first() else {
                return false;
            };
            is_string_literal_array(array)
        }
        _ => false,
    }
}

fn is_string_literal_array(array: &ArrayExpression<'_>) -> bool {
    array
        .elements
        .iter()
        .all(|element| matches!(element, ArrayExpressionElement::StringLiteral(_)))
}

fn unwrap_static_expr<'a, 'b>(mut expr: &'b Expression<'a>) -> &'b Expression<'a> {
    loop {
        match expr {
            Expression::ParenthesizedExpression(paren) => expr = &paren.expression,
            Expression::TSAsExpression(ts_as) => expr = &ts_as.expression,
            Expression::TSSatisfiesExpression(ts_sat) => expr = &ts_sat.expression,
            Expression::TSNonNullExpression(ts_non_null) => expr = &ts_non_null.expression,
            _ => return expr,
        }
    }
}

/// Recursively unwrap parenthesized expressions to reach the inner expression.
fn unwrap_parens<'a, 'b>(mut expr: &'b Expression<'a>) -> &'b Expression<'a> {
    while let Expression::ParenthesizedExpression(paren) = expr {
        expr = &paren.expression;
    }
    expr
}

/// Flatten an `Identifier` or `StaticMemberExpression` callee to a dotted path.
///
/// Deliberately narrower than `static_member_object_name`: it accepts ONLY bare
/// identifiers and static member chains (no call/new forms), so the catalogue
/// matcher sees a clean dotted callee path. Returns `None` for dynamic dispatch,
/// computed members, or aliased call forms (those count as blind spots).
fn flatten_callee_path(expr: &Expression<'_>) -> Option<String> {
    match unwrap_parens(expr) {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            flatten_callee_path(&member.object)?,
            member.property.name
        )),
        _ => None,
    }
}

/// Flatten an expression to a dotted member path, unwrapping `await` and parens.
/// Returns `None` for anything that is not an identifier-rooted static-member
/// chain (call results, computed members, etc. are not flattened: a conservative
/// miss, never a wrong source link).
fn flatten_member_path(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::ParenthesizedExpression(paren) => flatten_member_path(&paren.expression),
        Expression::AwaitExpression(await_expr) => flatten_member_path(&await_expr.argument),
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            flatten_member_path(&member.object)?,
            member.property.name
        )),
        _ => None,
    }
}

/// The source path for a DIRECT binding (`const id = req.query.id`): the OBJECT
/// path of the member-access init, i.e. the chain with its final property
/// dropped (`req.query`). A bare-identifier init (`const x = req`) has no object
/// to drop and is not a source binding on its own (`None`).
fn tainted_source_path(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::ParenthesizedExpression(paren) => tainted_source_path(&paren.expression),
        Expression::AwaitExpression(await_expr) => tainted_source_path(&await_expr.argument),
        Expression::StaticMemberExpression(member) => flatten_member_path(&member.object),
        _ => None,
    }
}

fn push_unique_string(out: &mut Vec<String>, value: String) {
    if !out.iter().any(|existing| existing == &value) {
        out.push(value);
    }
}

fn source_path_candidates(expr: &Expression<'_>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(path) = flatten_member_path(expr) {
        push_unique_string(&mut out, path);
    }
    if let Some(path) = tainted_source_path(expr) {
        push_unique_string(&mut out, path);
    }
    out
}

fn source_returning_helper(
    params: &FormalParameters<'_>,
    expr: &Expression<'_>,
) -> Option<super::SourceReturningHelper> {
    let param_names = params
        .items
        .iter()
        .map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut paths = Vec::new();
    for source_path in source_path_candidates(expr) {
        for (arg_index, param_name) in param_names.iter().enumerate() {
            let Some(param_name) = param_name else {
                continue;
            };
            if source_path == *param_name {
                paths.push(super::SourceReturnPath {
                    arg_index,
                    suffixes: vec![String::new()],
                });
                break;
            }
            let Some(suffix) = source_path.strip_prefix(&format!("{param_name}.")) else {
                continue;
            };
            paths.push(super::SourceReturnPath {
                arg_index,
                suffixes: vec![suffix.to_string()],
            });
            break;
        }
    }
    if paths.is_empty() {
        None
    } else {
        Some(super::SourceReturningHelper { paths })
    }
}

fn apply_source_return_path(expr: &Expression<'_>, suffixes: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if suffixes.iter().any(String::is_empty) {
        for candidate in source_path_candidates(expr) {
            push_unique_string(&mut out, candidate);
        }
    }
    let Some(base) = flatten_member_path(expr) else {
        return out;
    };
    for suffix in suffixes {
        if suffix.is_empty() {
            push_unique_string(&mut out, base.clone());
        } else {
            push_unique_string(&mut out, format!("{base}.{suffix}"));
        }
    }
    out
}

/// The source path for a DESTRUCTURE binding (`const { id } = req.query`): the
/// FULL flattened init path (`req.query`), since the destructured keys are the
/// leaves. A bare-identifier init (`const { id } = req`) yields `req`.
fn destructure_source_path(expr: &Expression<'_>) -> Option<String> {
    flatten_member_path(expr)
}

/// Whether an expression is a non-literal argument (a conservative trigger for
/// sink capture). A fully-literal argument is never captured.
fn is_non_literal_arg(expr: &Expression<'_>) -> bool {
    match unwrap_static_expr(expr) {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_) => false,
        Expression::TemplateLiteral(tpl) => !tpl.expressions.is_empty(),
        _ => true,
    }
}

/// Classify a captured non-literal argument into a finer-grained [`SinkArgKind`]
/// so the catalogue can require unsafe shapes (concat, template-with-substitution)
/// and exclude safe ones (object literal, the parameterized form). Static
/// TypeScript wrappers are unwrapped first; a `+` `BinaryExpression` is treated
/// as a string concatenation (a numeric `+` reaching a SQL/HTML sink is already
/// noise-free because the catalogue gates on the sink shape and callee).
fn classify_arg_kind(expr: &Expression<'_>) -> SinkArgKind {
    match unwrap_static_expr(expr) {
        Expression::TemplateLiteral(_) => SinkArgKind::TemplateWithSubst,
        Expression::BinaryExpression(bin)
            if bin.operator == oxc_ast::ast::BinaryOperator::Addition =>
        {
            SinkArgKind::Concat
        }
        Expression::ObjectExpression(_) => SinkArgKind::Object,
        Expression::CallExpression(_) => SinkArgKind::Call,
        _ => SinkArgKind::Other,
    }
}

fn sink_literal_value(expr: &Expression<'_>) -> Option<SinkLiteralValue> {
    match unwrap_static_expr(expr) {
        Expression::StringLiteral(lit) => Some(SinkLiteralValue::String(lit.value.to_string())),
        Expression::BooleanLiteral(lit) => Some(SinkLiteralValue::Boolean(lit.value)),
        Expression::NullLiteral(_) => Some(SinkLiteralValue::Null),
        _ => None,
    }
}

fn static_string_literal_value(expr: &Expression<'_>) -> Option<String> {
    match unwrap_static_expr(expr) {
        Expression::StringLiteral(lit) => Some(lit.value.to_string()),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() && tpl.quasis.len() == 1 => {
            tpl.quasis
                .first()
                .and_then(|quasi| quasi.value.cooked.as_ref())
                .map(ToString::to_string)
        }
        _ => None,
    }
}

fn should_capture_hardcoded_secret_literal(context_name: &str, value: &str) -> bool {
    is_secret_shaped_context_name(context_name) || has_provider_prefix_capture_hint(value)
}

fn is_secret_shaped_context_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "apikey",
        "api_key",
        "accesskey",
        "access_key",
        "privatekey",
        "private_key",
        "clientsecret",
        "client_secret",
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "jwt",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn has_provider_prefix_capture_hint(value: &str) -> bool {
    value.starts_with("AKIA")
        || value.starts_with("ASIA")
        || value.starts_with("ghp_")
        || value.starts_with("gho_")
        || value.starts_with("ghu_")
        || value.starts_with("ghs_")
        || value.starts_with("ghr_")
        || value.starts_with("github_pat_")
        || value.starts_with("glpat-")
        || value.starts_with("xoxb-")
        || value.starts_with("xoxp-")
        || value.starts_with("xoxa-")
        || value.starts_with("xoxr-")
        || value.starts_with("xoxs-")
        || value.starts_with("sk_live_")
        || value.starts_with("rk_live_")
        || value.starts_with("sk-ant-")
        || value.starts_with("sk-proj-")
        || value.starts_with("AIza")
        || value.starts_with("SG.")
        || value.starts_with("npm_")
        || value.starts_with("pypi-")
        || value.starts_with("sq0atp-")
        || value.starts_with("shpat_")
        || value.starts_with("shpss_")
        || value.starts_with("shpca_")
        || value.starts_with("shppa_")
        || value.starts_with("dp.pt.")
        || value.starts_with("doo_v1_")
        || value.starts_with("dop_v1_")
        || value.starts_with("dor_v1_")
        || value.starts_with("dot_v1_")
        || value.starts_with("dapi")
        || value.starts_with("lin_api_")
        || value.starts_with("PMAK-")
        || value.starts_with("hf_")
        || value.starts_with("AGE-SECRET-KEY-1")
        || value.contains("-----BEGIN")
}

fn object_literal_properties(expr: &Expression<'_>) -> Vec<SinkObjectProperty> {
    let Expression::ObjectExpression(obj) = unwrap_static_expr(expr) else {
        return Vec::new();
    };
    obj.properties
        .iter()
        .filter_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return None;
            };
            let key = prop.key.static_name()?.to_string();
            let value = sink_literal_value(&prop.value)?;
            Some(SinkObjectProperty { key, value })
        })
        .collect()
}

struct ObjectKeyMetadata {
    keys: Vec<String>,
    complete: bool,
}

fn object_key_metadata(expr: &Expression<'_>) -> ObjectKeyMetadata {
    let Expression::ObjectExpression(obj) = unwrap_static_expr(expr) else {
        return ObjectKeyMetadata {
            keys: Vec::new(),
            complete: false,
        };
    };
    let mut keys = Vec::new();
    let mut complete = true;
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            complete = false;
            continue;
        };
        let Some(key) = prop.key.static_name() else {
            complete = false;
            continue;
        };
        let key = key.to_string();
        if !keys.iter().any(|existing| existing == &key) {
            keys.push(key);
        }
    }
    ObjectKeyMetadata { keys, complete }
}

fn should_capture_literal_sink_arg(
    callee_path: &str,
    sink_shape: SinkShape,
    arg_index: u32,
    expr: &Expression<'_>,
) -> bool {
    let Some(literal) = sink_literal_value(expr) else {
        return false;
    };
    let SinkLiteralValue::String(value) = literal else {
        return false;
    };
    match sink_shape {
        SinkShape::Call | SinkShape::MemberCall => {
            (arg_index == 1 && is_post_message_callee(callee_path) && value == "*")
                || (arg_index == 0 && is_weak_crypto_literal_callee(callee_path))
                || (arg_index == 0 && is_string_code_callee(callee_path))
                || (arg_index == 0
                    && is_cleartext_transport_literal_callee(callee_path)
                    && is_cleartext_transport_literal(&value))
                || (arg_index == 0
                    && is_literal_metadata_url_callee(callee_path)
                    && is_metadata_service_literal(&value))
        }
        SinkShape::NewExpression => {
            arg_index == 0
                && (callee_path == "Function"
                    || (callee_path == "WebSocket" && is_cleartext_websocket_literal(&value)))
        }
        SinkShape::MemberAssign => {
            arg_index == 0
                && callee_path == "process.env.NODE_TLS_REJECT_UNAUTHORIZED"
                && value == "0"
        }
        SinkShape::TaggedTemplate | SinkShape::JsxAttr | SinkShape::SecretLiteral => false,
    }
}

fn is_post_message_callee(callee_path: &str) -> bool {
    callee_path == "postMessage" || callee_path.ends_with(".postMessage")
}

fn is_weak_crypto_literal_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "createHash"
            | "createCipher"
            | "createDecipher"
            | "createCipheriv"
            | "createDecipheriv"
            | "crypto.createHash"
            | "crypto.createCipher"
            | "crypto.createDecipher"
            | "crypto.createCipheriv"
            | "crypto.createDecipheriv"
    )
}

fn is_string_code_callee(callee_path: &str) -> bool {
    matches!(callee_path, "setTimeout" | "setInterval")
}

fn is_cleartext_transport_literal_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "fetch"
            | "axios.get"
            | "axios.post"
            | "got"
            | "ky"
            | "needle"
            | "request"
            | "http.request"
            | "http.get"
            | "superagent.get"
            | "undici.request"
    )
}

fn is_cleartext_transport_literal(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("ftp://")
}

fn is_cleartext_websocket_literal(value: &str) -> bool {
    value.to_ascii_lowercase().starts_with("ws://")
}

fn is_literal_metadata_url_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "fetch"
            | "axios.get"
            | "axios.post"
            | "got"
            | "ky"
            | "needle"
            | "request"
            | "http.request"
            | "https.request"
            | "undici.request"
    )
}

fn is_metadata_service_literal(value: &str) -> bool {
    value.contains("169.254.169.254") || value.contains("metadata.google.internal")
}

fn should_capture_missing_jwt_verify_options(
    callee_path: &str,
    sink_shape: SinkShape,
    arg_len: usize,
) -> bool {
    arg_len == 2
        && matches!(sink_shape, SinkShape::Call | SinkShape::MemberCall)
        && (callee_path == "verify" || callee_path.ends_with(".verify"))
}

fn is_token_like_security_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "token",
        "secret",
        "session",
        "jwt",
        "auth",
        "csrf",
        "nonce",
        "salt",
        "password",
        "credential",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn expression_contains_math_random_call(expr: &Expression<'_>) -> bool {
    match unwrap_parens(expr) {
        Expression::CallExpression(call) => {
            if call.arguments.is_empty()
                && flatten_callee_path(&call.callee).as_deref() == Some("Math.random")
            {
                return true;
            }
            expression_callee_contains_math_random(&call.callee)
                || call
                    .arguments
                    .iter()
                    .filter_map(Argument::as_expression)
                    .any(expression_contains_math_random_call)
        }
        Expression::BinaryExpression(bin) => {
            expression_contains_math_random_call(&bin.left)
                || expression_contains_math_random_call(&bin.right)
        }
        Expression::LogicalExpression(logical) => {
            expression_contains_math_random_call(&logical.left)
                || expression_contains_math_random_call(&logical.right)
        }
        Expression::ConditionalExpression(cond) => {
            expression_contains_math_random_call(&cond.test)
                || expression_contains_math_random_call(&cond.consequent)
                || expression_contains_math_random_call(&cond.alternate)
        }
        Expression::TemplateLiteral(tpl) => tpl
            .expressions
            .iter()
            .any(expression_contains_math_random_call),
        Expression::ArrayExpression(array) => array.elements.iter().any(|element| {
            element
                .as_expression()
                .is_some_and(expression_contains_math_random_call)
        }),
        Expression::ObjectExpression(obj) => obj.properties.iter().any(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return false;
            };
            expression_contains_math_random_call(&prop.value)
        }),
        Expression::ParenthesizedExpression(paren) => {
            expression_contains_math_random_call(&paren.expression)
        }
        Expression::StaticMemberExpression(member) => {
            expression_contains_math_random_call(&member.object)
        }
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => {
                if call.arguments.is_empty()
                    && flatten_callee_path(&call.callee).as_deref() == Some("Math.random")
                {
                    return true;
                }
                expression_callee_contains_math_random(&call.callee)
            }
            ChainElement::StaticMemberExpression(member) => {
                expression_contains_math_random_call(&member.object)
            }
            _ => false,
        },
        _ => false,
    }
}

fn expression_callee_contains_math_random(callee: &Expression<'_>) -> bool {
    match callee {
        Expression::StaticMemberExpression(member) => {
            expression_contains_math_random_call(&member.object)
        }
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::StaticMemberExpression(member) => {
                expression_contains_math_random_call(&member.object)
            }
            ChainElement::CallExpression(call) => {
                expression_callee_contains_math_random(&call.callee)
            }
            _ => false,
        },
        _ => false,
    }
}

fn assignment_target_security_context_name(target: &AssignmentTarget<'_>) -> Option<String> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        AssignmentTarget::StaticMemberExpression(member) => Some(member.property.name.to_string()),
        _ => None,
    }
}

/// Collect the bare identifier names referenced anywhere inside a sink argument
/// expression, deduped in source order. Used by the analyze layer to back-trace
/// the argument to a source-tainted local binding. This is a bounded, shallow
/// structural walk over the common taint-carrying shapes (member roots, binary /
/// template / call / paren / conditional / sequence / await / unary), NOT a full
/// expression evaluator: an identifier that never surfaces in these shapes is
/// simply not collected (a conservative miss, never a false source link).
fn collect_arg_idents(expr: &Expression<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_idents_into(expr, &mut out);
    out
}

fn collect_arg_source_paths(expr: &Expression<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_source_paths_into(expr, &mut out);
    out
}

fn push_ident(name: &str, out: &mut Vec<String>) {
    if !out.iter().any(|n| n == name) {
        out.push(name.to_string());
    }
}

fn push_source_path(path: String, out: &mut Vec<String>) {
    if !out.iter().any(|existing| existing == &path) {
        out.push(path);
    }
}

fn push_member_source_paths(path: &str, out: &mut Vec<String>) {
    push_source_path(path.to_string(), out);
    if let Some((object, _)) = path.rsplit_once('.') {
        push_source_path(object.to_string(), out);
    }
}

fn collect_source_paths_into(expr: &Expression<'_>, out: &mut Vec<String>) {
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            collect_source_paths_into(&paren.expression, out);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_source_paths_into(&ts_as.expression, out);
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            collect_source_paths_into(&ts_sat.expression, out);
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            collect_source_paths_into(&ts_non_null.expression, out);
        }
        Expression::StaticMemberExpression(member) => {
            if let Some(path) = flatten_member_path(expr) {
                push_member_source_paths(&path, out);
            }
            collect_source_paths_into(&member.object, out);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_source_paths_into(&member.object, out);
            collect_source_paths_into(&member.expression, out);
        }
        Expression::BinaryExpression(bin) => {
            collect_source_paths_into(&bin.left, out);
            collect_source_paths_into(&bin.right, out);
        }
        Expression::LogicalExpression(logical) => {
            collect_source_paths_into(&logical.left, out);
            collect_source_paths_into(&logical.right, out);
        }
        Expression::ConditionalExpression(cond) => {
            collect_source_paths_into(&cond.test, out);
            collect_source_paths_into(&cond.consequent, out);
            collect_source_paths_into(&cond.alternate, out);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_source_paths_into(e, out);
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for e in &tpl.expressions {
                collect_source_paths_into(e, out);
            }
        }
        Expression::AwaitExpression(await_expr) => {
            collect_source_paths_into(&await_expr.argument, out);
        }
        Expression::UnaryExpression(unary) => collect_source_paths_into(&unary.argument, out),
        Expression::CallExpression(call) => {
            collect_source_paths_into(&call.callee, out);
            for arg in &call.arguments {
                if let Some(arg_expr) = arg.as_expression() {
                    collect_source_paths_into(arg_expr, out);
                }
            }
        }
        _ => {}
    }
}

fn is_http_route_handler_name(name: &str) -> bool {
    matches!(
        name,
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "HEAD"
    )
}

fn is_route_registration_method(method: &str) -> bool {
    matches!(
        method,
        "all" | "delete" | "get" | "head" | "options" | "patch" | "post" | "put" | "use"
    )
}

fn function_body_has_use_server(body: Option<&FunctionBody<'_>>) -> bool {
    body.is_some_and(|body| {
        body.directives
            .iter()
            .any(|directive| directive.directive.as_str() == "use server")
    })
}

fn callback_params<'a>(arg: &'a Argument<'a>) -> Option<&'a FormalParameters<'a>> {
    match arg {
        Argument::ArrowFunctionExpression(expr) => Some(&expr.params),
        Argument::FunctionExpression(expr) => Some(&expr.params),
        _ => arg.as_expression().and_then(|expr| match expr {
            Expression::ArrowFunctionExpression(expr) => Some(&*expr.params),
            Expression::FunctionExpression(expr) => Some(&*expr.params),
            _ => None,
        }),
    }
}

fn last_callback_params<'a>(args: &'a [Argument<'a>]) -> Option<&'a FormalParameters<'a>> {
    args.iter().rev().find_map(callback_params)
}

fn route_callback_params<'a>(
    args: &'a [Argument<'a>],
    method: &str,
) -> Option<&'a FormalParameters<'a>> {
    if method == "use" {
        return last_callback_params(args);
    }
    args.iter().skip(1).find_map(callback_params)
}

fn collect_idents_into(expr: &Expression<'_>, out: &mut Vec<String>) {
    match expr {
        Expression::Identifier(ident) => push_ident(&ident.name, out),
        Expression::ParenthesizedExpression(paren) => collect_idents_into(&paren.expression, out),
        Expression::TSAsExpression(ts_as) => collect_idents_into(&ts_as.expression, out),
        Expression::TSSatisfiesExpression(ts_sat) => collect_idents_into(&ts_sat.expression, out),
        Expression::TSNonNullExpression(ts_non_null) => {
            collect_idents_into(&ts_non_null.expression, out);
        }
        Expression::StaticMemberExpression(member) => {
            // The leading object root carries the taint (`id` in `id.value`,
            // `req` in `req.query.id`); the property name is a static key.
            collect_idents_into(&member.object, out);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_idents_into(&member.object, out);
            collect_idents_into(&member.expression, out);
        }
        Expression::BinaryExpression(bin) => {
            collect_idents_into(&bin.left, out);
            collect_idents_into(&bin.right, out);
        }
        Expression::LogicalExpression(logical) => {
            collect_idents_into(&logical.left, out);
            collect_idents_into(&logical.right, out);
        }
        Expression::ConditionalExpression(cond) => {
            collect_idents_into(&cond.test, out);
            collect_idents_into(&cond.consequent, out);
            collect_idents_into(&cond.alternate, out);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_idents_into(e, out);
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for e in &tpl.expressions {
                collect_idents_into(e, out);
            }
        }
        Expression::AwaitExpression(await_expr) => collect_idents_into(&await_expr.argument, out),
        Expression::UnaryExpression(unary) => collect_idents_into(&unary.argument, out),
        Expression::CallExpression(call) => {
            // The callee can carry the taint (`getId().trim()` -> getId), as can
            // each argument (`escape(id)` -> id). Bounded one level by recursion.
            collect_idents_into(&call.callee, out);
            for arg in &call.arguments {
                if let Some(arg_expr) = arg.as_expression() {
                    collect_idents_into(arg_expr, out);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(prop) = prop {
                    collect_idents_into(&prop.value, out);
                }
            }
        }
        _ => {}
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
        Expression::CallExpression(call) if call.arguments.is_empty() => {
            Some(format!("{}()", static_member_object_name(&call.callee)?))
        }
        Expression::NewExpression(new_expr) => match &new_expr.callee {
            Expression::Identifier(callee) => Some(callee.name.to_string()),
            _ => None,
        },
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

fn is_import_meta_env_object(expr: &Expression<'_>) -> bool {
    matches!(
        expr,
        Expression::StaticMemberExpression(member)
            if member.property.name == "env"
                && matches!(
                    &member.object,
                    Expression::MetaProperty(meta)
                        if meta.meta.name == "import" && meta.property.name == "meta"
                )
    )
}

fn is_require_resolve_callee(expr: &Expression<'_>) -> bool {
    let Expression::StaticMemberExpression(member) = expr else {
        return false;
    };
    let Expression::Identifier(object) = &member.object else {
        return false;
    };
    object.name == "require" && member.property.name == "resolve"
}

fn package_from_resolution_specifier(specifier: &str) -> Option<String> {
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

fn package_values_from_raw_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| package_from_resolution_specifier(value))
        .collect()
}

fn static_object_string_property_values(
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

fn for_of_binding_name(left: &ForStatementLeft<'_>) -> Option<String> {
    match left {
        ForStatementLeft::VariableDeclaration(decl) => {
            let declarator = decl.declarations.first()?;
            binding_pattern_value_name(&declarator.id)
        }
        ForStatementLeft::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn binding_pattern_value_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
        BindingPattern::ArrayPattern(array) => array.elements.iter().rev().find_map(|element| {
            let Some(pattern) = element else {
                return None;
            };
            let BindingPattern::BindingIdentifier(id) = pattern else {
                return None;
            };
            Some(id.name.to_string())
        }),
        _ => None,
    }
}

fn object_values_or_entries_argument_name(expr: &Expression<'_>) -> Option<String> {
    let Expression::CallExpression(call) = expr else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    let Expression::Identifier(object) = &member.object else {
        return None;
    };
    if object.name != "Object" || !matches!(member.property.name.as_str(), "values" | "entries") {
        return None;
    }
    let Some(Argument::Identifier(arg)) = call.arguments.first() else {
        return None;
    };
    Some(arg.name.to_string())
}

fn package_resolution_arg_index(
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

/// Returns true when the tagged template's tag is the bare identifier `html`.
fn is_html_tagged_template(tag: &Expression<'_>) -> bool {
    matches!(tag, Expression::Identifier(id) if id.name == "html")
}

/// Collect `(local_name, class_name)` pairs from an `instanceof` guard expression.
///
/// Recurses through `&&`-chained conditions so `a instanceof A && b instanceof B`
/// yields both pairs. Only simple identifier left-hand sides (`x instanceof Cls`)
/// are collected; complex left-hand expressions are skipped conservatively.
fn collect_instanceof_narrowings<'a>(expr: &'a Expression<'a>, out: &mut Vec<(String, String)>) {
    match expr {
        Expression::BinaryExpression(bin) if bin.operator == BinaryOperator::Instanceof => {
            if let Expression::Identifier(left) = &bin.left
                && let Expression::Identifier(right) = &bin.right
            {
                out.push((left.name.to_string(), right.name.to_string()));
            }
        }
        Expression::LogicalExpression(logical) if logical.operator == LogicalOperator::And => {
            collect_instanceof_narrowings(&logical.left, out);
            collect_instanceof_narrowings(&logical.right, out);
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_instanceof_narrowings(&paren.expression, out);
        }
        _ => {}
    }
}

impl ModuleInfoExtractor {
    /// Capture a call/member-call sink site (category-blind). Pushes one
    /// `SinkSite` per admitted positional argument; a callee that cannot be
    /// flattened to a static path increments the blind-spot counter instead.
    fn capture_call_sink(&mut self, expr: &CallExpression<'_>) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            self.security_sinks_skipped += 1;
            return;
        };
        let sink_shape = if callee_path.contains('.') {
            SinkShape::MemberCall
        } else {
            SinkShape::Call
        };
        for (index, arg) in expr.arguments.iter().enumerate() {
            let Some(arg_expr) = arg.as_expression() else {
                continue;
            };
            let Ok(arg_index) = u32::try_from(index) else {
                continue;
            };
            let arg_is_non_literal = is_non_literal_arg(arg_expr);
            if !arg_is_non_literal
                && !should_capture_literal_sink_arg(&callee_path, sink_shape, arg_index, arg_expr)
            {
                continue;
            }
            if arg_is_non_literal {
                self.record_sanitized_sink_arg(expr.span.start, arg_index, arg_expr);
            }
            let object_keys = object_key_metadata(arg_expr);
            self.security_sinks.push(SinkSite {
                sink_shape,
                callee_path: callee_path.clone(),
                arg_index,
                arg_is_non_literal,
                arg_kind: if arg_is_non_literal {
                    classify_arg_kind(arg_expr)
                } else {
                    SinkArgKind::Literal
                },
                arg_literal: sink_literal_value(arg_expr),
                object_properties: object_literal_properties(arg_expr),
                object_property_keys: object_keys.keys,
                object_property_keys_complete: object_keys.complete,
                arg_idents: if arg_is_non_literal {
                    collect_arg_idents(arg_expr)
                } else {
                    Vec::new()
                },
                arg_source_paths: if arg_is_non_literal {
                    collect_arg_source_paths(arg_expr)
                } else {
                    Vec::new()
                },
                span_start: expr.span.start,
                span_end: expr.span.end,
            });
        }
        if should_capture_missing_jwt_verify_options(&callee_path, sink_shape, expr.arguments.len())
        {
            self.security_sinks.push(SinkSite {
                sink_shape,
                callee_path,
                arg_index: 2,
                arg_is_non_literal: false,
                arg_kind: SinkArgKind::Object,
                arg_literal: None,
                object_properties: Vec::new(),
                object_property_keys: Vec::new(),
                object_property_keys_complete: true,
                arg_idents: Vec::new(),
                arg_source_paths: Vec::new(),
                span_start: expr.span.start,
                span_end: expr.span.end,
            });
        }
    }

    /// Capture constructor-call sink sites. This is intentionally separate from
    /// call capture because oxc represents `new Function("...")` as a
    /// `NewExpression`, not a `CallExpression`.
    fn capture_new_expression_sink(&mut self, expr: &oxc_ast::ast::NewExpression<'_>) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            return;
        };
        for (index, arg) in expr.arguments.iter().enumerate() {
            let Some(arg_expr) = arg.as_expression() else {
                continue;
            };
            let Ok(arg_index) = u32::try_from(index) else {
                continue;
            };
            let arg_is_non_literal = is_non_literal_arg(arg_expr);
            if !arg_is_non_literal
                && !should_capture_literal_sink_arg(
                    &callee_path,
                    SinkShape::NewExpression,
                    arg_index,
                    arg_expr,
                )
            {
                continue;
            }
            let object_keys = object_key_metadata(arg_expr);
            self.security_sinks.push(SinkSite {
                sink_shape: SinkShape::NewExpression,
                callee_path: callee_path.clone(),
                arg_index,
                arg_is_non_literal,
                arg_kind: if arg_is_non_literal {
                    classify_arg_kind(arg_expr)
                } else {
                    SinkArgKind::Literal
                },
                arg_literal: sink_literal_value(arg_expr),
                object_properties: object_literal_properties(arg_expr),
                object_property_keys: object_keys.keys,
                object_property_keys_complete: object_keys.complete,
                arg_idents: if arg_is_non_literal {
                    collect_arg_idents(arg_expr)
                } else {
                    Vec::new()
                },
                arg_source_paths: if arg_is_non_literal {
                    collect_arg_source_paths(arg_expr)
                } else {
                    Vec::new()
                },
                span_start: expr.span.start,
                span_end: expr.span.end,
            });
        }
    }

    fn capture_math_random_context_sink(
        &mut self,
        context_name: &str,
        expr: &Expression<'_>,
        span: Span,
    ) {
        if !is_token_like_security_name(context_name) || !expression_contains_math_random_call(expr)
        {
            return;
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::MemberCall,
            callee_path: "Math.random".to_string(),
            arg_index: 0,
            arg_is_non_literal: false,
            arg_kind: SinkArgKind::NoArg,
            arg_literal: None,
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents: vec![context_name.to_string()],
            arg_source_paths: Vec::new(),
            span_start: span.start,
            span_end: span.end,
        });
    }

    fn capture_hardcoded_secret_literal_sink(
        &mut self,
        context_name: &str,
        expr: &Expression<'_>,
        span: Span,
    ) {
        let Some(value) = static_string_literal_value(expr) else {
            return;
        };
        if !should_capture_hardcoded_secret_literal(context_name, &value) {
            return;
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::SecretLiteral,
            callee_path: context_name.to_string(),
            arg_index: 0,
            arg_is_non_literal: false,
            arg_kind: SinkArgKind::Literal,
            arg_literal: Some(SinkLiteralValue::String(value)),
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents: vec![context_name.to_string()],
            arg_source_paths: Vec::new(),
            span_start: span.start,
            span_end: span.end,
        });
    }

    /// Capture a member-assignment sink site (e.g. `el.innerHTML = userInput`).
    /// Static-member targets with a non-literal RHS are captured; one exact
    /// literal TLS-env assignment is admitted because the literal value is the
    /// security signal. A target whose object cannot be flattened increments
    /// the blind-spot counter.
    fn capture_member_assign_sink(&mut self, expr: &AssignmentExpression<'_>) {
        let AssignmentTarget::StaticMemberExpression(member) = &expr.left else {
            return;
        };
        let Some(object_path) = flatten_callee_path(&member.object) else {
            self.security_sinks_skipped += 1;
            return;
        };
        let callee_path = format!("{}.{}", object_path, member.property.name);
        let arg_is_non_literal = is_non_literal_arg(&expr.right);
        if !arg_is_non_literal
            && !should_capture_literal_sink_arg(
                &callee_path,
                SinkShape::MemberAssign,
                0,
                &expr.right,
            )
        {
            return;
        }
        if arg_is_non_literal {
            self.record_sanitized_sink_arg(expr.span.start, 0, &expr.right);
        }
        let object_keys = object_key_metadata(&expr.right);
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::MemberAssign,
            callee_path,
            arg_index: 0,
            arg_is_non_literal,
            arg_kind: if arg_is_non_literal {
                classify_arg_kind(&expr.right)
            } else {
                SinkArgKind::Literal
            },
            arg_literal: sink_literal_value(&expr.right),
            object_properties: object_literal_properties(&expr.right),
            object_property_keys: object_keys.keys,
            object_property_keys_complete: object_keys.complete,
            arg_idents: if arg_is_non_literal {
                collect_arg_idents(&expr.right)
            } else {
                Vec::new()
            },
            arg_source_paths: if arg_is_non_literal {
                collect_arg_source_paths(&expr.right)
            } else {
                Vec::new()
            },
            span_start: expr.span.start,
            span_end: expr.span.end,
        });
    }

    /// Capture a tagged-template sink site (e.g. ``sql`...${x}...` ``). Only
    /// templates with at least one substitution are captured.
    fn capture_tagged_template_sink(&mut self, expr: &TaggedTemplateExpression<'_>) {
        if expr.quasi.expressions.is_empty() {
            return;
        }
        let Some(callee_path) = flatten_callee_path(&expr.tag) else {
            return;
        };
        let mut arg_idents: Vec<String> = Vec::new();
        let mut arg_source_paths: Vec<String> = Vec::new();
        for substitution in &expr.quasi.expressions {
            collect_idents_into(substitution, &mut arg_idents);
            collect_source_paths_into(substitution, &mut arg_source_paths);
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::TaggedTemplate,
            callee_path,
            arg_index: 0,
            arg_is_non_literal: true,
            // A tagged template is captured only with substitutions, so the
            // argument is always a template-with-substitution.
            arg_kind: SinkArgKind::TemplateWithSubst,
            arg_literal: None,
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents,
            arg_source_paths,
            span_start: expr.span.start,
            span_end: expr.span.end,
        });
    }

    /// Capture a JSX-attribute sink site (e.g. `dangerouslySetInnerHTML={x}`).
    /// Only identifier-named attributes with a non-literal expression-container
    /// value are captured; the empty `{}` form yields no expression and is
    /// skipped without an explicit arm.
    fn capture_jsx_attr_sink(&mut self, attr: &JSXAttribute<'_>) {
        let JSXAttributeName::Identifier(name) = &attr.name else {
            return;
        };
        let Some(JSXAttributeValue::ExpressionContainer(container)) = &attr.value else {
            return;
        };
        let Some(value_expr) = container.expression.as_expression() else {
            return;
        };
        if !is_non_literal_arg(value_expr) {
            return;
        }
        self.record_sanitized_sink_arg(attr.span.start, 0, value_expr);
        let object_keys = object_key_metadata(value_expr);
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::JsxAttr,
            callee_path: name.name.to_string(),
            arg_index: 0,
            arg_is_non_literal: true,
            arg_kind: classify_arg_kind(value_expr),
            arg_literal: None,
            object_properties: object_literal_properties(value_expr),
            object_property_keys: object_keys.keys,
            object_property_keys_complete: object_keys.complete,
            arg_idents: collect_arg_idents(value_expr),
            arg_source_paths: collect_arg_source_paths(value_expr),
            span_start: attr.span.start,
            span_end: attr.span.end,
        });
    }

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
