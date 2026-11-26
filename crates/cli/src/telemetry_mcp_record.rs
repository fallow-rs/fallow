//! The hidden `fallow telemetry record` command.
//!
//! The MCP server runs most analysis tools in its own process through
//! `fallow-api`, so no CLI run and no CLI telemetry event happens for them.
//! After such a call, the server runs this command with the same
//! `FALLOW_INTEGRATION_SURFACE=mcp` and `FALLOW_MCP_TOOL=<tool>` tags that it
//! gives a CLI fallback. This command builds the one event for that call.
//! The consent rules, the event schema and the spool stay in this crate, so
//! the server never decides on its own whether to send.

use std::process::ExitCode;
use std::time::Duration;

use fallow_config::OutputFormat;

use super::{
    AnalysisMode, ConfigShape, EffectiveMode, FailureReason, IntegrationSurface, OutputDestination,
    RunAccumulatorSnapshot, RunScope, SPOOL_FILE_NAME, Workflow, WorkflowContext, WorkflowRecord,
    build_workflow_event_with, drain_spool_file, effective_config, exit_codes, integration_surface,
    mcp_tool, parent_run_context, post_telemetry_payload, print_event_to_stderr,
    resolve_install_id_for_send, spool_event, spool_file,
};

/// The arguments of `fallow telemetry record`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::Args)]
pub struct McpCall {
    /// How long the tool call took.
    #[arg(long)]
    pub duration_ms: u64,
    /// The tool call returned an error.
    #[arg(long)]
    pub failed: bool,
}

/// Record one in-process MCP tool call. Always exits 0: telemetry never
/// changes the result of the caller.
pub(super) fn record_mcp_call(call: McpCall) -> ExitCode {
    let McpCall {
        duration_ms,
        failed,
    } = call;
    let mode = effective_config().mode;
    if matches!(mode, EffectiveMode::Off | EffectiveMode::DisabledByAdmin) {
        return ExitCode::SUCCESS;
    }
    // Only the MCP server tags a run this way. Without both tags the call is
    // not an MCP tool call, and an untagged event would count as a CLI run.
    if integration_surface(OutputFormat::Json) != IntegrationSurface::Mcp {
        return ExitCode::SUCCESS;
    }
    let Some(tool) = mcp_tool() else {
        return ExitCode::SUCCESS;
    };
    let (workflow, analysis_mode) = workflow_for_mcp_tool(tool);
    let record = WorkflowRecord {
        workflow,
        output: OutputFormat::Json,
        quiet: true,
        elapsed: Duration::from_millis(duration_ms),
        exit_code: if failed {
            exit_codes::exit_invalid_input()
        } else {
            ExitCode::SUCCESS
        },
        failure_reason: failed.then_some(FailureReason::Unknown),
        parent_run: None,
        context: WorkflowContext {
            run_scope: RunScope::Unknown,
            config_shape: ConfigShape::Unknown,
            output_destination: OutputDestination::Unknown,
            analysis_mode,
        },
    };
    let parent_run = parent_run_context(None, workflow);
    let event = build_workflow_event_with(&record, &parent_run, &RunAccumulatorSnapshot::UNSET);
    match mode {
        EffectiveMode::Inspect => print_event_to_stderr(&event),
        EffectiveMode::On => {
            spool_event(&event, None);
            // The server does not wait for this process, so the upload can
            // run in the foreground. A server that only makes in-process
            // calls starts no other fallow run that would drain the spool.
            if let Some(spool) = spool_file(SPOOL_FILE_NAME) {
                let install_id = resolve_install_id_for_send();
                drain_spool_file(&spool, install_id.as_deref(), post_telemetry_payload);
            }
        }
        EffectiveMode::Off | EffectiveMode::DisabledByAdmin => {}
    }
    ExitCode::SUCCESS
}

/// The workflow and the analysis mode of the CLI command that does the same
/// work as the tool, so the in-process route and the CLI route of one tool
/// give the same values.
fn workflow_for_mcp_tool(tool: &str) -> (Workflow, AnalysisMode) {
    let workflow = match tool {
        "analyze" | "check_changed" | "trace_export" | "trace_file" | "trace_dependency" => {
            Workflow::DeadCode
        }
        "check_architecture" => Workflow::Architecture,
        "find_dupes" | "trace_clone" => Workflow::Dupes,
        "check_health" => Workflow::Health,
        "audit" => Workflow::Audit,
        "feature_flags" => Workflow::CodeQualityReview,
        "decision_surface" => return (Workflow::Audit, AnalysisMode::Unknown),
        "fallow_explain" => return (Workflow::Explain, AnalysisMode::Unknown),
        "project_info" | "list_boundaries" | "trace_import_path" | "trace_error" => {
            return (Workflow::ProjectInventory, AnalysisMode::Unknown);
        }
        _ => return (Workflow::Unknown, AnalysisMode::Unknown),
    };
    (workflow, AnalysisMode::Static)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_in_process_analysis_tool_has_a_workflow() {
        for tool in [
            "analyze",
            "audit",
            "check_architecture",
            "check_changed",
            "check_health",
            "decision_surface",
            "fallow_explain",
            "feature_flags",
            "find_dupes",
            "list_boundaries",
            "project_info",
            "trace_clone",
            "trace_dependency",
            "trace_error",
            "trace_export",
            "trace_file",
            "trace_import_path",
        ] {
            assert_ne!(
                workflow_for_mcp_tool(tool).0,
                Workflow::Unknown,
                "{tool} needs a workflow"
            );
        }
    }

    #[test]
    fn mutation_keeps_the_unknown_workflow_of_the_cli() {
        assert_eq!(
            workflow_for_mcp_tool("mutation_plan"),
            (Workflow::Unknown, AnalysisMode::Unknown)
        );
    }

    #[test]
    fn inventory_tools_keep_the_unknown_analysis_mode_of_the_cli() {
        assert_eq!(
            workflow_for_mcp_tool("trace_error"),
            (Workflow::ProjectInventory, AnalysisMode::Unknown)
        );
        assert_eq!(
            workflow_for_mcp_tool("check_health"),
            (Workflow::Health, AnalysisMode::Static)
        );
    }
}
