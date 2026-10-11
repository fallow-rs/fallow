//! Possible constructor origins follow shared value slots, independently of strict proofs.

use std::sync::Arc;

use oxc_ast::ast::IdentifierReference;
use oxc_span::Span;

use super::super::{
    BindingClassProof, ConstructorWrite, ConstructorWriteTarget, DirectBindingValue,
    DirectObjectBindingBranch, MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES, ModuleInfoExtractor,
    PendingSubclassMemberAccess, SubclassAccessProof,
};
use super::visit_object_bindings::MAX_OBJECT_BINDING_TARGETS;
use super::visit_projections::MAX_PROJECTION_VALUES;

const MAX_POSSIBLE_CONSTRUCTOR_WORK: usize = MAX_OBJECT_BINDING_TARGETS;
type ConstructorOutcomes = Vec<(ConstructorWriteTarget, Arc<[BindingClassProof]>)>;

impl ModuleInfoExtractor {
    fn proof_work(&self, proof: &BindingClassProof) -> usize {
        self.requirement_len(proof.requirements)
            .saturating_add(proof.builtin_containers.len())
            .saturating_add(1)
    }

    fn constructor_proofs_equal(
        &mut self,
        left: &BindingClassProof,
        right: &BindingClassProof,
    ) -> Result<bool, ()> {
        if !self.charge_constructor_work(1) {
            return Err(());
        }
        if left.target_name != right.target_name
            || left.target_reference_span != right.target_reference_span
            || left.builtin_containers != right.builtin_containers
        {
            return Ok(false);
        }
        let mut remaining =
            MAX_POSSIBLE_CONSTRUCTOR_WORK.saturating_sub(self.possible_constructor_work);
        let result = self.requirements_equal(left.requirements, right.requirements, &mut remaining);
        self.possible_constructor_work = MAX_POSSIBLE_CONSTRUCTOR_WORK - remaining;
        result
    }

    fn join_constructor_proofs(
        &mut self,
        left: &[BindingClassProof],
        right: &[BindingClassProof],
    ) -> Result<Arc<[BindingClassProof]>, ()> {
        let mut joined: Vec<BindingClassProof> = Vec::new();
        for proof in left.iter().chain(right) {
            let mut duplicate = false;
            for previous in &joined {
                if self.constructor_proofs_equal(previous, proof)? {
                    duplicate = true;
                    break;
                }
            }
            if !duplicate {
                joined.push(proof.clone());
            }
        }
        Ok(joined.into())
    }

    fn constructor_owner_accepts_write(&self, owner: Option<Span>) -> bool {
        self.active_constructor_function.is_none()
            || self.active_constructor_function == owner
            || self
                .direct_object_binding_branch_scopes
                .last()
                .is_some_and(|branch| {
                    self.direct_object_binding_function_scopes
                        .last()
                        .is_some_and(|frame| branch.index > *frame)
                })
    }

    fn constructor_target_accepts_write(&self, target: &ConstructorWriteTarget) -> bool {
        match target {
            ConstructorWriteTarget::Property { parent, .. } => self
                .constructor_owner_accepts_write(
                    self.projection_value_view(*parent).constructor_owner,
                ),
            ConstructorWriteTarget::Root { path, declaration } => {
                self.direct_binding_entry(path).is_some_and(|entry| {
                    entry.declaration_span == Some(*declaration)
                        && self.constructor_owner_accepts_write(entry.constructor_owner)
                })
            }
        }
    }

    fn current_constructor_outcome(
        &self,
        target: &ConstructorWriteTarget,
    ) -> Arc<[BindingClassProof]> {
        let value = match target {
            ConstructorWriteTarget::Property { parent, property } => self
                .projection_value_view(*parent)
                .properties
                .get(property)
                .copied(),
            ConstructorWriteTarget::Root { path, declaration } => self
                .direct_binding_entry(path)
                .filter(|entry| entry.declaration_span == Some(*declaration))
                .and_then(|entry| entry.value),
        };
        value
            .map(|id| Arc::clone(&self.projection_value_view(id).constructor_alternatives))
            .unwrap_or_default()
    }

    fn refuse_constructor_root_write(&mut self, path: &str, declaration: Span) {
        let Some(branch) = self.direct_object_binding_branch_scopes.last_mut() else {
            return;
        };
        // Rejection must erase an older receipt without allocating another key or touching
        // its scalar value. This scan is bounded by the already retained branch entries.
        for (target, write) in &mut branch.constructor_writes {
            if matches!(target, ConstructorWriteTarget::Root { path: recorded, declaration: span }
                if recorded == path && *span == declaration)
            {
                write.output = Arc::default();
                write.refused = true;
            }
        }
    }

    fn charge_constructor_work(&mut self, amount: usize) -> bool {
        if amount > MAX_POSSIBLE_CONSTRUCTOR_WORK.saturating_sub(self.possible_constructor_work) {
            return false;
        }
        self.possible_constructor_work += amount;
        true
    }

    pub(super) fn retain_new_constructor(
        &mut self,
        path: &str,
        reference: &IdentifierReference<'_>,
    ) {
        if !self.capture_subclass_provenance
            || self.direct_binding_values.len() >= MAX_PROJECTION_VALUES
            || !self.charge_constructor_work(1)
        {
            return;
        }
        let Some(id) = self.ensure_direct_binding_value(path) else {
            return;
        };
        let id = self.projection_view_id(id);
        if self.is_projection_barrier(id) {
            return;
        }
        let admitted = if path.contains('.') {
            self.direct_object_binding_targets(path)
                .is_some_and(|(names, _)| names.contains(reference.name.as_str()))
        } else {
            self.direct_binding_values[id]
                .proof
                .as_ref()
                .is_some_and(|proof| {
                    proof.target_name == reference.name.as_str()
                        && proof.target_reference_span == reference.span
                })
        };
        if !admitted {
            return;
        }
        let owner = if let Some((parent, _)) = path.rsplit_once('.') {
            let Some(parent) = self.ensure_direct_binding_value(parent) else {
                return;
            };
            self.projection_value_view(parent).constructor_owner
        } else {
            self.direct_binding_entry(path)
                .and_then(|entry| entry.constructor_owner)
        };
        if !self.constructor_owner_accepts_write(owner) {
            return;
        }
        self.direct_binding_values[id].constructor_alternatives = Arc::from([BindingClassProof {
            builtin_containers: Arc::default(),
            target_name: reference.name.to_string(),
            target_reference_span: reference.span,
            requirements: super::super::BindingRequirements::Empty,
        }]);
        // Acquisition finishes after shared attachment and exact constructor admission.
        if let Some((parent, property)) = path.rsplit_once('.') {
            if let Some(parent) = self.ensure_direct_binding_value(parent) {
                self.prepare_constructor_property_write(parent, property, id);
            }
        } else {
            self.prepare_constructor_root_write(path, id);
        }
    }

    fn retain_constructor_write(
        &mut self,
        target: ConstructorWriteTarget,
        input: Arc<[BindingClassProof]>,
        output: Arc<[BindingClassProof]>,
    ) -> bool {
        if self.direct_object_binding_branch_scopes.is_empty()
            || !self.constructor_target_accepts_write(&target)
        {
            return true;
        }
        let exists = self
            .direct_object_binding_branch_scopes
            .last()
            .is_some_and(|branch| branch.constructor_writes.contains_key(&target));
        if !exists && !self.charge_constructor_work(1) {
            return false;
        }
        let Some(branch) = self.direct_object_binding_branch_scopes.last_mut() else {
            return false;
        };
        branch
            .constructor_writes
            .entry(target)
            .and_modify(|write| {
                if !write.refused {
                    write.output = Arc::clone(&output);
                }
            })
            .or_insert(ConstructorWrite {
                input,
                output,
                refused: false,
            });
        true
    }

    pub(super) fn prepare_constructor_property_write(
        &mut self,
        parent: usize,
        property: &str,
        value: usize,
    ) -> Option<usize> {
        let view = self.projection_view_id(parent);
        if self.is_projection_barrier(view) {
            return None;
        }
        if !self.capture_subclass_provenance {
            return Some(view);
        }
        let input = self.direct_binding_values[view]
            .properties
            .get(property)
            .map(|id| Arc::clone(&self.projection_value_view(*id).constructor_alternatives))
            .unwrap_or_default();
        let output = Arc::clone(&self.projection_value_view(value).constructor_alternatives);
        if self.direct_object_binding_branch_scopes.is_empty() {
            return Some(view);
        }
        let replaces_container = self.direct_binding_values[view]
            .properties
            .get(property)
            .is_some_and(|id| !self.projection_value_view(*id).properties.is_empty());
        if input.is_empty()
            && output.is_empty()
            && self.direct_binding_values[value].proof.is_none()
            && !replaces_container
        {
            return Some(view);
        }
        if !self.charge_constructor_work(1) {
            self.refuse_projection_mutation(parent);
            return None;
        }
        let target = ConstructorWriteTarget::Property {
            parent,
            property: property.to_string(),
        };
        if !self.retain_constructor_write(target, input, output) {
            self.refuse_projection_mutation(parent);
            return None;
        }
        let Some(view) = self.writable_projection_view(parent) else {
            self.refuse_projection_mutation(parent);
            return None;
        };
        if let Some(branch) = self.direct_object_binding_branch_scopes.last_mut() {
            branch
                .projection_writes
                .entry(parent)
                .or_default()
                .insert(Some(property.to_string()));
        }
        Some(view)
    }

    pub(super) fn prepare_constructor_root_write(&mut self, path: &str, value: usize) {
        if !self.capture_subclass_provenance || self.direct_object_binding_branch_scopes.is_empty()
        {
            return;
        }
        let Some(declaration) = self
            .direct_binding_entry(path)
            .and_then(|entry| entry.declaration_span)
        else {
            return;
        };
        if !self.charge_constructor_work(1) {
            self.refuse_constructor_root_write(path, declaration);
            return;
        }
        let Some(entry) = self.direct_binding_entry(path) else {
            return;
        };
        let input = entry
            .value
            .map(|id| Arc::clone(&self.projection_value_view(id).constructor_alternatives))
            .unwrap_or_default();
        let output = Arc::clone(&self.projection_value_view(value).constructor_alternatives);
        let target = ConstructorWriteTarget::Root {
            path: path.to_string(),
            declaration,
        };
        let recorded = self
            .direct_object_binding_branch_scopes
            .last()
            .is_some_and(|branch| branch.constructor_writes.contains_key(&target));
        if input.is_empty() && output.is_empty() && !recorded {
            return;
        }
        if !self.retain_constructor_write(target, input, output) {
            self.refuse_constructor_root_write(path, declaration);
        }
    }

    pub(super) fn join_constructor_outcomes(
        &mut self,
        consequent: &DirectObjectBindingBranch,
        alternate: &DirectObjectBindingBranch,
    ) -> ConstructorOutcomes {
        let mut outcomes = Vec::new();
        for (branch, other) in [(consequent, alternate), (alternate, consequent)] {
            for (target, write) in &branch.constructor_writes {
                if std::ptr::eq(branch, alternate) && other.constructor_writes.contains_key(target)
                {
                    continue;
                }
                if write.refused
                    || other
                        .constructor_writes
                        .get(target)
                        .is_some_and(|write| write.refused)
                {
                    if let ConstructorWriteTarget::Root { path, declaration } = target {
                        self.refuse_constructor_root_write(path, *declaration);
                    }
                    continue;
                }
                if let ConstructorWriteTarget::Property { parent, .. } = target
                    && [consequent, alternate].iter().any(|branch| {
                        branch
                            .projection_writes
                            .get(parent)
                            .is_some_and(|writes| writes.contains(&None))
                    })
                {
                    continue;
                }
                if !self.constructor_target_accepts_write(target) {
                    continue;
                }
                // The same optional update is joined before and after a for body. Its
                // bypass input is the current shared slot, including body replacements.
                let other_output = other.constructor_writes.get(target).map_or_else(
                    || self.current_constructor_outcome(target),
                    |other| Arc::clone(&other.output),
                );
                let work = write
                    .output
                    .iter()
                    .chain(other_output.iter())
                    .map(|proof| self.proof_work(proof))
                    .fold(0usize, usize::saturating_add);
                if !self.charge_constructor_work(work.saturating_mul(2).saturating_add(1)) {
                    continue;
                }
                let Ok(joined) = self.join_constructor_proofs(&write.output, &other_output) else {
                    if let ConstructorWriteTarget::Root { path, declaration } = target {
                        self.refuse_constructor_root_write(path, *declaration);
                    }
                    continue;
                };
                // Preserve the enclosing branch input before strict invalidation below.
                if !self.retain_constructor_write(
                    target.clone(),
                    Arc::clone(&write.input),
                    Arc::clone(&joined),
                ) {
                    continue;
                }
                outcomes.push((target.clone(), joined));
            }
        }
        outcomes
    }

    pub(super) fn install_constructor_outcomes(&mut self, outcomes: ConstructorOutcomes) {
        for (target, alternatives) in outcomes {
            if alternatives.is_empty() || !self.constructor_target_accepts_write(&target) {
                continue;
            }
            match &target {
                ConstructorWriteTarget::Property { parent, .. } => {
                    let view = self.projection_view_id(*parent);
                    if self.is_projection_barrier(view)
                        || self.direct_binding_values[view].unknown_properties
                    {
                        continue;
                    }
                }
                ConstructorWriteTarget::Root { .. } => {}
            }
            let Some(id) = self.allocate_projection_value(DirectBindingValue {
                constructor_alternatives: alternatives,
                ..DirectBindingValue::unknown()
            }) else {
                if let ConstructorWriteTarget::Property { parent, .. } = target {
                    self.refuse_projection_mutation(parent);
                }
                continue;
            };
            match target {
                ConstructorWriteTarget::Property { parent, property } => {
                    if let Some(view) =
                        self.prepare_constructor_property_write(parent, &property, id)
                    {
                        self.direct_binding_values[view]
                            .properties
                            .insert(property, id);
                    }
                }
                ConstructorWriteTarget::Root { path, .. } => {
                    self.attach_direct_binding_value(&path, id);
                }
            }
        }
    }

    pub(super) fn capture_constructor_alternatives(
        &mut self,
        path: &str,
        reference: Option<Span>,
        member: &str,
        strict: Option<&BindingClassProof>,
    ) -> bool {
        let Some(id) = self.ensure_direct_binding_value(path) else {
            return false;
        };
        let value = self.projection_value_view(id);
        let alternatives = Arc::clone(&value.constructor_alternatives);
        if alternatives.is_empty() {
            return false;
        }
        let root = path.split_once('.').map_or(path, |(root, _)| root);
        let Some(entry) = self.direct_binding_entry(root) else {
            return false;
        };
        let (Some(reference), Some(declaration)) = (reference, entry.declaration_span) else {
            return false;
        };
        let work = alternatives
            .iter()
            .map(|proof| {
                self.proof_work(proof)
                    .saturating_add(self.requirement_len(value.requirements))
                    .saturating_add(self.requirement_len(entry.requirements))
                    .saturating_add(2)
            })
            .fold(0usize, usize::saturating_add);
        if self
            .pending_subclass_member_accesses
            .len()
            .saturating_add(alternatives.len())
            > MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES
            || !self.charge_constructor_work(work)
        {
            return false;
        }
        let value = self.projection_value_view(id);
        let Some(entry) = self.direct_binding_entry(root) else {
            return false;
        };
        let requirements = self.combine_requirements(
            &[value.requirements, entry.requirements],
            &[(reference, declaration)],
        );
        for proof in alternatives.iter() {
            let mut proof = proof.clone();
            proof.requirements = self.concat_requirements(proof.requirements, requirements);
            if let Some(strict) = strict {
                match self.constructor_proofs_equal(strict, &proof) {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(()) => return false,
                }
            }
            self.pending_subclass_member_accesses
                .push(PendingSubclassMemberAccess {
                    proof: SubclassAccessProof::Known(proof),
                    member: member.to_string(),
                });
        }
        true
    }
}
