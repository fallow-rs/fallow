//! The stderr warning for config globs that match fewer files since a single
//! `*` or `?` stays within one path segment (see
//! [`fallow_config::find_legacy_glob_narrowings`]).

use std::path::{Path, PathBuf};

use fallow_config::ResolvedConfig;
use fallow_types::discover::{DiscoveredFile, EntryPointSource};
use fallow_types::path_util::display_relative;
use rustc_hash::FxHashSet;

use super::CategorizedEntryPoints;
use super::walk::should_emit_note_once;

/// Find the config globs that match fewer discovered files now that a
/// single `*` or `?` stays within one path segment. `entry_fields` selects
/// the `entry` and `dynamicallyLoaded` lists, or every other list.
///
/// The old matcher compiles only for patterns with a single-segment
/// wildcard, and one pass over the discovered files checks every pattern.
fn find_legacy_glob_narrowings(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    entry_fields: bool,
) -> Vec<fallow_config::LegacyGlobNarrowing> {
    if config.quiet {
        return Vec::new();
    }
    let groups: Vec<fallow_config::LegacyGlobGroup> = config
        .legacy_glob_groups
        .iter()
        .filter(|group| group.counts_entry_sources() == entry_fields)
        .cloned()
        .collect();
    if groups.is_empty() {
        return Vec::new();
    }
    let relative: Vec<&Path> = files
        .iter()
        .map(|file| file.path.strip_prefix(&config.root).unwrap_or(&file.path))
        .collect();
    fallow_config::find_legacy_glob_narrowings(&groups, &relative)
}

/// Print the warning for `narrowing` on stderr once per process. JSON
/// output does not change.
fn warn_legacy_glob_narrowing(
    config: &ResolvedConfig,
    narrowing: &fallow_config::LegacyGlobNarrowing,
) {
    if should_emit_note_once(format!(
        "legacy-glob::{}::{}::{}",
        config.root.display(),
        narrowing.field,
        narrowing.pattern
    )) {
        tracing::warn!("{}", narrowing.message());
    }
}

/// Warn for each narrowed glob of a field that does not make entry points.
/// The entry-point stage checks `entry` and `dynamicallyLoaded`, because
/// only that stage knows the other entry sources.
pub(super) fn warn_legacy_glob_narrowings(config: &ResolvedConfig, files: &[DiscoveredFile]) {
    for narrowing in find_legacy_glob_narrowings(config, files, false) {
        warn_legacy_glob_narrowing(config, &narrowing);
    }
}

/// Warn for each `entry` or `dynamicallyLoaded` glob that lost files to the
/// single-segment `*` rule. A lost file that another entry source declares
/// is still an entry point, so it does not count.
pub(super) fn warn_entry_legacy_glob_narrowings(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    entry_points: &CategorizedEntryPoints,
) {
    for mut narrowing in find_legacy_glob_narrowings(config, files, true) {
        let is_entry_field = narrowing.field == "entry";
        let declared: FxHashSet<String> = entry_points
            .all
            .iter()
            .filter(|entry| {
                if is_entry_field {
                    !matches!(entry.source, EntryPointSource::ManualEntry)
                } else {
                    !matches!(entry.source, EntryPointSource::DynamicallyLoaded)
                }
            })
            // `components` drops the `./` part that a package.json path can
            // add, for example `packages/p/./lib/a.ts`.
            .map(|entry| {
                display_relative(&config.root, &entry.path.components().collect::<PathBuf>())
            })
            .collect();
        if narrowing.retain_files(|file| !declared.contains(file)) {
            warn_legacy_glob_narrowing(config, &narrowing);
        }
    }
}
