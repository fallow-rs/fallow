//! The duplication detector runs of `fallow dupes`, and the audit view of
//! their result.

use fallow_api::audit_run::DuplicationView;
use fallow_config::ResolvedConfig;
use fallow_engine::duplicates::DuplicationAnalysis;

use super::{DupesOptions, DupesResult};

impl DupesResult {
    /// The duplication analysis of this result, as `fallow audit` reads it.
    pub(crate) fn audit_view(&self) -> DuplicationView<'_> {
        DuplicationView {
            clone_groups: self.report.clone_groups.iter().collect(),
            ignored_groups: &self.ignored_clone_groups,
            root: &self.config.root,
            duplication_percentage: self.report.stats.duplication_percentage,
            threshold: self.threshold,
            config: &self.config,
        }
    }
}

pub(super) fn run_duplication_analysis(
    opts: &DupesOptions<'_>,
    config: &ResolvedConfig,
    files: &[fallow_types::discover::DiscoveredFile],
    dupes_config: &fallow_config::DuplicatesConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) -> DuplicationAnalysis {
    let cache_dir = (!opts.no_cache).then_some(config.cache_dir.as_path());
    if let Some(changed_files) = changed_files {
        let changed_files = changed_files.iter().cloned().collect::<Vec<_>>();
        fallow_engine::duplicates::find_duplicates_touching_files_with_defaults(
            &config.root,
            files,
            dupes_config,
            &changed_files,
            cache_dir,
        )
    } else {
        fallow_engine::duplicates::find_duplicates_with_defaults(
            &config.root,
            files,
            dupes_config,
            cache_dir,
        )
    }
}

pub(super) fn run_duplication_analysis_with_session(
    opts: &DupesOptions<'_>,
    session: &fallow_engine::session::AnalysisSession,
    dupes_config: &fallow_config::DuplicatesConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) -> DuplicationAnalysis {
    let cache_dir = (!opts.no_cache).then_some(session.config().cache_dir.as_path());
    if let Some(changed_files) = changed_files {
        let changed_files = changed_files.iter().cloned().collect::<Vec<_>>();
        session.find_duplicates_touching_files_with_defaults(
            dupes_config,
            &changed_files,
            cache_dir,
        )
    } else {
        session.find_duplicates_with_defaults(dupes_config, cache_dir)
    }
}
