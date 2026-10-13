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
fn min_score_fails_with_exit_1_only_when_set_and_missed() {
    let project = untested_project();
    let (plain, value) = json_mutation(project.path(), &["--all"]);
    assert_eq!(plain.code, 0, "{}", plain.stderr);
    assert_eq!(value["summary"]["score"], 0.0);
    assert!(value["summary"]["counts"]["no_coverage"].as_u64().unwrap() > 0);
    assert_eq!(value["no_test_files"], true);

    let (missed, _) = json_mutation(project.path(), &["--all", "--min-score", "50"]);
    assert_eq!(missed.code, 1, "{}", missed.stderr);

    let (met, _) = json_mutation(project.path(), &["--all", "--min-score", "0"]);
    assert_eq!(met.code, 0, "{}", met.stderr);

    fs::write(
        project.path().join(".fallowrc.json"),
        r#"{"mutation":{"minScore":40}}"#,
    )
    .expect("config");
    commit_all(project.path(), "config");
    let (from_config, _) = json_mutation(project.path(), &["--all"]);
    assert_eq!(from_config.code, 1, "{}", from_config.stderr);
    let (flag_wins, _) = json_mutation(project.path(), &["--all", "--min-score", "0"]);
    assert_eq!(flag_wins.code, 0, "{}", flag_wins.stderr);

    let invalid = mutation(project.path(), &["--all", "--min-score", "120", "--quiet"]);
    assert_eq!(invalid.code, 2, "{}", invalid.stderr);
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

    #[test]
    fn a_vitest_run_kills_add_mutants_and_reports_a_survivor_of_is_positive() {
        let Some(package) = companion_package() else {
            return;
        };
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
        git(&root, &["init", "-q"]);
        commit_all(&root, "init");

        let (output, report) = json_mutation(&root, &["src/math.ts", "--concurrency", "1"]);
        assert_eq!(output.code, 0, "{}", output.stderr);
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
        assert!(
            human.stdout.contains("fallow mutation inspect "),
            "{}",
            human.stdout
        );
    }
}
