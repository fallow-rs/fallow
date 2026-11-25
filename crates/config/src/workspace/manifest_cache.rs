//! The per-discovery memo of member manifest loads.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use super::PackageJson;
use super::deno_json::load_member_package_manifest_in;

/// The outcome of one member manifest load: the name, the manifest and the
/// dependency names, `None` for a directory with no manifest, or the error.
type ManifestOutcome = Result<Option<(String, PackageJson, Vec<String>)>, String>;

/// Memoized member manifest outcomes for one discovery run.
///
/// The same directory is commonly matched by more than one workspace source
/// (identical globs in `package.json` and `pnpm-workspace.yaml`, tsconfig
/// references overlapping npm workspaces). Each manifest load costs a
/// `package.json` read plus Deno `deno.json` / `deno.jsonc` probes, so repeat
/// visits replay the memoized outcome instead of touching the filesystem again.
///
/// The cache also holds the project scope base of the analysis root (see
/// [`crate::project_scope_base`]), so a member directory that links to
/// another directory inside the project loads.
#[derive(Debug, Default)]
pub(super) struct ManifestCache {
    scope: Option<PathBuf>,
    outcomes: FxHashMap<PathBuf, ManifestOutcome>,
}

impl ManifestCache {
    /// An empty cache for a discovery from the analysis root `root`.
    pub(super) fn new(root: &Path) -> Self {
        Self {
            scope: Some(crate::project_scope_base(root)),
            outcomes: FxHashMap::default(),
        }
    }

    /// The number of memoized outcomes.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.outcomes.len()
    }
}

/// Load a member manifest through the per-discovery memo, preserving the
/// exact per-call outcome (including errors) on repeat visits.
pub(super) fn load_member_package_manifest_cached(
    dir: &Path,
    cache: &mut ManifestCache,
) -> ManifestOutcome {
    if let Some(cached) = cache.outcomes.get(dir) {
        return cached.clone();
    }
    let outcome = load_member_package_manifest_in(cache.scope.as_deref(), dir);
    cache.outcomes.insert(dir.to_path_buf(), outcome.clone());
    outcome
}
