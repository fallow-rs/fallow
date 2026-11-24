//! Pinia `defineStore` member harvesting helpers.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::MemberAccess;
use crate::js_text::JsText;
use crate::visitor::FunctionTypeAliasBinding;
use fallow_types::extract::{MemberInfo, MemberKind};

use super::visit_structural::nearest_type_binding;
use super::{
    BindingTarget, ModuleInfoExtractor, extract_arrow_return_expr,
    extract_function_body_final_return_expr, static_member_object_name, unwrap_paren_expr,
};

/// The Pinia store property that holds the whole state object.
const PINIA_STATE_PROPERTY: &str = "$state";

/// The receiver of a Pinia store member read.
enum StoreReceiver {
    /// An inline factory call, `useFooStore()`: credit the factory import.
    Factory(String),
    /// A bound store instance local or member path: credit it through its
    /// binding target.
    Local(String),
}

/// Whether a call's callee is a bare `defineStore` identifier (Pinia). The
/// harvest is loose here; the analyzer's `pinia` / `@pinia/nuxt` dependency
/// gate is the activation boundary.
fn is_define_store_callee(callee: &Expression<'_>) -> bool {
    matches!(callee, Expression::Identifier(id) if id.name.as_str() == "defineStore")
}

/// Harvest declared store members from a `defineStore('id', <arg>)` second
/// argument. Option store: `state` returned-object keys + `getters` keys +
/// `actions` keys. Setup store: the last return statement's object-literal
/// keys. Names starting with `$` (Pinia's `$patch` / `$reset` / `$subscribe`
/// API) are excluded. A setup store whose returned object spreads
/// (`return { ...base, count }`) abstains, keeping the whole store opaque.
fn harvest_define_store_members(args: &[Argument<'_>]) -> Option<Vec<MemberInfo>> {
    let second = args.get(1)?.as_expression()?;
    match unwrap_paren_expr(second) {
        Expression::ObjectExpression(obj) => Some(harvest_option_store(obj)),
        Expression::ArrowFunctionExpression(arrow) => {
            harvest_setup_return(extract_arrow_return_expr(arrow)?)
        }
        Expression::FunctionExpression(func) => harvest_setup_return(
            extract_function_body_final_return_expr(func.body.as_ref()?)?,
        ),
        _ => None,
    }
}

/// Returned setup-store keys whose local binding the setup body reads outside
/// the final `return`. Pinia needs returned state for registration and
/// hydration, so a member that a live getter or action reads is live too.
/// Only a block-body setup function can read a binding before its return.
fn setup_store_members_read_in_body(args: &[Argument<'_>]) -> Vec<String> {
    let Some(second) = args.get(1).and_then(Argument::as_expression) else {
        return Vec::new();
    };
    let body = match unwrap_paren_expr(second) {
        Expression::ArrowFunctionExpression(arrow) => match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => body,
            _ => return Vec::new(),
        },
        Expression::FunctionExpression(func) => match func.body.as_ref() {
            Some(body) => body,
            None => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    let Some((Statement::ReturnStatement(ret), before_return)) = body.statements.split_last()
    else {
        return Vec::new();
    };
    let Some(Expression::ObjectExpression(obj)) = ret.argument.as_ref().map(unwrap_paren_expr)
    else {
        return Vec::new();
    };
    let mut keys_by_local: FxHashMap<&str, Vec<String>> = FxHashMap::default();
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let (Some(key), Expression::Identifier(local)) = (prop.key.static_name(), &prop.value)
        else {
            continue;
        };
        if key.as_js_str().starts_with('$') {
            continue;
        }
        keys_by_local
            .entry(local.name.as_str())
            .or_default()
            .push(key.text().into_owned());
    }
    if keys_by_local.is_empty() {
        return Vec::new();
    }
    let mut reader = SetupBodyReadCollector {
        wanted: &keys_by_local,
        read: FxHashSet::default(),
    };
    for statement in before_return {
        reader.visit_statement(statement);
    }
    let mut credited: Vec<String> = reader
        .read
        .into_iter()
        .filter_map(|local| keys_by_local.get(local))
        .flatten()
        .cloned()
        .collect();
    credited.sort_unstable();
    credited
}

/// Collect the wanted local names that a setup-store body references.
struct SetupBodyReadCollector<'w> {
    wanted: &'w FxHashMap<&'w str, Vec<String>>,
    read: FxHashSet<&'w str>,
}

impl<'a> Visit<'a> for SetupBodyReadCollector<'_> {
    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        if let Some((local, _)) = self.wanted.get_key_value(ident.name.as_str()) {
            self.read.insert(local);
        }
    }

    /// A plain write (`token.value = next`, `token = next`) does not read the
    /// state, so write-only state stays reported. Compound operators read first.
    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        if expr.operator != AssignmentOperator::Assign {
            walk::walk_assignment_expression(self, expr);
            return;
        }
        match &expr.left {
            AssignmentTarget::AssignmentTargetIdentifier(_) => {}
            AssignmentTarget::StaticMemberExpression(member)
                if matches!(member.object, Expression::Identifier(_)) => {}
            AssignmentTarget::ComputedMemberExpression(member)
                if matches!(member.object, Expression::Identifier(_)) =>
            {
                self.visit_expression(&member.expression);
            }
            left => self.visit_assignment_target(left),
        }
        self.visit_expression(&expr.right);
    }
}

/// Option-store state keys that a getter or an action reads through
/// `this.$state`. Pinia exposes each state key on the store, so
/// `this.$state.key` reads `key`. A `this.$state` value that no key read,
/// destructure or whole write consumes can reach every state key, so it
/// credits each key of `state`. It credits no getter or action, because
/// `$state` holds only the state.
fn option_store_state_read_through_this(args: &[Argument<'_>]) -> Vec<String> {
    let Some(Expression::ObjectExpression(options)) = args
        .get(1)
        .and_then(Argument::as_expression)
        .map(unwrap_paren_expr)
    else {
        return Vec::new();
    };
    let mut collector = ThisStateReadCollector::default();
    collector.visit_object_expression(options);
    if collector.escaped {
        return option_store_state_keys(options);
    }
    let mut keys: Vec<String> = collector.keys.into_iter().collect();
    keys.sort_unstable();
    keys
}

fn option_store_state_keys(options: &ObjectExpression<'_>) -> Vec<String> {
    let mut keys = Vec::new();
    for prop in &options.properties {
        if let ObjectPropertyKind::ObjectProperty(prop) = prop
            && prop.key.static_name().is_some_and(|name| name == "state")
            && let Some(state_obj) = state_returned_object(&prop.value)
        {
            let mut members = Vec::new();
            collect_store_member_keys(state_obj, &mut members);
            keys.extend(members.into_iter().map(|member| member.name));
        }
    }
    keys
}

/// The span start of a `this.$state` expression.
fn this_state_start(expr: &Expression<'_>) -> Option<u32> {
    match unwrap_paren_expr(expr) {
        Expression::StaticMemberExpression(member) if is_this_state(member) => {
            Some(member.span.start)
        }
        _ => None,
    }
}

fn is_this_state(member: &StaticMemberExpression<'_>) -> bool {
    member.property.name == PINIA_STATE_PROPERTY
        && matches!(member.object, Expression::ThisExpression(_))
}

/// Collect the state keys that an option store reads through `this.$state`.
/// A parent node consumes its `this.$state` child before the walk reaches the
/// child, so `named_uses` holds each consumed child when the child is visited.
#[derive(Default)]
struct ThisStateReadCollector {
    keys: FxHashSet<String>,
    named_uses: FxHashSet<u32>,
    escaped: bool,
}

impl<'a> Visit<'a> for ThisStateReadCollector {
    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        if let Some(start) = this_state_start(&expr.object) {
            self.named_uses.insert(start);
            self.keys.insert(expr.property.name.to_string());
        }
        if is_this_state(expr) && !self.named_uses.contains(&expr.span.start) {
            self.escaped = true;
        }
        walk::walk_static_member_expression(self, expr);
    }

    fn visit_computed_member_expression(&mut self, expr: &ComputedMemberExpression<'a>) {
        if let Some(start) = this_state_start(&expr.object)
            && let Some(key) = expr.static_property_name()
        {
            self.named_uses.insert(start);
            self.keys.insert(key.text().into_owned());
        }
        walk::walk_computed_member_expression(self, expr);
    }

    /// Pinia applies `this.$state = { ... }` as a patch, and the write reads
    /// no key.
    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        if expr.operator == AssignmentOperator::Assign
            && let AssignmentTarget::StaticMemberExpression(member) = &expr.left
            && is_this_state(member)
        {
            self.named_uses.insert(member.span.start);
        }
        walk::walk_assignment_expression(self, expr);
    }

    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        if let BindingPattern::ObjectPattern(pattern) = &decl.id
            && pattern.rest.is_none()
            && let Some(start) = decl.init.as_ref().and_then(this_state_start)
            && let Some(keys) = pattern
                .properties
                .iter()
                .map(|prop| prop.key.static_name().map(|name| name.text().into_owned()))
                .collect::<Option<Vec<_>>>()
        {
            self.named_uses.insert(start);
            self.keys.extend(keys);
        }
        walk::walk_variable_declarator(self, decl);
    }
}

fn harvest_option_store(obj: &ObjectExpression<'_>) -> Vec<MemberInfo> {
    let mut members = Vec::new();
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(section) = prop.key.static_name() else {
            continue;
        };
        match &*section.text() {
            "state" => {
                if let Some(state_obj) = state_returned_object(&prop.value) {
                    collect_store_member_keys(state_obj, &mut members);
                }
            }
            "getters" | "actions" => {
                if let Expression::ObjectExpression(section_obj) = unwrap_paren_expr(&prop.value) {
                    collect_store_member_keys(section_obj, &mut members);
                }
            }
            _ => {}
        }
    }
    members
}

/// The object returned by a Pinia option-store `state` value (an arrow or
/// function returning an object literal: `state: () => ({ count: 0 })`).
fn state_returned_object<'a, 'b>(value: &'b Expression<'a>) -> Option<&'b ObjectExpression<'a>> {
    let returned = match value {
        Expression::ArrowFunctionExpression(arrow) => extract_arrow_return_expr(arrow)?,
        Expression::FunctionExpression(func) => {
            extract_function_body_final_return_expr(func.body.as_ref()?)?
        }
        _ => return None,
    };
    match unwrap_paren_expr(returned) {
        Expression::ObjectExpression(obj) => Some(obj),
        _ => None,
    }
}

/// Harvest members from a setup-store return expression. Abstains on a spread so
/// the whole store stays opaque rather than under-counting.
fn harvest_setup_return(returned: &Expression<'_>) -> Option<Vec<MemberInfo>> {
    let Expression::ObjectExpression(obj) = unwrap_paren_expr(returned) else {
        return None;
    };
    if obj
        .properties
        .iter()
        .any(|prop| matches!(prop, ObjectPropertyKind::SpreadProperty(_)))
    {
        return None;
    }
    let mut members = Vec::new();
    collect_store_member_keys(obj, &mut members);
    Some(members)
}

fn collect_store_member_keys(obj: &ObjectExpression<'_>, members: &mut Vec<MemberInfo>) {
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(name) = prop.key.static_name() else {
            continue;
        };
        if name.as_js_str().starts_with('$') {
            continue;
        }
        members.push(MemberInfo {
            name: name.text().into_owned(),
            kind: MemberKind::StoreMember,
            span: prop.key.span(),
            has_decorator: false,
            decorator_names: Vec::new(),
            is_instance_returning_static: false,
            is_self_returning: false,
            override_base: None,
            is_protected: false,
            is_static: false,
            pairs_with_symbol_dispose: false,
        });
    }
}

impl ModuleInfoExtractor {
    /// Recognize Pinia `defineStore` declarations and store-consumption shapes.
    pub(super) fn record_pinia_store(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Expression::CallExpression(call) = init
            && is_define_store_callee(&call.callee)
        {
            if let Some(members) = harvest_define_store_members(&call.arguments) {
                for member in option_store_state_read_through_this(&call.arguments) {
                    self.member_accesses.push(MemberAccess {
                        object: id.name.to_string(),
                        member,
                    });
                }
                self.store_member_decls.insert(id.name.to_string(), members);
            }
            for member in setup_store_members_read_in_body(&call.arguments) {
                self.member_accesses.push(MemberAccess {
                    object: id.name.to_string(),
                    member,
                });
            }
            return;
        }

        if let BindingPattern::BindingIdentifier(id) = &declarator.id
            && let Expression::CallExpression(call) = init
            && let Expression::Identifier(callee) = &call.callee
            && self.is_store_factory_call(callee.name.as_str())
        {
            self.insert_class_binding_target_if_absent(
                id.name.to_string(),
                callee.name.to_string(),
            );
            self.store_instance_locals.insert(id.name.to_string());
            return;
        }

        let BindingPattern::ObjectPattern(obj_pat) = &declarator.id else {
            return;
        };
        if obj_pat.rest.is_some() {
            return;
        }
        if let Expression::StaticMemberExpression(state) = init
            && obj_pat
                .properties
                .iter()
                .all(|prop| prop.key.static_name().is_some())
            && let Some(receiver) = self.pinia_state_receiver(state)
        {
            self.pinia_state_named_uses.insert(state.span.start);
            let object = match receiver {
                StoreReceiver::Factory(name) | StoreReceiver::Local(name) => name,
            };
            self.credit_store_pattern_members(obj_pat, &object);
            return;
        }

        self.record_pinia_object_pattern_store_members(obj_pat, init);
    }

    fn record_pinia_object_pattern_store_members(
        &mut self,
        obj_pat: &ObjectPattern<'_>,
        init: &Expression<'_>,
    ) {
        match init {
            Expression::CallExpression(call) => {
                let Expression::Identifier(callee) = &call.callee else {
                    return;
                };
                if matches!(callee.name.as_str(), "storeToRefs" | "toRefs") {
                    if let Some(object_name) = call
                        .arguments
                        .first()
                        .and_then(|arg| self.store_name_from_refs_arg(arg))
                    {
                        self.credit_store_pattern_members(obj_pat, object_name);
                    }
                } else if self.is_store_factory_call(callee.name.as_str()) {
                    self.credit_store_pattern_members(obj_pat, callee.name.as_str());
                }
            }
            Expression::Identifier(ident)
                if self.store_instance_locals.contains(ident.name.as_str()) =>
            {
                self.credit_store_pattern_members(obj_pat, ident.name.as_str());
            }
            Expression::StaticMemberExpression(_) => {
                if let Some(path) = static_member_object_name(init)
                    && let Some(BindingTarget::Class(factory)) =
                        self.binding_target_names.get(&path)
                    && factory.starts_with("use")
                    && factory.ends_with("Store")
                {
                    let factory = factory.clone();
                    self.credit_store_pattern_members(obj_pat, &factory);
                }
            }
            _ => {}
        }
    }

    /// The store receiver of a `<store>.$state` expression, or `None` when the
    /// expression is not `$state` or its object is not a known store.
    fn pinia_state_receiver(&self, state: &StaticMemberExpression<'_>) -> Option<StoreReceiver> {
        if state.property.name != PINIA_STATE_PROPERTY {
            return None;
        }
        if let Some(factory) = Self::inline_store_factory_receiver(&state.object) {
            return Some(StoreReceiver::Factory(factory));
        }
        let path = static_member_object_name(&state.object)?;
        let is_store = self.store_instance_locals.contains(&path)
            || matches!(
                self.binding_target_names.get(&path),
                Some(BindingTarget::Class(factory))
                    if factory.starts_with("use") && factory.ends_with("Store")
            );
        is_store.then_some(StoreReceiver::Local(path))
    }

    /// Credit `key` on the store when `object` is `<store>.$state`
    /// (`store.$state.key`, `store.$state['key']`). Pinia exposes each state
    /// key on the store itself, so the read is the same as `store.key`.
    pub(super) fn record_pinia_state_key_read(&mut self, object: &Expression<'_>, key: &str) {
        let Expression::StaticMemberExpression(state) = unwrap_paren_expr(object) else {
            return;
        };
        let Some(receiver) = self.pinia_state_receiver(state) else {
            return;
        };
        self.pinia_state_named_uses.insert(state.span.start);
        match receiver {
            StoreReceiver::Factory(factory) => self.member_accesses.push(MemberAccess {
                object: factory,
                member: key.to_string(),
            }),
            StoreReceiver::Local(path) => self.record_named_member_access(&path, key),
        }
    }

    /// Mark a whole `store.$state = { ... }` write as consumed. Pinia applies
    /// the write as a patch, and the write reads no key.
    pub(super) fn record_pinia_state_write(&mut self, expr: &AssignmentExpression<'_>) {
        if expr.operator == AssignmentOperator::Assign
            && let AssignmentTarget::StaticMemberExpression(member) = &expr.left
            && member.property.name == PINIA_STATE_PROPERTY
        {
            self.pinia_state_named_uses.insert(member.span.start);
        }
    }

    /// Record a `<store>.$state.key` read on the store, and mark the whole store
    /// as used for a store `$state` value that no key read, destructure or write
    /// consumes (a call argument, a spread, an alias), as it can reach every key.
    pub(super) fn record_pinia_state_access(&mut self, expr: &StaticMemberExpression<'_>) {
        self.record_pinia_state_key_read(&expr.object, expr.property.name.as_str());
        self.record_pinia_state_escape(expr);
    }

    fn record_pinia_state_escape(&mut self, state: &StaticMemberExpression<'_>) {
        if self.pinia_state_named_uses.contains(&state.span.start) {
            return;
        }
        let Some(receiver) = self.pinia_state_receiver(state) else {
            return;
        };
        self.push_whole_object_use(match receiver {
            StoreReceiver::Factory(name) | StoreReceiver::Local(name) => name,
        });
    }

    /// Options-API map helpers reference store members as string-array or object
    /// arguments. Mark store-factory arguments as whole-object uses.
    pub(super) fn record_pinia_map_helpers(&mut self, expr: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &expr.callee else {
            return;
        };
        if !matches!(
            callee.name.as_str(),
            "mapState" | "mapGetters" | "mapActions" | "mapWritableState" | "mapStores"
        ) {
            return;
        }
        for arg in &expr.arguments {
            if let Argument::Identifier(ident) = arg {
                self.whole_object_uses.push(ident.name.to_string());
            }
        }
    }

    /// Track a module-scope `const NAME = "literal"` so the `unprovided-inject`
    /// detector treats an inject/provide keyed by `NAME` as a string-keyed DI
    /// link (string identity), not a symbol.
    pub(super) fn record_di_string_key_const(
        &mut self,
        name: &str,
        decl: &VariableDeclaration<'_>,
        init: &Expression<'_>,
    ) {
        if decl.kind != VariableDeclarationKind::Const || !self.is_module_scope() {
            return;
        }
        let is_string_literal = match init {
            Expression::StringLiteral(_) => true,
            Expression::TemplateLiteral(t) => t.expressions.is_empty(),
            _ => false,
        };
        if is_string_literal {
            self.string_keyed_di_consts.insert(name.to_string());
        }
    }

    /// Whether a bare-identifier callee is a Pinia store factory.
    fn is_store_factory_call(&self, name: &str) -> bool {
        self.imports.iter().any(|i| i.local_name == name)
            || (name.starts_with("use") && name.ends_with("Store"))
    }

    /// For an inline `useFooStore().member` receiver, return the factory name so
    /// the member is credited on the store export directly.
    pub(super) fn inline_store_factory_receiver(object: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = object else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        let name = callee.name.as_str();
        (name.starts_with("use") && name.ends_with("Store")).then(|| name.to_string())
    }

    /// Match `ReturnType<typeof useFooStore>` and return the store factory name.
    pub(super) fn store_factory_from_return_type(ty: &TSType<'_>) -> Option<String> {
        let TSType::TSTypeReference(type_ref) = ty else {
            return None;
        };
        let TSTypeName::IdentifierReference(root) = &type_ref.type_name else {
            return None;
        };
        if root.name != "ReturnType" {
            return None;
        }
        let TSType::TSTypeQuery(query) = type_ref.type_arguments.as_deref()?.params.first()? else {
            return None;
        };
        let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name else {
            return None;
        };
        let factory = ident.name.to_string();
        (factory.starts_with("use") && factory.ends_with("Store")).then_some(factory)
    }

    /// The store factory of each object-type property typed with an inline
    /// `ReturnType<typeof useFooStore>`, by dotted property path.
    /// `{ deps: { store: ReturnType<typeof useFooStore> } }` maps `deps.store`
    /// to `useFooStore`. The type name of such a property is `ReturnType`, so
    /// a lookup by type name cannot find the store.
    pub(super) fn store_return_type_properties(ty: &TSType<'_>) -> FxHashMap<String, String> {
        let mut properties = FxHashMap::default();
        Self::collect_store_return_type_properties(ty, None, &mut properties);
        properties
    }

    fn collect_store_return_type_properties(
        ty: &TSType<'_>,
        prefix: Option<&str>,
        properties: &mut FxHashMap<String, String>,
    ) {
        match ty {
            TSType::TSTypeLiteral(type_lit) => {
                for member in &type_lit.members {
                    let TSSignature::TSPropertySignature(prop) = member else {
                        continue;
                    };
                    let (Some(name), Some(annotation)) =
                        (prop.key.static_name(), prop.type_annotation.as_deref())
                    else {
                        continue;
                    };
                    let path = match prefix {
                        Some(prefix) => format!("{prefix}.{}", name.text()),
                        None => name.text().into_owned(),
                    };
                    let property_type = &annotation.type_annotation;
                    if let Some(factory) = Self::store_factory_of_property_type(property_type) {
                        properties.insert(path, factory);
                    } else {
                        Self::collect_store_return_type_properties(
                            property_type,
                            Some(&path),
                            properties,
                        );
                    }
                }
            }
            TSType::TSParenthesizedType(paren) => {
                Self::collect_store_return_type_properties(
                    &paren.type_annotation,
                    prefix,
                    properties,
                );
            }
            _ => {}
        }
    }

    /// The store factory of a property type `ReturnType<typeof useFooStore>`,
    /// also in parentheses or in a union with `null` or `undefined`.
    fn store_factory_of_property_type(ty: &TSType<'_>) -> Option<String> {
        match ty {
            TSType::TSParenthesizedType(paren) => {
                Self::store_factory_of_property_type(&paren.type_annotation)
            }
            TSType::TSUnionType(union) => {
                let mut present = union.types.iter().filter(|branch| {
                    !matches!(
                        branch,
                        TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_)
                    )
                });
                let only = present.next()?;
                if present.next().is_some() {
                    return None;
                }
                Self::store_factory_of_property_type(only)
            }
            _ => Self::store_factory_from_return_type(ty),
        }
    }

    /// Resolve a param type annotation to a store factory, accepting either the
    /// inline `ReturnType<typeof useFooStore>` form or a recorded alias of it.
    pub(super) fn store_factory_for_type(&self, ty: &TSType<'_>) -> Option<String> {
        if let Some(factory) = Self::store_factory_from_return_type(ty) {
            return Some(factory);
        }
        if let TSType::TSTypeReference(type_ref) = ty
            && let TSTypeName::IdentifierReference(ident) = &type_ref.type_name
        {
            return self.store_factory_for_type_name(ident.name.as_str());
        }
        None
    }

    /// Resolve a bare type-reference name (`CounterStore`) to its store factory
    /// via a recorded `type CounterStore = ReturnType<typeof useCounterStore>`
    /// alias. An alias in the nearest function body or block wins over a
    /// module alias, and another local type of that name shadows it.
    pub(super) fn store_factory_for_type_name(&self, type_name: &str) -> Option<String> {
        match nearest_type_binding(&self.function_type_alias_scopes, type_name) {
            Some(FunctionTypeAliasBinding::Store(factory)) => Some(factory.clone()),
            Some(FunctionTypeAliasBinding::ClassSelf) | None => {
                self.type_alias_store_factory.get(type_name).cloned()
            }
            Some(_) => None,
        }
    }

    fn store_name_from_refs_arg<'a>(&self, arg: &'a Argument<'_>) -> Option<&'a str> {
        match arg {
            Argument::Identifier(ident)
                if self.store_instance_locals.contains(ident.name.as_str()) =>
            {
                Some(ident.name.as_str())
            }
            Argument::CallExpression(call) => {
                let Expression::Identifier(callee) = &call.callee else {
                    return None;
                };
                self.is_store_factory_call(callee.name.as_str())
                    .then_some(callee.name.as_str())
            }
            Argument::ParenthesizedExpression(paren) => {
                self.store_name_from_refs_expression(&paren.expression)
            }
            _ => None,
        }
    }

    fn store_name_from_refs_expression<'a>(&self, expr: &'a Expression<'_>) -> Option<&'a str> {
        match expr {
            Expression::Identifier(ident)
                if self.store_instance_locals.contains(ident.name.as_str()) =>
            {
                Some(ident.name.as_str())
            }
            Expression::CallExpression(call) => {
                let Expression::Identifier(callee) = &call.callee else {
                    return None;
                };
                self.is_store_factory_call(callee.name.as_str())
                    .then_some(callee.name.as_str())
            }
            Expression::ParenthesizedExpression(paren) => {
                self.store_name_from_refs_expression(&paren.expression)
            }
            _ => None,
        }
    }

    /// Emit a `MemberAccess` for each statically-named destructured key against
    /// `object_name` (the store instance local or factory import).
    fn credit_store_pattern_members(&mut self, obj_pat: &ObjectPattern<'_>, object_name: &str) {
        for prop in &obj_pat.properties {
            let Some(member) = prop.key.static_name() else {
                continue;
            };
            self.member_accesses.push(MemberAccess {
                object: object_name.to_string(),
                member: member.text().into_owned(),
            });
        }
    }

    /// Copy harvested store members onto the matching `ExportInfo`, mirroring
    /// `enrich_local_class_exports`.
    pub(in crate::visitor) fn enrich_store_exports(&mut self) {
        if self.store_member_decls.is_empty() {
            return;
        }
        for export in &mut self.exports {
            if !export.members.is_empty() {
                continue;
            }
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            if let Some(members) = self.store_member_decls.get(local_name) {
                export.members = members.clone();
            }
        }
    }
}
