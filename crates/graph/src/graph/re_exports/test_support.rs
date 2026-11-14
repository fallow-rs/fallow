//! Fixture builders shared by the re-export propagation test files.

use std::path::PathBuf;

use crate::graph::{ExportSymbol, ModuleGraph};
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use fallow_types::extract::{
    ExportInfo, ExportName, ImportInfo, ImportedName, ReExportInfo, VisibilityTag,
};
use oxc_span::Span;

/// The module that a fixture import or re-export resolves to: a file id, or
/// the module itself. A bare id can name a module that does not exist.
pub(super) trait Target: Copy {
    fn file_id(&self) -> FileId;
}

impl Target for u32 {
    fn file_id(&self) -> FileId {
        FileId(*self)
    }
}

impl Target for &ResolvedModule {
    fn file_id(&self) -> FileId {
        self.file_id
    }
}

fn source(target: FileId) -> String {
    format!("./module{}", target.0)
}

pub(super) fn module(id: u32, name: &str) -> ResolvedModule {
    ResolvedModule {
        file_id: FileId(id),
        path: PathBuf::from(format!("/project/{name}.ts")),
        ..Default::default()
    }
}

fn export_info(name: &str, is_type_only: bool) -> ExportInfo {
    ExportInfo {
        name: if name == "default" {
            ExportName::Default
        } else {
            ExportName::Named(name.to_string())
        },
        local_name: Some(name.to_string()),
        is_type_only,
        visibility: VisibilityTag::None,
        expected_unused_reason: None,
        span: Span::new(0, 20),
        members: vec![],
        is_side_effect_used: false,
        super_class: None,
        deprecated: false,
        deprecated_reason: None,
    }
}

/// A value export. The name `default` gives the default export.
pub(super) fn export(name: &str) -> ExportInfo {
    export_info(name, false)
}

pub(super) fn type_export(name: &str) -> ExportInfo {
    export_info(name, true)
}

pub(super) fn re_export(target: impl Target, imported: &str, exported: &str) -> ResolvedReExport {
    let target = target.file_id();
    ResolvedReExport {
        info: ReExportInfo {
            source: source(target),
            imported_name: imported.to_string(),
            exported_name: exported.to_string(),
            is_type_only: false,
            span: Span::default(),
            statement_span: Span::default(),
            source_span: Span::default(),
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
        },
        target: ResolveResult::InternalModule(target),
    }
}

fn import_info(
    target: FileId,
    imported_name: ImportedName,
    local_name: &str,
    is_type_only: bool,
) -> ResolvedImport {
    ResolvedImport {
        info: ImportInfo {
            source: source(target),
            imported_name,
            local_name: local_name.to_string(),
            is_type_only,
            is_type_only_star: false,
            from_style: false,
            span: Span::default(),
            source_span: Span::default(),
        },
        target: ResolveResult::InternalModule(target),
    }
}

/// A value import of `imported` under the local name `local`.
pub(super) fn import(target: impl Target, imported: ImportedName, local: &str) -> ResolvedImport {
    import_info(target.file_id(), imported, local, false)
}

/// `import { name } from '...'`.
pub(super) fn named_import(target: impl Target, name: &str) -> ResolvedImport {
    import_info(
        target.file_id(),
        ImportedName::Named(name.to_string()),
        name,
        false,
    )
}

/// `import type { name } from '...'`.
pub(super) fn type_import(target: impl Target, name: &str) -> ResolvedImport {
    import_info(
        target.file_id(),
        ImportedName::Named(name.to_string()),
        name,
        true,
    )
}

/// The imports extraction records for `export *` inside a
/// `declare module '...'` body. A type-only import with an empty local name
/// marks the ambient form. The `export * as ns` spelling adds a `Default`
/// import at the same span, which the graph reads as an observation of the
/// whole namespace object.
pub(super) fn ambient_imports(target: impl Target, namespace_object: bool) -> Vec<ResolvedImport> {
    let target = target.file_id();
    let names = if namespace_object {
        vec![ImportedName::Namespace, ImportedName::Default]
    } else {
        vec![ImportedName::Namespace]
    };
    names
        .into_iter()
        .map(|imported_name| {
            let mut import = import_info(target, imported_name, "", true);
            import.info.span = Span::new(25, 65);
            import.info.source_span = Span::new(50, 64);
            import
        })
        .collect()
}

pub(super) fn discovered_files(modules: &[ResolvedModule]) -> Vec<DiscoveredFile> {
    modules
        .iter()
        .map(|module| DiscoveredFile {
            id: module.file_id,
            path: module.path.clone(),
            size_bytes: 100,
        })
        .collect()
}

/// Build the graph with the module at index `entry` as the only entry point.
pub(super) fn build(modules: &[ResolvedModule], entry: usize) -> ModuleGraph {
    let entry_points = [EntryPoint {
        path: modules[entry].path.clone(),
        source: EntryPointSource::PackageJsonMain,
    }];
    ModuleGraph::build(modules, &entry_points, &discovered_files(modules))
}

pub(super) fn build_without_entry_points(modules: &[ResolvedModule]) -> ModuleGraph {
    let graph = ModuleGraph::build(modules, &[], &discovered_files(modules));
    assert!(graph.modules.iter().all(|module| !module.is_entry_point()));
    graph
}

pub(super) fn symbol<'a>(graph: &'a ModuleGraph, module: usize, name: &str) -> &'a ExportSymbol {
    graph.modules[module]
        .exports
        .iter()
        .find(|export| export.name.matches_str(name))
        .unwrap_or_else(|| panic!("module {module} should export {name}"))
}

pub(super) fn is_used(graph: &ModuleGraph, module: usize, name: &str) -> bool {
    !symbol(graph, module, name).references.is_empty()
}
