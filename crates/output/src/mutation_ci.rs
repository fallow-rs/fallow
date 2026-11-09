//! The mutation testing section of the CI surfaces.
//!
//! One pure renderer turns a [`MutationCiView`] into the markdown section of
//! the sticky comment, the review summary and the job summary, and into the
//! `mutation` row of the PR decision. No function here reads a file, the clock
//! or the environment, so the same view always gives the same bytes.
//!
//! A CI job writes the report after it ran project code, so each string in a
//! view is untrusted. Paths, function names, codes and the root go into code
//! spans with each line ending collapsed to a space. Messages go into a fenced
//! block that uses four backticks, and each run of three backticks in a message
//! becomes three single quotes, so no message can close the fence.

use crate::ci_output::markdown_code_span;
use crate::gate_outcomes::{GateOutcome, GateStatus};
use crate::mutation::{
    MutantChange, MutationCompletion, MutationCompletionStatus, MutationIncompleteReason,
    MutationScopeKind, MutationStatusCounts,
};
use crate::pr_decision::{PrDecisionAnnotationLevel, PrDecisionConclusion, PrDecisionGate};

/// The maximum number of characters of a rendered mutation section. A longer
/// section keeps only the heading, the headline and the count table.
pub const MUTATION_SECTION_MAX_CHARS: usize = 6000;

/// The maximum number of items in the list of lines that no test reaches.
pub const MUTATION_NO_COVERAGE_ITEMS: usize = 10;

/// The maximum number of characters of an error message or a help text.
pub const MUTATION_MESSAGE_MAX_CHARS: usize = 1000;

/// Why the CI job did not run mutation testing.
///
/// The CI wiring selects the reason, and this module owns the text of each
/// reason. A run that a signal stopped is [`MutationCiState::Interrupted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationNotRunReason {
    /// The event can run the code of a fork with the secrets of the base
    /// repository.
    EventRefused,
    /// The merge request comes from a fork, and the pipeline runs in the
    /// target project.
    ForkRefused,
    /// The changed scope needs a diff or a base ref, and the job has neither.
    NoChangeScope,
}

/// The state of the mutation run that a CI job renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationCiState {
    /// The run wrote a report.
    Ran,
    /// `fallow mutation` exited with an error envelope.
    Failed {
        /// The exit code of `fallow mutation`.
        exit_code: i32,
        /// The error code of the envelope, for example `FALLOW_X`.
        code: String,
        /// The error message of the envelope.
        message: String,
        /// The help text of the envelope, when it has one.
        help: Option<String>,
    },
    /// The CI job did not run mutation testing.
    NotRun(MutationNotRunReason),
    /// A signal stopped the run before it wrote a report.
    Interrupted,
    /// The report has `kind: "mutation"`, but its content does not parse.
    Unreadable {
        /// Why the parse failed.
        reason: String,
    },
}

/// One source line that no test reaches.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MutationCiLocation {
    /// The root-relative path, with forward slashes.
    pub file: String,
    /// The 1-based line.
    pub line: u32,
    /// The name of the enclosing function, when it has one.
    pub function: Option<String>,
}

/// The heading level of a rendered mutation section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationHeading {
    /// `####`, under the `### Fallow <title>` heading of the sticky comment.
    /// The comment has the findings tables, so the text points at them.
    Comment,
    /// `####`, in the summary body of an inline review. The survived mutants
    /// are inline comments there, not table rows.
    Review,
    /// `##`, in the job summary. Only this heading prints the elapsed time
    /// and the cache hits, because a job summary belongs to one run.
    Summary,
}

/// The facts of a mutation run that the CI surfaces render.
#[derive(Debug, Clone, PartialEq)]
pub struct MutationCiView {
    /// The state of the run.
    pub state: MutationCiState,
    /// The scope of the run. `None` when the state has no report.
    pub scope: Option<MutationScopeKind>,
    /// The number of mutants for each status.
    pub counts: MutationStatusCounts,
    /// The mutation score, a fraction from 0 to 1. `None` when no mutant was
    /// counted.
    pub score: Option<f64>,
    /// The completion of the run, when the report has one.
    pub completion: Option<MutationCompletion>,
    /// The `mutation-min-score` gate entry of the report, when a minimum
    /// applies.
    pub min_score_gate: Option<GateOutcome>,
    /// The lines that no test reaches, one item for each line. The renderer
    /// sorts the list and removes duplicates.
    pub no_coverage: Vec<MutationCiLocation>,
    /// The survived mutants that are not shown because their file changed
    /// after the run.
    pub stale_hidden: usize,
    /// The survived mutants that are not shown because the diff filter
    /// removed them: they are on lines that the change did not add.
    pub off_diff_hidden: usize,
    /// The survived mutants that are not shown because of the inline cap.
    pub inline_overflow: usize,
    /// The `--root` of the run, as the inspect command must spell it.
    pub root_flag: Option<String>,
    /// The duration of the run in milliseconds.
    pub elapsed_ms: Option<u64>,
    /// The number of mutants whose status came from the result cache.
    pub cache_hits: Option<usize>,
}

impl MutationCiView {
    /// A view in `state` with no counts, no score and no lists.
    #[must_use]
    pub fn new(state: MutationCiState) -> Self {
        Self {
            state,
            scope: None,
            counts: MutationStatusCounts::default(),
            score: None,
            completion: None,
            min_score_gate: None,
            no_coverage: Vec::new(),
            stale_hidden: 0,
            off_diff_hidden: 0,
            inline_overflow: 0,
            root_flag: None,
            elapsed_ms: None,
            cache_hits: None,
        }
    }
}

/// A workflow annotation that states the run state: an error for a failed,
/// stopped or unreadable run, and a notice for a partial run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationRunNotice {
    /// `failure` for an error, `notice` for a partial run.
    pub level: PrDecisionAnnotationLevel,
    /// The annotation title.
    pub title: &'static str,
    /// The annotation message, as plain text. The caller escapes it.
    pub message: String,
}

/// Render the mutation section for `heading`.
///
/// A not-run, failed, stopped or unreadable state replaces everything after
/// the heading with the text of that state. A section longer than
/// [`MUTATION_SECTION_MAX_CHARS`] keeps the heading, the partial-run line, the
/// headline and the count table, followed by a note.
#[must_use]
pub fn render_mutation_section(view: &MutationCiView, heading: MutationHeading) -> String {
    let title = match heading {
        MutationHeading::Comment | MutationHeading::Review => "#### Mutation testing",
        MutationHeading::Summary => "## Mutation testing",
    };
    let body = section_body(view, heading);
    let full = join_paragraphs(title, &body.core, &body.extra);
    if full.chars().count() <= MUTATION_SECTION_MAX_CHARS {
        return full;
    }
    join_paragraphs(title, &body.core, &[SHORTENED_SECTION_NOTE.to_owned()])
}

/// The `mutation` row of the PR decision, which follows the command row.
///
/// A complete run with no survivor and no uncovered mutant is `success`. A
/// partial run, a survivor or an uncovered mutant is `neutral`. A run that
/// the CI job did not start is `skipped`, and a failed, stopped or unreadable
/// run is `failure`. The row has no threshold: the `mutation-min-score` row
/// carries the gate.
#[must_use]
pub fn mutation_decision_row(view: &MutationCiView) -> PrDecisionGate {
    let (status, observed) = decision_outcome(view);
    PrDecisionGate {
        id: DECISION_ROW_ID.to_owned(),
        label: TITLE.to_owned(),
        status,
        observed,
        threshold: None,
        scope: decision_scope(view.scope).to_owned(),
    }
}

/// The plain-text description of a survived mutant, for the finding of the
/// rule `fallow/mutation-survived`.
///
/// For example `Changing >= to > in isAdult made no test fail. Mutant
/// 9f3c2a1b.` The text has no markdown, because each surface escapes a
/// finding description for its own format. Each value becomes one line, and
/// a long value is cut.
#[must_use]
pub fn survived_description(
    operator: &str,
    change: Option<&MutantChange>,
    function: Option<&str>,
    mutant_id: &str,
) -> String {
    let token = |value: &str| single_line(value, TOKEN_MAX_CHARS);
    let action = match change.map(|change| (token(&change.from), token(&change.to))) {
        Some((from, to)) if !from.is_empty() && !to.is_empty() => {
            format!("Changing {from} to {to}")
        }
        Some((from, _)) if !from.is_empty() => format!("Removing {from}"),
        Some((_, to)) if !to.is_empty() => format!("Adding {to}"),
        _ => format!("A {} change", token(operator)),
    };
    let place = function
        .map(|name| single_line(name, INLINE_VALUE_MAX_CHARS))
        .filter(|name| !name.is_empty())
        .map_or_else(String::new, |name| format!(" in {name}"));
    format!(
        "{action}{place} made no test fail. Mutant {}.",
        token(mutant_id)
    )
}

/// The mutation line of the review summary, for example `Mutation testing:
/// score 62.5%, 6 survivors (4 inline).`
///
/// `inline_count` is the number of survivor comments that the review posts.
#[must_use]
pub fn mutation_review_summary_line(view: &MutationCiView, inline_count: usize) -> String {
    match &view.state {
        MutationCiState::NotRun(_) => "Mutation testing: not run.".to_owned(),
        MutationCiState::Interrupted => "Mutation testing: stopped.".to_owned(),
        MutationCiState::Failed { .. } | MutationCiState::Unreadable { .. } => {
            "Mutation testing: failed.".to_owned()
        }
        MutationCiState::Ran => {
            let partial = if is_partial(view) {
                "partial run, "
            } else {
                ""
            };
            let score = view.score.map_or_else(
                || "no score".to_owned(),
                |score| format!("score {}", score_percent(score)),
            );
            let survivors = count_noun(view.counts.survived, "survivor", "survivors");
            format!("Mutation testing: {partial}{score}, {survivors} ({inline_count} inline).")
        }
    }
}

/// The workflow annotation that states the run state, when the state needs
/// one: an error for a failed, stopped or unreadable run, and a notice for a
/// partial run. The message is plain text, and the caller escapes it for the
/// workflow command.
#[must_use]
pub fn mutation_run_notice(view: &MutationCiView) -> Option<MutationRunNotice> {
    let (level, message) = match &view.state {
        MutationCiState::NotRun(_) => return None,
        MutationCiState::Ran => {
            let completion = view.completion.as_ref().filter(|_| is_partial(view))?;
            let reasons = completion
                .reasons
                .iter()
                .map(|reason| reason_token(*reason))
                .collect::<Vec<_>>()
                .join(", ");
            (
                PrDecisionAnnotationLevel::Notice,
                format!(
                    "Partial run ({reasons}). The annotations cover only the mutants that ran."
                ),
            )
        }
        MutationCiState::Failed {
            exit_code,
            code,
            message,
            ..
        } => {
            let code = single_line(code, CODE_MAX_CHARS);
            let code = if code.is_empty() {
                String::new()
            } else {
                format!(", {code}")
            };
            let message = cap_chars(&normalize_block(message), MUTATION_MESSAGE_MAX_CHARS);
            (
                PrDecisionAnnotationLevel::Failure,
                format!("Mutation testing failed (exit code {exit_code}{code}): {message}"),
            )
        }
        MutationCiState::Interrupted => (
            PrDecisionAnnotationLevel::Failure,
            INTERRUPTED_TEXT.to_owned(),
        ),
        MutationCiState::Unreadable { .. } => (
            PrDecisionAnnotationLevel::Failure,
            format!("Fallow could not read the mutation report ({INVALID_REPORT_CODE})."),
        ),
    };
    Some(MutationRunNotice {
        level,
        title: TITLE,
        message,
    })
}

const TITLE: &str = "Mutation testing";
const DECISION_ROW_ID: &str = "mutation";
const REPORT_FILE: &str = "fallow-mutation.json";
const INVALID_REPORT_CODE: &str = "FALLOW_MUTATION_REPORT_INVALID";
const SHORTENED_SUFFIX: &str = " ... (shortened)";
const CUT_SUFFIX: &str = "...";
const SHORTENED_SECTION_NOTE: &str =
    "The mutation section is shortened. The fallow-mutation.json file of this job has all results.";
const INTERRUPTED_TEXT: &str =
    "Mutation testing stopped. A signal stopped the run, and the run wrote no report.";
const FENCE: &str = "````";
const THREE_BACKTICKS: &str = "```";
const THREE_QUOTES: &str = "'''";
/// A path or a function name longer than this is cut. The section cap still
/// applies to the sum.
const INLINE_VALUE_MAX_CHARS: usize = 512;
/// An operator, a change token or a mutant id longer than this is cut.
const TOKEN_MAX_CHARS: usize = 128;
const CODE_MAX_CHARS: usize = 80;
/// Added before the floor, so that a percent such as `28.999999999999996`
/// from `0.29 * 100` shows as `29.0` and not as `28.9`.
const FLOOR_EPSILON: f64 = 1e-9;
const PERCENT: f64 = 100.0;
const TENTHS: f64 = 10.0;
const MILLIS_PER_SECOND: u64 = 1000;
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3600;

/// The paragraphs of a section. `core` survives the section cap, and `extra`
/// does not.
struct SectionBody {
    core: Vec<String>,
    extra: Vec<String>,
}

fn join_paragraphs(title: &str, core: &[String], extra: &[String]) -> String {
    std::iter::once(title)
        .chain(core.iter().map(String::as_str))
        .chain(extra.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn section_body(view: &MutationCiView, heading: MutationHeading) -> SectionBody {
    let only = |text: String| SectionBody {
        core: vec![text],
        extra: Vec::new(),
    };
    match &view.state {
        MutationCiState::Ran => ran_body(view, heading),
        MutationCiState::NotRun(reason) => only(not_run_text(*reason).to_owned()),
        MutationCiState::Interrupted => only(
            "**Mutation testing stopped.** A signal stopped the run, and the run wrote no report."
                .to_owned(),
        ),
        MutationCiState::Unreadable { reason } => SectionBody {
            core: vec![format!(
                "**Fallow could not read the mutation report** ({}).",
                markdown_code_span(INVALID_REPORT_CODE)
            )],
            extra: text_block(reason).into_iter().collect(),
        },
        MutationCiState::Failed {
            exit_code,
            code,
            message,
            help,
        } => {
            let code = single_line(code, CODE_MAX_CHARS);
            let code = if code.is_empty() {
                String::new()
            } else {
                format!(", {}", markdown_code_span(&code))
            };
            SectionBody {
                core: vec![format!(
                    "**Mutation testing failed** (exit code {exit_code}{code})."
                )],
                extra: text_block(message)
                    .into_iter()
                    .chain(help.as_deref().and_then(text_block))
                    .collect(),
            }
        }
    }
}

const fn not_run_text(reason: MutationNotRunReason) -> &'static str {
    match reason {
        MutationNotRunReason::EventRefused => {
            "**Mutation testing did not run.** This event can run the code of a fork with the secrets of the base repository. Mutation testing runs only for the pull_request, push, schedule, workflow_dispatch and merge_group events."
        }
        MutationNotRunReason::ForkRefused => {
            "**Mutation testing did not run.** The merge request comes from a fork, and the pipeline runs in the target project."
        }
        MutationNotRunReason::NoChangeScope => {
            "**Mutation testing did not run.** The changed scope needs a pull request diff or a base ref, and this job has neither. Run the job in a pull request, set a base ref, or use the all scope."
        }
    }
}

fn ran_body(view: &MutationCiView, heading: MutationHeading) -> SectionBody {
    let mut core = Vec::new();
    if let Some(line) = partial_line(view) {
        core.push(line);
    }
    core.push(headline(view));
    if view.counts.total > 0 {
        core.push(count_table(view));
    }
    let facts = match heading {
        MutationHeading::Comment | MutationHeading::Review => None,
        MutationHeading::Summary => run_facts_line(view),
    };
    let extra = [
        scope_line(view.scope).map(str::to_owned),
        facts,
        survivor_explanation(view, heading),
        hidden_line(
            view.stale_hidden,
            "survived mutant is not shown because its file changed after the run.",
            "survived mutants are not shown because their file changed after the run.",
        ),
        hidden_line(
            view.off_diff_hidden,
            "survived mutant is on a line that this change did not add, so it has no finding.",
            "survived mutants are on lines that this change did not add, so they have no finding.",
        ),
        hidden_line(
            view.inline_overflow,
            "more survived mutant is not shown inline. The fallow-mutation.json file of this job lists it.",
            "more survived mutants are not shown inline. The fallow-mutation.json file of this job lists them.",
        ),
        no_coverage_details(&view.no_coverage),
        inspect_line(view),
    ]
    .into_iter()
    .flatten()
    .collect();
    SectionBody { core, extra }
}

fn is_partial(view: &MutationCiView) -> bool {
    view.completion
        .as_ref()
        .is_some_and(|completion| completion.status == MutationCompletionStatus::Partial)
}

fn partial_line(view: &MutationCiView) -> Option<String> {
    let completion = view.completion.as_ref().filter(|_| is_partial(view))?;
    let reasons = completion
        .reasons
        .iter()
        .map(|reason| markdown_code_span(reason_token(*reason)))
        .collect::<Vec<_>>()
        .join(", ");
    let mut line = if reasons.is_empty() {
        "**Partial run.** The counts cover only the mutants that ran.".to_owned()
    } else {
        format!("**Partial run** ({reasons}). The counts cover only the mutants that ran.")
    };
    if completion
        .reasons
        .contains(&MutationIncompleteReason::TimeBudget)
    {
        line.push_str(" The next run with the same budget continues from the result cache.");
    }
    Some(line)
}

/// The kebab-case wire name of a reason. The match is exhaustive, so a new
/// reason does not compile until it has a name here.
const fn reason_token(reason: MutationIncompleteReason) -> &'static str {
    match reason {
        MutationIncompleteReason::RuntimeError => "runtime-error",
        MutationIncompleteReason::MutantLimit => "mutant-limit",
        MutationIncompleteReason::SampledOut => "sampled-out",
        MutationIncompleteReason::TimeBudget => "time-budget",
        MutationIncompleteReason::SkippedFiles => "skipped-files",
        MutationIncompleteReason::UnloadedFiles => "unloaded-files",
        MutationIncompleteReason::TestFilesNotDiscovered => "test-files-not-discovered",
        MutationIncompleteReason::BaselineFailure => "baseline-failure",
        MutationIncompleteReason::WorktreeChanged => "worktree-changed",
        MutationIncompleteReason::TypecheckFailed => "typecheck-failed",
        MutationIncompleteReason::WorktreeUnchecked => "worktree-unchecked",
    }
}

fn headline(view: &MutationCiView) -> String {
    let gate = view.min_score_gate.as_ref();
    let minimum = gate.and_then(|gate| gate.threshold).map(minimum_percent);
    if let Some(gate) = gate.filter(|gate| gate.status == GateStatus::Fail) {
        let observed = gate
            .observed
            .or_else(|| view.score.map(|score| score * PERCENT));
        return match (observed, minimum) {
            (Some(observed), Some(minimum)) => format!(
                "**Score below the minimum.** The score is {}, and the minimum is {minimum}.",
                percent_floor(observed)
            ),
            (None, Some(minimum)) => {
                format!("**Score below the minimum.** The minimum is {minimum}.")
            }
            (_, None) => "**Score below the minimum.**".to_owned(),
        };
    }
    if view.counts.total == 0 {
        return "The scope has no mutant.".to_owned();
    }
    let Some(score) = view.score else {
        return if gate.is_some() {
            "**No mutant was counted.** The score gate was skipped.".to_owned()
        } else {
            "**No mutant was counted.**".to_owned()
        };
    };
    if view.counts.survived > 0 {
        let minimum = minimum.map_or_else(String::new, |minimum| format!(" (minimum {minimum})"));
        return format!(
            "**{} survived.** Score {}{minimum}.",
            count_noun(view.counts.survived, "mutant", "mutants"),
            score_percent(score)
        );
    }
    "The tests detected each mutant that ran.".to_owned()
}

fn count_table(view: &MutationCiView) -> String {
    let counts = &view.counts;
    // The counts come from a file that test code can write, so the sums saturate.
    let not_measured = counts
        .pending
        .saturating_add(counts.runtime_error)
        .saturating_add(counts.flaky)
        .saturating_add(counts.compile_error);
    let score = view.score.map_or_else(|| "-".to_owned(), score_percent);
    format!(
        "| Killed | Survived | No coverage | Not measured | Score |\n\
         | --- | --- | --- | --- | --- |\n\
         | {} | {} | {} | {not_measured} | {score} |",
        counts.killed.saturating_add(counts.timeout),
        counts.survived,
        counts.no_coverage,
    )
}

const fn scope_line(scope: Option<MutationScopeKind>) -> Option<&'static str> {
    match scope {
        Some(MutationScopeKind::Changed) => Some("Scope: the changed lines."),
        Some(MutationScopeKind::All) => Some("Scope: all files."),
        Some(MutationScopeKind::Paths) => Some("Scope: the selected paths."),
        None => None,
    }
}

fn run_facts_line(view: &MutationCiView) -> Option<String> {
    let elapsed = view
        .elapsed_ms
        .map(|ms| format!("The run took {}.", duration_text(ms)));
    let cache = view.cache_hits.filter(|hits| *hits > 0).map(|hits| {
        if hits == 1 {
            "1 result came from the cache.".to_owned()
        } else {
            format!("{hits} results came from the cache.")
        }
    });
    let parts: Vec<String> = elapsed.into_iter().chain(cache).collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn duration_text(ms: u64) -> String {
    let seconds = ms / MILLIS_PER_SECOND;
    if seconds == 0 {
        return "less than 1s".to_owned();
    }
    let hours = seconds / SECONDS_PER_HOUR;
    let minutes = seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
    let rest = seconds % SECONDS_PER_MINUTE;
    if hours > 0 {
        format!("{hours}h {minutes}m {rest}s")
    } else if minutes > 0 {
        format!("{minutes}m {rest}s")
    } else {
        format!("{rest}s")
    }
}

fn survivor_explanation(view: &MutationCiView, heading: MutationHeading) -> Option<String> {
    const FIRST: &str = "Each survived mutant is a small code change that made no test fail.";
    const LAST: &str = "Add a test that fails on the change.";
    if view.counts.survived == 0 {
        return None;
    }
    let lines = if view.scope == Some(MutationScopeKind::Changed) {
        " on the changed lines"
    } else {
        ""
    };
    // Point at the findings only when this render shows at least one. A
    // survivor in a changed file or off the diff has no finding.
    let shown = view
        .counts
        .survived
        .saturating_sub(view.stale_hidden.saturating_add(view.off_diff_hidden));
    if shown == 0 {
        return Some(format!("{FIRST} {LAST}"));
    }
    // The job summary has no findings, so only the comment and the review
    // point at the place where the survived mutants are.
    Some(match heading {
        MutationHeading::Summary => format!("{FIRST} {LAST}"),
        MutationHeading::Comment => {
            format!("{FIRST} The **Tests** findings above show the survived mutants{lines}. {LAST}")
        }
        MutationHeading::Review => {
            format!("{FIRST} The inline comments show the survived mutants{lines}. {LAST}")
        }
    })
}

fn hidden_line(count: usize, singular: &str, plural: &str) -> Option<String> {
    match count {
        0 => None,
        1 => Some(format!("1 {singular}")),
        _ => Some(format!("{count} {plural}")),
    }
}

fn no_coverage_details(locations: &[MutationCiLocation]) -> Option<String> {
    if locations.is_empty() {
        return None;
    }
    let mut sorted = locations.to_vec();
    sorted.sort();
    sorted.dedup();
    let total = sorted.len();
    let count = if total > MUTATION_NO_COVERAGE_ITEMS {
        format!("{MUTATION_NO_COVERAGE_ITEMS} of {total}")
    } else {
        total.to_string()
    };
    let items = sorted
        .iter()
        .take(MUTATION_NO_COVERAGE_ITEMS)
        .map(location_item)
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "<details>\n<summary>No test reaches these lines ({count})</summary>\n\n{items}\n\n</details>"
    ))
}

fn location_item(location: &MutationCiLocation) -> String {
    let place = inline_code(&format!("{}:{}", location.file, location.line));
    let function = location
        .function
        .as_deref()
        .map(|name| single_line(name, INLINE_VALUE_MAX_CHARS))
        .filter(|name| !name.is_empty());
    match function {
        Some(name) => format!("- {place}, function {}", markdown_code_span(&name)),
        None => format!("- {place}"),
    }
}

fn inspect_line(view: &MutationCiView) -> Option<String> {
    if view.counts.survived.saturating_add(view.counts.no_coverage) == 0 {
        return None;
    }
    let mut command = format!("fallow mutation inspect <mutant-id> --report {REPORT_FILE}");
    let root = view
        .root_flag
        .as_deref()
        .map(|root| single_line(root, INLINE_VALUE_MAX_CHARS))
        .filter(|root| !root.is_empty() && root != ".");
    if let Some(root) = root {
        command.push_str(" --root ");
        command.push_str(&shell_quote(&root));
    }
    Some(format!(
        "To inspect a mutant, run {} with the `{REPORT_FILE}` file of this job.",
        markdown_code_span(&command)
    ))
}

fn shell_quote(value: &str) -> String {
    let plain = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '/' | '-' | '@' | '+'));
    if plain {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

fn decision_outcome(view: &MutationCiView) -> (PrDecisionConclusion, String) {
    let counts = &view.counts;
    match &view.state {
        MutationCiState::NotRun(_) => (PrDecisionConclusion::Skipped, "not run".to_owned()),
        MutationCiState::Failed { .. }
        | MutationCiState::Interrupted
        | MutationCiState::Unreadable { .. } => {
            (PrDecisionConclusion::Failure, "failed".to_owned())
        }
        MutationCiState::Ran if is_partial(view) => (
            PrDecisionConclusion::Neutral,
            format!(
                "partial: {} of {} mutants ran",
                counts.executed(),
                counts.total
            ),
        ),
        MutationCiState::Ran if counts.total == 0 => {
            (PrDecisionConclusion::Success, "no mutant".to_owned())
        }
        MutationCiState::Ran => {
            let score = view.score.map_or_else(
                || "no score".to_owned(),
                |score| format!("{} score", score_percent(score)),
            );
            if counts.survived > 0 {
                (
                    PrDecisionConclusion::Neutral,
                    format!("{score}, {} survived", counts.survived),
                )
            } else if counts.no_coverage > 0 {
                (
                    PrDecisionConclusion::Neutral,
                    format!("{score}, {} not covered", counts.no_coverage),
                )
            } else {
                (PrDecisionConclusion::Success, score)
            }
        }
    }
}

const fn decision_scope(scope: Option<MutationScopeKind>) -> &'static str {
    match scope {
        Some(MutationScopeKind::Changed) => "changed lines",
        Some(MutationScopeKind::All) => "all files",
        Some(MutationScopeKind::Paths) => "selected paths",
        None => "-",
    }
}

fn count_noun(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

/// A score fraction as a percent with one decimal.
fn score_percent(score: f64) -> String {
    percent_floor(score * PERCENT)
}

/// A percent with one decimal, rounded down, so that a score just below the
/// minimum never shows as equal to it.
fn percent_floor(percent: f64) -> String {
    let floored = percent.mul_add(TENTHS, FLOOR_EPSILON).floor() / TENTHS;
    format!("{floored:.1}%")
}

/// A configured minimum with at most two decimals and no trailing zero.
fn minimum_percent(percent: f64) -> String {
    let text = format!("{percent:.2}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    format!("{trimmed}%")
}

/// A code span around an untrusted value on one line.
fn inline_code(value: &str) -> String {
    markdown_code_span(&single_line(value, INLINE_VALUE_MAX_CHARS))
}

/// An untrusted value on one line: each line ending and each other control
/// character becomes one space, and a value longer than `max` is cut.
fn single_line(value: &str, max: usize) -> String {
    let flat: String = value
        .replace("\r\n", " ")
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let flat = flat.trim();
    if flat.chars().count() <= max {
        return flat.to_owned();
    }
    let mut cut: String = flat.chars().take(max).collect();
    cut.push_str(CUT_SUFFIX);
    cut
}

/// An untrusted text with `\n` line endings, and no other control character
/// than a tab.
fn normalize_block(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .chars()
        .map(|ch| match ch {
            '\r' => '\n',
            '\n' | '\t' => ch,
            _ if ch.is_control() => ' ',
            _ => ch,
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Cut `value` to `max` characters and mark the cut.
fn cap_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let mut cut: String = value.chars().take(max).collect();
    cut.push_str(SHORTENED_SUFFIX);
    cut
}

/// A fenced text block around an untrusted text, or `None` for an empty text.
/// Each run of three backticks becomes three single quotes, so the longest
/// backtick run left is two, and the four-backtick fence cannot close early.
fn text_block(value: &str) -> Option<String> {
    let text = normalize_block(value).replace(THREE_BACKTICKS, THREE_QUOTES);
    if text.is_empty() {
        return None;
    }
    let text = cap_chars(&text, MUTATION_MESSAGE_MAX_CHARS);
    Some(format!("{FENCE}text\n{text}\n{FENCE}"))
}

#[cfg(test)]
#[path = "mutation_ci_tests.rs"]
mod tests;
