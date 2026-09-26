//! Regression baseline for `fallow list --entry-weight`.
//!
//! The baseline stores `eager_bytes`, `eager_modules` and the eager package
//! names of each runtime entry. A comparison is report-only by default;
//! `--fail-on-regression` makes an entry that grew more than the tolerance fail
//! the run. An entry that is new in the run has nothing to grow from, so it is
//! reported and never fails the gate.

use std::path::Path;
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_output::{EntryWeightDelta, EntryWeightListing, EntryWeightRegression};
use rustc_hash::{FxHashMap, FxHashSet};

use super::counts::{REGRESSION_SCHEMA_VERSION, RegressionBaseline};
use super::tolerance::Tolerance;
use crate::error::emit_error;

/// Entry weights saved in a regression baseline file.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EntryWeightCounts {
    /// One row per runtime entry, sorted by path.
    #[serde(default)]
    pub entries: Vec<EntryWeightBaselineEntry>,
}

/// Saved weight of one runtime entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntryWeightBaselineEntry {
    /// Entry file, relative to the project root.
    pub path: String,
    /// Source bytes that load before the entry runs.
    pub eager_bytes: u64,
    /// Modules that load before the entry runs.
    pub eager_modules: usize,
    /// Names of the packages on the eager path.
    #[serde(default)]
    pub eager_packages: Vec<String>,
}

impl EntryWeightCounts {
    /// Capture the weights of the current listing.
    #[must_use]
    pub fn from_listing(listing: &EntryWeightListing) -> Self {
        let mut entries: Vec<EntryWeightBaselineEntry> = listing
            .entries
            .iter()
            .map(|entry| EntryWeightBaselineEntry {
                path: entry.path.clone(),
                eager_bytes: entry.eager_bytes,
                eager_modules: entry.eager_modules,
                eager_packages: entry
                    .eager_packages
                    .iter()
                    .map(|package| package.name.clone())
                    .collect(),
            })
            .collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Self { entries }
    }
}

/// Where a `list --entry-weight` run reads and writes its regression baseline.
pub struct EntryWeightGate<'a> {
    /// Fail the run when an entry grew more than the tolerance.
    pub fail_on_regression: bool,
    /// Allowed growth of `eager_bytes` per entry: bytes or a percentage.
    pub tolerance: Tolerance,
    /// Baseline file to compare against.
    pub baseline_file: Option<&'a Path>,
    /// Baseline file to write after the run.
    pub save_file: Option<&'a Path>,
    /// `--save-regression-baseline` without a PATH, which targets the config
    /// file. The config baseline holds issue counts only.
    pub save_to_config: bool,
}

impl EntryWeightGate<'_> {
    /// Refuse flag combinations that have no entry weight meaning.
    ///
    /// # Errors
    ///
    /// Returns exit code 2 with a message for a gate without a baseline file,
    /// or for a save into the config file.
    pub fn validate(&self, output: OutputFormat) -> Result<(), ExitCode> {
        if self.save_to_config {
            return Err(emit_error(
                "list --entry-weight saves its regression baseline to a file: pass \
                 --save-regression-baseline <PATH>",
                2,
                output,
            ));
        }
        if self.fail_on_regression && self.baseline_file.is_none() {
            return Err(emit_error(
                "list --entry-weight --fail-on-regression needs --regression-baseline <PATH>; \
                 create one with list --entry-weight --save-regression-baseline <PATH>",
                2,
                output,
            ));
        }
        Ok(())
    }
}

/// Load the entry weights of a baseline file.
///
/// # Errors
///
/// Returns exit code 2 when the file cannot be read or holds no entry weights.
pub fn load_entry_weight_baseline(
    path: &Path,
    output: OutputFormat,
) -> Result<EntryWeightCounts, ExitCode> {
    let baseline = super::load_regression_baseline(path, output)?;
    baseline.entry_weight.ok_or_else(|| {
        emit_error(
            &format!(
                "regression baseline '{}' has no entry weight data; save one with \
                 fallow list --entry-weight --save-regression-baseline {}",
                path.display(),
                path.display()
            ),
            2,
            output,
        )
    })
}

/// Compare the listing with the saved weights.
///
/// `in_scope` keeps a saved entry in the comparison; a scoped run passes the
/// positional path filter so saved entries outside it are not reported as gone.
#[must_use]
pub fn compare_entry_weight(
    listing: &EntryWeightListing,
    baseline: &EntryWeightCounts,
    tolerance: Tolerance,
    enforced: bool,
    in_scope: impl Fn(&str) -> bool,
) -> EntryWeightRegression {
    let saved: FxHashMap<&str, &EntryWeightBaselineEntry> = baseline
        .entries
        .iter()
        .filter(|entry| in_scope(&entry.path))
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let mut entries: Vec<EntryWeightDelta> = listing
        .entries
        .iter()
        .map(|current| {
            let before = saved.get(current.path.as_str()).copied();
            let exceeded = before.is_some_and(|before| {
                tolerance.exceeded(to_usize(before.eager_bytes), to_usize(current.eager_bytes))
            });
            let known: FxHashSet<&str> = before
                .map(|before| before.eager_packages.iter().map(String::as_str).collect())
                .unwrap_or_default();
            let new_eager_packages = if before.is_some() {
                current
                    .eager_packages
                    .iter()
                    .filter(|package| !known.contains(package.name.as_str()))
                    .map(|package| package.name.clone())
                    .collect()
            } else {
                Vec::new()
            };
            EntryWeightDelta {
                path: current.path.clone(),
                baseline_eager_bytes: before.map(|before| before.eager_bytes),
                current_eager_bytes: Some(current.eager_bytes),
                baseline_eager_modules: before.map(|before| before.eager_modules),
                current_eager_modules: Some(current.eager_modules),
                new_eager_packages,
                exceeded,
            }
        })
        .collect();
    let current_paths: FxHashSet<&str> = listing
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    entries.extend(
        saved
            .values()
            .filter(|before| !current_paths.contains(before.path.as_str()))
            .map(|before| EntryWeightDelta {
                path: before.path.clone(),
                baseline_eager_bytes: Some(before.eager_bytes),
                current_eager_bytes: None,
                baseline_eager_modules: Some(before.eager_modules),
                current_eager_modules: None,
                new_eager_packages: Vec::new(),
                exceeded: false,
            }),
    );
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    EntryWeightRegression {
        tolerance: tolerance.label(),
        enforced,
        exceeded: entries.iter().any(|entry| entry.exceeded),
        entries,
    }
}

fn to_usize(bytes: u64) -> usize {
    usize::try_from(bytes).unwrap_or(usize::MAX)
}

/// Write the entry weights into `path`, keeping any issue counts the file
/// already holds.
///
/// # Errors
///
/// Returns exit code 2 when the file cannot be serialized or written.
pub fn save_entry_weight_baseline(
    path: &Path,
    root: &Path,
    counts: EntryWeightCounts,
    output: OutputFormat,
) -> Result<(), ExitCode> {
    let existing = super::baseline::read_existing_baseline(path);
    let baseline = RegressionBaseline {
        schema_version: REGRESSION_SCHEMA_VERSION,
        fallow_version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: super::baseline::chrono_now(),
        git_sha: super::baseline::current_git_sha(root),
        analysis_identity: existing
            .as_ref()
            .map(|existing| existing.analysis_identity.clone())
            .unwrap_or_default(),
        check: existing
            .as_ref()
            .and_then(|existing| existing.check.clone()),
        dupes: existing
            .as_ref()
            .and_then(|existing| existing.dupes.clone()),
        entry_weight: Some(counts),
        flags: existing.and_then(|existing| existing.flags),
    };
    super::baseline::write_regression_baseline(path, root, &baseline, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{EagerPackageOutput, EntryWeightOutput, EntryWeightUnit};

    fn listing(rows: &[(&str, u64, &[&str])]) -> EntryWeightListing {
        let entries: Vec<EntryWeightOutput> = rows
            .iter()
            .map(|&(path, eager_bytes, packages)| EntryWeightOutput {
                path: path.to_string(),
                source: "test".to_string(),
                eager_modules: 1,
                eager_bytes,
                eager_css_bytes: 0,
                deferred_modules: 0,
                deferred_bytes: 0,
                out_of_thread_modules: 0,
                out_of_thread_bytes: 0,
                eager_package_count: packages.len(),
                eager_packages: packages
                    .iter()
                    .map(|name| EagerPackageOutput {
                        name: (*name).to_string(),
                        specifiers: vec![(*name).to_string()],
                    })
                    .collect(),
                dominating_imports: Vec::new(),
            })
            .collect();
        EntryWeightListing {
            unit: EntryWeightUnit::SourceBytes,
            entry_count: entries.len(),
            entries,
            regression: None,
        }
    }

    #[test]
    fn a_new_entry_is_reported_but_never_exceeds_and_a_gone_entry_is_listed() {
        let before =
            EntryWeightCounts::from_listing(&listing(&[("a.ts", 100, &[]), ("gone.ts", 5, &[])]));
        let now = listing(&[("a.ts", 100, &[]), ("new.ts", 9_000, &["react"])]);

        let regression =
            compare_entry_weight(&now, &before, Tolerance::Absolute(0), true, |_| true);

        assert!(!regression.exceeded);
        let paths: Vec<&str> = regression.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["a.ts", "gone.ts", "new.ts"]);
        assert_eq!(regression.entries[1].current_eager_bytes, None);
        assert_eq!(regression.entries[2].baseline_eager_bytes, None);
        assert!(regression.entries[2].new_eager_packages.is_empty());
    }

    #[test]
    fn saved_entries_outside_the_scope_are_not_reported() {
        let before =
            EntryWeightCounts::from_listing(&listing(&[("a.ts", 100, &[]), ("b/x.ts", 5, &[])]));
        let now = listing(&[("a.ts", 101, &[])]);

        let regression =
            compare_entry_weight(&now, &before, Tolerance::Absolute(0), false, |path| {
                path == "a.ts"
            });

        assert_eq!(regression.entries.len(), 1);
        assert!(
            regression.exceeded,
            "one byte of growth exceeds a zero tolerance"
        );
        assert!(!regression.enforced);
    }
}
