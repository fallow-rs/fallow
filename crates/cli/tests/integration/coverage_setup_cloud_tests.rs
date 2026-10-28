#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow coverage setup --cloud`: the Fallow Cloud plan ties the beacon,
//! the inventory, the source maps and the deployment registration to one
//! commit SHA, and `--wait` reads the activation endpoint against a mock
//! HTTP server.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::common::{CommandOutput, fallow_bin, parse_json};
use crate::http_stub::read_request;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

type Captured = Arc<Mutex<Vec<String>>>;

/// Serve `(status, body)` answers in order, one connection each.
fn serve(responses: Vec<(u16, String)>) -> (String, Captured, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("mock addr");
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let handle = thread::spawn(move || {
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().expect("accept request");
            let request = read_request(&mut stream);
            sink.lock().expect("capture lock").push(request);
            write_response(&mut stream, status, &body);
        }
    });
    (format!("http://{addr}"), captured, handle)
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) {
    let head = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).expect("write head");
    stream.write_all(body.as_bytes()).expect("write body");
}

fn vite_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"web","scripts":{"build":"vite build"},"devDependencies":{"vite":"6.0.0"}}"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(dir.path().join(".github/workflows")).expect("workflows dir");
    std::fs::write(
        dir.path().join(".github/workflows/deploy.yml"),
        "name: Deploy\non: push\njobs:\n  deploy:\n    runs-on: ubuntu-latest\n",
    )
    .expect("write workflow");
    std::fs::write(
        dir.path().join(".github/workflows/lint.yml"),
        "name: Lint\n",
    )
    .expect("write workflow");
    dir
}

fn run(args: &[&str], root: &Path) -> CommandOutput {
    let output = Command::new(fallow_bin())
        .args(args)
        .arg("--root")
        .arg(root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_API_KEY", "fallow_live_k1_testkeyvalue")
        .env("FALLOW_AGENT_SOURCE", "none")
        .env_remove("FALLOW_API_URL")
        .env_remove("GITHUB_REPOSITORY")
        .env_remove("CI_PROJECT_PATH")
        .output()
        .expect("run fallow");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn activation_body(complete: bool) -> String {
    format!(
        r#"{{"data":{{"repo":"acme/web","sha":"{SHA}","shaSource":"query","complete":{complete},
        "steps":[{{"id":"beacon","state":"done","detail":null}},{{"id":"runtime","state":"{state}","detail":null}},
        {{"id":"new_step","state":"new_state","detail":null}}],
        "next":{next}}}}}"#,
        state = if complete { "done" } else { "waiting" },
        next = if complete {
            "null".to_owned()
        } else {
            r#"{"id":"runtime","message":"Send production traffic.","command":null}"#.to_owned()
        },
    )
}

#[test]
fn cloud_plan_ties_every_step_to_one_commit_sha() {
    let project = vite_project();
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--json",
            "--project-id",
            "acme/web",
        ],
        project.path(),
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let cloud = &json["cloud"];

    assert_eq!(cloud["project_id"], "acme/web");
    assert_eq!(cloud["api_key_env"], "FALLOW_API_KEY");
    assert_eq!(cloud["browser_key_env"], "VITE_FALLOW_API_KEY");
    assert_eq!(cloud["ci"]["provider"], "github_actions");
    assert_eq!(
        cloud["ci"]["workflows_detected"],
        serde_json::json!([".github/workflows/deploy.yml"])
    );
    assert_eq!(cloud["source_maps"]["needed"], true);
    assert_eq!(cloud["source_maps"]["dirs"], serde_json::json!(["dist"]));

    let snippet = json["snippets"][0]["content"].as_str().unwrap();
    assert!(
        snippet.contains("commitSha: import.meta.env.VITE_FALLOW_COMMIT_SHA"),
        "{snippet}"
    );
    assert!(
        snippet.contains(r#"coverageOrigin: "production""#),
        "{snippet}"
    );
    assert!(snippet.contains(r#"projectId: "acme/web""#), "{snippet}");
    assert!(
        snippet.contains(r#"runtimeSurface: "browser""#),
        "{snippet}"
    );
    assert!(!snippet.contains("FALLOW_TRANSPORT"), "{snippet}");

    let steps = cloud["ci"]["steps"].as_str().unwrap();
    assert!(steps.contains("fetch-depth: 0"), "{steps}");
    assert!(
        steps.contains("VITE_FALLOW_COMMIT_SHA: ${{ github.sha }}"),
        "{steps}"
    );
    assert!(
        steps
            .contains(r#"coverage upload-inventory --project-id=acme/web --git-sha="$GITHUB_SHA""#),
        "{steps}"
    );
    assert!(
        steps.contains(
            r#"coverage upload-source-maps --dir=dist --repo=acme/web --git-sha="$GITHUB_SHA""#
        ),
        "{steps}"
    );
    assert!(
        steps.contains("https://api.fallow.cloud/v1/deployments"),
        "{steps}"
    );
    assert!(steps.contains(r#"\"gitSha\":\"$GITHUB_SHA\""#), "{steps}");

    // The plan names the key variable, never a key value from the env.
    assert!(!output.stdout.contains("testkeyvalue"));
    assert!(!json["warnings"].as_array().unwrap().iter().any(|warning| {
        warning
            .as_str()
            .unwrap()
            .contains("local coverage artifact")
    }));
}

#[test]
fn cloud_plan_for_a_node_service_sets_the_commit_sha_and_v8_coverage() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"api","main":"src/server.js","scripts":{"start":"node src/server.js"}}"#,
    )
    .expect("write package.json");
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--json",
            "--project-id",
            "acme/api",
        ],
        dir.path(),
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);

    let snippet = json["snippets"][0]["content"].as_str().unwrap();
    assert!(
        snippet.contains("process.env.FALLOW_COMMIT_SHA ??"),
        "{snippet}"
    );
    assert!(
        snippet.contains("process.env.VERCEL_GIT_COMMIT_SHA"),
        "{snippet}"
    );
    let dockerfile = json["dockerfile_snippet"].as_str().unwrap();
    assert!(
        dockerfile.contains("ENV FALLOW_COMMIT_SHA=$FALLOW_COMMIT_SHA"),
        "{dockerfile}"
    );
    assert!(dockerfile.contains("NODE_V8_COVERAGE"), "{dockerfile}");
    assert_eq!(json["cloud"]["source_maps"]["needed"], false);
    assert!(json["cloud"]["browser_key_env"].is_null());
    let steps = json["cloud"]["ci"]["steps"].as_str().unwrap();
    assert!(!steps.contains("upload-source-maps"), "{steps}");
}

#[test]
fn cloud_plan_refuses_a_project_id_that_could_become_a_flag() {
    let project = vite_project();
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--json",
            "--project-id=-x/web",
        ],
        project.path(),
    );
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(output.stderr.contains("project id"), "{}", output.stderr);
}

#[test]
fn local_setup_json_has_no_cloud_block() {
    let project = vite_project();
    let output = run(&["coverage", "setup", "--json"], project.path());
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert!(json.get("cloud").is_none());
}

#[test]
fn wait_reads_the_activation_of_one_sha_and_exits_zero_when_complete() {
    let project = vite_project();
    let (endpoint, captured, handle) = serve(vec![(200, activation_body(true))]);
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--wait",
            "--json",
            "--project-id",
            "acme/web",
            "--git-sha",
            SHA,
            "--api-endpoint",
            &endpoint,
        ],
        project.path(),
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);

    let request = captured.lock().unwrap()[0].clone();
    assert!(
        request.starts_with(&format!("GET /v1/repos/acme%2Fweb/activation?sha={SHA} ")),
        "{request}"
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer fallow_live_k1_testkeyvalue"),
        "{request}"
    );

    let json = parse_json(&output);
    let wait = &json["cloud"]["wait"];
    assert_eq!(wait["outcome"], "complete");
    // New step ids and states pass through unchanged.
    assert_eq!(wait["activation"]["steps"][2]["state"], "new_state");
    assert_eq!(json["cloud"]["git_sha"], SHA);
}

#[test]
fn wait_exits_one_on_timeout_with_the_last_state() {
    let project = vite_project();
    let (endpoint, _captured, handle) = serve(vec![(200, activation_body(false))]);
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--wait",
            "--wait-timeout",
            "0",
            "--json",
            "--project-id",
            "acme/web",
            "--git-sha",
            SHA,
            "--api-endpoint",
            &endpoint,
        ],
        project.path(),
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["cloud"]["wait"]["outcome"], "timeout");
    assert_eq!(json["cloud"]["wait"]["activation"]["next"]["id"], "runtime");
}

#[test]
fn wait_stops_at_once_on_an_auth_error() {
    let project = vite_project();
    let (endpoint, _captured, handle) = serve(vec![(
        401,
        r#"{"error":true,"message":"Invalid API key","code":"unauthorized"}"#.to_owned(),
    )]);
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--wait",
            "--json",
            "--project-id",
            "acme/web",
            "--git-sha",
            SHA,
            "--api-endpoint",
            &endpoint,
        ],
        project.path(),
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["cloud"]["wait"]["outcome"], "error");
}

#[test]
fn wait_needs_the_cloud_flag() {
    let project = vite_project();
    let output = run(&["coverage", "setup", "--wait"], project.path());
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
}

#[test]
fn cloud_plan_for_a_workspace_tags_each_member_and_uploads_each_build() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"mono","private":true,"workspaces":["apps/*"]}"#,
    )
    .expect("write root package.json");
    for app in ["admin", "web"] {
        std::fs::create_dir_all(root.join("apps").join(app)).expect("app dir");
        std::fs::write(
            root.join("apps").join(app).join("package.json"),
            format!(
                r#"{{"name":"{app}","scripts":{{"dev":"vite","build":"vite build"}},"devDependencies":{{"vite":"6.0.0"}}}}"#
            ),
        )
        .expect("write app package.json");
    }
    std::fs::create_dir_all(root.join(".github/workflows")).expect("workflows dir");
    // A CI file that only mentions the word is not a deploy workflow.
    std::fs::write(
        root.join(".github/workflows/ci.yml"),
        "name: CI\n# runs before the deploy\n",
    )
    .expect("write ci workflow");
    std::fs::write(
        root.join(".github/workflows/pages.yml"),
        "jobs:\n  ship:\n    steps:\n      - run: npx wrangler pages deploy dist\n",
    )
    .expect("write pages workflow");

    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--json",
            "--project-id",
            "acme/mono",
        ],
        root,
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);

    assert_eq!(
        json["cloud"]["source_maps"]["dirs"],
        serde_json::json!(["apps/admin/dist", "apps/web/dist"])
    );
    let steps = json["cloud"]["ci"]["steps"].as_str().unwrap();
    assert!(steps.contains("--dir=apps/admin/dist"), "{steps}");
    assert!(steps.contains("--dir=apps/web/dist"), "{steps}");
    assert_eq!(
        json["cloud"]["ci"]["workflows_detected"],
        serde_json::json!([".github/workflows/pages.yml"])
    );
    let surfaces: Vec<&str> = json["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member["snippets"][0]["content"].as_str().unwrap())
        .collect();
    assert!(
        surfaces[0].contains(r#"runtimeSurface: "admin""#),
        "{}",
        surfaces[0]
    );
    assert!(
        surfaces[1].contains(r#"runtimeSurface: "web""#),
        "{}",
        surfaces[1]
    );
}

fn vite_workspace(member_names: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"mono","private":true,"workspaces":["apps/*"]}"#,
    )
    .expect("write root package.json");
    for name in member_names {
        let member = dir.path().join("apps").join(name);
        std::fs::create_dir_all(&member).expect("member dir");
        std::fs::write(
            member.join("package.json"),
            r#"{"name":"app","scripts":{"dev":"vite","build":"vite build"},"devDependencies":{"vite":"6.0.0"}}"#,
        )
        .expect("write member package.json");
    }
    dir
}

#[test]
fn cloud_plan_leaves_an_unsafe_build_dir_out_of_the_shell_steps() {
    let project = vite_workspace(&["ok", "$(id)"]);
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--json",
            "--project-id",
            "acme/mono",
        ],
        project.path(),
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let steps = json["cloud"]["ci"]["steps"].as_str().unwrap();
    assert!(!steps.contains("$(id)"), "{steps}");
    assert!(steps.contains("--dir=apps/ok/dist"), "{steps}");
    assert!(
        json["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("apps/$(id)/dist")),
        "{}",
        json["warnings"]
    );
}

#[test]
fn cloud_plan_refuses_an_endpoint_that_could_break_the_shell() {
    let project = vite_project();
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--json",
            "--project-id",
            "acme/web",
            "--api-endpoint",
            r#"https://x"; id; echo ""#,
        ],
        project.path(),
    );
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(output.stderr.contains("API endpoint"), "{}", output.stderr);
}

#[test]
fn cloud_plan_sends_every_step_to_a_custom_endpoint() {
    let project = vite_project();
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--json",
            "--project-id",
            "acme/web",
            "--api-endpoint",
            "https://staging.example.test",
        ],
        project.path(),
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let steps = json["cloud"]["ci"]["steps"].as_str().unwrap();
    assert!(
        steps.contains("upload-inventory --project-id=acme/web --git-sha=\"$GITHUB_SHA\" --api-endpoint=https://staging.example.test"),
        "{steps}"
    );
    assert!(
        steps.contains("--endpoint=https://staging.example.test"),
        "{steps}"
    );
    assert!(
        steps.contains("https://staging.example.test/v1/deployments"),
        "{steps}"
    );
}

#[test]
fn cloud_plan_refuses_an_invalid_explicit_sha() {
    let project = vite_project();
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--json",
            "--project-id",
            "acme/web",
            "--git-sha",
            "<deployed-sha>",
        ],
        project.path(),
    );
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(output.stderr.contains("git sha"), "{}", output.stderr);
}

#[test]
fn cloud_plan_uses_a_shell_safe_placeholder_without_a_project_id() {
    let project = vite_project();
    let output = run(&["coverage", "setup", "--cloud", "--json"], project.path());
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert!(json["cloud"]["project_id"].is_null());
    let steps = json["cloud"]["ci"]["steps"].as_str().unwrap();
    assert!(steps.contains("--project-id=OWNER/REPO"), "{steps}");
    assert!(!steps.contains('<'), "{steps}");
}

#[test]
fn cloud_plan_for_nextjs_has_no_browser_key_or_build_variable() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"site","scripts":{"build":"next build","start":"next start"},"dependencies":{"next":"15.0.0"}}"#,
    )
    .expect("write package.json");
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--json",
            "--project-id",
            "acme/site",
        ],
        dir.path(),
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert!(json["cloud"]["browser_key_env"].is_null());
    let steps = json["cloud"]["ci"]["steps"].as_str().unwrap();
    assert!(!steps.contains("VITE_"), "{steps}");
    assert!(
        !json["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("browser beacon")),
        "{}",
        json["warnings"]
    );
    assert_eq!(
        json["cloud"]["source_maps"]["dirs"],
        serde_json::json!([".next"])
    );
}

#[test]
fn wait_stops_at_once_on_a_scope_error() {
    let project = vite_project();
    let (endpoint, captured, handle) = serve(vec![(
        403,
        r#"{"error":true,"message":"This key cannot read","code":"scope_insufficient"}"#.to_owned(),
    )]);
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--wait",
            "--json",
            "--project-id",
            "acme/web",
            "--git-sha",
            SHA,
            "--api-endpoint",
            &endpoint,
        ],
        project.path(),
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert_eq!(captured.lock().unwrap().len(), 1);
    assert_eq!(parse_json(&output)["cloud"]["wait"]["outcome"], "error");
}

#[test]
fn wait_retries_a_server_error_and_then_completes() {
    let project = vite_project();
    let (endpoint, captured, handle) = serve(vec![
        (
            500,
            r#"{"error":true,"message":"boom","code":"internal_error"}"#.to_owned(),
        ),
        (200, activation_body(true)),
    ]);
    let output = run(
        &[
            "coverage",
            "setup",
            "--cloud",
            "--wait",
            "--wait-timeout",
            "60",
            "--json",
            "--project-id",
            "acme/web",
            "--git-sha",
            SHA,
            "--api-endpoint",
            &endpoint,
        ],
        project.path(),
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(captured.lock().unwrap().len(), 2);
    assert_eq!(parse_json(&output)["cloud"]["wait"]["outcome"], "complete");
}
