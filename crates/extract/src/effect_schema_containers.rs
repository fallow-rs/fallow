//! Local object and array literals that hold Effect schemas, for
//! `effect.schemaExportsAsTypes`.
//!
//! A member read `record.User`, `tuple[0]` or `record["User"]` is a schema
//! when the root is a `const` binding of an object or array literal, and the
//! member with that literal key is a schema by the rules of
//! `effect_schema.rs`. A call `record.make()` is a schema when the member is
//! a function whose each `return` is a schema. The check reads syntax only,
//! so it accepts a literal only when no code can change the member:
//! - the literal has no spread, no computed key with a value that is not a
//!   literal, and no `this`;
//! - each value reference to the binding reads a member that the literal
//!   has, or is the source of a destructuring declaration, and no reference
//!   assigns, updates or deletes a member;
//! - the binding has one declaration, and the module does not export it.
//!
//! A destructured binding of such a literal, or of a literal in the
//! declaration itself (`const { User } = { User: Schema.String }`), is a
//! schema when its source member is one.

use oxc_ast::AstKind;
use oxc_ast::ast::{
    ArrayExpressionElement, BindingPattern, Expression, ObjectPropertyKind, PropertyKind,
    UnaryOperator,
};
use oxc_ast_visit::Visit;
use oxc_semantic::{AstNodes, NodeId, SymbolId};
use oxc_span::{GetSpan, Span};

use super::helpers::{HelperBody, arrow_body, function_body};
use super::{Link, SchemaShape, SchemaShapes, is_schema_tail, literal_key, unwrap};

/// The value of one member of a literal.
enum Member<'e, 'a> {
    Value(&'e Expression<'a>),
    Function(HelperBody<'e, 'a>),
}

impl<'a> SchemaShapes<'_, 'a> {
    /// The shape of a chain whose root is the literal container `symbol`.
    pub(super) fn container_chain(
        &self,
        symbol: SymbolId,
        links: &[Link<'_, 'a>],
    ) -> Option<SchemaShape> {
        let (key, rest) = match links.split_first()? {
            (Link::Member(name), rest) => (*name, rest),
            (Link::Key(key), rest) => (key.as_str(), rest),
            (Link::Call(_), _) => return None,
        };
        let literal = self.container_literal(symbol)?;
        let (shape, rest) = match literal_member(literal, key)? {
            Member::Value(value) => (self.member_shape(value)?, rest),
            Member::Function(body) => {
                let Some((Link::Call(call), rest)) = rest.split_first() else {
                    return None;
                };
                (self.body_call_shape(body, &call.arguments)?, rest)
            }
        };
        if matches!(rest.first(), Some(Link::Call(_))) || !is_schema_tail(rest) {
            return None;
        }
        Some(shape)
    }

    /// The bindings of a destructuring declaration that are schemas, with the
    /// shape of each.
    pub fn pattern_shapes(
        &self,
        pattern: &BindingPattern<'a>,
        init: &Expression<'a>,
    ) -> Vec<(SymbolId, String, SchemaShape)> {
        let mut bindings = Vec::new();
        let mut add = |target: &BindingPattern<'a>, key: &str| {
            let BindingPattern::BindingIdentifier(id) = target else {
                return;
            };
            if let Some(shape) = self.source_member_shape(init, key) {
                bindings.push((id.symbol_id(), id.name.to_string(), shape));
            }
        };
        match pattern {
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    if let Some(key) = property.key.static_name()
                        && let Some(key) = key.as_str()
                    {
                        add(&property.value, key);
                    }
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for (index, element) in array.elements.iter().enumerate() {
                    if let Some(element) = element {
                        add(element, &index.to_string());
                    }
                }
            }
            BindingPattern::BindingIdentifier(_) | BindingPattern::AssignmentPattern(_) => {}
        }
        bindings
    }

    fn source_member_shape(&self, init: &Expression<'a>, key: &str) -> Option<SchemaShape> {
        match unwrap(init) {
            Expression::Identifier(identifier) => {
                let symbol = self.symbol(identifier)?;
                if !self.is_container(symbol) {
                    return None;
                }
                self.container_chain(symbol, &[Link::Key(key.to_string())])
            }
            literal => match literal_member(literal, key)? {
                Member::Value(value) => self.member_shape(value),
                Member::Function(_) => None,
            },
        }
    }

    /// A member value can read its own container (`const a = { b: a.b }`).
    fn member_shape(&self, value: &Expression<'a>) -> Option<SchemaShape> {
        self.guarded(value.span().start, || self.expression_shape(value))
    }

    /// True when `symbol` is a `const` binding of a literal whose members no
    /// code can change.
    pub(super) fn is_container(&self, symbol: SymbolId) -> bool {
        if let Some(known) = self.containers.borrow().get(&symbol) {
            return *known;
        }
        let known = self
            .container_literal(symbol)
            .is_some_and(|literal| self.reads_members_only(symbol, literal));
        self.containers.borrow_mut().insert(symbol, known);
        known
    }

    /// The object or array literal that the `const` binding `symbol` holds.
    fn container_literal(&self, symbol: SymbolId) -> Option<&Expression<'a>> {
        let scoping = self.semantic.scoping();
        if !scoping.symbol_flags(symbol).is_const_variable() || scoping.symbol_is_mutated(symbol) {
            return None;
        }
        let mut declarations = scoping.symbol_declarations(symbol);
        let declaration = declarations.next()?;
        if declarations.next().is_some() {
            return None;
        }
        let nodes = self.semantic.nodes();
        let AstKind::VariableDeclarator(declarator) = nodes.kind(declaration) else {
            return None;
        };
        // Another module can write to the members of an exported literal.
        if matches!(
            nodes.parent_kind(nodes.parent_id(declaration)),
            AstKind::ExportDeclaration(_) | AstKind::ExportNamedDeclaration(_)
        ) {
            return None;
        }
        let literal = unwrap(declarator.init.as_ref()?);
        let shape_ok = match literal {
            Expression::ObjectExpression(_) | Expression::ArrayExpression(_) => !uses_this(literal),
            _ => false,
        };
        shape_ok.then_some(literal)
    }

    /// True when each value reference to `symbol` reads a member that the
    /// literal has, and no reference writes or deletes that member. A read
    /// of a key that the literal does not have can be an array method that
    /// changes the members (`tuple.shift()`).
    fn reads_members_only(&self, symbol: SymbolId, literal: &Expression<'a>) -> bool {
        let nodes = self.semantic.nodes();
        self.semantic
            .scoping()
            .get_resolved_references(symbol)
            .filter(|reference| reference.is_value())
            .all(|reference| {
                let node = reference.node_id();
                let reference_span = nodes.kind(node).span();
                let parent = nodes.parent_id(node);
                let (key, member_span) = match nodes.kind(parent) {
                    AstKind::StaticMemberExpression(member)
                        if member.object.span() == reference_span =>
                    {
                        (Some(member.property.name.to_string()), member.span)
                    }
                    AstKind::ComputedMemberExpression(member)
                        if member.object.span() == reference_span =>
                    {
                        (literal_key(&member.expression), member.span)
                    }
                    // `const [User] = tuple` copies members and keeps none.
                    AstKind::VariableDeclarator(declarator) => {
                        return !matches!(declarator.id, BindingPattern::BindingIdentifier(_))
                            && declarator.init.as_ref().map(GetSpan::span) == Some(reference_span);
                    }
                    _ => return false,
                };
                key.is_some_and(|key| literal_member(literal, &key).is_some())
                    && !is_written(nodes, parent, member_span)
            })
    }
}

/// True when the member read at `node` is the target of a write or a
/// `delete`, also through parentheses, a type assertion or an optional chain.
fn is_written(nodes: &AstNodes<'_>, node: NodeId, span: Span) -> bool {
    let mut current = node;
    let mut span = span;
    loop {
        let parent = nodes.parent_id(current);
        match nodes.kind(parent) {
            AstKind::ParenthesizedExpression(_)
            | AstKind::TSAsExpression(_)
            | AstKind::TSSatisfiesExpression(_)
            | AstKind::TSNonNullExpression(_)
            | AstKind::TSTypeAssertion(_)
            | AstKind::ChainExpression(_) => {
                current = parent;
                span = nodes.kind(parent).span();
            }
            AstKind::AssignmentExpression(assignment) => {
                return !contains(assignment.right.span(), span);
            }
            AstKind::ForInStatement(statement) => return !contains(statement.right.span(), span),
            AstKind::ForOfStatement(statement) => return !contains(statement.right.span(), span),
            AstKind::UnaryExpression(unary) => return unary.operator == UnaryOperator::Delete,
            AstKind::UpdateExpression(_)
            | AstKind::ArrayAssignmentTarget(_)
            | AstKind::ObjectAssignmentTarget(_)
            | AstKind::AssignmentTargetRest(_)
            | AstKind::AssignmentTargetWithDefault(_)
            | AstKind::AssignmentTargetPropertyIdentifier(_)
            | AstKind::AssignmentTargetPropertyProperty(_) => return true,
            _ => return false,
        }
    }
}

fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// The member with `key` of an object or array literal. A literal with a
/// spread, or with a computed key that is not a literal, has no known member.
fn literal_member<'e, 'a>(literal: &'e Expression<'a>, key: &str) -> Option<Member<'e, 'a>> {
    match literal {
        Expression::ObjectExpression(object) => {
            let mut found = None;
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    return None;
                };
                let name = property.key.static_name()?;
                if name.as_str() == Some(key) {
                    found = Some(property);
                }
            }
            let property = found?;
            if property.kind != PropertyKind::Init {
                return None;
            }
            value_member(&property.value)
        }
        Expression::ArrayExpression(array) => {
            if array
                .elements
                .iter()
                .any(|element| matches!(element, ArrayExpressionElement::SpreadElement(_)))
            {
                return None;
            }
            // `tuple["00"]` and `tuple["+0"]` are not elements.
            let index: usize = key
                .parse()
                .ok()
                .filter(|index: &usize| index.to_string() == key)?;
            value_member(array.elements.get(index)?.as_expression()?)
        }
        _ => None,
    }
}

fn value_member<'e, 'a>(value: &'e Expression<'a>) -> Option<Member<'e, 'a>> {
    Some(match unwrap(value) {
        Expression::ArrowFunctionExpression(arrow) => Member::Function(arrow_body(arrow)?),
        Expression::FunctionExpression(function) => Member::Function(function_body(function)?),
        _ => Member::Value(value),
    })
}

/// True when the literal reads `this`: a method can then change a member.
fn uses_this(literal: &Expression<'_>) -> bool {
    struct This(bool);
    impl Visit<'_> for This {
        fn visit_this_expression(&mut self, _: &oxc_ast::ast::ThisExpression) {
            self.0 = true;
        }
    }
    let mut finder = This(false);
    finder.visit_expression(literal);
    finder.0
}
