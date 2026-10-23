//! Meters for the work that extraction does on a thread, read by tests.
//!
//! A walk to the root at each link of a chain costs O(N^2) for N links, and so
//! does a path that is built again at each link. A test that extracts a chain
//! of N and of 4N links can compare the two meters and see that the work grows
//! linearly. The meters count work, not time, so the check is deterministic.
//!
//! - [`walk_step`] counts one step of a walk over the links of a chain. Each
//!   loop that walks a chain toward its root, or that scans a list that grows
//!   with a chain, calls it once for each step.
//! - The test allocator counts the bytes that each thread allocates, so a
//!   string or a fact that grows with the chain at each link shows too.
//!
//! In a build without `cfg(test)`, [`walk_step`] is empty.

/// Count one step of a walk over the links of a chain.
#[inline]
pub fn walk_step() {
    #[cfg(test)]
    meter::WALK_STEPS.with(|steps| steps.set(steps.get() + 1));
}

#[cfg(test)]
pub use meter::{Work, measure};

#[cfg(test)]
mod meter {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        pub(super) static WALK_STEPS: Cell<usize> = const { Cell::new(0) };
        static ALLOCATED_BYTES: Cell<usize> = const { Cell::new(0) };
    }

    /// The work that a closure did on the current thread.
    #[derive(Debug, Clone, Copy)]
    pub struct Work {
        pub walk_steps: usize,
        pub allocated_bytes: usize,
    }

    /// Run `run` and return its result and the work it did on this thread.
    /// Work on other threads does not count.
    pub fn measure<R>(run: impl FnOnce() -> R) -> (R, Work) {
        let steps = WALK_STEPS.with(Cell::get);
        let bytes = ALLOCATED_BYTES.with(Cell::get);
        let result = run();
        let work = Work {
            walk_steps: WALK_STEPS.with(Cell::get) - steps,
            allocated_bytes: ALLOCATED_BYTES.with(Cell::get) - bytes,
        };
        (result, work)
    }

    fn count(bytes: usize) {
        // A thread that is in teardown has no meter, and its work does not count.
        let _ = ALLOCATED_BYTES.try_with(|allocated| allocated.set(allocated.get() + bytes));
    }

    /// The system allocator, with a count of the bytes that each thread asks for.
    struct MeteredAllocator;

    #[expect(unsafe_code, reason = "a global allocator implements an unsafe trait")]
    // SAFETY: each method forwards to `System` with the same arguments, so the
    // contract of `GlobalAlloc` holds. The count is a thread-local `Cell` with a
    // const initializer and no destructor, so it does not allocate.
    unsafe impl GlobalAlloc for MeteredAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count(layout.size());
            // SAFETY: the caller upholds the contract of `GlobalAlloc::alloc`.
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            count(layout.size());
            // SAFETY: the caller upholds the contract of `GlobalAlloc::alloc_zeroed`.
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: the caller upholds the contract of `GlobalAlloc::dealloc`.
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            count(new_size.saturating_sub(layout.size()));
            // SAFETY: the caller upholds the contract of `GlobalAlloc::realloc`.
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: MeteredAllocator = MeteredAllocator;
}
