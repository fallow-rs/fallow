use std::fmt;
use std::path::PathBuf;

use fallow_types::serde_path;

/// Runtime coverage JSON contract version. This is scoped to the
/// `runtime_coverage` block and is independent of the top-level fallow
/// JSON `schema_version`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum RuntimeCoverageSchemaVersion {
    /// First release of the runtime coverage block contract.
    #[default]
    #[serde(rename = "1")]
    V1,
}

/// Top-level verdict for the whole runtime-coverage report. Mirrors
/// `fallow_cov_protocol::ReportVerdict`. The verdict is the SINGLE most
/// actionable finding; for the full set of findings see
/// [`RuntimeCoverageReport::signals`]. The verdict promotes `hot-path-touched`
/// above `cold-code-detected` in PR-review context (when the CLI was
/// given a change-scope: `--diff-file` or `--changed-since`) because the
/// touched-hot-path is event-tied to the current diff and reviewers need
/// it to be the top-line signal. In standalone analysis (no change
/// scope), `cold-code-detected` remains primary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeCoverageReportVerdict {
    Clean,
    HotPathTouched,
    ColdCodeDetected,
    LicenseExpiredGrace,
    #[default]
    Unknown,
}

/// Discrete signal captured during runtime-coverage post-processing.
/// `verdict` collapses to one summary value; `signals` enumerates ALL
/// findings the report carries so JSON consumers, CI dashboards, and
/// agents can reason about them independently of the headline. Order is
/// stable: severity-descending so the first entry mirrors a sensible
/// non-PR-context verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeCoverageSignal {
    LicenseExpiredGrace,
    ColdCodeDetected,
    HotPathTouched,
}

impl RuntimeCoverageSignal {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LicenseExpiredGrace => "license-expired-grace",
            Self::ColdCodeDetected => "cold-code-detected",
            Self::HotPathTouched => "hot-path-touched",
        }
    }
}

impl fmt::Display for RuntimeCoverageSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl RuntimeCoverageReportVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::HotPathTouched => "hot-path-touched",
            Self::ColdCodeDetected => "cold-code-detected",
            Self::LicenseExpiredGrace => "license-expired-grace",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for RuntimeCoverageReportVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Protocol-level per-function runtime coverage verdict derived from the
/// decision table in fallow-cov-protocol. The CLI's `runtime_coverage.findings`
/// array omits `active` entries even though the underlying enum still includes
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCoverageVerdict {
    SafeToDelete,
    ReviewRequired,
    CoverageUnavailable,
    LowTraffic,
    Active,
    Unknown,
}

impl RuntimeCoverageVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SafeToDelete => "safe_to_delete",
            Self::ReviewRequired => "review_required",
            Self::CoverageUnavailable => "coverage_unavailable",
            Self::LowTraffic => "low_traffic",
            Self::Active => "active",
            Self::Unknown => "unknown",
        }
    }

    #[must_use]
    pub const fn human_label(self) -> &'static str {
        match self {
            Self::SafeToDelete => "safe to delete",
            Self::ReviewRequired => "review required",
            Self::CoverageUnavailable => "coverage unavailable",
            Self::LowTraffic => "low traffic",
            Self::Active => "active",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for RuntimeCoverageVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Confidence level for a runtime coverage finding.
pub enum RuntimeCoverageConfidence {
    VeryHigh,
    High,
    Medium,
    Low,
    None,
    Unknown,
}

impl RuntimeCoverageConfidence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VeryHigh => "very_high",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::None => "none",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for RuntimeCoverageConfidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
/// License or trial watermark applied to runtime coverage output.
pub enum RuntimeCoverageWatermark {
    TrialExpired,
    LicenseExpiredGrace,
    Unknown,
}

impl RuntimeCoverageWatermark {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TrialExpired => "trial-expired",
            Self::LicenseExpiredGrace => "license-expired-grace",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for RuntimeCoverageWatermark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Runtime coverage source used to produce the summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCoverageDataSource {
    #[default]
    Local,
    Cloud,
}

impl RuntimeCoverageDataSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cloud => "cloud",
        }
    }
}

impl fmt::Display for RuntimeCoverageDataSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Summary block mirroring `fallow_cov_protocol::Summary` (0.3 shape).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeCoverageSummary {
    /// Runtime evidence source used for this report. Local mode reads a
    /// supplied runtime coverage artifact; cloud mode pulls the latest
    /// fallow.cloud runtime context after explicit opt-in.
    pub data_source: RuntimeCoverageDataSource,
    /// Timestamp of the newest runtime payload included in the report. Null for
    /// local single-capture artifacts that do not carry cloud receipt metadata.
    pub last_received_at: Option<String>,
    /// Number of functions the sidecar could observe in the V8 or Istanbul
    /// dump.
    pub functions_tracked: usize,
    /// Tracked functions that received at least one invocation.
    pub functions_hit: usize,
    /// Tracked functions that were never invoked.
    pub functions_unhit: usize,
    /// Functions the sidecar could not track (lazy-parsed, worker thread,
    /// dynamic code, unresolved source map).
    pub functions_untracked: usize,
    /// Ratio of functions_hit / functions_tracked, expressed as a percent.
    pub coverage_percent: f64,
    /// Total number of observed invocations across all functions. Denominator
    /// for low-traffic classification.
    pub trace_count: u64,
    /// Days of observation covered by the supplied dump (Phase 2 local analysis
    /// emits 0 — set by the beacon/cloud in Phase 3+).
    pub period_days: u32,
    /// Distinct deployments contributing to the supplied dump (Phase 2 local
    /// analysis emits 0).
    pub deployments_seen: u32,
    /// Capture-quality telemetry. `None` for protocol-0.2 sidecars; protocol-0.3+
    /// sidecars always populate it. Fuels the human-output short-window warning
    /// and the quantified trial CTA, and is passed through to JSON consumers so
    /// agent pipelines can surface the same signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_quality: Option<RuntimeCoverageCaptureQuality>,
}

/// Quality-of-capture signals emitted by the sidecar so the CLI can explain
/// short-window captures honestly instead of letting users blame the tool.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeCoverageCaptureQuality {
    /// Total observation window in seconds. Finer-grained than period_days
    /// (which rounds up to whole days).
    pub window_seconds: u64,
    /// Number of distinct production instances that contributed to the dump.
    pub instances_observed: u32,
    /// True when the untracked-function ratio exceeds the sidecar's lazy-parse
    /// threshold (30%). Signals that many untracked functions likely reflect
    /// lazy-parsed code rather than unreachable code.
    pub lazy_parse_warning: bool,
    /// functions_untracked / functions_tracked as a percentage, rounded to 2
    /// decimal places.
    pub untracked_ratio_percent: f64,
}

/// Supporting evidence for a finding (mirrors `fallow_cov_protocol::Evidence`).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeCoverageEvidence {
    /// `used` when the function is reachable in the module graph, `unused`
    /// otherwise.
    pub static_status: String,
    /// `covered` when the project's test suite hits this function,
    /// `not_covered` otherwise.
    pub test_coverage: String,
    /// `tracked` when V8 observed the function, `untracked` otherwise.
    pub v8_tracking: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Reason the function is untracked. Populated only when v8_tracking is
    /// `untracked`. Values: `lazy_parsed`, `worker_thread`, `dynamic_eval`,
    /// `unknown`.
    pub untracked_reason: Option<String>,
    /// Days of observation backing this finding.
    pub observation_days: u32,
    /// Distinct deployments backing this finding.
    pub deployments_observed: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
/// Suggested follow-up action for a runtime coverage finding.
pub struct RuntimeCoverageAction {
    /// Action identifier, normalized to `type` in JSON output. Known values
    /// emitted by `fallow coverage analyze`: `delete-cold-code`
    /// (verdict=safe_to_delete), `review-runtime` (verdict=review_required).
    /// The sidecar may emit additional protocol-specific identifiers;
    /// consumers should treat unknown values as forward-compat extensions.
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
    /// Whether fallow can apply this action automatically.
    pub auto_fixable: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeCoverageMessage {
    pub code: String,
    /// Human-readable warning message.
    pub message: String,
}

/// Discriminator inputs that PRODUCED a finding's verdict (fallow-rs/fallow-cloud#321),
/// emitted alongside the verdict so an agent can reproduce it and see the
/// minimum-observation confidence cap instead of re-deriving them from scratch.
/// F4: these make the EXISTING Fallow-owned discriminators legible; they are not
/// a new or external signal and gate nothing. Pairs with `evidence.static_status`
/// (the static half of the discriminator set).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeCoverageDiscriminators {
    /// Three-state runtime tracking: `called` (invocations > 0), `never_called`
    /// (V8 tracked it, invocations == 0), or `untracked` (V8 never saw it). The
    /// ONLY signal that can issue a deletion verdict.
    pub tracking_state: String,
    /// `invocations / trace_count` for this function; `null` when untracked (no
    /// invocation count). The per-function value behind the low-traffic split.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub invocation_ratio: Option<f64>,
    /// Active/low_traffic split ratio in effect (CLI default 0.001). A tracked
    /// function whose `invocation_ratio` is below this reads `low_traffic`, else
    /// `active`.
    pub low_traffic_threshold: f64,
    /// Total observed invocations across all functions (the `invocation_ratio`
    /// denominator), echoed per finding so the verdict is self-contained.
    pub trace_count: u64,
    /// High-confidence verdict floor (CLI default 5000). When `trace_count` is
    /// below it, confidence is capped regardless of the per-function signal.
    pub min_observation_volume: u32,
    /// `trace_count >= min_observation_volume`: whether the dump cleared the
    /// confidence floor. `false` means this verdict's confidence is capped.
    pub meets_observation_volume: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeCoverageFinding {
    /// Per-finding suppression key of the form `fallow:prod:<hash>` (first 8 hex
    /// of SHA-256(file + function + line + 'prod')). Hashes the current line, so
    /// it changes when the function moves. Use this to suppress one finding.
    pub id: String,
    /// Cross-surface join key of the form `fallow:fn:<hash>`
    /// (`fallow_cov_protocol::function_identity_id`, hashes file + name +
    /// start_line). The same function shares ONE value across findings, hot
    /// paths, blast-radius, and importance entries (the per-finding `id` uses a
    /// per-surface salt, so it differs by surface), and across V8, Istanbul,
    /// and oxc producers (columns are excluded from the hash). Like `id`, it
    /// changes when the function's file, name, or start line changes; it is a
    /// cross-surface / cross-producer join key, not a line-move-immune one.
    /// `null` when the producing surface (or an un-migrated cloud) supplied no
    /// `FunctionIdentity`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub stable_id: Option<String>,
    /// Content digest of the function's full-span source slice
    /// (`fallow_cov_protocol::source_hash_for`: first 8 bytes of SHA-256 as 16
    /// lowercase hex). Unlike `stable_id`, this is stable across line moves: a
    /// moved-but-unedited function keeps the same value, so baselines can
    /// suppress it after a pure line shift. `null` when the producing surface
    /// supplied no `source_hash`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub source_hash: Option<String>,
    /// File path relative to the project root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Static function name as reported in the merged coverage result.
    pub function: String,
    /// 1-indexed line number the function starts on.
    pub line: u32,
    pub verdict: RuntimeCoverageVerdict,
    /// Raw V8 invocation count. `None` when the function was untracked
    /// (lazy-parsed, worker thread, or dynamic code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocations: Option<u64>,
    pub confidence: RuntimeCoverageConfidence,
    pub evidence: RuntimeCoverageEvidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    /// Suggested actions for this finding. Omitted when empty.
    pub actions: Vec<RuntimeCoverageAction>,
    /// The discriminator inputs that produced this verdict (#321), emitted so an
    /// agent can reproduce it and see the confidence cap. `None` for findings
    /// not built from the merge pipeline (e.g. baseline round-trips). Omitted
    /// from JSON when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub discriminators: Option<RuntimeCoverageDiscriminators>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeCoverageHotPath {
    /// Stable content-hash ID of the form `fallow:hot:<hash>`.
    pub id: String,
    /// Cross-surface join key (`fallow:fn:<hash>`) for the hot function. Stable
    /// across line moves; shared with the same function's findings / blast /
    /// importance entries. `null` when no `FunctionIdentity` was supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub stable_id: Option<String>,
    /// File path relative to the project root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Function name for the hot path.
    pub function: String,
    /// 1-indexed line number the function starts on.
    pub line: u32,
    /// 1-indexed line the function ends on (inclusive). Mirrors
    /// `fallow_cov_protocol::HotPath::end_line` (added in protocol 0.5).
    /// Older 0.4-shape sidecars omit the field on the wire; serde defaults
    /// to `0`, which the line-overlap filter MUST treat as a single-line
    /// range (`line..=line`) rather than a span.
    pub end_line: u32,
    /// Observed invocation count for the hot path.
    pub invocations: u64,
    /// Percentile rank over this response's hot-path distribution. `100`
    /// means the busiest, `0` means the quietest function that qualified.
    pub percentile: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    /// Suggested actions for this hot path (e.g., review-on-change). Omitted
    /// when empty.
    pub actions: Vec<RuntimeCoverageAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Blast-radius risk band. The current thresholds are high at >=20 static
/// callers or >=1,000,000 traffic-weighted caller reach; medium at >=5 callers
/// or >=50,000 weighted reach; low otherwise.
pub enum RuntimeCoverageRiskBand {
    Low,
    Medium,
    High,
}

impl RuntimeCoverageRiskBand {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl fmt::Display for RuntimeCoverageRiskBand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeCoverageBlastRadiusEntry {
    /// Stable content-hash ID of the form `fallow:blast:<hash>`.
    pub id: String,
    /// Cross-surface join key (`fallow:fn:<hash>`) for the function. Stable
    /// across line moves; shared with the same function's findings / hot-path /
    /// importance entries. `null` when no `FunctionIdentity` was supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub stable_id: Option<String>,
    /// File path relative to the project root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// Function name for the blast-radius entry.
    pub function: String,
    /// 1-indexed line number the function starts on.
    pub line: u32,
    /// Static caller count from the module graph.
    pub caller_count: u32,
    /// Caller reach weighted by observed runtime traffic.
    pub caller_count_weighted_by_traffic: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Distinct deploy SHAs that touched the function in the observation
    /// window. Cloud mode only; omitted in local mode.
    pub deploys_touched: Option<u32>,
    pub risk_band: RuntimeCoverageRiskBand,
}

#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeCoverageImportanceEntry {
    /// Stable content-hash ID of the form `fallow:importance:<hash>`.
    pub id: String,
    /// Cross-surface join key (`fallow:fn:<hash>`) for the function. Stable
    /// across line moves; shared with the same function's findings / hot-path /
    /// blast-radius entries. `null` when no `FunctionIdentity` was supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub stable_id: Option<String>,
    /// File path relative to the project root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// Function name for the importance entry.
    pub function: String,
    /// 1-indexed line number the function starts on.
    pub line: u32,
    /// Observed invocation count for this function.
    pub invocations: u64,
    /// Cyclomatic complexity from the static health pipeline.
    pub cyclomatic: u32,
    /// Number of CODEOWNERS owners matched for this file. Zero means no owner
    /// was resolved.
    pub owner_count: u32,
    /// 0-100 explainable score from log-scaled traffic, capped complexity
    /// weight, and ownership-risk weight.
    pub importance_score: f64,
    /// Templated one-sentence explanation for the score.
    pub reason: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
/// Runtime coverage findings merged into the health report or emitted by
/// `fallow coverage analyze`. Present in health output when --runtime-coverage
/// is used. Shape mirrors the runtime coverage JSON contract; cloud mode
/// fetches runtime facts explicitly and merges them locally with AST/static
/// analysis.
pub struct RuntimeCoverageReport {
    /// Runtime coverage JSON contract version. This is scoped to the
    /// `runtime_coverage` block and is independent of the top-level fallow
    /// JSON `schema_version`.
    pub schema_version: RuntimeCoverageSchemaVersion,
    /// Single most actionable runtime-coverage signal under the current
    /// context. In PR-review context (CLI saw `--diff-file` or
    /// `--changed-since`) the verdict is `hot-path-touched` whenever a hot
    /// function was touched, regardless of cold-code findings; in standalone
    /// analysis `cold-code-detected` remains primary. For the full set of
    /// findings the report carries, see `signals`.
    pub verdict: RuntimeCoverageReportVerdict,
    /// All signals captured by post-processing. Independent of `verdict`,
    /// which is the single most actionable signal under the current
    /// context. Empty when the report is `Clean` and not under license
    /// grace. Order is stable severity-descending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub signals: Vec<RuntimeCoverageSignal>,
    /// Aggregate tracked / hit / unhit / untracked counts for the analyzed
    /// runtime coverage input.
    pub summary: RuntimeCoverageSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    /// Surfaced runtime coverage findings (`safe_to_delete`, `review_required`,
    /// `low_traffic`, `coverage_unavailable`). Omitted when empty. `active`
    /// functions stay out of this list so the CLI output remains actionable.
    pub findings: Vec<RuntimeCoverageFinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    /// Top runtime functions by invocation count. Omitted when empty.
    pub hot_paths: Vec<RuntimeCoverageHotPath>,
    /// First-class blast-radius entries for runtime-observed functions. Present
    /// whenever runtime coverage analysis runs.
    pub blast_radius: Vec<RuntimeCoverageBlastRadiusEntry>,
    /// First-class production-importance entries for runtime-observed
    /// functions. Present whenever runtime coverage analysis runs.
    pub importance: Vec<RuntimeCoverageImportanceEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// License/trial watermark for grace-mode output. Omitted when not
    /// applicable.
    pub watermark: Option<RuntimeCoverageWatermark>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    /// Non-fatal merge or coverage diagnostics. Omitted when empty.
    pub warnings: Vec<RuntimeCoverageMessage>,
    /// Whether an autonomous agent may act on this report (fallow-rs/fallow-cloud#316,
    /// mirrors the cloud runtime-context contract). `false` when the capture
    /// carries no usable runtime evidence (no tracked functions); then
    /// `actionability_verdict` is `insufficient_evidence` and
    /// `actionability_reason` explains. F4: a non-action floor, never a gate on a
    /// positive verdict.
    pub actionable: bool,
    /// Why the report is non-actionable; `null` when `actionable` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub actionability_reason: Option<String>,
    /// First-class non-action verdict (`insufficient_evidence`) when not
    /// actionable; `null` otherwise. Mirrors the cloud runtime-context `verdict`;
    /// named distinctly from the report-context `verdict` above to avoid a
    /// collision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub actionability_verdict: Option<String>,
    /// Provenance an agent reads to self-attenuate confidence (fallow-rs/fallow-cloud#319,
    /// mirrors the cloud runtime-context `provenance`). F4: context only.
    pub provenance: RuntimeCoverageProvenance,
}

/// Provenance of a runtime-coverage report (fallow-rs/fallow-cloud#319), mirroring
/// the cloud runtime-context `provenance` block so the local-capture and cloud
/// surfaces present one portable shape. F4: provenance is context only; it never
/// gates a verdict or confidence.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RuntimeCoverageProvenance {
    /// `local` for a local capture, `cloud` for the cloud read path.
    pub data_source: RuntimeCoverageDataSource,
    /// `true` / `false` / `unknown`. Always `unknown` for a local capture: the
    /// local path has no deployment-origin signal (the cloud may resolve it).
    pub is_production: String,
    /// Age in whole days of the most recent evidence; `0` for a fresh local
    /// capture, `null` when no runtime data is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub freshness_days: Option<u32>,
    /// `functions_untracked / (functions_tracked + functions_untracked)`, in
    /// `[0, 1]`. High ratios mark a thin / partial capture.
    pub untracked_ratio: f64,
    /// Fraction of resolution-attempted functions whose position could not be
    /// mapped to source, in `[0, 1]`. `0` for a local capture (positions resolve
    /// natively or via the sidecar).
    pub unresolved_ratio: f64,
    /// Whether `freshness_days` exceeds `stale_after_days`.
    pub stale: bool,
    /// The documented staleness cutoff (days), echoed so the rule travels in-band.
    pub stale_after_days: u32,
}

impl Default for RuntimeCoverageProvenance {
    fn default() -> Self {
        Self {
            data_source: RuntimeCoverageDataSource::Local,
            is_production: "unknown".to_owned(),
            freshness_days: Some(0),
            untracked_ratio: 0.0,
            unresolved_ratio: 0.0,
            stale: false,
            stale_after_days: RUNTIME_STALE_AFTER_DAYS,
        }
    }
}

/// Staleness cutoff in days, mirrored from the cloud runtime-context
/// (`RUNTIME_STALE_AFTER_DAYS`) so the local and cloud contracts agree.
pub const RUNTIME_STALE_AFTER_DAYS: u32 = 14;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_verdict_display_matches_kebab_case_serde() {
        assert_eq!(RuntimeCoverageReportVerdict::Clean.to_string(), "clean");
        assert_eq!(
            RuntimeCoverageReportVerdict::HotPathTouched.to_string(),
            "hot-path-touched",
        );
        assert_eq!(
            RuntimeCoverageReportVerdict::ColdCodeDetected.to_string(),
            "cold-code-detected",
        );
        assert_eq!(
            RuntimeCoverageReportVerdict::LicenseExpiredGrace.to_string(),
            "license-expired-grace",
        );
        assert_eq!(RuntimeCoverageReportVerdict::Unknown.to_string(), "unknown",);
    }

    #[test]
    fn verdict_display_matches_snake_case_serde() {
        assert_eq!(
            RuntimeCoverageVerdict::SafeToDelete.to_string(),
            "safe_to_delete",
        );
        assert_eq!(
            RuntimeCoverageVerdict::ReviewRequired.to_string(),
            "review_required",
        );
        assert_eq!(
            RuntimeCoverageVerdict::CoverageUnavailable.to_string(),
            "coverage_unavailable",
        );
        assert_eq!(
            RuntimeCoverageVerdict::LowTraffic.to_string(),
            "low_traffic",
        );
        assert_eq!(RuntimeCoverageVerdict::Active.to_string(), "active");
    }

    #[test]
    fn confidence_display_matches_snake_case_serde() {
        assert_eq!(RuntimeCoverageConfidence::VeryHigh.to_string(), "very_high",);
        assert_eq!(RuntimeCoverageConfidence::High.to_string(), "high");
        assert_eq!(RuntimeCoverageConfidence::Medium.to_string(), "medium");
        assert_eq!(RuntimeCoverageConfidence::Low.to_string(), "low");
        assert_eq!(RuntimeCoverageConfidence::None.to_string(), "none");
        assert_eq!(RuntimeCoverageConfidence::Unknown.to_string(), "unknown");
    }

    #[test]
    fn watermark_display_matches_kebab_case_serde() {
        assert_eq!(
            RuntimeCoverageWatermark::TrialExpired.to_string(),
            "trial-expired",
        );
        assert_eq!(
            RuntimeCoverageWatermark::LicenseExpiredGrace.to_string(),
            "license-expired-grace",
        );
    }

    #[test]
    fn action_serializes_kind_as_type() {
        let action = RuntimeCoverageAction {
            kind: "review-deletion".to_owned(),
            description: "Remove the function.".to_owned(),
            auto_fixable: false,
        };
        let value = serde_json::to_value(&action).expect("action should serialize");
        assert_eq!(value["type"], "review-deletion");
        assert!(
            value.get("kind").is_none(),
            "kind should be renamed to type"
        );
    }
}
