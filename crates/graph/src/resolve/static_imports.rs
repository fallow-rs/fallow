//! Resolution of static ES module imports (`import x from './y'`).
//!
//! Handles standard ES module `import` declarations by delegating each specifier
//! to the specifier resolver. Each [`ImportInfo`] is paired with its resolution
//! result to produce a [`ResolvedImport`].
//!
//! This is the simplest resolution submodule. Called from
//! [`super::resolve_all_imports`] as the first resolution step for each module.

use std::path::Path;

use fallow_types::extract::{ImportInfo, ImportedName};

use super::ResolvedImport;
use super::fallbacks::nearest_package_manifest;
use super::specifier::{is_style_file, resolve_import_specifier};
use super::types::{ResolveContext, ResolveResult};

/// The specifier of the side-effect import that CSS extraction adds to a
/// stylesheet with `@apply` or `@tailwind`.
const TAILWIND_PACKAGE: &str = "tailwindcss";

/// Resolve standard ES module imports (`import x from './y'`).
pub(super) fn resolve_static_imports(
    ctx: &ResolveContext,
    file_path: &Path,
    imports: &[ImportInfo],
) -> Vec<ResolvedImport> {
    imports
        .iter()
        .filter_map(|imp| {
            let target = resolve_import_specifier(ctx, file_path, &imp.source, imp.from_style);
            if is_tailwind_self_import(ctx, file_path, imp, &target) {
                return None;
            }
            Some(ResolvedImport {
                info: imp.clone(),
                target,
            })
        })
        .collect()
}

/// The synthetic `tailwindcss` import of a stylesheet inside the `tailwindcss`
/// package resolves to a file of that same package. The stylesheet does not
/// load that file, so the edge would only invent an import cycle.
fn is_tailwind_self_import(
    ctx: &ResolveContext,
    file_path: &Path,
    imp: &ImportInfo,
    target: &ResolveResult,
) -> bool {
    imp.source == TAILWIND_PACKAGE
        && imp.span.is_empty()
        && matches!(imp.imported_name, ImportedName::SideEffect)
        && target.internal_file_id().is_some()
        && is_style_file(file_path)
        && nearest_package_manifest(ctx.package_manifests, file_path)
            .is_some_and(|manifest| manifest.name.as_deref() == Some(TAILWIND_PACKAGE))
}
