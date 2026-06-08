//! `--output-file` / `-o`: redirect the rendered report to a file instead of
//! stdout, for any `--format`, with no ANSI codes and a stderr confirmation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

mod common;

use common::{fallow_bin, fixture_path, run_fallow, run_fallow_raw};
use std::process::Command;

const FIXTURE: &str = "basic-project";

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).expect("output file should exist and be UTF-8")
}

#[test]
fn human_report_goes_to_file_and_stdout_stays_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("report.txt");
    let result = run_fallow(
        "dead-code",
        FIXTURE,
        &["-o", out.to_str().expect("utf8 path")],
    );

    // The report content is in the file, not on stdout.
    assert!(
        result.stdout.is_empty(),
        "stdout should be empty when -o is set, got: {}",
        result.stdout
    );
    let contents = read(&out);
    assert!(!contents.is_empty(), "the file should contain the report");
    // No ANSI escape codes in the file, even attached to a TTY (forced off).
    assert!(
        !contents.contains('\u{1b}'),
        "file must not contain ANSI escape codes"
    );
    // The confirmation goes to stderr, not into the file.
    assert!(
        result.stderr.contains("Report written to"),
        "expected a stderr confirmation, got: {}",
        result.stderr
    );
    assert!(!contents.contains("Report written to"));
}

#[test]
fn json_report_is_written_to_the_file_and_parses() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("report.json");
    let result = run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "json", "-o", out.to_str().expect("utf8 path")],
    );

    assert!(result.stdout.is_empty(), "stdout empty for -o json");
    let contents = read(&out);
    let value: serde_json::Value =
        serde_json::from_str(&contents).expect("the file should contain valid JSON");
    assert!(value.is_object(), "JSON report should be an object");
}

#[test]
fn quiet_suppresses_the_confirmation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("report.txt");
    let result = run_fallow(
        "dead-code",
        FIXTURE,
        &["--quiet", "-o", out.to_str().expect("utf8 path")],
    );

    assert!(
        !result.stderr.contains("Report written to"),
        "--quiet should suppress the confirmation, stderr was: {}",
        result.stderr
    );
    assert!(
        !read(&out).is_empty(),
        "the file is still written under --quiet"
    );
}

#[test]
fn long_and_short_flags_are_equivalent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let short = dir.path().join("short.txt");
    let long = dir.path().join("long.txt");
    run_fallow("dead-code", FIXTURE, &["-o", short.to_str().unwrap()]);
    run_fallow(
        "dead-code",
        FIXTURE,
        &["--output-file", long.to_str().unwrap()],
    );
    assert_eq!(
        read(&short),
        read(&long),
        "-o and --output-file must write the same report"
    );
}

#[test]
fn rejected_for_a_non_analysis_command() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("nope.txt");
    let result = run_fallow_raw(&["list", "-o", out.to_str().unwrap()]);

    assert_eq!(result.code, 2, "non-analysis command should exit 2");
    assert!(
        result.stderr.contains("--output-file"),
        "error should name --output-file, got: {}",
        result.stderr
    );
    assert!(!out.exists(), "no file is created on the rejection path");
}

#[test]
fn errors_when_the_parent_path_is_a_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A regular file used as a parent directory: create_dir_all must fail.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"x").expect("write blocker file");
    let bad = blocker.join("report.txt");

    let result = run_fallow("dead-code", FIXTURE, &["-o", bad.to_str().unwrap()]);
    assert_eq!(result.code, 2, "an unopenable path should exit 2");
    assert!(
        result.stderr.contains("--output-file"),
        "error should name --output-file, got: {}",
        result.stderr
    );
}

#[test]
fn coexists_with_sarif_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let primary = dir.path().join("report.txt");
    let sarif = dir.path().join("out.sarif");
    let result = run_fallow(
        "dead-code",
        FIXTURE,
        &[
            "-o",
            primary.to_str().unwrap(),
            "--sarif-file",
            sarif.to_str().unwrap(),
        ],
    );

    assert!(result.stdout.is_empty(), "primary report goes to the file");
    assert!(!read(&primary).is_empty(), "primary report written");
    let sarif_value: serde_json::Value =
        serde_json::from_str(&read(&sarif)).expect("sarif sidecar should be valid JSON");
    assert_eq!(
        sarif_value["version"], "2.1.0",
        "sarif sidecar still written"
    );
}

#[test]
fn confirmation_is_suppressed_when_a_command_errors_before_rendering() {
    // `--report-only` with `--min-score` is rejected inside dispatch (exit 2),
    // after the sink is opened: the error goes to stdout, the file stays empty,
    // and we must not claim "Report written" over it.
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("report.txt");
    let result = run_fallow(
        "health",
        FIXTURE,
        &[
            "--report-only",
            "--min-score",
            "50",
            "-o",
            out.to_str().unwrap(),
        ],
    );

    assert_eq!(result.code, 2, "the conflicting flags should exit 2");
    assert!(
        !result.stderr.contains("Report written to"),
        "no confirmation when nothing was rendered to the file, stderr: {}",
        result.stderr
    );
}

#[test]
fn bare_combined_mode_writes_json_to_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("combined.json");
    let bin = fallow_bin();
    let root = fixture_path(FIXTURE);
    let output = Command::new(&bin)
        .arg("--root")
        .arg(&root)
        .arg("--format")
        .arg("json")
        .arg("-o")
        .arg(&out)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .output()
        .expect("run fallow");

    assert!(
        output.stdout.is_empty(),
        "combined-mode report goes to the file, not stdout"
    );
    let value: serde_json::Value =
        serde_json::from_str(&read(&out)).expect("combined JSON report should parse");
    assert!(value.is_object());
}
