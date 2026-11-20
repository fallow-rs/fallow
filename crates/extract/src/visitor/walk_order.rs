//! Credit reads through a binding that sibling scopes bind to other classes.
//!
//! `binding_target_names` is module-flat. When two sibling functions each
//! declare `view` with another class, the name becomes ambiguous and credits
//! no class. The walk keeps the class that the binding held at each read, and
//! this pass credits that class when the flat binding is ambiguous.

use super::{BindingTarget, ModuleInfoExtractor, TypedPropertyExpansion};
use crate::MemberAccess;

impl ModuleInfoExtractor {
    /// Record a whole-object use of `binding`, and keep the class that the
    /// binding holds at this point in the walk.
    pub(super) fn push_walk_order_whole_object_use(&mut self, binding: String) {
        if let Some(class) = self.walk_order_class_bindings.get(&binding)
            && !self.walk_read_is_shadowed(&binding)
        {
            self.walk_order_whole_object_uses
                .push((binding.clone(), class.clone()));
        }
        self.push_whole_object_use(binding);
    }

    pub(super) fn resolve_ambiguous_walk_order_uses(&mut self) {
        self.resolve_ambiguous_walk_order_member_accesses();
        self.resolve_ambiguous_walk_order_whole_object_uses();
    }

    fn binding_is_ambiguous(&self, binding: &str) -> bool {
        matches!(
            self.binding_target_names.get(binding),
            Some(BindingTarget::Ambiguous)
        )
    }

    fn resolve_ambiguous_walk_order_whole_object_uses(&mut self) {
        for (binding, class) in std::mem::take(&mut self.walk_order_whole_object_uses) {
            if self.binding_is_ambiguous(&binding) {
                self.push_whole_object_use(class);
            }
        }
    }

    /// Scoped compound targets survive scope exit; module-flat targets only need
    /// walk-order credit when sibling scopes make their final binding ambiguous.
    fn resolve_ambiguous_walk_order_member_accesses(&mut self) {
        for (binding, object, member) in std::mem::take(&mut self.walk_order_member_accesses) {
            if let Some(binding) = binding {
                if !self.binding_is_ambiguous(&binding) {
                    continue;
                }
            } else {
                match self.expand_typed_property_compound(&object, None) {
                    TypedPropertyExpansion::Resolved(terminal, _, _) => {
                        self.member_accesses.push(MemberAccess {
                            object: terminal,
                            member: member.clone(),
                        });
                    }
                    TypedPropertyExpansion::CrossModule {
                        type_name,
                        property_path,
                        ..
                    } => self.record_typed_property_member_fact(
                        type_name,
                        property_path,
                        member.clone(),
                    ),
                    TypedPropertyExpansion::Opaque => {}
                }
            }
            self.member_accesses.push(MemberAccess { object, member });
        }
    }
}
