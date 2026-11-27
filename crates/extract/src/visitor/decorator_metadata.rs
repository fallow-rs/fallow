//! The local names that TypeScript emits as decorator metadata (issue #5362).
//!
//! Under `emitDecoratorMetadata`, TypeScript emits `design:type`,
//! `design:paramtypes` and `design:returntype` for decorated class members.
//! These rules follow the TypeScript checker, which uses them to decide
//! which imports stay in the output (`getEntityNameForDecoratorMetadata`).

use oxc_ast::ast::{
    Class, ClassElement, ClassType, FormalParameters, Function, MethodDefinition,
    MethodDefinitionKind, TSType, TSTypeAnnotation, TSTypeName,
};

/// One name that a metadata type position emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MetadataName {
    /// The root identifier of the type name.
    pub(super) local: String,
    /// The name reaches the metadata only without `strictNullChecks`.
    pub(super) without_strict_null_checks: bool,
}

/// The name of one type position, and whether the type name is a plain
/// identifier. TypeScript compares only plain identifiers across the members
/// of a union.
struct Emitted {
    name: MetadataName,
    is_identifier: bool,
}

/// The names that the metadata of the decorated members of `class` emits.
pub(super) fn class_metadata_names(class: &Class<'_>) -> Vec<MetadataName> {
    let mut names = Vec::new();
    if class.r#type != ClassType::ClassDeclaration || class.declare {
        return names;
    }
    let constructor = class.body.body.iter().find_map(|element| match element {
        ClassElement::MethodDefinition(method)
            if method.kind == MethodDefinitionKind::Constructor && method.value.body.is_some() =>
        {
            Some(&method.value)
        }
        _ => None,
    });
    if let Some(constructor) = constructor
        && (!class.decorators.is_empty() || has_parameter_decorators(&constructor.params))
    {
        push_parameters(&mut names, &constructor.params);
    }
    for element in &class.body.body {
        match element {
            ClassElement::MethodDefinition(method) => push_method(&mut names, class, method),
            ClassElement::PropertyDefinition(property) if !property.decorators.is_empty() => {
                push_annotation(&mut names, property.type_annotation.as_deref());
            }
            ClassElement::AccessorProperty(property) if !property.decorators.is_empty() => {
                push_annotation(&mut names, property.type_annotation.as_deref());
            }
            _ => {}
        }
    }
    names
}

fn push_method(names: &mut Vec<MetadataName>, class: &Class<'_>, method: &MethodDefinition<'_>) {
    let function = &method.value;
    match method.kind {
        MethodDefinitionKind::Method => {
            if function.body.is_some()
                && (!method.decorators.is_empty() || has_parameter_decorators(&function.params))
            {
                if let Some(this_param) = &function.this_param {
                    push_annotation(names, this_param.type_annotation.as_deref());
                }
                push_parameters(names, &function.params);
                push_annotation(names, function.return_type.as_deref());
            }
        }
        MethodDefinitionKind::Get | MethodDefinitionKind::Set => {
            if function.body.is_some() && !method.decorators.is_empty() {
                let annotation = accessor_type(function)
                    .or_else(|| accessor_pair(class, method).and_then(accessor_type));
                push_annotation(names, annotation);
            }
        }
        MethodDefinitionKind::Constructor => {}
    }
}

fn push_annotation(names: &mut Vec<MetadataName>, annotation: Option<&TSTypeAnnotation<'_>>) {
    push_type(
        names,
        annotation.map(|annotation| &annotation.type_annotation),
    );
}

fn push_type(names: &mut Vec<MetadataName>, ty: Option<&TSType<'_>>) {
    if let Some(emitted) = ty.and_then(emitted_name) {
        names.push(emitted.name);
    }
}

fn has_parameter_decorators(params: &FormalParameters<'_>) -> bool {
    params
        .items
        .iter()
        .any(|param| !param.decorators.is_empty())
        || params
            .rest
            .as_ref()
            .is_some_and(|rest| !rest.decorators.is_empty())
}

/// Pushes the type of each parameter. A rest parameter emits the type of its
/// element: `...items: Dto[]` and `...items: Array<Dto>` emit `Dto`.
fn push_parameters(names: &mut Vec<MetadataName>, params: &FormalParameters<'_>) {
    for param in &params.items {
        push_annotation(names, param.type_annotation.as_deref());
    }
    let element = params
        .rest
        .as_ref()
        .and_then(|rest| rest.type_annotation.as_deref())
        .and_then(|annotation| match &annotation.type_annotation {
            TSType::TSArrayType(array) => Some(&array.element_type),
            TSType::TSTypeReference(reference) => {
                reference.type_arguments.as_deref().and_then(|arguments| {
                    match arguments.params.as_slice() {
                        [only] => Some(only),
                        _ => None,
                    }
                })
            }
            _ => None,
        });
    push_type(names, element);
}

/// The annotated type of an accessor: the return type of a getter or the
/// parameter type of a setter.
fn accessor_type<'b, 'a>(function: &'b Function<'a>) -> Option<&'b TSTypeAnnotation<'a>> {
    function.return_type.as_deref().or_else(|| {
        function
            .params
            .items
            .first()
            .and_then(|param| param.type_annotation.as_deref())
    })
}

/// The other accessor of the same property. TypeScript reads the type of a
/// decorated accessor from it when the decorated one has no annotation.
fn accessor_pair<'b, 'a>(
    class: &'b Class<'a>,
    method: &MethodDefinition<'a>,
) -> Option<&'b Function<'a>> {
    let key = method.key.static_name()?;
    class.body.body.iter().find_map(|element| match element {
        ClassElement::MethodDefinition(other)
            if other.kind != method.kind
                && matches!(
                    other.kind,
                    MethodDefinitionKind::Get | MethodDefinitionKind::Set
                )
                && other.r#static == method.r#static
                && other.key.static_name().is_some_and(|name| name == key) =>
        {
            Some(&*other.value)
        }
        _ => None,
    })
}

/// The name that a metadata type position emits, if any. A generic type
/// emits only its outer name, and an array, a function type or a keyword
/// emits a global (`Array`, `Function`, `String`), never an import.
fn emitted_name(ty: &TSType<'_>) -> Option<Emitted> {
    match ty {
        TSType::TSTypeReference(reference) => {
            root_identifier(&reference.type_name).map(|local| Emitted {
                name: MetadataName {
                    local,
                    without_strict_null_checks: false,
                },
                is_identifier: matches!(reference.type_name, TSTypeName::IdentifierReference(_)),
            })
        }
        TSType::TSUnionType(union) => common_name(union.types.iter()),
        TSType::TSIntersectionType(intersection) => common_name(intersection.types.iter()),
        TSType::TSConditionalType(conditional) => {
            common_name([&conditional.true_type, &conditional.false_type].into_iter())
        }
        TSType::TSParenthesizedType(_) | TSType::TSNamedTupleMember(_) => {
            let inner = skip_parentheses(ty);
            if std::ptr::eq(inner, ty) {
                None
            } else {
                emitted_name(inner)
            }
        }
        _ => None,
    }
}

/// The one name that each member of a union, intersection or conditional
/// emits. TypeScript skips `never`, and skips `null` and `undefined` only
/// without `strictNullChecks`. Two members that emit different names, or
/// two members of which one is a qualified name, emit `Object`.
fn common_name<'b, 'a: 'b>(types: impl Iterator<Item = &'b TSType<'a>>) -> Option<Emitted> {
    let mut common: Option<Emitted> = None;
    let mut skipped_null = false;
    for ty in types {
        match skip_parentheses(ty) {
            TSType::TSNeverKeyword(_) => {}
            TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => skipped_null = true,
            member => {
                let emitted = emitted_name(member)?;
                match &mut common {
                    Some(existing)
                        if !existing.is_identifier
                            || !emitted.is_identifier
                            || existing.name.local != emitted.name.local =>
                    {
                        return None;
                    }
                    Some(existing) => {
                        existing.name.without_strict_null_checks |=
                            emitted.name.without_strict_null_checks;
                    }
                    None => common = Some(emitted),
                }
            }
        }
    }
    common.map(|mut emitted| {
        emitted.name.without_strict_null_checks |= skipped_null;
        emitted
    })
}

fn skip_parentheses<'b, 'a>(mut ty: &'b TSType<'a>) -> &'b TSType<'a> {
    loop {
        ty = match ty {
            TSType::TSParenthesizedType(parenthesized) => &parenthesized.type_annotation,
            TSType::TSNamedTupleMember(member) => match member.element_type.as_ts_type() {
                Some(inner) => inner,
                None => return ty,
            },
            _ => return ty,
        };
    }
}

/// The leftmost identifier of a type name: `ns` in `ns.Dto`.
fn root_identifier(name: &TSTypeName<'_>) -> Option<String> {
    match name {
        TSTypeName::IdentifierReference(identifier) => Some(identifier.name.to_string()),
        TSTypeName::QualifiedName(qualified) => root_identifier(&qualified.left),
        TSTypeName::ThisExpression(_) => None,
    }
}
