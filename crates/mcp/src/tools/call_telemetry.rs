//! Opt-in telemetry for tool calls that run in this process.
//!
//! A tool call that runs the fallow CLI gets its one event from that CLI run,
//! tagged with the `mcp` surface and the tool name. Most analysis tools run in
//! this process through `fallow-api` instead, so no CLI event exists for them.
//! For such a call, the server runs `fallow telemetry record` after the
//! answer. The CLI owns the consent rules, the event schema and the spool, so
//! this module decides only whether a call needs that command.
//!
//! The server runs the command only while `fallow telemetry status` reports
//! `on` or `inspect`. It reads that state at most once per [`STATE_TTL`], and
//! never when a kill switch is set, so a server with telemetry off starts no
//! telemetry process per call.

use std::future::Future;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResponse;
use tokio::process::Command;

/// Code Mode runs its host calls on a blocking thread outside the call scope,
/// and each CLI host call already records its own event. A server event for
/// the snippet would count those calls twice.
const UNTRACKED_TOOLS: &[&str] = &["code_execute"];

/// How long the server trusts one reading of the telemetry state. A change
/// that the user makes in a shell reaches a running server within this time.
const STATE_TTL: Duration = Duration::from_mins(10);

/// How long the server waits for `fallow telemetry status`.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

tokio::task_local! {
    static CLI_STARTED: Arc<AtomicBool>;
}

/// The last reading of the telemetry state: when, and whether a call event
/// is wanted.
static STATE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// Note that the current tool call started a fallow CLI process. That process
/// records the event of the call.
pub(super) fn note_cli_started() {
    let _ = CLI_STARTED.try_with(|started| started.store(true, Ordering::Relaxed));
}

/// Run one tool call and record it when it ran in this process.
pub async fn track_call<F>(binary: &str, tool: &str, work: F) -> Result<CallToolResponse, McpError>
where
    F: Future<Output = Result<CallToolResponse, McpError>>,
{
    let started = Instant::now();
    let cli_started = Arc::new(AtomicBool::new(false));
    let result = CLI_STARTED.scope(Arc::clone(&cli_started), work).await;
    let Some(failed) = call_outcome(&result) else {
        return result;
    };
    if cli_started.load(Ordering::Relaxed) || UNTRACKED_TOOLS.contains(&tool) {
        return result;
    }
    if telemetry_wanted(binary).await {
        spawn_record(binary, tool, started.elapsed(), failed);
    }
    result
}

/// `Some(failed)` for a completed call. `None` for a call that waits for
/// client input: its completing request records the call.
fn call_outcome(result: &Result<CallToolResponse, McpError>) -> Option<bool> {
    match result {
        Ok(CallToolResponse::Complete(result)) => Some(result.is_error == Some(true)),
        Ok(_) => None,
        Err(_) => Some(true),
    }
}

/// Whether the user opted in, from the cached state of `fallow telemetry
/// status`.
async fn telemetry_wanted(binary: &str) -> bool {
    if fallow_types::telemetry_opt_out::telemetry_opted_out() {
        return false;
    }
    if let Some(wanted) = cached_state(Instant::now()) {
        return wanted;
    }
    let wanted = read_state(binary).await;
    store_state(Instant::now(), wanted);
    wanted
}

fn cached_state(now: Instant) -> Option<bool> {
    let (read_at, wanted) = (*STATE.lock().ok()?)?;
    (now.duration_since(read_at) < STATE_TTL).then_some(wanted)
}

fn store_state(now: Instant, wanted: bool) {
    if let Ok(mut guard) = STATE.lock() {
        *guard = Some((now, wanted));
    }
}

async fn read_state(binary: &str) -> bool {
    let mut command = Command::new(binary);
    command
        .args(["telemetry", "status", "--format", "json", "--quiet"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let Ok(Ok(output)) = tokio::time::timeout(STATUS_TIMEOUT, command.output()).await else {
        return false;
    };
    output.status.success() && state_wants_events(&output.stdout)
}

/// True when the JSON of `fallow telemetry status` reports a state in which
/// the CLI keeps an event: `on` spools it and `inspect` prints it.
fn state_wants_events(stdout: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(stdout)
        .is_ok_and(|value| matches!(value["telemetry"]["state"].as_str(), Some("on" | "inspect")))
}

/// Start `fallow telemetry record` for one call. The answer of the call does
/// not wait for it, and its result never reaches the client.
fn spawn_record(binary: &str, tool: &str, elapsed: Duration, failed: bool) {
    let mut command = Command::new(binary);
    let duration_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    command
        .args(["telemetry", "record", "--duration-ms"])
        .arg(duration_ms.to_string())
        .env("FALLOW_INTEGRATION_SURFACE", "mcp")
        .env("FALLOW_MCP_TOOL", tool)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // Inspect mode prints the event on stderr, which the client logs.
        .stderr(Stdio::inherit());
    if failed {
        command.arg("--failed");
    }
    let Ok(mut child) = command.spawn() else {
        return;
    };
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
}

#[cfg(test)]
mod tests {
    use rmcp::model::{CallToolResult, ContentBlock};

    use super::*;

    #[test]
    fn status_on_and_inspect_want_events() {
        assert!(state_wants_events(br#"{"telemetry":{"state":"on"}}"#));
        assert!(state_wants_events(br#"{"telemetry":{"state":"inspect"}}"#));
    }

    #[test]
    fn status_off_disabled_or_unreadable_wants_no_events() {
        for raw in [
            br#"{"telemetry":{"state":"off"}}"#.as_slice(),
            br#"{"telemetry":{"state":"disabled_by_admin"}}"#,
            b"not json",
            b"{}",
        ] {
            assert!(!state_wants_events(raw), "{}", String::from_utf8_lossy(raw));
        }
    }

    #[test]
    fn a_reading_expires_after_the_ttl() {
        let now = Instant::now();
        store_state(now, true);
        assert_eq!(cached_state(now + Duration::from_secs(1)), Some(true));
        assert_eq!(cached_state(now + STATE_TTL), None);
    }

    #[test]
    fn a_completed_call_reports_its_outcome() {
        let ok = Ok(CallToolResult::success(vec![ContentBlock::text("{}")]).into());
        let error = Ok(CallToolResult::error(vec![ContentBlock::text("{}")]).into());
        let protocol_error = Err(McpError::internal_error("boom", None));
        assert_eq!(call_outcome(&ok), Some(false));
        assert_eq!(call_outcome(&error), Some(true));
        assert_eq!(call_outcome(&protocol_error), Some(true));
    }

    #[tokio::test]
    async fn a_cli_start_inside_the_call_is_seen() {
        let started = Arc::new(AtomicBool::new(false));
        CLI_STARTED
            .scope(Arc::clone(&started), async { note_cli_started() })
            .await;
        assert!(started.load(Ordering::Relaxed));
    }

    #[test]
    fn a_cli_start_outside_a_call_is_ignored() {
        note_cli_started();
    }
}
