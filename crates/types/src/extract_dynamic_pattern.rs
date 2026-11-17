//! Dynamic import patterns of a module.

use oxc_span::Span;

use super::{ImportedName, ModuleLoadMechanism};

/// A dynamic import with a partially resolved pattern.
#[derive(Debug, Clone)]
pub struct DynamicImportPattern {
    /// Glob prefix of the import path (e.g., "./locales/"). An
    /// `import.meta.glob` pattern is the full glob. For a template literal, a
    /// string concatenation, or `require.context`, each glob metacharacter of
    /// the literal text is escaped as a one-character class, such as `[[]`.
    pub prefix: String,
    /// Glob suffix of the import path (e.g., ".json"), if any.
    pub suffix: Option<String>,
    /// Globs of the files that the pattern does not load. Vite reads a
    /// negative `import.meta.glob` array entry, such as `'!./a/skip.js'`, as
    /// an exclude for every positive entry of the same call.
    pub excludes: Vec<String>,
    /// Source span in the original file.
    pub span: Span,
    /// Runtime mechanism used to load modules matching this pattern.
    pub mechanism: ModuleLoadMechanism,
    /// What each match gives the importer, which decides the exports that the
    /// pattern credits. A pattern loads the whole module by default
    /// (`Namespace`). The `import` option of `import.meta.glob` selects one
    /// export (`Default` or `Named`). A raw or url query reads each
    /// match as text or a URL, so the match stays in use but no export gets
    /// credit (`SideEffect`).
    pub imported_name: ImportedName,
}
