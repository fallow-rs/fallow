//! The imports of the files that production mode leaves out (issue #5251).
//!
//! A production run excludes test, story, fixture and bench files, the
//! `production.ignore` globs, and the files that only a support entry seeds.
//! Its graph has no edge from these files, so an export that only they import
//! reads as unused. The scan builds the graph of the same project without
//! production mode, and reads the references of the asked exports from the
//! test-side files of that graph. [`TestSideMatcher`] decides which files are
//! test-side.
//!
//! The scan keeps its persisted caches in a subfolder of the cache directory.
//! The graph cache holds one graph, so with a shared folder each production
//! run would replace the graph of the other mode.

use std::path::{Path, PathBuf};

use fallow_config::ResolvedConfig;
use fallow_graph::graph::{ExportNamespace, ModuleGraph, ModuleNode};
use fallow_types::output_dead_code::TestImporters;
use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashMap;

use crate::EngineResult;
use crate::core_backend::TestSideMatcher;
use crate::session::AnalysisSession;
use crate::thread_pool::{WORKER_STACK_SIZE, register_thread_stack};

/// The subfolder of the cache directory that the scan uses.
const SCAN_CACHE_DIR: &str = "test-side-scan";

/// One test-side file that references an export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestSideImporter {
    /// Absolute path of the referencing file.
    pub path: PathBuf,
    /// True when the file uses the export as a value, false for a type-only
    /// use.
    pub value: bool,
}

/// The test-side importers of the exports that a scan was asked about.
#[derive(Debug, Default)]
pub struct TestSideImports {
    importers: FxHashMap<(PathBuf, String), Vec<TestSideImporter>>,
}

impl TestSideImports {
    /// The importers of `export_name` in `path`, sorted by path. Empty when no
    /// test-side file references the export, or when the scan was not asked
    /// about it.
    #[must_use]
    pub fn importers(&self, path: &Path, export_name: &str) -> &[TestSideImporter] {
        self.importers
            .get(&(path.to_path_buf(), export_name.to_string()))
            .map_or(&[], Vec::as_slice)
    }
}

/// Find the test-side importers of each export in `targets`, in a graph of
/// the project of `config` without production mode. Each target is the
/// absolute path of the declaring file and the export name.
///
/// # Errors
///
/// Returns an error when the scan analysis fails.
pub fn scan(
    config: &ResolvedConfig,
    targets: &[(PathBuf, String)],
) -> EngineResult<TestSideImports> {
    if targets.is_empty() {
        return Ok(TestSideImports::default());
    }
    // The scan is a second analysis of the same root. Its diagnostics, and
    // its warnings on stderr, are about the non-production view, so they must
    // not reach the report of the production run. Both guards are per thread,
    // and a scoped thread runs no other work. A worker of the thread pool
    // could pick up a task of a parallel analysis, such as the duplication
    // pass of a combined run, while it waits. The thread gets the stack size
    // of a pool worker, because the analysis recurses on deep inputs.
    std::thread::scope(|scope| {
        let spawned = std::thread::Builder::new()
            .name("fallow-test-side-scan".to_string())
            .stack_size(WORKER_STACK_SIZE)
            .spawn_scoped(scope, || {
                register_thread_stack(WORKER_STACK_SIZE);
                fallow_types::diagnostic_scope::with_scratch_diagnostics(|| {
                    tracing::subscriber::with_default(
                        tracing::subscriber::NoSubscriber::default(),
                        || scan_on_this_thread(config, targets),
                    )
                })
            });
        let handle = spawned.map_err(|error| {
            crate::EngineError::new(format!(
                "failed to start the test-side import scan: {error}"
            ))
        })?;
        handle
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

fn scan_on_this_thread(
    config: &ResolvedConfig,
    targets: &[(PathBuf, String)],
) -> EngineResult<TestSideImports> {
    let mut scan_config = config.clone();
    scan_config.production = false;
    scan_config.quiet = true;
    scan_config.cache_dir = config.cache_dir.join(SCAN_CACHE_DIR);
    let session = AnalysisSession::from_resolved_config(scan_config)?;
    let graph = session.build_module_graph()?;
    Ok(collect_importers(
        graph.as_graph(),
        &TestSideMatcher::new(config),
        targets,
    ))
}

/// Read the test-side references of each target export from `graph`, a
/// graph that has the test edges.
#[must_use]
fn collect_importers(
    graph: &ModuleGraph,
    matcher: &TestSideMatcher,
    targets: &[(PathBuf, String)],
) -> TestSideImports {
    let modules: FxHashMap<&Path, &ModuleNode> = graph
        .modules()
        .iter()
        .map(|module| (module.path.as_path(), module))
        .collect();
    let mut importers: FxHashMap<(PathBuf, String), Vec<TestSideImporter>> = FxHashMap::default();
    for (path, export_name) in targets {
        let Some(module) = modules.get(path.as_path()) else {
            continue;
        };
        let Some(export) = module
            .exports
            .iter()
            .find(|export| export.name.matches_str(export_name))
        else {
            continue;
        };
        let mut found: FxHashMap<&Path, bool> = FxHashMap::default();
        for reference in &export.references {
            let Some(from) = graph
                .module(reference.from_file)
                .filter(|node| matcher.is_test_side(node))
            else {
                continue;
            };
            *found.entry(from.path.as_path()).or_default() |=
                reference.namespace == ExportNamespace::Value;
        }
        if found.is_empty() {
            continue;
        }
        let mut list: Vec<TestSideImporter> = found
            .into_iter()
            .map(|(path, value)| TestSideImporter {
                path: path.to_path_buf(),
                value,
            })
            .collect();
        list.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        importers.insert((path.clone(), export_name.clone()), list);
    }
    TestSideImports { importers }
}

/// Set `test_importers` on each unused-export and unused-type finding of a
/// production run that a test-side file imports. Does nothing outside
/// production mode or when the run has no such finding.
///
/// # Errors
///
/// Returns an error when the scan analysis fails.
pub(crate) fn attach_test_importers(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
) -> EngineResult<()> {
    if !config.production || (results.unused_exports.is_empty() && results.unused_types.is_empty())
    {
        return Ok(());
    }
    let targets: Vec<(PathBuf, String)> = results
        .unused_exports
        .iter()
        .map(|finding| &finding.export)
        .chain(results.unused_types.iter().map(|finding| &finding.export))
        .map(|export| (export.path.clone(), export.export_name.clone()))
        .collect();
    let imports = scan(config, &targets)?;
    let importers_of = |export: &fallow_types::results::UnusedExport| {
        TestImporters::from_files(
            imports
                .importers(&export.path, &export.export_name)
                .iter()
                .map(|importer| importer.path.clone())
                .collect(),
        )
    };
    for finding in &mut results.unused_exports {
        finding.test_importers = importers_of(&finding.export);
    }
    for finding in &mut results.unused_types {
        finding.test_importers = importers_of(&finding.export);
    }
    Ok(())
}
