//! Unlisted-dependency collection and the per-workspace `ignoreDependencies`
//! filter of unused dependencies.

use std::path::Path;

use rustc_hash::FxHashMap;

use fallow_config::ResolvedConfig;

use super::{
    UnlistedDependencyContext, collect_unlisted_import_sites, should_skip_unlisted_package,
};
use crate::results::{ImportSite, UnlistedDependency, UnusedDependency};

type UnusedCategories = (
    Vec<UnusedDependency>,
    Vec<UnusedDependency>,
    Vec<UnusedDependency>,
);

/// Drop the unused dependencies of the workspace manifest at `pkg_path` that a
/// `workspaces.settings` list hides. It runs after the global list, so a
/// workspace key gets credit only for a finding that the global list did not
/// hide.
pub(super) fn drop_workspace_ignored(
    (mut prod, mut dev, mut optional): UnusedCategories,
    config: &ResolvedConfig,
    pkg_path: &Path,
) -> UnusedCategories {
    let settings = &config.workspace_settings;
    if !settings.is_empty() {
        for findings in [&mut prod, &mut dev, &mut optional] {
            findings.retain(|dep| !settings.suppresses_dependency_at(pkg_path, &dep.package_name));
        }
    }
    (prod, dev, optional)
}

/// Walk `package_usage`, gathering per-package import sites into findings.
pub(super) fn collect_unlisted_dependencies(
    ctx: &UnlistedDependencyContext<'_>,
) -> Vec<UnlistedDependency> {
    let mut unlisted: FxHashMap<String, Vec<ImportSite>> = FxHashMap::default();

    for (package_name, file_ids) in &ctx.graph.package_usage {
        if should_skip_unlisted_package(package_name, ctx) {
            continue;
        }
        let mut unlisted_sites = collect_unlisted_import_sites(package_name, file_ids, ctx);
        if unlisted_sites.is_empty() || ctx.ignore_deps.suppresses(package_name) {
            continue;
        }
        let settings = &ctx.config.workspace_settings;
        if !settings.is_empty() {
            unlisted_sites
                .retain(|site| !settings.suppresses_dependency_at(&site.path, package_name));
        }
        if !unlisted_sites.is_empty() {
            unlisted_sites.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
            unlisted_sites.dedup_by(|a, b| a.path == b.path);
            unlisted.insert(package_name.clone(), unlisted_sites);
        }
    }

    unlisted
        .into_iter()
        .map(|(name, sites)| UnlistedDependency {
            package_name: name,
            imported_from: sites,
        })
        .collect()
}
