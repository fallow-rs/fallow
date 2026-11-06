//! Validation of the type-aware options of a dead-code run.

use super::*;

pub(super) fn validate_type_aware_check_options(
    dispatch: &DispatchContext<'_>,
    args: &CheckDispatchArgs,
) -> Option<ExitCode> {
    let output = dispatch.output;
    if !args.type_aware_project.is_empty() && args.type_aware != Some(true) {
        return Some(emit_input_error(
            "--type-aware-project requires --type-aware",
            output,
        ));
    }
    if args.type_aware_require.is_some() && args.type_aware != Some(true) {
        return Some(emit_input_error(
            "--type-aware-require requires --type-aware",
            output,
        ));
    }
    if args.trace_opts.symbol_impact.is_some() && args.type_aware != Some(true) {
        return Some(emit_input_error(
            "--symbol-impact requires --type-aware",
            output,
        ));
    }
    let focused_output = args.trace_opts.trace_export.is_some()
        || args.trace_opts.trace_file.is_some()
        || args.trace_opts.trace_dependency.is_some()
        || args.trace_opts.impact_closure.is_some()
        || args.trace_opts.symbol_impact.is_some();
    if focused_output
        && !matches!(
            output,
            fallow_config::OutputFormat::Human | fallow_config::OutputFormat::Json
        )
    {
        return Some(emit_input_error(
            "focused trace and impact queries support human and JSON output",
            output,
        ));
    }
    if args.type_aware == Some(true)
        && !matches!(
            output,
            fallow_config::OutputFormat::Human
                | fallow_config::OutputFormat::Json
                | fallow_config::OutputFormat::Sarif
                | fallow_config::OutputFormat::Compact
                | fallow_config::OutputFormat::Markdown
                | fallow_config::OutputFormat::CodeClimate
                | fallow_config::OutputFormat::PrCommentGithub
                | fallow_config::OutputFormat::PrCommentGitlab
                | fallow_config::OutputFormat::ReviewGithub
                | fallow_config::OutputFormat::ReviewGitlab
        )
    {
        return Some(emit_input_error(
            "--type-aware supports human, JSON, SARIF, compact, markdown, CodeClimate, PR-comment, and review output; pair presentation formats with the JSON artifact to preserve semantic provenance",
            output,
        ));
    }
    None
}
