//! Module graph construction and import resolution for fallow codebase intelligence.
//!
//! This crate builds the dependency graph from parsed modules, resolves import
//! specifiers to their targets, and tracks export usage through re-export chains.

#![warn(missing_docs)]
// fallow's analysis never executes the analyzed project's code, and this crate
// spawns no external process at all. The deny (paired with the `.clippy.toml`
// ban on `std::process::Command::new`) keeps it that way: any future process
// spawn here fails the build. Test helpers are exempt via `not(test)`.
#![cfg_attr(not(test), deny(clippy::disallowed_methods))]

pub mod graph;
pub mod project;
pub mod resolve;
