use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

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
    let mut child = Command::new(binary)
        .args(args)
        .stdout(Stdio::from(
            stdout_file
                .reopen()
                .map_err(|err| format!("failed to reopen stdout temp file: {err}"))?,
        ))
        .stderr(Stdio::from(
            stderr_file
                .reopen()
                .map_err(|err| format!("failed to reopen stderr temp file: {err}"))?,
        ))
        .env("FALLOW_INTEGRATION_SURFACE", "mcp")
        .env("FALLOW_MCP_TOOL", tool)
        .spawn()
        .map_err(|err| {
            format!(
                "failed to execute fallow binary '{binary}': {err}. Ensure fallow is installed and available in PATH, or set FALLOW_BIN."
            )
        })?;

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|err| format!("failed to wait for fallow subprocess: {err}"))?
        {
            let stdout_len = file_len(stdout_file.as_file())?;
            if stdout_len > max_output_bytes as u64 {
                return Err(format!(
                    "code mode host output exceeded {max_output_bytes} bytes"
                ));
            }

            let stdout = read_file(stdout_file.as_file_mut(), "stdout")?;
            let stderr = read_limited_file(stderr_file.as_file_mut(), STDERR_LIMIT_BYTES)?;
            return normalize_output(status.code().unwrap_or(-1), &stdout, &stderr);
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("code mode execution timed out while running fallow".to_string());
        }
        if file_len(stdout_file.as_file())? > max_output_bytes as u64 {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "code mode host output exceeded {max_output_bytes} bytes"
            ));
        }

        thread::sleep(POLL_INTERVAL);
    }
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
