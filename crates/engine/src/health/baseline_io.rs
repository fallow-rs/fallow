//! Health baseline filesystem IO.

#![allow(
    clippy::print_stderr,
    reason = "health baseline save/load preserves existing human stderr notes"
)]

use crate::baseline::{
    BaselineStalenessWarning, HealthBaselineData, HealthBaselineMode, filter_new_health_findings,
};

use super::HealthError;

pub(super) struct HealthBaselineSaveInput<'a> {
    pub(super) save_path: &'a std::path::Path,
    pub(super) findings: &'a [fallow_output::ComplexityViolation],
    pub(super) runtime_coverage_findings: &'a [fallow_output::RuntimeCoverageFinding],
    pub(super) targets: &'a [fallow_output::RefactoringTarget],
    pub(super) config_root: &'a std::path::Path,
    pub(super) quiet: bool,
    pub(super) mode: HealthBaselineMode,
    pub(super) mode_explicit: bool,
}

/// Refuse a save aimed at a baseline another command wrote, which this save
/// would overwrite with a health baseline and destroy.
///
/// Checked before [`check_identity_overwrite`], which reads the same file
/// through `HealthBaselineData` and swallows a parse failure: a foreign file
/// that happens to parse carries no identity buckets, so that guard would let
/// this save through.
fn check_kind_overwrite(save_path: &std::path::Path) -> Result<(), HealthError> {
    match crate::baseline::refuse_baseline_kind_overwrite(
        save_path,
        crate::baseline::BaselineKind::Health,
    ) {
        Some(refusal) => Err(HealthError::message(refusal, 2)),
        None => Ok(()),
    }
}

/// Refuse a defaulted count save over a baseline that carries identity
/// buckets: the count save would silently drop them, and the loss only
/// surfaces later, when an identity-mode comparison on another machine
/// rejects the file. An explicit `--baseline-mode count` expresses intent
/// to downgrade and is honored. An unreadable or unparsable existing file
/// is not a guard condition; the save proceeds and overwrites it.
fn check_identity_overwrite(
    save_path: &std::path::Path,
    mode: HealthBaselineMode,
    mode_explicit: bool,
) -> Result<(), HealthError> {
    if mode != HealthBaselineMode::Count || mode_explicit {
        return Ok(());
    }
    let Ok(existing_json) = std::fs::read_to_string(save_path) else {
        return Ok(());
    };
    let Ok(existing) = serde_json::from_str::<HealthBaselineData>(&existing_json) else {
        return Ok(());
    };
    if existing.lacks_identity_data() {
        return Ok(());
    }
    Err(HealthError::message(
        format!(
            "refusing to overwrite health baseline {}: it carries per-function \
             identities (saved with --baseline-mode identity), and this count-mode \
             save would drop them, breaking later --baseline-mode identity runs. \
             Re-save with --baseline-mode identity to keep them, or pass \
             --baseline-mode count explicitly to downgrade the baseline",
            save_path.display()
        ),
        2,
    ))
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
        mode_explicit,
    } = *input;
    check_kind_overwrite(save_path)?;
    check_identity_overwrite(save_path, mode, mode_explicit)?;
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
            match crate::write_guard::write_file(
                save_path,
                json.as_bytes(),
                crate::write_guard::WriteTarget::Path,
            ) {
                Ok(()) => {}
                Err(e) if e.is_directory() => {
                    return Err(HealthError::message(
                        format!("failed to create health baseline directory: {e}"),
                        2,
                    ));
                }
                Err(e) => {
                    return Err(HealthError::message(
                        format!("failed to save health baseline: {e}"),
                        2,
                    ));
                }
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

pub(super) struct LoadedHealthBaseline {
    pub(super) data: HealthBaselineData,
    pub(super) staleness: fallow_output::BaselineStaleness,
}

/// Load and apply a health baseline, filtering findings to show only new ones.
///
/// `scope_reasons` names the channels that narrowed this run below the whole
/// project (a changed-file set, a diff, or workspace scoping). Staleness counts
/// are still reported for such runs, but `stale` stays false and no re-save
/// advice is printed: a baseline re-saved from a scoped run would carry only
/// the scoped findings and silently gut the gate.
pub(super) fn load_health_baseline(
    baseline_path: &std::path::Path,
    findings: &mut Vec<fallow_output::ComplexityViolation>,
    root: &std::path::Path,
    quiet: bool,
    mode: HealthBaselineMode,
    scope_reasons: fallow_output::BaselineScopeReasons,
) -> Result<LoadedHealthBaseline, HealthError> {
    let json = std::fs::read_to_string(baseline_path)
        .map_err(|e| HealthError::message(format!("failed to read health baseline: {e}"), 2))?;
    let baseline: HealthBaselineData = serde_json::from_str(&json)
        .map_err(|e| HealthError::message(format!("failed to parse health baseline: {e}"), 2))?;
    // A file this command did not write carries no identity buckets either, so
    // without this the identity mode would reject another command's baseline
    // with advice to re-save it in identity mode, instead of saying it is not a
    // health baseline at all.
    let unrecognised_format = !matches!(
        crate::baseline::classify_baseline_file(&json, crate::baseline::BaselineKind::Health),
        crate::baseline::BaselineFileKind::Own
    );
    if !unrecognised_format
        && mode == HealthBaselineMode::Identity
        && baseline.lacks_identity_data()
    {
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
    let counts = StalenessCounts {
        baseline_entries,
        matched_entries: overlap.matched_entries,
        moved_entries: overlap.moved_entries,
        current_findings: before,
        unrecognised_format,
        scope_reasons,
    };
    let staleness = staleness_from_counts(&counts);
    if !quiet {
        warn_on_staleness(&counts, baseline_path);
        if counts.moved_entries > 0 {
            eprintln!(
                "Note: {} baseline entr{} matched through a followed file move.",
                counts.moved_entries,
                if counts.moved_entries == 1 {
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

/// Warn when the loaded baseline went stale, branching on the shared decision
/// in [`crate::baseline::BaselineStaleness`] rather than re-deriving it.
fn warn_on_staleness(counts: &StalenessCounts, baseline_path: &std::path::Path) {
    let baseline_entries = counts.baseline_entries;
    let stale_entries = baseline_entries.saturating_sub(counts.matched_entries);
    match staleness_decision(counts).warning() {
        BaselineStalenessWarning::None => {}
        BaselineStalenessWarning::ZeroOverlap => eprintln!(
            "Warning: health baseline has {baseline_entries} entries but matched \
             0 current findings. Your paths may have changed, or the baseline \
             was saved on a different machine. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        ),
        BaselineStalenessWarning::Partial => eprintln!(
            "Warning: health baseline is partially stale: {stale_entries} of \
             {baseline_entries} entries matched no current finding, so the \
             gate protects less than what was saved. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        ),
    }
}

/// The shared staleness decision for this run's counts.
const fn staleness_decision(counts: &StalenessCounts) -> crate::baseline::BaselineStaleness {
    crate::baseline::BaselineStaleness {
        entries: counts.baseline_entries,
        matched: counts.matched_entries,
        current_findings: counts.current_findings,
        change_scoped: !counts.scope_reasons.is_empty(),
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
    /// True when the file is another command's baseline rather than an empty
    /// health one: it names another command, or it names none and carries no key
    /// the health format writes.
    unrecognised_format: bool,
    /// Which channels narrowed this run. `change_scoped` is derived from it, so
    /// the boolean and the published array cannot disagree.
    scope_reasons: fallow_output::BaselineScopeReasons,
}

/// Staleness data for a loaded baseline that matched `matched_entries` of its
/// `baseline_entries` saved entries on this run.
fn staleness_from_counts(counts: &StalenessCounts) -> fallow_output::BaselineStaleness {
    staleness_decision(counts).to_envelope(
        counts.moved_entries,
        counts.scope_reasons,
        counts.unrecognised_format,
    )
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
            unrecognised_format: false,
            scope_reasons: fallow_output::BaselineScopeReasons::empty(),
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
            scope_reasons: fallow_output::BaselineScopeReasons::empty()
                .with(fallow_output::ScopeReason::ChangedFiles),
            ..counts(8, 2)
        });
        assert_eq!(staleness.stale_entries, 6);
        assert!(staleness.change_scoped);
        assert!(!staleness.stale);
        assert_eq!(
            staleness.scope_reasons,
            fallow_output::BaselineScopeReasons::empty()
                .with(fallow_output::ScopeReason::ChangedFiles)
        );
    }

    #[test]
    fn an_unscoped_run_reports_no_scope_reasons() {
        let staleness = staleness_from_counts(&counts(8, 2));
        assert!(!staleness.change_scoped);
        assert!(staleness.scope_reasons.is_empty());
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
