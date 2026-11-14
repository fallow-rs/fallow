//! Credit the member accesses on a local with a class binding.
//!
//! The walk records `r.translate` as a read on the local `r`. After the walk,
//! this pass maps each local to the class paths that it is bound to, and
//! records the read on each path.

use super::{BindingTarget, ModuleInfoExtractor, TypedPropertyExpansion};
use crate::{FactoryCallMemberAccessFact, MemberAccess, SemanticFact};

impl ModuleInfoExtractor {
    pub(super) fn resolve_bound_member_accesses(&mut self) {
        if self.binding_target_names.is_empty() && self.receiver_alias_alternatives.is_empty() {
            return;
        }
        let mut additional_accesses = Vec::new();
        let mut additional_facts = Vec::new();
        let mut additional_typed_property_facts = Vec::new();
        for (index, access) in self.member_accesses.iter().enumerate() {
            if self.shadowed_reads.skips(index, &access.object) {
                continue;
            }
            let alternatives = self
                .receiver_alias_alternative_targets(&access.object)
                .into_iter()
                .map(BindingTarget::Class);
            let targets = self
                .resolve_bound_object_name(&access.object)
                .into_iter()
                .chain(alternatives);
            for target in targets {
                match target {
                    BindingTarget::Class(object) => {
                        // A compound target (`Opts.c` from `this.opts.c` with binding
                        // `this.opts -> Opts`) may hop through a named interface /
                        // type-literal alias (issue #1785) or a locally-declared
                        // class's typed-property bindings (issue #1788); expand it
                        // so the terminal property type is credited. The compound
                        // access itself is still pushed: exported-class compounds
                        // also resolve downstream via `instance_bindings`, and
                        // interface compounds are inert.
                        match self.expand_typed_property_compound(&object, None) {
                            TypedPropertyExpansion::Resolved(terminal, _, _) => {
                                additional_accesses.push(MemberAccess {
                                    object: terminal,
                                    member: access.member.clone(),
                                });
                            }
                            TypedPropertyExpansion::CrossModule {
                                type_name,
                                property_path,
                                ..
                            } => additional_typed_property_facts.push((
                                type_name,
                                property_path,
                                access.member.clone(),
                            )),
                            TypedPropertyExpansion::Opaque => {}
                        }
                        additional_accesses.push(MemberAccess {
                            object,
                            member: access.member.clone(),
                        });
                    }
                    BindingTarget::FactoryCall {
                        callee_object,
                        callee_method,
                    } => {
                        additional_facts.push((
                            callee_object,
                            callee_method,
                            access.member.clone(),
                        ));
                    }
                    BindingTarget::Ambiguous => {}
                }
            }
        }
        let additional_whole: Vec<String> =
            self.whole_object_uses
                .iter()
                .filter_map(|name| self.resolve_bound_object_name(name))
                .filter_map(|target| {
                    if let BindingTarget::Class(name) = target {
                        Some(name)
                    } else {
                        None
                    }
                })
                .flat_map(|name| {
                    // Mirror the member-access expansion for whole-object uses:
                    // `use(this.opts.c)` through a local interface hop credits the
                    // terminal class wholesale. Cross-module whole-object hops are
                    // out of scope (issue #1785 covers member accesses).
                    let expanded = match self.expand_typed_property_compound(&name, None) {
                        TypedPropertyExpansion::Resolved(terminal, _, _) => Some(terminal),
                        TypedPropertyExpansion::CrossModule { .. }
                        | TypedPropertyExpansion::Opaque => None,
                    };
                    std::iter::once(name).chain(expanded)
                })
                .collect();
        self.member_accesses.extend(additional_accesses);
        for (callee_object, callee_method, member) in additional_facts {
            self.semantic_facts
                .push(SemanticFact::FactoryCallMemberAccess(
                    FactoryCallMemberAccessFact {
                        callee_object,
                        callee_method,
                        member,
                    },
                ));
        }
        for (type_name, property_path, member) in additional_typed_property_facts {
            self.record_typed_property_member_fact(type_name, property_path, member);
        }
        self.whole_object_uses.extend(additional_whole);
    }
}
