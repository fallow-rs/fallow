use super::*;
use crate::gate_outcomes::GateStatus;
use crate::mutation::MutationIncompleteReason;
use crate::pr_decision::PrDecisionConclusion;

const FENCE_OPEN: &str = "````text";
const FENCE_CLOSE: &str = "````";

fn counts(
    killed: usize,
    survived: usize,
    no_coverage: usize,
    pending: usize,
) -> MutationStatusCounts {
    MutationStatusCounts {
        total: killed + survived + no_coverage + pending,
        killed,
        survived,
        no_coverage,
        pending,
        ..MutationStatusCounts::default()
    }
}

fn gate(status: GateStatus, observed: Option<f64>, threshold: f64) -> GateOutcome {
    GateOutcome {
        status,
        enforced: true,
        observed,
        threshold: Some(threshold),
        threshold_label: None,
        files: Vec::new(),
    }
}

fn location(file: &str, line: u32, function: Option<&str>) -> MutationCiLocation {
    MutationCiLocation {
        file: file.to_owned(),
        line,
        function: function.map(str::to_owned),
    }
}

fn ran(counts: MutationStatusCounts, score: Option<f64>) -> MutationCiView {
    MutationCiView {
        scope: Some(MutationScopeKind::Changed),
        counts,
        score,
        completion: Some(MutationCompletion::from_reasons(Vec::new())),
        elapsed_ms: Some(252_000),
        cache_hits: Some(3),
        ..MutationCiView::new(MutationCiState::Ran)
    }
}

fn clean() -> MutationCiView {
    ran(counts(12, 0, 0, 0), Some(1.0))
}

fn survivors() -> MutationCiView {
    ran(counts(10, 6, 0, 0), Some(0.625))
}

fn partial_time_budget() -> MutationCiView {
    MutationCiView {
        completion: Some(MutationCompletion::from_reasons(vec![
            MutationIncompleteReason::TimeBudget,
        ])),
        ..ran(counts(30, 10, 0, 80), Some(0.75))
    }
}

fn spec_example() -> MutationCiView {
    MutationCiView {
        completion: Some(MutationCompletion::from_reasons(vec![
            MutationIncompleteReason::TimeBudget,
        ])),
        min_score_gate: Some(gate(GateStatus::Fail, Some(62.5), 80.0)),
        no_coverage: vec![
            location("src/age.ts", 41, None),
            location("src/age.ts", 30, Some("formatAge")),
        ],
        ..ran(counts(10, 6, 2, 0), Some(0.625))
    }
}

fn gate_fail() -> MutationCiView {
    MutationCiView {
        min_score_gate: Some(gate(GateStatus::Fail, Some(62.5), 80.0)),
        ..survivors()
    }
}

fn empty_scope() -> MutationCiView {
    ran(MutationStatusCounts::default(), None)
}

fn null_score() -> MutationCiView {
    MutationCiView {
        min_score_gate: Some(GateOutcome {
            observed: None,
            ..gate(GateStatus::Skipped, None, 80.0)
        }),
        no_coverage: vec![location("src/new.ts", 3, Some("build"))],
        ..ran(counts(0, 0, 4, 0), None)
    }
}

fn stale_hidden() -> MutationCiView {
    MutationCiView {
        stale_hidden: 2,
        ..survivors()
    }
}

fn inline_overflow() -> MutationCiView {
    MutationCiView {
        inline_overflow: 14,
        ..ran(counts(40, 34, 0, 0), Some(40.0 / 74.0))
    }
}

fn no_coverage_cap() -> MutationCiView {
    let no_coverage = (1..=12)
        .rev()
        .map(|line| location("src/wide.ts", line, None))
        .collect();
    MutationCiView {
        no_coverage,
        ..ran(counts(5, 0, 12, 0), Some(1.0))
    }
}

fn not_run(reason: MutationNotRunReason) -> MutationCiView {
    MutationCiView::new(MutationCiState::NotRun(reason))
}

fn failed(message: &str, help: Option<&str>) -> MutationCiView {
    MutationCiView::new(MutationCiState::Failed {
        exit_code: 2,
        code: "FALLOW_MUTATION_RUNNER".to_owned(),
        message: message.to_owned(),
        help: help.map(str::to_owned),
    })
}

fn unreadable() -> MutationCiView {
    MutationCiView::new(MutationCiState::Unreadable {
        reason: "invalid type: string \"x\", expected a sequence at line 1 column 40".to_owned(),
    })
}

fn non_dot_root() -> MutationCiView {
    MutationCiView {
        root_flag: Some("packages/web app".to_owned()),
        ..spec_example()
    }
}

fn both_headings(view: &MutationCiView) -> String {
    format!(
        "{}\n\n===== summary heading =====\n\n{}",
        render_mutation_section(view, MutationHeading::Comment),
        render_mutation_section(view, MutationHeading::Summary)
    )
}

fn snapshot_states() -> Vec<(&'static str, MutationCiView)> {
    vec![
        ("clean", clean()),
        ("survivors", survivors()),
        ("partial_time_budget", partial_time_budget()),
        ("gate_fail", gate_fail()),
        ("partial_gate_fail_no_coverage", spec_example()),
        ("empty_scope", empty_scope()),
        ("null_score", null_score()),
        ("stale_hidden", stale_hidden()),
        ("inline_overflow", inline_overflow()),
        ("no_coverage_cap", no_coverage_cap()),
        (
            "not_run_event_refused",
            not_run(MutationNotRunReason::EventRefused),
        ),
        (
            "not_run_fork_refused",
            not_run(MutationNotRunReason::ForkRefused),
        ),
        (
            "not_run_no_change_scope",
            not_run(MutationNotRunReason::NoChangeScope),
        ),
        (
            "interrupted",
            MutationCiView::new(MutationCiState::Interrupted),
        ),
        (
            "failed_with_code_and_help",
            failed(
                "The test runner exited before the dry run.",
                Some("Run `fallow mutation --dry-run` to see the runner output."),
            ),
        ),
        ("failed_long_message", failed(&"x".repeat(5000), None)),
        ("unreadable", unreadable()),
        ("non_dot_root", non_dot_root()),
    ]
}

#[test]
fn each_state_matches_its_snapshot_with_both_headings() {
    for (name, view) in snapshot_states() {
        insta::assert_snapshot!(name, both_headings(&view));
    }
}

#[test]
fn spec_example_renders_the_documented_comment_section() {
    let expected = "#### Mutation testing

**Partial run** (`time-budget`). The counts cover only the mutants that ran. The next run with the same budget continues from the result cache.

**Score below the minimum.** The score is 62.5%, and the minimum is 80%.

| Killed | Survived | No coverage | Not measured | Score |
| --- | --- | --- | --- | --- |
| 10 | 6 | 2 | 0 | 62.5% |

Scope: the changed lines.

Each survived mutant is a small code change that made no test fail. The **Tests** findings above show the survived mutants on the changed lines. Add a test that fails on the change.

<details>
<summary>No test reaches these lines (2)</summary>

- `src/age.ts:30`, function `formatAge`
- `src/age.ts:41`

</details>

To inspect a mutant, run `fallow mutation inspect <mutant-id> --report fallow-mutation.json` with the `fallow-mutation.json` file of this job.";
    assert_eq!(
        render_mutation_section(&spec_example(), MutationHeading::Comment),
        expected
    );
}

#[test]
fn headings_use_their_level() {
    let view = clean();
    assert!(
        render_mutation_section(&view, MutationHeading::Comment)
            .starts_with("#### Mutation testing\n\n")
    );
    assert!(
        render_mutation_section(&view, MutationHeading::Summary)
            .starts_with("## Mutation testing\n\n")
    );
}

#[test]
fn headline_follows_the_first_matching_rule() {
    let cases = [
        (
            gate_fail(),
            "**Score below the minimum.** The score is 62.5%, and the minimum is 80%.",
        ),
        (empty_scope(), "The scope has no mutant."),
        (
            null_score(),
            "**No mutant was counted.** The score gate was skipped.",
        ),
        (survivors(), "**6 mutants survived.** Score 62.5%."),
        (
            MutationCiView {
                min_score_gate: Some(gate(GateStatus::Pass, Some(62.5), 60.0)),
                ..survivors()
            },
            "**6 mutants survived.** Score 62.5% (minimum 60%).",
        ),
        (
            ran(counts(3, 1, 0, 0), Some(0.75)),
            "**1 mutant survived.** Score 75.0%.",
        ),
        (clean(), "The tests detected each mutant that ran."),
    ];
    for (view, headline) in cases {
        let out = render_mutation_section(&view, MutationHeading::Comment);
        let first = out.split("\n\n").nth(1).unwrap_or_default();
        assert_eq!(first, headline, "{out}");
    }
}

#[test]
fn score_text_never_rounds_up_to_the_minimum() {
    let view = MutationCiView {
        min_score_gate: Some(gate(GateStatus::Fail, Some(79.96), 80.0)),
        ..ran(counts(7996, 2004, 0, 0), Some(0.7996))
    };
    let out = render_mutation_section(&view, MutationHeading::Comment);
    assert!(
        out.contains("The score is 79.9%, and the minimum is 80%."),
        "{out}"
    );
}

#[test]
fn not_measured_counts_pending_runtime_errors_flaky_and_compile_errors() {
    let view = ran(
        MutationStatusCounts {
            total: 14,
            killed: 3,
            timeout: 1,
            pending: 2,
            runtime_error: 3,
            flaky: 4,
            compile_error: 1,
            ..MutationStatusCounts::default()
        },
        Some(1.0),
    );
    let out = render_mutation_section(&view, MutationHeading::Comment);
    assert!(out.contains("| 4 | 0 | 0 | 10 | 100.0% |"), "{out}");
}

#[test]
fn counts_near_the_integer_limit_do_not_overflow() {
    // The counts come from a file that test code can write.
    let view = ran(
        MutationStatusCounts {
            total: usize::MAX,
            killed: usize::MAX,
            timeout: 5,
            survived: usize::MAX,
            no_coverage: 1,
            pending: usize::MAX,
            runtime_error: 1,
            ..MutationStatusCounts::default()
        },
        Some(0.5),
    );
    for heading in [
        MutationHeading::Comment,
        MutationHeading::Review,
        MutationHeading::Summary,
    ] {
        let out = render_mutation_section(&view, heading);
        assert!(out.contains(&format!("| {} |", usize::MAX)), "{out}");
    }
    assert_eq!(view.counts.executed(), usize::MAX);
}

#[test]
fn survivors_without_a_finding_get_no_pointer_to_the_findings() {
    let all_off_diff = MutationCiView {
        off_diff_hidden: 6,
        ..survivors()
    };
    let out = render_mutation_section(&all_off_diff, MutationHeading::Comment);
    assert!(!out.contains("findings above"), "{out}");
    assert!(
        out.contains(
            "6 survived mutants are on lines that this change did not add, so they have no finding."
        ),
        "{out}"
    );

    let some_off_diff = MutationCiView {
        off_diff_hidden: 2,
        ..survivors()
    };
    let out = render_mutation_section(&some_off_diff, MutationHeading::Review);
    assert!(
        out.contains("The inline comments show the survived mutants"),
        "{out}"
    );
    assert!(out.contains("2 survived mutants are on lines"), "{out}");
}

#[test]
fn each_surface_points_at_the_place_where_its_survivors_are() {
    let comment = render_mutation_section(&survivors(), MutationHeading::Comment);
    assert!(
        comment.contains("The **Tests** findings above"),
        "{comment}"
    );

    // A review summary has no findings tables: the survivors are inline comments.
    let review = render_mutation_section(&survivors(), MutationHeading::Review);
    assert!(review.starts_with("#### Mutation testing"), "{review}");
    assert!(
        review.contains("The inline comments show the survived mutants"),
        "{review}"
    );
    assert!(!review.contains("findings above"), "{review}");

    let summary = render_mutation_section(&survivors(), MutationHeading::Summary);
    assert!(!summary.contains("findings above"), "{summary}");
    assert!(!summary.contains("inline comments"), "{summary}");
}

#[test]
fn optional_lines_appear_only_when_their_count_is_not_zero() {
    let plain = render_mutation_section(&survivors(), MutationHeading::Comment);
    assert!(!plain.contains("not shown"), "{plain}");

    let stale = render_mutation_section(&stale_hidden(), MutationHeading::Comment);
    assert!(
        stale
            .contains("2 survived mutants are not shown because their file changed after the run."),
        "{stale}"
    );

    let overflow = render_mutation_section(&inline_overflow(), MutationHeading::Comment);
    assert!(
        overflow.contains(
            "14 more survived mutants are not shown inline. The fallow-mutation.json file of this job lists them."
        ),
        "{overflow}"
    );
}

#[test]
fn no_coverage_list_is_capped_with_a_count_in_the_summary() {
    let out = render_mutation_section(&no_coverage_cap(), MutationHeading::Comment);
    assert!(
        out.contains("<summary>No test reaches these lines (10 of 12)</summary>"),
        "{out}"
    );
    let items = out
        .lines()
        .filter(|line| line.starts_with("- `src/wide.ts:"))
        .count();
    assert_eq!(items, MUTATION_NO_COVERAGE_ITEMS);
    assert!(out.contains("- `src/wide.ts:1`\n"), "{out}");
    assert!(!out.contains("src/wide.ts:11`"), "{out}");
}

#[test]
fn inspect_command_adds_a_quoted_root_only_when_the_root_is_not_dot() {
    let dot = MutationCiView {
        root_flag: Some(".".to_owned()),
        ..spec_example()
    };
    assert!(!render_mutation_section(&dot, MutationHeading::Comment).contains("--root"));

    let out = render_mutation_section(&non_dot_root(), MutationHeading::Comment);
    assert!(
        out.contains(
            "`fallow mutation inspect <mutant-id> --report fallow-mutation.json --root 'packages/web app'`"
        ),
        "{out}"
    );
}

#[test]
fn long_message_is_cut_at_the_cap_and_marked() {
    let out = render_mutation_section(&failed(&"x".repeat(5000), None), MutationHeading::Comment);
    let expected = format!(
        "{FENCE_OPEN}\n{} ... (shortened)\n{FENCE_CLOSE}",
        "x".repeat(MUTATION_MESSAGE_MAX_CHARS)
    );
    assert!(out.contains(&expected), "{out}");
}

#[test]
fn failed_state_renders_the_code_the_message_and_the_help() {
    let out = render_mutation_section(
        &failed("Runner exited.", Some("Check the runner.")),
        MutationHeading::Comment,
    );
    assert_eq!(
        out,
        "#### Mutation testing\n\n\
         **Mutation testing failed** (exit code 2, `FALLOW_MUTATION_RUNNER`).\n\n\
         ````text\nRunner exited.\n````\n\n\
         ````text\nCheck the runner.\n````"
    );
}

#[test]
fn oversized_section_keeps_the_heading_the_headline_and_the_table() {
    let long_path = format!("src/{}.ts", "p".repeat(900));
    let long_function = "f".repeat(900);
    let no_coverage = (1..=10)
        .map(|line| location(&long_path, line, Some(&long_function)))
        .collect();
    let view = MutationCiView {
        no_coverage,
        ..spec_example()
    };
    let out = render_mutation_section(&view, MutationHeading::Comment);
    assert!(
        out.chars().count() <= MUTATION_SECTION_MAX_CHARS,
        "{}",
        out.len()
    );
    assert!(
        out.starts_with("#### Mutation testing\n\n**Partial run**"),
        "{out}"
    );
    assert!(out.contains("**Score below the minimum.**"), "{out}");
    assert!(out.contains("| 10 | 6 | 2 | 0 | 62.5% |"), "{out}");
    assert!(
        out.ends_with(
            "The mutation section is shortened. The fallow-mutation.json file of this job has all results."
        ),
        "{out}"
    );
    assert!(!out.contains("<details>"), "{out}");
}

const HOSTILE: &str = "a|b`c``` <details>--> \r\nx\r```suggestion\n````\n</details>\ny";

fn hostile_view() -> MutationCiView {
    MutationCiView {
        no_coverage: vec![location(HOSTILE, 7, Some(HOSTILE))],
        root_flag: Some(HOSTILE.to_owned()),
        ..spec_example()
    }
}

fn hostile_failed() -> MutationCiView {
    MutationCiView::new(MutationCiState::Failed {
        exit_code: 2,
        code: HOSTILE.to_owned(),
        message: HOSTILE.to_owned(),
        help: Some(HOSTILE.to_owned()),
    })
}

fn hostile_unreadable() -> MutationCiView {
    MutationCiView::new(MutationCiState::Unreadable {
        reason: HOSTILE.to_owned(),
    })
}

/// Walks the lines, and fails when a hostile value left its code span or its
/// fence: a stray line ending, a fence that opens or closes, or a raw
/// `<details>` or `</details>` line that the renderer did not write.
fn assert_inert(out: &str, own_details: usize) {
    assert!(!out.contains('\r'), "{out:?}");
    let mut in_fence = false;
    let mut details_lines = 0;
    for line in out.lines() {
        if in_fence {
            if line == FENCE_CLOSE {
                in_fence = false;
            } else {
                assert!(!line.starts_with("````"), "fence closed early: {out}");
            }
            continue;
        }
        if line == FENCE_OPEN {
            in_fence = true;
            continue;
        }
        assert!(!line.trim_start().starts_with("```"), "stray fence: {out}");
        assert!(!line.starts_with('y'), "line ending survived: {out}");
        assert!(!line.starts_with('x'), "line ending survived: {out}");
        if line == "<details>" || line == "</details>" {
            details_lines += 1;
        }
    }
    assert!(!in_fence, "fence never closed: {out}");
    assert_eq!(details_lines, own_details * 2, "{out}");
}

#[test]
fn hostile_strings_stay_inert_in_every_state() {
    for heading in [MutationHeading::Comment, MutationHeading::Summary] {
        assert_inert(&render_mutation_section(&hostile_view(), heading), 1);
        assert_inert(&render_mutation_section(&hostile_failed(), heading), 0);
        assert_inert(&render_mutation_section(&hostile_unreadable(), heading), 0);
    }
}

#[test]
fn hostile_list_item_stays_in_code_spans() {
    let out = render_mutation_section(&hostile_view(), MutationHeading::Comment);
    let flat = "a|b`c``` <details>-->  x ```suggestion ```` </details> y";
    let expected = format!(
        "- {}, function {}",
        crate::ci_output::markdown_code_span(&format!("{flat}:7")),
        crate::ci_output::markdown_code_span(flat),
    );
    assert!(out.lines().any(|line| line == expected), "{out}");
}

#[test]
fn no_output_has_a_suggestion_fence() {
    let mut views: Vec<MutationCiView> = snapshot_states()
        .into_iter()
        .map(|(_, view)| view)
        .collect();
    views.extend([hostile_view(), hostile_failed(), hostile_unreadable()]);
    for view in &views {
        for heading in [MutationHeading::Comment, MutationHeading::Summary] {
            let out = render_mutation_section(view, heading);
            assert!(
                out.lines()
                    .all(|line| !line.trim_start().starts_with("```suggestion")),
                "{out}"
            );
        }
        assert!(!mutation_review_summary_line(view, 3).contains("```"));
    }
}

#[test]
fn rendering_is_deterministic_and_order_independent() {
    for (_, view) in snapshot_states() {
        for heading in [MutationHeading::Comment, MutationHeading::Summary] {
            assert_eq!(
                render_mutation_section(&view, heading),
                render_mutation_section(&view.clone(), heading)
            );
        }
    }
    let mut reversed = spec_example();
    reversed.no_coverage.reverse();
    assert_eq!(
        render_mutation_section(&reversed, MutationHeading::Comment),
        render_mutation_section(&spec_example(), MutationHeading::Comment)
    );
}

#[test]
fn only_the_summary_heading_prints_the_elapsed_time_and_the_cache_hits() {
    for (_, view) in snapshot_states() {
        let comment = render_mutation_section(&view, MutationHeading::Comment);
        assert!(!comment.contains("The run took"), "{comment}");
        assert!(!comment.contains("from the cache."), "{comment}");
    }
    let summary = render_mutation_section(&survivors(), MutationHeading::Summary);
    assert!(
        summary.contains("The run took 4m 12s. 3 results came from the cache."),
        "{summary}"
    );
}

fn row(view: &MutationCiView) -> (PrDecisionConclusion, String, String) {
    let row = mutation_decision_row(view);
    assert_eq!(row.id, "mutation");
    assert_eq!(row.label, "Mutation testing");
    assert_eq!(row.threshold, None);
    (row.status, row.observed, row.scope)
}

#[test]
fn decision_row_maps_each_state() {
    use PrDecisionConclusion::{Failure, Neutral, Skipped, Success};
    let changed = "changed lines".to_owned();
    assert_eq!(
        row(&clean()),
        (Success, "100.0% score".to_owned(), changed.clone())
    );
    assert_eq!(
        row(&survivors()),
        (
            Neutral,
            "62.5% score, 6 survived".to_owned(),
            changed.clone()
        )
    );
    assert_eq!(
        row(&partial_time_budget()),
        (
            Neutral,
            "partial: 40 of 120 mutants ran".to_owned(),
            changed
        )
    );
    assert_eq!(
        row(&MutationCiView {
            scope: Some(MutationScopeKind::All),
            ..no_coverage_cap()
        }),
        (
            Neutral,
            "100.0% score, 12 not covered".to_owned(),
            "all files".to_owned()
        )
    );
    assert_eq!(
        row(&MutationCiView {
            scope: Some(MutationScopeKind::Paths),
            ..empty_scope()
        }),
        (Success, "no mutant".to_owned(), "selected paths".to_owned())
    );
    for reason in [
        MutationNotRunReason::EventRefused,
        MutationNotRunReason::ForkRefused,
        MutationNotRunReason::NoChangeScope,
    ] {
        assert_eq!(
            row(&not_run(reason)),
            (Skipped, "not run".to_owned(), "-".to_owned())
        );
    }
    for view in [
        failed("boom", None),
        MutationCiView::new(MutationCiState::Interrupted),
        unreadable(),
    ] {
        assert_eq!(row(&view), (Failure, "failed".to_owned(), "-".to_owned()));
    }
}

#[test]
fn survived_description_names_the_change_the_function_and_the_mutant() {
    let change = MutantChange {
        from: ">=".to_owned(),
        to: ">".to_owned(),
    };
    assert_eq!(
        survived_description(
            "conditional-boundary",
            Some(&change),
            Some("isAdult"),
            "9f3c2a1b"
        ),
        "Changing >= to > in isAdult made no test fail. Mutant 9f3c2a1b."
    );
    let removal = MutantChange {
        from: "<call>".to_owned(),
        to: String::new(),
    };
    assert_eq!(
        survived_description("statement-removal", Some(&removal), None, "ab12"),
        "Removing <call> made no test fail. Mutant ab12."
    );
    assert_eq!(
        survived_description("statement-removal", None, Some(""), "ab12"),
        "A statement-removal change made no test fail. Mutant ab12."
    );
}

#[test]
fn survived_description_is_one_line() {
    let change = MutantChange {
        from: "a\r\nb".to_owned(),
        to: "c\rd".to_owned(),
    };
    let text = survived_description("op\n", Some(&change), Some("f\ng"), "id\r\n");
    assert_eq!(
        text,
        "Changing a b to c d in f g made no test fail. Mutant id."
    );
}

#[test]
fn review_summary_line_counts_survivors_and_inline_comments() {
    assert_eq!(
        mutation_review_summary_line(&survivors(), 4),
        "Mutation testing: score 62.5%, 6 survivors (4 inline)."
    );
    assert_eq!(
        mutation_review_summary_line(&ran(counts(3, 1, 0, 0), Some(0.75)), 1),
        "Mutation testing: score 75.0%, 1 survivor (1 inline)."
    );
    assert_eq!(
        mutation_review_summary_line(&partial_time_budget(), 0),
        "Mutation testing: partial run, score 75.0%, 10 survivors (0 inline)."
    );
    assert_eq!(
        mutation_review_summary_line(&empty_scope(), 0),
        "Mutation testing: no score, 0 survivors (0 inline)."
    );
    assert_eq!(
        mutation_review_summary_line(&not_run(MutationNotRunReason::EventRefused), 0),
        "Mutation testing: not run."
    );
    assert_eq!(
        mutation_review_summary_line(&MutationCiView::new(MutationCiState::Interrupted), 0),
        "Mutation testing: stopped."
    );
    assert_eq!(
        mutation_review_summary_line(&unreadable(), 0),
        "Mutation testing: failed."
    );
}

#[test]
fn run_notice_states_failures_and_partial_runs() {
    assert_eq!(mutation_run_notice(&clean()), None);
    assert_eq!(
        mutation_run_notice(&not_run(MutationNotRunReason::ForkRefused)),
        None
    );
    assert_eq!(
        mutation_run_notice(&partial_time_budget()),
        Some(MutationRunNotice {
            level: PrDecisionAnnotationLevel::Notice,
            title: "Mutation testing",
            message: "Partial run (time-budget). The annotations cover only the mutants that ran."
                .to_owned(),
        })
    );
    assert_eq!(
        mutation_run_notice(&failed("Runner exited.", None)),
        Some(MutationRunNotice {
            level: PrDecisionAnnotationLevel::Failure,
            title: "Mutation testing",
            message:
                "Mutation testing failed (exit code 2, FALLOW_MUTATION_RUNNER): Runner exited."
                    .to_owned(),
        })
    );
    let long = mutation_run_notice(&failed(&"m".repeat(3000), None)).map(|notice| notice.message);
    assert_eq!(
        long,
        Some(format!(
            "Mutation testing failed (exit code 2, FALLOW_MUTATION_RUNNER): {} ... (shortened)",
            "m".repeat(MUTATION_MESSAGE_MAX_CHARS)
        ))
    );
    assert_eq!(
        mutation_run_notice(&MutationCiView::new(MutationCiState::Interrupted))
            .map(|notice| notice.message),
        Some(
            "Mutation testing stopped. A signal stopped the run, and the run wrote no report."
                .to_owned()
        )
    );
    assert_eq!(
        mutation_run_notice(&unreadable()).map(|notice| notice.message),
        Some(
            "Fallow could not read the mutation report (FALLOW_MUTATION_REPORT_INVALID)."
                .to_owned()
        )
    );
}
