//! Contract of the `--lean` JSON profile.
//!
//! The CLI writes a lean document from the full envelope: it leaves out empty
//! arrays and zero dead-code summary counts, groups workspace diagnostics by
//! kind, caps the health tables and moves repeated action text to
//! `_meta.actions`. The full envelope types stay the contract of the default
//! output. These types describe only the root of a lean document, so a
//! validator can tell the two shapes apart.

use std::collections::BTreeMap;

use serde::Serialize;

/// Value of the root `profile` field of a lean document.
pub const LEAN_PROFILE: &str = "lean";

/// Root `kind` values that the lean profile changes. A document of another
/// kind keeps its full shape and has no `profile` field.
pub const LEAN_PROFILE_KINDS: &[&str] = &["dead-code", "dead-code-grouped", "health", "combined"];

/// Root of a `--format json --lean` document.
///
/// The body is the envelope of the same `kind`, with these changes:
///
/// - Arrays that are empty are left out: at the root, in each `groups[]`
///   entry, and in each combined section (`check`, `dupes`, `health`).
/// - Dead-code `summary` counts that are zero are left out (the dead-code
///   root and the combined `check` section).
/// - `workspace_diagnostics` holds one entry per `kind`: the first
///   diagnostic of the kind plus `count` when the kind has more than one.
/// - The health tables `hotspots`, `file_scores` and `large_functions` keep
///   their first rows (10 by default, the `--top` value when given, all rows
///   with `--full-tables`). `<table>_omitted` counts the rows left out. With
///   `fallow health --top`, the analysis cuts the tables first, so a table
///   that `--top` cut has no `<table>_omitted` count.
/// - Repeated action text is in `_meta.actions`. An action without a text
///   field reads it from `_meta.actions[<type>].<field>`.
///
/// `schema_version` is the version of the full envelope of the same `kind`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow --format json --lean"))]
pub struct LeanOutput {
    /// Kind of the full envelope that this document trims.
    pub kind: LeanOutputKind,
    /// Always `lean`. A full document has no `profile` field.
    pub profile: LeanProfile,
    /// Schema version of the full envelope of the same `kind`.
    pub schema_version: u32,
    /// Workspace diagnostics, one entry per `kind`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<LeanDiagnosticGroup>,
    /// The `_meta` object of the full envelope plus `actions`.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<LeanMeta>,
}

/// Envelope kinds that have a lean profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum LeanOutputKind {
    /// `fallow dead-code --format json --lean`.
    #[serde(rename = "dead-code")]
    DeadCode,
    /// `fallow dead-code --group-by ... --format json --lean`.
    #[serde(rename = "dead-code-grouped")]
    DeadCodeGrouped,
    /// `fallow health --format json --lean`.
    #[serde(rename = "health")]
    Health,
    /// Bare `fallow --format json --lean`.
    #[serde(rename = "combined")]
    Combined,
}

/// Marker of a lean document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum LeanProfile {
    /// The `--lean` profile.
    #[serde(rename = "lean")]
    Lean,
}

/// One workspace diagnostic kind in a lean document. The entry also carries
/// the kind-specific fields of the first diagnostic of the kind.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LeanDiagnosticGroup {
    /// Diagnostic kind.
    pub kind: String,
    /// Path of the first diagnostic of the kind.
    pub path: String,
    /// Message of the first diagnostic of the kind.
    pub message: String,
    /// `true` when any diagnostic of the kind degrades the analysis.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub degrades_analysis: bool,
    /// Number of diagnostics of the kind, when more than 1. An absent count
    /// means 1. Run without `--lean` for each diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

/// The `_meta` keys that the lean profile adds. The keys of the full envelope
/// `_meta` stay.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LeanMeta {
    /// Shared action text, keyed by action `type`. An action without
    /// `description`, `note` or `comment` reads the value here.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub actions: BTreeMap<String, LeanSharedActionText>,
}

/// The most common text of one action type. Actions with other text keep
/// their own field.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct LeanSharedActionText {
    /// Shared `description`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Shared `note`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Shared `comment`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}
