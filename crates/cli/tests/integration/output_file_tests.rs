//! `--output-file` / `-o`: redirect the rendered report to a file instead of
//! stdout, for any `--format`, with no ANSI codes and a stderr confirmation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{fallow_bin, fixture_path, run_fallow, run_fallow_raw, run_fallow_raw_in_dir};
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

/// `--output-file` and `--sarif-file` write two reports. When both name one
/// file, the later write replaces the earlier report, so one report is lost
/// with no error. The run must refuse the pair before the analysis.
#[test]
fn refuses_one_file_for_the_primary_and_the_sarif_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fixture = fixture_path(FIXTURE);
    for (primary, sarif) in [
        ("out.sarif", "out.sarif"),
        ("out.sarif", "./sub/../out.sarif"),
    ] {
        let result = run_fallow_raw_in_dir(
            dir.path(),
            &[
                "dead-code",
                "--root",
                fixture.to_str().expect("utf8 path"),
                "-o",
                primary,
                "--sarif-file",
                sarif,
            ],
        );

        assert_eq!(
            result.code, 2,
            "{primary} and {sarif} are one file; stderr: {}",
            result.stderr
        );
        assert!(
            result.stderr.contains("--output-file") && result.stderr.contains("--sarif-file"),
            "error should name both flags, got: {}",
            result.stderr
        );
        assert!(
            !dir.path().join("out.sarif").exists(),
            "no report is written when the destinations overlap"
        );
    }
}

/// The overlap check covers each save flag, not only the two report flags.
#[test]
fn refuses_one_file_for_a_saved_baseline_and_the_primary_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fixture = fixture_path(FIXTURE);
    let result = run_fallow_raw_in_dir(
        dir.path(),
        &[
            "dead-code",
            "--root",
            fixture.to_str().expect("utf8 path"),
            "--save-baseline",
            "out.json",
            "-o",
            "out.json",
        ],
    );

    assert_eq!(result.code, 2, "stderr: {}", result.stderr);
    assert!(
        result.stderr.contains("--save-baseline") && result.stderr.contains("--output-file"),
        "error should name both flags, got: {}",
        result.stderr
    );
}

/// A device takes more than one write, and a `--sarif-file` that the command
/// does not write is no second destination.
#[test]
fn allows_a_device_or_an_unwritten_sarif_file_for_two_flags() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fixture = fixture_path(FIXTURE);
    let fixture = fixture.to_str().expect("utf8 path");
    #[cfg(unix)]
    {
        let result = run_fallow_raw_in_dir(
            dir.path(),
            &[
                "dead-code",
                "--root",
                fixture,
                "-o",
                "/dev/null",
                "--sarif-file",
                "/dev/null",
            ],
        );
        assert!(result.code <= 1, "a device is allowed: {}", result.stderr);
    }
    let result = run_fallow_raw_in_dir(
        dir.path(),
        &[
            "health",
            "--root",
            fixture,
            "-o",
            "out.txt",
            "--sarif-file",
            "out.txt",
        ],
    );
    assert!(
        !result.stderr.contains("name the same file"),
        "health writes no SARIF file, so the pair does not overlap: {}",
        result.stderr
    );
}

#[test]
fn sarif_file_on_a_command_that_writes_no_sarif_file_exits_2() {
    // Only bare `fallow`, `dead-code`, `architecture` and `security` write the
    // SARIF file. Any other command must reject the flag: an exit 0 with no
    // file tells a CI job that the upload input exists when it does not.
    let dir = tempfile::tempdir().expect("tempdir");
    let sarif = dir.path().join("audit.sarif");
    let result = run_fallow(
        "audit",
        FIXTURE,
        &[
            "--base",
            "HEAD",
            "--quiet",
            "--sarif-file",
            sarif.to_str().unwrap(),
        ],
    );

    assert_eq!(
        result.code, 2,
        "audit must reject --sarif-file; stdout: {}; stderr: {}",
        result.stdout, result.stderr
    );
    let error_lines: Vec<&str> = result
        .stderr
        .lines()
        .filter(|line| line.contains("--sarif-file"))
        .collect();
    assert_eq!(
        error_lines.len(),
        1,
        "one error line names the flag; stderr: {}",
        result.stderr
    );
    assert!(
        error_lines[0].contains("`fallow audit`"),
        "the error names the command; stderr: {}",
        result.stderr
    );
    assert!(!sarif.exists(), "no SARIF file is written");
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

/// A saved JSON report of the fixture, written to `dir/name`.
fn saved_report(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let result = run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "json", "-o", path.to_str().expect("utf8 path")],
    );
    assert!(
        result.code <= 1,
        "saving the report failed: {}",
        result.stderr
    );
    path
}

fn assert_refused_alias(dir: &std::path::Path, from: &str, output_file: &str, saved: &str) {
    let result = run_fallow_raw_in_dir(
        dir,
        &[
            "report",
            "--from",
            from,
            "--format",
            "markdown",
            "--output-file",
            output_file,
        ],
    );

    assert_eq!(result.code, 2, "an aliased destination should exit 2");
    assert!(
        result.stderr.contains("--output-file") && result.stderr.contains("--from"),
        "error should name both flags, got: {}",
        result.stderr
    );
    assert_eq!(
        read(&dir.join("report.json")),
        saved,
        "the saved report must keep its bytes"
    );
}

#[test]
fn report_refuses_an_output_file_that_is_its_from_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let saved = read(&saved_report(dir.path(), "report.json"));

    assert_refused_alias(dir.path(), "report.json", "report.json", &saved);
}

#[test]
fn report_refuses_another_spelling_of_its_from_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
    let saved = read(&saved_report(dir.path(), "report.json"));

    assert_refused_alias(dir.path(), "report.json", "sub/../report.json", &saved);
}

#[test]
fn report_refuses_a_spelling_through_a_missing_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let saved = read(&saved_report(dir.path(), "report.json"));

    assert_refused_alias(dir.path(), "report.json", "new/../report.json", &saved);
}

#[cfg(unix)]
#[test]
fn report_refuses_a_link_to_its_from_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let saved = read(&saved_report(dir.path(), "report.json"));
    std::os::unix::fs::symlink("report.json", dir.path().join("link.json")).expect("symlink");

    assert_refused_alias(dir.path(), "report.json", "link.json", &saved);
}

#[cfg(unix)]
#[test]
fn report_refuses_a_hard_link_to_its_from_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let saved = read(&saved_report(dir.path(), "report.json"));
    std::fs::hard_link(dir.path().join("report.json"), dir.path().join("copy.json"))
        .expect("hard link");

    assert_refused_alias(dir.path(), "report.json", "copy.json", &saved);
}

#[cfg(unix)]
#[test]
fn report_refuses_a_spelling_through_a_linked_folder_and_a_missing_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("d/sub")).expect("mkdir");
    let saved = read(&saved_report(dir.path(), "report.json"));
    std::fs::rename(
        dir.path().join("report.json"),
        dir.path().join("d/report.json"),
    )
    .expect("move report");
    std::fs::write(dir.path().join("report.json"), &saved).expect("decoy");
    std::os::unix::fs::symlink("d/sub", dir.path().join("lk")).expect("symlink");

    let result = run_fallow_raw_in_dir(
        dir.path(),
        &[
            "report",
            "--from",
            "d/report.json",
            "--format",
            "markdown",
            "--output-file",
            "lk/missing/../../report.json",
        ],
    );

    assert_eq!(result.code, 2, "an aliased destination should exit 2");
    assert_eq!(read(&dir.path().join("d/report.json")), saved);
}

#[cfg(unix)]
#[test]
fn report_writes_through_a_linked_folder_to_a_different_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("d/sub")).expect("mkdir");
    let saved = read(&saved_report(dir.path(), "report.json"));
    std::os::unix::fs::symlink("d/sub", dir.path().join("lk")).expect("symlink");

    let result = run_fallow_raw_in_dir(
        dir.path(),
        &[
            "report",
            "--from",
            "report.json",
            "--format",
            "markdown",
            "--output-file",
            "lk/../report.json",
        ],
    );

    assert!(result.code <= 1, "report should render: {}", result.stderr);
    assert!(!read(&dir.path().join("d/report.json")).is_empty());
    assert_eq!(read(&dir.path().join("report.json")), saved);
}

#[test]
fn report_from_a_saved_file_writes_a_separate_output_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let saved = read(&saved_report(dir.path(), "report.json"));

    let result = run_fallow_raw_in_dir(
        dir.path(),
        &[
            "report",
            "--from",
            "report.json",
            "--format",
            "markdown",
            "--output-file",
            "report.md",
        ],
    );

    assert!(result.code <= 1, "report should render: {}", result.stderr);
    assert!(!read(&dir.path().join("report.md")).is_empty());
    assert_eq!(read(&dir.path().join("report.json")), saved);
}

/// `viz --out` through a project folder that is a symlink to a place outside
/// the project is refused, and no map is written there.
#[cfg(unix)]
#[test]
fn viz_out_through_a_project_link_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("project");
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::create_dir_all(&elsewhere).expect("elsewhere");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"viz-link","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(root.join("src/index.ts"), "export const a = 1;\n").expect("index");
    std::os::unix::fs::symlink(&elsewhere, root.join("maps")).expect("link");

    let result = run_fallow_raw_in_dir(
        &root,
        &["viz", "--out", "maps/map.html", "--no-open", "--quiet"],
    );
    assert_eq!(result.code, 2, "stderr: {}", result.stderr);
    assert!(result.stderr.contains("--out"), "stderr: {}", result.stderr);
    assert!(!elsewhere.join("map.html").exists());

    let inside = run_fallow_raw_in_dir(
        &root,
        &["viz", "--out", "reports/map.html", "--no-open", "--quiet"],
    );
    assert_eq!(inside.code, 0, "stderr: {}", inside.stderr);
    assert!(root.join("reports/map.html").is_file());
}
