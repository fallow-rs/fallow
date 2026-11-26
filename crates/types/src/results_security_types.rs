//! Security candidates, sinks, boundaries, taint flows, runtime context,
//! defensive controls, and the attack surface.

use super::*;

/// The kind of security candidate. Findings are candidates for downstream agent
/// verification, NOT verified vulnerabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityFindingKind {
    /// A `"use client"` file transitively imports a module that reads a
    /// non-public `process.env` secret (graph-structural; bespoke, not catalogue).
    ClientServerLeak,
    /// A syntactic sink site matched against the data-driven catalogue
    /// (`security_matchers.toml`). Serializes `"tainted-sink"`; the CWE class is
    /// carried in `category` + `cwe`. ONE variant covers all catalogue categories.
    TaintedSink,
}

/// How strongly the untrusted-source signal is associated with the sink, a
/// structured discriminator so a consumer can tier candidates without parsing
/// the human `evidence` prose. Present only when
/// [`SecurityReachability::reachable_from_untrusted_source`] is true. Neither
/// value proves exploitability; both are ranking signals (issue #885 doctrine:
/// rank, never gate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TaintConfidence {
    /// The sink's argument traces back to a known untrusted-source read in the
    /// same statement / module (the intra-module back-trace, issue #859). The
    /// strong, high-value candidate: a specific source expression is implicated.
    ArgLevel,
    /// The sink merely lives in a module that is import-reachable from a module
    /// containing an untrusted source (issue #885). The weak candidate: only the
    /// module is implicated, not a specific value path to the sink argument.
    ModuleLevel,
}

/// Graph-derived reachability ranking signal for a security candidate. Computed
/// from the existing module graph after detection, never proven exploitable.
/// Used to surface candidates that sit on a request/runtime-reachable surface,
/// receive same-module source evidence, or are import-reachable from an
/// untrusted-source module above isolated helpers or scripts.
///
/// This is a relative-ordering signal, NOT a `confidence` or `signal_strength`
/// score: fallow does not prove the path is exploitable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityReachability {
    /// Whether the anchor module is reachable from a runtime/application entry
    /// point (route handlers, server entry, framework runtime roots), the
    /// closest graph proxy for an external/request input surface. Code reachable
    /// only from test entry points does not count.
    pub reachable_from_entry: bool,
    /// Whether the anchor module is reachable over value imports from a module
    /// that reads a known untrusted input source. Module-level only: this does
    /// not prove a specific source value reaches the sink argument.
    #[serde(default)]
    pub reachable_from_untrusted_source: bool,
    /// Structured tier of the untrusted-source association: `arg-level` when the
    /// sink argument traces to a same-module source read (strong), `module-level`
    /// when only the module is import-reachable from a source (weak). Present
    /// exactly when `reachable_from_untrusted_source` is true, so a consumer can
    /// separate strong from weak candidates from this field alone without parsing
    /// the `evidence` string. Not an exploitability proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taint_confidence: Option<TaintConfidence>,
    /// Number of value-import hops from the untrusted-source module to the sink
    /// module when `reachable_from_untrusted_source` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub untrusted_source_hop_count: Option<u32>,
    /// Module-level import path from the untrusted-source module to the sink
    /// anchor. Empty when no source module reaches this candidate. The path is a
    /// ranking explanation, not a value-flow proof.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untrusted_source_trace: Vec<TraceHop>,
    /// Number of distinct modules that transitively depend on the anchor module
    /// (fan-in via the graph's reverse-dependency index). A higher value means a
    /// wider surface: more call sites could route untrusted input into the sink.
    pub blast_radius: u32,
    /// Whether the anchor module participates in an architecture-boundary
    /// violation found in the same run (as the importing or imported file).
    /// Optional pairing: a candidate that also crosses a declared boundary is a
    /// stronger review target.
    pub crosses_boundary: bool,
}

/// Dead-code cross-link attached to a security candidate when fallow's dead-code
/// pass reports the same anchor as removable code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityDeadCodeContext {
    /// Dead-code issue kind that matched the security candidate.
    pub kind: SecurityDeadCodeKind,
    /// Unused export name when `kind` is `unused-export`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_name: Option<String>,
    /// Dead-code finding line when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Agent-facing guidance for deciding between deletion and hardening.
    pub guidance: String,
}

/// Dead-code issue kind linked to a security candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityDeadCodeKind {
    /// The candidate's anchor file is also reported as an unused file.
    UnusedFile,
    /// The candidate's anchor sits on an unused export declaration.
    UnusedExport,
}

/// Internal row for a security sink-shaped callee that extraction could not
/// flatten to a static catalogue path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeDiagnostic {
    /// File containing the skipped callee. Absolute internally.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line of the skipped callee.
    pub line: u32,
    /// 0-based byte column of the skipped callee.
    pub col: u32,
    /// Why the callee could not be flattened.
    pub reason: SkippedSecurityCalleeReason,
    /// Compact syntax shape of the skipped callee.
    pub expression_kind: SkippedSecurityCalleeExpressionKind,
}

/// The sink slot of a [`SecurityCandidate`]: a self-contained description of the
/// matched sink site. Echoes the finding's own span (`path`/`line`/`col`) plus
/// the catalogue `category`/`cwe` and the captured `callee`, so an agent can act
/// on `candidate.sink` in isolation (e.g. after fanning a finding out to a
/// sub-agent) without reading the parent finding.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityCandidateSink {
    /// File of the sink site. Absolute internally; JSON strips the project root
    /// via `serde_path::serialize`.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line of the sink site.
    pub line: u32,
    /// 0-based byte column of the sink site.
    pub col: u32,
    /// Catalogue category id of the sink (e.g. `"dangerous-html"`). For
    /// `client-server-leak` this is `"client-server-leak"` for the secret-leak
    /// finding, and `"server-only-import"` when a `"use client"` cone reaches
    /// server-only code. Older releases left it absent for the secret leak.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// CWE number declared by the catalogue entry. `None` for
    /// `client-server-leak`; never fabricated beyond the catalogue's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwe: Option<u32>,
    /// The sink callee (the dangerous function or member path, e.g.
    /// `"el.innerHTML"`, `"child_process.exec"`) captured by the catalogue match.
    /// `None` for `client-server-leak` and matches that name no callee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callee: Option<String>,
    /// URL construction shape for SSRF and open-redirect style candidates when
    /// fallow can classify whether the origin is fixed or dynamic. Absent for
    /// non-URL sinks and unclassified URL expressions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_shape: Option<SecurityUrlShape>,
}

/// A declared architecture-zone crossing, recovered by correlating a finding's
/// anchor against the run's architecture-boundary violations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityZoneCrossing {
    /// Zone the importing side belongs to.
    pub from: String,
    /// Zone the imported side belongs to.
    pub to: String,
}

/// The boundary slot of a [`SecurityCandidate`]: which structural boundaries the
/// candidate's flow crosses. A flow that crosses a client/server or module
/// boundary is a stronger review target than a self-contained one; the boundary
/// is fallow's structural signal over a pure source-sink match.
///
/// Two further boundary kinds are reserved for a follow-up and are deliberately
/// absent here rather than emitted as always-false: `export_visibility` (is the
/// sink on a publicly-exported symbol?) and a package boundary (does the flow
/// cross an npm-package edge?). Both need new graph derivation that does not
/// exist today; emitting them as `false` would misreport "we checked and it does
/// not cross" when fallow has not checked at all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityCandidateBoundary {
    /// Whether the finding crosses a client/server boundary (a `"use client"`
    /// file appears in the trace). True only for `client-server-leak` today;
    /// `tainted-sink` candidates carry no client/server marker.
    pub client_server: bool,
    /// Whether an untrusted source reaches the sink across one or more
    /// value-import (module) hops. Derived from the reachability hop count.
    pub cross_module: bool,
    /// The architecture-zone crossing when the anchor participates in a declared
    /// boundary-rule violation in the same run. `None` when it crosses no
    /// declared zone boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture_zone: Option<SecurityZoneCrossing>,
}

/// Network-destination context for a `secret-to-network` candidate (#890): where
/// the secret-bearing network call sends its data. Present only on
/// network-category candidates. A consuming agent uses it to triage exfil
/// (dynamic / untrusted destination) from intended auth (a literal provider
/// host) without re-reading source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityNetworkContext {
    /// The network call's destination as a static URL string literal, or absent
    /// when the destination is dynamic (not a literal). A dynamic destination is
    /// the higher-signal exfil case; a literal provider host is usually intended
    /// auth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
}

/// An agent-actionable candidate record on a [`SecurityFinding`]. fallow fills
/// `source_kind`, `sink`, and `boundary`. The exploitability impact is
/// deliberately NOT a field: `severity` on the parent finding is only a
/// review-priority tier, while deciding exploitability remains the consuming
/// agent's job. A perpetually-null `impact` key would only train consumers to
/// ignore it. The agent reads this record, then writes its own impact verdict
/// downstream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityCandidate {
    /// The kind of untrusted input that reaches the sink, as a stable catalogue
    /// source id (`"http-request-input"`, `"process-env"`, `"process-argv"`,
    /// `"message-event-data"`, `"location-input"`, ...). `None`/absent when no
    /// untrusted source was matched (always `None` for `client-server-leak`).
    /// This is an open string set, driven by the data-driven source catalogue; a
    /// consumer should treat an unknown id as "untrusted source of unknown kind"
    /// and never drop the candidate on that basis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    /// The sink the candidate fires on, self-contained so the record is
    /// actionable without reading the parent finding.
    pub sink: SecurityCandidateSink,
    /// The structural boundary the flow crosses.
    pub boundary: SecurityCandidateBoundary,
    /// Network-destination context, present only on `secret-to-network` (#890)
    /// candidates: the host the secret-bearing call targets, so an agent can
    /// triage exfil from intended auth. Absent for every other category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<SecurityNetworkContext>,
}

/// One endpoint (source or sink node) of a [`SecurityTaintFlow`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TaintEndpoint {
    /// File of the endpoint. Absolute internally; JSON strips the project root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line of the endpoint.
    pub line: u32,
    /// 0-based byte column of the endpoint.
    pub col: u32,
}

/// Compact taint-flow path shape. The ordered per-hop trace is NOT duplicated
/// here: it lives on [`SecurityReachability::untrusted_source_trace`]. This
/// carries only the flow's structural summary (intra-module flow plus the
/// cross-module hop count) so consumers do not parse two copies of the hops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TaintPath {
    /// Whether the source and sink sit in the same module (no import hop between
    /// them); the source-to-sink association is intra-module.
    pub intra_module: bool,
    /// Number of value-import hops from the untrusted-source module to the sink
    /// module. Zero for an intra-module flow.
    pub cross_module_hops: u32,
}

/// A source-to-sink taint-flow triple, emitted only when an untrusted source is
/// import-reachable to the sink (`reachability.reachable_from_untrusted_source`).
/// The `{ source, sink, path }` shape matches the model agent SAST tooling
/// expects (cf. Semgrep `taint_source` / `taint_sink`, SARIF `threadFlows`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityTaintFlow {
    /// The untrusted-source endpoint (first hop of the reachability trace).
    pub source: TaintEndpoint,
    /// The sink endpoint (terminal hop of the reachability trace / the anchor).
    pub sink: TaintEndpoint,
    /// Compact flow shape: same-module flag plus module hop count. The full
    /// ordered path is `reachability.untrusted_source_trace`.
    pub path: TaintPath,
}

/// Runtime coverage state for the function enclosing a security sink.
/// This is production-observation evidence, not an exploitability verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityRuntimeState {
    /// The sink sits inside a runtime hot path.
    RuntimeHot,
    /// The sink sits inside a tracked function with zero production invocations.
    RuntimeCold,
    /// The sink sits inside a tracked function the runtime layer marked as safe
    /// to delete because it was never executed.
    NeverExecuted,
    /// The sink sits inside a function that executed, but below the low-traffic
    /// threshold.
    LowTraffic,
    /// Runtime coverage could not classify the enclosing function.
    CoverageUnavailable,
    /// A static enclosing function was found, but the runtime report carried no
    /// matching evidence for it.
    RuntimeUnknown,
}

/// Runtime coverage context attached to a security candidate when
/// `fallow security --runtime-coverage` is supplied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityRuntimeContext {
    /// Runtime state for the enclosing function.
    pub state: SecurityRuntimeState,
    /// Enclosing function name from static extraction.
    pub function: String,
    /// 1-based line where the enclosing function starts.
    pub line: u32,
    /// Observed invocation count when the runtime report provides it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocations: Option<u64>,
    /// Runtime coverage stable function id, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_id: Option<String>,
    /// Short candidate-framed explanation of the runtime evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

/// Verification-priority tier for a security candidate. This is ranking, not an
/// exploitability verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum SecuritySeverity {
    /// Highest-priority candidate based on reachability, boundary, or runtime-hot signals.
    High,
    /// Candidate has source-reachability evidence but no high-priority signal.
    Medium,
    /// Candidate has no source-reachability or boundary signal.
    Low,
}

/// Control pattern observed in a file on an attack-surface import trace.
/// Its presence does not prove that it executes before the sink or protects
/// the same input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityDefensiveControl {
    /// Control family.
    pub kind: SecurityControlKind,
    /// File of the control site. Absolute internally; JSON strips the project root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line of the control site.
    pub line: u32,
    /// 0-based byte column of the control site.
    pub col: u32,
    /// Flattened callee path or a stable synthetic guard name.
    pub callee: String,
}

/// Agent-facing defensive-boundary verification context for one surface path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityDefensiveBoundary {
    /// Control patterns observed in files on this import trace. These are
    /// verification hints, not proof of sink protection or value-level data flow.
    pub controls: Vec<SecurityDefensiveControl>,
    /// Verification question for the consuming agent. It is a prompt, not a
    /// missing-guard verdict.
    pub verification_prompt: String,
}

/// One untrusted entry to reachable sink path for `fallow security --surface`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityAttackSurfaceEntry {
    /// The untrusted-source endpoint.
    pub source: TaintEndpoint,
    /// The reachable sink endpoint and catalogue metadata.
    pub sink: SecurityCandidateSink,
    /// Ordered source to sink path. Same shape as the reachability trace so
    /// consumers can reuse existing path handling.
    pub path: Vec<TraceHop>,
    /// Defensive-boundary context detected on this path.
    pub defensive_boundary: SecurityDefensiveBoundary,
}
