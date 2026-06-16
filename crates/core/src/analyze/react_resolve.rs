//! Shared React/Preact child-component resolver.
//!
//! Lifted (rule-of-three) from the per-analyzer copies in `prop_drilling.rs`,
//! `thin_wrapper.rs`, and the new `render_fan_in.rs`. Resolves a rendered child
//! component NAME (as written in a parent file) to the [`CompKey`] of its
//! defining component, reusing the same-file / named-import / default-import
//! resolution shape all three consumers need.
//!
//! Pure static analysis (ADR-001): no type resolution. The constructor builds
//! `components_per_file` / `sole_component` directly from `graph.modules` plus
//! `ModuleInfo.component_functions`, so it carries NO dependency on any
//! analyzer-specific state map. This is the strictly more reusable shape (the
//! render-fan-in computation only has the graph + modules + resolved modules).
//!
//! Resolution behavior is reused VERBATIM, so a member-expression tag
//! (`Foo.Bar`), a spread-only / dynamic child the extractor never named, or an
//! unresolved import all fall through to `None`. For the render-fan-in metric
//! that means such render sites are UNDERCOUNTED (never credited), the safe
//! direction: a true high-fan-in component can only be undersold, never falsely
//! flagged.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::resolve::{ResolveResult, ResolvedModule};

/// A component key: the file it lives in plus its name (a file can declare
/// several components).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct CompKey {
    pub file: FileId,
    pub name: String,
}

/// Resolves a rendered child component NAME to its defining component, and
/// answers whether a name is a resolved imported binding. Same-file components
/// win; otherwise the resolved import map maps the local name to a target file,
/// and a component in that file with the imported name (or the sole component
/// for a default import) is the target. An ambiguous or unresolvable name yields
/// `None` (abstain).
pub(super) struct ChildResolver<'a> {
    /// Per-file set of component names that exist in that file.
    components_per_file: FxHashMap<FileId, FxHashSet<&'a str>>,
    /// Per-file resolved import map: local binding name -> resolved target file.
    import_targets: FxHashMap<FileId, FxHashMap<&'a str, FileId>>,
    /// The sole component name of a file that declares exactly one (resolves a
    /// default-imported component).
    sole_component: FxHashMap<FileId, &'a str>,
}

impl<'a> ChildResolver<'a> {
    /// Build the resolver over every reachable React module. The
    /// `components_per_file` / `sole_component` maps come straight from
    /// `graph.modules` + `ModuleInfo.component_functions`, so the resolver has no
    /// dependency on any analyzer-specific per-component state.
    pub(super) fn new(
        graph: &'a ModuleGraph,
        modules_by_id: &FxHashMap<FileId, &'a ModuleInfo>,
        resolved_by_id: &FxHashMap<FileId, &'a ResolvedModule>,
    ) -> Self {
        let mut components_per_file: FxHashMap<FileId, FxHashSet<&'a str>> = FxHashMap::default();
        let mut sole_component: FxHashMap<FileId, &'a str> = FxHashMap::default();
        for node in &graph.modules {
            let Some(module) = modules_by_id.get(&node.file_id) else {
                continue;
            };
            if module.component_functions.is_empty() {
                continue;
            }
            let set: FxHashSet<&'a str> = module
                .component_functions
                .iter()
                .map(|c| c.name.as_str())
                .collect();
            if module.component_functions.len() == 1 {
                sole_component.insert(node.file_id, module.component_functions[0].name.as_str());
            }
            components_per_file.insert(node.file_id, set);
        }

        let mut import_targets: FxHashMap<FileId, FxHashMap<&'a str, FileId>> =
            FxHashMap::default();
        for (file, resolved) in resolved_by_id {
            let mut map: FxHashMap<&'a str, FileId> = FxHashMap::default();
            for import in &resolved.resolved_imports {
                if let ResolveResult::InternalModule(target)
                | ResolveResult::InternalPackageModule {
                    file_id: target, ..
                } = &import.target
                {
                    let local = import.info.local_name.as_str();
                    if !local.is_empty() {
                        map.insert(local, *target);
                    }
                }
            }
            import_targets.insert(*file, map);
        }

        Self {
            components_per_file,
            import_targets,
            sole_component,
        }
    }

    /// Resolve a child component rendered in `parent_file` to its defining
    /// component key. Returns `None` (abstain) for a member-expression tag
    /// (`Foo.Bar`, namespace / compound-component indirection), a name that is
    /// neither same-file nor a resolved import, or an import whose target file
    /// has no matching component.
    pub(super) fn resolve(&self, parent_file: FileId, child_name: &str) -> Option<CompKey> {
        // Member-expression tags (`Foo.Bar`) are compound-component indirection;
        // abstain (the dotted form never matches a plain component key).
        if child_name.contains('.') {
            return None;
        }
        // Same-file component wins.
        if self
            .components_per_file
            .get(&parent_file)
            .is_some_and(|set| set.contains(child_name))
        {
            return Some(CompKey {
                file: parent_file,
                name: child_name.to_string(),
            });
        }
        // Cross-file: resolve the local name to a target file via the import map.
        let target = *self.import_targets.get(&parent_file)?.get(child_name)?;
        // A named import: the target file declares a component with this name.
        if self
            .components_per_file
            .get(&target)
            .is_some_and(|set| set.contains(child_name))
        {
            return Some(CompKey {
                file: target,
                name: child_name.to_string(),
            });
        }
        // A default import (the local name differs from the export): the target
        // file must declare exactly one component, which the import names.
        if let Some(&sole) = self.sole_component.get(&target) {
            return Some(CompKey {
                file: target,
                name: sole.to_string(),
            });
        }
        None
    }

    /// Whether `child_name` is a resolved internal import binding in
    /// `parent_file`, even if its target file declares no inspectable component
    /// (so a wrapper that re-points at an imported component the analyzer cannot
    /// see as a `ComponentFunction` still counts as forwarding to a real
    /// binding). Member-expression names never match.
    pub(super) fn is_imported_binding(&self, parent_file: FileId, child_name: &str) -> bool {
        if child_name.contains('.') {
            return false;
        }
        self.import_targets
            .get(&parent_file)
            .is_some_and(|map| map.contains_key(child_name))
    }
}
