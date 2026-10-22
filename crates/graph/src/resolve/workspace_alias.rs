//! Dependency aliases of workspace packages.
//!
//! pnpm installs `"vite": "workspace:@scope/core@*"` as the workspace package
//! `@scope/core` under the name `vite`. An import of `vite/pack` from that
//! package then reaches `@scope/core/pack`, also when another workspace member
//! has the name `vite`.

use std::path::Path;

use super::fallbacks::{nearest_package_manifest, try_workspace_package_fallback};
use super::path_info::extract_package_name;
use super::types::{ResolveContext, ResolveResult};

/// The specifier with the dependency alias replaced by the name of the
/// aliased project package.
///
/// The nearest `package.json` of `from_file` must declare the package name of
/// `specifier` as a `workspace:<name>@<range>` dependency, and `<name>` must be
/// a project package. Otherwise the result is `None`.
pub(super) fn aliased_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<String> {
    let alias = extract_package_name(specifier);
    let target = nearest_package_manifest(ctx.package_manifests, from_file)?
        .package_json
        .workspace_alias_target(&alias)?;
    let is_project_package = ctx.workspace_roots.contains_key(target)
        || ctx
            .package_manifests
            .iter()
            .any(|manifest| manifest.name.as_deref() == Some(target));
    if target == alias || !is_project_package {
        return None;
    }
    Some(format!("{target}{}", &specifier[alias.len()..]))
}

/// [`try_workspace_package_fallback`] through the dependency alias of
/// `from_file`, when it has one.
///
/// A result keeps the alias as its package name, because the importer declares
/// the alias, not the aliased package.
pub(super) fn try_aliased_workspace_package_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    style_context: bool,
) -> Option<ResolveResult> {
    let aliased = aliased_specifier(ctx, from_file, specifier);
    workspace_fallback_through_alias(ctx, specifier, aliased.as_deref(), style_context)
}

fn workspace_fallback_through_alias(
    ctx: &ResolveContext<'_>,
    specifier: &str,
    aliased: Option<&str>,
    style_context: bool,
) -> Option<ResolveResult> {
    let Some(aliased) = aliased else {
        return try_workspace_package_fallback(ctx, specifier, style_context);
    };
    let alias = extract_package_name(specifier);
    let result = try_workspace_package_fallback(ctx, aliased, style_context)?;
    Some(match result {
        ResolveResult::InternalPackageModule { file_id, .. } => {
            ResolveResult::InternalPackageModule {
                file_id,
                package_name: alias,
            }
        }
        ResolveResult::CommonJsInternalPackageModule { file_id, .. } => {
            ResolveResult::CommonJsInternalPackageModule {
                file_id,
                package_name: alias,
            }
        }
        ResolveResult::NpmPackage(_) => ResolveResult::NpmPackage(alias),
        ResolveResult::CommonJsNpmPackage(_) => ResolveResult::CommonJsNpmPackage(alias),
        other => other,
    })
}

/// Classify a bare package specifier that the resolver and the earlier
/// fallbacks could not resolve.
///
/// A project package (or the project package that a dependency alias names)
/// gives an unresolved import. Any other package is an npm package.
pub(super) fn resolve_failed_bare_package_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    style_context: bool,
    matches_tsconfig_path_alias: bool,
) -> ResolveResult {
    let aliased = aliased_specifier(ctx, from_file, specifier);
    if let Some(result) =
        workspace_fallback_through_alias(ctx, specifier, aliased.as_deref(), style_context)
    {
        return result;
    }
    if matches_tsconfig_path_alias {
        return ResolveResult::Unresolvable(specifier.to_string());
    }
    let pkg_name = extract_package_name(specifier);
    let lookup = aliased.map_or_else(
        || pkg_name.clone(),
        |aliased| extract_package_name(&aliased),
    );
    if ctx.workspace_roots.contains_key(lookup.as_str())
        || ctx
            .package_manifests
            .iter()
            .any(|manifest| manifest.name.as_deref() == Some(lookup.as_str()))
    {
        ResolveResult::Unresolvable(specifier.to_string())
    } else {
        ResolveResult::NpmPackage(pkg_name)
    }
}
