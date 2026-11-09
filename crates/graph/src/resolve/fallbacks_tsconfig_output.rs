//! Map TypeScript build output paths back to source with the `rootDir` and
//! `outDir` of the tsconfig files of the package that owns the path, or with
//! a sibling source file of the same stem.

use std::path::Path;

use fallow_config::TsconfigOutputResolution;
use fallow_types::discover::FileId;

use super::{
    PackageManifestInfo, ResolveContext, ResolveResult, SOURCE_EXTS, lookup_internal_file_id,
    nearest_package_manifest, normalize_path_lexically, resolve_package_map_target,
    safe_relative_package_source_path, try_pnpm_workspace_fallback, try_source_fallback,
    try_source_subpath,
};

/// Resolve a relative import of a build output file to its source file:
/// first through the tsconfig output mapping, then through a sibling source
/// file with the same stem.
pub(in crate::resolve) fn try_relative_output_source_fallbacks(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    try_relative_tsconfig_output_fallback(ctx, from_file, specifier)
        .or_else(|| try_output_extension_sibling_fallback(ctx, from_file, specifier))
}

/// Resolve a relative import into a TypeScript output directory to its source.
///
/// A script or a test can import the emitted file (`../lib/types/a.js`) of a
/// package whose tsconfig declares `rootDir` and `outDir`. The output is often
/// not on disk, and `outDir` can have any name. The tsconfig files of the
/// package that owns the target path map the output path to the source file.
fn try_relative_tsconfig_output_fallback(
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

/// The output extensions that a package subpath without an extension can name.
const OUTPUT_SUBPATH_EXTS: &[&str] = &["js", "mjs", "cjs", "jsx"];

/// Resolve a package subpath (empty for the package root) through the source
/// layout of the package, then through the output-to-source mapping.
pub(in crate::resolve) fn try_package_source_or_output(
    ctx: &ResolveContext<'_>,
    manifest: &PackageManifestInfo,
    subpath: &str,
) -> Option<FileId> {
    try_source_subpath(ctx, manifest, Path::new(subpath))
        .or_else(|| try_output_subpath_source(ctx, manifest, subpath))
}

/// Map a package subpath that names build output to its source file.
///
/// The package root (an empty `subpath`) maps through `main`. A deep import
/// such as `pkg/dist/util` names the output file `dist/util.js` or
/// `dist/util/index.js`. Node resolves these paths at run time, so they get
/// the same output-to-source mapping as a package map target.
fn try_output_subpath_source(
    ctx: &ResolveContext<'_>,
    manifest: &PackageManifestInfo,
    subpath: &str,
) -> Option<FileId> {
    let resolve = |target: &str| resolve_package_map_target(ctx, manifest, target, None);
    if subpath.is_empty() {
        let main = manifest.package_json.main.as_deref()?;
        safe_relative_package_source_path(main)?;
        return resolve(&format!("./{}", main.trim_start_matches("./")));
    }
    safe_relative_package_source_path(subpath)?;
    let has_output_extension = Path::new(subpath)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| OUTPUT_SUBPATH_EXTS.contains(&ext));
    if has_output_extension {
        return resolve(&format!("./{subpath}"));
    }
    resolve(&format!("./{subpath}.js")).or_else(|| resolve(&format!("./{subpath}/index.js")))
}

/// The source extensions of the files that a bundler compiles. TypeScript
/// itself maps `.cjs` only to `.cts` and `.mjs` only to `.mts`, and the
/// resolver already applies that mapping.
const COMPILED_SOURCE_EXTS: &[&str] = &["ts", "tsx", "mts", "cts"];

/// The output extensions that a bundler can give a compiled source file.
const OUTPUT_EXTS: &[&str] = &["cjs", "mjs"];

/// Resolve `./name.cjs` or `./name.mjs` to a sibling TypeScript source file.
///
/// A bundler can compile `src/loader.ts` and `src/app.ts` into one output
/// folder as `loader.cjs` and `app.cjs`. The loader then loads `./app.cjs`,
/// which exists only in the output folder. This fallback applies only when
/// the importer is a compiled source file, the target is not on disk, and a
/// source file with the same stem is in the same folder as the target.
///
/// The fallback does not read the bundler or the tsconfig module settings. A
/// project that builds with plain `tsc` emits `x.js` for `x.ts`, so there
/// `./x.mjs` fails at run time. TypeScript reports that import as TS2307.
fn try_output_extension_sibling_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if !specifier.starts_with("./") && !specifier.starts_with("../") {
        return None;
    }
    if !has_extension(from_file, COMPILED_SOURCE_EXTS) {
        return None;
    }
    let target = normalize_path_lexically(&from_file.parent()?.join(specifier));
    if !has_extension(&target, OUTPUT_EXTS) {
        return None;
    }
    COMPILED_SOURCE_EXTS
        .iter()
        .find_map(|ext| lookup_internal_file_id(ctx, &target.with_extension(ext)))
        .map(ResolveResult::InternalModule)
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| extensions.contains(&ext))
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
