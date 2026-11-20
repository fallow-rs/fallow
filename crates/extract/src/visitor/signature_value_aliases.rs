//! Module bindings whose type flows into the inferred type of another binding.
//!
//! A module-level `const` without a type annotation gets its type from its
//! initializer. When the initializer holds another module binding as a value
//! (`{ latency: entryBox }`, `[items]`, `a ?? b`), the inferred type contains
//! the type of that binding. A declaration file of a module that holds the
//! exported value must then name the types of that binding. See issue #4850.
//!
//! [`collect_initializer_value_aliases`] finds these bindings. The visitor
//! adds the signature type references of each found binding to the public
//! signature of the export, transitively.

use oxc_ast::ast::{
    ArrayExpressionElement, Expression, ObjectPropertyKind, PropertyKind, TSType, TSTypeName,
};
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{LocalTypeDeclaration, PublicSignatureTypeReference};

use super::{LocalSignatureTypeReference, ModuleInfoExtractor};

/// Collect the names of the bindings that the inferred type of `init`
/// contains as a whole value.
///
/// The walk follows object property values, array elements, conditional and
/// logical branches, parentheses, `!`, `satisfies` and `as const`. It stops at
/// a member read, a call, a spread, a function, and any other type assertion,
/// because the type of these expressions is not the type of a held binding.
pub(super) fn collect_initializer_value_aliases(init: &Expression<'_>, aliases: &mut Vec<String>) {
    match init {
        Expression::Identifier(identifier) => aliases.push(identifier.name.to_string()),
        Expression::ParenthesizedExpression(inner) => {
            collect_initializer_value_aliases(&inner.expression, aliases);
        }
        Expression::TSNonNullExpression(inner) => {
            collect_initializer_value_aliases(&inner.expression, aliases);
        }
        Expression::TSSatisfiesExpression(inner) => {
            collect_initializer_value_aliases(&inner.expression, aliases);
        }
        Expression::TSAsExpression(assertion) if is_const_assertion(&assertion.type_annotation) => {
            collect_initializer_value_aliases(&assertion.expression, aliases);
        }
        Expression::ObjectExpression(object) => {
            for property in &object.properties {
                if let ObjectPropertyKind::ObjectProperty(property) = property
                    && property.kind == PropertyKind::Init
                    && !property.method
                {
                    collect_initializer_value_aliases(&property.value, aliases);
                }
            }
        }
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                if !matches!(element, ArrayExpressionElement::SpreadElement(_))
                    && let Some(element) = element.as_expression()
                {
                    collect_initializer_value_aliases(element, aliases);
                }
            }
        }
        Expression::ConditionalExpression(conditional) => {
            collect_initializer_value_aliases(&conditional.consequent, aliases);
            collect_initializer_value_aliases(&conditional.alternate, aliases);
        }
        Expression::LogicalExpression(logical) => {
            collect_initializer_value_aliases(&logical.left, aliases);
            collect_initializer_value_aliases(&logical.right, aliases);
        }
        _ => {}
    }
}

/// `as const` keeps the structure of the expression, so the held bindings
/// keep their types.
fn is_const_assertion(ty: &TSType<'_>) -> bool {
    matches!(
        ty,
        TSType::TSTypeReference(reference)
            if matches!(&reference.type_name, TSTypeName::IdentifierReference(name) if name.name == "const")
    )
}

/// Every binding that `owner` holds, directly or through a held binding that
/// holds more bindings. The result excludes `owner` and is in visit order.
fn held_value_closure<'a>(
    held_by_owner: &FxHashMap<&'a str, Vec<&'a str>>,
    owner: &str,
) -> Vec<&'a str> {
    let Some(direct) = held_by_owner.get(owner) else {
        return Vec::new();
    };
    let mut seen: FxHashSet<&str> = FxHashSet::default();
    seen.insert(owner);
    let mut pending: Vec<&'a str> = direct.iter().rev().copied().collect();
    let mut closure = Vec::new();
    while let Some(held) = pending.pop() {
        if !seen.insert(held) {
            continue;
        }
        closure.push(held);
        if let Some(next) = held_by_owner.get(held) {
            pending.extend(next.iter().rev().copied());
        }
    }
    closure
}

/// The held bindings of each owner, without classes, enums and namespaces.
/// A held class or enum gives `typeof Name`, which names only the binding
/// itself and not the types of its members.
fn held_value_bindings<'a>(
    aliases: &'a [(String, String)],
    declarations: &[LocalTypeDeclaration],
) -> FxHashMap<&'a str, Vec<&'a str>> {
    if aliases.is_empty() {
        return FxHashMap::default();
    }
    let declared_types: FxHashSet<&str> = declarations
        .iter()
        .map(|declaration| declaration.name.as_str())
        .collect();
    let mut held_by_owner: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
    for (owner, held) in aliases {
        if !declared_types.contains(held.as_str()) {
            held_by_owner
                .entry(owner.as_str())
                .or_default()
                .push(held.as_str());
        }
    }
    held_by_owner
}

impl ModuleInfoExtractor {
    /// Add the signature type references of each export, including the
    /// references of the bindings that its initializer holds.
    pub(super) fn map_local_signature_refs_to_exports(&mut self) {
        if self.local_signature_type_references.is_empty() {
            return;
        }

        let mut references_by_owner: FxHashMap<&str, Vec<&LocalSignatureTypeReference>> =
            FxHashMap::default();
        // Appending to each owner bucket preserves collection order. Iterating
        // exports below preserves export and alias order while avoiding a full
        // reference scan for every export.
        for reference in &self.local_signature_type_references {
            references_by_owner
                .entry(reference.owner_name.as_str())
                .or_default()
                .push(reference);
        }
        let held_by_owner =
            held_value_bindings(&self.local_value_aliases, &self.local_type_declarations);

        for export in &self.exports {
            let export_name = export.name.to_string();
            let local_name = export.local_name.as_deref().unwrap_or(&export_name);
            if let Some(references) = references_by_owner.get(local_name) {
                self.public_signature_type_references
                    .extend(
                        references
                            .iter()
                            .map(|reference| PublicSignatureTypeReference {
                                export_name: export_name.clone(),
                                type_name: reference.type_name.clone(),
                                span: reference.span,
                                from_satisfies: reference.from_satisfies,
                            }),
                    );
            }
            // A `satisfies` clause does not change the type of a held binding,
            // so only the type of the binding itself flows into the export.
            for held in held_value_closure(&held_by_owner, local_name) {
                let Some(references) = references_by_owner.get(held) else {
                    continue;
                };
                self.public_signature_type_references.extend(
                    references
                        .iter()
                        .filter(|reference| !reference.from_satisfies)
                        .map(|reference| PublicSignatureTypeReference {
                            export_name: export_name.clone(),
                            type_name: reference.type_name.clone(),
                            span: reference.span,
                            from_satisfies: false,
                        }),
                );
            }
        }
    }
}
