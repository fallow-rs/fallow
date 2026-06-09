//! Opt-in product telemetry for agent and CI workflow quality.
//!
//! The payload is intentionally small and allowlisted. It must never include
//! repository names, paths, package names, config values, raw command lines, or
//! raw agent-detection evidence.

use std::ffi::OsString;
use std::io::{IsTerminal, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use fallow_config::OutputFormat;
use serde::{Deserialize, Serialize};

use crate::api::{api_url, try_api_agent_with_timeout};

const CONFIG_SCHEMA_VERSION: u8 = 1;
const TELEMETRY_SCHEMA_VERSION: u8 = 1;
const CONNECT_TIMEOUT_SECS: u64 = 1;
const TOTAL_TIMEOUT_SECS: u64 = 1;
/// Maximum time the main thread waits for a telemetry upload to finish before
/// continuing. The upload runs on a detached thread; if it has not completed
/// within this grace window the process continues and the thread is abandoned
/// at exit. Telemetry must never add meaningful latency to a sub-second run.
const UPLOAD_GRACE_MS: u64 = 200;
const TELEMETRY_PATH: &str = "/v1/telemetry/events";

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

const FINDINGS_UNSET: u8 = 0;
const FINDINGS_CLEAN: u8 = 1;
const FINDINGS_FOUND: u8 = 2;

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
    /// Whether the analysis surfaced any findings, independent of the exit-code
    /// `outcome` gate. Absent on commands that run no analysis (admin commands)
    /// and on older binaries. On the combined `code_quality_review` and `audit`
    /// workflows this is an OR across the sub-analyses; per-analysis find-rate
    /// is answerable only on the standalone `dead_code` / `dupes` / `health`
    /// workflows.
    #[serde(skip_serializing_if = "Option::is_none")]
    findings_present: Option<bool>,
    /// The MCP tool that triggered this run, when invoked through the MCP
    /// server. Allowlisted to the fixed set of tool names; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    mcp_tool: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_run: Option<String>,
}

pub struct WorkflowRecord<'a> {
    pub workflow: Workflow,
    pub output: OutputFormat,
    pub quiet: bool,
    pub elapsed: Duration,
    pub exit_code: ExitCode,
    pub parent_run: Option<&'a str>,
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
    match effective_config().mode {
        EffectiveMode::Off | EffectiveMode::DisabledByAdmin => {}
        EffectiveMode::Inspect => print_event_to_stderr(&build_workflow_event(record)),
        EffectiveMode::On => upload_event_best_effort(build_workflow_event(record)),
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
            EffectiveMode::On => upload_event_best_effort(event),
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

fn build_workflow_event(record: &WorkflowRecord<'_>) -> TelemetryEvent {
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
        findings_present: findings_present(),
        mcp_tool: mcp_tool(),
        parent_run: record.parent_run.and_then(sanitize_parent_run),
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
        findings_present: None,
        mcp_tool: None,
        parent_run: None,
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
        findings_present: Some(true),
        mcp_tool: Some("find_dupes"),
        parent_run: Some("tmp_8x7p4k".to_owned()),
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
            "parent_run",
            "Links explicit agent follow-up runs using a short allowlisted token, never a path or free-form string.",
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
            "findings_present",
            "Whether the analysis surfaced any findings, decoupled from the exit-code gate. On combined and audit workflows it is an OR across sub-analyses; per-analysis find-rate is answerable only on standalone dead_code, dupes, and health.",
        ),
        (
            "mcp_tool",
            "Which MCP tool an agent called, from a fixed allowlist, so MCP usage is attributable per tool.",
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

/// Send a telemetry event without blocking the caller for meaningful time.
///
/// The bounded HTTP POST runs on a detached thread. The main thread waits only
/// up to [`UPLOAD_GRACE_MS`] for it to finish on a healthy network; if the grace
/// window elapses, the caller returns and the thread is abandoned (terminated at
/// process exit). Delivery is best-effort and lossy by design: errors are
/// already discarded, so blocking process exit on the upload would buy nothing.
fn upload_event_best_effort(event: TelemetryEvent) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(upload_event(&event));
    });
    let _ = rx.recv_timeout(Duration::from_millis(UPLOAD_GRACE_MS));
}

fn upload_event(event: &TelemetryEvent) -> Result<(), String> {
    let agent = try_api_agent_with_timeout(CONNECT_TIMEOUT_SECS, TOTAL_TIMEOUT_SECS)
        .map_err(|err| err.to_string())?;
    let url = api_url(TELEMETRY_PATH);
    let response = agent
        .post(&url)
        .send_json(event)
        .map_err(|err| err.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("telemetry endpoint returned {}", response.status()))
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
            parent_run: Some("tmp_123"),
        };
        let event = build_workflow_event(&record);
        assert_eq!(event.event, "workflow_completed");
        assert_eq!(event.duration_bucket_ms, "500-2000");
        assert_eq!(event.outcome, "issues_found");
        assert_eq!(event.exit_code_bucket, "1");
        assert_eq!(event.parent_run.as_deref(), Some("tmp_123"));
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
}
