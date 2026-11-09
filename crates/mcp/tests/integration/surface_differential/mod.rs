//! Surface differential: the CLI, the MCP server, the language server and
//! the Node bindings give the same facts for one project.
//!
//! Each surface has its own wire format, so the oracle compares normalized
//! facts (`facts.rs`) and not the bytes. Each surface takes its real path
//! (`surfaces.rs`). The checks are:
//!
//! - the dead-code findings, with paths, lines, columns and finding ids,
//!   agree on each surface, and the editor diagnostics sit at the same
//!   positions with the same ids;
//! - the clone groups and families, in report order, agree on each surface
//!   and on each scope that covers every file;
//! - `trace` for a dependency or an export agrees with the report about it,
//!   and the trace answers agree on each surface;
//! - `FALLOW_CACHE_DIR` and `FALLOW_CACHE_MAX_SIZE` have the same effect on
//!   each surface (`cache_env.rs`).
//!
//! Mutation testing is out of scope, because MCP does not run mutants.
//!
//! The Node bindings run only when `FALLOW_SURFACE_NODE_ADDON` names the
//! `crates/napi` directory of a build. Without it, the oracle calls the
//! `fallow_api` runners and serializers that the bindings call.
//!
//! Pull request CI runs three fixed project seeds. The daily `Oracle Seeds`
//! workflow runs `seed_sweep_agrees_on_every_surface` over
//! `FALLOW_SURFACE_SEEDS` project seeds from `FALLOW_SURFACE_SEED_START`.
//!
//! The tests need the `fallow` and `fallow-lsp` binaries of the same build:
//! `cargo build -p fallow-cli -p fallow-lsp`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

mod cache_env;
mod facts;
mod oracle;
mod project;
mod surfaces;
mod templates;
