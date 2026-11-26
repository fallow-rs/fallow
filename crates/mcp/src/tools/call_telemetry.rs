//! Opt-in telemetry for tool calls that run in this process.
//!
//! A tool call that runs the fallow CLI gets its events from those CLI runs,
//! tagged with the `mcp` surface and the tool name. Most analysis tools run in
//! this process through `fallow-api` instead, so no CLI event exists for them.
//! For such a call, the server runs `fallow telemetry record` after the
//! answer. The CLI owns the consent rules, the event schema and the spool, so
//! this module decides only whether a call needs that command.
//!
//! All the work happens after the answer, on a detached task. The server
//! runs `record` only while `fallow telemetry status` reports `on` or
//! `inspect`. It reads that state at most once per [`STATE_TTL`], and never
//! when a kill switch is set, so a server with telemetry off starts no
//! `record` process.

use std::future::Future;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResponse;
use tokio::process::Command;
use tokio::sync::{Mutex, Semaphore};

/// Code Mode runs its host calls on a blocking thread outside the call scope,
/// and each CLI host call already records its own event. A server event for
/// the snippet would count those calls twice.
const UNTRACKED_TOOLS: &[&str] = &["code_execute"];

/// How long the server trusts one reading of the telemetry state. A change
/// that the user makes in a shell reaches a running server within this time.
const STATE_TTL: Duration = Duration::from_mins(10);

/// How long the server trusts a status read that failed or timed out. A
/// failed read counts as "no event", which is the safe answer, but a slow
/// start under load must not stop the events for the full [`STATE_TTL`].
const FAILED_READ_TTL: Duration = Duration::from_mins(1);

/// How long the server waits for `fallow telemetry status`.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// How long one `fallow telemetry record` process may run. Its upload uses
/// short network timeouts, so a process past this limit is stuck.
const RECORD_TIMEOUT: Duration = Duration::from_secs(60);

/// How many `fallow telemetry record` processes run at once. More calls wait
/// for a free slot on their detached task.
const RECORD_SLOTS: usize = 2;

tokio::task_local! {
    static CLI_STARTED: Arc<AtomicBool>;
}

/// What the CLI does with a call event in the current telemetry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wanted {
    /// Telemetry is off: no `record` process.
    No,
    /// Telemetry is on: `record` spools the event silently.
    Spool,
    /// Inspect mode: `record` prints the event on stderr, which the client
    /// logs.
    Print,
}

/// One reading of the telemetry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Reading {
    read_at: Instant,
    wanted: Wanted,
    /// The read failed or timed out.
    failed: bool,
}

/// The last reading. The lock is held during a read, so calls that end
/// together share one `fallow telemetry status` process.
static STATE: Mutex<Option<Reading>> = Mutex::const_new(None);

static RECORDS: Semaphore = Semaphore::const_new(RECORD_SLOTS);

/// Tool calls that run now. The consent form waits until this is 0.
static CALLS_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// When the last tool call ended, for [`idle_for`].
static LAST_CALL_END: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// How long the server has had no running tool call. `None` while a call
/// runs, and before the first call ends.
pub(super) fn idle_for() -> Option<Duration> {
    if CALLS_IN_FLIGHT.load(Ordering::SeqCst) > 0 {
        return None;
    }
    let ended = (*LAST_CALL_END.lock().ok()?)?;
    Some(ended.elapsed())
}

/// Counts one running call until it drops, also when the call future is
/// dropped on a client cancel.
struct InFlight;

impl InFlight {
    fn start() -> Self {
        CALLS_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if let Ok(mut ended) = LAST_CALL_END.lock() {
            *ended = Some(Instant::now());
        }
        CALLS_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Forget the cached telemetry state, so the next call reads it again. The
/// consent form calls this after it changes the state.
pub(super) async fn forget_state() {
    *STATE.lock().await = None;
}

/// Note that the current tool call started a fallow CLI process. That process
/// records the event of the call.
pub(super) fn note_cli_started() {
    let _ = CLI_STARTED.try_with(|started| started.store(true, Ordering::Relaxed));
}

/// Run one tool call and, when it ran in this process, record it after the
/// answer. The answer never waits for telemetry.
pub async fn track_call<F>(binary: &str, tool: &str, work: F) -> Result<CallToolResponse, McpError>
where
    F: Future<Output = Result<CallToolResponse, McpError>>,
{
    let started = Instant::now();
    let cli_started = Arc::new(AtomicBool::new(false));
    let in_flight = InFlight::start();
    let result = CLI_STARTED.scope(Arc::clone(&cli_started), work).await;
    drop(in_flight);
    let Some(failed) = call_outcome(&result) else {
        return result;
    };
    if cli_started.load(Ordering::Relaxed) || fallow_types::telemetry_opt_out::telemetry_opted_out()
    {
        return result;
    }
    let Some(tool) = recorded_tool(tool) else {
        return result;
    };
    let binary = binary.to_owned();
    let elapsed = started.elapsed();
    tokio::spawn(async move {
        let wanted = telemetry_wanted(&binary).await;
        if wanted != Wanted::No {
            run_record(&binary, tool, elapsed, failed, wanted).await;
        }
    });
    result
}

/// The manifest name of `tool` when the server records it. An unknown name,
/// for example from a malformed request, gives no `record` process.
fn recorded_tool(tool: &str) -> Option<&'static str> {
    if UNTRACKED_TOOLS.contains(&tool) {
        return None;
    }
    fallow_types::mcp_manifest::MCP_TOOLS
        .iter()
        .map(|entry| entry.name)
        .find(|name| *name == tool)
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

/// The current telemetry state, from the cache or from one new read.
async fn telemetry_wanted(binary: &str) -> Wanted {
    let mut state = STATE.lock().await;
    if let Some(wanted) = fresh(*state, Instant::now()) {
        return wanted;
    }
    let read = read_state(binary).await;
    let reading = Reading {
        read_at: Instant::now(),
        wanted: read.unwrap_or(Wanted::No),
        failed: read.is_none(),
    };
    *state = Some(reading);
    drop(state);
    reading.wanted
}

/// The cached answer while `reading` is still fresh at `now`.
fn fresh(reading: Option<Reading>, now: Instant) -> Option<Wanted> {
    let reading = reading?;
    let ttl = if reading.failed {
        FAILED_READ_TTL
    } else {
        STATE_TTL
    };
    (now.duration_since(reading.read_at) < ttl).then_some(reading.wanted)
}

/// Run `fallow telemetry status`. `None` when the run fails, times out or
/// prints no status.
async fn read_state(binary: &str) -> Option<Wanted> {
    let mut command = Command::new(binary);
    command
        .args(["telemetry", "status", "--format", "json", "--quiet"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(STATUS_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    wanted_from_status(&output.stdout)
}

/// Read the state from the JSON of `fallow telemetry status`. `on` spools
/// an event and `inspect` prints it; every other state keeps none.
fn wanted_from_status(stdout: &[u8]) -> Option<Wanted> {
    let value = serde_json::from_slice::<serde_json::Value>(stdout).ok()?;
    Some(match value["telemetry"]["state"].as_str()? {
        "on" => Wanted::Spool,
        "inspect" => Wanted::Print,
        _ => Wanted::No,
    })
}

/// Run `fallow telemetry record` for one call, in a bounded slot and under
/// [`RECORD_TIMEOUT`]. Its result never reaches the client.
async fn run_record(binary: &str, tool: &str, elapsed: Duration, failed: bool, wanted: Wanted) {
    let Ok(_slot) = RECORDS.acquire().await else {
        return;
    };
    let mut command = Command::new(binary);
    let duration_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    command
        .args(["telemetry", "record", "--duration-ms"])
        .arg(duration_ms.to_string())
        .env("FALLOW_INTEGRATION_SURFACE", "mcp")
        .env("FALLOW_MCP_TOOL", tool)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if wanted == Wanted::Print {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .kill_on_drop(true);
    if failed {
        command.arg("--failed");
    }
    let Ok(mut child) = command.spawn() else {
        return;
    };
    let _ = tokio::time::timeout(RECORD_TIMEOUT, child.wait()).await;
}

#[cfg(test)]
mod tests {
    use rmcp::model::{CallToolResult, ContentBlock};

    use super::*;

    #[test]
    fn status_on_spools_and_inspect_prints() {
        assert_eq!(
            wanted_from_status(br#"{"telemetry":{"state":"on"}}"#),
            Some(Wanted::Spool)
        );
        assert_eq!(
            wanted_from_status(br#"{"telemetry":{"state":"inspect"}}"#),
            Some(Wanted::Print)
        );
    }

    #[test]
    fn status_off_or_disabled_wants_no_event() {
        for raw in [
            br#"{"telemetry":{"state":"off"}}"#.as_slice(),
            br#"{"telemetry":{"state":"disabled_by_admin"}}"#,
        ] {
            assert_eq!(wanted_from_status(raw), Some(Wanted::No));
        }
    }

    #[test]
    fn an_unreadable_status_is_a_failed_read() {
        for raw in [b"not json".as_slice(), b"{}"] {
            assert_eq!(wanted_from_status(raw), None);
        }
    }

    #[test]
    fn a_reading_expires_after_its_ttl() {
        let now = Instant::now();
        let ok = Reading {
            read_at: now,
            wanted: Wanted::Spool,
            failed: false,
        };
        assert_eq!(fresh(Some(ok), now + FAILED_READ_TTL), Some(Wanted::Spool));
        assert_eq!(fresh(Some(ok), now + STATE_TTL), None);
        let failed = Reading {
            wanted: Wanted::No,
            failed: true,
            ..ok
        };
        assert_eq!(fresh(Some(failed), now), Some(Wanted::No));
        assert_eq!(fresh(Some(failed), now + FAILED_READ_TTL), None);
        assert_eq!(fresh(None, now), None);
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

    #[test]
    fn only_known_tools_outside_code_mode_are_recorded() {
        assert_eq!(recorded_tool("project_info"), Some("project_info"));
        assert_eq!(recorded_tool("code_execute"), None);
        assert_eq!(recorded_tool("../private/tool"), None);
    }

    #[tokio::test]
    async fn a_cli_start_inside_the_call_is_seen() {
        let started = Arc::new(AtomicBool::new(false));
        CLI_STARTED
            .scope(Arc::clone(&started), async { note_cli_started() })
            .await;
        assert!(started.load(Ordering::Relaxed));
    }
}
