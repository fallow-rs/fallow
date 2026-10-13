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
    assert_eq!(report.summary.score, Some(0.0));
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
            crate::mutation::discovery::find_on_path(&path)
                .filter(|(kind, _)| *kind == crate::mutation::discovery::RuntimeKind::Node)
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

    #[test]
    fn the_embedded_companion_runs_vitest_against_the_fixture_end_to_end() {
        let Some(package) = companion_fixture() else {
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
        commit_all(&root);
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
}
