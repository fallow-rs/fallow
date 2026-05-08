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
#[cfg(test)]
pub mod test_helpers;

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use fallow_config::{OutputFormat, RulesConfig, Severity};
use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::AnalysisResults;
use fallow_core::trace::{CloneTrace, DependencyTrace, ExportTrace, FileTrace, PipelineTimings};

pub use grouping::OwnershipResolver;
#[allow(
    unused_imports,
    reason = "used by binary crate modules (combined.rs, audit.rs)"
)]
pub use json::strip_root_prefix;

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
    /// Human-only: print a one-line hint pointing at `fallow explain`.
    pub show_explain_tip: bool,
    /// When a baseline was loaded: (total entries in baseline, entries that matched).
    pub baseline_matched: Option<(usize, usize)>,
    /// JSON-only: controls emission of `suppress-line` action entries on
    /// health findings. Other formats ignore this field. Computed by the
    /// caller from `HealthResult::baseline_active` and
    /// `health.suggestInlineSuppression` config (see
    /// `crate::health::health_action_opts`).
    pub health_action_opts: HealthActionOptions,
}

/// Strip the project root prefix from a path for display, falling back to the full path.
#[must_use]
pub fn relative_path<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
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
            println!("{json}");
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
fn relative_uri(path: &Path, root: &Path) -> String {
    normalize_uri(&relative_path(path, root).display().to_string())
}

/// Normalize a path string to a valid URI: forward slashes and percent-encoded brackets.
///
/// Brackets (`[`, `]`) are not valid in URI path segments per RFC 3986 and cause
/// SARIF validation warnings (e.g., Next.js dynamic routes like `[slug]`).
#[must_use]
pub fn normalize_uri(path_str: &str) -> String {
    path_str
        .replace('\\', "/")
        .replace('[', "%5B")
        .replace(']', "%5D")
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
        // Off issues are filtered before reporting; fall back to Info.
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
    // Grouped output: partition results and render per-group
    if let Some(ref resolver) = ctx.group_by {
        let groups = grouping::group_analysis_results(results, ctx.root, resolver);
        return print_grouped_results(&groups, results, ctx, output, resolver);
    }

    match output {
        OutputFormat::Human => {
            if ctx.summary {
                human::check::print_check_summary(results, ctx.rules, ctx.elapsed, ctx.quiet);
            } else {
                human::print_human(
                    results,
                    ctx.root,
                    ctx.rules,
                    ctx.elapsed,
                    ctx.quiet,
                    ctx.top,
                    ctx.show_explain_tip,
                );
            }
            ExitCode::SUCCESS
        }
        OutputFormat::Json => json::print_json(
            results,
            ctx.root,
            ctx.elapsed,
            ctx.explain,
            regression,
            ctx.baseline_matched,
        ),
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
        OutputFormat::PrCommentGithub => {
            let value = codeclimate::build_codeclimate(results, ctx.root, ctx.rules);
            ci::pr_comment::print_pr_comment("dead-code", ci::pr_comment::Provider::Github, &value)
        }
        OutputFormat::PrCommentGitlab => {
            let value = codeclimate::build_codeclimate(results, ctx.root, ctx.rules);
            ci::pr_comment::print_pr_comment("dead-code", ci::pr_comment::Provider::Gitlab, &value)
        }
        OutputFormat::ReviewGithub => {
            let value = codeclimate::build_codeclimate(results, ctx.root, ctx.rules);
            ci::review::print_review_envelope("dead-code", ci::pr_comment::Provider::Github, &value)
        }
        OutputFormat::ReviewGitlab => {
            let value = codeclimate::build_codeclimate(results, ctx.root, ctx.rules);
            ci::review::print_review_envelope("dead-code", ci::pr_comment::Provider::Gitlab, &value)
        }
        OutputFormat::Badge => {
            eprintln!("Error: badge format is only supported for the health command");
            ExitCode::from(2)
        }
    }
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
            human::print_grouped_human(
                groups,
                ctx.root,
                ctx.rules,
                ctx.elapsed,
                ctx.quiet,
                Some(resolver),
            );
            ExitCode::SUCCESS
        }
        OutputFormat::Json => json::print_grouped_json(
            groups,
            original,
            ctx.root,
            ctx.elapsed,
            ctx.explain,
            resolver,
        ),
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
        OutputFormat::PrCommentGithub => {
            let value = codeclimate::build_codeclimate(original, ctx.root, ctx.rules);
            ci::pr_comment::print_pr_comment("dead-code", ci::pr_comment::Provider::Github, &value)
        }
        OutputFormat::PrCommentGitlab => {
            let value = codeclimate::build_codeclimate(original, ctx.root, ctx.rules);
            ci::pr_comment::print_pr_comment("dead-code", ci::pr_comment::Provider::Gitlab, &value)
        }
        OutputFormat::ReviewGithub => {
            let value = codeclimate::build_codeclimate(original, ctx.root, ctx.rules);
            ci::review::print_review_envelope("dead-code", ci::pr_comment::Provider::Github, &value)
        }
        OutputFormat::ReviewGitlab => {
            let value = codeclimate::build_codeclimate(original, ctx.root, ctx.rules);
            ci::review::print_review_envelope("dead-code", ci::pr_comment::Provider::Gitlab, &value)
        }
        OutputFormat::Badge => {
            eprintln!("Error: badge format is only supported for the health command");
            ExitCode::from(2)
        }
    }
}

// ── Duplication report ────────────────────────────────────────────

/// Print duplication analysis results in the configured format.
#[must_use]
pub fn print_duplication_report(
    report: &DuplicationReport,
    ctx: &ReportContext<'_>,
    output: OutputFormat,
) -> ExitCode {
    // Grouped output: build the grouping payload once and dispatch
    // per-format. Compact, markdown, and badge fall back to ungrouped output
    // with a stderr note (parity with the health grouped fallback).
    if let Some(ref resolver) = ctx.group_by {
        let grouping = dupes_grouping::build_duplication_grouping(report, ctx.root, resolver);
        return print_grouped_duplication_report(report, &grouping, ctx, output, resolver);
    }

    match output {
        OutputFormat::Human => {
            if ctx.summary {
                human::dupes::print_duplication_summary(report, ctx.elapsed, ctx.quiet);
            } else {
                human::print_duplication_human(
                    report,
                    ctx.root,
                    ctx.elapsed,
                    ctx.quiet,
                    ctx.show_explain_tip,
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
        OutputFormat::PrCommentGithub => {
            let value = codeclimate::build_duplication_codeclimate(report, ctx.root);
            ci::pr_comment::print_pr_comment("dupes", ci::pr_comment::Provider::Github, &value)
        }
        OutputFormat::PrCommentGitlab => {
            let value = codeclimate::build_duplication_codeclimate(report, ctx.root);
            ci::pr_comment::print_pr_comment("dupes", ci::pr_comment::Provider::Gitlab, &value)
        }
        OutputFormat::ReviewGithub => {
            let value = codeclimate::build_duplication_codeclimate(report, ctx.root);
            ci::review::print_review_envelope("dupes", ci::pr_comment::Provider::Github, &value)
        }
        OutputFormat::ReviewGitlab => {
            let value = codeclimate::build_duplication_codeclimate(report, ctx.root);
            ci::review::print_review_envelope("dupes", ci::pr_comment::Provider::Gitlab, &value)
        }
        OutputFormat::Badge => {
            eprintln!("Error: badge format is only supported for the health command");
            ExitCode::from(2)
        }
    }
}

/// Render grouped duplication results across all output formats.
#[must_use]
fn print_grouped_duplication_report(
    report: &DuplicationReport,
    grouping: &dupes_grouping::DuplicationGrouping,
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
        OutputFormat::PrCommentGithub => {
            let value = codeclimate::build_duplication_codeclimate(report, ctx.root);
            ci::pr_comment::print_pr_comment("dupes", ci::pr_comment::Provider::Github, &value)
        }
        OutputFormat::PrCommentGitlab => {
            let value = codeclimate::build_duplication_codeclimate(report, ctx.root);
            ci::pr_comment::print_pr_comment("dupes", ci::pr_comment::Provider::Gitlab, &value)
        }
        OutputFormat::ReviewGithub => {
            let value = codeclimate::build_duplication_codeclimate(report, ctx.root);
            ci::review::print_review_envelope("dupes", ci::pr_comment::Provider::Github, &value)
        }
        OutputFormat::ReviewGitlab => {
            let value = codeclimate::build_duplication_codeclimate(report, ctx.root);
            ci::review::print_review_envelope("dupes", ci::pr_comment::Provider::Gitlab, &value)
        }
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

fn warn_dupes_grouping_unsupported(grouping: &dupes_grouping::DuplicationGrouping, format: &str) {
    eprintln!(
        "note: --group-by {} is not supported for {format} duplication output, falling back to \
         ungrouped output (use --format json for the full grouped envelope)",
        grouping.mode
    );
}

// ── Health / complexity report ─────────────────────────────────────

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
    report: &crate::health_types::HealthReport,
    grouping: Option<&crate::health_types::HealthGrouping>,
    group_resolver: Option<&grouping::OwnershipResolver>,
    ctx: &ReportContext<'_>,
    output: OutputFormat,
) -> ExitCode {
    match output {
        OutputFormat::Human => {
            if ctx.summary {
                human::health::print_health_summary(report, ctx.elapsed, ctx.quiet);
            } else {
                human::print_health_human(
                    report,
                    ctx.root,
                    ctx.elapsed,
                    ctx.quiet,
                    ctx.show_explain_tip,
                );
                if let Some(grouping) = grouping {
                    human::print_health_grouping(grouping, ctx.root, ctx.quiet);
                }
            }
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
                ctx.health_action_opts,
            ),
            None => json::print_health_json(
                report,
                ctx.root,
                ctx.elapsed,
                ctx.explain,
                ctx.health_action_opts,
            ),
        },
        OutputFormat::CodeClimate => match group_resolver {
            Some(resolver) => {
                codeclimate::print_grouped_health_codeclimate(report, ctx.root, resolver)
            }
            None => codeclimate::print_health_codeclimate(report, ctx.root),
        },
        OutputFormat::PrCommentGithub => {
            let value = codeclimate::build_health_codeclimate(report, ctx.root);
            ci::pr_comment::print_pr_comment("health", ci::pr_comment::Provider::Github, &value)
        }
        OutputFormat::PrCommentGitlab => {
            let value = codeclimate::build_health_codeclimate(report, ctx.root);
            ci::pr_comment::print_pr_comment("health", ci::pr_comment::Provider::Gitlab, &value)
        }
        OutputFormat::ReviewGithub => {
            let value = codeclimate::build_health_codeclimate(report, ctx.root);
            ci::review::print_review_envelope("health", ci::pr_comment::Provider::Github, &value)
        }
        OutputFormat::ReviewGitlab => {
            let value = codeclimate::build_health_codeclimate(report, ctx.root);
            ci::review::print_review_envelope("health", ci::pr_comment::Provider::Gitlab, &value)
        }
        OutputFormat::Badge => {
            warn_grouping_unsupported(grouping, "badge");
            badge::print_health_badge(report)
        }
    }
}

fn warn_grouping_unsupported(grouping: Option<&crate::health_types::HealthGrouping>, format: &str) {
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
    cross_ref: &fallow_core::cross_reference::CrossReferenceResult,
    root: &Path,
    quiet: bool,
    output: OutputFormat,
) {
    human::print_cross_reference_findings(cross_ref, root, quiet, output);
}

// ── Trace output ──────────────────────────────────────────────────

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
pub fn print_health_performance(
    timings: &crate::health_types::HealthTimings,
    format: OutputFormat,
) {
    match format {
        OutputFormat::Json => match serde_json::to_string_pretty(timings) {
            Ok(json) => eprintln!("{json}"),
            Err(e) => eprintln!("Error: failed to serialize timings: {e}"),
        },
        _ => human::print_health_performance_human(timings),
    }
}

// Re-exported for snapshot testing via the lib target.
// Uses #[allow] because unused_imports is target-dependent (used in lib, unused in bin).
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use codeclimate::build_codeclimate;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use codeclimate::build_duplication_codeclimate;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use codeclimate::build_health_codeclimate;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use compact::build_compact_lines;
pub use json::HealthActionOptions;
pub use json::build_baseline_deltas_json;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use json::build_duplication_json;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use json::build_grouped_duplication_json;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use json::build_health_json;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use json::build_json;
#[allow(
    unused_imports,
    reason = "target-dependent: used in bin audit.rs, unused in lib"
)]
#[allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) deliberately limits visibility — report is pub but these are internal"
)]
pub(crate) use json::inject_dupes_actions;
#[allow(
    unused_imports,
    reason = "target-dependent: used in bin audit.rs, unused in lib"
)]
#[allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) deliberately limits visibility, report is pub but these are internal"
)]
pub(crate) use json::inject_health_actions;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use markdown::build_duplication_markdown;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use markdown::build_health_markdown;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use markdown::build_markdown;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use sarif::build_health_sarif;
#[allow(
    unused_imports,
    reason = "target-dependent: used in lib, unused in bin"
)]
pub use sarif::build_sarif;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ── normalize_uri ────────────────────────────────────────────────

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

    // ── relative_path ────────────────────────────────────────────────

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

    // ── relative_uri ─────────────────────────────────────────────────

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

    // ── severity_to_level ────────────────────────────────────────────

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

    // ── normalize_uri bracket encoding ──────────────────────────────

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

    // ── relative_path edge cases ────────────────────────────────────

    #[test]
    fn relative_path_identical_paths_returns_empty() {
        let root = Path::new("/project");
        assert_eq!(relative_path(root, root), Path::new(""));
    }

    #[test]
    fn relative_path_partial_name_match_not_stripped() {
        // "/project-two/src/a.ts" should NOT strip "/project" because
        // "/project" is not a proper prefix of "/project-two".
        let root = Path::new("/project");
        let path = Path::new("/project-two/src/a.ts");
        assert_eq!(relative_path(path, root), path);
    }

    // ── relative_uri edge cases ─────────────────────────────────────

    #[test]
    fn relative_uri_combines_stripping_and_encoding() {
        let root = PathBuf::from("/project");
        let path = root.join("src/app/[slug]/page.tsx");
        let uri = relative_uri(&path, &root);
        // Should both strip the prefix AND encode brackets.
        assert_eq!(uri, "src/app/%5Bslug%5D/page.tsx");
        assert!(!uri.starts_with('/'));
    }

    #[test]
    fn relative_uri_at_root_file() {
        let root = PathBuf::from("/project");
        let path = root.join("index.ts");
        assert_eq!(relative_uri(&path, &root), "index.ts");
    }

    // ── severity_to_level exhaustiveness ────────────────────────────

    #[test]
    fn severity_to_level_is_const_evaluable() {
        // Verify the function can be used in const context.
        const LEVEL_FROM_ERROR: Level = severity_to_level(Severity::Error);
        const LEVEL_FROM_WARN: Level = severity_to_level(Severity::Warn);
        const LEVEL_FROM_OFF: Level = severity_to_level(Severity::Off);
        assert!(matches!(LEVEL_FROM_ERROR, Level::Error));
        assert!(matches!(LEVEL_FROM_WARN, Level::Warn));
        assert!(matches!(LEVEL_FROM_OFF, Level::Info));
    }

    // ── Level is Copy ───────────────────────────────────────────────

    #[test]
    fn level_is_copy() {
        let level = severity_to_level(Severity::Error);
        let copy = level;
        // Both should still be usable (Copy semantics).
        assert!(matches!(level, Level::Error));
        assert!(matches!(copy, Level::Error));
    }

    // ── elide_common_prefix ─────────────────────────────────────────

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
        // Same dir, different file
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

    // ── split_dir_filename ───────────────────────────────────────

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

    // ── plural ──────────────────────────────────────────────────

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

    // ── elide_common_prefix edge cases ──────────────────────────

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
        // "src/utils.ts" vs "src/utils.js" — common prefix is "src/"
        assert_eq!(
            elide_common_prefix("src/utils.ts", "src/utils.js"),
            "utils.js"
        );
    }

    #[test]
    fn elide_common_prefix_partial_filename_match_not_stripped() {
        // "src/App.tsx" vs "src/AppUtils.tsx" — both in src/, but file names differ
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
