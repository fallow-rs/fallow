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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::discover::FileId;

    use super::super::tests::with_package_map_ctx;
    use super::super::{resolve_package_map_target, resolve_package_map_targets};

    #[test]
    #[cfg_attr(miri, ignore)]
    fn package_map_targets_use_first_reachable_target() {
        let root = PathBuf::from("/project");
        let src_path = root.join("src/feature.ts");
        let targets = vec![
            "./dist/missing.js".to_string(),
            "./src/feature.ts".to_string(),
        ];

        with_package_map_ctx(
            root,
            Some("pkg"),
            fallow_config::PackageJson::default(),
            &[(src_path, FileId(9))],
            |ctx, manifest, _| {
                assert_eq!(
                    resolve_package_map_targets(ctx, manifest, &targets, None),
                    Some(FileId(9))
                );
            },
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn package_map_lib_mapping_ignores_lib_above_package_root() {
        let root = PathBuf::from("/project/packages/lib");
        let outer_src = PathBuf::from("/project/packages/src/index.ts");

        with_package_map_ctx(
            root,
            Some("pkg"),
            fallow_config::PackageJson::default(),
            &[(outer_src, FileId(5))],
            |ctx, manifest, _| {
                assert_eq!(
                    resolve_package_map_target(ctx, manifest, "./index.js", None),
                    None
                );
            },
        );
    }
}
