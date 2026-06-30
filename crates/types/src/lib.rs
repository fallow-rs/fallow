//! Shared types for fallow codebase intelligence.
//!
//! This crate contains type definitions used across multiple fallow crates
//! (core, CLI, LSP). It has no analysis logic, only data structures.

#![warn(missing_docs)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

/// Typed audit cache-key inputs.
pub mod audit_cache;
/// Shared churn output contracts.
pub mod churn;
/// File discovery types: discovered files, file IDs, and entry points.
pub mod discover;
/// Shared duplicate-code output contracts.
pub mod duplicates;
/// JSON-output envelope and utility types: `SchemaVersion`, `ToolVersion`,
/// `ElapsedMs`, `AuditIntroduced`, plus the shared `Meta`, `BaselineDeltas`,
/// `BaselineMatch`, `RegressionResult`, `EntryPoints`, and `CheckSummary`
/// shapes referenced by every per-command envelope. The structs are always
/// compiled (the JSON emission layer constructs them at runtime); the
/// `schemars::JsonSchema` derive is gated per-struct on the `schema` feature.
pub mod envelope;
/// Module extraction types: exports, imports, re-exports, and member info.
pub mod extract;
/// Shared issue-type contract metadata used by CLI, LSP, MCP, and suppression
/// helpers.
pub mod issue_meta;
/// Machine-readable manifest of the fallow MCP server's tools, shared by
/// `fallow schema` and the telemetry tool-name allowlist; kept in sync with
/// the live tool router by a drift test in `crates/mcp`.
pub mod mcp_manifest;
/// JSON-output augmentation types: `IssueAction` enum + variants.
/// Schema-side counterpart of the augmentations the JSON layer adds to each
/// dead-code finding. The structs are always compiled (typed dead-code
/// wrappers in [`output_dead_code`] consume them at runtime); the
/// `schemars::JsonSchema` derive is gated per-struct on the `schema`
/// feature.
pub mod output;
/// Typed envelope wrappers for the simple 1:1 dead-code findings
/// (`UnusedFile`, `PrivateTypeLeak`, `UnresolvedImport`, `CircularDependency`,
/// `BoundaryViolation`). Each wrapper flattens the bare finding via
/// `#[serde(flatten)]` and carries a typed `actions` array populated at
/// construction time, replacing the per-finding post-pass injection that
/// previously grafted `actions[]` and `introduced` onto the schema. The
/// `introduced` field is set by the audit pass via JSON map insertion and
/// is `None` when serialized directly from Rust. The `schemars::JsonSchema`
/// derive is gated per-struct on the `schema` feature.
pub mod output_dead_code;
/// Shared output-format selector used by CLI, config, output, and API layers.
pub mod output_format;
/// Per-action types attached to health findings, hotspots, refactoring
/// targets, and coverage-gap entries. Separated from the generic
/// `IssueAction` tree in the `output` module so the health-specific
/// variants live in a dedicated module. The structs are always compiled
/// (the JSON emission layer constructs them through typed wrappers such as
/// [`output_health::UntestedFileAction`]); the `schemars::JsonSchema`
/// derive is gated per-struct on the `schema` feature.
pub mod output_health;
/// Cross-platform path classification helpers.
pub mod path_util;
/// Analysis result types: unused files, exports, dependencies, and members.
pub mod results;
/// Custom serde serializers for cross-platform path output.
pub mod serde_path;
/// Shared source-file freshness metadata used by cache invalidation.
pub mod source_fingerprint;
/// Inline suppression comment types and issue kind definitions.
pub mod suppress;
/// Trace output contracts shared by core, engine, CLI, API, and MCP.
pub mod trace;
/// Symbol-level trace-chain output contracts.
pub mod trace_chain;
/// Workspace and source-discovery diagnostic data types
/// (`WorkspaceDiagnostic`, `WorkspaceDiagnosticKind`). Re-exported by
/// `fallow-config` for back-compat; embedded directly by `fallow-output` so
/// `workspace_diagnostics[]` keeps its typed JSON schema. The
/// `schemars::JsonSchema` derive is gated on the `schema` feature.
pub mod workspace;
