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
