#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

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
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let len = stream.read(&mut buffer).expect("read request");
        if len == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..len]);
        if request_is_complete(&request) {
            break;
        }
    }
    String::from_utf8_lossy(&request).to_string()
}

fn request_is_complete(request: &[u8]) -> bool {
    let Some(header_end) = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
    else {
        return false;
    };
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    request.len() >= header_end + content_length
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
    write_envelope_with_review_id(fingerprints, None)
}

fn write_envelope_with_review_id(
    fingerprints: &[&str],
    review_id: Option<&str>,
) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let scope_marker = review_id
        .map(|review_id| format!("\n<!-- fallow-review-id: {review_id} -->"))
        .unwrap_or_default();
    let comments = fingerprints
        .iter()
        .map(|fingerprint| {
            serde_json::json!({
                "fingerprint": fingerprint,
                "body": format!("<!-- fallow-fingerprint: {fingerprint} -->{scope_marker}"),
            })
        })
        .collect::<Vec<_>>();
    let mut envelope = serde_json::json!({
        "body": format!("<!-- fallow-review -->{scope_marker}"),
        "comments": comments,
    });
    if let Some(review_id) = review_id {
        envelope["meta"] = serde_json::json!({ "review_id": review_id });
    }
    std::fs::write(
        dir.path().join("review.json"),
        serde_json::to_vec(&envelope).expect("serialize envelope"),
    )
    .expect("write envelope");
    dir
}

fn write_raw_envelope(envelope: &serde_json::Value) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("review.json"),
        serde_json::to_vec(envelope).expect("serialize envelope"),
    )
    .expect("write envelope");
    dir
}

#[test]
fn reconcile_rejects_scope_downgrade_and_cross_scope_before_provider_lookup() {
    for comment_body in ["finding", "finding\n<!-- fallow-review-id: backend -->"] {
        let envelope = write_raw_envelope(&serde_json::json!({
            "body": "summary\n<!-- fallow-review-id: frontend -->",
            "comments": [{"fingerprint":"same", "body":comment_body}],
            "meta": {"review_id":"frontend"}
        }));
        let output = run_reconcile(
            &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
            "http://127.0.0.1:9",
            &envelope,
        );

        assert_eq!(output.code, 2);
        let rendered = format!("{}{}", output.stdout, output.stderr);
        assert!(rendered.contains("comments[0].body scope"));
        assert!(!rendered.contains("GitHub request failed"));
    }
}

#[test]
fn post_review_rejects_scope_downgrade_and_cross_scope_before_provider_lookup() {
    for comment_body in ["finding", "finding\n<!-- fallow-review-id: backend -->"] {
        let envelope = write_raw_envelope(&serde_json::json!({
            "body": "summary\n<!-- fallow-review-id: frontend -->",
            "comments": [{"fingerprint":"same", "body":comment_body}],
            "meta": {"review_id":"frontend"}
        }));
        let output = Command::new(fallow_bin())
            .args(["--format", "json", "--quiet", "ci", "post-review"])
            .args(["--provider", "github", "--pr", "7", "--repo", "owner/repo"])
            .args(["--api-url", "http://127.0.0.1:9"])
            .arg("--envelope")
            .arg(envelope.path().join("review.json"))
            .env("GH_TOKEN", "test-token")
            .output()
            .expect("run fallow");

        assert_eq!(output.status.code(), Some(2));
        let rendered = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(rendered.contains("comments[0].body scope"));
        assert!(!rendered.contains("GitHub request failed"));
    }
}

#[test]
fn github_provider_state_isolates_identical_fingerprints_and_reports_scoped_stale() {
    let envelope = write_envelope_with_review_id(&["same"], Some("frontend"));
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[
                {"id":1,"body":"<!-- fallow-fingerprint: same -->\n<!-- fallow-review-id: backend -->","user":{"type":"Bot"}},
                {"id":2,"body":"<!-- fallow-fingerprint: same -->\n<!-- fallow-review-id: frontend -->","user":{"type":"Bot"}},
                {"id":3,"body":"<!-- fallow-fingerprint: stale -->\n<!-- fallow-review-id: frontend -->","user":{"type":"Bot"}}
            ]"#,
        ),
        github_threads_empty(),
    ]);

    let output = run_reconcile(
        &[
            "--provider",
            "github",
            "--pr",
            "7",
            "--repo",
            "owner/repo",
            "--dry-run",
        ],
        &api_url,
        &envelope,
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["existing_fingerprints"], 2);
    assert_eq!(json["new_fingerprints"], 0);
    assert_eq!(json["stale"], serde_json::json!(["stale"]));
    assert_eq!(server.join().expect("server thread").len(), 2);
}

#[test]
fn github_non_advancing_thread_cursor_reports_incomplete_provider_state() {
    let envelope = write_envelope(&["old"]);
    let (api_url, server) = serve(vec![
        github_comments(r"[]"),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":true,"endCursor":null}}}}}}"#,
        ),
    ]);

    let output = run_reconcile(
        &[
            "--provider",
            "github",
            "--pr",
            "7",
            "--repo",
            "owner/repo",
            "--dry-run",
        ],
        &api_url,
        &envelope,
    );
    let json = parse_json(&output);
    assert!(
        json["provider_warning"]
            .as_str()
            .expect("provider warning")
            .contains("non-advancing cursor")
    );
    assert_eq!(json["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 2);
}

#[test]
fn gitlab_provider_state_keeps_unscoped_review_separate_from_scoped_twin() {
    let envelope = write_envelope(&["same"]);
    let discussions = r#"[
        {"id":"scoped","notes":[{"body":"<!-- fallow-fingerprint: same -->\n<!-- fallow-review-id: frontend -->","resolved":false,"author":{"bot":true}}]},
        {"id":"unscoped","notes":[{"body":"<!-- fallow-fingerprint: same -->","resolved":false,"author":{"bot":true}}]},
        {"id":"stale","notes":[{"body":"<!-- fallow-fingerprint: stale -->","resolved":false,"author":{"bot":true}}]}
    ]"#;
    let (api_url, server) = serve(vec![MockResponse {
        method: "GET",
        path_contains: "/projects/123/merge_requests/9/discussions?per_page=100&page=1",
        status: 200,
        body: discussions,
    }]);

    let output = run_reconcile(
        &[
            "--provider",
            "gitlab",
            "--mr",
            "9",
            "--project-id",
            "123",
            "--dry-run",
        ],
        &api_url,
        &envelope,
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["existing_fingerprints"], 2);
    assert_eq!(json["new_fingerprints"], 0);
    assert_eq!(json["stale"], serde_json::json!(["stale"]));
    assert_eq!(server.join().expect("server thread").len(), 1);
}

fn run_reconcile(
    provider_args: &[&str],
    api_url: &str,
    envelope_dir: &tempfile::TempDir,
) -> common::CommandOutput {
    run_reconcile_with_identity(
        provider_args,
        api_url,
        envelope_dir,
        "abcdef1234567890",
        None,
    )
}

fn run_reconcile_with_identity(
    provider_args: &[&str],
    api_url: &str,
    envelope_dir: &tempfile::TempDir,
    sha: &str,
    bot_login: Option<&str>,
) -> common::CommandOutput {
    run_reconcile_with_identity_and_retries(
        provider_args,
        api_url,
        envelope_dir,
        sha,
        bot_login,
        "1",
    )
}

fn run_reconcile_with_identity_and_retries(
    provider_args: &[&str],
    api_url: &str,
    envelope_dir: &tempfile::TempDir,
    sha: &str,
    bot_login: Option<&str>,
    retries: &str,
) -> common::CommandOutput {
    let mut command = Command::new(fallow_bin());
    command
        .args(["--format", "json", "--quiet", "ci", "reconcile-review"])
        .args(provider_args)
        .args(["--api-url", api_url])
        .arg("--envelope")
        .arg(envelope_dir.path().join("review.json"))
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_API_RETRIES", retries)
        .env("FALLOW_API_RETRY_DELAY", "0")
        .env("GH_TOKEN", "test-token")
        .env("GITHUB_SHA", sha)
        .env("GITLAB_TOKEN", "test-token")
        .env("CI_COMMIT_SHA", sha)
        .env_remove("FALLOW_BOT_LOGIN");
    if let Some(bot_login) = bot_login {
        command.env("FALLOW_BOT_LOGIN", bot_login);
    }
    let output = command.output().expect("run fallow");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn run_post_review(
    provider_args: &[&str],
    api_url: &str,
    envelope_dir: &tempfile::TempDir,
) -> common::CommandOutput {
    let output = Command::new(fallow_bin())
        .args(["--format", "json", "--quiet", "ci", "post-review"])
        .args(provider_args)
        .args(["--api-url", api_url])
        .arg("--envelope")
        .arg(envelope_dir.path().join("review.json"))
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_API_RETRIES", "1")
        .env("GH_TOKEN", "test-token")
        .env("GITHUB_SHA", "eeeeeee123")
        .env("GITLAB_TOKEN", "test-token")
        .env("CI_COMMIT_SHA", "eeeeeee123")
        .env_remove("FALLOW_BOT_LOGIN")
        .output()
        .expect("run fallow post-review");
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

fn github_threads(body: &'static str) -> MockResponse {
    MockResponse {
        method: "POST",
        path_contains: "/graphql",
        status: 200,
        body,
    }
}

fn github_comment_preflight(comment_id: u64) -> MockResponse {
    match comment_id {
        1 => MockResponse {
            method: "GET",
            path_contains: "/repos/owner/repo/pulls/comments/1",
            status: 200,
            body: r#"{"id":1}"#,
        },
        20 => MockResponse {
            method: "GET",
            path_contains: "/repos/owner/repo/pulls/comments/20",
            status: 200,
            body: r#"{"id":20}"#,
        },
        _ => panic!("unsupported mock comment id"),
    }
}

fn github_thread_preflight(thread_id: &str, resolved: bool) -> MockResponse {
    match (thread_id, resolved) {
        ("T1", false) => github_threads(r#"{"data":{"node":{"id":"T1","isResolved":false}}}"#),
        ("T2", false) => github_threads(r#"{"data":{"node":{"id":"T2","isResolved":false}}}"#),
        _ => panic!("unsupported mock thread preflight"),
    }
}

fn github_resolution_reply(comment_id: u64, body: &'static str) -> MockResponse {
    match comment_id {
        1 => MockResponse {
            method: "POST",
            path_contains: "/repos/owner/repo/pulls/7/comments/1/replies",
            status: 201,
            body,
        },
        20 => MockResponse {
            method: "POST",
            path_contains: "/repos/owner/repo/pulls/7/comments/20/replies",
            status: 201,
            body,
        },
        _ => panic!("unsupported mock resolution reply"),
    }
}

fn github_thread_resolution(thread_id: &str) -> MockResponse {
    match thread_id {
        "T1" => github_threads(
            r#"{"data":{"resolveReviewThread":{"thread":{"id":"T1","isResolved":true}}}}"#,
        ),
        "T2" => github_threads(
            r#"{"data":{"resolveReviewThread":{"thread":{"id":"T2","isResolved":true}}}}"#,
        ),
        _ => panic!("unsupported mock thread resolution"),
    }
}

fn github_threads_with_old() -> MockResponse {
    MockResponse {
        method: "POST",
        path_contains: "/graphql",
        status: 200,
        body: r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":false,"comments":{"nodes":[{"databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
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
fn github_preflight_rejects_mismatched_thread_identity() {
    let clean = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}}]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":false,"comments":{"nodes":[{"databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comment_preflight(1),
        github_threads(r#"{"data":{"node":{"id":"OTHER","isResolved":false}}}"#),
    ]);

    let output = run_reconcile(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &clean,
    );
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["threads_resolved"], 0);
    assert_eq!(json["failed_fingerprints"], serde_json::json!(["old"]));
    assert!(
        json["apply_errors"][0]
            .as_str()
            .expect("apply error")
            .contains("preflight failed for review thread T1")
    );
    assert_eq!(server.join().expect("server thread").len(), 4);
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
            body: r#"{"id":11,"in_reply_to_id":1,"body":"<!-- fallow-resolved-fingerprint: a@abcdef1 -->"}"#,
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
fn github_existing_legacy_bare_marker_skips_reply_and_resolves_active_thread() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":9,"in_reply_to_id":1,"body":"<!-- fallow-resolved-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":false,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_thread_preflight("T1", false),
        github_thread_resolution("T1"),
    ]);

    let output = run_reconcile(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &envelope,
    );
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["threads_resolved"], 1);
    assert_eq!(server.join().expect("server thread").len(), 4);
}

#[test]
fn github_sha_marker_mismatch_does_not_imply_new_lifecycle() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":9,"body":"Resolved.\n\n<!-- fallow-resolved-fingerprint: old@1111111 -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#,
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
    assert_eq!(json["stale_fingerprints"], 0);
    assert_eq!(json["apply_errors"], serde_json::json!([]));
    let requests = server.join().expect("server thread");
    assert_eq!(requests.len(), 2);
}

#[test]
fn github_unattached_legacy_marker_closes_only_its_preceding_generation() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[
                {"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}},
                {"id":10,"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","user":{"type":"Bot"}},
                {"id":20,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}}
            ]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":true,"comments":{"nodes":[{"databaseId":1}]}},{"id":"T2","isResolved":false,"comments":{"nodes":[{"databaseId":20}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comment_preflight(20),
        github_thread_preflight("T2", false),
        github_thread_resolution("T2"),
        github_resolution_reply(
            20,
            r#"{"id":21,"in_reply_to_id":20,"body":"<!-- fallow-resolved-fingerprint: old@abcdef1 -->"}"#,
        ),
    ]);

    let output = run_reconcile(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &envelope,
    );
    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 1);
    assert_eq!(json["threads_resolved"], 1);
    assert!(json["failed_fingerprints"].is_null());
    let requests = server.join().expect("server thread");
    assert_eq!(requests.len(), 6);
    assert!(
        requests
            .iter()
            .any(|request| request.contains("comments/20/replies"))
    );
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("comments/1/replies"))
    );
}

#[test]
fn github_resolution_lifecycle_resolves_once_across_heads_and_supports_recurrence() {
    let clean = write_envelope(&[]);
    let current = write_envelope(&["old"]);
    let active_comments = r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#;
    let resolved_comments = r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":10,"in_reply_to_id":1,"body":"Resolved.\n<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#;
    let (api_url, server) = serve(vec![
        github_comments(active_comments),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":false,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comment_preflight(1),
        github_thread_preflight("T1", false),
        github_thread_resolution("T1"),
        github_resolution_reply(
            1,
            r#"{"id":10,"in_reply_to_id":1,"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->"}"#,
        ),
        github_comments(resolved_comments),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":true,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comments(resolved_comments),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":true,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comments(resolved_comments),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":true,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comments(resolved_comments),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":true,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":10,"in_reply_to_id":1,"body":"Resolved.\n<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":20,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":true,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":1}]}},{"id":"T2","isResolved":false,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":20}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comment_preflight(20),
        github_thread_preflight("T2", false),
        github_thread_resolution("T2"),
        github_resolution_reply(
            20,
            r#"{"id":21,"in_reply_to_id":20,"body":"<!-- fallow-resolved-fingerprint: old@fffffff -->"}"#,
        ),
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":10,"in_reply_to_id":1,"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":20,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":21,"in_reply_to_id":20,"body":"<!-- fallow-resolved-fingerprint: old@fffffff -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":true,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":1}]}},{"id":"T2","isResolved":true,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":20}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
    ]);
    let args = ["--provider", "github", "--pr", "7", "--repo", "owner/repo"];

    let resolved_at_b = run_reconcile_with_identity(&args, &api_url, &clean, "bbbbbbb1", None);
    let resolved_json = parse_json(&resolved_at_b);
    assert_eq!(resolved_json["resolution_comments_posted"], 1);
    assert_eq!(resolved_json["threads_resolved"], 1);
    assert_eq!(resolved_json["stale_fingerprints"], 1);

    for sha in ["ccccccc1", "ddddddd1", "ccccccc1"] {
        let later = run_reconcile_with_identity(&args, &api_url, &clean, sha, None);
        let json = parse_json(&later);
        assert_eq!(json["resolution_comments_posted"], 0);
        assert_eq!(json["threads_resolved"], 0);
        assert_eq!(json["stale_fingerprints"], 0);
    }

    let reappeared = run_reconcile_with_identity(&args, &api_url, &current, "eeeeeee1", None);
    let reappeared_json = parse_json(&reappeared);
    assert_eq!(reappeared_json["new"], serde_json::json!(["old"]));
    assert_eq!(reappeared_json["existing_fingerprints"], 0);

    let second_resolution = run_reconcile_with_identity(&args, &api_url, &clean, "fffffff1", None);
    let second_json = parse_json(&second_resolution);
    assert_eq!(second_json["resolution_comments_posted"], 1);
    assert_eq!(second_json["threads_resolved"], 1);

    let final_clean = run_reconcile_with_identity(&args, &api_url, &clean, "ggggggg1", None);
    let final_json = parse_json(&final_clean);
    assert_eq!(final_json["resolution_comments_posted"], 0);
    assert_eq!(final_json["threads_resolved"], 0);
    assert_eq!(final_json["stale_fingerprints"], 0);
    assert_eq!(server.join().expect("server thread").len(), 22);
}

#[test]
fn github_post_review_creates_new_discussion_for_reopened_resolved_lifecycle() {
    let envelope = write_raw_envelope(&serde_json::json!({
        "body": "<!-- fallow-review -->",
        "comments": [{
            "fingerprint": "old",
            "path": "src/index.ts",
            "line": 1,
            "side": "RIGHT",
            "body": "<!-- fallow-fingerprint: old -->"
        }]
    }));
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"github-actions[bot]"}},{"id":10,"in_reply_to_id":1,"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","user":{"type":"Bot","login":"github-actions[bot]"}}]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":false,"comments":{"nodes":[{"databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        MockResponse {
            method: "POST",
            path_contains: "/repos/owner/repo/pulls/7/reviews",
            status: 201,
            body: r#"{"id":20}"#,
        },
    ]);

    let output = run_post_review(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &envelope,
    );
    let json = parse_json(&output);
    assert_eq!(json["action"], "post_review");
    assert_eq!(json["comments_posted"], 1);
    assert_eq!(json["comments_skipped"], 0);
    assert_eq!(server.join().expect("server thread").len(), 3);
}

#[test]
fn github_resolve_review_thread_graphql_errors_are_apply_failures() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}}]"#,
        ),
        github_threads_with_old(),
        github_comment_preflight(1),
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
    assert_eq!(requests.len(), 5);
}

fn gitlab_discussions(body: &'static str) -> MockResponse {
    MockResponse {
        method: "GET",
        path_contains: "/projects/group%2Frepo/merge_requests/7/discussions?per_page=100&page=1",
        status: 200,
        body,
    }
}

fn gitlab_discussion_preflight(discussion_id: &str) -> MockResponse {
    match discussion_id {
        "d1" => MockResponse {
            method: "GET",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d1",
            status: 200,
            body: r#"{"id":"d1"}"#,
        },
        "d2" => MockResponse {
            method: "GET",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d2",
            status: 200,
            body: r#"{"id":"d2"}"#,
        },
        _ => panic!("unsupported mock discussion preflight"),
    }
}

fn gitlab_resolution_note(discussion_id: &str, body: &'static str) -> MockResponse {
    match discussion_id {
        "d1" => MockResponse {
            method: "POST",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d1/notes",
            status: 201,
            body,
        },
        "d2" => MockResponse {
            method: "POST",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d2/notes",
            status: 201,
            body,
        },
        _ => panic!("unsupported mock resolution note"),
    }
}

fn gitlab_discussion_resolution(discussion_id: &str) -> MockResponse {
    match discussion_id {
        "d1" => MockResponse {
            method: "PUT",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d1",
            status: 200,
            body: r#"{"id":"d1","resolved":true}"#,
        },
        "d2" => MockResponse {
            method: "PUT",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d2",
            status: 200,
            body: r#"{"id":"d2","resolved":true}"#,
        },
        _ => panic!("unsupported mock discussion resolution"),
    }
}

#[test]
fn gitlab_deletion_race_stops_before_note_or_resolve() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":false,"author":{"bot":true,"username":"project-bot"}}]}]"#,
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
fn gitlab_preflight_rejects_mismatched_discussion_identity() {
    let clean = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":false,"author":{"bot":true}}]}]"#,
        ),
        MockResponse {
            method: "GET",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d1",
            status: 200,
            body: r#"{"id":"other"}"#,
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
        &clean,
    );
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["threads_resolved"], 0);
    assert_eq!(json["failed_fingerprints"], serde_json::json!(["old"]));
    assert!(
        json["apply_errors"][0]
            .as_str()
            .expect("apply error")
            .contains("wrong discussion")
    );
    assert_eq!(server.join().expect("server thread").len(), 2);
}

#[test]
fn gitlab_existing_legacy_marker_skips_duplicate_resolution_note() {
    let envelope = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":false,"author":{"bot":true,"username":"project-bot"}},{"body":"<!-- fallow-resolved-fingerprint: old -->","author":{"bot":true,"username":"project-bot"}}]}]"#,
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

#[test]
fn gitlab_resolution_lifecycle_resolves_once_across_heads_and_supports_recurrence() {
    let clean = write_envelope(&[]);
    let current = write_envelope(&["old"]);
    let active = r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":false,"author":{"bot":true,"username":"project-bot"}}]}]"#;
    let resolved = r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":true,"author":{"bot":true,"username":"project-bot"}},{"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","author":{"bot":true,"username":"project-bot"}}]}]"#;
    let (api_url, server) = serve(vec![
        gitlab_discussions(active),
        gitlab_discussion_preflight("d1"),
        gitlab_discussion_resolution("d1"),
        gitlab_resolution_note(
            "d1",
            r#"{"id":10,"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->"}"#,
        ),
        gitlab_discussions(resolved),
        gitlab_discussions(resolved),
        gitlab_discussions(resolved),
        gitlab_discussions(resolved),
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":true,"author":{"bot":true,"username":"project-bot"}},{"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","author":{"bot":true,"username":"project-bot"}}]},{"id":"d2","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":false,"author":{"bot":true,"username":"project-bot"}}]}]"#,
        ),
        gitlab_discussion_preflight("d2"),
        gitlab_discussion_resolution("d2"),
        gitlab_resolution_note(
            "d2",
            r#"{"id":20,"body":"<!-- fallow-resolved-fingerprint: old@fffffff -->"}"#,
        ),
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":true,"author":{"bot":true,"username":"project-bot"}},{"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","author":{"bot":true,"username":"project-bot"}}]},{"id":"d2","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":true,"author":{"bot":true,"username":"project-bot"}},{"body":"<!-- fallow-resolved-fingerprint: old@fffffff -->","author":{"bot":true,"username":"project-bot"}}]}]"#,
        ),
    ]);
    let args = [
        "--provider",
        "gitlab",
        "--mr",
        "7",
        "--project-id",
        "group/repo",
    ];

    let resolved_at_b = run_reconcile_with_identity(&args, &api_url, &clean, "bbbbbbb1", None);
    let resolved_json = parse_json(&resolved_at_b);
    assert_eq!(resolved_json["resolution_comments_posted"], 1);
    assert_eq!(resolved_json["threads_resolved"], 1);

    for sha in ["ccccccc1", "ddddddd1", "ccccccc1"] {
        let later = run_reconcile_with_identity(&args, &api_url, &clean, sha, None);
        let json = parse_json(&later);
        assert_eq!(json["resolution_comments_posted"], 0);
        assert_eq!(json["threads_resolved"], 0);
        assert_eq!(json["stale_fingerprints"], 0);
    }

    let reappeared = run_reconcile_with_identity(&args, &api_url, &current, "eeeeeee1", None);
    let reappeared_json = parse_json(&reappeared);
    assert_eq!(reappeared_json["new"], serde_json::json!(["old"]));
    assert_eq!(reappeared_json["existing_fingerprints"], 0);

    let second_resolution = run_reconcile_with_identity(&args, &api_url, &clean, "fffffff1", None);
    let second_json = parse_json(&second_resolution);
    assert_eq!(second_json["resolution_comments_posted"], 1);
    assert_eq!(second_json["threads_resolved"], 1);

    let final_clean = run_reconcile_with_identity(&args, &api_url, &clean, "ggggggg1", None);
    let final_json = parse_json(&final_clean);
    assert_eq!(final_json["resolution_comments_posted"], 0);
    assert_eq!(final_json["threads_resolved"], 0);
    assert_eq!(final_json["stale_fingerprints"], 0);
    assert_eq!(server.join().expect("server thread").len(), 13);
}

#[test]
fn gitlab_post_review_creates_new_discussion_for_reopened_resolved_lifecycle() {
    let envelope = write_raw_envelope(&serde_json::json!({
        "body": "<!-- fallow-review -->",
        "comments": [{
            "fingerprint": "old",
            "body": "<!-- fallow-fingerprint: old -->",
            "position": {
                "base_sha": "aaaaaaa",
                "start_sha": "aaaaaaa",
                "head_sha": "eeeeeee",
                "new_path": "src/index.ts",
                "new_line": 1,
                "position_type": "text"
            }
        }]
    }));
    let (api_url, server) = serve(vec![
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":false,"author":{"bot":true,"username":"project-bot"}},{"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","author":{"bot":true,"username":"project-bot"}}]}]"#,
        ),
        MockResponse {
            method: "POST",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions",
            status: 201,
            body: r#"{"id":"d2"}"#,
        },
    ]);

    let output = run_post_review(
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
    let json = parse_json(&output);
    assert_eq!(json["action"], "post_review");
    assert_eq!(json["comments_posted"], 1);
    assert_eq!(json["comments_skipped"], 0);
    assert_eq!(server.join().expect("server thread").len(), 2);
}

#[test]
fn github_bot_login_fallback_owns_resolution_marker_without_native_bot_metadata() {
    let clean = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"User","login":"token-user"}},{"id":2,"in_reply_to_id":1,"body":"<!-- fallow-resolved-fingerprint: old -->","user":{"type":"User","login":"token-user"}}]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":false,"comments":{"nodes":[{"body":"<!-- fallow-fingerprint: old -->","databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_thread_preflight("T1", false),
        github_thread_resolution("T1"),
    ]);

    let output = run_reconcile_with_identity(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &clean,
        "abcdef123",
        Some("token-user"),
    );
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["threads_resolved"], 1);
    assert_eq!(server.join().expect("server thread").len(), 4);
}

#[test]
fn gitlab_bot_login_fallback_owns_project_token_marker_without_bot_field() {
    let clean = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        gitlab_discussions(
            r#"[{"id":"d1","resolved":false,"notes":[{"body":"<!-- fallow-fingerprint: old -->","system":false,"author":{"username":"project-token-user"}},{"body":"<!-- fallow-resolved-fingerprint: old -->","system":false,"author":{"username":"project-token-user"}}]}]"#,
        ),
        gitlab_discussion_preflight("d1"),
        gitlab_discussion_resolution("d1"),
    ]);

    let output = run_reconcile_with_identity(
        &[
            "--provider",
            "gitlab",
            "--mr",
            "7",
            "--project-id",
            "group/repo",
        ],
        &api_url,
        &clean,
        "abcdef123",
        Some("project-token-user"),
    );
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["threads_resolved"], 1);
    assert_eq!(server.join().expect("server thread").len(), 3);
}

#[test]
fn github_bot_login_narrows_ownership_to_the_configured_bot() {
    let current = write_envelope(&["old"]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot","login":"foreign[bot]"}}]"#,
        ),
        github_threads_empty(),
    ]);

    let output = run_reconcile_with_identity(
        &[
            "--provider",
            "github",
            "--pr",
            "7",
            "--repo",
            "owner/repo",
            "--dry-run",
        ],
        &api_url,
        &current,
        "abcdef123",
        Some("fallow[bot]"),
    );
    let json = parse_json(&output);
    assert_eq!(json["existing_fingerprints"], 0);
    assert_eq!(json["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 2);
}

#[test]
fn gitlab_bot_login_narrows_ownership_to_the_configured_bot() {
    let current = write_envelope(&["old"]);
    let (api_url, server) = serve(vec![gitlab_discussions(
        r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","author":{"bot":true,"username":"foreign-bot"}}]}]"#,
    )]);

    let output = run_reconcile_with_identity(
        &[
            "--provider",
            "gitlab",
            "--mr",
            "7",
            "--project-id",
            "group/repo",
            "--dry-run",
        ],
        &api_url,
        &current,
        "abcdef123",
        Some("fallow-bot"),
    );
    let json = parse_json(&output);
    assert_eq!(json["existing_fingerprints"], 0);
    assert_eq!(json["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 1);
}

#[test]
fn github_owned_root_without_numeric_id_fails_provider_state_closed() {
    let current = write_envelope(&["old"]);
    let (api_url, server) = serve(vec![github_comments(
        r#"[{"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}}]"#,
    )]);

    let output = run_reconcile(
        &[
            "--provider",
            "github",
            "--pr",
            "7",
            "--repo",
            "owner/repo",
            "--dry-run",
        ],
        &api_url,
        &current,
    );
    let json = parse_json(&output);
    assert!(
        json["provider_warning"]
            .as_str()
            .expect("provider warning")
            .contains("did not contain a numeric id")
    );
    assert_eq!(json["existing_fingerprints"], 0);
    assert_eq!(json["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 1);
}

#[test]
fn github_resolution_marker_with_nonnumeric_parent_fails_provider_state_closed() {
    let current = write_envelope(&[]);
    let (api_url, server) = serve(vec![github_comments(
        r#"[{"id":2,"in_reply_to_id":"bad-parent","body":"<!-- fallow-resolved-fingerprint: old -->","user":{"type":"Bot"}}]"#,
    )]);

    let output = run_reconcile(
        &[
            "--provider",
            "github",
            "--pr",
            "7",
            "--repo",
            "owner/repo",
            "--dry-run",
        ],
        &api_url,
        &current,
    );
    let json = parse_json(&output);
    assert!(
        json["provider_warning"]
            .as_str()
            .expect("provider warning")
            .contains("nonnumeric in_reply_to_id")
    );
    assert_eq!(json["stale_fingerprints"], 0);
    assert_eq!(server.join().expect("server thread").len(), 1);
}

#[test]
fn github_owned_finding_with_nonnumeric_parent_fails_provider_state_closed() {
    let current = write_envelope(&["old"]);
    let (api_url, server) = serve(vec![github_comments(
        r#"[{"id":2,"in_reply_to_id":"bad-parent","body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}}]"#,
    )]);

    let output = run_reconcile(
        &[
            "--provider",
            "github",
            "--pr",
            "7",
            "--repo",
            "owner/repo",
            "--dry-run",
        ],
        &api_url,
        &current,
    );
    let json = parse_json(&output);
    assert!(
        json["provider_warning"]
            .as_str()
            .expect("provider warning")
            .contains("nonnumeric in_reply_to_id")
    );
    assert_eq!(json["existing_fingerprints"], 0);
    assert_eq!(json["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 1);
}

#[test]
fn gitlab_discussion_without_string_id_fails_provider_state_closed() {
    let current = write_envelope(&["old"]);
    let (api_url, server) = serve(vec![gitlab_discussions(
        r#"[{"notes":[{"body":"<!-- fallow-fingerprint: old -->","author":{"bot":true}}]}]"#,
    )]);

    let output = run_reconcile(
        &[
            "--provider",
            "gitlab",
            "--mr",
            "7",
            "--project-id",
            "group/repo",
            "--dry-run",
        ],
        &api_url,
        &current,
    );
    let json = parse_json(&output);
    assert!(
        json["provider_warning"]
            .as_str()
            .expect("provider warning")
            .contains("did not contain a string id")
    );
    assert_eq!(json["existing_fingerprints"], 0);
    assert_eq!(json["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 1);
}

#[test]
fn gitlab_discussion_without_root_note_fails_provider_state_closed() {
    let current = write_envelope(&[]);
    let (api_url, server) = serve(vec![gitlab_discussions(r#"[{"id":"d1","notes":[]}]"#)]);

    let output = run_reconcile(
        &[
            "--provider",
            "gitlab",
            "--mr",
            "7",
            "--project-id",
            "group/repo",
            "--dry-run",
        ],
        &api_url,
        &current,
    );
    let json = parse_json(&output);
    assert!(
        json["provider_warning"]
            .as_str()
            .expect("provider warning")
            .contains("did not contain a root note")
    );
    assert_eq!(json["stale_fingerprints"], 0);
    assert_eq!(server.join().expect("server thread").len(), 1);
}

#[test]
fn gitlab_owned_discussion_without_resolved_status_fails_provider_state_closed() {
    let current = write_envelope(&["old"]);
    let (api_url, server) = serve(vec![gitlab_discussions(
        r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","author":{"bot":true}}]}]"#,
    )]);

    let output = run_reconcile(
        &[
            "--provider",
            "gitlab",
            "--mr",
            "7",
            "--project-id",
            "group/repo",
            "--dry-run",
        ],
        &api_url,
        &current,
    );
    let json = parse_json(&output);
    assert!(
        json["provider_warning"]
            .as_str()
            .expect("provider warning")
            .contains("did not contain a boolean resolved status")
    );
    assert_eq!(json["existing_fingerprints"], 0);
    assert_eq!(json["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 1);
}

#[test]
fn github_non_idempotent_resolution_reply_does_not_retry_ambiguous_gateway_error() {
    let clean = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}}]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":true,"comments":{"nodes":[{"databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comment_preflight(1),
        MockResponse {
            method: "POST",
            path_contains: "/repos/owner/repo/pulls/7/comments/1/replies",
            status: 503,
            body: r#"{"message":"ambiguous gateway failure"}"#,
        },
    ]);

    let output = run_reconcile_with_identity_and_retries(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &clean,
        "abcdef123",
        None,
        "3",
    );
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert!(
        json["apply_errors"][0]
            .as_str()
            .expect("apply error")
            .contains("HTTP 503")
    );
    assert_eq!(server.join().expect("server thread").len(), 4);
}

#[test]
fn gitlab_non_idempotent_resolution_note_does_not_retry_ambiguous_gateway_error() {
    let clean = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":true,"author":{"bot":true}}]}]"#,
        ),
        gitlab_discussion_preflight("d1"),
        MockResponse {
            method: "POST",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d1/notes",
            status: 503,
            body: r#"{"message":"ambiguous gateway failure"}"#,
        },
    ]);

    let output = run_reconcile_with_identity_and_retries(
        &[
            "--provider",
            "gitlab",
            "--mr",
            "7",
            "--project-id",
            "group/repo",
        ],
        &api_url,
        &clean,
        "abcdef123",
        None,
        "3",
    );
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert!(
        json["apply_errors"][0]
            .as_str()
            .expect("apply error")
            .contains("HTTP 503")
    );
    assert_eq!(server.join().expect("server thread").len(), 3);
}

#[test]
fn github_human_root_marker_cannot_own_or_suppress_a_finding() {
    let current = write_envelope(&["old"]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"User","login":"alice"}}]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":false,"comments":{"nodes":[{"databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
    ]);

    let output = run_reconcile_with_identity(
        &[
            "--provider",
            "github",
            "--pr",
            "7",
            "--repo",
            "owner/repo",
            "--dry-run",
        ],
        &api_url,
        &current,
        "abcdef123",
        None,
    );
    let json = parse_json(&output);
    assert_eq!(json["existing_fingerprints"], 0);
    assert_eq!(json["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 2);
}

#[test]
fn gitlab_human_root_marker_cannot_own_or_suppress_a_finding() {
    let current = write_envelope(&["old"]);
    let (api_url, server) = serve(vec![gitlab_discussions(
        r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","system":false,"author":{"bot":false,"username":"alice"}}]}]"#,
    )]);

    let output = run_reconcile_with_identity(
        &[
            "--provider",
            "gitlab",
            "--mr",
            "7",
            "--project-id",
            "group/repo",
            "--dry-run",
        ],
        &api_url,
        &current,
        "abcdef123",
        None,
    );
    let json = parse_json(&output);
    assert_eq!(json["existing_fingerprints"], 0);
    assert_eq!(json["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 1);
}

#[test]
fn github_manual_resolution_deduplicates_until_fallow_closes_the_lifecycle() {
    let current = write_envelope(&["old"]);
    let clean = write_envelope(&[]);
    let root = r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}}]"#;
    let closed = r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}},{"id":2,"in_reply_to_id":1,"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","user":{"type":"Bot"}}]"#;
    let resolved_thread = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":true,"comments":{"nodes":[{"databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#;
    let (api_url, server) = serve(vec![
        github_comments(root),
        github_threads(resolved_thread),
        github_comments(root),
        github_threads(resolved_thread),
        github_comment_preflight(1),
        github_resolution_reply(
            1,
            r#"{"id":10,"in_reply_to_id":1,"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->"}"#,
        ),
        github_comments(closed),
        github_threads(resolved_thread),
    ]);
    let args = ["--provider", "github", "--pr", "7", "--repo", "owner/repo"];

    let manual = parse_json(&run_reconcile_with_identity(
        &args, &api_url, &current, "aaaaaaa1", None,
    ));
    assert_eq!(manual["new_fingerprints"], 0);
    assert_eq!(manual["stale_fingerprints"], 0);

    let disappeared = parse_json(&run_reconcile_with_identity(
        &args, &api_url, &clean, "bbbbbbb1", None,
    ));
    assert_eq!(disappeared["resolution_comments_posted"], 1);
    assert_eq!(disappeared["threads_resolved"], 0);

    let recurrence = parse_json(&run_reconcile_with_identity(
        &args, &api_url, &current, "ccccccc1", None,
    ));
    assert_eq!(recurrence["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 8);
}

#[test]
fn gitlab_manual_resolution_deduplicates_until_fallow_closes_the_lifecycle() {
    let current = write_envelope(&["old"]);
    let clean = write_envelope(&[]);
    let root = r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":true,"author":{"bot":true}}]}]"#;
    let closed = r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":true,"author":{"bot":true}},{"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->","author":{"bot":true}}]}]"#;
    let (api_url, server) = serve(vec![
        gitlab_discussions(root),
        gitlab_discussions(root),
        gitlab_discussion_preflight("d1"),
        gitlab_resolution_note(
            "d1",
            r#"{"id":10,"body":"<!-- fallow-resolved-fingerprint: old@bbbbbbb -->"}"#,
        ),
        gitlab_discussions(closed),
    ]);
    let args = [
        "--provider",
        "gitlab",
        "--mr",
        "7",
        "--project-id",
        "group/repo",
    ];

    let manual = parse_json(&run_reconcile_with_identity(
        &args, &api_url, &current, "aaaaaaa1", None,
    ));
    assert_eq!(manual["new_fingerprints"], 0);
    assert_eq!(manual["stale_fingerprints"], 0);

    let disappeared = parse_json(&run_reconcile_with_identity(
        &args, &api_url, &clean, "bbbbbbb1", None,
    ));
    assert_eq!(disappeared["resolution_comments_posted"], 1);
    assert_eq!(disappeared["threads_resolved"], 0);

    let recurrence = parse_json(&run_reconcile_with_identity(
        &args, &api_url, &current, "ccccccc1", None,
    ));
    assert_eq!(recurrence["new"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 5);
}

#[test]
fn github_resolution_counter_requires_confirmed_provider_state() {
    let clean = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}}]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":false,"comments":{"nodes":[{"databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comment_preflight(1),
        github_thread_preflight("T1", false),
        github_threads(
            r#"{"data":{"resolveReviewThread":{"thread":{"id":"OTHER","isResolved":true}}}}"#,
        ),
    ]);

    let output = run_reconcile(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &clean,
    );
    let json = parse_json(&output);
    assert_eq!(json["threads_resolved"], 0);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["failed_fingerprints"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 5);
}

#[test]
fn gitlab_resolution_counter_requires_confirmed_provider_state() {
    let clean = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":false,"author":{"bot":true}}]}]"#,
        ),
        gitlab_discussion_preflight("d1"),
        MockResponse {
            method: "PUT",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d1",
            status: 200,
            body: r#"{"id":"other","resolved":true}"#,
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
        &clean,
    );
    let json = parse_json(&output);
    assert_eq!(json["threads_resolved"], 0);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["failed_fingerprints"], serde_json::json!(["old"]));
    assert_eq!(server.join().expect("server thread").len(), 3);
}

#[test]
fn github_resolution_reply_counter_requires_confirmed_created_reply() {
    let clean = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        github_comments(
            r#"[{"id":1,"body":"<!-- fallow-fingerprint: old -->","user":{"type":"Bot"}}]"#,
        ),
        github_threads(
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"T1","isResolved":true,"comments":{"nodes":[{"databaseId":1}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        ),
        github_comment_preflight(1),
        MockResponse {
            method: "POST",
            path_contains: "/repos/owner/repo/pulls/7/comments/1/replies",
            status: 201,
            body: r#"{"id":10,"in_reply_to_id":1,"body":"sanitized"}"#,
        },
    ]);

    let output = run_reconcile(
        &["--provider", "github", "--pr", "7", "--repo", "owner/repo"],
        &api_url,
        &clean,
    );
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["failed_fingerprints"], serde_json::json!(["old"]));
    assert!(
        json["apply_errors"][0]
            .as_str()
            .expect("apply error")
            .contains("did not confirm the created reply")
    );
    assert_eq!(server.join().expect("server thread").len(), 4);
}

#[test]
fn gitlab_resolution_reply_counter_requires_confirmed_created_note() {
    let clean = write_envelope(&[]);
    let (api_url, server) = serve(vec![
        gitlab_discussions(
            r#"[{"id":"d1","notes":[{"body":"<!-- fallow-fingerprint: old -->","resolved":true,"author":{"bot":true}}]}]"#,
        ),
        gitlab_discussion_preflight("d1"),
        MockResponse {
            method: "POST",
            path_contains: "/projects/group%2Frepo/merge_requests/7/discussions/d1/notes",
            status: 201,
            body: r#"{"id":10,"body":"sanitized"}"#,
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
        &clean,
    );
    let json = parse_json(&output);
    assert_eq!(json["resolution_comments_posted"], 0);
    assert_eq!(json["failed_fingerprints"], serde_json::json!(["old"]));
    assert!(
        json["apply_errors"][0]
            .as_str()
            .expect("apply error")
            .contains("did not confirm the created note")
    );
    assert_eq!(server.join().expect("server thread").len(), 3);
}
