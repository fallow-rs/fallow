//! Dispatch of `fallow audit`: maps the command flags to the audit inputs.

use super::*;
use crate::cli_args::AuditGateRunArgs;

pub(super) fn dispatch_audit_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Audit {
        production_dead_code,
        production_health,
        production_dupes,
        dead_code_baseline,
        health_baseline,
        dupes_baseline,
        max_crap,
        coverage,
        coverage_root,
        only,
        skip,
        no_css,
        css_deep,
        no_css_deep,
        gate,
        worsened,
        runtime_coverage,
        min_invocations_hot,
        mutation_report,
        gate_run: AuditGateRunArgs {
            gate_marker,
            staged,
        },
        brief,
        max_decisions,
        walkthrough_guide,
        walkthrough_file,
        walkthrough,
        mark_viewed,
        show_cleared,
        show_deprioritized,
        path,
    } = command
    else {
        unreachable!("audit dispatcher only handles audit commands");
    };

    // The walkthrough flags imply the brief path (the guide digest + the
    // graph-snapshot pin are brief-path data).
    let brief = brief || walkthrough_guide || walkthrough || walkthrough_file.is_some();

    let scope = match crate::scope_path::resolve_command_scope(dispatch.root, dispatch.output, path)
    {
        Ok(scope) => scope.map(|resolved| resolved.absolute),
        Err(code) => return code,
    };

    let args = AuditDispatchArgs {
        production_dead_code,
        production_health,
        production_dupes,
        dead_code_baseline,
        health_baseline,
        dupes_baseline,
        max_crap,
        coverage,
        coverage_root,
        only,
        skip,
        no_css,
        css_deep,
        no_css_deep,
        gate,
        worsened,
        runtime_coverage,
        min_invocations_hot,
        mutation_report,
        gate_marker,
        brief,
        max_decisions,
        walkthrough_guide,
        walkthrough_file,
        walkthrough,
        mark_viewed,
        show_cleared,
        show_deprioritized,
        scope,
        staged_host: None,
    };
    if staged {
        return super::audit_staged::dispatch_staged_audit(dispatch, args);
    }
    dispatch_audit(dispatch, &args)
}
