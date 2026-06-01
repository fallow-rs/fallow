//! Module graph construction and import resolution for fallow codebase intelligence.
//!
//! This crate builds the dependency graph from parsed modules, resolves import
//! specifiers to their targets, and tracks export usage through re-export chains.

#![warn(missing_docs)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

pub mod graph;
pub mod project;
pub mod resolve;
