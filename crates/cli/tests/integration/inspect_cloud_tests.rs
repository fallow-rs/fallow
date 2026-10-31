#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

//! `fallow inspect --file <path> --cloud`, `--format compact` and
//! `fallow cloud login` / `logout`, against a mock Fallow Cloud. No test
//! reaches the network.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use fallow_cov_protocol::function_identity_id;
use tempfile::{TempDir, tempdir};

use crate::common::{CommandOutput, fallow_bin, parse_json};
use crate::http_stub::read_request;

type Captured = Arc<Mutex<Vec<String>>>;

/// Serve the scripted `(status, body)` answers in order, one connection each,
/// and capture every request.
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

/// A project with three functions in `src/api.ts`:
/// `loadUser` (line 1), `parseRow` (line 2) and `newHelper` (line 3).
fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"inspect-cloud-fixture","type":"module"}"#,
    )
    .unwrap();
    std::fs::write(root.join(".fallowrc.json"), r#"{"entry":["src/index.ts"]}"#).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { loadUser, parseRow, newHelper } from './api';\nexport const boot = () => [loadUser('1'), parseRow('a'), newHelper()];\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/api.ts"),
        "export function loadUser(id: string) { return id; }\nexport function parseRow(row: string) { return row; }\nexport function newHelper() { return 1; }\n",
    )
    .unwrap();
}

/// A file-context body: `loadUser` with the stable id of its local line,
/// `parseRow` deployed at another line (so only the name ordinal pairs it),
/// and `removedFn`, which the local file no longer has.
fn file_context_body() -> String {
    let load_user = function_identity_id("src/api.ts", "loadUser", 1);
    let parse_row_old = function_identity_id("src/api.ts", "parseRow", 9);
    let removed = function_identity_id("src/api.ts", "removedFn", 20);
    format!(
        r#"{{"data":{{
      "schema_version":"fallow-cloud-file-context-v1",
      "repo":"acme/app","path":"src/api.ts","state":"observed",
      "latest_sha":"abc123","period_days":30,
      "generated_at":"2026-10-29T10:00:00.000Z",
      "actionable":true,"actionability_reason":null,"never_called_withheld":false,
      "summary":{{"functions":3,"called":1,"never_called":1,"untracked":1,
        "calls":4120,"error_groups":1,"error_events":7,
        "text":"3 functions: 1 called, 1 never called, 1 untracked."}},
      "functions":[
        {{"name":"loadUser","line":1,"end_line":1,"stable_id":"{load_user}",
          "state":"called","period_state":"called","calls":4120,
          "deployments_observed":2,"last_seen_at":null,
          "errors":{{"state":"tracked","groups":1,"events":7,"top":null}},
          "cpu":{{"state":"tracked","self_ms_per_hour":812.5,"total_ms_per_hour":900,
            "share_of_process":0.1,"samples":40}},
          "browser":null,
          "branches":{{"state":"observed","ranges_total":4,"ranges_never_run":2,
            "never_run":[],"never_run_truncated":false,"function_calls":4120}},
          "static_callers":null,"production_callers":null,
          "summary":"loadUser ran 4120 times."}},
        {{"name":"parseRow","line":9,"end_line":9,"stable_id":"{parse_row_old}",
          "state":"never_called","period_state":"never_called","calls":0,
          "deployments_observed":2,"last_seen_at":null,
          "errors":{{"state":"untracked","groups":null,"events":null,"top":null}},
          "cpu":{{"state":"untracked","self_ms_per_hour":null,"total_ms_per_hour":null,
            "share_of_process":null,"samples":null}},
          "browser":null,"branches":null,
          "static_callers":null,"production_callers":null,
          "summary":"parseRow did not run in 30 days."}},
        {{"name":"removedFn","line":20,"end_line":20,"stable_id":"{removed}",
          "state":"untracked","period_state":"untracked","calls":null,
          "deployments_observed":0,"last_seen_at":null,"errors":null,"cpu":null,
          "browser":null,"branches":null,"static_callers":null,
          "production_callers":null,"summary":"removedFn has no data."}}
      ],
      "warnings":[],"context_only":true}}}}"#
    )
}

struct Env<'a> {
    api_url: Option<&'a str>,
    api_key: Option<&'a str>,
    home: &'a Path,
}

fn run(args: &[&str], root: &Path, env: &Env<'_>) -> CommandOutput {
    run_with(args, root, env, |_| {})
}

/// [`run`] with a last change to the command, for example to give it a
/// keychain.
fn run_with(
    args: &[&str],
    root: &Path,
    env: &Env<'_>,
    customize: impl FnOnce(&mut Command),
) -> CommandOutput {
    let mut command = Command::new(fallow_bin());
    command
        .args(args)
        .arg("--root")
        .arg(root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_AGENT_SOURCE", "none")
        .env("FALLOW_CREDENTIAL_STORE", "file")
        .env("HOME", env.home)
        .env("USERPROFILE", env.home)
        .env("XDG_CONFIG_HOME", env.home.join(".config"))
        .env_remove("FALLOW_REPO")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_CHILD_SESSION");
    match env.api_url {
        Some(url) => command.env("FALLOW_API_URL", url),
        None => command.env_remove("FALLOW_API_URL"),
    };
    match env.api_key {
        Some(key) => command.env("FALLOW_API_KEY", key),
        None => command.env_remove("FALLOW_API_KEY"),
    };
    customize(&mut command);
    let output = command.output().expect("run fallow");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn project() -> (TempDir, TempDir) {
    let dir = tempdir().unwrap();
    write_project(dir.path());
    (dir, tempdir().unwrap())
}

const INSPECT_JSON: &[&str] = &[
    "inspect",
    "--file",
    "src/api.ts",
    "--cloud",
    "--repo",
    "acme/app",
    "--format",
    "json",
    "--quiet",
];

#[test]
fn inspect_cloud_pairs_functions_by_stable_id_then_name_ordinal() {
    let (dir, home) = project();
    let (endpoint, captured, handle) = serve(vec![(200, file_context_body())]);
    let output = run(
        INSPECT_JSON,
        dir.path(),
        &Env {
            api_url: Some(&endpoint),
            api_key: Some("fallow_read_k1_test"),
            home: home.path(),
        },
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);

    let request = captured.lock().unwrap()[0].clone();
    assert!(
        request.starts_with("GET /v1/coverage/acme%2Fapp/file-context?path=src%2Fapi.ts"),
        "request line: {request}"
    );
    assert!(
        request.lines().next().unwrap().contains("maxBytes=200000"),
        "the full body is requested, so the cloud cap drops no function: {request}"
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer fallow_read_k1_test"),
        "the resolved key must be sent: {request}"
    );

    let json = parse_json(&output);
    let cloud = &json["evidence"]["cloud"];
    assert_eq!(cloud["status"], "ok");
    assert!(cloud["message"].is_null());
    assert_eq!(cloud["repo"], "acme/app");
    assert_eq!(cloud["latest_sha"], "abc123");
    assert_eq!(cloud["period_days"], 30);
    assert_eq!(cloud["summary"]["calls"], 4120);
    assert_eq!(cloud["summary"]["error_events"], 7);

    let functions = cloud["functions"].as_array().unwrap();
    assert_eq!(functions.len(), 2, "two local functions pair: {cloud}");
    assert_eq!(functions[0]["name"], "loadUser");
    assert_eq!(functions[0]["line"], 1);
    assert_eq!(functions[0]["match"], "stable_id");
    assert_eq!(functions[0]["state"], "called");
    assert_eq!(functions[0]["calls"], 4120);
    assert_eq!(functions[0]["errors"]["groups"], 1);
    assert_eq!(functions[0]["errors"]["events"], 7);
    assert_eq!(functions[0]["cpu_ms_per_hour"], 812.5);
    assert_eq!(functions[0]["never_run_ranges"], 2);
    assert_eq!(
        functions[0]["stable_id"],
        function_identity_id("src/api.ts", "loadUser", 1)
    );

    assert_eq!(functions[1]["name"], "parseRow");
    assert_eq!(functions[1]["line"], 2, "the local line is reported");
    assert_eq!(functions[1]["match"], "name_ordinal");
    assert_eq!(functions[1]["state"], "never_called");
    assert!(
        functions[1]["errors"].is_null(),
        "untracked errors are null, never zero"
    );
    assert!(functions[1]["cpu_ms_per_hour"].is_null());
    assert!(functions[1]["never_run_ranges"].is_null());

    let unpaired = cloud["unpaired_local"].as_array().unwrap();
    assert_eq!(unpaired.len(), 1);
    assert_eq!(unpaired[0]["name"], "newHelper");
    assert_eq!(unpaired[0]["line"], 3);
    assert_eq!(
        unpaired[0]["note"],
        "no production data (new or moved since deploy)"
    );
}

#[test]
fn inspect_cloud_without_a_key_reports_no_key_and_succeeds() {
    let (dir, home) = project();
    let output = run(
        INSPECT_JSON,
        dir.path(),
        &Env {
            api_url: Some("http://127.0.0.1:9"),
            api_key: None,
            home: home.path(),
        },
    );
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    let json = parse_json(&output);
    let cloud = &json["evidence"]["cloud"];
    assert_eq!(cloud["status"], "no_key");
    assert!(
        cloud["message"]
            .as_str()
            .unwrap()
            .contains("fallow cloud login"),
        "{cloud}"
    );
    assert!(cloud["functions"].as_array().unwrap().is_empty());
}

/// Give the command a keychain: a fake `secret-tool` that never answers, as a
/// locked keychain does while it shows an unlock prompt. Linux only, because
/// on macOS the keychain tool has a fixed path.
#[cfg(target_os = "linux")]
fn hanging_keychain(tools: &Path) -> impl FnOnce(&mut Command) {
    use std::os::unix::fs::PermissionsExt as _;
    let tool = tools.join("secret-tool");
    std::fs::write(&tool, "#!/bin/sh\nexec sleep 60\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(tools.to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    move |command: &mut Command| {
        command
            .env_remove("FALLOW_CREDENTIAL_STORE")
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
            .env("PATH", path);
    }
}

/// The keychain read has a time limit of about one second. The bound here is
/// far below the 60 s of the fake tool and leaves room for a slow CI runner.
#[cfg(target_os = "linux")]
const HUNG_KEYCHAIN_BOUND: std::time::Duration = std::time::Duration::from_secs(20);

#[cfg(target_os = "linux")]
#[test]
fn inspect_cloud_with_a_hanging_keychain_reports_no_key_within_the_limit() {
    let (dir, home) = project();
    let tools = tempdir().unwrap();
    let start = std::time::Instant::now();
    let output = run_with(
        INSPECT_JSON,
        dir.path(),
        &Env {
            api_url: Some("http://127.0.0.1:9"),
            api_key: None,
            home: home.path(),
        },
        hanging_keychain(tools.path()),
    );
    let elapsed = start.elapsed();
    assert!(elapsed < HUNG_KEYCHAIN_BOUND, "took {elapsed:?}");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    assert_eq!(parse_json(&output)["evidence"]["cloud"]["status"], "no_key");
}

#[cfg(target_os = "linux")]
#[test]
fn inspect_cloud_with_a_hanging_keychain_uses_the_credentials_file() {
    let (dir, home) = project();
    let tools = tempdir().unwrap();
    let (endpoint, captured, handle) = serve(vec![(200, file_context_body())]);
    let config = home.path().join(".config").join("fallow");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("credentials"),
        format!(r#"{{"version":1,"keys":{{"{endpoint}":"fallow_read_k1_fromfile"}}}}"#),
    )
    .unwrap();
    let start = std::time::Instant::now();
    let output = run_with(
        INSPECT_JSON,
        dir.path(),
        &Env {
            api_url: Some(&endpoint),
            api_key: None,
            home: home.path(),
        },
        hanging_keychain(tools.path()),
    );
    handle.join().expect("server joins");
    let elapsed = start.elapsed();
    assert!(elapsed < HUNG_KEYCHAIN_BOUND, "took {elapsed:?}");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    assert_eq!(parse_json(&output)["evidence"]["cloud"]["status"], "ok");
    let request = captured.lock().unwrap()[0].to_ascii_lowercase();
    assert!(
        request.contains("authorization: bearer fallow_read_k1_fromfile"),
        "the key of the credentials file is sent: {request}"
    );
}

#[test]
fn inspect_cloud_maps_the_tier_gate_to_plan_required() {
    let (dir, home) = project();
    let (endpoint, _captured, handle) = serve(vec![(
        403,
        r#"{"error":true,"message":"Upgrade","code":"tier_required"}"#.to_owned(),
    )]);
    let output = run(
        INSPECT_JSON,
        dir.path(),
        &Env {
            api_url: Some(&endpoint),
            api_key: Some("fallow_read_k1_test"),
            home: home.path(),
        },
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    assert_eq!(
        parse_json(&output)["evidence"]["cloud"]["status"],
        "plan_required"
    );
}

#[test]
fn inspect_cloud_reports_an_unreachable_cloud_as_unavailable() {
    let (dir, home) = project();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let output = run(
        INSPECT_JSON,
        dir.path(),
        &Env {
            api_url: Some(&endpoint),
            api_key: Some("fallow_read_k1_test"),
            home: home.path(),
        },
    );
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    assert_eq!(
        parse_json(&output)["evidence"]["cloud"]["status"],
        "unavailable"
    );
}

#[test]
fn inspect_cloud_reports_an_untracked_file() {
    let (dir, home) = project();
    let body = r#"{"data":{"schema_version":"fallow-cloud-file-context-v1","repo":"acme/app","path":"src/api.ts","state":"untracked","latest_sha":null,"period_days":30,"generated_at":"2026-10-29T10:00:00.000Z","actionable":false,"actionability_reason":null,"never_called_withheld":true,"summary":{"functions":0,"called":0,"never_called":0,"untracked":0,"calls":null,"error_groups":null,"error_events":null,"text":"No production data for this file."},"functions":[],"warnings":[],"context_only":true}}"#;
    let (endpoint, _captured, handle) = serve(vec![(200, body.to_owned())]);
    let output = run(
        INSPECT_JSON,
        dir.path(),
        &Env {
            api_url: Some(&endpoint),
            api_key: Some("fallow_read_k1_test"),
            home: home.path(),
        },
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    let cloud = &parse_json(&output)["evidence"]["cloud"];
    assert_eq!(cloud["status"], "untracked");
    assert!(cloud["summary"]["calls"].is_null());
}

#[test]
fn inspect_compact_prints_one_line_per_item() {
    let (dir, home) = project();
    let (endpoint, _captured, handle) = serve(vec![(200, file_context_body())]);
    let output = run(
        &[
            "inspect",
            "--file",
            "src/api.ts",
            "--cloud",
            "--repo",
            "acme/app",
            "--format",
            "compact",
            "--quiet",
        ],
        dir.path(),
        &Env {
            api_url: Some(&endpoint),
            api_key: Some("fallow_read_k1_test"),
            home: home.path(),
        },
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    let lines: Vec<&str> = output.stdout.lines().collect();
    assert_eq!(lines[0], "fallow src/api.ts");
    assert!(
        lines.contains(
            &"prod loadUser:1 called 4120 calls, 1 error group (7 events), 812.5 ms CPU/h, 2 never-run branches"
        ),
        "{}",
        output.stdout
    );
    assert!(
        lines.contains(&"prod parseRow:2 never_called in 30 days"),
        "{}",
        output.stdout
    );
    assert!(
        lines.contains(&"prod newHelper:3 no production data (new or moved since deploy)"),
        "{}",
        output.stdout
    );
    assert!(!output.stdout.contains('\u{1b}'), "no colors");
    assert!(output.stdout.len() <= 1024, "{}", output.stdout.len());
}

#[test]
fn inspect_compact_names_a_non_ok_cloud_status() {
    let (dir, home) = project();
    let output = run(
        &[
            "inspect",
            "--file",
            "src/api.ts",
            "--cloud",
            "--repo",
            "acme/app",
            "--format",
            "compact",
        ],
        dir.path(),
        &Env {
            api_url: Some("http://127.0.0.1:9"),
            api_key: None,
            home: home.path(),
        },
    );
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    assert!(
        output
            .stdout
            .lines()
            .any(|line| line.starts_with("cloud no_key: ")),
        "{}",
        output.stdout
    );
}

#[test]
fn inspect_compact_without_cloud_prints_static_findings() {
    let (dir, home) = project();
    let output = run(
        &["inspect", "--file", "src/api.ts", "--format", "compact"],
        dir.path(),
        &Env {
            api_url: None,
            api_key: None,
            home: home.path(),
        },
    );
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    assert_eq!(output.stdout.lines().next(), Some("fallow src/api.ts"));
    assert!(!output.stdout.contains("prod "), "{}", output.stdout);
    assert!(!output.stdout.contains("cloud "), "{}", output.stdout);
}

const DEVICE_START: &str = r#"{"data":{"deviceCode":"dev-code","userCode":"ABCD-EFGH","verificationUri":"https://fallow.cloud/device","expiresIn":900,"interval":0}}"#;
const DEVICE_PENDING: &str =
    r#"{"error":true,"message":"Authorization pending","code":"authorization_pending"}"#;
const DEVICE_TOKEN: &str = r#"{"data":{"licenseJwt":"jwt","apiKey":{"id":"key_1","key":"fallow_read_k1_fromlogin","prefix":"fallow_read_k1_fro","type":"read","name":"fallow cloud login (ci-box)"}}}"#;

#[test]
fn cloud_login_stores_the_key_and_inspect_uses_it_until_logout() {
    let (dir, home) = project();
    let (endpoint, captured, handle) = serve(vec![
        (200, DEVICE_START.to_owned()),
        (400, DEVICE_PENDING.to_owned()),
        (200, DEVICE_TOKEN.to_owned()),
        (200, file_context_body()),
    ]);
    let env = Env {
        api_url: Some(&endpoint),
        api_key: None,
        home: home.path(),
    };
    let login = run(
        &["cloud", "login", "--machine-name", "ci-box"],
        dir.path(),
        &env,
    );
    assert_eq!(login.code, 0, "stderr={}", login.stderr);
    assert!(
        login.stderr.contains("ABCD-EFGH"),
        "the user code is shown: {}",
        login.stderr
    );
    assert!(
        !login.stdout.contains("fallow_read_k1_fromlogin")
            && !login.stderr.contains("fallow_read_k1_fromlogin"),
        "the key is never printed"
    );

    let credentials = home.path().join(".config/fallow/credentials");
    let stored = std::fs::read_to_string(&credentials).expect("credentials file");
    assert!(stored.contains("fallow_read_k1_fromlogin"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&credentials)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "credentials file mode");
    }

    let inspect = run(INSPECT_JSON, dir.path(), &env);
    handle.join().expect("server joins");
    assert_eq!(inspect.code, 0, "stderr={}", inspect.stderr);
    assert_eq!(parse_json(&inspect)["evidence"]["cloud"]["status"], "ok");

    let requests: Vec<String> = captured
        .lock()
        .unwrap()
        .iter()
        .map(|request| request.replace(char::is_whitespace, ""))
        .collect();
    assert!(
        requests[0].starts_with("POST/v1/auth/device"),
        "{}",
        requests[0]
    );
    assert!(
        requests[0].contains(r#""keyType":"read""#),
        "{}",
        requests[0]
    );
    assert!(
        requests[0].contains(r#""machineName":"ci-box""#),
        "{}",
        requests[0]
    );
    assert!(requests[1].starts_with("POST/v1/auth/device/token"));
    assert!(requests[1].contains(r#""deviceCode":"dev-code""#));
    assert!(
        requests[3]
            .to_ascii_lowercase()
            .contains("authorization:bearerfallow_read_k1_fromlogin"),
        "inspect uses the stored key"
    );

    let logout = run(&["cloud", "logout"], dir.path(), &env);
    assert_eq!(logout.code, 0, "stderr={}", logout.stderr);
    let after = std::fs::read_to_string(&credentials).unwrap_or_default();
    assert!(!after.contains("fallow_read_k1_fromlogin"), "{after}");

    let no_key = run(INSPECT_JSON, dir.path(), &env);
    assert_eq!(parse_json(&no_key)["evidence"]["cloud"]["status"], "no_key");
}

#[test]
fn environment_key_wins_over_the_stored_key() {
    let (dir, home) = project();
    let config = home.path().join(".config/fallow");
    std::fs::create_dir_all(&config).unwrap();
    let (endpoint, captured, handle) = serve(vec![(200, file_context_body())]);
    std::fs::write(
        config.join("credentials"),
        format!(r#"{{"version":1,"keys":{{"{endpoint}":"fallow_read_k1_stored"}}}}"#),
    )
    .unwrap();
    let output = run(
        INSPECT_JSON,
        dir.path(),
        &Env {
            api_url: Some(&endpoint),
            api_key: Some("fallow_live_k1_env"),
            home: home.path(),
        },
    );
    handle.join().expect("server joins");
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    let request = captured.lock().unwrap()[0].to_ascii_lowercase();
    assert!(
        request.contains("authorization: bearer fallow_live_k1_env"),
        "{request}"
    );
}

#[test]
fn inspect_cloud_without_a_repository_names_the_repo_flag() {
    let (dir, home) = project();
    let output = run(
        &[
            "inspect",
            "--file",
            "src/api.ts",
            "--cloud",
            "--format",
            "json",
            "--quiet",
        ],
        dir.path(),
        &Env {
            api_url: Some("http://127.0.0.1:9"),
            api_key: Some("fallow_read_k1_test"),
            home: home.path(),
        },
    );
    assert_eq!(output.code, 0, "stderr={}", output.stderr);
    let cloud = &parse_json(&output)["evidence"]["cloud"];
    assert_eq!(cloud["status"], "unavailable");
    let message = cloud["message"].as_str().unwrap();
    assert!(message.contains("--repo"), "{message}");
    assert!(!message.contains("coverage analyze"), "{message}");
}

#[test]
fn cloud_login_names_the_owner_rule_when_the_cloud_mints_no_key() {
    let (dir, home) = project();
    let token = r#"{"data":{"licenseJwt":"jwt","apiKeyError":{"code":"owner_required","message":"Only an organization owner can create an API key."}}}"#;
    let (endpoint, _captured, handle) = serve(vec![
        (200, DEVICE_START.to_owned()),
        (200, token.to_owned()),
    ]);
    let env = Env {
        api_url: Some(&endpoint),
        api_key: None,
        home: home.path(),
    };
    let login = run(
        &["cloud", "login", "--machine-name", "ci-box"],
        dir.path(),
        &env,
    );
    handle.join().expect("server joins");
    assert_eq!(
        login.code, 2,
        "stdout={} stderr={}",
        login.stdout, login.stderr
    );
    let text = format!("{}{}", login.stdout, login.stderr);
    assert!(
        text.contains("Only an organization owner can create an API key"),
        "{text}"
    );
    assert!(!text.contains("may not support"), "{text}");
    assert!(
        !home.path().join(".config/fallow/credentials").exists(),
        "no key is stored"
    );
}

#[test]
fn cloud_login_names_the_revoke_step_when_an_earlier_poll_took_the_key() {
    let (dir, home) = project();
    let claimed =
        r#"{"error":true,"code":"already_claimed","message":"This code was already used."}"#;
    let (endpoint, _captured, handle) = serve(vec![
        (200, DEVICE_START.to_owned()),
        (400, claimed.to_owned()),
    ]);
    let env = Env {
        api_url: Some(&endpoint),
        api_key: None,
        home: home.path(),
    };
    let login = run(
        &["cloud", "login", "--machine-name", "ci-box"],
        dir.path(),
        &env,
    );
    handle.join().expect("server joins");
    assert_eq!(
        login.code, 2,
        "stdout={} stderr={}",
        login.stdout, login.stderr
    );
    let text = format!("{}{}", login.stdout, login.stderr);
    assert!(text.contains("did not reach this machine"), "{text}");
    assert!(text.contains("fallow cloud login (ci-box)"), "{text}");
    assert!(text.contains("settings#api-keys"), "{text}");
    assert!(
        !home.path().join(".config/fallow/credentials").exists(),
        "no key is stored"
    );
}

#[test]
fn an_upload_does_not_use_the_read_only_key_of_cloud_login() {
    let (dir, home) = project();
    let config = home.path().join(".config/fallow");
    std::fs::create_dir_all(&config).unwrap();
    let endpoint = "http://127.0.0.1:9";
    std::fs::write(
        config.join("credentials"),
        format!(r#"{{"version":1,"keys":{{"{endpoint}":"fallow_read_k1_stored"}}}}"#),
    )
    .unwrap();
    let output = run(
        &[
            "coverage",
            "upload-inventory",
            "--project-id",
            "acme/app",
            "--git-sha",
            "abc1234",
            "--allow-dirty",
        ],
        dir.path(),
        &Env {
            api_url: Some(endpoint),
            api_key: None,
            home: home.path(),
        },
    );
    assert_ne!(output.code, 0, "stdout={}", output.stdout);
    let text = format!("{}{}", output.stdout, output.stderr);
    assert!(text.contains("no API key"), "{text}");
    assert!(text.contains("cannot upload"), "{text}");
}
