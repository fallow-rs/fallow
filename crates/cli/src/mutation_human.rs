//! Human output of `fallow mutation`.
//!
//! The renderers return plain text, so the snapshot tests see the exact
//! output. The report holds no source text; the survivor lines show
//! `original -> replacement` only when the caller reproduced the code from an
//! unchanged file.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use fallow_output::{
    MutantResult, MutantStatus, MutationCompletionStatus, MutationInspectOutput,
    MutationInspectStatus, MutationPlanDisposition, MutationPlanOutput, MutationReport,
    MutationScopeKind, MutationScopeOutput, MutationStaleReason,
};
use rustc_hash::FxHashMap;

/// The longest code fragment that a survivor line shows.
const MAX_FRAGMENT_CHARS: usize = 60;
/// The most covering tests that a survivor line names.
const MAX_TESTS_SHOWN: usize = 3;
/// The most planned mutants that the plan output lists.
const MAX_PLAN_MUTANTS_SHOWN: usize = 20;

/// The reproduced code of a mutant: the original source line and the same
/// line with the replacement applied, both trimmed.
pub type CodeLookup = FxHashMap<String, (String, String)>;

/// The options of the human run report.
pub struct RunReportOptions<'a> {
    /// The minimum score in percent, from the flag or the config.
    pub min_score: Option<f64>,
    /// The reproduced code of the survivors and the unreached mutants.
    pub code: &'a CodeLookup,
    /// The command that saves a JSON report of the same run, for `inspect`.
    pub save_command: &'a str,
}

fn scope_line(scope: &MutationScopeOutput) -> String {
    match (scope.kind, scope.base_ref.as_deref()) {
        (MutationScopeKind::Changed, Some(base)) => format!("changed code against {base}"),
        (MutationScopeKind::Changed, None) => "changed code of the diff".to_owned(),
        (MutationScopeKind::All, _) => "all source files".to_owned(),
        (MutationScopeKind::Paths, _) => "the given paths".to_owned(),
    }
}

/// The line for a scope with no mutant. A changed-code scope also says how
/// to widen the scope.
fn no_data_line(scope: &MutationScopeOutput) -> String {
    match scope.kind {
        MutationScopeKind::Changed => format!(
            "No mutation data: the {} has no mutant. Use --all or pass paths to widen the scope.\n",
            scope_line(scope)
        ),
        MutationScopeKind::All | MutationScopeKind::Paths => {
            format!("No mutation data: {} have no mutant.\n", scope_line(scope))
        }
    }
}

fn percent(score: Option<f64>) -> String {
    score.map_or_else(
        || "n/a".to_owned(),
        |score| format!("{:.1}%", score * 100.0),
    )
}

fn one_line(text: &str) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_FRAGMENT_CHARS {
        return collapsed;
    }
    let mut cut: String = collapsed.chars().take(MAX_FRAGMENT_CHARS - 3).collect();
    cut.push_str("...");
    cut
}

fn code_fragment(code: &CodeLookup, id: &str) -> Option<String> {
    code.get(id).map(|(original, replacement)| {
        let replacement = if replacement.trim().is_empty() {
            "(empty line)".to_owned()
        } else {
            one_line(replacement)
        };
        format!("{} -> {replacement}", one_line(original))
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
            let pending = report.summary.counts.pending;
            if pending > 0 {
                format!("partial ({}, {pending} pending)", reasons.join(", "))
            } else {
                format!("partial ({})", reasons.join(", "))
            }
        }
    }
}

/// Picks the noun form for `count`.
const fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

/// The summary line: the main counts always, the rare counts only when they
/// are not zero.
fn summary_line(report: &MutationReport) -> String {
    let counts = &report.summary.counts;
    let mut line = format!(
        "Score {}  killed {}  survived {}  no coverage {}  timeout {}",
        percent(report.summary.score),
        counts.killed,
        counts.survived,
        counts.no_coverage,
        counts.timeout
    );
    for (label, count) in [
        ("equivalent", counts.equivalent),
        ("runtime error", counts.runtime_error),
        ("flaky", counts.flaky),
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

/// The mutants with `status`, grouped by file. Files are in the order of the
/// best rank of their mutants; mutants are in rank order.
fn grouped(report: &MutationReport, status: MutantStatus) -> Vec<(&str, Vec<&MutantResult>)> {
    let mut files: BTreeMap<&str, Vec<&MutantResult>> = BTreeMap::new();
    for mutant in report
        .mutants
        .iter()
        .filter(|mutant| mutant.status == status)
    {
        files
            .entry(mutant.location.file.as_str())
            .or_default()
            .push(mutant);
    }
    let rank = |mutant: &MutantResult| mutant.location.rank.unwrap_or(u32::MAX);
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

fn render_survivors(out: &mut String, report: &MutationReport, code: &CodeLookup) {
    let groups = grouped(report, MutantStatus::Survived);
    if groups.is_empty() {
        return;
    }
    let _ = writeln!(out, "\nSurvived ({})", report.summary.counts.survived);
    for (file, mutants) in groups {
        let _ = writeln!(out, "  {file}");
        for mutant in mutants {
            let rank = mutant
                .location
                .rank
                .map_or_else(String::new, |rank| format!("#{rank} "));
            let _ = write!(out, "    {rank}{}", mutant_head(mutant));
            if let Some(fragment) = code_fragment(code, &mutant.location.mutant_id) {
                let _ = write!(out, "  {fragment}");
            }
            let _ = writeln!(out);
            let _ = writeln!(out, "      id {}", mutant.location.mutant_id);
            if !mutant.covering_tests.is_empty() {
                let shown: Vec<&str> = mutant
                    .covering_tests
                    .iter()
                    .take(MAX_TESTS_SHOWN)
                    .map(String::as_str)
                    .collect();
                let more = mutant.covering_tests.len().saturating_sub(MAX_TESTS_SHOWN);
                let suffix = if more > 0 {
                    format!(" (+{more} more)")
                } else {
                    String::new()
                };
                let _ = writeln!(out, "      covered by {}{suffix}", shown.join(", "));
            }
        }
    }
}

fn render_no_coverage(out: &mut String, report: &MutationReport, code: &CodeLookup) {
    let groups = grouped(report, MutantStatus::NoCoverage);
    if groups.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "\nNo coverage ({}): no test reaches these mutants",
        report.summary.counts.no_coverage
    );
    for (file, mutants) in groups {
        let _ = writeln!(out, "  {file}");
        for mutant in mutants {
            let _ = write!(out, "    {}", mutant_head(mutant));
            if let Some(fragment) = code_fragment(code, &mutant.location.mutant_id) {
                let _ = write!(out, "  {fragment}");
            }
            let _ = writeln!(out);
        }
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

fn render_footer_lines(out: &mut String, report: &MutationReport) {
    let pruned = prune_counts(
        report
            .mutants
            .iter()
            .filter_map(|mutant| mutant.prune_reason.as_deref()),
    );
    if !pruned.is_empty() {
        let _ = writeln!(out, "\nPruned: {pruned}");
    }
    if let Some(verdicts) = &report.verdicts {
        let _ = writeln!(
            out,
            "Verdicts: {} equivalent, {} contradicted by a test, {} deferred, {} unmatched",
            verdicts.applied.len(),
            verdicts.contradicted.len(),
            verdicts.deferred.len(),
            verdicts.unmatched.len()
        );
    }
    if !report.baseline_failures.is_empty() {
        let _ = writeln!(
            out,
            "Baseline failures: {} {} with no active mutant and decide no mutant",
            report.baseline_failures.len(),
            plural(
                report.baseline_failures.len(),
                "test failed",
                "tests failed"
            )
        );
    }
    for error in &report.errors {
        let _ = writeln!(out, "Error ({}): {}", error.code, error.message);
    }
    if !report.worktree.changed_paths.is_empty() {
        let _ = writeln!(
            out,
            "Worktree changed during the run: {}",
            report.worktree.changed_paths.join(", ")
        );
    }
}

/// Renders the human report of a run.
pub fn render_run(report: &MutationReport, options: &RunReportOptions<'_>) -> String {
    let counts = &report.summary.counts;
    let tested =
        counts.killed + counts.survived + counts.timeout + counts.runtime_error + counts.flaky;
    let mut out = if report.runner.is_some() || tested > 0 {
        String::from("Mutation testing (ran the project tests)\n")
    } else {
        String::from("Mutation testing (no test ran)\n")
    };
    let _ = writeln!(out, "Scope: {}", scope_line(&report.scope));
    let _ = writeln!(out, "{}", summary_line(report));
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
    if let Some(min_score) = options.min_score {
        let verdict = match report.summary.score {
            Some(score) if score * 100.0 < min_score => "missed",
            Some(_) => "met",
            None => "not measured",
        };
        let _ = writeln!(out, "Minimum score {min_score}%: {verdict}");
    }
    render_survivors(&mut out, report, options.code);
    if !report.no_test_files {
        render_no_coverage(&mut out, report, options.code);
    }
    render_footer_lines(&mut out, report);
    if let Some(first) = report
        .mutants
        .iter()
        .filter(|mutant| mutant.status == MutantStatus::Survived)
        .min_by_key(|mutant| mutant.location.rank.unwrap_or(u32::MAX))
    {
        let _ = writeln!(
            out,
            "\nNext\n  Save a JSON report (runs the tests again): {}\n  Inspect a survivor (runs no code): fallow mutation inspect {} --report mutation.json",
            options.save_command, first.location.mutant_id
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
        "Files: {} mutated, {} {} them",
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
    let planned: Vec<_> = plan
        .mutants
        .iter()
        .filter(|mutant| mutant.disposition == MutationPlanDisposition::Planned)
        .collect();
    if !planned.is_empty() {
        let _ = writeln!(out, "\nPlanned, in run order");
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
                location.file,
                location.line,
                location.column,
                location.operator,
                location.mutant_id
            );
        }
        if planned.len() > MAX_PLAN_MUTANTS_SHOWN {
            let _ = writeln!(
                out,
                "  ... {} more (use --format json)",
                planned.len() - MAX_PLAN_MUTANTS_SHOWN
            );
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
        location.file, location.line, location.column
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
            let _ = writeln!(out, "- {:>4} | {}", location.line, code.original_lines);
            let _ = writeln!(out, "+ {:>4} | {}", location.line, code.mutated_lines);
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
        let _ = writeln!(out, "Covering tests:");
        for test in &packet.covering_tests {
            let _ = writeln!(out, "  {test}");
        }
    } else if !packet.static_test_files.is_empty() {
        let _ = writeln!(
            out,
            "Test files that reach the file: {}",
            packet.static_test_files.join(", ")
        );
    }
    if let Some(killed_by) = &packet.killed_by {
        let _ = writeln!(out, "Killed by: {killed_by}");
    }
    out
}
