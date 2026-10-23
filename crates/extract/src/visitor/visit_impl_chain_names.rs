//! Dotted names of static member chains for visitor extraction.

#[allow(clippy::wildcard_imports, reason = "many AST node variants used")]
use oxc_ast::ast::*;
use rustc_hash::FxHashSet;

use super::super::{ModuleInfoExtractor, SemanticFact};
use super::visit_source_paths::join_member_path;
use crate::{FluentChainMemberAccessFact, FluentChainNewMemberAccessFact, MemberAccess};

/// The most links a recorded receiver name holds (`a` plus 64 `.b`).
///
/// No real receiver path is this deep, and factory credit reads only the start
/// of a path. Without a cap, each link of a long chain records the full name of
/// its receiver, so the output grows with the square of the chain length. The
/// builders return `None` past the cap, so every caller treats a deeper
/// receiver as a name it cannot read: it records no member access and no
/// binding key for it.
pub(super) const MAX_RECORDED_CHAIN_LINKS: usize = 64;

/// One link of a member chain name, recorded from the outermost link inward.
enum ChainLink<'b> {
    Property(&'b str),
    PrivateField(&'b str),
    Call,
}

/// The dotted name of a static member chain (`a.b.c`, `this.#dep`, `f().x`).
///
/// The visitor asks for this name at every link of a chain, so the walk
/// collects the links first and builds the string once. A recursive build
/// copies the parent name at each link, and a chain of a few thousand links
/// then takes seconds. The walk stops after `MAX_RECORDED_CHAIN_LINKS` links,
/// so the cost of one call stays bounded.
pub(super) fn static_member_object_name(expr: &Expression<'_>) -> Option<String> {
    let mut links = Vec::new();
    let mut current = expr;
    let root = loop {
        crate::cost::walk_step();
        if links.len() > MAX_RECORDED_CHAIN_LINKS {
            return None;
        }
        match current {
            Expression::ParenthesizedExpression(wrapper) => current = &wrapper.expression,
            Expression::TSAsExpression(wrapper) => current = &wrapper.expression,
            Expression::TSTypeAssertion(wrapper) => current = &wrapper.expression,
            Expression::TSSatisfiesExpression(wrapper) => current = &wrapper.expression,
            Expression::TSNonNullExpression(wrapper) => current = &wrapper.expression,
            Expression::Identifier(obj) => break obj.name.as_str(),
            Expression::ThisExpression(_) => break "this",
            Expression::StaticMemberExpression(member) => {
                links.push(ChainLink::Property(member.property.name.as_str()));
                current = &member.object;
            }
            Expression::ComputedMemberExpression(member) => {
                links.push(ChainLink::Property(member.static_property_name()?.as_str()));
                current = &member.object;
            }
            // `#`-private field receiver (`this.#dep`): oxc's `PrivateIdentifier.name`
            // excludes the leading `#`, so spell the key `this.#dep` to match the
            // binding key `visit_property_definition` / the assignment arms record
            // for private fields (issue #1821).
            Expression::PrivateFieldExpression(member) => {
                links.push(ChainLink::PrivateField(member.field.name.as_str()));
                current = &member.object;
            }
            Expression::CallExpression(call) if call.arguments.is_empty() => {
                links.push(ChainLink::Call);
                current = &call.callee;
            }
            Expression::NewExpression(new_expr) => match &new_expr.callee {
                Expression::Identifier(callee) => break callee.name.as_str(),
                _ => return None,
            },
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::CallExpression(call) if call.arguments.is_empty() => {
                    links.push(ChainLink::Call);
                    current = &call.callee;
                }
                ChainElement::StaticMemberExpression(member) => {
                    links.push(ChainLink::Property(member.property.name.as_str()));
                    current = &member.object;
                }
                _ => return None,
            },
            _ => return None,
        }
    };
    let mut name = String::from(root);
    for link in links.iter().rev() {
        match link {
            ChainLink::Property(property) => {
                name.push('.');
                name.push_str(property);
            }
            ChainLink::PrivateField(field) => {
                name.push_str(".#");
                name.push_str(field);
            }
            ChainLink::Call => name.push_str("()"),
        }
    }
    Some(name)
}

/// Flatten a JSX member-expression object to its dotted receiver spelling
/// (`SC`, `A.B`, `this.helpers`), the JSX counterpart of
/// `static_member_object_name`.
///
/// The visitor asks for this name at every level of a tag, so a name past
/// `MAX_RECORDED_CHAIN_LINKS` links is `None`, like `static_member_object_name`.
pub(in crate::visitor) fn jsx_member_object_name(
    object: &JSXMemberExpressionObject<'_>,
) -> Option<String> {
    let mut properties = Vec::new();
    let mut object = object;
    let root = loop {
        crate::cost::walk_step();
        if properties.len() > MAX_RECORDED_CHAIN_LINKS {
            return None;
        }
        match object {
            JSXMemberExpressionObject::IdentifierReference(ident) => break ident.name.as_str(),
            JSXMemberExpressionObject::ThisExpression(_) => break "this",
            JSXMemberExpressionObject::MemberExpression(inner) => {
                properties.push(inner.property.name.as_str());
                object = &inner.object;
            }
        }
    };
    Some(join_member_path(root, &properties))
}

/// The name path of a type name, spelled as `static_member_object_name`
/// spells the same member expression (`a`, `a.b`, `this`).
///
/// The heritage walk asks for this name at every level of a qualified name, so
/// a path past `MAX_RECORDED_CHAIN_LINKS` links is `None`, like
/// `static_member_object_name`.
pub(super) fn type_name_member_path(name: &TSTypeName<'_>) -> Option<String> {
    let mut properties = Vec::new();
    let mut name = name;
    let root = loop {
        if properties.len() > MAX_RECORDED_CHAIN_LINKS {
            return None;
        }
        match name {
            TSTypeName::IdentifierReference(ident) => break ident.name.as_str(),
            TSTypeName::ThisExpression(_) => break "this",
            TSTypeName::QualifiedName(qualified) => {
                properties.push(qualified.right.name.as_str());
                name = &qualified.left;
            }
        }
    };
    Some(join_member_path(root, &properties))
}

/// Record the member accesses a dotted component tag name implies, one per
/// nesting level (`A.B.C` records `{A.B, C}` then `{A, B}`), for template
/// pipelines that scan markup as text instead of visiting a JSX AST (Astro
/// markup, MDX bodies; issue #2355). This is the text counterpart of
/// `record_jsx_member_tag_accesses`, so namespace-import narrowing sees the
/// same stream from every pipeline. A name without a dot records nothing.
///
/// A level whose object has more than `MAX_RECORDED_CHAIN_LINKS` links
/// records nothing, the same cap that `jsx_member_object_name` applies, so
/// the output of a long tag does not grow with the square of its length.
pub fn push_member_tag_accesses(accesses: &mut Vec<MemberAccess>, dotted_tag: &str) {
    let mut end = dotted_tag.len();
    let mut object_links = dotted_tag.matches('.').count();
    while let Some(dot) = dotted_tag[..end].rfind('.') {
        object_links -= 1;
        let object = &dotted_tag[..dot];
        let member = &dotted_tag[dot + 1..end];
        if object.is_empty() || member.is_empty() {
            return;
        }
        if object_links <= MAX_RECORDED_CHAIN_LINKS {
            accesses.push(MemberAccess {
                object: object.to_string(),
                member: member.to_string(),
            });
        }
        end = dot;
    }
}

impl ModuleInfoExtractor {
    /// Emit one typed fluent-chain fact for a chain of calls, at its outermost
    /// call.
    ///
    /// The walk from the outermost call to the root marks each inner call, and
    /// an inner call does not walk again, so a chain of N calls costs O(N). The
    /// fact keeps each step name once, so its size is the set of method names,
    /// not the chain length. A walk that finds no root marks the inner calls
    /// too: their walks pass the same lower part of the chain and fail the same
    /// way.
    pub(super) fn try_record_fluent_chain_access(&mut self, expr: &CallExpression<'_>) {
        if self.fluent_chain_inner_calls.remove(&expr.span) {
            return;
        }
        let Expression::StaticMemberExpression(member) = &expr.callee else {
            return;
        };
        let Expression::CallExpression(_) = &member.object else {
            return;
        };
        let mut methods_outermost_first = vec![member.property.name.as_str()];
        let mut current = &member.object;
        let root = loop {
            crate::cost::walk_step();
            let Expression::CallExpression(call) = current else {
                break None;
            };
            let Expression::StaticMemberExpression(inner_member) = &call.callee else {
                break None;
            };
            let method = inner_member.property.name.as_str();
            match &inner_member.object {
                Expression::Identifier(root_id) => {
                    break Some((ChainRoot::Factory(root_id.name.as_str()), method));
                }
                Expression::NewExpression(new_expr) => match &new_expr.callee {
                    Expression::Identifier(class_id) => {
                        break Some((ChainRoot::New(class_id.name.as_str()), method));
                    }
                    _ => break None,
                },
                object => {
                    self.fluent_chain_inner_calls.insert(call.span);
                    methods_outermost_first.push(method);
                    current = object;
                }
            }
        };
        let Some((root, first_method)) = root else {
            return;
        };
        let mut seen = FxHashSet::default();
        let steps = methods_outermost_first
            .iter()
            .rev()
            .filter(|method| seen.insert(**method))
            .map(|method| (*method).to_string())
            .collect();
        self.semantic_facts.push(match root {
            ChainRoot::Factory(root_object) => {
                SemanticFact::FluentChainMemberAccess(FluentChainMemberAccessFact {
                    root_object: root_object.to_string(),
                    root_method: first_method.to_string(),
                    steps,
                })
            }
            ChainRoot::New(class_name) => {
                SemanticFact::FluentChainNewMemberAccess(FluentChainNewMemberAccessFact {
                    class_name: class_name.to_string(),
                    first_method: first_method.to_string(),
                    steps,
                })
            }
        });
    }
}

/// The root of a fluent chain: `Root.factory()` or `new Class().method()`.
enum ChainRoot<'b> {
    Factory(&'b str),
    New(&'b str),
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    /// The name of the expression statement in `class C { #a: any; m() { ... } }`.
    fn method_statement_name(body: &str) -> Option<String> {
        let allocator = oxc_allocator::Allocator::default();
        let source = format!("class C {{ #a: any; m() {{ {body} }} }}");
        let parsed =
            oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::ts()).parse();
        assert!(
            parsed.diagnostics.is_empty(),
            "the fixture must parse without errors"
        );
        let Some(Statement::ClassDeclaration(class)) = parsed.program.body.first() else {
            panic!("expected a class declaration");
        };
        let Some(ClassElement::MethodDefinition(method)) = class.body.body.get(1) else {
            panic!("expected a method");
        };
        let Some(Statement::ExpressionStatement(statement)) = method
            .value
            .body
            .as_ref()
            .and_then(|body| body.statements.first())
        else {
            panic!("expected an expression statement");
        };
        static_member_object_name(&statement.expression)
    }

    #[test]
    fn static_member_object_name_builds_a_chain_at_the_cap() {
        // `#a`, the `.b` links, `.c`, `()` and `.d` fill the cap exactly.
        let middle = ".b".repeat(MAX_RECORDED_CHAIN_LINKS - 4);
        assert_eq!(
            method_statement_name(&format!("(this.#a{middle}.c() as T)?.d;")),
            Some(format!("this.#a{middle}.c().d"))
        );
    }

    #[test]
    fn static_member_object_name_refuses_a_chain_past_the_cap() {
        let middle = ".b".repeat(MAX_RECORDED_CHAIN_LINKS - 3);
        assert_eq!(
            method_statement_name(&format!("(this.#a{middle}.c() as T)?.d;")),
            None
        );
    }

    /// The object name of the JSX member tag in `<a{links}.c />`.
    fn jsx_tag_object_name(links: &str) -> Option<String> {
        let allocator = oxc_allocator::Allocator::default();
        let source = format!("<a{links}.c />;");
        let parsed =
            oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::jsx()).parse();
        let Some(Statement::ExpressionStatement(statement)) = parsed.program.body.first() else {
            panic!("expected an expression statement");
        };
        let Expression::JSXElement(element) = &statement.expression else {
            panic!("expected a JSX element");
        };
        let JSXElementName::MemberExpression(member) = &element.opening_element.name else {
            panic!("expected a member tag");
        };
        jsx_member_object_name(&member.object)
    }

    #[test]
    fn jsx_member_object_name_stops_at_the_cap() {
        let links = ".b".repeat(MAX_RECORDED_CHAIN_LINKS);
        assert_eq!(jsx_tag_object_name(&links), Some(format!("a{links}")));
        assert_eq!(jsx_tag_object_name(&format!("{links}.b")), None);
    }

    #[test]
    fn push_member_tag_accesses_stops_at_the_cap() {
        let links = ".b".repeat(MAX_RECORDED_CHAIN_LINKS);
        let mut accesses = Vec::new();
        push_member_tag_accesses(&mut accesses, &format!("a{links}.b.c"));
        let objects: Vec<&str> = accesses.iter().map(|a| a.object.as_str()).collect();
        assert_eq!(objects.len(), MAX_RECORDED_CHAIN_LINKS + 1);
        assert_eq!(objects.first().copied(), Some(format!("a{links}").as_str()));
        assert_eq!(objects.last().copied(), Some("a"));
    }

    #[test]
    fn static_member_object_name_refuses_a_long_chain() {
        // A recursive walk overflows a test thread stack on a chain this long.
        let links = ".b".repeat(300_000);
        assert_eq!(method_statement_name(&format!("this.#a{links}.c;")), None);
    }
}
