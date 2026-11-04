//! Entry points from the `entry` and `supportEntry` config globs.

use fallow_config::ResolvedConfig;
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};

use super::EntryPointDiscovery;

/// Push entries for files matching the user-configured manual entry glob patterns.
#[expect(
    clippy::expect_used,
    reason = "entry glob patterns are validated before entry point discovery"
)]
pub(super) fn push_manual_entry_matches(
    entries: &mut Vec<EntryPoint>,
    config: &ResolvedConfig,
    relative_paths: &[String],
    files: &[DiscoveredFile],
) {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in &config.entry_patterns {
        builder.add(
            fallow_config::compile_path_glob(pattern)
                .expect("entry pattern was validated at config load time"),
        );
    }
    let Ok(glob_set) = builder.build() else {
        return;
    };
    if glob_set.is_empty() {
        return;
    }
    for (idx, rel) in relative_paths.iter().enumerate() {
        if glob_set.is_match(rel) {
            entries.push(EntryPoint {
                path: files[idx].path.clone(),
                source: EntryPointSource::ManualEntry,
            });
        }
    }
}

/// Push the files that a `supportEntry` glob matches: support entry points in
/// default mode, and files that take no part in the analysis in production
/// mode. A file that another entry source also seeds stays an entry point,
/// because the dedup of the categorized entry points drops it from the
/// excluded set.
#[expect(
    clippy::expect_used,
    reason = "supportEntry glob patterns are validated before entry point discovery"
)]
pub(super) fn push_support_entry_matches(
    discovery: &mut EntryPointDiscovery,
    config: &ResolvedConfig,
    relative_paths: &[String],
    files: &[DiscoveredFile],
) {
    if config.support_entry_patterns.is_empty() {
        return;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in &config.support_entry_patterns {
        builder.add(
            fallow_config::compile_path_glob(pattern)
                .expect("supportEntry pattern was validated at config load time"),
        );
    }
    let Ok(glob_set) = builder.build() else {
        return;
    };
    for (idx, rel) in relative_paths.iter().enumerate() {
        if !glob_set.is_match(rel) {
            continue;
        }
        let path = files[idx].path.clone();
        if config.production {
            discovery.production_excluded.push(path);
        } else {
            discovery.support_entries.push(EntryPoint {
                path,
                source: EntryPointSource::ManualEntry,
            });
        }
    }
}
