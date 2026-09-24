//! Synthetic graph edges for convention auto-imports.
//!
//! A framework such as Nuxt makes a file's exports available by name with no
//! `import` statement. The plugin run turns the convention directories into
//! [`AutoImportRule`]s, and this pass matches the names each module references
//! against those rules. A match adds a [`ResolvedImport`] with a
//! [`ResolveResult::SyntheticAutoImport`] target, so the graph builder credits
//! the edge like any other import.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{AutoImportKind, AutoImportRule};
use fallow_types::discover::FileId;
use fallow_types::extract::{ImportedName, ModuleInfo, ReExportInfo};

use super::types::{ResolveResult, ResolvedImport, ResolvedModule};
use super::{is_auto_import_builtin, synthetic_auto_import_info};

/// Framework modules that re-export a convention auto-import surface.
///
/// Nuxt generates `#components` and `#imports` at build time, and importing a
/// name from one is the explicit spelling of the bare reference the scanners
/// already credit (issue #2737).
const AUTO_IMPORT_VIRTUAL_MODULES: &[&str] = &[COMPONENTS_MODULE, IMPORTS_MODULE];
const COMPONENTS_MODULE: &str = "#components";
const IMPORTS_MODULE: &str = "#imports";

/// One file that reads an auto-import virtual module in a way the graph cannot
/// follow to single names, such as a spread of a `#components` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnreadableAutoImportRead {
    /// The file that holds the read.
    pub file_id: FileId,
    /// The virtual module it reads (`#components` or `#imports`).
    pub module: &'static str,
}

/// One provider of an auto-import name.
struct AutoImportTarget<'a> {
    file_id: FileId,
    kind: AutoImportKind,
    /// The roots whose files see the name, merged over every rule for this
    /// file and kind. `None` means every file.
    scope: Option<Vec<&'a Path>>,
}

type AutoImportTable<'a> = FxHashMap<&'a str, Vec<AutoImportTarget<'a>>>;

/// What one module reads from the auto-import virtual modules.
#[derive(Default)]
struct VirtualModuleReads<'a> {
    /// Names read one by one, from either module.
    names: Vec<&'a str>,
    /// Modules read in a way the graph cannot narrow to names, with the file
    /// that holds each read.
    unreadable: Vec<UnreadableAutoImportRead>,
}

/// Synthesize module-graph edges for convention auto-imports.
///
/// For each module, every captured `auto_import_candidates` name is matched
/// against the active plugins' auto-import table; on a hit a synthetic
/// [`ResolvedImport`] is added so the existing graph builder credits the edge.
/// A rule credits only a module under one of the roots in its scope, so a name
/// in one app does not credit the file of the same name in a sibling app
/// (issue #2752). Inside that scope, a name collision across files credits
/// every match, which keeps each provider reachable. Resolution is recomputed
/// from the live file index each run.
///
/// A name a module reads from a framework's auto-import module
/// ([`AUTO_IMPORT_VIRTUAL_MODULES`]) is credited through the same table: the
/// module is generated at build time, so no resolver can follow the specifier,
/// while the name means exactly what the bare reference means. A read that the
/// graph cannot narrow to names credits every name of that module.
pub(super) fn synthesize_auto_import_edges(
    resolved: &mut [ResolvedModule],
    modules: &[ModuleInfo],
    auto_imports: &[AutoImportRule],
    path_to_id: &FxHashMap<&Path, FileId>,
    raw_path_to_id: &FxHashMap<&Path, FileId>,
) {
    if auto_imports.is_empty() {
        return;
    }

    let mut table: AutoImportTable<'_> = FxHashMap::default();
    for rule in auto_imports {
        let source = rule.source.as_path();
        let Some(file_id) = raw_path_to_id
            .get(source)
            .or_else(|| path_to_id.get(source))
            .copied()
        else {
            continue;
        };
        add_table_target(&mut table, rule, file_id);
    }
    if table.is_empty() {
        return;
    }

    let virtual_reads: Vec<(usize, Vec<String>, Vec<&'static str>)> =
        collect_virtual_module_reads(resolved)
            .into_iter()
            .filter_map(|(index, reads)| {
                let names: Vec<String> = reads.names.into_iter().map(str::to_owned).collect();
                let mut modules: Vec<&'static str> =
                    reads.unreadable.iter().map(|read| read.module).collect();
                modules.sort_unstable();
                modules.dedup();
                (!names.is_empty() || !modules.is_empty()).then_some((index, names, modules))
            })
            .collect();
    let mut sorted_names: Vec<&str> = table.keys().copied().collect();
    sorted_names.sort_unstable();

    let candidates: FxHashMap<FileId, &[String]> = modules
        .iter()
        .filter(|module| !module.auto_import_candidates.is_empty())
        .map(|module| (module.file_id, module.auto_import_candidates.as_slice()))
        .collect();
    for module in resolved.iter_mut() {
        if let Some(names) = candidates.get(&module.file_id) {
            for name in *names {
                credit_auto_import_name(module, name, &table, None);
            }
        }
    }
    for (index, names, modules) in virtual_reads {
        let module = &mut resolved[index];
        for name in &names {
            credit_auto_import_name(module, name, &table, None);
        }
        for virtual_module in modules {
            for name in &sorted_names {
                credit_auto_import_name(module, name, &table, Some(virtual_module));
            }
        }
    }
}

/// Add one rule to the table. Rules for the same file and kind share one
/// target, whose scope is the union of theirs, so a name earns one edge per
/// provider however many roots declared it.
fn add_table_target<'a>(
    table: &mut AutoImportTable<'a>,
    rule: &'a AutoImportRule,
    file_id: FileId,
) {
    let targets = table.entry(rule.name.as_str()).or_default();
    let rule_scope: Option<Vec<&Path>> =
        (!rule.scope.is_empty()).then(|| rule.scope.iter().map(PathBuf::as_path).collect());
    let Some(target) = targets
        .iter_mut()
        .find(|target| target.file_id == file_id && target.kind == rule.kind)
    else {
        targets.push(AutoImportTarget {
            file_id,
            kind: rule.kind,
            scope: rule_scope,
        });
        return;
    };
    match (&mut target.scope, rule_scope) {
        (Some(scope), Some(extra)) => {
            for root in extra {
                if !scope.contains(&root) {
                    scope.push(root);
                }
            }
        }
        (scope, _) => *scope = None,
    }
}

/// The reads of an auto-import virtual module that the graph cannot narrow to
/// single names, in file order. Each one credits every name of its module and
/// earns the file a `plugin-effect-not-modeled` diagnostic.
#[must_use]
pub fn unreadable_auto_import_reads(resolved: &[ResolvedModule]) -> Vec<UnreadableAutoImportRead> {
    let mut reads: Vec<UnreadableAutoImportRead> = collect_virtual_module_reads(resolved)
        .into_iter()
        .flat_map(|(_, reads)| reads.unreadable)
        .collect();
    reads.sort_unstable_by(|a, b| a.file_id.0.cmp(&b.file_id.0).then(a.module.cmp(b.module)));
    reads.dedup();
    reads
}

/// The virtual-module reads of every module that reads one, keyed by the
/// module's index in `resolved`.
fn collect_virtual_module_reads(
    resolved: &[ResolvedModule],
) -> Vec<(usize, VirtualModuleReads<'_>)> {
    let forwards_star = resolved.iter().any(|module| {
        module
            .re_exports
            .iter()
            .any(|re| is_star_from_virtual_module(&re.info))
    });
    let importers = forwards_star.then(|| ImporterIndex::build(resolved));

    resolved
        .iter()
        .enumerate()
        .filter_map(|(index, module)| {
            let reads = virtual_module_reads(module, resolved, importers.as_ref());
            (!reads.names.is_empty() || !reads.unreadable.is_empty()).then_some((index, reads))
        })
        .collect()
}

/// The names one module reads from an auto-import virtual module: a named
/// import or re-export, a member access on a namespace import, and, for a
/// module that holds `export * from '#components'`, the names its importers
/// take from it.
fn virtual_module_reads<'a>(
    module: &'a ResolvedModule,
    resolved: &'a [ResolvedModule],
    importers: Option<&ImporterIndex>,
) -> VirtualModuleReads<'a> {
    let mut reads = VirtualModuleReads::default();
    for import in &module.resolved_imports {
        let Some(virtual_module) = auto_import_virtual_module(&import.info.source) else {
            continue;
        };
        match &import.info.imported_name {
            ImportedName::Named(name) => reads.names.push(name.as_str()),
            ImportedName::Namespace => {
                read_namespace(module, &import.info.local_name, virtual_module, &mut reads);
            }
            ImportedName::Default | ImportedName::SideEffect => {}
        }
    }
    for re_export in &module.re_exports {
        let Some(virtual_module) = auto_import_virtual_module(&re_export.info.source) else {
            continue;
        };
        if re_export.info.imported_name != "*" {
            reads.names.push(re_export.info.imported_name.as_str());
        } else if re_export.info.exported_name == "*" {
            if let Some(importers) = importers {
                let mut visited = FxHashSet::default();
                forwarded_reads(
                    module,
                    virtual_module,
                    resolved,
                    importers,
                    &mut visited,
                    &mut reads,
                );
            }
        } else {
            reads.unreadable.push(UnreadableAutoImportRead {
                file_id: module.file_id,
                module: virtual_module,
            });
        }
    }
    reads
}

/// Record what a module reads through the namespace binding `local`: each
/// member access, or the whole module when the binding escapes as an object.
fn read_namespace<'a>(
    module: &'a ResolvedModule,
    local: &str,
    virtual_module: &'static str,
    reads: &mut VirtualModuleReads<'a>,
) {
    if module.unused_import_bindings.contains(local) {
        return;
    }
    let mut accessed = false;
    for access in module.member_accesses.iter() {
        if access.object == local {
            reads.names.push(access.member.as_str());
            accessed = true;
        }
    }
    let whole = module.whole_object_uses.iter().any(|name| name == local)
        || module
            .exports
            .iter()
            .any(|export| export.local_name.as_deref() == Some(local));
    if whole || !accessed {
        reads.unreadable.push(UnreadableAutoImportRead {
            file_id: module.file_id,
            module: virtual_module,
        });
    }
}

/// Record the names that the importers of `forwarder` take from it. The
/// forwarder passes these names on from `virtual_module` through a star
/// re-export, so each name is a read of that module. An importer that takes
/// the forwarder as a whole object records an unreadable read on its own file.
fn forwarded_reads<'a>(
    forwarder: &ResolvedModule,
    virtual_module: &'static str,
    resolved: &'a [ResolvedModule],
    importers: &ImporterIndex,
    visited: &mut FxHashSet<FileId>,
    reads: &mut VirtualModuleReads<'a>,
) {
    if !visited.insert(forwarder.file_id) {
        return;
    }
    for &index in importers.of(forwarder.file_id) {
        let importer = &resolved[index];
        let edges = importer
            .resolved_imports
            .iter()
            .chain(&importer.resolved_dynamic_imports)
            .filter(|import| imports_file(import, forwarder.file_id));
        for import in edges {
            match &import.info.imported_name {
                ImportedName::Named(name) => reads.names.push(name.as_str()),
                ImportedName::Namespace => {
                    let mut importer_reads = VirtualModuleReads::default();
                    read_namespace(
                        importer,
                        &import.info.local_name,
                        virtual_module,
                        &mut importer_reads,
                    );
                    reads.names.extend(importer_reads.names);
                    reads.unreadable.extend(importer_reads.unreadable);
                }
                ImportedName::Default | ImportedName::SideEffect => {}
            }
        }
        for re_export in &importer.re_exports {
            if re_export.target.internal_file_id() != Some(forwarder.file_id) {
                continue;
            }
            if re_export.info.imported_name != "*" {
                reads.names.push(re_export.info.imported_name.as_str());
            } else if re_export.info.exported_name == "*" {
                forwarded_reads(
                    importer,
                    virtual_module,
                    resolved,
                    importers,
                    visited,
                    reads,
                );
            } else {
                reads.unreadable.push(UnreadableAutoImportRead {
                    file_id: importer.file_id,
                    module: virtual_module,
                });
            }
        }
    }
}

/// Whether a real (not synthetic) import edge targets `file_id`.
fn imports_file(import: &ResolvedImport, file_id: FileId) -> bool {
    !import.target.is_synthetic_auto_import() && import.target.internal_file_id() == Some(file_id)
}

/// For each file, the indexes of the modules that import or re-export it.
struct ImporterIndex {
    importers: FxHashMap<FileId, Vec<usize>>,
}

impl ImporterIndex {
    fn build(resolved: &[ResolvedModule]) -> Self {
        let mut importers: FxHashMap<FileId, Vec<usize>> = FxHashMap::default();
        for (index, module) in resolved.iter().enumerate() {
            let targets = module
                .resolved_imports
                .iter()
                .chain(&module.resolved_dynamic_imports)
                .filter(|import| !import.target.is_synthetic_auto_import())
                .map(|import| &import.target)
                .chain(module.re_exports.iter().map(|re| &re.target))
                .filter_map(ResolveResult::internal_file_id);
            for target in targets {
                let entry = importers.entry(target).or_default();
                if entry.last() != Some(&index) {
                    entry.push(index);
                }
            }
        }
        Self { importers }
    }

    fn of(&self, file_id: FileId) -> &[usize] {
        self.importers.get(&file_id).map_or(&[], Vec::as_slice)
    }
}

/// Whether a re-export is `export * from` an auto-import virtual module.
fn is_star_from_virtual_module(info: &ReExportInfo) -> bool {
    info.imported_name == "*"
        && info.exported_name == "*"
        && auto_import_virtual_module(&info.source).is_some()
}

/// The auto-import virtual module a specifier names, if any.
fn auto_import_virtual_module(source: &str) -> Option<&'static str> {
    if !source.starts_with('#') {
        return None;
    }
    AUTO_IMPORT_VIRTUAL_MODULES
        .iter()
        .copied()
        .find(|module| *module == source)
}

/// Whether a rule kind is a name that `virtual_module` provides: components
/// come from `#components`, composables and utils from `#imports`.
fn kind_belongs_to(kind: AutoImportKind, virtual_module: &str) -> bool {
    matches!(kind, AutoImportKind::DefaultComponent) == (virtual_module == COMPONENTS_MODULE)
}

/// Whether a target scope covers `path`. `None` covers every file.
fn scope_covers(scope: Option<&[&Path]>, path: &Path) -> bool {
    scope.is_none_or(|roots| roots.iter().any(|root| path.starts_with(root)))
}

/// Add the synthetic edges one referenced name earns from the auto-import
/// table. With `only_module` set, only the rules of that virtual module count.
fn credit_auto_import_name(
    module: &mut ResolvedModule,
    name: &str,
    table: &AutoImportTable<'_>,
    only_module: Option<&str>,
) {
    if is_auto_import_builtin(name) {
        return;
    }
    let Some(targets) = table.get(name) else {
        return;
    };
    for target in targets {
        if target.file_id == module.file_id
            || !scope_covers(target.scope.as_deref(), &module.path)
            || only_module
                .is_some_and(|virtual_module| !kind_belongs_to(target.kind, virtual_module))
        {
            continue;
        }
        module.resolved_imports.push(ResolvedImport {
            info: synthetic_auto_import_info(name, target.kind),
            target: ResolveResult::SyntheticAutoImport(target.file_id),
        });
    }
}
