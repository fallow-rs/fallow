//! Shared types for fallow codebase intelligence.
//!
//! This crate contains type definitions used across multiple fallow crates
//! (core, CLI, LSP). It has no analysis logic, only data structures.
//!
//! User documentation for fallow is on [fallow.tools](https://fallow.tools/docs/).

#![warn(missing_docs)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

/// Atomic cache-file replacement, shared by the extraction cache and the graph
/// cache so concurrent fallow processes never interleave a write.
pub mod atomic_file;
/// Typed audit cache-key inputs.
pub mod audit_cache;
/// The base snapshots of `fallow audit` and the lookups above a project.
pub mod audit_snapshot;
/// Why a persisted cache was refused, shared by the extraction cache, the
/// graph cache, the perf table, and `fallow doctor`.
pub mod cache_rejection;
/// Classification of the source file of a package manager catalog finding.
pub mod catalog_source;
/// Shared churn output contracts.
pub mod churn;
/// Cross-surface strings for the cloud runtime-coverage path, shared by the
/// CLI's cloud `coverage` commands and the MCP cloud tools.
pub mod cloud;
/// One row per analysis subcommand: its JSON kinds and the surfaces that must
/// know about it. Drift tests across the workspace read this table.
pub mod command_surfaces;
/// Per-construct complexity increments behind the complexity scores.
pub mod complexity_contribution;
/// The config file names that discovery reads and the near-miss names.
pub mod config_file_names;
/// A per-thread scope that keeps the diagnostics of a second analysis out
/// of the report.
pub mod diagnostic_scope;
/// File discovery types: discovered files, file IDs, and entry points.
pub mod discover;
/// Display forms of project strings for text output and shell commands.
pub mod display_text;
/// Shared duplicate-code output contracts.
pub mod duplicates;
/// JSON-output envelope and utility types: `SchemaVersion`, `ToolVersion`,
/// `ElapsedMs`, `AuditIntroduced`, plus the shared `Meta`, `BaselineDeltas`,
/// `BaselineMatch`, `RegressionResult`, `EntryPoints`, and `CheckSummary`
/// shapes referenced by every per-command envelope. The structs are always
/// compiled (the JSON emission layer constructs them at runtime); the
/// `schemars::JsonSchema` derive is gated per-struct on the `schema` feature.
pub mod envelope;
/// Where a fact sits on each JSON envelope shape, shared by the surfaces that
/// read a saved envelope back rather than holding the run's typed state.
pub mod envelope_sites;
/// Module extraction types: exports, imports, re-exports, and member info.
pub mod extract;
mod extract_namespace_member_uses;
pub mod flag_retirement;
/// Guard output contracts for pre-edit architecture-rule lookup.
pub mod guard;
/// Stable finding identity: the shared FNV-1a 64 hash and dead-code
/// `finding_id` values.
pub mod identity;
/// Inline health threshold override comments.
pub mod inline_threshold;
/// Shared issue-type contract metadata used by CLI, LSP, MCP, and suppression
/// helpers.
pub mod issue_meta;
/// Levenshtein distance and closest-match policy for typo suggestions.
pub mod levenshtein;
mod link_walk;
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
/// `#[serde(flatten)]` and creates a typed `actions` array at construction.
/// The `introduced` field defaults to `None`. The audit pass inserts it into the
/// JSON output. Each wrapper derives `schemars::JsonSchema` only with `schema`.
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
/// Refuse cache writes through a symlink that leaves the project.
pub mod path_guard;
/// Cross-platform path classification helpers.
pub mod path_util;
/// The `--performance` span tree and the process-level clock.
pub mod pipeline_spans;
/// Bounded reads of project files: no link out of the root, regular files
/// only, and a size limit.
pub mod read_guard;
/// The directory that the reads of a project may reach.
pub mod read_scope;
/// Analysis result types: unused files, exports, dependencies, and members.
pub mod results;
/// The `Next:` step of a `client-server-leak` candidate, shared by the CLI
/// and the LSP hover.
pub mod security_next_step;
/// Security sink names shared by the extractor and the sink catalogue.
pub mod security_sinks;
/// The line-free input of the stable id of a security finding.
pub mod security_stable_id;
/// Shared TypeScript semantic identity, provenance, API surface, impact, and
/// public-signature coupling contracts.
pub mod semantic;
/// Custom serde serializers for cross-platform path output.
pub mod serde_path;
/// Transient source-extraction contracts for provider-backed similar-code analysis.
pub mod similar_code;
/// Shared source-file freshness metadata used by cache invalidation.
pub mod source_fingerprint;
/// Values with the cause of their value: the input layer, the scope, and the
/// flag, variable, config key or default that gave the value.
pub mod sourced;
/// Inline suppression comment types and issue kind definitions.
pub mod suppress;
/// Agent task-to-command matrix rows shared by `fallow schema`, the
/// `init --agents` template, the agent hook block, root `--help`, and the
/// `fallow://task-matrix` MCP resource. Data only; the CLI owns rendering.
pub mod task_matrix;
/// The telemetry kill switches, shared by the CLI and the MCP server.
pub mod telemetry_opt_out;
/// Trace output contracts shared by core, engine, CLI, API, and MCP.
pub mod trace;
/// Symbol-level trace-chain output contracts.
pub mod trace_chain;
/// Stack-trace frame resolution output contracts (`fallow trace-error`).
pub mod trace_error;
/// Per-specifier dependency usage contracts (`fallow trace --dependency`).
pub mod trace_usage;
/// Workspace and source-discovery diagnostic data types
/// (`WorkspaceDiagnostic`, `WorkspaceDiagnosticKind`). Re-exported by
/// `fallow-config` for back-compat; embedded directly by `fallow-output` so
/// `workspace_diagnostics[]` keeps its typed JSON schema. The
/// `schemars::JsonSchema` derive is gated on the `schema` feature.
pub mod workspace;

/// The `tracing` target of a warning that repeats a fact the JSON envelope
/// carries, or that describes the quality of the analysis inputs.
///
/// `--quiet`, `--ci` and `FALLOW_QUIET` hide this target on stderr. An explicit
/// `RUST_LOG` shows it again. Config-load warnings, such as an unknown rule
/// key, do not use this target, so they stay visible in a quiet run.
pub const ANALYSIS_DIAGNOSTIC_LOG_TARGET: &str = "fallow::analysis_diagnostic";

/// Optional architecture shared-state facts and inventory.
pub mod shared_state;
