//! Human output of `fallow mutation`.
//!
//! The renderers return text with the color codes of the other commands
//! (`colored` drops them for `NO_COLOR`, a pipe and `--output-file`). The
//! report holds no source text; the survivor lines show `original ->
//! replacement` only when the caller reproduced the code from an unchanged
//! file.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use colored::Colorize as _;
use fallow_output::{
    MutantResult, MutantStatus, MutationCompletionStatus, MutationInspectOutput,
    MutationInspectStatus, MutationPlanDisposition, MutationPlanOutput, MutationReport,
    MutationScopeKind, MutationScopeOutput, MutationSkippedFile, MutationStaleReason,
    MutationTypecheckOutput, TestReachClass,
};
use rustc_hash::FxHashMap;

use crate::report::{HumanStatus, Level, build_section_header, format_path, human_status_line};

/// The longest code fragment that a survivor line shows.
const MAX_FRAGMENT_CHARS: usize = 60;
/// The marker of a cut at the start or the end of a fragment.
const ELLIPSIS: &str = "...";
/// The longest covering-test name that a survivor line shows.
const MAX_TEST_NAME_CHARS: usize = 60;
/// The length of a full hex commit SHA.
const FULL_SHA_CHARS: usize = 40;
/// The length of a shortened commit SHA in the scope line.
const SHORT_SHA_CHARS: usize = 12;
/// The most planned mutants that the plan output lists.
const MAX_PLAN_MUTANTS_SHOWN: usize = 20;
/// The most files that the no-coverage section lists.
const MAX_NO_COVERAGE_FILES: usize = 10;
/// The most covering tests and test files that the inspect output lists.
const MAX_INSPECT_ITEMS_SHOWN: usize = 10;
/// The most survivors that the run report lists. The grouped sections of
/// the other commands use the same cap.
const MAX_SURVIVORS_SHOWN: usize = 10;
/// The runner note for a type-check pass that the run did not execute.
const TYPECHECK_NOT_RUN_NOTE: &str = "typecheck-not-run";

/// The source line of a mutant in parts: the text before the span, the span,
/// its replacement and the text after the span. The parts let a long line
/// keep the change in view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeExcerpt {
    pub before: String,
    pub original: String,
    pub replacement: String,
    pub after: String,
}

/// The reproduced code of a mutant, by mutant id.
pub type CodeLookup = FxHashMap<String, CodeExcerpt>;

/// The options of the human run report.
pub struct RunReportOptions<'a> {
    /// The minimum score in percent, from the flag or the config.
    pub min_score: Option<f64>,
    /// The reproduced code of the survivors and the unreached mutants.
    pub code: Option<&'a CodeLookup>,
    /// The global flags that `inspect` needs to find the saved report, for
    /// example ` --root packages/app`. `None` when the run saved no report.
    pub inspect_flags: Option<&'a str>,
}

/// Picks the noun form for `count`.
const fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

/// Cuts a full 40-character hex SHA to 12 characters, as `audit` does. A
/// branch name or another ref stays as it is.
fn short_base_ref(base_ref: &str) -> &str {
    if base_ref.len() == FULL_SHA_CHARS && base_ref.bytes().all(|b| b.is_ascii_hexdigit()) {
        &base_ref[..SHORT_SHA_CHARS]
    } else {
        base_ref
    }
}

fn scope_line(scope: &MutationScopeOutput) -> String {
    match (scope.kind, scope.base_ref.as_deref()) {
        (MutationScopeKind::Changed, Some(base)) => {
            format!("changed code against {}", short_base_ref(base))
        }
        (MutationScopeKind::Changed, None) => "changed code of the diff".to_owned(),
        (MutationScopeKind::All, _) => "all source files".to_owned(),
        (MutationScopeKind::Paths, _) => "the given paths".to_owned(),
    }
}

/// The line for a scope with no mutant. The scope line above already names
/// the base ref. A changed-code scope also names the read-only plan of each
/// file.
fn no_data_line(scope: &MutationScopeOutput) -> String {
    match scope.kind {
        MutationScopeKind::Changed => "No mutation data: the changed code has no mutant. `fallow mutation --plan --all` lists the mutants of each file and runs no code.\n".to_owned(),
        MutationScopeKind::All | MutationScopeKind::Paths => {
            format!("No mutation data: {} have no mutant.\n", scope_line(scope))
        }
    }
}

/// The line under a capped list: the count of hidden items and where the
/// full list is.
fn more_line(hidden: usize, one: &str, many: &str) -> String {
    format!(
        "  and {hidden} more {} (see --format json)",
        plural(hidden, one, many)
    )
}

/// `text` cut to `max` characters, with an ellipsis at the cut.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max - ELLIPSIS.len()).collect();
    format!("{kept}{ELLIPSIS}")
}

fn percent(score: Option<f64>) -> String {
    score.map_or_else(
        || "n/a".to_owned(),
        |score| format!("{:.1}%", score * 100.0),
    )
}

/// Collapses each whitespace run to one space. A run at an edge stays one
/// space, so the joined parts keep their separation.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    out
}

/// The length of the common prefix of `a` and `b`.
fn common_prefix(a: &[char], b: &[char]) -> usize {
    a.iter()
        .zip(b)
        .take_while(|(left, right)| left == right)
        .count()
}

/// The original and the mutated line on one line each, at most
/// `MAX_FRAGMENT_CHARS` characters. A long line shows one window around the
/// changed token, the text that differs between the original and the
/// replacement. Both sides use the same window start, so they stay aligned.
fn fragments(excerpt: &CodeExcerpt) -> (String, String) {
    let before: Vec<char> = collapse(&excerpt.before).trim_start().chars().collect();
    let original: Vec<char> = collapse(&excerpt.original).chars().collect();
    let replacement: Vec<char> = collapse(&excerpt.replacement).chars().collect();
    let after: Vec<char> = collapse(&excerpt.after).trim_end().chars().collect();
    let prefix = common_prefix(&original, &replacement);
    let rest_original: Vec<char> = original[prefix..].iter().rev().copied().collect();
    let rest_replacement: Vec<char> = replacement[prefix..].iter().rev().copied().collect();
    let suffix = common_prefix(&rest_original, &rest_replacement);
    let change_start = before.len() + prefix;
    let change_len = original.len().max(replacement.len()) - prefix - suffix;
    let line = |changed: &[char]| -> Vec<char> {
        before
            .iter()
            .chain(changed)
            .chain(&after)
            .copied()
            .collect()
    };
    let lines = [line(&original), line(&replacement)];
    let longest = lines.iter().map(Vec::len).max().unwrap_or(0);
    if longest <= MAX_FRAGMENT_CHARS {
        let [original, mutated] = lines;
        return (
            original.into_iter().collect(),
            mutated.into_iter().collect(),
        );
    }
    let width = MAX_FRAGMENT_CHARS - 2 * ELLIPSIS.len();
    let window_start = if change_len >= width {
        change_start
    } else {
        (change_start + change_len / 2)
            .saturating_sub(width / 2)
            .min(longest - width)
    };
    lines
        .map(|line| {
            let start = window_start.min(line.len());
            let end = (start + width).min(line.len());
            let mut out = String::new();
            if start > 0 {
                out.push_str(ELLIPSIS);
            }
            out.extend(&line[start..end]);
            if end < line.len() {
                out.push_str(ELLIPSIS);
            }
            out
        })
        .into()
}

fn code_fragment(code: Option<&CodeLookup>, id: &str) -> Option<String> {
    code?.get(id).map(|excerpt| {
        let (original, mutated) = fragments(excerpt);
        let mutated = if mutated.trim().is_empty() {
            "(empty line)".to_owned()
        } else {
            mutated
        };
        format!("{original} -> {mutated}")
    })
}

fn completion_text(report: &MutationReport) -> String {
    match report.completion.status {
        MutationCompletionStatus::Complete => "complete".to_owned(),
        MutationCompletionStatus::Partial => {
            let reasons: Vec<String> = report
                .completion
                .reasons
                .iter()
                .map(|reason| {
                    serde_json::to_value(reason)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .unwrap_or_default()
                })
                .collect();
            format!("partial ({})", reasons.join(", "))
        }
    }
}

/// The score line: the score over the executed mutants and the main counts
/// always, the rare counts only when they are not zero.
fn summary_line(report: &MutationReport) -> String {
    let counts = &report.summary.counts;
    let mut line = format!(
        "Score {}  killed {}  survived {}  timeout {}",
        percent(report.summary.score),
        counts.killed,
        counts.survived,
        counts.timeout
    );
    for (label, count) in [
        ("pending", counts.pending),
        ("equivalent", counts.equivalent),
        ("runtime error", counts.runtime_error),
        ("flaky", counts.flaky),
        ("compile error", counts.compile_error),
    ] {
        if count > 0 {
            let _ = write!(line, "  {label} {count}");
        }
    }
    let _ = write!(line, "  {}", completion_text(report));
    line
}

fn mutant_head(mutant: &MutantResult) -> String {
    let location = &mutant.location;
    let mut head = format!("{}:{}", location.line, location.column);
    if let Some(function) = &location.function {
        let _ = write!(head, "  {function}");
    }
    let _ = write!(head, "  {}", location.operator);
    head
}

/// The `limit` best-ranked mutants with `status`, grouped by file. Files
/// are in the order of the best rank of their mutants; mutants are in rank
/// order.
fn grouped(
    report: &MutationReport,
    status: MutantStatus,
    limit: usize,
) -> Vec<(&str, Vec<&MutantResult>)> {
    let rank = |mutant: &MutantResult| mutant.location.rank.unwrap_or(u32::MAX);
    let mut selected: Vec<&MutantResult> = report
        .mutants
        .iter()
        .filter(|mutant| mutant.status == status)
        .collect();
    selected.sort_by(|a, b| {
        (rank(a), &a.location.file, a.location.line).cmp(&(
            rank(b),
            &b.location.file,
            b.location.line,
        ))
    });
    selected.truncate(limit);
    let mut files: BTreeMap<&str, Vec<&MutantResult>> = BTreeMap::new();
    for mutant in selected {
        files
            .entry(mutant.location.file.as_str())
            .or_default()
            .push(mutant);
    }
    let mut groups: Vec<(&str, Vec<&MutantResult>)> = files
        .into_iter()
        .map(|(file, mut mutants)| {
            mutants.sort_by_key(|mutant| (rank(mutant), mutant.location.line));
            (file, mutants)
        })
        .collect();
    groups.sort_by_key(|(file, mutants)| (mutants.first().map_or(u32::MAX, |m| rank(m)), *file));
    groups
}

/// The covering tests of a survivor on one short line: the count and the
/// first test name, cut to `MAX_TEST_NAME_CHARS`. The full list is in the
/// JSON report and in `inspect`.
fn covered_by_line(tests: &[String]) -> Option<String> {
    let name = shorten(tests.first()?, MAX_TEST_NAME_CHARS);
    Some(if tests.len() == 1 {
        format!("covered by 1 test: {name}")
    } else {
        format!("covered by {} tests, first: {name}", tests.len())
    })
}

fn render_survivors(out: &mut String, report: &MutationReport, code: Option<&CodeLookup>) {
    let groups = grouped(report, MutantStatus::Survived, MAX_SURVIVORS_SHOWN);
    if groups.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "\n{}",
        build_section_header("Survived", report.summary.counts.survived, Level::Error)
    );
    let mut shown = 0;
    for (file, mutants) in groups {
        let _ = writeln!(out, "  {}", format_path(file));
        for mutant in mutants {
            shown += 1;
            let rank = mutant
                .location
                .rank
                .map_or_else(String::new, |rank| format!("#{rank} "));
            let _ = write!(out, "    {rank}{}", mutant_head(mutant));
            if let Some(fragment) = code_fragment(code, &mutant.location.mutant_id) {
                let _ = write!(out, "  {fragment}");
            }
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "      {}",
                format!("id {}", mutant.location.mutant_id).dimmed()
            );
            if let Some(line) = covered_by_line(&mutant.covering_tests) {
                let _ = writeln!(out, "      {}", line.dimmed());
            }
        }
    }
    let hidden = report.summary.counts.survived.saturating_sub(shown);
    if hidden > 0 {
        let _ = writeln!(out, "{}", more_line(hidden, "survivor", "survivors"));
    }
}

/// The no-coverage section: one line per file with its count and the counts
/// by test reach class, the files with the most mutants first, capped. The
/// full list is in the JSON report.
fn render_no_coverage(out: &mut String, report: &MutationReport) {
    let mut files: BTreeMap<&str, Vec<&MutantResult>> = BTreeMap::new();
    for mutant in report
        .mutants
        .iter()
        .filter(|mutant| mutant.status == MutantStatus::NoCoverage)
    {
        files
            .entry(mutant.location.file.as_str())
            .or_default()
            .push(mutant);
    }
    if files.is_empty() {
        return;
    }
    let mut files: Vec<(&str, usize, String)> = files
        .into_iter()
        .map(|(file, mutants)| {
            let reach = reach_counts(
                mutants
                    .iter()
                    .filter_map(|mutant| mutant.test_reach.as_ref())
                    .map(|reach| reach.reach),
            );
            (file, mutants.len(), reach)
        })
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let _ = writeln!(
        out,
        "\n{}: no test reaches these mutants",
        build_section_header(
            "No coverage",
            report.summary.counts.no_coverage,
            Level::Warn
        )
    );
    for (file, count, reach) in files.iter().take(MAX_NO_COVERAGE_FILES) {
        let reach = if reach.is_empty() {
            String::new()
        } else {
            format!("  {}", format!("test reach: {reach}").dimmed())
        };
        let _ = writeln!(
            out,
            "  {}  {count} {}{reach}",
            format_path(file),
            plural(*count, "mutant", "mutants")
        );
    }
    let hidden = files.len().saturating_sub(MAX_NO_COVERAGE_FILES);
    if hidden > 0 {
        let _ = writeln!(out, "{}", more_line(hidden, "file", "files"));
    }
}

/// The test reach classes with their counts, in the order of
/// [`TestReachClass`]: `2 mocked-only, 1 none-file`.
fn reach_counts(classes: impl Iterator<Item = TestReachClass>) -> String {
    let mut counts: BTreeMap<TestReachClass, usize> = BTreeMap::new();
    for class in classes {
        *counts.entry(class).or_default() += 1;
    }
    counts
        .iter()
        .map(|(class, count)| format!("{count} {}", reach_label(*class)))
        .collect::<Vec<_>>()
        .join(", ")
}

const fn reach_label(class: TestReachClass) -> &'static str {
    match class {
        TestReachClass::MockedOnly => "mocked-only",
        TestReachClass::None => "none",
        TestReachClass::NoneFile => "none-file",
        TestReachClass::Transitive => "transitive",
        TestReachClass::Direct => "direct",
        TestReachClass::Framework => "framework",
    }
}

fn prune_counts<'a>(reasons: impl Iterator<Item = &'a str>) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for reason in reasons {
        *counts.entry(reason).or_default() += 1;
    }
    counts
        .iter()
        .map(|(reason, count)| format!("{count} {reason}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The skip reason of a component file under `node:test` or `bun test`.
const RUNNER_CANNOT_LOAD: &str = "runner-cannot-load";

/// One line with the count of each skip reason, or nothing. A
/// `runner-cannot-load` skip adds the next step.
fn write_skipped_files(out: &mut String, skipped: &[MutationSkippedFile]) {
    let counts = prune_counts(skipped.iter().map(|file| file.reason.as_str()));
    if !counts.is_empty() {
        let _ = writeln!(out, "Skipped files: {counts} (see --format json)");
    }
    if skipped.iter().any(|file| file.reason == RUNNER_CANNOT_LOAD) {
        out.push_str(
            "Node.js and Bun cannot load a component file. Set mutation.runner to vitest to test component files.\n",
        );
    }
}

fn render_footer_lines(out: &mut String, report: &MutationReport) {
    let mut footer = String::new();
    let pruned = prune_counts(
        report
            .mutants
            .iter()
            .filter_map(|mutant| mutant.prune_reason.as_deref()),
    );
    if !pruned.is_empty() {
        let _ = writeln!(footer, "Pruned: {pruned}");
    }
    write_skipped_files(&mut footer, &report.skipped_files);
    if let Some(verdicts) = &report.verdicts {
        let _ = writeln!(
            footer,
            "Verdicts: {} equivalent, {} contradicted by a test, {} deferred, {} unmatched",
            verdicts.applied.len(),
            verdicts.contradicted.len(),
            verdicts.deferred.len(),
            verdicts.unmatched.len()
        );
    }
    if let Some(first) = report.baseline_failures.first() {
        let name = shorten(
            first
                .test_id
                .as_deref()
                .or(first.file.as_deref())
                .or(first.message.as_deref())
                .unwrap_or("a test file"),
            MAX_TEST_NAME_CHARS,
        );
        let failed = report.baseline_failures.len();
        if failed == 1 {
            let _ = writeln!(
                footer,
                "Baseline failure: {name} fails with no active mutant, so no mutant uses its result."
            );
        } else {
            let _ = writeln!(
                footer,
                "Baseline failures: {failed} tests fail with no active mutant, so no mutant uses their results. First: {name} (see --format json)"
            );
        }
    }
    for error in &report.errors {
        match &error.mutant_id {
            Some(id) => {
                let _ = writeln!(
                    footer,
                    "Error ({}), mutant {id}: {}",
                    error.code, error.message
                );
            }
            None => {
                let _ = writeln!(footer, "Error ({}): {}", error.code, error.message);
            }
        }
    }
    if !report.worktree.changed_paths.is_empty() {
        let _ = writeln!(
            footer,
            "Worktree changed during the run: {}",
            report.worktree.changed_paths.join(", ")
        );
    }
    if !footer.is_empty() {
        out.push('\n');
        out.push_str(&footer);
    }
}

/// The lines under the score line: the coverage, the empty states, the
/// minimum score and the runner facts.
fn render_status_lines(out: &mut String, report: &MutationReport, min_score: Option<f64>) {
    let counts = &report.summary.counts;
    if let Some(coverage) = report.summary.coverage
        && !report.no_test_files
    {
        let _ = writeln!(
            out,
            "Coverage {}  no coverage {}",
            percent(Some(coverage)),
            counts.no_coverage
        );
    }
    if report.no_test_files && counts.no_coverage > 0 {
        let _ = writeln!(
            out,
            "No tests: the project has no test files, so the {} {} no coverage.",
            counts.no_coverage,
            plural(counts.no_coverage, "mutant has", "mutants have")
        );
    } else if report.no_test_files {
        out.push_str("No tests: the project has no test files, so no mutant can be killed.\n");
    } else if counts.total == 0 {
        out.push_str(&no_data_line(&report.scope));
    }
    if let Some(min_score) = min_score {
        let line = match report.summary.score {
            Some(score) if score * 100.0 < min_score => human_status_line(
                HumanStatus::Failure,
                format!("Minimum score {min_score}%: missed"),
            )
            .red()
            .bold()
            .to_string(),
            Some(_) => {
                human_status_line(HumanStatus::Ok, format!("Minimum score {min_score}%: met"))
                    .green()
                    .to_string()
            }
            None => human_status_line(
                HumanStatus::Inactive,
                format!("Minimum score {min_score}%: not measured"),
            )
            .dimmed()
            .to_string(),
        };
        let _ = writeln!(out, "{line}");
    }
    let typecheck_not_run = report.runner.as_ref().is_some_and(|runner| {
        runner
            .notes
            .iter()
            .any(|note| note == TYPECHECK_NOT_RUN_NOTE)
    });
    let survivors_checked = report
        .typecheck
        .as_ref()
        .is_some_and(|typecheck| typecheck.completed);
    if typecheck_not_run {
        out.push_str(if survivors_checked {
            "Type check: the project enables the Vitest type-check pass, and this run did not run it. Only type errors in the mutated file are counted.\n"
        } else {
            "Type check: the project enables the Vitest type-check pass, and this run did not run it. Type-level kills are not counted.\n"
        });
    }
    if let Some(typecheck) = &report.typecheck {
        let _ = writeln!(out, "{}", typecheck_line(typecheck));
    }
    if let Some(cache) = report.cache.as_ref().filter(|cache| cache.hits > 0) {
        let _ = writeln!(
            out,
            "Cache: {} {} from an earlier run. Pass --no-cache to run each mutant.",
            cache.hits,
            plural(cache.hits, "result came", "results came")
        );
    }
}

/// The line of the survivor type check (`--typecheck`).
fn typecheck_line(typecheck: &MutationTypecheckOutput) -> String {
    if !typecheck.completed {
        return format!(
            "Survivor type check: failed, so the {} {} keep the status survived",
            typecheck.survivors,
            plural(typecheck.survivors, "survivor", "survivors")
        );
    }
    let mut line = format!(
        "Survivor type check: {} checked, {} {} not type-check",
        typecheck.checked,
        typecheck.compile_errors,
        plural(typecheck.compile_errors, "does", "do")
    );
    if !typecheck.skipped.is_empty() {
        let reasons: Vec<String> = typecheck
            .skipped
            .iter()
            .map(|skip| {
                let reason = serde_json::to_value(skip.reason)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default();
                format!("{reason} {}", skip.count)
            })
            .collect();
        let skipped: usize = typecheck.skipped.iter().map(|skip| skip.count).sum();
        let _ = write!(line, ", {skipped} not checked ({})", reasons.join(", "));
    }
    line
}

/// Renders the human report of a run.
pub fn render_run(report: &MutationReport, options: &RunReportOptions<'_>) -> String {
    let counts = &report.summary.counts;
    let tested = counts.killed
        + counts.survived
        + counts.timeout
        + counts.runtime_error
        + counts.flaky
        + counts.compile_error;
    let mut out = if report.runner.is_some() || tested > 0 {
        String::from("Mutation testing (ran the project tests)\n")
    } else {
        String::from("Mutation testing (no test ran)\n")
    };
    let _ = writeln!(out, "Scope: {}", scope_line(&report.scope));
    let _ = writeln!(out, "{}", summary_line(report));
    render_status_lines(&mut out, report, options.min_score);
    render_survivors(&mut out, report, options.code);
    if !report.no_test_files {
        render_no_coverage(&mut out, report);
    }
    render_footer_lines(&mut out, report);
    if let Some(flags) = options.inspect_flags
        && let Some(first) = report
            .mutants
            .iter()
            .filter(|mutant| mutant.status == MutantStatus::Survived)
            .min_by_key(|mutant| mutant.location.rank.unwrap_or(u32::MAX))
    {
        let _ = writeln!(
            out,
            "\nNext\n  Inspect a survivor (runs no code): fallow mutation inspect {}{flags}",
            first.location.mutant_id
        );
    }
    out
}

/// Renders the human output of `--plan`.
pub fn render_plan(plan: &MutationPlanOutput) -> String {
    let counts = &plan.counts;
    let mut out = String::from("Mutation plan (no code runs)\n");
    let _ = writeln!(out, "Scope: {}", scope_line(&plan.scope));
    let pruned = counts.pruned_dead_code
        + counts.pruned_generated
        + counts.pruned_side_effect_sink
        + counts.pruned_framework_config;
    let _ = writeln!(
        out,
        "Mutants: {} planned, {} pending, {} no coverage, {pruned} pruned",
        counts.planned, counts.pending, counts.no_coverage
    );
    let _ = writeln!(
        out,
        "Files: {} instrumented, {} {} them",
        plan.files.len(),
        plan.test_files.len(),
        plural(
            plan.test_files.len(),
            "test file reaches",
            "test files reach"
        )
    );
    if plan.no_test_files {
        out.push_str("No tests: the project has no test files, so no mutant can be killed.\n");
    } else if counts.total == 0 {
        out.push_str(&no_data_line(&plan.scope));
    }
    let pruned_by_reason = prune_counts(
        plan.mutants
            .iter()
            .filter_map(|mutant| mutant.prune_reason.as_deref()),
    );
    if !pruned_by_reason.is_empty() {
        let _ = writeln!(out, "Pruned: {pruned_by_reason}");
    }
    write_skipped_files(&mut out, &plan.skipped_files);
    let no_coverage_reach = reach_counts(
        plan.mutants
            .iter()
            .filter(|mutant| mutant.disposition == MutationPlanDisposition::NoCoverage)
            .filter_map(|mutant| mutant.test_reach.as_ref())
            .map(|reach| reach.reach),
    );
    if !plan.no_test_files && !no_coverage_reach.is_empty() {
        let _ = writeln!(out, "No coverage by test reach: {no_coverage_reach}");
    }
    let planned: Vec<_> = plan
        .mutants
        .iter()
        .filter(|mutant| mutant.disposition == MutationPlanDisposition::Planned)
        .collect();
    if !planned.is_empty() {
        let _ = writeln!(
            out,
            "\n{}",
            build_section_header("Planned", planned.len(), Level::Info)
        );
        for mutant in planned.iter().take(MAX_PLAN_MUTANTS_SHOWN) {
            let location = &mutant.location;
            let rank = location
                .rank
                .map_or_else(String::new, |rank| format!("#{rank} "));
            let function = location
                .function
                .as_deref()
                .map_or_else(String::new, |name| format!("  {name}"));
            let _ = writeln!(
                out,
                "  {rank}{}:{}:{}{function}  {}  {}",
                format_path(&location.file),
                location.line,
                location.column,
                location.operator,
                location.mutant_id
            );
        }
        let hidden = planned.len().saturating_sub(MAX_PLAN_MUTANTS_SHOWN);
        if hidden > 0 {
            let _ = writeln!(out, "{}", more_line(hidden, "mutant", "mutants"));
        }
    }
    out
}

const fn stale_text(reason: MutationStaleReason) -> &'static str {
    match reason {
        MutationStaleReason::FileMissing => "the file is missing",
        MutationStaleReason::NoDigest => "the report has no digest for the file",
        MutationStaleReason::SourceChanged => "the file changed after the report",
        MutationStaleReason::MutantNotReproduced => "the current file does not give this mutant",
    }
}

/// Writes each line of `text` with the `marker` and its own line number,
/// starting at `first_line`. A mutant can span more than one line.
fn write_diff_lines(out: &mut String, marker: char, first_line: u32, text: &str) {
    for (line, offset) in text.split('\n').zip(0u32..) {
        let row = format!(
            "{marker} {:>4} | {}",
            first_line.saturating_add(offset),
            line.trim_end_matches('\r')
        );
        let row = if marker == '-' {
            row.red()
        } else {
            row.green()
        };
        let _ = writeln!(out, "{row}");
    }
}

/// Renders the human output of `inspect`.
pub fn render_inspect(packet: &MutationInspectOutput) -> String {
    let location = &packet.location;
    let mut out = String::new();
    let status = match packet.status {
        MutationInspectStatus::Fresh => "fresh",
        MutationInspectStatus::Stale => "stale",
    };
    let _ = writeln!(out, "Mutant {}  {status}", location.mutant_id);
    let _ = write!(
        out,
        "{}:{}:{}",
        format_path(&location.file),
        location.line,
        location.column
    );
    if let Some(function) = &location.function {
        let _ = write!(out, "  {function}");
    }
    let _ = write!(out, "  {}", location.operator);
    if let Some(status) = packet.report_status {
        let status = serde_json::to_value(status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let _ = write!(out, "  (report: {status})");
    }
    out.push('\n');
    match (&packet.code, packet.stale_reason) {
        (Some(code), _) => {
            write_diff_lines(&mut out, '-', location.line, &code.original_lines);
            write_diff_lines(&mut out, '+', location.line, &code.mutated_lines);
        }
        (None, Some(reason)) => {
            let _ = writeln!(
                out,
                "Stale: {}. The report no longer describes this code.",
                stale_text(reason)
            );
        }
        (None, None) => {}
    }
    if !packet.covering_tests.is_empty() {
        let tests = &packet.covering_tests;
        let _ = writeln!(
            out,
            "\n{}",
            build_section_header("Covering tests", tests.len(), Level::Info)
        );
        for test in tests.iter().take(MAX_INSPECT_ITEMS_SHOWN) {
            let _ = writeln!(out, "  {test}");
        }
        let hidden = tests.len().saturating_sub(MAX_INSPECT_ITEMS_SHOWN);
        if hidden > 0 {
            let _ = writeln!(out, "{}", more_line(hidden, "test", "tests"));
        }
    } else if !packet.static_test_files.is_empty() {
        let files = &packet.static_test_files;
        let _ = writeln!(
            out,
            "\n{}",
            build_section_header("Test files that reach the file", files.len(), Level::Info)
        );
        for file in files.iter().take(MAX_INSPECT_ITEMS_SHOWN) {
            let _ = writeln!(out, "  {}", format_path(file));
        }
        let hidden = files.len().saturating_sub(MAX_INSPECT_ITEMS_SHOWN);
        if hidden > 0 {
            let _ = writeln!(out, "{}", more_line(hidden, "file", "files"));
        }
    }
    if let Some(killed_by) = &packet.killed_by {
        let _ = writeln!(out, "\nKilled by: {killed_by}");
    }
    out
}
