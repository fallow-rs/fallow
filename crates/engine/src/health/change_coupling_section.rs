//! The `change_coupling` health section and the change dispersion values.

use fallow_output::{ChangeCouplingReport, ChangeCouplingThresholds, HiddenCouplingPair};

use super::HealthOptions;
use super::hotspots::ChurnFetchResult;
use crate::change_coupling::{
    ChangeCouplingSettings, ChangeHistoryAnalysis, PairScope, STRONG_COUPLING_DEGREE,
    StaticLinkIndex, analyze_change_history,
};

/// Change history facts for one health run.
pub(super) struct ChangeCouplingOutcome {
    /// Dispersion per file and the hidden pairs in the report scope.
    pub(super) analysis: ChangeHistoryAnalysis,
    /// The section, before `--top`.
    pub(super) report: ChangeCouplingReport,
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
    scope: &PairScope<'_>,
) -> ChangeCouplingOutcome {
    let settings = ChangeCouplingSettings::from(&config.health.change_coupling);
    let mut analysis = analyze_change_history(&churn_fetch.result.commits, links, &settings);

    // The diagnostic explains an empty section, so a run that only reads the
    // pairs for its targets does not record it.
    let section_requested = opts.hotspots && !opts.score_only_output;
    if churn_fetch.result.shallow_clone {
        analysis.hidden_pairs.clear();
        if section_requested {
            record_skipped(opts, "shallow-clone");
        }
    } else if section_requested && analysis.commits_analyzed < settings.min_revisions as usize {
        record_skipped(opts, "too-few-commits");
    }
    analysis
        .hidden_pairs
        .retain(|pair| scope.contains(&pair.path_a, &pair.path_b));

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

fn record_skipped(opts: &HealthOptions<'_>, cause: &str) {
    super::diagnostics::record_health_diagnostic(
        opts.root,
        None,
        fallow_types::workspace::WorkspaceDiagnosticKind::ChangeCouplingSkipped {
            cause: cause.to_owned(),
        },
    );
}
