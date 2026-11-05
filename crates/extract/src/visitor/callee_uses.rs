//! Records the static callee paths that `banned-call`, `banned-effect` and
//! `boundaries.calls.forbidden` match.

use std::collections::hash_map::Entry;

use fallow_types::extract::CalleeUse;
use oxc_ast::ast::Expression;

use super::ModuleInfoExtractor;
use super::visit_impl::flatten_callee_path;

impl ModuleInfoExtractor {
    /// Record the statically flattenable callee path of a call or `new` site. Each site is
    /// kept, grouped under one entry per path and form, so a suppression comment covers one
    /// site only. Capture is unconditional because extraction is config-blind. Computed
    /// members, dynamic dispatch, and optional-chaining callees flatten to `None` and stay
    /// uncaptured (documented false negatives).
    pub(super) fn record_callee_use(
        &mut self,
        callee: &Expression<'_>,
        span_start: u32,
        is_new: bool,
    ) {
        let Some(callee_path) = flatten_callee_path(callee) else {
            return;
        };
        match self.callee_use_index.entry((callee_path, is_new)) {
            Entry::Occupied(entry) => self.callee_uses[*entry.get()].span_starts.push(span_start),
            Entry::Vacant(entry) => {
                let callee_path = entry.key().0.clone();
                entry.insert(self.callee_uses.len());
                self.callee_uses.push(CalleeUse {
                    callee_path,
                    span_starts: vec![span_start],
                    is_new,
                });
            }
        }
    }
}
