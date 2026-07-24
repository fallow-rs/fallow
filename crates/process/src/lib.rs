//! Shared subprocess lifecycle management for Fallow.
//!
//! Protocol adapters and analysis facades use the same registered child
//! handles and operating-system process-tree primitives. This keeps timeout
//! cleanup and signal cleanup identical without introducing dependencies on
//! the CLI crate.
//!
//! The optional `tokio` feature adds async command setup and bounded cleanup
//! while preserving the same operating-system tree owner.

mod process_tree;
mod registry;
mod scoped_child;

pub use process_tree::{ChildCleanup, ProcessTree, cleanup_std_child, configure_std_command};
#[cfg(feature = "tokio")]
pub use process_tree::{cleanup_tokio_child, configure_tokio_command};
pub use scoped_child::{ProcessTreeTerminator, ScopedChild, output, status};

/// Terminate every registered child or process tree and wait for bounded
/// cleanup.
pub fn drain_and_kill() {
    registry::drain_and_kill();
}
