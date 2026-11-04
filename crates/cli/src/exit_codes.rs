//! Process exit codes for public CLI workflows: the non-default codes and the
//! one table that maps a gate verdict to an exit code.
//!
//! Keep command-specific producers and the machine-readable capability
//! manifest on these constants so agents never receive a stale copied ladder.
//!
//! The codes 0, 1 and 2 belong to the shared report contract in
//! `fallow-report-protocol`. The CLI gets the codes 1 and 2 from
//! [`exit_gate_failed`] and [`exit_invalid_input`]. A test below rejects a
//! literal `1` or `2` in `ExitCode::from` or `process::exit`.

use std::process::ExitCode;

use fallow_output::exit_code::{EXIT_GATE_FAILED, EXIT_INVALID_INPUT, EXIT_OK};

/// Exit code 1: an enforced gate failed. The run found problems, the tool did
/// not fail.
#[must_use]
pub fn exit_gate_failed() -> ExitCode {
    ExitCode::from(EXIT_GATE_FAILED)
}

/// Exit code 2: the input was not valid, or the run could not complete.
#[must_use]
pub fn exit_invalid_input() -> ExitCode {
    ExitCode::from(EXIT_INVALID_INPUT)
}

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
            _ => EXIT_GATE_FAILED,
        },
        _ => EXIT_OK,
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
pub fn run_exit_code(codes: impl IntoIterator<Item = u8>) -> ExitCode {
    ExitCode::from(codes.into_iter().max().unwrap_or(EXIT_OK))
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
    fn the_gate_table_uses_the_report_contract_codes() {
        assert_eq!(
            gate_exit_code(GateName::ErrorSeverityFindings, GateStatus::Fail),
            EXIT_GATE_FAILED
        );
        assert_eq!(
            gate_exit_code(GateName::ErrorSeverityFindings, GateStatus::Pass),
            EXIT_OK
        );
        assert_eq!(run_exit_code([]), ExitCode::from(EXIT_OK));
    }

    #[test]
    fn the_run_exits_with_the_highest_failed_code() {
        assert_eq!(run_exit_code([0, 1, 0]), exit_gate_failed());
        assert_eq!(run_exit_code([]), ExitCode::SUCCESS);
        assert_eq!(exit_invalid_input(), ExitCode::from(EXIT_INVALID_INPUT));
    }

    /// The exit codes 1 and 2 have one definition in the report contract. A
    /// literal copy in the CLI can drift from it without a failed build.
    #[test]
    fn cli_reads_contract_exit_codes_from_the_shared_constants() {
        use crate::architecture_boundaries::{
            read_source_without_line_comments, rust_sources_under,
        };
        let needles = [
            concat!("ExitCode::from(", "1)"),
            concat!("ExitCode::from(", "2)"),
            concat!("process::exit(", "1)"),
            concat!("process::exit(", "2)"),
        ];
        // The stub copies the exit codes of the external sidecar, not the
        // report contract.
        let stub_sidecar = "crates/cli/src/bin/stub_sidecar.rs";
        let offenders: Vec<String> = rust_sources_under(["crates/cli/src", "crates/cli/tests"])
            .into_iter()
            .filter(|source_path| source_path != stub_sidecar)
            .filter(|source_path| {
                let source = read_source_without_line_comments(source_path)
                    .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
                needles.iter().any(|needle| source.contains(needle))
            })
            .collect();
        assert!(
            offenders.is_empty(),
            "use exit_gate_failed(), exit_invalid_input() or the contract constants: {offenders:?}"
        );
    }
}
