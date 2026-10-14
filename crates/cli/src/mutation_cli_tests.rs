use std::path::Path;
use std::time::Duration;

use fallow_api::MutationRunScope;
use fallow_config::{MutationConfig, MutationOperatorName, OutputFormat};
use fallow_output::{
    MutantLocation, MutantResult, MutantStatus, MutantTestReach, MutationBaselineFailure,
    MutationCompletion, MutationFunctionResult, MutationIncompleteReason, MutationPlanCountsOutput,
    MutationPlanDisposition, MutationPlanFile, MutationPlanMutant, MutationPlanOutput,
    MutationPlanSchemaVersion, MutationReport, MutationSchemaVersion, MutationScopeKind,
    MutationScopeOutput, MutationStatusCounts, MutationSummary, MutationTimings,
    MutationWorktreeCheck, TestReachClass,
};
use fallow_types::envelope::{ElapsedMs, ToolVersion};

use super::*;
use crate::mutation_human::{
    CodeExcerpt, CodeLookup, RunReportOptions, render_inspect, render_plan, render_run,
};

/// The human output without color codes, so a snapshot shows the text.
fn plain(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        output.push(ch);
    }
    output
}

fn excerpt(before: &str, original: &str, replacement: &str, after: &str) -> CodeExcerpt {
    CodeExcerpt {
        before: before.to_owned(),
        original: original.to_owned(),
        replacement: replacement.to_owned(),
        after: after.to_owned(),
    }
}

/// Renders a run report as plain text.
fn run_text(report: &MutationReport, options: &RunReportOptions<'_>) -> String {
    plain(&render_run(report, options))
}

const OPTIONS: RunReportOptions<'static> = RunReportOptions {
    min_score: None,
    code: None,
    inspect_flags: Some(""),
};

fn input(root: &Path, args: MutationArgs, config: MutationConfig) -> MutationCliInput<'_> {
    MutationCliInput {
        root,
        config_path: None,
        root_flag: None,
        output_file: None,
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
    assert_eq!(
        from_config.operators, None,
        "the API plan applies the config operators, so CLI and MCP share one rule"
    );
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
        setup_timeout: Some("5m".to_owned()),
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
    assert_eq!(from_flags.runner_timeout, Some(Duration::from_secs(300)));
    assert_eq!(min_score(&flagged).unwrap(), Some(90.0));
}

#[test]
fn invalid_flags_and_scope_conflicts_exit_2() {
    let root = Path::new("/project");
    let cases: [(MutationArgs, Option<&str>, &str); 7] = [
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
                setup_timeout: Some("later".to_owned()),
                ..MutationArgs::default()
            },
            None,
            "--setup-timeout `later` is not a duration",
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
        covering_tests: if matches!(status, MutantStatus::Survived | MutantStatus::Killed) {
            vec![
                "test/a.test.ts::adds".to_owned(),
                "test/a.test.ts::subtracts".to_owned(),
            ]
        } else {
            Vec::new()
        },
        static_mutant: false,
        duration_ms: Some(3),
        test_reach: None,
        retried: false,
        cached: false,
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
        cache: None,
        summary: MutationSummary::from_results(&mutants),
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
    code.insert("m4".to_owned(), excerpt("return a ", "+", "-", " b;"));
    code.insert("m5".to_owned(), excerpt("", "log(x);", "", ""));
    let text = run_text(
        &report,
        &RunReportOptions {
            min_score: Some(50.0),
            code: Some(&code),
            inspect_flags: Some(""),
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
    let text = run_text(&no_tests, &OPTIONS);
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
    let text = run_text(&one, &OPTIONS);
    assert!(text.contains("so the 1 mutant has no coverage."), "{text}");

    let mut all_pruned = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Pruned)],
        Vec::new(),
    );
    all_pruned.no_test_files = true;
    let text = run_text(&all_pruned, &OPTIONS);
    assert!(
        text.contains("No tests: the project has no test files, so no mutant can be killed.\n"),
        "{text}"
    );

    let no_data = report(Vec::new(), Vec::new());
    let text = run_text(&no_data, &OPTIONS);
    assert!(
        text.contains(
            "No mutation data: the changed code has no mutant. `fallow mutation --plan --all` lists the mutants of each file and runs no code.\n"
        ),
        "{text}"
    );
    assert!(
        !text.contains("Use --all"),
        "no step runs the tests: {text}"
    );
    assert!(text.contains("Score n/a"), "{text}");
}

#[test]
fn the_score_line_counts_executed_mutants_and_the_coverage_line_counts_unreached_ones() {
    let mut mutants = vec![
        mutant("m1", "src/a.ts", 1, MutantStatus::Killed),
        mutant("m2", "src/a.ts", 2, MutantStatus::Survived),
    ];
    for index in 0..6 {
        mutants.push(mutant(
            &format!("n{index}"),
            "src/b.ts",
            10,
            MutantStatus::NoCoverage,
        ));
    }
    let text = run_text(&report(mutants, Vec::new()), &OPTIONS);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[2], "Score 50.0%  killed 1  survived 1  timeout 0  complete",
        "{text}"
    );
    assert_eq!(lines[3], "Coverage 25.0%  no coverage 6", "{text}");
}

#[test]
fn human_summary_names_pending_equivalent_errors_and_flaky_only_when_not_zero() {
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
    let text = run_text(&partial, &OPTIONS);
    assert_eq!(
        text.lines().nth(2).unwrap_or_default(),
        "Score 50.0%  killed 1  survived 1  timeout 0  pending 2  equivalent 1  runtime error 1  flaky 1  partial (mutant-limit)",
        "{text}"
    );

    let complete = report(
        vec![
            mutant("m1", "src/a.ts", 1, MutantStatus::Killed),
            mutant("m2", "src/a.ts", 2, MutantStatus::Survived),
        ],
        Vec::new(),
    );
    let text = run_text(&complete, &OPTIONS);
    assert_eq!(
        text.lines().nth(2).unwrap_or_default(),
        "Score 50.0%  killed 1  survived 1  timeout 0  complete",
        "{text}"
    );
}

#[test]
fn the_only_next_step_is_a_read_only_inspect_of_the_saved_report() {
    let survivor = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Survived)],
        Vec::new(),
    );
    let text = run_text(
        &survivor,
        &RunReportOptions {
            min_score: None,
            code: None,
            inspect_flags: Some(" --root packages/app"),
        },
    );
    let next = text.split("\nNext\n").nth(1).unwrap_or_default();
    assert_eq!(
        next,
        "  Inspect a survivor (runs no code): fallow mutation inspect m1 --root packages/app\n",
        "{text}"
    );
    assert!(!text.contains("runs the tests again"), "{text}");
    assert!(!text.contains('>'), "no shell redirect: {text}");

    let unsaved = run_text(
        &survivor,
        &RunReportOptions {
            min_score: None,
            code: None,
            inspect_flags: None,
        },
    );
    assert!(
        !unsaved.contains("\nNext\n"),
        "no inspect step without a saved report: {unsaved}"
    );
}

#[test]
fn the_inspect_flags_repeat_the_root_and_the_config_of_the_run() {
    let root = Path::new("/project");
    let plain_input = input(root, MutationArgs::default(), MutationConfig::default());
    assert_eq!(inspect_flags(&plain_input), "");
    let mut flagged = input(root, MutationArgs::default(), MutationConfig::default());
    flagged.root_flag = Some(Path::new("packages/my app"));
    flagged.config_path = Some(Path::new("ci/.fallowrc.json"));
    assert_eq!(
        inspect_flags(&flagged),
        " --root 'packages/my app' --config ci/.fallowrc.json"
    );
}

#[test]
fn the_header_says_that_tests_ran_only_when_a_mutant_ran() {
    let ran = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Killed)],
        Vec::new(),
    );
    assert!(run_text(&ran, &OPTIONS).starts_with("Mutation testing (ran the project tests)\n"));

    let mut no_tests = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::NoCoverage)],
        Vec::new(),
    );
    no_tests.no_test_files = true;
    let text = run_text(&no_tests, &OPTIONS);
    assert!(
        text.starts_with("Mutation testing (no test ran)\n"),
        "{text}"
    );

    let empty = report(Vec::new(), Vec::new());
    let text = run_text(&empty, &OPTIONS);
    insta::assert_snapshot!(text);
}

#[test]
fn an_empty_scope_of_paths_or_all_has_no_widen_hint() {
    let mut empty = report(Vec::new(), Vec::new());
    empty.scope = MutationScopeOutput {
        kind: MutationScopeKind::All,
        base_ref: None,
    };
    let text = run_text(&empty, &OPTIONS);
    assert!(
        text.contains("No mutation data: all source files have no mutant.\n"),
        "{text}"
    );
    assert!(!text.contains("--all"), "{text}");
}

#[test]
fn human_counts_use_singular_and_plural_nouns() {
    let mut one = report(Vec::new(), Vec::new());
    let failure = |id: &str| MutationBaselineFailure {
        file: Some("test/a.test.ts".to_owned()),
        test_id: Some(format!("test/a.test.ts::{id}")),
        message: None,
        mutant_ids: Vec::new(),
    };
    one.baseline_failures = vec![failure("adds")];
    let text = run_text(&one, &OPTIONS);
    assert!(
        text.contains(
            "Baseline failure: test/a.test.ts::adds fails with no active mutant, so no mutant uses its result.\n"
        ),
        "{text}"
    );
    let mut two = one;
    two.baseline_failures.push(failure("subtracts"));
    let text = run_text(&two, &OPTIONS);
    assert!(
        text.contains(
            "Baseline failures: 2 tests fail with no active mutant, so no mutant uses their results. First: test/a.test.ts::adds (see --format json)\n"
        ),
        "{text}"
    );
    assert!(!text.contains("(s)"), "{text}");
}

#[test]
fn no_coverage_is_grouped_by_file_with_a_count_and_a_cap_of_ten_files() {
    let mut mutants = vec![mutant("k", "src/a.ts", 1, MutantStatus::Killed)];
    for file in 0..12 {
        let count = if file == 3 { 3 } else { 1 };
        for index in 0..count {
            mutants.push(mutant(
                &format!("n{file}-{index}"),
                &format!("src/f{file:02}.ts"),
                20 + file,
                MutantStatus::NoCoverage,
            ));
        }
    }
    let text = run_text(&report(mutants, Vec::new()), &OPTIONS);
    let section: Vec<&str> = text
        .split("No coverage (14)")
        .nth(1)
        .unwrap_or_default()
        .lines()
        .skip(1)
        .take_while(|line| !line.is_empty())
        .collect();
    assert_eq!(
        section,
        vec![
            "  src/f03.ts  3 mutants",
            "  src/f00.ts  1 mutant",
            "  src/f01.ts  1 mutant",
            "  src/f02.ts  1 mutant",
            "  src/f04.ts  1 mutant",
            "  src/f05.ts  1 mutant",
            "  src/f06.ts  1 mutant",
            "  src/f07.ts  1 mutant",
            "  src/f08.ts  1 mutant",
            "  src/f09.ts  1 mutant",
            "  and 2 more files (see --format json)",
        ],
        "{text}"
    );
}

#[test]
fn a_long_line_keeps_the_changed_token_in_the_excerpt() {
    let report = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Survived)],
        Vec::new(),
    );
    let mut code = CodeLookup::default();
    code.insert(
        "m1".to_owned(),
        excerpt(
            "    if (sindex === 1 && segments[segments.length - 1]?.endsWith(",
            "\"/\"",
            "\"\"",
            ")) { return joinedSegments; }",
        ),
    );
    let text = run_text(
        &report,
        &RunReportOptions {
            min_score: None,
            code: Some(&code),
            inspect_flags: None,
        },
    );
    let line = text
        .lines()
        .find(|line| line.contains(" -> "))
        .unwrap_or_default();
    let (original, mutated) = line
        .split_once("arithmetic-operator  ")
        .and_then(|(_, code)| code.split_once(" -> "))
        .unwrap_or_default();
    assert!(original.contains("endsWith(\"/\"))"), "{line}");
    assert!(mutated.contains("endsWith(\"\"))"), "{line}");
    assert!(
        original.starts_with("...") && original.ends_with("..."),
        "{line}"
    );
    assert!(original.chars().count() <= 60, "{line}");
}

#[test]
fn a_typecheck_note_says_that_type_level_kills_are_not_counted() {
    let mut noted = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Killed)],
        Vec::new(),
    );
    let text = run_text(&noted, &OPTIONS);
    assert!(!text.contains("type-check"), "{text}");
    noted.runner = Some(fallow_output::MutationRunnerOutput {
        name: "vitest".to_owned(),
        version: "5.0.4".to_owned(),
        protocol_version: 1,
        runtime: fallow_output::MutationRuntimeOutput {
            name: "node".to_owned(),
            version: "v24.0.0".to_owned(),
        },
        concurrency: 1,
        notes: vec!["typecheck-not-run".to_owned()],
    });
    let text = run_text(&noted, &OPTIONS);
    assert!(
        text.contains(
            "Type check: the project enables the Vitest type-check pass, and this run did not run it. Type-level kills are not counted.\n"
        ),
        "{text}"
    );
}

#[test]
fn cached_results_get_one_line_with_the_opt_out() {
    let mut cached = report(
        vec![
            mutant("m1", "src/a.ts", 1, MutantStatus::Killed),
            mutant("m2", "src/a.ts", 2, MutantStatus::Killed),
        ],
        Vec::new(),
    );
    cached.cache = Some(fallow_output::MutationCacheOutput {
        hits: 0,
        stored: 2,
        not_keyed: vec!["dynamic-imports".to_owned(), "file-reads".to_owned()],
    });
    let text = run_text(&cached, &OPTIONS);
    assert!(!text.contains("Cache:"), "no line without a hit: {text}");
    cached.cache = Some(fallow_output::MutationCacheOutput {
        hits: 2,
        stored: 0,
        not_keyed: vec!["dynamic-imports".to_owned(), "file-reads".to_owned()],
    });
    let text = run_text(&cached, &OPTIONS);
    assert!(
        text.contains(
            "Cache: 2 results came from an earlier run. Pass --no-cache to run each mutant.\n"
        ),
        "{text}"
    );
}

#[test]
fn inspect_marks_each_line_of_a_multi_line_mutant() {
    let packet = fallow_output::MutationInspectOutput {
        schema_version: fallow_output::MutationInspectSchemaVersion::V1,
        version: ToolVersion("0.0.0".to_owned()),
        status: fallow_output::MutationInspectStatus::Fresh,
        stale_reason: None,
        location: mutant("m1", "src/a.ts", 3, MutantStatus::Survived).location,
        report_status: Some(MutantStatus::Survived),
        report_sha256: None,
        current_sha256: None,
        code: Some(fallow_output::MutationInspectCode {
            original: "record(\n    items.length,\n    0,\n  );".to_owned(),
            replacement: "void 0;".to_owned(),
            original_lines: "  record(\n    items.length,\n    0,\n  );".to_owned(),
            mutated_lines: "  void 0;".to_owned(),
        }),
        covering_tests: Vec::new(),
        static_test_files: Vec::new(),
        killed_by: None,
    };
    let text = plain(&render_inspect(&packet));
    let diff: Vec<&str> = text.lines().skip(2).take(5).collect();
    assert_eq!(
        diff,
        vec![
            "-    4 |   record(",
            "-    5 |     items.length,",
            "-    6 |     0,",
            "-    7 |   );",
            "+    4 |   void 0;",
        ],
        "{text}"
    );
}

fn reached(mut mutant: MutantResult, reach: TestReachClass) -> MutantResult {
    mutant.test_reach = Some(MutantTestReach {
        reach,
        export_name: None,
        tests: Vec::new(),
        mocked_targets: Vec::new(),
    });
    mutant
}

#[test]
fn each_no_coverage_file_line_counts_the_test_reach_classes() {
    let no_coverage = |id: &str, file: &str| mutant(id, file, 5, MutantStatus::NoCoverage);
    let mutants = vec![
        reached(no_coverage("n1", "src/a.ts"), TestReachClass::NoneFile),
        reached(no_coverage("n2", "src/a.ts"), TestReachClass::MockedOnly),
        reached(no_coverage("n3", "src/a.ts"), TestReachClass::MockedOnly),
        reached(no_coverage("n4", "src/b.ts"), TestReachClass::None),
        no_coverage("n5", "src/c.ts"),
    ];
    let text = run_text(&report(mutants, Vec::new()), &OPTIONS);
    let section: Vec<&str> = text
        .split("No coverage (5)")
        .nth(1)
        .unwrap_or_default()
        .lines()
        .skip(1)
        .take(3)
        .collect();
    assert_eq!(
        section,
        vec![
            "  src/a.ts  3 mutants  test reach: 2 mocked-only, 1 none-file",
            "  src/b.ts  1 mutant  test reach: 1 none",
            "  src/c.ts  1 mutant",
        ],
        "{text}"
    );
}

#[test]
fn human_plan_counts_the_no_coverage_mutants_by_test_reach() {
    let entry = |id: &str, reach: Option<TestReachClass>| MutationPlanMutant {
        location: mutant(id, "src/a.ts", 1, MutantStatus::Pending).location,
        disposition: MutationPlanDisposition::NoCoverage,
        prune_reason: None,
        test_reach: reach.map(|reach| MutantTestReach {
            reach,
            export_name: None,
            tests: Vec::new(),
            mocked_targets: Vec::new(),
        }),
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
            total: 3,
            no_coverage: 3,
            ..MutationPlanCountsOutput::default()
        },
        mutants: vec![
            entry("m1", Some(TestReachClass::NoneFile)),
            entry("m2", Some(TestReachClass::MockedOnly)),
            entry("m3", Some(TestReachClass::MockedOnly)),
        ],
        files: Vec::new(),
        test_files: Vec::new(),
        no_test_files: false,
        framework_config_checked: true,
        skipped_files: Vec::new(),
        unmatched_paths: Vec::new(),
    };
    let text = plain(&render_plan(&plan));
    assert!(
        text.contains("No coverage by test reach: 2 mocked-only, 1 none-file\n"),
        "{text}"
    );
}

#[test]
fn human_plan_lists_planned_mutants_and_prune_reasons() {
    let planned = |id: &str, rank: u32, disposition, reason: Option<&str>| MutationPlanMutant {
        location: mutant(id, "src/a.ts", rank, MutantStatus::Pending).location,
        disposition,
        prune_reason: reason.map(str::to_owned),
        test_reach: None,
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

fn inspect_packet(
    status: fallow_output::MutationInspectStatus,
    stale_reason: Option<fallow_output::MutationStaleReason>,
) -> fallow_output::MutationInspectOutput {
    fallow_output::MutationInspectOutput {
        schema_version: fallow_output::MutationInspectSchemaVersion::V1,
        version: ToolVersion("0.0.0".to_owned()),
        status,
        stale_reason,
        location: mutant("m1", "src/a.ts", 3, MutantStatus::Survived).location,
        report_status: Some(MutantStatus::Survived),
        report_sha256: None,
        current_sha256: None,
        code: None,
        covering_tests: Vec::new(),
        static_test_files: Vec::new(),
        killed_by: None,
    }
}

#[test]
fn a_stale_inspect_names_the_reason_and_suggests_no_run() {
    let packet = inspect_packet(
        fallow_output::MutationInspectStatus::Stale,
        Some(fallow_output::MutationStaleReason::SourceChanged),
    );
    let text = plain(&render_inspect(&packet));
    assert!(
        text.contains(
            "Stale: the file changed after the report. The report no longer describes this code.\n"
        ),
        "{text}"
    );
    assert!(!text.contains("again"), "{text}");
    assert!(!text.contains("Run `fallow mutation`"), "{text}");
}

#[test]
fn the_covered_by_line_names_the_count_and_one_shortened_test() {
    let mut survivor = mutant("m1", "src/a.ts", 1, MutantStatus::Survived);
    let long_name = format!(
        "test/join.test.ts::joinRelativeURL > {}",
        "joinRelativeURL(\"../a\", \"../../../../b\") === \"../../../b\"".repeat(4)
    );
    survivor.covering_tests = (0..28)
        .map(|index| format!("{long_name} {index}"))
        .collect();
    let text = run_text(&report(vec![survivor], Vec::new()), &OPTIONS);
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("covered by"))
        .expect("a covered-by line");
    assert!(
        line.trim_start()
            .starts_with("covered by 28 tests, first: test/join.test.ts::joinRelativeURL"),
        "{line}"
    );
    assert!(line.ends_with("..."), "{line}");
    assert!(
        line.chars().count() <= 100,
        "{} chars: {line}",
        line.chars().count()
    );

    let mut single = mutant("m1", "src/a.ts", 1, MutantStatus::Survived);
    single.covering_tests = vec!["test/a.test.ts::adds".to_owned()];
    let text = run_text(&report(vec![single], Vec::new()), &OPTIONS);
    assert!(
        text.contains("      covered by 1 test: test/a.test.ts::adds\n"),
        "{text}"
    );
}

#[test]
fn the_scope_line_shortens_a_full_sha_base_ref() {
    let mut empty = report(Vec::new(), Vec::new());
    empty.scope.base_ref = Some("f06c800d0c59f2a4a1b9ba65eb6cb61a84419be6".to_owned());
    let text = run_text(&empty, &OPTIONS);
    assert!(
        text.contains("Scope: changed code against f06c800d0c59\n"),
        "{text}"
    );
    assert_eq!(
        text.matches("f06c800d0c59").count(),
        1,
        "the base ref is printed once: {text}"
    );
    assert!(!text.contains("f06c800d0c59f2a4"), "{text}");
}

fn inspect_with_tests(count: usize) -> fallow_output::MutationInspectOutput {
    let mut packet = inspect_packet(fallow_output::MutationInspectStatus::Fresh, None);
    packet.covering_tests = (1..=count)
        .map(|index| format!("test/a.test.ts::case {index}"))
        .collect();
    packet
}

#[test]
fn inspect_caps_the_covering_tests_at_ten() {
    let text = plain(&render_inspect(&inspect_with_tests(12)));
    assert!(
        text.contains("\n\n\u{25cf} Covering tests (12)\n"),
        "a blank line comes before the section, as in the run output: {text}"
    );
    assert!(text.contains("  test/a.test.ts::case 10\n"), "{text}");
    assert!(!text.contains("case 11"), "{text}");
    assert!(
        text.contains("  and 2 more tests (see --format json)\n"),
        "{text}"
    );
}

#[test]
fn inspect_puts_a_blank_line_before_each_block_after_the_code() {
    let mut killed = inspect_with_tests(12);
    killed.killed_by = Some("test/a.test.ts::case 3".to_owned());
    let text = plain(&render_inspect(&killed));
    assert!(
        text.contains(
            "  and 2 more tests (see --format json)\n\nKilled by: test/a.test.ts::case 3\n"
        ),
        "{text}"
    );

    let mut unreached = inspect_packet(fallow_output::MutationInspectStatus::Fresh, None);
    unreached.static_test_files = vec!["test/a.test.ts".to_owned()];
    let text = plain(&render_inspect(&unreached));
    assert!(
        text.contains("\n\n\u{25cf} Test files that reach the file (1)\n  test/a.test.ts\n"),
        "{text}"
    );
}

#[test]
fn inspect_names_one_hidden_covering_test_in_the_singular() {
    let text = plain(&render_inspect(&inspect_with_tests(11)));
    assert!(
        text.contains("  and 1 more test (see --format json)\n"),
        "{text}"
    );
    assert!(!text.contains("more tests"), "{text}");
}

#[test]
fn inspect_shows_ten_covering_tests_with_no_more_line() {
    let text = plain(&render_inspect(&inspect_with_tests(10)));
    assert!(text.contains("  test/a.test.ts::case 10\n"), "{text}");
    assert!(!text.contains("more test"), "{text}");
}

fn min_score_line(score: Option<f64>, min_score: f64) -> String {
    let mut scored = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Killed)],
        Vec::new(),
    );
    scored.summary.score = score;
    let options = RunReportOptions {
        min_score: Some(min_score),
        code: None,
        inspect_flags: Some(""),
    };
    run_text(&scored, &options)
        .lines()
        .find(|line| line.contains("Minimum score"))
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn a_met_minimum_score_shows_the_ok_prefix() {
    assert_eq!(
        min_score_line(Some(0.8), 50.0),
        "[OK] Minimum score 50%: met"
    );
}

#[test]
fn a_missed_minimum_score_shows_the_failure_prefix() {
    assert_eq!(
        min_score_line(Some(0.4), 50.0),
        "[X] Minimum score 50%: missed"
    );
}

#[test]
fn an_unmeasured_minimum_score_shows_the_inactive_prefix() {
    assert_eq!(
        min_score_line(None, 50.0),
        "[-] Minimum score 50%: not measured"
    );
}

#[test]
fn an_error_line_names_its_mutant() {
    let mut errored = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::RuntimeError)],
        Vec::new(),
    );
    errored.errors = vec![
        fallow_output::MutationRunIssue {
            code: "runtime-error".to_owned(),
            message: "the companion exited".to_owned(),
            mutant_id: Some("m1".to_owned()),
            stderr_tail: None,
        },
        fallow_output::MutationRunIssue {
            code: "test-files-not-discovered".to_owned(),
            message: "the runner does not list 1 static test file".to_owned(),
            mutant_id: None,
            stderr_tail: None,
        },
    ];
    let text = run_text(&errored, &OPTIONS);
    assert!(
        text.contains("Error (runtime-error), mutant m1: the companion exited\n"),
        "{text}"
    );
    assert!(
        text.contains("Error (test-files-not-discovered): the runner does not list"),
        "{text}"
    );
}

#[test]
fn the_survived_section_shows_ten_mutants_and_one_more_line() {
    let survivors: Vec<MutantResult> = (1..=12)
        .map(|rank| {
            mutant(
                &format!("s{rank}"),
                if rank % 2 == 0 {
                    "src/a.ts"
                } else {
                    "src/b.ts"
                },
                rank,
                MutantStatus::Survived,
            )
        })
        .collect();
    let text = run_text(&report(survivors, Vec::new()), &OPTIONS);
    assert!(text.contains("\u{25cf} Survived (12)\n"), "{text}");
    assert_eq!(text.matches("      id s").count(), 10, "{text}");
    assert!(text.contains("id s10\n"), "{text}");
    assert!(!text.contains("id s11\n"), "{text}");
    assert!(
        text.contains("  and 2 more survivors (see --format json)\n"),
        "{text}"
    );

    let ten: Vec<MutantResult> = (1..=10)
        .map(|rank| {
            mutant(
                &format!("s{rank}"),
                "src/a.ts",
                rank,
                MutantStatus::Survived,
            )
        })
        .collect();
    let text = run_text(&report(ten, Vec::new()), &OPTIONS);
    assert_eq!(text.matches("      id s").count(), 10, "{text}");
    assert!(!text.contains("more survivor"), "{text}");
}

#[test]
fn inspect_caps_the_static_test_files_under_a_section_header() {
    let mut packet = inspect_packet(fallow_output::MutationInspectStatus::Fresh, None);
    packet.static_test_files = (1..=12)
        .map(|index| format!("test/case-{index:02}.test.ts"))
        .collect();
    let text = plain(&render_inspect(&packet));
    assert!(
        text.contains("\n\n\u{25cf} Test files that reach the file (12)\n"),
        "a blank line comes before the section, as in the run output: {text}"
    );
    assert!(text.contains("  test/case-10.test.ts\n"), "{text}");
    assert!(!text.contains("case-11"), "{text}");
    assert!(
        text.contains("  and 2 more files (see --format json)\n"),
        "{text}"
    );
}

fn excerpt_sides(code: CodeExcerpt) -> (String, String) {
    let report = report(
        vec![mutant("m1", "src/a.ts", 1, MutantStatus::Survived)],
        Vec::new(),
    );
    let mut lookup = CodeLookup::default();
    lookup.insert("m1".to_owned(), code);
    let text = run_text(
        &report,
        &RunReportOptions {
            min_score: None,
            code: Some(&lookup),
            inspect_flags: None,
        },
    );
    let line = text
        .lines()
        .find(|line| line.contains(" -> "))
        .unwrap_or_default()
        .to_owned();
    let (original, mutated) = line
        .split_once("arithmetic-operator  ")
        .and_then(|(_, code)| code.split_once(" -> "))
        .unwrap_or_default();
    (original.to_owned(), mutated.to_owned())
}

#[test]
fn a_long_span_centers_the_excerpt_on_the_changed_token() {
    let (original, mutated) = excerpt_sides(excerpt(
        "  return ",
        "alpha_very_long_operand_name_number_one === beta_very_long_operand_name_two || gamma === delta",
        "alpha_very_long_operand_name_number_one === beta_very_long_operand_name_two && gamma === delta",
        ";",
    ));
    assert!(original.contains("two || gamma"), "{original}");
    assert!(mutated.contains("two && gamma"), "{mutated}");
    assert_eq!(
        original.replace("||", "&&"),
        mutated,
        "one window for both sides"
    );
    assert!(original.chars().count() <= 60, "{original}");
}

#[test]
fn the_original_and_the_replacement_share_one_window() {
    let (original, mutated) = excerpt_sides(excerpt(
        "if (inputString === \".\" || ",
        "inputString === \"..\"",
        "false",
        ") return true;",
    ));
    assert_eq!(
        original, "...utString === \".\" || inputString === \"..\") return true;",
        "{original}"
    );
    assert_eq!(
        mutated, "...utString === \".\" || false) return true;",
        "{mutated}"
    );
}

#[test]
fn a_relative_output_file_resolves_against_the_process_directory() {
    let mut relative = input(
        Path::new("/project"),
        MutationArgs::default(),
        MutationConfig::default(),
    );
    relative.output_file = Some(Path::new("reports/mutation.json"));
    let options = run_options(&relative).expect("options");
    let cwd = std::env::current_dir().expect("cwd");
    assert_eq!(
        options.output_paths,
        vec![cwd.join("reports/mutation.json")]
    );
    relative.output_file = Some(Path::new("/tmp/mutation.json"));
    let options = run_options(&relative).expect("options");
    assert_eq!(
        options.output_paths,
        vec![std::path::PathBuf::from("/tmp/mutation.json")]
    );
}

#[test]
fn the_plan_list_shows_twenty_mutants_and_one_more_line() {
    let plan = MutationPlanOutput {
        schema_version: MutationPlanSchemaVersion::V1,
        version: ToolVersion("0.0.0".to_owned()),
        elapsed_ms: ElapsedMs(0),
        scope: MutationScopeOutput {
            kind: MutationScopeKind::All,
            base_ref: None,
        },
        counts: MutationPlanCountsOutput {
            total: 21,
            planned: 21,
            ..MutationPlanCountsOutput::default()
        },
        mutants: (1..=21)
            .map(|rank| MutationPlanMutant {
                location: mutant(&format!("p{rank}"), "src/a.ts", rank, MutantStatus::Pending)
                    .location,
                disposition: MutationPlanDisposition::Planned,
                prune_reason: None,
                test_reach: None,
            })
            .collect(),
        files: Vec::new(),
        test_files: vec!["test/a.test.ts".to_owned()],
        no_test_files: false,
        framework_config_checked: true,
        skipped_files: Vec::new(),
        unmatched_paths: Vec::new(),
    };
    let text = plain(&render_plan(&plan));
    assert!(text.contains("  p20\n"), "{text}");
    assert!(!text.contains("  p21\n"), "{text}");
    assert!(
        text.contains("  and 1 more mutant (see --format json)\n"),
        "{text}"
    );
}
