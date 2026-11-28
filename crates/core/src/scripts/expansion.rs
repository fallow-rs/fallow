//! Bookkeeping for script indirection: the cycle guard, the per-command
//! bounds, and the expansion budget that the commands of one catalog share.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The bytes of script text that the commands of one catalog may expand
/// together.
///
/// The per-command bounds restart for each command, and a shell helper or a
/// script list can hold many commands that call the same scripts with
/// arguments. A project with many such calls expands a few kilobytes in
/// total.
const MAX_EXPANDED_SCRIPT_BYTES: usize = 256 * 1024;

/// What is left of the expansion budget of a catalog. Clones share it.
///
/// When the budget is used up, later calls are not expanded, as with the
/// per-command bounds.
#[derive(Debug, Clone)]
pub(super) struct ExpansionBudget(Option<Arc<AtomicUsize>>);

impl Default for ExpansionBudget {
    fn default() -> Self {
        Self(Some(Arc::new(AtomicUsize::new(MAX_EXPANDED_SCRIPT_BYTES))))
    }
}

impl ExpansionBudget {
    /// A budget that allows no expansion, for a catalog without scripts.
    #[cfg(test)]
    pub(super) const NONE: Self = Self(None);

    /// Take `bytes` from the budget. Returns `false`, and takes nothing, when
    /// less is left.
    pub(super) fn take(&self, bytes: usize) -> bool {
        self.0.as_ref().is_some_and(|left| {
            left.try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(bytes)
            })
            .is_ok()
        })
    }
}

impl super::ScriptCatalog {
    /// Take `bytes` of expanded script text from the budget of this catalog.
    /// Returns `false`, and takes nothing, when less is left. A reader that
    /// expands script calls on its own takes from the same budget as the
    /// script parser.
    pub(crate) fn take_expansion(&self, bytes: usize) -> bool {
        self.expansion_budget.take(bytes)
    }
}

/// Bookkeeping for one top-level command while script indirection is followed.
pub(super) struct ScriptExpansion {
    /// Script names on the current expansion path, for the cycle guard.
    pub(super) active: Vec<String>,
    /// Bodies expanded so far, bounded by
    /// [`MAX_SCRIPT_EXPANSIONS`](super::MAX_SCRIPT_EXPANSIONS).
    pub(super) expansions: usize,
    /// `false` once a body from another workspace package has been entered.
    pub(super) local_paths: bool,
    /// Command substitutions on the current path, bounded by
    /// [`MAX_SUBSTITUTION_DEPTH`](super::MAX_SUBSTITUTION_DEPTH).
    pub(super) substitution_depth: usize,
    /// The budget of the catalog that the command belongs to.
    pub(super) budget: ExpansionBudget,
}

impl ScriptExpansion {
    pub(super) fn new(budget: &ExpansionBudget) -> Self {
        Self {
            active: Vec::new(),
            expansions: 0,
            local_paths: true,
            substitution_depth: 0,
            budget: budget.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ScriptCatalog, parse_script_with_catalog};

    #[test]
    #[expect(
        clippy::disallowed_types,
        reason = "ScriptCatalog takes the serde-deserialized std HashMap"
    )]
    fn commands_of_one_catalog_share_one_expansion_budget() {
        // Each call expands `fan` and its 50 calls of `leaf`: about 5,000
        // commands. Without a shared budget, 200 calls give about 1,000,000.
        let scripts = std::collections::HashMap::from([
            ("leaf".to_string(), "a;".repeat(100)),
            ("fan".to_string(), vec!["npm run leaf -- y"; 50].join(";")),
        ]);
        let catalog = ScriptCatalog::from_scripts(&scripts);
        let commands: usize = (0..200)
            .map(|index| {
                parse_script_with_catalog(&format!("npm run fan -- x{index}"), &catalog).len()
            })
            .sum();
        assert!(commands < 300_000, "{commands} commands");
        let first =
            parse_script_with_catalog("npm run leaf -- y", &ScriptCatalog::from_scripts(&scripts));
        assert_eq!(first.len(), 101);
    }
}
