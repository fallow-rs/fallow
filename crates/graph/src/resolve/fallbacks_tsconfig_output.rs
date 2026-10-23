//! Map TypeScript build output paths back to source with the `rootDir` and
//! `outDir` of the tsconfig files of the package that owns the path.

use std::path::Path;

use fallow_config::TsconfigOutputResolution;
use fallow_types::discover::FileId;

use super::{
    ResolveContext, ResolveResult, SOURCE_EXTS, lookup_internal_file_id, nearest_package_manifest,
    normalize_path_lexically, try_pnpm_workspace_fallback, try_source_fallback,
};

/// Resolve a relative import into a TypeScript output directory to its source.
///
/// A script or a test can import the emitted file (`../lib/types/a.js`) of a
/// package whose tsconfig declares `rootDir` and `outDir`. The output is often
/// not on disk, and `outDir` can have any name. The tsconfig files of the
/// package that owns the target path map the output path to the source file.
pub(in crate::resolve) fn try_relative_tsconfig_output_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if !specifier.starts_with("./") && !specifier.starts_with("../") {
        return None;
    }
    let target = normalize_path_lexically(&from_file.parent()?.join(specifier));
    try_tsconfig_output_source(ctx, &target).map(ResolveResult::InternalModule)
}

/// Map a canonical resolved path outside the file set to a source file: the
/// generic output-directory mapping first, then the pnpm workspace store, then
/// the tsconfig output mapping of the owning workspace package.
pub(in crate::resolve) fn try_canonical_source_fallbacks(
    ctx: &ResolveContext<'_>,
    canonical: &Path,
) -> Option<FileId> {
    try_source_fallback(canonical, ctx.path_to_id)
        .or_else(|| try_pnpm_workspace_fallback(canonical, ctx.path_to_id, ctx.workspace_roots))
        .or_else(|| try_workspace_output_source(ctx, canonical))
}

/// Map build output that a workspace package resolves to (through a
/// `node_modules` symlink) back to its source file. A path inside
/// `node_modules` belongs to an installed package and is not mapped.
fn try_workspace_output_source(ctx: &ResolveContext<'_>, canonical: &Path) -> Option<FileId> {
    if canonical
        .components()
        .any(|component| component.as_os_str() == "node_modules")
    {
        return None;
    }
    try_tsconfig_output_source(ctx, canonical)
}

/// Map a build output path of a project package to its source file with the
/// `rootDir` and `outDir` of the tsconfig files of that package.
///
/// The output path does not need to exist on disk. With `rootDir: "./"`, `tsc`
/// emits `src/index.ts` as `dist/src/index.js`, which the generic
/// output-directory mapping does not cover.
pub(in crate::resolve) fn try_tsconfig_output_source(
    ctx: &ResolveContext<'_>,
    output_path: &Path,
) -> Option<FileId> {
    let manifest = nearest_package_manifest(ctx.package_manifests, output_path)?;
    let output_map = ctx.tsconfig_cache.output_map(&manifest.root);
    let TsconfigOutputResolution::Resolved(source) =
        output_map.resolve_source_for_output_path(output_path, SOURCE_EXTS)
    else {
        return None;
    };
    lookup_internal_file_id(ctx, &source)
}
