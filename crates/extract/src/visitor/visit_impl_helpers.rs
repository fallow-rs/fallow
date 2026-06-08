#[allow(clippy::wildcard_imports, reason = "many AST helper types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::ScopeFlags;
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Component, PathBuf};

use crate::MemberAccess;

use super::super::helpers::{extract_type_annotation_name, is_meta_url_arg};
use super::{static_member_object_name, unwrap_static_expr};

pub(super) const PINO_PACKAGE: &str = "pino";
pub(super) const PINO_FACTORY_EXPORT: &str = "pino";
pub(super) const PINO_TRANSPORT_KEY: &str = "transport";
pub(super) const PINO_TARGET_KEY: &str = "target";
pub(super) const PINO_TARGETS_KEY: &str = "targets";
pub(super) const FRAMEWORK_REQUEST_SOURCE: &str = "framework.request";
pub(super) const NEXT_REQUEST_SOURCE: &str = "next.request";
pub(super) const NEXT_FORM_DATA_SOURCE: &str = "next.form-data";
pub(super) const QUEUE_JOB_SOURCE: &str = "queue.job";
pub(super) const MCP_TOOL_INPUT_SOURCE: &str = "mcp.tool-input";
pub(super) const GRAPHQL_ARGS_SOURCE: &str = "graphql.args";
pub(super) const TRPC_INPUT_SOURCE: &str = "trpc.input";

pub(super) type StaticPackageStringBindings = FxHashMap<String, Vec<String>>;
pub(super) type StaticPackageObjectBindings = FxHashMap<String, FxHashMap<String, Vec<String>>>;
pub(super) type StaticPackageLoopBindings =
    (StaticPackageStringBindings, StaticPackageObjectBindings);

#[derive(Default)]
pub(super) struct SignatureTypeCollector {
    pub(super) refs: Vec<(String, Span)>,
}

impl<'a> Visit<'a> for SignatureTypeCollector {
    fn visit_ts_type_reference(&mut self, type_ref: &TSTypeReference<'a>) {
        if let Some((name, span)) = type_name_root(&type_ref.type_name) {
            self.refs.push((name, span));
        }
        walk::walk_ts_type_reference(self, type_ref);
    }
}

pub(super) struct StructuralParamMemberCollector {
    target_params: FxHashSet<String>,
    shadowed_stack: Vec<FxHashSet<String>>,
    pub(super) members: FxHashMap<String, FxHashSet<String>>,
}

impl StructuralParamMemberCollector {
    pub(super) fn new(target_params: FxHashSet<String>) -> Self {
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

pub(super) fn type_name_root(name: &TSTypeName<'_>) -> Option<(String, Span)> {
    match name {
        TSTypeName::IdentifierReference(ident) => Some((ident.name.to_string(), ident.span)),
        TSTypeName::QualifiedName(qualified) => type_name_root(&qualified.left),
        TSTypeName::ThisExpression(_) => None,
    }
}

pub(super) fn expression_root_name(expr: &Expression<'_>) -> Option<(String, Span)> {
    match expr {
        Expression::Identifier(ident) => Some((ident.name.to_string(), ident.span)),
        Expression::StaticMemberExpression(member) => expression_root_name(&member.object),
        _ => None,
    }
}

pub(super) fn is_private_member_key(key: &PropertyKey<'_>) -> bool {
    matches!(key, PropertyKey::PrivateIdentifier(_))
}

pub(super) fn vitest_mock_source(call: &CallExpression<'_>) -> Option<String> {
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

pub(super) fn vitest_auto_mock_source(source: &str) -> Option<String> {
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

pub(super) fn pino_factory_callee_name(callee: &Expression<'_>) -> Option<String> {
    match unwrap_static_expr(callee) {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}

pub(super) fn collect_pino_config_targets(expr: &Expression<'_>, out: &mut Vec<String>) {
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

pub(super) fn collect_pino_transport_targets(expr: &Expression<'_>, out: &mut Vec<String>) {
    match unwrap_static_expr(expr) {
        Expression::ObjectExpression(obj) => collect_pino_transport_object_targets(obj, out),
        Expression::ConditionalExpression(cond) => {
            collect_pino_transport_targets(&cond.consequent, out);
            collect_pino_transport_targets(&cond.alternate, out);
        }
        _ => {}
    }
}

pub(super) fn collect_pino_transport_object_targets(
    obj: &ObjectExpression<'_>,
    out: &mut Vec<String>,
) {
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

pub(super) fn record_pino_target_value(expr: &Expression<'_>, out: &mut Vec<String>) {
    if let Expression::StringLiteral(lit) = unwrap_static_expr(expr) {
        record_pino_target(lit.value.as_str(), out);
    }
}

pub(super) fn record_pino_targets_array(expr: &Expression<'_>, out: &mut Vec<String>) {
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

pub(super) fn record_pino_target(source: &str, out: &mut Vec<String>) {
    if source.is_empty() || out.iter().any(|existing| existing == source) {
        return;
    }
    out.push(source.to_string());
}

pub(super) fn is_dompurify_source(source: &str) -> bool {
    matches!(source, "dompurify" | "isomorphic-dompurify")
}

/// Detect whether `vi.mock(specifier, factory, ...)` passes a factory.
pub(super) fn vi_mock_has_factory(call: &CallExpression<'_>) -> bool {
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
pub(super) fn is_value_returning_memo_callee(callee: &Expression<'_>) -> bool {
    match callee {
        Expression::Identifier(id) => id.name == "useMemo",
        Expression::StaticMemberExpression(member) => member.property.name == "useMemo",
        _ => false,
    }
}

/// Specifier source string from the first argument of `register(...)`.
pub(super) fn node_module_register_specifier(call: &CallExpression<'_>) -> Option<String> {
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
pub(super) const NODE_MODULE_REGISTER_HOOK_EXPORTS: &[&str] = &[
    "initialize",
    "resolve",
    "load",
    "globalPreload",
    "getFormat",
    "getSource",
    "transformSource",
];

pub(super) fn loader_hook_exports_for_source(source: &str) -> Vec<String> {
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

pub(super) fn new_url_import_source(expr: &NewExpression<'_>) -> Option<String> {
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

pub(super) fn is_child_process_source(source: &str) -> bool {
    matches!(source, "node:child_process" | "child_process")
}

pub(super) fn is_node_path_source(source: &str) -> bool {
    matches!(source, "node:path" | "path")
}

pub(super) fn is_node_url_source(source: &str) -> bool {
    matches!(source, "node:url" | "url")
}

pub(super) fn local_fork_source(source: &str) -> Option<String> {
    if (source.starts_with("./") || source.starts_with("../")) && !source.ends_with('/') {
        Some(source.to_string())
    } else {
        None
    }
}

pub(super) fn normalize_module_file_relative_path(relative: &str) -> Option<String> {
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
pub(super) struct PlaywrightFixtureMemberCollector {
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

pub(super) fn extract_binding_local_name<'a>(pattern: &'a BindingPattern<'a>) -> Option<&'a str> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        BindingPattern::AssignmentPattern(assign) => extract_binding_local_name(&assign.left),
        _ => None,
    }
}

/// Collect `property -> class type` mappings from an object-type member list.
pub(super) fn collect_object_type_property_types(
    members: &[TSSignature<'_>],
) -> FxHashMap<String, String> {
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

pub(super) fn extract_object_pattern_bindings(
    pattern: &ObjectPattern<'_>,
) -> FxHashMap<String, String> {
    let mut bindings = FxHashMap::default();
    collect_object_pattern_bindings(pattern, "", &mut bindings);
    bindings
}

pub(super) fn collect_object_pattern_bindings(
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

pub(super) fn resolve_object_to_fixture_path(
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

pub(super) fn playwright_test_callee_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        Expression::StaticMemberExpression(member) => playwright_test_callee_name(&member.object),
        Expression::CallExpression(call) => playwright_test_callee_name(&call.callee),
        _ => None,
    }
}

/// Find the call expression returned by a function body.
pub(super) fn extract_function_body_final_return_call<'a, 'b>(
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

/// Find the call expression used as an arrow function body.
pub(super) fn extract_arrow_return_call<'a, 'b>(
    arrow: &'b oxc_ast::ast::ArrowFunctionExpression<'a>,
) -> Option<&'b CallExpression<'a>> {
    if arrow.expression {
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

pub(super) fn collect_playwright_fixture_member_uses(
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

pub(super) fn playwright_extend_base_name(call: &CallExpression<'_>) -> Option<String> {
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

pub(super) fn collect_fixture_type_bindings_from_type(
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

pub(super) fn fixture_type_reference_name(ty: &TSType<'_>) -> Option<(String, Span)> {
    match ty {
        TSType::TSTypeReference(type_ref) => type_name_root(&type_ref.type_name),
        TSType::TSParenthesizedType(paren) => fixture_type_reference_name(&paren.type_annotation),
        _ => None,
    }
}
