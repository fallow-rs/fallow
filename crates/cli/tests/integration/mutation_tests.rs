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
}
