//! The imports that TypeScript removes from its output (issues #5262 and
//! #5309).
//!
//! TypeScript removes a plain `import { X }` when the file never reads `X`
//! as a value: `X` names only a type in the target, or the file uses `X`
//! only in type positions, or not at all. Such an import does not load the
//! target at runtime, so cycle detection must not count it. Some compiler
//! options keep the import. The caller reads them per importer and gives an
//! [`ImportElision`].

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::{ImportLoadKind, ImportedName, ModuleLoadMechanism};
use rustc_hash::FxHashSet;

use super::types::ModuleNode;
use super::{EffectiveExportResolution, ExportNamespace, ImportedSymbol, ModuleGraph};

/// The file extensions of the modules that TypeScript compiles.
const TYPESCRIPT_EXTENSIONS: [&str; 4] = ["ts", "tsx", "mts", "cts"];

/// Which plain imports TypeScript removes from the output of one importer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportElision {
    /// The compiler options keep each import: `verbatimModuleSyntax`,
    /// `preserveValueImports`, or `importsNotUsedAsValues` with `preserve`
    /// or `error`.
    Keep,
    /// `emitDecoratorMetadata` is on. In a file with a decorator, a type
    /// annotation can become a runtime reference, so only an import of names
    /// that are types in the target goes away there.
    KeepDecoratedTypes,
    /// TypeScript removes each import that the file never reads as a value.
    RemoveUnread,
}

/// How the importer uses the local binding of an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum BindingUse {
    /// The file reads the binding as a value, or the use is not known.
    Value,
    /// The file uses the binding only in type positions.
    TypeOnly,
    /// The file never uses the binding.
    Unread,
}

/// The binding use lists of one importer, as extraction records them.
pub(super) struct BindingUses<'a> {
    value: &'a [String],
    type_only: &'a [String],
    unread: &'a FxHashSet<String>,
}

impl<'a> BindingUses<'a> {
    /// The lists of `module`. Extraction sorts the value and type lists.
    pub(super) fn of(module: &'a crate::resolve::ResolvedModule) -> Self {
        Self {
            value: &module.value_referenced_import_bindings,
            type_only: &module.type_referenced_import_bindings,
            unread: &module.unused_import_bindings,
        }
    }

    /// No use facts, so each binding counts as a value.
    #[cfg(test)]
    pub(super) fn unknown(unread: &'a FxHashSet<String>) -> Self {
        Self {
            value: &[],
            type_only: &[],
            unread,
        }
    }

    /// The use of `local`. A binding that no list names counts as a value
    /// read, so a file without use facts keeps each import.
    pub(super) fn of_binding(&self, local: &str) -> BindingUse {
        let in_sorted = |list: &[String]| {
            list.binary_search_by(|name| name.as_str().cmp(local))
                .is_ok()
        };
        if local.is_empty() || in_sorted(self.value) {
            BindingUse::Value
        } else if self.unread.contains(local) {
            BindingUse::Unread
        } else if in_sorted(self.type_only) {
            BindingUse::TypeOnly
        } else {
            BindingUse::Value
        }
    }
}

/// Whether TypeScript compiles `path`. A JavaScript importer keeps each
/// import, and a framework file (`.vue`, `.svelte`) compiles with rules of
/// its own.
fn is_typescript_source(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| TYPESCRIPT_EXTENSIONS.contains(&extension))
}

/// The [`ImportElision`] of one importer, read from the caller on first use.
pub(super) enum ImporterElision {
    NotTypeScript,
    Unknown,
    Known(ImportElision),
}

impl ImporterElision {
    pub(super) fn new(path: &Path) -> Self {
        if is_typescript_source(path) {
            Self::Unknown
        } else {
            Self::NotTypeScript
        }
    }

    pub(super) fn get(
        &mut self,
        path: &Path,
        elision_of: &mut dyn FnMut(&Path) -> ImportElision,
    ) -> ImportElision {
        match *self {
            Self::NotTypeScript => ImportElision::Keep,
            Self::Known(elision) => elision,
            Self::Unknown => {
                let elision = elision_of(path);
                *self = Self::Known(elision);
                elision
            }
        }
    }
}

impl ModuleGraph {
    /// Whether TypeScript removes `symbol`, an import of `target` in
    /// `importer`, under `elision`.
    ///
    /// Only a static ES import of a named or default binding can go away. A
    /// namespace or side-effect import, a re-export, a dynamic `import()`
    /// and a `require()` stay in the output. A binding that the file reads as
    /// a value stays. Then:
    ///
    /// - an import of a name that resolves in the type namespace of the
    ///   target and not in its value namespace goes away;
    /// - an import that the file uses only in type positions or not at all
    ///   goes away, except in a `.tsx` file and under
    ///   [`ImportElision::KeepDecoratedTypes`] in a file with a decorator. In
    ///   a `.tsx` file the binding can be the JSX factory of the classic
    ///   runtime, which TypeScript keeps whatever the file does with it.
    pub(super) fn is_removed_import(
        &self,
        target: FileId,
        symbol: &ImportedSymbol,
        importer: &ModuleNode,
        elision: ImportElision,
    ) -> bool {
        if elision == ImportElision::Keep
            || symbol.binding_use == BindingUse::Value
            || symbol.mechanism != ModuleLoadMechanism::EsModule
            || symbol.load_kind != ImportLoadKind::Static
        {
            return false;
        }
        let name = match &symbol.imported_name {
            ImportedName::Named(name) => name.as_str(),
            ImportedName::Default => "default",
            ImportedName::Namespace | ImportedName::SideEffect => return false,
        };
        if self.names_only_a_type(target, name) {
            return true;
        }
        let is_tsx = importer
            .path
            .extension()
            .is_some_and(|extension| extension == "tsx");
        !is_tsx && (elision == ImportElision::RemoveUnread || !importer.has_decorators())
    }

    /// Whether `name` resolves in the type namespace of `target` and not in
    /// its value namespace. A name that does not resolve at all does not: the
    /// export can come from code that the graph cannot read.
    fn names_only_a_type(&self, target: FileId, name: &str) -> bool {
        matches!(
            self.resolve_export(target, name, ExportNamespace::Value),
            EffectiveExportResolution::Missing
        ) && !matches!(
            self.resolve_export(target, name, ExportNamespace::Type),
            EffectiveExportResolution::Missing
        )
    }
}
