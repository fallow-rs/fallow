//! Each pair of a command and a global option that writes a file, which the
//! `command_support` table of `fallow schema --cli-reference` marks as
//! supported, runs without an error and writes a file with content.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
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
/// A part of the message for a format that a command does not support for
/// `--output-file`.
const FORMATS_MESSAGE_PART: &str = "writes the file of `--output-file` only with";

/// Subcommand forms that the file test runs besides the command paths that
/// the table names. Each form has the support of its parent command.
const EXTRA_FORMS: &[&str] = &["agent install", "agent uninstall"];

/// The global options that read a path, with the extra arguments that make
/// the command read it.
const READ_OPTIONS: &[(&str, &[&str])] = &[
    ("--root", &[]),
    ("--config", &[]),
    ("--diff-file", &[]),
    ("--churn-file", &[]),
    ("--baseline", &[]),
    ("--regression-baseline", &["--fail-on-regression"]),
    ("--type-aware-project", &["--type-aware"]),
];

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
        "agent" => vec!["agent".into(), "status".into()],
        "agent install" => vec!["agent".into(), "install".into(), "--dry-run".into()],
        "agent uninstall" => vec!["agent".into(), "uninstall".into(), "--dry-run".into()],
        "fix" => vec!["fix".into(), "--dry-run".into()],

        "flags" => vec!["flags".into(), "--retirement".into()],
        "init" | "recommend" | "workspaces" | "suppressions" | "plugin-check" | "config"
        | "doctor" => vec![command.to_owned()],
        "inspect" => vec!["inspect".into(), "--file".into(), "src/index.ts".into()],
        "trace" => vec!["trace".into(), "src/utils.ts:unusedFunction".into()],
        "explain" => vec!["explain".into(), "unused-files".into()],
        "hooks" => vec!["hooks".into(), "status".into()],
        "impact" => vec!["impact".into(), "status".into()],
        "impact statusline" => vec!["impact".into(), "statusline".into()],
        "license" => vec!["license".into(), "status".into()],
        "audit-cache" => vec!["audit-cache".into(), "prune".into()],
        "ci-template" => vec!["ci-template".into(), "github".into()],
        "migrate" | "setup-hooks" => vec![command.into(), "--dry-run".into()],
        "cloud" => vec!["cloud".into(), "logout".into()],
        "coverage" => vec!["coverage".into(), "analyze".into()],
        "rule-pack" | "rule-pack list" => vec!["rule-pack".into(), "list".into()],
        "rule-pack schema" => vec!["rule-pack".into(), "schema".into()],
        "viz" => vec![
            "viz".into(),
            "--no-open".into(),
            "--out".into(),
            path(&scratch.join("viz.html")),
        ],
        "ci" => {
            let body = scratch.join("body.md");
            std::fs::write(&body, "body").expect("write body");
            vec![
                "ci".into(),
                "plan-pr-comment".into(),
                "--body".into(),
                path(&body),
                "--marker-id".into(),
                "m".into(),
            ]
        }
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

/// The support of `command` in a `command_support` value: the entry of the
/// longest command path, else the default.
fn support_of(support: &serde_json::Value, command: &str) -> serde_json::Value {
    let entries = support["commands"].as_array().expect("commands");
    let mut path = command;
    loop {
        if let Some(entry) = entries.iter().find(|entry| entry["command"] == path) {
            return entry.clone();
        }
        match path.rsplit_once(' ') {
            Some((parent, _)) => path = parent,
            None => return support["default"].clone(),
        }
    }
}

/// One supported pair of a file option and a command path.
struct Pair {
    option: String,
    command: String,
    /// The formats to run. `None` runs the format of the command line, or
    /// JSON when the command line names none.
    formats: Option<Vec<String>>,
    /// The formats that the command rejects for the option.
    rejected_formats: Vec<String>,
}

/// Each supported pair of a file option and a command path. The paths are
/// the bare run, each top-level command and each subcommand path that a row
/// names.
fn supported_pairs() -> Vec<Pair> {
    supported_pairs_for(FILE_OPTIONS)
}

/// Each supported pair of one of `options` and a command path.
fn supported_pairs_for(options: &[&str]) -> Vec<Pair> {
    let reference = run_fallow_raw(&["schema", "--cli-reference"]);
    assert_eq!(reference.code, 0, "{}", reference.stderr);
    let doc: serde_json::Value = serde_json::from_str(&reference.stdout).expect("reference JSON");
    let globals = doc["global_options"].as_array().expect("global_options");
    let all_formats: Vec<String> = doc["output_formats"]
        .as_array()
        .expect("output_formats")
        .iter()
        .map(|format| format["name"].as_str().expect("name").to_owned())
        .collect();
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
    commands.extend(EXTRA_FORMS.iter().map(|form| (*form).to_owned()));
    let mut pairs = Vec::new();
    for option in options {
        let row = globals
            .iter()
            .find(|row| row["long"] == *option)
            .unwrap_or_else(|| panic!("{option} is a global option"));
        for command in &commands {
            let support = support_of(&row["command_support"], command);
            let (formats, rejected_formats) = match support["status"].as_str() {
                Some("supported") => (None, Vec::new()),
                Some("supported_with_formats") => {
                    let allowed: Vec<String> = support["formats"]
                        .as_array()
                        .expect("formats")
                        .iter()
                        .map(|format| format.as_str().expect("format").to_owned())
                        .collect();
                    let rejected = all_formats
                        .iter()
                        .filter(|format| !allowed.contains(format))
                        .cloned()
                        .collect();
                    (Some(allowed), rejected)
                }
                _ => continue,
            };
            pairs.push(Pair {
                option: (*option).to_owned(),
                command: command.clone(),
                formats,
                rejected_formats,
            });
        }
    }
    pairs
}

/// The result of one check of a pair.
enum FileCheck {
    /// The pair behaves as the table states.
    Pass,
    /// What went wrong.
    Problem(String),
}

/// Run the command and return the problem when it fails or writes no content.
///
/// `watch` runs until it is stopped. The test stops it with SIGTERM after it
/// prints [`WATCH_READY`], and `watch` then writes the file.
fn check_file(
    args: &[String],
    format: Option<&str>,
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
    match format {
        Some(format) => {
            cmd.args(["--format", format]);
        }
        None if !args.iter().any(|arg| arg == "--format") => {
            cmd.args(["--format", "json"]);
        }
        None => {}
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
        FileCheck::Pass
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
    commit_project(project.path());
    project
}

/// Make `dir` a git repository with one commit of each file.
fn commit_project(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["add", "-A"]);
    git(
        dir,
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
}

/// Run the command with a format that it rejects for the option, and
/// return the problem when it does not exit with code 2.
fn check_rejected_format(
    args: &[String],
    format: &str,
    option: &str,
    project: &Path,
    file: &Path,
) -> FileCheck {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    let output = cmd
        .current_dir(project)
        .args(args)
        .args([
            "--format",
            format,
            option,
            file.to_str().expect("utf8 path"),
        ])
        .arg("--quiet")
        .output()
        .expect("run fallow");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.code() != Some(2) {
        return FileCheck::Problem(format!(
            "--format {format} exited with {}, not 2",
            output.status
        ));
    }
    if text.contains(FORMATS_MESSAGE_PART) {
        FileCheck::Pass
    } else {
        FileCheck::Problem(format!(
            "--format {format} exited with 2, but not with the message of the table: {text}"
        ))
    }
}

#[test]
fn each_supported_file_option_writes_its_file() {
    let pairs = supported_pairs();
    assert!(
        pairs
            .iter()
            .any(|pair| pair.option == "--save-regression-baseline" && pair.command == "dead-code"),
        "the reference marks dead-code --save-regression-baseline as supported"
    );
    assert!(
        pairs.iter().any(|pair| pair.formats.is_some()),
        "the reference marks some pairs as supported with some formats"
    );
    let mut failures = Vec::new();
    for pair in &pairs {
        let (option, command) = (&pair.option, &pair.command);
        if NOT_RUNNABLE.iter().any(|(name, _)| name == command) {
            continue;
        }
        let formats: Vec<Option<&str>> = pair.formats.as_ref().map_or_else(
            || vec![None],
            |formats| formats.iter().map(|format| Some(format.as_str())).collect(),
        );
        for format in formats {
            let project = committed_fixture();
            let scratch = tempfile::tempdir().expect("scratch dir");
            let home = scratch.path().join("home");
            std::fs::create_dir_all(&home).expect("home dir");
            let Some(args) = command_args(command, project.path(), scratch.path()) else {
                failures.push(format!(
                    "{command} {option}: the table marks the pair as supported, but the test has no command line for {command}"
                ));
                break;
            };
            let file = scratch.path().join("written.out");
            if let FileCheck::Problem(problem) =
                check_file(&args, format, option, project.path(), &home, &file)
            {
                failures.push(format!("{command} {option} {format:?}: {problem}"));
            }
            if format
                == pair
                    .formats
                    .as_ref()
                    .and_then(|f| f.first())
                    .map(String::as_str)
            {
                for rejected in &pair.rejected_formats {
                    if let FileCheck::Problem(problem) =
                        check_rejected_format(&args, rejected, option, project.path(), &file)
                    {
                        failures.push(format!("{command} {option}: {problem}"));
                    }
                }
            }
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
            pairs.iter().any(|pair| pair.command == *name),
            "{name} is skipped ({reason}) but has no supported file option"
        );
    }
}

/// Run the command with a path that does not exist, and return the problem
/// when the command exits 0 or 1 and names the path nowhere.
fn check_missing_path(
    args: &[String],
    option: &str,
    extra: &[&str],
    project: &Path,
    home: &Path,
) -> FileCheck {
    let missing = project.join("missing-input-path.json");
    let missing_arg = missing.to_str().expect("utf8 path");
    let out_path = home.join("stdout.log");
    let err_path = home.join("stderr.log");
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .env("HOME", home);
    scrub_analysis_env(&mut cmd);
    let mut child = cmd
        .current_dir(project)
        .args(args)
        .args([option, missing_arg])
        .args(extra)
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(&out_path).expect("stdout log"))
        .stderr(std::fs::File::create(&err_path).expect("stderr log"))
        .spawn()
        .expect("start fallow");
    let names_path = || {
        [&out_path, &err_path].iter().any(|log| {
            std::fs::read_to_string(log).is_ok_and(|text| text.contains("missing-input-path"))
        })
    };
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll fallow") {
            break Some(status);
        }
        if names_path() || start.elapsed() > FILE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    if status.is_some_and(|status| status.code() == Some(2)) || names_path() {
        return FileCheck::Pass;
    }
    let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
    FileCheck::Problem(format!(
        "accepted the missing path in silence ({}): {stderr}",
        status.map_or_else(|| "still running".to_owned(), |status| status.to_string())
    ))
}

/// A supported option that reads a path fails, or warns with the path, when
/// the path does not exist. A command that accepts the option and reads
/// nothing would exit 0 in silence, which is the shape of issue #4931.
#[test]
fn each_supported_read_option_reads_its_path() {
    let options: Vec<&str> = READ_OPTIONS.iter().map(|(option, _)| *option).collect();
    let pairs = supported_pairs_for(&options);
    let mut failures = Vec::new();
    for pair in &pairs {
        let (option, command) = (&pair.option, &pair.command);
        if NOT_RUNNABLE.iter().any(|(name, _)| name == command) {
            continue;
        }
        let extra = READ_OPTIONS
            .iter()
            .find(|(name, _)| name == option)
            .map_or(&[][..], |(_, extra)| *extra);
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
        if let FileCheck::Problem(problem) =
            check_missing_path(&args, option, extra, project.path(), &home)
        {
            failures.push(format!("{command} {option}: {problem}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The project, the option arguments and the config of one duplication
/// override case. The override must change the clone groups of the project.
struct DupesCase {
    generated: bool,
    args: &'static [&'static str],
    config: Option<&'static str>,
}

fn dupes_case(option: &str) -> Option<DupesCase> {
    let fixture = |args| {
        Some(DupesCase {
            generated: false,
            args,
            config: None,
        })
    };
    let generated = |args, config| {
        Some(DupesCase {
            generated: true,
            args,
            config,
        })
    };
    match option {
        "--dupes-mode" => fixture(&["semantic"]),
        "--dupes-threshold" => fixture(&["0.5"]),
        "--dupes-min-tokens" => fixture(&["500"]),
        "--dupes-min-lines" => fixture(&["40"]),
        "--dupes-min-occurrences" => fixture(&["3"]),
        "--dupes-near" | "--dupes-skip-local" | "--dupes-cross-language" => fixture(&[]),
        "--dupes-no-ignore-imports" | "--dupes-ignore-symlinks" => generated(&[], None),
        "--dupes-ignore-imports" => {
            generated(&[], Some(r#"{"duplicates":{"ignoreImports":false}}"#))
        }
        "--dupes-no-ignore-symlinks" => {
            generated(&[], Some(r#"{"duplicates":{"ignoreSymlinks":true}}"#))
        }
        _ => None,
    }
}

/// A project with a block of imports in two files and a file with its
/// symlink, so the import and symlink overrides change the clone groups.
fn generated_dupes_project(dir: &Path) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("src dir");
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"dupes-case","main":"src/a.ts"}"#,
    )
    .expect("package.json");
    let mut imports = String::new();
    for i in 1..=14 {
        let _ = writeln!(imports, "import {{ value{i} }} from \"./dep{i}\";");
    }
    for name in ["a", "b"] {
        std::fs::write(
            src.join(format!("{name}.ts")),
            format!("{imports}\nexport const {name} = [value1, value2, value3];\n"),
        )
        .expect("import file");
    }
    for i in 1..=14 {
        std::fs::write(
            src.join(format!("dep{i}.ts")),
            format!("export const value{i} = {i};\n"),
        )
        .expect("dep file");
    }
    let mut body = String::new();
    for i in 1..=15 {
        let _ = writeln!(
            body,
            "  const v{i} = input.map((x) => x * {i} + {}).filter((y) => y > {i});",
            i * 3
        );
    }
    std::fs::write(
        src.join("long.ts"),
        format!("export function compute(input: number[]): number {{\n{body}  return v1.length + v15.length;\n}}\n"),
    )
    .expect("long file");
    #[cfg(unix)]
    std::os::unix::fs::symlink("long.ts", src.join("linked.ts")).expect("symlink");
}

/// The clone groups, the statistics and the gate outcomes of a run, or the
/// problem. `--dupes-threshold` changes the gate outcomes only.
fn dupes_section(project: &Path, args: &[String]) -> Result<serde_json::Value, String> {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    let output = cmd
        .current_dir(project)
        .args(args)
        .args(["--format", "json", "--quiet"])
        .output()
        .map_err(|err| err.to_string())?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!(
            "exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let doc: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "clone_groups": doc["dupes"]["clone_groups"],
        "stats": doc["dupes"]["stats"],
        "gate_outcomes": doc["gate_outcomes"],
    }))
}

/// Each supported pair of a global duplication override and a command
/// changes the clone groups. A command that accepts the override and keeps
/// its result, as `fallow audit --dupes-min-lines` did, fails.
#[test]
fn each_supported_dupes_override_changes_the_result() {
    let reference = run_fallow_raw(&["schema", "--cli-reference"]);
    let doc: serde_json::Value = serde_json::from_str(&reference.stdout).expect("reference JSON");
    let options: Vec<String> = doc["global_options"]
        .as_array()
        .expect("global_options")
        .iter()
        .filter_map(|option| option["long"].as_str())
        .filter(|long| long.starts_with("--dupes-"))
        .map(str::to_owned)
        .collect();
    assert!(options.len() > 1, "{options:?}");
    let option_refs: Vec<&str> = options.iter().map(String::as_str).collect();
    let pairs = supported_pairs_for(&option_refs);
    let mut failures = Vec::new();
    for pair in &pairs {
        let (option, command) = (&pair.option, &pair.command);
        let Some(case) = dupes_case(option) else {
            failures.push(format!(
                "{command} {option}: the test has no case for {option}"
            ));
            continue;
        };
        if case.generated && cfg!(not(unix)) && option.contains("symlinks") {
            continue;
        }
        let project = if case.generated {
            let dir = tempfile::tempdir().expect("project dir");
            generated_dupes_project(dir.path());
            dir
        } else {
            copy_fixture("duplicate-code")
        };
        if let Some(config) = case.config {
            std::fs::write(project.path().join(".fallowrc.json"), config).expect("config");
        }
        commit_project(project.path());
        let scratch = tempfile::tempdir().expect("scratch dir");
        let Some(mut args) = command_args(command, project.path(), scratch.path()) else {
            failures.push(format!(
                "{command} {option}: the test has no command line for {command}"
            ));
            continue;
        };
        if command == BARE_RUN {
            args.extend(["--only".to_owned(), "dupes".to_owned()]);
        }
        let mut with_option = args.clone();
        with_option.push(option.clone());
        with_option.extend(case.args.iter().map(|arg| (*arg).to_owned()));
        match (
            dupes_section(project.path(), &args),
            dupes_section(project.path(), &with_option),
        ) {
            (Ok(base), Ok(changed)) if base == changed => {
                failures.push(format!("{command} {option}: the result did not change"));
            }
            (Err(problem), _) | (_, Err(problem)) => {
                failures.push(format!("{command} {option}: {problem}"));
            }
            _ => {}
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
