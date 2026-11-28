#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Credential headers of the CI provider requests, and what happens to them
//! when the provider API answers with a redirect.

use std::io::Write;
use std::net::TcpListener;
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::common::fallow_bin;
use crate::http_stub::{accept_within, read_request};

const GITLAB_TOKEN: &str = "glpat-provider-auth-test";
const GITHUB_TOKEN: &str = "ghp-provider-auth-test";

/// A stub that answers every request with `respond(request)` until the
/// returned flag is set. The thread returns every request that it read.
struct Stub {
    port: u16,
    stop: Arc<AtomicBool>,
    handle: thread::JoinHandle<Vec<String>>,
}

impl Stub {
    fn start(respond: impl Fn(&str) -> String + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
        let port = listener.local_addr().expect("local addr").port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            while !stop_flag.load(Ordering::SeqCst) {
                let Some(mut stream) = accept_within(&listener, Duration::from_millis(100)) else {
                    continue;
                };
                let request = read_request(&mut stream);
                let response = respond(&request);
                requests.push(request);
                stream
                    .write_all(response.as_bytes())
                    .expect("write response");
            }
            requests
        });
        Self { port, stop, handle }
    }

    fn requests(self) -> Vec<String> {
        self.stop.store(true, Ordering::SeqCst);
        self.handle.join().expect("stub thread")
    }
}

fn request_path(request: &str) -> &str {
    request.split_whitespace().nth(1).unwrap_or("/")
}

fn has_header(request: &str, name: &str, value: &str) -> bool {
    request.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, rest)| {
            key.trim().eq_ignore_ascii_case(name) && rest.trim() == value
        })
    })
}

fn json_list(extra_headers: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{extra_headers}\
         Content-Length: 2\r\nConnection: close\r\n\r\n[]"
    )
}

fn unauthorized() -> String {
    "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
}

fn redirect(status: &str, location: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

/// Run `ci post-pr-comment --dry-run` for one provider against `api_url`.
fn dry_run_comment(provider: &str, api_url: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let body = dir.path().join("comment.md");
    std::fs::write(&body, "<!-- fallow-id: summary -->\nnew body").expect("write body");
    let target = match provider {
        "gitlab" => ["--mr", "7", "--project-id", "group/repo"],
        _ => ["--pr", "7", "--repo", "owner/repo"],
    };
    Command::new(fallow_bin())
        .args(["--format", "json", "--quiet", "ci", "post-pr-comment"])
        .args(["--provider", provider])
        .args(target)
        .args(["--marker-id", "summary", "--dry-run", "--api-url", api_url])
        .arg("--body")
        .arg(&body)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_API_RETRIES", "1")
        .env_remove("FALLOW_BOT_LOGIN")
        .env("GITLAB_TOKEN", GITLAB_TOKEN)
        .env("GH_TOKEN", GITHUB_TOKEN)
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .output()
        .expect("run fallow post-pr-comment")
}

fn describe(output: &Output) -> String {
    format!(
        "exit {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn gitlab_requests_send_the_token_as_a_bearer_authorization() {
    let bearer = format!("Bearer {GITLAB_TOKEN}");
    let stub = Stub::start(move |request| {
        if has_header(request, "authorization", &bearer) {
            json_list("X-Next-Page: \r\n")
        } else {
            unauthorized()
        }
    });

    let output = dry_run_comment("gitlab", &format!("http://127.0.0.1:{}", stub.port));
    let requests = stub.requests();

    assert_eq!(output.status.code(), Some(0), "{}", describe(&output));
    assert!(!requests.is_empty(), "{}", describe(&output));
    for request in &requests {
        assert!(
            !request.to_ascii_lowercase().contains("private-token"),
            "{request}"
        );
    }
}

/// A redirect to another host is followed, but the target gets no token.
#[test]
fn a_redirect_to_another_host_receives_no_provider_token() {
    for (provider, token) in [("gitlab", GITLAB_TOKEN), ("github", GITHUB_TOKEN)] {
        let target = Stub::start(|_| json_list("X-Next-Page: \r\n"));
        let target_port = target.port;
        let first = Stub::start(move |request| {
            let path = request_path(request);
            redirect(
                "302 Found",
                &format!("http://localhost:{target_port}{path}"),
            )
        });

        let output = dry_run_comment(provider, &format!("http://127.0.0.1:{}", first.port));
        let first_requests = first.requests();
        let target_requests = target.requests();

        assert!(
            !first_requests.is_empty(),
            "{provider}: {}",
            describe(&output)
        );
        assert!(
            first_requests.iter().all(|request| request.contains(token)),
            "{provider}: the first host gets the token"
        );
        assert!(
            !target_requests.is_empty(),
            "{provider}: the redirect is followed. {}",
            describe(&output)
        );
        for request in &target_requests {
            assert!(!request.contains(token), "{provider}: {request}");
        }
    }
}

/// GitHub answers a request for a renamed repository with a 301 to the new
/// path on the same host. The request follows it with the token.
#[test]
fn a_same_host_redirect_keeps_the_authorization() {
    let bearer = format!("Bearer {GITHUB_TOKEN}");
    let target = Stub::start(move |request| {
        if has_header(request, "authorization", &bearer) {
            json_list("")
        } else {
            unauthorized()
        }
    });
    let target_port = target.port;
    let first = Stub::start(move |request| {
        let path = request_path(request);
        redirect(
            "301 Moved Permanently",
            &format!("http://127.0.0.1:{target_port}{path}"),
        )
    });

    let output = dry_run_comment("github", &format!("http://127.0.0.1:{}", first.port));
    let _ = first.requests();
    let target_requests = target.requests();

    assert_eq!(output.status.code(), Some(0), "{}", describe(&output));
    assert!(!target_requests.is_empty(), "{}", describe(&output));
}
