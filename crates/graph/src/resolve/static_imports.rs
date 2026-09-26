//! Resolution of static ES module imports (`import x from './y'`).
//!
//! Handles standard ES module `import` declarations by delegating each specifier
//! to the specifier resolver. Each [`ImportInfo`] is paired with its resolution
//! result to produce a [`ResolvedImport`].
//!
//! This is the simplest resolution submodule — a direct 1:1 mapping from extracted
//! imports to resolved imports. Called from [`super::resolve_all_imports`] as the
//! first resolution step for each module.

use std::path::Path;

use fallow_types::extract::ImportInfo;

use super::ResolvedImport;
use super::specifier::resolve_import_specifier;
use super::types::ResolveContext;

/// Resolve standard ES module imports (`import x from './y'`).
pub(super) fn resolve_static_imports(
    ctx: &ResolveContext,
    file_path: &Path,
    imports: &[ImportInfo],
) -> Vec<ResolvedImport> {
    imports
        .iter()
        .map(|imp| ResolvedImport {
            info: imp.clone(),
            target: resolve_import_specifier(ctx, file_path, &imp.source, imp.from_style),
        })
        .collect()
}
