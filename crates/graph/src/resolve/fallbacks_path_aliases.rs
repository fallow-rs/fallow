//! Plugin path alias lookups: the alias prefix match, the workspace scope of
//! an alias, and the alias fallback of the resolver.

use std::path::Path;

use super::super::types::{ResolveContext, ResolveResult};
use super::{
    extract_package_name_from_node_modules_path, try_pnpm_workspace_fallback, try_source_fallback,
};

/// Return the post-prefix remainder when `specifier` matches the alias `prefix`
/// at a path boundary, else `None`.
///
/// The match is segment-aware: a bare exact-key alias (e.g. `@scope/sdk` or
/// `vscode`) matches only on an exact hit or a `/`-delimited continuation, so it
/// never captures a longer package that merely shares the prefix
/// (`@scope/sdk-extra`). Prefixes that already end in `/` (`~/`, `@/`, `$lib/`)
/// match any continuation by construction, preserving their existing behavior.
pub(in crate::resolve) fn alias_match_remainder<'a>(
    specifier: &'a str,
    prefix: &str,
) -> Option<&'a str> {
    let remainder = specifier.strip_prefix(prefix)?;
    (remainder.is_empty() || prefix.ends_with('/') || remainder.starts_with('/'))
        .then_some(remainder)
}

/// Return the plugin path aliases that apply to `specifier` in `from_file`, in
/// declaration order.
///
/// A workspace alias applies only to files inside its workspace. The deepest
/// workspace that contains `from_file` and declares a matching prefix owns the
/// specifier, and only its aliases apply. A missing target then stays
/// unresolved, and the specifier does not resolve into another workspace that
/// declares the same prefix. When no workspace owns the specifier, the
/// project-level aliases apply.
pub(super) fn applicable_path_aliases<'c>(
    ctx: &'c ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Vec<(&'c str, &'c str)> {
    let owner = ctx
        .scoped_path_aliases
        .iter()
        .filter(|alias| {
            alias_match_remainder(specifier, &alias.prefix).is_some()
                && from_file.starts_with(ctx.root.join(&alias.scope))
        })
        .map(|alias| alias.scope.as_str())
        .max_by_key(|scope| Path::new(scope).components().count());
    match owner {
        Some(owner) => ctx
            .scoped_path_aliases
            .iter()
            .filter(|alias| alias.scope == owner)
            .map(|alias| (alias.prefix.as_str(), alias.replacement.as_str()))
            .collect(),
        None => ctx
            .path_aliases
            .iter()
            .map(|(prefix, replacement)| (prefix.as_str(), replacement.as_str()))
            .collect(),
    }
}

/// Whether a plugin path alias that applies to `from_file` matches `specifier`.
pub(in crate::resolve) fn matches_plugin_path_alias(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> bool {
    applicable_path_aliases(ctx, from_file, specifier)
        .into_iter()
        .any(|(prefix, _)| alias_match_remainder(specifier, prefix).is_some())
}

/// Try resolving a specifier using plugin-provided path aliases.
///
/// Substitutes a matching alias prefix (e.g., `~/`) with a directory relative to the
/// project root (e.g., `app/`) and resolves the resulting path. This handles framework
/// aliases like Nuxt's `~/`, `~~/`, `#shared/` that aren't defined in tsconfig.json
/// but map to real filesystem paths.
pub(in crate::resolve) fn try_path_alias_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    style_context: bool,
) -> Option<ResolveResult> {
    applicable_path_aliases(ctx, from_file, specifier)
        .into_iter()
        .find_map(|(prefix, replacement)| {
            let remainder = alias_match_remainder(specifier, prefix)?;
            let substituted = match (replacement.is_empty(), remainder.is_empty()) {
                (true, _) => format!("./{remainder}"),
                (false, true) => format!("./{replacement}"),
                (false, false) => format!("./{replacement}/{remainder}"),
            };
            let resolved = super::super::specifier::resolve_with_extension_policy(
                ctx,
                ctx.root,
                &substituted,
                style_context,
            )
            .ok()?;
            classify_alias_target(ctx, resolved.path())
        })
}

/// Map the file that an alias substitution resolved to onto a resolve result.
fn classify_alias_target(ctx: &ResolveContext<'_>, resolved_path: &Path) -> Option<ResolveResult> {
    if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    let canonical = ctx.canonicalize_cache.get(resolved_path)?;
    if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    if let Some(file_id) = try_source_fallback(&canonical, ctx.path_to_id) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    if let Some(file_id) =
        try_pnpm_workspace_fallback(&canonical, ctx.path_to_id, ctx.workspace_roots)
    {
        return Some(ResolveResult::InternalModule(file_id));
    }
    if let Some(pkg_name) = extract_package_name_from_node_modules_path(&canonical) {
        return Some(ResolveResult::NpmPackage(pkg_name));
    }
    Some(ResolveResult::ExternalFile(canonical))
}
