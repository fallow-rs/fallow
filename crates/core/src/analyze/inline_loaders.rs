//! Dependency credit for webpack inline loaders (`!raw-loader!./file.js`).
//!
//! The resolver sends an inline loader request to its resource. The loaders
//! run at build time, so they are tooling, not runtime imports. This module
//! returns their package names so the unused-dependency pass treats them like
//! the loaders that a webpack config references. A loader that is not in
//! `package.json` is not reported as unlisted, the same as a loader in a
//! webpack config.

use std::path::Path;

use rustc_hash::FxHashSet;

use crate::graph::ModuleGraph;
use crate::resolve::{
    InlineLoaderRequest, ResolveResult, ResolvedModule, extract_package_name, is_bare_specifier,
};

/// Suffix that webpack 1 added to a loader name without it (`raw` loads
/// `raw-loader`).
const LOADER_SUFFIX: &str = "-loader";

/// Collect the package names of the inline loaders in every import source.
///
/// A loader that is a path to a local file has no package, so it is skipped.
/// An unscoped loader name without the `-loader` suffix also credits the name
/// with the suffix, because webpack 1 resolved `raw` to `raw-loader`. The
/// result is sorted, so the order is deterministic.
pub(super) fn collect_inline_loader_referenced_deps(
    modules: &[ResolvedModule],
    graph: &ModuleGraph,
) -> Vec<String> {
    let mut packages: FxHashSet<String> = FxHashSet::default();
    for module in modules {
        for edge in module.all_resolved_source_edges() {
            let Some(request) = InlineLoaderRequest::resolved(edge.source_specifier(), || {
                target_path(edge.target(), graph)
            }) else {
                continue;
            };
            for loader in request.loaders() {
                if !is_bare_specifier(loader) {
                    continue;
                }
                let package = extract_package_name(loader);
                if !package.starts_with('@') && !package.ends_with(LOADER_SUFFIX) {
                    packages.insert(format!("{package}{LOADER_SUFFIX}"));
                }
                packages.insert(package);
            }
        }
    }
    let mut packages: Vec<String> = packages.into_iter().collect();
    packages.sort_unstable();
    packages
}

/// The path of the file that an import resolved to, if it is a file.
fn target_path<'a>(target: &'a ResolveResult, graph: &'a ModuleGraph) -> Option<&'a Path> {
    if let ResolveResult::ExternalFile(path) = target {
        return Some(path);
    }
    let file_id = target.internal_file_id()?;
    graph
        .modules
        .get(file_id.0 as usize)
        .map(|module| module.path.as_path())
}
