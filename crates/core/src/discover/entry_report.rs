//! What a run reports about its entry points: the count per source, and the
//! `entry` patterns that had no effect (issue #3824).

use std::path::PathBuf;

use fallow_config::ResolvedConfig;
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
use rustc_hash::{FxHashMap, FxHashSet};

use super::CategorizedEntryPoints;
use super::entry_points::relative_paths_for;
use crate::results::EntryPointSummary;

/// Summarize entry points by source category for user-facing output.
pub fn summarize_entry_points(entry_points: &[EntryPoint]) -> EntryPointSummary {
    let mut counts: FxHashMap<String, usize> = FxHashMap::default();
    for ep in entry_points {
        let category = match &ep.source {
            EntryPointSource::PackageJsonMain
            | EntryPointSource::PackageJsonModule
            | EntryPointSource::PackageJsonExports
            | EntryPointSource::PackageJsonBin
            | EntryPointSource::PackageJsonScript => "package.json",
            EntryPointSource::Plugin { .. } => "plugin",
            EntryPointSource::TestFile => "test file",
            EntryPointSource::DefaultIndex => "default index",
            EntryPointSource::ManualEntry => "manual entry",
            EntryPointSource::InfrastructureConfig => "config",
            EntryPointSource::DynamicallyLoaded => "dynamically loaded",
        };
        *counts.entry(category.to_string()).or_insert(0) += 1;
    }
    let mut by_source: Vec<(String, usize)> = counts.into_iter().collect();
    by_source.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    EntryPointSummary {
        total: entry_points.len(),
        by_source,
    }
}

/// The paths of `entries` that a source other than the `entry` config
/// declares. The root pass collects them before its dedup, which keeps the
/// manual entry and drops the other source of the same path.
pub(super) fn non_manual_paths(entries: &[EntryPoint]) -> FxHashSet<PathBuf> {
    entries
        .iter()
        .filter(|entry| !matches!(entry.source, EntryPointSource::ManualEntry))
        .map(|entry| entry.path.clone())
        .collect()
}

/// Record which `entry` patterns matched a file that no other source made a
/// runtime entry point. `other_sources` holds the non-manual paths of the
/// root pass, collected before its dedup.
pub fn record_entry_pattern_effect(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    entry_points: &CategorizedEntryPoints,
    mut other_sources: FxHashSet<PathBuf>,
) {
    if config.entry_patterns.is_empty() {
        return;
    }
    other_sources.extend(non_manual_paths(&entry_points.runtime));
    config
        .entry_pattern_effect
        .record(manual_entry_pattern_effect(config, files, &other_sources));
}

/// Which `entry` config patterns matched a discovered file that no other
/// runtime entry source declares, one flag per pattern in config order
/// (issue #3824).
///
/// A pattern that matches no file gets `true`: it adds nothing either, but
/// the hint is about entries that duplicate what fallow already detects, so a
/// pattern that matches nothing is out of scope here.
#[expect(
    clippy::expect_used,
    reason = "entry glob patterns are validated before entry point discovery"
)]
fn manual_entry_pattern_effect(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    non_manual_runtime_paths: &FxHashSet<PathBuf>,
) -> Vec<bool> {
    let relative_paths = relative_paths_for(files, &config.root);
    config
        .entry_patterns
        .iter()
        .map(|pattern| {
            let matcher = fallow_config::compile_path_glob(pattern)
                .expect("entry pattern was validated at config load time")
                .compile_matcher();
            let mut matched = relative_paths
                .iter()
                .zip(files)
                .filter(|(relative, _)| matcher.is_match(relative.as_str()))
                .map(|(_, file)| &file.path)
                .peekable();
            matched.peek().is_none() || matched.any(|path| !non_manual_runtime_paths.contains(path))
        })
        .collect()
}
