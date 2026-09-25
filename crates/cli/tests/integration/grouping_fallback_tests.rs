//! `--group-by` on a format that cannot carry groups says so.
//!
//! The fallback itself is old and deliberate: the target renders one flat
//! document and the run still exits 0. What issue #2691 is about is that three
//! formats printed a note, the six a CI integration actually uses printed
//! nothing at all, and no target put the fact in what it rendered, so a
//! consumer that asked for groups received a document that is valid, complete
//! and not what it asked for.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, parse_json, run_fallow_raw};
use tempfile::TempDir;

/// The formats that drop grouping. `compact`, `markdown` and `badge` already
/// printed the note; the rest are the six silent arms.
const DROPPING_FORMATS: [&str; 8] = [
    "compact",
    "markdown",
    "pr-comment-github",
    "pr-comment-gitlab",
    "review-github",
    "review-gitlab",
    "github-annotations",
    "github-summary",
];

/// The four formats whose rendered body can carry the fact.
const BODY_FORMATS: [&str; 4] = [
    "pr-comment-github",
    "pr-comment-gitlab",
    "review-github",
    "review-gitlab",
];

fn project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/alpha")).expect("alpha dir");
    std::fs::create_dir_all(root.join("src/beta")).expect("beta dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"grouping-fx","version":"1.0.0","private":true,"main":"src/alpha/index.js"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/alpha/index.ts"),
        "export const main = (value: number): number => (value > 1 ? value : 0);\n",
    )
    .expect("entry");
    std::fs::write(
        root.join("src/beta/orphan.ts"),
        "export const orphan = (value: number): number => (value > 2 ? value : 1);\n",
    )
    .expect("orphan");
    dir
}

fn root_arg(dir: &TempDir) -> &str {
    dir.path().to_str().expect("temp path is UTF-8")
}

fn run_grouped(command: &str, root: &str, format: &str) -> CommandOutput {
    run_fallow_raw(&[
        command,
        "--root",
        root,
        "--group-by",
        "directory",
        "--format",
        format,
        "--quiet",
    ])
}

/// No format that drops grouping may do it silently, on either command that
/// offers the fallback.
#[test]
fn every_format_that_drops_grouping_says_so_on_stderr() {
    let project = project();
    let root = root_arg(&project);
    for command in ["health", "dupes"] {
        for format in DROPPING_FORMATS {
            let out = run_grouped(command, root, format);
            assert!(
                out.stderr.contains("--group-by directory is not supported"),
                "`{command} --format {format}` dropped grouping silently: {}",
                out.stderr
            );
            assert!(
                out.stderr.contains(&format!("for {format} output")),
                "the note must name the format the way --format spells it: {}",
                out.stderr
            );
        }
    }
}

/// The formats that DO carry grouping must stay silent, or the note becomes
/// noise that says nothing about this run.
#[test]
fn a_format_that_carries_grouping_prints_no_note() {
    let project = project();
    let root = root_arg(&project);
    for format in ["json", "sarif", "codeclimate"] {
        for command in ["health", "dupes"] {
            let out = run_grouped(command, root, format);
            assert!(
                !out.stderr.contains("--group-by"),
                "`{command} --format {format}` carries groups and must stay silent: {}",
                out.stderr
            );
        }
    }
    let grouped = parse_json(&run_grouped("health", root, "json"));
    assert_eq!(
        grouped["grouped_by"], "directory",
        "the JSON envelope still carries the grouping it was asked for"
    );
}

/// The rendered comment and review bodies are what a reviewer reads, and a
/// flat body that says nothing reads as the grouped report they asked for.
#[test]
fn the_rendered_body_states_the_requested_grouping() {
    let project = project();
    let root = root_arg(&project);
    for command in ["health", "dupes"] {
        for format in BODY_FORMATS {
            let out = run_grouped(command, root, format);
            assert!(
                out.stdout.contains(
                    "--group-by directory was requested and this format renders one flat document"
                ),
                "`{command} --format {format}` body must state it: {}",
                out.stdout
            );
        }
    }
}

/// An ungrouped run is byte-identical to one produced before the clause
/// existed, so the note never appears on a run that did not ask for groups.
#[test]
fn an_ungrouped_run_carries_no_clause_and_no_note() {
    let project = project();
    let root = root_arg(&project);
    for format in BODY_FORMATS {
        let out = run_fallow_raw(&["health", "--root", root, "--format", format, "--quiet"]);
        assert!(
            !out.stdout.contains("--group-by"),
            "`--format {format}` was asked for nothing: {}",
            out.stdout
        );
        assert!(!out.stderr.contains("--group-by"), "{}", out.stderr);
    }
}

/// `fallow report --from` renders the same body for the same envelope, which is
/// the contract the parity suite exists for. The saved envelope carries
/// `grouped_by`, so the re-render reaches the clause without being told.
#[test]
fn a_saved_grouped_envelope_renders_the_same_clause() {
    let project = project();
    let root = root_arg(&project);
    let envelope_path = project.path().join("health.json");
    let grouped = run_grouped("health", root, "json");
    std::fs::write(&envelope_path, &grouped.stdout).expect("save envelope");
    let saved_arg = envelope_path.to_str().expect("utf8");

    for format in BODY_FORMATS {
        let live = run_grouped("health", root, format);
        let saved = run_fallow_raw(&[
            "report", "--from", saved_arg, "--root", root, "--format", format, "--quiet",
        ]);
        assert_eq!(
            live.stdout, saved.stdout,
            "`report --from --format {format}` must be byte-identical to the live render"
        );
    }
}

/// The fallback stays a note rather than a verdict: nothing about it moves an
/// exit code.
#[test]
fn dropping_the_grouping_does_not_change_the_exit_code() {
    let project = project();
    let root = root_arg(&project);
    for format in DROPPING_FORMATS {
        let ungrouped = run_fallow_raw(&["health", "--root", root, "--format", format, "--quiet"]);
        let grouped = run_grouped("health", root, format);
        assert_eq!(
            ungrouped.code, grouped.code,
            "`--format {format}` must exit the same with and without --group-by"
        );
    }
}
