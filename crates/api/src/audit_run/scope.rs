//! Which files and findings one audit run covers.

use std::path::{Path, PathBuf};

use fallow_engine::changed_files::RenamedFile;
use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashSet;

/// Keep a dependency-level finding only when its manifest is in `changed_files`.
///
/// A dependency finding (unused, type-only, test-only or misplaced
/// dependency, unused catalog entry) is anchored to the package manifest or
/// the catalog file that declares it. `--changed-since` keeps these findings
/// whatever changed, because whether a dependency is used is a fact about the
/// whole graph. An audit reviews a changeset, so it reports them only when the
/// changeset touched the file that declares them. The rule covers the root
/// manifest and each workspace package manifest. A relative anchor (a catalog
/// file) is relative to `root`, the root of the analysis.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn scope_dependency_findings(
    results: &mut AnalysisResults,
    root: &Path,
    changed_files: &FxHashSet<PathBuf>,
) {
    let changed: FxHashSet<PathBuf> = changed_files
        .iter()
        .map(|path| dunce::simplified(path).to_path_buf())
        .collect();
    // Git reports changed files under the canonical top level, while a root
    // can be spelled through a symbolic link, so the canonical form of an
    // anchor is tried as well.
    let declared_in_change = |path: &Path| {
        let anchored = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        changed.contains(dunce::simplified(&anchored))
            || dunce::canonicalize(&anchored).is_ok_and(|canonical| changed.contains(&canonical))
    };
    results
        .unused_dependencies
        .retain(|finding| declared_in_change(&finding.dep.path));
    results
        .unused_dev_dependencies
        .retain(|finding| declared_in_change(&finding.dep.path));
    results
        .unused_optional_dependencies
        .retain(|finding| declared_in_change(&finding.dep.path));
    results
        .type_only_dependencies
        .retain(|finding| declared_in_change(&finding.dep.path));
    results
        .test_only_dependencies
        .retain(|finding| declared_in_change(&finding.dep.path));
    results
        .dev_dependencies_in_production
        .retain(|finding| declared_in_change(&finding.dep.path));
    results
        .unused_catalog_entries
        .retain(|finding| declared_in_change(&finding.entry.path));
}

/// Detect base..head renames for rename-aware attribution.
///
/// Best effort: when git fails, the audit uses plain path-keyed attribution,
/// which reports findings that moved with a file as introduced.
#[must_use]
pub fn renamed_files(root: &Path, base_ref: &str) -> Vec<RenamedFile> {
    fallow_engine::changed_files::try_get_renamed_files(root, base_ref).unwrap_or_default()
}

/// The files the base pass covers: the changed files plus the pre-rename path
/// of each rename, so base findings on moved files are in the base snapshot and
/// the rename remap can move them onto their head paths.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn base_focus_files(
    changed_files: &FxHashSet<PathBuf>,
    renames: &[RenamedFile],
) -> FxHashSet<PathBuf> {
    changed_files
        .iter()
        .cloned()
        .chain(renames.iter().map(|rename| rename.from.clone()))
        .collect()
}

/// Express `files` (absolute paths under `from_root`) under `to_root`.
///
/// Returns `None` when no path maps. The caller then leaves the base results
/// unfiltered: a filter with an empty set would remove every base finding and
/// make every inherited head finding look introduced.
///
/// The focus set comes from `git rev-parse --show-toplevel`, whose spelling
/// can differ from the canonical root of the caller (Windows 8.3 components,
/// drive-letter case, verbatim `\\?\` prefixes), so the simplified and the
/// canonical forms are both tried before a path is given up.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn remap_focus_files(
    files: &FxHashSet<PathBuf>,
    from_root: &Path,
    to_root: &Path,
) -> Option<FxHashSet<PathBuf>> {
    let simple_from = dunce::simplified(from_root).to_path_buf();
    let canonical_from = dunce::canonicalize(from_root).unwrap_or_else(|_| simple_from.clone());
    let mut remapped = FxHashSet::default();
    for file in files {
        let simple_file = dunce::simplified(file);
        let relative = simple_file
            .strip_prefix(&simple_from)
            .or_else(|_| simple_file.strip_prefix(&canonical_from))
            .map(Path::to_path_buf)
            .ok()
            .or_else(|| {
                let canonical_file = dunce::canonicalize(file).ok()?;
                canonical_file
                    .strip_prefix(&canonical_from)
                    .map(Path::to_path_buf)
                    .ok()
            });
        if let Some(relative) = relative {
            remapped.insert(to_root.join(relative));
        }
    }
    if remapped.is_empty() {
        return None;
    }
    Some(remapped)
}

/// Istanbul coverage inputs of the base pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BaseCoverageInputs {
    /// Coverage map path, resolved against the head root.
    pub coverage: Option<PathBuf>,
    /// Prefix to strip from recorded paths before they rebase onto the base
    /// worktree.
    pub coverage_root: Option<PathBuf>,
}

/// Coverage inputs for the base-worktree pass.
///
/// The Istanbul map records head-checkout paths, while the base pass analyzes
/// a temporary worktree. Without a rebase no coverage entry matches a base
/// file, base CRAP falls back to the reachability estimate, and unchanged
/// functions flip to `introduced` (#2347). Without an explicit map, the head
/// pass auto-detects `coverage/coverage-final.json` against the head root,
/// which the base worktree never has, so the same detection runs here. When no
/// explicit `coverage_root` exists, the canonical head root becomes the strip
/// prefix, so every entry rebases onto the base worktree. An explicit
/// `coverage_root` stays as it is: the base pass rebases it onto its own root.
#[must_use]
pub fn base_coverage_inputs(
    root: &Path,
    coverage: Option<&Path>,
    coverage_root: Option<&Path>,
) -> BaseCoverageInputs {
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let coverage = coverage.map_or_else(
        || fallow_engine::health::scoring::auto_detect_coverage(&canonical_root),
        |coverage| {
            Some(fallow_engine::health::scoring::resolve_relative_to_root(
                coverage,
                Some(&canonical_root),
            ))
        },
    );
    let coverage_root = match (&coverage, coverage_root) {
        (_, Some(explicit)) => Some(explicit.to_path_buf()),
        (Some(_), None) => Some(canonical_root),
        (None, None) => None,
    };
    BaseCoverageInputs {
        coverage,
        coverage_root,
    }
}
