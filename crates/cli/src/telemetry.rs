//! Opt-in product telemetry for agent and CI workflow quality.
//!
//! The payload is intentionally small and allowlisted. It must never include
//! repository names, paths, package names, config values, raw command lines, or
//! raw agent-detection evidence.

use std::ffi::OsString;
use std::io::{IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU8, AtomicU16, Ordering};
use std::time::Duration;

use fallow_config::OutputFormat;
use serde::{Deserialize, Serialize};

use crate::api::{api_url, try_api_agent_with_timeout};

const CONFIG_SCHEMA_VERSION: u8 = 1;
const TELEMETRY_SCHEMA_VERSION: u8 = 2;
const CONNECT_TIMEOUT_SECS: u64 = 1;
const TOTAL_TIMEOUT_SECS: u64 = 1;
const TELEMETRY_PATH: &str = "/v1/telemetry/events";
const PARENT_RUN_HEADER: &str = "X-Fallow-Parent-Run";

/// Maximum number of events retained in the spool. The spool grows one line per
/// telemetry-enabled run and is drained on the next run. Two paths keep it to the
/// newest `SPOOL_MAX_EVENTS` lines: the drain caps the undelivered tail once it
/// has delivered part of a backlog, and the over-cap write-path trim
/// ([`SPOOL_MAX_BYTES`]) bounds it even on a machine where delivery never
/// succeeds, so the oldest events are dropped rather than letting the file grow.
const SPOOL_MAX_EVENTS: usize = 64;
/// Hot-path size ceiling for the spool, checked with a single `fstat` on each
/// append. When the live spool grows past it (a machine that stays offline, so
/// the background drain never delivers and never trims), the append rewrites the
/// file down to the newest `SPOOL_MAX_EVENTS` lines. This bounds the spool
/// unconditionally, independent of whether any drain ever completes, so a fast
/// command whose drain is always abandoned mid-upload still cannot grow it. Set
/// well above `SPOOL_MAX_EVENTS` coarse events so a normal append never trims.
const SPOOL_MAX_BYTES: u64 = 64 * 1024;
/// Live append-only spool of events awaiting upload, next to `telemetry.json`.
const SPOOL_FILE_NAME: &str = "telemetry-spool.jsonl";
/// Advisory `flock` sidecar serialising drains/trims across concurrent processes.
const SPOOL_LOCK_NAME: &str = "telemetry-spool.lock";

const DO_NOT_TRACK: &str = "DO_NOT_TRACK";
const DISABLED_ENV: &str = "FALLOW_TELEMETRY_DISABLED";
const MODE_ENV: &str = "FALLOW_TELEMETRY";
const DEBUG_ENV: &str = "FALLOW_TELEMETRY_DEBUG";
const AGENT_SOURCE_ENV: &str = "FALLOW_AGENT_SOURCE";
const INTEGRATION_SURFACE_ENV: &str = "FALLOW_INTEGRATION_SURFACE";
const MCP_TOOL_ENV: &str = "FALLOW_MCP_TOOL";

/// Allowlist of MCP tool names accepted in the `mcp_tool` dimension. Anything
/// outside this set is dropped (the field becomes absent) so a user-set or
/// adversarial `FALLOW_MCP_TOOL` value can never inject a free-form string into
/// the allowlisted payload. Kept in sync with the tools the MCP server exposes
/// (see `crates/mcp/src/server/mod.rs`).
const MCP_TOOLS: &[&str] = &[
    "analyze",
    "check_changed",
    "security_candidates",
    "find_dupes",
    "check_health",
    "check_runtime_coverage",
    "get_hot_paths",
    "get_blast_radius",
    "get_importance",
    "get_cleanup_candidates",
    "audit",
    "fallow_explain",
    "fix_preview",
    "fix_apply",
    "project_info",
    "list_boundaries",
    "feature_flags",
    "impact",
    "trace_export",
    "trace_file",
    "trace_dependency",
    "trace_clone",
];

/// Process-wide accumulator for whether the analysis that ran this invocation
/// actually surfaced any findings, independent of the exit-code gate.
///
/// `outcome` is derived purely from the exit code, but several analyses are
/// informational (non-gating) under their default config: `fallow dupes`
/// exits 0 even at 100% duplication because the default duplication threshold
/// is `0.0` ("never gate"). So the exit-code-derived `outcome` cannot tell a
/// genuinely-clean dupes run from one that surfaced clones. This accumulator
/// lets each analysis report findings presence from its real result.
///
/// States: `0` = unset (no analysis reported), `1` = ran and found nothing,
/// `2` = ran and found something. `fetch_max` gives OR semantics for free in
/// combined mode (bare `fallow` runs check + dupes + health), where "found
/// something" wins. The accumulator assumes one analysis batch per process
/// (the CLI one-shot model); an in-process embedder running several batches
/// would see the bit stick at the max across all of them.
static FINDINGS_PRESENT: AtomicU8 = AtomicU8::new(FINDINGS_UNSET);
static FAILURE_REASON: AtomicU8 = AtomicU8::new(FAILURE_REASON_UNSET);
static RESULT_COUNT_CAPPED: AtomicU16 = AtomicU16::new(RESULT_COUNT_UNSET);
static REPORT_TRUNCATED: AtomicU8 = AtomicU8::new(REPORT_TRUNCATION_UNSET);
static TRUNCATION_REASON: AtomicU8 = AtomicU8::new(TRUNCATION_REASON_UNSET);
static CACHE_STATE: AtomicU8 = AtomicU8::new(CACHE_STATE_UNSET);
static CONFIG_SHAPE: AtomicU8 = AtomicU8::new(CONFIG_SHAPE_UNSET);

const FINDINGS_UNSET: u8 = 0;
const FINDINGS_CLEAN: u8 = 1;
const FINDINGS_FOUND: u8 = 2;
const FAILURE_REASON_UNSET: u8 = 0;
const FAILURE_REASON_UNKNOWN: u8 = 1;
const FAILURE_REASON_VALIDATION: u8 = 2;
const FAILURE_REASON_UNSUPPORTED_FORMAT: u8 = 3;
const FAILURE_REASON_CONFIG: u8 = 4;
const FAILURE_REASON_ANALYSIS: u8 = 5;
const FAILURE_REASON_DIFF: u8 = 6;
const FAILURE_REASON_NETWORK: u8 = 7;
const FAILURE_REASON_AUTH: u8 = 8;
const FAILURE_REASON_GATE: u8 = 9;
const FAILURE_REASON_SIGNAL: u8 = 10;
const RESULT_COUNT_MAX: usize = 100;
const RESULT_COUNT_UNSET: u16 = u16::MAX;
const RESULT_COUNT_UNKNOWN: u16 = u16::MAX - 1;
const REPORT_TRUNCATION_UNSET: u8 = 0;
const REPORT_TRUNCATION_FALSE: u8 = 1;
const REPORT_TRUNCATION_TRUE: u8 = 2;
const TRUNCATION_REASON_UNSET: u8 = 0;
const TRUNCATION_REASON_UNKNOWN: u8 = 1;
const TRUNCATION_REASON_MAX_ITEMS: u8 = 2;
const TRUNCATION_REASON_COMMENT_LIMIT: u8 = 3;
const TRUNCATION_REASON_SIZE_LIMIT: u8 = 4;
const CACHE_STATE_UNSET: u8 = 0;
const CACHE_STATE_COLD: u8 = 1;
const CACHE_STATE_WARM: u8 = 2;
const CACHE_STATE_PARTIAL: u8 = 3;
const CACHE_STATE_UNKNOWN: u8 = 4;
const CONFIG_SHAPE_UNSET: u8 = 0;
const CONFIG_SHAPE_UNKNOWN: u8 = 1;
const CONFIG_SHAPE_DEFAULT: u8 = 2;
const CONFIG_SHAPE_CUSTOM_CONFIG: u8 = 3;
const CONFIG_SHAPE_CUSTOM_RULES: u8 = 4;
const CONFIG_SHAPE_PLUGINS_ENABLED: u8 = 5;

/// Record whether the analysis that just completed surfaced any findings.
///
/// Called from each analysis `execute` path with the real result
/// (`results.total_issues() > 0`, clone groups present, non-empty health
/// findings, etc.), independent of the exit code. Safe to call repeatedly; the
/// "found something" state is sticky so combined-mode sub-analyses OR together.
pub fn note_findings_present(present: bool) {
    let value = if present {
        FINDINGS_FOUND
    } else {
        FINDINGS_CLEAN
    };
    FINDINGS_PRESENT.fetch_max(value, Ordering::Relaxed);
    if present {
        mark_result_count_unknown();
    } else {
        note_result_count(0);
    }
}

/// Record the number of findings in the analysis batch that just completed.
///
/// The exact count is used only in-process to combine sub-analyses from a
/// single CLI invocation. The event serializes only the coarse bucket.
pub fn note_result_count(count: usize) {
    let value = if count > 0 {
        FINDINGS_FOUND
    } else {
        FINDINGS_CLEAN
    };
    FINDINGS_PRESENT.fetch_max(value, Ordering::Relaxed);

    let capped = count.min(RESULT_COUNT_MAX) as u16;
    let _ = RESULT_COUNT_CAPPED.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        let next = match current {
            RESULT_COUNT_UNSET => capped,
            RESULT_COUNT_UNKNOWN => RESULT_COUNT_UNKNOWN,
            value => value.saturating_add(capped).min(RESULT_COUNT_MAX as u16),
        };
        Some(next)
    });
}

/// Record a final command-level count after nested analysis helpers ran.
pub fn note_final_result_count(count: usize) {
    let value = if count > 0 {
        FINDINGS_FOUND
    } else {
        FINDINGS_CLEAN
    };
    FINDINGS_PRESENT.fetch_max(value, Ordering::Relaxed);
    RESULT_COUNT_CAPPED.store(count.min(RESULT_COUNT_MAX) as u16, Ordering::Relaxed);
}

fn mark_result_count_unknown() {
    RESULT_COUNT_CAPPED.store(RESULT_COUNT_UNKNOWN, Ordering::Relaxed);
}

/// Record whether a report/comment style output path was truncated.
pub fn note_report_truncation(truncated: bool, reason: TruncationReason) {
    if truncated {
        REPORT_TRUNCATED.store(REPORT_TRUNCATION_TRUE, Ordering::Relaxed);
        let reason_state = truncation_reason_to_state(reason);
        let _ = TRUNCATION_REASON.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.max(reason_state))
        });
    } else {
        let _ = REPORT_TRUNCATED.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            if current == REPORT_TRUNCATION_UNSET {
                Some(REPORT_TRUNCATION_FALSE)
            } else {
                Some(current)
            }
        });
    }
}

/// Map the accumulator state to the optional payload field. Pure so it can be
/// unit-tested without touching the process-global atomic.
fn findings_present_from_state(state: u8) -> Option<bool> {
    match state {
        FINDINGS_CLEAN => Some(false),
        FINDINGS_FOUND => Some(true),
        _ => None,
    }
}

fn findings_present() -> Option<bool> {
    findings_present_from_state(FINDINGS_PRESENT.load(Ordering::Relaxed))
}

/// Record the coarse loaded configuration shape after config resolution.
///
/// The most specific shape wins. This stores only fixed enum buckets and never
/// records config paths, rule names, plugin names, or config values.
pub fn note_config_shape(shape: ConfigShape) {
    CONFIG_SHAPE.fetch_max(config_shape_rank(shape), Ordering::Relaxed);
}

fn config_shape_rank(shape: ConfigShape) -> u8 {
    match shape {
        ConfigShape::Unknown => CONFIG_SHAPE_UNKNOWN,
        ConfigShape::Default => CONFIG_SHAPE_DEFAULT,
        ConfigShape::CustomConfig => CONFIG_SHAPE_CUSTOM_CONFIG,
        ConfigShape::CustomRules => CONFIG_SHAPE_CUSTOM_RULES,
        ConfigShape::PluginsEnabled => CONFIG_SHAPE_PLUGINS_ENABLED,
    }
}

fn config_shape_from_state(state: u8) -> Option<ConfigShape> {
    match state {
        CONFIG_SHAPE_UNKNOWN => Some(ConfigShape::Unknown),
        CONFIG_SHAPE_DEFAULT => Some(ConfigShape::Default),
        CONFIG_SHAPE_CUSTOM_CONFIG => Some(ConfigShape::CustomConfig),
        CONFIG_SHAPE_CUSTOM_RULES => Some(ConfigShape::CustomRules),
        CONFIG_SHAPE_PLUGINS_ENABLED => Some(ConfigShape::PluginsEnabled),
        _ => None,
    }
}

fn noted_config_shape() -> Option<ConfigShape> {
    config_shape_from_state(CONFIG_SHAPE.load(Ordering::Relaxed))
}

fn config_shape_for_record(record: &WorkflowRecord<'_>) -> ConfigShape {
    noted_config_shape().unwrap_or(record.context.config_shape)
}

/// Coarse allowlisted reason for a failed workflow telemetry event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    Validation,
    UnsupportedFormat,
    Config,
    Analysis,
    Diff,
    Network,
    Auth,
    Gate,
    Signal,
    Unknown,
}

impl FailureReason {
    const fn state(self) -> u8 {
        match self {
            Self::Validation => FAILURE_REASON_VALIDATION,
            Self::UnsupportedFormat => FAILURE_REASON_UNSUPPORTED_FORMAT,
            Self::Config => FAILURE_REASON_CONFIG,
            Self::Analysis => FAILURE_REASON_ANALYSIS,
            Self::Diff => FAILURE_REASON_DIFF,
            Self::Network => FAILURE_REASON_NETWORK,
            Self::Auth => FAILURE_REASON_AUTH,
            Self::Gate => FAILURE_REASON_GATE,
            Self::Signal => FAILURE_REASON_SIGNAL,
            Self::Unknown => FAILURE_REASON_UNKNOWN,
        }
    }
}

/// Record a coarse failure reason without storing raw error text.
///
/// The first known reason wins. A later known reason may replace `unknown`, but
/// otherwise earlier domain knowledge is kept so downstream code cannot
/// accidentally overwrite a specific bucket with a generic one.
pub fn note_failure_reason(reason: FailureReason) {
    let next = reason.state();
    let mut current = FAILURE_REASON.load(Ordering::Relaxed);
    loop {
        let should_update = current == FAILURE_REASON_UNSET
            || (current == FAILURE_REASON_UNKNOWN && next != FAILURE_REASON_UNKNOWN);
        if !should_update {
            return;
        }
        match FAILURE_REASON.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(actual) => current = actual,
        }
    }
}

fn failure_reason_from_state(state: u8) -> Option<FailureReason> {
    match state {
        FAILURE_REASON_UNKNOWN => Some(FailureReason::Unknown),
        FAILURE_REASON_VALIDATION => Some(FailureReason::Validation),
        FAILURE_REASON_UNSUPPORTED_FORMAT => Some(FailureReason::UnsupportedFormat),
        FAILURE_REASON_CONFIG => Some(FailureReason::Config),
        FAILURE_REASON_ANALYSIS => Some(FailureReason::Analysis),
        FAILURE_REASON_DIFF => Some(FailureReason::Diff),
        FAILURE_REASON_NETWORK => Some(FailureReason::Network),
        FAILURE_REASON_AUTH => Some(FailureReason::Auth),
        FAILURE_REASON_GATE => Some(FailureReason::Gate),
        FAILURE_REASON_SIGNAL => Some(FailureReason::Signal),
        _ => None,
    }
}

fn failure_reason() -> Option<FailureReason> {
    failure_reason_from_state(FAILURE_REASON.load(Ordering::Relaxed))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ResultCountBucket {
    #[serde(rename = "0")]
    Zero,
    #[serde(rename = "1-9")]
    OneToNine,
    #[serde(rename = "10-99")]
    TenToNinetyNine,
    #[serde(rename = "100+")]
    OneHundredPlus,
    #[serde(rename = "unknown")]
    Unknown,
}

fn result_count_bucket_from_state(state: u16) -> Option<ResultCountBucket> {
    match state {
        RESULT_COUNT_UNSET => None,
        RESULT_COUNT_UNKNOWN => Some(ResultCountBucket::Unknown),
        0 => Some(ResultCountBucket::Zero),
        1..=9 => Some(ResultCountBucket::OneToNine),
        10..=99 => Some(ResultCountBucket::TenToNinetyNine),
        _ => Some(ResultCountBucket::OneHundredPlus),
    }
}

fn result_count_bucket() -> Option<ResultCountBucket> {
    result_count_bucket_from_state(RESULT_COUNT_CAPPED.load(Ordering::Relaxed))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TruncationReason {
    CommentLimit,
    MaxItems,
    SizeLimit,
    Unknown,
}

fn truncation_reason_to_state(reason: TruncationReason) -> u8 {
    match reason {
        TruncationReason::Unknown => TRUNCATION_REASON_UNKNOWN,
        TruncationReason::MaxItems => TRUNCATION_REASON_MAX_ITEMS,
        TruncationReason::CommentLimit => TRUNCATION_REASON_COMMENT_LIMIT,
        TruncationReason::SizeLimit => TRUNCATION_REASON_SIZE_LIMIT,
    }
}

fn truncation_reason_from_state(state: u8) -> Option<TruncationReason> {
    match state {
        TRUNCATION_REASON_UNKNOWN => Some(TruncationReason::Unknown),
        TRUNCATION_REASON_MAX_ITEMS => Some(TruncationReason::MaxItems),
        TRUNCATION_REASON_COMMENT_LIMIT => Some(TruncationReason::CommentLimit),
        TRUNCATION_REASON_SIZE_LIMIT => Some(TruncationReason::SizeLimit),
        _ => None,
    }
}

fn report_truncated_from_state(state: u8) -> Option<bool> {
    match state {
        REPORT_TRUNCATION_FALSE => Some(false),
        REPORT_TRUNCATION_TRUE => Some(true),
        _ => None,
    }
}

fn report_truncated() -> Option<bool> {
    report_truncated_from_state(REPORT_TRUNCATED.load(Ordering::Relaxed))
}

fn truncation_reason() -> Option<TruncationReason> {
    if report_truncated() != Some(true) {
        return None;
    }
    truncation_reason_from_state(TRUNCATION_REASON.load(Ordering::Relaxed))
        .or(Some(TruncationReason::Unknown))
}

/// Record a coarse cache state from aggregate cache counts.
///
/// Only the derived enum is serialized. Raw hit and miss counts remain local.
pub fn note_cache_state(cache_hits: usize, cache_misses: usize) {
    let value = match (cache_hits, cache_misses) {
        (0, 0) => CACHE_STATE_UNKNOWN,
        (0, _) => CACHE_STATE_COLD,
        (_, 0) => CACHE_STATE_WARM,
        (_, _) => CACHE_STATE_PARTIAL,
    };
    CACHE_STATE.store(value, Ordering::Relaxed);
}

pub fn note_cache_state_unknown() {
    CACHE_STATE.store(CACHE_STATE_UNKNOWN, Ordering::Relaxed);
}

fn cache_state_from_state(state: u8) -> Option<CacheState> {
    match state {
        CACHE_STATE_COLD => Some(CacheState::Cold),
        CACHE_STATE_WARM => Some(CacheState::Warm),
        CACHE_STATE_PARTIAL => Some(CacheState::Partial),
        CACHE_STATE_UNKNOWN => Some(CacheState::Unknown),
        _ => None,
    }
}

fn cache_state() -> Option<CacheState> {
    cache_state_from_state(CACHE_STATE.load(Ordering::Relaxed))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CacheState {
    Cold,
    Warm,
    Partial,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryCommand {
    Status,
    Enable,
    Disable,
    Inspect { example: bool },
}

#[expect(
    dead_code,
    reason = "telemetry schema reserves v1 values for LSP/editor/programmatic surfaces before every surface is wired"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Workflow {
    Audit,
    DeadCode,
    Health,
    Dupes,
    DependencyCleanup,
    CodeQualityReview,
    GithubAction,
    GitlabCi,
    EditorDiagnostic,
    ProgrammaticAnalysis,
    RuntimeCoverageSetup,
    Impact,
    Security,
    Fix,
    Explain,
    ProjectInventory,
    Setup,
    License,
    Unknown,
}

#[expect(
    dead_code,
    reason = "telemetry schema reserves v1 values for LSP/editor/programmatic surfaces before every surface is wired"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationSurface {
    CliHuman,
    CliJson,
    Mcp,
    Lsp,
    Vscode,
    GithubAction,
    GitlabCi,
    Napi,
    Programmatic,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationContext {
    Human,
    Agent,
    Ci,
    Editor,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunScope {
    FullProject,
    ChangedOnly,
    WorkspaceScoped,
    FileScoped,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigShape {
    Default,
    CustomConfig,
    CustomRules,
    PluginsEnabled,
    Unknown,
}

#[expect(
    dead_code,
    reason = "telemetry schema reserves v1 destination values before every sink is wired"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputDestination {
    Stdout,
    File,
    CiComment,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMode {
    Static,
    RuntimeCoverage,
    ProductionCoverage,
    Security,
    Fix,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkflowContext {
    pub run_scope: RunScope,
    pub config_shape: ConfigShape,
    pub output_destination: OutputDestination,
    pub analysis_mode: AnalysisMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSource {
    None,
    Codex,
    ClaudeCode,
    Cursor,
    Copilot,
    Opencode,
    Aider,
    Roo,
    Windsurf,
    Gemini,
    Cline,
    Continue,
    Zed,
    Goose,
    OtherKnown,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RunRole {
    Root,
    Followup,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FollowupKind {
    Audit,
    Security,
    Health,
    Check,
    Dupes,
    Fix,
    Explain,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EffectiveMode {
    Off,
    On,
    Inspect,
    DisabledByAdmin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModeSource {
    AdminEnv,
    Env,
    UserConfig,
    Default,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TelemetryConfig {
    schema_version: u8,
    enabled: bool,
    prompt_shown: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            enabled: false,
            prompt_shown: false,
        }
    }
}

#[derive(Debug)]
struct EffectiveConfig {
    mode: EffectiveMode,
    source: ModeSource,
    config_path: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct TelemetryEvent {
    schema_version: u8,
    event: &'static str,
    fallow_version: &'static str,
    workflow: Workflow,
    integration_surface: IntegrationSurface,
    invocation_context: InvocationContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_source: Option<AgentSource>,
    output_format: &'static str,
    quiet: bool,
    ci: bool,
    tty: bool,
    os: &'static str,
    arch: &'static str,
    duration_bucket_ms: &'static str,
    outcome: &'static str,
    exit_code_bucket: &'static str,
    /// Coarse allowlisted failure class. Present only on `workflow_failed`
    /// events and never derived from raw error text.
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_reason: Option<FailureReason>,
    /// Coarse scope of the analyzed files. Never includes file, workspace,
    /// branch, or ref names.
    #[serde(skip_serializing_if = "Option::is_none")]
    run_scope: Option<RunScope>,
    /// Coarse shape of the loaded configuration. Never includes paths, rule
    /// names, plugin names, or config values.
    #[serde(skip_serializing_if = "Option::is_none")]
    config_shape: Option<ConfigShape>,
    /// Coarse report destination bucket. Never includes destination paths,
    /// URLs, or integration identifiers.
    #[serde(skip_serializing_if = "Option::is_none")]
    output_destination: Option<OutputDestination>,
    /// Coarse analysis family. Never includes raw command lines or paths to
    /// coverage artifacts.
    #[serde(skip_serializing_if = "Option::is_none")]
    analysis_mode: Option<AnalysisMode>,
    /// Whether the analysis surfaced any findings, independent of the exit-code
    /// `outcome` gate. Absent on commands that run no analysis (admin commands)
    /// and on older binaries. On the combined `code_quality_review` and `audit`
    /// workflows this is an OR across the sub-analyses; per-analysis find-rate
    /// is answerable only on the standalone `dead_code` / `dupes` / `health`
    /// workflows.
    #[serde(skip_serializing_if = "Option::is_none")]
    findings_present: Option<bool>,
    /// Coarse bucket of the rendered analysis result count. Never serializes
    /// exact counts, paths, finding names, rule ids, or snippets.
    #[serde(skip_serializing_if = "Option::is_none")]
    result_count_bucket: Option<ResultCountBucket>,
    /// Whether a report/comment style output path was truncated. Absent on
    /// output formats that have no truncation-aware reporting path.
    #[serde(skip_serializing_if = "Option::is_none")]
    report_truncated: Option<bool>,
    /// Why a report/comment style output path was truncated. Only present when
    /// `report_truncated` is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    truncation_reason: Option<TruncationReason>,
    /// Coarse cache state for combined `code_quality_review` runs.
    /// Derived from aggregate cache hits and misses, never raw counts or paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_state: Option<CacheState>,
    /// The MCP tool that triggered this run, when invoked through the MCP
    /// server. Allowlisted to the fixed set of tool names; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    mcp_tool: Option<&'static str>,
    has_parent_run: bool,
    run_role: RunRole,
    followup_kind: FollowupKind,
}

#[derive(Clone, Debug)]
struct ParentRunContext {
    token: Option<String>,
    has_parent_run: bool,
    run_role: RunRole,
    followup_kind: FollowupKind,
}

pub struct WorkflowRecord<'a> {
    pub workflow: Workflow,
    pub output: OutputFormat,
    pub quiet: bool,
    pub elapsed: Duration,
    pub exit_code: ExitCode,
    pub failure_reason: Option<FailureReason>,
    pub parent_run: Option<&'a str>,
    pub context: WorkflowContext,
}

pub fn run(command: TelemetryCommand, output: OutputFormat) -> ExitCode {
    match command {
        TelemetryCommand::Status => print_status(output),
        TelemetryCommand::Enable => set_enabled(true, output),
        TelemetryCommand::Disable => set_enabled(false, output),
        TelemetryCommand::Inspect { example } => inspect(example, output),
    }
}

pub fn record_workflow(record: &WorkflowRecord<'_>) {
    let parent_run = parent_run_context(record.parent_run, record.workflow);
    let event = build_workflow_event(record, &parent_run);
    match effective_config().mode {
        EffectiveMode::Off | EffectiveMode::DisabledByAdmin => {}
        EffectiveMode::Inspect => print_event_to_stderr(&event),
        EffectiveMode::On => spool_event(&event, parent_run.token.as_deref()),
    }
}

/// Print the one-time telemetry opt-in note if this is the first eligible run.
///
/// Returns `true` when the note was printed this run, so the caller can enforce
/// mutual exclusion with the upgrade nudge (at most one unsolicited stderr
/// notice per run; the consent-bearing note wins).
pub fn maybe_print_opt_in_note(output: OutputFormat, quiet: bool) -> bool {
    if quiet
        || !matches!(output, OutputFormat::Human)
        || !std::io::stderr().is_terminal()
        || admin_disabled()
    {
        return false;
    }
    let Some(path) = config_path() else {
        return false;
    };
    let mut config = read_config_from(&path).unwrap_or_default();
    if config.enabled || config.prompt_shown {
        return false;
    }
    config.prompt_shown = true;
    let _ = write_config_to(&path, &config);
    eprintln!(
        "Help improve Fallow's agent and CI workflows with minimal, allowlisted opt-in telemetry.\n\
         No repository names, paths, package names, source code, config values, or raw errors are collected.\n\
         Inspect the exact payload: FALLOW_TELEMETRY=inspect fallow audit --format json --quiet\n\
         Enable it: fallow telemetry enable\n\
         This notice is shown once; your preference (still off) is stored at {}",
        path.display()
    );
    true
}

fn print_status(output: OutputFormat) -> ExitCode {
    let effective = effective_config();
    let state = mode_label(effective.mode);
    let source = source_label(effective.source);
    match output {
        OutputFormat::Json => {
            let value = serde_json::json!({
                "telemetry": {
                    "state": state,
                    "source": source,
                    "config_path": effective.config_path.as_ref().map(|p| p.display().to_string()),
                    "admin_disabled": matches!(effective.mode, EffectiveMode::DisabledByAdmin),
                    "commands": {
                        "enable": "fallow telemetry enable",
                        "disable": "fallow telemetry disable",
                        "inspect_example": "fallow telemetry inspect --example",
                        "inspect_command": "FALLOW_TELEMETRY=inspect fallow audit --format json --quiet"
                    },
                    "docs": "docs/telemetry.md"
                }
            });
            crate::report::emit_json(&value, "telemetry status")
        }
        _ => {
            println!("Telemetry: {state} ({source})");
            if let Some(path) = effective.config_path {
                println!("Config: {}", path.display());
            }
            println!("Enable:  fallow telemetry enable");
            println!("Disable: fallow telemetry disable");
            println!("Inspect an example: fallow telemetry inspect --example");
            println!(
                "Inspect a real command: FALLOW_TELEMETRY=inspect fallow audit --format json --quiet"
            );
            println!("Docs: docs/telemetry.md");
            ExitCode::SUCCESS
        }
    }
}

fn set_enabled(enabled: bool, output: OutputFormat) -> ExitCode {
    if admin_disabled() && enabled {
        return crate::error::emit_error(
            "telemetry is disabled by DO_NOT_TRACK or FALLOW_TELEMETRY_DISABLED",
            2,
            output,
        );
    }
    let Some(path) = config_path() else {
        return crate::error::emit_error("could not determine user config directory", 2, output);
    };
    let mut config = read_config_from(&path).unwrap_or_default();
    config.enabled = enabled;
    config.prompt_shown = true;
    if let Err(err) = write_config_to(&path, &config) {
        return crate::error::emit_error(
            &format!("failed to write telemetry config: {err}"),
            2,
            output,
        );
    }
    let event = status_changed_event(enabled);
    if enabled {
        match effective_config().mode {
            EffectiveMode::Inspect => print_event_to_stderr(&event),
            EffectiveMode::On => spool_event(&event, None),
            EffectiveMode::Off | EffectiveMode::DisabledByAdmin => {}
        }
    }
    match output {
        OutputFormat::Json => {
            let value = serde_json::json!({
                "telemetry": {
                    "state": if enabled { "on" } else { "off" },
                    "config_path": path.display().to_string()
                }
            });
            crate::report::emit_json(&value, "telemetry config")
        }
        _ => {
            println!(
                "Telemetry {}.",
                if enabled { "enabled" } else { "disabled" }
            );
            println!("Config: {}", path.display());
            ExitCode::SUCCESS
        }
    }
}

fn inspect(example: bool, output: OutputFormat) -> ExitCode {
    if !example {
        match output {
            OutputFormat::Json => {
                let value = serde_json::json!({
                    "telemetry": {
                        "state": mode_label(effective_config().mode),
                        "inspect_real_command": "FALLOW_TELEMETRY=inspect fallow audit --format json --quiet",
                        "example_command": "fallow telemetry inspect --example"
                    }
                });
                return crate::report::emit_json(&value, "telemetry inspect");
            }
            _ => {
                println!(
                    "To inspect the exact payload for a real command, prefix it with FALLOW_TELEMETRY=inspect:"
                );
                println!("  FALLOW_TELEMETRY=inspect fallow audit --format json --quiet");
                println!();
                println!("To print documented example payloads:");
                println!("  fallow telemetry inspect --example");
                return ExitCode::SUCCESS;
            }
        }
    }

    let event = example_event();
    match output {
        OutputFormat::Json => {
            let value = serde_json::json!({
                "example": event,
                "field_purposes": field_purposes(),
            });
            crate::report::emit_json(&value, "telemetry inspect")
        }
        _ => {
            println!(
                "{}",
                serde_json::to_string_pretty(&event)
                    .unwrap_or_else(|_| "{\"error\":\"example serialization failed\"}".to_owned())
            );
            println!();
            println!("Field purposes:");
            for (field, purpose) in field_purposes() {
                println!("- {field}: {purpose}");
            }
            ExitCode::SUCCESS
        }
    }
}

fn build_workflow_event(
    record: &WorkflowRecord<'_>,
    parent_run: &ParentRunContext,
) -> TelemetryEvent {
    let invocation_context = classify_invocation_context();
    let agent_source = if invocation_context == InvocationContext::Agent {
        Some(classify_agent_source())
    } else {
        None
    };
    TelemetryEvent {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        event: if is_failed(record.exit_code) {
            "workflow_failed"
        } else {
            "workflow_completed"
        },
        fallow_version: env!("CARGO_PKG_VERSION"),
        workflow: record.workflow,
        integration_surface: integration_surface(record.output),
        invocation_context,
        agent_source,
        output_format: output_format_label(record.output),
        quiet: record.quiet,
        ci: is_ci(),
        tty: std::io::stdout().is_terminal(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        duration_bucket_ms: duration_bucket(record.elapsed),
        outcome: outcome(record.exit_code),
        exit_code_bucket: exit_code_bucket(record.exit_code),
        failure_reason: failure_reason_for(record),
        run_scope: Some(record.context.run_scope),
        config_shape: Some(config_shape_for_record(record)),
        output_destination: Some(record.context.output_destination),
        analysis_mode: Some(record.context.analysis_mode),
        findings_present: findings_present(),
        result_count_bucket: result_count_bucket(),
        report_truncated: report_truncated(),
        truncation_reason: truncation_reason(),
        cache_state: cache_state(),
        mcp_tool: mcp_tool(),
        has_parent_run: parent_run.has_parent_run,
        run_role: parent_run.run_role,
        followup_kind: parent_run.followup_kind,
    }
}

fn failure_reason_for(record: &WorkflowRecord<'_>) -> Option<FailureReason> {
    failure_reason_for_value(record, failure_reason())
}

fn failure_reason_for_value(
    record: &WorkflowRecord<'_>,
    recorded: Option<FailureReason>,
) -> Option<FailureReason> {
    if is_failed(record.exit_code) {
        Some(
            record
                .failure_reason
                .or(recorded)
                .unwrap_or(FailureReason::Unknown),
        )
    } else {
        None
    }
}

fn status_changed_event(enabled: bool) -> TelemetryEvent {
    TelemetryEvent {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        event: "telemetry_status_changed",
        fallow_version: env!("CARGO_PKG_VERSION"),
        workflow: Workflow::Unknown,
        integration_surface: IntegrationSurface::CliHuman,
        invocation_context: classify_invocation_context(),
        agent_source: None,
        output_format: "human",
        quiet: false,
        ci: is_ci(),
        tty: std::io::stdout().is_terminal(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        duration_bucket_ms: "<100",
        outcome: if enabled { "enabled" } else { "disabled" },
        exit_code_bucket: "0",
        failure_reason: None,
        run_scope: None,
        config_shape: None,
        output_destination: None,
        analysis_mode: None,
        findings_present: None,
        result_count_bucket: None,
        report_truncated: None,
        truncation_reason: None,
        cache_state: None,
        mcp_tool: None,
        has_parent_run: false,
        run_role: RunRole::Root,
        followup_kind: FollowupKind::Unknown,
    }
}

fn example_event() -> TelemetryEvent {
    TelemetryEvent {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        event: "workflow_completed",
        fallow_version: env!("CARGO_PKG_VERSION"),
        workflow: Workflow::Audit,
        integration_surface: IntegrationSurface::Mcp,
        invocation_context: InvocationContext::Agent,
        agent_source: Some(AgentSource::Codex),
        output_format: "json",
        quiet: true,
        ci: false,
        tty: false,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        duration_bucket_ms: "500-2000",
        outcome: "issues_found",
        exit_code_bucket: "1",
        failure_reason: None,
        run_scope: Some(RunScope::ChangedOnly),
        config_shape: Some(ConfigShape::CustomRules),
        output_destination: Some(OutputDestination::Stdout),
        analysis_mode: Some(AnalysisMode::Static),
        findings_present: Some(true),
        result_count_bucket: Some(ResultCountBucket::OneToNine),
        report_truncated: Some(true),
        truncation_reason: Some(TruncationReason::CommentLimit),
        cache_state: Some(CacheState::Warm),
        mcp_tool: Some("find_dupes"),
        has_parent_run: true,
        run_role: RunRole::Followup,
        followup_kind: FollowupKind::Audit,
    }
}

fn field_purposes() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "workflow",
            "Prioritizes audit, dead-code, health, dupes, and integration workflows.",
        ),
        (
            "integration_surface",
            "Shows whether agents use CLI JSON, MCP, CI, editor, or programmatic surfaces.",
        ),
        (
            "invocation_context",
            "Separates human, CI, editor, and agent-driven use without storing detection evidence.",
        ),
        (
            "agent_source",
            "Identifies which agent integrations need compatibility work using a fixed allowlist.",
        ),
        (
            "duration_bucket_ms",
            "Finds slow workflow classes without recording exact timings.",
        ),
        (
            "exit_code_bucket",
            "Measures success, findings, and failure classes without raw errors.",
        ),
        (
            "failure_reason",
            "Groups failed workflows into a fixed privacy-safe allowlist; unknown stays unknown instead of parsing raw error text.",
        ),
        (
            "run_scope",
            "Classifies the run as full-project, changed-only, workspace-scoped, or file-scoped without storing names or refs.",
        ),
        (
            "config_shape",
            "Classifies config as default, custom config, custom rules, or plugins enabled without storing paths, rules, plugin names, or values.",
        ),
        (
            "output_destination",
            "Classifies the report sink as stdout, file, or CI comment without storing paths or URLs.",
        ),
        (
            "analysis_mode",
            "Classifies static, runtime-coverage, production-coverage, security, and fix workflows without storing raw command lines.",
        ),
        (
            "findings_present",
            "Whether the analysis surfaced any findings, decoupled from the exit-code gate. On combined and audit workflows it is an OR across sub-analyses; per-analysis find-rate is answerable only on standalone dead_code, dupes, and health.",
        ),
        (
            "result_count_bucket",
            "Coarse analysis result volume: 0, 1-9, 10-99, 100+, or unknown. Exact counts, paths, finding names, rule ids, and snippets are never sent.",
        ),
        (
            "report_truncated",
            "Whether a report/comment output path was truncated before delivery.",
        ),
        (
            "truncation_reason",
            "Why report/comment output was truncated: comment_limit, max_items, size_limit, or unknown.",
        ),
        (
            "cache_state",
            "Segments combined code-quality review durations into cold, warm, partial, or unknown cache states without uploading cache paths or raw counts.",
        ),
        (
            "mcp_tool",
            "Which MCP tool an agent called, from a fixed allowlist, so MCP usage is attributable per tool.",
        ),
        (
            "has_parent_run",
            "Shows whether a run has a sanitized parent-run correlation token without exposing the token.",
        ),
        (
            "run_role",
            "Separates root runs from follow-up runs using an allowlisted enum.",
        ),
        (
            "followup_kind",
            "Classifies follow-up runs by workflow using an allowlisted enum.",
        ),
    ]
}

fn effective_config() -> EffectiveConfig {
    if admin_disabled() {
        return EffectiveConfig {
            mode: EffectiveMode::DisabledByAdmin,
            source: ModeSource::AdminEnv,
            config_path: config_path(),
        };
    }
    if debug_enabled() {
        return EffectiveConfig {
            mode: EffectiveMode::Inspect,
            source: ModeSource::Env,
            config_path: config_path(),
        };
    }
    if let Ok(value) = std::env::var(MODE_ENV)
        && let Some(mode) = parse_env_mode(&value)
    {
        return EffectiveConfig {
            mode,
            source: ModeSource::Env,
            config_path: config_path(),
        };
    }
    if is_ci() {
        return EffectiveConfig {
            mode: EffectiveMode::Off,
            source: ModeSource::Default,
            config_path: config_path(),
        };
    }
    let path = config_path();
    if let Some(path_ref) = path.as_ref()
        && let Ok(config) = read_config_from(path_ref)
    {
        return EffectiveConfig {
            mode: if config.enabled {
                EffectiveMode::On
            } else {
                EffectiveMode::Off
            },
            source: ModeSource::UserConfig,
            config_path: path,
        };
    }
    EffectiveConfig {
        mode: EffectiveMode::Off,
        source: ModeSource::Default,
        config_path: path,
    }
}

fn parse_env_mode(value: &str) -> Option<EffectiveMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "off" | "0" | "false" | "disabled" => Some(EffectiveMode::Off),
        "on" | "1" | "true" | "enabled" => Some(EffectiveMode::On),
        "inspect" | "debug" | "log" => Some(EffectiveMode::Inspect),
        _ => None,
    }
}

fn admin_disabled() -> bool {
    env_truthy(DO_NOT_TRACK) || env_truthy(DISABLED_ENV)
}

fn debug_enabled() -> bool {
    env_truthy(DEBUG_ENV)
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn config_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(base.join("fallow").join("telemetry.json"))
}

fn read_config_from(path: &std::path::Path) -> Result<TelemetryConfig, String> {
    let raw = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&raw).map_err(|err| err.to_string())
}

fn write_config_to(path: &std::path::Path, config: &TelemetryConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut raw = serde_json::to_string_pretty(config).map_err(|err| err.to_string())?;
    raw.push('\n');
    std::fs::write(path, raw).map_err(|err| err.to_string())
}

/// Path to a spool-related file in the same directory as `telemetry.json`.
fn spool_file(name: &str) -> Option<PathBuf> {
    Some(config_path()?.with_file_name(name))
}

/// Append one serialized event line to the spool.
///
/// The line and its newline are written in a single `write_all`, so under
/// `O_APPEND` two processes that exit at the same instant cannot interleave a
/// half-line (each write lands atomically at the end). The append never takes
/// the drain lock, so the hot path (process exit) pays only one small write.
fn append_spool_line(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let mut record = String::with_capacity(line.len() + 1);
    record.push_str(line);
    record.push('\n');
    file.write_all(record.as_bytes())
}

/// Persist a telemetry event for delivery on a later run instead of uploading it
/// now.
///
/// Telemetry is recorded last, at process exit, once `elapsed` and `exit_code`
/// are known, so a synchronous upload here would block the command by the full
/// network round-trip. Spooling is sub-millisecond and network-free; the event
/// is drained and POSTed by [`flush_spool_in_background`] at the start of the
/// next telemetry-enabled run, where the upload overlaps the analysis work and
/// adds no perceptible latency. Delivery stays best-effort and lossy by design
/// (errors are discarded and the spool is bounded), but a fast run now defers
/// its event rather than dropping it.
fn spool_event(event: &TelemetryEvent, parent_run: Option<&str>) {
    let Some(path) = spool_file(SPOOL_FILE_NAME) else {
        return;
    };
    let value = if let Some(parent_run) = parent_run {
        serde_json::json!({
            "payload": event,
            "parent_run_header": parent_run,
        })
    } else {
        let Ok(value) = serde_json::to_value(event) else {
            return;
        };
        value
    };
    let Ok(line) = serde_json::to_string(&value) else {
        return;
    };
    if append_spool_line(&path, &line).is_ok() {
        trim_spool_if_oversized(&path);
    }
}

/// Keep the spool from growing without bound when no drain ever delivers.
///
/// The size check is a single `fstat`, so a normal append pays nothing. A trim
/// only runs once the file grows past [`SPOOL_MAX_BYTES`], which happens only on
/// a machine whose drains never complete (offline, or every run shorter than one
/// upload). It takes the same flock as the drain so the two never rewrite the
/// spool at once; on contention it skips and the next append retries. This is the
/// load-bearing bound: the drain's own cap only applies once it has delivered
/// something, so on a fast command whose drain is always abandoned mid-upload,
/// this is what keeps the file bounded.
fn trim_spool_if_oversized(path: &Path) {
    let oversized = std::fs::metadata(path).is_ok_and(|meta| meta.len() > SPOOL_MAX_BYTES);
    if !oversized {
        return;
    }
    let lock_path = path.with_file_name(SPOOL_LOCK_NAME);
    let Some(_lock) = SpoolLock::try_acquire(&lock_path) else {
        return;
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return;
    };
    let lines: Vec<&str> = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() <= SPOOL_MAX_EVENTS {
        return;
    }
    let keep_from = lines.len() - SPOOL_MAX_EVENTS;
    rewrite_spool(path, &lines[keep_from..]);
}

/// Atomically replace the spool with `lines`, or remove it when `lines` is empty.
///
/// The temp file is named per-process so a trim and a drain in different
/// processes cannot clobber each other's staging file; the final `rename` is
/// atomic, so a reader (or a process killed mid-write) sees either the old spool
/// or the new one, never a torn file.
fn rewrite_spool(path: &Path, lines: &[&str]) {
    if lines.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let Some(parent) = path.parent() else {
        return;
    };
    let mut body = String::with_capacity(lines.iter().map(|line| line.len() + 1).sum());
    for line in lines {
        body.push_str(line);
        body.push('\n');
    }
    let tmp = parent.join(format!("telemetry-spool.{}.tmp", std::process::id()));
    if std::fs::write(&tmp, body).is_ok() {
        if std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Drain any spooled telemetry events on a detached background thread.
///
/// Only runs when telemetry is `On`; the opt-out majority and the env kill
/// switches short-circuit in [`effective_config`] before any spool I/O, and no
/// thread is spawned when there is nothing to drain. The upload overlaps the
/// analysis work that follows in `main`, so spooled events are delivered with no
/// perceptible latency on any run longer than the POST, and a trivial run that
/// exits first simply leaves them spooled for the run after.
pub fn flush_spool_in_background() {
    if !matches!(effective_config().mode, EffectiveMode::On) {
        return;
    }
    let Some(spool) = spool_file(SPOOL_FILE_NAME) else {
        return;
    };
    if !spool.exists() {
        return;
    }
    std::thread::spawn(move || {
        drain_spool_file(&spool, post_telemetry_payload);
    });
}

/// POST spooled events oldest-first and drop the delivered ones in place.
///
/// Generic over the uploader so tests can inject a fake without touching the
/// network. The flock is held for the whole drain so a concurrent drain or trim
/// cannot rewrite the spool underneath it. Events are delivered oldest-first and
/// the drain stops at the first POST failure (a likely-unreachable endpoint), so
/// the removed set is always a prefix; the file is rewritten to the undelivered
/// tail (capped, in case it grew while the endpoint was down). Crucially, if the
/// thread is abandoned at process exit mid-upload, no rewrite runs and the spool
/// is simply retried next run, bounded meanwhile by [`trim_spool_if_oversized`]
/// rather than by this function completing.
fn drain_spool_file<P>(spool: &Path, mut post: P)
where
    P: FnMut(&serde_json::Value, Option<&str>) -> Result<(), String>,
{
    let lock_path = spool.with_file_name(SPOOL_LOCK_NAME);
    let Some(_lock) = SpoolLock::try_acquire(&lock_path) else {
        return;
    };
    let Ok(contents) = std::fs::read_to_string(spool) else {
        return;
    };
    let lines: Vec<&str> = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        let _ = std::fs::remove_file(spool);
        return;
    }

    let mut removed = 0usize;
    for line in &lines {
        match parse_spool_line(line) {
            // A corrupt (non-JSON) line cannot be delivered, so it is dropped.
            Err(_) => removed += 1,
            Ok((payload, parent_run)) => {
                if post(&payload, parent_run.as_deref()).is_ok() {
                    removed += 1;
                } else {
                    // The endpoint is likely unreachable; stop and keep the rest.
                    break;
                }
            }
        }
    }

    if removed == 0 {
        return;
    }
    let remaining = &lines[removed..];
    let keep_from = remaining.len().saturating_sub(SPOOL_MAX_EVENTS);
    rewrite_spool(spool, &remaining[keep_from..]);
}

fn parse_spool_line(line: &str) -> serde_json::Result<(serde_json::Value, Option<String>)> {
    let value = serde_json::from_str::<serde_json::Value>(line)?;
    let Some(payload) = value.get("payload") else {
        return Ok((value, None));
    };
    let parent_run = value
        .get("parent_run_header")
        .and_then(serde_json::Value::as_str)
        .and_then(sanitize_parent_run);
    Ok((payload.clone(), parent_run))
}

/// POST one already-serialized telemetry payload to the events endpoint.
fn post_telemetry_payload(
    payload: &serde_json::Value,
    parent_run: Option<&str>,
) -> Result<(), String> {
    let agent = try_api_agent_with_timeout(CONNECT_TIMEOUT_SECS, TOTAL_TIMEOUT_SECS)
        .map_err(|err| err.to_string())?;
    let url = api_url(TELEMETRY_PATH);
    let request = agent.post(&url);
    let request = if let Some(parent_run) = parent_run {
        request.header(PARENT_RUN_HEADER, parent_run)
    } else {
        request
    };
    let response = request.send_json(payload).map_err(|err| err.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("telemetry endpoint returned {}", response.status()))
    }
}

/// Advisory lock serialising spool rewrites across concurrent `fallow` processes.
///
/// A normal append never takes this lock (the hot path stays lock-free); only a
/// drain or an over-cap trim does, so at most one process rewrites the spool at a
/// time. The `.lock` sidecar is intentionally never deleted: an
/// unlinked-but-flocked inode plus a racer's `open(O_CREAT)` would split the lock
/// across two inodes. The kernel releases the lock when the file handle drops,
/// including at process exit, so an abandoned drain never wedges the next run.
struct SpoolLock {
    _file: std::fs::File,
}

impl SpoolLock {
    fn try_acquire(lock_path: &Path) -> Option<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)
            .ok()?;
        match file.try_lock() {
            Ok(()) => Some(Self { _file: file }),
            // Another process holds the lock; skip and let the next run retry.
            Err(std::fs::TryLockError::WouldBlock) => None,
            Err(std::fs::TryLockError::Error(err)) => {
                tracing::debug!(error = %err, "could not acquire telemetry spool lock");
                None
            }
        }
    }
}

fn print_event_to_stderr(event: &TelemetryEvent) {
    let stderr = std::io::stderr();
    let mut lock = stderr.lock();
    if let Ok(raw) = serde_json::to_string_pretty(event) {
        let _ = writeln!(lock, "{raw}");
    }
}

fn classify_invocation_context() -> InvocationContext {
    if classify_agent_source() != AgentSource::None {
        return InvocationContext::Agent;
    }
    if is_ci() {
        return InvocationContext::Ci;
    }
    if std::env::var_os("VSCODE_PID").is_some() || std::env::var_os("TERM_PROGRAM").is_some() {
        return InvocationContext::Editor;
    }
    if std::io::stdout().is_terminal() {
        InvocationContext::Human
    } else {
        InvocationContext::Unknown
    }
}

fn classify_agent_source() -> AgentSource {
    if let Ok(value) = std::env::var(AGENT_SOURCE_ENV) {
        return parse_agent_source_value(&value).unwrap_or(AgentSource::None);
    }
    classify_agent_source_from_env(std::env::vars_os().map(|(key, _)| key))
}

fn parse_agent_source_value(value: &str) -> Option<AgentSource> {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "" | "none" => Some(AgentSource::None),
        "codex" | "openai_codex" => Some(AgentSource::Codex),
        "claude" | "claude_code" => Some(AgentSource::ClaudeCode),
        "cursor" => Some(AgentSource::Cursor),
        "copilot" | "github_copilot" => Some(AgentSource::Copilot),
        "opencode" | "open_code" => Some(AgentSource::Opencode),
        "aider" => Some(AgentSource::Aider),
        "roo" | "roo_code" => Some(AgentSource::Roo),
        "windsurf" => Some(AgentSource::Windsurf),
        "gemini" | "gemini_cli" | "antigravity" => Some(AgentSource::Gemini),
        "cline" => Some(AgentSource::Cline),
        "continue" | "continue_dev" => Some(AgentSource::Continue),
        "zed" => Some(AgentSource::Zed),
        "goose" => Some(AgentSource::Goose),
        "other" | "other_known" => Some(AgentSource::OtherKnown),
        "unknown" => Some(AgentSource::Unknown),
        _ => None,
    }
}

fn classify_agent_source_from_env<I>(keys: I) -> AgentSource
where
    I: IntoIterator<Item = OsString>,
{
    const VENDORS: &[(&str, AgentSource)] = &[
        ("CODEX", AgentSource::Codex),
        ("CLAUDE", AgentSource::ClaudeCode),
        ("CURSOR", AgentSource::Cursor),
        ("COPILOT", AgentSource::Copilot),
        ("OPENCODE", AgentSource::Opencode),
        ("AIDER", AgentSource::Aider),
        ("ROO", AgentSource::Roo),
        ("WINDSURF", AgentSource::Windsurf),
        ("GEMINI", AgentSource::Gemini),
        ("ANTIGRAVITY", AgentSource::Gemini),
        ("CLINE", AgentSource::Cline),
        ("CONTINUE", AgentSource::Continue),
        ("ZED", AgentSource::Zed),
        ("GOOSE", AgentSource::Goose),
    ];
    let mut saw_agent = false;
    for key in keys {
        let key = key.to_string_lossy().to_ascii_uppercase();
        for (token, source) in VENDORS {
            if key_has_token(&key, token) {
                return *source;
            }
        }
        if key_has_token(&key, "AGENT") {
            saw_agent = true;
        }
    }
    if saw_agent {
        AgentSource::OtherKnown
    } else {
        AgentSource::None
    }
}

/// True when `token` appears in `key` at a leading word boundary: either at the
/// start of the key or immediately after an underscore. Avoids matching a token
/// embedded mid-word (`CHROOT` must not classify as `ROO`).
fn key_has_token(key: &str, token: &str) -> bool {
    key.match_indices(token)
        .any(|(idx, _)| idx == 0 || key.as_bytes()[idx - 1] == b'_')
}

fn is_ci() -> bool {
    std::env::var_os("CI").is_some()
        || std::env::var_os("GITHUB_ACTIONS").is_some()
        || std::env::var_os("GITLAB_CI").is_some()
}

fn integration_surface(output: OutputFormat) -> IntegrationSurface {
    // An explicit surface override (set by the MCP server, and reserved for the
    // other non-CLI surfaces) wins over env/format derivation. This is how an
    // MCP tool call, which shells out to the CLI and would otherwise look like
    // any other `cli_json` run, is correctly tagged `mcp`. Only an allowlisted
    // value is honored; anything else falls through to the existing derivation.
    if let Some(surface) = std::env::var(INTEGRATION_SURFACE_ENV)
        .ok()
        .and_then(|v| parse_integration_surface_override(&v))
    {
        return surface;
    }
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        IntegrationSurface::GithubAction
    } else if std::env::var_os("GITLAB_CI").is_some() {
        IntegrationSurface::GitlabCi
    } else if matches!(output, OutputFormat::Json) {
        IntegrationSurface::CliJson
    } else {
        IntegrationSurface::CliHuman
    }
}

/// Parse an allowlisted `FALLOW_INTEGRATION_SURFACE` override. Only the non-CLI
/// surfaces are accepted; the CLI surfaces stay auto-derived from env + format,
/// so an override cannot relabel a genuine CLI run as one of those.
fn parse_integration_surface_override(value: &str) -> Option<IntegrationSurface> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mcp" => Some(IntegrationSurface::Mcp),
        "lsp" => Some(IntegrationSurface::Lsp),
        "vscode" => Some(IntegrationSurface::Vscode),
        "napi" => Some(IntegrationSurface::Napi),
        "programmatic" => Some(IntegrationSurface::Programmatic),
        _ => None,
    }
}

/// The MCP tool that triggered this run, when set via `FALLOW_MCP_TOOL` and
/// present in the allowlist. Returns the `&'static str` from `MCP_TOOLS` (never
/// the caller's string) so an off-allowlist or adversarial value is dropped to
/// `None` rather than echoed into the payload.
fn mcp_tool() -> Option<&'static str> {
    mcp_tool_from_value(&std::env::var(MCP_TOOL_ENV).ok()?)
}

/// Resolve an `FALLOW_MCP_TOOL` value against the allowlist. Pure so it can be
/// unit-tested without touching process env. Returns the `&'static str` from
/// `MCP_TOOLS`, so an off-allowlist value drops to `None` and cannot be echoed
/// into the payload.
fn mcp_tool_from_value(value: &str) -> Option<&'static str> {
    let value = value.trim();
    MCP_TOOLS.iter().copied().find(|name| *name == value)
}

fn output_format_label(output: OutputFormat) -> &'static str {
    match output {
        OutputFormat::Human => "human",
        OutputFormat::Json => "json",
        OutputFormat::Sarif => "sarif",
        OutputFormat::Compact => "compact",
        OutputFormat::Markdown => "markdown",
        OutputFormat::CodeClimate => "codeclimate",
        OutputFormat::PrCommentGithub => "pr_comment_github",
        OutputFormat::PrCommentGitlab => "pr_comment_gitlab",
        OutputFormat::ReviewGithub => "review_github",
        OutputFormat::ReviewGitlab => "review_gitlab",
        OutputFormat::Badge => "badge",
    }
}

fn duration_bucket(duration: Duration) -> &'static str {
    let ms = duration.as_millis();
    match ms {
        0..=99 => "<100",
        100..=499 => "100-500",
        500..=1_999 => "500-2000",
        2_000..=9_999 => "2s-10s",
        _ => "10s+",
    }
}

fn exit_code_bucket(code: ExitCode) -> &'static str {
    if code == ExitCode::SUCCESS {
        "0"
    } else if code == ExitCode::from(1) {
        "1"
    } else if code == ExitCode::from(2) {
        "2"
    } else {
        "3-7"
    }
}

fn outcome(code: ExitCode) -> &'static str {
    if code == ExitCode::SUCCESS {
        "success"
    } else if code == ExitCode::from(1) {
        "issues_found"
    } else {
        "failed"
    }
}

fn parent_run_context(parent_run: Option<&str>, workflow: Workflow) -> ParentRunContext {
    match parent_run {
        Some(value) => {
            if let Some(token) = sanitize_parent_run(value) {
                ParentRunContext {
                    token: Some(token),
                    has_parent_run: true,
                    run_role: RunRole::Followup,
                    followup_kind: followup_kind(workflow),
                }
            } else {
                ParentRunContext {
                    token: None,
                    has_parent_run: false,
                    run_role: RunRole::Unknown,
                    followup_kind: FollowupKind::Unknown,
                }
            }
        }
        None => ParentRunContext {
            token: None,
            has_parent_run: false,
            run_role: RunRole::Root,
            followup_kind: FollowupKind::Unknown,
        },
    }
}

fn followup_kind(workflow: Workflow) -> FollowupKind {
    match workflow {
        Workflow::Audit => FollowupKind::Audit,
        Workflow::Security => FollowupKind::Security,
        Workflow::Health => FollowupKind::Health,
        Workflow::DeadCode => FollowupKind::Check,
        Workflow::Dupes => FollowupKind::Dupes,
        Workflow::Fix => FollowupKind::Fix,
        Workflow::Explain => FollowupKind::Explain,
        Workflow::DependencyCleanup
        | Workflow::CodeQualityReview
        | Workflow::GithubAction
        | Workflow::GitlabCi
        | Workflow::EditorDiagnostic
        | Workflow::ProgrammaticAnalysis
        | Workflow::RuntimeCoverageSetup
        | Workflow::Impact
        | Workflow::ProjectInventory
        | Workflow::Setup
        | Workflow::License
        | Workflow::Unknown => FollowupKind::Unknown,
    }
}

fn sanitize_parent_run(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if !(6..=64).contains(&trimmed.len()) {
        return None;
    }
    if trimmed
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

fn is_failed(code: ExitCode) -> bool {
    code != ExitCode::SUCCESS && code != ExitCode::from(1)
}

fn mode_label(mode: EffectiveMode) -> &'static str {
    match mode {
        EffectiveMode::Off => "off",
        EffectiveMode::On => "on",
        EffectiveMode::Inspect => "inspect",
        EffectiveMode::DisabledByAdmin => "disabled_by_admin",
    }
}

fn source_label(source: ModeSource) -> &'static str {
    match source {
        ModeSource::AdminEnv => "admin_env",
        ModeSource::Env => "env",
        ModeSource::UserConfig => "user_config",
        ModeSource::Default => "default",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_mode_parsing_accepts_expected_values() {
        assert_eq!(parse_env_mode("off"), Some(EffectiveMode::Off));
        assert_eq!(parse_env_mode("on"), Some(EffectiveMode::On));
        assert_eq!(parse_env_mode("inspect"), Some(EffectiveMode::Inspect));
        assert_eq!(parse_env_mode("garbage"), None);
    }

    #[test]
    fn agent_source_is_allowlisted_not_raw() {
        let source = classify_agent_source_from_env([
            OsString::from("CURSOR_TRACE_ID"),
            OsString::from("PRIVATE_AGENT_PATH"),
        ]);
        assert_eq!(source, AgentSource::Cursor);

        let event = example_event();
        let raw = serde_json::to_string(&event).expect("event serializes");
        assert!(raw.contains("\"agent_source\":\"codex\""));
        assert!(!raw.contains("CURSOR_TRACE_ID"));
        assert!(!raw.contains("PRIVATE_AGENT_PATH"));
    }

    #[test]
    fn generic_agent_source_does_not_emit_env_name() {
        let source = classify_agent_source_from_env([OsString::from("MY_PRIVATE_AGENT_WRAPPER")]);
        assert_eq!(source, AgentSource::OtherKnown);
    }

    #[test]
    fn explicit_agent_source_accepts_only_allowlist() {
        assert_eq!(parse_agent_source_value("codex"), Some(AgentSource::Codex));
        assert_eq!(
            parse_agent_source_value("claude-code"),
            Some(AgentSource::ClaudeCode)
        );
        assert_eq!(parse_agent_source_value("private-agent-x"), None);
    }

    #[test]
    fn explicit_agent_source_accepts_new_vendors() {
        assert_eq!(
            parse_agent_source_value("windsurf"),
            Some(AgentSource::Windsurf)
        );
        assert_eq!(
            parse_agent_source_value("gemini_cli"),
            Some(AgentSource::Gemini)
        );
        assert_eq!(
            parse_agent_source_value("antigravity"),
            Some(AgentSource::Gemini)
        );
        assert_eq!(parse_agent_source_value("cline"), Some(AgentSource::Cline));
        assert_eq!(
            parse_agent_source_value("continue"),
            Some(AgentSource::Continue)
        );
        assert_eq!(parse_agent_source_value("zed"), Some(AgentSource::Zed));
        assert_eq!(parse_agent_source_value("goose"), Some(AgentSource::Goose));
    }

    #[test]
    fn heuristic_detects_new_vendors_at_word_boundary() {
        assert_eq!(
            classify_agent_source_from_env([OsString::from("WINDSURF_SESSION")]),
            AgentSource::Windsurf
        );
        assert_eq!(
            classify_agent_source_from_env([OsString::from("MY_GEMINI_KEY")]),
            AgentSource::Gemini
        );
    }

    #[test]
    fn heuristic_does_not_match_token_mid_word() {
        assert_eq!(
            classify_agent_source_from_env([
                OsString::from("CHROOT"),
                OsString::from("AUTHORIZED_KEYS"),
            ]),
            AgentSource::None
        );
    }

    #[test]
    fn workflow_event_buckets_exit_codes() {
        let record = WorkflowRecord {
            workflow: Workflow::Audit,
            output: OutputFormat::Json,
            quiet: true,
            elapsed: Duration::from_millis(750),
            exit_code: ExitCode::from(1),
            failure_reason: None,
            parent_run: Some("tmp_123"),
            context: WorkflowContext {
                run_scope: RunScope::ChangedOnly,
                config_shape: ConfigShape::CustomRules,
                output_destination: OutputDestination::Stdout,
                analysis_mode: AnalysisMode::Static,
            },
        };
        let parent_run = parent_run_context(record.parent_run, record.workflow);
        let event = build_workflow_event(&record, &parent_run);
        assert_eq!(event.event, "workflow_completed");
        assert_eq!(event.duration_bucket_ms, "500-2000");
        assert_eq!(event.outcome, "issues_found");
        assert_eq!(event.exit_code_bucket, "1");
        assert_eq!(event.failure_reason, None);
        assert_eq!(event.run_scope, Some(RunScope::ChangedOnly));
        assert_eq!(event.config_shape, Some(ConfigShape::CustomRules));
        assert_eq!(event.output_destination, Some(OutputDestination::Stdout));
        assert_eq!(event.analysis_mode, Some(AnalysisMode::Static));
        assert_eq!(parent_run.token.as_deref(), Some("tmp_123"));
        assert!(event.has_parent_run);
        assert_eq!(event.run_role, RunRole::Followup);
        assert_eq!(event.followup_kind, FollowupKind::Audit);
    }

    #[test]
    fn failed_workflow_defaults_to_unknown_failure_reason() {
        let record = WorkflowRecord {
            workflow: Workflow::Audit,
            output: OutputFormat::Json,
            quiet: true,
            elapsed: Duration::from_millis(750),
            exit_code: ExitCode::from(2),
            failure_reason: None,
            parent_run: None,
            context: WorkflowContext {
                run_scope: RunScope::ChangedOnly,
                config_shape: ConfigShape::Default,
                output_destination: OutputDestination::Stdout,
                analysis_mode: AnalysisMode::Static,
            },
        };
        assert_eq!(
            failure_reason_for_value(&record, None),
            Some(FailureReason::Unknown)
        );
    }

    #[test]
    fn explicit_failure_reason_wins_for_failed_workflow() {
        let record = WorkflowRecord {
            workflow: Workflow::Audit,
            output: OutputFormat::Json,
            quiet: true,
            elapsed: Duration::from_millis(750),
            exit_code: ExitCode::from(2),
            failure_reason: Some(FailureReason::Diff),
            parent_run: None,
            context: WorkflowContext {
                run_scope: RunScope::ChangedOnly,
                config_shape: ConfigShape::Default,
                output_destination: OutputDestination::Stdout,
                analysis_mode: AnalysisMode::Static,
            },
        };
        assert_eq!(
            failure_reason_for_value(&record, Some(FailureReason::Validation)),
            Some(FailureReason::Diff)
        );
    }

    #[test]
    fn failure_reason_state_accepts_only_allowlist() {
        assert_eq!(
            failure_reason_from_state(FAILURE_REASON_VALIDATION),
            Some(FailureReason::Validation)
        );
        assert_eq!(
            failure_reason_from_state(FAILURE_REASON_UNSUPPORTED_FORMAT),
            Some(FailureReason::UnsupportedFormat)
        );
        assert_eq!(
            failure_reason_from_state(FAILURE_REASON_CONFIG),
            Some(FailureReason::Config)
        );
        assert_eq!(
            failure_reason_from_state(FAILURE_REASON_ANALYSIS),
            Some(FailureReason::Analysis)
        );
        assert_eq!(
            failure_reason_from_state(FAILURE_REASON_DIFF),
            Some(FailureReason::Diff)
        );
        assert_eq!(
            failure_reason_from_state(FAILURE_REASON_NETWORK),
            Some(FailureReason::Network)
        );
        assert_eq!(
            failure_reason_from_state(FAILURE_REASON_AUTH),
            Some(FailureReason::Auth)
        );
        assert_eq!(
            failure_reason_from_state(FAILURE_REASON_GATE),
            Some(FailureReason::Gate)
        );
        assert_eq!(
            failure_reason_from_state(FAILURE_REASON_SIGNAL),
            Some(FailureReason::Signal)
        );
        assert_eq!(
            failure_reason_from_state(FAILURE_REASON_UNKNOWN),
            Some(FailureReason::Unknown)
        );
        assert_eq!(failure_reason_from_state(99), None);
    }

    #[test]
    fn parent_run_rejects_free_form_values() {
        assert_eq!(
            sanitize_parent_run("run_abc-123").as_deref(),
            Some("run_abc-123")
        );
        assert_eq!(sanitize_parent_run("../repo/main"), None);
        assert_eq!(sanitize_parent_run("customer project"), None);
        assert_eq!(sanitize_parent_run("x"), None);
    }

    #[test]
    fn parent_run_context_never_serializes_raw_token() {
        let record = WorkflowRecord {
            workflow: Workflow::Explain,
            output: OutputFormat::Json,
            quiet: true,
            elapsed: Duration::from_millis(10),
            exit_code: ExitCode::SUCCESS,
            failure_reason: None,
            parent_run: Some("tmp_abc-123"),
            context: WorkflowContext {
                run_scope: RunScope::FullProject,
                config_shape: ConfigShape::Default,
                output_destination: OutputDestination::Stdout,
                analysis_mode: AnalysisMode::Static,
            },
        };
        let parent_run = parent_run_context(record.parent_run, record.workflow);
        let event = build_workflow_event(&record, &parent_run);
        let value = serde_json::to_value(&event).expect("event serializes");

        assert_eq!(parent_run.token.as_deref(), Some("tmp_abc-123"));
        assert_eq!(value.get("parent_run"), None);
        assert_eq!(value["has_parent_run"].as_bool(), Some(true));
        assert_eq!(value["run_role"].as_str(), Some("followup"));
        assert_eq!(value["followup_kind"].as_str(), Some("explain"));
    }

    #[test]
    fn invalid_parent_run_marks_unknown_without_token() {
        let parent_run = parent_run_context(Some("../repo/main"), Workflow::Fix);

        assert_eq!(parent_run.token, None);
        assert!(!parent_run.has_parent_run);
        assert_eq!(parent_run.run_role, RunRole::Unknown);
        assert_eq!(parent_run.followup_kind, FollowupKind::Unknown);
    }

    #[test]
    fn config_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("telemetry.json");
        let config = TelemetryConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            enabled: true,
            prompt_shown: true,
        };
        write_config_to(&path, &config).expect("write config");
        let loaded = read_config_from(&path).expect("read config");
        assert!(loaded.enabled);
        assert!(loaded.prompt_shown);
        assert_eq!(loaded.schema_version, CONFIG_SCHEMA_VERSION);
    }

    fn read_telemetry_doc() -> Option<String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/telemetry.md");
        std::fs::read_to_string(path).ok()
    }

    fn first_fenced_block(haystack: &str, fence: &str) -> Option<String> {
        let start = haystack.find(fence)? + fence.len();
        let rest = &haystack[start..];
        let end = rest.find("```")?;
        Some(rest[..end].to_owned())
    }

    #[test]
    fn docs_agent_source_allowlist_matches_code() {
        use std::collections::BTreeSet;

        let all: &[AgentSource] = &[
            AgentSource::None,
            AgentSource::Codex,
            AgentSource::ClaudeCode,
            AgentSource::Cursor,
            AgentSource::Copilot,
            AgentSource::Opencode,
            AgentSource::Aider,
            AgentSource::Roo,
            AgentSource::Windsurf,
            AgentSource::Gemini,
            AgentSource::Cline,
            AgentSource::Continue,
            AgentSource::Zed,
            AgentSource::Goose,
            AgentSource::OtherKnown,
            AgentSource::Unknown,
        ];
        for &source in all {
            match source {
                AgentSource::None
                | AgentSource::Codex
                | AgentSource::ClaudeCode
                | AgentSource::Cursor
                | AgentSource::Copilot
                | AgentSource::Opencode
                | AgentSource::Aider
                | AgentSource::Roo
                | AgentSource::Windsurf
                | AgentSource::Gemini
                | AgentSource::Cline
                | AgentSource::Continue
                | AgentSource::Zed
                | AgentSource::Goose
                | AgentSource::OtherKnown
                | AgentSource::Unknown => {}
            }
        }
        let canonical: BTreeSet<String> = all
            .iter()
            .map(|source| {
                serde_json::to_value(source)
                    .expect("AgentSource serializes")
                    .as_str()
                    .expect("AgentSource serializes to a string")
                    .to_owned()
            })
            .collect();

        let Some(doc) = read_telemetry_doc() else {
            return;
        };
        let section = doc
            .split("## Agent Source")
            .nth(1)
            .expect("docs/telemetry.md has an `## Agent Source` section");
        let block = first_fenced_block(section, "```text")
            .expect("`## Agent Source` has a ```text allowlist block");
        let documented: BTreeSet<String> = block.split_whitespace().map(str::to_owned).collect();

        assert_eq!(
            documented, canonical,
            "docs/telemetry.md `## Agent Source` allowlist is out of sync with the AgentSource enum"
        );
    }

    #[test]
    fn docs_example_payload_fields_match_emitted_event() {
        use std::collections::BTreeSet;

        let Some(doc) = read_telemetry_doc() else {
            return;
        };
        let json_block =
            first_fenced_block(&doc, "```json").expect("docs/telemetry.md has a ```json example");
        let doc_value: serde_json::Value =
            serde_json::from_str(&json_block).expect("doc example is valid JSON");
        let real_value = serde_json::to_value(example_event()).expect("example event serializes");

        let doc_keys: BTreeSet<&str> = doc_value
            .as_object()
            .expect("doc example is an object")
            .keys()
            .map(String::as_str)
            .collect();
        let real_keys: BTreeSet<&str> = real_value
            .as_object()
            .expect("event is an object")
            .keys()
            .map(String::as_str)
            .collect();

        assert_eq!(
            doc_keys, real_keys,
            "docs/telemetry.md example payload fields are out of sync with the emitted \
             TelemetryEvent (compare against `fallow telemetry inspect --example`)"
        );
    }

    #[test]
    fn findings_present_state_maps_to_tristate() {
        assert_eq!(findings_present_from_state(FINDINGS_UNSET), None);
        assert_eq!(findings_present_from_state(FINDINGS_CLEAN), Some(false));
        assert_eq!(findings_present_from_state(FINDINGS_FOUND), Some(true));
        // Any unexpected value is treated as unset, never a misleading false.
        assert_eq!(findings_present_from_state(99), None);
    }

    #[test]
    fn result_count_state_maps_to_buckets() {
        assert_eq!(result_count_bucket_from_state(RESULT_COUNT_UNSET), None);
        assert_eq!(
            result_count_bucket_from_state(RESULT_COUNT_UNKNOWN),
            Some(ResultCountBucket::Unknown)
        );
        assert_eq!(
            result_count_bucket_from_state(0),
            Some(ResultCountBucket::Zero)
        );
        assert_eq!(
            result_count_bucket_from_state(9),
            Some(ResultCountBucket::OneToNine)
        );
        assert_eq!(
            result_count_bucket_from_state(10),
            Some(ResultCountBucket::TenToNinetyNine)
        );
        assert_eq!(
            result_count_bucket_from_state(99),
            Some(ResultCountBucket::TenToNinetyNine)
        );
        assert_eq!(
            result_count_bucket_from_state(100),
            Some(ResultCountBucket::OneHundredPlus)
        );
    }

    #[test]
    fn cache_state_maps_to_allowlisted_enum() {
        assert_eq!(cache_state_from_state(CACHE_STATE_UNSET), None);
        assert_eq!(
            cache_state_from_state(CACHE_STATE_COLD),
            Some(CacheState::Cold)
        );
        assert_eq!(
            cache_state_from_state(CACHE_STATE_WARM),
            Some(CacheState::Warm)
        );
        assert_eq!(
            cache_state_from_state(CACHE_STATE_PARTIAL),
            Some(CacheState::Partial)
        );
        assert_eq!(
            cache_state_from_state(CACHE_STATE_UNKNOWN),
            Some(CacheState::Unknown)
        );
        assert_eq!(cache_state_from_state(99), None);
    }

    #[test]
    fn report_truncation_state_maps_to_payload_fields() {
        assert_eq!(report_truncated_from_state(REPORT_TRUNCATION_UNSET), None);
        assert_eq!(
            report_truncated_from_state(REPORT_TRUNCATION_FALSE),
            Some(false)
        );
        assert_eq!(
            report_truncated_from_state(REPORT_TRUNCATION_TRUE),
            Some(true)
        );
        assert_eq!(
            truncation_reason_from_state(TRUNCATION_REASON_COMMENT_LIMIT),
            Some(TruncationReason::CommentLimit)
        );
        assert_eq!(
            truncation_reason_from_state(TRUNCATION_REASON_SIZE_LIMIT),
            Some(TruncationReason::SizeLimit)
        );
    }

    #[test]
    fn mcp_tool_value_is_allowlist_validated() {
        // Known tool names round-trip to the static allowlist entry.
        assert_eq!(mcp_tool_from_value("find_dupes"), Some("find_dupes"));
        assert_eq!(mcp_tool_from_value("  audit  "), Some("audit"));
        // Anything off-allowlist is dropped, never echoed into the payload.
        assert_eq!(mcp_tool_from_value("/etc/passwd"), None);
        assert_eq!(mcp_tool_from_value(""), None);
        assert_eq!(mcp_tool_from_value("dupes"), None);
    }

    #[test]
    fn integration_surface_override_accepts_only_non_cli_surfaces() {
        assert_eq!(
            parse_integration_surface_override("mcp"),
            Some(IntegrationSurface::Mcp)
        );
        assert_eq!(
            parse_integration_surface_override("LSP"),
            Some(IntegrationSurface::Lsp)
        );
        assert_eq!(
            parse_integration_surface_override(" programmatic "),
            Some(IntegrationSurface::Programmatic)
        );
        // CLI surfaces stay auto-derived; an override cannot relabel them.
        assert_eq!(parse_integration_surface_override("cli_json"), None);
        assert_eq!(parse_integration_surface_override("github_action"), None);
        assert_eq!(parse_integration_surface_override(""), None);
    }

    #[test]
    fn append_spool_line_accumulates_newline_terminated_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(SPOOL_FILE_NAME);
        append_spool_line(&path, "{\"a\":1}").expect("append");
        append_spool_line(&path, "{\"b\":2}").expect("append");
        let contents = std::fs::read_to_string(&path).expect("read");
        assert_eq!(contents, "{\"a\":1}\n{\"b\":2}\n");
    }

    #[test]
    fn drain_delivers_all_events_and_removes_spool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        append_spool_line(&spool, "{\"n\":1}").expect("append");
        append_spool_line(&spool, "{\"n\":2}").expect("append");

        let mut seen = Vec::new();
        drain_spool_file(&spool, |value, _parent_run| {
            seen.push(
                value
                    .get("n")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
            );
            Ok(())
        });

        assert_eq!(seen, vec![1, 2]);
        assert!(!spool.exists(), "fully delivered spool should be removed");
    }

    #[test]
    fn drain_keeps_undelivered_and_stops_after_first_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        for n in 0..3 {
            append_spool_line(&spool, &format!("{{\"n\":{n}}}")).expect("append");
        }

        let mut calls = 0;
        drain_spool_file(&spool, |_value, _parent_run| {
            calls += 1;
            Err("offline".to_owned())
        });

        assert_eq!(
            calls, 1,
            "network-down short-circuit should stop after the first failure",
        );
        let contents = std::fs::read_to_string(&spool).expect("spool retained");
        assert_eq!(
            contents.lines().count(),
            3,
            "nothing delivered, so the spool is left untouched for the next run",
        );
    }

    #[test]
    fn drain_drops_corrupt_lines_and_delivers_valid_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        append_spool_line(&spool, "not json").expect("append");
        append_spool_line(&spool, "{\"n\":7}").expect("append");

        let mut seen = Vec::new();
        drain_spool_file(&spool, |value, _parent_run| {
            seen.push(value.clone());
            Ok(())
        });

        assert_eq!(seen.len(), 1, "corrupt line dropped, valid line delivered");
        assert_eq!(
            seen[0].get("n").and_then(serde_json::Value::as_i64),
            Some(7)
        );
        assert!(!spool.exists());
    }

    #[test]
    fn drain_caps_undelivered_tail_after_partial_delivery() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        let total = SPOOL_MAX_EVENTS + 6;
        for n in 0..total {
            append_spool_line(&spool, &format!("{{\"n\":{n}}}")).expect("append");
        }

        // Deliver the first event, then the endpoint goes down for the rest.
        let mut calls = 0;
        drain_spool_file(&spool, |_value, _parent_run| {
            calls += 1;
            if calls == 1 {
                Ok(())
            } else {
                Err("offline".to_owned())
            }
        });
        assert_eq!(calls, 2, "deliver one, fail on the second, then stop");

        let contents = std::fs::read_to_string(&spool).expect("spool retained");
        let kept: Vec<i64> = contents
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|value| value.get("n").and_then(serde_json::Value::as_i64))
            .collect();

        // First event delivered (dropped), the 69-event tail capped to the newest 64.
        assert_eq!(
            kept.len(),
            SPOOL_MAX_EVENTS,
            "undelivered tail bounded to the cap"
        );
        assert_eq!(kept.first().copied(), Some(6), "oldest of the tail dropped");
        assert_eq!(
            kept.last().copied(),
            Some((total - 1) as i64),
            "newest kept"
        );
    }

    #[test]
    fn trim_caps_oversized_spool_to_newest_events() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        // Pad each event so a modest line count blows past `SPOOL_MAX_BYTES`.
        let pad = "x".repeat(650);
        let total = 100;
        for n in 0..total {
            append_spool_line(&spool, &format!("{{\"n\":{n},\"pad\":\"{pad}\"}}")).expect("append");
        }
        assert!(
            std::fs::metadata(&spool).expect("metadata").len() > SPOOL_MAX_BYTES,
            "fixture must exceed the byte ceiling so the trim fires",
        );

        trim_spool_if_oversized(&spool);

        let contents = std::fs::read_to_string(&spool).expect("spool retained");
        let kept: Vec<i64> = contents
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|value| value.get("n").and_then(serde_json::Value::as_i64))
            .collect();

        assert_eq!(
            kept.len(),
            SPOOL_MAX_EVENTS,
            "write-path trim bounds the spool"
        );
        assert_eq!(
            kept.first().copied(),
            Some((total - SPOOL_MAX_EVENTS) as i64)
        );
        assert_eq!(
            kept.last().copied(),
            Some((total - 1) as i64),
            "newest kept"
        );
    }

    #[test]
    fn trim_leaves_small_spool_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        for n in 0..3 {
            append_spool_line(&spool, &format!("{{\"n\":{n}}}")).expect("append");
        }
        let before = std::fs::read_to_string(&spool).expect("read");

        trim_spool_if_oversized(&spool);

        let after = std::fs::read_to_string(&spool).expect("read");
        assert_eq!(
            before, after,
            "a spool under the byte ceiling is never rewritten"
        );
    }

    #[test]
    fn spool_lock_excludes_concurrent_acquire() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lock_path = dir.path().join(SPOOL_LOCK_NAME);
        let first = SpoolLock::try_acquire(&lock_path).expect("first acquire");
        assert!(
            SpoolLock::try_acquire(&lock_path).is_none(),
            "second acquire should contend while the first is held",
        );
        drop(first);
        assert!(
            SpoolLock::try_acquire(&lock_path).is_some(),
            "lock should be free after the holder drops",
        );
    }

    #[test]
    fn spooled_event_round_trips_through_drain() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        let record = WorkflowRecord {
            workflow: Workflow::DeadCode,
            output: OutputFormat::Json,
            quiet: true,
            elapsed: Duration::from_millis(10),
            exit_code: ExitCode::from(0),
            failure_reason: None,
            parent_run: None,
            context: WorkflowContext {
                run_scope: RunScope::FullProject,
                config_shape: ConfigShape::Default,
                output_destination: OutputDestination::Stdout,
                analysis_mode: AnalysisMode::Static,
            },
        };
        let parent_run = parent_run_context(record.parent_run, record.workflow);
        let line =
            serde_json::to_string(&build_workflow_event(&record, &parent_run)).expect("serialize");
        append_spool_line(&spool, &line).expect("append");

        let mut seen = Vec::new();
        drain_spool_file(&spool, |value, parent_run| {
            assert_eq!(parent_run, None);
            seen.push(value.clone());
            Ok(())
        });

        assert_eq!(seen.len(), 1);
        assert_eq!(
            seen[0].get("event").and_then(serde_json::Value::as_str),
            Some("workflow_completed"),
        );
        assert!(!spool.exists());
    }

    #[test]
    fn spooled_parent_run_uses_private_header_without_payload_field() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        let record = WorkflowRecord {
            workflow: Workflow::Explain,
            output: OutputFormat::Json,
            quiet: true,
            elapsed: Duration::from_millis(10),
            exit_code: ExitCode::SUCCESS,
            failure_reason: None,
            parent_run: Some("tmp_abc-123"),
            context: WorkflowContext {
                run_scope: RunScope::FullProject,
                config_shape: ConfigShape::Default,
                output_destination: OutputDestination::Stdout,
                analysis_mode: AnalysisMode::Static,
            },
        };
        let parent_run = parent_run_context(record.parent_run, record.workflow);
        let event = build_workflow_event(&record, &parent_run);
        let line = serde_json::to_string(&serde_json::json!({
            "payload": event,
            "parent_run_header": parent_run.token,
        }))
        .expect("serialize");
        append_spool_line(&spool, &line).expect("append");

        let mut seen = Vec::new();
        drain_spool_file(&spool, |value, parent_run| {
            seen.push((value.clone(), parent_run.map(str::to_owned)));
            Ok(())
        });

        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0.get("parent_run"), None);
        assert_eq!(seen[0].0["has_parent_run"].as_bool(), Some(true));
        assert_eq!(seen[0].0["run_role"].as_str(), Some("followup"));
        assert_eq!(seen[0].0["followup_kind"].as_str(), Some("explain"));
        assert_eq!(seen[0].1.as_deref(), Some("tmp_abc-123"));
        assert!(!spool.exists());
    }
}
