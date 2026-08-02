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
///
/// `change_scoped` marks runs whose findings cover only part of the project
/// (changed-file, diff, or workspace scoping). Staleness counts are still
/// reported for such runs, but `stale` stays false and no re-save advice is
/// printed: a baseline re-saved from a scoped run would carry only the scoped
/// findings and silently gut the gate.
pub(super) fn load_health_baseline(
    baseline_path: &std::path::Path,
    findings: &mut Vec<fallow_output::ComplexityViolation>,
    root: &std::path::Path,
    quiet: bool,
    mode: HealthBaselineMode,
    change_scoped: bool,
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
    let overlap = baseline.overlap_entries(findings, root, mode);
    *findings = filter_new_health_findings(std::mem::take(findings), &baseline, root, mode);
    if !quiet {
        eprintln!(
            "Comparing against health baseline: {}",
            baseline_path.display()
        );
    }
    let staleness = staleness_from_counts(&StalenessCounts {
        baseline_entries,
        matched_entries: overlap.matched_entries,
        moved_entries: overlap.moved_entries,
        current_findings: before,
        change_scoped,
    });
    if !quiet {
        warn_on_staleness(&staleness, baseline_path);
        if staleness.moved_entries > 0 {
            eprintln!(
                "Note: {} baseline entr{} matched through a followed file move.",
                staleness.moved_entries,
                if staleness.moved_entries == 1 {
                    "y"
                } else {
                    "ies"
                },
            );
        }
    }
    Ok(LoadedHealthBaseline {
        data: baseline,
        staleness,
    })
}

/// Warn when the loaded baseline went stale, mirroring the `stale` bool
/// exactly: a warning prints if and only if `stale` is true.
fn warn_on_staleness(
    staleness: &fallow_output::HealthBaselineStaleness,
    baseline_path: &std::path::Path,
) {
    if !staleness.stale {
        return;
    }
    let baseline_entries = staleness.baseline_entries;
    let stale_entries = staleness.stale_entries;
    if staleness.matched_entries == 0 {
        eprintln!(
            "Warning: health baseline has {baseline_entries} entries but matched \
             0 current findings. Your paths may have changed, or the baseline \
             was saved on a different machine. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        );
    } else {
        eprintln!(
            "Warning: health baseline is partially stale: {stale_entries} of \
             {baseline_entries} entries matched no current finding, so the \
             gate protects less than what was saved. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        );
    }
}

struct StalenessCounts {
    baseline_entries: usize,
    matched_entries: usize,
    moved_entries: usize,
    /// Current findings present before baseline filtering. Zero means the run
    /// found nothing to compare, either because the project is clean or the
    /// scope was empty, so staleness cannot be judged and `stale` stays false.
    current_findings: usize,
    change_scoped: bool,
}

/// Staleness data for a loaded baseline that matched `matched_entries` of its
/// `baseline_entries` saved entries on this run.
fn staleness_from_counts(counts: &StalenessCounts) -> fallow_output::HealthBaselineStaleness {
    let stale_entries = counts
        .baseline_entries
        .saturating_sub(counts.matched_entries);
    fallow_output::HealthBaselineStaleness {
        baseline_entries: counts.baseline_entries,
        matched_entries: counts.matched_entries,
        stale_entries,
        moved_entries: counts.moved_entries,
        change_scoped: counts.change_scoped,
        stale: !counts.change_scoped
            && counts.current_findings > 0
            && stale_entries > 0
            && stale_entries * 100 >= counts.baseline_entries * STALE_WARN_PERCENT,
    }
}

#[cfg(test)]
mod tests {
    use super::{StalenessCounts, staleness_from_counts};

    fn counts(baseline_entries: usize, matched_entries: usize) -> StalenessCounts {
        StalenessCounts {
            baseline_entries,
            matched_entries,
            moved_entries: 0,
            current_findings: baseline_entries.max(1),
            change_scoped: false,
        }
    }

    #[test]
    fn staleness_below_threshold_is_not_flagged() {
        let staleness = staleness_from_counts(&counts(100, 76));
        assert_eq!(staleness.stale_entries, 24);
        assert!(!staleness.stale);
    }

    #[test]
    fn staleness_at_threshold_is_flagged() {
        let staleness = staleness_from_counts(&counts(100, 75));
        assert_eq!(staleness.stale_entries, 25);
        assert!(staleness.stale);
    }

    #[test]
    fn zero_overlap_is_flagged_as_fully_stale() {
        let staleness = staleness_from_counts(&counts(8, 0));
        assert_eq!(staleness.stale_entries, 8);
        assert!(staleness.stale);
    }

    #[test]
    fn empty_baseline_is_never_stale() {
        let staleness = staleness_from_counts(&counts(0, 0));
        assert_eq!(staleness.stale_entries, 0);
        assert!(!staleness.stale);
    }

    #[test]
    fn small_baselines_flag_meaningful_drift() {
        assert!(staleness_from_counts(&counts(4, 3)).stale);
        assert!(!staleness_from_counts(&counts(5, 4)).stale);
    }

    #[test]
    fn change_scoped_run_is_never_stale() {
        let staleness = staleness_from_counts(&StalenessCounts {
            change_scoped: true,
            ..counts(8, 2)
        });
        assert_eq!(staleness.stale_entries, 6);
        assert!(staleness.change_scoped);
        assert!(!staleness.stale);
    }

    #[test]
    fn run_without_current_findings_is_never_stale() {
        let staleness = staleness_from_counts(&StalenessCounts {
            current_findings: 0,
            ..counts(8, 0)
        });
        assert_eq!(staleness.stale_entries, 8);
        assert!(!staleness.stale);
    }

    #[test]
    fn moved_entries_are_carried_through() {
        let staleness = staleness_from_counts(&StalenessCounts {
            moved_entries: 2,
            ..counts(10, 9)
        });
        assert_eq!(staleness.moved_entries, 2);
        assert!(!staleness.stale);
    }
}
