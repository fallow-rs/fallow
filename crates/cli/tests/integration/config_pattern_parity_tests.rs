//! Parity of the unmatched config pattern entries (`ignoreDependencies`,
//! `ignoreFindings`) across every command, every output format and
//! `fallow report --from`.
//!
//! Each format carries the entries in exactly one place: the document itself
//! (JSON, SARIF, Markdown, the job summary and the PR comment) or a stderr note
//! (every other format). A live run and a re-render of its saved envelope use
//! the same place.

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_raw};

const UNMATCHED_GLOB: &str = "@acm/*";
const UNMATCHED_FINDING_PATTERN: &str = "src/hiden.ts";

/// Where a format carries the unmatched config patterns.
#[derive(Clone, Copy, Debug)]
enum Carrier {
    /// A `workspace_diagnostics[]` entry in the JSON document.
    Json,
    /// A SARIF `toolConfigurationNotifications[]` entry.
    Sarif,
    /// A Markdown line in the document on stdout.
    Markdown,
    /// A note on stderr, with nothing on stdout.
    StderrNote,
}

const fn carrier(format: &str) -> Carrier {
    match format.as_bytes() {
        b"json" => Carrier::Json,
        b"sarif" => Carrier::Sarif,
        b"markdown" | b"github-summary" | b"pr-comment-github" | b"pr-comment-gitlab" => {
            Carrier::Markdown
        }
        _ => Carrier::StderrNote,
    }
}

/// A git project with one used `@acme/*` dependency, one unmatched
/// `ignoreDependencies` glob, one unmatched `ignoreFindings` pattern and one
/// uncommitted change, so `fallow audit --base HEAD` has a scope.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"config-patterns","private":true,"main":"src/index.ts",
            "dependencies":{"@acme/lib":"1.0.0"}}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(
            r#"{{"ignoreDependencies":["@acme/*","{UNMATCHED_GLOB}"],
                "ignoreFindings":["{UNMATCHED_FINDING_PATTERN}"]}}"#
        ),
    )
    .expect("write config");
    std::fs::write(
        root.join("src/index.ts"),
        "import '@acme/lib';\nexport const main = 1;\n",
    )
    .expect("write entry point");
    std::fs::write(root.join("src/orphan.ts"), "export const orphan = 1;\n")
        .expect("write orphan file");
    git(root, &["init", "-q"]);
    commit_all(root, "initial");
    std::fs::write(
        root.join("src/orphan.ts"),
        "export const orphan = 1;\nexport const second = 2;\n",
    )
    .expect("change orphan file");
    dir
}

/// The command line of one command in `root`: `dead-code`, the combined run
/// (no subcommand) or `audit`.
fn command_args(command: &str, root: &str, format: &str) -> Vec<String> {
    let mut args = match command {
        "combined" => Vec::new(),
        "audit" => vec!["audit".to_owned(), "--base".to_owned(), "HEAD".to_owned()],
        other => vec![other.to_owned()],
    };
    args.extend(
        ["--root", root, "--format", format]
            .iter()
            .map(|arg| (*arg).to_owned()),
    );
    args
}

fn run(args: &[String]) -> CommandOutput {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_fallow_raw(&args)
}

/// `(kind, pattern)` of each unmatched config pattern in a JSON envelope,
/// wherever the command nests the dead-code diagnostics.
fn json_entries(json: &serde_json::Value) -> Vec<(String, String)> {
    let diagnostics = json
        .pointer("/dead_code/workspace_diagnostics")
        .or_else(|| json.get("workspace_diagnostics"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    diagnostics
        .iter()
        .filter_map(|diagnostic| {
            let kind = diagnostic["kind"].as_str()?;
            kind.ends_with("-unmatched").then(|| {
                (
                    kind.to_owned(),
                    diagnostic["pattern"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                )
            })
        })
        .collect()
}

fn sarif_entries(sarif: &serde_json::Value) -> Vec<(String, String)> {
    sarif["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|run| {
            run.pointer("/invocations/0/toolConfigurationNotifications")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .map(|notification| {
            (
                notification["descriptor"]["id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                notification["properties"]["pattern"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

fn expected_entries() -> Vec<(String, String)> {
    vec![
        (
            "ignore-findings-pattern-unmatched".to_owned(),
            UNMATCHED_FINDING_PATTERN.to_owned(),
        ),
        (
            "ignore-dependencies-glob-unmatched".to_owned(),
            UNMATCHED_GLOB.to_owned(),
        ),
    ]
}

fn assert_carried(output: &CommandOutput, format: &str, label: &str) {
    let context = format!(
        "{label} --format {format}\nstdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let has_note = output.stderr.contains("Note: ignoreFindings pattern")
        && output.stderr.contains("Note: ignoreDependencies glob");
    let any_note = output.stderr.contains("Note: ignoreFindings")
        || output.stderr.contains("Note: ignoreDependencies");
    match carrier(format) {
        Carrier::Json => {
            assert_eq!(
                json_entries(&parse_json(output)),
                expected_entries(),
                "{context}"
            );
            assert!(!any_note, "{context}");
        }
        Carrier::Sarif => {
            assert_eq!(
                sarif_entries(&parse_json(output)),
                expected_entries(),
                "{context}"
            );
            assert!(!any_note, "{context}");
        }
        Carrier::Markdown => {
            for (setting, pattern) in [
                ("ignoreFindings", UNMATCHED_FINDING_PATTERN),
                ("ignoreDependencies", UNMATCHED_GLOB),
            ] {
                let line = format!("- `{setting}`: `{pattern}` matched nothing in this run");
                assert!(output.stdout.contains(&line), "missing `{line}`: {context}");
            }
            assert!(
                output.stdout.contains("Unmatched config patterns"),
                "{context}"
            );
            assert!(!any_note, "{context}");
        }
        Carrier::StderrNote => {
            assert!(has_note, "{context}");
            assert!(
                !output.stdout.contains(UNMATCHED_GLOB)
                    && !output.stdout.contains(UNMATCHED_FINDING_PATTERN),
                "the note stays off stdout: {context}"
            );
        }
    }
}

/// Every format of the live dead-code run.
const DEAD_CODE_FORMATS: &[&str] = &[
    "human",
    "json",
    "sarif",
    "markdown",
    "compact",
    "codeclimate",
    "github-annotations",
    "github-summary",
    "pr-comment-github",
    "pr-comment-gitlab",
    "review-github",
    "review-gitlab",
];

/// One format per render path of the combined run and `fallow audit`. They
/// render human, compact and markdown through the dead-code printer, and
/// each other format through a printer of their own.
const COMBINED_AND_AUDIT_FORMATS: &[&str] = &[
    "human",
    "json",
    "sarif",
    "codeclimate",
    "github-annotations",
    "github-summary",
    "pr-comment-github",
    "review-gitlab",
];

const REPORT_FORMATS: &[&str] = &[
    "sarif",
    "codeclimate",
    "github-annotations",
    "github-summary",
    "pr-comment-github",
    "pr-comment-gitlab",
    "review-github",
    "review-gitlab",
];

const COMMANDS: &[&str] = &["dead-code", "combined", "audit"];

fn live_formats(command: &str) -> &'static [&'static str] {
    if command == "dead-code" {
        DEAD_CODE_FORMATS
    } else {
        COMBINED_AND_AUDIT_FORMATS
    }
}

#[test]
fn unmatched_config_patterns_reach_every_live_format_of_every_command() {
    let dir = project();
    let root = dir.path().to_string_lossy().into_owned();
    // One thread per command keeps the wall time of the slow audit runs low.
    std::thread::scope(|scope| {
        for command in COMMANDS {
            let root = &root;
            scope.spawn(move || {
                for format in live_formats(command) {
                    let output = run(&command_args(command, root, format));
                    assert_carried(&output, format, command);
                }
            });
        }
    });
}

#[test]
fn unmatched_config_patterns_reach_every_format_of_report_from() {
    let dir = project();
    let root = dir.path().to_string_lossy().into_owned();
    for command in COMMANDS {
        let saved = run(&command_args(command, &root, "json"));
        assert_eq!(json_entries(&parse_json(&saved)), expected_entries());
        let saved_path = dir.path().join(format!("{command}.json"));
        std::fs::write(&saved_path, &saved.stdout).expect("write saved envelope");
        let saved_path = saved_path.to_string_lossy().into_owned();
        for format in REPORT_FORMATS {
            let output = run(&[
                "report".to_owned(),
                "--from".to_owned(),
                saved_path.clone(),
                "--root".to_owned(),
                root.clone(),
                "--format".to_owned(),
                (*format).to_owned(),
            ]);
            assert_carried(&output, format, &format!("report --from {command}"));
        }
    }
}
