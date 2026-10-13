use std::path::Path;
use std::time::Duration;

use fallow_api::MutationRunScope;
use fallow_config::{MutationConfig, MutationOperatorName, OutputFormat};
use fallow_output::{
    MutantLocation, MutantResult, MutantStatus, MutationBaselineFailure, MutationCompletion,
    MutationFunctionResult, MutationIncompleteReason, MutationPlanCountsOutput,
    MutationPlanDisposition, MutationPlanFile, MutationPlanMutant, MutationPlanOutput,
    MutationPlanSchemaVersion, MutationReport, MutationSchemaVersion, MutationScopeKind,
    MutationScopeOutput, MutationStatusCounts, MutationSummary, MutationTimings,
    MutationWorktreeCheck,
};
use fallow_types::envelope::{ElapsedMs, ToolVersion};

use super::*;
use crate::mutation_human::{CodeLookup, RunReportOptions, render_plan, render_run};

fn input(root: &Path, args: MutationArgs, config: MutationConfig) -> MutationCliInput<'_> {
    MutationCliInput {
        root,
        config_path: None,
        root_flag: None,
        allow_remote_extends: false,
        no_cache: false,
        threads: 1,
        changed_since: None,
        diff_file: None,
        quiet: true,
        output: OutputFormat::Json,
        json_style: JsonStyle::default(),
        config,
        args,
        subcommand: None,
    }
}

#[test]
fn durations_take_a_unit_and_reject_zero_and_garbage() {
    let cases: [(&str, Option<Duration>); 9] = [
        ("90", Some(Duration::from_secs(90))),
        ("90s", Some(Duration::from_secs(90))),
        ("1500ms", Some(Duration::from_millis(1500))),
        ("10m", Some(Duration::from_secs(600))),
        ("2h", Some(Duration::from_secs(7200))),
        ("0s", None),
        ("", None),
        ("5d", None),
        ("m5", None),
    ];
    for (text, expected) in cases {
        assert_eq!(parse_duration(text), expected, "{text}");
    }
}

#[test]
fn flags_win_over_config_and_config_fills_the_gaps() {
    let root = Path::new("/project");
    let config = MutationConfig {
        operators: Some(vec![MutationOperatorName::RemovedCall]),
        concurrency: Some(3),
        min_score: Some(60.0),
        ..MutationConfig::default()
    };
    let from_config = run_options(&input(root, MutationArgs::default(), config.clone())).unwrap();
    assert_eq!(from_config.operators, Some(vec![Operator::RemovedCall]));
    assert_eq!(from_config.concurrency, Some(3));
    assert_eq!(from_config.scope, MutationRunScope::Changed { base: None });
    assert!(!from_config.allow_side_effects);

    let args = MutationArgs {
        operators: vec!["arithmetic-operator".to_owned(), "return-value".to_owned()],
        concurrency: Some(1),
        min_score: Some(90.0),
        all: true,
        allow_side_effects: true,
        time_budget: Some("2m".to_owned()),
        ..MutationArgs::default()
    };
    let flagged = input(root, args, config);
    let from_flags = run_options(&flagged).unwrap();
    assert_eq!(
        from_flags.operators,
        Some(vec![Operator::ArithmeticOperator, Operator::ReturnValue])
    );
    assert_eq!(from_flags.concurrency, Some(1));
    assert_eq!(from_flags.scope, MutationRunScope::All);
    assert!(from_flags.allow_side_effects);
    assert_eq!(from_flags.time_budget, Some(Duration::from_secs(120)));
    assert_eq!(min_score(&flagged).unwrap(), Some(90.0));
}

#[test]
fn invalid_flags_and_scope_conflicts_exit_2() {
    let root = Path::new("/project");
    let cases: [(MutationArgs, Option<&str>, &str); 6] = [
        (
            MutationArgs {
                operators: vec!["bogus".to_owned()],
                ..MutationArgs::default()
            },
            None,
            "unknown mutation operator `bogus`",
        ),
        (
            MutationArgs {
                time_budget: Some("soon".to_owned()),
                ..MutationArgs::default()
            },
            None,
            "not a duration",
        ),
        (
            MutationArgs {
                concurrency: Some(0),
                ..MutationArgs::default()
            },
            None,
            "--concurrency must be at least 1",
        ),
        (
            MutationArgs {
                max_mutants: Some(0),
                ..MutationArgs::default()
            },
            None,
            "--max-mutants must be at least 1",
        ),
        (
            MutationArgs {
                all: true,
                ..MutationArgs::default()
            },
            Some("main"),
            "cannot be combined with --changed-since",
        ),
        (
            MutationArgs {
                paths: vec!["src".into()],
                ..MutationArgs::default()
            },
            Some("main"),
            "cannot be combined with --changed-since",
        ),
    ];
    for (args, changed_since, needle) in cases {
        let mut cli = input(root, args, MutationConfig::default());
        cli.changed_since = changed_since;
        let error = run_options(&cli).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert!(error.message.contains(needle), "{}", error.message);
    }
    let out_of_range = input(
        root,
        MutationArgs {
            min_score: Some(101.0),
            ..MutationArgs::default()
        },
        MutationConfig::default(),
    );
    assert!(
        min_score(&out_of_range)
            .unwrap_err()
            .message
            .contains("between 0 and 100")
    );
}

fn mutant(id: &str, file: &str, rank: u32, status: MutantStatus) -> MutantResult {
    MutantResult {
        location: MutantLocation {
            mutant_id: id.to_owned(),
            file: file.to_owned(),
            line: rank + 1,
            column: 10,
            span: (rank * 10, rank * 10 + 5),
            operator: "arithmetic-operator".to_owned(),
            function: Some("add".to_owned()),
            function_line: Some(1),
            rank: Some(rank),
        },
        status,
        prune_reason: (status == MutantStatus::Pruned).then(|| "side-effect-sink".to_owned()),
        killed_by: (status == MutantStatus::Killed).then(|| "test/a.test.ts::adds".to_owned()),
        covering_tests: if status == MutantStatus::Survived {
            vec![
                "test/a.test.ts::adds".to_owned(),
                "test/a.test.ts::subtracts".to_owned(),
            ]
        } else {
            Vec::new()
        },
        static_mutant: false,
        duration_ms: Some(3),
    }
}

fn report(mutants: Vec<MutantResult>, reasons: Vec<MutationIncompleteReason>) -> MutationReport {
    let mut counts = MutationStatusCounts::default();
    for mutant in &mutants {
        counts.add(mutant.status);
    }
    MutationReport {
        schema_version: MutationSchemaVersion::V1,
        version: ToolVersion("0.0.0".to_owned()),
        elapsed_ms: ElapsedMs(0),
        completion: MutationCompletion::from_reasons(reasons),
        scope: MutationScopeOutput {
            kind: MutationScopeKind::Changed,
            base_ref: Some("origin/main".to_owned()),
        },
        runner: None,
        summary: MutationSummary {
            counts,
            score: counts.score(),
        },
        functions: Vec::<MutationFunctionResult>::new(),
        mutants,
        no_test_files: false,
        baseline_failures: Vec::new(),
        skipped_files: Vec::new(),
        unmatched_paths: Vec::new(),
        unloaded_paths: Vec::new(),
        worktree: MutationWorktreeCheck {
            checked: true,
            allow_dirty: false,
            dirty_at_start: Vec::new(),
            changed_paths: Vec::new(),
        },
        errors: Vec::new(),
        timings: MutationTimings::default(),
        files: Vec::new(),
        verdicts: None,
    }
}

#[test]
fn exit_code_is_1_only_when_a_minimum_is_set_and_missed() {
    let half = report(
        vec![
            mutant("m1", "src/a.ts", 1, MutantStatus::Killed),
            mutant("m2", "src/a.ts", 2, MutantStatus::Survived),
        ],
        Vec::new(),
    );
    assert_eq!(run_exit_code(&half, None), 0);
    assert_eq!(run_exit_code(&half, Some(50.0)), 0);
    assert_eq!(run_exit_code(&half, Some(50.1)), 1);
    let unscored = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Pruned)],
        Vec::new(),
    );
    assert_eq!(run_exit_code(&unscored, Some(100.0)), 0);
}

#[test]
fn human_run_report_groups_survivors_by_file_in_rank_order() {
    let report = report(
        vec![
            mutant("m1", "src/b.ts", 1, MutantStatus::Killed),
            mutant("m2", "src/b.ts", 4, MutantStatus::Survived),
            mutant("m3", "src/a.ts", 3, MutantStatus::Survived),
            mutant("m4", "src/b.ts", 2, MutantStatus::Survived),
            mutant("m5", "src/c.ts", 5, MutantStatus::NoCoverage),
            mutant("m6", "src/c.ts", 6, MutantStatus::Pruned),
        ],
        vec![MutationIncompleteReason::TimeBudget],
    );
    let mut code = CodeLookup::default();
    code.insert(
        "m4".to_owned(),
        ("return a + b;".to_owned(), "return a - b;".to_owned()),
    );
    code.insert("m5".to_owned(), ("log(x);".to_owned(), String::new()));
    let text = render_run(
        &report,
        &RunReportOptions {
            min_score: Some(50.0),
            code: &code,
            save_command: SAVE,
        },
    );
    insta::assert_snapshot!(text);
}

#[test]
fn human_run_report_has_one_line_for_no_tests_and_no_data() {
    let mut no_tests = report(
        vec![
            mutant("m1", "src/a.ts", 1, MutantStatus::NoCoverage),
            mutant("m2", "src/a.ts", 2, MutantStatus::NoCoverage),
            mutant("m3", "src/b.ts", 3, MutantStatus::NoCoverage),
        ],
        Vec::new(),
    );
    no_tests.no_test_files = true;
    let options = RunReportOptions {
        min_score: None,
        code: &CodeLookup::default(),
        save_command: SAVE,
    };
    let text = render_run(&no_tests, &options);
    assert!(
        text.contains(
            "No tests: the project has no test files, so the 3 mutants have no coverage."
        ),
        "{text}"
    );
    assert!(!text.contains("No coverage ("), "{text}");
    assert!(!text.contains("src/a.ts"), "{text}");
    assert!(!text.contains("No mutation data"), "{text}");
    assert!(!text.contains("Survived"), "{text}");

    let mut one = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::NoCoverage)],
        Vec::new(),
    );
    one.no_test_files = true;
    let text = render_run(&one, &options);
    assert!(text.contains("so the 1 mutant has no coverage."), "{text}");

    let mut all_pruned = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Pruned)],
        Vec::new(),
    );
    all_pruned.no_test_files = true;
    let text = render_run(&all_pruned, &options);
    assert!(
        text.contains("No tests: the project has no test files, so no mutant can be killed.\n"),
        "{text}"
    );

    let no_data = report(Vec::new(), Vec::new());
    let text = render_run(&no_data, &options);
    assert!(
        text.contains("No mutation data: the changed code against origin/main has no mutant."),
        "{text}"
    );
    assert!(text.contains("Score n/a"), "{text}");
}

#[test]
fn human_summary_names_pending_equivalent_errors_and_flaky_only_when_not_zero() {
    let options = RunReportOptions {
        min_score: None,
        code: &CodeLookup::default(),
        save_command: SAVE,
    };
    let mut mutants = vec![
        mutant("m1", "src/a.ts", 1, MutantStatus::Killed),
        mutant("m2", "src/a.ts", 2, MutantStatus::Survived),
    ];
    for (index, status) in [
        MutantStatus::Pending,
        MutantStatus::Pending,
        MutantStatus::Equivalent,
        MutantStatus::RuntimeError,
        MutantStatus::Flaky,
    ]
    .into_iter()
    .enumerate()
    {
        mutants.push(mutant(&format!("x{index}"), "src/a.ts", 10, status));
    }
    let partial = report(mutants, vec![MutationIncompleteReason::MutantLimit]);
    let text = render_run(&partial, &options);
    let summary = text.lines().nth(2).unwrap_or_default();
    assert_eq!(
        summary,
        "Score 50.0%  killed 1  survived 1  no coverage 0  timeout 0  equivalent 1  runtime error 1  flaky 1  partial (mutant-limit, 2 pending)",
        "{text}"
    );

    let complete = report(
        vec![
            mutant("m1", "src/a.ts", 1, MutantStatus::Killed),
            mutant("m2", "src/a.ts", 2, MutantStatus::Survived),
        ],
        Vec::new(),
    );
    let text = render_run(&complete, &options);
    assert_eq!(
        text.lines().nth(2).unwrap_or_default(),
        "Score 50.0%  killed 1  survived 1  no coverage 0  timeout 0  complete",
        "{text}"
    );
}

const SAVE: &str = "fallow mutation --format json --quiet > mutation.json";

#[test]
fn human_next_step_saves_the_report_first_and_says_that_the_save_runs_the_tests() {
    let options = RunReportOptions {
        min_score: None,
        code: &CodeLookup::default(),
        save_command: "fallow mutation --all --verdicts v.json --format json --quiet > mutation.json",
    };
    let changed = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Survived)],
        Vec::new(),
    );
    let text = render_run(&changed, &options);
    let next = text.split("\nNext\n").nth(1).unwrap_or_default();
    assert_eq!(
        next,
        "  Save a JSON report (runs the tests again): fallow mutation --all --verdicts v.json --format json --quiet > mutation.json\n  \
         Inspect a survivor (runs no code): fallow mutation inspect m1 --report mutation.json\n",
        "{text}"
    );
}

#[test]
fn the_save_command_repeats_the_scope_and_the_flags_of_the_run() {
    let root = Path::new("/project");
    let changed_scope = MutationScopeOutput {
        kind: MutationScopeKind::Changed,
        base_ref: Some("origin/main".to_owned()),
    };
    let detected = input(root, MutationArgs::default(), MutationConfig::default());
    assert_eq!(
        save_report_command(&detected, &changed_scope),
        "fallow mutation --changed-since origin/main --format json --quiet > mutation.json"
    );

    let mut explicit = input(root, MutationArgs::default(), MutationConfig::default());
    explicit.changed_since = Some("v1.2.0");
    assert_eq!(
        save_report_command(&explicit, &changed_scope),
        "fallow mutation --changed-since v1.2.0 --format json --quiet > mutation.json"
    );

    let mut diff = input(root, MutationArgs::default(), MutationConfig::default());
    diff.diff_file = Some(Path::new("pr.diff"));
    let diff_scope = MutationScopeOutput {
        kind: MutationScopeKind::Changed,
        base_ref: None,
    };
    assert_eq!(
        save_report_command(&diff, &diff_scope),
        "fallow mutation --diff-file pr.diff --format json --quiet > mutation.json"
    );

    let args = MutationArgs {
        paths: vec!["src/math.ts".into(), "src/my dir".into()],
        operators: vec!["arithmetic-operator".to_owned(), "return-value".to_owned()],
        max_mutants: Some(40),
        time_budget: Some("10m".to_owned()),
        allow_side_effects: true,
        allow_dirty: true,
        runtime: Some("/usr/bin/node".into()),
        verdicts: Some("../v.json".into()),
        ..MutationArgs::default()
    };
    let mut flagged = input(root, args, MutationConfig::default());
    flagged.config_path = Some(Path::new("ci/.fallowrc.json"));
    flagged.root_flag = Some(Path::new("packages/app"));
    let paths_scope = MutationScopeOutput {
        kind: MutationScopeKind::Paths,
        base_ref: None,
    };
    assert_eq!(
        save_report_command(&flagged, &paths_scope),
        "fallow mutation --root packages/app --config ci/.fallowrc.json src/math.ts 'src/my dir' \
         --operators arithmetic-operator,return-value --max-mutants 40 --time-budget 10m \
         --allow-side-effects --allow-dirty --runtime /usr/bin/node --verdicts ../v.json \
         --format json --quiet > mutation.json"
    );

    let all = input(
        root,
        MutationArgs {
            all: true,
            ..MutationArgs::default()
        },
        MutationConfig::default(),
    );
    let all_scope = MutationScopeOutput {
        kind: MutationScopeKind::All,
        base_ref: None,
    };
    assert_eq!(
        save_report_command(&all, &all_scope),
        "fallow mutation --all --format json --quiet > mutation.json"
    );
}

#[test]
fn the_header_says_that_tests_ran_only_when_a_mutant_ran() {
    let options = RunReportOptions {
        min_score: None,
        code: &CodeLookup::default(),
        save_command: SAVE,
    };
    let ran = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Killed)],
        Vec::new(),
    );
    assert!(render_run(&ran, &options).starts_with("Mutation testing (ran the project tests)\n"));

    let mut no_tests = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::NoCoverage)],
        Vec::new(),
    );
    no_tests.no_test_files = true;
    let text = render_run(&no_tests, &options);
    assert!(
        text.starts_with("Mutation testing (no test ran)\n"),
        "{text}"
    );

    let empty = report(Vec::new(), Vec::new());
    let text = render_run(&empty, &options);
    insta::assert_snapshot!(text);
}

#[test]
fn an_empty_scope_of_paths_or_all_has_no_widen_hint() {
    let options = RunReportOptions {
        min_score: None,
        code: &CodeLookup::default(),
        save_command: SAVE,
    };
    let mut empty = report(Vec::new(), Vec::new());
    empty.scope = MutationScopeOutput {
        kind: MutationScopeKind::All,
        base_ref: None,
    };
    let text = render_run(&empty, &options);
    assert!(
        text.contains("No mutation data: all source files have no mutant.\n"),
        "{text}"
    );
    assert!(!text.contains("--all"), "{text}");
}

#[test]
fn human_counts_use_singular_and_plural_nouns() {
    let options = RunReportOptions {
        min_score: None,
        code: &CodeLookup::default(),
        save_command: SAVE,
    };
    let mut one = report(Vec::new(), Vec::new());
    let failure = |id: &str| MutationBaselineFailure {
        file: Some("test/a.test.ts".to_owned()),
        test_id: Some(id.to_owned()),
        message: None,
    };
    one.baseline_failures = vec![failure("adds")];
    let text = render_run(&one, &options);
    assert!(
        text.contains("Baseline failures: 1 test failed with"),
        "{text}"
    );
    let mut two = one;
    two.baseline_failures.push(failure("subtracts"));
    let text = render_run(&two, &options);
    assert!(
        text.contains("Baseline failures: 2 tests failed with"),
        "{text}"
    );
}

#[test]
fn human_plan_lists_planned_mutants_and_prune_reasons() {
    let planned = |id: &str, rank: u32, disposition, reason: Option<&str>| MutationPlanMutant {
        location: mutant(id, "src/a.ts", rank, MutantStatus::Pending).location,
        disposition,
        prune_reason: reason.map(str::to_owned),
    };
    let plan = MutationPlanOutput {
        schema_version: MutationPlanSchemaVersion::V1,
        version: ToolVersion("0.0.0".to_owned()),
        elapsed_ms: ElapsedMs(0),
        scope: MutationScopeOutput {
            kind: MutationScopeKind::All,
            base_ref: None,
        },
        counts: MutationPlanCountsOutput {
            total: 4,
            planned: 2,
            no_coverage: 0,
            pending: 0,
            pruned_side_effect_sink: 1,
            pruned_generated: 1,
            ..MutationPlanCountsOutput::default()
        },
        mutants: vec![
            planned("m1", 1, MutationPlanDisposition::Planned, None),
            planned("m2", 2, MutationPlanDisposition::Planned, None),
            planned(
                "m3",
                3,
                MutationPlanDisposition::Pruned,
                Some("side-effect-sink"),
            ),
            planned("m4", 4, MutationPlanDisposition::Pruned, Some("generated")),
        ],
        files: vec![MutationPlanFile {
            path: "src/a.ts".to_owned(),
            source_sha256: "ab".repeat(32),
            static_test_files: vec!["test/a.test.ts".to_owned()],
        }],
        test_files: vec!["test/a.test.ts".to_owned()],
        no_test_files: false,
        framework_config_checked: true,
        skipped_files: Vec::new(),
        unmatched_paths: Vec::new(),
    };
    insta::assert_snapshot!(render_plan(&plan));
}

#[test]
fn every_config_operator_name_parses_as_an_engine_operator() {
    for operator in Operator::ALL {
        let config: MutationConfig = serde_json::from_value(serde_json::json!({
            "operators": [operator.as_str()]
        }))
        .unwrap();
        let parsed = parse_operators(&[], &config).unwrap();
        assert_eq!(parsed, Some(vec![operator]));
    }
}
