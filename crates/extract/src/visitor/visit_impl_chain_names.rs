//! Dotted names of static member chains for visitor extraction.

#[allow(clippy::wildcard_imports, reason = "many AST node variants used")]
use oxc_ast::ast::*;

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
/// then takes seconds.
pub(super) fn static_member_object_name(expr: &Expression<'_>) -> Option<String> {
    let mut links = Vec::new();
    let mut current = expr;
    let root = loop {
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

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    /// A chain this long overflows a test thread stack with a recursive walk,
    /// and a build that copies the parent name at each link takes seconds.
    const LINKS: usize = 300_000;

    #[test]
    fn static_member_object_name_builds_a_long_chain_in_one_pass() {
        let allocator = oxc_allocator::Allocator::default();
        let source = format!("(this.#a{}.c() as T)?.d;", ".b".repeat(LINKS));
        let parsed =
            oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::ts()).parse();
        let Some(Statement::ExpressionStatement(statement)) = parsed.program.body.first() else {
            panic!("expected an expression statement");
        };
        let start = std::time::Instant::now();
        let name = static_member_object_name(&statement.expression);
        let elapsed = start.elapsed();
        assert_eq!(name, Some(format!("this.#a{}.c().d", ".b".repeat(LINKS))));
        assert!(
            elapsed.as_secs() < 1,
            "a chain of {LINKS} links took {elapsed:?}"
        );
    }
}
