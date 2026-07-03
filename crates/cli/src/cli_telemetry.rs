use std::process::ExitCode;

use fallow_config::OutputFormat;

use super::{Cli, Command, setup_tracing};
use crate::cli_format::FormatConfig;
use crate::{api, cache_notice, output_runtime, telemetry, update_check};

#[derive(Clone, Copy)]
pub struct TelemetryRun {
    pub workflow: telemetry::Workflow,
    pub output: OutputFormat,
    pub quiet: bool,
    pub start: std::time::Instant,
    pub context: telemetry::WorkflowContext,
}

pub fn record_run_epilogue(
    run: TelemetryRun,
    exit_code: ExitCode,
    failure_reason: Option<telemetry::FailureReason>,
    parent_run: Option<&str>,
) -> ExitCode {
    let cache_notice_printed = cache_notice::maybe_print_created_notice();
    let effective_failure_reason = failure_reason
        .or_else(telemetry::noted_failure_reason)
        .or_else(|| fallback_failure_reason_for(&run, exit_code));
    telemetry::record_workflow(&telemetry::WorkflowRecord {
        workflow: run.workflow,
        output: run.output,
        quiet: run.quiet,
        elapsed: run.start.elapsed(),
        exit_code,
        failure_reason: effective_failure_reason,
        parent_run,
        context: run.context,
    });
    if exit_code == ExitCode::SUCCESS {
        let note_printed = telemetry::maybe_print_opt_in_note(run.output, run.quiet);
        update_check::maybe_nudge(run.output, run.quiet, note_printed || cache_notice_printed);
    }
    exit_code
}

pub fn fallback_failure_reason_for(
    run: &TelemetryRun,
    exit_code: ExitCode,
) -> Option<telemetry::FailureReason> {
    if matches!(exit_code, ExitCode::SUCCESS) || exit_code == ExitCode::from(1) {
        return None;
    }
    if exit_code == ExitCode::from(api::NETWORK_EXIT_CODE) {
        return Some(telemetry::FailureReason::Network);
    }
    if exit_code == ExitCode::from(12) {
        return Some(telemetry::FailureReason::Auth);
    }
    if matches!(
        run.context.analysis_mode,
        telemetry::AnalysisMode::ProductionCoverage
    ) && exit_code == ExitCode::from(3)
    {
        return Some(telemetry::FailureReason::Auth);
    }
    if matches!(
        run.context.analysis_mode,
        telemetry::AnalysisMode::Static
            | telemetry::AnalysisMode::Security
            | telemetry::AnalysisMode::Fix
            | telemetry::AnalysisMode::RuntimeCoverage
            | telemetry::AnalysisMode::ProductionCoverage
    ) {
        return Some(telemetry::FailureReason::Analysis);
    }
    Some(telemetry::FailureReason::Unknown)
}

pub fn start_telemetry_run(cli: &Cli, fmt: &FormatConfig) -> TelemetryRun {
    setup_tracing();
    let run = TelemetryRun {
        workflow: telemetry_workflow_for_command(cli.command.as_ref(), fmt.output),
        output: fmt.output,
        quiet: fmt.quiet,
        start: std::time::Instant::now(),
        context: telemetry_context_for_command(cli, cli.command.as_ref(), fmt.output),
    };
    output_runtime::set_telemetry_analysis_run_id(
        matches!(fmt.output, OutputFormat::Json).then(telemetry::new_analysis_run_id),
    );
    telemetry::flush_spool_in_background();
    run
}

fn telemetry_context_for_command(
    cli: &Cli,
    command: Option<&Command>,
    output: OutputFormat,
) -> telemetry::WorkflowContext {
    telemetry::WorkflowContext {
        run_scope: telemetry_run_scope_for_command(cli, command),
        config_shape: telemetry_config_shape_for_cli(cli),
        output_destination: telemetry_output_destination_for_command(cli, command, output),
        analysis_mode: telemetry_analysis_mode_for_command(command),
    }
}

fn telemetry_run_scope_for_command(cli: &Cli, command: Option<&Command>) -> telemetry::RunScope {
    if command_is_file_scoped(command) {
        return telemetry::RunScope::FileScoped;
    }
    if cli
        .workspace
        .as_ref()
        .is_some_and(|workspaces| !workspaces.is_empty())
        || cli.changed_workspaces.is_some()
    {
        return telemetry::RunScope::WorkspaceScoped;
    }
    if cli.changed_since.is_some()
        || cli.diff_file.is_some()
        || cli.diff_stdin
        || matches!(command, Some(Command::Audit { .. }))
    {
        return telemetry::RunScope::ChangedOnly;
    }
    if command_runs_full_project_analysis(command) {
        return telemetry::RunScope::FullProject;
    }
    telemetry::RunScope::Unknown
}

fn command_is_file_scoped(command: Option<&Command>) -> bool {
    matches!(
        command,
        Some(Command::Check { file, .. } | Command::Security { file, .. }) if !file.is_empty()
    )
}

fn command_runs_full_project_analysis(command: Option<&Command>) -> bool {
    matches!(
        command,
        None | Some(
            Command::Check { .. }
                | Command::Dupes { .. }
                | Command::Health { .. }
                | Command::Flags { .. }
                | Command::Security { .. }
                | Command::Fix { .. }
                | Command::Watch { .. },
        )
    )
}

fn telemetry_config_shape_for_cli(cli: &Cli) -> telemetry::ConfigShape {
    if cli.config.is_some() {
        telemetry::ConfigShape::CustomConfig
    } else {
        telemetry::ConfigShape::Unknown
    }
}

fn telemetry_output_destination_for_command(
    cli: &Cli,
    command: Option<&Command>,
    output: OutputFormat,
) -> telemetry::OutputDestination {
    if matches!(command, Some(Command::Ci { .. }))
        || matches!(
            output,
            OutputFormat::PrCommentGithub
                | OutputFormat::PrCommentGitlab
                | OutputFormat::ReviewGithub
                | OutputFormat::CodeClimate
        )
    {
        return telemetry::OutputDestination::CiComment;
    }
    if cli.output_file.is_some() || cli.sarif_file.is_some() {
        return telemetry::OutputDestination::File;
    }
    telemetry::OutputDestination::Stdout
}

fn telemetry_analysis_mode_for_command(command: Option<&Command>) -> telemetry::AnalysisMode {
    match command {
        Some(Command::Security { .. }) => telemetry::AnalysisMode::Security,
        Some(Command::Fix { .. }) => telemetry::AnalysisMode::Fix,
        Some(Command::Health {
            runtime_coverage: Some(_),
            ..
        })
        | Some(Command::Audit {
            runtime_coverage: Some(_),
            ..
        })
        | Some(Command::Coverage { .. }) => telemetry::AnalysisMode::ProductionCoverage,
        Some(Command::Health {
            coverage: Some(_), ..
        })
        | Some(Command::Audit {
            coverage: Some(_), ..
        }) => telemetry::AnalysisMode::RuntimeCoverage,
        None
        | Some(
            Command::Check { .. }
            | Command::Dupes { .. }
            | Command::Health { .. }
            | Command::Audit { .. }
            | Command::Flags { .. }
            | Command::Watch { .. },
        ) => telemetry::AnalysisMode::Static,
        _ => telemetry::AnalysisMode::Unknown,
    }
}

pub fn telemetry_workflow_for_command(
    command: Option<&Command>,
    output: OutputFormat,
) -> telemetry::Workflow {
    match command {
        None | Some(Command::Flags { .. } | Command::Watch { .. }) => {
            telemetry::Workflow::CodeQualityReview
        }
        Some(Command::Check { .. }) => telemetry::Workflow::DeadCode,
        Some(Command::Dupes { .. }) => telemetry::Workflow::Dupes,
        Some(Command::Health { .. }) => telemetry::Workflow::Health,
        Some(Command::Audit { .. } | Command::DecisionSurface { .. }) => telemetry::Workflow::Audit,
        Some(Command::Ci { .. }) => match output {
            OutputFormat::ReviewGitlab
            | OutputFormat::PrCommentGitlab
            | OutputFormat::CodeClimate => telemetry::Workflow::GitlabCi,
            _ => telemetry::Workflow::GithubAction,
        },
        Some(Command::Coverage { .. }) => telemetry::Workflow::RuntimeCoverageSetup,
        Some(Command::Impact { .. }) => telemetry::Workflow::Impact,
        Some(Command::Security { .. }) => telemetry::Workflow::Security,
        Some(Command::Fix { .. }) => telemetry::Workflow::Fix,
        Some(Command::Explain { .. }) => telemetry::Workflow::Explain,
        Some(
            Command::Inspect { .. }
            | Command::Guard { .. }
            | Command::Trace { .. }
            | Command::List { .. }
            | Command::Workspaces
            | Command::Schema,
        ) => telemetry::Workflow::ProjectInventory,
        Some(Command::License { .. }) => telemetry::Workflow::License,
        Some(
            Command::Init { .. }
            | Command::Hooks { .. }
            | Command::ConfigSchema
            | Command::PluginSchema
            | Command::RulePackSchema
            | Command::RulePack { .. }
            | Command::Config { .. }
            | Command::CiTemplate { .. }
            | Command::Migrate { .. }
            | Command::Telemetry { .. }
            | Command::SetupHooks { .. },
        ) => telemetry::Workflow::Setup,
    }
}
