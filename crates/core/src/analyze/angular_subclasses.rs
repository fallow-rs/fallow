//! Cross-file `extends` index for the Angular component input and output
//! detectors.
//!
//! A base class (often an abstract `@Directive()`) can declare inputs and
//! outputs that only its subclasses read or emit, in their class bodies and
//! their templates. The per-module usage scan cannot see those uses, so the
//! detectors add the usage evidence of every subclass module, direct or
//! indirect, to the base module.
//!
//! Resolution is by import binding: the `extends` name of a subclass is
//! matched to the import with the same local name, and the class is credited
//! on the imported module and on the module that declares the export behind a
//! re-export chain. Over-crediting is the safe direction for these detectors.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{ImportedName, ModuleInfo};

use crate::discover::FileId;
use crate::graph::{ExportNamespace, ModuleGraph};

/// For each module that declares a base class, the modules that declare a
/// class extending it in another file.
pub(in crate::analyze) struct AngularSubclassIndex {
    children_by_base: FxHashMap<FileId, Vec<FileId>>,
}

impl AngularSubclassIndex {
    /// Build the direct `base file -> subclass files` edges for every module.
    pub(in crate::analyze) fn build(graph: &ModuleGraph, modules: &[ModuleInfo]) -> Self {
        let mut children_by_base: FxHashMap<FileId, Vec<FileId>> = FxHashMap::default();
        for module in modules {
            for super_name in super_class_names(module) {
                for base in base_class_files(graph, module.file_id, super_name) {
                    if base == module.file_id {
                        continue;
                    }
                    let children = children_by_base.entry(base).or_default();
                    if !children.contains(&module.file_id) {
                        children.push(module.file_id);
                    }
                }
            }
        }
        Self { children_by_base }
    }

    /// Every module that extends a class of `base`, directly or through an
    /// intermediate subclass, in breadth-first order. `base` itself is not
    /// part of the result.
    pub(in crate::analyze) fn descendants(&self, base: FileId) -> Vec<FileId> {
        let mut seen: FxHashSet<FileId> = FxHashSet::default();
        seen.insert(base);
        let mut out = Vec::new();
        let mut cursor = 0;
        let mut frontier = vec![base];
        while cursor < frontier.len() {
            let current = frontier[cursor];
            cursor += 1;
            let Some(children) = self.children_by_base.get(&current) else {
                continue;
            };
            for &child in children {
                if seen.insert(child) {
                    out.push(child);
                    frontier.push(child);
                }
            }
        }
        out
    }
}

/// The `extends` names of the classes in `module`. The folded outer target of
/// an unexported local intermediate base counts too.
fn super_class_names(module: &ModuleInfo) -> FxHashSet<&str> {
    let mut names: FxHashSet<&str> = FxHashSet::default();
    for export in module.exports.iter() {
        if let Some(name) = export.super_class.as_deref() {
            names.insert(name);
        }
    }
    for heritage in &module.class_heritage {
        if let Some(name) = heritage.super_class.as_deref() {
            names.insert(name);
        }
        if let Some((name, _)) = &heritage.binding_super_class {
            names.insert(name.as_str());
        }
    }
    names
}

/// The modules that can declare the class that `child` names as `super_name`:
/// the imported module and, behind a re-export chain, the declaring module.
fn base_class_files(graph: &ModuleGraph, child: FileId, super_name: &str) -> Vec<FileId> {
    let (local, member) = match super_name.split_once('.') {
        Some((local, member)) => (local, Some(member)),
        None => (super_name, None),
    };
    let mut out = Vec::new();
    for (target, symbols) in graph.outgoing_symbol_edges(child) {
        for symbol in symbols {
            if symbol.local_name != local {
                continue;
            }
            let export_name = match (&symbol.imported_name, member) {
                (ImportedName::Named(name), None) => name.as_str(),
                (ImportedName::Default, None) => "default",
                (ImportedName::Namespace, Some(member)) => member,
                _ => continue,
            };
            out.push(target);
            if let Some(origin) =
                graph.resolve_export_origin(target, export_name, ExportNamespace::Value)
            {
                out.push(origin.file_id());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendants_are_transitive_and_stop_on_a_cycle() {
        let mut children_by_base: FxHashMap<FileId, Vec<FileId>> = FxHashMap::default();
        children_by_base.insert(FileId(0), vec![FileId(1)]);
        children_by_base.insert(FileId(1), vec![FileId(2), FileId(0)]);
        children_by_base.insert(FileId(2), vec![FileId(1)]);
        let index = AngularSubclassIndex { children_by_base };
        assert_eq!(index.descendants(FileId(0)), vec![FileId(1), FileId(2)]);
        assert_eq!(index.descendants(FileId(3)), Vec::<FileId>::new());
    }
}
