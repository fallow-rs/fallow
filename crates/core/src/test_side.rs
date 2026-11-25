//! The files of a project that production mode leaves out.
//!
//! The `test-only-export` rule and the test importers of a production
//! `unused-export` finding use this one definition.

use std::path::PathBuf;

use fallow_config::ResolvedConfig;
use fixedbitset::FixedBitSet;
use globset::GlobSet;
use rustc_hash::FxHashSet;

use crate::discover::{FileId, production_exclude_globs};
use crate::graph::{ModuleGraph, ModuleNode};

/// Decides which files of a graph without production mode are test-side.
///
/// A file is test-side when an entry point reaches it and one of these is
/// true:
///
/// - a production exclude pattern or a `production.ignore` glob matches its
///   root-relative path (test, story, fixture, bench and config files);
/// - no runtime entry point reaches it, as for a test helper below `src/`
///   that only tests import, or a file that only a support entry seeds.
///
/// An unreachable file is never test-side: an unused test helper is not an
/// importer.
///
/// [`Self::with_support_reach`] narrows the second rule to the files that
/// no support entry reaches either. Production mode analyzes most files that
/// a support entry reaches, for example a source file that a bundler config
/// names, so the `test-only-export` rule treats them as production code.
pub struct TestSideMatcher {
    root: PathBuf,
    excludes: Option<GlobSet>,
    support_reachable: Option<FixedBitSet>,
}

impl TestSideMatcher {
    /// Build the matcher for the project of `config`, in any mode. A file
    /// that only a support entry reaches is test-side.
    ///
    /// The test importers of a production finding use this rule. The
    /// export of such a finding is unused in production mode, so a file that
    /// production mode analyzes cannot import it.
    #[must_use]
    pub fn new(config: &ResolvedConfig) -> Self {
        Self {
            root: config.root.clone(),
            excludes: production_exclude_globs(config),
            support_reachable: None,
        }
    }

    /// Build the matcher for `graph`, where a file that a support entry
    /// reaches is not test-side unless a production exclude glob matches it.
    #[must_use]
    pub fn with_support_reach(config: &ResolvedConfig, graph: &ModuleGraph) -> Self {
        Self {
            support_reachable: Some(graph.support_reachable()),
            ..Self::new(config)
        }
    }

    /// Whether `node` is a test-side file. See [`TestSideMatcher`].
    #[must_use]
    pub fn is_test_side(&self, node: &ModuleNode) -> bool {
        if !node.is_reachable() {
            return false;
        }
        let support_reached = self
            .support_reachable
            .as_ref()
            .is_some_and(|reach| reach.contains(node.file_id.0 as usize));
        if !node.is_runtime_reachable() && !support_reached {
            return true;
        }
        let relative = node.path.strip_prefix(&self.root).unwrap_or(&node.path);
        self.excludes
            .as_ref()
            .is_some_and(|excludes| excludes.is_match(relative))
    }
}

/// The test-side files of `graph` for the `test-only-export` rule. See
/// [`TestSideMatcher::with_support_reach`].
pub(crate) fn test_side_files(graph: &ModuleGraph, config: &ResolvedConfig) -> FxHashSet<FileId> {
    let matcher = TestSideMatcher::with_support_reach(config, graph);
    graph
        .modules()
        .iter()
        .filter(|module| matcher.is_test_side(module))
        .map(|module| module.file_id)
        .collect()
}
