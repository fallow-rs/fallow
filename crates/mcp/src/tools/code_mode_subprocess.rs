use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use super::super::process_tree::{ProcessTree, cleanup_std_child, configure_std_command};

const STDERR_LIMIT_BYTES: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(super) fn run_fallow_sync(
    binary: &str,
    tool: &'static str,
    args: &[String],
    deadline: Instant,
    max_output_bytes: usize,
) -> Result<String, String> {
    let mut stdout_file = tempfile::NamedTempFile::new()
        .map_err(|err| format!("failed to create stdout temp file: {err}"))?;
    let mut stderr_file = tempfile::NamedTempFile::new()
        .map_err(|err| format!("failed to create stderr temp file: {err}"))?;
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdout(Stdio::from(stdout_file.reopen().map_err(|err| {
            format!("failed to reopen stdout temp file: {err}")
        })?))
        .stderr(Stdio::from(stderr_file.reopen().map_err(|err| {
            format!("failed to reopen stderr temp file: {err}")
        })?))
        .env("FALLOW_INTEGRATION_SURFACE", "mcp")
        .env("FALLOW_MCP_TOOL", tool);
    let (mut child, process_tree) = spawn_managed_child(command, binary)?;

    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                let cleanup_errors = cleanup_std_child(Some(&process_tree), &mut child);
                return Err(with_cleanup_errors(
                    format!("failed to wait for fallow subprocess: {error}"),
                    &cleanup_errors,
                ));
            }
        };
        if let Some(status) = status {
            let cleanup_errors = cleanup_std_child(Some(&process_tree), &mut child);
            let result = (|| {
                let stdout_len = file_len(stdout_file.as_file())?;
                if stdout_len > max_output_bytes as u64 {
                    return Err(format!(
                        "code mode host output exceeded {max_output_bytes} bytes"
                    ));
                }

                let stdout = read_file(stdout_file.as_file_mut(), "stdout")?;
                let stderr = read_limited_file(stderr_file.as_file_mut(), STDERR_LIMIT_BYTES)?;
                normalize_output(status.code().unwrap_or(-1), &stdout, &stderr)
            })();
            return with_completed_cleanup(result, &cleanup_errors);
        }

        if Instant::now() >= deadline {
            let cleanup_errors = cleanup_std_child(Some(&process_tree), &mut child);
            return Err(with_cleanup_errors(
                "code mode execution timed out while running fallow".to_string(),
                &cleanup_errors,
            ));
        }
        let stdout_len = match file_len(stdout_file.as_file()) {
            Ok(stdout_len) => stdout_len,
            Err(error) => {
                let cleanup_errors = cleanup_std_child(Some(&process_tree), &mut child);
                return Err(with_cleanup_errors(error, &cleanup_errors));
            }
        };
        if stdout_len > max_output_bytes as u64 {
            let cleanup_errors = cleanup_std_child(Some(&process_tree), &mut child);
            return Err(with_cleanup_errors(
                format!("code mode host output exceeded {max_output_bytes} bytes"),
                &cleanup_errors,
            ));
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn spawn_managed_child(
    mut command: Command,
    binary: &str,
) -> Result<(std::process::Child, ProcessTree), String> {
    configure_std_command(&mut command);
    let mut child = command.spawn().map_err(|err| {
        format!(
            "failed to execute fallow binary '{binary}': {err}. Ensure fallow is installed and available in PATH, or set FALLOW_BIN."
        )
    })?;
    let process_tree = match ProcessTree::for_std_child(&child) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let cleanup_errors = cleanup_std_child(None, &mut child);
            return Err(with_cleanup_errors(
                format!("failed to configure fallow subprocess tree: {error}"),
                &cleanup_errors,
            ));
        }
    };
    Ok((child, process_tree))
}

fn with_cleanup_errors(message: String, cleanup_errors: &[String]) -> String {
    if cleanup_errors.is_empty() {
        message
    } else {
        format!("{message}; cleanup errors: {}", cleanup_errors.join("; "))
    }
}

fn with_completed_cleanup<T>(
    result: Result<T, String>,
    cleanup_errors: &[String],
) -> Result<T, String> {
    match result {
        Ok(value) if cleanup_errors.is_empty() => Ok(value),
        Ok(_) => Err(with_cleanup_errors(
            "failed to clean completed fallow subprocess".to_string(),
            cleanup_errors,
        )),
        Err(error) => Err(with_structured_cleanup_errors(error, cleanup_errors)),
    }
}

fn with_structured_cleanup_errors(error: String, cleanup_errors: &[String]) -> String {
    if cleanup_errors.is_empty() {
        return error;
    }

    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&error) else {
        return with_cleanup_errors(error, cleanup_errors);
    };
    let Some(object) = value.as_object_mut() else {
        return with_cleanup_errors(error, cleanup_errors);
    };
    object.insert("cleanup_errors".to_string(), json!(cleanup_errors));
    serde_json::to_string(&value).unwrap_or_else(|_| with_cleanup_errors(error, cleanup_errors))
}

fn file_len(file: &fs::File) -> Result<u64, String> {
    file.metadata()
        .map(|metadata| metadata.len())
        .map_err(|err| format!("failed to inspect fallow output file: {err}"))
}

fn read_file(file: &mut fs::File, label: &str) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|err| format!("failed to rewind fallow {label}: {err}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| format!("failed to read fallow {label}: {err}"))?;
    Ok(bytes)
}

fn read_limited_file(file: &mut fs::File, limit: usize) -> Result<Vec<u8>, String> {
    let len = file_len(file)?;
    if len > limit as u64 {
        return Ok(format!("stderr exceeded {limit} bytes").into_bytes());
    }
    read_file(file, "stderr")
}

pub(super) fn normalize_output(
    exit_code: i32,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(stdout).to_string();
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();

    match exit_code {
        0 | 1 => Ok(if stdout.is_empty() {
            "{}".to_string()
        } else {
            stdout
        }),
        _ if !stdout.is_empty() && serde_json::from_str::<serde_json::Value>(&stdout).is_ok() => {
            Err(stdout)
        }
        _ => Err(json!({
            "error": true,
            "message": if stderr.is_empty() {
                format!("fallow exited with code {exit_code}")
            } else {
                stderr
            },
            "exit_code": exit_code
        })
        .to_string()),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    struct ProcessCleanup(u32);

    impl Drop for ProcessCleanup {
        fn drop(&mut self) {
            drop(
                Command::new("kill")
                    .args(["-KILL", &self.0.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status(),
            );
        }
    }

    fn process_exists(pid: u32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn wait_for_process_exit(pid: u32) -> bool {
        for _ in 0..200 {
            if !process_exists(pid) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn run_completed_subprocess(
        completion_script: &str,
        max_output_bytes: usize,
    ) -> Result<String, String> {
        let temp = tempfile::tempdir().expect("temp directory");
        let descendant_pid_path = temp.path().join("descendant.pid");
        let script = format!(r#"trap '' HUP; sleep 30 & echo $! > "$1"; {completion_script}"#);
        let args = [
            "-c".to_string(),
            script,
            "fallow-code-mode-completion-test".to_string(),
            descendant_pid_path.to_string_lossy().into_owned(),
        ];

        let result = run_fallow_sync(
            "/bin/sh",
            "test",
            &args,
            Instant::now() + Duration::from_secs(5),
            max_output_bytes,
        );
        let descendant_pid: u32 = fs::read_to_string(&descendant_pid_path)
            .expect("descendant PID file")
            .trim()
            .parse()
            .expect("descendant PID");
        let _cleanup = ProcessCleanup(descendant_pid);
        assert!(
            wait_for_process_exit(descendant_pid),
            "completed subprocess descendant {descendant_pid} survived"
        );
        result
    }

    #[test]
    fn completed_success_cleans_descendant_process_tree() {
        let output = run_completed_subprocess("printf '{}'", 1024)
            .expect("completed subprocess should preserve successful output");
        assert_eq!(output, "{}");
    }

    #[test]
    fn completed_nonzero_cleans_descendant_and_preserves_json_error() {
        let expected = r#"{"error":true,"message":"config error","exit_code":2}"#;
        let error = run_completed_subprocess(&format!("printf '{expected}'; exit 2"), 1024)
            .expect_err("nonzero subprocess should preserve JSON stdout as the error");
        assert_eq!(error, expected);
    }

    #[test]
    fn completed_output_overflow_cleans_descendant_and_preserves_error() {
        let error = run_completed_subprocess(
            r#"i=0; while [ "$i" -lt 32 ]; do printf x; i=$((i + 1)); done"#,
            16,
        )
        .expect_err("completed subprocess should enforce the output limit");
        assert_eq!(error, "code mode host output exceeded 16 bytes");
    }

    #[test]
    fn completed_cleanup_error_preserves_structured_subprocess_error() {
        let subprocess_error = r#"{"error":true,"message":"config error","exit_code":2}"#;
        let error = with_completed_cleanup::<String>(
            Err(subprocess_error.to_string()),
            &["descendant survived".to_string()],
        )
        .expect_err("cleanup failure should remain an error");
        let value: serde_json::Value = serde_json::from_str(&error)
            .expect("cleanup diagnostics must preserve the JSON envelope");

        assert_eq!(value["message"], "config error");
        assert_eq!(value["cleanup_errors"][0], "descendant survived");
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    struct ProcessCleanup(u32);

    impl Drop for ProcessCleanup {
        fn drop(&mut self) {
            drop(
                Command::new("taskkill")
                    .args(["/PID", &self.0.to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status(),
            );
        }
    }

    fn process_exists(pid: u32) -> bool {
        let pid = pid.to_string();
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .is_ok_and(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .any(|field| field == pid)
            })
    }

    fn wait_for_process_exit(pid: u32) -> bool {
        for _ in 0..200 {
            if !process_exists(pid) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn completed_success_cleans_descendant_process_tree() {
        let temp = tempfile::tempdir().expect("temp directory");
        let descendant_pid_path = temp.path().join("descendant.pid");
        let script_path = temp.path().join("process-tree-fixture.ps1");
        let script = r"
param(
    [Parameter(Mandatory = $true)][string]$DescendantPidPath
)
$child = Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 30' -PassThru
$child.Id | Set-Content -NoNewline -LiteralPath $DescendantPidPath
[Console]::Out.Write('{}')
";
        fs::write(&script_path, script).expect("PowerShell fixture script");
        let args = [
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-ExecutionPolicy".to_string(),
            "Bypass".to_string(),
            "-File".to_string(),
            script_path.to_string_lossy().into_owned(),
            "-DescendantPidPath".to_string(),
            descendant_pid_path.to_string_lossy().into_owned(),
        ];

        let output = run_fallow_sync(
            "powershell.exe",
            "test",
            &args,
            Instant::now() + Duration::from_secs(5),
            1024,
        )
        .expect("completed subprocess should preserve successful output");
        let descendant_pid: u32 = fs::read_to_string(&descendant_pid_path)
            .expect("descendant PID file")
            .trim()
            .parse()
            .expect("descendant PID");
        let _cleanup = ProcessCleanup(descendant_pid);

        assert_eq!(output, "{}");
        assert!(
            wait_for_process_exit(descendant_pid),
            "completed subprocess descendant {descendant_pid} survived"
        );
    }
}
