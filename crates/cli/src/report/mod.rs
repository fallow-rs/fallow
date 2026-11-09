mod badge;
pub mod baseline_advisory_text;
pub mod ci;
pub(crate) mod codeclimate;
mod compact;
pub(crate) mod config_pattern_text;
pub mod dupes_grouping;
pub(crate) mod gate_outcome_text;
pub mod github;
pub mod github_annotations;
pub mod github_summary;
pub mod grouping;
pub(crate) mod grouping_note;
mod human;
mod json;
mod markdown;
pub(crate) mod mutation_evidence;
pub(crate) mod request_outcome_text;
pub(crate) mod sarif;
mod shared;
pub(crate) mod sink;
mod status;
pub(crate) mod suggestions;
#[cfg(test)]
pub(crate) mod test_helpers;

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use fallow_api::DuplicationGrouping;
use fallow_config::{OutputFormat, RulesConfig, Severity};
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::AnalysisResults;
use fallow_types::semantic::SemanticSymbolImpact;
use fallow_types::trace::{
    CloneTrace, DependencyTrace, ExportTrace, FileTrace, ImpactClosureTrace, PipelineTimings,
};

use crate::{exit_codes::exit_invalid_input, report::sink::outln};

pub use fallow_output::strip_root_prefix;
pub use grouping::OwnershipResolver;
pub(crate) use human::dupes::MAX_CLONE_GROUPS;
pub(crate) use human::health::{render_health_score, render_health_trend};
pub(crate) use human::{build_section_header, format_path};
pub(crate) use markdown::{print_saved_markdown, saved_markdown_unsupported};
pub(crate) use status::{
    HumanStatus, line as human_status_line, semantic_status, type_aware_meta_status,
};

/// The three line-groups of a human `fallow review --walkthrough` render: the
/// orientation header and final status (stderr), and the staged tour body
/// (stdout). The entry point in `audit_brief.rs` owns the stream split; this
/// keeps the pure line builder behind the private `human` module while exposing
/// exactly what the entry point needs.
pub(crate) struct WalkthroughHumanRender {
    /// Review Focus orientation header lines (stderr).
    pub(crate) header: Vec<String>,
    /// The staged tour body lines (stdout).
    pub(crate) body: Vec<String>,
    /// The final green status line (stderr).
    pub(crate) status: String,
}

/// The root-relative files (in `direction.order`) the local ledger marked viewed
/// against the guide's current hash. Exposed so the markdown surface can collapse
/// the same viewed files into Cleared that the human surface does, keeping the two
/// formats consistent on the same on-disk `--mark-viewed` state.
#[must_use]
pub(crate) fn walkthrough_viewed_files(
    guide: &fallow_output::StandardWalkthroughGuide,
    viewed: &crate::walkthrough_state::ViewedState,
) -> Vec<String> {
    human::walkthrough::viewed_files_for(guide, viewed)
}

/// Build the human walkthrough tour from the in-memory guide. Pure: no IO, no
/// mutation. `viewed` decorates each file row; `show_cleared` expands the
/// Cleared panel.
#[must_use]
pub(crate) fn build_walkthrough_human(
    guide: &fallow_output::StandardWalkthroughGuide,
    viewed: &crate::walkthrough_state::ViewedState,
    show_cleared: bool,
) -> WalkthroughHumanRender {
    let input = human::walkthrough::WalkthroughHumanInput {
        guide,
        viewed,
        show_cleared,
    };
    WalkthroughHumanRender {
        header: human::walkthrough::build_focus_header(guide, viewed),
        body: human::walkthrough::build_walkthrough_human_lines(&input),
        status: human::walkthrough::build_status_line(guide, viewed),
    }
}

/// Shared context for all report dispatch functions.
///
/// Bundles the common parameters that every format renderer needs,
/// replacing per-parameter threading through the dispatch match arms.
pub(crate) struct ReportContext<'a> {
    pub(crate) package_baselines: &'a [fallow_api::PackageBaselineStatus],
    pub(crate) root: &'a Path,
    pub(crate) rules: &'a RulesConfig,
    /// Workspace diagnostics captured by the analysis that owns this report.
    pub(crate) workspace_diagnostics: &'a [fallow_config::WorkspaceDiagnostic],
    pub(crate) elapsed: Duration,
    pub(crate) quiet: bool,
    pub(crate) explain: bool,
    /// Provenance for the opt-in TypeScript semantic analysis pass.
    pub(crate) type_aware: Option<&'a fallow_types::envelope::TypeAwareMeta>,
    /// Optional label for semantic metadata when one command renders multiple
    /// analysis scopes into the same stream.
    pub(crate) type_aware_scope: Option<&'static str>,
    /// When set, group all output by this resolver.
    pub(crate) group_by: Option<OwnershipResolver>,
    /// Limit displayed items per section (--top N).
    pub(crate) top: Option<usize>,
    /// When set, print a concise summary instead of the full report.
    pub(crate) summary: bool,
    /// Human-only: print the summary renderer's own title line. Combined mode
    /// already prints section headers, so it disables this to avoid duplicate
    /// "Dead Code" / "Dead Code Summary" headings.
    pub(crate) summary_heading: bool,
    /// Human-only: print a one-line hint pointing at `fallow explain`.
    pub(crate) show_explain_tip: bool,
    /// When a baseline was loaded: (total entries in baseline, entries that matched).
    pub(crate) baseline_matched: Option<(usize, usize)>,
    /// This run's full view of the loaded baseline, for the JSON envelope's
    /// `baseline_staleness`. Carries what `baseline_matched` cannot: whether the
    /// run was change-scoped, the advisory verdict and the
    /// `--fail-on-stale-baseline` verdict.
    pub(crate) baseline_staleness: Option<fallow_output::BaselineStaleness>,
    /// The answer to `--finding-id`, for the JSON envelope's
    /// `finding_id_query`. `None` keeps the key off the wire.
    pub(crate) finding_id_query: Option<fallow_output::FindingIdQuery>,
    /// Every gate this run evaluated, for the JSON envelope's `gate_outcomes`.
    /// `None` when the run evaluated none, which keeps the key off the wire.
    pub(crate) gate_outcomes: Option<fallow_output::GateOutcomes>,
    /// The number of files an armed `parse-error` gate failed on, 0 when the
    /// gate is off or passed. The human dead-code status line reads it, so a
    /// run with no finding does not say it is clean while the gate fails it.
    pub(crate) failed_parse_files: usize,
    /// Whether config-edit actions can be applied by `fallow fix`.
    ///
    /// This is caller-provided because an explicit `--config` path is fixable
    /// even when default config discovery from the root would find nothing.
    pub(crate) config_fixable: bool,
    /// When set, the human health renderer skips the `● Health score:` and
    /// trend table sections because they have already been rendered upstream
    /// (combined-mode orientation header). Standalone `fallow health` keeps
    /// the default `false` and renders both sections inline.
    pub(crate) skip_score_and_trend: bool,
    /// Human-only: whether `--css` was requested. When `true` but no stylesheet
    /// was import-reachable, the CSS-health section renders an explanatory note
    /// instead of being silently omitted. Defaults `false` for non-css callers.
    pub(crate) css_requested: bool,
    /// Presentation style for report JSON. Non-JSON renderers ignore it.
    pub(crate) json_style: crate::json_style::JsonStyle,
    /// Duplication JSON only: whether each clone instance carries its verbatim
    /// source text. `false` is `fallow dupes --no-fragments`. Every other
    /// renderer and analysis ignores it.
    pub(crate) include_fragments: bool,
    /// Human dead-code only: where the architecture findings render. Every
    /// other renderer and format ignores it.
    pub(crate) architecture_layout: ArchitectureLayout,
}

/// Where the human dead-code report renders the architecture findings
/// (import cycles, boundary violations and rule-pack policy violations).
///
/// The layout changes the human lines. The findings, the totals and the
/// footer stay the same. In JSON, `Only` changes the command that
/// `next_steps` names and the `_meta.docs` URL; see [`ArchitectureLayout::command`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ArchitectureLayout {
    /// Inside the dead-code "Structure" and "Policy" categories (`fallow dead-code`).
    #[default]
    Embedded,
    /// In one "Architecture" category after the dead-code categories (bare `fallow`).
    Split,
    /// Under one "Architecture" category (`fallow architecture`).
    Only,
}

/// The subcommand name of `fallow architecture`.
pub(crate) const ARCHITECTURE_COMMAND: &str = "architecture";

impl ArchitectureLayout {
    /// The subcommand that a JSON report in this layout names in its
    /// `next_steps` and `_meta.docs`: `architecture` for `Only`, else `dead-code`.
    pub(crate) const fn command(self) -> &'static str {
        match self {
            Self::Only => ARCHITECTURE_COMMAND,
            Self::Embedded | Self::Split => "dead-code",
        }
    }

    /// The JSON envelope that a report in this layout writes:
    /// `architecture` for `Only`, else `dead-code`.
    ///
    /// Bare `fallow --only architecture` also uses `Only`, but its JSON and
    /// CI formats go through the combined renderer and never reach the
    /// dead-code JSON path, so the combined shape does not change.
    pub(crate) const fn envelope(self) -> fallow_output::CheckEnvelope {
        match self {
            Self::Only => fallow_output::CheckEnvelope::Architecture,
            Self::Embedded | Self::Split => fallow_output::CheckEnvelope::DeadCode,
        }
    }
}

/// Whether the results hold an architecture finding: an import cycle, a
/// boundary violation or a rule-pack policy violation.
pub(crate) fn has_architecture_findings(results: &fallow_types::results::AnalysisResults) -> bool {
    !results.circular_dependencies.is_empty()
        || !results.re_export_cycles.is_empty()
        || !results.package_cycles.is_empty()
        || !results.boundary_violations.is_empty()
        || !results.boundary_coverage_violations.is_empty()
        || !results.boundary_call_violations.is_empty()
        || !results.policy_violations.is_empty()
}

/// Strip the project root prefix from a path for display, falling back to the full path.
#[must_use]
pub(crate) fn relative_path<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

/// Format a path for human-facing display: project-relative when the path is
/// under `root`, falling back to the full path otherwise. Always
/// forward-slash-normalized so Windows backslashes do not leak into
/// terminal output.
///
/// Use this for any human-output site that today renders bare `file_name()`,
/// since bare basenames are ambiguous in Nx / Angular / Rust-workspace layouts
/// where many files share names like `index.ts`, `mod.rs`, or
/// `*.component.ts`. See issue #547.
#[must_use]
pub(crate) fn format_display_path(path: &Path, root: &Path) -> String {
    relative_path(path, root)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// Split a path string into (directory, filename) for display.
/// Directory includes the trailing `/`. If no directory, returns `("", filename)`.
#[must_use]
pub(crate) fn split_dir_filename(path: &str) -> (&str, &str) {
    path.rfind('/')
        .map_or(("", path), |pos| (&path[..=pos], &path[pos + 1..]))
}

/// Return `"s"` for plural or `""` for singular.
#[must_use]
pub(crate) const fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Format a byte count in KiB / MiB / GiB for terminal output. Byte-exact
/// sizes are available in JSON output paths; humans get a readable form.
#[expect(
    clippy::cast_precision_loss,
    reason = "reported byte counts are well under the f64 precision loss range"
)]
#[must_use]
pub(crate) fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.0} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Serialize a spec-defined JSON value with its established pretty formatting.
///
/// On success prints the JSON and returns `ExitCode::SUCCESS`.
/// On serialization failure prints an error to stderr and returns exit code 2.
#[must_use]
pub(crate) fn emit_json(value: &serde_json::Value, kind: &str) -> ExitCode {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            outln!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: failed to serialize {kind} output: {e}");
            exit_invalid_input()
        }
    }
}

/// Serialize report JSON with the requested presentation style.
#[must_use]
pub(crate) fn emit_report_json(
    value: &serde_json::Value,
    kind: &str,
    style: crate::json_style::JsonStyle,
) -> ExitCode {
    match style.serialize(value) {
        Ok(json) => {
            outln!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: failed to serialize {kind} output: {e}");
            exit_invalid_input()
        }
    }
}

pub(crate) struct CheckJsonRenderInput<'a> {
    pub(crate) package_baselines: &'a [fallow_api::PackageBaselineStatus],
    pub(crate) results: &'a AnalysisResults,
    pub(crate) root: &'a Path,
    pub(crate) elapsed: Duration,
    pub(crate) type_aware: Option<&'a fallow_types::envelope::TypeAwareMeta>,
    pub(crate) regression: Option<&'a crate::regression::RegressionOutcome>,
    pub(crate) baseline_matched: Option<(usize, usize)>,
    pub(crate) baseline_staleness: Option<fallow_output::BaselineStaleness>,
    pub(crate) gate_outcomes: Option<fallow_output::GateOutcomes>,
    pub(crate) config_fixable: bool,
    pub(crate) workspace_diagnostics: &'a [fallow_config::WorkspaceDiagnostic],
    pub(crate) json_style: crate::json_style::JsonStyle,
}

pub(crate) fn render_check_json(
    input: &CheckJsonRenderInput<'_>,
) -> Result<String, serde_json::Error> {
    json::render_json(&json::PrintJsonInput {
        package_baselines: input.package_baselines,
        results: input.results,
        root: input.root,
        elapsed: input.elapsed,
        explain: false,
        type_aware: input.type_aware,
        regression: input.regression,
        baseline_matched: input.baseline_matched,
        baseline_staleness: input.baseline_staleness,
        finding_id_query: None,
        gate_outcomes: input.gate_outcomes.clone(),
        config_fixable: input.config_fixable,
        workspace_diagnostics: input.workspace_diagnostics,
        json_style: input.json_style,
        envelope: fallow_output::CheckEnvelope::DeadCode,
    })
}

/// Elide the common directory prefix between a base path and a target path.
/// Only strips complete directory segments (never partial filenames).
/// Returns the remaining suffix of `target`.
///
/// Example: `elide_common_prefix("a/b/c/foo.ts", "a/b/d/bar.ts")` → `"d/bar.ts"`
#[must_use]
pub(crate) fn elide_common_prefix<'a>(base: &str, target: &'a str) -> &'a str {
    let mut last_sep = 0;
    for (i, (a, b)) in base.bytes().zip(target.bytes()).enumerate() {
        if a != b {
            break;
        }
        if a == b'/' {
            last_sep = i + 1;
        }
    }
    if last_sep > 0 && last_sep <= target.len() {
        &target[last_sep..]
    } else {
        target
    }
}

/// Compute a SARIF-compatible relative URI from an absolute path and project root.
#[cfg(test)]
fn relative_uri(path: &Path, root: &Path) -> String {
    normalize_uri(&relative_path(path, root).display().to_string())
}

/// Normalize a path string to a valid URI: forward slashes and percent-encoded brackets.
///
/// Brackets (`[`, `]`) are not valid in URI path segments per RFC 3986 and cause
/// SARIF validation warnings (e.g., Next.js dynamic routes like `[slug]`).
#[must_use]
pub(crate) fn normalize_uri(path_str: &str) -> String {
    fallow_output::normalize_uri(path_str)
}

/// Severity level for human-readable output.
#[derive(Clone, Copy, Debug)]
pub enum Level {
    Warn,
    Info,
    Error,
}

#[must_use]
pub(crate) const fn severity_to_level(s: Severity) -> Level {
    match s {
        Severity::Error => Level::Error,
        Severity::Warn => Level::Warn,
        Severity::Off => Level::Info,
    }
}

/// Whether a gate of the run fails it, for the mark of the human status line.
///
/// A context without gate outcomes (watch mode) keeps the failure mark for a
/// run with findings.
fn run_fails(ctx: &ReportContext<'_>) -> bool {
    ctx.gate_outcomes
        .as_ref()
        .is_none_or(fallow_output::GateOutcomes::fails_run)
}

/// Whether a gate of a duplication run fails it, for the mark of the human
/// status line.
///
/// `dupes` has no default rule: a run that armed no gate has no gate
/// outcomes and always exits 0, so clones then show the warning mark.
fn duplication_run_fails(ctx: &ReportContext<'_>) -> bool {
    ctx.gate_outcomes
        .as_ref()
        .is_some_and(fallow_output::GateOutcomes::fails_run)
}

/// Print analysis results in the configured format.
/// Returns exit code 2 if serialization fails, SUCCESS otherwise.
///
/// When `regression` is `Some`, the JSON format includes a `regression` key in the output envelope.
/// When `ctx.group_by` is `Some`, results are partitioned into labeled groups before rendering.
#[must_use]
pub(crate) fn print_results(
    results: &AnalysisResults,
    ctx: &ReportContext<'_>,
    output: OutputFormat,
    regression: Option<&crate::regression::RegressionOutcome>,
) -> ExitCode {
    if let Some(ref resolver) = ctx.group_by {
        let groups = grouping::group_analysis_results(results, ctx.root, resolver);
        return print_grouped_results(&groups, results, ctx, output, resolver);
    }

    match output {
        OutputFormat::Human => {
            if ctx.summary {
                human::check::print_check_summary(
                    results,
                    ctx.rules,
                    ctx.elapsed,
                    ctx.quiet,
                    human::check::SummaryStyle {
                        heading: ctx.summary_heading.then_some(
                            if ctx.architecture_layout == ArchitectureLayout::Only {
                                "Architecture Summary"
                            } else {
                                "Dead Code Summary"
                            },
                        ),
                        layout: ctx.architecture_layout,
                    },
                    human::check::RunStatus {
                        run_fails: run_fails(ctx),
                        failed_parse_files: ctx.failed_parse_files,
                    },
                );
            } else {
                human::print_human(&human::PrintHumanInput {
                    results,
                    root: ctx.root,
                    rules: ctx.rules,
                    elapsed: ctx.elapsed,
                    quiet: ctx.quiet,
                    top: ctx.top,
                    show_explain_tip: ctx.show_explain_tip,
                    explain: ctx.explain,
                    run_fails: run_fails(ctx),
                    failed_parse_files: ctx.failed_parse_files,
                    architecture_layout: ctx.architecture_layout,
                });
            }
            ExitCode::SUCCESS
        }
        OutputFormat::Json => json::print_json(&json::PrintJsonInput {
            package_baselines: ctx.package_baselines,
            results,
            root: ctx.root,
            elapsed: ctx.elapsed,
            explain: ctx.explain,
            type_aware: ctx.type_aware,
            regression,
            baseline_matched: ctx.baseline_matched,
            baseline_staleness: ctx.baseline_staleness,
            finding_id_query: ctx.finding_id_query.clone(),
            gate_outcomes: ctx.gate_outcomes.clone(),
            config_fixable: ctx.config_fixable,
            workspace_diagnostics: ctx.workspace_diagnostics,
            json_style: ctx.json_style,
            envelope: ctx.architecture_layout.envelope(),
        }),
        OutputFormat::Compact => {
            compact::print_compact(results, ctx.root);
            compact::print_type_aware_compact(ctx.type_aware, ctx.type_aware_scope);
            ExitCode::SUCCESS
        }
        OutputFormat::Sarif => sarif::print_sarif(
            results,
            ctx.root,
            ctx.rules,
            ctx.type_aware,
            ctx.workspace_diagnostics,
        ),
        OutputFormat::Markdown => {
            markdown::print_markdown(results, ctx.root);
            markdown::print_type_aware_markdown(ctx.type_aware, ctx.type_aware_scope);
            markdown::print_config_pattern_markdown(ctx.workspace_diagnostics);
            ExitCode::SUCCESS
        }
        OutputFormat::CodeClimate => codeclimate::print_codeclimate(results, ctx.root, ctx.rules),
        OutputFormat::GithubAnnotations => print_check_github_annotations(results, ctx),
        OutputFormat::GithubSummary => {
            print_check_github_format(results, ctx, GithubTarget::Summary)
        }
        ci_format => print_results_ci_comment(results, ctx, ci_format),
    }
}

/// Which GitHub-native renderer a dispatch arm targets.
#[derive(Clone, Copy)]
enum GithubTarget {
    Annotations,
    Summary,
}

fn print_github_format(
    kind: github_annotations::EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    target: GithubTarget,
) -> ExitCode {
    match target {
        GithubTarget::Annotations => github_annotations::print_annotations(kind, envelope, root),
        GithubTarget::Summary => github_summary::print_summary(kind, envelope, root),
    }
}

/// Render dead-code results as GitHub workflow-command annotations by
/// building the same JSON envelope `--format json` serializes and feeding it
/// to the value-driven renderer (which keeps `fallow report --from` output
/// byte-identical to the direct format run).
fn print_check_github_annotations(results: &AnalysisResults, ctx: &ReportContext<'_>) -> ExitCode {
    print_check_github_format(results, ctx, GithubTarget::Annotations)
}

fn print_check_github_format(
    results: &AnalysisResults,
    ctx: &ReportContext<'_>,
    target: GithubTarget,
) -> ExitCode {
    match json::api_check_json_document_with_config_fixable_meta_and_extras(
        results,
        ctx.root,
        ctx.elapsed,
        ctx.config_fixable,
        None,
        // This envelope is a render input for the GitHub-native targets, so it
        // carries exactly what those two surfaces state: the scope the run
        // covered (issues #2687, #2688), the gate inventory, and this run's view
        // of the loaded baseline. Leaving the last two defaulted made the
        // `Gate outcomes:` line and the baseline advisory reachable only through
        // `fallow report --from`, so a direct run and a re-render of its own
        // envelope disagreed (issue #2734).
        fallow_api::CheckJsonExtraOutputs {
            request_outcomes: crate::requests::request_outcomes(),
            baseline_staleness: ctx.baseline_staleness,
            gate_outcomes: ctx.gate_outcomes.clone(),
            ..Default::default()
        },
        ctx.workspace_diagnostics,
        ctx.architecture_layout.envelope(),
    ) {
        Ok(envelope) => print_github_format(
            ctx.architecture_layout.envelope().into(),
            &envelope,
            ctx.root,
            target,
        ),
        Err(e) => {
            eprintln!("Error: failed to serialize results: {e}");
            exit_invalid_input()
        }
    }
}

/// The note a CI comment or review body carries: the type-aware message, the
/// baseline advisory, the gate verdict, whether the run did what it was asked,
/// a grouping this target cannot carry, or any combination of them.
///
/// Shared by the live renderers and by `fallow report --from`, because the two
/// must produce byte-identical bodies for one envelope and that parity has its
/// own suite. The clauses join in a fixed order, so two identical runs render
/// identical bodies and a run that has nothing to say renders no note at all.
///
/// `grouping_dropped` is the `--group-by` mode when one was requested, because
/// every target this note reaches renders one flat document (issue #2691).
pub fn ci_status_note(
    existing: Option<&'static str>,
    baseline_advisory: Option<&str>,
    gates: Option<&fallow_output::GateOutcomes>,
    requests: Option<&fallow_output::RequestOutcomes>,
    grouping_dropped: Option<&str>,
) -> Option<String> {
    let gate_summary = gate_outcome_text::summary_line_for_gates(gates);
    let request_summary = request_outcome_text::summary_line_for_requests(requests);
    let grouping_clause = grouping_dropped.map(grouping_note::dropped_grouping_clause);
    join_status_clauses(&[
        existing,
        baseline_advisory,
        gate_summary.as_deref(),
        request_summary.as_deref(),
        grouping_clause.as_deref(),
    ])
}

/// Space-join the clauses a status note is made of, dropping the absent ones.
///
/// Order is fixed: the type-aware message first because it says the analysis
/// was incomplete, then the baseline advisory as the specific fact, then the
/// gate inventory as the summary, then what the run was asked to do, and last
/// the grouping this target could not carry. Both comment renderers emit the
/// note as a single `> ` blockquote line, so the join is a space rather than a
/// newline.
pub(crate) fn join_status_clauses(clauses: &[Option<&str>]) -> Option<String> {
    let joined = clauses
        .iter()
        .filter_map(|clause| clause.filter(|value| !value.is_empty()))
        .collect::<Vec<_>>()
        .join(" ");
    (!joined.is_empty()).then_some(joined)
}

/// Render the CI comment / review / badge fallback arms for dead-code results.
fn print_results_ci_comment(
    results: &AnalysisResults,
    ctx: &ReportContext<'_>,
    output: OutputFormat,
) -> ExitCode {
    // Analysis-root-relative on purpose: the review renderer applies the
    // presentation prefix after its diff lookups, and rebasing here would
    // prefix twice and key the filter in the wrong namespace.
    let issues = codeclimate::api_codeclimate_issues(results, ctx.root, ctx.rules);
    let incomplete = ci::required_type_aware_incomplete(ctx.type_aware);
    let conclusion = incomplete.then_some(fallow_output::PrDecisionConclusion::Failure);
    let advisory =
        baseline_advisory_text::advisory_line_for_staleness(ctx.baseline_staleness.as_ref());
    let requests = crate::requests::request_outcomes();
    let grouping_dropped = dropped_grouping_mode(ctx, output);
    let status_message = ci_status_note(
        incomplete.then_some(ci::TYPE_AWARE_INCOMPLETE_MESSAGE),
        advisory.as_deref(),
        ctx.gate_outcomes.as_ref(),
        requests.as_ref(),
        grouping_dropped,
    );
    print_ci_comment_format_with_status(
        ctx.architecture_layout.command(),
        &issues,
        output,
        conclusion,
        ci::pr_comment::PrCommentStatus {
            message: status_message.as_deref(),
            gates: &gate_outcome_text::gate_rows_for_gates(ctx.gate_outcomes.as_ref()),
            config_patterns: ctx.workspace_diagnostics,
        },
    )
    .unwrap_or_else(|| {
        eprintln!("Error: badge format is only supported for the health command");
        exit_invalid_input()
    })
}

/// Render grouped results across all output formats.
#[must_use]
fn print_grouped_results(
    groups: &[grouping::ResultGroup],
    original: &AnalysisResults,
    ctx: &ReportContext<'_>,
    output: OutputFormat,
    resolver: &OwnershipResolver,
) -> ExitCode {
    match output {
        OutputFormat::Human => {
            human::print_grouped_human(&human::PrintGroupedHumanInput {
                groups,
                root: ctx.root,
                rules: ctx.rules,
                elapsed: ctx.elapsed,
                quiet: ctx.quiet,
                resolver: Some(resolver),
                explain: ctx.explain,
                run_fails: run_fails(ctx),
                failed_parse_files: ctx.failed_parse_files,
                architecture_layout: ctx.architecture_layout,
            });
            ExitCode::SUCCESS
        }
        OutputFormat::Json => json::print_grouped_json(&json::PrintGroupedJsonInput {
            package_baselines: ctx.package_baselines,
            groups,
            original,
            root: ctx.root,
            elapsed: ctx.elapsed,
            explain: ctx.explain,
            type_aware: ctx.type_aware,
            resolver,
            config_fixable: ctx.config_fixable,
            baseline_staleness: ctx.baseline_staleness,
            finding_id_query: ctx.finding_id_query.clone(),
            gate_outcomes: ctx.gate_outcomes.clone(),
            workspace_diagnostics: ctx.workspace_diagnostics,
            json_style: ctx.json_style,
            envelope: ctx.architecture_layout.envelope(),
        }),
        OutputFormat::Compact => {
            compact::print_grouped_compact(groups, ctx.root);
            compact::print_type_aware_compact(ctx.type_aware, ctx.type_aware_scope);
            ExitCode::SUCCESS
        }
        OutputFormat::Markdown => {
            markdown::print_grouped_markdown(groups, ctx.root, original.cascade_hidden);
            markdown::print_type_aware_markdown(ctx.type_aware, ctx.type_aware_scope);
            markdown::print_config_pattern_markdown(ctx.workspace_diagnostics);
            ExitCode::SUCCESS
        }
        OutputFormat::Sarif => sarif::print_grouped_sarif(
            original,
            ctx.root,
            ctx.rules,
            resolver,
            ctx.type_aware,
            ctx.workspace_diagnostics,
        ),
        OutputFormat::CodeClimate => {
            codeclimate::print_grouped_codeclimate(original, ctx.root, ctx.rules, resolver)
        }
        // The GitHub-native formats have no grouping concept, so they render
        // the ungrouped document from the original results and say so on
        // stderr. Deliberately stderr only: both documents COULD carry the
        // sentence (each already appends the gate and request lines), but a
        // dropped grouping is a fact about the invocation rather than about the
        // findings, and neither document has a reader who can act on it. The
        // comment and review bodies carry the clause because a human reads them
        // and would otherwise take the flat list for a grouped one (issue
        // #2691).
        OutputFormat::GithubAnnotations => {
            dropped_grouping_mode(ctx, output);
            print_check_github_annotations(original, ctx)
        }
        OutputFormat::GithubSummary => {
            dropped_grouping_mode(ctx, output);
            print_check_github_format(original, ctx, GithubTarget::Summary)
        }
        ci_format => print_results_ci_comment(original, ctx, ci_format),
    }
}

/// Print duplication analysis results in the configured format.
#[must_use]
pub(crate) fn print_duplication_report(
    report: &DuplicationReport,
    ctx: &ReportContext<'_>,
    output: OutputFormat,
) -> ExitCode {
    if let Some(ref resolver) = ctx.group_by {
        let grouping = dupes_grouping::build_duplication_grouping(report, ctx.root, resolver);
        return print_grouped_duplication_report(report, &grouping, ctx, output, resolver);
    }

    match output {
        OutputFormat::Human => {
            if ctx.summary {
                human::dupes::print_duplication_summary(
                    report,
                    ctx.elapsed,
                    ctx.quiet,
                    ctx.summary_heading,
                    duplication_run_fails(ctx),
                );
            } else {
                human::print_duplication_human(
                    report,
                    ctx.root,
                    ctx.elapsed,
                    &human::dupes::DuplicationHumanOptions {
                        quiet: ctx.quiet,
                        show_explain_tip: ctx.show_explain_tip,
                        explain: ctx.explain,
                        run_fails: duplication_run_fails(ctx),
                    },
                );
            }
            ExitCode::SUCCESS
        }
        OutputFormat::Json => json::print_duplication_json(
            report,
            ctx.root,
            ctx.elapsed,
            &json::DuplicationJsonRender {
                package_baselines: ctx.package_baselines,
                explain: ctx.explain,
                include_fragments: ctx.include_fragments,
                baseline_staleness: ctx.baseline_staleness,
                gate_outcomes: ctx.gate_outcomes.clone(),
            },
            ctx.workspace_diagnostics,
            ctx.json_style,
        ),
        OutputFormat::Compact => {
            compact::print_duplication_compact(report, ctx.root);
            ExitCode::SUCCESS
        }
        OutputFormat::Sarif => sarif::print_duplication_sarif(report, ctx.root),
        OutputFormat::Markdown => {
            markdown::print_duplication_markdown(report, ctx.root);
            ExitCode::SUCCESS
        }
        OutputFormat::CodeClimate => codeclimate::print_duplication_codeclimate(report, ctx.root),
        OutputFormat::GithubAnnotations => {
            print_dupes_github_format(report, ctx, GithubTarget::Annotations)
        }
        OutputFormat::GithubSummary => {
            print_dupes_github_format(report, ctx, GithubTarget::Summary)
        }
        ci_format => print_duplication_ci_comment(report, ctx.root, ci_format, ctx, None),
    }
}

/// Render duplication results in a GitHub-native format from the same JSON
/// envelope `--format json` serializes.
fn print_dupes_github_format(
    report: &DuplicationReport,
    ctx: &ReportContext<'_>,
    target: GithubTarget,
) -> ExitCode {
    match json::api_duplication_json_document(
        report,
        ctx.root,
        ctx.elapsed,
        &json::DuplicationJsonRender {
            package_baselines: ctx.package_baselines,
            explain: ctx.explain,
            include_fragments: ctx.include_fragments,
            baseline_staleness: ctx.baseline_staleness,
            gate_outcomes: ctx.gate_outcomes.clone(),
        },
        ctx.workspace_diagnostics,
    ) {
        Ok(envelope) => print_github_format(
            github_annotations::EnvelopeKind::Dupes,
            &envelope,
            ctx.root,
            target,
        ),
        Err(e) => {
            eprintln!("Error: failed to serialize duplication report: {e}");
            exit_invalid_input()
        }
    }
}

/// Render the CI comment / review / badge fallback arms for duplication results.
fn print_duplication_ci_comment(
    report: &DuplicationReport,
    root: &Path,
    output: OutputFormat,
    ctx: &ReportContext<'_>,
    grouping_dropped: Option<&str>,
) -> ExitCode {
    let issues = codeclimate::api_duplication_codeclimate_issues(report, root);
    let advisory =
        baseline_advisory_text::advisory_line_for_staleness(ctx.baseline_staleness.as_ref());
    let requests = crate::requests::request_outcomes();
    let note = ci_status_note(
        None,
        advisory.as_deref(),
        ctx.gate_outcomes.as_ref(),
        requests.as_ref(),
        grouping_dropped,
    );
    print_ci_comment_format_with_status(
        "dupes",
        &issues,
        output,
        None,
        ci::pr_comment::PrCommentStatus {
            message: note.as_deref(),
            gates: &gate_outcome_text::gate_rows_for_gates(ctx.gate_outcomes.as_ref()),
            config_patterns: &[],
        },
    )
    .unwrap_or_else(|| {
        eprintln!("Error: badge format is only supported for the health command");
        exit_invalid_input()
    })
}

/// Render grouped duplication results across all output formats.
#[must_use]
fn print_grouped_duplication_report(
    report: &DuplicationReport,
    grouping: &DuplicationGrouping,
    ctx: &ReportContext<'_>,
    output: OutputFormat,
    resolver: &OwnershipResolver,
) -> ExitCode {
    match output {
        OutputFormat::Human => {
            human::print_grouped_duplication_human(
                report,
                grouping,
                ctx.root,
                ctx.elapsed,
                ctx.quiet,
                duplication_run_fails(ctx),
            );
            ExitCode::SUCCESS
        }
        OutputFormat::Json => json::print_grouped_duplication_json(
            report,
            grouping,
            ctx.root,
            ctx.elapsed,
            &json::DuplicationJsonRender {
                package_baselines: ctx.package_baselines,
                explain: ctx.explain,
                include_fragments: ctx.include_fragments,
                baseline_staleness: ctx.baseline_staleness,
                gate_outcomes: ctx.gate_outcomes.clone(),
            },
            ctx.workspace_diagnostics,
            ctx.json_style,
        ),
        OutputFormat::Sarif => sarif::print_grouped_duplication_sarif(report, ctx.root, resolver),
        OutputFormat::CodeClimate => {
            codeclimate::print_grouped_duplication_codeclimate(report, ctx.root, resolver)
        }
        OutputFormat::PrCommentGithub
        | OutputFormat::PrCommentGitlab
        | OutputFormat::ReviewGithub
        | OutputFormat::ReviewGitlab => print_duplication_ci_comment(
            report,
            ctx.root,
            output,
            ctx,
            note_dropped_grouping(Some(grouping.mode), output),
        ),
        // The GitHub-native formats have no grouping concept, so they render
        // the ungrouped document and say so on stderr, which is the whole fix
        // for those two targets. Not because those documents have nowhere to
        // put it (both already append the gate and request lines) but because a
        // dropped grouping is a fact about the invocation, and the reader who
        // can act on it is the one reading the job log (issue #2691).
        OutputFormat::GithubAnnotations => {
            note_dropped_grouping(Some(grouping.mode), output);
            print_dupes_github_format(report, ctx, GithubTarget::Annotations)
        }
        OutputFormat::GithubSummary => {
            note_dropped_grouping(Some(grouping.mode), output);
            print_dupes_github_format(report, ctx, GithubTarget::Summary)
        }
        OutputFormat::Compact => {
            compact::print_duplication_compact(report, ctx.root);
            note_dropped_grouping(Some(grouping.mode), output);
            ExitCode::SUCCESS
        }
        OutputFormat::Markdown => {
            markdown::print_duplication_markdown(report, ctx.root);
            note_dropped_grouping(Some(grouping.mode), output);
            ExitCode::SUCCESS
        }
        OutputFormat::Badge => {
            eprintln!("Error: badge format is only supported for the health command");
            exit_invalid_input()
        }
    }
}

/// Dispatch a PR-comment / review CI format from precomputed CodeClimate issues.
///
/// The issues stay typed, so the legacy fingerprint that review comments carry
/// for one release reaches the renderer. A round trip through the CodeClimate
/// wire shape would drop it.
///
/// Returns `Some(exit_code)` for the four CI comment/review formats and `None`
/// for every other output format, so callers keep their exhaustive match arms.
fn print_ci_comment_format_with_status(
    analysis: &str,
    issues: &[fallow_output::CodeClimateIssue],
    output: OutputFormat,
    conclusion: Option<fallow_output::PrDecisionConclusion>,
    status: ci::pr_comment::PrCommentStatus<'_>,
) -> Option<ExitCode> {
    let provider = match output {
        OutputFormat::PrCommentGithub | OutputFormat::ReviewGithub => {
            ci::pr_comment::Provider::Github
        }
        OutputFormat::PrCommentGitlab | OutputFormat::ReviewGitlab => {
            ci::pr_comment::Provider::Gitlab
        }
        _ => return None,
    };
    let exit = if matches!(
        output,
        OutputFormat::PrCommentGithub | OutputFormat::PrCommentGitlab
    ) {
        ci::pr_comment::print_pr_comment_from_codeclimate_issues(
            analysis, provider, issues, conclusion, status,
        )
    } else {
        conclusion.map_or_else(
            || {
                ci::review::print_review_envelope_from_codeclimate_issues(
                    analysis,
                    provider,
                    issues,
                    status.into(),
                )
            },
            |conclusion| {
                ci::review::print_review_envelope_from_codeclimate_issues_with_conclusion(
                    analysis,
                    provider,
                    issues,
                    conclusion,
                    status.into(),
                )
            },
        )
    };
    Some(exit)
}

/// Note on stderr that this render target dropped the requested grouping, and
/// return the mode so the rendered body can say the same thing.
///
/// The two travel together on purpose: until issue #2691 three targets printed
/// the note, six said nothing at all, and no target put the fact in what it
/// rendered.
fn note_dropped_grouping(mode: Option<&str>, output: OutputFormat) -> Option<&str> {
    if let Some(mode) = mode {
        eprintln!(
            "note: --group-by {mode} is not supported for {format} output, falling back to \
             ungrouped output (use --format json for the full grouped envelope)",
            format = output.flag_label()
        );
    }
    mode
}

/// The requested `--group-by` mode when this target cannot carry it, noted on
/// stderr on the way out.
fn dropped_grouping_mode<'a>(ctx: &'a ReportContext<'_>, output: OutputFormat) -> Option<&'a str> {
    note_dropped_grouping(
        ctx.group_by
            .as_ref()
            .map(grouping::OwnershipResolver::mode_label),
        output,
    )
}

/// Print health (complexity) analysis results in the configured format.
///
/// `grouping` and `group_resolver` carry per-group output produced by
/// `--group-by`:
/// - **JSON** renders the grouped envelope (`{ grouped_by, vital_signs,
///   health_score, groups: [...] }`).
/// - **Human** prints a per-group summary block (score / files / hot / p90)
///   after the project-level report.
/// - **SARIF** and **CodeClimate** tag every per-finding result with the
///   resolver-derived group key (`properties.group` for SARIF, top-level
///   `group` for CodeClimate) so CI consumers like GitHub Code Scanning
///   and GitLab Code Quality can partition findings per team / package
///   without re-parsing the project structure.
/// - **Markdown** and **GitHub summary** add a `## Health by <mode>` table
///   and one collapsible findings block per group after the project section.
/// - **Compact**, **Badge**, **GitHub annotations** and the PR comment and
///   review formats fall back to ungrouped output and emit a one-line stderr
///   note pointing at `--format json` for the richer grouped envelope.
#[must_use]
pub(crate) fn print_health_report(
    report: &fallow_output::HealthReport,
    grouping: Option<&fallow_output::HealthGrouping>,
    group_resolver: Option<&grouping::OwnershipResolver>,
    ctx: &ReportContext<'_>,
    output: OutputFormat,
) -> ExitCode {
    match output {
        OutputFormat::Human => {
            print_health_human_report(report, grouping, ctx);
            ExitCode::SUCCESS
        }
        OutputFormat::Compact => {
            compact::print_health_compact(report, ctx.root);
            compact::print_type_aware_compact(ctx.type_aware, ctx.type_aware_scope);
            dropped_health_grouping_mode(grouping, output);
            ExitCode::SUCCESS
        }
        OutputFormat::Markdown => {
            markdown::print_health_markdown(report, ctx.root);
            if let Some(grouping) = grouping {
                markdown::print_health_grouping_markdown(grouping, ctx.root);
            }
            markdown::print_type_aware_markdown(ctx.type_aware, ctx.type_aware_scope);
            ExitCode::SUCCESS
        }
        OutputFormat::Sarif => match group_resolver {
            Some(resolver) => {
                sarif::print_grouped_health_sarif(report, ctx.root, resolver, ctx.type_aware)
            }
            None => sarif::print_health_sarif(report, ctx.root, ctx.type_aware),
        },
        OutputFormat::Json => match grouping {
            Some(grouping) => json::print_grouped_health_json(
                report,
                grouping,
                ctx.root,
                ctx.elapsed,
                ctx.explain,
                ctx.type_aware,
                ctx.workspace_diagnostics,
                ctx.json_style,
                ctx.gate_outcomes.clone(),
            ),
            None => json::print_health_json(
                report,
                ctx.root,
                ctx.elapsed,
                ctx.explain,
                ctx.type_aware,
                ctx.workspace_diagnostics,
                ctx.json_style,
                ctx.gate_outcomes.clone(),
            ),
        },
        OutputFormat::CodeClimate => match group_resolver {
            Some(resolver) => {
                codeclimate::print_grouped_health_codeclimate(report, ctx.root, resolver)
            }
            None => codeclimate::print_health_codeclimate(report, ctx.root),
        },
        OutputFormat::PrCommentGithub
        | OutputFormat::PrCommentGitlab
        | OutputFormat::ReviewGithub
        | OutputFormat::ReviewGitlab => print_health_ci_comment(
            report,
            ctx.root,
            output,
            ctx,
            dropped_health_grouping_mode(grouping, output),
        ),
        // The GitHub-native formats have no grouping concept, so they render
        // the ungrouped document and say so on stderr. There is nowhere in an
        // annotation stream or a job summary to put the fact (issue #2691).
        OutputFormat::GithubAnnotations => {
            dropped_health_grouping_mode(grouping, output);
            print_health_github_format(report, None, ctx, GithubTarget::Annotations)
        }
        // The job summary renders the grouped envelope, so it carries the
        // per-group table that `report --from` renders from a saved envelope.
        OutputFormat::GithubSummary => {
            print_health_github_format(report, grouping, ctx, GithubTarget::Summary)
        }
        OutputFormat::Badge => {
            dropped_health_grouping_mode(grouping, output);
            badge::print_health_badge(report)
        }
    }
}

/// Render health results in a GitHub-native format from the same JSON
/// envelope `--format json` serializes.
fn print_health_github_format(
    report: &fallow_output::HealthReport,
    grouping: Option<&fallow_output::HealthGrouping>,
    ctx: &ReportContext<'_>,
    target: GithubTarget,
) -> ExitCode {
    let document = match grouping {
        Some(grouping) => json::api_grouped_health_json_document(
            report,
            grouping,
            ctx.root,
            ctx.elapsed,
            ctx.explain,
            ctx.type_aware,
            ctx.workspace_diagnostics,
            ctx.gate_outcomes.clone(),
        ),
        None => json::api_health_json_document(
            report,
            ctx.root,
            ctx.elapsed,
            ctx.explain,
            ctx.type_aware,
            ctx.workspace_diagnostics,
            ctx.gate_outcomes.clone(),
        ),
    };
    match document {
        Ok(envelope) => print_github_format(
            github_annotations::EnvelopeKind::Health,
            &envelope,
            ctx.root,
            target,
        ),
        Err(e) => {
            eprintln!("Error: failed to serialize health report: {e}");
            exit_invalid_input()
        }
    }
}

/// Render the human-format health report, including the per-group summary block.
fn print_health_human_report(
    report: &fallow_output::HealthReport,
    grouping: Option<&fallow_output::HealthGrouping>,
    ctx: &ReportContext<'_>,
) {
    if ctx.summary {
        human::health::print_health_summary(report, ctx.elapsed, ctx.quiet, ctx.summary_heading);
        return;
    }
    human::print_health_human(&human::PrintHealthHumanInput {
        report,
        root: ctx.root,
        elapsed: ctx.elapsed,
        quiet: ctx.quiet,
        show_explain_tip: ctx.show_explain_tip,
        explain: ctx.explain,
        skip_score_and_trend: ctx.skip_score_and_trend,
        css_requested: ctx.css_requested,
        type_aware: ctx.type_aware,
        run_fails: run_fails(ctx),
        parse_degraded: &crate::gates::parse_degraded_files(ctx.root, ctx.workspace_diagnostics),
    });
    if let Some(grouping) = grouping {
        human::print_health_grouping(grouping, ctx.root, ctx.quiet);
    }
}

/// Render the CI comment / review fallback arms for health results.
fn print_health_ci_comment(
    report: &fallow_output::HealthReport,
    root: &Path,
    output: OutputFormat,
    ctx: &ReportContext<'_>,
    grouping_dropped: Option<&str>,
) -> ExitCode {
    let issues = codeclimate::api_health_codeclimate_issues(report, root);
    // Health's own summary is the carrier on the envelope, and the report in
    // hand is the same object, so read it from there rather than from the
    // context: a grouped render rebuilds the report but not the context.
    let advisory = baseline_advisory_text::advisory_line_for_staleness(
        report.summary.baseline_staleness.as_ref(),
    );
    let requests = crate::requests::request_outcomes();
    let note = ci_status_note(
        None,
        advisory.as_deref(),
        ctx.gate_outcomes.as_ref(),
        requests.as_ref(),
        grouping_dropped,
    );
    print_ci_comment_format_with_status(
        "health",
        &issues,
        output,
        None,
        ci::pr_comment::PrCommentStatus {
            message: note.as_deref(),
            gates: &gate_outcome_text::gate_rows_for_gates(ctx.gate_outcomes.as_ref()),
            config_patterns: &[],
        },
    )
    .unwrap_or_else(|| {
        eprintln!("Error: badge format is only supported for the health command");
        exit_invalid_input()
    })
}

/// The requested health `--group-by` mode when this target cannot carry it.
///
/// Health threads its grouping through its own parameter rather than
/// `ReportContext::group_by`, which standalone `fallow health` leaves unset.
fn dropped_health_grouping_mode(
    grouping: Option<&fallow_output::HealthGrouping>,
    output: OutputFormat,
) -> Option<&str> {
    note_dropped_grouping(grouping.map(|grouping| grouping.mode), output)
}

/// Print cross-reference findings (duplicated code that is also dead code).
///
/// Only emits output in human format to avoid corrupting structured JSON/SARIF output.
pub(crate) fn print_cross_reference_findings(
    cross_ref: &fallow_engine::cross_reference::CrossReferenceResult,
    root: &Path,
    quiet: bool,
    output: OutputFormat,
) {
    human::print_cross_reference_findings(cross_ref, root, quiet, output);
}

/// Print export trace results.
pub(crate) fn print_export_trace(
    trace: &ExportTrace,
    format: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace, json_style),
        _ => human::print_export_trace_human(trace),
    }
}

/// Print a syntactic export trace with its authoritative checker-backed
/// semantic section and optional field definitions.
pub(crate) fn print_semantic_export_trace(
    trace: &ExportTrace,
    format: OutputFormat,
    explain: bool,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => json::print_semantic_trace_json(trace, explain, json_style),
        _ => human::print_export_trace_human(trace),
    }
}

/// Print class-member trace results (the `--trace FILE:MEMBER` fallback).
pub(crate) fn print_class_member_trace(
    trace: &fallow_engine::trace::ClassMemberTrace,
    format: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace, json_style),
        _ => human::print_class_member_trace_human(trace),
    }
}

/// Print a class-member trace with authoritative checker-backed evidence and
/// optional field definitions.
pub(crate) fn print_semantic_class_member_trace(
    trace: &fallow_engine::trace::ClassMemberTrace,
    format: OutputFormat,
    explain: bool,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => json::print_semantic_trace_json(trace, explain, json_style),
        _ => human::print_class_member_trace_human(trace),
    }
}

/// Print file trace results.
pub(crate) fn print_file_trace(
    trace: &FileTrace,
    format: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace, json_style),
        _ => human::print_file_trace_human(trace),
    }
}

/// Print dependency trace results.
pub(crate) fn print_dependency_trace(
    trace: &DependencyTrace,
    format: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace, json_style),
        _ => human::print_dependency_trace_human(trace),
    }
}

/// Print `fallow trace --dependency` as human output: the dependency trace,
/// then the usage by imported name.
pub(crate) fn print_dependency_usage_trace_human(trace: &DependencyTrace, quiet: bool) {
    human::print_dependency_usage_trace_human(trace, quiet);
}

/// Print clone trace results.
pub(crate) fn print_clone_trace(
    trace: &CloneTrace,
    root: &Path,
    format: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace, json_style),
        _ => human::print_clone_trace_human(trace, root),
    }
}

/// Print impact-closure trace results. JSON only emits the structured
/// closure; human renders a short summary.
pub(crate) fn print_impact_closure_trace(
    trace: &ImpactClosureTrace,
    format: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace, json_style),
        _ => {
            outln!("Impact closure for {}", trace.seed);
            outln!(
                "  affected beyond the diff: {} file{}",
                trace.affected_not_shown.len(),
                plural(trace.affected_not_shown.len())
            );
            for gap in &trace.coordination_gap {
                outln!(
                    "  coordination gap: {} consumes {}{}",
                    gap.consumer_file,
                    gap.consumed_symbols.join(", "),
                    human::traces::gap_search_hint(gap)
                );
            }
        }
    }
}

/// Print exact-symbol impact and targeted-test recommendations.
pub(crate) fn print_symbol_impact(
    impact: &SemanticSymbolImpact,
    format: OutputFormat,
    explain: bool,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => json::print_semantic_impact_json(impact, explain, json_style),
        _ => human::print_symbol_impact_human(impact),
    }
}

/// The `--performance` JSON document: the stage timings plus the span tree
/// and the process clock, as additive fields.
#[derive(serde::Serialize)]
struct PerformanceJson<'a> {
    #[serde(flatten)]
    timings: &'a PipelineTimings,
    spans: Vec<fallow_types::pipeline_spans::PipelineSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    process: Option<fallow_types::pipeline_spans::ProcessTimings>,
}

/// Print pipeline performance timings.
///
/// `process` is the process clock. Only a caller that clocks every process
/// span, the report output included, passes it. Other callers pass `None`, so
/// the report never shows an unclocked span as `0.0ms`.
/// In JSON mode, outputs to stderr to avoid polluting the JSON analysis output on stdout.
pub(crate) fn print_performance(
    timings: &PipelineTimings,
    process: Option<fallow_types::pipeline_spans::ProcessTimings>,
    duplication_concurrent: bool,
    format: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => {
            let document = PerformanceJson {
                timings,
                spans: fallow_types::pipeline_spans::pipeline_span_tree(
                    fallow_types::pipeline_spans::SpanTreeInput {
                        timings,
                        process: process.as_ref(),
                        duplication_concurrent,
                    },
                ),
                process,
            };
            match json_style.serialize(&document) {
                Ok(json) => eprintln!("{json}"),
                Err(e) => eprintln!("Error: failed to serialize timings: {e}"),
            }
        }
        _ => human::print_performance_human(timings, process.as_ref(), duplication_concurrent),
    }
}

/// Print health pipeline performance timings.
/// In JSON mode, outputs to stderr to avoid polluting the JSON analysis output on stdout.
pub(crate) fn print_health_performance(
    timings: &fallow_output::HealthTimings,
    format: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) {
    match format {
        OutputFormat::Json => match json_style.serialize(timings) {
            Ok(json) => eprintln!("{json}"),
            Err(e) => eprintln!("Error: failed to serialize timings: {e}"),
        },
        _ => human::print_health_performance_human(timings),
    }
}

pub use fallow_api::build_compact_lines;
pub use fallow_api::build_duplication_markdown;
pub use fallow_api::build_health_markdown;
pub use fallow_api::build_markdown;
pub(crate) use json::api_check_json_payload_with_config_fixable;
pub(crate) use json::{build_baseline_deltas_output, check_json_extras};
pub(crate) use sarif::api_health_sarif_document;
pub(crate) use sarif::api_sarif_document;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
