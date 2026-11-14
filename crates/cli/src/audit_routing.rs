pub use fallow_api::routing::*;

/// Reviewer routing for the brief. Reuses the churn pass that the co-change
/// gaps read, and reads the history only when no such pass is on `check`.
pub(super) fn brief_routing(
    root: &std::path::Path,
    check: &crate::check::CheckResult,
    changed_files: &rustc_hash::FxHashSet<std::path::PathBuf>,
) -> RoutingFacts {
    match check.brief_churn.as_ref() {
        Some(churn) => compute_routing_from_churn(root, &check.config, changed_files, churn),
        None => compute_routing(root, &check.config, changed_files),
    }
}
