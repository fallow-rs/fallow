//! Class bindings of `for...of` loop variables.

use oxc_ast::ast::*;

use super::super::ModuleInfoExtractor;
use super::{iteration_source_reference, static_member_object_name};

/// Array iteration methods whose callback's FIRST parameter is an element of the
/// receiver array (so it can be typed to the receiver's element class). `reduce`
/// and `reduceRight` are intentionally excluded: their first callback parameter
/// is the accumulator, not an element. `sort` is excluded to keep the set to
/// single-element-per-call iterators. See issue #1707 follow-up.
pub(in crate::visitor) const ITERABLE_ELEMENT_CALLBACK_METHODS: &[&str] = &[
    "map",
    "forEach",
    "filter",
    "find",
    "findLast",
    "findIndex",
    "findLastIndex",
    "flatMap",
    "some",
    "every",
];

impl ModuleInfoExtractor {
    /// Bind a `for (const util of utils)` loop variable to the element class of
    /// `utils` so member accesses on the loop variable (`util.getter`) in the
    /// loop body credit the class. The loop scope owns the binding, so a
    /// binding of the same name in another scope keeps its own type (issue
    /// #4352). Bare-identifier loop bindings over a name path receiver or a
    /// call of a local function with an array return type only; destructured
    /// bindings and other receivers are out of scope.
    pub(super) fn bind_for_of_element(&mut self, stmt: &ForOfStatement<'_>) {
        // Class-scope the `for (const x of this.<field>)` receiver so it matches
        // the same class's qualified query-list key (issue #1821).
        let receiver_name = static_member_object_name(&stmt.right)
            .map(|receiver_name| self.qualify_this_scope(&receiver_name));
        let Some(element_type) = receiver_name
            .as_deref()
            .and_then(|receiver_name| self.iterable_element_type_for(receiver_name))
            // `for (const t of all())` with `function all(): Table[]` (issue #4313).
            .or_else(|| self.local_call_array_element_type(&stmt.right))
        else {
            return;
        };
        let ForStatementLeft::VariableDeclaration(decl) = &stmt.left else {
            return;
        };
        if let Some(declarator) = decl.declarations.first()
            && let BindingPattern::BindingIdentifier(id) = &declarator.id
        {
            self.record_binding_declaration(id.name.as_str(), id.span);
            if let Some(receiver_name) = receiver_name.as_deref() {
                self.bind_iteration_class_proof(
                    receiver_name
                        .strip_suffix(".values()")
                        .unwrap_or(receiver_name),
                    id.name.as_str(),
                    iteration_source_reference(&stmt.right),
                );
            }
            self.record_scoped_class_binding(id.name.as_str(), element_type);
        }
    }

    /// `Map.entries()` yields `[key, value]`; only the second binding has `V`.
    pub(super) fn bind_for_of_map_value(&mut self, stmt: &ForOfStatement<'_>) {
        let Some(receiver_name) = static_member_object_name(&stmt.right) else {
            return;
        };
        let Some(map_name) = receiver_name.strip_suffix(".entries()") else {
            return;
        };
        let Some(value_type) = self.map_value_type_for(&self.qualify_this_scope(map_name)) else {
            return;
        };
        let ForStatementLeft::VariableDeclaration(declaration) = &stmt.left else {
            return;
        };
        let Some(declarator) = declaration.declarations.first() else {
            return;
        };
        let BindingPattern::ArrayPattern(pattern) = &declarator.id else {
            return;
        };
        if let Some(Some(BindingPattern::BindingIdentifier(value))) = pattern.elements.get(1) {
            self.record_binding_declaration(value.name.as_str(), value.span);
            self.bind_iteration_class_proof(
                &self.qualify_this_scope(map_name),
                value.name.as_str(),
                iteration_source_reference(&stmt.right),
            );
            self.record_scoped_class_binding(value.name.as_str(), value_type);
        }
    }
}
