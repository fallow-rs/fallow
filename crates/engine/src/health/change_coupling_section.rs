//! The `change_coupling` health section and the change dispersion values.

use fallow_output::{ChangeCouplingReport, ChangeCouplingThresholds, HiddenCouplingPair};

use super::HealthOptions;
use super::hotspots::{ChurnFetchResult, is_excluded_from_hotspots};
use crate::change_coupling::{
    ChangeCouplingSettings, ChangeHistoryAnalysis, STRONG_COUPLING_DEGREE, StaticLinkIndex,
    analyze_change_history,
};

/// Change history facts for one health run.
pub(super) struct ChangeCouplingOutcome {
    /// Dispersion per file and the hidden pairs in the report scope.
    pub(super) analysis: ChangeHistoryAnalysis,
    /// The section, before `--top`.
    pub(super) report: ChangeCouplingReport,
}

/// Scope filters for the hidden pairs.
pub(super) struct ChangeCouplingScope<'a> {
    pub(super) root: &'a std::path::Path,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(super) diff_index: Option<&'a fallow_output::DiffIndex>,
}

/// Analyze the commits of the churn pass for change dispersion and hidden
/// change coupling.
///
/// A shallow clone or a history with fewer analyzed commits than the minimum
/// revisions gives an empty pair list and a workspace diagnostic. Dispersion
/// is still computed from the commits that the history holds.
pub(super) fn compute_change_coupling(
    opts: &HealthOptions<'_>,
    config: &fallow_config::ResolvedConfig,
    churn_fetch: &ChurnFetchResult,
    links: &StaticLinkIndex,
    scope: &ChangeCouplingScope<'_>,
) -> ChangeCouplingOutcome {
    let settings = ChangeCouplingSettings::from(&config.health.change_coupling);
    let mut analysis = analyze_change_history(&churn_fetch.result.commits, links, &settings);

    if churn_fetch.result.shallow_clone {
        analysis.hidden_pairs.clear();
        record_skipped(opts, "shallow-clone");
    } else if analysis.commits_analyzed < settings.min_revisions as usize {
        record_skipped(opts, "too-few-commits");
    }
    analysis
        .hidden_pairs
        .retain(|pair| pair_in_scope(&pair.path_a, &pair.path_b, scope));

    let report = ChangeCouplingReport {
        since: churn_fetch.history.display.clone(),
        commits_analyzed: analysis.commits_analyzed,
        commits_skipped: analysis.commits_skipped,
        total_pairs: analysis.hidden_pairs.len(),
        thresholds: ChangeCouplingThresholds {
            max_files_per_commit: settings.max_files_per_commit,
            min_revisions: settings.min_revisions,
            min_shared_revisions: settings.min_shared_revisions,
            min_degree: settings.min_degree,
            strong_degree: STRONG_COUPLING_DEGREE,
        },
        pairs: analysis
            .hidden_pairs
            .iter()
            .map(|pair| HiddenCouplingPair {
                path_a: pair.path_a.clone(),
                path_b: pair.path_b.clone(),
                revisions_a: pair.revisions_a,
                revisions_b: pair.revisions_b,
                shared_revisions: pair.shared_revisions,
                degree: pair.degree,
                strong: pair.is_strong(),
            })
            .collect(),
    };
    ChangeCouplingOutcome { analysis, report }
}

/// Keep a pair when neither file is ignored, at least one file is in the
/// workspace scope, and (with a diff) at least one file is in the diff.
fn pair_in_scope(
    path_a: &std::path::Path,
    path_b: &std::path::Path,
    scope: &ChangeCouplingScope<'_>,
) -> bool {
    let ignored = |path: &std::path::Path| {
        is_excluded_from_hotspots(path, scope.root, scope.ignore_set, None)
    };
    if ignored(path_a) || ignored(path_b) {
        return false;
    }
    let in_workspace = |path: &std::path::Path| {
        scope
            .ws_roots
            .is_none_or(|roots| roots.iter().any(|root| path.starts_with(root)))
    };
    if !in_workspace(path_a) && !in_workspace(path_b) {
        return false;
    }
    let Some(diff_index) = scope.diff_index else {
        return true;
    };
    let in_diff = |path: &std::path::Path| match diff_index.key_for(path, scope.root) {
        Some(rel) => diff_index.touches_file(&rel),
        None => true,
    };
    in_diff(path_a) || in_diff(path_b)
}

fn record_skipped(opts: &HealthOptions<'_>, cause: &str) {
    super::diagnostics::record_health_diagnostic(
        opts.root,
        None,
        fallow_types::workspace::WorkspaceDiagnosticKind::ChangeCouplingSkipped {
            cause: cause.to_owned(),
        },
    );
}
