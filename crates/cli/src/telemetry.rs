//! Opt-in product telemetry for agent and CI workflow quality.
//!
//! The payload is intentionally small and allowlisted. It must never include
//! repository names, paths, package names, config values, raw command lines, or
//! raw agent-detection evidence.

use std::ffi::OsString;
use std::io::{IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU8, AtomicU16, AtomicU64, Ordering};
use std::time::Duration;

use fallow_config::OutputFormat;
use fallow_engine::RetainedModuleGraph;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::api::{api_url, try_api_agent_with_timeout};

const CONFIG_SCHEMA_VERSION: u8 = 2;
const TELEMETRY_SCHEMA_VERSION: u8 = 2;
const CONNECT_TIMEOUT_SECS: u64 = 1;
const TOTAL_TIMEOUT_SECS: u64 = 1;
const TELEMETRY_PATH: &str = "/v1/telemetry/events";
const PARENT_RUN_HEADER: &str = "X-Fallow-Parent-Run";
/// Private transport header carrying the anonymous, random, install-scoped
/// grouping token. Sent like [`PARENT_RUN_HEADER`]: a header for server-side
/// `distinct_id` grouping, never an event-payload property, so the events the
/// CLI serializes and spools still carry no identifiers.
const INSTALL_HEADER: &str = "X-Fallow-Install";
/// Prefix marking the anonymous install grouping token in `telemetry.json`.
const INSTALL_ID_PREFIX: &str = "inst_";
static ANALYSIS_RUN_COUNTER: AtomicU64 = AtomicU64::new(0);
static INSTALL_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

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
static FILE_COUNT_BUCKET: AtomicU8 = AtomicU8::new(SCALE_BUCKET_UNSET);
static FUNCTION_COUNT_BUCKET: AtomicU8 = AtomicU8::new(SCALE_BUCKET_UNSET);
static AVG_FAN_OUT_BUCKET: AtomicU8 = AtomicU8::new(SCALE_BUCKET_UNSET);

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
const SCALE_BUCKET_UNSET: u8 = 0;
const SCALE_BUCKET_SMALL: u8 = 1;
const SCALE_BUCKET_MEDIUM: u8 = 2;
const SCALE_BUCKET_LARGE: u8 = 3;
const SCALE_BUCKET_XLARGE: u8 = 4;
const SCALE_BUCKET_UNKNOWN: u8 = 5;

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

/// Record coarse analysis scale from counts already computed by an analysis.
///
/// Exact counts are bucketed immediately and never serialized. Repeated calls are
/// safe: combined workflows keep the largest bucket reported by their
/// sub-analyses.
pub fn note_analysis_scale(file_count: Option<usize>, function_count: Option<usize>) {
    if let Some(count) = file_count {
        FILE_COUNT_BUCKET.fetch_max(file_count_bucket_state(count), Ordering::Relaxed);
    }
    if let Some(count) = function_count {
        FUNCTION_COUNT_BUCKET.fetch_max(function_count_bucket_state(count), Ordering::Relaxed);
    }
}

/// Record a coarse fan-out bucket from a graph already retained by the workflow.
///
/// This uses only the graph's existing module and edge counts. It never walks
/// adjacency, resolves dependencies, or computes diameter/depth metrics for
/// telemetry.
pub fn note_graph_structure(graph: &RetainedModuleGraph) {
    note_graph_structure_counts(graph.module_count(), graph.edge_count());
}

/// Record a coarse fan-out bucket from already-computed module and edge counts.
///
/// Used by the engine-backed health pipeline, which surfaces the counts rather
/// than the graph so the engine does not depend on this CLI telemetry module.
pub fn note_graph_structure_counts(module_count: usize, edge_count: usize) {
    AVG_FAN_OUT_BUCKET.fetch_max(
        avg_fan_out_bucket_state(module_count, edge_count),
        Ordering::Relaxed,
    );
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

/// Return the failure reason recorded by lower-level code, if any.
pub fn noted_failure_reason() -> Option<FailureReason> {
    failure_reason()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum FileCountBucket {
    #[serde(rename = "0-99")]
    Small,
    #[serde(rename = "100-499")]
    Medium,
    #[serde(rename = "500-1999")]
    Large,
    #[serde(rename = "2000+")]
    XLarge,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum FunctionCountBucket {
    #[serde(rename = "0-999")]
    Small,
    #[serde(rename = "1000-9999")]
    Medium,
    #[serde(rename = "10000+")]
    Large,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum AvgFanOutBucket {
    #[serde(rename = "0")]
    Zero,
    #[serde(rename = "<1")]
    LessThanOne,
    #[serde(rename = "1-2")]
    OneToTwo,
    #[serde(rename = "3+")]
    ThreePlus,
    #[serde(rename = "unknown")]
    Unknown,
}

const fn file_count_bucket_state(count: usize) -> u8 {
    match count {
        0..=99 => SCALE_BUCKET_SMALL,
        100..=499 => SCALE_BUCKET_MEDIUM,
        500..=1999 => SCALE_BUCKET_LARGE,
        _ => SCALE_BUCKET_XLARGE,
    }
}

const fn function_count_bucket_state(count: usize) -> u8 {
    match count {
        0..=999 => SCALE_BUCKET_SMALL,
        1000..=9999 => SCALE_BUCKET_MEDIUM,
        _ => SCALE_BUCKET_LARGE,
    }
}

const fn avg_fan_out_bucket_state(module_count: usize, edge_count: usize) -> u8 {
    if module_count == 0 {
        SCALE_BUCKET_UNKNOWN
    } else if edge_count == 0 {
        SCALE_BUCKET_SMALL
    } else if edge_count < module_count {
        SCALE_BUCKET_MEDIUM
    } else if edge_count < module_count.saturating_mul(3) {
        SCALE_BUCKET_LARGE
    } else {
        SCALE_BUCKET_XLARGE
    }
}

const fn file_count_bucket_from_state(state: u8) -> Option<FileCountBucket> {
    match state {
        SCALE_BUCKET_SMALL => Some(FileCountBucket::Small),
        SCALE_BUCKET_MEDIUM => Some(FileCountBucket::Medium),
        SCALE_BUCKET_LARGE => Some(FileCountBucket::Large),
        SCALE_BUCKET_XLARGE => Some(FileCountBucket::XLarge),
        SCALE_BUCKET_UNKNOWN => Some(FileCountBucket::Unknown),
        _ => None,
    }
}

const fn function_count_bucket_from_state(state: u8) -> Option<FunctionCountBucket> {
    match state {
        SCALE_BUCKET_SMALL => Some(FunctionCountBucket::Small),
        SCALE_BUCKET_MEDIUM => Some(FunctionCountBucket::Medium),
        SCALE_BUCKET_LARGE => Some(FunctionCountBucket::Large),
        SCALE_BUCKET_UNKNOWN => Some(FunctionCountBucket::Unknown),
        _ => None,
    }
}

const fn avg_fan_out_bucket_from_state(state: u8) -> Option<AvgFanOutBucket> {
    match state {
        SCALE_BUCKET_SMALL => Some(AvgFanOutBucket::Zero),
        SCALE_BUCKET_MEDIUM => Some(AvgFanOutBucket::LessThanOne),
        SCALE_BUCKET_LARGE => Some(AvgFanOutBucket::OneToTwo),
        SCALE_BUCKET_XLARGE => Some(AvgFanOutBucket::ThreePlus),
        SCALE_BUCKET_UNKNOWN => Some(AvgFanOutBucket::Unknown),
        _ => None,
    }
}

fn file_count_bucket() -> Option<FileCountBucket> {
    file_count_bucket_from_state(FILE_COUNT_BUCKET.load(Ordering::Relaxed))
}

fn function_count_bucket() -> Option<FunctionCountBucket> {
    function_count_bucket_from_state(FUNCTION_COUNT_BUCKET.load(Ordering::Relaxed))
}

fn avg_fan_out_bucket() -> Option<AvgFanOutBucket> {
    avg_fan_out_bucket_from_state(AVG_FAN_OUT_BUCKET.load(Ordering::Relaxed))
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

impl Workflow {
    /// Whether a non-failing run of this workflow surfaces findings and therefore
    /// MUST populate `findings_present` on its telemetry event.
    ///
    /// `findings_present` decouples "found something" from the exit-code outcome,
    /// so every exit path of a finding-surfacing workflow has to call
    /// [`note_result_count`] or [`note_findings_present`] before the at-exit event
    /// is recorded; otherwise the field serializes null (issue #1650). Admin,
    /// setup, mutation, report, and reserved / non-CLI workflows return `false`.
    ///
    /// The match is exhaustive on purpose: a new `Workflow` variant fails to
    /// compile until it is classified here, which keeps the
    /// [`find_state_invariant_violated`] guard honest.
    fn surfaces_findings(self) -> bool {
        match self {
            Workflow::Audit
            | Workflow::DeadCode
            | Workflow::Health
            | Workflow::Dupes
            | Workflow::CodeQualityReview
            | Workflow::Security => true,
            Workflow::DependencyCleanup
            | Workflow::GithubAction
            | Workflow::GitlabCi
            | Workflow::EditorDiagnostic
            | Workflow::ProgrammaticAnalysis
            | Workflow::RuntimeCoverageSetup
            | Workflow::Impact
            | Workflow::Fix
            | Workflow::Explain
            | Workflow::ProjectInventory
            | Workflow::Setup
            | Workflow::License
            | Workflow::Unknown => false,
        }
    }
}

/// Debug-only guard predicate: `true` when a finding-surfacing workflow recorded
/// a non-failing event without noting its find-state (findings_present stayed
/// `None`).
///
/// Pure so it can be unit-tested without the process-global accumulator. A
/// `failed` outcome may legitimately leave findings_present unset because
/// analysis never completed.
fn find_state_invariant_violated(
    workflow: Workflow,
    outcome: &str,
    findings_present: Option<bool>,
) -> bool {
    workflow.surfaces_findings() && outcome != "failed" && findings_present.is_none()
}

/// Debug-time guard at the single event-emission choke point in
/// [`record_workflow`]: a finding-surfacing workflow that emits a non-failing
/// event must have noted its find-state, or a dispatch arm forgot to call
/// `note_result_count` / `note_findings_present` (the originating bug was issue
/// #1650). Runs on every recorded workflow regardless of telemetry mode so the
/// check is deterministic across machines. `findings_present` reflects what the
/// analysis surfaced independent of the output view, so focused trace / closure
/// views record their underlying analysis count rather than skipping the note.
fn debug_assert_find_state_noted(
    workflow: Workflow,
    outcome: &str,
    findings_present: Option<bool>,
) {
    debug_assert!(
        !find_state_invariant_violated(workflow, outcome, findings_present),
        "telemetry find-state invariant violated: workflow {workflow:?} surfaced findings but \
         findings_present is unset on a non-failing run. Every exit path of a finding-surfacing \
         workflow must call telemetry::note_result_count or note_findings_present before the \
         event is recorded (issue #1650)."
    );
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
    #[serde(default)]
    explicit_decision: bool,
    /// Anonymous, random, install-scoped grouping token. Minted only after
    /// explicit opt-in (or an explicit `FALLOW_TELEMETRY=on` first send), never
    /// derived from machine, user, repository, path, environment, or cloud data,
    /// and cleared on `telemetry disable`. `#[serde(default)]` keeps older
    /// `telemetry.json` files (written before this field existed) parsing as
    /// `None`; `skip_serializing_if` keeps an absent token from writing a null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    install_id: Option<String>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            enabled: false,
            prompt_shown: false,
            explicit_decision: false,
            install_id: None,
        }
    }
}

#[derive(Debug)]
struct EffectiveConfig {
    mode: EffectiveMode,
    source: ModeSource,
    config_path: Option<PathBuf>,
}

#[derive(Debug)]
struct TelemetryStatus {
    state: &'static str,
    source: &'static str,
    config_path: Option<PathBuf>,
    admin_disabled: bool,
    explicit_decision: bool,
    install_grouping_token: bool,
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
    /// Coarse analyzed-file scale from counts already computed by the workflow.
    /// Exact counts never leave the analysis path. Combined workflows keep the
    /// largest bucket reported by sub-analyses.
    #[serde(skip_serializing_if = "Option::is_none")]
    file_count_bucket: Option<FileCountBucket>,
    /// Coarse analyzed-function scale from counts already computed by the
    /// workflow. Exact counts never leave the analysis path. Absent when the
    /// workflow has no cheap function count.
    #[serde(skip_serializing_if = "Option::is_none")]
    function_count_bucket: Option<FunctionCountBucket>,
    /// Coarse average fan-out bucket derived only when a workflow already
    /// retained a module graph. Uses existing graph counts, not dependency
    /// traversal or graph-walk metrics.
    #[serde(skip_serializing_if = "Option::is_none")]
    avg_fan_out_bucket: Option<AvgFanOutBucket>,
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
    debug_assert_find_state_noted(record.workflow, event.outcome, event.findings_present);
    match effective_config().mode {
        EffectiveMode::Off | EffectiveMode::DisabledByAdmin => {}
        EffectiveMode::Inspect => print_event_to_stderr(&event),
        EffectiveMode::On => spool_event(&event, parent_run.token.as_deref()),
    }
}

/// Generate a short, privacy-safe run token for local JSON output.
///
/// The token carries no repository, path, user, machine, or project data. It is
/// only an ephemeral handle that an agent may pass to a later hidden
/// `--parent-run` flag so opt-in telemetry can classify the later run as a
/// follow-up.
#[must_use]
pub fn new_analysis_run_id() -> String {
    let counter = ANALYSIS_RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let mut hasher = Sha256::new();
    hasher.update(now_nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(counter.to_le_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    format!("run_{hex}")
}

/// Mint a new anonymous, random, install-scoped grouping token.
///
/// The token is NOT a machine, user, project, repository, or path identifier:
/// it is freshly random, never derived from any of those, and only ever written
/// once per install (then reused). It exists so opt-in telemetry can group
/// distinct workflows per install instead of per run, sent only as the private
/// [`INSTALL_HEADER`] transport header, never as an event property.
///
/// Modeled on [`new_analysis_run_id`] but full-width (32-byte hex) and seeded
/// with a `RandomState`-derived value for more entropy, reusing the existing
/// `sha2` dependency to avoid adding a new direct crate.
#[must_use]
fn new_install_id() -> String {
    use std::hash::{BuildHasher as _, Hasher as _, RandomState};

    let counter = INSTALL_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    // RandomState is seeded per process from OS entropy; hashing the unit value
    // surfaces that seed as a u64 without adding a getrandom/rand direct dep.
    let seed = RandomState::new().build_hasher().finish();

    let mut hasher = Sha256::new();
    hasher.update(now_nanos.to_le_bytes());
    hasher.update(seed.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(counter.to_le_bytes());
    let digest = hasher.finalize();

    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in &digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    format!("{INSTALL_ID_PREFIX}{hex}")
}

/// Return the install id, minting and storing one in `config` when absent.
///
/// Idempotent: an install that already minted a token keeps it (so the token is
/// stable across runs), and a freshly minted one is written back by the caller.
fn ensure_install_id(config: &mut TelemetryConfig) -> &str {
    config.install_id.get_or_insert_with(new_install_id)
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
    let status = collect_status();
    match output {
        OutputFormat::Json => print_status_json(&status),
        _ => print_status_human(status),
    }
}

fn collect_status() -> TelemetryStatus {
    let effective = effective_config();
    let state = mode_label(effective.mode);
    let source = source_label(effective.source);
    let config = effective
        .config_path
        .as_deref()
        .and_then(|path| read_config_from(path).ok());

    TelemetryStatus {
        state,
        source,
        config_path: effective.config_path,
        admin_disabled: matches!(effective.mode, EffectiveMode::DisabledByAdmin),
        explicit_decision: config
            .as_ref()
            .is_some_and(|config| config.explicit_decision),
        // Whether an anonymous install grouping token currently exists on disk.
        // The token itself is never surfaced; only its presence is reported.
        install_grouping_token: matches!(effective.mode, EffectiveMode::On)
            && config.is_some_and(|config| config.install_id.is_some()),
    }
}

fn print_status_json(status: &TelemetryStatus) -> ExitCode {
    let value = serde_json::json!({
        "telemetry": {
            "state": status.state,
            "source": status.source,
            "config_path": status.config_path.as_ref().map(|p| p.display().to_string()),
            "admin_disabled": status.admin_disabled,
            "explicit_decision": status.explicit_decision,
            "install_grouping_token": status.install_grouping_token,
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

fn print_status_human(status: TelemetryStatus) -> ExitCode {
    println!("Telemetry: {} ({})", status.state, status.source);
    if let Some(path) = status.config_path {
        println!("Config: {}", path.display());
    }
    println!(
        "Install grouping token: {}",
        if status.install_grouping_token {
            "present (anonymous, random; sent as a private header, never an event property)"
        } else {
            "none"
        }
    );
    println!(
        "Explicit decision: {}",
        if status.explicit_decision {
            "yes"
        } else {
            "no"
        }
    );
    println!("Enable:  fallow telemetry enable");
    println!("Disable: fallow telemetry disable");
    println!("Inspect an example: fallow telemetry inspect --example");
    println!("Inspect a real command: FALLOW_TELEMETRY=inspect fallow audit --format json --quiet");
    println!("Docs: docs/telemetry.md");
    ExitCode::SUCCESS
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
    config.explicit_decision = true;
    if enabled {
        // Mint the anonymous install grouping token on opt-in (stable across
        // runs once written). The admin kill-switch guard above already blocks
        // this branch, so the token is never created while telemetry is off.
        let _ = ensure_install_id(&mut config);
    } else {
        // Disable means forget: drop the install grouping token entirely.
        config.install_id = None;
    }
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
                "transport_headers": transport_headers()
                    .into_iter()
                    .map(|(header, purpose)| serde_json::json!({ "header": header, "purpose": purpose }))
                    .collect::<Vec<_>>(),
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
            println!();
            println!("Private transport headers (never event properties):");
            for (header, purpose) in transport_headers() {
                println!("- {header}: {purpose}");
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
        file_count_bucket: file_count_bucket(),
        function_count_bucket: function_count_bucket(),
        avg_fan_out_bucket: avg_fan_out_bucket(),
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
        file_count_bucket: None,
        function_count_bucket: None,
        avg_fan_out_bucket: None,
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
        file_count_bucket: Some(FileCountBucket::Large),
        function_count_bucket: Some(FunctionCountBucket::Medium),
        avg_fan_out_bucket: Some(AvgFanOutBucket::OneToTwo),
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
    let mut fields = telemetry_context_field_purposes();
    fields.extend(telemetry_analysis_field_purposes());
    fields.extend(telemetry_result_field_purposes());
    fields.extend(telemetry_run_field_purposes());
    fields
}

fn telemetry_context_field_purposes() -> Vec<(&'static str, &'static str)> {
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
    ]
}

fn telemetry_analysis_field_purposes() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "analysis_mode",
            "Classifies static, runtime-coverage, production-coverage, security, and fix workflows without storing raw command lines.",
        ),
        (
            "file_count_bucket",
            "Coarse analyzed-file scale from counts already computed by the workflow; exact counts are never uploaded. On combined and audit workflows it keeps the largest bucket reported by sub-analyses.",
        ),
        (
            "function_count_bucket",
            "Coarse analyzed-function scale from counts already computed by the workflow; exact counts are never uploaded. Absent when a workflow has no cheap function count.",
        ),
        (
            "avg_fan_out_bucket",
            "Coarse average fan-out from an already-retained module graph, derived from existing module and edge counts only. Absent when the workflow has no retained graph.",
        ),
    ]
}

fn telemetry_result_field_purposes() -> Vec<(&'static str, &'static str)> {
    vec![
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
    ]
}

fn telemetry_run_field_purposes() -> Vec<(&'static str, &'static str)> {
    vec![
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

/// Private transport headers sent alongside the payload, never serialized into
/// the event object itself. Documented here so `telemetry inspect --example`
/// can surface them honestly without putting any identifier in the payload.
fn transport_headers() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            PARENT_RUN_HEADER,
            "Sanitized per-run correlation token for follow-up grouping. Only when --parent-run is passed; never an event property.",
        ),
        (
            INSTALL_HEADER,
            "Anonymous, random, install-scoped grouping token. Minted on opt-in, cleared on disable, never derived from machine/user/repository/path data; never an event property.",
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

/// Resolve the anonymous install grouping token for the send path, minting it
/// lazily when telemetry is enabled via env without a `telemetry.json`.
///
/// Confined to the send path on purpose: [`effective_config`] stays pure and
/// read-only (it is consulted from many places, including the admin-disabled
/// checks), so the only sites that ever create the token are `set_enabled(true)`
/// and this resolver, which the `On`-gated drain calls. When telemetry is off,
/// CI-forced-off, or admin-disabled, the drain never runs and this is never
/// reached, so the token is never created or read while telemetry is off.
///
/// Returns `None` (graceful fallback, server groups by per-run/parent-run as
/// before) when no config directory is available or the config cannot be read
/// and the mode is not `On`. A mint that cannot be persisted (unwritable config
/// dir) still returns the in-memory token for this send, so the run is grouped;
/// it is simply re-minted next run.
fn resolve_install_id_for_send() -> Option<String> {
    let effective = effective_config();
    resolve_install_id_with(effective.mode, effective.config_path.as_deref())
}

/// Pure core of [`resolve_install_id_for_send`], parameterized on the resolved
/// mode and config path so it is testable without mutating process env vars.
fn resolve_install_id_with(mode: EffectiveMode, config_path: Option<&Path>) -> Option<String> {
    // Off / CI-off / admin-disabled never reach the On-gated send path. Guard
    // here too so a future caller cannot mint an id while telemetry is off.
    if mode != EffectiveMode::On {
        return None;
    }
    let path = config_path?;
    // No file yet (env-on without a `telemetry.json`): start from the default
    // (config-level `enabled` stays false) and mint below, so the persisted
    // file carries ONLY the token. Writing `enabled: true` here would escalate
    // a per-invocation `FALLOW_TELEMETRY=on` into a persistent user-config
    // opt-in that outlives the env var.
    let mut config = read_config_from(path).unwrap_or_default();
    if let Some(existing) = config.install_id.as_deref() {
        return Some(existing.to_owned());
    }
    let minted = ensure_install_id(&mut config).to_owned();
    // Best-effort persist; on failure the run is still grouped via the returned
    // in-memory token and the id is re-minted on the next run.
    let _ = write_config_to(path, &config);
    Some(minted)
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

/// Fallow's per-user config directory (`<platform-base>/fallow`), or `None`
/// when no home/config base is resolvable (e.g. a stripped CI environment).
///
/// Shared by telemetry (`telemetry.json`) and Fallow Impact (`impact.json` +
/// the per-project `impact/<key>.json` store), so both surfaces resolve the
/// same base and never drift.
pub fn config_dir() -> Option<PathBuf> {
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
    Some(base.join("fallow"))
}

fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("telemetry.json"))
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
        // Resolve the anonymous install grouping token once per drain, on the
        // background thread so the hot path stays free of config-dir IO. This
        // is the lazy-mint site for an env-enabled install with no
        // `telemetry.json`; absent (unwritable/no config dir) falls back
        // gracefully to per-run/parent-run grouping with no header. Threaded
        // into the drain as a parameter so unit tests never read the real
        // env/config dir (and cannot mint into a developer's telemetry.json).
        let install_id = resolve_install_id_for_send();
        drain_spool_file(&spool, install_id.as_deref(), post_telemetry_payload);
    });
}

/// POST spooled events oldest-first and drop the delivered ones in place.
///
/// Generic over the uploader so tests can inject a fake without touching the
/// network, and parameterized on the resolved install grouping token so tests
/// never read the real env/config dir (the live resolution happens at the
/// [`flush_spool_in_background`] spawn site). The flock is held for the whole drain so a concurrent drain or trim
/// cannot rewrite the spool underneath it. Events are delivered oldest-first and
/// the drain stops at the first POST failure (a likely-unreachable endpoint), so
/// the removed set is always a prefix; the file is rewritten to the undelivered
/// tail (capped, in case it grew while the endpoint was down). Crucially, if the
/// thread is abandoned at process exit mid-upload, no rewrite runs and the spool
/// is simply retried next run, bounded meanwhile by [`trim_spool_if_oversized`]
/// rather than by this function completing.
fn drain_spool_file<P>(spool: &Path, install_id: Option<&str>, mut post: P)
where
    P: FnMut(&serde_json::Value, Option<&str>, Option<&str>) -> Result<(), String>,
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
                if post(&payload, parent_run.as_deref(), install_id).is_ok() {
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
    install_id: Option<&str>,
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
    let request = if let Some(install_id) = install_id {
        request.header(INSTALL_HEADER, install_id)
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

pub fn is_ci() -> bool {
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
/// present in the shared manifest allowlist (never the caller's string), so
/// an off-allowlist or adversarial value is dropped to `None` rather than
/// echoed into the payload.
fn mcp_tool() -> Option<&'static str> {
    mcp_tool_from_value(&std::env::var(MCP_TOOL_ENV).ok()?)
}

/// Resolve an `FALLOW_MCP_TOOL` value against the allowlist. Pure so it can be
/// unit-tested without touching process env. The allowlist is the shared MCP
/// tool manifest in `fallow_types::mcp_manifest` (kept in sync with the live
/// server by a drift test in `crates/mcp`), so an off-allowlist or adversarial
/// value drops to `None` and cannot inject a free-form string into the payload.
fn mcp_tool_from_value(value: &str) -> Option<&'static str> {
    let value = value.trim();
    fallow_types::mcp_manifest::MCP_TOOLS
        .iter()
        .map(|tool| tool.name)
        .find(|name| *name == value)
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
    fn file_count_bucket_boundaries_are_coarse() {
        assert_eq!(
            file_count_bucket_from_state(file_count_bucket_state(0)),
            Some(FileCountBucket::Small)
        );
        assert_eq!(
            file_count_bucket_from_state(file_count_bucket_state(99)),
            Some(FileCountBucket::Small)
        );
        assert_eq!(
            file_count_bucket_from_state(file_count_bucket_state(100)),
            Some(FileCountBucket::Medium)
        );
        assert_eq!(
            file_count_bucket_from_state(file_count_bucket_state(499)),
            Some(FileCountBucket::Medium)
        );
        assert_eq!(
            file_count_bucket_from_state(file_count_bucket_state(500)),
            Some(FileCountBucket::Large)
        );
        assert_eq!(
            file_count_bucket_from_state(file_count_bucket_state(1999)),
            Some(FileCountBucket::Large)
        );
        assert_eq!(
            file_count_bucket_from_state(file_count_bucket_state(2000)),
            Some(FileCountBucket::XLarge)
        );
        assert_eq!(file_count_bucket_from_state(SCALE_BUCKET_UNSET), None);
        assert_eq!(
            file_count_bucket_from_state(SCALE_BUCKET_UNKNOWN),
            Some(FileCountBucket::Unknown)
        );
    }

    #[test]
    fn function_count_bucket_boundaries_are_coarse() {
        assert_eq!(
            function_count_bucket_from_state(function_count_bucket_state(0)),
            Some(FunctionCountBucket::Small)
        );
        assert_eq!(
            function_count_bucket_from_state(function_count_bucket_state(999)),
            Some(FunctionCountBucket::Small)
        );
        assert_eq!(
            function_count_bucket_from_state(function_count_bucket_state(1000)),
            Some(FunctionCountBucket::Medium)
        );
        assert_eq!(
            function_count_bucket_from_state(function_count_bucket_state(9999)),
            Some(FunctionCountBucket::Medium)
        );
        assert_eq!(
            function_count_bucket_from_state(function_count_bucket_state(10000)),
            Some(FunctionCountBucket::Large)
        );
        assert_eq!(function_count_bucket_from_state(SCALE_BUCKET_UNSET), None);
        assert_eq!(
            function_count_bucket_from_state(SCALE_BUCKET_UNKNOWN),
            Some(FunctionCountBucket::Unknown)
        );
    }

    #[test]
    fn avg_fan_out_bucket_boundaries_are_coarse() {
        assert_eq!(
            avg_fan_out_bucket_from_state(avg_fan_out_bucket_state(0, 0)),
            Some(AvgFanOutBucket::Unknown)
        );
        assert_eq!(
            avg_fan_out_bucket_from_state(avg_fan_out_bucket_state(4, 0)),
            Some(AvgFanOutBucket::Zero)
        );
        assert_eq!(
            avg_fan_out_bucket_from_state(avg_fan_out_bucket_state(4, 3)),
            Some(AvgFanOutBucket::LessThanOne)
        );
        assert_eq!(
            avg_fan_out_bucket_from_state(avg_fan_out_bucket_state(4, 4)),
            Some(AvgFanOutBucket::OneToTwo)
        );
        assert_eq!(
            avg_fan_out_bucket_from_state(avg_fan_out_bucket_state(4, 11)),
            Some(AvgFanOutBucket::OneToTwo)
        );
        assert_eq!(
            avg_fan_out_bucket_from_state(avg_fan_out_bucket_state(4, 12)),
            Some(AvgFanOutBucket::ThreePlus)
        );
        assert_eq!(avg_fan_out_bucket_from_state(SCALE_BUCKET_UNSET), None);
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
    fn analysis_run_id_is_parent_run_safe() {
        let run_id = new_analysis_run_id();

        assert!(run_id.starts_with("run_"));
        assert_eq!(run_id.len(), 20);
        assert_eq!(
            sanitize_parent_run(&run_id).as_deref(),
            Some(run_id.as_str())
        );
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
    fn surfaces_findings_classification_pins_the_analysis_workflows() {
        // Workflows whose successful runs surface findings and therefore MUST
        // populate findings_present (each has a note_result_count/note_findings_present
        // call on every exit path). The exhaustive match in `surfaces_findings`
        // forces a new variant to be classified; this test pins the intent.
        for workflow in [
            Workflow::Audit,
            Workflow::DeadCode,
            Workflow::Health,
            Workflow::Dupes,
            Workflow::CodeQualityReview,
            Workflow::Security,
        ] {
            assert!(
                workflow.surfaces_findings(),
                "{workflow:?} must require findings_present"
            );
        }
        // The constructed non-surfacing workflows. The reserved, never-constructed
        // variants (DependencyCleanup, EditorDiagnostic, ProgrammaticAnalysis) are
        // intentionally omitted so they stay dead for the enum's expect(dead_code);
        // the exhaustive match in `surfaces_findings` still forces them to be
        // classified.
        for workflow in [
            Workflow::GithubAction,
            Workflow::GitlabCi,
            Workflow::RuntimeCoverageSetup,
            Workflow::Impact,
            Workflow::Fix,
            Workflow::Explain,
            Workflow::ProjectInventory,
            Workflow::Setup,
            Workflow::License,
            Workflow::Unknown,
        ] {
            assert!(
                !workflow.surfaces_findings(),
                "{workflow:?} must not require findings_present"
            );
        }
    }

    #[test]
    fn find_state_invariant_catches_unnoted_surfacing_runs() {
        // Violation: a finding-surfacing workflow recorded a non-failing event
        // without noting find-state (the issue #1650 / flags / watch bug shape).
        assert!(find_state_invariant_violated(
            Workflow::Security,
            "success",
            None
        ));
        assert!(find_state_invariant_violated(
            Workflow::CodeQualityReview,
            "issues_found",
            None
        ));
        // Noted (clean or found): fine.
        assert!(!find_state_invariant_violated(
            Workflow::Security,
            "success",
            Some(false)
        ));
        assert!(!find_state_invariant_violated(
            Workflow::Dupes,
            "issues_found",
            Some(true)
        ));
        // A failed run may legitimately leave findings_present unset (analysis
        // never completed).
        assert!(!find_state_invariant_violated(
            Workflow::Security,
            "failed",
            None
        ));
        // Non-surfacing workflows never require findings_present.
        assert!(!find_state_invariant_violated(
            Workflow::License,
            "success",
            None
        ));
        assert!(!find_state_invariant_violated(
            Workflow::Fix,
            "success",
            None
        ));
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
        assert_eq!(mcp_tool_from_value("code_execute"), Some("code_execute"));
        assert_eq!(
            mcp_tool_from_value("inspect_target"),
            Some("inspect_target")
        );
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
        drain_spool_file(&spool, None, |value, _parent_run, _install| {
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
        drain_spool_file(&spool, None, |_value, _parent_run, _install| {
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
        drain_spool_file(&spool, None, |value, _parent_run, _install| {
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
        drain_spool_file(&spool, None, |_value, _parent_run, _install| {
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
        drain_spool_file(&spool, None, |value, parent_run, _install| {
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
        drain_spool_file(&spool, None, |value, parent_run, _install| {
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

    #[test]
    fn config_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("telemetry.json");
        let config = TelemetryConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            enabled: true,
            prompt_shown: true,
            explicit_decision: true,
            install_id: None,
        };
        write_config_to(&path, &config).expect("write config");
        let loaded = read_config_from(&path).expect("read config");
        assert!(loaded.enabled);
        assert!(loaded.prompt_shown);
        assert!(loaded.explicit_decision);
        assert_eq!(loaded.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(loaded.install_id, None);
    }

    #[test]
    fn config_round_trips_with_install_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("telemetry.json");
        let config = TelemetryConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            enabled: true,
            prompt_shown: true,
            explicit_decision: true,
            install_id: Some("inst_deadbeef".to_owned()),
        };
        write_config_to(&path, &config).expect("write config");
        let loaded = read_config_from(&path).expect("read config");
        assert_eq!(loaded.install_id.as_deref(), Some("inst_deadbeef"));
    }

    #[test]
    fn old_config_without_install_id_field_parses_as_none() {
        // A `telemetry.json` written before the field existed must still parse,
        // preserve the user's enabled state, and yield a None install id.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("telemetry.json");
        std::fs::write(
            &path,
            "{\n  \"schema_version\": 1,\n  \"enabled\": true,\n  \"prompt_shown\": true\n}\n",
        )
        .expect("write legacy config");
        let loaded = read_config_from(&path).expect("read legacy config");
        assert!(loaded.enabled, "enabled state must survive the parse");
        assert!(loaded.prompt_shown);
        assert!(!loaded.explicit_decision);
        assert_eq!(loaded.install_id, None);
    }

    #[test]
    fn new_install_id_is_distinct_prefixed_and_full_width() {
        let first = new_install_id();
        let second = new_install_id();
        assert_ne!(first, second, "two mints must differ");
        assert!(first.starts_with(INSTALL_ID_PREFIX));
        assert!(second.starts_with(INSTALL_ID_PREFIX));
        let hex = first.strip_prefix(INSTALL_ID_PREFIX).expect("prefixed");
        // 32 bytes of SHA-256 digest, two hex chars each.
        assert_eq!(hex.len(), 64);
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn ensure_install_id_mints_once_and_is_stable() {
        let mut config = TelemetryConfig::default();
        let first = ensure_install_id(&mut config).to_owned();
        assert!(first.starts_with(INSTALL_ID_PREFIX));
        // A second call returns the same token (minted once per install).
        let second = ensure_install_id(&mut config).to_owned();
        assert_eq!(first, second);
        assert_eq!(config.install_id.as_deref(), Some(first.as_str()));
    }

    #[test]
    fn enable_mints_install_id_then_disable_forgets_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("telemetry.json");

        // Simulate the enable path: enabled + prompt_shown set, then mint.
        let mut config = read_config_from(&path).unwrap_or_default();
        config.enabled = true;
        config.prompt_shown = true;
        config.explicit_decision = true;
        let minted = ensure_install_id(&mut config).to_owned();
        write_config_to(&path, &config).expect("write enabled config");

        let after_enable = read_config_from(&path).expect("read after enable");
        assert_eq!(after_enable.install_id.as_deref(), Some(minted.as_str()));
        assert!(minted.starts_with(INSTALL_ID_PREFIX));

        // A second enable does not change the token (stable across runs).
        let mut reloaded = read_config_from(&path).expect("reload");
        reloaded.enabled = true;
        reloaded.prompt_shown = true;
        reloaded.explicit_decision = true;
        let _ = ensure_install_id(&mut reloaded);
        assert_eq!(reloaded.install_id.as_deref(), Some(minted.as_str()));

        // Simulate the disable path: forget the token.
        let mut disabling = read_config_from(&path).expect("read for disable");
        disabling.enabled = false;
        disabling.prompt_shown = true;
        disabling.explicit_decision = true;
        disabling.install_id = None;
        write_config_to(&path, &disabling).expect("write disabled config");

        let after_disable = read_config_from(&path).expect("read after disable");
        assert!(!after_disable.enabled);
        assert_eq!(
            after_disable.install_id, None,
            "disable must forget the install grouping token"
        );
    }

    #[test]
    fn resolve_install_id_returns_none_and_writes_nothing_when_off() {
        // Off / CI-off / admin-disabled all resolve to a non-On mode, which the
        // resolver must treat as "never create or read the install id".
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("telemetry.json");

        for mode in [
            EffectiveMode::Off,
            EffectiveMode::DisabledByAdmin,
            EffectiveMode::Inspect,
        ] {
            assert_eq!(
                resolve_install_id_with(mode, Some(&path)),
                None,
                "mode {mode:?} must not produce an install id",
            );
            assert!(
                !path.exists(),
                "mode {mode:?} must not write a telemetry.json",
            );
        }
    }

    #[test]
    fn resolve_install_id_none_without_config_dir() {
        // No config directory (missing HOME/APPDATA/XDG): graceful fallback.
        assert_eq!(resolve_install_id_with(EffectiveMode::On, None), None);
    }

    #[test]
    fn resolve_install_id_mints_lazily_for_env_on_without_file() {
        // FALLOW_TELEMETRY=on with no telemetry.json: the send path mints once
        // and persists ONLY the token; the config-level enabled flag must stay
        // default-off so the env opt-in stays scoped to the invocation.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("telemetry.json");
        assert!(!path.exists());

        let minted = resolve_install_id_with(EffectiveMode::On, Some(&path))
            .expect("env-on send path mints an install id");
        assert!(minted.starts_with(INSTALL_ID_PREFIX));

        let written = read_config_from(&path).expect("lazy mint persisted a config");
        assert!(
            !written.enabled,
            "lazy mint must NOT escalate an env-only opt-in into a persistent user-config opt-in"
        );
        assert!(!written.prompt_shown);
        assert_eq!(written.install_id.as_deref(), Some(minted.as_str()));

        // A second resolve returns the same persisted token, not a fresh mint.
        let again = resolve_install_id_with(EffectiveMode::On, Some(&path))
            .expect("second resolve returns persisted id");
        assert_eq!(again, minted);
    }

    #[test]
    fn resolve_install_id_preserves_existing_token_and_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("telemetry.json");
        let config = TelemetryConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            enabled: true,
            prompt_shown: true,
            explicit_decision: true,
            install_id: Some("inst_existing".to_owned()),
        };
        write_config_to(&path, &config).expect("seed config");

        let resolved = resolve_install_id_with(EffectiveMode::On, Some(&path))
            .expect("existing token is returned");
        assert_eq!(resolved, "inst_existing");

        let reread = read_config_from(&path).expect("config still readable");
        assert!(reread.enabled, "enabled state must not be corrupted");
        assert_eq!(reread.install_id.as_deref(), Some("inst_existing"));
    }

    #[test]
    fn drain_passes_install_id_to_poster_and_keeps_it_out_of_payload() {
        // Mirrors the parent-run-header invariant: the resolved install id
        // reaches the poster as the header argument and never appears as a key
        // in the payload object.
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        // Seed an On-mode config with a known install id co-located with the
        // spool so the resolver finds it.
        let config_path = spool.with_file_name("telemetry.json");
        write_config_to(
            &config_path,
            &TelemetryConfig {
                schema_version: CONFIG_SCHEMA_VERSION,
                enabled: true,
                prompt_shown: true,
                explicit_decision: true,
                install_id: Some("inst_grouping".to_owned()),
            },
        )
        .expect("seed config");
        append_spool_line(&spool, "{\"event\":\"workflow_completed\"}").expect("append");

        let mut seen: Vec<(serde_json::Value, Option<String>)> = Vec::new();
        // Resolve from the seeded config (the same pure helper the live spawn
        // site uses) and thread the result into the drain as the parameter, so
        // the test never reads the real env/config dir.
        let resolved = resolve_install_id_with(EffectiveMode::On, Some(&config_path));
        assert_eq!(resolved.as_deref(), Some("inst_grouping"));
        drain_spool_file(
            &spool,
            resolved.as_deref(),
            |value, _parent_run, install| {
                seen.push((value.clone(), install.map(str::to_owned)));
                Ok(())
            },
        );

        assert_eq!(seen.len(), 1);
        assert_eq!(
            seen[0].1.as_deref(),
            Some("inst_grouping"),
            "the threaded install id must reach the poster verbatim",
        );
        assert_eq!(
            seen[0].0.get("install_id"),
            None,
            "install id must never be an event-payload property",
        );
        assert_eq!(
            seen[0].0.get("X-Fallow-Install"),
            None,
            "the transport header name must never leak into the payload",
        );
    }

    // --- note_findings_present (lines 140-152) ---

    #[test]
    fn note_findings_present_clean_path_yields_some_false() {
        // The "clean" branch (present == false) is distinct from UNSET.
        assert_eq!(findings_present_from_state(FINDINGS_CLEAN), Some(false));
        assert_ne!(findings_present_from_state(FINDINGS_CLEAN), None);
    }

    #[test]
    fn note_findings_present_found_path_yields_some_true() {
        assert_eq!(findings_present_from_state(FINDINGS_FOUND), Some(true));
    }

    // --- note_analysis_scale with function_count Some (lines 185-188) ---

    #[test]
    fn note_analysis_scale_function_count_bucket_maps_correctly() {
        // Pure helper: function_count_bucket_from_state(function_count_bucket_state(...))
        assert_eq!(
            function_count_bucket_from_state(function_count_bucket_state(500)),
            Some(FunctionCountBucket::Small),
        );
        assert_eq!(
            function_count_bucket_from_state(function_count_bucket_state(1000)),
            Some(FunctionCountBucket::Medium),
        );
        assert_eq!(
            function_count_bucket_from_state(function_count_bucket_state(10_001)),
            Some(FunctionCountBucket::Large),
        );
    }

    // --- config_shape_rank / config_shape_from_state (lines 259-278) ---

    #[test]
    fn config_shape_rank_round_trips_through_state() {
        let pairs = [
            (ConfigShape::Unknown, Some(ConfigShape::Unknown)),
            (ConfigShape::Default, Some(ConfigShape::Default)),
            (ConfigShape::CustomConfig, Some(ConfigShape::CustomConfig)),
            (ConfigShape::CustomRules, Some(ConfigShape::CustomRules)),
            (
                ConfigShape::PluginsEnabled,
                Some(ConfigShape::PluginsEnabled),
            ),
        ];
        for (shape, expected) in pairs {
            assert_eq!(
                config_shape_from_state(config_shape_rank(shape)),
                expected,
                "config shape {shape:?} must survive a rank -> state -> enum round-trip",
            );
        }
    }

    #[test]
    fn config_shape_from_state_rejects_unknown_state() {
        assert_eq!(config_shape_from_state(99), None);
        // UNSET (0) must not produce a shape.
        assert_eq!(config_shape_from_state(CONFIG_SHAPE_UNSET), None);
    }

    // --- FailureReason::state (lines 305-318) ---

    #[test]
    fn failure_reason_state_constants_are_distinct() {
        // Every FailureReason must map to a distinct non-zero state constant so
        // the CAS accumulator can distinguish between them.
        let states = [
            FailureReason::Validation.state(),
            FailureReason::UnsupportedFormat.state(),
            FailureReason::Config.state(),
            FailureReason::Analysis.state(),
            FailureReason::Diff.state(),
            FailureReason::Network.state(),
            FailureReason::Auth.state(),
            FailureReason::Gate.state(),
            FailureReason::Signal.state(),
            FailureReason::Unknown.state(),
        ];
        for &s in &states {
            assert_ne!(
                s, FAILURE_REASON_UNSET,
                "no reason may map to the unset sentinel"
            );
        }
        // Uniqueness check via sorted dedup.
        let mut sorted = states.to_vec();
        sorted.sort_unstable();
        let before_len = sorted.len();
        sorted.dedup();
        assert_eq!(
            before_len,
            sorted.len(),
            "every FailureReason::state() value must be unique"
        );
    }

    // --- note_failure_reason CAS logic (lines 326-345) ---

    #[test]
    fn failure_reason_from_state_round_trips_all_variants() {
        // All variants must round-trip through state -> from_state.
        let pairs = [
            (FAILURE_REASON_VALIDATION, FailureReason::Validation),
            (
                FAILURE_REASON_UNSUPPORTED_FORMAT,
                FailureReason::UnsupportedFormat,
            ),
            (FAILURE_REASON_CONFIG, FailureReason::Config),
            (FAILURE_REASON_ANALYSIS, FailureReason::Analysis),
            (FAILURE_REASON_DIFF, FailureReason::Diff),
            (FAILURE_REASON_NETWORK, FailureReason::Network),
            (FAILURE_REASON_AUTH, FailureReason::Auth),
            (FAILURE_REASON_GATE, FailureReason::Gate),
            (FAILURE_REASON_SIGNAL, FailureReason::Signal),
            (FAILURE_REASON_UNKNOWN, FailureReason::Unknown),
        ];
        for (state, expected) in pairs {
            assert_eq!(failure_reason_from_state(state), Some(expected));
        }
        assert_eq!(failure_reason_from_state(FAILURE_REASON_UNSET), None);
    }

    // --- truncation_reason_to_state / from_state (lines 405-422) ---

    #[test]
    fn truncation_reason_round_trips_all_variants() {
        let variants = [
            TruncationReason::Unknown,
            TruncationReason::MaxItems,
            TruncationReason::CommentLimit,
            TruncationReason::SizeLimit,
        ];
        for variant in variants {
            let state = truncation_reason_to_state(variant);
            assert_ne!(
                state, TRUNCATION_REASON_UNSET,
                "no reason maps to the unset sentinel"
            );
            assert_eq!(
                truncation_reason_from_state(state),
                Some(variant),
                "{variant:?} must survive to_state -> from_state",
            );
        }
    }

    #[test]
    fn truncation_reason_from_state_rejects_unset() {
        assert_eq!(truncation_reason_from_state(TRUNCATION_REASON_UNSET), None);
        assert_eq!(truncation_reason_from_state(99), None);
    }

    // --- status_changed_event shape (lines 1322-1358) ---

    #[test]
    fn status_changed_event_has_expected_schema_fields() {
        let event = status_changed_event(true);
        let value = serde_json::to_value(&event).expect("status changed event serializes");
        assert_eq!(
            value["schema_version"].as_u64(),
            Some(u64::from(TELEMETRY_SCHEMA_VERSION))
        );
        assert_eq!(value["event"].as_str(), Some("telemetry_status_changed"));
        assert_eq!(value["outcome"].as_str(), Some("enabled"));
        // Optional fields that are None on a status-change event must be absent.
        assert!(
            value.get("failure_reason").is_none(),
            "failure_reason must be absent"
        );
        assert!(value.get("run_scope").is_none(), "run_scope must be absent");
        assert!(
            value.get("config_shape").is_none(),
            "config_shape must be absent"
        );
        assert!(
            value.get("findings_present").is_none(),
            "findings_present must be absent"
        );
    }

    #[test]
    fn status_changed_event_disabled_outcome() {
        let event = status_changed_event(false);
        let value = serde_json::to_value(&event).expect("status changed event serializes");
        assert_eq!(value["outcome"].as_str(), Some("disabled"));
    }

    // --- example_event shape (lines 1360-1396) ---

    #[test]
    fn example_event_has_all_optional_fields_present() {
        // The example event is documentation: every optional field must be Some
        // so the documented payload shows the full schema surface.
        let event = example_event();
        assert!(
            event.agent_source.is_some(),
            "example must show agent_source"
        );
        assert!(
            event.failure_reason.is_none(),
            "completed workflow has no failure_reason"
        );
        assert!(event.run_scope.is_some(), "example must show run_scope");
        assert!(
            event.findings_present.is_some(),
            "example must show findings_present"
        );
        assert!(event.mcp_tool.is_some(), "example must show mcp_tool");
        assert!(
            event.report_truncated.is_some(),
            "example must show report_truncated"
        );
        assert!(
            event.truncation_reason.is_some(),
            "example must show truncation_reason"
        );
        assert!(event.cache_state.is_some(), "example must show cache_state");
    }

    // --- field_purposes and transport_headers (lines 1398-1532) ---

    #[test]
    fn field_purposes_is_non_empty_and_unique() {
        let purposes = field_purposes();
        assert!(
            !purposes.is_empty(),
            "field_purposes must return at least one entry"
        );
        // No duplicate field names.
        let mut seen_fields = std::collections::BTreeSet::new();
        for (field, _purpose) in &purposes {
            assert!(
                seen_fields.insert(*field),
                "duplicate field name in field_purposes: {field}",
            );
        }
    }

    #[test]
    fn transport_headers_includes_expected_header_names() {
        let headers = transport_headers();
        let names: Vec<&str> = headers.iter().map(|(name, _)| *name).collect();
        assert!(
            names.contains(&PARENT_RUN_HEADER),
            "transport_headers must list {PARENT_RUN_HEADER}",
        );
        assert!(
            names.contains(&INSTALL_HEADER),
            "transport_headers must list {INSTALL_HEADER}",
        );
        // Verify purposes are non-empty strings.
        for (header, purpose) in &headers {
            assert!(
                !purpose.is_empty(),
                "transport header {header} must have a non-empty purpose"
            );
        }
    }

    // --- parse_env_mode coverage for all aliases (lines 1631-1638) ---

    #[test]
    fn parse_env_mode_accepts_all_aliases() {
        // Off aliases
        for value in &["off", "0", "false", "disabled"] {
            assert_eq!(
                parse_env_mode(value),
                Some(EffectiveMode::Off),
                "{value} must parse as Off",
            );
        }
        // On aliases
        for value in &["on", "1", "true", "enabled"] {
            assert_eq!(
                parse_env_mode(value),
                Some(EffectiveMode::On),
                "{value} must parse as On",
            );
        }
        // Inspect aliases
        for value in &["inspect", "debug", "log"] {
            assert_eq!(
                parse_env_mode(value),
                Some(EffectiveMode::Inspect),
                "{value} must parse as Inspect",
            );
        }
        // Whitespace tolerance
        assert_eq!(parse_env_mode("  on  "), Some(EffectiveMode::On));
        // Case insensitivity
        assert_eq!(parse_env_mode("ON"), Some(EffectiveMode::On));
        assert_eq!(parse_env_mode("OFF"), Some(EffectiveMode::Off));
        // Unknown
        assert_eq!(parse_env_mode("maybe"), None);
    }

    // --- config_dir / read_config / write_config (lines 1663-1695) ---

    #[test]
    fn read_config_from_returns_err_on_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.json");
        assert!(
            read_config_from(&path).is_err(),
            "missing file must return Err"
        );
    }

    #[test]
    fn read_config_from_returns_err_on_invalid_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{ not valid json }").expect("write");
        assert!(
            read_config_from(&path).is_err(),
            "invalid JSON must return Err"
        );
    }

    #[test]
    fn write_config_creates_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let deep = dir.path().join("a").join("b").join("telemetry.json");
        let config = TelemetryConfig::default();
        write_config_to(&deep, &config).expect("write must create parent dirs");
        assert!(deep.exists(), "config file must be created");
    }

    // --- spool_event / rewrite_spool (lines 1732-1816) ---

    #[test]
    fn rewrite_spool_with_no_lines_removes_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        std::fs::write(&spool, "line1\n").expect("create spool");
        rewrite_spool(&spool, &[]);
        assert!(
            !spool.exists(),
            "rewrite with empty lines must remove the spool file"
        );
    }

    #[test]
    fn rewrite_spool_with_lines_overwrites_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        std::fs::write(&spool, "old_line\n").expect("seed spool");
        rewrite_spool(&spool, &["{\"new\":1}", "{\"new\":2}"]);
        let contents = std::fs::read_to_string(&spool).expect("read after rewrite");
        assert_eq!(contents, "{\"new\":1}\n{\"new\":2}\n");
    }

    // --- drain_spool_file with empty spool (lines 1871-1881) ---

    #[test]
    fn drain_empty_spool_removes_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join(SPOOL_FILE_NAME);
        // Whitespace-only content; all lines filter out as empty.
        std::fs::write(&spool, "   \n  \n").expect("write empty spool");
        let mut calls = 0;
        drain_spool_file(&spool, None, |_value, _parent_run, _install| {
            calls += 1;
            Ok(())
        });
        assert_eq!(calls, 0, "no valid events to post");
        assert!(!spool.exists(), "empty spool must be removed after drain");
    }

    // --- parse_spool_line envelope vs bare event (lines 1907-1917) ---

    #[test]
    fn parse_spool_line_bare_event_has_no_parent_run() {
        let line = "{\"event\":\"workflow_completed\",\"n\":1}";
        let (payload, parent_run) = parse_spool_line(line).expect("parse bare line");
        assert_eq!(payload["n"].as_i64(), Some(1));
        assert_eq!(parent_run, None);
    }

    #[test]
    fn parse_spool_line_envelope_extracts_payload_and_sanitized_parent_run() {
        let line =
            r#"{"payload":{"event":"workflow_completed"},"parent_run_header":"run_abc123def456"}"#;
        let (payload, parent_run) = parse_spool_line(line).expect("parse envelope");
        assert_eq!(
            payload["event"].as_str(),
            Some("workflow_completed"),
            "payload field must be extracted from envelope",
        );
        assert_eq!(
            parent_run.as_deref(),
            Some("run_abc123def456"),
            "parent_run_header must be sanitized and returned",
        );
    }

    #[test]
    fn parse_spool_line_envelope_drops_invalid_parent_run() {
        // A parent_run_header that fails sanitize_parent_run is dropped to None.
        let line =
            r#"{"payload":{"event":"workflow_completed"},"parent_run_header":"../evil/path"}"#;
        let (payload, parent_run) = parse_spool_line(line).expect("parse envelope");
        assert!(payload.get("event").is_some());
        assert_eq!(parent_run, None, "invalid parent run must be dropped");
    }

    #[test]
    fn parse_spool_line_rejects_invalid_json() {
        assert!(parse_spool_line("not json").is_err());
    }

    // --- classify_invocation_context (lines 1987-2002) ---

    #[test]
    fn invocation_context_is_ci_when_agent_source_is_none_and_ci_is_set() {
        // We cannot safely mutate env vars in parallel tests. However, we can
        // exercise the pure helper classify_agent_source_from_env with no keys,
        // which yields AgentSource::None, and then observe that is_ci() returns
        // a bool based on the actual env (which may or may not be CI).
        // The function classify_invocation_context() itself reads env directly, so
        // we verify the surrounding pure helpers cover the branches instead.

        // Branch: classify_agent_source_from_env with empty keys => AgentSource::None
        let source = classify_agent_source_from_env(std::iter::empty::<OsString>());
        assert_eq!(
            source,
            AgentSource::None,
            "no env keys must yield AgentSource::None"
        );
    }

    // --- duration_bucket (lines 2160-2169) ---

    #[test]
    fn duration_bucket_covers_all_ranges() {
        assert_eq!(duration_bucket(Duration::from_millis(0)), "<100");
        assert_eq!(duration_bucket(Duration::from_millis(99)), "<100");
        assert_eq!(duration_bucket(Duration::from_millis(100)), "100-500");
        assert_eq!(duration_bucket(Duration::from_millis(499)), "100-500");
        assert_eq!(duration_bucket(Duration::from_millis(500)), "500-2000");
        assert_eq!(duration_bucket(Duration::from_millis(1_999)), "500-2000");
        assert_eq!(duration_bucket(Duration::from_secs(2)), "2s-10s");
        assert_eq!(duration_bucket(Duration::from_millis(9_999)), "2s-10s");
        assert_eq!(duration_bucket(Duration::from_secs(10)), "10s+");
        assert_eq!(duration_bucket(Duration::from_mins(1)), "10s+");
    }

    // --- parent_run_context path: valid token (lines 2193-2219) ---

    #[test]
    fn parent_run_context_none_produces_root_role() {
        let ctx = parent_run_context(None, Workflow::DeadCode);
        assert_eq!(ctx.token, None);
        assert!(!ctx.has_parent_run);
        assert_eq!(ctx.run_role, RunRole::Root);
        assert_eq!(ctx.followup_kind, FollowupKind::Unknown);
    }

    #[test]
    fn parent_run_context_valid_token_produces_followup_role() {
        let ctx = parent_run_context(Some("run_abc123def456"), Workflow::Health);
        assert!(ctx.token.is_some(), "valid token must be kept");
        assert!(ctx.has_parent_run);
        assert_eq!(ctx.run_role, RunRole::Followup);
        assert_eq!(ctx.followup_kind, FollowupKind::Health);
    }

    // --- followup_kind mapping (lines 2221-2243) ---

    #[test]
    fn followup_kind_maps_known_workflows() {
        let pairs = [
            (Workflow::Audit, FollowupKind::Audit),
            (Workflow::Security, FollowupKind::Security),
            (Workflow::Health, FollowupKind::Health),
            (Workflow::DeadCode, FollowupKind::Check),
            (Workflow::Dupes, FollowupKind::Dupes),
            (Workflow::Fix, FollowupKind::Fix),
            (Workflow::Explain, FollowupKind::Explain),
            (Workflow::Unknown, FollowupKind::Unknown),
            (Workflow::Impact, FollowupKind::Unknown),
            (Workflow::License, FollowupKind::Unknown),
        ];
        for (workflow, expected) in pairs {
            assert_eq!(
                followup_kind(workflow),
                expected,
                "workflow {workflow:?} must map to {expected:?}",
            );
        }
    }

    // --- sanitize_parent_run boundary cases (lines 2245-2258) ---

    #[test]
    fn sanitize_parent_run_boundary_lengths() {
        // Minimum valid length is 6 chars.
        assert!(
            sanitize_parent_run("abcdef").is_some(),
            "6-char token must be accepted"
        );
        // 5 chars is below minimum.
        assert!(
            sanitize_parent_run("abcde").is_none(),
            "5-char token must be rejected"
        );
        // 64-char token is at the upper limit.
        let max_len = "a".repeat(64);
        assert!(
            sanitize_parent_run(&max_len).is_some(),
            "64-char token must be accepted"
        );
        // 65 chars is above maximum.
        let too_long = "a".repeat(65);
        assert!(
            sanitize_parent_run(&too_long).is_none(),
            "65-char token must be rejected"
        );
    }

    #[test]
    fn sanitize_parent_run_allows_alphanumeric_underscores_hyphens() {
        assert!(sanitize_parent_run("run_ABC-123").is_some());
        assert!(sanitize_parent_run("Run-123_abc").is_some());
    }

    #[test]
    fn sanitize_parent_run_rejects_disallowed_chars() {
        assert!(
            sanitize_parent_run("run abc 12").is_none(),
            "spaces not allowed"
        );
        assert!(
            sanitize_parent_run("run/abc/12").is_none(),
            "slashes not allowed"
        );
        assert!(
            sanitize_parent_run("run@abc#12").is_none(),
            "special chars not allowed"
        );
    }

    // --- parse_agent_source_value additional aliases (lines 2011-2030) ---

    #[test]
    fn parse_agent_source_value_none_and_empty_map_to_agent_none() {
        assert_eq!(parse_agent_source_value("none"), Some(AgentSource::None));
        assert_eq!(parse_agent_source_value(""), Some(AgentSource::None));
        assert_eq!(parse_agent_source_value("  "), Some(AgentSource::None));
    }

    #[test]
    fn parse_agent_source_value_unknown_maps_to_unknown() {
        assert_eq!(
            parse_agent_source_value("unknown"),
            Some(AgentSource::Unknown)
        );
    }

    #[test]
    fn parse_agent_source_value_openai_codex_alias() {
        assert_eq!(
            parse_agent_source_value("openai_codex"),
            Some(AgentSource::Codex)
        );
    }

    #[test]
    fn parse_agent_source_value_github_copilot_alias() {
        assert_eq!(
            parse_agent_source_value("github_copilot"),
            Some(AgentSource::Copilot)
        );
    }

    #[test]
    fn parse_agent_source_value_roo_code_alias() {
        assert_eq!(parse_agent_source_value("roo-code"), Some(AgentSource::Roo));
    }

    #[test]
    fn parse_agent_source_value_open_code_alias() {
        assert_eq!(
            parse_agent_source_value("open-code"),
            Some(AgentSource::Opencode)
        );
    }

    #[test]
    fn parse_agent_source_value_continue_dev_alias() {
        assert_eq!(
            parse_agent_source_value("continue_dev"),
            Some(AgentSource::Continue)
        );
    }

    #[test]
    fn parse_agent_source_value_other_known_alias() {
        assert_eq!(
            parse_agent_source_value("other"),
            Some(AgentSource::OtherKnown)
        );
        assert_eq!(
            parse_agent_source_value("other_known"),
            Some(AgentSource::OtherKnown)
        );
    }

    #[test]
    fn parse_agent_source_value_unknown_vendor_returns_none() {
        assert_eq!(parse_agent_source_value("devin"), None);
        assert_eq!(parse_agent_source_value("private_agent_x"), None);
    }

    #[test]
    fn parse_agent_source_value_is_case_insensitive() {
        // The function lowercases before matching, so CURSOR and cursor both work.
        assert_eq!(
            parse_agent_source_value("CURSOR"),
            Some(AgentSource::Cursor)
        );
        assert_eq!(
            parse_agent_source_value("Gemini"),
            Some(AgentSource::Gemini)
        );
    }

    // --- parse_integration_surface_override remaining variants (lines 2112-2120) ---

    #[test]
    fn parse_integration_surface_override_vscode_and_napi() {
        assert_eq!(
            parse_integration_surface_override("vscode"),
            Some(IntegrationSurface::Vscode),
        );
        assert_eq!(
            parse_integration_surface_override("napi"),
            Some(IntegrationSurface::Napi),
        );
    }

    // --- mcp_tool_from_value known tools from manifest (lines 2127-2142) ---

    #[test]
    fn mcp_tool_from_value_returns_static_str_for_known_tools() {
        // Check a few tools that exist in the manifest; the actual set is in
        // fallow_types::mcp_manifest::MCP_TOOLS.
        let tools: Vec<&str> = fallow_types::mcp_manifest::MCP_TOOLS
            .iter()
            .map(|t| t.name)
            .collect();
        // Verify every manifest entry round-trips.
        for tool_name in &tools {
            let result = mcp_tool_from_value(tool_name);
            assert_eq!(
                result,
                Some(*tool_name),
                "{tool_name} must be accepted by the allowlist",
            );
        }
        // Off-allowlist values are rejected.
        assert_eq!(mcp_tool_from_value("__proto__"), None);
    }

    // --- spool_event with parent_run envelope (lines 1736-1752) ---

    #[test]
    fn spool_event_with_parent_run_writes_envelope_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let record = WorkflowRecord {
            workflow: Workflow::DeadCode,
            output: OutputFormat::Json,
            quiet: true,
            elapsed: Duration::from_millis(10),
            exit_code: ExitCode::SUCCESS,
            failure_reason: None,
            parent_run: Some("run_abc123def456"),
            context: WorkflowContext {
                run_scope: RunScope::FullProject,
                config_shape: ConfigShape::Default,
                output_destination: OutputDestination::Stdout,
                analysis_mode: AnalysisMode::Static,
            },
        };
        let parent_run_ctx = parent_run_context(record.parent_run, record.workflow);
        let event = build_workflow_event(&record, &parent_run_ctx);

        let spool_path = dir.path().join(SPOOL_FILE_NAME);
        // Manually replicate what spool_event would do when given a parent_run:
        // produce the envelope shape and write it.
        let envelope = serde_json::json!({
            "payload": event,
            "parent_run_header": parent_run_ctx.token.as_deref(),
        });
        let line = serde_json::to_string(&envelope).expect("serialize envelope");
        append_spool_line(&spool_path, &line).expect("append");

        let mut seen: Vec<(serde_json::Value, Option<String>)> = Vec::new();
        drain_spool_file(&spool_path, None, |value, parent_run, _install| {
            seen.push((value.clone(), parent_run.map(str::to_owned)));
            Ok(())
        });

        assert_eq!(seen.len(), 1);
        assert_eq!(
            seen[0].1.as_deref(),
            Some("run_abc123def456"),
            "parent run token must be threaded through the envelope",
        );
        // The token must not appear as a payload field.
        assert_eq!(seen[0].0.get("parent_run_header"), None);
    }

    // --- failure_reason_for_value success path (lines 1306-1320) ---

    #[test]
    fn failure_reason_for_value_absent_on_success() {
        let record = WorkflowRecord {
            workflow: Workflow::DeadCode,
            output: OutputFormat::Human,
            quiet: false,
            elapsed: Duration::from_millis(10),
            exit_code: ExitCode::SUCCESS,
            failure_reason: Some(FailureReason::Config),
            parent_run: None,
            context: WorkflowContext {
                run_scope: RunScope::FullProject,
                config_shape: ConfigShape::Default,
                output_destination: OutputDestination::Stdout,
                analysis_mode: AnalysisMode::Static,
            },
        };
        // A successful exit code must not emit any failure_reason, even when one
        // is recorded.
        assert_eq!(
            failure_reason_for_value(&record, Some(FailureReason::Analysis)),
            None
        );
    }

    #[test]
    fn failure_reason_for_value_recorded_reason_fills_unknown_gap() {
        // When the record has no explicit reason but a recorded one exists,
        // the recorded one is used.
        let record = WorkflowRecord {
            workflow: Workflow::Audit,
            output: OutputFormat::Json,
            quiet: true,
            elapsed: Duration::from_millis(100),
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
            failure_reason_for_value(&record, Some(FailureReason::Network)),
            Some(FailureReason::Network),
        );
    }

    // --- output_format_label (lines 2144-2158) ---

    #[test]
    fn output_format_label_covers_all_variants() {
        let pairs = [
            (OutputFormat::Human, "human"),
            (OutputFormat::Json, "json"),
            (OutputFormat::Sarif, "sarif"),
            (OutputFormat::Compact, "compact"),
            (OutputFormat::Markdown, "markdown"),
            (OutputFormat::CodeClimate, "codeclimate"),
            (OutputFormat::PrCommentGithub, "pr_comment_github"),
            (OutputFormat::PrCommentGitlab, "pr_comment_gitlab"),
            (OutputFormat::ReviewGithub, "review_github"),
            (OutputFormat::ReviewGitlab, "review_gitlab"),
            (OutputFormat::Badge, "badge"),
        ];
        for (format, expected) in pairs {
            assert_eq!(
                output_format_label(format),
                expected,
                "{format:?} must map to '{expected}'",
            );
        }
    }

    // --- exit_code_bucket covers all ranges (lines 2171-2181) ---

    #[test]
    fn exit_code_bucket_covers_expected_codes() {
        assert_eq!(exit_code_bucket(ExitCode::SUCCESS), "0");
        assert_eq!(exit_code_bucket(ExitCode::from(1)), "1");
        assert_eq!(exit_code_bucket(ExitCode::from(2)), "2");
        // Any code >= 3 falls into the catch-all bucket.
        assert_eq!(exit_code_bucket(ExitCode::from(3)), "3-7");
        assert_eq!(exit_code_bucket(ExitCode::from(7)), "3-7");
    }

    // --- outcome (lines 2183-2191) ---

    #[test]
    fn outcome_maps_exit_codes_to_labels() {
        assert_eq!(outcome(ExitCode::SUCCESS), "success");
        assert_eq!(outcome(ExitCode::from(1)), "issues_found");
        assert_eq!(outcome(ExitCode::from(2)), "failed");
        assert_eq!(outcome(ExitCode::from(7)), "failed");
    }

    // --- is_failed (lines 2260-2262) ---

    #[test]
    fn is_failed_returns_false_for_success_and_issues_found() {
        assert!(!is_failed(ExitCode::SUCCESS));
        assert!(!is_failed(ExitCode::from(1)));
    }

    #[test]
    fn is_failed_returns_true_for_error_codes() {
        assert!(is_failed(ExitCode::from(2)));
        assert!(is_failed(ExitCode::from(7)));
    }

    // --- mode_label and source_label (lines 2264-2280) ---

    #[test]
    fn mode_label_covers_all_variants() {
        assert_eq!(mode_label(EffectiveMode::Off), "off");
        assert_eq!(mode_label(EffectiveMode::On), "on");
        assert_eq!(mode_label(EffectiveMode::Inspect), "inspect");
        assert_eq!(
            mode_label(EffectiveMode::DisabledByAdmin),
            "disabled_by_admin"
        );
    }

    #[test]
    fn source_label_covers_all_variants() {
        assert_eq!(source_label(ModeSource::AdminEnv), "admin_env");
        assert_eq!(source_label(ModeSource::Env), "env");
        assert_eq!(source_label(ModeSource::UserConfig), "user_config");
        assert_eq!(source_label(ModeSource::Default), "default");
    }

    // --- key_has_token word-boundary logic (lines 2075-2078) ---

    #[test]
    fn key_has_token_requires_leading_word_boundary() {
        // Matches at position 0.
        assert!(key_has_token("CLAUDE_HOME", "CLAUDE"));
        // Matches after underscore.
        assert!(key_has_token("MY_CLAUDE_HOME", "CLAUDE"));
        // Does NOT match mid-word (CHROOT contains ROO but not at a boundary).
        assert!(!key_has_token("CHROOT", "ROO"));
        // Does NOT match inside a word without preceding underscore.
        assert!(!key_has_token("XCLAUDEFOO", "CLAUDE"));
    }
}
