//! Shared, immutable declaration conditions for source-time value observations.

use std::hash::{Hash, Hasher};

use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};

use super::ModuleInfoExtractor;
use super::visit_impl::visit_projections::MAX_PROJECTION_VALUES;

pub(super) const MAX_BINDING_REQUIREMENT_NODES: usize = MAX_PROJECTION_VALUES;
const SEQUENCE_BASE: u64 = 0x9e37_79b1_85eb_ca87;
type Condition = (Span, Span);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(super) enum BindingRequirements {
    #[default]
    Empty,
    Node(usize),
    Refused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RequirementKind {
    Append(BindingRequirements, Condition),
    Concat(BindingRequirements, BindingRequirements),
}

#[derive(Debug)]
struct RequirementNode {
    kind: RequirementKind,
    length: usize,
    hash: u64,
    factor: u64,
    admitted: bool,
}

#[derive(Debug, Default)]
pub(super) struct RequirementArena {
    nodes: Vec<RequirementNode>,
    interned: FxHashMap<RequirementKind, usize>,
    exhausted: bool,
    semantic_ready: bool,
}

impl RequirementArena {
    fn metadata(&self, requirements: BindingRequirements) -> (usize, u64, u64) {
        match requirements {
            BindingRequirements::Empty => (0, 0, 1),
            BindingRequirements::Node(id) => {
                let node = &self.nodes[id];
                (node.length, node.hash, node.factor)
            }
            BindingRequirements::Refused => (usize::MAX, 0, 0),
        }
    }

    fn admitted(&self, requirements: BindingRequirements) -> bool {
        match requirements {
            BindingRequirements::Empty => true,
            BindingRequirements::Node(id) => self.semantic_ready && self.nodes[id].admitted,
            BindingRequirements::Refused => false,
        }
    }

    fn evaluate(&self, kind: &RequirementKind, references: &FxHashMap<Span, Span>) -> bool {
        match kind {
            RequirementKind::Append(parent, (reference, expected)) => {
                self.admitted(*parent) && references.get(reference) == Some(expected)
            }
            RequirementKind::Concat(left, right) => self.admitted(*left) && self.admitted(*right),
        }
    }

    fn install_map(&mut self, references: &FxHashMap<Span, Span>) {
        self.semantic_ready = true;
        for id in 0..self.nodes.len() {
            self.nodes[id].admitted = self.evaluate(&self.nodes[id].kind, references);
        }
    }

    fn reserve(&mut self, nodes: usize) -> bool {
        if self.exhausted || nodes > MAX_BINDING_REQUIREMENT_NODES.saturating_sub(self.nodes.len())
        {
            self.exhausted = true;
            return false;
        }
        true
    }

    fn push(
        &mut self,
        kind: RequirementKind,
        metadata: (usize, u64, u64),
        references: &FxHashMap<Span, Span>,
    ) -> BindingRequirements {
        let admitted = self.semantic_ready && self.evaluate(&kind, references);
        let id = self.nodes.len();
        self.interned.insert(kind, id);
        self.nodes.push(RequirementNode {
            kind,
            length: metadata.0,
            hash: metadata.1,
            factor: metadata.2,
            admitted,
        });
        BindingRequirements::Node(id)
    }

    fn append(
        &mut self,
        parent: BindingRequirements,
        condition: Condition,
        references: &FxHashMap<Span, Span>,
    ) -> BindingRequirements {
        let kind = RequirementKind::Append(parent, condition);
        if let Some(id) = self.interned.get(&kind) {
            return BindingRequirements::Node(*id);
        }
        let (length, hash, factor) = self.metadata(parent);
        let Some(length) = length.checked_add(1) else {
            self.exhausted = true;
            return BindingRequirements::Refused;
        };
        if !self.reserve(1) {
            return BindingRequirements::Refused;
        }
        let mut hasher = FxHasher::default();
        condition.hash(&mut hasher);
        self.push(
            RequirementKind::Append(parent, condition),
            (
                length,
                hash.wrapping_mul(SEQUENCE_BASE)
                    .wrapping_add(hasher.finish()),
                factor.wrapping_mul(SEQUENCE_BASE),
            ),
            references,
        )
    }

    fn concat(
        &mut self,
        left: BindingRequirements,
        right: BindingRequirements,
        references: &FxHashMap<Span, Span>,
    ) -> BindingRequirements {
        if left == BindingRequirements::Refused || right == BindingRequirements::Refused {
            return BindingRequirements::Refused;
        }
        if left == BindingRequirements::Empty {
            return right;
        }
        if right == BindingRequirements::Empty {
            return left;
        }
        let kind = RequirementKind::Concat(left, right);
        if let Some(id) = self.interned.get(&kind) {
            return BindingRequirements::Node(*id);
        }
        let (left_len, left_hash, left_factor) = self.metadata(left);
        let (right_len, right_hash, right_factor) = self.metadata(right);
        let Some(length) = left_len.checked_add(right_len) else {
            self.exhausted = true;
            return BindingRequirements::Refused;
        };
        if !self.reserve(1) {
            return BindingRequirements::Refused;
        }
        self.push(
            RequirementKind::Concat(left, right),
            (
                length,
                left_hash
                    .wrapping_mul(right_factor)
                    .wrapping_add(right_hash),
                left_factor.wrapping_mul(right_factor),
            ),
            references,
        )
    }

    fn planned_node(&self, kind: RequirementKind, allocated: &mut usize) -> BindingRequirements {
        if let Some(id) = self.interned.get(&kind) {
            return BindingRequirements::Node(*id);
        }
        let id = self.nodes.len() + *allocated;
        *allocated += 1;
        BindingRequirements::Node(id)
    }

    fn planned_concat(
        &self,
        left: BindingRequirements,
        right: BindingRequirements,
        allocated: &mut usize,
    ) -> BindingRequirements {
        if left == BindingRequirements::Empty {
            return right;
        }
        if right == BindingRequirements::Empty {
            return left;
        }
        self.planned_node(RequirementKind::Concat(left, right), allocated)
    }

    fn combine(
        &mut self,
        handles: &[BindingRequirements],
        conditions: &[Condition],
        following: BindingRequirements,
        references: &FxHashMap<Span, Span>,
    ) -> BindingRequirements {
        if handles.len().saturating_add(conditions.len()) > MAX_BINDING_REQUIREMENT_NODES {
            self.exhausted = true;
            return BindingRequirements::Refused;
        }
        let mut length = conditions.len();
        for handle in handles.iter().chain(std::iter::once(&following)) {
            if *handle == BindingRequirements::Refused {
                return BindingRequirements::Refused;
            }
            let Some(next) = length.checked_add(self.metadata(*handle).0) else {
                self.exhausted = true;
                return BindingRequirements::Refused;
            };
            length = next;
        }
        // Each nonempty step grows the ordered prefix, so newly planned keys cannot
        // repeat within this operation. Prospective IDs require no scratch allocation.
        let mut allocated = 0;
        let mut planned = BindingRequirements::Empty;
        for handle in handles {
            planned = self.planned_concat(planned, *handle, &mut allocated);
        }
        for condition in conditions {
            planned =
                self.planned_node(RequirementKind::Append(planned, *condition), &mut allocated);
        }
        self.planned_concat(planned, following, &mut allocated);
        if allocated > 0 && !self.reserve(allocated) {
            return BindingRequirements::Refused;
        }
        let mut result = BindingRequirements::Empty;
        for handle in handles {
            result = self.concat(result, *handle, references);
        }
        for condition in conditions {
            result = self.append(result, *condition, references);
        }
        self.concat(result, following, references)
    }

    fn next_condition(
        &self,
        stack: &mut Vec<RequirementStep>,
        remaining: &mut usize,
    ) -> Result<Option<Condition>, ()> {
        while let Some(step) = stack.pop() {
            *remaining = remaining.checked_sub(1).ok_or(())?;
            match step {
                RequirementStep::Condition(condition) => return Ok(Some(condition)),
                RequirementStep::Handle(BindingRequirements::Empty) => {}
                RequirementStep::Handle(BindingRequirements::Refused) => return Err(()),
                RequirementStep::Handle(BindingRequirements::Node(id)) => match self.nodes[id].kind
                {
                    RequirementKind::Append(parent, condition) => {
                        stack.push(RequirementStep::Condition(condition));
                        stack.push(RequirementStep::Handle(parent));
                    }
                    RequirementKind::Concat(left, right) => {
                        stack.push(RequirementStep::Handle(right));
                        stack.push(RequirementStep::Handle(left));
                    }
                },
            }
        }
        Ok(None)
    }

    fn equal(
        &self,
        left: BindingRequirements,
        right: BindingRequirements,
        remaining: &mut usize,
    ) -> Result<bool, ()> {
        *remaining = remaining.checked_sub(1).ok_or(())?;
        if left == BindingRequirements::Refused || right == BindingRequirements::Refused {
            return Err(());
        }
        if left == right {
            return Ok(true);
        }
        if self.metadata(left) != self.metadata(right) {
            return Ok(false);
        }
        let mut left_stack = vec![RequirementStep::Handle(left)];
        let mut right_stack = vec![RequirementStep::Handle(right)];
        loop {
            let left = self.next_condition(&mut left_stack, remaining)?;
            let right = self.next_condition(&mut right_stack, remaining)?;
            if left != right {
                return Ok(false);
            }
            if left.is_none() {
                return Ok(true);
            }
        }
    }
}

enum RequirementStep {
    Handle(BindingRequirements),
    Condition(Condition),
}

impl ModuleInfoExtractor {
    pub(super) fn requirement_len(&self, requirements: BindingRequirements) -> usize {
        self.binding_requirements.borrow().metadata(requirements).0
    }

    pub(super) fn append_requirement(
        &self,
        parent: BindingRequirements,
        condition: Condition,
    ) -> BindingRequirements {
        let mut arena = self.binding_requirements.borrow_mut();
        let result = arena.append(parent, condition, &self.subclass_binding_references);
        if arena.exhausted {
            self.projection_exhausted.set(true);
        }
        result
    }

    pub(super) fn concat_requirements(
        &self,
        left: BindingRequirements,
        right: BindingRequirements,
    ) -> BindingRequirements {
        let mut arena = self.binding_requirements.borrow_mut();
        let result = arena.concat(left, right, &self.subclass_binding_references);
        if arena.exhausted {
            self.projection_exhausted.set(true);
        }
        result
    }

    pub(super) fn combine_requirements(
        &self,
        handles: &[BindingRequirements],
        conditions: &[Condition],
    ) -> BindingRequirements {
        let mut arena = self.binding_requirements.borrow_mut();
        let result = arena.combine(
            handles,
            conditions,
            BindingRequirements::Empty,
            &self.subclass_binding_references,
        );
        if arena.exhausted {
            self.projection_exhausted.set(true);
        }
        result
    }

    pub(super) fn import_requirements(&self, conditions: &[Condition]) -> BindingRequirements {
        self.combine_requirements(&[], conditions)
    }

    pub(super) fn append_then_concat_requirements(
        &self,
        prefix: BindingRequirements,
        conditions: &[Condition],
        following: BindingRequirements,
    ) -> BindingRequirements {
        let mut arena = self.binding_requirements.borrow_mut();
        let result = arena.combine(
            &[prefix],
            conditions,
            following,
            &self.subclass_binding_references,
        );
        if arena.exhausted {
            self.projection_exhausted.set(true);
        }
        result
    }

    pub(super) fn import_requirements_before(
        &self,
        conditions: &[Condition],
        following: BindingRequirements,
    ) -> BindingRequirements {
        self.append_then_concat_requirements(BindingRequirements::Empty, conditions, following)
    }

    pub(super) fn admitted_requirements(&self, requirements: BindingRequirements) -> bool {
        self.binding_requirements.borrow().admitted(requirements)
    }

    pub(super) fn requirement_reference_candidates(&self) -> FxHashSet<Span> {
        self.binding_requirements
            .borrow()
            .nodes
            .iter()
            .filter_map(|node| match node.kind {
                RequirementKind::Append(_, (reference, _)) => Some(reference),
                RequirementKind::Concat(_, _) => None,
            })
            .collect()
    }

    pub(super) fn install_requirement_provenance(&self) {
        self.binding_requirements
            .borrow_mut()
            .install_map(&self.subclass_binding_references);
    }

    pub(super) fn requirements_equal(
        &self,
        left: BindingRequirements,
        right: BindingRequirements,
        remaining: &mut usize,
    ) -> Result<bool, ()> {
        self.binding_requirements
            .borrow()
            .equal(left, right, remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conditions() -> (Condition, Condition, FxHashMap<Span, Span>) {
        let first = (Span::new(1, 2), Span::new(3, 4));
        let second = (Span::new(5, 6), Span::new(7, 8));
        (first, second, FxHashMap::from_iter([first, second]))
    }

    #[test]
    fn ordered_requirements_preserve_grouping_and_multiplicity() {
        let (first, second, map) = conditions();
        let mut arena = RequirementArena::default();
        let left = arena.append(BindingRequirements::Empty, first, &map);
        let right = arena.append(BindingRequirements::Empty, second, &map);
        let joined = arena.concat(left, right, &map);
        let appended = arena.append(left, second, &map);
        assert_eq!(arena.equal(joined, appended, &mut 64), Ok(true));
        let reversed = arena.concat(right, left, &map);
        assert_eq!(arena.equal(joined, reversed, &mut 64), Ok(false));
        let doubled = arena.concat(left, left, &map);
        assert_eq!(arena.equal(left, doubled, &mut 64), Ok(false));
        assert_eq!(arena.equal(joined, appended, &mut 1), Err(()));
    }

    #[test]
    fn late_conditions_use_installed_and_replaced_semantic_provenance() {
        let (first, second, map) = conditions();
        let mut arena = RequirementArena::default();
        let first_handle = arena.append(BindingRequirements::Empty, first, &map);
        assert!(!arena.admitted(first_handle));
        arena.install_map(&map);
        assert!(arena.admitted(first_handle));
        let late = arena.append(first_handle, second, &map);
        assert!(arena.admitted(late));
        let replaced = FxHashMap::from_iter([first]);
        arena.install_map(&replaced);
        assert!(arena.admitted(first_handle));
        assert!(!arena.admitted(late));
        arena.install_map(&map);
        assert!(arena.admitted(late));
    }

    #[test]
    fn condition_refusal_preserves_complete_observations_and_never_admits_partial_suffix() {
        let (first, second, map) = conditions();
        let mut arena = RequirementArena::default();
        arena.install_map(&map);
        let saved = arena.append(BindingRequirements::Empty, first, &map);
        let mut chain = saved;
        for _ in 1..MAX_BINDING_REQUIREMENT_NODES {
            chain = arena.append(chain, first, &map);
        }
        assert!(arena.admitted(chain));
        let refused = arena.append(chain, second, &map);
        assert_eq!(refused, BindingRequirements::Refused);
        assert!(!arena.admitted(refused));
        assert!(arena.admitted(saved));
        assert_eq!(arena.concat(saved, BindingRequirements::Empty, &map), saved);
        assert_eq!(arena.equal(saved, saved, &mut 1), Ok(true));
    }

    #[test]
    fn repeated_conjunction_comparison_refuses_without_expansion() {
        let (first, _, map) = conditions();
        let mut arena = RequirementArena::default();
        let single = arena.append(BindingRequirements::Empty, first, &map);
        let mut left = arena.append(single, first, &map);
        let mut right = arena.concat(single, single, &map);
        for _ in 0..32 {
            left = arena.concat(left, left, &map);
            right = arena.concat(right, right, &map);
        }
        assert_ne!(left, right);
        assert_eq!(arena.equal(left, left, &mut 1), Ok(true));
        assert_eq!(arena.equal(left, right, &mut 128), Err(()));
        arena.install_map(&map);
        assert!(arena.admitted(left));
        assert!(arena.admitted(right));
    }
    #[test]
    fn complete_compositions_reuse_conditions_after_capacity_refusal() {
        let (first, second, map) = conditions();
        let mut arena = RequirementArena::default();
        arena.install_map(&map);
        let first_handle = arena.append(BindingRequirements::Empty, first, &map);
        let second_handle = arena.append(BindingRequirements::Empty, second, &map);
        let original = arena.combine(&[first_handle], &[second], second_handle, &map);
        let mut pressure = second_handle;
        for _ in 0..MAX_BINDING_REQUIREMENT_NODES {
            pressure = arena.append(pressure, first, &map);
        }
        assert_eq!(pressure, BindingRequirements::Refused);
        let repeated = arena.combine(&[first_handle], &[second], second_handle, &map);
        assert_eq!(arena.equal(original, repeated, &mut 1), Ok(true));
        assert!(arena.admitted(repeated));
        let rejected = arena.combine(&[second_handle], &[second], first_handle, &map);
        assert!(!arena.admitted(rejected));
    }

    #[test]
    fn rejected_multi_node_conversion_never_installs_its_prefix() {
        let (first, second, map) = conditions();
        let mut arena = RequirementArena::default();
        arena.install_map(&map);
        let mut prefix = BindingRequirements::Empty;
        for _ in 0..MAX_BINDING_REQUIREMENT_NODES - 1 {
            prefix = arena.append(prefix, first, &map);
        }
        let rejected = arena.combine(
            &[prefix],
            &[second, first],
            BindingRequirements::Empty,
            &map,
        );
        assert!(!arena.admitted(rejected));
        // A partially installed first append would be reusable after refusal.
        let partial = arena.append(prefix, second, &map);
        assert_eq!(partial, BindingRequirements::Refused);
        assert!(arena.admitted(prefix));
    }
}
