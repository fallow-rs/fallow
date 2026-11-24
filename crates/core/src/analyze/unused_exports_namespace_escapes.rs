//! Namespace bindings that hide exports from the unused-export check
//! (issue #5054).
//!
//! The graph records each namespace binding that gets the full export credit
//! of its target ([`NamespaceEscape`]). This pass counts, for each binding,
//! the exports that would be unused-export or unused-type findings without
//! that credit, with every other rule of the unused-export pass unchanged.
//! A binding that hides no export is not a candidate.
//!
//! An export is escape-credited when each of its references comes from an
//! escape credit: a namespace reference from an escape site for a name that
//! the site does not also read by name, or a reference from a barrel inside
//! the escape closure whose own export of that name is escape-credited (or
//! that only exposes the name through `export *` or `export * as`).
//!
//! An escape-credited export is hidden when the unused-export pass reports
//! it without its references and does not report it with them. Each binding
//! counts the hidden exports that have at least one reference from its own
//! credit.

use fallow_config::Severity;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::FileId;
use crate::graph::types::{ReferenceKind, SymbolReference};
use fallow_types::extract::ImportedName;

use crate::graph::{ExportSymbol, ModuleGraph, ModuleNode, NamespaceEscape, NamespaceEscapeReason};
use rayon::prelude::*;

use super::{UnusedExportInput, find_unused_exports_for_module, with_unused_export_context};

/// A namespace binding that hides at least one export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceEscapeCandidate {
    /// The file that declares the binding.
    pub consumer: FileId,
    /// Byte offset of the import that declares the binding.
    pub import_start: u32,
    /// The local name of the binding.
    pub local_name: String,
    /// The module whose exports get the full credit.
    pub target: FileId,
    /// Why the binding gets the full credit.
    pub reason: NamespaceEscapeReason,
    /// The count of exports in the target and its re-export closure that
    /// would be findings without the escape credit.
    pub hidden_exports: u32,
}

/// Find the namespace bindings that hide at least one export.
pub fn find_namespace_escape_candidates(
    input: UnusedExportInput<'_>,
) -> Vec<NamespaceEscapeCandidate> {
    let graph = input.graph;
    let config = input.config;
    let escapes: Vec<&NamespaceEscape> = graph
        .namespace_escapes
        .iter()
        .filter(|escape| hides_from_a_check(graph, config, escape))
        .collect();
    if escapes.is_empty() {
        return Vec::new();
    }

    let mut lineage = EscapeLineage::new(graph, config, &escapes, Quantifier::Every);
    let credited = lineage.credited_exports();
    if credited.is_empty() {
        return Vec::new();
    }
    let hidden = hidden_exports(input, &credited);
    if hidden.is_empty() {
        return Vec::new();
    }

    escapes
        .into_iter()
        .filter_map(|escape| {
            let mut own = EscapeLineage::new(graph, config, &[escape], Quantifier::Any);
            let count = hidden
                .iter()
                .filter(|&&(file_id, index)| {
                    own.closure.contains(&file_id) && own.is_credited(file_id, index)
                })
                .count();
            let hidden_exports = u32::try_from(count).unwrap_or(u32::MAX);
            (hidden_exports > 0).then(|| NamespaceEscapeCandidate {
                consumer: escape.consumer,
                import_start: escape.import_span.start,
                local_name: escape.local_name.clone(),
                target: escape.target,
                reason: escape.reason,
                hidden_exports,
            })
        })
        .collect()
}

/// Whether the full credit of `escape` can hide an export from a check.
///
/// `export { Ns }` in an entry file hands the namespace to the public API.
/// Without `includeEntryExports` for that file, the public API credit stands
/// with or without the escape, so the binding hides nothing.
fn hides_from_a_check(
    graph: &ModuleGraph,
    config: &fallow_config::ResolvedConfig,
    escape: &NamespaceEscape,
) -> bool {
    if escape.reason != NamespaceEscapeReason::ReExportedBinding {
        return true;
    }
    graph
        .modules()
        .get(escape.consumer.0 as usize)
        .is_some_and(|consumer| !is_public_entry(config, consumer))
}

/// An entry file whose exports are public API, so no consumer must use them.
fn is_public_entry(config: &fallow_config::ResolvedConfig, module: &ModuleNode) -> bool {
    module.is_entry_point()
        && !module.is_consumer_checked()
        && !config.include_entry_exports_for(&module.path)
}

/// The escape-credited exports that the unused-export pass reports without
/// their references and does not report with them, and whose rule is on for
/// their path.
fn hidden_exports(
    input: UnusedExportInput<'_>,
    credited: &FxHashMap<FileId, FxHashSet<usize>>,
) -> FxHashSet<(FileId, usize)> {
    let graph = input.graph;
    let mut files: Vec<(&FileId, &FxHashSet<usize>)> = credited.iter().collect();
    files.sort_unstable_by_key(|(file_id, _)| file_id.0);
    let reported = |assume_unreferenced: bool| -> FxHashSet<(FileId, usize)> {
        with_unused_export_context(input, assume_unreferenced, |ctx| {
            files
                .par_iter()
                .filter_map(|&(&file_id, only)| {
                    let module = graph.module(file_id)?;
                    let rules = input.config.resolve_rules_for_path(&module.path);
                    let (exports, types, _) =
                        find_unused_exports_for_module(module, ctx, Some(only));
                    let indexes: Vec<(FileId, usize)> =
                        [(rules.unused_exports, exports), (rules.unused_types, types)]
                            .into_iter()
                            .filter(|(severity, _)| *severity != Severity::Off)
                            .flat_map(|(_, findings)| findings)
                            .filter_map(|finding| {
                                export_index(
                                    module,
                                    &finding.export_name,
                                    finding.is_type_only,
                                    finding.span_start,
                                )
                                .map(|index| (file_id, index))
                            })
                            .collect();
                    Some(indexes)
                })
                .flatten()
                .collect()
        })
    };
    let with_references = reported(false);
    let mut hidden = reported(true);
    hidden.retain(|key| !with_references.contains(key));
    hidden
}

fn export_index(
    module: &ModuleNode,
    name: &str,
    is_type_only: bool,
    span_start: u32,
) -> Option<usize> {
    module.exports.iter().position(|export| {
        export.span.start == span_start
            && export.is_type_only == is_type_only
            && export.name.to_string() == name
    })
}

fn is_reachable(graph: &ModuleGraph, file_id: FileId) -> bool {
    graph
        .modules()
        .get(file_id.0 as usize)
        .is_some_and(ModuleNode::is_reachable)
}

/// The target and every module that it re-exports from, transitively.
fn closure_of(graph: &ModuleGraph, target: FileId) -> Vec<FileId> {
    let mut seen: FxHashSet<FileId> = FxHashSet::default();
    let mut stack = vec![target];
    let mut order = Vec::new();
    while let Some(file_id) = stack.pop() {
        if !seen.insert(file_id) {
            continue;
        }
        order.push(file_id);
        if let Some(module) = graph.module(file_id) {
            stack.extend(module.re_exports.iter().map(|edge| edge.source_file));
        }
    }
    order
}

/// How the references of an export must come from the escape credit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Quantifier {
    /// Every reference: no other credit keeps the export alive.
    Every,
    /// At least one reference: the credit reaches the export.
    Any,
}

/// Decides which exports get their references from the escape credit.
struct EscapeLineage<'a> {
    graph: &'a ModuleGraph,
    config: &'a fallow_config::ResolvedConfig,
    quantifier: Quantifier,
    /// The escape sites, keyed by consumer and import span, with the members
    /// that each site also reads by name.
    sites: FxHashMap<(FileId, u32, u32), FxHashSet<&'a str>>,
    /// Every module in the re-export closure of an escape target.
    closure: FxHashSet<FileId>,
    memo: FxHashMap<(FileId, usize), bool>,
    on_stack: FxHashSet<(FileId, usize)>,
    /// Set when a walk meets a node that is already on the stack. A result
    /// that depends on such a cut is not stored, so the order of the walk
    /// does not change later results.
    cut: bool,
    /// The names that importers read by name from each star barrel, or `None`
    /// when the barrel is public API that credits every name.
    star_readers: FxHashMap<FileId, Option<FxHashSet<String>>>,
}

impl<'a> EscapeLineage<'a> {
    fn new(
        graph: &'a ModuleGraph,
        config: &'a fallow_config::ResolvedConfig,
        escapes: &[&'a NamespaceEscape],
        quantifier: Quantifier,
    ) -> Self {
        let mut sites: FxHashMap<(FileId, u32, u32), FxHashSet<&'a str>> = FxHashMap::default();
        let mut closure = FxHashSet::default();
        for escape in escapes {
            sites
                .entry((
                    escape.consumer,
                    escape.import_span.start,
                    escape.import_span.end,
                ))
                .or_default()
                .extend(escape.read_members.iter().map(String::as_str));
            closure.extend(closure_of(graph, escape.target));
        }
        Self {
            graph,
            config,
            quantifier,
            sites,
            closure,
            memo: FxHashMap::default(),
            on_stack: FxHashSet::default(),
            cut: false,
            star_readers: FxHashMap::default(),
        }
    }

    /// The credited export indexes of each module in the closure.
    fn credited_exports(&mut self) -> FxHashMap<FileId, FxHashSet<usize>> {
        let mut files: Vec<FileId> = self.closure.iter().copied().collect();
        files.sort_unstable_by_key(|file_id| file_id.0);
        let mut credited: FxHashMap<FileId, FxHashSet<usize>> = FxHashMap::default();
        for file_id in files {
            let Some(module) = self.graph.module(file_id) else {
                continue;
            };
            for index in 0..module.exports.len() {
                if self.is_credited(file_id, index) {
                    credited.entry(file_id).or_default().insert(index);
                }
            }
        }
        credited
    }

    fn is_credited(&mut self, file_id: FileId, index: usize) -> bool {
        let key = (file_id, index);
        if let Some(&known) = self.memo.get(&key) {
            return known;
        }
        // A re-export cycle gives no credit: a reference that only loops back
        // cannot show where the credit comes from.
        if !self.on_stack.insert(key) {
            self.cut = true;
            return false;
        }
        let outer_cut = std::mem::take(&mut self.cut);
        let graph = self.graph;
        let quantifier = self.quantifier;
        let credited = graph
            .modules()
            .get(file_id.0 as usize)
            .and_then(|module| module.exports.get(index))
            .is_some_and(|export| {
                let mut references = export
                    .references
                    .iter()
                    .map(|reference| self.is_escape_reference(export, reference));
                match quantifier {
                    Quantifier::Every => !export.references.is_empty() && references.all(|is| is),
                    Quantifier::Any => references.any(|is| is),
                }
            });
        self.on_stack.remove(&key);
        if !self.cut {
            self.memo.insert(key, credited);
        }
        self.cut |= outer_cut;
        credited
    }

    fn is_escape_reference(&mut self, export: &ExportSymbol, reference: &SymbolReference) -> bool {
        let name = export.name.to_string();
        match reference.kind {
            ReferenceKind::NamespaceImport
                if reference.import_span == oxc_span::Span::default() =>
            {
                // A barrel in the closure hands its `export * as` namespace
                // object on as a whole. An entry file that is public API gives
                // that credit with or without the escape.
                let barrel = reference.from_file;
                self.closure.contains(&barrel)
                    && self
                        .graph
                        .modules()
                        .get(barrel.0 as usize)
                        .is_some_and(|module| !is_public_entry(self.config, module))
            }
            ReferenceKind::NamespaceImport => self
                .sites
                .get(&(
                    reference.from_file,
                    reference.import_span.start,
                    reference.import_span.end,
                ))
                .is_some_and(|read_members| !read_members.contains(name.as_str())),
            ReferenceKind::ReExport => {
                let barrel = reference.from_file;
                if !self.closure.contains(&barrel) {
                    return false;
                }
                let barrel_export =
                    self.graph
                        .modules()
                        .get(barrel.0 as usize)
                        .and_then(|module| {
                            module.exports.iter().position(|candidate| {
                                candidate.is_type_only == export.is_type_only
                                    && candidate.name.to_string() == name
                            })
                        });
                match barrel_export {
                    Some(index) => self.is_credited(barrel, index),
                    None => {
                        self.quantifier == Quantifier::Any
                            || !self.star_barrel_has_named_reader(barrel, &name)
                    }
                }
            }
            _ => false,
        }
    }

    /// Whether a consumer other than an escape reads `name` through the
    /// `export *` surface of `barrel`.
    ///
    /// When the escape opens a star barrel, star propagation credits each
    /// source export with one re-export reference from the barrel and does
    /// not copy the references of the named importers. Those importers still
    /// read the name, so the escape does not hide it.
    fn star_barrel_has_named_reader(&mut self, barrel: FileId, name: &str) -> bool {
        let mut visiting = FxHashSet::default();
        let mut cut = false;
        let readers = self.star_readers_of(barrel, &mut visiting, &mut cut);
        readers.as_ref().is_none_or(|names| names.contains(name))
    }

    fn star_readers_of(
        &mut self,
        barrel: FileId,
        visiting: &mut FxHashSet<FileId>,
        cut: &mut bool,
    ) -> Option<FxHashSet<String>> {
        if let Some(known) = self.star_readers.get(&barrel) {
            return known.clone();
        }
        if !visiting.insert(barrel) {
            *cut = true;
            return Some(FxHashSet::default());
        }
        let outer_cut = std::mem::take(cut);
        let module = self.graph.module(barrel)?;
        let readers = if is_public_entry(self.config, module) {
            None
        } else {
            self.collect_star_readers(barrel, visiting, cut)
        };
        visiting.remove(&barrel);
        if !*cut {
            self.star_readers.insert(barrel, readers.clone());
        }
        *cut |= outer_cut;
        readers
    }

    /// The names that the reachable importers of `barrel` read by name,
    /// directly or through an outer barrel that forwards it with `export *`.
    /// `None` when an outer barrel is public API.
    fn collect_star_readers(
        &mut self,
        barrel: FileId,
        visiting: &mut FxHashSet<FileId>,
        cut: &mut bool,
    ) -> Option<FxHashSet<String>> {
        let graph = self.graph;
        let mut names = FxHashSet::default();
        for &importer in graph.importers_of(barrel) {
            if !is_reachable(graph, importer) {
                continue;
            }
            names.extend(named_imports_from(graph, importer, barrel));
            if forwards_star_of(graph, importer, barrel) {
                names.extend(self.star_readers_of(importer, visiting, cut)?);
            }
        }
        Some(names)
    }
}

/// The names that `importer` imports by name from `barrel`.
fn named_imports_from(graph: &ModuleGraph, importer: FileId, barrel: FileId) -> Vec<String> {
    graph
        .outgoing_symbol_edges(importer)
        .filter(|(target, _)| *target == barrel)
        .flat_map(|(_, symbols)| symbols)
        .filter_map(|symbol| match &symbol.imported_name {
            ImportedName::Named(imported) => Some(imported.clone()),
            _ => None,
        })
        .collect()
}

/// Whether `importer` forwards every name of `barrel` with `export *`.
fn forwards_star_of(graph: &ModuleGraph, importer: FileId, barrel: FileId) -> bool {
    graph.module(importer).is_some_and(|outer| {
        outer.re_exports.iter().any(|edge| {
            edge.source_file == barrel && edge.imported_name == "*" && edge.exported_name == "*"
        })
    })
}
