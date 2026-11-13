//! Dotted names of class heritage and type references, and the type that
//! holds the members of a wrapper type reference.

use oxc_ast::ast::*;

use crate::visitor::visit_impl::join_member_path;

/// The dotted name of `a.b.c`, joined once: a recursive `format!` copies the
/// whole name at each link, so a long heritage chain costs O(N^2) bytes.
pub(super) fn extract_static_expression_name(expr: &Expression<'_>) -> Option<String> {
    let (mut properties, mut expr) = (Vec::new(), expr);
    while let Expression::StaticMemberExpression(member) = expr {
        crate::cost::walk_step();
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
        crate::cost::walk_step();
        properties.push(qualified.right.name.as_str());
        name = &qualified.left;
    }
    let TSTypeName::IdentifierReference(ident) = name else {
        return None;
    };
    Some(join_member_path(ident.name.as_str(), &properties))
}

/// Generic wrappers whose value exposes members of the first type argument.
///
/// - `Pick`, `Omit`, `Partial`, `Required`, `Readonly` and `NonNullable` keep
///   a subset of the members of `T`, or all of them. TypeScript rejects a
///   member access that `T` does not declare, so an access on the wrapper is
///   an access on `T`.
/// - `DurableObjectNamespace`, `DurableObjectStub` and `Rpc.Stub` are the
///   Cloudflare Workers types of a Durable Object binding and of its RPC stub.
///   A method call on the stub runs the method of the class `T`.
const MEMBER_PRESERVING_WRAPPERS: &[&str] = &[
    "Pick",
    "Omit",
    "Partial",
    "Required",
    "Readonly",
    "NonNullable",
    "DurableObjectNamespace",
    "DurableObjectStub",
    "Rpc.Stub",
];

/// The type that holds the members of `ty`: the first type argument of each
/// wrapper in [`MEMBER_PRESERVING_WRAPPERS`], also through parentheses and
/// nested wrappers. Any other type comes back unchanged.
pub(super) fn unwrap_member_preserving_type<'t, 'a>(ty: &'t TSType<'a>) -> &'t TSType<'a> {
    let mut ty = ty;
    loop {
        crate::cost::walk_step();
        match ty {
            TSType::TSParenthesizedType(paren) => ty = &paren.type_annotation,
            TSType::TSTypeReference(reference) => {
                let Some(first) = reference
                    .type_arguments
                    .as_deref()
                    .and_then(|arguments| arguments.params.first())
                else {
                    return ty;
                };
                let is_wrapper = extract_type_name(&reference.type_name)
                    .is_some_and(|name| MEMBER_PRESERVING_WRAPPERS.contains(&name.as_str()));
                if !is_wrapper {
                    return ty;
                }
                ty = first;
            }
            _ => return ty,
        }
    }
}

#[cfg(all(test, not(miri)))]
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

    fn alias_type_reference_name(alias_type: &str) -> Option<String> {
        let source = format!("type Probe = {alias_type};");
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::ts()).parse();
        let Some(Statement::TSTypeAliasDeclaration(alias)) = parsed.program.body.first() else {
            panic!("expected a type alias");
        };
        super::super::extract_type_reference_name(&alias.type_annotation)
    }

    #[test]
    fn member_preserving_wrappers_resolve_to_their_first_type_argument() {
        for (alias_type, expected) in [
            ("Pick<Coordinator, 'run'>", Some("Coordinator")),
            ("Omit<Coordinator, 'run'>", Some("Coordinator")),
            ("Readonly<Partial<Coordinator>>", Some("Coordinator")),
            ("(Required<Coordinator>)", Some("Coordinator")),
            ("NonNullable<Coordinator | null>", Some("Coordinator")),
            ("DurableObjectNamespace<Quota>", Some("Quota")),
            ("DurableObjectStub<Quota>", Some("Quota")),
            ("Rpc.Stub<Quota>", Some("Quota")),
            ("Promise<Coordinator>", Some("Promise")),
            ("Readonly<Coordinator[]>", None),
            ("Partial", Some("Partial")),
        ] {
            assert_eq!(
                alias_type_reference_name(alias_type).as_deref(),
                expected,
                "type: {alias_type}"
            );
        }
    }
}
