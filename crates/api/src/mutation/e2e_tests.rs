use std::fs;
use std::process::Command;

use fallow_engine::changed_files::clear_ambient_git_env;
use std::path::Path;
use std::time::Duration;

use crate::mutation::{MutationRunOptions, MutationRunScope};
use crate::{AnalysisOptions, plan_mutation, run_mutation};

fn git(root: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    command.args(args).current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().expect("git runs");
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

fn commit_all(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
}

fn options(root: &Path, scope: MutationRunScope) -> MutationRunOptions {
    MutationRunOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        scope,
        concurrency: Some(1),
        ..MutationRunOptions::default()
    }
}

fn small_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("dir");
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"p","type":"module","main":"src/a.ts"}"#,
    )
    .expect("write");
    fs::create_dir_all(dir.path().join("src")).expect("dir");
    fs::write(
        dir.path().join("src/a.ts"),
        "export function add(a: number, b: number) { return a + b; }\n",
    )
    .expect("write");
    dir
}

#[test]
fn a_dirty_worktree_is_refused_before_the_plan_unless_allowed() {
    let project = small_project();
    commit_all(project.path());
    fs::write(
        project.path().join("src/a.ts"),
        "export const changed = 1;\n",
    )
    .expect("write");
    let error = run_mutation(&options(project.path(), MutationRunScope::All)).unwrap_err();
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_DIRTY_WORKTREE")
    );
    assert!(error.message.contains("src/a.ts"), "{}", error.message);

    let mut allowed = options(project.path(), MutationRunScope::All);
    allowed.allow_dirty = true;
    let report = run_mutation(&allowed).expect("an allowed dirty run");
    assert!(report.worktree.checked);
    assert_eq!(report.worktree.dirty_at_start, vec!["src/a.ts"]);
    assert!(report.worktree.changed_paths.is_empty());
}

#[test]
fn a_root_outside_git_is_refused_unless_allowed() {
    let project = small_project();
    let error = run_mutation(&options(project.path(), MutationRunScope::All)).unwrap_err();
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_NOT_A_REPOSITORY")
    );
}

#[test]
fn a_project_without_tests_reports_no_coverage_without_a_companion() {
    let project = small_project();
    commit_all(project.path());
    let report = run_mutation(&options(project.path(), MutationRunScope::All)).expect("run");
    assert!(report.no_test_files);
    assert!(
        report.runner.is_none(),
        "no companion starts without planned mutants"
    );
    assert!(report.summary.counts.total > 0);
    assert_eq!(
        report.summary.counts.no_coverage,
        report.summary.counts.total
    );
    assert_eq!(
        report.summary.score, None,
        "no mutant ran, so the score has no value"
    );
    assert_eq!(report.summary.coverage, Some(0.0));
}

/// A `node:test` project with a Vue component. The test does not import the
/// component, because Node cannot load it.
fn component_project(runner: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("dir");
    let write = |path: &str, text: &str| {
        let target = dir.path().join(path);
        fs::create_dir_all(target.parent().expect("parent")).expect("dir");
        fs::write(target, text).expect("write");
    };
    write(
        "package.json",
        r#"{"name":"p","type":"module","scripts":{"test":"node --test"}}"#,
    );
    write(
        ".fallowrc.json",
        &format!(r#"{{"mutation":{{"runner":"{runner}"}}}}"#),
    );
    write(
        "src/math.ts",
        "export function add(a: number, b: number) { return a + b; }\n",
    );
    write(
        "src/Badge.vue",
        "<template><span>{{ label(1) }}</span></template>\n<script>\nexport function label(n) {\n  return n === 1 ? \"one\" : \"many\";\n}\n</script>\n",
    );
    write(
        "test/math.test.ts",
        "import assert from \"node:assert/strict\";\nimport { test } from \"node:test\";\nimport { add } from \"../src/math.ts\";\n\ntest(\"add\", () => {\n  assert.equal(add(1, 2), 3);\n});\n",
    );
    dir
}

#[test]
fn a_runner_that_cannot_load_components_leaves_them_out_of_the_plan() {
    for runner in ["node-test", "bun-test", "auto"] {
        let project = component_project(runner);
        let plan = plan_mutation(&options(project.path(), MutationRunScope::All)).expect("plan");
        assert!(
            plan.mutants
                .iter()
                .all(|mutant| mutant.location.file != "src/Badge.vue"),
            "{runner}: {:#?}",
            plan.mutants
        );
        assert!(
            plan.mutants
                .iter()
                .any(|mutant| mutant.location.file == "src/math.ts"),
            "{runner}"
        );
        assert_eq!(plan.skipped_files.len(), 1, "{runner}");
        assert_eq!(plan.skipped_files[0].path, "src/Badge.vue");
        assert_eq!(plan.skipped_files[0].reason, "runner-cannot-load");
    }

    let project = component_project("node-test");
    let only_component = plan_mutation(&options(
        project.path(),
        MutationRunScope::Paths(vec![std::path::PathBuf::from("src/Badge.vue")]),
    ))
    .expect("plan");
    assert!(only_component.mutants.is_empty());
    assert!(only_component.unmatched_paths.is_empty());
    assert_eq!(only_component.skipped_files.len(), 1);
    assert_eq!(only_component.skipped_files[0].reason, "runner-cannot-load");

    let project = component_project("vitest");
    let plan = plan_mutation(&options(project.path(), MutationRunScope::All)).expect("plan");
    assert!(
        plan.mutants
            .iter()
            .any(|mutant| mutant.location.file == "src/Badge.vue"),
        "Vitest loads a component through the framework plugin"
    );
    assert!(plan.skipped_files.is_empty());
}

/// A type test (`tsd`, Vitest `*.test-d.ts`) is checked by a compiler, not
/// run by a test runner. It must not count as a test file of the plan.
#[test]
fn a_type_test_is_not_a_test_file_of_the_plan() {
    let dir = tempfile::tempdir().expect("dir");
    let write = |path: &str, text: &str| {
        let target = dir.path().join(path);
        fs::create_dir_all(target.parent().expect("parent")).expect("dir");
        fs::write(target, text).expect("write");
    };
    write(
        "package.json",
        r#"{"name":"p","main":"index.ts","devDependencies":{"tsd":"0.31.0"}}"#,
    );
    write(
        "index.ts",
        "export const add = (a: number, b: number) => a + b;\n",
    );
    write(
        "test/types/index.test-d.ts",
        "import { expectType } from \"tsd\";\nimport { add } from \"../../index\";\nexpectType<number>(add(1, 2));\n",
    );
    let plan = plan_mutation(&options(dir.path(), MutationRunScope::All)).expect("plan");
    assert!(plan.test_files.is_empty(), "{:?}", plan.test_files);
    assert!(plan.no_test_files);
    assert_eq!(plan.counts.planned, 0, "no runner test covers a mutant");
    assert_eq!(plan.counts.no_coverage, plan.counts.total);
    assert!(
        plan.mutants.iter().all(|mutant| mutant
            .test_reach
            .as_ref()
            .is_some_and(|reach| reach.reach == fallow_output::TestReachClass::NoneFile)),
        "a type test is not a test that loads the file: {:#?}",
        plan.mutants
    );
    assert!(
        plan.files
            .iter()
            .all(|file| file.static_test_files.is_empty()),
        "{:?}",
        plan.files
    );
}

#[test]
fn the_plan_uses_the_config_operators_unless_the_caller_names_operators() {
    let project = small_project();
    fs::write(
        project.path().join(".fallowrc.json"),
        r#"{"mutation":{"operators":["return-value"]}}"#,
    )
    .expect("config");
    let operators = |options: &MutationRunOptions| -> Vec<String> {
        let mut names: Vec<String> = plan_mutation(options)
            .expect("plan")
            .mutants
            .into_iter()
            .map(|mutant| mutant.location.operator)
            .collect();
        names.sort();
        names.dedup();
        names
    };
    let from_config = options(project.path(), MutationRunScope::All);
    assert_eq!(operators(&from_config), vec!["return-value"]);

    let mut explicit = options(project.path(), MutationRunScope::All);
    explicit.operators = Some(vec![crate::mutation::Operator::ArithmeticOperator]);
    assert_eq!(operators(&explicit), vec!["arithmetic-operator"]);
}

#[test]
fn an_output_path_of_the_caller_does_not_make_the_worktree_dirty() {
    let project = small_project();
    commit_all(project.path());
    let report_path = project.path().join("reports/mutation.json");
    fs::create_dir_all(report_path.parent().unwrap()).expect("dir");
    fs::write(&report_path, "").expect("write");

    let error = run_mutation(&options(project.path(), MutationRunScope::All)).unwrap_err();
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_DIRTY_WORKTREE")
    );
    assert!(
        error.message.contains("1 uncommitted path:"),
        "{}",
        error.message
    );

    let mut with_output = options(project.path(), MutationRunScope::All);
    with_output.output_paths = vec![report_path];
    let report = run_mutation(&with_output).expect("the output path is not dirty");
    assert!(report.worktree.dirty_at_start.is_empty());
}

#[test]
fn the_last_report_is_saved_in_the_ignored_cache() {
    let project = small_project();
    commit_all(project.path());
    let options = options(project.path(), MutationRunScope::All);
    let report = run_mutation(&options).expect("run");
    let value = fallow_output::serialize_mutation_json_output(report).expect("json");

    let path =
        crate::mutation::save_last_mutation_report(&options.analysis, &value).expect("saved");

    assert_eq!(
        path,
        crate::mutation::last_mutation_report_path(&options.analysis).expect("path")
    );
    assert!(
        path.ends_with(".fallow/mutation/last-report.json"),
        "{}",
        path.display()
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("read")).expect("json");
    assert_eq!(saved["kind"], "mutation");
    let status = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(project.path())
        .output()
        .expect("git status");
    assert_eq!(
        String::from_utf8_lossy(&status.stdout),
        "",
        "the cache is ignored"
    );
}

#[test]
fn a_cache_dir_that_hides_project_files_is_refused() {
    for (dir, shown) in [(".", "the project root"), ("src", "src/a.ts")] {
        let project = small_project();
        fs::write(
            project.path().join(".fallowrc.json"),
            format!(r#"{{"cache":{{"dir":"{dir}"}}}}"#),
        )
        .expect("config");
        // A `node --test` file makes `src/a.ts` a planned file. The refusal
        // comes before the run looks for a runtime.
        fs::write(
            project.path().join("package.json"),
            r#"{"name":"p","type":"module","main":"src/a.ts","scripts":{"test":"node --test"}}"#,
        )
        .expect("write");
        fs::create_dir_all(project.path().join("test")).expect("dir");
        fs::write(
            project.path().join("test/a.test.ts"),
            "import { test } from \"node:test\";\nimport { add } from \"../src/a.ts\";\n\ntest(\"add\", () => {\n  if (add(1, 2) !== 3) throw new Error(\"add\");\n});\n",
        )
        .expect("write");
        commit_all(project.path());
        let error = run_mutation(&options(project.path(), MutationRunScope::All)).unwrap_err();
        assert_eq!(
            error.code.as_deref(),
            Some("FALLOW_MUTATION_UNSAFE_CACHE_DIR"),
            "{dir}: {}",
            error.message
        );
        assert_eq!(error.exit_code, 2);
        assert!(error.message.contains(shown), "{dir}: {}", error.message);
    }
}

#[test]
fn invalid_limits_are_refused() {
    let project = small_project();
    let mut zero = options(project.path(), MutationRunScope::All);
    zero.concurrency = Some(0);
    assert_eq!(
        run_mutation(&zero).unwrap_err().code.as_deref(),
        Some("FALLOW_INVALID_MUTATION_OPTIONS")
    );
    let mut budget = options(project.path(), MutationRunScope::All);
    budget.time_budget = Some(Duration::ZERO);
    assert_eq!(
        plan_mutation(&budget).unwrap_err().code.as_deref(),
        Some("FALLOW_INVALID_MUTATION_OPTIONS")
    );
}

/// The end-to-end run needs a symlink to `node_modules`, so it runs on Unix
/// only. Its helper lives in the same module, so no item is dead on Windows.
#[cfg(unix)]
mod end_to_end {
    use std::fs;
    use std::path::{Path, PathBuf};

    use fallow_output::{MutantStatus, MutationCompletionStatus};

    use super::{commit_all, options};
    use crate::mutation::MutationRunScope;
    use crate::run_mutation;

    /// The companion package and its Vitest fixture, when `npm ci` ran in
    /// `tools/mutation-runner`.
    #[expect(
        clippy::print_stderr,
        reason = "a skipped end-to-end test says why it did not run"
    )]
    fn companion_fixture() -> Option<PathBuf> {
        let package =
            Path::new(&std::env::var_os("CARGO_MANIFEST_DIR")?).join("../../tools/mutation-runner");
        let vitest = package.join("node_modules/vitest/package.json");
        let node = std::env::var_os("PATH").and_then(|path| {
            crate::mutation::discovery::find_kinds_on_path(
                &path,
                &[crate::mutation::discovery::RuntimeKind::Node],
            )
        });
        if !vitest.is_file() || node.is_none() {
            eprintln!(
                "skipped: the end-to-end mutation test needs `node` on PATH and `npm ci` in {}",
                package.display()
            );
            return None;
        }
        Some(dunce::canonicalize(package).expect("package path"))
    }

    /// A committed copy of the Vitest fixture of the companion package.
    fn vitest_project(package: &Path) -> (tempfile::TempDir, PathBuf) {
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
        commit_all(&root);
        (project, root)
    }

    #[test]
    fn a_second_run_reuses_the_results_until_a_key_file_changes() {
        let Some(package) = companion_fixture() else {
            return;
        };
        let (_project, root) = vitest_project(&package);
        let mut cached = options(
            &root,
            MutationRunScope::Paths(vec![PathBuf::from("src/math.ts")]),
        );
        cached.analysis.no_cache = false;

        let first = run_mutation(&cached).expect("the first run completes");
        let cache = first.cache.as_ref().expect("the cache is on");
        assert_eq!(cache.hits, 0);
        assert!(cache.stored > 0, "{cache:#?}");
        assert_eq!(cache.not_keyed, vec!["dynamic-imports", "file-reads"]);
        assert!(first.mutants.iter().all(|mutant| !mutant.cached));

        let second = run_mutation(&cached).expect("the second run completes");
        assert_eq!(
            second.cache.as_ref().expect("the cache is on").hits,
            cache.stored,
            "each stored result is used"
        );
        assert_eq!(second.cache.as_ref().expect("cache").stored, 0);
        let statuses = |report: &fallow_output::MutationReport| {
            report
                .mutants
                .iter()
                .map(|mutant| (mutant.location.mutant_id.clone(), mutant.status))
                .collect::<Vec<_>>()
        };
        assert_eq!(statuses(&first), statuses(&second));
        assert_eq!(first.summary.score, second.summary.score);
        assert!(
            second
                .mutants
                .iter()
                .filter(|mutant| matches!(
                    mutant.status,
                    MutantStatus::Killed | MutantStatus::Survived
                ))
                .all(|mutant| mutant.cached)
        );

        fs::write(
            root.join("test/user-setup.ts"),
            format!(
                "{}// changed\n",
                fs::read_to_string(root.join("test/user-setup.ts")).expect("read")
            ),
        )
        .expect("write");
        commit_all(&root);
        let third = run_mutation(&cached).expect("the third run completes");
        assert_eq!(
            third.cache.as_ref().expect("cache").hits,
            0,
            "a change to the runner setup file runs each mutant again"
        );

        let mut uncached = cached;
        uncached.analysis.no_cache = true;
        let fourth = run_mutation(&uncached).expect("the run without a cache completes");
        assert!(fourth.cache.is_none(), "--no-cache turns the cache off");
        assert!(fourth.mutants.iter().all(|mutant| !mutant.cached));
    }

    #[test]
    fn the_embedded_companion_runs_vitest_against_the_fixture_end_to_end() {
        let Some(package) = companion_fixture() else {
            return;
        };
        let (_project, root) = vitest_project(&package);
        let before = fs::read_to_string(root.join("src/math.ts")).expect("read");

        let report = run_mutation(&options(
            &root,
            MutationRunScope::Paths(vec![PathBuf::from("src/math.ts")]),
        ))
        .expect("the run completes");

        let runner = report.runner.as_ref().expect("a runner ran");
        assert_eq!(runner.name, "vitest");
        assert_eq!(runner.version, "5.0.4");
        assert_eq!(runner.runtime.name, "node");
        assert_eq!(
            report.completion.status,
            MutationCompletionStatus::Complete,
            "{report:#?}"
        );
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(report.worktree.checked);
        assert!(report.worktree.changed_paths.is_empty());
        assert_eq!(
            fs::read_to_string(root.join("src/math.ts")).expect("read"),
            before
        );

        let in_function = |name: &str| {
            report
                .mutants
                .iter()
                .filter(|mutant| mutant.location.function.as_deref() == Some(name))
                .collect::<Vec<_>>()
        };
        let add = in_function("add");
        assert!(!add.is_empty());
        for mutant in &add {
            assert_eq!(mutant.status, MutantStatus::Killed, "{mutant:#?}");
            assert!(
                mutant
                    .killed_by
                    .as_deref()
                    .is_some_and(|test| test.starts_with("test/math.test.ts::")),
                "{mutant:#?}"
            );
        }
        let positive = in_function("isPositive");
        assert!(
            positive
                .iter()
                .any(|mutant| mutant.status == MutantStatus::Survived
                    && mutant.covering_tests
                        == vec!["test/math.test.ts::positive without an assertion".to_owned()]),
            "a test with no assertion lets a mutant of isPositive survive: {positive:#?}"
        );
        let limit = report
            .mutants
            .iter()
            .find(|mutant| mutant.location.function.is_none() && mutant.static_mutant)
            .expect("the LIMIT mutant runs while the module loads");
        assert_eq!(limit.status, MutantStatus::Killed, "{limit:#?}");
        assert_eq!(limit.killed_by.as_deref(), Some("test/math.test.ts::limit"));
        assert!(
            report
                .functions
                .iter()
                .any(|function| function.name.as_deref() == Some("isPositive")
                    && !function.survived.is_empty())
        );
        assert!(report.timings.dry_run_ms > 0);
    }

    /// The companion package, when a runtime of `kind` is on `PATH` and
    /// reports a version that the manifest range of `runner` contains.
    #[expect(
        clippy::print_stderr,
        reason = "a skipped end-to-end test says why it did not run"
    )]
    fn child_runner_package(
        runner: &str,
        kind: crate::mutation::discovery::RuntimeKind,
    ) -> Option<PathBuf> {
        let package =
            Path::new(&std::env::var_os("CARGO_MANIFEST_DIR")?).join("../../tools/mutation-runner");
        let runtime = crate::mutation::discovery::discover_runtime(None, &[kind]).ok();
        let range = crate::mutation::manifest::manifest()
            .ok()
            .and_then(|manifest| manifest.runner(runner));
        let supported = runtime
            .as_ref()
            .zip(range)
            .is_some_and(|(runtime, range)| range.contains(&runtime.version));
        if !supported {
            eprintln!(
                "skipped: the end-to-end {runner} test needs a supported `{}` on PATH",
                kind.name()
            );
            return None;
        }
        Some(dunce::canonicalize(package).expect("package path"))
    }

    /// Copies `files` of a companion fixture to a new committed repository.
    fn fixture_repository(fixture: &Path, files: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let project = tempfile::tempdir().expect("dir");
        let root = dunce::canonicalize(project.path()).expect("root");
        for file in files {
            let target = root.join(file);
            fs::create_dir_all(target.parent().expect("parent")).expect("dir");
            fs::copy(fixture.join(file), &target).expect("copy");
        }
        commit_all(&root);
        (project, root)
    }

    /// Checks the statuses of the math fixture: the tests kill each mutant
    /// of `add` and the `LIMIT` mutant, and a test with no assertion lets a
    /// mutant of `isPositive` survive.
    fn assert_math_statuses(report: &fallow_output::MutationReport) {
        assert_eq!(
            report.completion.status,
            MutationCompletionStatus::Complete,
            "{report:#?}"
        );
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert!(report.worktree.changed_paths.is_empty());
        let in_function = |name: &str| {
            report
                .mutants
                .iter()
                .filter(|mutant| mutant.location.function.as_deref() == Some(name))
                .collect::<Vec<_>>()
        };
        let add = in_function("add");
        assert!(!add.is_empty());
        for mutant in &add {
            assert_eq!(mutant.status, MutantStatus::Killed, "{mutant:#?}");
            assert!(
                mutant
                    .killed_by
                    .as_deref()
                    .is_some_and(|test| test.starts_with("test/math.test.ts::add > ")),
                "{mutant:#?}"
            );
        }
        let positive = in_function("isPositive");
        assert!(
            positive
                .iter()
                .any(|mutant| mutant.status == MutantStatus::Survived
                    && mutant.covering_tests
                        == vec!["test/math.test.ts::positive without an assertion".to_owned()]),
            "a test with no assertion lets a mutant of isPositive survive: {positive:#?}"
        );
        let limit = report
            .mutants
            .iter()
            .find(|mutant| mutant.location.function.is_none() && mutant.static_mutant)
            .expect("the LIMIT mutant runs while the module loads");
        assert_eq!(limit.status, MutantStatus::Killed, "{limit:#?}");
        assert_eq!(limit.killed_by.as_deref(), Some("test/math.test.ts::limit"));
    }

    #[test]
    fn the_embedded_companion_runs_node_test_against_the_fixture_end_to_end() {
        use crate::mutation::discovery::RuntimeKind;
        let Some(package) = child_runner_package("node-test", RuntimeKind::Node) else {
            return;
        };
        let (_project, root) = fixture_repository(
            &package.join("test/fixtures/node-basic"),
            &["package.json", "src/math.ts", "test/math.test.ts"],
        );

        let report = run_mutation(&options(
            &root,
            MutationRunScope::Paths(vec![PathBuf::from("src/math.ts")]),
        ))
        .expect("the run completes");

        let runner = report.runner.as_ref().expect("a runner ran");
        assert_eq!(runner.name, "node-test");
        assert_eq!(runner.runtime.name, "node");
        assert_math_statuses(&report);
    }

    /// The acceptance case of #3470: a `node:test` run gives no
    /// `no_coverage` mutant for a Vue file, and the report names the skip.
    #[test]
    fn a_node_test_run_leaves_a_component_out_and_says_why() {
        use crate::mutation::discovery::RuntimeKind;
        if child_runner_package("node-test", RuntimeKind::Node).is_none() {
            return;
        }
        let project = super::component_project("auto");
        commit_all(project.path());

        let report = run_mutation(&options(project.path(), MutationRunScope::All))
            .expect("the run completes");

        assert_eq!(
            report.runner.as_ref().expect("a runner ran").name,
            "node-test"
        );
        assert!(
            report
                .mutants
                .iter()
                .all(|mutant| mutant.location.file != "src/Badge.vue"),
            "{:#?}",
            report.mutants
        );
        assert_eq!(
            report.summary.counts.no_coverage, 0,
            "{:#?}",
            report.mutants
        );
        assert_eq!(report.skipped_files.len(), 1);
        assert_eq!(report.skipped_files[0].reason, "runner-cannot-load");
        // The component survivors are unknown, so an empty survivor list of
        // this run is not conclusive.
        assert_eq!(report.completion.status, MutationCompletionStatus::Partial);
        assert!(
            report
                .completion
                .reasons
                .contains(&fallow_output::MutationIncompleteReason::SkippedFiles)
        );
    }

    #[test]
    fn a_test_that_writes_to_the_root_is_reported_as_a_worktree_change() {
        use crate::mutation::discovery::RuntimeKind;
        let Some(package) = child_runner_package("node-test", RuntimeKind::Node) else {
            return;
        };
        let (_project, root) = fixture_repository(
            &package.join("test/fixtures/node-basic"),
            &["package.json", "src/math.ts", "test/math.test.ts"],
        );
        fs::write(
            root.join("test/touch.test.ts"),
            "import assert from \"node:assert/strict\";\n\
             import { writeFileSync } from \"node:fs\";\n\
             import { test } from \"node:test\";\n\n\
             import { add } from \"../src/math.ts\";\n\n\
             test(\"touch\", () => {\n  writeFileSync(\"touched.txt\", \"x\");\n  assert.equal(add(1, 2), 3);\n});\n",
        )
        .expect("write");
        commit_all(&root);

        let report = run_mutation(&options(
            &root,
            MutationRunScope::Paths(vec![PathBuf::from("src/math.ts")]),
        ))
        .expect("the run completes");

        assert!(report.worktree.checked);
        assert_eq!(report.worktree.changed_paths, vec!["touched.txt"]);
        assert!(
            report
                .errors
                .iter()
                .any(|issue| issue.code == "worktree-changed"),
            "{:#?}",
            report.errors
        );
        assert_eq!(report.completion.status, MutationCompletionStatus::Partial);
        assert!(
            report
                .completion
                .reasons
                .contains(&fallow_output::MutationIncompleteReason::WorktreeChanged)
        );
    }

    #[test]
    fn the_embedded_companion_runs_bun_test_against_the_fixture_end_to_end() {
        use crate::mutation::discovery::RuntimeKind;
        let Some(package) = child_runner_package("bun-test", RuntimeKind::Bun) else {
            return;
        };
        let (_project, root) = fixture_repository(
            &package.join("test/fixtures/bun-basic"),
            &[
                "package.json",
                "bunfig.toml",
                "src/math.ts",
                "test/math.test.ts",
            ],
        );

        let report = run_mutation(&options(
            &root,
            MutationRunScope::Paths(vec![PathBuf::from("src/math.ts")]),
        ))
        .expect("the run completes");

        let runner = report.runner.as_ref().expect("a runner ran");
        assert_eq!(runner.name, "bun-test");
        assert_eq!(runner.runtime.name, "bun");
        assert_math_statuses(&report);
    }
}
