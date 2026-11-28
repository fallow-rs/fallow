//! The allocator of the Code Mode JS engine: the Rust global allocator, with a
//! count of the engine bytes in use and of their peak on this thread.
//!
//! The snippet process reads the peak after each snippet. A snippet with a
//! large engine heap leaves pages in the allocator, so the process then asks
//! the server for a replacement. The count covers only the engine heap of this
//! thread, so the memory of the server or of the host calls does not change
//! the decision. The engine refuses an allocation over its memory limit before
//! it calls the allocator, so a refused allocation costs and counts nothing.

#![expect(
    unsafe_code,
    reason = "the engine allocator interface of rquickjs is an unsafe trait"
)]

use std::cell::Cell;

use rquickjs::Runtime;
use rquickjs::allocator::{Allocator, RustAllocator};

thread_local! {
    static IN_USE: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
}

/// A new engine runtime on [`CountingAllocator`].
pub(in super::super) fn new_engine_runtime() -> Result<Runtime, String> {
    Runtime::new_with_alloc(CountingAllocator)
        .map_err(|err| format!("failed to create JS runtime: {err}"))
}

/// The engine allocator. Each runtime of this thread shares the count.
pub(super) struct CountingAllocator;

impl CountingAllocator {
    fn add(bytes: usize) {
        let in_use = IN_USE.get().saturating_add(bytes);
        IN_USE.set(in_use);
        if in_use > PEAK.get() {
            PEAK.set(in_use);
        }
    }

    fn remove(bytes: usize) {
        IN_USE.set(IN_USE.get().saturating_sub(bytes));
    }

    /// Start a new peak at the bytes in use now.
    pub(super) fn reset_peak() {
        PEAK.set(IN_USE.get());
    }

    /// The most engine bytes in use since the last [`Self::reset_peak`].
    pub(super) fn peak() -> usize {
        PEAK.get()
    }

    #[cfg(test)]
    fn in_use() -> usize {
        IN_USE.get()
    }

    /// The size that `ptr` counts for: zero for a null pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be null or a live allocation of `RustAllocator`, which
    /// stores the size in front of each allocation.
    unsafe fn size_of(ptr: *mut u8) -> usize {
        if ptr.is_null() {
            return 0;
        }
        // SAFETY: `ptr` is not null, and the caller guarantees that it is a
        // live allocation of `RustAllocator`.
        unsafe { RustAllocator::usable_size(ptr) }
    }
}

// SAFETY: every call goes to `RustAllocator`, which meets the contract of
// `Allocator`. The count changes no pointer and no size.
unsafe impl Allocator for CountingAllocator {
    fn alloc(&mut self, size: usize) -> *mut u8 {
        let ptr = RustAllocator.alloc(size);
        // SAFETY: `ptr` is null or the new allocation of `RustAllocator`.
        Self::add(unsafe { Self::size_of(ptr) });
        ptr
    }

    fn calloc(&mut self, count: usize, size: usize) -> *mut u8 {
        let ptr = RustAllocator.calloc(count, size);
        // SAFETY: `ptr` is null or the new allocation of `RustAllocator`.
        Self::add(unsafe { Self::size_of(ptr) });
        ptr
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        // SAFETY: the caller passes a live pointer of this allocator, so it
        // came from `RustAllocator`.
        Self::remove(unsafe { Self::size_of(ptr) });
        // SAFETY: the caller passes a pointer of this allocator, so it came
        // from `RustAllocator`.
        unsafe { RustAllocator.dealloc(ptr) };
    }

    unsafe fn realloc(&mut self, ptr: *mut u8, new_size: usize) -> *mut u8 {
        // SAFETY: the caller passes a null pointer or a live pointer of this
        // allocator, so it came from `RustAllocator`.
        let old = unsafe { Self::size_of(ptr) };
        // SAFETY: the caller passes a pointer of this allocator, so it came
        // from `RustAllocator`.
        let new_ptr = unsafe { RustAllocator.realloc(ptr, new_size) };
        if !new_ptr.is_null() {
            Self::remove(old);
            // SAFETY: `new_ptr` is the live allocation that `RustAllocator`
            // just returned.
            Self::add(unsafe { Self::size_of(new_ptr) });
        }
        new_ptr
    }

    unsafe fn usable_size(ptr: *mut u8) -> usize {
        // SAFETY: the caller passes a pointer of this allocator, so it came
        // from `RustAllocator`.
        unsafe { RustAllocator::usable_size(ptr) }
    }
}

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    use super::CountingAllocator;

    #[test]
    fn the_count_follows_the_engine_heap() {
        let before = CountingAllocator::in_use();
        CountingAllocator::reset_peak();
        {
            let runtime = Runtime::new_with_alloc(CountingAllocator).expect("runtime");
            let context = Context::full(&runtime).expect("context");
            let length: usize = context
                .with(|ctx| ctx.eval("'x'.repeat(4 * 1024 * 1024).length"))
                .expect("eval");
            assert_eq!(length, 4 * 1024 * 1024);
        }
        assert!(
            CountingAllocator::peak() > before + 4 * 1024 * 1024,
            "the peak must hold the large string: {}",
            CountingAllocator::peak()
        );
        assert_eq!(
            CountingAllocator::in_use(),
            before,
            "the drop of the runtime must free each counted byte"
        );
    }
}
