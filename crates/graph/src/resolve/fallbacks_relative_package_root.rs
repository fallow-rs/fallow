//! Relative imports that name a package root, such as `require('..')`.

use std::path::Path;
#[cfg(miri)]
use std::path::PathBuf;

use super::{ResolveContext, ResolveResult, normalize_path_lexically, try_source_subpath};

/// Whether `specifier` is relative: `.`, `..`, `./*` or `../*`.
fn is_relative_specifier(specifier: &str) -> bool {
    matches!(specifier, "." | "..") || specifier.starts_with("./") || specifier.starts_with("../")
}

/// Resolve a relative import that lands on a known package root whose built
/// entry points are absent but whose package metadata points at source files.
/// `require('..')` names the package root the same as `require('../')`.
pub(in crate::resolve) fn try_relative_package_root_source_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if !is_relative_specifier(specifier) {
        return None;
    }

    let from_dir = from_file.parent()?;
    let candidate = from_dir.join(specifier);
    let normalized_candidate = normalize_path_lexically(&candidate);
    #[cfg(not(miri))]
    let canonical_candidate = ctx.canonicalize_cache.get(&candidate);
    #[cfg(miri)]
    let canonical_candidate: Option<PathBuf> = None;

    ctx.package_manifests.iter().find_map(|manifest| {
        let matches_manifest = candidate == manifest.root
            || normalized_candidate == manifest.root
            || canonical_candidate
                .as_deref()
                .is_some_and(|canonical| canonical == manifest.canonical_root);
        matches_manifest
            .then(|| try_source_subpath(ctx, manifest, Path::new("")))
            .flatten()
            .map(ResolveResult::InternalModule)
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::discover::FileId;

    use super::super::tests::with_package_map_ctx;
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn relative_package_root_source_fallback_uses_package_source_entry() {
        let root = PathBuf::from("/project");
        let source_path = root.join("custom/entry.js");
        with_package_map_ctx(
            root,
            Some("pkg"),
            fallow_config::PackageJson {
                source: Some("custom/entry.js".to_string()),
                ..Default::default()
            },
            &[(source_path, FileId(11))],
            |ctx, _, root| {
                let result = try_relative_package_root_source_fallback(
                    ctx,
                    &root.join("test/shared/exports.test.js"),
                    "../../",
                );
                assert!(matches!(
                    result,
                    Some(ResolveResult::InternalModule(FileId(11)))
                ));
            },
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn relative_package_root_source_fallback_accepts_bare_parent_and_current_dir() {
        let root = PathBuf::from("/project");
        let source_path = root.join("src/index.ts");
        with_package_map_ctx(
            root,
            Some("pkg"),
            fallow_config::PackageJson {
                main: Some("./dist/index.js".to_string()),
                ..Default::default()
            },
            &[(source_path, FileId(12))],
            |ctx, _, root| {
                let parent = try_relative_package_root_source_fallback(
                    ctx,
                    &root.join("bench/index.cjs"),
                    "..",
                );
                assert!(matches!(
                    parent,
                    Some(ResolveResult::InternalModule(FileId(12)))
                ));
                let current =
                    try_relative_package_root_source_fallback(ctx, &root.join("index.cjs"), ".");
                assert!(matches!(
                    current,
                    Some(ResolveResult::InternalModule(FileId(12)))
                ));
            },
        );
    }
}
