//! Resolved paths inside and outside the analyzed project root.

use std::path::Path;

use super::path_info::{extract_package_name, is_bare_specifier};
use super::types::{ResolveContext, ResolveResult};
use super::workspace_alias::{aliased_specifier, try_aliased_workspace_package_fallback};

/// Whether `path` lives under the analyzed project root.
///
/// Containment is tested against both the configured root and its canonicalized
/// form: the resolver returns realpaths (`ResolveOptions::symlinks` defaults to
/// true) while the configured root may itself be reached through a symlink, as
/// it is under `/tmp` on macOS.
///
/// This separates the two cases that reach the bare-specifier classification
/// tail with no `node_modules` segment in the resolved path. A path-alias target
/// inside the root is a project file the alias points at, which stays a concrete
/// `ExternalFile` when `ignorePatterns` kept it out of the file index. A target
/// outside the root is a package reached through its install symlink. A project
/// package resolves into this project (see
/// [`try_project_package_outside_root`]). Any other package keeps npm-package
/// accounting so the declared dependency is credited (see
/// `package_usage_name_for_external_bare_specifier`).
pub(super) fn is_inside_project_root(ctx: &ResolveContext<'_>, path: &Path) -> bool {
    if path.starts_with(ctx.root) {
        return true;
    }
    ctx.canonicalize_cache
        .get(ctx.root)
        .is_some_and(|canonical_root| path.starts_with(canonical_root))
}

/// A bare import of a project package whose install link points outside
/// the project root.
///
/// The base worktree of `fallow audit` links `node_modules` to the
/// `node_modules` of the checkout under review, so its workspace links
/// reach the packages of that checkout. The import still names the package
/// of this project, so it resolves into this project. Without this, the
/// base reports the import as an npm package, and every finding that the
/// edge carries in the head looks introduced.
///
/// A dependency alias such as `"lib-alias": "workspace:@acme/lib@*"` names
/// the project package `@acme/lib`, so its import resolves the same way.
pub(super) fn try_project_package_outside_root(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    style_context: bool,
    path: &Path,
) -> Option<ResolveResult> {
    if !is_bare_specifier(specifier) || is_inside_project_root(ctx, path) {
        return None;
    }
    let aliased = aliased_specifier(ctx, from_file, specifier);
    let package_name = extract_package_name(aliased.as_deref().unwrap_or(specifier));
    if !ctx.workspace_roots.contains_key(package_name.as_str()) {
        return None;
    }
    try_aliased_workspace_package_fallback(ctx, from_file, specifier, style_context)
}
