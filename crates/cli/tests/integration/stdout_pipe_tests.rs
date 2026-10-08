//! Report output on a stdout pipe that the parent process made non-blocking
//! (issue #3276), or that the reader closed early.
//!
//! Bun sets `O_NONBLOCK` on a pipe that it shares with a child. A large write
//! then fails with `EAGAIN`. The report must still arrive complete, and a
//! closed reader must not cause an error message or a different exit code.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{fallow_bin, scrub_analysis_env};
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use std::fmt::Write as _;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// The default pipe buffer on Linux and macOS is 64 KiB. The report must be
/// much larger so that the writer fills the pipe before the reader starts.
const MIN_REPORT_BYTES: usize = 512 * 1024;
const FILE_COUNT: usize = 400;
const EXPORTS_PER_FILE: usize = 20;
/// The reader starts to read only when the pipe holds at least this many
/// bytes. The smallest pipe buffer (macOS, before it grows) is 16 KiB.
const PIPE_FULL_BYTES: u64 = 16 * 1024;
/// Time after the pipe is full, so that the writer gets `EAGAIN` at least once.
const FULL_PIPE_GRACE: Duration = Duration::from_millis(200);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const ANALYSIS_TIMEOUT: Duration = Duration::from_secs(120);

/// Write a project with many unused exports, so that the JSON report is large.
fn write_large_project(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"large-report","version":"1.0.0","main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src");
    let mut index = String::new();
    for file in 0..FILE_COUNT {
        let mut body = String::new();
        for export in 0..EXPORTS_PER_FILE {
            writeln!(
                body,
                "export const unusedValueWithALongName_{file}_{export} = {export};"
            )
            .expect("write to string");
        }
        writeln!(body, "export const used_{file} = 1;").expect("write to string");
        std::fs::write(src.join(format!("module_{file}.ts")), body).expect("write module");
        writeln!(
            index,
            "import {{ used_{file} }} from './module_{file}';\nconsole.log(used_{file});"
        )
        .expect("write to string");
    }
    std::fs::write(src.join("index.ts"), index).expect("write index");
}

fn dead_code_command(root: &Path) -> Command {
    let mut cmd = Command::new(fallow_bin());
    scrub_analysis_env(&mut cmd);
    cmd.arg("dead-code")
        .arg("--root")
        .arg(root)
        .args(["--format", "json", "--quiet", "--no-cache"])
        .env("NO_COLOR", "1")
        .env("FALLOW_TELEMETRY_DISABLED", "1")
        .stderr(Stdio::piped());
    cmd
}

fn set_nonblocking(fd: &impl rustix::fd::AsFd) {
    let flags = fcntl_getfl(fd).expect("read the file status flags");
    fcntl_setfl(fd, flags | OFlags::NONBLOCK).expect("set O_NONBLOCK");
}

/// Do not read until the child fills the pipe, so that the child gets `EAGAIN`
/// on a slow runner too. Stop the wait when the child exits first.
fn wait_until_pipe_is_full(reader: &std::io::PipeReader, child: &mut std::process::Child) {
    let start = std::time::Instant::now();
    loop {
        let pending = rustix::io::ioctl_fionread(reader).expect("read the pipe fill level");
        if pending >= PIPE_FULL_BYTES {
            std::thread::sleep(FULL_PIPE_GRACE);
            return;
        }
        if child.try_wait().expect("check fallow").is_some() {
            return;
        }
        assert!(
            start.elapsed() < ANALYSIS_TIMEOUT,
            "fallow did not fill the pipe in {ANALYSIS_TIMEOUT:?}"
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[test]
fn json_report_is_complete_when_stdout_is_nonblocking() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_large_project(dir.path());

    let (mut reader, writer) = std::io::pipe().expect("create pipe");
    set_nonblocking(&writer);

    let mut cmd = dead_code_command(dir.path());
    cmd.stdout(Stdio::from(writer));
    let mut child = cmd.spawn().expect("spawn fallow");
    // Drop the parent copy of the write end, so that the reader sees EOF.
    drop(cmd);

    wait_until_pipe_is_full(&reader, &mut child);
    let mut stdout = Vec::new();
    reader.read_to_end(&mut stdout).expect("read stdout");
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("piped stderr")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    let status = child.wait().expect("wait for fallow");

    assert!(
        stdout.len() > MIN_REPORT_BYTES,
        "the report must be larger than {MIN_REPORT_BYTES} bytes, got {} bytes (stderr: {stderr})",
        stdout.len()
    );
    let parsed: serde_json::Value = serde_json::from_slice(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout must be complete JSON, got {} bytes: {e} (stderr: {stderr})",
            stdout.len()
        )
    });
    assert!(parsed.is_object(), "the report must be a JSON object");
    assert_eq!(
        status.code(),
        Some(1),
        "unused exports give exit 1 (stderr: {stderr})"
    );
}

#[test]
fn closed_stdout_reader_exits_without_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_large_project(dir.path());

    let (reader, writer) = std::io::pipe().expect("create pipe");
    // Close the read end before the child writes, so every write gets EPIPE.
    drop(reader);

    let mut cmd = dead_code_command(dir.path());
    cmd.stdout(Stdio::from(writer));
    let output = cmd.output().expect("run fallow");
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The same run with stdout on /dev/null gives the expected exit code and
    // the expected stderr (the gate summary line).
    let mut baseline = dead_code_command(dir.path());
    baseline.stdout(Stdio::null());
    let expected = baseline.output().expect("run fallow");
    let expected_stderr = String::from_utf8_lossy(&expected.stderr);

    assert_eq!(
        output.status.code(),
        expected.status.code(),
        "a closed reader keeps the exit code of the analysis (stderr: {stderr})"
    );
    assert_eq!(
        stderr, expected_stderr,
        "a closed reader must not add output to stderr"
    );
}

/// A write error other than `EPIPE` must not look like a successful report.
/// `/dev/full` returns `ENOSPC` for every write. macOS has no `/dev/full`.
#[cfg(target_os = "linux")]
#[test]
fn stdout_write_error_exits_with_code_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_large_project(dir.path());

    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .expect("open /dev/full");
    let mut cmd = dead_code_command(dir.path());
    cmd.stdout(Stdio::from(full));
    let output = cmd.output().expect("run fallow");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("failed to write the report to stdout"),
        "stderr must name the write failure, got: {stderr}"
    );
}
