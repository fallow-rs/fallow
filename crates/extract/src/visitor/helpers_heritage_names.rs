//! Dotted names of class heritage and type references.

#[allow(clippy::wildcard_imports, reason = "many AST node variants used")]
use oxc_ast::ast::*;

use crate::visitor::visit_impl::join_member_path;

/// The dotted name of `a.b.c`, joined once: a recursive `format!` copies the
/// whole name at each link, so a long heritage chain costs O(N^2) bytes.
pub(super) fn extract_static_expression_name(expr: &Expression<'_>) -> Option<String> {
    let (mut properties, mut expr) = (Vec::new(), expr);
    while let Expression::StaticMemberExpression(member) = expr {
        properties.push(member.property.name.as_str());
        expr = &member.object;
    }
    let Expression::Identifier(ident) = expr else {
        return None;
    };
    Some(join_member_path(ident.name.as_str(), &properties))
}

/// The dotted name of the type name `A.B.C`, or `None` for a `this` root.
pub(super) fn extract_type_name(name: &TSTypeName<'_>) -> Option<String> {
    let (mut properties, mut name) = (Vec::new(), name);
    while let TSTypeName::QualifiedName(qualified) = name {
        properties.push(qualified.right.name.as_str());
        name = &qualified.left;
    }
    let TSTypeName::IdentifierReference(ident) = name else {
        return None;
    };
    Some(join_member_path(ident.name.as_str(), &properties))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heritage_names_of_a_long_chain_build_without_recursion() {
        // A recursive build overflows a test thread stack on a chain this long.
        let links = ".b".repeat(300_000);
        let source = format!("class C extends a{links} implements T{links} {{}}");
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::ts()).parse();
        let Some(Statement::ClassDeclaration(class)) = parsed.program.body.first() else {
            panic!("expected a class declaration");
        };
        let heritage = class.heritage.as_ref().expect("a heritage clause");
        assert_eq!(
            extract_static_expression_name(&heritage.expression),
            Some(format!("a{links}"))
        );
        let implemented = class.implements.first().expect("an implements clause");
        assert_eq!(
            extract_type_name(&implemented.expression),
            Some(format!("T{links}"))
        );
    }
}
