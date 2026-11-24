//! The `duplicates.ignore` and `health.ignore` globs that match no file, as
//! `workspace_diagnostics[]` entries of the run that reads the setting
//! (issue #5202). The dead-code envelope reports the same entries through
//! [`crate::dead_code::config_pattern_diagnostics`].

use fallow_config::{ResolvedConfig, WorkspaceDiagnostic, WorkspaceDiagnosticKind};
use fallow_types::discover::DiscoveredFile;

/// One `duplicates-ignore-unmatched` entry per `duplicates.ignore` glob that
/// matches none of `files` and no other project file, in config order.
#[must_use]
pub fn duplicates_ignore_diagnostics(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<WorkspaceDiagnostic> {
    unmatched(config, &config.duplicates.ignore, files, |pattern| {
        WorkspaceDiagnosticKind::DuplicatesIgnoreUnmatched { pattern }
    })
}

/// One `health-ignore-unmatched` entry per `health.ignore` glob that matches
/// none of `files` and no other project file, in config order.
#[must_use]
pub fn health_ignore_diagnostics(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<WorkspaceDiagnostic> {
    unmatched(config, &config.health.ignore, files, |pattern| {
        WorkspaceDiagnosticKind::HealthIgnoreUnmatched { pattern }
    })
}

/// A `--production` run analyzes fewer files, so a glob for test files
/// matches no file of that run. Such a run reports no entry, like the
/// dead-code envelope.
fn unmatched(
    config: &ResolvedConfig,
    patterns: &[String],
    files: &[DiscoveredFile],
    kind: impl Fn(String) -> WorkspaceDiagnosticKind,
) -> Vec<WorkspaceDiagnostic> {
    if config.production || patterns.is_empty() {
        return Vec::new();
    }
    let hits = config
        .path_glob_effect(&[patterns], files.iter().map(|file| file.path.as_path()))
        .into_iter()
        .next()
        .unwrap_or_default();
    patterns
        .iter()
        .zip(hits)
        .filter(|(_, hit)| !hit)
        .map(|(pattern, _)| {
            WorkspaceDiagnostic::new(&config.root, config.root.clone(), kind(pattern.clone()))
                .into_root_relative(&config.root)
        })
        .collect()
}
