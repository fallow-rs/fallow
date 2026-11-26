//! Long tool calls as MCP tasks (`io.modelcontextprotocol/tasks`).
//!
//! A `tools/call` of a task tool returns a task handle instead of the result
//! when the request declares the tasks extension in
//! `_meta["io.modelcontextprotocol/clientCapabilities"].extensions`. The
//! client then polls `tasks/get`, and `tasks/cancel` stops the work. A request
//! without the extension keeps the synchronous result.
//!
//! [`TaskStore`] wraps rmcp's [`TaskManager`]. The work of a task runs inside
//! a task-local scope, so the tool code can read [`current_task`]: the timeout
//! helpers use the task deadline, and a tool can surface a server-to-client
//! request with [`TaskContext::request_input`], which sets the task to
//! `input_required`.

use std::future::Future;
use std::time::{Duration, Instant};

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult, Task};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::task_manager::{TaskContext, TaskExit, TaskFuture, TaskManager, TaskOptions};
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;

use crate::params::{AnalyzeParams, AuditParams, FindSimilarCodeParams, MutationPlanParams};

/// The default deadline of a tool call that runs as a task. A task frees the
/// client from its request timeout, so the server default of 120 s would only
/// stop work that the client is willing to wait for. `FALLOW_TIMEOUT_SECS`
/// still sets the deadline when an operator sets it.
pub const TASK_DEFAULT_TIMEOUT_SECS: u64 = 30 * 60;

/// How long a finished task stays readable through `tasks/get`. A user can
/// leave a session and resume it later, so the result must outlive the run.
const TASK_RETENTION: Duration = Duration::from_hours(1);

/// The poll interval for a call that usually ends within a minute.
const SHORT_POLL_INTERVAL_MS: u64 = 2_000;

/// The poll interval for a call that usually runs for minutes.
const LONG_POLL_INTERVAL_MS: u64 = 10_000;

/// How often a running task refreshes its status message.
const STATUS_INTERVAL: Duration = Duration::from_secs(5);

/// How long the server waits at shutdown for its tasks to stop.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

const SHUTDOWN_POLL: Duration = Duration::from_millis(25);

tokio::task_local! {
    static CURRENT_TASK: TaskContext;
}

/// The tasks of one server. All clones share one store.
///
/// The work of each task gets a child of one server token. The stdio loop
/// cancels the request tokens of a synchronous call when the client
/// disconnects, and [`TaskStore::shutdown`] does the same for the tasks, so a
/// task does not leave its subprocess running after the server stops.
#[derive(Clone, Default)]
pub struct TaskStore {
    manager: TaskManager,
    shutdown: CancellationToken,
}

impl TaskStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a task. `make_work` gets the task context and the cancellation
    /// token of the work, which `tasks/cancel` and [`Self::shutdown`] set.
    pub fn spawn<F>(&self, options: TaskOptions, make_work: F) -> Task
    where
        F: FnOnce(TaskContext, CancellationToken) -> TaskFuture,
    {
        let cancellation = self.shutdown.child_token();
        self.manager
            .spawn(options, move |task| make_work(task, cancellation))
    }

    pub fn manager(&self) -> &TaskManager {
        &self.manager
    }

    /// Cancel the work of every task, then wait a short grace period for the
    /// work to stop, so each subprocess route can kill its process tree.
    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        let give_up_at = Instant::now() + SHUTDOWN_GRACE;
        while self.manager.running_task_count() > 0 && Instant::now() < give_up_at {
            tokio::time::sleep(SHUTDOWN_POLL).await;
        }
    }
}

/// How long a task tool usually runs, which sets its poll interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPace {
    /// Usually under a minute.
    Short,
    /// Usually minutes.
    Long,
}

impl TaskPace {
    const fn poll_interval_ms(self) -> u64 {
        match self {
            Self::Short => SHORT_POLL_INTERVAL_MS,
            Self::Long => LONG_POLL_INTERVAL_MS,
        }
    }
}

/// The pace of `request` when its tool runs as a task, else `None`.
///
/// A call whose arguments do not parse stays synchronous, so the client gets
/// the same invalid-params error as without the extension. `analyze` is a
/// task tool only with type-aware analysis, because a syntactic run ends in
/// seconds and a task adds a poll round trip.
pub fn task_pace(request: &CallToolRequestParams) -> Option<TaskPace> {
    match request.name.as_ref() {
        "audit" => parses::<AuditParams>(request).then_some(TaskPace::Short),
        "mutation_plan" => parses::<MutationPlanParams>(request).then_some(TaskPace::Short),
        "find_similar_code" => parses::<FindSimilarCodeParams>(request).then_some(TaskPace::Long),
        "analyze" => arguments::<AnalyzeParams>(request)
            .is_some_and(|params| crate::tools::analyze_requests_type_aware(&params))
            .then_some(TaskPace::Long),
        _ => None,
    }
}

fn arguments<T: DeserializeOwned>(request: &CallToolRequestParams) -> Option<T> {
    let arguments = serde_json::Value::Object(request.arguments.clone().unwrap_or_default());
    serde_json::from_value(arguments).ok()
}

fn parses<T: DeserializeOwned>(request: &CallToolRequestParams) -> bool {
    arguments::<T>(request).is_some()
}

/// Whether the request itself declares the tasks extension. The capabilities
/// of `initialize` do not count: a client declares per request whether it can
/// read a task handle.
pub fn request_declares_tasks(context: &RequestContext<RoleServer>) -> bool {
    context
        .meta
        .client_capabilities()
        .is_some_and(|capabilities| capabilities.supports_tasks())
}

/// The options of a new task for a call with `pace`.
///
/// `ttlMs` counts from the creation of the task. It covers the deadline of
/// the run plus the retention of the result, so a client never sees a task
/// expire while it runs or within an hour after it ends.
pub fn task_options(tool: &str, pace: TaskPace) -> TaskOptions {
    TaskOptions::new()
        .with_ttl_ms(ttl_ms(crate::tools::task_timeout()))
        .with_poll_interval_ms(pace.poll_interval_ms())
        .with_status_message(format!("{tool}: started"))
}

fn ttl_ms(deadline: Duration) -> u64 {
    u64::try_from(deadline.saturating_add(TASK_RETENTION).as_millis()).unwrap_or(u64::MAX)
}

/// The task of the current tool call, when the call runs as a task.
pub fn current_task() -> Option<TaskContext> {
    CURRENT_TASK.try_with(TaskContext::clone).ok()
}

/// Drive the tool call `work` of `task` to its terminal state.
///
/// `cancellation` must be the request token of `work`. A `tasks/cancel` sets
/// it, so the subprocess route kills its process tree and the typed route sets
/// the analysis cancellation flag, as for `notifications/cancelled`. The task
/// then settles as `cancelled`, also when the work returned a result.
pub async fn drive_task<F>(
    task: TaskContext,
    tool: &str,
    cancellation: CancellationToken,
    work: F,
) -> Result<CallToolResult, TaskExit>
where
    F: Future<Output = Result<CallToolResponse, McpError>>,
{
    let started = Instant::now();
    let work = CURRENT_TASK.scope(task.clone(), work);
    tokio::pin!(work);
    let mut status = tokio::time::interval_at(
        tokio::time::Instant::now() + STATUS_INTERVAL,
        STATUS_INTERVAL,
    );
    let response = loop {
        tokio::select! {
            biased;
            response = &mut work => break response,
            () = task.cancelled(), if !cancellation.is_cancelled() => {
                task.set_status_message(format!("{tool}: stopping after a cancel request"));
                cancellation.cancel();
            }
            _ = status.tick(), if !cancellation.is_cancelled() => {
                task.set_status_message(format!("{tool}: running, {} elapsed", elapsed(started.elapsed())));
            }
        }
    };
    if cancellation.is_cancelled() {
        task.set_status_message(format!("{tool}: cancelled"));
        return Err(TaskExit::Cancelled);
    }
    task.set_status_message(format!(
        "{tool}: finished in {}",
        elapsed(started.elapsed())
    ));
    match response {
        Ok(CallToolResponse::Complete(result)) => Ok(result),
        Ok(_) => Err(TaskExit::Error(McpError::internal_error(
            format!("{tool} returned no tool result"),
            None,
        ))),
        Err(error) => Err(TaskExit::Error(error)),
    }
}

/// A duration as `12 s` or `3 min 4 s`.
fn elapsed(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs} s")
    } else {
        format!("{} min {} s", secs / 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(name: &'static str, arguments: serde_json::Value) -> CallToolRequestParams {
        let serde_json::Value::Object(arguments) = arguments else {
            panic!("arguments must be an object");
        };
        CallToolRequestParams::new(name).with_arguments(arguments)
    }

    #[test]
    fn only_the_long_tools_run_as_tasks() {
        let empty = serde_json::json!({});
        assert_eq!(
            task_pace(&request("audit", empty.clone())),
            Some(TaskPace::Short)
        );
        assert_eq!(
            task_pace(&request("mutation_plan", empty.clone())),
            Some(TaskPace::Short)
        );
        assert_eq!(
            task_pace(&request("find_similar_code", empty.clone())),
            Some(TaskPace::Long)
        );
        assert_eq!(task_pace(&request("check_health", empty.clone())), None);
        assert_eq!(task_pace(&request("code_execute", empty)), None);
    }

    #[test]
    fn analyze_runs_as_a_task_only_with_type_aware_analysis() {
        assert_eq!(task_pace(&request("analyze", serde_json::json!({}))), None);
        assert_eq!(
            task_pace(&request(
                "analyze",
                serde_json::json!({ "type_aware": false })
            )),
            None
        );
        assert_eq!(
            task_pace(&request(
                "analyze",
                serde_json::json!({ "type_aware": true })
            )),
            Some(TaskPace::Long)
        );
        assert_eq!(
            task_pace(&request(
                "analyze",
                serde_json::json!({ "type_aware_projects": ["tsconfig.json"] })
            )),
            Some(TaskPace::Long)
        );
    }

    #[test]
    fn the_ttl_covers_the_run_and_an_hour_of_retention() {
        assert_eq!(ttl_ms(Duration::from_mins(30)), 90 * 60 * 1000);
        assert_eq!(ttl_ms(Duration::MAX), u64::MAX);
    }

    #[test]
    fn a_call_with_invalid_arguments_stays_synchronous() {
        let invalid = serde_json::json!({ "root": 42 });
        assert_eq!(task_pace(&request("audit", invalid.clone())), None);
        assert_eq!(task_pace(&request("mutation_plan", invalid.clone())), None);
        assert_eq!(task_pace(&request("find_similar_code", invalid)), None);
    }

    #[tokio::test]
    async fn shutdown_cancels_the_work_of_a_running_task() {
        let store = TaskStore::new();
        let (seen, cancelled) = tokio::sync::oneshot::channel();
        store.spawn(TaskOptions::new(), move |_task, cancellation| {
            Box::pin(async move {
                cancellation.cancelled().await;
                let _ = seen.send(());
                Err(TaskExit::Cancelled)
            })
        });
        store.shutdown().await;
        tokio::time::timeout(Duration::from_secs(5), cancelled)
            .await
            .expect("the work sees the cancel")
            .expect("the work reports the cancel");
        assert_eq!(store.manager().running_task_count(), 0);
    }

    #[test]
    fn elapsed_reads_in_seconds_and_minutes() {
        assert_eq!(elapsed(Duration::from_millis(12_400)), "12 s");
        assert_eq!(elapsed(Duration::from_secs(184)), "3 min 4 s");
    }
}
