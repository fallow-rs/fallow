//! Deterministic work counts for one import-resolution run.
//!
//! Resolution runs one module per rayon task, and a module never spawns nested
//! rayon work, so each module's counts live in a thread-local scope while the
//! module resolves. The scope closes with the module and its counts travel back
//! in the module's output, where the sequential merge sums them. The hot path
//! therefore never touches a shared atomic.
//!
//! The counts are exact for a given project and commit. They do not depend on
//! the thread count or on scheduling, which makes them usable as a regression
//! metric where wall-clock time is too noisy.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::ops::AddAssign;

use rustc_hash::{FxHashSet, FxHasher};

/// Work counts for import resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResolveWork {
    /// Specifier resolutions that the import sites asked for: one for each
    /// static import binding, re-export, `require()`, `import()` and module
    /// mock. Internal retries inside the resolver are not counted here.
    pub specifier_calls: u64,
    /// Distinct `(specifier, from_style)` pairs for each importing file,
    /// summed over all files. `specifier_calls / unique_specifiers` above 1.0
    /// shows bindings that share a specifier. The resolver runs at most once
    /// for each of these pairs. A pair that returns before the resolver, such
    /// as an external URL, makes no resolver call.
    pub unique_specifiers: u64,
    /// Calls into `oxc_resolver`, including fallback retries.
    pub oxc_resolve_calls: u64,
    /// Path canonicalize calls that the run needs: each direct call, plus one
    /// for each distinct path that goes through the canonicalize cache.
    pub canonicalize_calls: u64,
}

impl AddAssign for ResolveWork {
    fn add_assign(&mut self, other: Self) {
        self.specifier_calls += other.specifier_calls;
        self.unique_specifiers += other.unique_specifiers;
        self.oxc_resolve_calls += other.oxc_resolve_calls;
        self.canonicalize_calls += other.canonicalize_calls;
    }
}

struct ModuleScope {
    work: ResolveWork,
    seen_specifiers: FxHashSet<u64>,
}

thread_local! {
    static SCOPE: RefCell<Option<ModuleScope>> = const { RefCell::new(None) };
    /// The specifier set of the last closed scope, kept so that the next
    /// module on this worker reuses its capacity instead of allocating.
    static SPARE_SET: RefCell<FxHashSet<u64>> = RefCell::new(FxHashSet::default());
}

/// Restores the previous scope when the module scope ends, also on unwind.
struct ScopeGuard {
    previous: Option<ModuleScope>,
    restored: bool,
}

impl ScopeGuard {
    fn finish(mut self) -> ResolveWork {
        self.restored = true;
        let previous = self.previous.take();
        let Some(closed) = SCOPE.with(|scope| scope.replace(previous)) else {
            return ResolveWork::default();
        };
        let mut seen = closed.seen_specifiers;
        seen.clear();
        SPARE_SET.with(|spare| *spare.borrow_mut() = seen);
        closed.work
    }
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        if !self.restored {
            let previous = self.previous.take();
            SCOPE.with(|scope| scope.replace(previous));
        }
    }
}

/// Run `resolve` inside a fresh work scope and return its counts.
pub(super) fn in_module_scope<T>(resolve: impl FnOnce() -> T) -> (T, ResolveWork) {
    let fresh = ModuleScope {
        work: ResolveWork::default(),
        seen_specifiers: SPARE_SET.with(|spare| std::mem::take(&mut *spare.borrow_mut())),
    };
    let guard = ScopeGuard {
        previous: SCOPE.with(|scope| scope.replace(Some(fresh))),
        restored: false,
    };
    let output = resolve();
    (output, guard.finish())
}

fn with_scope(record: impl FnOnce(&mut ModuleScope)) {
    SCOPE.with(|scope| {
        if let Some(scope) = scope.borrow_mut().as_mut() {
            record(scope);
        }
    });
}

/// Record one specifier resolution that an import site asked for.
pub(super) fn note_specifier(specifier: &str, from_style: bool) {
    with_scope(|scope| {
        scope.work.specifier_calls += 1;
        let mut hasher = FxHasher::default();
        specifier.hash(&mut hasher);
        from_style.hash(&mut hasher);
        if scope.seen_specifiers.insert(hasher.finish()) {
            scope.work.unique_specifiers += 1;
        }
    });
}

/// Record one call into `oxc_resolver`.
pub(super) fn note_oxc_resolve() {
    with_scope(|scope| scope.work.oxc_resolve_calls += 1);
}

/// Record `count` direct path canonicalize calls.
pub(super) fn note_canonicalize(count: u64) {
    with_scope(|scope| scope.work.canonicalize_calls += count);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_scope_counts_calls_and_distinct_specifiers() {
        let ((), work) = in_module_scope(|| {
            note_specifier("./a", false);
            note_specifier("./a", false);
            note_specifier("./a", true);
            note_oxc_resolve();
            note_canonicalize(2);
        });
        assert_eq!(
            work,
            ResolveWork {
                specifier_calls: 3,
                unique_specifiers: 2,
                oxc_resolve_calls: 1,
                canonicalize_calls: 2,
            }
        );
    }

    #[test]
    fn notes_outside_a_scope_are_dropped() {
        note_specifier("./a", false);
        let ((), work) = in_module_scope(|| note_specifier("./b", false));
        assert_eq!(work.specifier_calls, 1);
    }

    #[test]
    fn each_scope_starts_with_an_empty_specifier_set() {
        let ((), first) = in_module_scope(|| note_specifier("./a", false));
        let ((), second) = in_module_scope(|| note_specifier("./a", false));
        assert_eq!(first.unique_specifiers, 1);
        assert_eq!(second.unique_specifiers, 1);
    }

    #[test]
    fn a_nested_scope_keeps_its_counts_apart_from_the_outer_scope() {
        let (inner, outer) = in_module_scope(|| {
            note_specifier("./outer", false);
            let ((), inner) = in_module_scope(|| note_specifier("./inner", false));
            note_specifier("./outer-2", false);
            inner
        });
        assert_eq!(inner.specifier_calls, 1);
        assert_eq!(outer.specifier_calls, 2);
    }
}
