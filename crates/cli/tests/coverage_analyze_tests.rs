#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use common::{fallow_bin, fixture_path, parse_json};

fn serve_once(body: &'static str) -> (String, Arc<Mutex<String>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("mock addr");
    let request = Arc::new(Mutex::new(String::new()));
    let captured = Arc::clone(&request);
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut buf = [0_u8; 4096];
        let read = stream.read(&mut buf).expect("read request");
        *captured.lock().expect("capture lock") =
            String::from_utf8_lossy(&buf[..read]).into_owned();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    });
    (format!("http://{addr}"), request, handle)
}

fn cloud_body() -> &'static str {
    r#"{
      "data": {
        "repo": "acme/web",
        "window": { "period_days": 30 },
        "summary": {
          "trace_count": 100,
          "deployments_seen": 2,
          "functions_tracked": 1,
          "functions_hit": 0,
          "functions_unhit": 1,
          "functions_untracked": 0,
          "coverage_percent": 0,
          "last_received_at": "2026-04-30T10:00:00.000Z"
        },
        "functions": [{
          "file_path": "src/covered.ts",
          "function_name": "covered",
          "line_number": 1,
          "start_line": 1,
          "end_line": 3,
          "hit_count": 0,
          "tracking_state": "never_called",
          "never_called_source": "runtime_observed",
          "deployments_observed": 2
        }],
        "warnings": []
      }
    }"#
}

#[test]
fn coverage_analyze_cloud_fetches_percent_encoded_runtime_context() {
    let (endpoint, request, handle) = serve_once(cloud_body());
    let output = std::process::Command::new(fallow_bin())
        .args([
            "coverage",
            "analyze",
            "--cloud",
            "--repo",
            "acme/web",
            "--api-endpoint",
            &endpoint,
            "--format",
            "json",
            "--root",
        ])
        .arg(fixture_path("coverage-gaps"))
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_API_KEY", "fallow_live_test")
        .env_remove("FALLOW_RUNTIME_COVERAGE_SOURCE")
        .output()
        .expect("run fallow");
    handle.join().expect("server joins");
    let command_output = common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    };
    assert_eq!(command_output.code, 0, "stderr={}", command_output.stderr);
    let json = parse_json(&command_output);
    assert_eq!(
        json.pointer("/runtime_coverage/schema_version")
            .and_then(serde_json::Value::as_str),
        Some("1")
    );
    assert_eq!(
        json.pointer("/runtime_coverage/findings/0/function")
            .and_then(serde_json::Value::as_str),
        Some("covered")
    );
    assert_eq!(
        json.pointer("/runtime_coverage/findings/0/evidence/test_coverage")
            .and_then(serde_json::Value::as_str),
        Some("not_covered")
    );
    assert_eq!(
        json.pointer("/runtime_coverage/findings/0/evidence/v8_tracking")
            .and_then(serde_json::Value::as_str),
        Some("tracked")
    );
    assert_eq!(
        json.pointer("/runtime_coverage/findings/0/confidence")
            .and_then(serde_json::Value::as_str),
        Some("high")
    );
    assert_eq!(
        json.pointer("/runtime_coverage/summary/data_source")
            .and_then(serde_json::Value::as_str),
        Some("cloud")
    );
    assert_eq!(
        json.pointer("/runtime_coverage/summary/last_received_at")
            .and_then(serde_json::Value::as_str),
        Some("2026-04-30T10:00:00.000Z")
    );
    assert_eq!(
        json.pointer("/runtime_coverage/summary/capture_quality/instances_observed")
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );
    let request = request.lock().expect("request lock").clone();
    assert!(
        request.starts_with("GET /v1/coverage/acme%2Fweb/runtime-context?"),
        "request path was not percent-encoded: {request}"
    );
    let lower = request.to_lowercase();
    assert!(
        lower.contains("accept-encoding: identity"),
        "request did not negotiate identity encoding: {request}"
    );
    assert!(
        !lower.contains("accept-encoding: gzip"),
        "request must not advertise gzip without ureq's gzip feature: {request}"
    );
}

#[test]
fn coverage_analyze_cloud_keeps_inventory_backfill_candidates_conservative() {
    let body = r#"{
      "data": {
        "repo": "acme/web",
        "window": { "period_days": 30 },
        "summary": {
          "trace_count": 100,
          "deployments_seen": 2,
          "functions_tracked": 1,
          "functions_hit": 0,
          "functions_unhit": 1,
          "functions_untracked": 0,
          "coverage_percent": 0
        },
        "functions": [{
          "file_path": "src/covered.ts",
          "function_name": "indirectlyCovered",
          "line_number": 5,
          "start_line": 5,
          "end_line": 7,
          "hit_count": 0,
          "tracking_state": "never_called",
          "never_called_source": "inventory_backfill",
          "deployments_observed": 2
        }],
        "warnings": []
      }
    }"#;
    let (endpoint, _request, handle) = serve_once(body);
    let output = std::process::Command::new(fallow_bin())
        .args([
            "coverage",
            "analyze",
            "--cloud",
            "--repo",
            "acme/web",
            "--api-endpoint",
            &endpoint,
            "--format",
            "json",
            "--root",
        ])
        .arg(fixture_path("coverage-gaps"))
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_API_KEY", "fallow_live_test")
        .env_remove("FALLOW_RUNTIME_COVERAGE_SOURCE")
        .output()
        .expect("run fallow");
    handle.join().expect("server joins");
    let command_output = common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    };
    assert_eq!(command_output.code, 0, "stderr={}", command_output.stderr);
    let json = parse_json(&command_output);
    assert_eq!(
        json.pointer("/runtime_coverage/findings/0/function")
            .and_then(serde_json::Value::as_str),
        Some("indirectlyCovered")
    );
    assert_eq!(
        json.pointer("/runtime_coverage/findings/0/verdict")
            .and_then(serde_json::Value::as_str),
        Some("review_required")
    );
    assert_eq!(
        json.pointer("/runtime_coverage/findings/0/confidence")
            .and_then(serde_json::Value::as_str),
        Some("low")
    );
    assert_eq!(
        json.pointer("/runtime_coverage/findings/0/actions/0/type")
            .and_then(serde_json::Value::as_str),
        Some("review-runtime")
    );
}

#[test]
fn coverage_analyze_env_opt_in_enables_cloud() {
    let (endpoint, _request, handle) = serve_once(cloud_body());
    let output = std::process::Command::new(fallow_bin())
        .args([
            "coverage",
            "analyze",
            "--repo",
            "acme/web",
            "--api-endpoint",
            &endpoint,
            "--format",
            "json",
            "--root",
        ])
        .arg(fixture_path("coverage-gaps"))
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_API_KEY", "fallow_live_test")
        .env("FALLOW_RUNTIME_COVERAGE_SOURCE", "cloud")
        .output()
        .expect("run fallow");
    handle.join().expect("server joins");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn coverage_analyze_explain_attaches_meta_block() {
    let (endpoint, _request, handle) = serve_once(cloud_body());
    let output = std::process::Command::new(fallow_bin())
        .args([
            "--explain",
            "coverage",
            "analyze",
            "--cloud",
            "--repo",
            "acme/web",
            "--api-endpoint",
            &endpoint,
            "--format",
            "json",
            "--root",
        ])
        .arg(fixture_path("coverage-gaps"))
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_API_KEY", "fallow_live_test")
        .env_remove("FALLOW_RUNTIME_COVERAGE_SOURCE")
        .output()
        .expect("run fallow");
    handle.join().expect("server joins");
    let command_output = common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    };
    assert_eq!(command_output.code, 0, "stderr={}", command_output.stderr);
    let json = parse_json(&command_output);
    assert_eq!(
        json.pointer("/_meta/enums/data_source/0")
            .and_then(serde_json::Value::as_str),
        Some("local")
    );
    assert_eq!(
        json.pointer("/_meta/enums/action_type/0")
            .and_then(serde_json::Value::as_str),
        Some("delete-cold-code")
    );
    assert!(
        json.pointer("/_meta/warnings/cloud_functions_unmatched")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "expected _meta.warnings.cloud_functions_unmatched to be present"
    );
}
