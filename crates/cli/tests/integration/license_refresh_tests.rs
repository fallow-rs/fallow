#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! End-to-end coverage for the `fallow license refresh` credential fallback.
//!
//! A paying user who has not run the CLI for weeks holds a license JWT the
//! cloud refuses as `token_stale`, and the trial endpoint rejects an
//! organisation that already pays. The refresh endpoint accepts a full-access
//! API key as an equivalent bearer, so the CLI retries with one before giving
//! up. These tests drive the real binary against a stub of that endpoint and
//! assert the request sequence and the terminal error text.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::thread;

use crate::common::fallow_bin;
use crate::http_stub::{accept_before_deadline, read_request};

struct MockResponse {
    status: u16,
    body: &'static str,
}

/// Serve exactly `responses.len()` requests, returning the raw request text of
/// each in order.
fn serve(responses: Vec<MockResponse>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for response in responses {
            let Some(mut stream) = accept_before_deadline(&listener) else {
                break;
            };
            requests.push(read_request(&mut stream));
            write_response(&mut stream, response.status, response.body);
        }
        requests
    });
    (url, handle)
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = if status == 200 { "OK" } else { "Unauthorized" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .expect("write response");
}

fn refresh_command(home: &std::path::Path) -> Command {
    let mut command = Command::new(fallow_bin());
    command
        .args(["license", "refresh"])
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_UPDATE_CHECK", "off")
        .env("FALLOW_TELEMETRY", "off")
        .env_remove("FALLOW_LICENSE")
        .env_remove("FALLOW_LICENSE_PATH")
        .env_remove("FALLOW_API_KEY");
    command
}

fn authorization(request: &str) -> String {
    request
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization")
                .then(|| value.trim().to_owned())
        })
        .unwrap_or_else(|| panic!("no authorization header in:\n{request}"))
}

#[test]
fn refresh_retries_with_the_api_key_when_the_stored_jwt_is_stale() {
    let home = tempfile::tempdir().expect("temp home");
    let (endpoint, server) = serve(vec![
        MockResponse {
            status: 401,
            body: r#"{"error":true,"message":"token stale","code":"token_stale"}"#,
        },
        MockResponse {
            status: 200,
            body: r#"{"jwt":"header.payload.signature"}"#,
        },
    ]);

    let output = refresh_command(home.path())
        .env("FALLOW_API_URL", &endpoint)
        .env("FALLOW_LICENSE", "stale.stored.jwt")
        .env("FALLOW_API_KEY", "fallow_live_test")
        .output()
        .expect("run fallow license refresh");
    let requests = server.join().expect("server joins");

    assert_eq!(requests.len(), 2, "expected a retry, got: {requests:?}");
    assert_eq!(authorization(&requests[0]), "Bearer stale.stored.jwt");
    assert_eq!(authorization(&requests[1]), "Bearer fallow_live_test");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("fallow_live_test"),
        "the API key must never be echoed, got: {stderr}"
    );
}

#[test]
fn refresh_without_a_license_or_an_api_key_names_the_api_key_route() {
    let home = tempfile::tempdir().expect("temp home");

    let output = refresh_command(home.path())
        .env("FALLOW_API_URL", "http://127.0.0.1:1")
        .output()
        .expect("run fallow license refresh");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("FALLOW_API_KEY"),
        "expected the API-key recovery route, got: {stderr}"
    );
    assert!(
        stderr.contains("fallow license refresh"),
        "expected the command to retry, got: {stderr}"
    );
    assert!(
        !stderr.contains("--trial"),
        "the trial flow does not recover a paid license, got: {stderr}"
    );
}
