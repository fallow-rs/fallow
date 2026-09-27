//! The opt-in `baseline-growth` gate (issue #2938).
//!
//! A change can add a finding and re-save the baseline in the same commit.
//! Then `--baseline` suppresses the new finding and `--fail-on-stale-baseline`
//! passes. `--fail-on-baseline-growth` compares the baseline file with the same
//! file at a base ref and fails when the working-tree file has a key that the
//! base file does not have.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_raw};
use serde_json::Value;
use tempfile::TempDir;

const BASELINE: &str = "fallow-baseline.json";

/// A project with one unused file, `src/old.ts`, on a `main` branch.
fn project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"growth-fx","version":"1.0.0","private":true,"main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(root.join("src/index.ts"), "console.log('entry');\n").expect("entry");
    std::fs::write(root.join("src/old.ts"), "export const old = 1;\n").expect("old file");
    git(root, &["init", "-b", "main"]);
    dir
}

fn root_arg(dir: &TempDir) -> &str {
    dir.path().to_str().expect("temp path is UTF-8")
}

fn baseline_arg(dir: &TempDir) -> String {
    dir.path().join(BASELINE).display().to_string()
}

fn save_dead_code_baseline(dir: &TempDir) {
    let baseline = baseline_arg(dir);
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root_arg(dir),
        "--quiet",
        "--no-cache",
        "--save-baseline",
        &baseline,
    ]);
    assert!(
        Path::new(&baseline).is_file(),
        "the save wrote no baseline: {}",
        output.stderr
    );
}

/// Commit the baseline on `main`, then start a feature branch.
fn committed_baseline_on_main() -> TempDir {
    let dir = project();
    save_dead_code_baseline(&dir);
    commit_all(dir.path(), "save baseline");
    git(dir.path(), &["checkout", "-b", "feature"]);
    dir
}

fn run_dead_code(dir: &TempDir, extra: &[&str]) -> (CommandOutput, Value) {
    let baseline = baseline_arg(dir);
    let mut args = vec![
        "dead-code",
        "--root",
        root_arg(dir),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--baseline",
        &baseline,
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_raw(&args);
    let json = parse_json(&output);
    (output, json)
}

#[test]
fn a_baseline_that_gained_an_entry_fails_and_names_the_new_key() {
    let dir = committed_baseline_on_main();
    std::fs::write(dir.path().join("src/new.ts"), "export const fresh = 2;\n").expect("new file");
    save_dead_code_baseline(&dir);
    commit_all(dir.path(), "add a finding and re-save the baseline");

    // Without the gate, every other check passes: the re-saved baseline
    // suppresses the new finding and has no stale entry.
    let (quiet_run, _) = run_dead_code(&dir, &["--fail-on-stale-baseline"]);
    assert_eq!(quiet_run.code, 0, "stderr: {}", quiet_run.stderr);

    let (output, json) = run_dead_code(
        &dir,
        &["--fail-on-baseline-growth", "--baseline-base", "main"],
    );
    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("Baseline growth gate failed"),
        "stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("unused_files: src/new.ts"),
        "the message lists the new key: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("src/old.ts"),
        "a key that the base has is not growth: {}",
        output.stderr
    );
    let entry = &json["gate_outcomes"]["baseline-growth"];
    assert_eq!(entry["status"], "fail", "entry: {entry}");
    assert_eq!(entry["enforced"], true, "entry: {entry}");
    assert_eq!(entry["observed"], 1.0, "entry: {entry}");
}

#[test]
fn a_baseline_that_only_lost_entries_passes() {
    let dir = committed_baseline_on_main();
    std::fs::remove_file(dir.path().join("src/old.ts")).expect("remove old file");
    save_dead_code_baseline(&dir);
    commit_all(dir.path(), "fix the finding and re-save the baseline");

    let (output, json) = run_dead_code(
        &dir,
        &["--fail-on-baseline-growth", "--baseline-base", "main"],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let entry = &json["gate_outcomes"]["baseline-growth"];
    assert_eq!(entry["status"], "pass", "entry: {entry}");
    assert_eq!(entry["observed"], 0.0, "entry: {entry}");
}

#[test]
fn a_baseline_that_the_base_ref_does_not_have_passes_with_a_note() {
    let dir = project();
    commit_all(dir.path(), "initial");
    git(dir.path(), &["checkout", "-b", "feature"]);
    save_dead_code_baseline(&dir);
    commit_all(dir.path(), "adopt a baseline");

    let (output, json) = run_dead_code(
        &dir,
        &["--fail-on-baseline-growth", "--baseline-base", "main"],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("is a new baseline"),
        "stderr: {}",
        output.stderr
    );
    assert_eq!(json["gate_outcomes"]["baseline-growth"]["status"], "pass");
}

#[test]
fn a_base_ref_that_git_cannot_resolve_exits_2_and_names_the_fetch() {
    let dir = committed_baseline_on_main();
    let baseline = baseline_arg(&dir);
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root_arg(&dir),
        "--quiet",
        "--no-cache",
        "--baseline",
        &baseline,
        "--fail-on-baseline-growth",
        "--baseline-base",
        "origin/missing",
    ]);
    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("fetch-depth: 0")
            && output.stderr.contains("git fetch origin missing"),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn an_implicit_base_that_resolves_to_head_exits_2() {
    let dir = committed_baseline_on_main();
    std::fs::write(dir.path().join("src/new.ts"), "export const fresh = 2;\n").expect("new file");
    save_dead_code_baseline(&dir);
    commit_all(dir.path(), "add a finding and re-save the baseline");

    // A comparison with HEAD cannot see the committed growth, so the gate
    // must refuse to run instead of a pass.
    let baseline = baseline_arg(&dir);
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root_arg(&dir),
        "--quiet",
        "--no-cache",
        "--baseline",
        &baseline,
        "--fail-on-baseline-growth",
        "--changed-since",
        "HEAD",
    ]);
    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("(HEAD)") && output.stderr.contains("--baseline-base origin/main"),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn the_flags_need_a_baseline_and_each_other() {
    let dir = committed_baseline_on_main();
    let no_baseline = run_fallow_raw(&[
        "dead-code",
        "--root",
        root_arg(&dir),
        "--quiet",
        "--no-cache",
        "--fail-on-baseline-growth",
    ]);
    assert_eq!(no_baseline.code, 2, "stderr: {}", no_baseline.stderr);
    assert!(
        no_baseline
            .stderr
            .contains("--fail-on-baseline-growth needs a baseline"),
        "stderr: {}",
        no_baseline.stderr
    );

    let (base_alone, error) = run_dead_code(&dir, &["--baseline-base", "main"]);
    assert_eq!(base_alone.code, 2, "stderr: {}", base_alone.stderr);
    assert!(
        error["message"].as_str().is_some_and(
            |message| message.contains("--baseline-base needs --fail-on-baseline-growth")
        ),
        "error: {error}"
    );
}

#[test]
fn audit_judges_its_dead_code_baseline_against_the_base() {
    let dir = committed_baseline_on_main();
    std::fs::write(dir.path().join("src/new.ts"), "export const fresh = 2;\n").expect("new file");
    save_dead_code_baseline(&dir);
    commit_all(dir.path(), "add a finding and re-save the baseline");

    let baseline = baseline_arg(&dir);
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root_arg(&dir),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--base",
        "main",
        "--dead-code-baseline",
        &baseline,
        "--fail-on-baseline-growth",
    ]);
    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("unused_files: src/new.ts"),
        "stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["gate_outcomes"]["baseline-growth"]["status"], "fail",
        "gates: {}",
        json["gate_outcomes"]
    );
}

#[test]
fn a_command_without_a_baseline_rejects_the_flag() {
    let dir = committed_baseline_on_main();
    let output = run_fallow_raw(&[
        "list",
        "--root",
        root_arg(&dir),
        "--quiet",
        "--fail-on-baseline-growth",
    ]);
    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
}

/// Each command that loads a baseline publishes the gate in its own envelope.
#[test]
fn dupes_health_and_the_bare_run_publish_the_gate() {
    let dir = project();
    for command in ["dupes", "health"] {
        let path = dir.path().join(format!("{command}-baseline.json"));
        let path = path.display().to_string();
        let saved = run_fallow_raw(&[
            command,
            "--root",
            root_arg(&dir),
            "--quiet",
            "--no-cache",
            "--save-baseline",
            &path,
        ]);
        assert!(Path::new(&path).is_file(), "{command}: {}", saved.stderr);
    }
    save_dead_code_baseline(&dir);
    commit_all(dir.path(), "save baselines");

    for command in ["dupes", "health"] {
        let path = dir.path().join(format!("{command}-baseline.json"));
        let path = path.display().to_string();
        let output = run_fallow_raw(&[
            command,
            "--root",
            root_arg(&dir),
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--baseline",
            &path,
            "--fail-on-baseline-growth",
            "--baseline-base",
            "main",
        ]);
        assert_eq!(output.code, 0, "{command}: {}", output.stderr);
        let json = parse_json(&output);
        assert_eq!(
            json["gate_outcomes"]["baseline-growth"]["status"], "pass",
            "{command}: {}",
            json["gate_outcomes"]
        );
    }

    std::fs::write(dir.path().join("src/new.ts"), "export const fresh = 2;\n").expect("new file");
    save_dead_code_baseline(&dir);
    let baseline = baseline_arg(&dir);
    let output = run_fallow_raw(&[
        "--root",
        root_arg(&dir),
        "--only",
        "dead-code",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--baseline",
        &baseline,
        "--fail-on-baseline-growth",
        "--baseline-base",
        "main",
    ]);
    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(
        json["gate_outcomes"]["baseline-growth"]["status"], "fail",
        "gates: {}",
        json["gate_outcomes"]
    );
    assert!(
        json["check"]["gate_outcomes"]
            .get("baseline-growth")
            .is_none(),
        "the section does not repeat the root entry: {}",
        json["check"]
    );
}
