mod badge;
pub mod ci;
mod codeclimate;
mod compact;
pub mod dupes_grouping;
pub mod grouping;
mod human;
mod json;
mod markdown;
mod sarif;
mod shared;
pub mod sink;
pub mod suggestions;
#[cfg(test)]
pub mod test_helpers;

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use fallow_api::DuplicationGrouping;
use fallow_config::{OutputFormat, RulesConfig, Severity};
use fallow_engine::duplicates::DuplicationReport;
use fallow_engine::trace::{CloneTrace, DependencyTrace, ExportTrace, FileTrace, PipelineTimings};
use fallow_types::results::AnalysisResults;

use crate::report::sink::outln;

#[allow(
    unused_imports,
    reason = "used by binary crate modules (combined.rs, audit.rs)"
)]
pub use fallow_output::strip_root_prefix;
pub use grouping::OwnershipResolver;
pub use human::health::{render_health_score, render_health_trend};

/// The three line-groups of a human `fallow review --walkthrough` render: the
/// orientation header and final status (stderr), and the staged tour body
/// (stdout). The entry point in `audit_brief.rs` owns the stream split; this
/// keeps the pure line builder behind the private `human` module while exposing
/// exactly what the entry point needs.
pub struct WalkthroughHumanRender {
    /// Review Focus orientation header lines (stderr).
    pub header: Vec<String>,
    /// The staged tour body lines (stdout).
    pub body: Vec<String>,
    /// The final green status line (stderr).
    pub status: String,
}

/// Build the human walkthrough tour from the in-memory guide. Pure: no IO, no
/// mutation. `viewed` decorates each file row; `show_cleared` expands the
/// Cleared panel.
#[must_use]
pub fn build_walkthrough_human(
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
        header: human::walkthrough::build_focus_header(guide),
        body: human::walkthrough::build_walkthrough_human_lines(&input),
        status: human::walkthrough::build_status_line(guide),
    }
}

/// Shared context for all report dispatch functions.
///
/// Bundles the common parameters that every format renderer needs,
/// replacing per-parameter threading through the dispatch match arms.
pub struct ReportContext<'a> {
    pub root: &'a Path,
    pub rules: &'a RulesConfig,
    pub elapsed: Duration,
    pub quiet: bool,
    pub explain: bool,
    /// When set, group all output by this resolver.
    pub group_by: Option<OwnershipResolver>,
    /// Limit displayed items per section (--top N).
    pub top: Option<usize>,
    /// When set, print a concise summary instead of the full report.
    pub summary: bool,
    /// Human-only: print the summary renderer's own title line. Combined mode
    /// already prints section headers, so it disables this to avoid duplicate
    /// "Dead Code" / "Dead Code Summary" headings.
    pub summary_heading: bool,
    /// Human-only: print a one-line hint pointing at `fallow explain`.
    pub show_explain_tip: bool,
    /// When a baseline was loaded: (total entries in baseline, entries that matched).
    pub baseline_matched: Option<(usize, usize)>,
    /// Whether config-edit actions can be applied by `fallow fix`.
    ///
    /// This is caller-provided because an explicit `--config` path is fixable
    /// even when default config discovery from the root would find nothing.
    pub config_fixable: bool,
    /// When set, the human health renderer skips the `● Health score:` and
    /// trend table sections because they have already been rendered upstream
    /// (combined-mode orientation header). Standalone `fallow health` keeps
    /// the default `false` and renders both sections inline.
    pub skip_score_and_trend: bool,
}

/// Strip the project root prefix from a path for display, falling back to the full path.
#[must_use]
pub fn relative_path<'a>(path: &'a Path, root: &Path) -> &'a Path {
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
pub fn format_display_path(path: &Path, root: &Path) -> String {
    relative_path(path, root)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// Split a path string into (directory, filename) for display.
/// Directory includes the trailing `/`. If no directory, returns `("", filename)`.
#[must_use]
pub fn split_dir_filename(path: &str) -> (&str, &str) {
    path.rfind('/')
        .map_or(("", path), |pos| (&path[..=pos], &path[pos + 1..]))
}

/// Return `"s"` for plural or `""` for singular.
#[must_use]
pub const fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Serialize a JSON value to pretty-printed stdout, returning the appropriate exit code.
///
/// On success prints the JSON and returns `ExitCode::SUCCESS`.
/// On serialization failure prints an error to stderr and returns exit code 2.
#[must_use]
pub fn emit_json(value: &serde_json::Value, kind: &str) -> ExitCode {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            outln!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: failed to serialize {kind} output: {e}");
            ExitCode::from(2)
        }
    }
}

/// Elide the common directory prefix between a base path and a target path.
/// Only strips complete directory segments (never partial filenames).
/// Returns the remaining suffix of `target`.
///
/// Example: `elide_common_prefix("a/b/c/foo.ts", "a/b/d/bar.ts")` → `"d/bar.ts"`
#[must_use]
pub fn elide_common_prefix<'a>(base: &str, target: &'a str) -> &'a str {
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
pub fn normalize_uri(path_str: &str) -> String {
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
pub const fn severity_to_level(s: Severity) -> Level {
    match s {
        Severity::Error => Level::Error,
        Severity::Warn => Level::Warn,
        Severity::Off => Level::Info,
    }
}

/// Print analysis results in the configured format.
/// Returns exit code 2 if serialization fails, SUCCESS otherwise.
///
/// When `regression` is `Some`, the JSON format includes a `regression` key in the output envelope.
/// When `ctx.group_by` is `Some`, results are partitioned into labeled groups before rendering.
#[must_use]
pub fn print_results(
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
                    ctx.summary_heading,
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
                });
            }
            ExitCode::SUCCESS
        }
        OutputFormat::Json => json::print_json(&json::PrintJsonInput {
            results,
            root: ctx.root,
            elapsed: ctx.elapsed,
            explain: ctx.explain,
            regression,
            baseline_matched: ctx.baseline_matched,
            config_fixable: ctx.config_fixable,
        }),
        OutputFormat::Compact => {
            compact::print_compact(results, ctx.root);
            ExitCode::SUCCESS
        }
        OutputFormat::Sarif => sarif::print_sarif(results, ctx.root, ctx.rules),
        OutputFormat::Markdown => {
            markdown::print_markdown(results, ctx.root);
            ExitCode::SUCCESS
        }
        OutputFormat::CodeClimate => codeclimate::print_codeclimate(results, ctx.root, ctx.rules),
        ci_format => print_results_ci_comment(results, ctx, ci_format),
    }
}

/// Render the CI comment / review / badge fallback arms for dead-code results.
fn print_results_ci_comment(
    results: &AnalysisResults,
    ctx: &ReportContext<'_>,
    output: OutputFormat,
) -> ExitCode {
    let issues = codeclimate::api_codeclimate_issues(results, ctx.root, ctx.rules);
    let value = fallow_output::codeclimate_issues_to_value(&issues);
    print_ci_comment_format("dead-code", &value, output).unwrap_or_else(|| {
        eprintln!("Error: badge format is only supported for the health command");
        ExitCode::from(2)
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
            });
            ExitCode::SUCCESS
        }
        OutputFormat::Json => json::print_grouped_json(&json::PrintGroupedJsonInput {
            groups,
            original,
            root: ctx.root,
            elapsed: ctx.elapsed,
            explain: ctx.explain,
            resolver,
            config_fixable: ctx.config_fixable,
        }),
        OutputFormat::Compact => {
            compact::print_grouped_compact(groups, ctx.root);
            ExitCode::SUCCESS
        }
        OutputFormat::Markdown => {
            markdown::print_grouped_markdown(groups, ctx.root);
            ExitCode::SUCCESS
        }
        OutputFormat::Sarif => sarif::print_grouped_sarif(original, ctx.root, ctx.rules, resolver),
        OutputFormat::CodeClimate => {
            codeclimate::print_grouped_codeclimate(original, ctx.root, ctx.rules, resolver)
        }
        ci_format => print_results_ci_comment(original, ctx, ci_format),
    }
}

/// Print duplication analysis results in the configured format.
#[must_use]
pub fn print_duplication_report(
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
                );
            } else {
                human::print_duplication_human(
                    report,
                    ctx.root,
                    ctx.elapsed,
                    ctx.quiet,
                    ctx.show_explain_tip,
                    ctx.explain,
                );
            }
            ExitCode::SUCCESS
        }
        OutputFormat::Json => {
            json::print_duplication_json(report, ctx.root, ctx.elapsed, ctx.explain)
        }
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
        ci_format => print_duplication_ci_comment(report, ctx.root, ci_format),
    }
}

/// Render the CI comment / review / badge fallback arms for duplication results.
fn print_duplication_ci_comment(
    report: &DuplicationReport,
    root: &Path,
    output: OutputFormat,
) -> ExitCode {
    let issues = codeclimate::api_duplication_codeclimate_issues(report, root);
    let value = fallow_output::codeclimate_issues_to_value(&issues);
    print_ci_comment_format("dupes", &value, output).unwrap_or_else(|| {
        eprintln!("Error: badge format is only supported for the health command");
        ExitCode::from(2)
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
            );
            ExitCode::SUCCESS
        }
        OutputFormat::Json => json::print_grouped_duplication_json(
            report,
            grouping,
            ctx.root,
            ctx.elapsed,
            ctx.explain,
        ),
        OutputFormat::Sarif => sarif::print_grouped_duplication_sarif(report, ctx.root, resolver),
        OutputFormat::CodeClimate => {
            codeclimate::print_grouped_duplication_codeclimate(report, ctx.root, resolver)
        }
        OutputFormat::PrCommentGithub
        | OutputFormat::PrCommentGitlab
        | OutputFormat::ReviewGithub
        | OutputFormat::ReviewGitlab => print_duplication_ci_comment(report, ctx.root, output),
        OutputFormat::Compact => {
            compact::print_duplication_compact(report, ctx.root);
            warn_dupes_grouping_unsupported(grouping, "compact");
            ExitCode::SUCCESS
        }
        OutputFormat::Markdown => {
            markdown::print_duplication_markdown(report, ctx.root);
            warn_dupes_grouping_unsupported(grouping, "markdown");
            ExitCode::SUCCESS
        }
        OutputFormat::Badge => {
            eprintln!("Error: badge format is only supported for the health command");
            ExitCode::from(2)
        }
    }
}

/// Dispatch a PR-comment / review CI format from a precomputed CodeClimate value.
///
/// Returns `Some(exit_code)` for the four CI comment/review formats and `None`
/// for every other output format, so callers keep their exhaustive match arms.
fn print_ci_comment_format(
    analysis: &str,
    value: &serde_json::Value,
    output: OutputFormat,
) -> Option<ExitCode> {
    let exit = match output {
        OutputFormat::PrCommentGithub => {
            ci::pr_comment::print_pr_comment(analysis, ci::pr_comment::Provider::Github, value)
        }
        OutputFormat::PrCommentGitlab => {
            ci::pr_comment::print_pr_comment(analysis, ci::pr_comment::Provider::Gitlab, value)
        }
        OutputFormat::ReviewGithub => {
            ci::review::print_review_envelope(analysis, ci::pr_comment::Provider::Github, value)
        }
        OutputFormat::ReviewGitlab => {
            ci::review::print_review_envelope(analysis, ci::pr_comment::Provider::Gitlab, value)
        }
        _ => return None,
    };
    Some(exit)
}

fn warn_dupes_grouping_unsupported(grouping: &DuplicationGrouping, format: &str) {
    eprintln!(
        "note: --group-by {} is not supported for {format} duplication output, falling back to \
         ungrouped output (use --format json for the full grouped envelope)",
        grouping.mode
    );
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
/// - **Compact**, **Markdown**, and **Badge** fall back to ungrouped output
///   and emit a one-line stderr note pointing at `--format json` for the
///   richer grouped envelope.
#[must_use]
pub fn print_health_report(
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
            warn_grouping_unsupported(grouping, "compact");
            ExitCode::SUCCESS
        }
        OutputFormat::Markdown => {
            markdown::print_health_markdown(report, ctx.root);
            warn_grouping_unsupported(grouping, "markdown");
            ExitCode::SUCCESS
        }
        OutputFormat::Sarif => match group_resolver {
            Some(resolver) => sarif::print_grouped_health_sarif(report, ctx.root, resolver),
            None => sarif::print_health_sarif(report, ctx.root),
        },
        OutputFormat::Json => match grouping {
            Some(grouping) => json::print_grouped_health_json(
                report,
                grouping,
                ctx.root,
                ctx.elapsed,
                ctx.explain,
            ),
            None => json::print_health_json(report, ctx.root, ctx.elapsed, ctx.explain),
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
        | OutputFormat::ReviewGitlab => print_health_ci_comment(report, ctx.root, output),
        OutputFormat::Badge => {
            warn_grouping_unsupported(grouping, "badge");
            badge::print_health_badge(report)
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
) -> ExitCode {
    let issues = codeclimate::api_health_codeclimate_issues(report, root);
    let value = fallow_output::codeclimate_issues_to_value(&issues);
    print_ci_comment_format("health", &value, output).unwrap_or_else(|| {
        eprintln!("Error: badge format is only supported for the health command");
        ExitCode::from(2)
    })
}

fn warn_grouping_unsupported(grouping: Option<&fallow_output::HealthGrouping>, format: &str) {
    if let Some(g) = grouping {
        eprintln!(
            "note: --group-by {} is not supported for {format} output, falling back to \
             ungrouped output (use --format json for the full grouped envelope)",
            g.mode
        );
    }
}

/// Print cross-reference findings (duplicated code that is also dead code).
///
/// Only emits output in human format to avoid corrupting structured JSON/SARIF output.
pub fn print_cross_reference_findings(
    cross_ref: &fallow_engine::cross_reference::CrossReferenceResult,
    root: &Path,
    quiet: bool,
    output: OutputFormat,
) {
    human::print_cross_reference_findings(cross_ref, root, quiet, output);
}

/// Print export trace results.
pub fn print_export_trace(trace: &ExportTrace, format: OutputFormat) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace),
        _ => human::print_export_trace_human(trace),
    }
}

/// Print file trace results.
pub fn print_file_trace(trace: &FileTrace, format: OutputFormat) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace),
        _ => human::print_file_trace_human(trace),
    }
}

/// Print dependency trace results.
pub fn print_dependency_trace(trace: &DependencyTrace, format: OutputFormat) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace),
        _ => human::print_dependency_trace_human(trace),
    }
}

/// Print clone trace results.
pub fn print_clone_trace(trace: &CloneTrace, root: &Path, format: OutputFormat) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace),
        _ => human::print_clone_trace_human(trace, root),
    }
}

/// Print impact-closure trace results. JSON only emits the structured
/// closure; human renders a short summary.
pub fn print_impact_closure_trace(
    trace: &fallow_engine::trace::ImpactClosureTrace,
    format: OutputFormat,
) {
    match format {
        OutputFormat::Json => json::print_trace_json(trace),
        _ => {
            outln!("Impact closure for {}", trace.seed);
            outln!(
                "  affected beyond the diff: {} file{}",
                trace.affected_not_shown.len(),
                plural(trace.affected_not_shown.len())
            );
            for gap in &trace.coordination_gap {
                outln!(
                    "  coordination gap: {} consumes {}",
                    gap.consumer_file,
                    gap.consumed_symbols.join(", ")
                );
            }
        }
    }
}

/// Print pipeline performance timings.
/// In JSON mode, outputs to stderr to avoid polluting the JSON analysis output on stdout.
pub fn print_performance(timings: &PipelineTimings, format: OutputFormat) {
    match format {
        OutputFormat::Json => match serde_json::to_string_pretty(timings) {
            Ok(json) => eprintln!("{json}"),
            Err(e) => eprintln!("Error: failed to serialize timings: {e}"),
        },
        _ => human::print_performance_human(timings),
    }
}

/// Print health pipeline performance timings.
/// In JSON mode, outputs to stderr to avoid polluting the JSON analysis output on stdout.
pub fn print_health_performance(timings: &fallow_output::HealthTimings, format: OutputFormat) {
    match format {
        OutputFormat::Json => match serde_json::to_string_pretty(timings) {
            Ok(json) => eprintln!("{json}"),
            Err(e) => eprintln!("Error: failed to serialize timings: {e}"),
        },
        _ => human::print_health_performance_human(timings),
    }
}

#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use fallow_api::build_compact_lines;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use fallow_api::build_duplication_markdown;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use fallow_api::build_health_markdown;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use fallow_api::build_markdown;
#[allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) deliberately limits visibility, report is pub but these are internal"
)]
pub(crate) use json::SCHEMA_VERSION;
#[allow(
    clippy::redundant_pub_crate,
    reason = "target-dependent: report is public in lib, private in bin, but this adapter remains crate-internal"
)]
pub(crate) use json::api_check_json_payload_with_config_fixable;
#[allow(
    unused_imports,
    reason = "target-dependent: used in bin audit.rs, unused in lib"
)]
#[allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) deliberately limits visibility, report is pub but these are internal"
)]
pub(crate) use json::harmonize_multi_kind_suppress_line_actions;
#[allow(
    clippy::redundant_pub_crate,
    reason = "target-dependent: report is public in lib, private in bin, but these adapters remain crate-internal"
)]
pub(crate) use json::{build_baseline_deltas_output, check_json_extras};
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
#[allow(
    clippy::redundant_pub_crate,
    reason = "target-dependent: report is public in lib, private in bin, but this adapter remains crate-internal"
)]
pub(crate) use sarif::api_health_sarif_document;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
#[allow(
    clippy::redundant_pub_crate,
    reason = "target-dependent: report is public in lib, private in bin, but this adapter remains crate-internal"
)]
pub(crate) use sarif::api_sarif_document;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn test_context<'a>(root: &'a Path, rules: &'a RulesConfig) -> ReportContext<'a> {
        ReportContext {
            root,
            rules,
            elapsed: Duration::default(),
            quiet: true,
            explain: false,
            group_by: None,
            top: None,
            summary: false,
            summary_heading: false,
            show_explain_tip: false,
            baseline_matched: None,
            config_fixable: false,
            skip_score_and_trend: false,
        }
    }

    #[test]
    fn normalize_uri_forward_slashes_unchanged() {
        assert_eq!(normalize_uri("src/utils.ts"), "src/utils.ts");
    }

    #[test]
    fn normalize_uri_backslashes_replaced() {
        assert_eq!(normalize_uri("src\\utils\\index.ts"), "src/utils/index.ts");
    }

    #[test]
    fn normalize_uri_mixed_slashes() {
        assert_eq!(normalize_uri("src\\utils/index.ts"), "src/utils/index.ts");
    }

    #[test]
    fn normalize_uri_path_with_spaces() {
        assert_eq!(
            normalize_uri("src\\my folder\\file.ts"),
            "src/my folder/file.ts"
        );
    }

    #[test]
    fn normalize_uri_empty_string() {
        assert_eq!(normalize_uri(""), "");
    }

    #[test]
    fn relative_path_strips_root_prefix() {
        let root = Path::new("/project");
        let path = Path::new("/project/src/utils.ts");
        assert_eq!(relative_path(path, root), Path::new("src/utils.ts"));
    }

    #[test]
    fn relative_path_returns_full_path_when_no_prefix() {
        let root = Path::new("/other");
        let path = Path::new("/project/src/utils.ts");
        assert_eq!(relative_path(path, root), path);
    }

    #[test]
    fn relative_path_at_root_returns_empty_or_file() {
        let root = Path::new("/project");
        let path = Path::new("/project/file.ts");
        assert_eq!(relative_path(path, root), Path::new("file.ts"));
    }

    #[test]
    fn relative_path_deeply_nested() {
        let root = Path::new("/project");
        let path = Path::new("/project/packages/ui/src/components/Button.tsx");
        assert_eq!(
            relative_path(path, root),
            Path::new("packages/ui/src/components/Button.tsx")
        );
    }

    #[test]
    fn format_display_path_returns_workspace_relative() {
        let root = Path::new("/project");
        let path = Path::new("/project/apps/server/src/index.ts");
        assert_eq!(format_display_path(path, root), "apps/server/src/index.ts");
    }

    #[test]
    fn format_display_path_collides_in_nx_layout_renders_full_relative() {
        let root = Path::new("/project");
        let server = Path::new("/project/apps/server/src/index.ts");
        let client = Path::new("/project/apps/client/src/index.ts");
        assert_eq!(
            format_display_path(server, root),
            "apps/server/src/index.ts"
        );
        assert_eq!(
            format_display_path(client, root),
            "apps/client/src/index.ts"
        );
    }

    #[test]
    fn format_display_path_angular_component_renders_parent_directory() {
        let root = Path::new("/project");
        let path = Path::new(
            "/project/apps/admin/src/app/payments/payment-list/payment-list.component.html",
        );
        assert_eq!(
            format_display_path(path, root),
            "apps/admin/src/app/payments/payment-list/payment-list.component.html"
        );
    }

    #[test]
    fn format_display_path_falls_back_to_full_path_when_root_does_not_prefix() {
        let root = Path::new("/other");
        let path = Path::new("/project/src/utils.ts");
        let rendered = format_display_path(path, root);
        assert!(rendered.contains("project"));
        assert!(rendered.ends_with("utils.ts"));
        assert!(!rendered.contains('\\'));
    }

    #[test]
    fn format_display_path_normalizes_backslashes_to_forward_slashes() {
        let root = Path::new("/project");
        let path = Path::new("/project/src/sub\\file.ts");
        let rendered = format_display_path(path, root);
        assert!(
            !rendered.contains('\\'),
            "backslashes must be normalized: {rendered}"
        );
    }

    #[test]
    fn format_display_path_handles_brackets_verbatim() {
        let root = Path::new("/project");
        let path = Path::new("/project/app/[slug]/page.tsx");
        assert_eq!(format_display_path(path, root), "app/[slug]/page.tsx");
    }

    #[test]
    fn format_display_path_path_equals_root_returns_empty() {
        let root = Path::new("/project");
        let path = Path::new("/project");
        assert_eq!(format_display_path(path, root), "");
    }

    #[test]
    fn format_display_path_basename_only_when_path_is_at_root() {
        let root = Path::new("/project");
        let path = Path::new("/project/Cargo.toml");
        assert_eq!(format_display_path(path, root), "Cargo.toml");
    }

    #[test]
    fn relative_uri_produces_forward_slash_path() {
        let root = PathBuf::from("/project");
        let path = root.join("src").join("utils.ts");
        let uri = relative_uri(&path, &root);
        assert_eq!(uri, "src/utils.ts");
    }

    #[test]
    fn relative_uri_encodes_brackets() {
        let root = PathBuf::from("/project");
        let path = root.join("src/app/[...slug]/page.tsx");
        let uri = relative_uri(&path, &root);
        assert_eq!(uri, "src/app/%5B...slug%5D/page.tsx");
    }

    #[test]
    fn relative_uri_encodes_nested_dynamic_routes() {
        let root = PathBuf::from("/project");
        let path = root.join("src/app/[slug]/[id]/page.tsx");
        let uri = relative_uri(&path, &root);
        assert_eq!(uri, "src/app/%5Bslug%5D/%5Bid%5D/page.tsx");
    }

    #[test]
    fn relative_uri_no_common_prefix_returns_full() {
        let root = PathBuf::from("/other");
        let path = PathBuf::from("/project/src/utils.ts");
        let uri = relative_uri(&path, &root);
        assert!(uri.contains("project"));
        assert!(uri.contains("utils.ts"));
    }

    #[test]
    fn severity_error_maps_to_level_error() {
        assert!(matches!(severity_to_level(Severity::Error), Level::Error));
    }

    #[test]
    fn severity_warn_maps_to_level_warn() {
        assert!(matches!(severity_to_level(Severity::Warn), Level::Warn));
    }

    #[test]
    fn severity_off_maps_to_level_info() {
        assert!(matches!(severity_to_level(Severity::Off), Level::Info));
    }

    #[test]
    fn normalize_uri_single_bracket_pair() {
        assert_eq!(normalize_uri("app/[id]/page.tsx"), "app/%5Bid%5D/page.tsx");
    }

    #[test]
    fn normalize_uri_catch_all_route() {
        assert_eq!(
            normalize_uri("app/[...slug]/page.tsx"),
            "app/%5B...slug%5D/page.tsx"
        );
    }

    #[test]
    fn normalize_uri_optional_catch_all_route() {
        assert_eq!(
            normalize_uri("app/[[...slug]]/page.tsx"),
            "app/%5B%5B...slug%5D%5D/page.tsx"
        );
    }

    #[test]
    fn normalize_uri_multiple_dynamic_segments() {
        assert_eq!(
            normalize_uri("app/[lang]/posts/[id]"),
            "app/%5Blang%5D/posts/%5Bid%5D"
        );
    }

    #[test]
    fn normalize_uri_no_special_chars() {
        let plain = "src/components/Button.tsx";
        assert_eq!(normalize_uri(plain), plain);
    }

    #[test]
    fn normalize_uri_only_backslashes() {
        assert_eq!(normalize_uri("a\\b\\c"), "a/b/c");
    }

    #[test]
    fn relative_path_identical_paths_returns_empty() {
        let root = Path::new("/project");
        assert_eq!(relative_path(root, root), Path::new(""));
    }

    #[test]
    fn relative_path_partial_name_match_not_stripped() {
        let root = Path::new("/project");
        let path = Path::new("/project-two/src/a.ts");
        assert_eq!(relative_path(path, root), path);
    }

    #[test]
    fn relative_uri_combines_stripping_and_encoding() {
        let root = PathBuf::from("/project");
        let path = root.join("src/app/[slug]/page.tsx");
        let uri = relative_uri(&path, &root);
        assert_eq!(uri, "src/app/%5Bslug%5D/page.tsx");
        assert!(!uri.starts_with('/'));
    }

    #[test]
    fn relative_uri_at_root_file() {
        let root = PathBuf::from("/project");
        let path = root.join("index.ts");
        assert_eq!(relative_uri(&path, &root), "index.ts");
    }

    #[test]
    fn severity_to_level_is_const_evaluable() {
        const LEVEL_FROM_ERROR: Level = severity_to_level(Severity::Error);
        const LEVEL_FROM_WARN: Level = severity_to_level(Severity::Warn);
        const LEVEL_FROM_OFF: Level = severity_to_level(Severity::Off);
        assert!(matches!(LEVEL_FROM_ERROR, Level::Error));
        assert!(matches!(LEVEL_FROM_WARN, Level::Warn));
        assert!(matches!(LEVEL_FROM_OFF, Level::Info));
    }

    #[test]
    fn level_is_copy() {
        let level = severity_to_level(Severity::Error);
        let copy = level;
        assert!(matches!(level, Level::Error));
        assert!(matches!(copy, Level::Error));
    }

    #[test]
    fn print_results_rejects_badge_for_dead_code_reports() {
        let root = Path::new("/project");
        let rules = RulesConfig::default();
        let ctx = test_context(root, &rules);

        let code = print_results(&AnalysisResults::default(), &ctx, OutputFormat::Badge, None);

        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn print_duplication_report_rejects_badge_format() {
        let root = Path::new("/project");
        let rules = RulesConfig::default();
        let ctx = test_context(root, &rules);

        let code =
            print_duplication_report(&DuplicationReport::default(), &ctx, OutputFormat::Badge);

        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn elide_common_prefix_shared_dir() {
        assert_eq!(
            elide_common_prefix("src/components/A.tsx", "src/components/B.tsx"),
            "B.tsx"
        );
    }

    #[test]
    fn elide_common_prefix_partial_shared() {
        assert_eq!(
            elide_common_prefix("src/components/A.tsx", "src/utils/B.tsx"),
            "utils/B.tsx"
        );
    }

    #[test]
    fn elide_common_prefix_no_shared() {
        assert_eq!(
            elide_common_prefix("pkg-a/src/A.tsx", "pkg-b/src/B.tsx"),
            "pkg-b/src/B.tsx"
        );
    }

    #[test]
    fn elide_common_prefix_identical_files() {
        assert_eq!(elide_common_prefix("a/b/x.ts", "a/b/y.ts"), "y.ts");
    }

    #[test]
    fn elide_common_prefix_no_dirs() {
        assert_eq!(elide_common_prefix("foo.ts", "bar.ts"), "bar.ts");
    }

    #[test]
    fn elide_common_prefix_deep_monorepo() {
        assert_eq!(
            elide_common_prefix(
                "packages/rap/src/rap/components/SearchSelect/SearchSelect.tsx",
                "packages/rap/src/rap/components/SearchSelect/SearchSelectItem.tsx"
            ),
            "SearchSelectItem.tsx"
        );
    }

    #[test]
    fn split_dir_filename_with_dir() {
        let (dir, file) = split_dir_filename("src/utils/index.ts");
        assert_eq!(dir, "src/utils/");
        assert_eq!(file, "index.ts");
    }

    #[test]
    fn split_dir_filename_no_dir() {
        let (dir, file) = split_dir_filename("file.ts");
        assert_eq!(dir, "");
        assert_eq!(file, "file.ts");
    }

    #[test]
    fn split_dir_filename_deeply_nested() {
        let (dir, file) = split_dir_filename("a/b/c/d/e.ts");
        assert_eq!(dir, "a/b/c/d/");
        assert_eq!(file, "e.ts");
    }

    #[test]
    fn split_dir_filename_trailing_slash() {
        let (dir, file) = split_dir_filename("src/");
        assert_eq!(dir, "src/");
        assert_eq!(file, "");
    }

    #[test]
    fn split_dir_filename_empty() {
        let (dir, file) = split_dir_filename("");
        assert_eq!(dir, "");
        assert_eq!(file, "");
    }

    #[test]
    fn plural_zero_is_plural() {
        assert_eq!(plural(0), "s");
    }

    #[test]
    fn plural_one_is_singular() {
        assert_eq!(plural(1), "");
    }

    #[test]
    fn plural_two_is_plural() {
        assert_eq!(plural(2), "s");
    }

    #[test]
    fn plural_large_number() {
        assert_eq!(plural(999), "s");
    }

    #[test]
    fn elide_common_prefix_empty_base() {
        assert_eq!(elide_common_prefix("", "src/foo.ts"), "src/foo.ts");
    }

    #[test]
    fn elide_common_prefix_empty_target() {
        assert_eq!(elide_common_prefix("src/foo.ts", ""), "");
    }

    #[test]
    fn elide_common_prefix_both_empty() {
        assert_eq!(elide_common_prefix("", ""), "");
    }

    #[test]
    fn elide_common_prefix_same_file_different_extension() {
        assert_eq!(
            elide_common_prefix("src/utils.ts", "src/utils.js"),
            "utils.js"
        );
    }

    #[test]
    fn elide_common_prefix_partial_filename_match_not_stripped() {
        assert_eq!(
            elide_common_prefix("src/App.tsx", "src/AppUtils.tsx"),
            "AppUtils.tsx"
        );
    }

    #[test]
    fn elide_common_prefix_identical_paths() {
        assert_eq!(elide_common_prefix("src/foo.ts", "src/foo.ts"), "foo.ts");
    }

    #[test]
    fn split_dir_filename_single_slash() {
        let (dir, file) = split_dir_filename("/file.ts");
        assert_eq!(dir, "/");
        assert_eq!(file, "file.ts");
    }

    #[test]
    fn emit_json_returns_success_for_valid_value() {
        let value = serde_json::json!({"key": "value"});
        let code = emit_json(&value, "test");
        assert_eq!(code, ExitCode::SUCCESS);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// split_dir_filename always reconstructs the original path.
            #[test]
            fn split_dir_filename_reconstructs_path(path in "[a-zA-Z0-9_./\\-]{0,100}") {
                let (dir, file) = split_dir_filename(&path);
                let reconstructed = format!("{dir}{file}");
                prop_assert_eq!(
                    reconstructed, path,
                    "dir+file should reconstruct the original path"
                );
            }

            /// plural returns either "" or "s", nothing else.
            #[test]
            fn plural_returns_empty_or_s(n: usize) {
                let result = plural(n);
                prop_assert!(
                    result.is_empty() || result == "s",
                    "plural should return \"\" or \"s\", got {:?}",
                    result
                );
            }

            /// plural(1) is always "" and plural(n != 1) is always "s".
            #[test]
            fn plural_singular_only_for_one(n: usize) {
                let result = plural(n);
                if n == 1 {
                    prop_assert_eq!(result, "", "plural(1) should be empty");
                } else {
                    prop_assert_eq!(result, "s", "plural({}) should be \"s\"", n);
                }
            }

            /// normalize_uri never panics and always replaces backslashes.
            #[test]
            fn normalize_uri_no_backslashes(path in "[a-zA-Z0-9_.\\\\/ \\[\\]%-]{0,100}") {
                let result = normalize_uri(&path);
                prop_assert!(
                    !result.contains('\\'),
                    "Result should not contain backslashes: {result}"
                );
            }

            /// normalize_uri always encodes brackets.
            #[test]
            fn normalize_uri_encodes_all_brackets(path in "[a-zA-Z0-9_./\\[\\]%-]{0,80}") {
                let result = normalize_uri(&path);
                prop_assert!(
                    !result.contains('[') && !result.contains(']'),
                    "Result should not contain raw brackets: {result}"
                );
            }

            /// elide_common_prefix always returns a suffix of or equal to target.
            #[test]
            fn elide_common_prefix_returns_suffix_of_target(
                base in "[a-zA-Z0-9_./]{0,50}",
                target in "[a-zA-Z0-9_./]{0,50}",
            ) {
                let result = elide_common_prefix(&base, &target);
                prop_assert!(
                    target.ends_with(result),
                    "Result {:?} should be a suffix of target {:?}",
                    result, target
                );
            }

            /// relative_path never panics.
            #[test]
            fn relative_path_never_panics(
                root in "/[a-zA-Z0-9_/]{0,30}",
                suffix in "[a-zA-Z0-9_./]{0,30}",
            ) {
                let root_path = Path::new(&root);
                let full = PathBuf::from(format!("{root}/{suffix}"));
                let _ = relative_path(&full, root_path);
            }
        }
    }
}
