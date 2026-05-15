//! Shared types for fallow codebase intelligence.
//!
//! This crate contains type definitions used across multiple fallow crates
//! (core, CLI, LSP). It has no analysis logic, only data structures.

#![warn(missing_docs)]

/// File discovery types: discovered files, file IDs, and entry points.
pub mod discover;
/// JSON-output envelope and utility types: `SchemaVersion`, `ToolVersion`,
/// `ElapsedMs`, `AuditIntroduced`, plus the shared `Meta`, `BaselineDeltas`,
/// `BaselineMatch`, `RegressionResult`, `EntryPoints`, and `CheckSummary`
/// shapes referenced by every per-command envelope. The structs are always
/// compiled (the JSON emission layer constructs them at runtime); the
/// `schemars::JsonSchema` derive is gated per-struct on the `schema` feature.
pub mod envelope;
/// Module extraction types: exports, imports, re-exports, and member info.
pub mod extract;
/// JSON-output augmentation types: `IssueAction` enum + variants.
/// Schema-side counterpart of the augmentations the JSON layer adds to each
/// finding. Gated on the `schema` cargo feature so the (rare) consumers of
/// the typed output contract opt in explicitly; the JSON emission path will
/// adopt these types in a follow-up.
#[cfg(feature = "schema")]
pub mod output;
/// Schema-side wrappers for the per-action types attached to health findings,
/// hotspots, and refactoring targets. Separated from [`output`] so the
/// generic `IssueAction` tree stays focused while the health-specific
/// variants (with their own `type` discriminant enums) live in a dedicated
/// module. Gated on the `schema` cargo feature.
#[cfg(feature = "schema")]
pub mod output_health;
/// Analysis result types: unused files, exports, dependencies, and members.
pub mod results;
/// Custom serde serializers for cross-platform path output.
pub mod serde_path;
/// Inline suppression comment types and issue kind definitions.
pub mod suppress;
