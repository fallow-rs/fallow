//! Stack sizing for the parse and the walks of one user source.
//!
//! The Oxc parser and the Fallow AST walks recurse once per nesting level,
//! and a stack overflow aborts the process. Each nesting level uses at least
//! one source byte, and the stack use per source byte is bounded (see
//! `docs/reference/extract-internals.md`). So a stack sized from the source
//! length cannot overflow. A parse runs on the current thread when that
//! thread has enough stack left, and on a scoped thread of the needed size
//! otherwise.

use std::cell::Cell;

/// Stack bytes reserved per source byte.
///
/// A shape without brackets uses at most about 570 bytes per source byte in
/// a release build and about 1350 in a debug build. AddressSanitizer pads
/// every frame, so the fuzz build reserves 8 times the debug factor.
const STACK_PER_SOURCE_BYTE: usize = if cfg!(fuzzing) {
    8 * 2048
} else if cfg!(debug_assertions) {
    2048
} else {
    1024
};

/// Stack bytes reserved per level of bracket nesting, on top of the length
/// term. An unclosed `(` costs one source byte and about 1900 stack bytes per
/// level in a release build. The nesting guard caps the depth, so this term
/// adds at most a few MiB.
const STACK_PER_BRACKET_LEVEL: usize = if cfg!(fuzzing) { 8 * 4096 } else { 4096 };

/// Stack for the frames around the parse: the caller, the walks that do not
/// recurse with the source, and the thread start.
const BASE_STACK: usize = 256 * 1024;

/// Largest stack a parse asks for. Only a source of about a megabyte reaches
/// this cap.
const MAX_STACK: usize = 1 << 30;

thread_local! {
    /// Address near the top of this thread's stack and the stack size, when
    /// the thread was started with a known size.
    static KNOWN_STACK: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

/// Record that the current thread was started with `size` bytes of stack.
///
/// Call it first thing in the thread, for example from a Rayon
/// `start_handler`. A thread that never calls it counts as having no stack
/// to spare, so each bounded parse on it gets its own thread.
pub fn register_thread_stack(size: usize) {
    let top = stack_address();
    KNOWN_STACK.with(|known| known.set(Some((top, size))));
}

/// Stack that a parse of `source_len` bytes with `bracket_depth` nested
/// brackets can use.
pub fn stack_need(source_len: usize, bracket_depth: usize) -> usize {
    source_len
        .saturating_mul(STACK_PER_SOURCE_BYTE)
        .saturating_add(bracket_depth.saturating_mul(STACK_PER_BRACKET_LEVEL))
        .saturating_add(BASE_STACK)
        .min(MAX_STACK)
}

/// Run `run` with at least `need` bytes of stack.
///
/// When the sized thread cannot start, `run` runs on the current thread, as
/// it did before stack sizing existed.
pub fn run_with_stack<R: Send>(need: usize, run: impl FnOnce() -> R + Send) -> R {
    if remaining_stack() >= need {
        return run();
    }
    // The closure must survive a failed spawn, so the thread takes it from
    // a shared slot instead of by value.
    let slot = std::sync::Mutex::new(Some(run));
    let take = || {
        slot.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    };
    let spawned = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(need)
            .spawn_scoped(scope, || {
                register_thread_stack(need);
                take().map(|run| run())
            })
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
    });
    match spawned {
        Ok(Some(result)) => result,
        Ok(None) | Err(_) => match take() {
            Some(run) => run(),
            None => unreachable!("the spawned thread took the closure and returned its result"),
        },
    }
}

/// Stack left on the current thread, or 0 when its size is unknown.
fn remaining_stack() -> usize {
    KNOWN_STACK.with(Cell::get).map_or(0, |(top, size)| {
        // Every supported target grows the stack downward.
        let used = top.saturating_sub(stack_address());
        size.saturating_sub(used)
    })
}

#[inline(never)]
fn stack_address() -> usize {
    let marker = 0_u8;
    std::ptr::from_ref(std::hint::black_box(&marker)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE_STACK: usize = 4 << 20;

    fn on_registered_thread(probe: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(PROBE_STACK)
            .spawn(move || {
                register_thread_stack(PROBE_STACK);
                probe();
            })
            .expect("probe thread should start")
            .join()
            .expect("probe thread should finish");
    }

    #[test]
    fn need_grows_with_length_and_brackets() {
        assert_eq!(stack_need(0, 0), BASE_STACK);
        assert!(stack_need(1000, 0) > stack_need(100, 0));
        assert!(stack_need(1000, 500) > stack_need(1000, 0));
        assert_eq!(stack_need(usize::MAX, usize::MAX), MAX_STACK);
    }

    #[test]
    fn unknown_thread_has_no_stack_to_spare() {
        std::thread::spawn(|| assert_eq!(remaining_stack(), 0))
            .join()
            .expect("probe thread should finish");
    }

    #[test]
    fn registered_thread_reports_its_stack() {
        on_registered_thread(|| {
            let remaining = remaining_stack();
            assert!(remaining > PROBE_STACK - (1 << 20) && remaining <= PROBE_STACK);
        });
    }

    #[test]
    fn small_need_runs_on_the_current_thread() {
        on_registered_thread(|| {
            let caller = std::thread::current().id();
            assert_eq!(
                run_with_stack(1 << 20, || std::thread::current().id()),
                caller
            );
        });
    }

    #[test]
    fn large_need_runs_on_a_sized_thread() {
        let caller = std::thread::current().id();
        let (ran_on, remaining) = run_with_stack(64 << 20, || {
            (std::thread::current().id(), remaining_stack())
        });
        assert_ne!(ran_on, caller);
        assert!(remaining > 60 << 20);
    }
}
