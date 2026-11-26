//! The Angular template member helpers and `SemanticFactView`, which reads the
//! typed semantic facts of a module.

use super::*;

/// Iterate Angular template member names from a module's typed facts.
pub fn angular_template_member_names(module: &ModuleInfo) -> impl Iterator<Item = &str> {
    angular_template_member_names_from_parts(&module.semantic_facts)
}

/// Return true when the fact slice contains any Angular template member
/// reference.
#[must_use]
fn has_angular_template_members_from_parts(semantic_facts: &[SemanticFact]) -> bool {
    angular_template_member_names_from_parts(semantic_facts)
        .next()
        .is_some()
}

/// Return true when the module contains any Angular template member reference.
#[must_use]
pub fn has_angular_template_members(module: &ModuleInfo) -> bool {
    has_angular_template_members_from_parts(&module.semantic_facts)
}

/// Return true when a module spreads `this` in Angular template context.
#[must_use]
pub fn has_angular_this_spread(module: &ModuleInfo) -> bool {
    SemanticFactView::new(&module.semantic_facts, &module.member_accesses).has_angular_this_spread()
}

/// Return true when a module contains a dynamic custom-element render.
#[must_use]
pub fn has_dynamic_custom_element_render(module: &ModuleInfo) -> bool {
    module
        .semantic_facts
        .iter()
        .any(|fact| matches!(fact, SemanticFact::DynamicCustomElementRender(_)))
}

/// Typed-first view over semantic extraction facts.
///
/// Extraction populates `semantic_facts` directly. The `member_accesses` slice
/// holds only ordinary source member accesses, not encoded semantic facts.
#[derive(Debug, Clone, Copy)]
pub struct SemanticFactView<'a> {
    semantic_facts: &'a [SemanticFact],
    member_accesses: &'a [MemberAccess],
}

impl<'a> SemanticFactView<'a> {
    /// Create a typed semantic fact view from current semantic facts plus
    /// ordinary source member accesses.
    #[must_use]
    pub const fn new(
        semantic_facts: &'a [SemanticFact],
        member_accesses: &'a [MemberAccess],
    ) -> Self {
        Self {
            semantic_facts,
            member_accesses,
        }
    }

    /// Iterate typed semantic facts.
    pub fn facts(self) -> impl Iterator<Item = &'a SemanticFact> + 'a {
        self.semantic_facts.iter()
    }

    /// Iterate Angular template member references.
    pub fn angular_template_member_names(self) -> impl Iterator<Item = &'a str> + 'a {
        angular_template_member_names_from_parts(self.semantic_facts)
    }

    /// Collect Angular component field array-type facts.
    pub fn angular_component_field_array_types(self) -> Vec<AngularComponentFieldArrayTypeFact> {
        angular_component_field_array_type_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Return true when any Angular template member reference exists.
    #[must_use]
    pub fn has_angular_template_members(self) -> bool {
        self.angular_template_member_names().next().is_some()
    }

    /// Return true when a module spreads `this` in Angular template context.
    #[must_use]
    pub fn has_angular_this_spread(self) -> bool {
        self.semantic_facts
            .iter()
            .any(|fact| matches!(fact, SemanticFact::AngularThisSpread(_)))
    }

    /// Iterate ordinary source member accesses.
    pub fn ordinary_member_accesses(self) -> impl Iterator<Item = &'a MemberAccess> + 'a {
        self.member_accesses.iter()
    }

    /// Collect class-scoped `this` member-access facts.
    pub fn class_this_member_accesses(self) -> Vec<ClassThisMemberAccessFact> {
        class_this_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect class-scoped `this` whole-object-use facts.
    pub fn class_this_whole_object_uses(self) -> Vec<ClassThisWholeObjectUseFact> {
        class_this_whole_object_use_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect instance-export binding facts.
    pub fn instance_export_bindings(self) -> Vec<InstanceExportBindingFact> {
        instance_export_binding_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Iterate exported object properties that contain class instances.
    pub fn exported_object_instance_properties(
        self,
    ) -> impl Iterator<Item = &'a ExportedObjectInstancePropertyFact> + 'a {
        exported_object_instance_property_facts(self.semantic_facts)
    }

    /// Iterate proven namespace-qualified class member accesses.
    pub fn qualified_class_member_accesses(
        self,
    ) -> impl Iterator<Item = &'a QualifiedClassMemberAccessFact> + 'a {
        qualified_class_member_access_facts(self.semantic_facts)
    }

    /// Collect static factory call member facts.
    pub fn factory_call_member_accesses(self) -> Vec<FactoryCallMemberAccessFact> {
        factory_call_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect free-function factory-return member facts.
    pub fn factory_fn_member_accesses(self) -> Vec<FactoryFnMemberAccessFact> {
        factory_fn_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect factory-return whole-object consumption facts.
    pub fn factory_fn_whole_objects(self) -> Vec<FactoryFnWholeObjectFact> {
        factory_fn_whole_object_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect object-literal factory-return property member facts.
    pub fn factory_return_object_property_accesses(
        self,
    ) -> Vec<FactoryReturnObjectPropertyAccessFact> {
        factory_return_object_property_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Iterate deferred object-factory property sources without copying their names.
    pub fn factory_return_object_property_sources(
        self,
    ) -> impl Iterator<Item = &'a FactoryReturnObjectPropertySourceFact> + 'a {
        self.semantic_facts.iter().filter_map(|fact| match fact {
            SemanticFact::FactoryReturnObjectPropertySource(source) => Some(source.as_ref()),
            _ => None,
        })
    }

    /// Property accesses with source-owned receiver admission.
    pub fn proven_factory_return_object_property_accesses(
        self,
    ) -> impl Iterator<Item = &'a FactoryReturnObjectPropertyAccessFact> + 'a {
        self.semantic_facts.iter().filter_map(|fact| match fact {
            SemanticFact::ProvenFactoryReturnObjectPropertyAccess(access) => Some(access),
            _ => None,
        })
    }

    /// Iterate caller-owned callback property projections.
    pub fn callback_factory_property_accesses(
        self,
    ) -> impl Iterator<Item = &'a CallbackFactoryPropertyAccessFact> + 'a {
        self.semantic_facts.iter().filter_map(|fact| match fact {
            SemanticFact::CallbackFactoryPropertyAccess(access) => Some(access.as_ref()),
            _ => None,
        })
    }

    /// Collect typed-property-hop member facts.
    pub fn typed_property_member_accesses(self) -> Vec<TypedPropertyMemberAccessFact> {
        typed_property_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect members whose presence is required by a named structural type.
    pub fn required_type_members(self) -> impl Iterator<Item = &'a RequiredTypeMemberFact> + 'a {
        required_type_member_facts(self.semantic_facts)
    }

    /// Collect the parent interfaces of module-local interfaces.
    pub fn interface_extends(self) -> impl Iterator<Item = &'a InterfaceExtendsFact> + 'a {
        self.semantic_facts.iter().filter_map(|fact| match fact {
            SemanticFact::InterfaceExtends(edge) => Some(edge),
            _ => None,
        })
    }

    /// Collect type-alias receiver-surface edges.
    pub fn type_alias_surface_targets(self) -> Vec<TypeAliasSurfaceTargetFact> {
        type_alias_surface_target_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect string-valued enum member definitions.
    pub fn string_enum_member_values(self) -> Vec<StringEnumMemberValueFact> {
        string_enum_member_value_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect computed property accesses keyed by enum members.
    pub fn computed_enum_key_uses(self) -> Vec<ComputedEnumKeyUseFact> {
        computed_enum_key_use_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect static factory fluent-chain member facts.
    pub fn fluent_chain_member_accesses(self) -> Vec<FluentChainMemberAccessFact> {
        fluent_chain_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect constructor-rooted fluent-chain member facts.
    pub fn fluent_chain_new_member_accesses(self) -> Vec<FluentChainNewMemberAccessFact> {
        fluent_chain_new_member_access_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect Playwright fixture-use facts.
    pub fn playwright_fixture_uses(self) -> Vec<PlaywrightFixtureUseFact> {
        playwright_fixture_use_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect Playwright fixture-definition facts.
    pub fn playwright_fixture_definitions(self) -> Vec<PlaywrightFixtureDefinitionFact> {
        playwright_fixture_definition_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect Playwright fixture-alias facts.
    pub fn playwright_fixture_aliases(self) -> Vec<PlaywrightFixtureAliasFact> {
        playwright_fixture_alias_facts(self.semantic_facts)
            .cloned()
            .collect()
    }

    /// Collect conditional Playwright fixture-definition facts.
    pub fn playwright_conditional_fixture_definitions(
        self,
    ) -> Vec<PlaywrightConditionalFixtureDefinitionFact> {
        self.semantic_facts
            .iter()
            .filter_map(|fact| match fact {
                SemanticFact::PlaywrightConditionalFixtureDefinition(access) => {
                    Some(access.as_ref().clone())
                }
                _ => None,
            })
            .collect()
    }

    /// Collect the names of proven Playwright test bindings.
    pub fn playwright_test_bindings(self) -> Vec<PlaywrightTestBindingFact> {
        self.semantic_facts
            .iter()
            .filter_map(|fact| match fact {
                SemanticFact::PlaywrightTestBinding(binding) => Some(binding.clone()),
                _ => None,
            })
            .collect()
    }

    /// Collect Playwright fixture-type facts.
    pub fn playwright_fixture_types(self) -> Vec<PlaywrightFixtureTypeFact> {
        playwright_fixture_type_facts(self.semantic_facts)
            .cloned()
            .collect()
    }
}

/// Iterate ordinary whole-object uses.
pub fn ordinary_whole_object_uses(whole_object_uses: &[String]) -> impl Iterator<Item = &str> {
    whole_object_uses.iter().map(String::as_str)
}

/// Iterate typed instance-export binding facts.
fn instance_export_binding_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &InstanceExportBindingFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::InstanceExportBinding(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

fn exported_object_instance_property_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &ExportedObjectInstancePropertyFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::ExportedObjectInstanceProperty(property) = fact {
            Some(property)
        } else {
            None
        }
    })
}

fn qualified_class_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &QualifiedClassMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::QualifiedClassMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

fn class_this_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &ClassThisMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::ClassThisMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

fn class_this_whole_object_use_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &ClassThisWholeObjectUseFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::ClassThisWholeObjectUse(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

fn angular_component_field_array_type_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &AngularComponentFieldArrayTypeFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::AngularComponentFieldArrayType(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed factory-call member facts.
fn factory_call_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &FactoryCallMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::FactoryCallMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed free-function factory-return member facts.
fn factory_fn_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &FactoryFnMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::FactoryFnMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

fn factory_fn_whole_object_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &FactoryFnWholeObjectFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::FactoryFnWholeObject(fact) = fact {
            Some(fact)
        } else {
            None
        }
    })
}

/// Iterate object-literal factory-return property member facts.
fn factory_return_object_property_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &FactoryReturnObjectPropertyAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::FactoryReturnObjectPropertyAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed fluent-chain member facts.
fn fluent_chain_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &FluentChainMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::FluentChainMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed-property-hop member facts.
fn typed_property_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &TypedPropertyMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::TypedPropertyMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

fn required_type_member_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &RequiredTypeMemberFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::RequiredTypeMember(required) = fact {
            Some(required)
        } else {
            None
        }
    })
}

fn type_alias_surface_target_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &TypeAliasSurfaceTargetFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::TypeAliasSurfaceTarget(fact) = fact {
            Some(fact)
        } else {
            None
        }
    })
}

fn string_enum_member_value_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &StringEnumMemberValueFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::StringEnumMemberValue(fact) = fact {
            Some(fact)
        } else {
            None
        }
    })
}

fn computed_enum_key_use_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &ComputedEnumKeyUseFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::ComputedEnumKeyUse(fact) = fact {
            Some(fact)
        } else {
            None
        }
    })
}

/// Iterate typed constructor-rooted fluent-chain member facts.
fn fluent_chain_new_member_access_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &FluentChainNewMemberAccessFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::FluentChainNewMemberAccess(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}
