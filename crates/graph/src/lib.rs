//! Module graph construction and import resolution for fallow codebase intelligence.
//!
//! This crate builds the dependency graph from parsed modules, resolves import
//! specifiers to their targets, and tracks export usage through re-export chains.
//!
//! User documentation for module graph analysis is on [fallow.tools](https://fallow.tools/docs/explanations/dead-code/):
//!
//! - [How dead code detection works](https://fallow.tools/docs/explanations/dead-code/)
//! - [Architecture boundaries](https://fallow.tools/docs/analysis/boundaries/)
//! - [Workspaces](https://fallow.tools/docs/configuration/workspaces/)

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

pub mod cache;
pub mod graph;
pub mod installed_versions;
pub mod missed_probes;
pub mod project;
pub mod read_trace;
pub mod resolve;
