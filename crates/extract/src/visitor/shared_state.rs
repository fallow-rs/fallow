//! Preserve shared-state fact provenance across embedded script merges.
use super::{ModuleInfo, ModuleInfoExtractor};
use fallow_types::extract::SemanticFact;
use fallow_types::shared_state::{StateAccessFact, StateRoot};
use oxc_span::Span;
impl ModuleInfoExtractor {
    /// Append this script's import binding references. Call it before the
    /// imports are appended: each `import_index` moves by the number of
    /// imports that `info` already holds.
    pub(super) fn merge_import_binding_references(&mut self, info: &mut ModuleInfo) {
        if self.import_binding_references.is_empty() {
            return;
        }
        let base = u32::try_from(info.imports.len()).unwrap_or(u32::MAX);
        let mut references = std::mem::take(&mut info.import_binding_references).to_vec();
        references.extend(
            self.import_binding_references
                .drain(..)
                .map(|mut reference| {
                    reference.import_index = reference.import_index.saturating_add(base);
                    reference
                }),
        );
        references.sort_by_key(|reference| reference.span_start);
        info.import_binding_references = references.into();
    }

    pub(crate) fn push_shared_state(&mut self, mut facts: Vec<StateAccessFact>) {
        if facts.len() > crate::shared_state::MAX_STATE_FACTS {
            facts.truncate(crate::shared_state::MAX_STATE_FACTS);
            self.semantic_facts.push(SemanticFact::SharedStateTruncated);
        }
        self.semantic_facts
            .extend(facts.into_iter().map(SemanticFact::SharedState));
    }
    pub(crate) fn append_shared_state_imports(&mut self, mut other: Self) {
        other.rebase_shared_state(self.imports.len() as u32);
        self.imports.append(&mut other.imports);
        self.semantic_facts
            .extend(other.semantic_facts.into_iter().filter(|fact| {
                matches!(
                    fact,
                    SemanticFact::SharedState(_) | SemanticFact::SharedStateTruncated
                )
            }));
        crate::shared_state::bound_merged_facts(&mut self.semantic_facts);
    }
    pub(super) fn rebase_shared_state(&mut self, offset: u32) {
        for fact in &mut self.semantic_facts {
            if let SemanticFact::SharedState(StateAccessFact {
                root: StateRoot::Import { import_index },
                ..
            }) = fact
            {
                *import_index += offset;
            }
        }
    }
    pub(super) fn remap_shared_state(&mut self, remap: &mut impl FnMut(Span) -> Span) {
        for fact in &mut self.semantic_facts {
            if let SemanticFact::SharedState(access) = fact {
                access.span_start =
                    remap(Span::new(access.span_start, access.span_start + 1)).start;
            }
        }
    }
}
