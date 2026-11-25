//! A security candidate of the `fallow security` command.

use super::*;

/// A local security candidate for downstream agent verification, not a verified
/// vulnerability. Emitted only by `fallow security`, never under bare `fallow`
/// or the `audit` gate. There is deliberately no `confidence` or
/// `signal_strength` field: fallow does not prove exploitability, so the trace
/// (its hops and length) is the only honest signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityFinding {
    /// Location id (rule, path, line, column), equal to SARIF `fallowSecurity/v2`; edits move it.
    pub finding_id: String,
    /// Line-independent id (`sec1:<hash>`, `~k` for repeats); a verdict may use it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_finding_id: Option<String>,
    /// The rule that produced this candidate.
    pub kind: SecurityFindingKind,
    /// The catalogue category id (e.g. `"dangerous-html"`). `Some` for
    /// `TaintedSink`. For `ClientServerLeak` it is `"client-server-leak"` for the
    /// secret leak (older releases omit it) or `"server-only-import"` when a
    /// `"use client"` cone reaches server-only code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The CWE number declared by the matched catalogue entry. `None` for
    /// `ClientServerLeak`; never fabricated beyond the catalogue's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwe: Option<u32>,
    /// File the finding is anchored on (the client boundary). Absolute
    /// internally; JSON strips the project root via `serde_path::serialize`.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the anchor.
    pub line: u32,
    /// 0-based byte column offset of the anchor.
    pub col: u32,
    /// Agent/human-readable evidence (e.g. the named env var the chain reaches).
    pub evidence: String,
    /// Whether the sink argument was associated with a known untrusted source by
    /// the intra-module source-to-sink back-trace (issue #859): a local binding
    /// referenced in the argument was sourced from a catalogue source path
    /// (`req.query`, `process.argv`, message-event `data`, etc.). `true` ranks
    /// the candidate higher and annotates the evidence; `false` does NOT
    /// suppress the finding (the association is conservative, never a proof, and
    /// fallow prefers false-negatives over false-positives). Always `false` for
    /// `ClientServerLeak`. Skipped from JSON when `false` for output stability.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub source_backed: bool,
    /// Internal cross-pass carrier (NEVER serialized): the (1-based line, 0-based
    /// col) of the arg-level source read, resolved by the detector when
    /// `source_backed` is true and a concrete read span was captured. The ranking
    /// pass uses it to anchor the taint trace's source node at the real read
    /// instead of the module import line. `None` for module-level findings and
    /// for arg-level findings with no concrete read span (synthetic
    /// framework-param / helper-return sources), where the trace falls back to
    /// the sink site.
    #[serde(skip)]
    pub source_read: Option<(u32, u32)>,
    /// Verification-priority tier derived from existing reachability, boundary,
    /// source-backed, and runtime signals. Candidate-only: this does not prove
    /// exploitability and does not change gates.
    pub severity: SecuritySeverity,
    /// Structural import-hop trace from the client boundary to the secret source.
    /// The hop count is the uncalibrated signal; fallow does not prove the path
    /// is exploitable.
    pub trace: Vec<TraceHop>,
    /// Machine-actionable next steps. Always emitted (possibly empty for
    /// forward-compat). For security candidates this is a single file-level
    /// suppress hint (`auto_fixable: false`); there is no auto-fix because
    /// verification is the agent's job, not fallow's.
    pub actions: Vec<IssueAction>,
    /// Dead-code cross-link when the same sink candidate sits in code fallow also
    /// reports as removable. Agents should verify the dead-code finding and delete
    /// the code instead of hardening the sink when deletion is safe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code: Option<SecurityDeadCodeContext>,
    /// Graph-derived reachability ranking signal (issues #860 and #885). `None`
    /// until the post-detection ranking pass fills it; additive on the wire
    /// (skipped when absent). Drives the order findings are emitted in:
    /// runtime-reachable candidates sort first, followed by source-backed and
    /// source-reachable candidates, then wider blast radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reachability: Option<SecurityReachability>,
    /// Agent-actionable candidate record: the untrusted input kind, the sink, and the boundary
    /// the flow crosses. fallow fills these three slots; the exploitability verdict is the
    /// agent's job and is not a field here. Always present.
    pub candidate: SecurityCandidate,
    /// Source-to-sink taint-flow triple, present only when an untrusted source
    /// is import-reachable to this sink. Absent (skipped) otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taint_flow: Option<SecurityTaintFlow>,
    /// Production runtime coverage context for the function enclosing this
    /// security sink. Present only when `fallow security --runtime-coverage`
    /// runs and the candidate is a `tainted-sink`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<SecurityRuntimeContext>,
    /// Internal projection used by `fallow security --surface`. The CLI strips this from
    /// per-finding JSON and promotes it to the top-level `attack_surface` field only when requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_surface: Option<SecurityAttackSurfaceEntry>,
    /// Internal input of `stable_finding_id`. Never serialized; `None` after a JSON round trip.
    #[serde(skip)]
    pub stable_id_input: Option<crate::security_stable_id::SecurityStableIdInput>,
}
