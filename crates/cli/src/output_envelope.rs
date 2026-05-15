//! Typed envelope structs for the JSON output contract.
//!
//! Each top-level fallow command (`check`, `dupes`, `health`, `audit`,
//! `explain`, `coverage setup`, plus the bare combined invocation and the
//! CodeClimate / review-envelope side outputs) emits a distinct envelope
//! shape. This module is the schema-side source of truth for those shapes:
//! every type carries `Serialize` plus a cfg-gated `JsonSchema` derive so the
//! committed `docs/output-schema.json` can be regenerated from Rust.
//!
//! Living in `fallow-cli` rather than `fallow-types` because the body fields
//! pull in `DuplicationReport` (from `fallow-core`) and `HealthReport` (from
//! this crate), neither of which is reachable from the lower-level types
//! crate. The shared utility shapes (`SchemaVersion`, `Meta`,
//! `BaselineDeltas`, ...) still live in `fallow_types::envelope` because they
//! depend only on serde primitives.
//!
//! Runtime construction of these envelopes happens in
//! `crates/cli/src/report/json.rs`; the JSON layer builds an envelope struct
//! and converts it to a `serde_json::Value` via `serde_json::to_value`. Path
//! relativisation and the per-finding `actions` injection still run as
//! post-passes on the `Value` tree because they cross result-type boundaries
//! that typed wrappers do not.

use fallow_core::duplicates::DuplicationReport;
use fallow_types::envelope::{ElapsedMs, Meta, SchemaVersion, ToolVersion};
use serde::Serialize;

/// Envelope emitted by `fallow dupes --format json` (plus the `dupes` block
/// inside the combined and audit envelopes).
///
/// The body is the full `DuplicationReport` flattened into the envelope so
/// the wire shape stays `{ schema_version, version, elapsed_ms, clone_groups,
/// clone_families, stats, ... }` exactly as the existing JSON layer emits.
/// `grouped_by` / `groups` / `total_issues` are populated by the grouped
/// builder; on the ungrouped path they stay `None` and `skip_serializing_if`
/// drops them.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DupesOutput {
    /// Schema version for this output format.
    pub schema_version: SchemaVersion,
    /// Fallow tool version that produced this output.
    pub version: ToolVersion,
    /// Analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Project-level duplication payload (`clone_groups`, `clone_families`,
    /// `stats`, optional `mirrored_directories`). Flattened so the wire shape
    /// stays a single object.
    #[serde(flatten)]
    pub report: DuplicationReport,
    /// Resolver mode used for partitioning. Present only when `--group-by` is
    /// active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouped_by: Option<GroupByMode>,
    /// Total clone groups across all buckets when `--group-by` is active.
    /// Mirrors the grouped check / health envelopes which expose
    /// `total_issues` so MCP and CI consumers can read the same key across
    /// commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_issues: Option<usize>,
    /// Per-group buckets when `--group-by` is active. Each clone group is
    /// attributed to its largest-owner key (most instances; alphabetical
    /// tiebreak).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<serde_json::Value>,
    /// `_meta` block with metric / rule definitions, emitted when `--explain`
    /// is passed (always present in MCP responses).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

/// Resolver mode label for grouped envelopes (dead-code, dupes, health).
///
/// `owner` groups by CODEOWNERS team, `directory` groups by top-level
/// directory prefix, `package` groups by workspace package name, `section`
/// groups by GitLab CODEOWNERS `[Section]` header name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum GroupByMode {
    /// Group by CODEOWNERS team.
    Owner,
    /// Group by top-level directory prefix.
    Directory,
    /// Group by workspace package name.
    Package,
    /// Group by GitLab CODEOWNERS `[Section]` header name.
    Section,
}
