//! Process exit codes for public CLI workflows: the non-default codes and the
//! one table that maps a gate verdict to an exit code.
//!
//! Keep command-specific producers and the machine-readable capability
//! manifest on these constants so agents never receive a stale copied ladder.

pub const RESOURCE_UNAVAILABLE_EXIT_CODE: u8 = 3;
pub const RUNTIME_COVERAGE_SIDECAR_EXIT_CODE: u8 = 4;
pub const RUNTIME_COVERAGE_INPUT_EXIT_CODE: u8 = 5;
pub const RUNTIME_COVERAGE_INTERNAL_EXIT_CODE: u8 = 6;
pub const NETWORK_EXIT_CODE: u8 = 7;
pub const SECURITY_GATE_EXIT_CODE: u8 = 8;
pub const COVERAGE_UPLOAD_VALIDATION_EXIT_CODE: u8 = 10;
pub const COVERAGE_UPLOAD_PAYLOAD_TOO_LARGE_EXIT_CODE: u8 = 11;
pub const COVERAGE_UPLOAD_AUTH_REJECTED_EXIT_CODE: u8 = 12;
pub const COVERAGE_UPLOAD_SERVER_ERROR_EXIT_CODE: u8 = 13;

/// The exit code of a gate that failed the run.
const GATE_FAILURE_EXIT_CODE: u8 = 1;

/// The exit code that one gate verdict gives the process.
///
/// This is the one verdict-to-exit-code table of the analysis commands. Each
/// command decides its verdicts; this function decides what a verdict means
/// for the exit code. A failed gate exits 1, except `security --gate`, which
/// exits 8. A pass, a warning (the audit `warn` tier) and a gate that stood
/// down exit 0.
#[must_use]
pub const fn gate_exit_code(
    gate: fallow_output::GateName,
    status: fallow_output::GateStatus,
) -> u8 {
    match status {
        fallow_output::GateStatus::Fail => match gate {
            fallow_output::GateName::Security => SECURITY_GATE_EXIT_CODE,
            _ => GATE_FAILURE_EXIT_CODE,
        },
        _ => 0,
    }
}

/// [`gate_exit_code`] for a gate with a pass or fail verdict.
#[must_use]
pub const fn gate_failed_exit_code(gate: fallow_output::GateName, failed: bool) -> u8 {
    gate_exit_code(gate, crate::gates::status_of(failed))
}

/// The process exit code for the verdicts of one run: the highest code of the
/// gates that failed, or 0.
#[must_use]
pub fn run_exit_code(codes: impl IntoIterator<Item = u8>) -> std::process::ExitCode {
    std::process::ExitCode::from(codes.into_iter().max().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{GateName, GateStatus};

    #[test]
    fn a_failed_gate_exits_one_and_the_security_gate_exits_eight() {
        assert_eq!(
            gate_exit_code(GateName::ErrorSeverityFindings, GateStatus::Fail),
            1
        );
        assert_eq!(
            gate_exit_code(GateName::SecurityAdvisory, GateStatus::Fail),
            1
        );
        assert_eq!(
            gate_exit_code(GateName::Security, GateStatus::Fail),
            SECURITY_GATE_EXIT_CODE
        );
    }

    #[test]
    fn a_pass_a_warning_and_a_stand_down_exit_zero() {
        assert_eq!(gate_exit_code(GateName::AuditVerdict, GateStatus::Warn), 0);
        assert_eq!(gate_exit_code(GateName::Security, GateStatus::Pass), 0);
        assert_eq!(
            gate_exit_code(GateName::StaleBaseline, GateStatus::Skipped),
            0
        );
    }

    #[test]
    fn the_run_exits_with_the_highest_failed_code() {
        assert_eq!(run_exit_code([0, 1, 0]), std::process::ExitCode::from(1));
        assert_eq!(run_exit_code([]), std::process::ExitCode::SUCCESS);
    }
}
