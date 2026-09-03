//! Read-only project readiness command.

use std::path::Path;
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_output::{DoctorCheckStatus, DoctorOutput, DoctorStatus};

use crate::json_style::JsonStyle;
use crate::report::sink::outln;

pub fn run_doctor(
    root: &Path,
    config_path: Option<&Path>,
    output: OutputFormat,
    json_style: JsonStyle,
) -> ExitCode {
    if !matches!(output, OutputFormat::Human | OutputFormat::Json) {
        return crate::error::emit_error_with_style(
            "doctor supports human and json output",
            2,
            output,
            json_style,
        );
    }

    let report = fallow_api::run_doctor(&fallow_api::DoctorOptions { root, config_path });
    match output {
        OutputFormat::Json => render_json(&report, json_style),
        OutputFormat::Human => render_human(&report),
        _ => unreachable!("unsupported doctor output rejected above"),
    }
}

fn render_json(report: &DoctorOutput, json_style: JsonStyle) -> ExitCode {
    let value = match fallow_output::serialize_doctor_json_output(
        report.clone(),
        crate::output_runtime::current_root_envelope_mode(),
    ) {
        Ok(value) => value,
        Err(error) => {
            return crate::error::emit_error_with_style(
                &format!("failed to serialize doctor report: {error}"),
                2,
                OutputFormat::Json,
                json_style,
            );
        }
    };
    match json_style.serialize(&value) {
        Ok(json) => outln!("{json}"),
        Err(error) => {
            return crate::error::emit_error_with_style(
                &format!("failed to serialize doctor report: {error}"),
                2,
                OutputFormat::Json,
                json_style,
            );
        }
    }
    doctor_exit(report)
}

fn render_human(report: &DoctorOutput) -> ExitCode {
    outln!("Fallow doctor ({})", report.root);
    for check in &report.checks {
        let prefix = match check.status {
            DoctorCheckStatus::Pass => "[OK]",
            DoctorCheckStatus::Warn => "[W]",
            DoctorCheckStatus::Fail => "[X]",
            DoctorCheckStatus::Skipped => "[-]",
        };
        outln!("{prefix} {}: {}", check_id(check.id), check.message);
        if let Some(remediation) = &check.remediation {
            outln!("    Action (from project root): {}", remediation.command);
        }
    }
    outln!(
        "Status: {}",
        match report.status {
            DoctorStatus::Pass => "ready",
            DoctorStatus::Warn => "ready with warnings",
            DoctorStatus::Fail => "not ready",
        }
    );
    doctor_exit(report)
}

const fn check_id(id: fallow_output::DoctorCheckId) -> &'static str {
    match id {
        fallow_output::DoctorCheckId::Root => "root",
        fallow_output::DoctorCheckId::Config => "config",
        fallow_output::DoctorCheckId::Workspaces => "workspaces",
        fallow_output::DoctorCheckId::Plugins => "plugins",
        fallow_output::DoctorCheckId::TypeAware => "type-aware",
    }
}

fn doctor_exit(report: &DoctorOutput) -> ExitCode {
    if report.status == DoctorStatus::Fail {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_failure_uses_execution_error_exit() {
        let report = fallow_api::run_doctor(&fallow_api::DoctorOptions {
            root: Path::new("/definitely/missing/fallow-doctor-root"),
            config_path: None,
        });
        assert_eq!(doctor_exit(&report), ExitCode::from(2));
        assert_eq!(report.checks.len(), 5);
    }
}
