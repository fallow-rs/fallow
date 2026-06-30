//! Health baseline filesystem IO.

#![allow(
    clippy::print_stderr,
    reason = "health baseline save/load preserves existing human stderr notes"
)]

use std::process::ExitCode;

use fallow_types::output_format::OutputFormat;

use crate::baseline::{HealthBaselineData, filter_new_health_findings};
use crate::error::emit_error;

pub(super) struct HealthBaselineSaveInput<'a> {
    pub(super) save_path: &'a std::path::Path,
    pub(super) findings: &'a [fallow_output::ComplexityViolation],
    pub(super) runtime_coverage_findings: &'a [fallow_output::RuntimeCoverageFinding],
    pub(super) targets: &'a [fallow_output::RefactoringTarget],
    pub(super) config_root: &'a std::path::Path,
    pub(super) quiet: bool,
    pub(super) output: OutputFormat,
}

/// Save health baseline to disk.
pub(super) fn save_health_baseline(input: &HealthBaselineSaveInput<'_>) -> Result<(), ExitCode> {
    let HealthBaselineSaveInput {
        save_path,
        findings,
        runtime_coverage_findings,
        targets,
        config_root,
        quiet,
        output,
    } = *input;
    let baseline = HealthBaselineData::from_findings(
        findings,
        runtime_coverage_findings,
        targets,
        config_root,
    );
    match serde_json::to_string_pretty(&baseline) {
        Ok(json) => {
            if let Some(parent) = save_path.parent()
                && !parent.as_os_str().is_empty()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Err(emit_error(
                    &format!("failed to create health baseline directory: {e}"),
                    2,
                    output,
                ));
            }
            if let Err(e) = std::fs::write(save_path, json) {
                return Err(emit_error(
                    &format!("failed to save health baseline: {e}"),
                    2,
                    output,
                ));
            }
            if !quiet {
                eprintln!("Saved health baseline to {}", save_path.display());
            }
            Ok(())
        }
        Err(e) => Err(emit_error(
            &format!("failed to serialize health baseline: {e}"),
            2,
            output,
        )),
    }
}

/// Load and apply a health baseline, filtering findings to show only new ones.
pub(super) fn load_health_baseline(
    baseline_path: &std::path::Path,
    findings: &mut Vec<fallow_output::ComplexityViolation>,
    root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
) -> Result<HealthBaselineData, ExitCode> {
    let json = std::fs::read_to_string(baseline_path)
        .map_err(|e| emit_error(&format!("failed to read health baseline: {e}"), 2, output))?;
    let baseline: HealthBaselineData = serde_json::from_str(&json)
        .map_err(|e| emit_error(&format!("failed to parse health baseline: {e}"), 2, output))?;
    let baseline_entries = baseline.finding_entry_count();
    let before = findings.len();
    let overlap_entries = baseline.overlap_entry_count(findings, root);
    *findings = filter_new_health_findings(std::mem::take(findings), &baseline, root);
    if !quiet {
        eprintln!(
            "Comparing against health baseline: {}",
            baseline_path.display()
        );
    }
    if baseline_entries > 0 && before > 0 && overlap_entries == 0 && !quiet {
        eprintln!(
            "Warning: health baseline has {baseline_entries} entries but matched \
             0 current findings. Your paths may have changed, or the baseline \
             was saved on a different machine. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        );
    }
    Ok(baseline)
}
