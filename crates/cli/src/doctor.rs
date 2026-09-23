//! Read-only project readiness command.

use std::path::Path;
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_output::{DoctorCheckStatus, DoctorOutput, DoctorStatus};

use crate::json_style::JsonStyle;
use crate::report::{HumanStatus, human_status_line, sink::outln};

pub const HELP: &str = "\
Diagnose project readiness without analysis or mutation.

Checks the root, config resolution, workspace discovery, external plugins, the
optional type-aware companion, installed dependencies, and whether the
persisted extraction and module-graph caches would be reused. Uses only local
reads and supports human and JSON output.

Usage: fallow doctor [OPTIONS]

Options:
  -r, --root <ROOT>          Project root directory
  -c, --config <CONFIG>      Path to the Fallow config file
  -f, --format <FORMAT>      Output format: human or json [default: human] [alias: --output]
      --pretty               Indent JSON output
  -q, --quiet                Suppress progress output
  -h, --help                 Print help
";

pub fn collect_report(root: &Path, config_path: Option<&Path>) -> DoctorOutput {
    let cache_dir = crate::runtime_support::resolve_cache_dir_env();
    fallow_api::run_doctor_with_cache_dir(
        &fallow_api::DoctorOptions { root, config_path },
        cache_dir.as_deref(),
    )
}

pub fn render_report(
    report: &DoctorOutput,
    output: OutputFormat,
    json_style: JsonStyle,
) -> ExitCode {
    match output {
        OutputFormat::Json => render_json(report, json_style),
        OutputFormat::Human => render_human(report),
        _ => unreachable!("unsupported doctor output rejected before inspection"),
    }
}

pub fn validate_output(output: OutputFormat, json_style: JsonStyle) -> Result<(), ExitCode> {
    if matches!(output, OutputFormat::Human | OutputFormat::Json) {
        return Ok(());
    }
    Err(crate::error::emit_error_with_style(
        "doctor supports human and json output",
        2,
        output,
        json_style,
    ))
}

fn render_json(report: &DoctorOutput, json_style: JsonStyle) -> ExitCode {
    let value = match fallow_output::serialize_doctor_json_output(report.clone()) {
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
    report_exit(report)
}

fn render_human(report: &DoctorOutput) -> ExitCode {
    outln!("Fallow doctor ({})", report.root);
    for check in &report.checks {
        outln!(
            "{}",
            human_status_line(
                check_human_status(check.status),
                format_args!("{}: {}", check_id(check.id), check.message)
            )
        );
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
    report_exit(report)
}

const fn check_human_status(status: DoctorCheckStatus) -> HumanStatus {
    match status {
        DoctorCheckStatus::Pass => HumanStatus::Ok,
        DoctorCheckStatus::Warn => HumanStatus::Warning,
        DoctorCheckStatus::Fail => HumanStatus::Failure,
        DoctorCheckStatus::Skipped => HumanStatus::Inactive,
    }
}

const fn check_id(id: fallow_output::DoctorCheckId) -> &'static str {
    match id {
        fallow_output::DoctorCheckId::Root => "root",
        fallow_output::DoctorCheckId::Config => "config",
        fallow_output::DoctorCheckId::Workspaces => "workspaces",
        fallow_output::DoctorCheckId::Plugins => "plugins",
        fallow_output::DoctorCheckId::TypeAware => "type-aware",
        fallow_output::DoctorCheckId::Dependencies => "dependencies",
        fallow_output::DoctorCheckId::Cache => "cache",
        fallow_output::DoctorCheckId::GraphCache => "graph-cache",
    }
}

fn report_exit(report: &DoctorOutput) -> ExitCode {
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
        assert_eq!(report_exit(&report), ExitCode::from(2));
        // A required failure still emits the complete envelope, in the stable
        // order the compatibility contract documents.
        assert_eq!(
            report
                .checks
                .iter()
                .map(|c| check_id(c.id))
                .collect::<Vec<_>>(),
            vec![
                "root",
                "config",
                "workspaces",
                "plugins",
                "type-aware",
                "dependencies",
                "cache",
                "graph-cache",
            ]
        );
    }
}
