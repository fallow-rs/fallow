//! Health baseline filesystem IO.

#![allow(
    clippy::print_stderr,
    reason = "health baseline save/load preserves existing human stderr notes"
)]

use crate::baseline::{HealthBaselineData, HealthBaselineMode, filter_new_health_findings};

use super::HealthError;

pub(super) struct HealthBaselineSaveInput<'a> {
    pub(super) save_path: &'a std::path::Path,
    pub(super) findings: &'a [fallow_output::ComplexityViolation],
    pub(super) runtime_coverage_findings: &'a [fallow_output::RuntimeCoverageFinding],
    pub(super) targets: &'a [fallow_output::RefactoringTarget],
    pub(super) config_root: &'a std::path::Path,
    pub(super) quiet: bool,
    pub(super) mode: HealthBaselineMode,
}

/// Save health baseline to disk.
pub(super) fn save_health_baseline(input: &HealthBaselineSaveInput<'_>) -> Result<(), HealthError> {
    let HealthBaselineSaveInput {
        save_path,
        findings,
        runtime_coverage_findings,
        targets,
        config_root,
        quiet,
        mode,
    } = *input;
    let baseline = HealthBaselineData::from_findings(
        findings,
        runtime_coverage_findings,
        targets,
        config_root,
    );
    let baseline = match mode {
        HealthBaselineMode::Count => baseline,
        HealthBaselineMode::Identity => baseline.with_identity(findings, config_root),
    };
    match serde_json::to_string_pretty(&baseline) {
        Ok(json) => {
            if let Some(parent) = save_path.parent()
                && !parent.as_os_str().is_empty()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Err(HealthError::message(
                    format!("failed to create health baseline directory: {e}"),
                    2,
                ));
            }
            if let Err(e) = std::fs::write(save_path, json) {
                return Err(HealthError::message(
                    format!("failed to save health baseline: {e}"),
                    2,
                ));
            }
            if !quiet {
                eprintln!("Saved health baseline to {}", save_path.display());
            }
            Ok(())
        }
        Err(e) => Err(HealthError::message(
            format!("failed to serialize health baseline: {e}"),
            2,
        )),
    }
}

/// Load and apply a health baseline, filtering findings to show only new ones.
pub(super) fn load_health_baseline(
    baseline_path: &std::path::Path,
    findings: &mut Vec<fallow_output::ComplexityViolation>,
    root: &std::path::Path,
    quiet: bool,
    mode: HealthBaselineMode,
) -> Result<HealthBaselineData, HealthError> {
    let json = std::fs::read_to_string(baseline_path)
        .map_err(|e| HealthError::message(format!("failed to read health baseline: {e}"), 2))?;
    let baseline: HealthBaselineData = serde_json::from_str(&json)
        .map_err(|e| HealthError::message(format!("failed to parse health baseline: {e}"), 2))?;
    if mode == HealthBaselineMode::Identity && baseline.lacks_identity_data() {
        return Err(HealthError::message(
            format!(
                "health baseline {} carries no finding identities, so --baseline-mode identity \
                 cannot compare against it. Re-save it with: --save-baseline {}",
                baseline_path.display(),
                baseline_path.display()
            ),
            2,
        ));
    }
    let baseline_entries = baseline.finding_entry_count();
    let before = findings.len();
    let overlap_entries = baseline.overlap_entry_count(findings, root, mode);
    *findings = filter_new_health_findings(std::mem::take(findings), &baseline, root, mode);
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
