mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use common::{fallow_bin, parse_json};

#[derive(Clone)]
struct MockResponse {
    method: &'static str,
    path_contains: &'static str,
    status: u16,
    body: &'static str,
}

fn serve(responses: Vec<MockResponse>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let handle = {
        let requests = Arc::clone(&requests);
        thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept request");
                let request = read_request(&mut stream);
                assert!(
                    request.starts_with(response.method),
                    "expected {} request, got:\n{request}",
                    response.method
                );
                assert!(
                    request.contains(response.path_contains),
                    "expected path containing {}, got:\n{request}",
                    response.path_contains
                );
                requests.lock().expect("request lock").push(request);
                write_response(&mut stream, response.status, response.body);
            }
            Arc::try_unwrap(requests)
                .expect("request refs released")
                .into_inner()
                .expect("request lock")
        })
    };
    (url, handle)
}

fn read_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("set read timeout");
    let mut buffer = [0_u8; 8192];
    let len = stream.read(&mut buffer).expect("read request");
    String::from_utf8_lossy(&buffer[..len]).to_string()
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Status",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .expect("write response");
}

fn write_envelope(fingerprints: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let comments = fingerprints
        .iter()
        .map(|fingerprint| serde_json::json!({ "fingerprint": fingerprint }))
        .collect::<Vec<_>>();
    let envelope = serde_json::json!({ "comments": comments });
    std::fs::write(
        dir.path().join("review.json"),
        serde_json::to_vec(&envelope).expect("serialize envelope"),
    )
    .expect("write envelope");
    dir
}

fn run_reconcile(
    provider_args: &[&str],
    api_url: &str,
    envelope_dir: &tempfile::TempDir,
) -> common::CommandOutput {
    let output = Command::new(fallow_bin())
        .args(["--format", "json", "--quiet", "ci", "reconcile-review"])
        .args(provider_args)
        .args(["--api-url", api_url])
        .arg("--envelope")
        .arg(envelope_dir.path().join("review.json"))
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_API_RETRIES", "1")
        .env("FALLOW_API_RETRY_DELAY", "0")
        .env("GH_TOKEN", "test-token")
        .env("GITHUB_SHA", "abcdef1234567890")
        .env("GITLAB_TOKEN", "test-token")
        .env("CI_COMMIT_SHA", "abcdef1234567890")
        .output()
        .expect("run fallow");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn github_comments(body: &'static str) -> MockResponse {
    MockResponse {
        method: "GET",
        path_contains: "/repos/owner/repo/pulls/7/comments?per_page=100&page=1",
        status: 200,
        body,
    }
}

fn github_threads_empty() -> MockResponse {
    MockResponse {
        method: "POST",
        path_contains: "/graphql",
        status: 200,
        body: r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
    }
}

fn github_threads_with_old() -> MockResponse {
    MockResponse {
        method: "POST",
        path_contains: "/graphql",
        status: 200,
        body: r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":false,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->"}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
    }
}

#[test]
fn github_deletion_race_stops_before_mutation_and_reports_unapplied_fingerprint() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":101,"body":"finding\n<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#,
        ),
        github_threads_empty(),
        MockResponse {
            method: "GET",
            path_contains: "/repos/owner/repo/pulls/comments/101",
            status: 404,
            body: r#"{"message":"Not Found"}"#,
        },
    ]);

    let output = run_reconcile(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &envelope,
    );
    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["threads_resolved"], 0);
    assert_eq!(json["failed_fingerprints"], serde_json::json!(["old"]));
    assert_eq!(json["unapplied_fingerprints"], serde_json::json!(["old"]));
    assert!(
        json["apply_errors"][0]
            .as_str()
            .unwrap()
            .contains("preflight failed")
    );
    assert!(json["apply_hint"].as_str().unwrap().contains("rerun"));
    let requests = server.join().expect("server thread");
    assert_eq!(requests.len(), 3);
    assert!(
        !requests
            .iter()
            .any(|request| request
                .starts_with("POST /repos/owner/repo/pulls/7/comments/101/replies"))
    );
}

#[test]
fn github_mutation_failure_is_fail_fast_and_counts_only_completed_writes() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: a -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":2,"body":"<!-- fallow-fingerprint: b -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#,
        ),
        github_threads_empty(),
        MockResponse {
            method: "GET",
            path_contains: "/repos/owner/repo/pulls/comments/1",
            status: 200,
            body: r#"{"id":1}"#,
        },
        MockResponse {
            method: "GET",
            path_contains: "/repos/owner/repo/pulls/comments/2",
            status: 200,
            body: r#"{"id":2}"#,
        },
        MockResponse {
            method: "POST",
            path_contains: "/repos/owner/repo/pulls/7/comments/1/replies",
            status: 201,
            body: r#"{"id":11}"#,
        },
        MockResponse {
            method: "POST",
            path_contains: "/repos/owner/repo/pulls/7/comments/2/replies",
            status: 403,
            body: r#"{"message":"Forbidden"}"#,
        },
    ]);

    let output = run_reconcile(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &envelope,
    );
    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 1);
    assert_eq!(json["threads_resolved"], 0);
    assert_eq!(json["failed_fingerprints"], serde_json::json!(["b"]));
    assert_eq!(json["unapplied_fingerprints"], serde_json::json!(["b"]));
    assert!(
        json["apply_errors"][0]
            .as_str()
            .unwrap()
            .contains("HTTP 403")
    );
    let requests = server.join().expect("server thread");
    assert_eq!(requests.len(), 6);
}

#[test]
fn github_existing_sha_marker_skips_duplicate_resolution_reply() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":9,"body":"Resolved.\n\n<!-- fallow-resolved-fingerprint: old@abcdef1 -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#,
        ),
        github_threads_empty(),
    ]);

    let output = run_reconcile(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &envelope,
    );
    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["apply_errors"], serde_json::json!([]));
    let requests = server.join().expect("server thread");
    assert_eq!(requests.len(), 2);
}

#[test]
fn github_force_push_sha_marker_mismatch_posts_fresh_resolution_reply() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":9,"body":"Resolved.\n\n<!-- fallow-resolved-fingerprint: old@1111111 -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#,
        ),
        github_threads_empty(),
        MockResponse {
            method: "GET",
            path_contains: "/repos/owner/repo/pulls/comments/1",
            status: 200,
            body: r#"{"id":1}"#,
        },
        MockResponse {
            method: "POST",
            path_contains: "/repos/owner/repo/pulls/7/comments/1/replies",
            status: 201,
            body: r#"{"id":11}"#,
        },
    ]);

    let output = run_reconcile(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &envelope,
    );
    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 1);
    assert_eq!(json["apply_errors"], serde_json::json!([]));
    let requests = server.join().expect("server thread");
    assert_eq!(requests.len(), 4);
}

#[test]
fn github_resolve_review_thread_graphql_errors_are_apply_failures() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(r"[]"),
        github_threads_with_old(),
        MockResponse {
            method: "POST",
            path_contains: "/graphql",
            status: 200,
            body: r#"{"data":{"node":{"id":"T1","isResolved":false}}}"#,
        },
        MockResponse {
            method: "POST",
            path_contains: "/graphql",
            status: 200,
            body: r#"{"errors":[{"message":"cannot resolve thread"}]}"#,
        },
    ]);

    let output = run_reconcile(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &envelope,
    );
    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["threads_resolved"], 0);
    assert_eq!(json["failed_fingerprints"], serde_json::json!(["old"]));
    assert_eq!(json["unapplied_fingerprints"], serde_json::json!(["old"]));
    assert!(
        json["apply_errors"][0]
            .as_str()
            .unwrap()
            .contains("resolveReviewThread failed")
    );
    let requests = server.join().expect("server thread");
    assert_eq!(requests.len(), 4);
}

fn gitlab_discussions(body: &'static str) -> MockResponse {
    MockResponse {
        method: "GET",
        path_contains: "/projects/group%2Frepo/merge_requests/7/discussions?per_page=100&page=1",
        status: 200,
        body,
    }
}

#[test]
fn gitlab_deletion_race_stops_before_note_or_resolve() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","author":{"bot":true,"username":"project-bot"}}]}]"#,
        ),
        MockResponse {
            method: "GET",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d1",
            status: 404,
            body: r#"{"message":"404 Discussion Not Found"}"#,
        },
    ]);

    let output = run_reconcile(
        &[
            "--provider",
            "gitlab",
            "--mr",
            "7",
            "--project-id",
            "group/repo",
        ],
        &api_url,
        &envelope,
    );
    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["threads_resolved"], 0);
    assert_eq!(json["failed_fingerprints"], serde_json::json!(["old"]));
    assert_eq!(json["unapplied_fingerprints"], serde_json::json!(["old"]));
    let requests = server.join().expect("server thread");
    assert_eq!(requests.len(), 2);
    assert!(!requests.iter().any(|request| request.starts_with("POST ")));
    assert!(!requests.iter().any(|request| request.starts_with("PUT ")));
}

#[test]
fn gitlab_existing_legacy_marker_skips_duplicate_resolution_note() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","author":{"bot":true,"username":"project-bot"}},{"body":"<!-- fallow-resolved-fingerprint: old -->","author":{"bot":true,"username":"project-bot"}}]}]"#,
        ),
        MockResponse {
            method: "GET",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d1",
            status: 200,
            body: r#"{"id":"d1"}"#,
        },
        MockResponse {
            method: "PUT",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d1",
            status: 200,
            body: r#"{"id":"d1","resolved":true}"#,
        },
    ]);

    let output = run_reconcile(
        &[
            "--provider",
            "gitlab",
            "--mr",
            "7",
            "--project-id",
            "group/repo",
        ],
        &api_url,
        &envelope,
    );
    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["threads_resolved"], 1);
    assert_eq!(json["apply_errors"], serde_json::json!([]));
    let requests = server.join().expect("server thread");
    assert_eq!(requests.len(), 3);
    assert!(!requests.iter().any(|request| request.starts_with("POST ")));
}
