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

/// Stale fraction (in percent) at which the partial-staleness warning fires.
///
/// A little drift is the normal state of a living baseline, so warning on any
/// stale entry would train people to ignore the note. A quarter of the
/// baseline matching nothing means the gate protects meaningfully less than
/// what was saved.
const STALE_WARN_PERCENT: usize = 25;

pub(super) struct LoadedHealthBaseline {
    pub(super) data: HealthBaselineData,
    pub(super) staleness: fallow_output::HealthBaselineStaleness,
}

/// Load and apply a health baseline, filtering findings to show only new ones.
pub(super) fn load_health_baseline(
    baseline_path: &std::path::Path,
    findings: &mut Vec<fallow_output::ComplexityViolation>,
    root: &std::path::Path,
    quiet: bool,
    mode: HealthBaselineMode,
) -> Result<LoadedHealthBaseline, HealthError> {
    let json = std::fs::read_to_string(baseline_path)
        .map_err(|e| HealthError::message(format!("failed to read health baseline: {e}"), 2))?;
    let baseline: HealthBaselineData = serde_json::from_str(&json)
        .map_err(|e| HealthError::message(format!("failed to parse health baseline: {e}"), 2))?;
    if mode == HealthBaselineMode::Identity && baseline.lacks_identity_data() {
        return Err(HealthError::message(
            format!(
                "health baseline {} carries no finding identities, so --baseline-mode identity \
                 cannot compare against it. Re-save it with: --save-baseline {} \
                 --baseline-mode identity",
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
    let staleness = staleness_from_counts(baseline_entries, overlap_entries);
    let stale_entries = staleness.stale_entries;
    if baseline_entries > 0 && before > 0 && overlap_entries == 0 && !quiet {
        eprintln!(
            "Warning: health baseline has {baseline_entries} entries but matched \
             0 current findings. Your paths may have changed, or the baseline \
             was saved on a different machine. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        );
    } else if staleness.stale && overlap_entries > 0 && !quiet {
        eprintln!(
            "Warning: health baseline is partially stale: {stale_entries} of \
             {baseline_entries} entries matched no current finding, so the \
             gate protects less than what was saved. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        );
    }
    Ok(LoadedHealthBaseline {
        data: baseline,
        staleness,
    })
}

/// Staleness data for a loaded baseline that matched `matched_entries` of its
/// `baseline_entries` saved entries on this run.
fn staleness_from_counts(
    baseline_entries: usize,
    matched_entries: usize,
) -> fallow_output::HealthBaselineStaleness {
    let stale_entries = baseline_entries.saturating_sub(matched_entries);
    fallow_output::HealthBaselineStaleness {
        baseline_entries,
        matched_entries,
        stale_entries,
        stale: stale_entries > 0 && stale_entries * 100 >= baseline_entries * STALE_WARN_PERCENT,
    }
}

#[cfg(test)]
mod tests {
    use super::staleness_from_counts;

    #[test]
    fn staleness_below_threshold_is_not_flagged() {
        let staleness = staleness_from_counts(100, 76);
        assert_eq!(staleness.stale_entries, 24);
        assert!(!staleness.stale);
    }

    #[test]
    fn staleness_at_threshold_is_flagged() {
        let staleness = staleness_from_counts(100, 75);
        assert_eq!(staleness.stale_entries, 25);
        assert!(staleness.stale);
    }

    #[test]
    fn zero_overlap_is_flagged_as_fully_stale() {
        let staleness = staleness_from_counts(8, 0);
        assert_eq!(staleness.stale_entries, 8);
        assert!(staleness.stale);
    }

    #[test]
    fn empty_baseline_is_never_stale() {
        let staleness = staleness_from_counts(0, 0);
        assert_eq!(staleness.stale_entries, 0);
        assert!(!staleness.stale);
    }

    #[test]
    fn small_baselines_flag_meaningful_drift() {
        assert!(staleness_from_counts(4, 3).stale);
        assert!(!staleness_from_counts(5, 4).stale);
    }
}
