//! Files of circular dependencies, for file scores and refactoring targets.

/// Every file that takes part in a circular dependency.
pub(super) fn collect_circular_files(
    results: &crate::results::AnalysisResults,
) -> rustc_hash::FxHashSet<std::path::PathBuf> {
    results
        .circular_dependencies
        .iter()
        .flat_map(|c| c.cycle.files.iter().cloned())
        .collect()
}

/// The other files of each cycle, per file, sorted and without duplicates.
pub(super) fn collect_cycle_members(
    results: &crate::results::AnalysisResults,
) -> rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>> {
    let mut cycle_members: rustc_hash::FxHashMap<std::path::PathBuf, Vec<std::path::PathBuf>> =
        rustc_hash::FxHashMap::default();
    for cycle in &results.circular_dependencies {
        for file in &cycle.cycle.files {
            let others: Vec<std::path::PathBuf> = cycle
                .cycle
                .files
                .iter()
                .filter(|f| *f != file)
                .cloned()
                .collect();
            cycle_members
                .entry(file.clone())
                .or_default()
                .extend(others);
        }
    }
    for members in cycle_members.values_mut() {
        members.sort();
        members.dedup();
    }
    cycle_members
}
