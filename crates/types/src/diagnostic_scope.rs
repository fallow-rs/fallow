//! A per-thread scope for an analysis whose diagnostics must not reach the
//! report of the run (issue #5251).

use std::cell::Cell;

thread_local! {
    static SCRATCH_RUN: Cell<bool> = const { Cell::new(false) };
}

/// Whether the current thread runs inside [`with_scratch_diagnostics`].
#[must_use]
pub fn is_scratch_run() -> bool {
    SCRATCH_RUN.with(Cell::get)
}

/// Run `body` so that the workspace diagnostics it records stay out of the
/// process-wide registry and off stderr. The recorders still return their
/// diagnostics to the caller.
///
/// A pass that runs a second analysis of the same root for its own facts
/// (the test-side import scan of a production run) uses this, so that the
/// second analysis adds nothing to the report of the first. The flag is per
/// thread: run `body` on a thread that executes no other work, such as a
/// scoped thread that is not a worker of a thread pool. The flag does not
/// follow work that `body` hands to pool workers, so a recorder must run on
/// the calling thread to stay out of the registry.
pub fn with_scratch_diagnostics<R>(body: impl FnOnce() -> R) -> R {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            SCRATCH_RUN.with(|flag| flag.set(self.0));
        }
    }
    let _reset = Reset(SCRATCH_RUN.with(|flag| flag.replace(true)));
    body()
}

#[cfg(test)]
mod tests {
    use super::{is_scratch_run, with_scratch_diagnostics};

    #[test]
    fn the_flag_holds_inside_the_scope_and_resets_after() {
        assert!(!is_scratch_run());
        with_scratch_diagnostics(|| {
            assert!(is_scratch_run());
            with_scratch_diagnostics(|| assert!(is_scratch_run()));
            assert!(is_scratch_run(), "a nested scope keeps the outer flag");
        });
        assert!(!is_scratch_run());
    }

    #[test]
    fn the_flag_resets_after_a_panic() {
        let result = std::panic::catch_unwind(|| with_scratch_diagnostics(|| panic!("scan")));
        assert!(result.is_err());
        assert!(!is_scratch_run());
    }
}
