#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI binary produces intentional terminal output"
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
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
/// `fallow audit --brief` (alias `fallow review`): deterministic, always-exit-0
/// rendering mode layered over the audit analysis. Public so the schema-emit
/// binary can derive the `ReviewBriefOutput` envelope.
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
pub mod audit_brief;
/// Decision-surface extractor (stage 6 / 6.G): the apex of the review brief.
/// Public so the schema-emit binary can derive the `DecisionSurface` types.
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
pub mod audit_decision_surface;
/// Weighted focus map (stage 4): the composite attention score per review unit
/// plus the no-skip labels, confidence flags, and escape hatch. Public so the
/// schema-emit binary can derive the `FocusMap` types.
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
pub mod audit_focus;
/// Agent-contract loop (the codiff pattern, graph-extended): the walkthrough
/// guide (digest + schema + graph-snapshot pin) and the `--walkthrough-file`
/// post-validation against the live graph. Public so the schema-emit binary can
/// derive the guide + validation envelopes.
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
pub mod audit_walkthrough;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod base_worktree;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
pub mod walkthrough_state;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
use fallow_engine::baseline;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod cache_notice;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod check;
/// CODEOWNERS file parser and ownership lookup.
pub use fallow_engine::codeowners;
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
pub mod error {
    pub use fallow_engine::emit_error;
}

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
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod license;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse; the findings-present accumulator must be reachable from the lib-compiled analysis modules"
)]
mod telemetry;

/// Test-only coverage for the JSON output contract aliases.
#[cfg(test)]
mod architecture_boundaries;
#[cfg(test)]
pub mod output_envelope;
pub(crate) mod output_runtime;

/// Cross-platform path classification helpers (POSIX-style root + Windows
/// drive prefix detection that `Path::is_absolute()` misclassifies).
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse; `#[expect]` would be unfulfilled because the bin (not the lib) consumes these symbols"
)]
mod path_util;

/// Shared Rayon pool configuration for all embedded analysis entry points.
pub(crate) mod rayon_pool;

/// Regression detection: baseline comparison and tolerance checking.
pub mod regression;

/// Process-wide signal handling and scoped child-process registry.
/// See `signal/mod.rs` for the design rationale.
pub mod signal;

/// Report formatting utilities for analysis results.
///
/// Exposed for snapshot testing of output formats.
pub mod report;

#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
pub mod impact;
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
pub mod security;
/// Agent-discoverability task-to-command matrix shared by `init --agents`,
/// `hooks install --target agent`, the schema manifest, and root `--help`.
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
mod task_matrix;
#[allow(
    dead_code,
    unused_imports,
    reason = "shared CLI library compiles bin-oriented support modules for reuse"
)]
use fallow_engine::validate;
use fallow_engine::vital_signs;

pub use runtime_support::{AnalysisKind, GroupBy};
pub(crate) use runtime_support::{
    ConfigLoadOptions, build_ownership_resolver, load_config_for_analysis,
};
