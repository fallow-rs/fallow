//! Template usage, direct object binding reads, walk-order member accesses, JSX
//! member tags, and the typed member facts.

use super::*;

impl ModuleInfoExtractor {
    /// Run the bound-member resolution finalize and take every member access and
    /// whole-object use this extractor recorded. For the Astro template-expression
    /// pass: a fresh extractor seeded with the frontmatter element types visits
    /// the template `{...}` expression regions, `bind_iterable_callback_parameter`
    /// types each `.map((util) => ...)` callback param to its element class during
    /// the walk, and `resolve_bound_member_accesses` re-emits the class-qualified
    /// access (`Util.getter`, issue #1713). The raw accesses (`NS.Moon`,
    /// `utils.map`) are kept as well: namespace-import narrowing is only sound
    /// when the stream holds every access of a binding, not just the tagged ones
    /// (issue #2355), and they are the same pairs the visitor records for `.tsx`.
    /// Accesses are span-less name pairs, so no un-remapped template-region span
    /// ever leaks into the module, and `this@<id>` qualifiers are stripped exactly
    /// as `finalize_resolution_phase` does before any spelling is emitted.
    pub(crate) fn take_template_expression_usage(&mut self) -> (Vec<MemberAccess>, Vec<String>) {
        self.begin_resolution_phase();
        self.resolve_bound_member_accesses();
        self.finish_this_reads();
        (
            std::mem::take(&mut self.member_accesses),
            std::mem::take(&mut self.whole_object_uses),
        )
    }

    pub(super) fn direct_object_binding_targets(
        &self,
        object: &str,
    ) -> Option<(&FxHashSet<String>, u64)> {
        let root = object.split_once('.').map_or(object, |(root, _)| root);
        for index in (0..self.scoped_direct_object_binding_targets.len()).rev() {
            if let Some(targets) = self.scoped_direct_object_binding_targets[index].get(object) {
                let generation = self.scoped_direct_object_binding_generations[index]
                    .get(object)
                    .copied()
                    .unwrap_or_default();
                return (!targets.names.is_empty()).then_some((&targets.names, generation));
            }
            if self.scoped_direct_object_binding_roots[index].contains(root) {
                return None;
            }
        }
        self.module_direct_object_binding_targets
            .get(object)
            .map(|targets| {
                let generation = self
                    .module_direct_object_binding_generations
                    .get(object)
                    .copied()
                    .unwrap_or_default();
                (&targets.names, generation)
            })
            .filter(|(targets, _)| !targets.is_empty())
    }

    /// Remember the class a receiver named at this point in the walk, so an
    /// access written in one scope survives the same name being rebound to a
    /// different class in a sibling scope.
    pub(super) fn record_walk_order_member_access(&mut self, object: &str, member: &str) -> bool {
        self.observe_loop_update_access(object, member);
        if self.abstained_direct_object_binding_paths.contains(object)
            && self.direct_object_binding_targets(object).is_some()
        {
            return true;
        }
        if let Some((classes, generation)) = self.direct_object_binding_targets(object) {
            if self
                .direct_object_binding_access_generations
                .get(object)
                .and_then(|members| members.get(member))
                == Some(&generation)
            {
                return true;
            }
            if self.direct_object_binding_member_accesses.len()
                >= MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES
            {
                self.abstain_direct_object_binding_path(object);
                return true;
            }
            let mut classes = classes.iter().cloned().collect::<Vec<_>>();
            classes.sort_unstable();
            for class in classes {
                if self.direct_object_binding_member_accesses.len()
                    >= MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES
                {
                    self.abstain_direct_object_binding_path(object);
                    return true;
                }
                let access = (class, member.to_string());
                if self
                    .direct_object_binding_member_accesses
                    .insert(access.clone())
                {
                    if let Some((namespace_local, class_export_name)) = access.0.split_once('.') {
                        self.semantic_facts
                            .push(SemanticFact::QualifiedClassMemberAccess(
                                QualifiedClassMemberAccessFact {
                                    namespace_local: namespace_local.to_string(),
                                    class_export_name: class_export_name.to_string(),
                                    member: access.1,
                                },
                            ));
                    } else {
                        self.member_accesses.push(MemberAccess {
                            object: access.0,
                            member: access.1,
                        });
                    }
                }
            }
            self.direct_object_binding_access_generations
                .entry(object.to_string())
                .or_default()
                .insert(member.to_string(), generation);
            return true;
        }
        let mut prefix = object;
        loop {
            if let Some(targets) = self.direct_object_binding_path_targets(prefix) {
                let suffix = &object[prefix.len()..];
                let mut snapshots = targets
                    .iter()
                    .map(|target| (None, format!("{target}{suffix}"), member.to_string()))
                    .collect::<Vec<_>>();
                snapshots.sort_unstable();
                self.walk_order_member_accesses.extend(snapshots);
                // An indexed empty prefix was explicitly replaced. Neither it
                // nor an admitted snapshot may fall through to stale flat names.
                return true;
            }
            let Some((parent, _)) = prefix.rsplit_once('.') else {
                break;
            };
            prefix = parent;
        }
        if let Some(class) = self.walk_order_class_bindings.get(object)
            && !self.walk_read_is_shadowed(object)
        {
            self.walk_order_member_accesses.push((
                Some(object.to_string()),
                class.clone(),
                member.to_string(),
            ));
        }
        false
    }

    pub(super) fn record_dynamic_direct_object_binding_member_access(
        &mut self,
        object: &str,
        member: &str,
    ) -> bool {
        if self
            .dynamic_direct_object_binding_abstention_generations
            .get(object)
            == Some(&self.direct_object_binding_generation)
        {
            return true;
        }
        let access_key = (object.to_string(), member.to_string());
        if self
            .dynamic_direct_object_binding_access_generations
            .get(&access_key)
            == Some(&self.direct_object_binding_generation)
        {
            return true;
        }
        let root = object.split_once('.').map_or(object, |(root, _)| root);
        let prefix = format!("{object}.");
        let mut paths = None;
        for index in (0..self.scoped_direct_object_binding_targets.len()).rev() {
            if let Some(root_paths) =
                self.scoped_direct_object_binding_paths_by_root[index].get(root)
            {
                paths = Some(
                    root_paths
                        .iter()
                        .filter(|path| path.starts_with(&prefix))
                        .cloned()
                        .collect::<Vec<_>>(),
                );
                break;
            }
            if self.scoped_direct_object_binding_roots[index].contains(root) {
                return false;
            }
        }
        let mut paths = paths.unwrap_or_else(|| {
            self.module_direct_object_binding_paths_by_root
                .get(root)
                .into_iter()
                .flatten()
                .filter(|path| path.starts_with(&prefix))
                .cloned()
                .collect()
        });
        paths.sort_unstable();
        if paths.is_empty() {
            return false;
        }
        let mut recorded = false;
        for path in &paths {
            recorded |= self.record_walk_order_member_access(path, member);
            if self.direct_object_binding_member_accesses.len()
                >= MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES
            {
                for matching_path in &paths {
                    self.abstain_direct_object_binding_path(matching_path);
                }
                self.dynamic_direct_object_binding_abstention_generations
                    .insert(object.to_string(), self.direct_object_binding_generation);
                break;
            }
        }
        if recorded {
            self.dynamic_direct_object_binding_access_generations
                .insert(access_key, self.direct_object_binding_generation);
        }
        recorded
    }

    pub(super) fn abstain_direct_object_binding_path(&mut self, object: &str) {
        let mut classes = FxHashSet::default();
        if let Some(module_classes) = self.module_direct_object_binding_targets.get(object) {
            classes.extend(module_classes.iter().cloned());
        }
        for targets in &self.scoped_direct_object_binding_targets {
            if let Some(scoped_classes) = targets.get(object) {
                classes.extend(scoped_classes.iter().cloned());
            }
        }
        for class in classes {
            if self
                .direct_object_binding_whole_object_uses
                .insert(class.clone())
            {
                self.whole_object_uses.push(class);
            }
        }
        self.abstained_direct_object_binding_paths
            .insert(object.to_string());
    }

    /// Record member accesses for a JSX member-expression tag (`<SC.UsedStyle />`,
    /// `<A.B.C />`), mirroring the plain-expression walk of `A.B.C`: one access
    /// per nesting level (`{A.B, C}` and `{A, B}`), so namespace-import
    /// narrowing sees the same stream either way (issue #2348). `this` receivers
    /// get the same class-scope qualifier as `visit_static_member_expression`.
    pub(super) fn record_jsx_member_tag_accesses(&mut self, member: &JSXMemberExpression<'_>) {
        let mut current = member;
        loop {
            if let Some(object_name) = visit_impl::jsx_member_object_name(&current.object)
                && !self.namespace_like_binding_is_shadowed(&object_name)
            {
                let object = self.qualify_this_scope(&object_name);
                if !self.record_walk_order_member_access(&object, current.property.name.as_str()) {
                    self.push_receiver_member_access(object, current.property.name.to_string());
                }
            }
            match &current.object {
                JSXMemberExpressionObject::MemberExpression(inner) => current = inner,
                JSXMemberExpressionObject::IdentifierReference(reference) => {
                    self.record_jsx_subclass_member_access(
                        reference,
                        current.property.name.as_str(),
                    );
                    break;
                }
                JSXMemberExpressionObject::ThisExpression(_) => break,
            }
        }
    }

    /// `<Sub.Item />` reads `Item` through the tag root. Record it with the same
    /// identifier proof as `Sub.Item`, so a local subclass credits its base.
    fn record_jsx_subclass_member_access(
        &mut self,
        reference: &oxc_ast::ast::IdentifierReference<'_>,
        member: &str,
    ) {
        if !self.capture_subclass_provenance {
            return;
        }
        let proof = self
            .direct_binding_class_proof(reference.name.as_str(), Some(reference.span))
            .unwrap_or_else(|| BindingClassProof {
                builtin_containers: std::sync::Arc::default(),
                target_name: reference.name.to_string(),
                target_reference_span: reference.span,
                requirements: BindingRequirements::Empty,
            });
        self.pending_subclass_member_accesses
            .push(PendingSubclassMemberAccess {
                proof: SubclassAccessProof::Known(proof),
                member: member.to_string(),
            });
    }

    pub(super) fn record_angular_template_member_fact(&mut self, member: String) {
        self.semantic_facts
            .push(SemanticFact::AngularTemplateMemberAccess(
                AngularTemplateMemberAccessFact { member },
            ));
    }

    pub(super) fn record_factory_fn_member_fact(
        &mut self,
        callee_name: String,
        member: String,
        awaited: bool,
    ) {
        self.semantic_facts
            .push(SemanticFact::FactoryFnMemberAccess(
                FactoryFnMemberAccessFact {
                    callee_name,
                    member,
                    awaited,
                    element: false,
                },
            ));
    }

    /// Whether a call of the same-file function `callee` gives the value that the
    /// function returns: a sync call, or an awaited call of an `async` function.
    pub(super) fn call_result_is_returned_value(&self, callee: &str, awaited: bool) -> bool {
        if awaited {
            !self.generator_factory_functions.contains(callee)
        } else {
            !self.promise_factory_functions.contains(callee)
        }
    }

    pub(super) fn record_typed_property_member_fact(
        &mut self,
        type_name: String,
        property_path: String,
        member: String,
    ) {
        self.semantic_facts
            .push(SemanticFact::TypedPropertyMemberAccess(
                TypedPropertyMemberAccessFact {
                    type_name,
                    property_path,
                    member,
                },
            ));
    }

    pub(super) fn record_playwright_fixture_definition_fact(
        &mut self,
        test_name: String,
        fixture_name: String,
        type_name: String,
    ) {
        self.semantic_facts
            .push(SemanticFact::PlaywrightFixtureDefinition(
                PlaywrightFixtureDefinitionFact {
                    test_name,
                    fixture_name,
                    type_name,
                },
            ));
    }
}
