//! Each pair of a command and a global option that writes a file, which the
//! `command_support` table of `fallow schema --cli-reference` marks as
//! supported, runs without an error and writes a file with content.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::common::{copy_fixture, fallow_bin, git, run_fallow_raw, scrub_analysis_env};

/// The global options that write a file.
const FILE_OPTIONS: &[&str] = &[
    "--save-baseline",
    "--save-regression-baseline",
    "--sarif-file",
    "--output-file",
];

const BARE_RUN: &str = "fallow";

/// The supported commands that the test cannot run, with the reason.
const NOT_RUNNABLE: &[(&str, &str)] = &[
    (
        "similar-code",
        "needs the similar-code companion binary, which the test build does not have",
    ),
    #[cfg(not(unix))]
    ("watch", "the test stops `watch` with SIGTERM"),
];

/// The longest time that the test waits for a command.
const FILE_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// The line that `watch` prints on stderr when its first report is ready.
const WATCH_READY: &str = "Watching for changes";

/// The arguments that run `command` on `project` without interaction, or
/// `None` for a command that the test does not know. `scratch` holds the
/// input files, outside the project.
fn command_args(command: &str, project: &Path, scratch: &Path) -> Option<Vec<String>> {
    let path = |p: &Path| p.to_str().expect("utf8 path").to_owned();
    let args: Vec<String> = match command {
        BARE_RUN => Vec::new(),
        "dead-code" | "architecture" | "dupes" | "health" | "security" | "watch" | "mutation" => {
            vec![command.to_owned()]
        }
        "security blind-spots" => vec!["security".into(), "blind-spots".into()],
        "security survivors" => {
            let candidates = scratch.join("candidates.json");
            let verdicts = scratch.join("verdicts.json");
            std::fs::write(&candidates, r#"{"kind":"security","security_findings":[]}"#)
                .expect("write candidates");
            std::fs::write(&verdicts, "[]").expect("write verdicts");
            vec![
                "security".into(),
                "survivors".into(),
                "--candidates".into(),
                path(&candidates),
                "--verdicts".into(),
                path(&verdicts),
            ]
        }
        "audit" | "decision-surface" => vec![command.into(), "--base".into(), "HEAD".into()],
        "type-aware" => vec!["type-aware".into(), "status".into()],
        "trace-error" => vec!["trace-error".into(), "src/index.ts".into()],
        "guard" => vec!["guard".into(), "src/index.ts".into()],
        "baselines" => {
            let baseline = scratch.join("dead-code-baseline.json");
            run_quiet(project, &["dead-code", "--save-baseline", &path(&baseline)]);
            vec![
                "baselines".into(),
                "prune".into(),
                "--check".into(),
                "--dead-code-baseline".into(),
                path(&baseline),
            ]
        }
        "report" => {
            let results = scratch.join("results.json");
            let output = run_quiet(project, &["dead-code", "--format", "json"]);
            std::fs::write(&results, output).expect("write results");
            vec![
                "report".into(),
                "--from".into(),
                path(&results),
                "--format".into(),
                "markdown".into(),
            ]
        }
        "flags" => vec!["flags".into(), "--retirement".into()],
        "list" => vec!["list".into(), "--entry-weight".into()],
        _ => return None,
    };
    Some(args)
}

/// Run fallow in `project` and return its stdout.
fn run_quiet(project: &Path, args: &[&str]) -> Vec<u8> {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    cmd.current_dir(project)
        .args(args)
        .arg("--quiet")
        .output()
        .expect("run fallow")
        .stdout
}

/// The status of `command` in a `command_support` value: the entry of the
/// longest command path, else the default.
fn status_of(support: &serde_json::Value, command: &str) -> String {
    let entries = support["commands"].as_array().expect("commands");
    let mut path = command;
    loop {
        if let Some(entry) = entries.iter().find(|entry| entry["command"] == path) {
            return entry["status"].as_str().expect("status").to_owned();
        }
        match path.rsplit_once(' ') {
            Some((parent, _)) => path = parent,
            None => {
                return support["default"]["status"]
                    .as_str()
                    .expect("status")
                    .to_owned();
            }
        }
    }
}

/// Each supported pair of a file option and a command path. The paths are
/// the bare run, each top-level command and each subcommand path that a row
/// names.
fn supported_pairs() -> Vec<(String, String)> {
    let reference = run_fallow_raw(&["schema", "--cli-reference"]);
    assert_eq!(reference.code, 0, "{}", reference.stderr);
    let doc: serde_json::Value = serde_json::from_str(&reference.stdout).expect("reference JSON");
    let globals = doc["global_options"].as_array().expect("global_options");
    let mut commands = vec![BARE_RUN.to_owned()];
    commands.extend(
        doc["commands"]
            .as_array()
            .expect("commands")
            .iter()
            .map(|command| command["name"].as_str().expect("name").to_owned()),
    );
    for row in globals {
        for entry in row["command_support"]["commands"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let command = entry["command"].as_str().expect("command").to_owned();
            if !commands.contains(&command) {
                commands.push(command);
            }
        }
    }
    let mut pairs = Vec::new();
    for option in FILE_OPTIONS {
        let row = globals
            .iter()
            .find(|row| row["long"] == *option)
            .unwrap_or_else(|| panic!("{option} is a global option"));
        for command in &commands {
            if status_of(&row["command_support"], command) == "supported" {
                pairs.push(((*option).to_owned(), command.clone()));
            }
        }
    }
    pairs
}

/// The result of one run of [`check_file`].
enum FileCheck {
    /// The command ended without an error and wrote content.
    Written,
    /// What went wrong.
    Problem(String),
}

/// Run the command and return the problem when it fails or writes no content.
///
/// `watch` runs until it is stopped. The test stops it with SIGTERM after it
/// prints [`WATCH_READY`], and `watch` then writes the file.
fn check_file(
    args: &[String],
    option: &str,
    project: &Path,
    home: &Path,
    file: &Path,
) -> FileCheck {
    let watch = args.first().is_some_and(|arg| arg == "watch");
    let stderr_path = home.join("stderr.log");
    let stderr = std::fs::File::create(&stderr_path).expect("stderr log");
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .env("HOME", home);
    scrub_analysis_env(&mut cmd);
    cmd.current_dir(project).args(args);
    if !args.iter().any(|arg| arg == "--format") {
        cmd.args(["--format", "json"]);
    }
    let file_arg = file.to_str().expect("utf8 path");
    if option == "--save-regression-baseline" {
        cmd.arg(format!("{option}={file_arg}"));
    } else {
        cmd.arg(option).arg(file_arg);
    }
    if !watch {
        cmd.arg("--quiet");
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .expect("start fallow");
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll fallow") {
            break status;
        }
        if watch
            && std::fs::read_to_string(&stderr_path).is_ok_and(|text| text.contains(WATCH_READY))
        {
            stop_with_sigterm(child.id());
            break child.wait().expect("wait for watch");
        }
        if start.elapsed() > FILE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return FileCheck::Problem("the command did not end in time".into());
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    if !matches!(status.code(), Some(0 | 1)) {
        let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
        return FileCheck::Problem(format!("the command exited with {status}: {stderr}"));
    }
    if std::fs::metadata(file).is_ok_and(|meta| meta.len() > 0) {
        FileCheck::Written
    } else {
        FileCheck::Problem(format!("no content in {}", file.display()))
    }
}

#[cfg(unix)]
fn stop_with_sigterm(pid: u32) {
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .expect("run kill");
    assert!(status.success(), "kill -TERM {pid} failed");
}

#[cfg(not(unix))]
fn stop_with_sigterm(_pid: u32) {
    unreachable!("the test runs `watch` only on unix");
}

fn committed_fixture() -> tempfile::TempDir {
    let project = copy_fixture("basic-project");
    git(project.path(), &["init", "-q"]);
    git(project.path(), &["add", "-A"]);
    git(
        project.path(),
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-qm",
            "init",
        ],
    );
    project
}

#[test]
fn each_supported_file_option_writes_its_file() {
    let pairs = supported_pairs();
    assert!(
        pairs
            .iter()
            .any(|(option, command)| option == "--save-regression-baseline"
                && command == "dead-code"),
        "the reference marks dead-code --save-regression-baseline as supported"
    );
    let mut failures = Vec::new();
    for (option, command) in &pairs {
        if NOT_RUNNABLE.iter().any(|(name, _)| name == command) {
            continue;
        }
        let project = committed_fixture();
        let scratch = tempfile::tempdir().expect("scratch dir");
        let home = scratch.path().join("home");
        std::fs::create_dir_all(&home).expect("home dir");
        let Some(args) = command_args(command, project.path(), scratch.path()) else {
            failures.push(format!(
                "{command} {option}: the table marks the pair as supported, but the test has no command line for {command}"
            ));
            continue;
        };
        let file = scratch.path().join("written.out");
        if let FileCheck::Problem(problem) = check_file(&args, option, project.path(), &home, &file)
        {
            failures.push(format!("{command} {option}: {problem}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A command that the test skips must still be a supported pair, so the skip
/// list does not outlive the reason for it.
#[test]
fn each_skipped_command_is_a_supported_pair() {
    let pairs = supported_pairs();
    for (name, reason) in NOT_RUNNABLE {
        assert!(
            pairs.iter().any(|(_, command)| command == name),
            "{name} is skipped ({reason}) but has no supported file option"
        );
    }
}
