use fallow_output::{FileHealthScore, HotspotEntry};

use crate::vital_signs;

/// Apply duplication metrics to a vital-signs result.
pub fn apply_duplication_metrics(
    vital_signs: &mut fallow_output::VitalSigns,
    counts: &mut fallow_output::VitalSignsCounts,
    dupes_report: &crate::duplicates::DuplicationReport,
) {
    let pct = dupes_report.stats.duplication_percentage;
    vital_signs.duplication_pct = Some((pct * 10.0).round() / 10.0);
    counts.duplicated_lines = Some(dupes_report.stats.duplicated_lines);
    if let Some(ref mut vc) = vital_signs.counts {
        vc.duplicated_lines = Some(dupes_report.stats.duplicated_lines);
    }
}

/// Subset selector used when scoping `vital_signs`, `health_score`, and
/// `analysis_counts` to a workspace package or a `--group-by` bucket.
///
/// `Full` skips filtering entirely (project-wide). `Paths` matches files whose
/// absolute path is in the given set (exact match), which is what scoped
/// project runs and `--group-by` use to keep every score input on the same
/// filtered file set.
pub enum SubsetFilter<'a> {
    Full,
    Paths(&'a rustc_hash::FxHashSet<std::path::PathBuf>),
}

impl SubsetFilter<'_> {
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    pub fn matches(&self, path: &std::path::Path) -> bool {
        match self {
            Self::Full => true,
            Self::Paths(set) => set.contains(path),
        }
    }
}

/// Build vital signs and counts for the slice of files selected by `subset`.
///
/// When `subset` is anything other than `SubsetFilter::Full`, per-module
/// aggregates (cyclomatic distribution, total LOC, unit profiles) are
/// restricted to modules in the subset, the analysis counts (`dead_files`,
/// `dead_exports`, `unused_deps`, `circular_deps`, `total_exports`) are
/// recomputed from the snapshot for the same subset, and `total_files` should
/// already reflect the subset-scoped count.
pub struct VitalSignsAndCountsInput<'a> {
    pub(crate) score_output: Option<&'a super::scoring::FileScoreOutput>,
    pub(crate) modules: &'a [crate::source::ModuleInfo],
    pub(crate) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(crate) needs_file_scores: bool,
    pub(crate) file_scores_slice: &'a [FileHealthScore],
    pub(crate) needs_hotspots: bool,
    pub(crate) hotspots: &'a [HotspotEntry],
    pub(crate) total_files: usize,
    pub(crate) subset: &'a SubsetFilter<'a>,
}

pub fn compute_vital_signs_and_counts(
    input: &VitalSignsAndCountsInput<'_>,
) -> (fallow_output::VitalSigns, fallow_output::VitalSignsCounts) {
    let analysis_counts = input.score_output.map(|o| {
        o.analysis_snapshot
            .counts_for(input.subset, &o.analysis_counts)
    });
    let module_filter_set: Option<rustc_hash::FxHashSet<crate::discover::FileId>> =
        if input.subset.is_full() {
            None
        } else {
            Some(
                input
                    .modules
                    .iter()
                    .filter_map(|m| {
                        let path = input.file_paths.get(&m.file_id)?;
                        if input.subset.matches(path) {
                            Some(m.file_id)
                        } else {
                            None
                        }
                    })
                    .collect(),
            )
        };
    let vs_input = vital_signs::VitalSignsInput {
        modules: input.modules,
        module_filter: module_filter_set.as_ref(),
        file_scores: if input.needs_file_scores {
            Some(input.file_scores_slice)
        } else {
            None
        },
        hotspots: if input.needs_hotspots {
            Some(input.hotspots)
        } else {
            None
        },
        total_files: input.total_files,
        analysis_counts,
    };
    let signs = vital_signs::compute_vital_signs(&vs_input);
    let counts = vital_signs::build_counts(&vs_input);
    (signs, counts)
}
