//! A loaded baseline is judged against the scope the file records (issue #4070).
//!
//! `--fail-on-stale-baseline` and `fallow baselines prune` compare the run with
//! the file, channel by channel. A run in the mode of the file can judge it, and
//! a run in another mode cannot. A prune in another mode would remove valid
//! entries, so it refuses and leaves the file as it is.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::common::{CommandOutput, parse_json, run_fallow_raw};
use serde_json::Value;
use tempfile::TempDir;

const LIB: &str = "export function run() {\n  return 1;\n}\n\n\
                   export const onlyTestedA = 1;\nexport const onlyTestedB = 2;\n";
const LIB_FIXED: &str =
    "export function run() {\n  return 1;\n}\n\nexport const onlyTestedA = 1;\n";
const LIB_TEST: &str = "import { onlyTestedA, onlyTestedB } from './lib.js';\n\n\
                        console.log(onlyTestedA, onlyTestedB);\n";
const LIB_TEST_FIXED: &str =
    "import { onlyTestedA } from './lib.js';\n\nconsole.log(onlyTestedA);\n";
const REMOVED_ENTRY: &str = "onlyTestedB";
/// Findings stay at warn, so the exit code reports the stale-baseline gate only.
const RULES: &str = r#""rules":{"unused-files":"warn","unused-exports":"warn"}"#;

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// The project of the report: two exports that only a test file keeps alive.
fn project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"repro","private":true,"type":"module","main":"src/index.js"}"#,
    );
    write(root, "src/index.js", "export { run } from './lib.js';\n");
    write(root, "src/lib.js", LIB);
    write(root, "src/lib.test.js", LIB_TEST);
    write(root, ".fallowrc.json", &format!("{{{RULES}}}"));
    dir
}

fn write_production_config(dir: &TempDir, production: &str) {
    write(
        dir.path(),
        ".fallowrc.json",
        &format!(r#"{{"production":{production},{RULES}}}"#),
    );
}

fn fix_one_entry(dir: &TempDir) {
    write(dir.path(), "src/lib.js", LIB_FIXED);
    write(dir.path(), "src/lib.test.js", LIB_TEST_FIXED);
}

fn root_arg(dir: &TempDir) -> &str {
    dir.path().to_str().expect("temp path is UTF-8")
}

fn baseline_path(dir: &TempDir) -> PathBuf {
    dir.path().join("baseline.json")
}

fn fallow(dir: &TempDir, args: &[&str]) -> CommandOutput {
    let mut full = args.to_vec();
    full.extend_from_slice(&["--root", root_arg(dir), "--no-cache"]);
    run_fallow_raw(&full)
}

fn save(dir: &TempDir, command: &str, flags: &[&str]) -> PathBuf {
    let path = baseline_path(dir);
    let mut args = vec![command, "--save-baseline", path.to_str().unwrap()];
    args.extend_from_slice(flags);
    let output = fallow(dir, &args);
    assert!(path.is_file(), "no baseline saved: {}", output.stderr);
    path
}

/// A gated dead-code run, in JSON.
fn gate(dir: &TempDir, flags: &[&str]) -> (CommandOutput, Value) {
    gate_command(dir, "dead-code", flags)
}

fn gate_command(dir: &TempDir, command: &str, flags: &[&str]) -> (CommandOutput, Value) {
    let path = baseline_path(dir);
    let mut args = vec![
        command,
        "--baseline",
        path.to_str().unwrap(),
        "--fail-on-stale-baseline",
        "--format",
        "json",
    ];
    args.extend_from_slice(flags);
    let output = fallow(dir, &args);
    let json = parse_json(&output);
    (output, json)
}

fn prune(dir: &TempDir, kind_flag: &str, flags: &[&str]) -> (CommandOutput, Value) {
    // A prune flag resolves a relative path against the project root.
    let mut args = vec![
        "baselines",
        "prune",
        kind_flag,
        "baseline.json",
        "--format",
        "json",
    ];
    args.extend_from_slice(flags);
    let output = fallow(dir, &args);
    let json = parse_json(&output);
    (output, json)
}

fn prune_dead_code(dir: &TempDir, flags: &[&str]) -> (CommandOutput, Value) {
    prune(dir, "--dead-code-baseline", flags)
}

fn scope_reasons(json: &Value) -> Vec<String> {
    json["baseline_staleness"]["scope_reasons"]
        .as_array()
        .map(|reasons| {
            reasons
                .iter()
                .map(|reason| reason.as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn step<'a>(json: &'a Value, id: &str) -> Option<&'a Value> {
    json["next_steps"]
        .as_array()?
        .iter()
        .find(|step| step["id"] == id)
}

fn action<'a>(json: &'a Value, kind: &str) -> &'a Value {
    json["actions"]
        .as_array()
        .expect("actions array")
        .iter()
        .find(|action| action["type"] == kind)
        .unwrap_or_else(|| panic!("no {kind} action in {json}"))
}

fn removed_keys(json: &Value) -> Vec<String> {
    json["files"][0]["removed"]
        .as_array()
        .expect("removed array")
        .iter()
        .map(|entry| entry["key"].as_str().unwrap().to_owned())
        .collect()
}

fn assert_refused(path: &Path, before: &str, json: &Value) {
    assert_eq!(json["files"][0]["status"], "refused", "{json}");
    assert_eq!(json["files"][0]["reason_code"], "scope-mismatch", "{json}");
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        before,
        "a refusal leaves the file byte-identical"
    );
}

/// The exact commands of the report: same mode, same filter, one fixed entry.
#[test]
fn a_same_mode_run_gates_a_production_baseline_with_the_same_filter() {
    let dir = project();
    save(&dir, "dead-code", &["--production", "--unused-exports"]);
    fix_one_entry(&dir);

    let (output, json) = gate(&dir, &["--production", "--unused-exports"]);

    assert_eq!(output.code, 1, "{}", output.stderr);
    assert!(
        output
            .stderr
            .contains("Baseline gate failed: 1 of 2 entries"),
        "{}",
        output.stderr
    );
    let staleness = &json["baseline_staleness"];
    assert_eq!(staleness["change_scoped"], false, "{json}");
    assert_eq!(staleness["gate_trips"], true, "{json}");
    assert_eq!(staleness["stale_entries"], 1, "{json}");
    assert!(scope_reasons(&json).is_empty(), "{json}");
    assert_eq!(json["gate_outcomes"]["stale-baseline"]["status"], "fail");
}

/// The gate failure prints the remedy, and the remedy works when pasted: it
/// removes the fixed entry and nothing else.
#[test]
fn the_command_in_the_gate_failure_removes_only_the_fixed_entry() {
    let dir = project();
    save(&dir, "dead-code", &["--production", "--unused-exports"]);
    fix_one_entry(&dir);

    let (output, _) = gate(&dir, &["--production", "--unused-exports"]);
    let printed = output
        .stderr
        .split("Remove only the stale entries with: ")
        .nth(1)
        .and_then(|rest| rest.split(", or re-save").next())
        .unwrap_or_else(|| panic!("no remedy in {}", output.stderr));
    let mut args: Vec<&str> = printed.split_whitespace().skip(1).collect();
    args.extend_from_slice(&["--format", "json"]);
    let pruned = fallow(&dir, &args);

    assert_eq!(pruned.code, 0, "{printed}: {}", pruned.stdout);
    let json = parse_json(&pruned);
    let removed = removed_keys(&json);
    assert_eq!(removed.len(), 1, "{json}");
    assert!(removed[0].contains(REMOVED_ENTRY), "{json}");
    let (rerun, _) = gate(&dir, &["--production", "--unused-exports"]);
    assert_eq!(rerun.code, 0, "{}", rerun.stderr);
}

/// Case A: equal modes and no fix compare cleanly.
#[test]
fn a_same_mode_run_passes_a_healthy_production_baseline() {
    let dir = project();
    save(&dir, "dead-code", &["--production"]);

    let (output, json) = gate(&dir, &["--production"]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(json["baseline_staleness"]["change_scoped"], false, "{json}");
    assert_eq!(json["baseline_staleness"]["stale_entries"], 0, "{json}");
    assert_eq!(json["gate_outcomes"]["stale-baseline"]["status"], "pass");
    assert!(!output.stderr.contains("did not run"), "{}", output.stderr);
}

/// The stale advisory reads the same comparison, and its remedy keeps the mode.
#[test]
fn the_stale_advisory_of_a_same_mode_run_keeps_the_mode_in_its_remedy() {
    let dir = project();
    save(&dir, "dead-code", &["--production"]);
    fix_one_entry(&dir);

    let path = baseline_path(&dir);
    let output = fallow(
        &dir,
        &[
            "dead-code",
            "--production",
            "--baseline",
            path.to_str().unwrap(),
        ],
    );

    assert!(
        output.stderr.contains("partially stale"),
        "{}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("fallow baselines prune --production"),
        "{}",
        output.stderr
    );
}

/// Case B: a file with no production record stays inert under a production run.
#[test]
fn a_production_run_does_not_judge_a_file_without_a_production_record() {
    let dir = project();
    save(&dir, "dead-code", &[]);

    let (output, json) = gate(&dir, &["--production"]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(scope_reasons(&json), ["production"], "{json}");
    assert_eq!(json["gate_outcomes"]["stale-baseline"]["status"], "skipped");
    assert!(
        output.stderr.contains(
            "does not record production mode and this run uses it. Save it again with \
             --production, or run without --production, to gate on it."
        ),
        "{}",
        output.stderr
    );
}

/// A production file saved before the file recorded its scope looks the same
/// on disk, so it stays inert too.
#[test]
fn a_production_baseline_without_a_saved_scope_stays_skipped() {
    let dir = project();
    let path = save(&dir, "dead-code", &["--production"]);
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("scope_reasons");
    std::fs::write(&path, value.to_string()).unwrap();

    let (output, json) = gate(&dir, &["--production"]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(scope_reasons(&json), ["production"], "{json}");
    assert!(
        output.stderr.contains("does not record production mode"),
        "{}",
        output.stderr
    );
}

/// Case C: a whole-project run sees findings that a production file never held.
/// The gate failed here, and its remedy removed every valid entry.
#[test]
fn a_run_without_production_does_not_fail_on_a_production_baseline() {
    let dir = project();
    save(&dir, "dead-code", &["--production"]);

    let (output, json) = gate(&dir, &[]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        !output.stderr.contains("Baseline gate failed"),
        "{}",
        output.stderr
    );
    assert_eq!(scope_reasons(&json), ["production"], "{json}");
    assert_eq!(json["baseline_staleness"]["gate_trips"], false, "{json}");
    assert_eq!(json["gate_outcomes"]["stale-baseline"]["status"], "skipped");
    assert!(
        output.stderr.contains(
            "was saved with --production and this run does not use it. Add --production to gate \
             on it."
        ),
        "{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("Save it again from a run without"),
        "the two lines must give one remedy: {}",
        output.stderr
    );
}

/// An agent cannot read the direction from `scope_reasons`, so the step carries it.
#[test]
fn the_recheck_step_of_a_run_without_production_adds_the_flag() {
    let dir = project();
    save(&dir, "dead-code", &["--production"]);

    let (_, json) = gate(&dir, &[]);

    let recheck = step(&json, "recheck-baseline").unwrap_or_else(|| panic!("no step in {json}"));
    let command = recheck["command"].as_str().unwrap();
    assert!(command.ends_with(" --production"), "{command}");
    assert_eq!(
        recheck["reason"],
        "the baseline was saved with --production and this run was not"
    );
}

/// Case D: equal reason names do not mean equal scope.
#[test]
fn a_filter_that_does_not_cover_the_file_stays_skipped() {
    let dir = project();
    save(&dir, "dead-code", &["--production", "--unused-exports"]);

    let (output, json) = gate(&dir, &["--production", "--unused-files"]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(scope_reasons(&json), ["issue-type-filter"], "{json}");
    assert!(
        output.stderr.contains(
            "holds unused_exports, which this run does not report. Re-run without the issue-type \
             filter"
        ),
        "{}",
        output.stderr
    );
}

/// A covering filter drops nothing from the comparison, so a filtered run and
/// an unfiltered run report the same counts on one file.
#[test]
fn a_covering_filter_matches_the_same_entries_as_an_unfiltered_run() {
    let dir = project();
    save(&dir, "dead-code", &["--production", "--unused-exports"]);
    fix_one_entry(&dir);

    let (_, filtered) = gate(&dir, &["--production", "--unused-exports"]);
    let (_, unfiltered) = gate(&dir, &["--production"]);

    assert_eq!(
        filtered["baseline_staleness"]["matched_entries"],
        unfiltered["baseline_staleness"]["matched_entries"]
    );
    assert_eq!(filtered["baseline_staleness"]["change_scoped"], false);
    assert_eq!(unfiltered["baseline_staleness"]["change_scoped"], false);
}

/// A filter-only run judges a whole-project file that holds only what it reports.
#[test]
fn a_filter_only_run_gates_a_file_it_covers() {
    let dir = project();
    std::fs::remove_file(dir.path().join("src/lib.test.js")).unwrap();
    save(&dir, "dead-code", &[]);
    write(dir.path(), "src/lib.js", LIB_FIXED);

    let (output, json) = gate(&dir, &["--unused-exports"]);

    assert_eq!(output.code, 1, "{}", output.stderr);
    assert!(scope_reasons(&json).is_empty(), "{json}");
}

/// A diff still narrows the run. The recheck keeps the mode of the file, or it
/// comes back inert.
#[test]
fn a_diff_scoped_production_run_stays_skipped_and_its_recheck_keeps_the_mode() {
    let dir = project();
    save(&dir, "dead-code", &["--production"]);
    let diff = dir.path().join("scope.patch");
    std::fs::write(
        &diff,
        "diff --git a/src/index.js b/src/index.js\n--- a/src/index.js\n+++ b/src/index.js\n\
         @@ -1 +1 @@\n-x\n+y\n",
    )
    .unwrap();

    let (output, json) = gate(
        &dir,
        &["--production", "--diff-file", diff.to_str().unwrap()],
    );

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(scope_reasons(&json), ["diff"], "{json}");
    assert_eq!(json["gate_outcomes"]["stale-baseline"]["status"], "skipped");
    assert!(
        output.stderr.contains("(diff)") && output.stderr.contains("in the same mode"),
        "{}",
        output.stderr
    );
    let recheck = step(&json, "recheck-baseline").unwrap_or_else(|| panic!("no step in {json}"));
    assert!(
        recheck["command"]
            .as_str()
            .unwrap()
            .ends_with(" --production"),
        "{recheck}"
    );
}

/// The bare run baselines its dead-code sub-pass through the same comparison.
#[test]
fn a_bare_run_gates_a_production_baseline_in_the_same_mode() {
    let dir = project();
    save(&dir, "dead-code", &["--production"]);
    fix_one_entry(&dir);

    let path = baseline_path(&dir);
    let output = fallow(
        &dir,
        &[
            "--production",
            "--baseline",
            path.to_str().unwrap(),
            "--fail-on-stale-baseline",
            "--format",
            "json",
        ],
    );

    assert_eq!(output.code, 1, "{}", output.stderr);
    assert!(
        output
            .stderr
            .contains("Baseline gate failed: 1 of 2 entries"),
        "{}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("fallow baselines prune --production"),
        "{}",
        output.stderr
    );
}

/// Case J, gate half, in both directions.
#[test]
fn include_entry_exports_follows_the_same_rule_at_the_gate() {
    let dir = project();
    save(&dir, "dead-code", &["--include-entry-exports"]);

    let (same_output, same) = gate(&dir, &["--include-entry-exports"]);
    assert_eq!(same_output.code, 0, "{}", same_output.stderr);
    assert_eq!(same["baseline_staleness"]["change_scoped"], false, "{same}");
    assert_eq!(same["gate_outcomes"]["stale-baseline"]["status"], "pass");

    let (plain_output, plain) = gate(&dir, &[]);
    assert_eq!(plain_output.code, 0, "{}", plain_output.stderr);
    assert_eq!(scope_reasons(&plain), ["include-entry-exports"], "{plain}");
    assert!(
        plain_output
            .stderr
            .contains("was saved with --include-entry-exports and this run does not use it"),
        "{}",
        plain_output.stderr
    );

    let dir = project();
    save(&dir, "dead-code", &[]);
    let (output, json) = gate(&dir, &["--include-entry-exports"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(scope_reasons(&json), ["include-entry-exports"], "{json}");
    assert!(
        output
            .stderr
            .contains("does not record --include-entry-exports and this run uses it"),
        "{}",
        output.stderr
    );
}

/// Cases F and H: the plain prune removed every valid entry with exit 0.
#[test]
fn prune_refuses_a_production_baseline_without_production() {
    let dir = project();
    let path = save(&dir, "dead-code", &["--production"]);
    let before = std::fs::read_to_string(&path).unwrap();

    let (output, json) = prune_dead_code(&dir, &[]);

    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_refused(&path, &before, &json);
    assert_eq!(
        action(&json, "prune-baselines")["command"],
        "fallow baselines prune --production --dead-code-baseline baseline.json"
    );
}

#[test]
fn the_prune_refusal_names_the_mode_in_the_human_report() {
    let dir = project();
    save(&dir, "dead-code", &["--production"]);

    let output = fallow(
        &dir,
        &[
            "baselines",
            "prune",
            "--dead-code-baseline",
            "baseline.json",
        ],
    );

    assert_eq!(output.code, 2, "{}", output.stderr);
    assert!(
        output.stderr.contains(
            "Skipped the dead-code baseline baseline.json: the file was saved with --production \
             and this run does not use it, so a prune would remove valid entries. Run: fallow \
             baselines prune --production --dead-code-baseline baseline.json"
        ),
        "{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("Save it again with"),
        "a plain save would write another scope: {}",
        output.stderr
    );
}

/// `--check` refuses as well, so the exit code does not depend on whether a
/// removal exists.
#[test]
fn prune_refuses_a_mismatch_when_the_removal_set_is_empty() {
    let dir = project();
    save(&dir, "dead-code", &[]);

    let (output, json) = prune_dead_code(&dir, &["--production", "--check"]);

    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_eq!(json["files"][0]["reason_code"], "scope-mismatch", "{json}");
}

/// A file with no entries has nothing to lose, so it never refuses.
#[test]
fn prune_does_not_refuse_a_file_with_no_entries() {
    let dir = project();
    std::fs::remove_file(dir.path().join("src/lib.test.js")).unwrap();
    write(
        dir.path(),
        "src/lib.js",
        "export function run() {\n  return 1;\n}\n",
    );
    save(&dir, "dead-code", &["--production"]);

    let (output, json) = prune_dead_code(&dir, &[]);

    assert_eq!(output.code, 0, "{}", output.stdout);
    assert_eq!(json["files"][0]["status"], "unchanged", "{json}");
}

/// The prune flag is the printed remedy, and it removes only the fixed entry.
#[test]
fn prune_with_production_removes_only_the_fixed_entry() {
    let dir = project();
    let path = save(&dir, "dead-code", &["--production", "--unused-exports"]);
    fix_one_entry(&dir);

    let (check, check_json) = prune_dead_code(&dir, &["--production", "--check"]);
    assert_eq!(check.code, 1, "{}", check.stdout);
    let removed = removed_keys(&check_json);
    assert_eq!(removed.len(), 1, "{check_json}");
    assert!(removed[0].contains(REMOVED_ENTRY), "{check_json}");
    assert_eq!(
        action(&check_json, "prune-baselines")["command"],
        "fallow baselines prune --production"
    );

    let (output, _) = prune_dead_code(&dir, &["--production"]);
    assert_eq!(output.code, 0, "{}", output.stdout);
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(
        saved.contains("onlyTestedA") && !saved.contains(REMOVED_ENTRY),
        "{saved}"
    );
    assert!(saved.contains("\"production\""), "{saved}");
}

/// Case I: a whole-project file under a config-production run.
#[test]
fn prune_refuses_a_file_without_a_production_record_under_config_production() {
    let dir = project();
    let path = save(&dir, "dead-code", &[]);
    let before = std::fs::read_to_string(&path).unwrap();
    write_production_config(&dir, "true");

    let (output, json) = prune_dead_code(&dir, &[]);

    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_refused(&path, &before, &json);
    let resave = action(&json, "resave-baseline");
    assert_eq!(
        resave["command"],
        "fallow dead-code --production --save-baseline baseline.json"
    );
    assert_eq!(
        resave["description"],
        "Save this baseline again in production mode, after a review of its findings"
    );
    let reason = json["files"][0]["reason"].as_str().unwrap();
    assert!(
        reason.contains("does not record production mode and this run uses it"),
        "{reason}"
    );
}

/// The flag route refuses the same file.
#[test]
fn prune_refuses_a_file_without_a_production_record_under_the_flag() {
    let dir = project();
    let path = save(&dir, "dead-code", &[]);
    let before = std::fs::read_to_string(&path).unwrap();

    let (output, json) = prune_dead_code(&dir, &["--production"]);

    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_refused(&path, &before, &json);
    let human = fallow(
        &dir,
        &[
            "baselines",
            "prune",
            "--production",
            "--dead-code-baseline",
            "baseline.json",
        ],
    );
    assert!(
        human.stderr.contains(
            "Review the findings, then save it again with: fallow dead-code --production \
             --save-baseline baseline.json"
        ),
        "{}",
        human.stderr
    );
}

/// Case J, prune half: the flag was accepted and ignored, so the valid entry
/// went away.
#[test]
fn prune_honors_include_entry_exports() {
    let dir = project();
    let path = save(&dir, "dead-code", &["--include-entry-exports"]);
    let before = std::fs::read_to_string(&path).unwrap();
    assert!(before.contains(":run"), "{before}");

    let (plain, plain_json) = prune_dead_code(&dir, &[]);
    assert_eq!(plain.code, 2, "{}", plain.stdout);
    assert_refused(&path, &before, &plain_json);
    assert_eq!(
        action(&plain_json, "prune-baselines")["command"],
        "fallow baselines prune --include-entry-exports --dead-code-baseline baseline.json"
    );

    let (flag, flag_json) = prune_dead_code(&dir, &["--include-entry-exports"]);
    assert_eq!(flag.code, 0, "{}", flag.stdout);
    assert_eq!(flag_json["files"][0]["status"], "unchanged", "{flag_json}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

/// The other direction of case J.
#[test]
fn prune_refuses_include_entry_exports_on_a_file_without_the_record() {
    let dir = project();
    let path = save(&dir, "dead-code", &[]);
    let before = std::fs::read_to_string(&path).unwrap();

    let (output, json) = prune_dead_code(&dir, &["--include-entry-exports"]);

    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_refused(&path, &before, &json);
    assert_eq!(
        action(&json, "resave-baseline")["command"],
        "fallow dead-code --include-entry-exports --save-baseline baseline.json"
    );
}

/// Case K: the config route gives the right answer and stays as it was.
#[test]
fn prune_under_config_production_removes_only_the_fixed_entry() {
    for config in ["true", r#"{"deadCode":true}"#] {
        let dir = project();
        save(&dir, "dead-code", &["--production"]);
        fix_one_entry(&dir);
        write_production_config(&dir, config);

        let (output, json) = prune_dead_code(&dir, &["--check"]);

        assert_eq!(output.code, 1, "{config}: {}", output.stdout);
        let removed = removed_keys(&json);
        assert_eq!(removed.len(), 1, "{config}: {json}");
        assert!(removed[0].contains(REMOVED_ENTRY), "{config}: {json}");
    }
}

/// A function above the default cyclomatic threshold.
fn complex_source(name: &str) -> String {
    let mut branches = String::new();
    for value in 0..30 {
        writeln!(branches, "  if (input === {value}) return {value};").unwrap();
    }
    format!("export function {name}(input) {{\n{branches}  return -1;\n}}\n")
}

/// One complex function in shipped code and one in a test file, which a
/// production run does not see.
fn health_project() -> TempDir {
    let dir = project();
    write(
        dir.path(),
        "src/index.js",
        "export { run } from './lib.js';\nimport { shipped } from './complex.js';\n\
         console.log(shipped);\n",
    );
    write(dir.path(), "src/complex.js", &complex_source("shipped"));
    write(
        dir.path(),
        "src/complex.test.js",
        &complex_source("onlyInTests"),
    );
    dir
}

/// A production run does not see the test-file finding, so a prune without the
/// guard removes a valid entry of a whole-project health file.
#[test]
fn prune_refuses_a_health_baseline_without_a_production_record() {
    let dir = health_project();
    let path = save(&dir, "health", &[]);
    let before = std::fs::read_to_string(&path).unwrap();
    write_production_config(&dir, "true");

    let (output, json) = prune(&dir, "--health-baseline", &["--check"]);

    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_refused(&path, &before, &json);
    assert_eq!(
        action(&json, "resave-baseline")["command"],
        "fallow health --production --save-baseline baseline.json"
    );
}

/// The save records the mode, so "save it again" is a true remedy.
#[test]
fn a_health_baseline_saved_in_production_prunes_in_production_only() {
    let dir = health_project();
    let path = save(&dir, "health", &["--production"]);
    let before = std::fs::read_to_string(&path).unwrap();
    assert!(before.contains("\"production\""), "{before}");

    let (flag, flag_json) = prune(&dir, "--health-baseline", &["--production"]);
    assert_eq!(flag.code, 0, "{}", flag.stdout);
    assert_eq!(flag_json["files"][0]["status"], "unchanged", "{flag_json}");

    let (plain, plain_json) = prune(&dir, "--health-baseline", &[]);
    assert_eq!(plain.code, 2, "{}", plain.stdout);
    assert_refused(&path, &before, &plain_json);
    assert_eq!(
        action(&plain_json, "prune-baselines")["command"],
        "fallow baselines prune --production --health-baseline baseline.json"
    );
}

/// The health gate stays inert in production mode and says what is true.
#[test]
fn the_health_gate_stays_inert_in_production_mode() {
    let dir = health_project();
    save(&dir, "health", &["--production"]);

    let (output, _) = gate_command(&dir, "health", &["--production"]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output
            .stderr
            .contains("fallow does not gate a health baseline in production mode")
            && output
                .stderr
                .contains("Run without --production to gate on it."),
        "{}",
        output.stderr
    );
}

/// A function long enough to form a clone group with a copy of itself.
fn clone_source(name: &str) -> String {
    format!(
        "export function {name}(items, factor) {{
  const out = [];
  for (const item of items) {{
    if (item > 7) {{
      out.push(item * factor + 7);
    }} else if (item < -7) {{
      out.push(item - factor * 7);
    }} else {{
      out.push(item);
    }}
  }}
  const total = out.reduce((a, b) => a + b, 0);
  if (total > 700) {{
    console.log(\"big total\", total);
  }}
  const sorted = [...out].sort((a, b) => a - b);
  const mid = sorted[Math.floor(sorted.length / 2)];
  console.log(\"median\", mid);
  return out.map((v) => v + mid - 7);
}}
"
    )
}

/// One clone pair: one copy ships and one is a story, which a production run
/// does not see.
fn dupes_project() -> TempDir {
    let dir = project();
    write(
        dir.path(),
        "src/index.js",
        "export { run } from './lib.js';\nimport { a } from './a.js';\nconsole.log(a);\n",
    );
    write(dir.path(), "src/a.js", &clone_source("a"));
    write(dir.path(), "src/a.stories.js", &clone_source("b"));
    dir
}

#[test]
fn prune_refuses_a_dupes_baseline_without_a_production_record() {
    let dir = dupes_project();
    let path = save(&dir, "dupes", &[]);
    let before = std::fs::read_to_string(&path).unwrap();
    assert!(
        before.contains("a.stories.js"),
        "the fixture needs a clone group: {before}"
    );

    let (output, json) = prune(&dir, "--dupes-baseline", &["--production"]);

    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_refused(&path, &before, &json);
    assert_eq!(
        action(&json, "resave-baseline")["command"],
        "fallow dupes --production --save-baseline baseline.json"
    );
}

#[test]
fn the_dupes_gate_stays_inert_in_production_mode() {
    let dir = dupes_project();
    save(&dir, "dupes", &[]);

    let (output, _) = gate_command(&dir, "dupes", &["--production"]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output
            .stderr
            .contains("fallow does not gate a dupes baseline in production mode"),
        "{}",
        output.stderr
    );
}
