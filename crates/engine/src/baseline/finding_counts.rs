use std::path::Path;

use super::{BaselineData, filter_new_issues};

pub(super) fn baseline_finding_count(results: &crate::results::AnalysisResults) -> usize {
    results.total_issues()
        - results.enforced_prop_drilling_count()
        - results.enforced_thin_wrapper_count()
        - results.enforced_duplicate_prop_shape_count()
        + results.prop_drilling_chains.len()
        + results.thin_wrappers.len()
        + results.duplicate_prop_shapes.len()
}

/// Baseline entries that match a finding of the unused-file cascade filter.
///
/// Such a finding still exists: the report hides it because its file is an
/// unused file. Its baseline entry therefore still describes the project and
/// must not count as stale. The hidden findings sit in unused files and the
/// reported ones do not, so no entry can match both.
pub(super) fn cascade_hidden_matches(
    results: &crate::results::AnalysisResults,
    baseline: &BaselineData,
    root: &Path,
) -> usize {
    if results.cascade.hidden.is_empty() {
        return 0;
    }
    let hidden = results.cascade.hidden.to_results();
    let before = hidden.total_issues();
    before.saturating_sub(filter_new_issues(hidden, baseline, root).total_issues())
}
