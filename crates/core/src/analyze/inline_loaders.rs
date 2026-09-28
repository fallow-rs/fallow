//! Dependency credit for webpack inline loaders (`!raw-loader!./file.js`).
//!
//! The resolver sends an inline loader request to its resource. The loaders
//! run at build time, so they are tooling, not runtime imports. This module
//! returns their package names so the unused-dependency pass treats them like
//! the loaders that a webpack config references.

use rustc_hash::FxHashSet;

use crate::resolve::{
    ResolvedModule, extract_package_name, inline_loader_names, is_bare_specifier,
};

/// Collect the package names of the inline loaders in every import source.
///
/// A loader that is a path to a local file has no package, so it is skipped.
/// The result is sorted, so the order is deterministic.
pub(super) fn collect_inline_loader_referenced_deps(modules: &[ResolvedModule]) -> Vec<String> {
    let mut packages: FxHashSet<String> = FxHashSet::default();
    for module in modules {
        for edge in module.all_resolved_source_edges() {
            for loader in inline_loader_names(edge.source_specifier()) {
                if is_bare_specifier(loader) {
                    packages.insert(extract_package_name(loader));
                }
            }
        }
    }
    let mut packages: Vec<String> = packages.into_iter().collect();
    packages.sort_unstable();
    packages
}
