//! Wire types of the mutation runner protocol and the decoder of response
//! lines (`protocol.md` sections 3 and 4).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The `options` of an `init` request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct InitOptions {
    /// The test files that the adapter selects. Empty selects every file.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub test_files: Vec<String>,
    /// The test files of the static map, as root-relative paths. A runner
    /// that discovers its test files with glob patterns adds each file that
    /// its default patterns match outside a dot directory. The patterns do
    /// not match a path in a dot directory, such as `.github/`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_test_files: Vec<String>,
    /// Extra environment variables for the runner.
    pub env: BTreeMap<String, String>,
    /// The Node flags for each test process of the `node:test` runner. A
    /// flag with a value is two entries.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub node_args: Vec<String>,
}

/// The body of an `init` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InitRequest {
    pub root: String,
    pub runner: String,
    pub config_path: Option<String>,
    pub run_dir: String,
    pub path_map: BTreeMap<String, String>,
    pub options: InitOptions,
}

/// The body of a `run` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunRequest {
    /// The mutant to activate. `None` sends `null`: a control run of the
    /// tests with no active mutant.
    pub mutant_id: Option<String>,
    pub test_ids: Vec<String>,
    pub timeout_ms: u64,
    /// The test CPU time in ms after which the companion ends a test that
    /// still evaluates instrumented code, and reports it as a timeout. It
    /// stops a synchronous endless loop that blocks the test worker before
    /// the watchdog fires. `None` sends no field: the run has no loop check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_budget_ms: Option<u64>,
    pub bail: bool,
}

/// The body of a `replay` request (replay contract 1): call `export` of
/// `file` with `args` on the original and on `mutant_id`, `calls` times each,
/// in turns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplayRequest {
    /// The root-relative source file. It must be in the `path_map`.
    pub file: String,
    pub export: String,
    pub mutant_id: String,
    /// The arguments in the replay encoding.
    pub args: Vec<serde_json::Value>,
    pub calls: u32,
    pub call_timeout_ms: u64,
    /// The fixed value of `Date.now()` in each call.
    pub clock_ms: i64,
    /// The seed of `Math.random` in each call.
    pub seed: i64,
}

/// One request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Init(InitRequest),
    DryRun {
        test_files: Vec<String>,
    },
    /// Runs the dry-run test files again with the recorder on, and gives the
    /// recorded arguments of the wrapped exports. Its test results count for
    /// nothing.
    Record {
        test_files: Vec<String>,
    },
    Run(RunRequest),
    Reload,
    Replay(ReplayRequest),
    Shutdown,
}

impl Request {
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Init(_) => "init",
            Self::DryRun { .. } => "dry_run",
            Self::Run(_) => "run",
            Self::Reload => "reload",
            Self::Replay(_) => "replay",
            Self::Record { .. } => "record",
            Self::Shutdown => "shutdown",
        }
    }

    /// The response type that answers the request. `None` for `shutdown`.
    pub const fn response_type(&self) -> Option<&'static str> {
        match self {
            Self::Init(_) => Some("ready"),
            Self::DryRun { .. } => Some("dry_run_result"),
            Self::Run(_) => Some("run_result"),
            Self::Reload => Some("reloaded"),
            Self::Replay(_) => Some("replay_result"),
            Self::Record { .. } => Some("record_result"),
            Self::Shutdown => None,
        }
    }

    /// The JSON line of the request, with no line break.
    pub fn to_line(&self, version: u32, id: u64) -> String {
        let mut object = serde_json::Map::new();
        object.insert("v".to_owned(), version.into());
        object.insert("id".to_owned(), id.into());
        object.insert("type".to_owned(), self.type_name().into());
        let body = match self {
            Self::Init(body) => serde_json::to_value(body),
            Self::DryRun { test_files } | Self::Record { test_files } => {
                Ok(serde_json::json!({ "test_files": test_files }))
            }
            Self::Run(body) => serde_json::to_value(body),
            Self::Replay(body) => serde_json::to_value(body),
            Self::Reload | Self::Shutdown => Ok(serde_json::Value::Null),
        };
        if let Ok(serde_json::Value::Object(fields)) = body {
            object.extend(fields);
        }
        serde_json::Value::Object(object).to_string()
    }
}

/// The capabilities of an adapter. A flag that `ready` does not state is
/// false (fail-closed rule 4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Capabilities {
    pub per_test_attribution: bool,
    pub test_filter: bool,
    pub hot_switch: bool,
    pub bail: bool,
    pub fresh_modules_per_run: bool,
    pub batch_mutants: bool,
    /// The adapter answers a `replay` request.
    pub replay: bool,
    /// The adapter answers a `record` request.
    pub record: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Ready {
    pub runner_version: String,
    pub runtime_version: String,
    #[serde(default)]
    pub test_files: Vec<String>,
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Facts of the runner setup, for example `typecheck-not-run`.
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    Pass,
    Fail,
    Skip,
    Todo,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DryTest {
    pub test_id: String,
    pub file: String,
    #[serde(default)]
    pub duration_ms: Option<f64>,
    pub status: TestStatus,
    /// How a failed test failed: `assertion`, `error` or `timeout`. An older
    /// companion leaves it out.
    #[serde(default)]
    pub failure_kind: Option<String>,
    /// The first error of a failed test, in the shape of
    /// [`RunTest::message`]. An older companion leaves it out.
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UnhandledError {
    pub message: String,
    #[serde(default)]
    pub file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DryRunResult {
    pub tests: Vec<DryTest>,
    #[serde(default)]
    pub hits: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub static_hits: Vec<String>,
    #[serde(default)]
    pub file_hits: Option<BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    pub unhandled_errors: Vec<UnhandledError>,
    #[serde(default)]
    pub unloaded_paths: Vec<String>,
    /// The setup time of each test file outside its tests, in milliseconds:
    /// module load and suite hooks such as `beforeAll` (section 8).
    #[serde(default)]
    pub file_setup_ms: BTreeMap<String, f64>,
    /// The ids of tests that share a full name with another test of the same
    /// file, in sorted order. The companion keeps the first id as it is and
    /// numbers the others (`name (2)`) in report order, so the evidence of
    /// these ids is not reliable. An older companion leaves it out.
    #[serde(default)]
    pub duplicate_test_ids: Vec<String>,
}

/// The body of a `record_result` response.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecordResult {
    #[serde(default)]
    pub recorded_args: Vec<RecordedFunction>,
    #[serde(default)]
    pub duration_ms: u64,
}

/// The arguments that a recording dry run gives for one exported function.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecordedFunction {
    /// `<root-relative file>#<export name>`.
    pub key: String,
    /// The distinct inputs, each an argument list in the replay encoding.
    #[serde(default)]
    pub inputs: Vec<Vec<serde_json::Value>>,
    /// The calls whose arguments cannot be replayed, by reason.
    #[serde(default)]
    pub not_replayable: Vec<RecordRefusal>,
}

/// The count of calls of one reason that a recording dry run did not keep.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecordRefusal {
    pub reason: String,
    pub count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Killed,
    Survived,
    Timeout,
    RuntimeError,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RunTest {
    pub test_id: String,
    pub status: TestStatus,
    /// How a failed test failed: `assertion`, `error` or `timeout`. An older
    /// companion leaves it out.
    #[serde(default)]
    pub failure_kind: Option<String>,
    /// The first error of a failed test: its message and the stack frames
    /// of the project, bounded in size. An older companion leaves it out.
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RunResult {
    /// The mutant of the request. `None` for a control run.
    #[serde(default)]
    pub mutant_id: Option<String>,
    pub status: RunStatus,
    #[serde(default)]
    pub killed_by: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<f64>,
    pub tests_executed: usize,
    #[serde(default)]
    pub tests: Vec<RunTest>,
    #[serde(default)]
    pub unhandled_errors: Vec<UnhandledError>,
    /// True when the loop budget of the request decided a `timeout` status:
    /// the first failing test still evaluated instrumented code after the
    /// budget.
    #[serde(default)]
    pub loop_timeout: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadMethod {
    Invalidate,
    Noop,
    RestartRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Reloaded {
    pub method: ReloadMethod,
}

/// The body of an `error` response.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RemoteError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
}

/// The side of one replay call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplaySide {
    Original,
    Mutant,
}

/// The outcome of one replay call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayOutcome {
    Returned,
    Threw,
    TimedOut,
    NotCallable,
}

/// One replay call record.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReplayCall {
    pub side: ReplaySide,
    pub index: u32,
    pub outcome: ReplayOutcome,
    /// The returned or thrown value in the replay encoding. `null` for a
    /// call that timed out.
    #[serde(default)]
    pub value: serde_json::Value,
    /// A short text of the value for people.
    #[serde(default)]
    pub text: String,
}

/// The body of a `replay_result` response.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReplayResult {
    pub calls: Vec<ReplayCall>,
    #[serde(default)]
    pub duration_ms: u64,
}

/// One decoded response.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    Ready(Ready),
    DryRun(DryRunResult),
    Run(RunResult),
    Reloaded(Reloaded),
    Replay(ReplayResult),
    Record(RecordResult),
}

/// Why a companion exchange failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkFault {
    /// The watchdog fired. The companion tree is killed.
    Timeout,
    /// The companion exited before its response.
    Exited { stderr_tail: String },
    /// A line broke a fail-closed rule. The companion is killed.
    Protocol(String),
    /// The companion answered with an `error` response.
    Remote(RemoteError),
    /// The companion could not start or its pipes failed.
    Io(String),
}

impl std::fmt::Display for LinkFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "the companion did not answer before the watchdog"),
            Self::Exited { .. } => write!(f, "the companion exited before its response"),
            Self::Protocol(message) => write!(f, "protocol violation: {message}"),
            Self::Remote(error) => write!(f, "{}: {}", error.code, error.message),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

impl LinkFault {
    /// The stderr tail of the fault, when the companion exited.
    pub fn stderr_tail(&self) -> Option<&str> {
        match self {
            Self::Exited { stderr_tail } if !stderr_tail.is_empty() => Some(stderr_tail),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct Header {
    v: serde_json::Value,
    id: serde_json::Value,
    #[serde(rename = "type")]
    kind: String,
}

/// Decodes one response line for the open request with `id`.
///
/// Fail-closed rules of `protocol.md` section 4: a line that is not JSON, has
/// another `v`, an unknown or unexpected `type`, or another `id` is a
/// [`LinkFault::Protocol`]. An `error` response with the open `id` or a null
/// `id` is a [`LinkFault::Remote`].
pub fn decode_response(
    line: &str,
    version: u32,
    id: u64,
    expected_type: &str,
) -> Result<Response, LinkFault> {
    let value: serde_json::Value = serde_json::from_str(line)
        .map_err(|error| LinkFault::Protocol(format!("a response line is not JSON: {error}")))?;
    let header = Header::deserialize(&value).map_err(|error| {
        LinkFault::Protocol(format!("a response line has no v, id or type: {error}"))
    })?;
    if header.v.as_u64() != Some(u64::from(version)) {
        return Err(LinkFault::Protocol(format!(
            "a response has protocol version {}, expected {version}",
            header.v
        )));
    }
    if header.kind == "error" {
        if !(header.id.is_null() || header.id.as_u64() == Some(id)) {
            return Err(LinkFault::Protocol(format!(
                "an error response has id {}, expected {id}",
                header.id
            )));
        }
        let error = value
            .get("error")
            .cloned()
            .and_then(|error| serde_json::from_value::<RemoteError>(error).ok())
            .ok_or_else(|| LinkFault::Protocol("an error response has no error body".to_owned()))?;
        return Err(LinkFault::Remote(error));
    }
    if header.id.as_u64() != Some(id) {
        return Err(LinkFault::Protocol(format!(
            "a response has id {}, expected {id}",
            header.id
        )));
    }
    if header.kind != expected_type {
        return Err(LinkFault::Protocol(format!(
            "a response has type `{}`, expected `{expected_type}`",
            header.kind
        )));
    }
    let body = |error: serde_json::Error| {
        LinkFault::Protocol(format!(
            "a `{expected_type}` response does not parse: {error}"
        ))
    };
    Ok(match expected_type {
        "ready" => Response::Ready(serde_json::from_value(value).map_err(body)?),
        "dry_run_result" => Response::DryRun(serde_json::from_value(value).map_err(body)?),
        "run_result" => Response::Run(serde_json::from_value(value).map_err(body)?),
        "reloaded" => Response::Reloaded(serde_json::from_value(value).map_err(body)?),
        "replay_result" => Response::Replay(serde_json::from_value(value).map_err(body)?),
        "record_result" => Response::Record(serde_json::from_value(value).map_err(body)?),
        other => {
            return Err(LinkFault::Protocol(format!(
                "no response type `{other}` exists"
            )));
        }
    })
}

#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
#[cfg(test)]
mod tests {
    use super::*;

    const RUN_RESULT: &str = r#"{"v":1,"id":3,"type":"run_result","mutant_id":"m1","status":"killed","killed_by":"t::a","duration_ms":41,"tests_executed":1,"tests":[{"test_id":"t::a","status":"fail","failure_kind":"assertion"}]}"#;

    fn protocol_error(result: Result<Response, LinkFault>) -> String {
        match result {
            Err(LinkFault::Protocol(message)) => message,
            other => panic!("expected a protocol fault, got {other:?}"),
        }
    }

    #[test]
    fn a_replay_result_decodes_with_each_outcome() {
        let line = r#"{"v":1,"id":4,"type":"replay_result","duration_ms":12,"calls":[{"side":"original","index":1,"outcome":"returned","value":5,"text":"5"},{"side":"mutant","index":1,"outcome":"timed_out","value":null,"text":"timed out"}]}"#;
        let Response::Replay(result) = decode_response(line, 1, 4, "replay_result").unwrap() else {
            panic!("expected a replay result");
        };
        assert_eq!(result.calls.len(), 2);
        assert_eq!(result.calls[0].side, ReplaySide::Original);
        assert_eq!(result.calls[0].value, serde_json::json!(5));
        assert_eq!(result.calls[1].outcome, ReplayOutcome::TimedOut);
    }

    #[test]
    fn a_replay_request_line_has_every_field() {
        let request = Request::Replay(ReplayRequest {
            file: "src/a.ts".to_owned(),
            export: "add".to_owned(),
            mutant_id: "m1".to_owned(),
            args: vec![serde_json::json!(1)],
            calls: 3,
            call_timeout_ms: 2000,
            clock_ms: 1,
            seed: 2,
        });
        let line: serde_json::Value = serde_json::from_str(&request.to_line(1, 9)).unwrap();
        assert_eq!(line["type"], "replay");
        assert_eq!(line["export"], "add");
        assert_eq!(line["args"], serde_json::json!([1]));
        assert_eq!(request.response_type(), Some("replay_result"));
    }

    #[test]
    fn a_matching_run_result_decodes() {
        let Response::Run(result) = decode_response(RUN_RESULT, 1, 3, "run_result").unwrap() else {
            panic!("expected a run result");
        };
        assert_eq!(result.mutant_id.as_deref(), Some("m1"));
        assert_eq!(result.status, RunStatus::Killed);
        assert_eq!(result.killed_by.as_deref(), Some("t::a"));
        assert_eq!(result.tests_executed, 1);
        assert_eq!(result.tests[0].status, TestStatus::Fail);
        assert_eq!(result.tests[0].failure_kind.as_deref(), Some("assertion"));
        assert_eq!(
            result.tests[0].message, None,
            "an older companion sends no message"
        );
    }

    #[test]
    fn a_dry_run_result_keeps_the_error_of_a_failed_test_and_accepts_an_older_companion() {
        let line = r#"{"v":1,"id":2,"type":"dry_run_result","tests":[{"test_id":"t::a","file":"t","status":"fail","failure_kind":"assertion","message":"AssertionError: expected 1 to be 2\n    at t:3:5"},{"test_id":"t::b","file":"t","status":"fail"}]}"#;
        let Response::DryRun(result) = decode_response(line, 1, 2, "dry_run_result").unwrap()
        else {
            panic!("expected a dry run result");
        };
        assert_eq!(result.tests[0].failure_kind.as_deref(), Some("assertion"));
        assert_eq!(
            result.tests[0].message.as_deref(),
            Some("AssertionError: expected 1 to be 2\n    at t:3:5")
        );
        assert_eq!(
            (
                result.tests[1].failure_kind.as_deref(),
                result.tests[1].message.as_deref()
            ),
            (None, None),
            "an older companion sends no failure fields"
        );
    }

    #[test]
    fn a_run_result_keeps_the_error_of_a_failed_test() {
        let line = RUN_RESULT.replace(
            r#""failure_kind":"assertion""#,
            r#""failure_kind":"assertion","message":"AssertionError: expected 1 to be 2""#,
        );
        let Response::Run(result) = decode_response(&line, 1, 3, "run_result").unwrap() else {
            panic!("expected a run result");
        };
        assert_eq!(
            result.tests[0].message.as_deref(),
            Some("AssertionError: expected 1 to be 2")
        );
    }

    #[test]
    fn rule_2_a_line_that_is_not_json_is_a_protocol_fault() {
        let message = protocol_error(decode_response("vitest says hi", 1, 3, "run_result"));
        assert!(message.contains("not JSON"), "{message}");
    }

    #[test]
    fn rule_2_another_wire_version_is_a_protocol_fault() {
        let line = RUN_RESULT.replace(r#""v":1"#, r#""v":2"#);
        let message = protocol_error(decode_response(&line, 1, 3, "run_result"));
        assert!(message.contains("protocol version 2"), "{message}");
    }

    #[test]
    fn rule_2_an_unexpected_type_is_a_protocol_fault() {
        let line = RUN_RESULT.replace("run_result", "dry_run_result");
        let message = protocol_error(decode_response(&line, 1, 3, "run_result"));
        assert!(message.contains("type `dry_run_result`"), "{message}");
    }

    #[test]
    fn rule_3_another_id_is_a_protocol_fault() {
        let message = protocol_error(decode_response(RUN_RESULT, 1, 4, "run_result"));
        assert!(message.contains("expected 4"), "{message}");
    }

    #[test]
    fn an_unknown_run_status_is_a_protocol_fault_not_a_survivor() {
        let line = RUN_RESULT.replace(r#""status":"killed""#, r#""status":"maybe""#);
        let message = protocol_error(decode_response(&line, 1, 3, "run_result"));
        assert!(message.contains("does not parse"), "{message}");
    }

    #[test]
    fn an_error_response_is_a_remote_fault_for_the_open_id_or_a_null_id() {
        for id in ["3", "null"] {
            let line = format!(
                r#"{{"v":1,"id":{id},"type":"error","error":{{"code":"runner-load-failed","message":"cannot import vitest/node","retryable":false}}}}"#
            );
            assert_eq!(
                decode_response(&line, 1, 3, "ready"),
                Err(LinkFault::Remote(RemoteError {
                    code: "runner-load-failed".to_owned(),
                    message: "cannot import vitest/node".to_owned(),
                    retryable: false,
                }))
            );
        }
        let other = r#"{"v":1,"id":9,"type":"error","error":{"code":"internal","message":"x","retryable":true}}"#;
        assert!(matches!(
            decode_response(other, 1, 3, "ready"),
            Err(LinkFault::Protocol(_))
        ));
    }

    #[test]
    fn rule_4_a_capability_that_ready_does_not_state_is_false() {
        let line = r#"{"v":1,"id":1,"type":"ready","runner_version":"5.0.4","runtime_version":"v24.20.0","test_files":["a.test.ts"],"capabilities":{"per_test_attribution":true,"hot_switch":true}}"#;
        let Response::Ready(ready) = decode_response(line, 1, 1, "ready").unwrap() else {
            panic!("expected ready");
        };
        assert!(ready.capabilities.per_test_attribution);
        assert!(ready.capabilities.hot_switch);
        assert!(!ready.capabilities.bail);
        assert!(!ready.capabilities.fresh_modules_per_run);
        assert!(!ready.capabilities.test_filter);
        assert!(ready.notes.is_empty(), "notes default to none");

        let line = r#"{"v":1,"id":1,"type":"ready","runner_version":"5.0.4","runtime_version":"v24.20.0","notes":["typecheck-not-run"]}"#;
        let Response::Ready(ready) = decode_response(line, 1, 1, "ready").unwrap() else {
            panic!("expected ready");
        };
        assert_eq!(ready.notes, vec!["typecheck-not-run".to_owned()]);
    }

    #[test]
    fn a_request_line_carries_version_id_type_and_the_body() {
        let line = Request::Run(RunRequest {
            mutant_id: Some("m1".to_owned()),
            test_ids: vec!["t::a".to_owned()],
            timeout_ms: 6500,
            loop_budget_ms: Some(650),
            bail: true,
        })
        .to_line(1, 3);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap(),
            serde_json::json!({"v":1,"id":3,"type":"run","mutant_id":"m1","test_ids":["t::a"],"timeout_ms":6500,"loop_budget_ms":650,"bail":true})
        );
        let control = Request::Run(RunRequest {
            mutant_id: None,
            test_ids: vec!["t::a".to_owned()],
            timeout_ms: 6500,
            loop_budget_ms: None,
            bail: true,
        })
        .to_line(1, 4);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&control).unwrap(),
            serde_json::json!({"v":1,"id":4,"type":"run","mutant_id":null,"test_ids":["t::a"],"timeout_ms":6500,"bail":true})
        );
        let shutdown = Request::Shutdown.to_line(1, 5);
        assert!(!shutdown.contains('\n'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&shutdown).unwrap(),
            serde_json::json!({"v":1,"id":5,"type":"shutdown"})
        );
    }
}
