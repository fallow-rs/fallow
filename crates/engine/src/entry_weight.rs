//! Startup import weight per runtime entry point.
//!
//! For each runtime entry, the report counts the project modules and source
//! bytes that load before the entry runs, and compares them with the modules
//! that load only on demand or only on another thread. It also names the
//! single imports that keep the most bytes on the startup path. The numbers
//! come from on-disk file sizes and a traversal in `FileId` order, so repeated
//! runs on the same tree give identical output.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use fallow_graph::graph::{DominatingImport, ModuleGraph, is_declaration_file_path};
use fallow_output::{
    DominatingImportOutput, EagerPackageOutput, EntryWeightListing, EntryWeightOutput,
    EntryWeightUnit,
};
use fallow_types::discover::{DiscoveredFile, EntryPoint, FileId};
use rustc_hash::FxHashMap;

use crate::session::AnalysisSession;

/// Dominating imports reported per entry.
pub const DOMINATING_IMPORT_LIMIT: usize = 10;

/// Stylesheet extensions whose bytes also count as `eager_css_bytes`.
const STYLESHEET_EXTENSIONS: &[&str] = &["css", "scss", "sass", "less"];

/// Label for an entry whose declaring source is not known.
const UNKNOWN_ENTRY_SOURCE: &str = "entry point";

/// Compute the startup import weight of every runtime entry point.
///
/// `entry_points` gives the declaring source of each entry, as the listing
/// reports it. Entries are sorted by `eager_bytes` (heaviest first), then by
/// path.
///
/// # Errors
///
/// Returns an error if parsing or graph construction fails.
pub fn compute_entry_weight(
    session: &AnalysisSession,
    entry_points: &[EntryPoint],
) -> crate::EngineResult<EntryWeightListing> {
    let artifacts = session.analyze_dead_code_with_shared_artifacts(false, true)?;
    let graph = artifacts
        .graph
        .as_ref()
        .ok_or_else(|| crate::EngineError::new("entry weight requires a retained module graph"))?;
    Ok(entry_weight_listing(
        graph.as_graph(),
        session.files(),
        entry_points,
        session.root(),
    ))
}

/// Build the listing from a module graph and the discovered files.
#[must_use]
pub fn entry_weight_listing(
    graph: &ModuleGraph,
    files: &[DiscoveredFile],
    entry_points: &[EntryPoint],
    root: &Path,
) -> EntryWeightListing {
    let sizes = FileSizes::new(files);
    let sources: FxHashMap<&Path, String> = entry_points
        .iter()
        .map(|entry| (entry.path.as_path(), entry.source.to_string()))
        .collect();
    let mut entry_ids: Vec<FileId> = graph.runtime_entry_points.iter().copied().collect();
    entry_ids.sort_unstable_by_key(|id| id.0);

    let mut line_offsets = LineOffsetCache::default();
    let mut entries: Vec<EntryWeightOutput> = entry_ids
        .into_iter()
        .filter_map(|entry| {
            let module = graph.modules.get(entry.0 as usize)?;
            // A declaration file is erased at build time, so nothing loads it.
            if is_declaration_file_path(&module.path) {
                return None;
            }
            let source = sources
                .get(module.path.as_path())
                .cloned()
                .unwrap_or_else(|| UNKNOWN_ENTRY_SOURCE.to_string());
            Some(entry_weight(
                graph,
                EntryRow {
                    entry,
                    source,
                    root,
                    sizes: &sizes,
                },
                &mut line_offsets,
            ))
        })
        .collect();
    entries.sort_by(|a, b| {
        b.eager_bytes
            .cmp(&a.eager_bytes)
            .then_with(|| a.path.cmp(&b.path))
    });

    EntryWeightListing {
        unit: EntryWeightUnit::SourceBytes,
        entry_count: entries.len(),
        entries,
        regression: None,
    }
}

struct EntryRow<'a> {
    entry: FileId,
    source: String,
    root: &'a Path,
    sizes: &'a FileSizes,
}

fn entry_weight(
    graph: &ModuleGraph,
    row: EntryRow<'_>,
    line_offsets: &mut LineOffsetCache,
) -> EntryWeightOutput {
    let EntryRow {
        entry,
        source,
        root,
        sizes,
    } = row;
    let closure = graph.entry_load_closure(entry);
    let eager_css_bytes = closure
        .eager
        .iter()
        .filter(|&&id| {
            graph
                .modules
                .get(id.0 as usize)
                .is_some_and(|m| is_stylesheet(&m.path))
        })
        .map(|&id| sizes.get(id))
        .sum();
    let eager_packages = eager_packages(graph, &closure.eager);
    let dominating_imports = graph
        .eager_dominating_imports(entry, &closure.eager, |id| sizes.get(id))
        .into_iter()
        .take(DOMINATING_IMPORT_LIMIT)
        .map(|import| dominating_import_output(graph, root, &import, line_offsets))
        .collect();

    EntryWeightOutput {
        path: relative_path(graph, entry, root),
        source,
        eager_modules: closure.eager.len(),
        eager_bytes: sizes.sum(&closure.eager),
        eager_css_bytes,
        deferred_modules: closure.deferred.len(),
        deferred_bytes: sizes.sum(&closure.deferred),
        out_of_thread_modules: closure.out_of_thread.len(),
        out_of_thread_bytes: sizes.sum(&closure.out_of_thread),
        eager_package_count: eager_packages.len(),
        eager_packages,
        dominating_imports,
    }
}

fn eager_packages(graph: &ModuleGraph, eager: &[FileId]) -> Vec<EagerPackageOutput> {
    let mut packages: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for id in eager {
        let Some(imports) = graph.eager_package_imports.get(id) else {
            continue;
        };
        for import in imports
            .iter()
            .filter(|import| !crate::core_backend::is_builtin_module(&import.package))
        {
            packages
                .entry(import.package.as_str())
                .or_default()
                .insert(import.specifier.as_str());
        }
    }
    packages
        .into_iter()
        .map(|(name, specifiers)| EagerPackageOutput {
            name: name.to_string(),
            specifiers: specifiers.into_iter().map(str::to_string).collect(),
        })
        .collect()
}

fn dominating_import_output(
    graph: &ModuleGraph,
    root: &Path,
    import: &DominatingImport,
    line_offsets: &mut LineOffsetCache,
) -> DominatingImportOutput {
    let line = import
        .import_span_start
        .and_then(|start| line_offsets.line(graph, import.importer, start));
    DominatingImportOutput {
        importer: relative_path(graph, import.importer, root),
        line,
        target: relative_path(graph, import.target, root),
        exclusive_bytes: import.exclusive_weight,
        exclusive_modules: import.exclusive_modules,
    }
}

fn relative_path(graph: &ModuleGraph, id: FileId, root: &Path) -> String {
    graph
        .modules
        .get(id.0 as usize)
        .map(|module| crate::trace::trace_impl::relativize(&module.path, root))
        .unwrap_or_default()
}

fn is_stylesheet(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            STYLESHEET_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// On-disk file sizes indexed by `FileId`.
struct FileSizes(Vec<u64>);

impl FileSizes {
    fn new(files: &[DiscoveredFile]) -> Self {
        let len = files.iter().map(|f| f.id.0 as usize + 1).max().unwrap_or(0);
        let mut sizes = vec![0; len];
        for file in files {
            sizes[file.id.0 as usize] = file.size_bytes;
        }
        Self(sizes)
    }

    fn get(&self, id: FileId) -> u64 {
        self.0.get(id.0 as usize).copied().unwrap_or(0)
    }

    fn sum(&self, ids: &[FileId]) -> u64 {
        ids.iter().map(|&id| self.get(id)).sum()
    }
}

/// Line offsets of importer files, read on first use.
#[derive(Default)]
struct LineOffsetCache(FxHashMap<FileId, Option<Vec<u32>>>);

impl LineOffsetCache {
    fn line(&mut self, graph: &ModuleGraph, file: FileId, byte_offset: u32) -> Option<u32> {
        let offsets = self.0.entry(file).or_insert_with(|| {
            let module = graph.modules.get(file.0 as usize)?;
            std::fs::read_to_string(&module.path)
                .ok()
                .map(|source| fallow_types::extract::compute_line_offsets(&source))
        });
        offsets
            .as_ref()
            .map(|offsets| fallow_types::extract::byte_offset_to_line_col(offsets, byte_offset).0)
    }
}
