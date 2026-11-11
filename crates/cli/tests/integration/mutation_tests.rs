//! `fallow mutation`: plan, run, verdicts, inspect and the exit policy.
//!
//! Most tests use a project with no test files. Its run needs no JavaScript
//! runtime: every mutant is `no_coverage`, so the score is 0 and the exit
//! policy, the verdict join and inspect are observable without Node. The
//! Vitest end-to-end test skips with a message when `node` or the companion
//! `node_modules` is missing.

use std::fs;
use std::path::Path;

use super::common::{CommandOutput, commit_all, git, parse_json, redact_all, run_fallow_in_root};

const MATH: &str = "export function add(a: number, b: number): number {\n  return a + b;\n}\n\nexport function isPositive(n: number): boolean {\n  return n > 0;\n}\n";

/// A git project with one source file and no test file.
fn untested_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"mutation-fixture","private":true,"type":"module","main":"src/math.ts"}"#,
    )
    .expect("package.json");
    fs::create_dir_all(root.join("src")).expect("src");
    fs::write(root.join("src/math.ts"), MATH).expect("math.ts");
    fs::write(root.join(".gitignore"), "node_modules\n.fallow\n").expect(".gitignore");
    git(root, &["init", "-q"]);
    commit_all(root, "init");
    dir
}

fn mutation(root: &Path, args: &[&str]) -> CommandOutput {
    run_fallow_in_root("mutation", root, args)
}

fn json_mutation(root: &Path, args: &[&str]) -> (CommandOutput, serde_json::Value) {
    let mut all = vec!["--format", "json", "--quiet"];
    all.extend_from_slice(args);
    let output = mutation(root, &all);
    let value = parse_json(&output);
    (output, value)
}

#[test]
fn plan_json_lists_unreached_mutants_and_runs_no_code() {
    let project = untested_project();
    let (output, mut value) = json_mutation(project.path(), &["--plan", "--all"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(value["kind"], "mutation-plan");
    assert_eq!(value["no_test_files"], true);
    assert!(
        value["counts"]["no_coverage"].as_u64().unwrap() > 0,
        "{value}"
    );
    value["elapsed_ms"] = serde_json::json!(0);
    value["version"] = serde_json::Value::String("[version]".to_owned());
    insta::assert_snapshot!(serde_json::to_string_pretty(&value).unwrap());
}

#[test]
fn mutants_per_line_leaves_the_other_mutants_of_a_line_pending_in_the_plan() {
    let project = untested_project();
    let root = project.path();
    fs::write(
        root.join("src/math.ts"),
        "export const inRange = (n: number): boolean => n > 0 && n + 1 < 10;\n",
    )
    .expect("math.ts");
    fs::write(
        root.join("package.json"),
        r#"{"name":"mutation-fixture","private":true,"type":"module","main":"src/math.ts","devDependencies":{"vitest":"^5.0.0"}}"#,
    )
    .expect("package.json");
    fs::write(
        root.join("src/math.test.ts"),
        "import { expect, test } from \"vitest\";\nimport { inRange } from \"./math\";\n\ntest(\"in range\", () => {\n  expect(inRange(3)).toBe(true);\n});\n",
    )
    .expect("math.test.ts");
    commit_all(root, "add a test");

    let (full_output, full) = json_mutation(root, &["--plan", "--all"]);
    assert_eq!(full_output.code, 0, "{}", full_output.stderr);
    let planned = full["counts"]["planned"].as_u64().unwrap();
    assert!(planned > 1, "the line has several mutants: {full}");

    let (output, sampled) = json_mutation(root, &["--plan", "--all", "--mutants-per-line", "1"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let counts = &sampled["counts"];
    assert_eq!(counts["planned"], 1, "{sampled}");
    assert_eq!(counts["sampled_out"].as_u64().unwrap(), planned - 1);
    assert_eq!(counts["pending"], counts["sampled_out"]);
    let pending: Vec<&serde_json::Value> = sampled["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|mutant| mutant["disposition"] == "pending")
        .collect();
    assert_eq!(pending.len() as u64, planned - 1);
    assert!(
        pending
            .iter()
            .all(|mutant| mutant["pending_reason"] == "sampled-out"),
        "{sampled}"
    );

    let zero = mutation(root, &["--plan", "--all", "--mutants-per-line", "0"]);
    assert_eq!(zero.code, 2, "{}", zero.stderr);
}

#[test]
fn a_test_only_change_warns_about_its_skipped_tests_and_starts_no_runner() {
    let project = untested_project();
    let root = project.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"mutation-fixture","private":true,"type":"module","main":"src/math.ts","devDependencies":{"vitest":"^5.0.0"}}"#,
    )
    .expect("package.json");
    fs::write(
        root.join("src/math.test.ts"),
        "import { expect, it } from \"vitest\";\nimport { add } from \"./math\";\n\nit(\"adds\", () => expect(add(1, 2)).toBe(3));\nit.skip(\"adds negatives\", () => {});\n",
    )
    .expect("math.test.ts");
    commit_all(root, "add a test");
    let diff_dir = tempfile::tempdir().expect("temp dir");
    let diff = diff_dir.path().join("change.diff");
    fs::write(
        &diff,
        "diff --git a/src/math.test.ts b/src/math.test.ts\n--- a/src/math.test.ts\n+++ b/src/math.test.ts\n@@ -4,0 +5,1 @@\n+it.skip(\"adds negatives\", () => {});\n",
    )
    .expect("diff");

    let (output, report) = json_mutation(root, &["--diff-file", diff.to_str().expect("utf-8")]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let errors = report["errors"].as_array().expect("errors");
    let warning = errors
        .iter()
        .find(|error| error["code"] == "skipped-tests")
        .unwrap_or_else(|| panic!("a skipped-tests warning: {report}"));
    assert_eq!(
        warning["message"],
        "1 test is skipped or todo: src/math.test.ts:5 (adds negatives). A skipped test checks nothing, so it cannot kill a mutant. Run each test or remove it."
    );
    assert!(
        report["runner"].is_null(),
        "the scan starts no runner: {report}"
    );
    assert_eq!(report["completion"]["status"], "complete");

    let (_, full) = json_mutation(root, &["--all"]);
    assert!(
        full["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|error| error["code"] != "skipped-tests"),
        "a full run gives no per-test warning"
    );
}

#[test]
fn plan_mutates_the_script_blocks_of_vue_svelte_and_astro_components() {
    let project = untested_project();
    let root = project.path();
    fs::write(
        root.join("src/math.ts"),
        "import Counter from './Counter.vue';\nimport Toggle from './Toggle.svelte';\nimport Page from './Page.astro';\nimport { total } from './total.svelte.ts';\nexport const parts = [Counter, Toggle, Page, total];\n",
    )
    .expect("math.ts");
    fs::write(
        root.join("src/Counter.vue"),
        "<template>\n  <p>{{ count > 1 ? 'many' : 'one' }}</p>\n</template>\n<script setup lang=\"ts\">\nconst props = defineProps({ start: { type: Number, default: 1 + 1 } })\nconst count = props.start * 2\n</script>\n",
    )
    .expect("Counter.vue");
    fs::write(
        root.join("src/Toggle.svelte"),
        "<script>\n  let on = $state(false);\n  function flip() {\n    on = !on;\n  }\n</script>\n<button onclick={flip}>{on ? 'on' : 'off'}</button>\n",
    )
    .expect("Toggle.svelte");
    fs::write(
        root.join("src/Page.astro"),
        "---\nexport const prerender = true;\nconst title = Astro.props.title ?? 'Home';\n---\n<h1>{title}</h1>\n",
    )
    .expect("Page.astro");
    fs::write(
        root.join("src/total.svelte.ts"),
        "export const total = $state.raw(1 + 1) || 0;\n",
    )
    .expect("total.svelte.ts");
    commit_all(root, "components");

    let (output, value) = json_mutation(root, &["--plan", "--all"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let mut mutants: Vec<String> = value["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["file"] != "src/math.ts")
        .map(|mutant| {
            format!(
                "{}:{}:{} {}",
                mutant["file"].as_str().unwrap_or_default(),
                mutant["line"],
                mutant["column"],
                mutant["operator"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    mutants.sort();
    assert_eq!(
        mutants,
        vec![
            "src/Counter.vue:6:27 arithmetic-operator",
            "src/Page.astro:3:33 logical-operator",
            "src/Page.astro:3:36 string-literal",
            "src/Toggle.svelte:2:19 boolean-literal",
            "src/Toggle.svelte:4:10 unary-operator",
            "src/total.svelte.ts:1:35 arithmetic-operator",
        ],
        "{value}"
    );
}

/// The test file has no `node:test` import and no default test file name,
/// so only the `borp` script makes it a test file and selects `node-test`.
/// The `node-test` runner cannot load a component file, so the plan skips
/// the Svelte file.
#[test]
fn plan_reads_the_test_files_and_the_runner_of_a_borp_script() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"borp-fixture","private":true,"main":"index.js","scripts":{"test":"borp -p 'spec/**/*.js'"}}"#,
    )
    .expect("package.json");
    fs::write(
        root.join("index.js"),
        "'use strict'\nconst Toggle = require('./Toggle.svelte')\nmodule.exports = { Toggle, add: (a, b) => a + b }\n",
    )
    .expect("index.js");
    fs::write(
        root.join("Toggle.svelte"),
        "<script>\n  let on = $state(false);\n  function flip() {\n    on = !on;\n  }\n</script>\n<button onclick={flip}>{on ? 'on' : 'off'}</button>\n",
    )
    .expect("Toggle.svelte");
    fs::create_dir_all(root.join("spec")).expect("spec");
    fs::write(
        root.join("spec/add.js"),
        "'use strict'\nconst assert = require('node:assert')\nconst { add } = require('../index.js')\nassert.strictEqual(add(1, 2), 3)\n",
    )
    .expect("add.js");
    git(root, &["init", "-q"]);
    commit_all(root, "init");

    let (output, value) = json_mutation(root, &["--plan", "--all"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(value["no_test_files"], false, "{value}");
    assert_eq!(
        value["test_files"],
        serde_json::json!(["spec/add.js"]),
        "{value}"
    );
    assert_eq!(value["counts"]["no_coverage"], 0, "{value}");
    assert!(value["counts"]["planned"].as_u64().unwrap() > 0, "{value}");
    assert_eq!(
        value["skipped_files"],
        serde_json::json!([{"path": "Toggle.svelte", "reason": "runner-cannot-load"}]),
        "{value}"
    );
}

#[test]
fn plan_human_output_names_scope_counts_and_no_tests_in_one_line() {
    let project = untested_project();
    let output = mutation(project.path(), &["--plan", "--all", "--quiet"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    insta::assert_snapshot!(redact_all(&output.stdout, project.path()));
}

#[test]
fn a_dirty_worktree_exits_2_unless_allowed() {
    let project = untested_project();
    fs::write(
        project.path().join("src/math.ts"),
        format!("{MATH}// edit\n"),
    )
    .expect("edit");

    let (output, value) = json_mutation(project.path(), &["--all"]);
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_eq!(value["code"], "FALLOW_MUTATION_DIRTY_WORKTREE");
    assert!(
        value["message"].as_str().unwrap().contains("src/math.ts"),
        "{value}"
    );

    let (allowed, value) = json_mutation(project.path(), &["--all", "--allow-dirty"]);
    assert_eq!(allowed.code, 0, "{}", allowed.stderr);
    assert_eq!(value["kind"], "mutation");
    assert_eq!(value["worktree"]["dirty_at_start"][0], "src/math.ts");
}

/// The prefix of each phase progress line on stderr.
const PROGRESS: &str = "fallow: mutation:";

fn progress_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with(PROGRESS))
        .collect()
}

#[test]
fn human_output_shows_phase_progress_and_quiet_or_json_output_shows_none() {
    let project = untested_project();
    let human = mutation(project.path(), &["--all"]);
    assert_eq!(human.code, 0, "{}", human.stderr);
    let lines = progress_lines(&human.stderr);
    assert_eq!(lines.len(), 1, "{}", human.stderr);
    assert!(
        lines[0].starts_with("fallow: mutation: the tests stopped; worktree check and report ("),
        "no test file exists, so only the final phase runs: {}",
        human.stderr
    );
    assert!(!human.stdout.contains(PROGRESS), "{}", human.stdout);

    let quiet = mutation(project.path(), &["--all", "--quiet"]);
    assert_eq!(quiet.code, 0, "{}", quiet.stderr);
    assert!(progress_lines(&quiet.stderr).is_empty(), "{}", quiet.stderr);

    let json = mutation(project.path(), &["--all", "--format", "json"]);
    assert_eq!(json.code, 0, "{}", json.stderr);
    let value = parse_json(&json);
    assert_eq!(value["no_test_files"], true, "{value}");
    assert!(progress_lines(&json.stderr).is_empty(), "{}", json.stderr);
}

#[test]
fn unreached_mutants_give_a_coverage_but_no_score_so_min_score_cannot_fail() {
    let project = untested_project();
    let (plain, value) = json_mutation(project.path(), &["--all"]);
    assert_eq!(plain.code, 0, "{}", plain.stderr);
    assert_eq!(
        value["summary"]["score"],
        serde_json::Value::Null,
        "no mutant ran, so the score over executed mutants has no value: {value}"
    );
    assert_eq!(value["summary"]["coverage"], 0.0);
    assert!(value["summary"]["counts"]["no_coverage"].as_u64().unwrap() > 0);
    assert_eq!(value["no_test_files"], true);
    assert!(
        value.get("runner").is_none(),
        "no companion started, so the runner is absent: {value}"
    );

    let (unmeasured, _) = json_mutation(project.path(), &["--all", "--min-score", "50"]);
    assert_eq!(unmeasured.code, 0, "{}", unmeasured.stderr);

    let human = mutation(project.path(), &["--all", "--min-score", "50", "--quiet"]);
    assert!(
        human.stdout.contains("Minimum score 50%: not measured"),
        "{}",
        human.stdout
    );

    let invalid = mutation(project.path(), &["--all", "--min-score", "120", "--quiet"]);
    assert_eq!(invalid.code, 2, "{}", invalid.stderr);
}

#[test]
fn an_output_file_in_the_worktree_is_written_after_the_run_and_never_makes_it_dirty() {
    let project = untested_project();
    let report = project.path().join("reports/mutation.json");
    let report_arg = report.to_str().unwrap();
    for attempt in ["first", "second"] {
        let output = mutation(
            project.path(),
            &["--all", "--format", "json", "--quiet", "-o", report_arg],
        );
        assert_eq!(output.code, 0, "{attempt} run: {}", output.stdout);
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&report).expect("report")).expect("json");
        assert_eq!(saved["kind"], "mutation", "{attempt} run");
        assert!(
            saved["worktree"]["dirty_at_start"]
                .as_array()
                .is_none_or(Vec::is_empty),
            "{attempt} run: {saved}"
        );
    }
}

#[test]
fn inspect_reads_the_report_of_the_last_run_when_no_report_is_named() {
    let project = untested_project();
    let (none, value) = json_mutation(project.path(), &["inspect", "0000000000000000"]);
    assert_eq!(none.code, 2, "{value}");
    assert_eq!(value["code"], "FALLOW_MUTATION_NO_REPORT", "{value}");
    let help = value["help"].as_str().unwrap_or_default();
    assert!(help.contains("--report <file>"), "{value}");
    assert!(
        !help.contains("run `fallow mutation`"),
        "the help names no step that runs the tests: {value}"
    );

    let (_, report) = json_mutation(project.path(), &["--all"]);
    let id = report["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mutant| mutant["operator"] == "arithmetic-operator" && mutant["function"] == "add")
        .and_then(|mutant| mutant["mutant_id"].as_str())
        .expect("an arithmetic mutant in add")
        .to_owned();
    let (fresh, packet) = json_mutation(project.path(), &["inspect", &id]);
    assert_eq!(fresh.code, 0, "{}", fresh.stderr);
    assert_eq!(packet["status"], "fresh", "{packet}");
    assert_eq!(packet["code"]["mutated_lines"], "  return a - b;");
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(project.path())
        .output()
        .expect("git status");
    assert_eq!(
        String::from_utf8_lossy(&status.stdout),
        "",
        "the saved report stays out of the worktree"
    );
}

#[test]
fn inspect_of_an_id_from_an_earlier_run_says_which_report_it_read() {
    let project = untested_project();
    let (_, all) = json_mutation(project.path(), &["--all"]);
    let id = all["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mutant| mutant["function"] == "isPositive")
        .and_then(|mutant| mutant["mutant_id"].as_str())
        .expect("a mutant in isPositive")
        .to_owned();
    fs::write(
        project.path().join("src/other.ts"),
        "export function twice(n: number): number {\n  return n * 2;\n}\n",
    )
    .expect("other.ts");
    commit_all(project.path(), "other");
    let (narrow, _) = json_mutation(project.path(), &["src/other.ts"]);
    assert_eq!(narrow.code, 0, "{}", narrow.stderr);

    let (unknown, value) = json_mutation(project.path(), &["inspect", &id]);
    assert_eq!(unknown.code, 2, "{value}");
    assert_eq!(value["code"], "FALLOW_MUTATION_UNKNOWN_MUTANT", "{value}");
    let help = value["help"].as_str().unwrap_or_default();
    assert!(help.contains("last run"), "{value}");
    assert!(help.contains("--report <file>"), "{value}");
}

/// A relative `-o` path is relative to the process directory, not to
/// `--root`. The dirty-tree check must leave out the same file that the run
/// writes.
#[test]
fn a_relative_output_file_from_a_subdirectory_never_makes_the_tree_dirty() {
    let project = untested_project();
    let cwd = project.path().join("src");
    for attempt in ["first", "second"] {
        let output = std::process::Command::new(super::common::fallow_bin())
            .env("NO_COLOR", "1")
            .env("RUST_LOG", "")
            .current_dir(&cwd)
            .args([
                "mutation", "--root", "..", "--all", "--format", "json", "--quiet", "-o",
                "sub.json",
            ])
            .output()
            .expect("fallow");
        assert_eq!(
            output.status.code(),
            Some(0),
            "{attempt} run: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(cwd.join("sub.json")).expect("report"))
                .expect("json");
        assert_eq!(saved["kind"], "mutation", "{attempt} run");
    }
}

/// `inspect` without `--report` from a subdirectory finds the report that a
/// run at the project root saved, and reads the files of that root.
#[test]
fn inspect_from_a_subdirectory_reads_the_saved_report_of_the_project_root() {
    let project = untested_project();
    let (_, report) = json_mutation(project.path(), &["--all"]);
    let id = report["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mutant| mutant["operator"] == "arithmetic-operator" && mutant["function"] == "add")
        .and_then(|mutant| mutant["mutant_id"].as_str())
        .expect("an arithmetic mutant in add")
        .to_owned();
    let output = std::process::Command::new(super::common::fallow_bin())
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .current_dir(project.path().join("src"))
        .args(["mutation", "inspect", &id, "--format", "json", "--quiet"])
        .output()
        .expect("fallow");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    let packet: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(packet["status"], "fresh", "{packet}");
    assert_eq!(packet["code"]["mutated_lines"], "  return a - b;");
}

/// The `mutation --help` text uses no `(s)` plural form.
#[test]
fn mutation_help_has_no_parenthesized_plural() {
    let output = super::common::run_fallow_raw(&["mutation", "--help"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stdout.contains("inspect"), "{}", output.stdout);
    assert!(!output.stdout.contains("(s)"), "{}", output.stdout);
}

#[test]
fn equivalent_verdicts_leave_the_score_and_report_unmatched_keys() {
    let project = untested_project();
    let (_, before) = json_mutation(project.path(), &["--all"]);
    assert!(
        before.get("verdicts").is_none(),
        "a run without --verdicts has no verdict join: {before}"
    );
    let ids: Vec<String> = before["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|mutant| mutant["status"] == "no_coverage")
        .map(|mutant| mutant["mutant_id"].as_str().unwrap().to_owned())
        .collect();
    assert!(ids.len() >= 2, "{before}");

    let mut verdicts: Vec<serde_json::Value> = ids
        .iter()
        .map(|id| serde_json::json!({ "review_key": id, "verdict": "equivalent" }))
        .collect();
    verdicts.push(serde_json::json!({ "review_key": "ffffffffffffffff", "verdict": "equivalent" }));
    let verdict_path = project.path().join("verdicts.json");
    fs::write(
        &verdict_path,
        serde_json::to_string(&serde_json::json!({ "schema_version": "1", "verdicts": verdicts }))
            .unwrap(),
    )
    .expect("verdicts");
    commit_all(project.path(), "verdicts");
    let verdict_arg = verdict_path.to_str().unwrap();

    let (output, after) = json_mutation(
        project.path(),
        &["--all", "--min-score", "50", "--verdicts", verdict_arg],
    );
    assert_eq!(
        output.code, 0,
        "no counted mutant is left: {}",
        output.stderr
    );
    assert_eq!(after["summary"]["score"], serde_json::Value::Null);
    assert_eq!(after["summary"]["counts"]["no_coverage"], 0);
    assert_eq!(
        after["summary"]["counts"]["equivalent"].as_u64(),
        Some(ids.len() as u64)
    );
    assert!(
        after["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|mutant| ids.contains(&mutant["mutant_id"].as_str().unwrap().to_owned()))
            .all(|mutant| mutant["status"] == "equivalent")
    );
    assert_eq!(
        after["verdicts"]["unmatched"],
        serde_json::json!(["ffffffffffffffff"])
    );
    assert_eq!(
        after["verdicts"]["applied"].as_array().unwrap().len(),
        ids.len()
    );

    fs::write(&verdict_path, "{\"schema_version\":\"1\"}").expect("broken verdicts");
    let (broken, value) = json_mutation(
        project.path(),
        &["--all", "--allow-dirty", "--verdicts", verdict_arg],
    );
    assert_eq!(broken.code, 2);
    assert_eq!(value["code"], "FALLOW_MUTATION_VERDICTS_INVALID");

    let (plan, value) = json_mutation(
        project.path(),
        &["--plan", "--all", "--verdicts", verdict_arg],
    );
    assert_eq!(plan.code, 2, "{value}");
}

#[test]
fn an_invalid_verdict_document_exits_2_before_the_run_starts() {
    let project = untested_project();
    let verdict_path = project.path().join("verdicts.json");
    fs::write(
        &verdict_path,
        r#"{"schema_version":"1","verdicts":[{"review_key":"abc","verdict":"equivalent"},{"review_key":"abc","verdict":"equivalent"}]}"#,
    )
    .expect("verdicts");
    // The verdict file leaves the worktree dirty. A run that started would
    // fail on the dirty tree first, so the verdict error proves that the
    // document was checked before the run.
    let output = mutation(
        project.path(),
        &[
            "--all",
            "--format",
            "json",
            "--verdicts",
            verdict_path.to_str().unwrap(),
        ],
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
    let value = parse_json(&output);
    assert_eq!(value["code"], "FALLOW_MUTATION_VERDICTS_INVALID", "{value}");
    assert!(
        value["message"].as_str().unwrap().contains("`abc`"),
        "{value}"
    );
    assert!(
        !output.stderr.contains("runs the project tests"),
        "{}",
        output.stderr
    );
}

#[test]
fn inspect_shows_the_mutated_line_when_fresh_and_a_stale_status_after_an_edit() {
    let project = untested_project();
    let (_, report) = json_mutation(project.path(), &["--all"]);
    let mutant = report["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mutant| mutant["operator"] == "arithmetic-operator" && mutant["function"] == "add")
        .expect("an arithmetic mutant in add")
        .clone();
    let id = mutant["mutant_id"].as_str().unwrap();
    let report_path = project.path().join("mutation.json");
    fs::write(&report_path, serde_json::to_string(&report).unwrap()).expect("report");

    let (fresh, packet) = json_mutation(
        project.path(),
        &["inspect", id, "--report", report_path.to_str().unwrap()],
    );
    assert_eq!(fresh.code, 0, "{}", fresh.stderr);
    assert_eq!(packet["kind"], "mutation-inspect");
    assert_eq!(packet["status"], "fresh");
    assert_eq!(packet["report_status"], "no_coverage");
    assert_eq!(packet["code"]["original_lines"], "  return a + b;");
    assert_eq!(packet["code"]["mutated_lines"], "  return a - b;");

    let human = mutation(
        project.path(),
        &["inspect", id, "--report", report_path.to_str().unwrap()],
    );
    assert!(
        human.stdout.contains("-    2 |   return a + b;"),
        "{}",
        human.stdout
    );
    assert!(
        human.stdout.contains("+    2 |   return a - b;"),
        "{}",
        human.stdout
    );

    fs::write(
        project.path().join("src/math.ts"),
        MATH.replace("a + b", "b + a"),
    )
    .expect("edit");
    let (stale, packet) = json_mutation(
        project.path(),
        &["inspect", id, "--report", report_path.to_str().unwrap()],
    );
    assert_eq!(stale.code, 0, "{}", stale.stderr);
    assert_eq!(packet["status"], "stale");
    assert_eq!(packet["stale_reason"], "source-changed");
    assert_eq!(packet["code"], serde_json::Value::Null);

    let (unknown, value) = json_mutation(
        project.path(),
        &[
            "inspect",
            "0000000000000000",
            "--report",
            report_path.to_str().unwrap(),
        ],
    );
    assert_eq!(unknown.code, 2);
    assert_eq!(value["code"], "FALLOW_MUTATION_UNKNOWN_MUTANT");
}

#[test]
fn tasks_list_the_open_mutants_of_the_last_run_in_json_text_and_markdown() {
    let project = untested_project();
    let (_, report) = json_mutation(project.path(), &["--all"]);
    let open = report["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|mutant| mutant["status"] == "no_coverage" || mutant["status"] == "survived")
        .count();
    assert!(open > 0, "the untested project has open mutants");

    let (output, batch) = json_mutation(project.path(), &["tasks"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(batch["kind"], "mutation-tasks");
    assert_eq!(batch["schema_version"], "1");
    assert_eq!(batch["summary"]["tasks"], open);
    let add = batch["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["operator"] == "arithmetic-operator" && task["function"] == "add")
        .expect("an arithmetic task in add");
    assert_eq!(add["status"], "no_coverage");
    assert_eq!(add["original_lines"], "  return a + b;");
    assert_eq!(add["mutated_lines"], "  return a - b;");

    let text = mutation(project.path(), &["tasks"]);
    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(
        text.stdout.starts_with("Mutation tasks: "),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("   changed: return a - b;"),
        "{}",
        text.stdout
    );

    let markdown = mutation(project.path(), &["tasks", "--format", "markdown"]);
    assert_eq!(markdown.code, 0, "{}", markdown.stderr);
    assert!(
        markdown.stdout.starts_with("## Mutation tasks"),
        "{}",
        markdown.stdout
    );

    let plan = project.path().join("plan.json");
    let (_, plan_value) = json_mutation(project.path(), &["--all", "--plan"]);
    fs::write(&plan, serde_json::to_string(&plan_value).unwrap()).expect("plan");
    let (refused, error) = json_mutation(
        project.path(),
        &["tasks", "--report", plan.to_str().unwrap()],
    );
    assert_eq!(refused.code, 2);
    assert_eq!(error["code"], "FALLOW_MUTATION_REPORT_INVALID");
}

#[test]
fn export_writes_the_last_run_in_the_report_schema_format() {
    let project = untested_project();
    let (_, report) = json_mutation(project.path(), &["--all"]);
    let mutants = report["mutants"].as_array().unwrap().len();

    let (output, document) = json_mutation(project.path(), &["export"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(document["schemaVersion"], "2");
    let exported: usize = document["files"]
        .as_object()
        .unwrap()
        .values()
        .map(|file| file["mutants"].as_array().unwrap().len())
        .sum();
    assert_eq!(exported, mutants);
    let add = document["files"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|file| file["mutants"].as_array().unwrap())
        .find(|mutant| mutant["mutatorName"] == "arithmetic-operator")
        .expect("an arithmetic mutant");
    assert_eq!(add["status"], "NoCoverage");
    assert_eq!(add["replacement"], "-");

    let human = mutation(project.path(), &["export"]);
    assert_eq!(human.code, 0, "{}", human.stderr);
    let human_document: serde_json::Value = serde_json::from_str(&human.stdout).unwrap();
    assert_eq!(human_document, document);

    let plan = project.path().join("plan.json");
    let (_, plan_value) = json_mutation(project.path(), &["--all", "--plan"]);
    fs::write(&plan, serde_json::to_string(&plan_value).unwrap()).expect("plan");
    let (refused, error) = json_mutation(
        project.path(),
        &["export", "--report", plan.to_str().unwrap()],
    );
    assert_eq!(refused.code, 2);
    assert_eq!(error["code"], "FALLOW_MUTATION_REPORT_INVALID");
}

#[test]
fn recheck_reruns_the_open_tasks_and_refuses_a_changed_source() {
    let project = untested_project();
    let (_, report) = json_mutation(project.path(), &["--all"]);
    let open: Vec<String> = report["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|mutant| mutant["status"] == "no_coverage")
        .map(|mutant| mutant["mutant_id"].as_str().unwrap().to_owned())
        .collect();
    assert!(open.len() > 1, "the untested project has open mutants");

    let (output, recheck) = json_mutation(project.path(), &["--recheck"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(recheck["kind"], "mutation");
    assert_eq!(recheck["recheck"]["no_coverage"], open.len());
    assert_eq!(
        recheck["recheck"]["mutants"].as_array().unwrap().len(),
        open.len()
    );
    assert_eq!(
        recheck["recheck"]["mutants"][0]["previous_status"],
        "no_coverage"
    );
    assert_eq!(recheck["recheck"]["mutants"][0]["outcome"], "no_coverage");

    let one = open[0].as_str();
    let (output, single) = json_mutation(project.path(), &["--recheck", "--mutant", one]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(single["recheck"]["mutants"].as_array().unwrap().len(), 1);
    assert_eq!(single["recheck"]["mutants"][0]["mutant_id"], one);
    let not_selected = single["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|mutant| mutant["prune_reason"] == "not-selected")
        .count();
    assert_eq!(
        not_selected, 0,
        "each other mutant keeps its result from the saved report"
    );
    assert_eq!(single["recheck"]["carried"], open.len() - 1);
    assert_eq!(single["summary"]["counts"]["no_coverage"], open.len());
    let (_, tasks) = json_mutation(project.path(), &["tasks"]);
    assert_eq!(
        tasks["summary"]["tasks"],
        open.len(),
        "a recheck of named mutants keeps the other open tasks in the saved report"
    );

    let gated = mutation(project.path(), &["--recheck", "--min-score", "50"]);
    assert_eq!(gated.code, 2, "{}", gated.stderr);

    fs::write(
        project.path().join("src/other.ts"),
        "export const other = 1;\n",
    )
    .expect("other");
    let (dirty, error) = json_mutation(project.path(), &["--recheck"]);
    assert_eq!(dirty.code, 2);
    assert_eq!(error["code"], "FALLOW_MUTATION_NON_TEST_CHANGE", "{error}");
    fs::remove_file(project.path().join("src/other.ts")).expect("remove other");

    let (unknown, error) = json_mutation(
        project.path(),
        &["--recheck", "--mutant", "0000000000000000"],
    );
    assert_eq!(unknown.code, 2);
    assert_eq!(error["code"], "FALLOW_MUTATION_UNKNOWN_MUTANT");

    fs::write(
        project.path().join("src/math.ts"),
        MATH.replace("a + b", "b + a"),
    )
    .expect("edit");
    let (changed, error) = json_mutation(project.path(), &["--recheck"]);
    assert_eq!(changed.code, 2);
    assert_eq!(error["code"], "FALLOW_MUTATION_SOURCE_CHANGED", "{error}");
}

/// A source file whose mutants the recheck fixture marks as killed, so no
/// recheck selects them.
const EXTRA: &str = "export function double(n: number): number {\n  return n * 2;\n}\n";

/// The untested project with a second source file, and a saved report of
/// `--all` in a directory outside the project. The saved report marks the
/// first mutant of `src/math.ts` and each mutant of `src/extra.ts` as
/// killed, the result of an earlier run with tests. Returns the project, the
/// directory of the report, the report path and the ids of the killed
/// mutants, the one of `src/math.ts` first.
fn recheck_fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    std::path::PathBuf,
    Vec<String>,
) {
    let project = untested_project();
    fs::write(project.path().join("src/extra.ts"), EXTRA).expect("extra.ts");
    commit_all(project.path(), "extra");
    let (output, mut report) = json_mutation(project.path(), &["--all"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let mut killed = Vec::new();
    let mut math_killed = false;
    for mutant in report["mutants"].as_array_mut().unwrap() {
        let in_math = mutant["file"] == "src/math.ts";
        if in_math && math_killed {
            continue;
        }
        math_killed |= in_math;
        mutant["status"] = "killed".into();
        mutant["killed_by"] = "test/math.test.ts > adds".into();
        mutant["covering_tests"] = serde_json::json!(["test/math.test.ts > adds"]);
        let fields = mutant.as_object_mut().unwrap();
        fields.remove("test_reach");
        fields.remove("prune_reason");
        let id = mutant["mutant_id"].as_str().unwrap().to_owned();
        if in_math {
            killed.insert(0, id);
        } else {
            killed.push(id);
        }
    }
    let dir = tempfile::tempdir().expect("report dir");
    let path = dir.path().join("saved.json");
    fs::write(&path, serde_json::to_string(&report).unwrap()).expect("saved report");
    (project, dir, path, killed)
}

fn mutant<'a>(report: &'a serde_json::Value, id: &str) -> Option<&'a serde_json::Value> {
    report["mutants"]
        .as_array()?
        .iter()
        .find(|mutant| mutant["mutant_id"] == id)
}

#[test]
fn recheck_replaces_the_named_report_and_keeps_the_earlier_kills() {
    let (project, _dir, path, killed) = recheck_fixture();
    let report = path.to_str().unwrap();
    let (output, recheck) = json_mutation(project.path(), &["--recheck", "--report", report]);
    assert_eq!(output.code, 0, "{}", output.stderr);

    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("saved report")).unwrap();
    assert_eq!(saved, recheck, "the recheck replaces the named report");
    let last: serde_json::Value = serde_json::from_slice(
        &fs::read(project.path().join(".fallow/mutation/last-report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(last, recheck, "the last report stays in step");

    let counts = &recheck["summary"]["counts"];
    let total = recheck["mutants"].as_array().unwrap().len();
    assert_eq!(counts["killed"], killed.len(), "{counts}");
    assert_eq!(counts["pruned"], 0, "{counts}");
    assert_eq!(counts["no_coverage"], total - killed.len(), "{counts}");
    assert_eq!(recheck["recheck"]["carried"], killed.len());
    for id in &killed {
        let carried = mutant(&recheck, id).expect("each earlier mutant stays in the report");
        assert_eq!(carried["status"], "killed", "{carried}");
        assert_eq!(carried["carried"], true, "{carried}");
        assert_eq!(carried["killed_by"], "test/math.test.ts > adds");
    }
    let rechecked = recheck["recheck"]["mutants"][0]["mutant_id"]
        .as_str()
        .unwrap();
    assert!(
        mutant(&recheck, rechecked)
            .unwrap()
            .get("carried")
            .is_none(),
        "a rechecked mutant has the result of this run"
    );
    let extra = recheck["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|function| function["file"] == "src/extra.ts")
        .expect("the function of the file that no mutant of the recheck is in");
    assert_eq!(extra["score"], 1.0);
    let pruned: u64 = recheck["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|function| function["counts"]["pruned"].as_u64().unwrap())
        .sum();
    assert_eq!(pruned, 0, "the function results count the merged mutants");

    let (_, tasks) = json_mutation(project.path(), &["tasks", "--report", report]);
    assert_eq!(tasks["summary"]["tasks"], total - killed.len());
}

#[test]
fn recheck_does_not_keep_the_earlier_kills_of_a_changed_source() {
    let (project, _dir, path, killed) = recheck_fixture();
    fs::write(
        project.path().join("src/extra.ts"),
        EXTRA.replace("n * 2", "n * 3"),
    )
    .expect("edit extra.ts");
    commit_all(project.path(), "edit extra");
    let report = path.to_str().unwrap();
    let (output, recheck) = json_mutation(project.path(), &["--recheck", "--report", report]);
    assert_eq!(output.code, 0, "{}", output.stderr);

    assert!(
        recheck["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .all(|mutant| mutant["file"] != "src/extra.ts"),
        "the earlier results of a changed file do not apply"
    );
    assert_eq!(
        recheck["recheck"]["stale_files"],
        serde_json::json!(["src/extra.ts"])
    );
    assert_eq!(recheck["recheck"]["carried"], 1);
    assert_eq!(recheck["summary"]["counts"]["killed"], 1);
    assert_eq!(mutant(&recheck, &killed[0]).unwrap()["status"], "killed");
}

#[test]
fn recheck_does_not_keep_a_kill_of_a_test_file_that_is_gone() {
    let (project, _dir, path, killed) = recheck_fixture();
    fs::create_dir_all(project.path().join("fixtures")).expect("fixtures");
    fs::write(project.path().join("fixtures/kept.js"), "export {};\n").expect("kept.js");
    commit_all(project.path(), "kept test file");
    set_killing_test(&path, "src/math.ts", "test/gone.test.ts::adds");
    set_killing_test(&path, "src/extra.ts", "fixtures/kept.js::doubles");
    let report = path.to_str().unwrap();
    let (output, recheck) = json_mutation(project.path(), &["--recheck", "--report", report]);
    assert_eq!(output.code, 0, "{}", output.stderr);

    let gone = mutant(&recheck, &killed[0]).expect("the mutant of a run file stays");
    assert_eq!(gone["status"], "pruned", "{gone}");
    assert_eq!(gone["prune_reason"], "not-selected", "{gone}");
    assert_eq!(recheck["recheck"]["carried"], killed.len() - 1);
    for id in &killed[1..] {
        assert_eq!(mutant(&recheck, id).unwrap()["carried"], true);
    }

    set_killing_test(&path, "src/extra.ts", "test/gone.test.ts::doubles");
    let (output, recheck) = json_mutation(project.path(), &["--recheck", "--report", report]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        recheck["recheck"]["stale_files"],
        serde_json::json!(["src/extra.ts"])
    );
    assert_eq!(recheck["recheck"]["carried"], 0);
}

#[cfg(unix)]
#[test]
fn recheck_replaces_the_target_of_a_report_symlink_and_keeps_its_mode() {
    use std::os::unix::fs::PermissionsExt;

    let (project, dir, path, _) = recheck_fixture();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).expect("mode");
    let link = dir.path().join("link.json");
    std::os::unix::fs::symlink(&path, &link).expect("symlink");
    let (output, recheck) = json_mutation(
        project.path(),
        &["--recheck", "--report", link.to_str().unwrap()],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);

    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        saved, recheck,
        "the recheck replaces the target of the link"
    );
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
}

/// Sets the killing test of each killed mutant of `file` in the saved report
/// at `path` to `test`.
fn set_killing_test(path: &Path, file: &str, test: &str) {
    let mut saved: serde_json::Value =
        serde_json::from_slice(&fs::read(path).expect("saved report")).unwrap();
    for mutant in saved["mutants"].as_array_mut().unwrap() {
        if mutant["status"] == "killed" && mutant["file"] == file {
            mutant["killed_by"] = test.into();
            mutant["killing_tests"] = serde_json::json!([test]);
        }
    }
    fs::write(path, serde_json::to_string(&saved).unwrap()).expect("saved report");
}

#[test]
fn recheck_leaves_out_a_file_with_an_earlier_entry_that_does_not_parse() {
    let (project, _dir, path, _) = recheck_fixture();
    let mut saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("saved report")).unwrap();
    let extra = saved["mutants"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|mutant| mutant["file"] == "src/extra.ts")
        .expect("a mutant of src/extra.ts");
    // A report of a version before the kill matrix has no such field.
    extra
        .as_object_mut()
        .unwrap()
        .remove("kill_matrix_complete");
    fs::write(&path, serde_json::to_string(&saved).unwrap()).expect("saved report");
    let report = path.to_str().unwrap();
    let (output, recheck) = json_mutation(project.path(), &["--recheck", "--report", report]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        recheck["recheck"]["stale_files"],
        serde_json::json!(["src/extra.ts"])
    );
    assert!(
        recheck["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .all(|mutant| mutant["file"] != "src/extra.ts"),
        "the report shows no part of the file"
    );
}

#[test]
fn typecheck_without_a_companion_exits_2_before_the_tests_run() {
    let project = untested_project();
    let missing = project.path().join("no-such-companion");
    let missing = missing.to_str().expect("utf-8 path");
    let root = project.path().to_str().expect("utf-8 path");
    let output = crate::common::run_fallow_raw_with_env(
        &[
            "mutation",
            "--root",
            root,
            "--typecheck",
            "--format",
            "json",
            "--quiet",
        ],
        &[("FALLOW_TYPE_AWARE_BIN", missing)],
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
    let value = parse_json(&output);
    assert_eq!(value["code"], "FALLOW_MUTATION_TYPECHECK_UNAVAILABLE");
    assert!(
        !project
            .path()
            .join(".fallow/mutation/last-report.json")
            .exists(),
        "no run starts without the companion"
    );

    let plan = mutation(project.path(), &["--plan", "--typecheck", "--quiet"]);
    assert_eq!(plan.code, 2, "{}", plan.stdout);
    assert!(
        plan.stderr.contains("a plan has no survivors"),
        "{}",
        plan.stderr
    );
}

/// A git project with a test file and a `vitest` package of version
/// 4.0.18. The companion reads the version before it loads Vitest, so the
/// package needs no code.
fn vitest_4_0_project() -> tempfile::TempDir {
    let dir = untested_project();
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"mutation-fixture","private":true,"type":"module","devDependencies":{"vitest":"4.0.18"}}"#,
    )
    .expect("package.json");
    fs::create_dir_all(root.join("test")).expect("test");
    fs::write(
        root.join("test/math.test.ts"),
        "import { expect, test } from \"vitest\";\nimport { add } from \"../src/math\";\n\ntest(\"adds\", () => {\n  expect(add(2, 3)).toBe(5);\n});\n",
    )
    .expect("math.test.ts");
    commit_all(root, "test");
    fs::create_dir_all(root.join("node_modules/vitest")).expect("vitest");
    fs::write(
        root.join("node_modules/vitest/package.json"),
        r#"{"name":"vitest","version":"4.0.18"}"#,
    )
    .expect("vitest package.json");
    dir
}

/// A Vitest version outside the supported range exits 2 with
/// `FALLOW_MUTATION_RUNNER_UNSUPPORTED`. The help names the supported range
/// as the install step, in JSON and in the human `hint:` line.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "a skipped test says why it did not run"
)]
fn an_unsupported_vitest_version_gives_the_supported_range_as_help() {
    let has_node = std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !has_node {
        eprintln!("skipped: the runner version test needs `node` on PATH");
        return;
    }
    const HELP: &str = "install vitest 4.1.0 or a later version below 6.0.0; a vitest 4.x release from 4.1.0 is in the range";
    let project = vitest_4_0_project();

    let (output, value) = json_mutation(project.path(), &["--all"]);
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert_eq!(
        value["code"], "FALLOW_MUTATION_RUNNER_UNSUPPORTED",
        "{value}"
    );
    assert!(
        value["message"]
            .as_str()
            .unwrap_or_default()
            .contains("vitest 4.0.18"),
        "{value}"
    );
    assert_eq!(value["help"], HELP, "{value}");

    let human = mutation(project.path(), &["--all", "--quiet"]);
    assert_eq!(human.code, 2, "{}", human.stderr);
    assert!(
        human.stderr.contains(&format!("\n  hint: {HELP}")),
        "{}",
        human.stderr
    );
}

#[test]
fn mutation_rejects_unsupported_formats_and_scope_conflicts() {
    let project = untested_project();
    let sarif = mutation(project.path(), &["--plan", "--format", "sarif"]);
    assert_eq!(sarif.code, 2, "{}", sarif.stdout);
    let conflict = mutation(
        project.path(),
        &["--plan", "--all", "--changed-since", "HEAD", "--quiet"],
    );
    assert_eq!(conflict.code, 2, "{}", conflict.stderr);
    assert!(
        conflict.stderr.contains("--changed-since"),
        "{}",
        conflict.stderr
    );
    let workspace = mutation(project.path(), &["--plan", "--workspace", "web", "--quiet"]);
    assert_eq!(workspace.code, 2, "{}", workspace.stderr);
    assert!(
        workspace
            .stderr
            .contains("pass the workspace directory as a path"),
        "{}",
        workspace.stderr
    );
}

/// The Vitest run needs a symlink to `node_modules`, so it runs on Unix only.
#[cfg(unix)]
mod end_to_end {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{commit_all, git, json_mutation, mutation};

    #[expect(
        clippy::print_stderr,
        reason = "a skipped end-to-end test says why it did not run"
    )]
    fn companion_package() -> Option<PathBuf> {
        let package =
            Path::new(&std::env::var_os("CARGO_MANIFEST_DIR")?).join("../../tools/mutation-runner");
        let has_vitest = package.join("node_modules/vitest/package.json").is_file();
        let has_node = std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !has_vitest || !has_node {
            eprintln!(
                "skipped: the mutation end-to-end test needs `node` on PATH and `npm ci` in {}",
                package.display()
            );
            return None;
        }
        Some(dunce::canonicalize(package).expect("package path"))
    }

    /// A git project with a copy of the `vitest-basic` companion fixture.
    /// `extra_source` is appended to `src/math.ts` before the commit.
    fn vitest_project(package: &Path, extra_source: &str) -> (tempfile::TempDir, PathBuf) {
        let fixture = package.join("test/fixtures/vitest-basic");
        let project = tempfile::tempdir().expect("dir");
        let root = dunce::canonicalize(project.path()).expect("root");
        for file in [
            "package.json",
            "vitest.config.mjs",
            "src/math.ts",
            "test/math.test.ts",
            "test/user-setup.ts",
        ] {
            let target = root.join(file);
            fs::create_dir_all(target.parent().expect("parent")).expect("dir");
            fs::copy(fixture.join(file), &target).expect("copy");
        }
        fs::write(root.join(".gitignore"), "node_modules\n").expect("write");
        fs::create_dir_all(root.join("node_modules")).expect("dir");
        std::os::unix::fs::symlink(
            package.join("node_modules/vitest"),
            root.join("node_modules/vitest"),
        )
        .expect("symlink");
        if !extra_source.is_empty() {
            let math = root.join("src/math.ts");
            let mut source = fs::read_to_string(&math).expect("math.ts");
            source.push_str(extra_source);
            fs::write(&math, source).expect("math.ts");
        }
        git(&root, &["init", "-q"]);
        commit_all(&root, "init");
        (project, root)
    }

    /// The sorted ids of the `no_coverage` mutants of a run report.
    fn no_coverage_ids(report: &serde_json::Value) -> Vec<String> {
        let mut ids: Vec<String> = report["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|mutant| mutant["status"] == "no_coverage")
            .map(|mutant| mutant["mutant_id"].as_str().expect("id").to_owned())
            .collect();
        ids.sort();
        ids
    }

    /// `triple` is in a module that the test imports, so the static graph
    /// reaches it, but no test calls it. A run that `--max-mutants` caps
    /// leaves its mutants pending, and the dry run must still give them
    /// `no_coverage`, as the full run does.
    #[test]
    fn a_capped_run_reports_the_coverage_and_no_coverage_mutants_of_a_full_run() {
        let Some(package) = companion_package() else {
            return;
        };
        let (_project, root) = vitest_project(
            &package,
            "\nexport function triple(n: number): number {\n  return n * 3;\n}\n",
        );
        let args = ["src/math.ts", "--concurrency", "1"];
        let (full_output, full) = json_mutation(&root, &args);
        assert_eq!(full_output.code, 0, "{}", full_output.stderr);
        let mut capped_args = args.to_vec();
        capped_args.extend(["--max-mutants", "1"]);
        let (capped_output, capped) = json_mutation(&root, &capped_args);
        assert_eq!(capped_output.code, 0, "{}", capped_output.stderr);

        assert!(
            capped["summary"]["counts"]["pending"].as_u64() > Some(0),
            "{capped}"
        );
        let triple: Vec<String> = full["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|mutant| mutant["function"] == "triple")
            .map(|mutant| mutant["mutant_id"].as_str().expect("id").to_owned())
            .collect();
        assert!(!triple.is_empty(), "{full}");
        for report in [&full, &capped] {
            let reach: Vec<&serde_json::Value> = report["mutants"]
                .as_array()
                .expect("mutants")
                .iter()
                .filter(|mutant| mutant["function"] == "triple")
                .map(|mutant| &mutant["test_reach"])
                .collect();
            assert!(
                reach
                    .iter()
                    .all(|reach| reach["reach"] == "none" && reach["export_name"] == "triple"),
                "a test loads the file, but no test references triple: {report}"
            );
        }
        let full_ids = no_coverage_ids(&full);
        assert!(
            triple.iter().all(|id| full_ids.contains(id)),
            "the full run gives no_coverage to each mutant of triple: {full}"
        );
        assert_eq!(no_coverage_ids(&capped), full_ids);
        let coverage = full["summary"]["coverage"].as_f64().expect("a coverage");
        assert!(coverage > 0.0 && coverage < 1.0, "{full}");
        assert_eq!(capped["summary"]["coverage"], full["summary"]["coverage"]);
    }

    /// The type-aware companion needs `npm ci` in its package.
    #[expect(
        clippy::print_stderr,
        reason = "a skipped end-to-end test says why it did not run"
    )]
    fn type_aware_companion_installed() -> bool {
        let Some(manifest_dir) = std::env::var_os("CARGO_MANIFEST_DIR") else {
            return false;
        };
        let package = Path::new(&manifest_dir).join("../../tools/type-aware-sidecar");
        let installed = package
            .join("node_modules/typescript/package.json")
            .is_file();
        if !installed {
            eprintln!(
                "skipped: the mutation type check test needs `npm ci` in {}",
                package.display()
            );
        }
        installed
    }

    /// `isPositive` returns a boolean, and its test checks no value. The
    /// `return-value` mutant survives the tests, but `return undefined` does
    /// not type-check under `strict`, so the type check moves it to
    /// `compile_error` and out of the score. The run without `--typecheck`
    /// keeps it a survivor.
    #[test]
    fn a_typecheck_run_moves_a_survivor_that_does_not_type_check_out_of_the_score() {
        let Some(package) = companion_package() else {
            return;
        };
        if !type_aware_companion_installed() {
            return;
        }
        let (_project, root) = vitest_project(&package, "");
        fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"noEmit":true},"include":["src"]}"#,
        )
        .expect("tsconfig");
        commit_all(&root, "tsconfig");

        let args = ["src/math.ts", "--concurrency", "1"];
        let (plain_output, plain) = json_mutation(&root, &args);
        assert_eq!(plain_output.code, 0, "{}", plain_output.stderr);
        assert!(plain.get("typecheck").is_none(), "{plain}");
        let return_value = |report: &serde_json::Value| -> serde_json::Value {
            report["mutants"]
                .as_array()
                .expect("mutants")
                .iter()
                .find(|mutant| {
                    mutant["function"] == "isPositive" && mutant["operator"] == "return-value"
                })
                .cloned()
                .expect("a return-value mutant of isPositive")
        };
        assert_eq!(return_value(&plain)["status"], "survived", "{plain}");

        let mut command = std::process::Command::new(crate::common::fallow_bin());
        command
            .env("RUST_LOG", "")
            .env("NO_COLOR", "1")
            .args(["mutation", "--root"])
            .arg(&root)
            .args(args)
            .args(["--typecheck", "--format", "json", "--quiet"]);
        crate::common::configure_type_aware_sidecar(&mut command);
        let output = command.output().expect("fallow runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let checked: serde_json::Value = serde_json::from_str(&stdout).expect("json");
        let mutant = return_value(&checked);
        assert_eq!(mutant["status"], "compile_error", "{checked}");
        assert_eq!(mutant["type_error_codes"], serde_json::json!([2322]));
        assert_eq!(checked["typecheck"]["completed"], true, "{checked}");
        assert!(checked["typecheck"]["compile_errors"].as_u64() >= Some(1));
        let counts = &checked["summary"]["counts"];
        assert_eq!(
            counts["compile_error"],
            checked["typecheck"]["compile_errors"]
        );
        assert_eq!(
            counts["survived"].as_u64().expect("survived")
                + counts["compile_error"].as_u64().expect("compile errors"),
            plain["summary"]["counts"]["survived"]
                .as_u64()
                .expect("survived")
        );
        let detected = counts["killed"].as_f64().expect("killed")
            + counts["timeout"].as_f64().expect("timeout");
        let executed = detected + counts["survived"].as_f64().expect("survived");
        assert_eq!(
            checked["summary"]["score"].as_f64(),
            Some(detected / executed),
            "{checked}"
        );
        assert_eq!(checked["completion"]["status"], "complete", "{checked}");
    }

    /// A hinted survivor stays `survived`: no `equivalent` mutant, the same
    /// survivors as the plain run (less the compile errors), and the score
    /// is detected over executed.
    fn assert_hint_leaves_the_score(checked: &serde_json::Value, plain: &serde_json::Value) {
        let counts = &checked["summary"]["counts"];
        assert_eq!(counts["equivalent"], 0, "{checked}");
        assert_eq!(
            counts["survived"].as_u64().expect("survived")
                + counts["compile_error"].as_u64().expect("compile errors"),
            plain["summary"]["counts"]["survived"]
                .as_u64()
                .expect("survived"),
            "{checked}"
        );
        let detected = counts["killed"].as_f64().expect("killed")
            + counts["timeout"].as_f64().expect("timeout");
        let executed = detected + counts["survived"].as_f64().expect("survived");
        assert_eq!(
            checked["summary"]["score"].as_f64(),
            Some(detected / executed),
            "{checked}"
        );
        assert_eq!(checked["completion"]["status"], "complete", "{checked}");
    }

    /// `readB` reads `a?.b` where `a` is a plain object type. No test can
    /// tell `a?.b` from `a.b`, so the mutant survives the tests. With
    /// `strictNullChecks` and `noUncheckedIndexedAccess`, the type check adds
    /// the advisory `equivalence_hint` to the survivor. The hint never changes
    /// the status or the score. Without `--typecheck` the survivor has no
    /// hint.
    #[test]
    fn a_typecheck_run_hints_an_optional_link_on_a_non_nullable_receiver_without_changing_the_score()
     {
        let Some(package) = companion_package() else {
            return;
        };
        if !type_aware_companion_installed() {
            return;
        }
        let (_project, root) = vitest_project(
            &package,
            "\nexport function readB(a: { b: number }): number | undefined {\n  return a?.b;\n}\n",
        );
        fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"noUncheckedIndexedAccess":true,"noEmit":true},"include":["src"]}"#,
        )
        .expect("tsconfig");
        fs::write(
            root.join("test/chain.test.ts"),
            "import { expect, test } from \"vitest\";\nimport { readB } from \"../src/math\";\n\ntest(\"reads b\", () => {\n  expect(readB({ b: 4 })).toBe(4);\n});\n",
        )
        .expect("chain test");
        commit_all(&root, "chain");

        let args = ["src/math.ts", "--concurrency", "1"];
        let (plain_output, plain) = json_mutation(&root, &args);
        assert_eq!(plain_output.code, 0, "{}", plain_output.stderr);
        let link = |report: &serde_json::Value| -> serde_json::Value {
            report["mutants"]
                .as_array()
                .expect("mutants")
                .iter()
                .find(|mutant| {
                    mutant["function"] == "readB" && mutant["operator"] == "optional-chaining"
                })
                .cloned()
                .expect("an optional-chaining mutant of readB")
        };
        assert_eq!(link(&plain)["status"], "survived", "{plain}");
        assert!(link(&plain).get("equivalence_hint").is_none(), "{plain}");

        let typecheck_run = |extra: &[&str]| -> serde_json::Value {
            let mut command = std::process::Command::new(crate::common::fallow_bin());
            command
                .env("RUST_LOG", "")
                .env("NO_COLOR", "1")
                .args(["mutation", "--root"])
                .arg(&root)
                .args(args)
                .args(extra)
                .args(["--typecheck", "--format", "json", "--quiet"]);
            crate::common::configure_type_aware_sidecar(&mut command);
            let output = command.output().expect("fallow runs");
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("json")
        };
        let checked = typecheck_run(&[]);
        let mutant = link(&checked);
        assert_eq!(mutant["status"], "survived", "{checked}");
        assert_eq!(mutant["equivalence_hint"], "type-non-nullish", "{checked}");
        assert_eq!(checked["typecheck"]["completed"], true, "{checked}");
        assert_eq!(checked["typecheck"]["hinted"], 1, "{checked}");
        assert_hint_leaves_the_score(&checked, &plain);

        // A verdict for the hinted survivor makes it `equivalent` and
        // removes the hint, so the report does not show both.
        let verdict_path = root.join("verdicts.json");
        fs::write(
            &verdict_path,
            serde_json::to_string(&serde_json::json!({
                "schema_version": "1",
                "verdicts": [{ "review_key": mutant["mutant_id"], "verdict": "equivalent" }],
            }))
            .expect("verdict json"),
        )
        .expect("verdicts");
        commit_all(&root, "verdicts");
        let judged = typecheck_run(&["--verdicts", verdict_path.to_str().expect("path")]);
        let mutant = link(&judged);
        assert_eq!(mutant["status"], "equivalent", "{judged}");
        assert!(mutant.get("equivalence_hint").is_none(), "{judged}");
        assert!(judged["typecheck"].get("hinted").is_none(), "{judged}");
        assert_eq!(
            judged["typecheck"]["checked"],
            checked["typecheck"]["checked"]
        );
        assert_eq!(judged["summary"]["counts"]["equivalent"], 1, "{judged}");
    }

    /// `labelOf` reads `name ?? "anon"` where `name` is a `string`. No test
    /// can tell the expression from `name`, so the opt-in `nullish-fallback`
    /// mutant survives. The type check adds the advisory hint, and the
    /// status and the score do not change. A run of the default operators
    /// has no `nullish-fallback` mutant.
    #[test]
    fn a_typecheck_run_hints_a_nullish_fallback_on_a_non_nullable_receiver() {
        let Some(package) = companion_package() else {
            return;
        };
        if !type_aware_companion_installed() {
            return;
        }
        let (_project, root) = vitest_project(
            &package,
            "\nexport function labelOf(name: string): string {\n  return name ?? \"anon\";\n}\n",
        );
        fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"noUncheckedIndexedAccess":true,"noEmit":true},"include":["src"]}"#,
        )
        .expect("tsconfig");
        fs::write(
            root.join("test/label.test.ts"),
            "import { expect, test } from \"vitest\";\nimport { labelOf } from \"../src/math\";\n\ntest(\"labels\", () => {\n  expect(labelOf(\"ada\")).toBe(\"ada\");\n});\n",
        )
        .expect("label test");
        commit_all(&root, "label");

        let fallback = |report: &serde_json::Value| -> Option<serde_json::Value> {
            report["mutants"]
                .as_array()
                .expect("mutants")
                .iter()
                .find(|mutant| {
                    mutant["function"] == "labelOf" && mutant["operator"] == "nullish-fallback"
                })
                .cloned()
        };
        let (default_output, default_run) =
            json_mutation(&root, &["src/math.ts", "--concurrency", "1"]);
        assert_eq!(default_output.code, 0, "{}", default_output.stderr);
        assert!(fallback(&default_run).is_none(), "{default_run}");

        let args = [
            "src/math.ts",
            "--concurrency",
            "1",
            "--operators",
            "nullish-fallback",
        ];
        let (plain_output, plain) = json_mutation(&root, &args);
        assert_eq!(plain_output.code, 0, "{}", plain_output.stderr);
        let mutant = fallback(&plain).expect("a nullish-fallback mutant of labelOf");
        assert_eq!(mutant["status"], "survived", "{plain}");
        assert!(mutant.get("equivalence_hint").is_none(), "{plain}");

        let mut command = std::process::Command::new(crate::common::fallow_bin());
        command
            .env("RUST_LOG", "")
            .env("NO_COLOR", "1")
            .args(["mutation", "--root"])
            .arg(&root)
            .args(args)
            .args(["--typecheck", "--format", "json", "--quiet"]);
        crate::common::configure_type_aware_sidecar(&mut command);
        let output = command.output().expect("fallow runs");
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let checked: serde_json::Value =
            serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("json");
        let mutant = fallback(&checked).expect("a nullish-fallback mutant of labelOf");
        assert_eq!(mutant["status"], "survived", "{checked}");
        assert_eq!(mutant["equivalence_hint"], "type-non-nullish", "{checked}");
        assert_eq!(checked["typecheck"]["hinted"], 1, "{checked}");
        assert_hint_leaves_the_score(&checked, &plain);
    }

    /// A component compiler in the shape of the Vue plugin: its `transform`
    /// hook keeps the script block of a `.vue` file.
    const COMPONENT_CONFIG: &str = r#"const componentPlugin = () => ({
  name: "fixture-component",
  enforce: "pre",
  transform(code, id) {
    if (!id.endsWith(".vue")) return null;
    return /<script>([\s\S]*?)<\/script>/u.exec(code)?.[1] ?? "";
  },
});

export default {
  plugins: [componentPlugin()],
  test: { include: ["test/**/*.test.ts"] },
};
"#;

    /// The TypeScript checker cannot read a Vue file, so a survivor in a
    /// component gets the skip reason `unsupported-file`, not `no-project`.
    #[test]
    fn a_typecheck_run_gives_a_component_survivor_its_own_skip_reason() {
        let Some(package) = companion_package() else {
            return;
        };
        if !type_aware_companion_installed() {
            return;
        }
        let (_project, root) = vitest_project(&package, "");
        fs::write(root.join("vitest.config.mjs"), COMPONENT_CONFIG).expect("config");
        fs::write(
            root.join("src/Badge.vue"),
            "<template><span>{{ label(1) }}</span></template>\n<script>\nexport function label(n) {\n  return n === 1 ? \"one\" : \"many\";\n}\n</script>\n",
        )
        .expect("component");
        fs::write(
            root.join("test/badge.test.ts"),
            "import { test } from \"vitest\";\nimport { label } from \"../src/Badge.vue\";\n\ntest(\"label runs\", () => {\n  label(1);\n});\n",
        )
        .expect("test");
        fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"noEmit":true},"include":["src"]}"#,
        )
        .expect("tsconfig");
        commit_all(&root, "component");

        let mut command = std::process::Command::new(crate::common::fallow_bin());
        command
            .env("RUST_LOG", "")
            .env("NO_COLOR", "1")
            .args(["mutation", "--root"])
            .arg(&root)
            .args(["src/Badge.vue", "--concurrency", "1"])
            .args(["--typecheck", "--format", "json", "--quiet"]);
        crate::common::configure_type_aware_sidecar(&mut command);
        let output = command.output().expect("fallow runs");
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
        let survived = report["summary"]["counts"]["survived"]
            .as_u64()
            .expect("survived");
        assert!(survived > 0, "the test checks no value: {report}");
        assert_eq!(report["typecheck"]["completed"], true, "{report}");
        assert_eq!(report["typecheck"]["checked"], 0, "{report}");
        assert_eq!(
            report["typecheck"]["skipped"],
            serde_json::json!([{ "reason": "unsupported-file", "count": survived }]),
            "{report}"
        );
    }

    #[test]
    fn a_human_vitest_run_shows_each_phase_with_the_mutant_counts() {
        let Some(package) = companion_package() else {
            return;
        };
        let (_project, root) = vitest_project(&package, "");
        let human = mutation(&root, &["src/math.ts", "--concurrency", "1"]);
        assert_eq!(human.code, 0, "{}", human.stderr);
        let lines = super::progress_lines(&human.stderr);
        let position = |text: &str| {
            lines
                .iter()
                .position(|line| line.contains(text))
                .unwrap_or_else(|| panic!("no `{text}` line: {}", human.stderr))
        };
        let setup = position("runner setup for ");
        let baseline = position("baseline run of 1 test file with no active mutant");
        let first = position(" mutants done (");
        let finalize = position("the tests stopped; worktree check and report");
        assert!(
            setup < baseline && baseline < first && first < finalize,
            "{lines:#?}"
        );
        let counts: Vec<(usize, usize)> = lines
            .iter()
            .filter_map(|line| {
                let text = line.strip_prefix("fallow: mutation: ")?;
                let (count, rest) = text.split_once(' ')?;
                rest.starts_with("mutants done").then_some(())?;
                let (done, total) = count.split_once('/')?;
                Some((done.parse().ok()?, total.parse().ok()?))
            })
            .collect();
        let total = counts[0].1;
        assert!(total > 0, "{lines:#?}");
        assert_eq!(counts.first(), Some(&(0, total)), "{lines:#?}");
        assert_eq!(counts.last(), Some(&(total, total)), "{lines:#?}");
    }

    #[test]
    fn a_vitest_run_kills_add_mutants_and_reports_a_survivor_of_is_positive() {
        let Some(package) = companion_package() else {
            return;
        };
        let (_project, root) = vitest_project(&package, "");

        let (output, report) = json_mutation(
            &root,
            &["src/math.ts", "--concurrency", "1", "--min-score", "100"],
        );
        assert_eq!(
            output.code, 1,
            "a survivor misses a minimum of 100: {}",
            output.stderr
        );
        let score = report["summary"]["score"].as_f64().expect("a score");
        assert!(score > 0.0 && score < 1.0, "{report}");
        assert_eq!(report["runner"]["name"], "vitest");
        assert_eq!(report["completion"]["status"], "complete", "{report}");
        let mutants = report["mutants"].as_array().unwrap();
        assert!(
            mutants
                .iter()
                .filter(|mutant| mutant["function"] == "add")
                .all(|mutant| mutant["status"] == "killed")
        );
        assert!(
            mutants
                .iter()
                .any(|mutant| mutant["function"] == "isPositive" && mutant["status"] == "survived")
        );

        let human = mutation(&root, &["src/math.ts", "--concurrency", "1", "--quiet"]);
        assert_eq!(human.code, 0, "{}", human.stderr);
        assert!(human.stdout.contains("Survived ("), "{}", human.stdout);
        assert!(human.stdout.contains("src/math.ts"), "{}", human.stdout);
        assert!(
            human.stdout.contains("return n > 0; -> "),
            "{}",
            human.stdout
        );
        let inspect = human
            .stdout
            .lines()
            .find_map(|line| line.split_once("Inspect a survivor (runs no code): "))
            .map(|(_, command)| command.to_owned())
            .expect("an inspect step");
        let words: Vec<&str> = inspect.split_whitespace().collect();
        assert_eq!(&words[..3], &["fallow", "mutation", "inspect"], "{inspect}");
        let shown = crate::common::run_fallow_raw(&words[1..]);
        assert_eq!(
            shown.code, 0,
            "the printed step runs as-is: {}",
            shown.stderr
        );
        assert!(shown.stdout.contains("fresh"), "{}", shown.stdout);
    }

    /// Each test file fails at load with no active mutant, so no mutant gets
    /// a test result. The run exits 2 with the baseline error and still
    /// saves the report, also to `--output-file`.
    #[test]
    fn a_run_that_baseline_failures_block_exits_2_and_keeps_the_report() {
        let Some(package) = companion_package() else {
            return;
        };
        let (_project, root) = vitest_project(&package, "");
        fs::write(
            root.join("test/math.test.ts"),
            "import { test } from \"vitest\";\nimport { add } from \"../src/math\";\n\nif (add(1, 1) === 2) {\n  throw new Error(\"Invalid environment variables\");\n}\n\ntest(\"adds\", () => {});\n",
        )
        .expect("test file");
        commit_all(&root, "a test file that fails at load");
        let args = ["src/math.ts", "--concurrency", "1"];

        let (output, envelope) = json_mutation(&root, &args);
        assert_eq!(output.code, 2, "{}", output.stderr);
        assert_eq!(
            envelope["code"], "FALLOW_MUTATION_BASELINE_FAILED",
            "{envelope}"
        );
        let message = envelope["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("Invalid environment variables"),
            "{envelope}"
        );
        let saved_path = root.join(".fallow/mutation/last-report.json");
        let saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&saved_path).expect("saved report"))
                .expect("json");
        assert_eq!(saved["kind"], "mutation", "{saved}");
        assert!(
            saved["baseline_failures"]
                .as_array()
                .is_some_and(|failures| !failures.is_empty()),
            "{saved}"
        );
        assert!(
            envelope["help"]
                .as_str()
                .is_some_and(|help| help.contains("last-report.json")),
            "{envelope}"
        );

        let reports = tempfile::tempdir().expect("dir");
        let file = reports.path().join("mutation.json");
        let mut with_file = args.to_vec();
        with_file.extend(["-o", file.to_str().expect("path")]);
        let (output, envelope) = json_mutation(&root, &with_file);
        assert_eq!(output.code, 2, "{}", output.stderr);
        assert_eq!(
            envelope["code"], "FALLOW_MUTATION_BASELINE_FAILED",
            "{envelope}"
        );
        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).expect("output file")).expect("json");
        assert_eq!(written["kind"], "mutation", "{written}");

        let human = mutation(&root, &["src/math.ts", "--concurrency", "1", "--quiet"]);
        assert_eq!(human.code, 2, "{}", human.stderr);
        assert!(
            human
                .stdout
                .contains("  Message: Invalid environment variables"),
            "{}",
            human.stdout
        );
        assert!(
            human.stderr.contains("no mutant has a test result"),
            "{}",
            human.stderr
        );
    }

    /// The test file of an order-dependent suite: `adds after prepares`
    /// passes only after `prepares` ran in the same file.
    const ORDER_DEPENDENT_TEST: &str = "import { expect, test } from \"vitest\";\nimport { add } from \"../src/math\";\n\nlet prepared = false;\n\ntest(\"prepares\", () => {\n  prepared = true;\n});\n\ntest(\"adds after prepares\", () => {\n  expect(prepared).toBe(true);\n  expect(add(2, 3)).toBe(5);\n});\n";

    /// The dry run runs the whole file, so `adds after prepares` passes
    /// there. A mutant run selects only the covering test of an `add`
    /// mutant, so that test fails with no active mutant. The self-check
    /// finds this before the first mutant job. The test becomes a baseline
    /// failure and covers no mutant, and the run continues, so no `add`
    /// mutant gets a false kill.
    #[test]
    fn a_test_that_fails_in_the_self_check_is_a_baseline_failure_and_the_run_continues() {
        let Some(package) = companion_package() else {
            return;
        };
        let (_project, root) = vitest_project(&package, "");
        fs::write(root.join("test/math.test.ts"), ORDER_DEPENDENT_TEST).expect("test file");
        commit_all(&root, "an order-dependent test");

        let (output, report) = json_mutation(&root, &["src/math.ts", "--concurrency", "1"]);
        assert_eq!(output.code, 0, "{}", output.stderr);
        let failures = report["baseline_failures"].as_array().expect("failures");
        assert_eq!(failures.len(), 1, "{report}");
        assert_eq!(
            failures[0]["test_id"], "test/math.test.ts::adds after prepares",
            "{report}"
        );
        assert!(
            failures[0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("depend on another test of its file")),
            "{report}"
        );
        let mutants = report["mutants"].as_array().expect("mutants");
        let add: Vec<&serde_json::Value> = mutants
            .iter()
            .filter(|mutant| mutant["function"] == "add")
            .collect();
        assert!(!add.is_empty(), "{report}");
        assert!(
            add.iter().all(|mutant| mutant["status"] == "no_coverage"),
            "{report}"
        );
        assert!(
            mutants.iter().any(|mutant| mutant["status"] == "survived"),
            "the static mutant runs with `prepares`: {report}"
        );
        assert_eq!(report["completion"]["status"], "partial", "{report}");
    }

    /// The same suite with only `add` in the module: the baseline failure
    /// leaves each mutant without a test result, so the run measured nothing
    /// and exits 2 with the baseline error.
    #[test]
    fn a_self_check_failure_that_leaves_no_test_result_exits_2_with_the_baseline_error() {
        let Some(package) = companion_package() else {
            return;
        };
        let (_project, root) = vitest_project(&package, "");
        fs::write(
            root.join("src/math.ts"),
            "export function add(a: number, b: number): number {\n  return a + b;\n}\n",
        )
        .expect("math.ts");
        fs::write(root.join("test/math.test.ts"), ORDER_DEPENDENT_TEST).expect("test file");
        commit_all(&root, "an order-dependent test");

        let (output, envelope) = json_mutation(&root, &["src/math.ts", "--concurrency", "1"]);
        assert_eq!(output.code, 2, "{}", output.stderr);
        assert_eq!(
            envelope["code"], "FALLOW_MUTATION_BASELINE_FAILED",
            "{envelope}"
        );
        let message = envelope["message"].as_str().unwrap_or_default();
        assert!(message.contains("adds after prepares"), "{envelope}");
        assert!(
            message.contains("fails in a mutant run with no active mutant"),
            "{envelope}"
        );
    }

    /// A mutant of the guard throws when `src/math.ts` loads. The mutant run
    /// executes no test and reports the load error. The same tests pass with
    /// no active mutant, so the mutant stays `killed`, with no killer test.
    #[test]
    fn a_mutant_that_throws_at_module_load_stays_killed() {
        let Some(package) = companion_package() else {
            return;
        };
        let guard = "\nif (LIMIT !== 10) {\n  throw new Error(\"bad limit\");\n}\n";
        let (_project, root) = vitest_project(&package, guard);

        let (output, report) = json_mutation(&root, &["src/math.ts", "--concurrency", "1"]);
        assert_eq!(output.code, 0, "{}", output.stderr);
        let guard_line = fs::read_to_string(root.join("src/math.ts"))
            .expect("math.ts")
            .lines()
            .position(|line| line.starts_with("if (LIMIT"))
            .expect("the guard line")
            + 1;
        let guard_mutants: Vec<&serde_json::Value> = report["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|mutant| mutant["line"] == guard_line)
            .collect();
        assert!(!guard_mutants.is_empty(), "{report}");
        assert!(
            guard_mutants
                .iter()
                .any(|mutant| mutant["status"] == "killed" && mutant["killed_by"].is_null()),
            "{report}"
        );
        assert!(
            report["mutants"]
                .as_array()
                .expect("mutants")
                .iter()
                .all(|mutant| mutant["status"] != "runtime_error"),
            "{report}"
        );
    }

    /// Parameterized tests: `it.each` with distinct and with equal names,
    /// and `describe.each` with distinct and with equal suite names. Each
    /// case is its own test id. The self-check selects these ids through the
    /// mutant run path and must count each case once, so the run continues.
    #[test]
    fn parameterized_tests_pass_the_self_check_and_kill_mutants() {
        let Some(package) = companion_package() else {
            return;
        };
        let (_project, root) = vitest_project(&package, "");
        fs::write(
            root.join("test/math.test.ts"),
            "import { describe, expect, it, test } from \"vitest\";\nimport { add, isPositive } from \"../src/math\";\n\nit.each([\n  [1, 1, 2],\n  [2, 3, 5],\n])(\"adds %i and %i\", (a, b, sum) => {\n  expect(add(a, b)).toBe(sum);\n});\n\nit.each([\n  [1, 2],\n  [2, 3],\n])(\"adds one\", (a, b) => {\n  expect(add(a, 1)).toBe(b);\n});\n\ndescribe.each([1, 2])(\"sign of %i\", (n) => {\n  test(\"is positive\", () => {\n    expect(isPositive(n)).toBe(true);\n  });\n});\n\ndescribe.each([3, 4])(\"sign\", (n) => {\n  test(\"is positive\", () => {\n    expect(isPositive(n)).toBe(true);\n  });\n});\n",
        )
        .expect("test file");
        commit_all(&root, "parameterized tests");

        let (output, report) = json_mutation(&root, &["src/math.ts", "--concurrency", "1"]);
        assert_eq!(output.code, 0, "{}\n{report}", output.stderr);
        assert_eq!(
            report["baseline_failures"].as_array().map(Vec::len),
            Some(0),
            "{report}"
        );
        let mutants = report["mutants"].as_array().expect("mutants");
        for function in ["add", "isPositive"] {
            assert!(
                mutants.iter().any(|mutant| mutant["function"] == function
                    && mutant["status"] == "killed"
                    && mutant["covering_tests"]
                        .as_array()
                        .is_some_and(|tests| tests.len() >= 2)),
                "a {function} mutant is killed by its parameterized tests: {report}"
            );
        }
        assert!(
            mutants
                .iter()
                .all(|mutant| mutant["status"] != "runtime_error"),
            "{report}"
        );
    }
}

/// Issue #3783: `--resume` with a journal of other inputs exits 2, names the
/// input that differs, and keeps the journal. The header check runs after the
/// runtime discovery, so the test needs `node`.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "a skipped end-to-end test says why it did not run"
)]
fn resume_with_a_journal_of_other_inputs_exits_2_and_keeps_it() {
    let has_node = std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !has_node {
        eprintln!("skipped: the resume mismatch test needs `node` on PATH");
        return;
    }
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"resume-mismatch","private":true,"type":"module","scripts":{"test":"node --test"}}"#,
    )
    .expect("package.json");
    fs::create_dir_all(root.join("src")).expect("src");
    fs::create_dir_all(root.join("test")).expect("test");
    fs::write(
        root.join("src/math.js"),
        "export const inc = (x) => x + 1;\n",
    )
    .expect("math.js");
    fs::write(
        root.join("test/math.test.js"),
        "import assert from \"node:assert/strict\";\nimport { test } from \"node:test\";\nimport { inc } from \"../src/math.js\";\ntest(\"inc\", () => { assert.equal(inc(1), 2); });\n",
    )
    .expect("math.test.js");
    fs::write(root.join(".gitignore"), ".fallow\n").expect(".gitignore");
    let journal = root.join(".fallow/mutation/journal.jsonl");
    fs::create_dir_all(journal.parent().expect("parent")).expect("dir");
    let text = "{\"journal_format\":1,\"fallow_version\":\"0.0.1\",\"runner\":\"node-test\",\"runtime\":\"node 0.0.1\",\"run_dir\":\"0000\",\"runner_config\":null,\"scope\":\"all\",\"operators\":null,\"allow_side_effects\":false}\n{\"id\":\"m1\",\"key\":\"k1\",\"status\":\"killed\"}\n";
    fs::write(&journal, text).expect("journal");
    git(root, &["init", "-q"]);
    commit_all(root, "init");

    let (output, envelope) = json_mutation(root, &["--all", "--no-cache", "--resume"]);
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert_eq!(
        envelope["code"], "FALLOW_MUTATION_RESUME_MISMATCH",
        "{envelope}"
    );
    let message = envelope["message"].as_str().expect("message");
    assert!(
        message.contains("Fallow version (journal: 0.0.1, this run: "),
        "{message}"
    );
    assert_eq!(fs::read_to_string(&journal).expect("kept"), text);
}

/// Issue #3783: a run that SIGINT stops keeps its finished results, and
/// `--resume` continues it. The `node:test` runner needs only `node`.
#[cfg(unix)]
mod interrupt {
    use std::fs;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::{commit_all, git, json_mutation, mutation};
    use crate::common::{fallow_bin, scrub_analysis_env};

    const SOURCE: &str = "export const inc = (x) => x + 1;\nexport const dec = (x) => x - 1;\nexport const dbl = (x) => x * 2;\nexport const half = (x) => x / 2;\nexport const pos = (x) => x > 0;\nexport const neg = (x) => x < 0;\n";

    /// Each test waits, so the run lasts long enough for an interrupt.
    const TESTS: &str = "import assert from \"node:assert/strict\";\nimport { test } from \"node:test\";\nimport { inc, dec, dbl, half, pos, neg } from \"../src/math.js\";\nconst pause = () => new Promise((resolve) => setTimeout(resolve, 300));\ntest(\"inc\", async () => { await pause(); assert.equal(inc(1), 2); });\ntest(\"dec\", async () => { await pause(); assert.equal(dec(1), 0); });\ntest(\"dbl\", async () => { await pause(); assert.equal(dbl(3), 6); });\ntest(\"half\", async () => { await pause(); assert.equal(half(8), 4); });\ntest(\"pos\", async () => { await pause(); assert.equal(pos(1), true); assert.equal(pos(0), false); });\ntest(\"neg\", async () => { await pause(); assert.equal(neg(-1), true); assert.equal(neg(0), false); });\n";

    #[expect(
        clippy::print_stderr,
        reason = "a skipped end-to-end test says why it did not run"
    )]
    fn has_node() -> bool {
        let found = Command::new("node")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !found {
            eprintln!("skipped: the interrupt test needs `node` on PATH");
        }
        found
    }

    fn slow_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"resume-fixture","private":true,"type":"module","scripts":{"test":"node --test"}}"#,
        )
        .expect("package.json");
        fs::create_dir_all(root.join("src")).expect("src");
        fs::create_dir_all(root.join("test")).expect("test");
        fs::write(root.join("src/math.js"), SOURCE).expect("math.js");
        fs::write(root.join("test/math.test.js"), TESTS).expect("math.test.js");
        fs::write(root.join(".gitignore"), ".fallow\n").expect(".gitignore");
        git(root, &["init", "-q"]);
        commit_all(root, "init");
        dir
    }

    fn journal_results(journal: &Path) -> usize {
        fs::read_to_string(journal).map_or(0, |text| {
            text.lines()
                .skip(1)
                .filter(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
                .count()
        })
    }

    #[test]
    fn an_interrupted_run_keeps_its_results_and_resume_continues_it() {
        if !has_node() {
            return;
        }
        let project = slow_project();
        let root = project.path();
        let journal = root.join(".fallow/mutation/journal.jsonl");
        let temp = tempfile::tempdir().expect("temp dir");

        let mut command = Command::new(fallow_bin());
        scrub_analysis_env(&mut command);
        command
            .env("TMPDIR", temp.path())
            .args(["mutation", "--root"])
            .arg(root)
            .args([
                "--all",
                "--no-cache",
                "--concurrency",
                "1",
                "--format",
                "json",
                "--quiet",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().expect("spawn fallow mutation");
        let deadline = Instant::now() + Duration::from_secs(120);
        while journal_results(&journal) < 2 {
            assert!(
                child.try_wait().expect("poll").is_none(),
                "the run ended before it journaled two results"
            );
            assert!(Instant::now() < deadline, "no journal results in time");
            std::thread::sleep(Duration::from_millis(50));
        }
        let interrupted = Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .expect("kill");
        assert!(interrupted.success());
        let status = child.wait().expect("wait");
        assert_eq!(status.code(), Some(130), "{status:?}");
        // A signal ends Fallow without its normal cleanup, so the signal
        // handler deletes the temp directory of each companion.
        let left: Vec<_> = fs::read_dir(temp.path())
            .expect("temp dir")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(left, Vec::<std::ffi::OsString>::new());
        let kept = journal_results(&journal);
        assert!(kept >= 2, "the interrupted run keeps its results: {kept}");

        let (output, report) = json_mutation(root, &["--all", "--no-cache", "--resume"]);
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(report["completion"]["status"], "complete", "{report}");
        assert_eq!(report["resume"]["reused"], kept, "{report}");
        assert_eq!(report["resume"]["rejected"], 0, "{report}");
        let mutants = report["mutants"].as_array().expect("mutants");
        let resumed = mutants
            .iter()
            .filter(|mutant| mutant["resumed"] == true)
            .count();
        assert_eq!(resumed, kept, "{report}");
        assert!(
            mutants.iter().all(|mutant| mutant.get("cached").is_none()),
            "{report}"
        );
        assert!(
            !journal.exists(),
            "a complete run removes the journal after it saves the report"
        );

        let missing = mutation(root, &["--all", "--no-cache", "--resume", "--quiet"]);
        assert_eq!(missing.code, 0, "{}", missing.stderr);
        assert!(
            missing.stdout.contains(
                "Resumed: the run started from the beginning, because no journal exists (.fallow/mutation/journal.jsonl)."
            ),
            "{}",
            missing.stdout
        );
    }

    /// The one test writes the marker file of `DRY_RUN_MARKER`, then waits
    /// long enough for an interrupt during the dry run.
    const SLOW_DRY_RUN_TESTS: &str = "import assert from \"node:assert/strict\";\nimport { writeFileSync } from \"node:fs\";\nimport { test } from \"node:test\";\nimport { inc } from \"../src/math.js\";\ntest(\"inc\", async () => { writeFileSync(process.env.DRY_RUN_MARKER, \"\"); await new Promise((resolve) => setTimeout(resolve, 20000)); assert.equal(inc(1), 2); });\n";

    /// Issue #4302: a SIGINT during the dry run ends the run with exit code
    /// 130 and no error report. The signal kills the companion, and the run
    /// must not report that exit as a runner failure.
    #[test]
    fn an_interrupt_during_the_dry_run_prints_no_runner_error() {
        if !has_node() {
            return;
        }
        let project = slow_project();
        let root = project.path();
        fs::write(root.join("test/math.test.js"), SLOW_DRY_RUN_TESTS).expect("test file");
        commit_all(root, "slow dry run");
        let temp = tempfile::tempdir().expect("temp dir");
        let marker = temp.path().join("dry-run-started");

        let mut command = Command::new(fallow_bin());
        scrub_analysis_env(&mut command);
        command
            .env("DRY_RUN_MARKER", &marker)
            .args(["mutation", "--root"])
            .arg(root)
            .args(["--all", "--no-cache", "--format", "json", "--quiet"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn fallow mutation");
        let deadline = Instant::now() + Duration::from_secs(120);
        // The marker proves that the dry run runs the test.
        while !marker.exists() {
            if child.try_wait().expect("poll").is_some() || Instant::now() >= deadline {
                let _ = child.kill();
                let output = child.wait_with_output().expect("wait");
                panic!(
                    "the dry run did not start: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let interrupted = Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .expect("kill");
        assert!(interrupted.success());
        let output = child.wait_with_output().expect("wait");
        assert_eq!(output.status.code(), Some(130), "{:?}", output.status);
        assert_eq!(String::from_utf8_lossy(&output.stdout), "");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("Error:"), "{stderr}");
    }
}

/// Issue #4168: `--compare` lists each mutant that a weaker test no longer
/// detects. The `node:test` runner needs only `node`.
mod compare {
    use std::fs;
    use std::process::Command;

    use super::{commit_all, git, json_mutation, mutation};

    const SOURCE: &str = "export const inc = (x) => x + 1;\nexport const dec = (x) => x - 1;\n";
    const STRONG: &str = "import assert from \"node:assert/strict\";\nimport { test } from \"node:test\";\nimport { inc, dec } from \"../src/math.js\";\ntest(\"inc\", () => { assert.equal(inc(1), 2); });\ntest(\"dec\", () => { assert.equal(dec(1), 0); });\n";
    /// The `inc` test calls `inc` but asserts nothing, so it kills no mutant.
    const WEAK: &str = "import assert from \"node:assert/strict\";\nimport { test } from \"node:test\";\nimport { inc, dec } from \"../src/math.js\";\ntest(\"inc\", () => { inc(1); });\ntest(\"dec\", () => { assert.equal(dec(1), 0); });\n";

    #[expect(
        clippy::print_stderr,
        reason = "a skipped end-to-end test says why it did not run"
    )]
    fn has_node() -> bool {
        let found = Command::new("node")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !found {
            eprintln!("skipped: the compare test needs `node` on PATH");
        }
        found
    }

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"name":"compare-fixture","private":true,"type":"module","scripts":{"test":"node --test"}}"#,
        )
        .expect("package.json");
        fs::create_dir_all(root.join("src")).expect("src");
        fs::create_dir_all(root.join("test")).expect("test");
        fs::write(root.join("src/math.js"), SOURCE).expect("math.js");
        fs::write(root.join("test/math.test.js"), STRONG).expect("math.test.js");
        fs::write(root.join(".gitignore"), ".fallow\n").expect(".gitignore");
        git(root, &["init", "-q"]);
        commit_all(root, "init");
        dir
    }

    #[test]
    fn compare_lists_the_mutants_that_a_weaker_test_no_longer_kills() {
        if !has_node() {
            return;
        }
        let project = project();
        let root = project.path();
        let saved_dir = tempfile::tempdir().expect("saved dir");
        let saved = saved_dir.path().join("before.json");

        let (output, before) = json_mutation(root, &["--all", "--no-cache"]);
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(before["completion"]["status"], "complete", "{before}");
        let killed_in_inc = before["mutants"]
            .as_array()
            .expect("mutants")
            .iter()
            .filter(|mutant| mutant["status"] == "killed" && mutant["line"] == 1)
            .count();
        assert!(
            killed_in_inc > 0,
            "the strong test kills a mutant: {before}"
        );
        fs::write(&saved, &output.stdout).expect("save");

        fs::write(root.join("test/math.test.js"), WEAK).expect("weaken");
        commit_all(root, "weaken");
        let saved_arg = saved.to_str().expect("utf-8 path");
        let (output, after) = json_mutation(root, &["--all", "--no-cache", "--compare", saved_arg]);
        assert_eq!(output.code, 0, "{}", output.stderr);
        let compare = &after["compare"];
        assert_eq!(compare["regressed"], killed_in_inc, "{after}");
        assert_eq!(compare["added"], 0, "{after}");
        assert_eq!(compare["removed"], 0, "{after}");
        let first = &compare["mutants"][0];
        assert_eq!(first["change"], "regressed", "{after}");
        assert_eq!(first["previous_status"], "killed", "{after}");
        assert_eq!(first["status"], "survived", "{after}");
        let killer = first["previous_killing_tests"][0].as_str().expect("killer");
        assert!(killer.contains("inc"), "{after}");

        let human = mutation(
            root,
            &["--all", "--no-cache", "--quiet", "--compare", saved_arg],
        );
        assert_eq!(human.code, 0, "{}", human.stderr);
        assert!(
            human.stdout.contains("Compare with the saved report"),
            "{}",
            human.stdout
        );
        assert!(
            human.stdout.contains(&format!("killed before by {killer}")),
            "{}",
            human.stdout
        );

        let (output, partial_run) = json_mutation(
            root,
            &[
                "--all",
                "--no-cache",
                "--max-mutants",
                "1",
                "--compare",
                saved_arg,
            ],
        );
        assert_eq!(output.code, 2, "{}", output.stderr);
        assert_eq!(
            partial_run["code"], "FALLOW_MUTATION_COMPARE_INCOMPLETE",
            "{partial_run}"
        );

        let mut partial: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&saved).expect("read")).expect("json");
        partial["completion"]["status"] = "partial".into();
        fs::write(&saved, partial.to_string()).expect("partial");
        let (output, error) = json_mutation(root, &["--all", "--no-cache", "--compare", saved_arg]);
        assert_eq!(output.code, 2, "{}", output.stderr);
        assert_eq!(
            error["code"], "FALLOW_MUTATION_COMPARE_INCOMPLETE",
            "{error}"
        );

        let conflict = mutation(root, &["--plan", "--compare", saved_arg]);
        assert_eq!(conflict.code, 2, "{}", conflict.stderr);
    }
}

#[path = "mutation_copy_scope_tests.rs"]
mod copy_scope;

/// A `node:test` project for `inspect --replay`. `add` is exported, `hidden`
/// is not, and `useHidden` calls it. The replay needs only `node`.
fn replay_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"replay-fixture","private":true,"type":"module","scripts":{"test":"node --test"}}"#,
    )
    .expect("package.json");
    fs::create_dir_all(root.join("src")).expect("src");
    fs::create_dir_all(root.join("test")).expect("test");
    fs::write(
        root.join("src/math.js"),
        "export const add = (a, b) => a + b;\nconst hidden = (n) => n * 2;\nexport const useHidden = (n) => hidden(n);\n",
    )
    .expect("math.js");
    fs::write(
        root.join("test/math.test.js"),
        "import assert from \"node:assert/strict\";\nimport { test } from \"node:test\";\nimport { add, useHidden } from \"../src/math.js\";\ntest(\"add\", () => { add(1, 2); useHidden(1); assert.ok(true); });\n",
    )
    .expect("math.test.js");
    fs::write(root.join(".gitignore"), ".fallow\nplan.json\n").expect(".gitignore");
    git(root, &["init", "-q"]);
    commit_all(root, "init");
    dir
}

/// The plan of the replay project and the id of the mutant that turns
/// `original` into `replacement`.
fn replay_plan(root: &Path, original: &str, replacement: &str) -> (String, String) {
    let plan = root.join("plan.json");
    let plan_path = plan.to_str().expect("utf-8 path").to_owned();
    let output = mutation(
        root,
        &[
            "--plan", "--all", "--format", "json", "--quiet", "-o", &plan_path,
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&plan).expect("plan")).expect("plan json");
    let mutant = saved["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| {
            mutant["change"]["from"] == original && mutant["change"]["to"] == replacement
        })
        .unwrap_or_else(|| panic!("no {original} to {replacement} mutant: {saved}"));
    (
        mutant["mutant_id"].as_str().expect("id").to_owned(),
        plan_path,
    )
}

#[test]
fn inspect_replay_shows_a_witness_for_an_exported_function() {
    let project = replay_project();
    let root = project.path();
    let (id, plan) = replay_plan(root, "+", "-");
    let (output, packet) = json_mutation(
        root,
        &[
            "inspect", &id, "--report", &plan, "--replay", "--args", "[2, 3]",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let replay = &packet["replay"];
    assert_eq!(replay["verdict"], "witness", "{packet}");
    assert_eq!(replay["export"], "add");
    assert_eq!(replay["input"]["args"], serde_json::json!([2, 3]));
    assert_eq!(replay["contract_version"], 1);
    let values = |side: &str| -> Vec<serde_json::Value> {
        replay["calls"]
            .as_array()
            .expect("calls")
            .iter()
            .filter(|call| call["side"] == side)
            .map(|call| call["value"].clone())
            .collect()
    };
    assert_eq!(values("original"), vec![serde_json::json!(5); 3]);
    assert_eq!(values("mutant"), vec![serde_json::json!(-1); 3]);

    let human = mutation(
        root,
        &[
            "inspect", &id, "--report", &plan, "--replay", "--args", "[2, 3]",
        ],
    );
    assert_eq!(human.code, 0, "{}", human.stderr);
    assert!(
        human.stdout.contains("Replay  add(2, 3)"),
        "{}",
        human.stdout
    );
    assert!(human.stdout.contains("Witness"), "{}", human.stdout);
}

#[test]
fn inspect_replay_of_a_function_without_export_names_the_export_flag() {
    let project = replay_project();
    let root = project.path();
    let (id, plan) = replay_plan(root, "*", "/");
    let (output, packet) = json_mutation(
        root,
        &[
            "inspect", &id, "--report", &plan, "--replay", "--args", "[3]",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(packet["replay"]["verdict"], "not_replayable", "{packet}");
    let reason = packet["replay"]["reason"].as_str().expect("reason");
    assert!(
        reason.contains("`hidden`") && reason.contains("--export"),
        "{reason}"
    );

    let (output, packet) = json_mutation(
        root,
        &[
            "inspect",
            &id,
            "--report",
            &plan,
            "--replay",
            "--args",
            "[3]",
            "--export",
            "useHidden",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(packet["replay"]["verdict"], "witness", "{packet}");
}

#[test]
fn inspect_replay_refuses_bad_args_a_stale_file_and_args_without_replay() {
    let project = replay_project();
    let root = project.path();
    let (id, plan) = replay_plan(root, "+", "-");
    let (output, envelope) = json_mutation(
        root,
        &[
            "inspect",
            &id,
            "--report",
            &plan,
            "--replay",
            "--args",
            "{\"a\":1}",
        ],
    );
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert_eq!(
        envelope["code"], "FALLOW_INVALID_MUTATION_OPTIONS",
        "{envelope}"
    );

    let without = mutation(root, &["inspect", &id, "--report", &plan, "--args", "[1]"]);
    assert_eq!(without.code, 2, "{}", without.stderr);

    for limit in ["0s", "11m", "soon"] {
        let (output, envelope) = json_mutation(
            root,
            &[
                "inspect",
                &id,
                "--report",
                &plan,
                "--replay",
                "--call-timeout",
                limit,
            ],
        );
        assert_eq!(output.code, 2, "{limit}: {}", output.stderr);
        assert_eq!(
            envelope["code"], "FALLOW_INVALID_MUTATION_OPTIONS",
            "{limit}: {envelope}"
        );
    }

    fs::write(
        root.join("src/math.js"),
        "export const add = (a, b) => b + a;\n",
    )
    .expect("edit");
    let (output, envelope) = json_mutation(
        root,
        &[
            "inspect", &id, "--report", &plan, "--replay", "--args", "[1, 2]",
        ],
    );
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert_eq!(
        envelope["code"], "FALLOW_MUTATION_REPLAY_STALE",
        "{envelope}"
    );
}

/// A `node:test` project whose test calls `greet` with a marker string and
/// `apply` with a callback, which is not replayable.
fn recording_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"record-fixture","private":true,"type":"module","scripts":{"test":"node --test"}}"#,
    )
    .expect("package.json");
    fs::create_dir_all(root.join("src")).expect("src");
    fs::create_dir_all(root.join("test")).expect("test");
    fs::write(
        root.join("src/greet.js"),
        "export function greet(name) {\n  return name.length > 3 ? `hello ${name}` : \"hi\";\n}\nexport const apply = (fn, n) => fn(n) + 1;\n",
    )
    .expect("greet.js");
    fs::write(
        root.join("test/greet.test.js"),
        "import assert from \"node:assert/strict\";\nimport { test } from \"node:test\";\nimport { apply, greet } from \"../src/greet.js\";\ntest(\"greet\", () => { assert.ok(greet(\"zq-marker\").length > 0); });\ntest(\"apply\", () => { assert.ok(apply((n) => n, 1) > 0); });\ntest(\"source\", () => { assert.match(greet.toString(), /name\\.length/); });\ntest(\"name\", () => { assert.equal(apply.name, \"apply\"); });\n",
    )
    .expect("greet.test.js");
    fs::write(root.join(".gitignore"), ".fallow\n").expect(".gitignore");
    git(root, &["init", "-q"]);
    commit_all(root, "init");
    dir
}

#[test]
fn a_recording_run_feeds_the_replay_and_keeps_values_out_of_the_report() {
    let project = recording_project();
    let root = project.path();
    let (output, report) = json_mutation(root, &["--all", "--no-cache", "--record-args", "values"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        !output.stdout.contains("zq-marker"),
        "the report holds a recorded value: {}",
        output.stdout
    );
    let recording = &report["recording"];
    assert_eq!(recording["mode"], "values", "{report}");
    assert_eq!(recording["saved"], true);
    let functions = recording["functions"].as_array().expect("functions");
    let greet = functions
        .iter()
        .find(|function| function["export"] == "greet")
        .expect("greet");
    assert_eq!(greet["input_count"], 1);
    let apply = functions
        .iter()
        .find(|function| function["export"] == "apply")
        .expect("apply");
    assert_eq!(apply["input_count"], 0);
    assert_eq!(apply["not_replayable"][0]["reason"], "a function");
    // Only the record pass sees the wraps, so a test of a function name or of
    // the source text is no baseline failure of the run.
    assert_eq!(
        report["baseline_failures"],
        serde_json::json!([]),
        "{}",
        report["baseline_failures"]
    );
    let saved =
        fs::read_to_string(root.join(".fallow/mutation/recorded-args.json")).expect("store");
    assert!(saved.contains("zq-marker"), "{saved}");

    let survivor = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["function"] == "greet" && mutant["status"] == "survived")
        .unwrap_or_else(|| panic!("a greet survivor: {report}"));
    let id = survivor["mutant_id"].as_str().expect("id");
    let (output, packet) = json_mutation(root, &["inspect", id, "--replay"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let replay = &packet["replay"];
    assert_eq!(replay["input"]["source"], "recorded", "{packet}");
    assert_eq!(replay["input"]["args"], serde_json::json!(["zq-marker"]));
    assert_eq!(replay["inputs_checked"], 1);
    assert_ne!(replay["verdict"], "equivalent");

    let (output, packet) = json_mutation(
        root,
        &["inspect", &first_mutant_of(&report, "apply"), "--replay"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(packet["replay"]["verdict"], "not_replayable", "{packet}");
    let reason = packet["replay"]["reason"].as_str().expect("reason");
    assert!(reason.contains("a function (1)"), "{reason}");
}

fn first_mutant_of(report: &serde_json::Value, function: &str) -> String {
    report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["function"] == function)
        .and_then(|mutant| mutant["mutant_id"].as_str())
        .unwrap_or_else(|| panic!("a mutant of {function}"))
        .to_owned()
}

#[test]
fn a_run_without_recording_has_no_recording_and_writes_no_store() {
    let project = recording_project();
    let root = project.path();
    let (output, report) = json_mutation(root, &["--all", "--no-cache"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(report.get("recording").is_none(), "{report}");
    assert!(!root.join(".fallow/mutation/recorded-args.json").exists());
    let id = first_mutant_of(&report, "greet");
    let (output, packet) = json_mutation(root, &["inspect", &id, "--replay"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(packet["replay"]["verdict"], "not_replayable");
    let reason = packet["replay"]["reason"].as_str().expect("reason");
    assert!(reason.contains("--record-args values"), "{reason}");
}

/// A `node:test` project whose test calls `shout` but asserts nothing, so a
/// mutant of `shout` survives and its recorded input is a witness.
fn witness_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"witness-fixture","private":true,"type":"module","scripts":{"test":"node --test"}}"#,
    )
    .expect("package.json");
    fs::create_dir_all(root.join("src")).expect("src");
    fs::create_dir_all(root.join("test")).expect("test");
    fs::write(
        root.join("src/shout.js"),
        "export function shout(text) {\n  return text.toUpperCase() + \"!\";\n}\n",
    )
    .expect("shout.js");
    fs::write(
        root.join("test/shout.test.js"),
        "import { test } from \"node:test\";\nimport { shout } from \"../src/shout.js\";\ntest(\"shout\", () => { shout(\"qv-marker\"); });\n",
    )
    .expect("shout.test.js");
    fs::write(root.join(".gitignore"), ".fallow\n").expect(".gitignore");
    git(root, &["init", "-q"]);
    commit_all(root, "init");
    dir
}

#[test]
fn a_recording_run_gives_survivors_a_witness_and_tasks_give_the_ready_input() {
    let project = witness_project();
    let root = project.path();
    let (output, report) = json_mutation(root, &["--all", "--no-cache", "--record-args", "values"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        !output.stdout.contains("qv-marker") && !output.stdout.contains("QV-MARKER"),
        "the report holds a recorded value: {}",
        output.stdout
    );
    let survivor = report["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| mutant["status"] == "survived" && mutant["change"]["to"] == "\"\"")
        .unwrap_or_else(|| panic!("a string-literal survivor: {report}"));
    let witness = &survivor["witness"];
    assert_eq!(witness["verdict"], "witness", "{survivor}");
    assert_eq!(witness["inputs_checked"], 1);
    assert!(witness["input_id"].is_string(), "{witness}");
    assert_ne!(witness["verdict"], "equivalent");

    let (output, tasks) = json_mutation(root, &["tasks"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let id = survivor["mutant_id"].as_str().expect("id");
    let task = tasks["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["mutant_id"] == id)
        .expect("the task");
    let witness = &task["witness"];
    assert_eq!(witness["verdict"], "witness", "{task}");
    assert_eq!(witness["args"], serde_json::json!(["qv-marker"]));
    assert_eq!(witness["original"]["outcome"], "returned");
    assert_eq!(witness["original"]["value"], "QV-MARKER!");
    assert_eq!(witness["mutant"]["value"], "QV-MARKER");
    assert_eq!(
        witness["replay_command"],
        format!("fallow mutation inspect {id} --replay")
    );

    let markdown = mutation(root, &["tasks", "--format", "markdown"]);
    assert_eq!(markdown.code, 0, "{}", markdown.stderr);
    assert!(markdown.stdout.contains("witness"), "{}", markdown.stdout);
    assert!(
        !markdown.stdout.contains("qv-marker"),
        "markdown can go into a PR comment, so it holds no value: {}",
        markdown.stdout
    );
    let human = mutation(root, &["tasks"]);
    assert!(human.stdout.contains("\"qv-marker\""), "{}", human.stdout);
}

#[test]
fn tasks_without_the_witness_cache_keep_the_reference_and_the_command() {
    let project = witness_project();
    let root = project.path();
    let (output, _) = json_mutation(root, &["--all", "--no-cache", "--record-args", "values"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    fs::remove_file(root.join(".fallow/mutation/witnesses.json")).expect("remove");
    let (output, tasks) = json_mutation(root, &["tasks"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let witness = tasks["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find_map(|task| {
            task.get("witness")
                .filter(|witness| witness["verdict"] == "witness")
        })
        .expect("a witness reference");
    assert!(witness.get("args").is_none(), "{witness}");
    assert!(
        witness["replay_command"]
            .as_str()
            .expect("command")
            .contains("--replay")
    );
}

#[test]
fn a_stale_task_keeps_the_witness_reference_but_shows_no_value() {
    let project = witness_project();
    let root = project.path();
    let (output, _) = json_mutation(root, &["--all", "--no-cache", "--record-args", "values"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    fs::write(
        root.join("src/shout.js"),
        "export function shout(text) {\n  return text.toUpperCase() + \"?\";\n}\n",
    )
    .expect("edit");
    let (output, tasks) = json_mutation(root, &["tasks"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    for task in tasks["tasks"].as_array().expect("tasks") {
        assert!(task["stale_reason"].is_string(), "{task}");
        if let Some(witness) = task.get("witness") {
            assert!(
                witness.get("args").is_none(),
                "a stale task shows a value: {witness}"
            );
            assert!(witness["replay_command"].is_string());
        }
    }
}

#[test]
fn a_file_that_a_witness_replay_writes_shows_as_a_changed_worktree() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"stamp-fixture","private":true,"type":"module","scripts":{"test":"node --test"}}"#,
    )
    .expect("package.json");
    fs::create_dir_all(root.join("src")).expect("src");
    fs::create_dir_all(root.join("test")).expect("test");
    // The replay clock starts at 2026-01-01, so only a replay writes the
    // file. The write is in an unmutated helper, so no mutant run writes it.
    fs::create_dir_all(root.join("lib")).expect("lib");
    fs::write(
        root.join("lib/note.js"),
        "import { writeFileSync } from \"node:fs\";\nexport function note(tag) {\n  const now = new Date();\n  if (now.getFullYear() === 2026 && now.getMonth() === 0 && now.getDate() === 1) writeFileSync(\"replay-stamp.txt\", tag);\n}\n",
    )
    .expect("note.js");
    fs::write(
        root.join("src/stamp.js"),
        "import { note } from \"../lib/note.js\";\nexport function stamp(tag) {\n  note(tag);\n  return tag + \"!\";\n}\n",
    )
    .expect("stamp.js");
    fs::write(
        root.join("test/stamp.test.js"),
        "import { test } from \"node:test\";\nimport { stamp } from \"../src/stamp.js\";\ntest(\"stamp\", () => { stamp(\"a\"); });\n",
    )
    .expect("stamp.test.js");
    fs::write(root.join(".gitignore"), ".fallow\n").expect(".gitignore");
    git(root, &["init", "-q"]);
    commit_all(root, "init");
    let (output, report) = json_mutation(
        root,
        &[
            "src/stamp.js",
            "--no-cache",
            "--allow-side-effects",
            "--record-args",
            "values",
        ],
    );
    assert!(output.code == 0 || output.code == 2, "{}", output.stderr);
    assert!(
        root.join("replay-stamp.txt").exists(),
        "the replay ran no call"
    );
    let changed = report["worktree"]["changed_paths"].to_string();
    assert!(changed.contains("replay-stamp.txt"), "{report}");
}
