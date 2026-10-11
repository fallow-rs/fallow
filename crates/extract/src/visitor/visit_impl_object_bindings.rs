#[allow(
    clippy::wildcard_imports,
    reason = "object binding helpers use AST node shapes"
)]
use oxc_ast::ast::*;
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};

use super::super::{
    BindingClassProof, BindingProducer, DirectBindingOwner, DirectBindingTarget,
    DirectBindingValue, DirectObjectBindingBranch, DirectObjectBindingBranchScope,
    LoopUpdateAccess, LoopUpdateObservations, MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES,
    PendingBindingValue, PendingSubclassMemberAccess, SubclassAccessProof,
};

use super::super::{
    BindingTarget, ExportName, ExportedObjectInstancePropertyFact, ModuleInfoExtractor,
    ObjectBindingCandidate, SemanticFact,
};

/// Per-module breadth cap on recorded object-binding targets (issue #1843
/// follow-up): the companion to `MAX_TAINTED_BINDINGS_PER_MODULE` for the
/// `const obj = { key: ident }` and `const obj = { key: new Class() }` channels.
/// Identifier candidates are resolved by a fixpoint pass with an O(n^2) worst
/// case. Direct targets are preseeded into scope maps, so both channels need a
/// bounded working set on dense machine-generated bundles. Past the cap no new
/// target is recorded, matching the false-negative-preferring direction of the
/// taint caps. Deliberately a constant, not a config knob: real hand-written
/// modules stay far below it.
pub(super) const MAX_OBJECT_BINDING_TARGETS: usize = 4096;
const MAX_EXPORTED_OBJECT_INSTANCE_PROPERTY_FACTS: usize = 8192;

#[derive(Clone, Copy)]
enum DirectObjectBindingScope {
    Current,
    BindingOwner,
}

fn has_loop_update_overwrite(
    overwrites: &FxHashMap<DirectBindingOwner, FxHashSet<String>>,
    owner: DirectBindingOwner,
    path: &str,
) -> bool {
    let Some(paths) = overwrites.get(&owner) else {
        return false;
    };
    let mut prefix = path;
    loop {
        if paths.contains(prefix) {
            return true;
        }
        let Some((parent, _)) = prefix.rsplit_once('.') else {
            return false;
        };
        prefix = parent;
    }
}

impl ModuleInfoExtractor {
    pub(super) fn record_binding_declaration(&mut self, binding: &str, span: Span) {
        if !self.capture_subclass_provenance && !self.capture_factory_properties {
            return;
        }
        let owner = self.active_constructor_function;
        let entry = self.direct_binding_entry_mut(binding);
        entry.declaration_span = Some(span);
        entry.constructor_owner = owner;
    }

    pub(super) fn direct_binding_entry(&self, root: &str) -> Option<&DirectBindingTarget> {
        for (targets, roots) in self
            .scoped_direct_object_binding_targets
            .iter()
            .zip(&self.scoped_direct_object_binding_roots)
            .rev()
        {
            if let Some(entry) = targets.get(root) {
                return Some(entry);
            }
            if roots.contains(root) {
                return None;
            }
        }
        self.module_direct_object_binding_targets.get(root)
    }

    pub(super) fn direct_binding_entry_mut(&mut self, root: &str) -> &mut DirectBindingTarget {
        if let Some(index) =
            self.direct_object_binding_scope_index(DirectObjectBindingScope::BindingOwner, root)
        {
            self.scoped_direct_object_binding_targets[index]
                .entry(root.to_string())
                .or_default()
        } else {
            self.module_direct_object_binding_targets
                .entry(root.to_string())
                .or_default()
        }
    }

    pub(super) fn direct_binding_value(&self, path: &str) -> Option<(usize, &DirectBindingTarget)> {
        let mut parts = path.split('.');
        let root = self.direct_binding_entry(parts.next()?)?;
        let mut value = root.value?;
        for property in parts {
            value = *self.projection_value_view(value).properties.get(property)?;
        }
        Some((value, root))
    }

    pub(super) fn ensure_direct_binding_value(&mut self, path: &str) -> Option<usize> {
        let mut parts = path.split('.');
        let mut value = self.direct_binding_entry(parts.next()?)?.value?;
        for property in parts {
            let view = self.projection_view_id(value);
            if let Some(child) = self.direct_binding_values[view].properties.get(property) {
                value = *child;
                continue;
            }
            let parent = &self.direct_binding_values[view];
            let constructor_owner = parent.constructor_owner;
            if parent.unknown_properties {
                return None;
            }
            if let Some(pending) = parent.pending.clone() {
                let (result, path) = match pending {
                    PendingBindingValue::Call { .. } => (view, property.to_string()),
                    PendingBindingValue::Projection { result, path } => {
                        (result, format!("{path}.{property}"))
                    }
                };
                let child = self.direct_binding_values.len();
                self.direct_binding_values.push(DirectBindingValue {
                    constructor_owner,
                    pending: Some(PendingBindingValue::Projection {
                        result,
                        path: path.clone(),
                    }),
                    ..DirectBindingValue::default()
                });
                self.direct_binding_values[result]
                    .initial_projections
                    .insert(path, child);
                self.direct_binding_values[view]
                    .properties
                    .insert(property.to_string(), child);
                value = child;
                continue;
            }
            let proof = parent.proof.as_ref()?;
            let super::super::TypedPropertyExpansion::Resolved(target, Some(origin), requirements) =
                self.expand_typed_property_compound(
                    &format!("{}.{property}", proof.target_name),
                    Some(proof.target_reference_span),
                )
            else {
                return None;
            };
            let requirements = self.import_requirements_before(&requirements, proof.requirements);
            let condition_work = proof.builtin_containers.len();
            if !self.charge_binding_conditions(condition_work) {
                return None;
            }
            let builtin_containers = std::sync::Arc::clone(
                &self
                    .projection_value_view(value)
                    .proof
                    .as_ref()?
                    .builtin_containers,
            );
            let child = self.direct_binding_values.len();
            self.direct_binding_values
                .push(DirectBindingValue::class(BindingClassProof {
                    builtin_containers,
                    target_name: target,
                    target_reference_span: origin,
                    requirements,
                }));
            self.direct_binding_values[child].constructor_owner = constructor_owner;
            self.direct_binding_values[view]
                .properties
                .insert(property.to_string(), child);
            value = child;
        }
        Some(value)
    }

    fn set_direct_binding_value(&mut self, path: &str, mut value: DirectBindingValue) -> bool {
        if !self.capture_subclass_provenance
            && (self.capture_factory_properties || self.has_factory_value_root(path))
        {
            let Some(id) = self.allocate_projection_value(value) else {
                let unknown = self.reserved_projection_unknown();
                self.attach_direct_binding_value(path, unknown);
                return false;
            };
            self.attach_direct_binding_value(path, id);
            return true;
        }
        value.constructor_owner = self.active_constructor_function;
        let id = self.direct_binding_values.len();
        self.direct_binding_values.push(value);
        self.attach_direct_binding_value(path, id);
        true
    }

    pub(super) fn attach_direct_binding_value(&mut self, path: &str, value: usize) {
        let qualified = self.qualify_binding_value_path(path);
        let path = qualified.as_str();
        self.direct_object_binding_generation =
            self.direct_object_binding_generation.saturating_add(1);
        let root = path.split_once('.').map_or(path, |(root, _)| root);
        if let Some(index) =
            self.direct_object_binding_scope_index(DirectObjectBindingScope::BindingOwner, root)
        {
            self.scoped_direct_object_binding_generations[index]
                .insert(path.to_string(), self.direct_object_binding_generation);
        } else {
            self.module_direct_object_binding_generations
                .insert(path.to_string(), self.direct_object_binding_generation);
        }
        if let Some((parent, property)) = path.rsplit_once('.') {
            if let Some(parent_id) = self.ensure_direct_binding_value(parent) {
                let Some(parent_id) =
                    self.prepare_constructor_property_write(parent_id, property, value)
                else {
                    return;
                };
                if self.is_projection_barrier(parent_id) {
                    return;
                }
                self.direct_binding_values[parent_id]
                    .properties
                    .insert(property.to_string(), value);
            }
        } else {
            self.prepare_constructor_root_write(path, value);
            let entry = self.direct_binding_entry_mut(path);
            entry.value = Some(value);
            entry.requirements = super::super::BindingRequirements::Empty;
        }
    }

    fn qualify_binding_value_path(&self, path: &str) -> String {
        if self.independent_this_depth == 0 && path == "this" {
            self.this_member_key("").trim_end_matches('.').to_string()
        } else if self.independent_this_depth == 0 {
            self.qualify_this_scope(path)
        } else {
            path.to_string()
        }
    }

    pub(super) fn invalidate_direct_binding_proof(&mut self, path: &str, unknown_key: bool) {
        if !self.capture_subclass_provenance
            && self.direct_binding_value(path).is_none()
            && !self.has_factory_value_root(path)
        {
            return;
        }
        let qualified = self.qualify_binding_value_path(path);
        let path = qualified.as_str();
        if unknown_key {
            if let Some(id) = self.ensure_direct_binding_value(path) {
                if self.is_projection_barrier(self.projection_view_id(id)) {
                    return;
                }
                self.invalidate_projection_slot(id, None);
            }
            self.direct_object_binding_generation =
                self.direct_object_binding_generation.saturating_add(1);
        } else {
            self.set_direct_binding_value(path, DirectBindingValue::unknown());
        }
    }

    pub(super) fn record_binding_class_proof(
        &mut self,
        path: &str,
        target: &str,
        origin: Option<Span>,
    ) {
        if !self.capture_subclass_provenance && !self.capture_factory_properties {
            return;
        }
        let qualified = self.qualify_binding_value_path(path);
        let path = qualified.as_str();
        let value = origin.map_or_else(DirectBindingValue::unknown, |span| {
            DirectBindingValue::class(BindingClassProof {
                builtin_containers: std::sync::Arc::default(),
                target_name: target.to_string(),
                target_reference_span: span,
                requirements: super::super::BindingRequirements::Empty,
            })
        });
        if let Some((parent, _)) = path.rsplit_once('.')
            && self.direct_binding_value(parent).is_none()
        {
            self.record_binding_container(parent);
        }
        self.set_direct_binding_value(path, value);
    }

    fn record_binding_container(&mut self, path: &str) {
        let qualified = self.qualify_binding_value_path(path);
        let path = qualified.as_str();
        if let Some((parent, _)) = path.rsplit_once('.')
            && self.direct_binding_value(parent).is_none()
        {
            self.record_binding_container(parent);
        }
        self.set_direct_binding_value(path, DirectBindingValue::default());
    }

    pub(super) fn record_new_binding_proof(&mut self, path: &str, expression: &NewExpression<'_>) {
        if let Expression::Identifier(reference) = &expression.callee {
            self.record_binding_class_proof(path, reference.name.as_str(), Some(reference.span));
            self.retain_new_constructor(path, reference);
        } else {
            self.invalidate_direct_binding_proof(path, false);
        }
    }

    pub(super) fn copy_direct_binding_proof(
        &mut self,
        source: &str,
        target: &str,
        reference: Option<Span>,
    ) {
        if !self.capture_subclass_provenance && self.direct_binding_value(source).is_none() {
            return;
        }
        let qualified = self.qualify_this_scope(source);
        let source = qualified.as_str();
        if source.starts_with("this@") && self.independent_this_depth > 0 {
            self.invalidate_direct_binding_proof(target, false);
            return;
        }
        let Some(id) = self.ensure_direct_binding_value(source) else {
            self.invalidate_direct_binding_proof(target, false);
            return;
        };
        let root = source.split_once('.').map_or(source, |(root, _)| root);
        let Some(entry) = self.direct_binding_entry(root) else {
            return;
        };
        let mut requirements = entry.requirements;
        if !source.starts_with("this@") {
            let (Some(reference), Some(declaration)) = (reference, entry.declaration_span) else {
                self.invalidate_direct_binding_proof(target, false);
                return;
            };
            requirements = self.append_requirement(requirements, (reference, declaration));
        }
        self.attach_direct_binding_value(target, id);
        if target.contains('.') {
            // Property aliases retain source admission with the value being saved.
            let id = self.projection_view_id(id);
            if self.is_projection_barrier(id) {
                return;
            }
            self.direct_binding_values[id].requirements =
                self.concat_requirements(self.direct_binding_values[id].requirements, requirements);
        } else {
            self.direct_binding_entry_mut(target).requirements = requirements;
        }
    }

    pub(super) fn binding_iteration_target(&self, path: &str, map_only: bool) -> Option<String> {
        let (id, _) = self.direct_binding_value(path)?;
        if map_only && !self.projection_value_view(id).iteration_is_map {
            return None;
        }
        self.projection_value_view(id)
            .iteration_proof
            .as_ref()
            .map(|proof| proof.target_name.clone())
    }

    pub(super) fn record_binding_iteration_proof(
        &mut self,
        binding: &str,
        target: &str,
        origin: Option<Span>,
        is_map: bool,
    ) {
        if !self.capture_subclass_provenance {
            return;
        }
        self.set_direct_binding_value(
            binding,
            DirectBindingValue {
                iteration_proof: origin.map(|span| BindingClassProof {
                    builtin_containers: std::sync::Arc::default(),
                    target_name: target.to_string(),
                    target_reference_span: span,
                    requirements: super::super::BindingRequirements::Empty,
                }),
                iteration_is_map: is_map,
                ..DirectBindingValue::default()
            },
        );
    }

    pub(super) fn iteration_binding_class_proof(
        &self,
        source: &str,
        reference: Option<Span>,
    ) -> Option<BindingClassProof> {
        let (id, _) = self.direct_binding_value(source)?;
        let (_, entry) = self.direct_binding_value(source)?;
        let value = self.projection_value_view(id);
        let mut proof = value.iteration_proof.clone()?;
        let condition = if source.starts_with("this@") {
            None
        } else {
            Some((reference?, entry.declaration_span?))
        };
        proof.requirements = self.combine_requirements(
            &[proof.requirements, value.requirements, entry.requirements],
            condition.as_slice(),
        );
        Some(proof)
    }

    pub(super) fn bind_iteration_class_proof(
        &mut self,
        source: &str,
        target: &str,
        reference: Option<Span>,
    ) {
        if let Some(proof) = self.iteration_binding_class_proof(source, reference) {
            self.set_direct_binding_value(target, DirectBindingValue::class(proof));
        }
    }

    pub(super) fn record_captured_binding_class_proof(
        &mut self,
        target: &str,
        proof: BindingClassProof,
    ) {
        self.set_direct_binding_value(target, DirectBindingValue::class(proof));
    }

    pub(super) fn add_direct_binding_requirement(
        &mut self,
        path: &str,
        reference: Span,
        expected: Span,
    ) {
        if let Some((id, _)) = self.direct_binding_value(path)
            && let id = self.projection_view_id(id)
            && !self.is_projection_barrier(id)
            && let Some(proof) = &self.direct_binding_values[id].proof
        {
            let requirements = self.append_requirement(proof.requirements, (reference, expected));
            if let Some(proof) = &mut self.direct_binding_values[id].proof {
                proof.requirements = requirements;
            }
        }
    }

    pub(in crate::visitor) fn direct_binding_class_proof(
        &mut self,
        path: &str,
        reference: Option<Span>,
    ) -> Option<BindingClassProof> {
        let id = self.ensure_direct_binding_value(path)?;
        let root = path.split_once('.').map_or(path, |(root, _)| root);
        let entry = self.direct_binding_entry(root)?;
        let value = self.projection_value_view(id);
        let mut proof = value.proof.clone()?;
        let condition = if path.starts_with("this@") {
            None
        } else {
            Some((reference?, entry.declaration_span?))
        };
        proof.requirements = self.combine_requirements(
            &[proof.requirements, value.requirements, entry.requirements],
            condition.as_slice(),
        );
        Some(proof)
    }

    pub(super) fn register_binding_producer(&mut self, path: &str, producer: BindingProducer) {
        if !self.capture_subclass_provenance && !self.capture_factory_properties {
            return;
        }
        let existing = self
            .direct_binding_entry(path)
            .and_then(|entry| entry.initial_producer_value);
        if let Some(id) = existing {
            self.direct_binding_values[id].producer = Some(producer);
            return;
        }
        if let Some((parent, _)) = path.rsplit_once('.')
            && self.direct_binding_value(parent).is_none()
        {
            self.record_binding_container(parent);
        }
        let id = self.direct_binding_values.len();
        if self.set_direct_binding_value(
            path,
            DirectBindingValue {
                producer: Some(producer),
                ..DirectBindingValue::default()
            },
        ) {
            self.direct_binding_entry_mut(path).initial_producer_value = Some(id);
        }
    }

    pub(super) fn capture_binding_producer_call(
        &mut self,
        path: &str,
        reference: Span,
        awaited: bool,
    ) -> Option<usize> {
        let (producer, _) = self.direct_binding_value(path)?;
        let producer = self.projection_view_id(producer);
        self.direct_binding_values[producer].producer.as_ref()?;
        let id = self.direct_binding_values.len();
        self.direct_binding_values.push(DirectBindingValue {
            constructor_owner: self.active_constructor_function,
            pending: Some(PendingBindingValue::Call {
                producer,
                reference,
                awaited,
                result: None,
            }),
            ..DirectBindingValue::default()
        });
        Some(id)
    }

    pub(super) fn set_static_binding_call_origin(
        &mut self,
        id: usize,
        name: &str,
        reference: Span,
    ) {
        let Some(PendingBindingValue::Call { producer, .. }) =
            &self.direct_binding_values[id].pending
        else {
            return;
        };
        if !self.direct_binding_values[*producer]
            .producer
            .as_ref()
            .is_some_and(|producer| producer.is_static_method)
        {
            return;
        }
        // The reference belongs to this call, even when earlier calls share a producer.
        if let Some(PendingBindingValue::Call { result, .. }) =
            &mut self.direct_binding_values[id].pending
        {
            *result = Some(BindingClassProof {
                builtin_containers: std::sync::Arc::default(),
                target_name: name.to_string(),
                target_reference_span: reference,
                requirements: super::super::BindingRequirements::Empty,
            });
        }
    }

    fn loop_update_binding_owner(&self, root: &str) -> DirectBindingOwner {
        let mut owner = self
            .direct_object_binding_scope_index(DirectObjectBindingScope::BindingOwner, root)
            .map_or(DirectBindingOwner::Module, DirectBindingOwner::Scope);
        while let DirectBindingOwner::Scope(index) = owner {
            if self.scoped_direct_object_binding_paths_by_root[index]
                .get(root)
                .is_some_and(FxHashSet::is_empty)
            {
                break;
            }
            if self
                .direct_object_binding_branch_scopes
                .iter()
                .any(|branch| branch.index == index)
            {
                owner = self.scoped_direct_object_binding_roots[..index]
                    .iter()
                    .rposition(|roots| roots.contains(root))
                    .map_or(DirectBindingOwner::Module, DirectBindingOwner::Scope);
            } else if let Some(older) = self.scoped_direct_object_binding_targets[index]
                .get(root)
                .and_then(|entry| entry.possible_assignment_owner)
            {
                if matches!(older, DirectBindingOwner::Scope(older_index) if older_index >= index) {
                    break;
                }
                owner = older;
            } else {
                break;
            }
        }
        owner
    }

    pub(super) fn start_loop_update_observations(&mut self) {
        self.loop_update_observations.push(LoopUpdateObservations {
            function_scope: self.direct_object_binding_function_scopes.last().copied(),
            scope_index: self.scoped_direct_object_binding_roots.len() - 1,
            ..LoopUpdateObservations::default()
        });
    }

    pub(super) fn finish_loop_update_observations(&mut self) -> LoopUpdateObservations {
        self.loop_update_observations.pop().unwrap_or_default()
    }

    pub(super) fn replay_loop_update_observations(&mut self, observations: LoopUpdateObservations) {
        if observations.exhausted
            || observations.function_scope
                != self.direct_object_binding_function_scopes.last().copied()
        {
            return;
        }
        let enclosing_observers = std::mem::take(&mut self.loop_update_observations);
        for access in observations.accesses {
            let root = access
                .object
                .split_once('.')
                .map_or(access.object.as_str(), |(root, _)| root);
            if matches!(access.owner, DirectBindingOwner::Scope(index) if index >= self.scoped_direct_object_binding_roots.len())
            {
                continue;
            }
            if self.loop_update_binding_owner(root) == access.owner {
                self.record_walk_order_member_access(&access.object, &access.member);
            }
        }
        self.loop_update_observations = enclosing_observers;
    }

    pub(in super::super) fn observe_loop_update_access(&mut self, object: &str, member: &str) {
        let Some(observations) = self.loop_update_observations.last() else {
            return;
        };
        if observations.exhausted
            || observations.function_scope
                != self.direct_object_binding_function_scopes.last().copied()
        {
            return;
        }
        let root = object.split_once('.').map_or(object, |(root, _)| root);
        let owner = self.loop_update_binding_owner(root);
        if matches!(owner, DirectBindingOwner::Scope(index) if index >= observations.scope_index) {
            return;
        }
        if self
            .direct_object_binding_branch_scopes
            .iter()
            .filter(|branch| branch.index >= observations.scope_index)
            .any(|branch| has_loop_update_overwrite(&branch.update_overwrites, owner, object))
        {
            return;
        }
        let Some(observations) = self.loop_update_observations.last_mut() else {
            return;
        };
        if observations.accesses.len() >= MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES {
            observations.exhausted = true;
            return;
        }
        observations.accesses.push(LoopUpdateAccess {
            object: object.to_string(),
            member: member.to_string(),
            owner,
        });
    }

    fn retain_loop_update_overwrite(&mut self, path: &str, owner: DirectBindingOwner) {
        let Some(observations) = self.loop_update_observations.last_mut() else {
            return;
        };
        if observations.exhausted
            || observations.function_scope
                != self.direct_object_binding_function_scopes.last().copied()
            || matches!(owner, DirectBindingOwner::Scope(index) if index >= observations.scope_index)
        {
            return;
        }
        let Some(branch) = self
            .direct_object_binding_branch_scopes
            .last_mut()
            .filter(|branch| branch.index >= observations.scope_index)
        else {
            return;
        };
        if has_loop_update_overwrite(&branch.update_overwrites, owner, path) {
            return;
        }
        if observations.retained_paths >= MAX_OBJECT_BINDING_TARGETS {
            observations.exhausted = true;
            return;
        }
        observations.retained_paths += 1;
        branch
            .update_overwrites
            .entry(owner)
            .or_default()
            .insert(path.to_string());
    }

    fn join_loop_update_overwrites(
        &mut self,
        consequent: &DirectObjectBindingBranch,
        alternate: &DirectObjectBindingBranch,
    ) {
        for (branch, other) in [(consequent, alternate), (alternate, consequent)] {
            for (owner, paths) in &branch.update_overwrites {
                for path in paths {
                    if has_loop_update_overwrite(&other.update_overwrites, *owner, path) {
                        self.retain_loop_update_overwrite(path, *owner);
                    }
                }
            }
        }
    }

    pub(super) fn push_conditional_binding_scope(
        &mut self,
        narrowings: &[(String, String, Span, Span)],
    ) -> Vec<(String, usize)> {
        let index = self.scoped_direct_object_binding_roots.len();
        self.push_direct_object_binding_scope(FxHashSet::default());
        self.direct_object_binding_branch_scopes
            .push(DirectObjectBindingBranchScope {
                index,
                ..DirectObjectBindingBranchScope::default()
            });
        let mut values = Vec::new();
        for (local, target, reference, origin) in narrowings {
            let Some(declaration) = self
                .direct_binding_entry(local)
                .and_then(|entry| entry.declaration_span)
            else {
                continue;
            };
            self.fork_outer_direct_object_binding_root(local);
            self.record_binding_class_proof(local, target, Some(*origin));
            self.add_direct_binding_requirement(local, *reference, declaration);
            if let Some((value, _)) = self.direct_binding_value(local) {
                values.push((local.clone(), value));
            }
        }
        values
    }

    pub(super) fn pop_conditional_binding_scope(
        &mut self,
        values: Vec<(String, usize)>,
    ) -> DirectObjectBindingBranch {
        let Some(mut branch) = self.direct_object_binding_branch_scopes.pop() else {
            return DirectObjectBindingBranch::default();
        };
        for (local, value) in values {
            if self.direct_binding_value(&local).map(|(id, _)| id) != Some(value) {
                branch.proof_writes.insert(local);
            }
        }
        let result = DirectObjectBindingBranch {
            proof_writes: branch.proof_writes,
            constructor_writes: branch.constructor_writes,
            projection_writes: branch.projection_writes,
            update_overwrites: branch.update_overwrites,
            targets: std::mem::take(&mut self.scoped_direct_object_binding_targets[branch.index]),
            paths_by_root: std::mem::take(
                &mut self.scoped_direct_object_binding_paths_by_root[branch.index],
            ),
        };
        self.pop_direct_object_binding_scope();
        result
    }

    pub(super) fn pop_optional_binding_scope(&mut self, values: Vec<(String, usize)>) {
        let branch = self.pop_conditional_binding_scope(values);
        self.join_conditional_binding_scopes(&branch, &DirectObjectBindingBranch::default());
    }

    pub(super) fn join_conditional_binding_scopes(
        &mut self,
        consequent: &DirectObjectBindingBranch,
        alternate: &DirectObjectBindingBranch,
    ) {
        let constructors = self.join_constructor_outcomes(consequent, alternate);
        for path in consequent
            .proof_writes
            .iter()
            .chain(&alternate.proof_writes)
        {
            self.invalidate_direct_binding_proof(path, false);
            let root = path.split_once('.').map_or(path.as_str(), |(root, _)| root);
            let owner = self
                .direct_object_binding_scope_index(DirectObjectBindingScope::BindingOwner, root);
            if let Some(parent) = self.direct_object_binding_branch_scopes.last_mut()
                && owner == Some(parent.index)
            {
                parent.proof_writes.insert(path.clone());
            }
        }
        self.join_projection_writes(consequent, alternate);
        self.install_constructor_outcomes(constructors);
        self.join_loop_update_overwrites(consequent, alternate);
        let roots: FxHashSet<_> = consequent
            .paths_by_root
            .keys()
            .chain(alternate.paths_by_root.keys())
            .collect();
        for root in roots {
            self.fork_outer_direct_object_binding_root(root);
            let owner = self
                .direct_object_binding_scope_index(DirectObjectBindingScope::BindingOwner, root);
            let (incoming_targets, incoming_paths) = owner.map_or_else(
                || {
                    (
                        &self.module_direct_object_binding_targets,
                        &self.module_direct_object_binding_paths_by_root,
                    )
                },
                |index| {
                    (
                        &self.scoped_direct_object_binding_targets[index],
                        &self.scoped_direct_object_binding_paths_by_root[index],
                    )
                },
            );
            let mut joined: FxHashMap<String, FxHashSet<String>> = FxHashMap::default();
            let mut refused = false;
            for branch in [consequent, alternate] {
                let (targets, paths) = branch.paths_by_root.get(root).map_or_else(
                    || (incoming_targets, incoming_paths.get(root)),
                    |paths| (&branch.targets, Some(paths)),
                );
                let Some(paths) = paths else {
                    continue;
                };
                if paths.is_empty() {
                    refused = true;
                    break;
                }
                for path in paths {
                    joined.entry(path.clone()).or_default().extend(
                        targets
                            .get(path)
                            .into_iter()
                            .flat_map(|entry| entry.names.iter().cloned()),
                    );
                }
            }
            let needed = joined
                .values()
                .map(|names| names.len().max(1))
                .sum::<usize>();
            if refused
                || needed
                    > MAX_OBJECT_BINDING_TARGETS.saturating_sub(self.object_binding_target_count())
            {
                self.refuse_conditional_binding_root(root, owner);
                continue;
            }
            self.replace_conditional_binding_root(root, owner, joined);
        }
    }

    fn refuse_conditional_binding_root(&mut self, root: &str, owner: Option<usize>) {
        self.replace_conditional_binding_root(root, owner, FxHashMap::default());
    }

    fn replace_conditional_binding_root(
        &mut self,
        root: &str,
        owner: Option<usize>,
        joined: FxHashMap<String, FxHashSet<String>>,
    ) {
        self.direct_object_binding_target_count += joined
            .values()
            .map(|names| names.len().max(1))
            .sum::<usize>();
        self.direct_object_binding_generation =
            self.direct_object_binding_generation.saturating_add(1);
        let generation = self.direct_object_binding_generation;
        let (targets, generations, paths_by_root) = owner.map_or_else(
            || {
                (
                    &mut self.module_direct_object_binding_targets,
                    &mut self.module_direct_object_binding_generations,
                    &mut self.module_direct_object_binding_paths_by_root,
                )
            },
            |index| {
                (
                    &mut self.scoped_direct_object_binding_targets[index],
                    &mut self.scoped_direct_object_binding_generations[index],
                    &mut self.scoped_direct_object_binding_paths_by_root[index],
                )
            },
        );
        let paths = paths_by_root.entry(root.to_string()).or_default();
        for path in paths.iter() {
            if let Some(entry) = targets.get_mut(path) {
                entry.names.clear();
            }
            generations.insert(path.clone(), generation);
        }
        paths.clear();
        for (path, names) in joined {
            targets.entry(path.clone()).or_default().names = names;
            generations.insert(path.clone(), generation);
            paths.insert(path);
        }
    }

    pub(super) fn attach_captured_binding_value(&mut self, path: &str, id: usize) {
        self.attach_direct_binding_value(path, id);
    }

    pub(super) fn capture_subclass_binding_access(
        &mut self,
        path: &str,
        reference: Option<Span>,
        member: &str,
    ) -> bool {
        let strict = self.direct_binding_class_proof(path, reference);
        let alternatives =
            self.capture_constructor_alternatives(path, reference, member, strict.as_ref());
        if let Some(proof) = strict {
            self.pending_subclass_member_accesses
                .push(PendingSubclassMemberAccess {
                    proof: SubclassAccessProof::Known(proof),
                    member: member.to_string(),
                });
            return true;
        }
        let Some(id) = self.ensure_direct_binding_value(path) else {
            return alternatives;
        };
        let id = self.projection_view_id(id);
        if self.direct_binding_values[id].pending.is_none() {
            return alternatives;
        }
        let root = path.split_once('.').map_or(path, |(root, _)| root);
        let Some(entry) = self.direct_binding_entry(root) else {
            return false;
        };
        let mut requirements = entry.requirements;
        if !path.starts_with("this@") {
            let (Some(reference), Some(declaration)) = (reference, entry.declaration_span) else {
                return false;
            };
            requirements = self.append_requirement(requirements, (reference, declaration));
        }
        self.pending_subclass_member_accesses
            .push(PendingSubclassMemberAccess {
                proof: SubclassAccessProof::Value { id, requirements },
                member: member.to_string(),
            });
        true
    }

    pub(in crate::visitor) fn complete_binding_producer_calls(&mut self) {
        for id in 0..self.direct_binding_values.len() {
            let Some(PendingBindingValue::Call {
                producer,
                reference,
                awaited,
                result,
            }) = self.direct_binding_values[id].pending.clone()
            else {
                continue;
            };
            let Some(producer) = self.direct_binding_values[producer].producer.clone() else {
                continue;
            };
            if producer.is_generator || producer.is_async && !awaited {
                continue;
            }
            let requirement = (reference, producer.declaration_span);
            if let Some(mut proof) = result.or(producer.result)
                && (producer.is_static_method
                    || self.factory_return_functions.get(&producer.name)
                        == Some(&proof.target_name))
            {
                proof.requirements = self.append_then_concat_requirements(
                    proof.requirements,
                    &[requirement],
                    self.direct_binding_values[id].requirements,
                );
                self.direct_binding_values[id].proof = Some(proof);
            }
            if let Some(shape) = self.factory_return_object_shapes.get(&producer.name) {
                for (path, mut proof) in producer.shape {
                    if !shape.iter().any(|property| {
                        property.property_path == path
                            && property.class_local_name == proof.target_name
                    }) {
                        continue;
                    }
                    let Some(child) = self.direct_binding_values[id]
                        .initial_projections
                        .get(&path)
                        .copied()
                    else {
                        continue;
                    };
                    if matches!(&self.direct_binding_values[child].pending, Some(PendingBindingValue::Projection { result, path: original }) if *result == id && *original == path)
                    {
                        proof.requirements = self.append_then_concat_requirements(
                            proof.requirements,
                            &[requirement],
                            self.direct_binding_values[child].requirements,
                        );
                        self.direct_binding_values[child].proof = Some(proof);
                        self.direct_binding_values[child].pending = None;
                    }
                }
            }
            self.direct_binding_values[id].pending = None;
        }
    }

    pub(in crate::visitor) fn record_exported_direct_object_binding_facts(&mut self) {
        let exported_roots = self
            .exports
            .iter()
            .filter(|export| !export.is_type_only)
            .filter_map(|export| {
                let local_name = match (export.local_name.as_deref(), &export.name) {
                    (Some(local_name), _) => local_name,
                    (None, ExportName::Named(name)) => name.as_str(),
                    (None, ExportName::Default) => return None,
                };
                Some((local_name.to_string(), export.name.to_string()))
            })
            .collect::<Vec<_>>();
        let mut facts = Vec::new();
        let mut seen = FxHashSet::default();
        let mut overflowed = false;
        'exports: for (root_name, export_name) in exported_roots {
            let Some(paths) = self
                .module_direct_object_binding_paths_by_root
                .get(root_name.as_str())
            else {
                continue;
            };
            let prefix = format!("{root_name}.");
            let mut paths = paths.iter().collect::<Vec<_>>();
            paths.sort_unstable();
            for path in paths {
                let Some(property_path) = path.strip_prefix(&prefix) else {
                    continue;
                };
                let Some(class_names) = self.module_direct_object_binding_targets.get(path) else {
                    continue;
                };
                let mut class_names = class_names.iter().collect::<Vec<_>>();
                class_names.sort_unstable();
                for class_local_name in class_names {
                    let fact = (
                        export_name.clone(),
                        property_path.to_string(),
                        class_local_name.clone(),
                    );
                    if !seen.insert(fact.clone()) {
                        continue;
                    }
                    if facts.len() >= MAX_EXPORTED_OBJECT_INSTANCE_PROPERTY_FACTS {
                        overflowed = true;
                        break 'exports;
                    }
                    facts.push(fact);
                }
            }
        }
        if overflowed {
            let class_names = self
                .module_direct_object_binding_targets
                .values()
                .flatten()
                .cloned()
                .collect::<FxHashSet<_>>();
            for class_name in class_names {
                if self
                    .direct_object_binding_whole_object_uses
                    .insert(class_name.clone())
                {
                    self.whole_object_uses.push(class_name);
                }
            }
        }
        self.semantic_facts.extend(facts.into_iter().map(
            |(export_name, property_path, class_local_name)| {
                SemanticFact::ExportedObjectInstanceProperty(ExportedObjectInstancePropertyFact {
                    export_name,
                    property_path,
                    class_local_name,
                })
            },
        ));
    }

    pub(super) fn extract_angular_inject_target(
        &self,
        call: &CallExpression<'_>,
    ) -> Option<String> {
        super::super::helpers::extract_angular_inject_target(
            call,
            &|local_name, source, imported_name| {
                self.is_named_import_from(local_name, source, imported_name)
            },
        )
    }

    pub(super) fn copy_nested_binding_targets(
        &mut self,
        source_binding: &str,
        target_binding: &str,
    ) -> bool {
        // Nothing to copy from an empty map: skip the two `format!` allocations
        // and the no-op scan/collect below.
        if self.binding_target_names.is_empty() {
            return false;
        }
        let source_prefix = format!("{source_binding}.");
        let target_prefix = format!("{target_binding}.");
        // Prefix-index fast-path (issue #1843 follow-up): during the object-binding
        // fixed-point, enumerate the keys under `source_binding.` in O(matches) via
        // the index instead of scanning all of `binding_target_names`, which is
        // what made a real minified bundle full of nested object maps take tens of
        // seconds. Outside the pass (`None`) the map is small and the full scan is
        // used. Both branches produce the same `(binding, target)` set.
        let copied: Vec<(String, BindingTarget)> =
            if let Some(index) = &self.binding_target_prefix_index {
                index
                    .get(source_binding)
                    .into_iter()
                    .flatten()
                    .filter_map(|key| {
                        self.binding_target_names.get(key).map(|target| {
                            let suffix = &key[source_prefix.len()..];
                            (format!("{target_prefix}{suffix}"), target.clone())
                        })
                    })
                    .collect()
            } else {
                self.binding_target_names
                    .iter()
                    .filter_map(|(binding, target)| {
                        binding
                            .strip_prefix(&source_prefix)
                            .map(|suffix| (format!("{target_prefix}{suffix}"), target.clone()))
                    })
                    .collect()
            };

        let mut changed = false;
        for (binding, target) in copied {
            changed |= self.insert_binding_target(binding, target);
        }
        changed
    }

    fn insert_binding_target(&mut self, binding: String, target: BindingTarget) -> bool {
        if self.binding_target_names.get(&binding) == Some(&target) {
            return false;
        }
        // Hard size cap on the object-binding fixed-point's growth (issue #1843
        // follow-up). A pathological minified bundle (huge object maps copied
        // across many bindings) makes the fixed-point multiply
        // `binding_target_names` without bound, taking tens of seconds. Once the
        // map reaches the cap, stop recording NEW keys (an over-cap chain degrades
        // to a false negative, matching the FN-preferring doctrine); updates to an
        // already-present key still apply. Only reached via the fixed-point (the
        // index is `Some`), so the walk-time member crediting is unaffected.
        const MAX_BINDING_TARGET_NAMES: usize = 8192;
        if self.binding_target_prefix_index.is_some()
            && self.binding_target_names.len() >= MAX_BINDING_TARGET_NAMES
            && !self.binding_target_names.contains_key(&binding)
        {
            return false;
        }
        // Keep the ancestor-prefix index current for inserts made during the
        // fixed-point (issue #1843 follow-up), so a key added this pass is visible
        // to a later `copy_nested_binding_targets` call under every prefix.
        if let Some(index) = self.binding_target_prefix_index.as_mut() {
            for (dot, _) in binding.match_indices('.') {
                index
                    .entry(binding[..dot].to_string())
                    .or_default()
                    .push(binding.clone());
            }
        }
        self.binding_target_names.insert(binding, target);
        true
    }

    pub(in crate::visitor) fn resolve_object_binding_candidate(
        &mut self,
        candidate: &ObjectBindingCandidate,
    ) -> bool {
        let mut changed = false;
        if self
            .namespace_binding_names
            .iter()
            .any(|name| name == candidate.source_name.as_str())
        {
            changed |= self.insert_binding_target(
                candidate.binding_path.clone(),
                BindingTarget::Class(candidate.source_name.clone()),
            );
        } else if let Some(target_name) = self
            .binding_target_names
            .get(candidate.source_name.as_str())
            .cloned()
        {
            changed |= self.insert_binding_target(candidate.binding_path.clone(), target_name);
        }
        changed | self.copy_nested_binding_targets(&candidate.source_name, &candidate.binding_path)
    }

    pub(super) fn record_object_binding_targets(
        &mut self,
        binding_name: &str,
        obj: &ObjectExpression<'_>,
    ) {
        self.record_object_binding_targets_at_path(binding_name, binding_name, obj);
    }

    pub(super) fn preseed_direct_object_binding_targets(&mut self, statements: &[Statement<'_>]) {
        for statement in statements {
            let declaration = match statement {
                Statement::ExportDeclaration(export) => Some(&export.declaration),
                _ => statement.as_declaration(),
            };
            match declaration {
                Some(Declaration::VariableDeclaration(declaration)) => {
                    self.preseed_direct_object_binding_declaration(declaration);
                }
                Some(Declaration::FunctionDeclaration(function))
                    if self.capture_subclass_provenance || self.capture_factory_properties =>
                {
                    if let Some(id) = &function.id {
                        self.record_binding_declaration(id.name.as_str(), id.span);
                        self.register_binding_producer(
                            id.name.as_str(),
                            BindingProducer {
                                name: id.name.to_string(),
                                declaration_span: id.span,
                                result: None,
                                shape: Vec::new(),
                                is_async: function.r#async,
                                is_generator: function.generator,
                                is_static_method: false,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn preseed_direct_object_binding_declaration(
        &mut self,
        declaration: &VariableDeclaration<'_>,
    ) {
        if !declaration.kind.is_lexical() {
            return;
        }
        for declarator in &declaration.declarations {
            let Some(init) = declarator.init.as_ref() else {
                continue;
            };
            for id in declarator.id.get_binding_identifiers() {
                self.record_binding_declaration(id.name.as_str(), id.span);
            }
            match (&declarator.id, init) {
                (BindingPattern::BindingIdentifier(id), Expression::ObjectExpression(object)) => {
                    self.record_direct_object_binding_targets_at_path(
                        id.name.as_str(),
                        id.name.as_str(),
                        object,
                        DirectObjectBindingScope::Current,
                    );
                }
                (BindingPattern::BindingIdentifier(id), _) => {
                    if let Some(source_path) = direct_object_binding_expression_path(init) {
                        self.copy_direct_object_binding_targets(&source_path, id.name.as_str());
                        self.copy_direct_binding_proof(
                            &source_path,
                            id.name.as_str(),
                            binding_expression_reference(init),
                        );
                    }
                }
                (BindingPattern::ObjectPattern(pattern), _) => {
                    self.record_direct_object_binding_destructure(pattern, init);
                }
                _ => {}
            }
        }
    }

    fn record_direct_object_binding_destructure(
        &mut self,
        pattern: &ObjectPattern<'_>,
        init: &Expression<'_>,
    ) {
        let Some(source_path) = direct_object_binding_expression_path(init) else {
            return;
        };
        self.record_direct_object_binding_object_pattern(pattern, &source_path);
    }

    fn record_direct_object_binding_object_pattern(
        &mut self,
        pattern: &ObjectPattern<'_>,
        source_path: &str,
    ) {
        for property in &pattern.properties {
            let Some(property_name) = property.key.static_name() else {
                continue;
            };
            self.record_direct_object_binding_pattern(
                &property.value,
                &format!("{source_path}.{property_name}"),
            );
        }
    }

    fn record_direct_object_binding_pattern(
        &mut self,
        pattern: &BindingPattern<'_>,
        source_path: &str,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                self.copy_direct_object_binding_targets(source_path, binding.name.as_str());
            }
            BindingPattern::ObjectPattern(pattern) => {
                self.record_direct_object_binding_object_pattern(pattern, source_path);
            }
            BindingPattern::AssignmentPattern(assignment) => {
                self.record_direct_object_binding_pattern(&assignment.left, source_path);
                self.record_direct_object_binding_fallback(&assignment.left, &assignment.right);
            }
            BindingPattern::ArrayPattern(_) => {}
        }
    }

    fn record_direct_object_binding_fallback(
        &mut self,
        pattern: &BindingPattern<'_>,
        fallback: &Expression<'_>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => match fallback {
                Expression::NewExpression(new_expression) => {
                    if let Some(class_name) = self.direct_new_expression_class_name(new_expression)
                    {
                        self.record_direct_object_binding_target(
                            binding.name.as_str(),
                            binding.name.to_string(),
                            class_name,
                            DirectObjectBindingScope::BindingOwner,
                        );
                    }
                }
                Expression::ObjectExpression(object) => {
                    self.record_direct_object_binding_targets_at_path(
                        binding.name.as_str(),
                        binding.name.as_str(),
                        object,
                        DirectObjectBindingScope::BindingOwner,
                    );
                }
                fallback => {
                    if let Some(fallback_path) = direct_object_binding_expression_path(fallback) {
                        self.copy_direct_object_binding_targets(
                            &fallback_path,
                            binding.name.as_str(),
                        );
                    }
                }
            },
            BindingPattern::ObjectPattern(pattern) => match fallback {
                Expression::ObjectExpression(object) => {
                    for property in &pattern.properties {
                        let Some(property_name) = property.key.static_name() else {
                            continue;
                        };
                        let Some(value) = object.properties.iter().rev().find_map(|candidate| {
                            let ObjectPropertyKind::ObjectProperty(candidate) = candidate else {
                                return None;
                            };
                            (candidate.key.static_name().as_deref() == Some(property_name.as_ref()))
                                .then_some(&candidate.value)
                        }) else {
                            continue;
                        };
                        self.record_direct_object_binding_fallback(&property.value, value);
                    }
                }
                fallback => {
                    if let Some(fallback_path) = direct_object_binding_expression_path(fallback) {
                        self.record_direct_object_binding_object_pattern(pattern, &fallback_path);
                    }
                }
            },
            BindingPattern::AssignmentPattern(assignment) => {
                self.record_direct_object_binding_fallback(&assignment.left, fallback);
                self.record_direct_object_binding_fallback(&assignment.left, &assignment.right);
            }
            BindingPattern::ArrayPattern(_) => {}
        }
    }

    /// Only indexed ordinary paths participate. Proof-only metadata can be
    /// empty without meaning that an ordinary typed path was replaced.
    pub(in crate::visitor) fn direct_object_binding_path_targets(
        &self,
        path: &str,
    ) -> Option<&FxHashSet<String>> {
        let root = path.split_once('.').map_or(path, |(root, _)| root);
        let (targets, paths) = self
            .direct_object_binding_scope_index(DirectObjectBindingScope::BindingOwner, root)
            .map_or_else(
                || {
                    (
                        &self.module_direct_object_binding_targets,
                        &self.module_direct_object_binding_paths_by_root,
                    )
                },
                |index| {
                    (
                        &self.scoped_direct_object_binding_targets[index],
                        &self.scoped_direct_object_binding_paths_by_root[index],
                    )
                },
            );
        let indexed = paths.get(root)?;
        if indexed.is_empty() {
            // A refused function fork shadows its outer owner without allocating targets.
            return Some(indexed);
        }
        if !indexed.contains(path) {
            return None;
        }
        targets.get(path).map(|entry| &entry.names)
    }

    fn fork_outer_direct_object_binding_root(&mut self, root: &str) {
        if let Some(&frame) = self.direct_object_binding_function_scopes.last() {
            self.fork_direct_object_binding_root(root, frame, false);
        }
        if let Some(branch) = self.direct_object_binding_branch_scopes.last()
            && self
                .direct_object_binding_function_scopes
                .last()
                .is_none_or(|frame| *frame < branch.index)
        {
            self.fork_direct_object_binding_root(root, branch.index, true);
        }
    }

    fn fork_direct_object_binding_root(&mut self, root: &str, frame: usize, conditional: bool) {
        let owner =
            self.direct_object_binding_scope_index(DirectObjectBindingScope::BindingOwner, root);
        if owner.is_some_and(|index| index >= frame) {
            return;
        }
        let (targets, paths) = owner.map_or_else(
            || {
                (
                    &self.module_direct_object_binding_targets,
                    &self.module_direct_object_binding_paths_by_root,
                )
            },
            |index| {
                (
                    &self.scoped_direct_object_binding_targets[index],
                    &self.scoped_direct_object_binding_paths_by_root[index],
                )
            },
        );
        let Some(paths) = paths.get(root) else {
            if conditional && let Some(entry) = targets.get(root).cloned() {
                self.scoped_direct_object_binding_roots[frame].insert(root.to_string());
                self.scoped_direct_object_binding_targets[frame].insert(root.to_string(), entry);
            }
            return;
        };
        let remaining =
            MAX_OBJECT_BINDING_TARGETS.saturating_sub(self.object_binding_target_count());
        let count = (paths.len() <= remaining)
            .then(|| {
                paths.iter().try_fold(0_usize, |count, path| {
                    let total = count.saturating_add(targets.get(path)?.names.len().max(1));
                    (total <= remaining).then_some(total)
                })
            })
            .flatten();
        let Some(count) = count else {
            // Empty admission is local to this body; the outer candidate is unchanged.
            self.scoped_direct_object_binding_roots[frame].insert(root.to_string());
            self.scoped_direct_object_binding_paths_by_root[frame]
                .insert(root.to_string(), FxHashSet::default());
            return;
        };
        let paths = paths.clone();
        let copied = paths
            .iter()
            .filter_map(|path| targets.get(path).map(|entry| (path.clone(), entry.clone())))
            .collect::<Vec<_>>();
        let mut root_entry = targets.get(root).cloned().unwrap_or_default();
        if !conditional {
            root_entry.possible_assignment_owner =
                Some(owner.map_or(DirectBindingOwner::Module, DirectBindingOwner::Scope));
        }
        self.scoped_direct_object_binding_roots[frame].insert(root.to_string());
        self.direct_object_binding_target_count += count;
        self.direct_object_binding_generation =
            self.direct_object_binding_generation.saturating_add(1);
        let generation = self.direct_object_binding_generation;
        self.scoped_direct_object_binding_paths_by_root[frame].insert(root.to_string(), paths);
        for (path, entry) in copied {
            self.scoped_direct_object_binding_generations[frame].insert(path.clone(), generation);
            self.scoped_direct_object_binding_targets[frame].insert(path, entry);
        }
        self.scoped_direct_object_binding_targets[frame].insert(root.to_string(), root_entry);
    }

    fn record_direct_object_binding_path_barrier(&mut self, path: &str) {
        let root = path.split_once('.').map_or(path, |(root, _)| root);
        let index =
            self.direct_object_binding_scope_index(DirectObjectBindingScope::BindingOwner, root);
        if index.is_some_and(|index| {
            self.scoped_direct_object_binding_paths_by_root[index]
                .get(root)
                .is_some_and(FxHashSet::is_empty)
        }) {
            return;
        }
        let is_new = self.direct_object_binding_path_targets(path).is_none();
        let capped = is_new && self.object_binding_target_count() >= MAX_OBJECT_BINDING_TARGETS;
        self.direct_object_binding_generation =
            self.direct_object_binding_generation.saturating_add(1);
        if is_new && !capped {
            self.direct_object_binding_target_count += 1;
        }
        let generation = self.direct_object_binding_generation;
        let (targets, generations, paths_by_root) = if let Some(index) = index {
            (
                &mut self.scoped_direct_object_binding_targets[index],
                &mut self.scoped_direct_object_binding_generations[index],
                &mut self.scoped_direct_object_binding_paths_by_root[index],
            )
        } else {
            (
                &mut self.module_direct_object_binding_targets,
                &mut self.module_direct_object_binding_generations,
                &mut self.module_direct_object_binding_paths_by_root,
            )
        };
        if capped {
            // Reuse the existing breadth bound without allocating another path.
            // Existing indexed entries become barriers for this owner's root.
            if let Some(paths) = paths_by_root.get(root) {
                for existing in paths {
                    if let Some(entry) = targets.get_mut(existing) {
                        entry.names.clear();
                    }
                    generations.insert(existing.clone(), generation);
                }
            }
            return;
        }
        let nested = format!("{path}.");
        let paths = paths_by_root.entry(root.to_string()).or_default();
        let removed = paths
            .iter()
            .filter(|existing| existing.starts_with(&nested))
            .cloned()
            .collect::<Vec<_>>();
        for existing in removed {
            targets.remove(&existing);
            generations.remove(&existing);
            paths.remove(&existing);
        }
        // Retain declaration and value metadata, including saved overlapping IDs.
        targets.entry(path.to_string()).or_default().names.clear();
        paths.insert(path.to_string());
        generations.insert(path.to_string(), generation);
    }

    fn invalidate_direct_object_binding_path(&mut self, path: &str) {
        let root = path.split_once('.').map_or(path, |(root, _)| root);
        self.fork_outer_direct_object_binding_root(root);
        if !self.loop_update_observations.is_empty() {
            let lexical_owner = self.loop_update_binding_owner(root);
            self.retain_loop_update_overwrite(path, lexical_owner);
        }
        let owner =
            self.direct_object_binding_scope_index(DirectObjectBindingScope::BindingOwner, root);
        if let Some(branch) = self.direct_object_binding_branch_scopes.last_mut()
            && owner == Some(branch.index)
        {
            branch.proof_writes.insert(path.to_string());
        }
        let paths = self
            .direct_object_binding_scope_index(DirectObjectBindingScope::BindingOwner, root)
            .map_or_else(
                || self.module_direct_object_binding_paths_by_root.get(root),
                |index| self.scoped_direct_object_binding_paths_by_root[index].get(root),
            );
        if paths.is_some() {
            self.record_direct_object_binding_path_barrier(path);
        }
    }

    /// Real writes update ordinary admission even when optional provenance is off.
    pub(super) fn invalidate_direct_binding_path(&mut self, path: &str, unknown_key: bool) {
        self.invalidate_direct_object_binding_path(path);
        self.invalidate_direct_binding_proof(path, unknown_key);
    }

    pub(super) fn record_direct_object_binding_assignment(
        &mut self,
        binding_path: &str,
        value: &Expression<'_>,
        is_plain_assignment: bool,
    ) {
        let root_name = binding_path
            .split_once('.')
            .map_or(binding_path, |(root, _)| root);
        self.fork_outer_direct_object_binding_root(root_name);
        if !is_plain_assignment {
            self.invalidate_direct_binding_path(binding_path, false);
            return;
        }
        if let Some(source_path) = direct_object_binding_expression_path(value) {
            self.copy_direct_binding_proof(
                &source_path,
                binding_path,
                binding_expression_reference(value),
            );
            self.copy_direct_object_binding_targets(&source_path, binding_path);
            return;
        }
        self.invalidate_direct_binding_path(binding_path, false);
        if matches!(value, Expression::NewExpression(_)) {
            self.record_map_binding_from_declaration(binding_path.to_string(), None, Some(value));
        }

        match value {
            Expression::NewExpression(new_expression) => {
                if let Some(class_name) = self.direct_new_expression_class_name(new_expression) {
                    self.record_direct_object_binding_target(
                        root_name,
                        binding_path.to_string(),
                        class_name,
                        DirectObjectBindingScope::BindingOwner,
                    );
                    self.record_new_binding_proof(binding_path, new_expression);
                }
            }
            Expression::ObjectExpression(object) => {
                self.record_direct_object_binding_targets_at_path(
                    root_name,
                    binding_path,
                    object,
                    DirectObjectBindingScope::BindingOwner,
                );
            }
            _ => {
                if let Some(source_path) = direct_object_binding_expression_path(value) {
                    self.copy_direct_object_binding_targets(&source_path, binding_path);
                    self.copy_direct_binding_proof(
                        &source_path,
                        binding_path,
                        binding_expression_reference(value),
                    );
                }
            }
        }
    }

    fn copy_direct_object_binding_targets(&mut self, source_path: &str, target_path: &str) {
        let source_root = source_path
            .split_once('.')
            .map_or(source_path, |(root, _)| root);
        let nested_prefix = format!("{source_path}.");
        let mut copied = Vec::new();
        let mut shadowed = false;

        for index in (0..self.scoped_direct_object_binding_targets.len()).rev() {
            if let Some(paths) =
                self.scoped_direct_object_binding_paths_by_root[index].get(source_root)
            {
                copied.extend(paths.iter().filter_map(|path| {
                    if path != source_path && !path.starts_with(&nested_prefix) {
                        return None;
                    }
                    let suffix = &path[source_path.len()..];
                    self.scoped_direct_object_binding_targets[index]
                        .get(path)
                        .map(|targets| (format!("{target_path}{suffix}"), targets.clone()))
                }));
                shadowed = true;
                break;
            }
            if self.scoped_direct_object_binding_roots[index].contains(source_root) {
                shadowed = true;
                break;
            }
        }

        if !shadowed
            && let Some(paths) = self
                .module_direct_object_binding_paths_by_root
                .get(source_root)
        {
            copied.extend(paths.iter().filter_map(|path| {
                if path != source_path && !path.starts_with(&nested_prefix) {
                    return None;
                }
                let suffix = &path[source_path.len()..];
                self.module_direct_object_binding_targets
                    .get(path)
                    .map(|targets| (format!("{target_path}{suffix}"), targets.clone()))
            }));
        }

        self.invalidate_direct_object_binding_path(target_path);
        let target_root = target_path
            .split_once('.')
            .map_or(target_path, |(root, _)| root);
        copied.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        for (path, targets) in copied {
            if targets.names.is_empty() {
                self.record_direct_object_binding_path_barrier(&path);
            }
            for class_name in targets.names {
                self.record_direct_object_binding_target(
                    target_root,
                    path.clone(),
                    class_name,
                    DirectObjectBindingScope::BindingOwner,
                );
            }
        }
    }

    fn object_binding_target_count(&self) -> usize {
        self.object_binding_candidates
            .len()
            .saturating_add(self.direct_object_binding_target_count)
    }

    pub(super) fn record_scoped_class_binding(&mut self, binding: &str, class_name: String) {
        self.record_direct_object_binding_target(
            binding,
            binding.to_string(),
            class_name,
            DirectObjectBindingScope::BindingOwner,
        );
    }

    fn record_direct_object_binding_target(
        &mut self,
        root_name: &str,
        mut binding_path: String,
        mut class_name: String,
        scope: DirectObjectBindingScope,
    ) {
        let mut scope_index = self.direct_object_binding_scope_index(scope, root_name);
        loop {
            let owner = scope_index
                .map_or_else(
                    || self.module_direct_object_binding_targets.get(root_name),
                    |index| self.scoped_direct_object_binding_targets[index].get(root_name),
                )
                .and_then(|entry| entry.possible_assignment_owner);
            let next = owner.map(|owner| (owner, binding_path.clone(), class_name.clone()));
            if !self.record_direct_object_binding_target_in_scope(
                root_name,
                binding_path,
                class_name,
                scope_index,
            ) {
                break;
            }
            let Some((owner, path, target)) = next else {
                break;
            };
            binding_path = path;
            class_name = target;
            scope_index = match owner {
                DirectBindingOwner::Module => None,
                DirectBindingOwner::Scope(index) => Some(index),
            };
        }
    }

    fn record_direct_object_binding_target_in_scope(
        &mut self,
        root_name: &str,
        binding_path: String,
        class_name: String,
        scope_index: Option<usize>,
    ) -> bool {
        if scope_index.is_some_and(|index| {
            self.scoped_direct_object_binding_paths_by_root[index]
                .get(root_name)
                .is_some_and(FxHashSet::is_empty)
        }) {
            return false;
        }
        if let Some(index) = scope_index {
            self.scoped_direct_object_binding_roots[index].insert(root_name.to_string());
        }
        let is_new_target = if let Some(index) = scope_index {
            self.scoped_direct_object_binding_targets[index]
                .get(&binding_path)
                .is_none_or(|targets| !targets.contains(&class_name))
        } else {
            self.module_direct_object_binding_targets
                .get(&binding_path)
                .is_none_or(|targets| !targets.contains(&class_name))
        };
        if self
            .abstained_direct_object_binding_paths
            .contains(&binding_path)
            && self
                .direct_object_binding_whole_object_uses
                .insert(class_name.clone())
        {
            self.whole_object_uses.push(class_name.clone());
        }
        if is_new_target && self.object_binding_target_count() >= MAX_OBJECT_BINDING_TARGETS {
            self.abstain_direct_object_binding_path(&binding_path);
            if self
                .direct_object_binding_whole_object_uses
                .insert(class_name.clone())
            {
                self.whole_object_uses.push(class_name);
            }
            return false;
        }
        if is_new_target {
            self.direct_object_binding_target_count += 1;
            self.direct_object_binding_generation =
                self.direct_object_binding_generation.saturating_add(1);
            if let Some(index) = scope_index {
                self.scoped_direct_object_binding_generations[index]
                    .insert(binding_path.clone(), self.direct_object_binding_generation);
            } else {
                self.module_direct_object_binding_generations
                    .insert(binding_path.clone(), self.direct_object_binding_generation);
            }
        }

        if let Some(index) = scope_index {
            self.scoped_direct_object_binding_paths_by_root[index]
                .entry(root_name.to_string())
                .or_default()
                .insert(binding_path.clone());
            self.scoped_direct_object_binding_targets[index]
                .entry(binding_path)
                .or_default()
                .insert(class_name);
        } else {
            self.module_direct_object_binding_paths_by_root
                .entry(root_name.to_string())
                .or_default()
                .insert(binding_path.clone());
            self.module_direct_object_binding_targets
                .entry(binding_path)
                .or_default()
                .insert(class_name);
        }
        true
    }

    fn direct_object_binding_scope_index(
        &self,
        scope: DirectObjectBindingScope,
        root_name: &str,
    ) -> Option<usize> {
        match scope {
            DirectObjectBindingScope::Current => self
                .scoped_direct_object_binding_targets
                .len()
                .checked_sub(1),
            DirectObjectBindingScope::BindingOwner => self
                .scoped_direct_object_binding_roots
                .iter()
                .rposition(|roots| roots.contains(root_name)),
        }
    }

    fn record_direct_object_binding_targets_at_path(
        &mut self,
        root_name: &str,
        object_path: &str,
        obj: &ObjectExpression<'_>,
        scope: DirectObjectBindingScope,
    ) {
        if self
            .direct_object_binding_scope_index(scope, root_name)
            .is_some_and(|index| {
                self.scoped_direct_object_binding_paths_by_root[index]
                    .get(root_name)
                    .is_some_and(FxHashSet::is_empty)
            })
        {
            return;
        }
        if self.capture_subclass_provenance {
            self.record_binding_container(object_path);
        }
        let mut seen_keys = FxHashSet::default();
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                self.invalidate_direct_binding_proof(object_path, true);
                self.remove_direct_targets_preserving_barrier(object_path, scope);
                seen_keys.clear();
                continue;
            };
            let Some(key_name) = prop.key.static_name() else {
                self.invalidate_direct_binding_proof(object_path, true);
                self.remove_direct_targets_preserving_barrier(object_path, scope);
                seen_keys.clear();
                continue;
            };
            let binding_path = format!("{object_path}.{key_name}");
            if !seen_keys.insert(key_name.to_string()) {
                self.remove_direct_targets_preserving_barrier(&binding_path, scope);
            }
            self.invalidate_direct_binding_proof(&binding_path, false);
            match &prop.value {
                Expression::NewExpression(new_expr) => {
                    if let Some(class_name) = self.direct_new_expression_class_name(new_expr) {
                        self.record_direct_object_binding_target(
                            root_name,
                            binding_path.clone(),
                            class_name,
                            scope,
                        );
                        self.record_new_binding_proof(&binding_path, new_expr);
                    }
                }
                Expression::ObjectExpression(child) => {
                    self.record_direct_object_binding_targets_at_path(
                        root_name,
                        &binding_path,
                        child,
                        scope,
                    );
                }
                _ => {}
            }
        }
    }

    fn remove_direct_targets_preserving_barrier(
        &mut self,
        path: &str,
        scope: DirectObjectBindingScope,
    ) {
        let was_barrier = self
            .direct_object_binding_path_targets(path)
            .is_some_and(FxHashSet::is_empty);
        let root = path.split_once('.').map_or(path, |(root, _)| root);
        let index = self.direct_object_binding_scope_index(scope, root);
        let barrier = was_barrier
            .then(|| {
                index
                    .map_or_else(
                        || self.module_direct_object_binding_targets.get(path),
                        |index| self.scoped_direct_object_binding_targets[index].get(path),
                    )
                    .cloned()
            })
            .flatten();
        self.remove_direct_object_binding_targets_at_or_below(path, scope);
        if was_barrier {
            if let Some(entry) = barrier {
                if let Some(index) = index {
                    self.scoped_direct_object_binding_targets[index]
                        .insert(path.to_string(), entry);
                } else {
                    self.module_direct_object_binding_targets
                        .insert(path.to_string(), entry);
                }
            }
            self.record_direct_object_binding_path_barrier(path);
        }
    }

    fn remove_direct_object_binding_targets_at_or_below(
        &mut self,
        prefix: &str,
        scope: DirectObjectBindingScope,
    ) {
        let nested_prefix = format!("{prefix}.");
        let root_name = prefix.split_once('.').map_or(prefix, |(root, _)| root);
        if let Some(index) = self.direct_object_binding_scope_index(scope, root_name) {
            let removed = self.scoped_direct_object_binding_paths_by_root[index]
                .get(root_name)
                .map(|paths| {
                    paths
                        .iter()
                        .filter(|path| *path == prefix || path.starts_with(&nested_prefix))
                        .cloned()
                        .collect::<FxHashSet<_>>()
                })
                .unwrap_or_default();
            for path in &removed {
                self.scoped_direct_object_binding_targets[index].remove(path);
                self.scoped_direct_object_binding_generations[index].remove(path);
            }
            if !removed.is_empty() {
                self.direct_object_binding_generation =
                    self.direct_object_binding_generation.saturating_add(1);
            }
            if let Some(paths) =
                self.scoped_direct_object_binding_paths_by_root[index].get_mut(root_name)
            {
                paths.retain(|path| !removed.contains(path));
                if paths.is_empty() {
                    self.scoped_direct_object_binding_paths_by_root[index].remove(root_name);
                }
            }
        } else {
            let removed = self
                .module_direct_object_binding_paths_by_root
                .get(root_name)
                .map(|paths| {
                    paths
                        .iter()
                        .filter(|path| *path == prefix || path.starts_with(&nested_prefix))
                        .cloned()
                        .collect::<FxHashSet<_>>()
                })
                .unwrap_or_default();
            for path in &removed {
                self.module_direct_object_binding_targets.remove(path);
                self.module_direct_object_binding_generations.remove(path);
            }
            if !removed.is_empty() {
                self.direct_object_binding_generation =
                    self.direct_object_binding_generation.saturating_add(1);
            }
            if let Some(paths) = self
                .module_direct_object_binding_paths_by_root
                .get_mut(root_name)
            {
                paths.retain(|path| !removed.contains(path));
                if paths.is_empty() {
                    self.module_direct_object_binding_paths_by_root
                        .remove(root_name);
                }
            }
        }
    }

    fn record_object_binding_targets_at_path(
        &mut self,
        root_name: &str,
        object_path: &str,
        obj: &ObjectExpression<'_>,
    ) {
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                continue;
            };
            let Some(key_name) = prop.key.static_name() else {
                continue;
            };

            let binding_path = format!("{object_path}.{key_name}");
            match &prop.value {
                // Per-module breadth cap (issue #1843 follow-up): the guard stops
                // recording once at capacity so a pathological object literal
                // cannot grow the candidate set (and its O(n^2) fixpoint resolver)
                // without bound. At capacity the arm falls through to the no-op
                // `_ =>` arm, identical to skipping the push.
                Expression::Identifier(ident)
                    if self.object_binding_target_count() < MAX_OBJECT_BINDING_TARGETS =>
                {
                    // The candidate gives this placement a precise path, which
                    // `resolve_object_binding_candidates` turns back into
                    // `<source>.<member>` accesses, so a namespace placed here
                    // is resolved rather than handed over bare. What hands it
                    // to, which `record_object_literal_namespace_pass` covers.
                    // See issue #2377.
                    self.record_object_literal_namespace_placement(root_name, ident);
                    self.copy_direct_binding_proof(
                        ident.name.as_str(),
                        &binding_path,
                        Some(ident.span),
                    );
                    self.object_binding_candidates.push(ObjectBindingCandidate {
                        binding_path,
                        source_name: ident.name.to_string(),
                    });
                }
                Expression::ObjectExpression(child) => {
                    self.record_object_binding_targets_at_path(root_name, &binding_path, child);
                }
                _ => {}
            }
        }
    }

    fn direct_new_expression_class_name(&self, expression: &NewExpression<'_>) -> Option<String> {
        match &expression.callee {
            Expression::Identifier(callee) => {
                if super::super::helpers::is_builtin_constructor(callee.name.as_str()) {
                    return None;
                }
                Some(callee.name.to_string())
            }
            callee => {
                let path = direct_object_binding_expression_path(callee)?;
                let (namespace_local, export_name) = path.split_once('.')?;
                (self.namespace_import_locals.contains(namespace_local)
                    && !export_name.contains('.')
                    && !self
                        .scoped_direct_object_binding_roots
                        .iter()
                        .any(|roots| roots.contains(namespace_local))
                    && !self.namespace_like_binding_is_shadowed(namespace_local))
                .then_some(path)
            }
        }
    }
}

fn direct_object_binding_expression_path(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::Identifier(identifier) => Some(identifier.name.to_string()),
        Expression::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            direct_object_binding_expression_path(&member.object)?,
            member.property.name
        )),
        Expression::ComputedMemberExpression(member) => Some(format!(
            "{}.{}",
            direct_object_binding_expression_path(&member.object)?,
            member.static_property_name()?
        )),
        Expression::ParenthesizedExpression(parenthesized) => {
            direct_object_binding_expression_path(&parenthesized.expression)
        }
        Expression::TSAsExpression(assertion) => {
            direct_object_binding_expression_path(&assertion.expression)
        }
        Expression::TSSatisfiesExpression(assertion) => {
            direct_object_binding_expression_path(&assertion.expression)
        }
        Expression::TSNonNullExpression(assertion) => {
            direct_object_binding_expression_path(&assertion.expression)
        }
        Expression::TSTypeAssertion(assertion) => {
            direct_object_binding_expression_path(&assertion.expression)
        }
        _ => None,
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::{MAX_EXPORTED_OBJECT_INSTANCE_PROPERTY_FACTS, MAX_OBJECT_BINDING_TARGETS};
    use crate::SemanticFact;
    use crate::visitor::DirectBindingTarget;
    use crate::visitor::{MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES, ModuleInfoExtractor};
    use oxc_allocator::Allocator;
    use oxc_ast_visit::Visit;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use rustc_hash::FxHashSet;

    #[test]
    fn loop_update_observation_limits_do_not_replay_partial_state() {
        use std::fmt::Write as _;

        for capture in [false, true] {
            for (label, prefix, replay) in [
                ("admitted", "opaque.other=0,".to_string(), true),
                (
                    "observation limit",
                    "holder.property.read(),".repeat(MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES),
                    false,
                ),
                (
                    "overwrite limit",
                    (0..=MAX_OBJECT_BINDING_TARGETS).fold(String::new(), |mut source, index| {
                        let _ = write!(source, "opaque.p{index}=0,");
                        source
                    }),
                    false,
                ),
            ] {
                let source = format!(
                    "import {{Alpha,Gamma}} from './services';const holder={{property:new Alpha()}};function mutate(){{for(let i=0;i<1;({prefix}holder.property.read(),i++)){{holder.property=new Gamma();}}}}mutate();"
                );
                let allocator = Allocator::default();
                let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
                assert!(parsed.diagnostics.is_empty());
                let mut extractor = ModuleInfoExtractor::new();
                extractor.capture_subclass_provenance = capture;
                extractor.visit_program(&parsed.program);
                assert!(
                    extractor
                        .member_accesses
                        .iter()
                        .any(|a| a.object == "Alpha" && a.member == "read"),
                    "ordinary traversal remains admitted"
                );
                assert_eq!(
                    extractor
                        .member_accesses
                        .iter()
                        .any(|a| a.object == "Gamma" && a.member == "read"),
                    replay,
                    "{label}, capture={capture}"
                );
                assert!(extractor.loop_update_observations.is_empty());
            }
        }
    }

    /// A single object literal with far more identifier-valued properties than
    /// the per-module cap must not grow `object_binding_candidates` past the cap.
    /// Mirrors `tainted_binding_recording_is_bounded_on_dense_source`: the
    /// object-binding channel has the same super-linear failure mode (an O(n^2)
    /// fixpoint resolver over an unbounded candidate set) on dense machine-
    /// generated source, and the cap degrades over-cap files to module-level
    /// reachability rather than OOMing. See issue #1843 follow-up.
    #[test]
    fn object_binding_candidate_recording_is_bounded_on_dense_source() {
        use std::fmt::Write as _;

        let over_cap = MAX_OBJECT_BINDING_TARGETS + 1000;
        let mut props = String::new();
        for k in 0..over_cap {
            // Each identifier-valued property seeds one object-binding candidate.
            let _ = write!(props, "k{k}: v{k}, ");
        }
        let source = format!("const big = {{ {props} }};");

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);

        // The cap must engage (input deterministically exceeds it) but never
        // zero out recording.
        assert!(
            !extractor.object_binding_candidates.is_empty(),
            "the cap must not zero out object-binding recording"
        );
        assert!(
            extractor.object_binding_candidates.len() <= MAX_OBJECT_BINDING_TARGETS,
            "object-binding candidate recording must stay bounded at the \
             per-module cap on dense source (got {})",
            extractor.object_binding_candidates.len()
        );
    }

    #[test]
    fn exported_object_instance_property_facts_are_bounded() {
        use std::fmt::Write as _;

        let mut exports = String::new();
        for index in 0..=MAX_EXPORTED_OBJECT_INSTANCE_PROPERTY_FACTS {
            let _ = writeln!(exports, "export {{ holder as alias{index} }};");
        }
        let source = format!(
            "import * as NS from './services';\nconst holder = {{ service: new NS.Service() }};\n{exports}"
        );

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);
        extractor.resolve_pending_local_export_specifiers();
        extractor.record_exported_direct_object_binding_facts();

        let fact_count = extractor
            .semantic_facts
            .iter()
            .filter(|fact| matches!(fact, SemanticFact::ExportedObjectInstanceProperty(_)))
            .count();
        assert_eq!(fact_count, MAX_EXPORTED_OBJECT_INSTANCE_PROPERTY_FACTS);
        assert!(
            extractor
                .direct_object_binding_whole_object_uses
                .contains("NS.Service"),
            "cap exhaustion must conservatively credit every possible qualified class"
        );
    }

    #[test]
    fn direct_object_binding_target_recording_is_bounded_on_dense_source() {
        use std::fmt::Write as _;

        let over_cap = MAX_OBJECT_BINDING_TARGETS + 1000;
        let mut props = String::new();
        for k in 0..over_cap {
            let _ = write!(props, "k{k}: new Service(), ");
        }
        let source = format!("const big = {{ {props} }};");

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);

        assert_eq!(
            extractor.direct_object_binding_target_count, MAX_OBJECT_BINDING_TARGETS,
            "the direct target cap should engage on dense source"
        );
        assert!(
            extractor.module_direct_object_binding_targets.len() <= MAX_OBJECT_BINDING_TARGETS,
            "direct object binding targets must keep the scope map bounded"
        );
    }

    #[test]
    fn direct_object_binding_associations_are_bounded_on_repeated_path() {
        use std::fmt::Write as _;

        let over_cap = MAX_OBJECT_BINDING_TARGETS + 1000;
        let mut assignments = String::new();
        for k in 0..over_cap {
            let _ = writeln!(assignments, "holder.property = new Service{k}();");
        }
        let source = format!("const holder = {{ property: new InitialService() }};\n{assignments}");

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);

        assert_eq!(
            extractor.direct_object_binding_target_count, MAX_OBJECT_BINDING_TARGETS,
            "the direct target cap should include repeated-path associations"
        );
        assert!(
            extractor
                .module_direct_object_binding_targets
                .get("holder.property")
                .is_some_and(|targets| targets.len() <= MAX_OBJECT_BINDING_TARGETS),
            "a repeated path must not accumulate unbounded class targets"
        );

        extractor.record_direct_object_binding_target(
            "holder",
            "holder.property".to_string(),
            "ServiceAfterCap".to_string(),
            super::DirectObjectBindingScope::BindingOwner,
        );
        assert!(
            extractor
                .direct_object_binding_whole_object_uses
                .contains("ServiceAfterCap"),
            "an over-cap target must receive conservative whole-object credit"
        );
    }

    #[test]
    fn direct_object_binding_member_access_emission_is_bounded_and_deduplicated() {
        let mut extractor = ModuleInfoExtractor::new();
        let classes = (0..128).map(|index| format!("Service{index}")).collect();
        extractor
            .module_direct_object_binding_targets
            .insert("holder.property".to_string(), classes);

        for index in 0..128 {
            assert!(
                extractor
                    .record_walk_order_member_access("holder.property", &format!("member{index}"))
            );
        }
        assert_eq!(
            extractor.member_accesses.len(),
            MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES,
            "direct member access emission should stop at its module cap"
        );

        assert!(
            extractor.record_walk_order_member_access("holder.property", "member128"),
            "the direct receiver should remain resolved after reaching the cap"
        );
        assert_eq!(
            extractor.member_accesses.len(),
            MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES,
            "an over-cap access must not grow the emitted vector"
        );
        assert_eq!(
            extractor.direct_object_binding_whole_object_uses.len(),
            128,
            "cap exhaustion should conservatively abstain every possible class"
        );
    }

    #[test]
    fn repeated_multi_target_member_access_uses_generation_cache() {
        let mut extractor = ModuleInfoExtractor::new();
        let classes = (0..2048).map(|index| format!("Service{index}")).collect();
        extractor
            .module_direct_object_binding_targets
            .insert("holder.property".to_string(), classes);

        for _ in 0..2048 {
            assert!(extractor.record_walk_order_member_access("holder.property", "run"));
        }

        assert_eq!(
            extractor.member_accesses.len(),
            2048,
            "an unchanged repeated access should emit each class association once"
        );
        assert_eq!(
            extractor
                .direct_object_binding_access_generations
                .get("holder.property")
                .and_then(|members| members.get("run")),
            Some(&0),
            "the processed generation should be cached for constant-time repeats"
        );
    }

    #[test]
    fn repeated_dynamic_member_access_uses_generation_cache() {
        let mut extractor = ModuleInfoExtractor::new();
        extractor.module_direct_object_binding_paths_by_root.insert(
            "holder".to_string(),
            FxHashSet::from_iter(["holder.primary".to_string(), "holder.secondary".to_string()]),
        );
        extractor.module_direct_object_binding_targets.insert(
            "holder.primary".to_string(),
            FxHashSet::from_iter(["Primary".to_string()])
                .into_iter()
                .collect(),
        );
        extractor.module_direct_object_binding_targets.insert(
            "holder.secondary".to_string(),
            FxHashSet::from_iter(["Secondary".to_string()])
                .into_iter()
                .collect(),
        );

        for _ in 0..2048 {
            assert!(extractor.record_dynamic_direct_object_binding_member_access("holder", "run"));
        }

        assert_eq!(extractor.member_accesses.len(), 2);
        assert_eq!(
            extractor
                .dynamic_direct_object_binding_access_generations
                .get(&("holder".to_string(), "run".to_string())),
            Some(&0),
            "the effective dynamic receiver generation should be cached"
        );
    }

    #[test]
    fn dynamic_member_cap_abstains_receiver_once() {
        let mut extractor = ModuleInfoExtractor::new();
        extractor.module_direct_object_binding_paths_by_root.insert(
            "holder".to_string(),
            FxHashSet::from_iter(["holder.primary".to_string(), "holder.secondary".to_string()]),
        );
        extractor.module_direct_object_binding_targets.insert(
            "holder.primary".to_string(),
            (0..MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES)
                .map(|index| format!("Primary{index}"))
                .collect(),
        );
        extractor.module_direct_object_binding_targets.insert(
            "holder.secondary".to_string(),
            FxHashSet::from_iter(["Secondary".to_string()])
                .into_iter()
                .collect(),
        );

        assert!(extractor.record_dynamic_direct_object_binding_member_access("holder", "run"));
        assert_eq!(
            extractor
                .dynamic_direct_object_binding_abstention_generations
                .get("holder"),
            Some(&0),
            "cap exhaustion should cache whole-receiver abstention"
        );
        assert!(
            extractor
                .direct_object_binding_whole_object_uses
                .contains("Secondary"),
            "every matching path must receive conservative whole-object credit"
        );

        let emitted = extractor.member_accesses.len();
        assert!(extractor.record_dynamic_direct_object_binding_member_access("holder", "other"));
        assert_eq!(
            extractor.member_accesses.len(),
            emitted,
            "an unchanged abstained receiver should not be enumerated again"
        );
    }

    #[test]
    fn abstained_path_does_not_suppress_shadowed_raw_access() {
        let mut extractor = ModuleInfoExtractor::new();
        extractor.module_direct_object_binding_targets.insert(
            "holder.property".to_string(),
            DirectBindingTarget {
                names: FxHashSet::from_iter(["Alpha".to_string()]),
                ..Default::default()
            },
        );
        extractor
            .abstained_direct_object_binding_paths
            .insert("holder.property".to_string());
        extractor.push_direct_object_binding_scope(FxHashSet::from_iter(["holder".to_string()]));

        assert!(
            !extractor.record_walk_order_member_access("holder.property", "used"),
            "a nearer shadow without a direct target must preserve the legacy raw access"
        );
        extractor.pop_direct_object_binding_scope();
    }

    #[test]
    fn scoped_abstention_credits_same_path_targets_in_outer_scopes() {
        let mut extractor = ModuleInfoExtractor::new();
        extractor.module_direct_object_binding_targets.insert(
            "holder.property".to_string(),
            DirectBindingTarget {
                names: FxHashSet::from_iter(["Alpha".to_string()]),
                ..Default::default()
            },
        );
        extractor.push_direct_object_binding_scope(FxHashSet::from_iter(["holder".to_string()]));
        extractor.scoped_direct_object_binding_targets[0].insert(
            "holder.property".to_string(),
            DirectBindingTarget {
                names: FxHashSet::from_iter(["Beta".to_string()]),
                ..Default::default()
            },
        );
        for index in 0..MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES {
            extractor
                .direct_object_binding_member_accesses
                .insert((format!("Used{index}"), "member".to_string()));
        }

        assert!(extractor.record_walk_order_member_access("holder.property", "used"));
        assert!(
            extractor
                .direct_object_binding_whole_object_uses
                .contains("Alpha")
                && extractor
                    .direct_object_binding_whole_object_uses
                    .contains("Beta"),
            "scope-global abstention must credit both visible and shadowed targets"
        );
        extractor.pop_direct_object_binding_scope();
        assert!(extractor.record_walk_order_member_access("holder.property", "used"));
    }

    #[test]
    fn mixed_object_binding_targets_share_the_breadth_cap() {
        use std::fmt::Write as _;

        let over_cap = MAX_OBJECT_BINDING_TARGETS + 1000;
        let mut props = String::new();
        for k in 0..over_cap {
            if k % 2 == 0 {
                let _ = write!(props, "k{k}: new Service(), ");
            } else {
                let _ = write!(props, "k{k}: v{k}, ");
            }
        }
        let source = format!("const big = {{ {props} }};");

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);

        assert_eq!(
            extractor.object_binding_target_count(),
            MAX_OBJECT_BINDING_TARGETS,
            "identifier candidates and direct targets must share one cap"
        );
    }
}

fn binding_expression_reference(expression: &Expression<'_>) -> Option<Span> {
    match super::unwrap_static_expr(expression) {
        Expression::Identifier(reference) => Some(reference.span),
        Expression::StaticMemberExpression(member) => binding_expression_reference(&member.object),
        Expression::ComputedMemberExpression(member) if member.static_property_name().is_some() => {
            binding_expression_reference(&member.object)
        }
        _ => None,
    }
}
