//! Each pair of a command and a global option that writes a file, which the
//! `command_support` table of `fallow schema --cli-reference` marks as
//! supported, writes that file.
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

/// A command can stop with an error or run until it is stopped (`watch`). The
/// test waits this long for the file, then stops the command.
const FILE_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The arguments that run `command` on the fixture without interaction.
/// `None` for a command that the test cannot run.
fn command_args(command: &str, scratch: &Path) -> Option<Vec<String>> {
    let empty_results = scratch.join("empty.json");
    std::fs::write(&empty_results, "{}").expect("write empty results");
    let path = |p: &Path| p.to_str().expect("utf8 path").to_owned();
    let args: Vec<String> = match command {
        BARE_RUN => Vec::new(),
        "dead-code" | "architecture" | "dupes" | "health" | "security" | "audit" | "recommend"
        | "workspaces" | "suppressions" | "decision-surface" | "watch" | "similar-code"
        | "mutation" => vec![command.to_owned()],
        "fix" => vec!["fix".into(), "--dry-run".into()],
        "type-aware" => vec!["type-aware".into(), "status".into()],
        "trace-error" => vec!["trace-error".into(), "src/index.ts".into()],
        "agent" => vec!["agent".into(), "status".into()],
        "rule-pack" => vec!["rule-pack".into(), "list".into()],
        "guard" => vec!["guard".into(), "src/index.ts".into()],
        "baselines" => vec!["baselines".into(), "prune".into(), "--check".into()],
        "impact" => vec!["impact".into(), "status".into()],
        "report" => vec!["report".into(), "--from".into(), path(&empty_results)],
        "viz" => vec![
            "viz".into(),
            "--no-open".into(),
            "--out".into(),
            path(&scratch.join("viz.html")),
        ],
        "flags" => vec!["flags".into(), "--retirement".into()],
        "list" => vec!["list".into(), "--entry-weight".into()],
        _ => return None,
    };
    Some(args)
}

fn status_of<'a>(support: &'a serde_json::Value, command: &str) -> &'a serde_json::Value {
    support["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .find(|entry| entry["command"] == command)
        .map_or(&support["default"], |entry| entry)
}

fn supported_pairs() -> Vec<(String, String)> {
    let reference = run_fallow_raw(&["schema", "--cli-reference"]);
    assert_eq!(reference.code, 0, "{}", reference.stderr);
    let doc: serde_json::Value = serde_json::from_str(&reference.stdout).expect("reference JSON");
    let mut commands = vec![BARE_RUN.to_owned()];
    commands.extend(
        doc["commands"]
            .as_array()
            .expect("commands")
            .iter()
            .map(|command| command["name"].as_str().expect("name").to_owned()),
    );
    let mut pairs = Vec::new();
    for option in FILE_OPTIONS {
        let row = doc["global_options"]
            .as_array()
            .expect("global_options")
            .iter()
            .find(|row| row["long"] == *option)
            .unwrap_or_else(|| panic!("{option} is a global option"));
        for command in &commands {
            if status_of(&row["command_support"], command)["status"] == "supported" {
                pairs.push(((*option).to_owned(), command.clone()));
            }
        }
    }
    pairs
}

/// Run the command until `file` exists, the command ends, or the timeout.
fn writes_file(args: &[String], option: &str, root: &Path, file: &Path) -> bool {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    cmd.current_dir(root).args(args);
    let file_arg = file.to_str().expect("utf8 path");
    if option == "--save-regression-baseline" {
        cmd.arg(format!("{option}={file_arg}"));
    } else {
        cmd.arg(option).arg(file_arg);
    }
    let mut child = cmd
        .arg("--quiet")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start fallow");
    let start = Instant::now();
    loop {
        if file.exists() {
            break;
        }
        if child.try_wait().expect("poll fallow").is_some() || start.elapsed() > FILE_TIMEOUT {
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    let _ = child.kill();
    let _ = child.wait();
    file.exists()
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
        let scratch = tempfile::tempdir().expect("scratch dir");
        let Some(args) = command_args(command, scratch.path()) else {
            failures.push(format!(
                "{command} {option}: the table marks the pair as supported, but the test has no command line for {command}"
            ));
            continue;
        };
        let file = scratch.path().join("written.out");
        if !writes_file(&args, option, project.path(), &file) {
            failures.push(format!("{command} {option}: no file at {}", file.display()));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
