//! Health inputs that did not load reach `workspace_diagnostics[]`.
//!
//! Every one of these degradations used to exist as a printed line only, and
//! every shipped consumer runs fallow with `--quiet` and reads the envelope, so
//! a report whose scores were computed from nothing looked exactly like a
//! report of a clean project (issue #2689).
//!
//! These tests drive real runs into each condition rather than asserting a
//! constant: the kinds come from a project with no git repository, a corrupt
//! snapshot on disk, a malformed CODEOWNERS and a coverage file nobody asked
//! for. The quiet-parity case and the combined-mode case are the two that
//! would have caught the class.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw};
use serde_json::Value;
use std::path::Path;
use tempfile::TempDir;

/// The seven kinds this file is about. A clean run must carry none of them.
const HEALTH_KINDS: [&str; 7] = [
    "file-scores-unavailable",
    "hotspots-skipped",
    "shallow-clone",
    "unpinned-clock",
    "ownership-unavailable",
    "trend-snapshot-unreadable",
    "coverage-auto-detected",
];

/// A project with one function worth scoring, deliberately NOT a git
/// repository, which is the `hotspots-skipped` condition.
fn project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"health-fx","version":"1.0.0","private":true,"main":"src/index.js"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "export const main = (value: number): number => (value > 1 ? value : 0);\n",
    )
    .expect("entry");
    dir
}

fn root_arg(dir: &TempDir) -> &str {
    dir.path().to_str().expect("temp path is UTF-8")
}

fn diagnostics(envelope: &Value) -> &[Value] {
    envelope["workspace_diagnostics"]
        .as_array()
        .map_or(&[] as &[Value], Vec::as_slice)
}

fn diagnostic<'a>(envelope: &'a Value, kind: &str) -> &'a Value {
    diagnostics(envelope)
        .iter()
        .find(|entry| entry["kind"] == kind)
        .unwrap_or_else(|| {
            panic!(
                "expected a `{kind}` diagnostic, got {:?}",
                diagnostics(envelope)
                    .iter()
                    .map(|entry| entry["kind"].clone())
                    .collect::<Vec<_>>()
            )
        })
}

fn has_kind(envelope: &Value, kind: &str) -> bool {
    diagnostics(envelope)
        .iter()
        .any(|entry| entry["kind"] == kind)
}

fn health_json(root: &str, extra: &[&str]) -> Value {
    let mut args = vec!["health", "--root", root, "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    parse_json(&run_fallow_raw(&args))
}

/// The headline invariant of the whole family: a project whose inputs all
/// loaded carries none of these kinds, so a consumer that warns on them warns
/// about something.
#[test]
fn a_clean_health_run_carries_no_health_diagnostic() {
    let project = project();
    // A git repository with history, so neither `hotspots-skipped` nor
    // `shallow-clone` applies, and a pinned clock so the churn numbers are
    // reproducible and `unpinned-clock` does not apply either.
    let root = root_arg(&project);
    init_repo(project.path());
    let envelope = parse_json(&common::run_fallow_raw_with_env(
        &[
            "health",
            "--root",
            root,
            "--format",
            "json",
            "--quiet",
            "--hotspots",
        ],
        &[("FALLOW_CLOCK_EPOCH", "1700000000")],
    ));
    for kind in HEALTH_KINDS {
        assert!(
            !has_kind(&envelope, kind),
            "a clean run must not report `{kind}`: {}",
            envelope["workspace_diagnostics"]
        );
    }
}

/// Committing a real repository so the hotspot path has history to read.
fn init_repo(dir: &Path) {
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .status()
            .expect("git command failed");
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["commit", "-m", "initial"]);
}

/// A project that is not a repository reports that its churn-based sections
/// measured nothing, rather than reporting them as empty.
#[test]
fn a_project_without_a_repository_reports_hotspots_skipped() {
    let project = project();
    let envelope = health_json(root_arg(&project), &["--hotspots"]);
    let entry = diagnostic(&envelope, "hotspots-skipped");
    assert_eq!(entry["degrades_analysis"], true);
    assert_eq!(entry["path"], ".");
    assert_eq!(
        entry["cause"], "not-a-repository",
        "the cause decides the remedy and must reach the wire: {entry}"
    );
    let message = entry["message"].as_str().expect("a remedy sentence");
    assert!(
        message.contains("no git repository") && message.contains("--churn-file"),
        "the sentence must name the cause and a next step: {message}"
    );
}

/// A `--since` the run could not read as a window is a second way to lose the
/// same three sections, and it needs the opposite remedy: respell the flag
/// rather than move into a repository. Until now it warned through `tracing`
/// only, which every shipped consumer discards.
#[test]
fn a_malformed_since_reports_its_own_skip_cause() {
    let project = project();
    init_repo(project.path());
    let envelope = health_json(root_arg(&project), &["--hotspots", "--since", "nonsense"]);
    let entry = diagnostic(&envelope, "hotspots-skipped");
    assert_eq!(entry["cause"], "invalid-since", "{entry}");
    assert_eq!(entry["degrades_analysis"], true);
    let message = entry["message"].as_str().expect("a remedy sentence");
    assert!(
        message.contains("--since") && !message.contains("no git repository"),
        "a run inside a repository must not be told to move into one: {message}"
    );
    assert!(
        !message.contains('\n'),
        "the sentence travels into a CI annotation and must stay on one line: {message}"
    );
}

/// The three causes are one kind, so a consumer selecting on `degrades_analysis`
/// needs no change, and a consumer that wants the remedy reads `cause`.
#[test]
fn every_skip_cause_shares_the_kind_and_the_degraded_flag() {
    let repo = project();
    init_repo(repo.path());
    let no_repo = project();
    for (envelope, cause) in [
        (
            health_json(root_arg(&repo), &["--hotspots", "--since", "nonsense"]),
            "invalid-since",
        ),
        (
            health_json(root_arg(&no_repo), &["--hotspots"]),
            "not-a-repository",
        ),
    ] {
        let entry = diagnostic(&envelope, "hotspots-skipped");
        assert_eq!(entry["cause"], cause, "{entry}");
        assert_eq!(entry["degrades_analysis"], true, "{entry}");
    }
}

/// The parity case. `--quiet` removes the printed note and must not remove the
/// record, or the whole fix reaches none of the consumers that run fallow that
/// way.
#[test]
fn the_diagnostic_is_identical_with_and_without_quiet() {
    let project = project();
    let root = root_arg(&project);
    let quiet = parse_json(&run_fallow_raw(&[
        "health",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--hotspots",
    ]));
    let loud = parse_json(&run_fallow_raw(&[
        "health",
        "--root",
        root,
        "--format",
        "json",
        "--hotspots",
    ]));
    assert_eq!(
        diagnostic(&quiet, "hotspots-skipped"),
        diagnostic(&loud, "hotspots-skipped"),
        "the record must not depend on whether the note was printed"
    );
}

/// Combined mode re-loads config once per analysis, and the registry preserve
/// is what keeps a health-stage entry alive through that. This is the
/// regression guard on the `is_health_stage` classification: without it a bare
/// run reports nothing while `fallow health` reports the degradation.
#[test]
fn a_bare_combined_run_carries_the_health_diagnostic_at_the_root() {
    let project = project();
    let envelope = parse_json(&run_fallow_raw(&[
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
    ]));
    assert!(
        has_kind(&envelope, "hotspots-skipped"),
        "the combined envelope's root must carry it: {}",
        envelope["workspace_diagnostics"]
    );
}

/// A snapshot the trend could not read names the file, because the remedy is
/// to delete or rewrite that one file.
#[test]
fn an_unreadable_snapshot_reports_the_file_it_skipped() {
    let project = project();
    let snapshots = project.path().join(".fallow").join("snapshots");
    std::fs::create_dir_all(&snapshots).expect("snapshot dir");
    std::fs::write(snapshots.join("broken.json"), "{ not json").expect("corrupt snapshot");
    let envelope = health_json(root_arg(&project), &["--trend"]);
    let entry = diagnostic(&envelope, "trend-snapshot-unreadable");
    assert_eq!(entry["degrades_analysis"], true);
    assert_eq!(
        entry["path"], ".fallow/snapshots/broken.json",
        "the path is project-relative: {entry}"
    );
}

/// A malformed CODEOWNERS degrades ownership rather than failing the run, and
/// the cause distinguishes it from the other ownership input.
#[test]
fn a_malformed_codeowners_reports_ownership_unavailable() {
    let project = project();
    init_repo(project.path());
    // An unclosed character class: the pattern reaches the glob compiler and
    // fails there, which is the parse failure this kind reports.
    std::fs::write(project.path().join("CODEOWNERS"), "src/[unclosed @team\n").expect("codeowners");
    let envelope = health_json(root_arg(&project), &["--hotspots", "--ownership"]);
    let entry = diagnostic(&envelope, "ownership-unavailable");
    assert_eq!(entry["degrades_analysis"], true);
    assert_eq!(entry["cause"], "codeowners-parse-failed");
}

/// Auto-detected coverage is provenance, not a degradation: it names the file
/// that fed the scores and must NOT set `degrades_analysis`, or every project
/// that is happy with auto-detection would report a degraded run forever.
#[test]
fn auto_detected_coverage_names_its_file_and_does_not_degrade_the_run() {
    let project = project();
    let coverage = project.path().join("coverage");
    std::fs::create_dir_all(&coverage).expect("coverage dir");
    std::fs::write(coverage.join("coverage-final.json"), "{}").expect("coverage file");
    let envelope = health_json(root_arg(&project), &["--file-scores"]);
    let entry = diagnostic(&envelope, "coverage-auto-detected");
    assert!(
        entry.get("degrades_analysis").is_none(),
        "provenance is not a degraded run: {entry}"
    );
    assert_eq!(entry["path"], "coverage/coverage-final.json");
    let message = entry["message"].as_str().expect("a remedy sentence");
    assert!(
        message.contains("--coverage"),
        "the sentence must name the flag that makes the score reproducible: {message}"
    );
}

/// Explicit coverage is what the user asked for, so nothing is recorded about
/// it. Without this the provenance entry would report every run that passes
/// `--coverage` as well, which says nothing.
#[test]
fn explicit_coverage_reports_no_provenance_diagnostic() {
    let project = project();
    let coverage = project.path().join("coverage");
    std::fs::create_dir_all(&coverage).expect("coverage dir");
    std::fs::write(coverage.join("coverage-final.json"), "{}").expect("coverage file");
    let envelope = health_json(
        root_arg(&project),
        &[
            "--file-scores",
            "--coverage",
            "coverage/coverage-final.json",
        ],
    );
    assert!(
        !has_kind(&envelope, "coverage-auto-detected"),
        "an explicit input is not provenance worth reporting: {}",
        envelope["workspace_diagnostics"]
    );
}
