//! Namespace bindings that hide exports from the unused-export check
//! (issue #5054).
//!
//! The graph records each namespace binding that gets the full export credit
//! of its target ([`NamespaceEscape`]). This pass counts, for each binding,
//! the exports that would be unused-export or unused-type findings without
//! that credit, with every other rule of the unused-export pass unchanged.
//! A binding that hides no export is not a candidate.
//!
//! An export counts as hidden when each of its references comes from the
//! escape credit: a namespace reference from an escape site for a name that
//! the site does not also read by name, or a re-export reference from a
//! barrel inside the escape closure whose own export of that name is hidden
//! (or that only exposes the name through `export *`).

use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::FileId;
use crate::graph::types::{ReferenceKind, SymbolReference};
use crate::graph::{ExportSymbol, ModuleGraph, NamespaceEscape, NamespaceEscapeReason};

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
    let escapes: Vec<&NamespaceEscape> = graph
        .namespace_escapes
        .iter()
        .filter(|escape| is_reachable(graph, escape.consumer))
        .collect();
    if escapes.is_empty() {
        return Vec::new();
    }

    let mut lineage = EscapeLineage::new(graph, &escapes);
    let credited = lineage.credited_exports();
    let hidden_by_module: FxHashMap<FileId, u32> = with_unused_export_context(input, true, |ctx| {
        credited
            .iter()
            .filter_map(|(&file_id, only)| {
                let module = graph.modules.get(file_id.0 as usize)?;
                let (exports, types, _) = find_unused_exports_for_module(module, ctx, Some(only));
                let hidden = u32::try_from(exports.len() + types.len()).unwrap_or(u32::MAX);
                (hidden > 0).then_some((file_id, hidden))
            })
            .collect()
    });

    escapes
        .into_iter()
        .filter_map(|escape| {
            let hidden_exports = closure_of(graph, escape.target)
                .iter()
                .filter_map(|file_id| hidden_by_module.get(file_id))
                .fold(0u32, |sum, hidden| sum.saturating_add(*hidden));
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

fn is_reachable(graph: &ModuleGraph, file_id: FileId) -> bool {
    graph
        .modules
        .get(file_id.0 as usize)
        .is_some_and(crate::graph::ModuleNode::is_reachable)
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
        if let Some(module) = graph.modules.get(file_id.0 as usize) {
            stack.extend(module.re_exports.iter().map(|edge| edge.source_file));
        }
    }
    order
}

/// Decides which exports owe every reference to the escape credit.
struct EscapeLineage<'a> {
    graph: &'a ModuleGraph,
    /// The escape sites, keyed by consumer and import span, with the members
    /// that each site also reads by name.
    sites: FxHashMap<(FileId, u32, u32), FxHashSet<&'a str>>,
    /// Every module in the re-export closure of an escape target.
    closure: FxHashSet<FileId>,
    memo: FxHashMap<(FileId, usize), bool>,
    on_stack: FxHashSet<(FileId, usize)>,
}

impl<'a> EscapeLineage<'a> {
    fn new(graph: &'a ModuleGraph, escapes: &[&'a NamespaceEscape]) -> Self {
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
            sites,
            closure,
            memo: FxHashMap::default(),
            on_stack: FxHashSet::default(),
        }
    }

    /// The escape-credited export indexes of each module in the closure.
    fn credited_exports(&mut self) -> FxHashMap<FileId, FxHashSet<usize>> {
        let mut files: Vec<FileId> = self.closure.iter().copied().collect();
        files.sort_unstable_by_key(|file_id| file_id.0);
        let mut credited: FxHashMap<FileId, FxHashSet<usize>> = FxHashMap::default();
        for file_id in files {
            let Some(module) = self.graph.modules.get(file_id.0 as usize) else {
                continue;
            };
            for index in 0..module.exports.len() {
                if self.is_escape_credited(file_id, index) {
                    credited.entry(file_id).or_default().insert(index);
                }
            }
        }
        credited
    }

    fn is_escape_credited(&mut self, file_id: FileId, index: usize) -> bool {
        let key = (file_id, index);
        if let Some(&known) = self.memo.get(&key) {
            return known;
        }
        // A re-export cycle gives no escape credit: a reference that only
        // loops back cannot show that the escape is its only source.
        if !self.on_stack.insert(key) {
            return false;
        }
        let graph = self.graph;
        let credited = graph
            .modules
            .get(file_id.0 as usize)
            .and_then(|module| module.exports.get(index))
            .is_some_and(|export| {
                !export.references.is_empty()
                    && export
                        .references
                        .iter()
                        .all(|reference| self.is_escape_reference(export, reference))
            });
        self.on_stack.remove(&key);
        self.memo.insert(key, credited);
        credited
    }

    fn is_escape_reference(&mut self, export: &ExportSymbol, reference: &SymbolReference) -> bool {
        let name = export.name.to_string();
        match reference.kind {
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
                let barrel_export = self
                    .graph
                    .modules
                    .get(barrel.0 as usize)
                    .and_then(|module| {
                        module.exports.iter().position(|candidate| {
                            candidate.is_type_only == export.is_type_only
                                && candidate.name.to_string() == name
                        })
                    });
                barrel_export.is_none_or(|index| self.is_escape_credited(barrel, index))
            }
            _ => false,
        }
    }
}
