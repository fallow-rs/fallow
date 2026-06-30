//! Pinia `defineStore` member harvesting helpers.

#[allow(clippy::wildcard_imports, reason = "many Pinia helper AST types used")]
use oxc_ast::ast::*;
use oxc_span::GetSpan;

use crate::MemberAccess;
use fallow_types::extract::{MemberInfo, MemberKind};

use super::{
    BindingTarget, ModuleInfoExtractor, extract_arrow_return_expr,
    extract_function_body_final_return_expr, static_member_object_name, unwrap_paren_expr,
};

/// Whether a call's callee is a bare `defineStore` identifier (Pinia). The
/// harvest is loose here; the analyzer's `pinia` / `@pinia/nuxt` dependency
/// gate is the activation boundary.
pub(super) fn is_define_store_callee(callee: &Expression<'_>) -> bool {
    matches!(callee, Expression::Identifier(id) if id.name.as_str() == "defineStore")
}

/// Harvest declared store members from a `defineStore('id', <arg>)` second
/// argument. Option store: `state` returned-object keys + `getters` keys +
/// `actions` keys. Setup store: the last return statement's object-literal
/// keys. Names starting with `$` (Pinia's `$patch` / `$reset` / `$subscribe`
/// API) are excluded. A setup store whose returned object spreads
/// (`return { ...base, count }`) abstains, keeping the whole store opaque.
pub(super) fn harvest_define_store_members(args: &[Argument<'_>]) -> Option<Vec<MemberInfo>> {
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

fn harvest_option_store(obj: &ObjectExpression<'_>) -> Vec<MemberInfo> {
    let mut members = Vec::new();
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(section) = prop.key.static_name() else {
            continue;
        };
        match section.as_ref() {
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
        if name.starts_with('$') {
            continue;
        }
        members.push(MemberInfo {
            name: name.to_string(),
            kind: MemberKind::StoreMember,
            span: prop.key.span(),
            has_decorator: false,
            decorator_names: Vec::new(),
            is_instance_returning_static: false,
            is_self_returning: false,
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
                self.store_member_decls.insert(id.name.to_string(), members);
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

    /// Resolve a param type annotation to a store factory, accepting either the
    /// inline `ReturnType<typeof useFooStore>` form or a recorded alias of it.
    pub(super) fn store_factory_for_type(&self, ty: &TSType<'_>) -> Option<String> {
        if let Some(factory) = Self::store_factory_from_return_type(ty) {
            return Some(factory);
        }
        if let TSType::TSTypeReference(type_ref) = ty
            && let TSTypeName::IdentifierReference(ident) = &type_ref.type_name
        {
            return self
                .type_alias_store_factory
                .get(ident.name.as_str())
                .cloned();
        }
        None
    }

    /// Resolve a bare type-reference name (`CounterStore`) to its store factory
    /// via a recorded `type CounterStore = ReturnType<typeof useCounterStore>`
    /// alias.
    pub(super) fn store_factory_for_type_name(&self, type_name: &str) -> Option<String> {
        self.type_alias_store_factory.get(type_name).cloned()
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
                member: member.to_string(),
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
