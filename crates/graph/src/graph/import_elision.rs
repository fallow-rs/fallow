//! The imports that TypeScript removes from its output (issue #5262).
//!
//! TypeScript removes a plain `import { X }` when `X` names only a type in
//! the target module. Such an import does not load the target at runtime, so
//! cycle detection must not count it. TypeScript keeps the import when the
//! importer compiles with `verbatimModuleSyntax` or `preserveValueImports`.
//! The caller decides that per importer.

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::{ImportLoadKind, ImportedName, ModuleLoadMechanism};

use super::{EffectiveExportResolution, ExportNamespace, ImportedSymbol, ModuleGraph};

/// The file extensions of the modules that TypeScript compiles.
const TYPESCRIPT_EXTENSIONS: [&str; 4] = ["ts", "tsx", "mts", "cts"];

/// Whether TypeScript compiles `path`. A JavaScript importer keeps each
/// import, and a framework file (`.vue`, `.svelte`) compiles with rules of
/// its own.
pub(super) fn is_typescript_source(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| TYPESCRIPT_EXTENSIONS.contains(&extension))
}

impl ModuleGraph {
    /// Whether TypeScript removes `symbol`, an import of `target`, from the
    /// output of an importer that compiles without `verbatimModuleSyntax`
    /// and `preserveValueImports`.
    ///
    /// The import must be a static ES import of a named or default binding,
    /// and that name must resolve in the type namespace of the target and not
    /// in its value namespace. A namespace or side-effect import, a
    /// re-export, a dynamic `import()` and a `require()` stay in the output.
    /// A name that does not resolve at all also stays: the export can come
    /// from code that the graph cannot read.
    pub(super) fn is_elided_type_import(&self, target: FileId, symbol: &ImportedSymbol) -> bool {
        if symbol.mechanism != ModuleLoadMechanism::EsModule
            || symbol.load_kind != ImportLoadKind::Static
        {
            return false;
        }
        let name = match &symbol.imported_name {
            ImportedName::Named(name) => name.as_str(),
            ImportedName::Default => "default",
            ImportedName::Namespace | ImportedName::SideEffect => return false,
        };
        matches!(
            self.resolve_export(target, name, ExportNamespace::Value),
            EffectiveExportResolution::Missing
        ) && !matches!(
            self.resolve_export(target, name, ExportNamespace::Type),
            EffectiveExportResolution::Missing
        )
    }
}
