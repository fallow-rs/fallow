//! Module bindings whose type flows into the inferred type of another binding.
//!
//! A module binding without a type annotation gets its type from an
//! expression. When the expression holds another module binding as a value
//! (`{ latency: entryBox }`, `[items]`, `a ?? b`, `() => entryBox`), the
//! inferred type contains the type of that binding. A declaration file of a
//! module that holds the exported value must then name the types of that
//! binding. See issue #4850.
//!
//! The type flow walk of the visitor finds these held names. This module adds
//! the signature type references of each held name to the public signature of
//! the export, transitively.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{LocalTypeDeclaration, PublicSignatureTypeReference};

use super::{LocalSignatureTypeReference, ModuleInfoExtractor};

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
            // A property path and its root can hold the same reference, so
            // each reference counts once per export.
            let mut seen: FxHashSet<(&str, u32)> = FxHashSet::default();
            for held in held_value_closure(&held_by_owner, local_name) {
                let Some(references) = references_by_owner.get(held) else {
                    continue;
                };
                self.public_signature_type_references.extend(
                    references
                        .iter()
                        .filter(|reference| !reference.from_satisfies)
                        .filter(|reference| {
                            seen.insert((reference.type_name.as_str(), reference.span.start))
                        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closure_follows_held_bindings_and_stops_at_a_cycle() {
        let mut held_by_owner: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
        held_by_owner.insert("status", vec!["a"]);
        held_by_owner.insert("a", vec!["b"]);
        held_by_owner.insert("b", vec!["a", "status", "c"]);

        assert_eq!(
            held_value_closure(&held_by_owner, "status"),
            vec!["a", "b", "c"]
        );
        assert!(held_value_closure(&held_by_owner, "unknown").is_empty());
    }
}
