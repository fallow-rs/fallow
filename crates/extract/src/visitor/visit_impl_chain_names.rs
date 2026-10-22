//! Dotted names of static member chains for visitor extraction.

#[allow(clippy::wildcard_imports, reason = "many AST node variants used")]
use oxc_ast::ast::*;
use rustc_hash::FxHashSet;

use super::super::{ModuleInfoExtractor, SemanticFact};
use crate::{FluentChainMemberAccessFact, FluentChainNewMemberAccessFact};

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

impl ModuleInfoExtractor {
    /// Emit typed fluent-chain facts for chained calls.
    ///
    /// The consumer checks only that every step of the chain returns `this`, so
    /// the fact keeps each step name once. A builder that repeats a method then
    /// records a fact as large as its set of method names, not as long as the
    /// chain, and no step past `MAX_RECORDED_CHAIN_LINKS` loses its credit.
    pub(super) fn try_record_fluent_chain_access(&mut self, expr: &CallExpression<'_>) {
        let Expression::StaticMemberExpression(member) = &expr.callee else {
            return;
        };
        let Expression::CallExpression(_) = &member.object else {
            return;
        };
        let this_method = member.property.name.as_str();
        let mut steps = FluentChainSteps::default();
        let mut current = &member.object;
        loop {
            let Expression::CallExpression(call) = current else {
                return;
            };
            let Expression::StaticMemberExpression(inner_member) = &call.callee else {
                return;
            };
            if let Expression::Identifier(root_id) = &inner_member.object {
                self.semantic_facts
                    .push(SemanticFact::FluentChainMemberAccess(
                        FluentChainMemberAccessFact {
                            root_object: root_id.name.to_string(),
                            root_method: inner_member.property.name.to_string(),
                            chain: steps.into_chain(),
                            member: this_method.to_string(),
                        },
                    ));
                return;
            }
            steps.push(inner_member.property.name.as_str());
            if let Expression::NewExpression(new_expr) = &inner_member.object
                && let Expression::Identifier(class_id) = &new_expr.callee
            {
                self.semantic_facts
                    .push(SemanticFact::FluentChainNewMemberAccess(
                        FluentChainNewMemberAccessFact {
                            class_name: class_id.name.to_string(),
                            chain: steps.into_chain(),
                            member: this_method.to_string(),
                        },
                    ));
                return;
            }
            current = &inner_member.object;
        }
    }
}

/// The distinct step names of a fluent chain, collected from the outermost
/// step inward.
#[derive(Default)]
struct FluentChainSteps<'b> {
    names: Vec<&'b str>,
    seen: FxHashSet<&'b str>,
}

impl<'b> FluentChainSteps<'b> {
    fn push(&mut self, name: &'b str) {
        // A builder chain often repeats the step it just called.
        if self.names.last() != Some(&name) && self.seen.insert(name) {
            self.names.push(name);
        }
    }

    /// The step names from the root outward.
    fn into_chain(self) -> Vec<String> {
        self.names
            .iter()
            .rev()
            .map(|name| (*name).to_string())
            .collect()
    }
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

    #[test]
    fn static_member_object_name_refuses_a_long_chain() {
        // A recursive walk overflows a test thread stack on a chain this long.
        let links = ".b".repeat(300_000);
        assert_eq!(method_statement_name(&format!("this.#a{links}.c;")), None);
    }
}
