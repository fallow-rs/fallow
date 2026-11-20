//! Computed member keys with known string values (issue #4799).
//!
//! In `view[action]()`, TypeScript allows only keys that `action` can hold.
//! When `action` is typed as a union of string literals, the access reads
//! exactly the members that the union names. A key with another type reads an
//! unknown member, so the receiver counts as a use of the whole object.
//!
//! The visitor does not resolve scopes, so a key binding narrows only when no
//! other binding of its name can be the one that the key reads. A parameter
//! must be the only declaration of its name in its function. A variable must
//! be the only declaration of its name in the file. Any other binding of the
//! name (a `catch` parameter, a hoisted `var`, a later `const`, an import)
//! then keeps the whole-object use.

use oxc_ast::ast::{
    ArrowFunctionExpression, BindingIdentifier, BindingPattern, ComputedMemberExpression,
    Expression, FormalParameter, FormalParameters, Function, Program, TSLiteral, TSType,
    VariableDeclaration, VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::ScopeFlags;
use rustc_hash::{FxHashMap, FxHashSet};

use super::super::ModuleInfoExtractor;
use super::visit_chain_names::static_member_object_name;
use crate::js_text::JsText;

/// The string values of a type that is a string literal or a union of only
/// string literals. Any other branch gives `None`.
fn string_literal_union(ty: &TSType<'_>) -> Option<Vec<String>> {
    let mut values = Vec::new();
    collect_string_literals(ty, &mut values).then_some(values)
}

fn collect_string_literals(ty: &TSType<'_>, values: &mut Vec<String>) -> bool {
    match ty {
        TSType::TSLiteralType(literal) => match &literal.literal {
            TSLiteral::StringLiteral(value) => {
                values.push(value.value.text().into_owned());
                true
            }
            _ => false,
        },
        TSType::TSUnionType(union) => union
            .types
            .iter()
            .all(|branch| collect_string_literals(branch, values)),
        TSType::TSParenthesizedType(paren) => {
            collect_string_literals(&paren.type_annotation, values)
        }
        _ => false,
    }
}

/// The key values of one declarator with a plain identifier pattern and a
/// string-literal union annotation.
/// A `const` with a string literal value and no annotation is no key binding,
/// so `const k = "x"; E[k]` keeps its whole-object use.
fn declarator_literal_keys(declarator: &VariableDeclarator<'_>) -> Option<Vec<String>> {
    if !matches!(declarator.id, BindingPattern::BindingIdentifier(_)) {
        return None;
    }
    string_literal_union(&declarator.type_annotation.as_deref()?.type_annotation)
}

fn parameter_literal_keys(param: &FormalParameter<'_>) -> Option<Vec<String>> {
    let BindingPattern::BindingIdentifier(_) = &param.pattern else {
        return None;
    };
    string_literal_union(&param.type_annotation.as_deref()?.type_annotation)
}

/// Finds the key bindings that no other binding of the same name can hide.
#[derive(Default)]
struct TrustedLiteralKeyCollector<'a> {
    file_declarations: FxHashMap<&'a str, u32>,
    /// Declaration counts of each enclosing function, innermost last.
    function_declarations: Vec<FxHashMap<&'a str, u32>>,
    /// Candidate parameters of each enclosing function, innermost last.
    function_parameters: Vec<Vec<(&'a str, u32)>>,
    variables: Vec<(&'a str, u32)>,
    trusted: FxHashSet<u32>,
}

impl<'a> TrustedLiteralKeyCollector<'a> {
    fn enter_function(&mut self, params: &FormalParameters<'a>) {
        let candidates = params
            .items
            .iter()
            .filter_map(|param| match &param.pattern {
                BindingPattern::BindingIdentifier(id)
                    if parameter_literal_keys(param).is_some() =>
                {
                    Some((id.name.as_str(), id.span.start))
                }
                _ => None,
            })
            .collect();
        self.function_declarations.push(FxHashMap::default());
        self.function_parameters.push(candidates);
    }

    fn exit_function(&mut self) {
        let counts = self.function_declarations.pop().unwrap_or_default();
        for (name, start) in self.function_parameters.pop().unwrap_or_default() {
            if counts.get(name) == Some(&1) {
                self.trusted.insert(start);
            }
        }
    }
}

impl<'a> Visit<'a> for TrustedLiteralKeyCollector<'a> {
    fn visit_binding_identifier(&mut self, id: &BindingIdentifier<'a>) {
        let name = id.name.as_str();
        *self.file_declarations.entry(name).or_default() += 1;
        for counts in &mut self.function_declarations {
            *counts.entry(name).or_default() += 1;
        }
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        self.enter_function(&func.params);
        walk::walk_function(self, func, flags);
        self.exit_function();
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        self.enter_function(&expr.params);
        walk::walk_arrow_function_expression(self, expr);
        self.exit_function();
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        for declarator in &decl.declarations {
            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && declarator_literal_keys(declarator).is_some()
            {
                self.variables.push((id.name.as_str(), id.span.start));
            }
        }
        walk::walk_variable_declaration(self, decl);
    }
}

/// The string-literal key bindings of the visited program.
#[derive(Default)]
pub(in crate::visitor) struct LiteralKeyBindings {
    /// Module-scope bindings. `None` means the name has no trusted key type,
    /// which hides any outer match.
    module: FxHashMap<String, Option<Vec<String>>>,
    /// Bindings of the nested function and block scopes, innermost last.
    pub(in crate::visitor) stack: Vec<FxHashMap<String, Option<Vec<String>>>>,
    /// Start offsets of the key binding identifiers that no other binding of
    /// the same name can hide. Only these bindings narrow a computed key.
    trusted: FxHashSet<u32>,
}

impl ModuleInfoExtractor {
    /// Credit the members that a computed key can name on its receiver: an
    /// identifier or a static member chain such as `this.view`. A key that
    /// can name any member makes the receiver a whole-object use.
    pub(in crate::visitor) fn record_computed_key_access(
        &mut self,
        expr: &ComputedMemberExpression<'_>,
    ) {
        let object_name = match &expr.object {
            Expression::Identifier(obj) => obj.name.to_string(),
            Expression::StaticMemberExpression(_) => {
                let Some(name) = static_member_object_name(&expr.object) else {
                    return;
                };
                name
            }
            _ => return,
        };
        if self.namespace_like_binding_is_shadowed(&object_name)
            || self.namespace_import_root_is_shadowed(&object_name)
        {
            return;
        }
        match self.computed_key_members(&expr.expression) {
            Some(members) => {
                for member in members {
                    self.record_named_member_access(&object_name, &member);
                }
                if let Expression::Identifier(obj) = &expr.object {
                    self.mark_structured_namespace_reference(obj);
                }
            }
            None => self.record_whole_object_identifier_use(&object_name),
        }
    }

    /// Collect the key bindings of a program that no other binding can hide,
    /// by the start offset of their binding identifier.
    pub(in crate::visitor) fn record_trusted_literal_keys(&mut self, program: &Program<'_>) {
        let mut collector = TrustedLiteralKeyCollector::default();
        collector.visit_program(program);
        let mut trusted = collector.trusted;
        for (name, start) in collector.variables {
            if collector.file_declarations.get(name) == Some(&1) {
                trusted.insert(start);
            }
        }
        self.literal_keys.trusted = trusted;
    }

    /// The frame of string-literal key bindings for the parameters of a
    /// function. Each parameter hides an outer binding of the same name.
    pub(in crate::visitor) fn parameter_literal_key_scope(
        &self,
        params: &FormalParameters<'_>,
    ) -> FxHashMap<String, Option<Vec<String>>> {
        let mut scope = FxHashMap::default();
        for param in &params.items {
            for id in param.pattern.get_binding_identifiers() {
                scope.insert(id.name.to_string(), None);
            }
            if let BindingPattern::BindingIdentifier(id) = &param.pattern
                && self.literal_keys.trusted.contains(&id.span.start)
            {
                scope.insert(id.name.to_string(), parameter_literal_keys(param));
            }
        }
        if let Some(rest) = &params.rest {
            for id in rest.rest.argument.get_binding_identifiers() {
                scope.insert(id.name.to_string(), None);
            }
        }
        scope
    }

    /// Record the key values of the names that a declarator binds.
    pub(in crate::visitor) fn record_literal_key_declarator(
        &mut self,
        declarator: &VariableDeclarator<'_>,
    ) {
        let keys = match &declarator.id {
            BindingPattern::BindingIdentifier(id)
                if self.literal_keys.trusted.contains(&id.span.start) =>
            {
                declarator_literal_keys(declarator)
            }
            _ => None,
        };
        for id in declarator.id.get_binding_identifiers() {
            self.record_literal_key_binding(id.name.as_str(), keys.clone());
        }
    }

    fn record_literal_key_binding(&mut self, name: &str, keys: Option<Vec<String>>) {
        if self.is_module_scope() {
            self.literal_keys.module.insert(name.to_string(), keys);
            return;
        }
        if let Some(bindings) = self.literal_keys.stack.last_mut() {
            bindings.insert(name.to_string(), keys);
        }
    }

    /// The member names that a computed key can name: a string literal, or an
    /// identifier bound with a string-literal union type. `None` when the key
    /// can name any member.
    fn computed_key_members(&self, key: &Expression<'_>) -> Option<Vec<String>> {
        self.computed_key_part_members(&computed_key_parts(key)?, None)
    }

    /// The member names of the parts of a computed key. `params` is the frame
    /// of a function whose parameters the scope stack does not hold yet.
    /// `None` when a part can name any member.
    pub(in crate::visitor) fn computed_key_part_members(
        &self,
        parts: &[ComputedKeyPart],
        params: Option<&FxHashMap<String, Option<Vec<String>>>>,
    ) -> Option<Vec<String>> {
        let mut members = Vec::new();
        for part in parts {
            match part {
                ComputedKeyPart::Literal(value) => members.push(value.clone()),
                ComputedKeyPart::Binding(name) => {
                    members.extend(self.literal_key_binding(name, params)?);
                }
            }
        }
        Some(members)
    }

    fn literal_key_binding(
        &self,
        name: &str,
        params: Option<&FxHashMap<String, Option<Vec<String>>>>,
    ) -> Option<Vec<String>> {
        for bindings in params
            .into_iter()
            .chain(self.literal_keys.stack.iter().rev())
        {
            if let Some(keys) = bindings.get(name) {
                return keys.clone();
            }
        }
        self.literal_keys.module.get(name).cloned().flatten()
    }
}

/// One branch of a computed key.
#[derive(Clone)]
pub(in crate::visitor) enum ComputedKeyPart {
    /// A string literal key: `view["zoomIn"]`.
    Literal(String),
    /// An identifier key whose binding type gives the values: `view[action]`.
    Binding(String),
}

/// The branches of a computed key: a string literal, an identifier, or a
/// conditional or parenthesized form of these. `None` for any other key.
pub(in crate::visitor) fn computed_key_parts(key: &Expression<'_>) -> Option<Vec<ComputedKeyPart>> {
    let mut parts = Vec::new();
    collect_computed_key_parts(key, &mut parts).then_some(parts)
}

fn collect_computed_key_parts(key: &Expression<'_>, parts: &mut Vec<ComputedKeyPart>) -> bool {
    match key {
        Expression::StringLiteral(value) => {
            parts.push(ComputedKeyPart::Literal(value.value.text().into_owned()));
            true
        }
        Expression::Identifier(id) => {
            parts.push(ComputedKeyPart::Binding(id.name.to_string()));
            true
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_computed_key_parts(&paren.expression, parts)
        }
        Expression::ConditionalExpression(conditional) => {
            collect_computed_key_parts(&conditional.consequent, parts)
                && collect_computed_key_parts(&conditional.alternate, parts)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::{Statement, TSType};
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    use super::string_literal_union;

    fn alias_values(source: &str) -> Option<Vec<String>> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        let Some(Statement::TSTypeAliasDeclaration(alias)) = parsed.program.body.first() else {
            panic!("expected a type alias");
        };
        let ty: &TSType<'_> = &alias.type_annotation;
        string_literal_union(ty)
    }

    #[test]
    fn union_of_string_literals_gives_each_value() {
        assert_eq!(
            alias_values(r#"type K = "a" | ("b" | "c");"#),
            Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
    }

    #[test]
    fn union_with_another_branch_gives_no_values() {
        assert_eq!(alias_values(r#"type K = "a" | string;"#), None);
        assert_eq!(alias_values(r#"type K = "a" | 1;"#), None);
        assert_eq!(alias_values("type K = Keys;"), None);
    }
}
