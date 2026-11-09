//! Architecture selector composition and inventory format validation.
use super::*;

/// Destructure the `Command::Architecture` arm and forward to `dispatch_check`
/// with the architecture issue types selected. The analysis, the JSON envelope
/// and the finding ids are the ones `dead-code` produces for the same filters.
pub(super) fn dispatch_architecture_command(
    command: Command,
    dispatch: &DispatchContext<'_>,
) -> ExitCode {
    let Command::Architecture {
        shared_state,
        cycles,
        boundaries,
        policy,
        file,
        finding_id,
        path,
    } = command
    else {
        unreachable!("architecture dispatcher only handles architecture commands");
    };

    if shared_state && (!finding_id.is_empty() || dispatch.cli.group_by.is_some()) {
        return emit_input_error(
            "--shared-state does not support --finding-id or --group-by",
            dispatch.output,
        );
    }
    if shared_state
        && !matches!(
            dispatch.output,
            fallow_config::OutputFormat::Human
                | fallow_config::OutputFormat::Json
                | fallow_config::OutputFormat::Markdown
        )
    {
        return emit_input_error(
            "--shared-state supports human, json and markdown output",
            dispatch.output,
        );
    }
    let finding_ids = match fallow_engine::dead_code::FindingIdFilter::parse(&finding_id) {
        Ok(filter) => filter,
        Err(message) => {
            return emit_input_error(&format!("--finding-id: {message}"), dispatch.output);
        }
    };

    let scope = match crate::scope_path::resolve_command_scope(dispatch.root, dispatch.output, path)
    {
        Ok(scope) => scope.map(|resolved| resolved.absolute),
        Err(code) => return code,
    };

    dispatch_check(
        dispatch,
        &CheckDispatchArgs {
            filters: architecture_inventory_filters(cycles, boundaries, policy, shared_state),
            trace_opts: TraceOptions {
                trace_export: None,
                trace_file: None,
                trace_dependency: None,
                impact_closure: None,
                symbol_impact: None,
                performance: dispatch.cli.performance,
            },
            include_dupes: false,
            type_aware: dispatch.cli.type_aware_override(),
            type_aware_project: dispatch.cli.type_aware_project.clone(),
            type_aware_require: dispatch.cli.type_aware_require,
            top: None,
            file,
            scope,
            finding_ids,
            surface: check::CheckSurface::architecture(cycles, boundaries, policy),
        },
    )
}

/// Map the `fallow architecture` filter flags onto the `dead-code` filter flags
/// they stand for. No flag selects every architecture issue type.
#[cfg(test)]
pub fn architecture_issue_filters(cycles: bool, boundaries: bool, policy: bool) -> IssueFilters {
    architecture_inventory_filters(cycles, boundaries, policy, false)
}

fn architecture_inventory_filters(
    cycles: bool,
    boundaries: bool,
    policy: bool,
    shared_state: bool,
) -> IssueFilters {
    let all = !(cycles || boundaries || policy || shared_state);
    let mut filters = IssueFilters {
        shared_state,
        ..IssueFilters::default()
    };
    for (flag, active) in [
        ("--circular-deps", cycles || all),
        ("--re-export-cycles", cycles || all),
        ("--package-cycles", cycles || all),
        ("--boundary-violations", boundaries || all),
        ("--policy-violations", policy || all),
    ] {
        enable_check_filter(&mut filters, flag, active);
    }
    filters
}
