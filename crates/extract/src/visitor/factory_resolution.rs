//! Exported instance bindings, Playwright fixtures, and the resolution of factory
//! calls, returns, aliases and object shapes.

use super::*;

impl ModuleInfoExtractor {
    pub(super) fn record_exported_instance_bindings(&mut self) {
        if self.binding_target_names.is_empty() {
            return;
        }

        for export in self.exports.clone() {
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            let (target_name, factory_method) = match self.binding_target_names.get(local_name) {
                Some(BindingTarget::Class(class_name)) => (class_name.clone(), None),
                Some(BindingTarget::FactoryCall {
                    callee_object,
                    callee_method,
                }) => (callee_object.clone(), Some(callee_method.clone())),
                Some(BindingTarget::Ambiguous) | None => continue,
            };
            self.semantic_facts
                .push(SemanticFact::InstanceExportBinding(
                    InstanceExportBindingFact {
                        export_name: export.name.to_string(),
                        target_name,
                        factory_method,
                    },
                ));
        }
    }

    pub(super) fn resolve_playwright_factory_call_definitions(&mut self) {
        let pending_calls = std::mem::take(&mut self.pending_playwright_factory_calls);
        let pending_aliases = std::mem::take(&mut self.pending_playwright_factory_aliases);
        let local_fixture_defs = std::mem::take(&mut self.playwright_local_fixture_defs);
        if pending_calls.is_empty() && pending_aliases.is_empty() {
            return;
        }

        let mut factory_bindings: FxHashMap<String, Vec<(String, String)>> = FxHashMap::default();
        // Helper bindings on an imported base, keyed by helper name. Each entry
        // is `(base, fixture, type)` and becomes a conditional fact.
        let mut conditional_bindings: FxHashMap<String, Vec<(String, String, String)>> =
            FxHashMap::default();
        for entry in pending_calls {
            match entry.base_name {
                Some(base) if !self.is_playwright_test_name(&base) => {
                    if !self.is_conditional_playwright_base(&base) {
                        continue;
                    }
                    conditional_bindings
                        .entry(entry.test_name)
                        .or_default()
                        .extend(entry.type_bindings.into_iter().map(
                            |(fixture_name, type_name)| (base.clone(), fixture_name, type_name),
                        ));
                }
                _ => factory_bindings
                    .entry(entry.test_name)
                    .or_default()
                    .extend(entry.type_bindings),
            }
        }
        for bindings in factory_bindings.values_mut() {
            bindings.sort();
            bindings.dedup();
        }
        for bindings in conditional_bindings.values_mut() {
            bindings.sort();
            bindings.dedup();
        }

        let max_iters = pending_aliases.len() + 1;
        for _ in 0..max_iters {
            let mut changed = false;
            for (caller, callee) in &pending_aliases {
                if factory_bindings.contains_key(caller) {
                    continue;
                }
                // Inherit from another captured helper factory OR a local
                // `base.extend<T>({...})` fixture const wrapped via `<X>.extend(...)`
                // (issue #1791). Local const defs are only an inheritance SOURCE:
                // their facts were already emitted directly, so they are never
                // re-emitted (only `factory_bindings` keys, the helper names, are).
                if let Some(bindings) = factory_bindings
                    .get(callee)
                    .or_else(|| local_fixture_defs.get(callee))
                    .cloned()
                {
                    factory_bindings.insert(caller.clone(), bindings);
                    changed = true;
                } else if !conditional_bindings.contains_key(caller)
                    && let Some(bindings) = conditional_bindings.get(callee).cloned()
                {
                    conditional_bindings.insert(caller.clone(), bindings);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        for (test_name, bindings) in conditional_bindings {
            for (base_name, fixture_name, type_name) in bindings {
                self.record_playwright_conditional_fixture_definition_fact(
                    test_name.clone(),
                    base_name,
                    fixture_name,
                    type_name,
                );
            }
        }

        for (test_name, bindings) in factory_bindings {
            for (fixture_name, type_name) in bindings {
                self.record_playwright_fixture_definition_fact(
                    test_name.clone(),
                    fixture_name,
                    type_name,
                );
            }
        }
    }

    /// Mark each local name proven to hold a Playwright test. An untyped
    /// `base.extend({...})` emits no definition, so without this fact another
    /// file could not prove that an import of it is a Playwright test.
    pub(super) fn record_playwright_test_binding_facts(&mut self) {
        let mut names: Vec<String> = std::mem::take(&mut self.playwright_local_tests)
            .into_iter()
            .collect();
        names.sort_unstable();
        self.semantic_facts
            .extend(names.into_iter().map(|test_name| {
                SemanticFact::PlaywrightTestBinding(PlaywrightTestBindingFact { test_name })
            }));
    }

    pub(super) fn resolve_factory_call_candidates(&mut self) {
        if self.factory_call_candidates.is_empty() {
            return;
        }
        let candidates = std::mem::take(&mut self.factory_call_candidates);
        // Candidates come from every scope. Two different calls for one name
        // (a module-level `const sm = A.getInstance()` and a `const sm =
        // B.getInstance()` in a function) give no single target, the same as
        // two `new` expressions. The exported-instance fact reads this map
        // across modules, so the first candidate must not win. See #5233.
        let mut first_call: FxHashMap<&str, (&str, &str)> = FxHashMap::default();
        let mut conflicting: FxHashSet<String> = FxHashSet::default();
        for candidate in &candidates {
            let call = (
                candidate.callee_object.as_str(),
                candidate.callee_method.as_str(),
            );
            if *first_call
                .entry(candidate.local_name.as_str())
                .or_insert(call)
                != call
            {
                conflicting.insert(candidate.local_name.clone());
            }
        }
        for candidate in candidates {
            let FactoryCallCandidate {
                local_name,
                callee_object,
                callee_method,
            } = candidate;

            if self.binding_target_names.contains_key(&local_name) {
                continue;
            }
            if conflicting.contains(&local_name) {
                self.binding_target_names
                    .insert(local_name, BindingTarget::Ambiguous);
                continue;
            }

            if let Some(local_class) = self.local_class_exports.get(&callee_object)
                && local_class.members.iter().any(|m| {
                    m.is_instance_returning_static
                        && m.kind == MemberKind::ClassMethod
                        && m.name == callee_method
                })
            {
                self.insert_class_binding_target(local_name, callee_object);
                continue;
            }

            let has_import = self
                .imports
                .iter()
                .any(|import| import.local_name == callee_object);
            if has_import {
                self.binding_target_names.insert(
                    local_name,
                    BindingTarget::FactoryCall {
                        callee_object,
                        callee_method,
                    },
                );
            }
        }
    }

    /// Promote `useApi() { return api }`-style functions so `const x = useApi()`
    /// credits the class. TWO promotions with different proofs:
    ///
    /// - SAME-FILE (loose) `factory_return_functions`: the returned identifier
    ///   resolves to a class in `binding_target_names` (e.g. a typed `let api:
    ///   RESTApi`). A type annotation is acceptable here, the blast radius is one
    ///   file. This preserves the original var-return behavior.
    /// - CROSS-MODULE (strict) `strict_factory_return_functions`: ALSO requires a
    ///   VALUE proof, the returned local must be assigned `new Class()` or a
    ///   strict same-file factory (`value_prove_alias`), and the function must be
    ///   sync + non-falling-through (`strict_alias_eligible`). A type annotation
    ///   alone (`let api: RESTApi` assigned a mock) must NOT leak into
    ///   cross-module credit. See #1441 (Part A).
    pub(super) fn resolve_factory_return_aliases(&mut self) {
        if self.factory_return_alias_functions.is_empty() {
            return;
        }
        let aliases = std::mem::take(&mut self.factory_return_alias_functions);
        for (fn_name, returned_id) in aliases {
            if self.factory_return_functions.contains_key(&fn_name) {
                continue;
            }
            let Some(class_name) = self
                .binding_target_names
                .get(&returned_id)
                .and_then(BindingTarget::class_name)
            else {
                continue;
            };
            let class_name = class_name.to_string();
            // Cross-module strict promotion: a sync, terminal body AND a VALUE
            // proof tied to THIS function. Done before the loose insert so a
            // type-only binding never reaches the strict map.
            if self.strict_alias_eligible.contains(&fn_name)
                && let Some(proven_class) = self.value_prove_alias(&fn_name, &returned_id)
            {
                self.strict_factory_return_functions
                    .insert(fn_name.clone(), proven_class);
            }
            self.factory_return_functions.insert(fn_name, class_name);
        }
    }

    /// VALUE-prove the class an alias factory `fn_name` returns through its
    /// returned identifier `returned_id`. The proof is tied to the function:
    /// assignments to `returned_id` inside `fn_name`'s OWN body
    /// (`alias_in_body_assignments`) plus a MODULE-SCOPE initializer of
    /// `returned_id` (`module_scope_initializers`), never an assignment in an
    /// unrelated/sibling function. `new Class()` proves directly; `factory()`
    /// proves only when `factory` is a strict same-file factory. Proven ONLY with
    /// at least one resolvable source and ALL sources agreeing on one class; any
    /// `Other`/unresolved/conflicting source abstains. So `let api: RESTApi` with
    /// no class-proven write is not proven. See #1441 (Part A).
    fn value_prove_alias(&self, fn_name: &str, returned_id: &str) -> Option<String> {
        // PROOF: dominating writes, the alias's own dominating in-body
        // assignments and module-scope initializers. Must be unanimous on one
        // class, with at least one source.
        let mut class: Option<String> = None;
        let mut saw_source = false;
        let proof_sources = self
            .alias_in_body_assignments
            .get(fn_name)
            .into_iter()
            .flatten()
            .chain(
                self.module_scope_initializers
                    .get(returned_id)
                    .into_iter()
                    .flatten(),
            );
        for value in proof_sources {
            saw_source = true;
            let resolved = self.resolve_factory_assigned_value(value)?;
            match &class {
                None => class = Some(resolved),
                Some(existing) if *existing == resolved => {}
                Some(_) => return None,
            }
        }
        let class = if saw_source { class? } else { return None };

        // POISON: ANY write to the binding (any scope, incl. sibling functions
        // and non-dominating branches) that is `Other`/unresolved or a CONFLICTING
        // class means the binding can hold a non-`class` value at return time
        // (e.g. `poison() { api = {} as any }`). Abstain. A write that resolves to
        // the same class is harmless. See #1441 (Part A).
        let poison_sources = self
            .identifier_write_values
            .get(returned_id)
            .into_iter()
            .flatten()
            .chain(
                self.module_scope_initializers
                    .get(returned_id)
                    .into_iter()
                    .flatten(),
            );
        for value in poison_sources {
            match self.resolve_factory_assigned_value(value) {
                Some(resolved) if resolved == class => {}
                _ => return None,
            }
        }
        Some(class)
    }

    /// Resolve a classified assignment value to the class it produces: a direct
    /// `new Class()`, or a `factory()` call only when the callee is a strict
    /// same-file factory. `Other` and unresolved calls yield `None`. #1441 (A).
    fn resolve_factory_assigned_value(&self, value: &FactoryAssignedValue) -> Option<String> {
        match value {
            FactoryAssignedValue::NewClass(name) => Some(name.clone()),
            FactoryAssignedValue::Call(callee) => {
                self.strict_factory_return_functions.get(callee).cloned()
            }
            FactoryAssignedValue::Other => None,
        }
    }

    /// Resolve `const x = useApi()` bindings. A same-file factory whose body
    /// returns `new Class()` binds `x` directly to the class so `x.member`
    /// credits it. An IMPORTED factory callee instead emits a typed
    /// `FactoryFnMemberAccess` fact so the analyze layer resolves the returned
    /// class across the module boundary via `exported_factory_returns`. See issue
    /// #1441 (Part A).
    pub(super) fn resolve_factory_return_candidates(&mut self) {
        if self.factory_return_candidates.is_empty() {
            return;
        }
        let candidates = std::mem::take(&mut self.factory_return_candidates);
        let mut deferred_factory_facts: Vec<(String, String, bool)> = Vec::new();
        let mut deferred_object_property_facts: Vec<(String, String, String)> = Vec::new();
        let mut deferred_member_accesses: Vec<MemberAccess> = Vec::new();
        for candidate in candidates {
            if let Some(property) = &candidate.property_path {
                self.resolve_destructured_property_candidate(
                    &candidate,
                    property,
                    &mut deferred_member_accesses,
                    &mut deferred_object_property_facts,
                );
                continue;
            }
            // Same-file factory returning `new Class()`: bind the local to the
            // class so `resolve_bound_member_accesses` credits `x.member` directly.
            if let Some(class_name) = self.factory_return_functions.get(&candidate.callee_name) {
                // A plain call of an `async` factory gives a promise, not the class.
                if !self.call_result_is_returned_value(&candidate.callee_name, candidate.awaited) {
                    continue;
                }
                let class_name = class_name.clone();
                self.binding_target_names
                    .entry(candidate.local_name)
                    .or_insert(BindingTarget::Class(class_name));
                continue;
            }
            // Same-file factory returning an OBJECT LITERAL (`const ui = createUi();
            // ui.orders.member`): resolve each `<local>.<property-path>` access
            // through the captured shape and credit the property's class member
            // directly. The `<local>.` prefix is a segment boundary, so `uiExtra`
            // is not captured by `ui`. See issue #1858.
            if let Some(shape) = self
                .factory_return_object_shapes
                .get(&candidate.callee_name)
            {
                let prefix = format!("{}.", candidate.local_name);
                for access in &self.member_accesses {
                    let Some(property_path) = access.object.strip_prefix(&prefix) else {
                        continue;
                    };
                    if let Some(property) = shape.iter().find(|p| p.property_path == property_path)
                    {
                        deferred_member_accesses.push(MemberAccess {
                            object: property.class_local_name.clone(),
                            member: access.member.clone(),
                        });
                    }
                }
                continue;
            }
            // Cross-module: `const x = importedFactory()`. We do NOT route through
            // `binding_target_names` here: the Pinia store-consumption heuristic
            // (`is_store_factory_call`) already weakly binds every imported-call
            // local to its bare callee name, which would shadow a fact binding.
            // Instead emit the factory-fn member facts directly for the local's
            // first-level reads. The analyze layer credits a class only when the
            // callee resolves to a proven exported factory return; for any other
            // callee (a real store, a plain helper) it is a harmless no-op.
            // See issue #1441 (Part A).
            let callee_is_imported = self
                .imports
                .iter()
                .any(|import| import.local_name == candidate.callee_name);
            if !callee_is_imported {
                continue;
            }
            let object_prefix = format!("{}.", candidate.local_name);
            for access in &self.member_accesses {
                if access.object == candidate.local_name {
                    deferred_factory_facts.push((
                        candidate.callee_name.clone(),
                        access.member.clone(),
                        candidate.awaited,
                    ));
                } else if let Some(property_path) = access.object.strip_prefix(&object_prefix) {
                    // `const ui = importedObjectFactory(); ui.orders.member`: emit a
                    // fact the analyze layer joins against the factory's exported
                    // object shape. The `<local>.` prefix is a segment boundary. #1858.
                    deferred_object_property_facts.push((
                        candidate.callee_name.clone(),
                        property_path.to_string(),
                        access.member.clone(),
                    ));
                }
            }
        }
        self.member_accesses.extend(deferred_member_accesses);
        for (callee_name, member, awaited) in deferred_factory_facts {
            self.record_factory_fn_member_fact(callee_name, member, awaited);
        }
        for (callee_name, property_path, member) in deferred_object_property_facts {
            self.semantic_facts
                .push(SemanticFact::FactoryReturnObjectPropertyAccess(
                    FactoryReturnObjectPropertyAccessFact {
                        callee_name,
                        property_path,
                        member,
                    },
                ));
        }
    }

    /// Resolve the reads on `local` from `const { property: local } = callee()`
    /// as reads on `callee().property` (issue #5298). A same-file object
    /// factory credits the class of the matching shape path. An imported callee
    /// emits the object-property fact that the analyze layer joins against the
    /// exported object shape of the factory.
    fn resolve_destructured_property_candidate(
        &self,
        candidate: &FactoryReturnCandidate,
        property: &str,
        member_accesses: &mut Vec<MemberAccess>,
        object_property_facts: &mut Vec<(String, String, String)>,
    ) {
        if self
            .binding_name_counts
            .get(&candidate.local_name)
            .is_some_and(|count| *count > 1)
        {
            return;
        }
        let object_prefix = format!("{}.", candidate.local_name);
        let paths = self.member_accesses.iter().filter_map(|access| {
            if access.object == candidate.local_name {
                return Some((property.to_string(), access.member.clone()));
            }
            let rest = access.object.strip_prefix(&object_prefix)?;
            Some((format!("{property}.{rest}"), access.member.clone()))
        });
        if let Some(shape) = self
            .factory_return_object_shapes
            .get(&candidate.callee_name)
        {
            if !self.call_result_is_returned_value(&candidate.callee_name, false) {
                return;
            }
            for (path, member) in paths {
                if let Some(entry) = shape.iter().find(|entry| entry.property_path == path) {
                    member_accesses.push(MemberAccess {
                        object: entry.class_local_name.clone(),
                        member,
                    });
                }
            }
            return;
        }
        let callee_is_imported = self
            .imports
            .iter()
            .any(|import| import.local_name == candidate.callee_name);
        if !callee_is_imported {
            return;
        }
        for (path, member) in paths {
            object_property_facts.push((candidate.callee_name.clone(), path, member));
        }
    }

    /// Credit a member read straight off a factory result the source never named
    /// (`f().member`, `const { member } = f()`).
    ///
    /// The callee and the member are both known at capture time, so resolve them
    /// directly rather than routing through a stand-in local: a stand-in would make
    /// every `helper().x` in a file a candidate, and candidate resolution rescans
    /// every member access, which is quadratic on a file full of such calls.
    ///
    /// A same-file factory binds the class immediately. An imported callee emits the
    /// typed fact the analyze layer resolves through `exported_factory_returns`; any
    /// other callee resolves to no proven factory export and is a no-op there.
    ///
    /// The callee is matched by name and not by scope, so a local binding that shadows
    /// an imported factory is treated as that factory. It can only ADD credit, so the
    /// worst case is a member that stays unreported.
    pub(super) fn resolve_factory_unnamed_result_accesses(&mut self) {
        if self.factory_unnamed_result_accesses.is_empty() {
            return;
        }
        let inline_accesses = std::mem::take(&mut self.factory_unnamed_result_accesses);
        // Indexed once: a file with many `helper().x` reads would otherwise rescan
        // every import per access.
        let imported_locals: FxHashSet<&str> = self
            .imports
            .iter()
            .map(|import| import.local_name.as_str())
            .collect();
        let mut deferred_facts = Vec::new();
        for (callee_name, member, awaited) in inline_accesses {
            if let Some(class_name) = self.factory_return_functions.get(&callee_name) {
                if !self.call_result_is_returned_value(&callee_name, awaited) {
                    continue;
                }
                let object = class_name.clone();
                self.member_accesses.push(MemberAccess { object, member });
                continue;
            }
            if imported_locals.contains(callee_name.as_str()) {
                deferred_facts.push((callee_name, member, awaited));
            }
        }
        for (callee_name, member, awaited) in deferred_facts {
            self.record_factory_fn_member_fact(callee_name, member, awaited);
        }
    }

    /// `const { a, ...rest } = f()` and `const { [k]: v } = f()` can read ANY property
    /// of the factory result, so no set of visible keys describes what is used.
    /// Credit the returned class wholesale rather than credit the keys we happen to
    /// see: crediting only `a` would leave every other live member reported as dead.
    pub(super) fn resolve_factory_whole_object_candidates(&mut self) {
        if self.factory_whole_object_candidates.is_empty() {
            return;
        }
        let callees = std::mem::take(&mut self.factory_whole_object_candidates);
        for callee_name in callees {
            // Same-file factory: the class is known, mark it used wholesale.
            if let Some(class_name) = self.factory_return_functions.get(&callee_name) {
                if !self.call_result_is_returned_value(&callee_name, false) {
                    continue;
                }
                let class_name = class_name.clone();
                self.whole_object_uses.push(class_name);
                continue;
            }
            // Cross-module: the analyze layer resolves the callee to the class it
            // returns and suppresses that export.
            if self
                .imports
                .iter()
                .any(|import| import.local_name == callee_name)
            {
                self.semantic_facts.push(SemanticFact::FactoryFnWholeObject(
                    FactoryFnWholeObjectFact { callee_name },
                ));
            }
        }
    }

    /// Resolve each captured object-literal factory-return candidate's property
    /// values to terminal LOCAL class names, populating the same-file (loose) and
    /// cross-module (strict) shape maps. Reuses `resolve_bound_object_name` +
    /// `expand_typed_property_compound` (the proven receiver-type resolution) to turn
    /// a `new X()` / local-alias / `factory.prop` value into the class it instances.
    /// A value that does not resolve to a same-file terminal class is dropped (a
    /// conservative false negative, never an over-credit).
    ///
    /// I1 (issue #1858 FP-safety): this pass READS `binding_target_names` /
    /// `local_class_exports` / `interface_property_types` only; it never mutates
    /// `member_accesses` or `binding_target_names`. Must run after those maps are
    /// populated and before `resolve_factory_return_candidates` (the same-file
    /// consumer arm reads `factory_return_object_shapes`).
    pub(super) fn resolve_factory_return_object_shapes(&mut self) {
        if self.factory_return_object_candidates.is_empty() {
            return;
        }
        let candidates = std::mem::take(&mut self.factory_return_object_candidates);
        for candidate in candidates {
            let mut resolved: Vec<fallow_types::extract::FactoryReturnObjectProperty> = Vec::new();
            for (property_path, value) in &candidate.properties {
                let Some(class_local_name) = self.resolve_object_property_value_class(value) else {
                    continue;
                };
                resolved.push(fallow_types::extract::FactoryReturnObjectProperty {
                    property_path: property_path.clone(),
                    class_local_name,
                });
            }
            if resolved.is_empty() {
                continue;
            }
            if candidate.is_strict_eligible {
                self.strict_factory_return_object_shapes
                    .insert(candidate.fn_name.clone(), resolved.clone());
            }
            self.factory_return_object_shapes
                .insert(candidate.fn_name, resolved);
        }
    }

    /// Resolve one object-literal property value to a terminal local class name.
    ///
    /// A `new X()` leaf names the class directly. A `Path` leaf (`o`,
    /// `factory.ordersPage`) resolves through `binding_target_names`: a bare local
    /// bound to `new X()` yields `Class("X")` (no dot) taken directly; a compound
    /// (`OrdersInvokerFactory.ordersPage`) hops through the class's typed-property
    /// binding via `expand_typed_property_compound`. A cross-module factory class,
    /// an opaque hop, or an unbound path drops. See issue #1858.
    fn resolve_object_property_value_class(&self, value: &ObjectPropValueRef) -> Option<String> {
        match value {
            ObjectPropValueRef::Class(name, _) => Some(name.clone()),
            ObjectPropValueRef::Path(path) => match self.resolve_bound_object_name(path)? {
                // A bare local (`o` bound to `new X()`) resolves to a plain class
                // name with no `.`; take it directly. Piping a no-dot name through
                // `expand_typed_property_compound` returns `Opaque` (it early-returns
                // on an empty remaining path), silently dropping the alias shape.
                BindingTarget::Class(class) if !class.contains('.') => Some(class),
                // A compound (`OrdersInvokerFactory.ordersPage`) hops through the
                // class's typed-property binding to the terminal property class.
                BindingTarget::Class(compound) => {
                    match self.expand_typed_property_compound(&compound, None) {
                        TypedPropertyExpansion::Resolved(terminal, _, _) => Some(terminal),
                        TypedPropertyExpansion::CrossModule { .. }
                        | TypedPropertyExpansion::Opaque => None,
                    }
                }
                BindingTarget::FactoryCall { .. } | BindingTarget::Ambiguous => None,
            },
        }
    }

    /// Join the STRICT object-literal factory-return shapes against this module's
    /// exports (mirrors `collect_exported_factory_returns`): honors
    /// `export { createUi as createOrdersUi }`, skips type-only and default exports.
    /// Class names stay local to this module; resolution is deferred to the
    /// analyze-layer join. See issue #1858.
    pub(super) fn collect_exported_factory_return_object_shapes(
        &self,
    ) -> Vec<fallow_types::extract::FactoryReturnObjectShapeExport> {
        if self.strict_factory_return_object_shapes.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for export in &self.exports {
            if export.is_type_only {
                continue;
            }
            let local_name = match (export.local_name.as_deref(), &export.name) {
                (Some(local), _) => local,
                (None, ExportName::Named(name)) => name.as_str(),
                (None, ExportName::Default) => continue,
            };
            if let Some(properties) = self.strict_factory_return_object_shapes.get(local_name) {
                out.push(fallow_types::extract::FactoryReturnObjectShapeExport {
                    export_name: export.name.to_string(),
                    properties: properties.clone().into_boxed_slice(),
                });
            }
        }
        out
    }
}
