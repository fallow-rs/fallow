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

/// Hop depth of a directly captured tainted binding: a source read
/// (`const id = req.query.id`), a framework handler param, a one-hop helper
/// return, or a destructure-from-source. Chained bindings (issue #1146) start
/// counting from here.
pub(super) const DIRECT_TAINT_HOP: u8 = 1;

/// Maximum chained-binding depth for same-module taint propagation (issue
/// #1146): a direct capture is hop 1 and each chain step through another local
/// binding (`const b = \`wrap-${a}\``) adds 1, so the issue's headline 2-hop
/// case (`a` -> `b`) fits with one level of slack. A binding that would exceed
/// the cap is simply not recorded, so over-cap chains degrade to module-level
/// reachability instead of claiming a false arg-level tier. Deliberately a
/// constant, not a config knob: a `RUST_LOG=debug` line fires when a chain is
/// dropped at the cap, and the #1142 function-local relation layer is the
/// intended long-term substrate for deeper flows.
pub(super) const MAX_TAINT_BINDING_HOPS: u8 = 3;

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

    let normalized = PathBuf::from("__f_current_file").join(relative);

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

    if parts.first().is_some_and(|part| part == "__f_current_file") || parts.is_empty() {
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
    alias_by_local: FxHashMap<String, Vec<String>>,
    shadowed_stack: Vec<FxHashSet<String>>,
    block_declared_stack: Vec<FxHashSet<String>>,
    nested_function_depth: usize,
    accesses: Vec<MemberAccess>,
}

impl PlaywrightFixtureMemberCollector {
    fn new(fixture_by_local: FxHashMap<String, String>) -> Self {
        Self {
            fixture_by_local,
            alias_by_local: FxHashMap::default(),
            shadowed_stack: Vec::new(),
            block_declared_stack: Vec::new(),
            nested_function_depth: 0,
            accesses: Vec::new(),
        }
    }

    fn branch_collector(&self) -> Self {
        Self {
            fixture_by_local: self.fixture_by_local.clone(),
            alias_by_local: self.alias_by_local.clone(),
            shadowed_stack: self.shadowed_stack.clone(),
            block_declared_stack: self.block_declared_stack.clone(),
            nested_function_depth: self.nested_function_depth,
            accesses: Vec::new(),
        }
    }

    fn is_shadowed(&self, name: &str) -> bool {
        self.shadowed_stack.iter().any(|scope| scope.contains(name))
    }

    fn resolve_object_to_fixture_paths(
        &self,
        object_dotted: &str,
        include_aliases: bool,
    ) -> Option<Vec<String>> {
        let (root, rest) = object_dotted
            .split_once('.')
            .map_or((object_dotted, ""), |(r, x)| (r, x));
        if self.is_shadowed(root) {
            return None;
        }

        if let Some(base) = self.fixture_by_local.get(root) {
            return Some(vec![append_fixture_path(base, rest)]);
        }

        if include_aliases
            && self.nested_function_depth == 0
            && let Some(paths) = self.alias_by_local.get(root)
        {
            let resolved = paths
                .iter()
                .map(|path| append_fixture_path(path, rest))
                .collect();
            return Some(resolved);
        }

        None
    }

    fn resolve_alias_source_paths(&self, expr: &Expression<'_>) -> Option<Vec<String>> {
        match unwrap_static_expr(expr) {
            Expression::Identifier(ident) => {
                self.resolve_object_to_fixture_paths(ident.name.as_str(), false)
            }
            Expression::StaticMemberExpression(_) => static_member_object_name(expr)
                .and_then(|name| self.resolve_object_to_fixture_paths(&name, false)),
            Expression::ConditionalExpression(cond) => {
                let mut paths = self.resolve_alias_source_paths(&cond.consequent)?;
                let alternate = self.resolve_alias_source_paths(&cond.alternate)?;
                append_unique_paths(&mut paths, alternate);
                Some(paths)
            }
            _ => None,
        }
    }

    fn record_binding_alias(&mut self, name: &str, init: Option<&Expression<'_>>) {
        let Some(init) = init else {
            return;
        };
        if let Some(paths) = self.resolve_alias_source_paths(init) {
            self.alias_by_local.insert(name.to_string(), paths);
        } else {
            self.alias_by_local.remove(name);
        }
    }

    fn record_assignment_alias(&mut self, name: &str, expr: &Expression<'_>) {
        if self.is_shadowed(name) {
            return;
        }
        if let Some(paths) = self.resolve_alias_source_paths(expr) {
            self.alias_by_local.insert(name.to_string(), paths);
        } else {
            self.alias_by_local.remove(name);
            if self.fixture_by_local.contains_key(name)
                && let Some(scope) = self.shadowed_stack.last_mut()
            {
                scope.insert(name.to_string());
            }
        }
    }

    fn record_declared_name(&mut self, name: &str) {
        if let Some(scope) = self.block_declared_stack.last_mut() {
            scope.insert(name.to_string());
        }
        if (self.fixture_by_local.contains_key(name) || self.alias_by_local.contains_key(name))
            && let Some(scope) = self.shadowed_stack.last_mut()
        {
            scope.insert(name.to_string());
        }
    }

    fn collect_shadowed_params(&self, params: &FormalParameters<'_>) -> FxHashSet<String> {
        let mut shadowed = FxHashSet::default();
        for param in &params.items {
            for binding in param.pattern.get_binding_identifiers() {
                let name = binding.name.as_str();
                if self.fixture_by_local.contains_key(name)
                    || self.alias_by_local.contains_key(name)
                {
                    shadowed.insert(name.to_string());
                }
            }
        }
        shadowed
    }

    fn merge_branch_aliases(
        &mut self,
        before: &FxHashMap<String, Vec<String>>,
        branches: &[FxHashMap<String, Vec<String>>],
    ) {
        if branches.is_empty() {
            return;
        }

        let mut names = FxHashSet::default();
        names.extend(before.keys().cloned());
        for branch in branches {
            names.extend(branch.keys().cloned());
        }

        for name in names {
            let before_value = before.get(&name);
            let changed = branches
                .iter()
                .any(|branch| branch.get(&name) != before_value);
            if !changed {
                continue;
            }

            let mut merged = Vec::new();
            let mut all_fixture_derived = true;
            for branch in branches {
                let Some(paths) = branch.get(&name) else {
                    all_fixture_derived = false;
                    break;
                };
                append_unique_paths(&mut merged, paths.clone());
            }

            if all_fixture_derived {
                self.alias_by_local.insert(name, merged);
            } else {
                self.alias_by_local.remove(&name);
            }
        }
    }
}

impl<'a> Visit<'a> for PlaywrightFixtureMemberCollector {
    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        if let Some(object_dotted) = static_member_object_name(&expr.object)
            && let Some(fixture_paths) = self.resolve_object_to_fixture_paths(&object_dotted, true)
        {
            for fixture_path in fixture_paths {
                self.accesses.push(MemberAccess {
                    object: fixture_path,
                    member: expr.property.name.to_string(),
                });
            }
            return;
        }
        walk::walk_static_member_expression(self, expr);
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        for declarator in &decl.declarations {
            if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                let name = id.name.as_str();
                self.record_declared_name(name);
                if self.nested_function_depth == 0 {
                    self.record_binding_alias(name, declarator.init.as_ref());
                }
            }
        }
        walk::walk_variable_declaration(self, decl);
    }

    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        if self.nested_function_depth == 0
            && let Some(name) = assignment_target_identifier_name(&expr.left)
        {
            self.record_assignment_alias(name, &expr.right);
        }
        walk::walk_assignment_expression(self, expr);
    }

    fn visit_if_statement(&mut self, stmt: &IfStatement<'a>) {
        self.visit_expression(&stmt.test);
        let before = self.alias_by_local.clone();

        let mut consequent = self.branch_collector();
        consequent.visit_statement(&stmt.consequent);
        self.accesses.extend(consequent.accesses);

        let mut branches = vec![consequent.alias_by_local];
        if let Some(alternate) = &stmt.alternate {
            let mut alternate_collector = self.branch_collector();
            alternate_collector.visit_statement(alternate);
            self.accesses.extend(alternate_collector.accesses);
            branches.push(alternate_collector.alias_by_local);
        } else {
            branches.push(before.clone());
        }

        self.merge_branch_aliases(&before, &branches);
    }

    fn visit_switch_statement(&mut self, stmt: &SwitchStatement<'a>) {
        self.visit_expression(&stmt.discriminant);
        let before = self.alias_by_local.clone();
        let has_default = stmt.cases.iter().any(|case| case.test.is_none());
        let mut branches = Vec::new();

        for case in &stmt.cases {
            if let Some(test) = &case.test {
                self.visit_expression(test);
            }
            let mut case_collector = self.branch_collector();
            for statement in &case.consequent {
                case_collector.visit_statement(statement);
            }
            self.accesses.extend(case_collector.accesses);
            branches.push(case_collector.alias_by_local);
        }

        if !has_default {
            branches.push(before.clone());
        }
        self.merge_branch_aliases(&before, &branches);
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        let shadowed = self.collect_shadowed_params(&func.params);
        self.shadowed_stack.push(shadowed);
        self.nested_function_depth += 1;
        walk::walk_function(self, func, flags);
        self.nested_function_depth -= 1;
        self.shadowed_stack.pop();
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        let shadowed = self.collect_shadowed_params(&expr.params);
        self.shadowed_stack.push(shadowed);
        self.nested_function_depth += 1;
        walk::walk_arrow_function_expression(self, expr);
        self.nested_function_depth -= 1;
        self.shadowed_stack.pop();
    }

    fn visit_block_statement(&mut self, stmt: &BlockStatement<'a>) {
        let before_aliases = self.alias_by_local.clone();
        self.shadowed_stack.push(FxHashSet::default());
        self.block_declared_stack.push(FxHashSet::default());
        walk::walk_block_statement(self, stmt);
        if let Some(declared) = self.block_declared_stack.pop() {
            for name in declared {
                if let Some(previous) = before_aliases.get(&name) {
                    self.alias_by_local.insert(name, previous.clone());
                } else {
                    self.alias_by_local.remove(&name);
                }
            }
        }
        self.shadowed_stack.pop();
    }
}

fn append_fixture_path(base: &str, rest: &str) -> String {
    if rest.is_empty() {
        base.to_string()
    } else {
        format!("{base}.{rest}")
    }
}

fn append_unique_paths(target: &mut Vec<String>, paths: Vec<String>) {
    for path in paths {
        if !target.iter().any(|existing| existing == &path) {
            target.push(path);
        }
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

pub(super) struct PlaywrightFixtureMemberUse {
    pub(super) fixture_name: String,
    pub(super) member: String,
}

pub(super) fn collect_playwright_fixture_member_uses(
    arguments: &[Argument<'_>],
) -> Vec<PlaywrightFixtureMemberUse> {
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
        .map(|access| PlaywrightFixtureMemberUse {
            fixture_name: access.object,
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
            collect_fixture_type_bindings_from_members(
                &type_lit.members,
                path_prefix,
                aliases,
                bindings,
            );
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

/// Collect fixture bindings from a slice of type members (a `TSTypeLiteral`
/// body or a `TSInterfaceDeclaration` body; both are `TSSignature` lists, so
/// an interface-declared fixture map resolves identically to the alias form).
pub(super) fn collect_fixture_type_bindings_from_members(
    members: &[TSSignature<'_>],
    path_prefix: &str,
    aliases: &FxHashMap<String, Vec<(String, String)>>,
    bindings: &mut Vec<(String, String)>,
) {
    for member in members {
        collect_fixture_type_binding_from_member(member, path_prefix, aliases, bindings);
    }
}

/// Process one type-literal member: resolve its `<path_prefix>.<fixture>` key and
/// either record the concrete fixture type or recurse into an alias / nested type.
fn collect_fixture_type_binding_from_member(
    member: &TSSignature<'_>,
    path_prefix: &str,
    aliases: &FxHashMap<String, Vec<(String, String)>>,
    bindings: &mut Vec<(String, String)>,
) {
    let TSSignature::TSPropertySignature(prop) = member else {
        return;
    };
    let Some(fixture_name) = prop.key.static_name() else {
        return;
    };
    let Some(type_annotation) = prop.type_annotation.as_deref() else {
        return;
    };
    let next_path = if path_prefix.is_empty() {
        fixture_name.to_string()
    } else {
        format!("{path_prefix}.{fixture_name}")
    };
    if let Some((alias_name, _)) = fixture_type_reference_name(&type_annotation.type_annotation)
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

pub(super) fn fixture_type_reference_name(ty: &TSType<'_>) -> Option<(String, Span)> {
    match ty {
        TSType::TSTypeReference(type_ref) => type_name_root(&type_ref.type_name),
        TSType::TSParenthesizedType(paren) => fixture_type_reference_name(&paren.type_annotation),
        _ => None,
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // record_pino_target
    // -------------------------------------------------------------------------

    #[test]
    fn record_pino_target_pushes_new_source() {
        let mut out = Vec::new();
        record_pino_target("pino-pretty", &mut out);
        assert_eq!(out, vec!["pino-pretty"]);
    }

    #[test]
    fn record_pino_target_ignores_empty_source() {
        let mut out = Vec::new();
        record_pino_target("", &mut out);
        assert!(out.is_empty(), "empty source must be ignored");
    }

    #[test]
    fn record_pino_target_deduplicates_existing_source() {
        let mut out = vec!["pino-pretty".to_string()];
        record_pino_target("pino-pretty", &mut out);
        assert_eq!(out.len(), 1, "duplicate source must not be added twice");
    }

    #[test]
    fn record_pino_target_pushes_distinct_sources() {
        let mut out = vec!["pino-pretty".to_string()];
        record_pino_target("pino-elasticsearch", &mut out);
        assert_eq!(out.len(), 2);
    }

    // -------------------------------------------------------------------------
    // vitest_auto_mock_source
    // -------------------------------------------------------------------------

    #[test]
    fn vitest_auto_mock_source_empty_returns_none() {
        assert!(vitest_auto_mock_source("").is_none());
    }

    #[test]
    fn vitest_auto_mock_source_url_scheme_returns_none() {
        assert!(vitest_auto_mock_source("https://example.com/mod").is_none());
        assert!(vitest_auto_mock_source("file://foo/bar").is_none());
    }

    #[test]
    fn vitest_auto_mock_source_data_prefix_returns_none() {
        assert!(vitest_auto_mock_source("data:text/plain,foo").is_none());
    }

    #[test]
    fn vitest_auto_mock_source_already_under_mocks_returns_none() {
        assert!(
            vitest_auto_mock_source("./services/__mocks__/api").is_none(),
            "source already containing __mocks__ segment must be skipped"
        );
    }

    #[test]
    fn vitest_auto_mock_source_no_slash_returns_none() {
        // rsplit_once('/') on a bare package name fails the `?` and returns None
        assert!(vitest_auto_mock_source("axios").is_none());
    }

    #[test]
    fn vitest_auto_mock_source_trailing_slash_returns_none() {
        // file_name would be empty after rsplit_once
        assert!(vitest_auto_mock_source("./services/").is_none());
    }

    #[test]
    fn vitest_auto_mock_source_normal_path_synthesizes_mocks_sibling() {
        let result = vitest_auto_mock_source("./services/api");
        assert_eq!(
            result.as_deref(),
            Some("./services/__mocks__/api"),
            "expected mocks sibling path"
        );
    }

    #[test]
    fn vitest_auto_mock_source_nested_path_synthesizes_mocks_sibling() {
        let result = vitest_auto_mock_source("../utils/format");
        assert_eq!(result.as_deref(), Some("../utils/__mocks__/format"));
    }

    // -------------------------------------------------------------------------
    // normalize_module_file_relative_path
    // -------------------------------------------------------------------------

    #[test]
    fn normalize_empty_returns_none() {
        assert!(normalize_module_file_relative_path("").is_none());
    }

    #[test]
    fn normalize_absolute_returns_none() {
        assert!(normalize_module_file_relative_path("/abs/path").is_none());
    }

    #[test]
    fn normalize_trailing_slash_returns_none() {
        assert!(normalize_module_file_relative_path("./foo/").is_none());
    }

    #[test]
    fn normalize_current_dir_dot_returns_none() {
        // "." leaves only the synthetic current-file anchor in parts, which is filtered.
        assert!(normalize_module_file_relative_path(".").is_none());
    }

    #[test]
    fn normalize_too_many_parent_dirs_returns_none() {
        // Going above the current-file anchor with excess ".." pops until empty
        // then pop() returns None, so the whole function returns None.
        assert!(normalize_module_file_relative_path("../../..").is_none());
    }

    #[test]
    fn normalize_dot_slash_prefix_with_filename_returns_none() {
        // "./foo" leaves the file anchor in parts, so the path is rejected.
        // (The anchor represents the current FILE, not directory; "./foo" from
        // a file path produces a sub-path of the file itself, which is invalid.)
        assert!(normalize_module_file_relative_path("./foo").is_none());
    }

    #[test]
    fn normalize_parent_relative_produces_sibling_path() {
        // "../sibling" pops the file anchor, leaving only "sibling" with no "../"
        // prefix, so the result is "./sibling" (same-directory as the file's dir).
        let result =
            normalize_module_file_relative_path("../sibling").map(|s| s.replace('\\', "/"));
        assert_eq!(result.as_deref(), Some("./sibling"));
    }

    #[test]
    fn normalize_dot_slash_dotdot_inside_returns_none() {
        // "./a/../b" resolves with the file anchor still present, so it is rejected.
        assert!(normalize_module_file_relative_path("./a/../b").is_none());
    }

    #[test]
    fn normalize_parent_relative_deep_path_produces_dot_slash() {
        // "../a/b" pops the file anchor, leaves "a/b" which gets a "./" prefix.
        let result = normalize_module_file_relative_path("../a/b").map(|s| s.replace('\\', "/"));
        assert_eq!(result.as_deref(), Some("./a/b"));
    }

    // -------------------------------------------------------------------------
    // loader_hook_exports_for_source
    // -------------------------------------------------------------------------

    #[test]
    fn loader_hook_exports_for_relative_source_returns_all_hooks() {
        let hooks = loader_hook_exports_for_source("./loader.mjs");
        assert!(!hooks.is_empty());
        assert!(hooks.contains(&"resolve".to_string()));
        assert!(hooks.contains(&"load".to_string()));
        assert!(hooks.contains(&"initialize".to_string()));
    }

    #[test]
    fn loader_hook_exports_for_parent_relative_source_returns_hooks() {
        let hooks = loader_hook_exports_for_source("../hooks/loader.mjs");
        assert!(!hooks.is_empty());
    }

    #[test]
    fn loader_hook_exports_for_absolute_source_returns_hooks() {
        let hooks = loader_hook_exports_for_source("/absolute/loader.mjs");
        assert!(!hooks.is_empty());
    }

    #[test]
    fn loader_hook_exports_for_file_url_source_returns_hooks() {
        let hooks = loader_hook_exports_for_source("file:///home/user/loader.mjs");
        assert!(!hooks.is_empty());
    }

    #[test]
    fn loader_hook_exports_for_bare_package_returns_empty() {
        let hooks = loader_hook_exports_for_source("some-loader-package");
        assert!(
            hooks.is_empty(),
            "bare package specifiers must return no hooks"
        );
    }

    // -------------------------------------------------------------------------
    // local_fork_source
    // -------------------------------------------------------------------------

    #[test]
    fn local_fork_source_relative_dot_slash_accepted() {
        assert_eq!(
            local_fork_source("./worker.js").as_deref(),
            Some("./worker.js")
        );
    }

    #[test]
    fn local_fork_source_parent_relative_accepted() {
        assert_eq!(
            local_fork_source("../runner.js").as_deref(),
            Some("../runner.js")
        );
    }

    #[test]
    fn local_fork_source_trailing_slash_rejected() {
        assert!(local_fork_source("./workers/").is_none());
    }

    #[test]
    fn local_fork_source_bare_package_rejected() {
        assert!(local_fork_source("worker-threads").is_none());
    }

    // -------------------------------------------------------------------------
    // is_dompurify_source
    // -------------------------------------------------------------------------

    #[test]
    fn is_dompurify_source_exact_dompurify() {
        assert!(is_dompurify_source("dompurify"));
    }

    #[test]
    fn is_dompurify_source_isomorphic_variant() {
        assert!(is_dompurify_source("isomorphic-dompurify"));
    }

    #[test]
    fn is_dompurify_source_other_package_returns_false() {
        assert!(!is_dompurify_source("sanitize-html"));
    }

    // -------------------------------------------------------------------------
    // is_child_process_source / is_node_path_source / is_node_url_source
    // -------------------------------------------------------------------------

    #[test]
    fn is_child_process_source_both_forms() {
        assert!(is_child_process_source("child_process"));
        assert!(is_child_process_source("node:child_process"));
        assert!(!is_child_process_source("node:path"));
    }

    #[test]
    fn is_node_path_source_both_forms() {
        assert!(is_node_path_source("path"));
        assert!(is_node_path_source("node:path"));
        assert!(!is_node_path_source("child_process"));
    }

    #[test]
    fn is_node_url_source_both_forms() {
        assert!(is_node_url_source("url"));
        assert!(is_node_url_source("node:url"));
        assert!(!is_node_url_source("path"));
    }

    // -------------------------------------------------------------------------
    // append_fixture_path (private, tested indirectly through its invariant)
    // -------------------------------------------------------------------------

    #[test]
    fn append_unique_paths_adds_only_new_entries() {
        let mut target = vec!["a".to_string(), "b".to_string()];
        append_unique_paths(&mut target, vec!["b".to_string(), "c".to_string()]);
        assert_eq!(
            target,
            vec!["a", "b", "c"],
            "duplicate 'b' must not be added"
        );
    }

    #[test]
    fn append_unique_paths_adds_all_when_none_duplicate() {
        let mut target = vec!["x".to_string()];
        append_unique_paths(&mut target, vec!["y".to_string(), "z".to_string()]);
        assert_eq!(target, vec!["x", "y", "z"]);
    }
}
