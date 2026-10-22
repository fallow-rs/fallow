//! Per-thread count of the source bytes that the CSS comment mask read.

use std::cell::Cell;

thread_local! {
    /// Source bytes that the CSS comment mask read on this thread since the
    /// last [`take`]. The parse of one file runs on one thread, so it reads
    /// its own count.
    static COMMENT_MASKED_BYTES: Cell<u64> = const { Cell::new(0) };
}

/// Return the count of this thread and reset it.
pub fn take() -> u64 {
    COMMENT_MASKED_BYTES.with(|masked| masked.replace(0))
}

/// Add `bytes` to the count of this thread.
pub fn add(bytes: u64) {
    COMMENT_MASKED_BYTES.with(|masked| masked.set(masked.get() + bytes));
}
