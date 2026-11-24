//! Export credit for the siblings of a re-exported React Native platform family.
//!
//! Metro resolves `./Button` to `Button.ios.tsx` on iOS and to `Button.tsx`
//! on other platforms. The resolver keeps one target per re-export and adds a
//! side-effect edge from the barrel to each other family member, so every
//! member stays reachable. Re-export propagation credits names only on the
//! resolved target. This pass copies the credit of each re-exported name from
//! the target to the same export on each sibling, so a re-export credits the
//! family as an import does.
//!
//! The pass reads the sibling edges that the resolver adds. They exist only
//! when the React Native or Expo plugin is active and the specifier does not
//! name a platform variant, so the pass needs no separate gate.

use rustc_hash::FxHashMap;

use fallow_types::discover::FileId;
use fallow_types::extract::{ExportName, ImportedName};

use super::ModuleGraph;
use super::types::{RoutedReference, SymbolReference};
use crate::resolve::{ResolvedModule, platform_family_key};

/// Which names of the target one re-export forwards to the barrel.
enum ForwardedNames {
    /// `export { a as b } from`: only the imported name `a`.
    One(String),
    /// `export * from`: every name except `default`.
    AllButDefault,
    /// `export * as ns from`: every name.
    All,
}

impl ForwardedNames {
    fn new(imported_name: &str, exported_name: &str) -> Self {
        match (imported_name, exported_name) {
            ("*", "*") => Self::AllButDefault,
            ("*", _) => Self::All,
            (name, _) => Self::One(name.to_string()),
        }
    }

    fn contains(&self, name: &ExportName) -> bool {
        match self {
            Self::One(forwarded) => name.matches_str(forwarded),
            Self::AllButDefault => *name != ExportName::Default,
            Self::All => true,
        }
    }
}

/// One family sibling that a re-export reaches through a side-effect edge.
struct SiblingCredit {
    target: FileId,
    sibling: FileId,
    names: ForwardedNames,
}

/// Whether two files belong to the same Metro platform-extension family.
fn same_platform_family(graph: &ModuleGraph, a: FileId, b: FileId) -> bool {
    let (Some(a), Some(b)) = (
        graph.modules.get(a.0 as usize),
        graph.modules.get(b.0 as usize),
    ) else {
        return false;
    };
    match (platform_family_key(&a.path), platform_family_key(&b.path)) {
        (Some(a), Some(b)) => a.parent == b.parent && a.base == b.base,
        _ => false,
    }
}

/// Collect each (target, sibling) pair that a re-export reaches. A sibling
/// edge is a side-effect import of the re-export specifier that resolves to
/// another member of the target's platform family.
fn collect_sibling_credits(
    graph: &ModuleGraph,
    module_by_id: &FxHashMap<FileId, &ResolvedModule>,
) -> Vec<SiblingCredit> {
    let mut credits = Vec::new();
    for module in module_by_id.values() {
        for re_export in &module.re_exports {
            let Some(target) = re_export.target.internal_file_id() else {
                continue;
            };
            for import in &module.resolved_imports {
                if !matches!(import.info.imported_name, ImportedName::SideEffect)
                    || import.info.source != re_export.info.source
                {
                    continue;
                }
                let Some(sibling) = import.target.internal_file_id() else {
                    continue;
                };
                if sibling == target || !same_platform_family(graph, target, sibling) {
                    continue;
                }
                credits.push(SiblingCredit {
                    target,
                    sibling,
                    names: ForwardedNames::new(
                        &re_export.info.imported_name,
                        &re_export.info.exported_name,
                    ),
                });
            }
        }
    }
    // The map iteration order is not the module order. Sort so that the
    // copied references land in a stable order.
    credits.sort_by_key(|credit| (credit.sibling.0, credit.target.0));
    credits
}

fn same_reference(a: &SymbolReference, b: &SymbolReference) -> bool {
    a.from_file == b.from_file
        && a.kind == b.kind
        && a.namespace == b.namespace
        && a.import_span == b.import_span
}

impl ModuleGraph {
    /// Copy the re-export credit of each platform family target to its
    /// siblings. Run after re-export propagation, so the target holds its
    /// final references.
    pub(super) fn credit_platform_family_re_exports(
        &mut self,
        module_by_id: &FxHashMap<FileId, &ResolvedModule>,
    ) {
        for credit in collect_sibling_credits(self, module_by_id) {
            let target = &self.modules[credit.target.0 as usize];
            let copied: Vec<(ExportName, Vec<RoutedReference>)> = target
                .exports
                .iter()
                .filter(|export| credit.names.contains(&export.name))
                .filter(|export| !export.references.is_empty())
                .map(|export| (export.name.clone(), export.routed_references().collect()))
                .collect();
            let sibling = &mut self.modules[credit.sibling.0 as usize];
            for (name, refs) in copied {
                let Some(export) = sibling.exports.iter_mut().find(|e| e.name == name) else {
                    continue;
                };
                for routed in refs {
                    if export
                        .references
                        .iter()
                        .any(|existing| same_reference(existing, &routed.reference))
                    {
                        continue;
                    }
                    export.push_reference(routed.reference, routed.path);
                }
            }
        }
    }
}
