#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI binary produces intentional terminal output"
)]

#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod api;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
pub mod audit;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod baseline;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod check;
/// CODEOWNERS file parser and ownership lookup.
pub mod codeowners;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod combined;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod dupes;

/// Structured error output for CLI and JSON formats.
pub mod error;

#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod fix;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod init;

/// Metric and rule definitions for explainable CLI output.
pub mod explain;

#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod health;
/// Health / complexity analysis report types.
pub mod health_types;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod license;
/// Typed wrapper envelopes for duplication findings emitted by
/// `fallow dupes --format json`. Lives here (rather than in `fallow-types`)
/// because the bare findings live in `fallow-core` and `crates/cli/src/report/dupes_grouping.rs`.
pub mod output_dupes;

/// Typed envelope structs for the JSON output contract. Live here rather
/// than in `fallow-types` because the body fields reach into `fallow-core`
/// and into this crate's own `health_types`.
pub mod output_envelope;

/// Programmatic Rust API reused by the NAPI bindings.
pub mod programmatic;

/// Shared Rayon pool configuration for all embedded analysis entry points.
pub(crate) mod rayon_pool;

/// Regression detection: baseline comparison and tolerance checking.
pub mod regression;

/// Report formatting utilities for analysis results.
///
/// Exposed for snapshot testing of output formats.
pub mod report;

#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod runtime_support;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod validate;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod vital_signs;

pub use runtime_support::{AnalysisKind, GroupBy};
pub(crate) use runtime_support::{build_ownership_resolver, load_config_for_analysis};
