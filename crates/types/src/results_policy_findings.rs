//! Boundary coverage and call violations, and policy violations.

use super::*;

/// A source file that does not match any configured architecture boundary zone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundaryCoverageViolation {
    /// The unmatched source file.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number used for diagnostics.
    pub line: u32,
    /// 0-based byte column offset used for diagnostics.
    pub col: u32,
}

/// A call from a zoned file to a callee forbidden for that zone via
/// `boundaries.calls.forbidden`. One finding is reported per call or `new`
/// site.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundaryCallViolation {
    /// The zoned source file making the forbidden call.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the call site.
    pub line: u32,
    /// 0-based byte column offset of the call site.
    pub col: u32,
    /// The zone the calling file is classified into.
    pub zone: String,
    /// The callee path as written (e.g. `cp.exec`), with the prefix `new ` for a `new` expression.
    pub callee: String,
    /// The configured pattern that matched (e.g. `child_process.*`), so
    /// consumers can see both the written path and the rule that fired.
    pub pattern: String,
}

/// Which rule-pack rule kind produced a [`PolicyViolation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum PolicyRuleKind {
    /// A call or `new` expression matched a `banned-call` rule's callee patterns.
    BannedCall,
    /// An import or re-export specifier matched a `banned-import` rule.
    BannedImport,
    /// A call site matched a catalogue-derived `banned-effect` rule.
    BannedEffect,
    /// An exported name matched a `banned-export` rule.
    BannedExport,
    /// A resolved gdp-ts proof factory call occurred outside allowed modules.
    GdpProofProducer,
}

/// Effective severity of a single [`PolicyViolation`]. Per-rule `severity`
/// overrides the `rules."policy-violation"` master; `off` rules emit nothing,
/// so only `error` and `warn` appear on the wire. The exit-code gate inspects
/// this per-finding value, not the master severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum PolicyViolationSeverity {
    /// Sets a non-zero exit code.
    Error,
    /// Reported without an effect on the exit code. The dead-code count gate
    /// of the GitHub Action counts it unless `fail-on-issues` is `false`.
    Warn,
}

/// A banned call, banned import, banned effect, or banned export matched by a
/// declarative rule pack (`rulePacks` config). Banned-call and banned-effect
/// findings report one entry per call site (`new` sites count for banned-call
/// only), matching `boundary_call_violations`; banned-import findings anchor
/// at each matching import or re-export declaration; banned-export findings
/// anchor at matching export declarations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PolicyViolation {
    /// The source file containing the banned call, import, or effectful usage.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the call site or import declaration.
    pub line: u32,
    /// 0-based byte column offset of the call site or import declaration.
    pub col: u32,
    /// Name of the rule pack that declared the matching rule.
    pub pack: String,
    /// Id of the matching rule inside the pack. `pack` plus `rule_id` is the
    /// finding's policy identity.
    pub rule_id: String,
    /// Which rule kind matched.
    pub kind: PolicyRuleKind,
    /// What matched: the written callee path for `banned-call` (e.g. `cp.exec`, or `new Date` for a `new` expression), `<effect>: <callee>` for `banned-effect`,
    /// or the exported name for `banned-export`. For `gdp-proof-producer`, the canonical factory with its JSON-quoted literal label or `...` for a dynamic label.
    /// For `banned-import`: the raw import specifier (e.g. `moment/locale/nl`), or `<specifier>: <binding>` when the rule sets `allowNames`.
    /// `<binding>` is the source-side imported name, `default`, `*` for a namespace or star form,
    /// or `side-effect`.
    pub matched: String,
    /// Effective severity for this finding (per-rule `severity`, else the
    /// `rules."policy-violation"` master).
    pub severity: PolicyViolationSeverity,
    /// The rule's author-provided message, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
