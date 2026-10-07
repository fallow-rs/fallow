#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Cloud read transport and the scoped cloud read commands, against a mock
//! HTTP server: gzip bodies, one retry on a gateway error, the agent-source
//! header, `repo_path` matching, `coverage review-packet` and
//! `coverage deployment-changes`.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use flate2::Compression;
use flate2::write::GzEncoder;

use crate::common::{CommandOutput, fallow_bin, fixture_path, parse_json};
use crate::http_stub::read_request;

/// One scripted mock response.
struct MockResponse {
    status: u16,
    gzip: bool,
    body: String,
}

impl MockResponse {
    fn json(body: &str) -> Self {
        Self {
            status: 200,
            gzip: false,
            body: body.to_owned(),
        }
    }

    fn gzip(body: &str) -> Self {
        Self {
            status: 200,
            gzip: true,
            body: body.to_owned(),
        }
    }

    fn status(status: u16) -> Self {
        Self {
            status,
            gzip: false,
            body: r#"{"error":true,"message":"bad gateway","code":"bad_gateway"}"#.to_owned(),
        }
    }
}

type Captured = Arc<Mutex<Vec<String>>>;

/// Serve the scripted responses in order, one connection each, and capture
/// every request (head and body).
fn serve(responses: Vec<MockResponse>) -> (String, Captured, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("mock addr");
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let handle = thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().expect("accept request");
            let request = read_request(&mut stream);
            sink.lock().expect("capture lock").push(request);
            write_response(&mut stream, &response);
        }
    });
    (format!("http://{addr}"), captured, handle)
}

fn write_response(stream: &mut TcpStream, response: &MockResponse) {
    let (body, encoding) = if response.gzip {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(response.body.as_bytes())
            .expect("gzip body");
        (
            encoder.finish().expect("finish gzip"),
            "content-encoding: gzip\r\n",
        )
    } else {
        (response.body.clone().into_bytes(), "")
    };
    let head = format!(
        "HTTP/1.1 {} X\r\ncontent-type: application/json\r\n{encoding}content-length: {}\r\nconnection: close\r\n\r\n",
        response.status,
        body.len()
    );
    stream.write_all(head.as_bytes()).expect("write head");
    stream.write_all(&body).expect("write body");
}

fn run(args: &[&str], root: &Path, agent_source: Option<&str>) -> CommandOutput {
    let mut command = Command::new(fallow_bin());
    command
        .args(args)
        .arg("--root")
        .arg(root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_API_KEY", "fallow_live_test")
        .env_remove("FALLOW_RUNTIME_COVERAGE_SOURCE")
        .env_remove("FALLOW_API_URL");
    match agent_source {
        Some(value) => command.env("FALLOW_AGENT_SOURCE", value),
        None => command.env("FALLOW_AGENT_SOURCE", "none"),
    };
    let output = command.output().expect("run fallow");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn runtime_context_body(file_path: &str, repo_path: Option<&str>) -> String {
    let repo_path = repo_path.map_or_else(String::new, |path| format!(r#""repo_path": "{path}","#));
    format!(
        r#"{{
      "data": {{
        "repo": "acme/web",
        "window": {{ "period_days": 30 }},
        "evidence_window": {{
          "first_observed_at": "2026-04-29T10:00:00.000Z",
          "last_observed_at": "2026-04-30T10:00:00.000Z",
          "observed_hours": 24,
          "deployments_in_period": 3
        }},
        "summary": {{
          "trace_count": 100,
          "deployments_seen": 2,
          "functions_tracked": 1,
          "functions_hit": 0,
          "functions_unhit": 1,
          "functions_untracked": 0,
          "coverage_percent": 0,
          "last_received_at": "2026-04-30T10:00:00.000Z"
        }},
        "functions": [{{
          "file_path": "{file_path}",
          {repo_path}
          "function_name": "covered",
          "line_number": 1,
          "start_line": 1,
          "end_line": 3,
          "hit_count": 0,
          "tracking_state": "never_called",
          "period_tracking_state": "never_called",
          "never_called_source": "runtime_observed",
          "deployments_observed": 2
        }}],
        "warnings": []
      }}
    }}"#
    )
}

fn analyze_args(endpoint: &str) -> Vec<String> {
    [
        "coverage",
        "analyze",
        "--cloud",
        "--repo",
        "acme/web",
        "--api-endpoint",
        endpoint,
        "--format",
        "json",
    ]
    .iter()
    .map(|value| (*value).to_owned())
    .collect()
}

fn run_analyze(endpoint: &str, agent_source: Option<&str>) -> CommandOutput {
    let args = analyze_args(endpoint);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&refs, &fixture_path("coverage-gaps"), agent_source)
}

fn first_finding_function(output: &CommandOutput) -> Option<String> {
    parse_json(output)
        .pointer("/runtime_coverage/findings/0/function")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

#[test]
fn analyze_cloud_decodes_a_gzip_body() {
    let body = runtime_context_body("src/covered.ts", None);
    let (endpoint, captured, handle) = serve(vec![MockResponse::gzip(&body)]);
    let output = run_analyze(&endpoint, None);
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    assert_eq!(first_finding_function(&output).as_deref(), Some("covered"));
    let request = captured.lock().expect("lock")[0].to_lowercase();
    assert!(
        request.contains("accept-encoding: gzip"),
        "request did not ask for gzip: {request}"
    );
}

#[test]
fn analyze_cloud_retries_once_on_502() {
    let body = runtime_context_body("src/covered.ts", None);
    let (endpoint, captured, handle) =
        serve(vec![MockResponse::status(502), MockResponse::json(&body)]);
    let output = run_analyze(&endpoint, None);
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    assert_eq!(first_finding_function(&output).as_deref(), Some("covered"));
    assert_eq!(captured.lock().expect("lock").len(), 2);
}

#[test]
fn analyze_cloud_names_an_outage_after_two_gateway_errors() {
    let (endpoint, captured, handle) =
        serve(vec![MockResponse::status(503), MockResponse::status(503)]);
    let output = run_analyze(&endpoint, None);
    handle.join().expect("server joins");
    assert_eq!(
        output.code, 7,
        "stdout={} stderr={}",
        output.stdout, output.stderr
    );
    let text = format!("{}{}", output.stdout, output.stderr);
    assert!(text.contains("cloud outage"), "output: {text}");
    assert_eq!(captured.lock().expect("lock").len(), 2);
}

#[test]
fn analyze_cloud_names_an_unreachable_network() {
    // Bind and drop a listener, so the port refuses connections.
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let output = run_analyze(&format!("http://127.0.0.1:{port}"), None);
    assert_eq!(
        output.code, 7,
        "stdout={} stderr={}",
        output.stdout, output.stderr
    );
    let text = format!("{}{}", output.stdout, output.stderr);
    assert!(text.contains("network unreachable"), "output: {text}");
}

#[test]
fn analyze_cloud_sends_the_allowlisted_agent_source() {
    let body = runtime_context_body("src/covered.ts", None);
    let (endpoint, captured, handle) = serve(vec![MockResponse::json(&body)]);
    let output = run_analyze(&endpoint, Some("claude-code"));
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    let request = captured.lock().expect("lock")[0].to_lowercase();
    assert!(
        request.contains("x-fallow-agent-source: claude_code"),
        "request: {request}"
    );
}

#[test]
fn analyze_cloud_omits_the_agent_source_without_an_agent_or_off_the_allowlist() {
    for source in [None, Some("copilot")] {
        let body = runtime_context_body("src/covered.ts", None);
        let (endpoint, captured, handle) = serve(vec![MockResponse::json(&body)]);
        let output = run_analyze(&endpoint, source);
        handle.join().expect("server joins");
        assert_eq!(output.code, 0, "stderr={}", output.stderr);
        let request = captured.lock().expect("lock")[0].to_lowercase();
        assert!(
            !request.contains("x-fallow-agent-source"),
            "source {source:?} sent the header: {request}"
        );
    }
}

#[test]
fn analyze_cloud_matches_on_repo_path_before_the_suffix_fallback() {
    // The runtime path shares no directory with the checkout, so the suffix
    // fallback cannot place it. The proven repo path can.
    let body = runtime_context_body("/app/build/server/covered.ts", Some("src/covered.ts"));
    let (endpoint, _captured, handle) = serve(vec![MockResponse::json(&body)]);
    let output = run_analyze(&endpoint, None);
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    assert_eq!(first_finding_function(&output).as_deref(), Some("covered"));
}

#[test]
fn analyze_cloud_without_repo_path_keeps_the_suffix_fallback() {
    let body = runtime_context_body("/app/src/covered.ts", None);
    let (endpoint, _captured, handle) = serve(vec![MockResponse::json(&body)]);
    let output = run_analyze(&endpoint, None);
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    assert_eq!(first_finding_function(&output).as_deref(), Some("covered"));
}

const REVIEW_PACKET_BODY: &str = r#"{"data":{"schema_version":"fallow-review-packet-v1","repo":"acme/web","actionable":true,"functions":[{"file_path":"/app/src/a.ts","repo_path":"src/a.ts","function_name":"a","hit_count":12}]}}"#;

#[test]
fn review_packet_posts_explicit_files_and_functions() {
    let (endpoint, captured, handle) = serve(vec![MockResponse::gzip(REVIEW_PACKET_BODY)]);
    let output = run(
        &[
            "coverage",
            "review-packet",
            "--repo",
            "acme/web",
            "--api-endpoint",
            &endpoint,
            "--file",
            "src/a.ts",
            "--function",
            "src/b.ts:handler:12",
            "--function",
            "src/c.ts:render",
            "--coverage-period",
            "7",
            "--project-id",
            "web",
            "--commit-sha",
            "abc1234",
        ],
        &fixture_path("coverage-gaps"),
        Some("codex"),
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(
        json.pointer("/data/functions/0/repo_path")
            .and_then(serde_json::Value::as_str),
        Some("src/a.ts")
    );
    let request = captured.lock().expect("lock")[0].clone();
    assert!(
        request.starts_with("POST /v1/coverage/acme%2Fweb/review-packet "),
        "request: {request}"
    );
    assert!(
        request
            .to_lowercase()
            .contains("x-fallow-agent-source: codex")
    );
    let body = &request[request.find("\r\n\r\n").expect("body") + 4..];
    let sent: serde_json::Value = serde_json::from_str(body).expect("json body");
    assert_eq!(sent["files"], serde_json::json!(["src/a.ts"]));
    assert_eq!(
        sent["functions"],
        serde_json::json!([
            {"file": "src/b.ts", "name": "handler", "line": 12},
            {"file": "src/c.ts", "name": "render"}
        ])
    );
    assert_eq!(sent["periodDays"], 7);
    assert_eq!(sent["projectId"], "web");
    assert_eq!(sent["gitSha"], "abc1234");
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn review_packet_defaults_to_files_changed_against_the_base() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    std::fs::write(root.join("package.json"), r#"{"name":"demo"}"#).expect("write");
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/a.ts"), "export const a = 1;\n").expect("write");
    std::fs::write(root.join("src/b.ts"), "export const b = 1;\n").expect("write");
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["-c", "commit.gpgsign=false", "add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"],
    );
    std::fs::write(root.join("src/b.ts"), "export const b = 2;\n").expect("write");
    std::fs::write(root.join("README.md"), "docs\n").expect("write");

    let (endpoint, captured, handle) = serve(vec![MockResponse::json(REVIEW_PACKET_BODY)]);
    let output = run(
        &[
            "coverage",
            "review-packet",
            "--repo",
            "acme/web",
            "--api-endpoint",
            &endpoint,
            "--base",
            "HEAD",
        ],
        root,
        None,
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    let request = captured.lock().expect("lock")[0].clone();
    let body = &request[request.find("\r\n\r\n").expect("body") + 4..];
    let sent: serde_json::Value = serde_json::from_str(body).expect("json body");
    assert_eq!(sent["files"], serde_json::json!(["src/b.ts"]));
    assert!(sent.get("functions").is_none(), "body: {sent}");
}

#[test]
fn review_packet_without_changed_files_refuses_before_the_network() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    std::fs::write(root.join("package.json"), r#"{"name":"demo"}"#).expect("write");
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["-c", "commit.gpgsign=false", "add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"],
    );
    let output = run(
        &[
            "coverage",
            "review-packet",
            "--repo",
            "acme/web",
            "--api-endpoint",
            "http://127.0.0.1:9",
            "--base",
            "HEAD",
        ],
        root,
        None,
    );
    assert_eq!(
        output.code, 2,
        "stdout={} stderr={}",
        output.stdout, output.stderr
    );
    let text = format!("{}{}", output.stdout, output.stderr);
    assert!(text.contains("no changed source files"), "output: {text}");
}

#[test]
fn deployment_changes_gets_the_change_report_with_a_base() {
    let body = r#"{"comparable":true,"head":{"sha":"abc1234"},"functions":[{"change":"stopped","function_name":"a"}],"meta":{"cursor":null,"hasMore":false,"totalCount":1}}"#;
    let (endpoint, captured, handle) = serve(vec![MockResponse::gzip(body)]);
    let output = run(
        &[
            "coverage",
            "deployment-changes",
            "--repo",
            "acme/web",
            "--api-endpoint",
            &endpoint,
            "--sha",
            "abc1234",
            "--base",
            "def5678",
            "--change",
            "stopped",
            "--limit",
            "50",
        ],
        &fixture_path("coverage-gaps"),
        None,
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["comparable"], true);
    assert_eq!(json["functions"][0]["change"], "stopped");
    let request = captured.lock().expect("lock")[0].clone();
    let first_line = request.lines().next().expect("request line");
    assert!(
        first_line.starts_with("GET /v1/coverage/acme%2Fweb/deployments/abc1234/changes?"),
        "request: {first_line}"
    );
    assert!(first_line.contains("base=def5678"), "request: {first_line}");
    assert!(
        first_line.contains("change=stopped"),
        "request: {first_line}"
    );
    assert!(first_line.contains("limit=50"), "request: {first_line}");
}

#[test]
fn deployment_changes_reports_a_missing_deployment_as_not_found() {
    let (endpoint, _captured, handle) = serve(vec![MockResponse {
        status: 404,
        gzip: false,
        body: r#"{"error":true,"message":"No deployment or production runtime found for this commit","code":"not_found"}"#.to_owned(),
    }]);
    let output = run(
        &[
            "coverage",
            "deployment-changes",
            "--repo",
            "acme/web",
            "--api-endpoint",
            &endpoint,
            "--sha",
            "abc1234",
        ],
        &fixture_path("coverage-gaps"),
        None,
    );
    handle.join().expect("server joins");
    assert_eq!(
        output.code, 3,
        "stdout={} stderr={}",
        output.stdout, output.stderr
    );
    let text = format!("{}{}", output.stdout, output.stderr);
    assert!(
        text.contains("No deployment or production runtime found"),
        "output: {text}"
    );
}

/// Run a cloud read without an API key and return the parsed JSON refusal.
fn run_without_api_key(args: &[&str]) -> serde_json::Value {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    std::fs::write(root.join("package.json"), r#"{"name":"demo"}"#).expect("write");
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["-c", "commit.gpgsign=false", "add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"],
    );
    let output = Command::new(fallow_bin())
        .args(args)
        .args([
            "--api-endpoint",
            "http://127.0.0.1:9",
            "--format",
            "json",
            "--quiet",
        ])
        .arg("--root")
        .arg(root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env_remove("FALLOW_API_KEY")
        .env_remove("FALLOW_API_URL")
        .output()
        .expect("run fallow");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(3), "stdout={stdout}");
    serde_json::from_str(&stdout).expect("refusal is JSON")
}

#[test]
fn review_packet_without_api_key_names_review_packet_in_the_hint() {
    let body = run_without_api_key(&[
        "coverage",
        "review-packet",
        "--repo",
        "o/r",
        "--file",
        "a.ts",
    ]);
    assert_eq!(body["error"], true);
    assert_eq!(body["exit_code"], 3);
    assert_eq!(
        body["message"],
        fallow_types::cloud::cloud_api_key_missing_message(
            fallow_types::cloud::CloudCommand::ReviewPacket
        )
    );
    let message = body["message"].as_str().expect("message is a string");
    assert!(message.contains("fallow coverage review-packet --repo owner/repo"));
    assert!(!message.contains("coverage analyze"), "message: {message}");
}

#[test]
fn deployment_changes_without_api_key_names_deployment_changes_in_the_hint() {
    let body = run_without_api_key(&["coverage", "deployment-changes", "--repo", "o/r"]);
    assert_eq!(body["error"], true);
    assert_eq!(body["exit_code"], 3);
    let message = body["message"].as_str().expect("message is a string");
    assert!(message.contains("fallow coverage deployment-changes --repo owner/repo"));
    assert!(!message.contains("coverage analyze"), "message: {message}");
}
