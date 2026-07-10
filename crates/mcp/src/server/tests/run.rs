#[cfg(unix)]
use rmcp::model::*;
#[cfg(any(unix, windows))]
use std::time::Duration;

#[cfg(windows)]
const WINDOWS_FIXTURE_STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

use crate::tools::run_fallow;
#[cfg(any(unix, windows))]
use crate::tools::run_fallow_with_timeout;
#[cfg(unix)]
use crate::tools::{run_fallow_with_output_limit, run_fallow_with_top_level_warnings};

use super::super::resolve_binary;

/// Extract the text content from a `CallToolResult`.
#[cfg(unix)]
fn extract_text(result: &CallToolResult) -> &str {
    match &result.content[0] {
        ContentBlock::Text(t) => &t.text,
        _ => panic!("expected text content"),
    }
}

#[cfg(unix)]
struct ProcessCleanup(Vec<u32>);

#[cfg(unix)]
impl Drop for ProcessCleanup {
    fn drop(&mut self) {
        for pid in &self.0 {
            drop(
                std::process::Command::new("kill")
                    .args(["-KILL", &pid.to_string()])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status(),
            );
        }
    }
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(windows)]
struct ProcessCleanup(Vec<u32>);

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows test cleanup requires process FFI calls"
)]
impl Drop for ProcessCleanup {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_TERMINATE, TerminateProcess,
        };

        for pid in &self.0 {
            // SAFETY: The PID came from a subprocess started by this test.
            let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, *pid) };
            if process.is_null() {
                continue;
            }
            // SAFETY: The handle grants PROCESS_TERMINATE and remains live.
            unsafe { TerminateProcess(process, 1) };
            // SAFETY: The handle is owned by this loop iteration.
            unsafe { CloseHandle(process) };
        }
    }
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows process existence checks require FFI calls"
)]
fn process_exists(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: OpenProcess validates the PID and returns null when it no longer exists.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return false;
    }
    let mut exit_code = 0;
    // SAFETY: `process` is live and `exit_code` is valid writable storage.
    let queried = unsafe { GetExitCodeProcess(process, &raw mut exit_code) };
    // SAFETY: The handle is owned by this function.
    unsafe { CloseHandle(process) };
    queried != 0 && u32::try_from(STILL_ACTIVE).is_ok_and(|active| exit_code == active)
}

#[cfg(any(unix, windows))]
async fn read_pid(path: &std::path::Path) -> u32 {
    for _ in 0..50 {
        if let Ok(pid) = std::fs::read_to_string(path)
            && let Ok(pid) = pid.trim().parse()
        {
            return pid;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for PID file {}", path.display());
}

#[cfg(any(unix, windows))]
async fn wait_for_process_exit(pid: u32) -> bool {
    for _ in 0..200 {
        if !process_exists(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

#[tokio::test]
async fn run_fallow_missing_binary() {
    let result = run_fallow("nonexistent-binary-12345", &["dead-code".to_string()]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("nonexistent-binary-12345"));
    assert!(err.message.contains("FALLOW_BIN"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_0_with_stdout() {
    let result = run_fallow(
        "/bin/sh",
        &["-c".to_string(), "echo '{\"ok\":true}'".to_string()],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(false));
    let text = extract_text(&result);
    assert!(text.contains(r#"{"ok":true}"#));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_0_empty_stdout_returns_empty_json() {
    let result = run_fallow("/bin/sh", &["-c".to_string(), "true".to_string()])
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_text(&result), "{}");
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_with_top_level_warnings_inserts_empty_array() {
    let result = run_fallow_with_top_level_warnings(
        "/bin/sh",
        &[
            "-c".to_string(),
            "echo '{\"schema_version\":4,\"runtime_coverage\":{\"schema_version\":\"1\"}}'"
                .to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(false));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("should be valid JSON");
    assert_eq!(parsed["warnings"], serde_json::json!([]));
    assert_eq!(parsed["runtime_coverage"]["schema_version"], "1");
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_1_treated_as_success_with_issues() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            "echo '{\"issues\":[]}'; exit 1".to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(false));
    let text = extract_text(&result);
    assert!(text.contains("issues"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_1_empty_stdout_returns_empty_json() {
    let result = run_fallow("/bin/sh", &["-c".to_string(), "exit 1".to_string()])
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_text(&result), "{}");
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_2_with_stderr_returns_structured_json_error() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            "echo 'invalid config' >&2; exit 2".to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("error should be valid JSON");
    assert_eq!(parsed["error"], true);
    assert_eq!(parsed["exit_code"], 2);
    assert!(
        parsed["message"]
            .as_str()
            .unwrap()
            .contains("invalid config")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_2_empty_stderr_returns_structured_json_error() {
    let result = run_fallow("/bin/sh", &["-c".to_string(), "exit 2".to_string()])
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("error should be valid JSON");
    assert_eq!(parsed["error"], true);
    assert_eq!(parsed["exit_code"], 2);
    assert!(
        parsed["message"]
            .as_str()
            .unwrap()
            .contains("exited with code 2")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_high_exit_code_returns_error() {
    let result = run_fallow("/bin/sh", &["-c".to_string(), "exit 127".to_string()])
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("error should be valid JSON");
    assert_eq!(parsed["exit_code"], 127);
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_stderr_is_trimmed_in_error_message() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            "echo '  whitespace around  ' >&2; exit 3".to_string(),
        ],
    )
    .await
    .unwrap();
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("error should be valid JSON");
    let msg = parsed["message"].as_str().unwrap();
    assert!(msg.ends_with("whitespace around"));
}

#[test]
#[expect(unsafe_code, reason = "env var mutation requires unsafe")]
fn resolve_binary_behavior() {
    // SAFETY: These tests intentionally mutate the process environment to
    // prove the binary resolver respects the override and reset paths.
    unsafe { std::env::remove_var("FALLOW_BIN") };
    let bin = resolve_binary();
    assert!(bin.contains("fallow"));

    // SAFETY: Restore the override to validate that resolve_binary reads the
    // custom path and does not cache the prior unset state.
    unsafe { std::env::set_var("FALLOW_BIN", "/custom/path/fallow") };
    let bin = resolve_binary();
    assert_eq!(bin, "/custom/path/fallow");

    // SAFETY: Leave the environment clean for the rest of the test suite.
    unsafe { std::env::remove_var("FALLOW_BIN") };
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_killed_by_signal_returns_error_with_negative_code() {
    let result = run_fallow("/bin/sh", &["-c".to_string(), "kill -9 $$".to_string()])
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("error should be valid JSON");
    assert_eq!(parsed["exit_code"], -1);
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_1_with_stderr_returns_stdout_not_stderr() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            "echo '{\"issues\":1}'; echo 'debug warning' >&2; exit 1".to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(false));
    let text = extract_text(&result);
    assert!(text.contains("issues"));
    assert!(!text.contains("debug warning"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_multiline_stdout() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            "echo 'line1'; echo 'line2'; echo 'line3'".to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(false));
    let text = extract_text(&result);
    assert!(text.contains("line1"));
    assert!(text.contains("line2"));
    assert!(text.contains("line3"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_empty_args() {
    let result = run_fallow("/bin/sh", &["-c".to_string(), "echo ok".to_string()])
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    let text = extract_text(&result);
    assert!(text.contains("ok"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_multiline_stderr_in_error() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            "echo 'error line 1' >&2; echo 'error line 2' >&2; exit 2".to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("error should be valid JSON");
    let msg = parsed["message"].as_str().unwrap();
    assert!(msg.contains("error line 1"));
    assert!(msg.contains("error line 2"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_result_has_single_content_item() {
    let success = run_fallow("/bin/sh", &["-c".to_string(), "echo test".to_string()])
        .await
        .unwrap();
    assert_eq!(success.content.len(), 1);

    let error = run_fallow("/bin/sh", &["-c".to_string(), "exit 2".to_string()])
        .await
        .unwrap();
    assert_eq!(error.content.len(), 1);

    let issues = run_fallow("/bin/sh", &["-c".to_string(), "exit 1".to_string()])
        .await
        .unwrap();
    assert_eq!(issues.content.len(), 1);
}

#[tokio::test]
async fn run_fallow_missing_binary_error_includes_install_hint() {
    let result = run_fallow("nonexistent-binary-xyz", &[]).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.message.contains("Ensure fallow is installed"),
        "error should include install hint"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_unicode_in_stdout() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            "echo '{\"file\":\"ソース/コード.ts\"}'".to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(false));
    let text = extract_text(&result);
    assert!(text.contains("ソース/コード.ts"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_unicode_in_stderr_error() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            "echo 'Fehler: ungültige Konfiguration' >&2; exit 2".to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("error should be valid JSON");
    let msg = parsed["message"].as_str().unwrap();
    assert!(msg.contains("ungültige Konfiguration"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_255() {
    let result = run_fallow("/bin/sh", &["-c".to_string(), "exit 255".to_string()])
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("error should be valid JSON");
    assert_eq!(parsed["exit_code"], 255);
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_large_stderr_in_error() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            "for i in $(seq 1 100); do echo \"error line $i\" >&2; done; exit 2".to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("error should be valid JSON");
    let msg = parsed["message"].as_str().unwrap();
    assert!(msg.contains("error line 1"));
    assert!(msg.contains("error line 100"));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_stdout_preserves_content() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            r#"printf '{"key": "value"}\n'"#.to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(false));
    let text = extract_text(&result);
    assert!(text.contains(r#""key": "value""#));
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_1_only_stderr_returns_empty_json() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            "echo 'some warning' >&2; exit 1".to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_text(&result), "{}");
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_stdin_is_not_inherited() {
    let result = run_fallow(
        "/bin/sh",
        &["-c".to_string(), "cat < /dev/null".to_string()],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert_eq!(extract_text(&result), "{}");
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_timeout_returns_mcp_error() {
    let result = run_fallow_with_timeout(
        "/bin/sh",
        &["-c".to_string(), "sleep 10".to_string()],
        Duration::from_millis(20),
    )
    .await
    .expect("timeout should stay a tool result");

    assert_eq!(result.is_error, Some(true));
    let body: serde_json::Value =
        serde_json::from_str(extract_text(&result)).expect("timeout body is JSON");
    assert_eq!(body["error"], true);
    assert_eq!(body["exit_code"], 2);
    assert_eq!(body["code"], "FALLOW_MCP_SUBPROCESS_TIMEOUT");
    assert!(
        body["help"]
            .as_str()
            .is_some_and(|help| help.contains("FALLOW_TIMEOUT_SECS"))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_timeout_terminates_and_reaps_process_tree() {
    let temp = tempfile::tempdir().expect("temp directory");
    let direct_pid_path = temp.path().join("direct.pid");
    let descendant_pid_path = temp.path().join("descendant.pid");
    let script = r#"echo $$ > "$1"; sleep 30 & echo $! > "$2"; exec sleep 30"#;

    let result = run_fallow_with_timeout(
        "/bin/sh",
        &[
            "-c".to_string(),
            script.to_string(),
            "fallow-mcp-timeout-test".to_string(),
            direct_pid_path.to_string_lossy().into_owned(),
            descendant_pid_path.to_string_lossy().into_owned(),
        ],
        Duration::from_millis(500),
    )
    .await
    .expect("timeout should stay a tool result");

    assert_eq!(result.is_error, Some(true));
    let direct_pid = read_pid(&direct_pid_path).await;
    let descendant_pid = read_pid(&descendant_pid_path).await;
    let _cleanup = ProcessCleanup(vec![direct_pid, descendant_pid]);
    let direct_exited = wait_for_process_exit(direct_pid).await;
    let descendant_exited = wait_for_process_exit(descendant_pid).await;

    assert!(
        direct_exited && descendant_exited,
        "timed out process tree survived: direct={direct_pid} alive={}, descendant={descendant_pid} alive={}",
        process_exists(direct_pid),
        process_exists(descendant_pid),
    );
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_timeout_is_not_held_open_by_escaped_pipe_writer() {
    let temp = tempfile::tempdir().expect("temp directory");
    let escaped_pid_path = temp.path().join("escaped.pid");
    let script = r#"python3 -c 'import os,sys,time; os.setsid(); p=open(sys.argv[1], "w"); p.write(str(os.getpid())); p.close(); time.sleep(30)' "$1" & exec sleep 30"#;
    let started = std::time::Instant::now();

    let result = run_fallow_with_timeout(
        "/bin/sh",
        &[
            "-c".to_string(),
            script.to_string(),
            "fallow-mcp-escaped-writer-test".to_string(),
            escaped_pid_path.to_string_lossy().into_owned(),
        ],
        Duration::from_secs(1),
    )
    .await
    .expect("timeout should stay a tool result");

    let escaped_pid = read_pid(&escaped_pid_path).await;
    let _cleanup = ProcessCleanup(vec![escaped_pid]);
    assert_eq!(result.is_error, Some(true));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "escaped inherited pipe writer suppressed the timeout result"
    );
    assert!(
        process_exists(escaped_pid),
        "fixture process should have escaped the dedicated process group"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn run_fallow_timeout_terminates_and_reaps_windows_job_tree() {
    let temp = tempfile::tempdir().expect("temp directory");
    let direct_pid_path = temp.path().join("direct.pid");
    let descendant_pid_path = temp.path().join("descendant.pid");
    let script_path = temp.path().join("process-tree-fixture.ps1");
    let script = r"
param(
    [Parameter(Mandatory = $true)][string]$DirectPidPath,
    [Parameter(Mandatory = $true)][string]$DescendantPidPath
)
$PID | Set-Content -NoNewline -LiteralPath $DirectPidPath
$child = Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30' -PassThru
$child.Id | Set-Content -NoNewline -LiteralPath $DescendantPidPath
Start-Sleep -Seconds 30
";
    std::fs::write(&script_path, script).expect("PowerShell fixture script");

    let result = run_fallow_with_timeout(
        "powershell.exe",
        &[
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-ExecutionPolicy".to_string(),
            "Bypass".to_string(),
            "-File".to_string(),
            script_path.to_string_lossy().into_owned(),
            "-DirectPidPath".to_string(),
            direct_pid_path.to_string_lossy().into_owned(),
            "-DescendantPidPath".to_string(),
            descendant_pid_path.to_string_lossy().into_owned(),
        ],
        WINDOWS_FIXTURE_STARTUP_TIMEOUT,
    )
    .await
    .expect("timeout should stay a tool result");

    assert_eq!(result.is_error, Some(true));
    let direct_pid = read_pid(&direct_pid_path).await;
    let descendant_pid = read_pid(&descendant_pid_path).await;
    let _cleanup = ProcessCleanup(vec![direct_pid, descendant_pid]);
    let direct_exited = wait_for_process_exit(direct_pid).await;
    let descendant_exited = wait_for_process_exit(descendant_pid).await;

    assert!(
        direct_exited && descendant_exited,
        "timed out Windows job survived: direct={direct_pid} alive={}, descendant={descendant_pid} alive={}",
        process_exists(direct_pid),
        process_exists(descendant_pid),
    );
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_output_limit_returns_structured_error() {
    let script = r#"i=0; while [ "$i" -lt 2048 ]; do printf x; i=$((i + 1)); done"#;
    let result =
        run_fallow_with_output_limit("/bin/sh", &["-c".to_string(), script.to_string()], 1024)
            .await
            .expect("output limit should stay a tool result");

    assert_eq!(result.is_error, Some(true));
    let body: serde_json::Value =
        serde_json::from_str(extract_text(&result)).expect("output-limit body is JSON");
    assert_eq!(body["error"], true);
    assert_eq!(body["exit_code"], 2);
    assert_eq!(body["code"], "FALLOW_MCP_SUBPROCESS_OUTPUT_LIMIT");
    assert_eq!(body["limit_bytes"], 1024);
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_2_with_json_stdout_passes_through() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            r#"echo '{"error":true,"message":"config not found","exit_code":2}'; exit 2"#
                .to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("should be valid JSON");
    assert_eq!(parsed["error"], true);
    assert_eq!(parsed["message"], "config not found");
    assert_eq!(parsed["exit_code"], 2);
}

#[cfg(unix)]
#[tokio::test]
async fn run_fallow_exit_code_2_prefers_json_stdout_over_stderr() {
    let result = run_fallow(
        "/bin/sh",
        &[
            "-c".to_string(),
            r#"echo '{"error":true,"message":"structured error","exit_code":2}'; echo 'raw stderr msg' >&2; exit 2"#.to_string(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = extract_text(&result);
    let parsed: serde_json::Value = serde_json::from_str(text).expect("should be valid JSON");
    assert_eq!(parsed["message"], "structured error");
    assert!(!text.contains("raw stderr msg"));
}
